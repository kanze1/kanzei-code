//! 项目级 SQLite 会话事件存储。
//!
//! 存储层只负责持久化事实，不负责 runner 的执行策略。事件序列按 session
//! 独立递增；输入先进入 inbox，只有 runner 在安全边界提升后才成为可见消息。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// v3:episodes 表(R-106,轮次情景摘要;回滚 = DROP TABLE episodes 并把版本改回 2)。
// v5:session_inputs 补 running/completed/failed 终态 + finished_at;
//     episodes 补 provider/model/run_id/input_id/duration_ms(D-173 可观测性)。
// v6:回填 v5 之前遗留的 promoted 输入(D-180)。
// v7:v6 回填晚了一步——存量 promoted 已被 v5 期间的停止抹成 cancelled,
//     改从迁移前备份里把状态位捞回来(D-180 续)。
// v8:R-161 记忆漏斗遥测,三张表与 episodes 同库可 join。
// v9:R-166 反事实评估——memory_eval_agg 聚合表(F(m) 的 effect_mean/effect_ci/
//     eval_n/last_eval),每条记忆一行,离线回放周期更新。
// v10:R-178 线级状态持久化——processes 表存线/进程注册 + 模型/profile/reasoning/
//     子代理开关,重启后逐项目恢复(页签不丢,承接 R-030 遗留)。
const SCHEMA_VERSION: i64 = 10;
/// v6 回填的保护窗:promoted_at 晚于"迁移时刻减去这个窗口"的输入不回填,
/// 因为它可能正被另一个进程执行(桌面端与 CLI 共用同一个库)。
const LEGACY_PROMOTED_GRACE_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// 库比本二进制新。文案必须给出出路:光说"不兼容"会让人以为库坏了而去删库,
    /// 而删库丢的是全部会话历史——正确动作是把这个二进制升到同一版本。
    #[error(
        "数据库 schema 版本 {found} 高于本程序支持的 {supported}:这个 .kanzei/state.db \
         是更新版本的 kanzei 创建的。请把当前这个程序升到同一版本(桌面端用设置页「检查更新」;\
         CLI 用 cargo install --path crates/kanzei --force),不要删库——降级会丢掉全部会话历史。\
         升级前的库已自动备份为 state.db.v<n>.bak。"
    )]
    UnsupportedSchema { found: i64, supported: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub session_id: String,
    pub project_root: String,
    pub title: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredEvent {
    pub event_id: String,
    pub session_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub payload: Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    Steer,
    Queue,
}

impl Delivery {
    /// 仅限 store::* 子模块使用(S5 拆解后提 pub(super))。
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::Queue => "queue",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdmittedInput {
    pub input_id: String,
    pub session_id: String,
    pub prompt: String,
    pub delivery: Delivery,
    pub created_at: i64,
}

/// 一轮的情景摘要。参数从 9 个涨到 14 个之后位置参数已经不可读了(错位一个
/// 就把 provider 写进 model 也编译得过),所以收成具名结构。
#[derive(Debug, Clone, Default)]
pub struct EpisodeRecord<'a> {
    pub session_id: &'a str,
    pub prompt_head: &'a str,
    pub outcome: &'a str,
    pub steps: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tools_json: &'a str,
    pub context_json: &'a str,
    pub metrics_json: &'a str,
    /// 这一轮实际使用的 provider/model —— 事后复盘不能靠"当前配置"反推。
    pub provider: &'a str,
    pub model: &'a str,
    pub run_id: &'a str,
    pub input_id: &'a str,
    pub duration_ms: u64,
    /// 上下文溢出压缩时被丢弃轨迹的摘要 JSON 数组字符串(R-106),空数组 `[]` 代表无。
    pub overflow_json: &'a str,
}

mod episodes;
mod eval;
mod events;
mod inbox;
mod notifications;
mod processes;
mod schema;
mod session;
mod telemetry;

pub use eval::{EffectEstimate, EvalCaseSet};
pub use processes::StoredProcess;
pub use telemetry::{FunnelCounts, RecallEvent};

pub use session::{project_session_id, project_state_path};

pub struct SessionStore {
    /// 仅限 store::* 子模块使用(S1 拆壳后本字段 pub(crate))。
    pub(crate) connection: Connection,
    /// 落盘路径;内存库为 None。迁移前的备份要用它。
    /// 仅限 store::* 子模块使用(S1 拆壳后本字段 pub(crate))。
    pub(crate) path: Option<PathBuf>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间必须晚于 Unix epoch")
        .as_millis() as i64
}

#[cfg(test)]
pub(crate) mod testutil;
