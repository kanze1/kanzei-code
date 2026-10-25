//! 验证、提交和推送的节奏偏好，以及按显式键合并的配置合同。

use serde::{Deserialize, Serialize};

/// 开发档提供给模型的节奏偏好，不构成操作授权或后台调度。
/// 缺省值由类型定义；项目层仅覆盖显式写入的键。项目验证门禁独立执行。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Cadence {
    /// 全量测试安排，默认条目关闭前。
    #[serde(default)]
    pub full_test: FullTestCadence,
    /// EveryNBatches 的正整数间隔；未配置时不自行补成每批执行。
    #[serde(default)]
    pub full_test_batches: Option<u32>,
    /// 定向测试：每次提交前（默认）或按任务自主决定。
    #[serde(default)]
    pub targeted_test: TargetedTestCadence,
    /// 已授权提交的组织粒度，默认按实际工作批次。
    #[serde(default)]
    pub commit: CommitCadence,
    /// 已授权推送的安排，默认条目完成后；由 Agent 通过 Git 工具执行。
    #[serde(default)]
    pub push: PushCadence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FullTestCadence {
    /// 条目关闭前。
    #[default]
    EntryClose,
    /// 每次提交前全量。
    EveryCommit,
    /// 每 n 批全量一次,间隔见 Cadence::full_test_batches。
    EveryNBatches,
    /// 发布前；发布入口及门禁由项目定义。
    ReleaseOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetedTestCadence {
    /// 每次提交前。
    #[default]
    EveryCommit,
    /// 不设置固定触发频率，由 Agent 根据改动与验收选择。
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitCadence {
    /// 按 Agent 实际划分的工作批次组织提交。
    #[default]
    PerBatch,
    /// 按条目组织提交。
    PerEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PushCadence {
    /// 条目完成后推送。
    #[default]
    PerEntry,
    /// 每次提交后推送。
    PerCommit,
    /// 根据项目约定安排推送；不启动定时器或轮末自动推送。
    Periodic,
}

/// D-245:cadence 逐键 overlay。字段非 Option,「没写」与「显式写成默认值」在
/// merge 层不可区分(serde default 把两者都落成默认),所以由调用方把 raw toml
/// `[cadence]` 表里**显式出现的键**传进来,只覆盖这些——与 [limits] 的
/// 「项目层只覆盖显式写的键」同一套层叠语义,避免项目层只调一个 full_test
/// 就把其余字段全部打回默认。
pub(crate) fn overlay_cadence(
    base: &mut Cadence,
    layer: &Cadence,
    written: &std::collections::HashSet<&str>,
) {
    if written.contains("full_test") {
        base.full_test = layer.full_test;
    }
    if written.contains("full_test_batches") {
        base.full_test_batches = layer.full_test_batches;
    }
    if written.contains("targeted_test") {
        base.targeted_test = layer.targeted_test;
    }
    if written.contains("commit") {
        base.commit = layer.commit;
    }
    if written.contains("push") {
        base.push = layer.push;
    }
}

/// kanzei.toml [cadence] 节已知键名单(R-220 单源)。
pub(crate) const CADENCE_KEYS: &[&str] = &[
    "full_test",
    "full_test_batches",
    "targeted_test",
    "commit",
    "push",
];
