//! 项目级 SQLite 会话事件存储。
//!
//! 存储层只负责持久化事实，不负责 runner 的执行策略。事件序列按 session
//! 独立递增；输入先进入 inbox，只有 runner 在安全边界提升后才成为可见消息。

use std::path::{Path, PathBuf};
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
// v11:R-177 F11——processes 补 tracker_writes_enabled 列(存量线默认关闭)。
// v12:R-178 批3——processes 补 manual_models 列(项目级手填模型候选,JSON 数组,
//     由默认进程行承载;localStorage kz-manual-models 旧键上迁的目标)。
// v13:R-226——retired_processes 保存已注销线路身份,确保 p{n}/session_id 永不复用。
// v14:D-373——补齐 D-297 的 session_events_session_type_sequence 下推索引(它被加进
//     v13 的建表批却没提版本号,于是 migrate 的 `version == SCHEMA_VERSION` 早退让
//     **所有存量库**永远拿不到它:实测主库仍走 (session_id, sequence) 索引全扫 72,751
//     行再过滤 event_type);同时丢弃与 UNIQUE 自动索引完全重复的
//     session_events_session_sequence。
//
// v15:D-375——存量 legacy_seeded 丢弃整包 messages 副本改回引用(源快照仍在才丢),
//     随后 VACUUM 一次回收(实测占比约 22%,够不着 housekeeping 的 50% 阈值)。
//
// v17:D-433——R-280 往 processes 建表批加 subagents_enabled 却没提版本号,早退让存量库
//     永远拿不到这列,桌面端每次读进程注册都 `no such column` 崩在列表刷新上(实测
//     2026-08-17 build-ac637546 装机即坏)。D-373 的判据只冻结**对象名集合**,加列
//     不改对象名所以全绿——这一版补上列级判据。
// v18:Work Unit 底座——work_events 保存 append-only 执行事实，work_surfaces 保存可从
//     事件重建的当前投影；Requirement 回归长期 Outcome，不再兼任执行历史容器。
//
// **改建表批 = 同时 +1 本常量并更新 SCHEMA_OBJECTS/SCHEMA_COLUMNS**(schema.rs 的机械
// 判据会拦):早退分支让「代码里有、存量库里没有」不产生任何编译或测试信号,只能靠判据站岗。
const SCHEMA_VERSION: i64 = 18;
/// v6 回填的保护窗:promoted_at 晚于"迁移时刻减去这个窗口"的输入不回填,
/// 因为它可能正被另一个进程执行(桌面端与 CLI 共用同一个库)。
const LEGACY_PROMOTED_GRACE_MS: i64 = 5 * 60 * 1000;
/// D-298:housekeeping 节流窗口。open 高频(每个命令一次),VACUUM 与备份扫描
/// 是重操作,默认 24 小时才评估一次;窗口内直接跳过。
const HOUSEKEEPING_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;
/// D-298:freelist 死页占比超过该阈值才 VACUUM。实测主会话库 82MB 中约 68MB
/// (83%)是 freelist,50% 是合理的触发线——低于它说明库还健康,不必付整理成本。
const HOUSEKEEPING_FREELIST_THRESHOLD: f64 = 0.5;
/// R-245 B2:显式整理入口读取的存储占用快照；只读，不触发清理或过期策略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageReport {
    pub state_db_bytes: u64,
    pub wal_bytes: u64,
    pub shm_bytes: u64,
    pub page_count: i64,
    pub freelist_pages: i64,
    pub artifact_files: u64,
    pub artifact_bytes: u64,
    pub unreferenced_artifact_files: u64,
    pub unreferenced_artifact_bytes: u64,
    pub shadow_files: u64,
    pub shadow_bytes: u64,
    pub migration_backup_files: u64,
    pub migration_backup_bytes: u64,
}
/// 单个迁移备份的可审计占用记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageBackupReport {
    pub version: i64,
    pub relative_path: String,
    pub bytes: u64,
}

/// R-245 B5 显式安全整理计划；dry-run 只读取并列出预计释放量。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageCleanupPlan {
    pub dry_run: bool,
    pub eligible: bool,
    pub blocked_reason: Option<String>,
    pub report: StorageReport,
    pub unreferenced: Vec<ArtifactFileReport>,
    pub migration_backups: Vec<StorageBackupReport>,
    pub deletable_backup_versions: Vec<i64>,
    pub estimated_reclaim_bytes: u64,
}

/// 显式安全整理的执行结果；失败项保留在结果中供下一次重试。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageCleanupResult {
    pub before: StorageReport,
    pub after: StorageReport,
    pub checkpointed: bool,
    pub vacuumed: bool,
    pub deleted_artifacts: Vec<String>,
    pub deleted_backups: Vec<String>,
    pub artifact_cleanup_errors: Vec<String>,
    pub backup_cleanup_errors: Vec<String>,
    pub actual_freed_bytes: u64,
}

/// 单个 durable artifact 的引用图节点；路径来自项目 artifact 根目录内的实际文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactFileReport {
    pub artifact_id: String,
    pub relative_path: String,
    pub bytes: u64,
    pub reference_count: u64,
}

/// R-245 B3 的只读整理计划。它描述可回收候选，但绝不执行删除。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactCleanupPlan {
    pub dry_run: bool,
    pub total_artifact_files: u64,
    pub total_artifact_bytes: u64,
    pub referenced_artifact_files: u64,
    pub referenced_artifact_bytes: u64,
    pub unreferenced_artifact_files: u64,
    pub unreferenced_artifact_bytes: u64,
    pub unreferenced: Vec<ArtifactFileReport>,
}

/// B4 会话删除前的可审计计划；只描述目标，不执行任何删除。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDeletionPlan {
    pub dry_run: bool,
    pub session_id: String,
    pub eligible: bool,
    pub blocked_reason: Option<String>,
    pub event_count: u64,
    pub input_count: u64,
    pub episode_count: u64,
    pub recall_event_count: u64,
    pub memory_source_count: u64,
    pub target_artifacts: Vec<SessionArtifactReport>,
    pub deletable_artifacts: Vec<SessionArtifactReport>,
    pub missing_artifacts: Vec<String>,
}

/// 会话引用的 artifact 在全库删除后的处置判断。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionArtifactReport {
    pub artifact_id: String,
    pub relative_path: String,
    pub bytes: u64,
    pub session_reference_count: u64,
    pub other_reference_count: u64,
}

/// 会话删除提交后的结果；artifact 失败项保留，下一次显式整理可重试。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDeletionResult {
    pub session_id: String,
    pub deleted_events: u64,
    pub deleted_inputs: u64,
    pub deleted_episodes: u64,
    pub deleted_recall_events: u64,
    pub deleted_memory_sources: u64,
    pub deleted_artifacts: Vec<String>,
    pub artifact_cleanup_errors: Vec<String>,
}

/// D-374:`SessionStore::open` 的累计次数,**按库路径分桶**。
///
/// open **不便宜**(见 `session.rs::open`:create_dir_all + Connection::open + 三个
/// pragma + migrate 版本查询 + housekeeping 节流查询,132MB 库上实测约 4.3ms),
/// 而它曾经就坐在逐事件的轨迹落库路径上。这个计数器把「这条路径每个事件开一次连接」
/// 从"读代码才知道"变成可断言的事实——回归时是一条硬判据,不是注释里的自觉。
///
/// 为什么按路径而不是一个全局计数:测试是多线程并行跑的,全局计数的"前后差值"会把
/// 别的测试开的连接算进来(实测:单跑绿、并跑红)。按路径分桶后每个测试用自己的
/// 临时库路径,判据与并发无关。锁的代价相对 open 自身是噪声。
static OPEN_COUNTS: std::sync::Mutex<Option<std::collections::HashMap<PathBuf, u64>>> =
    std::sync::Mutex::new(None);

pub(crate) fn note_store_open(path: &Path) {
    let mut counts = OPEN_COUNTS.lock().unwrap_or_else(|e| e.into_inner());
    *counts
        .get_or_insert_with(std::collections::HashMap::new)
        .entry(path.to_path_buf())
        .or_insert(0) += 1;
}

/// 见 [`OPEN_COUNTS`]:某个 state.db 被打开过多少次。
pub fn store_open_count(path: &Path) -> u64 {
    let counts = OPEN_COUNTS.lock().unwrap_or_else(|e| e.into_inner());
    counts
        .as_ref()
        .and_then(|map| map.get(path))
        .copied()
        .unwrap_or(0)
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// 库比本二进制新。文案必须给出出路:光说"不兼容"会让人以为库坏了而去删库,
    /// 而删库丢的是全部会话历史——正确动作是把这个二进制升到同一版本。
    #[error("invalid store input: {0}")]
    InvalidInput(String),
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
mod mobile_devices;
mod notifications;
mod processes;
mod schema;
mod session;
mod task;
mod telemetry;
mod typed;
mod work;

pub use eval::{EffectEstimate, EvalCaseSet};
pub use processes::StoredProcess;
pub use task::{
    TaskOutcome, TASK_CLOSED_EVENT_TYPE, TASK_EVENT_SCHEMA_VERSION,
    TASK_MEMBERSHIP_ADDED_EVENT_TYPE, TASK_STARTED_EVENT_TYPE,
};
pub use telemetry::{FunnelCounts, RecallEvent, RecallLinkStats, RecallMetrics};

pub use typed::{
    compare_shadow, decode_session_fact, prepare_typed_session, project_session_facts,
    project_session_facts_with_surface, stable_json_hash, stable_message_hash,
    summarize_shadow_reports, InterruptedAssistant, SessionFact, SessionFactEnvelope,
    SessionFactError, SessionInvariant, SessionProjection, SessionTurnTerminal, ShadowComparison,
    ShadowVerdictStats, TypedSessionWriter, ASSISTANT_DRAFT_APPENDED, ASSISTANT_MESSAGE_COMMITTED,
    ASSISTANT_MESSAGE_INTERRUPTED, LEGACY_SEEDED, SESSION_EVENT_FORMAT_VERSION,
    SUBAGENT_TRANSCRIPT, TOOL_CALLED, TOOL_RESULT_COMMITTED, TOOL_RESULT_INTERRUPTED,
    TURN_COMPLETED, TURN_FAILED, TURN_STARTED, TURN_STOPPED, USER_MESSAGE_COMMITTED,
};

pub use session::{project_session_id, project_state_path};
pub use work::{
    project_work_events, StoredWorkEvent, WorkCheckpoint, WorkEvidence, WorkFact, WorkProjection,
    WorkUnitSpec, WorkUnitStatus, MAX_CHECKPOINT_SUMMARY_CHARS, MAX_WORK_ITEM_CHARS,
    MAX_WORK_LIST_ITEMS, MAX_WORK_OBJECTIVE_CHARS, WORK_PROJECTION_FORMAT_VERSION,
};

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
