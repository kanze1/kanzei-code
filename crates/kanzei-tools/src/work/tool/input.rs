//! Typed work tool inputs shared by lifecycle and revision actions.
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct WorkInput {
    /// 队列决策、Work Unit 生命周期、计划修订、验证作业及交接；合法动作见 enum。
    pub(super) action: String,
    /// next=true 时显式读取完整状态；默认返回有界决策摘要。
    #[serde(default)]
    pub(super) detail: bool,
    /// claim accepts a requirement/defect ID or a Work Unit ID. Unit lifecycle actions
    /// require the exact unit_id returned by create_unit/list_units, e.g. R-001/W1.
    #[serde(default)]
    pub(super) id: Option<String>,
    #[serde(default)]
    pub(super) requirement_id: Option<String>,
    #[serde(default)]
    pub(super) objective: Option<String>,
    #[serde(default)]
    pub(super) scope: Vec<String>,
    #[serde(default)]
    pub(super) dependencies: Vec<String>,
    #[serde(default)]
    pub(super) acceptance: Vec<String>,
    #[serde(default)]
    pub(super) verification: Vec<String>,
    #[serde(default)]
    pub(super) base_revision: Option<String>,
    /// Node purpose, knowledge gaps and sources; independent of execution status.
    #[serde(default)]
    pub(super) node: kanzei_core::WorkNodeContract,
    /// Required for decompose/refine; use source_sequence from get_unit.
    #[serde(default)]
    pub(super) expected_sequence: Option<i64>,
    #[serde(default)]
    pub(super) children: Vec<WorkChildInput>,
    /// Complete replacement contract. Identity and parent are immutable.
    #[serde(default)]
    pub(super) refinement: Option<kanzei_core::WorkUnitSpec>,
    /// Complete versions map returned by impact; refresh after yielding executors.
    #[serde(default)]
    pub(super) expected_versions: Option<kanzei_core::WorkGraphVersion>,
    #[serde(default)]
    pub(super) direction: Option<String>,
    #[serde(default)]
    pub(super) revision_id: Option<String>,
    #[serde(default)]
    pub(super) revision_decisions: Vec<kanzei_core::WorkRevisionDecision>,
    /// Full contracts with fresh IDs, validated with all dispositions in one transaction.
    #[serde(default)]
    pub(super) additions: Vec<kanzei_core::WorkUnitSpec>,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) next_action: Option<String>,
    #[serde(default)]
    pub(super) decisions: Vec<String>,
    #[serde(default)]
    pub(super) retrieval_refs: Vec<String>,
    #[serde(default)]
    pub(super) observed_head: Option<String>,
    #[serde(default)]
    pub(super) observed_worktree_hash: Option<String>,
    /// handoff 必填：本次声明范围内已满足的完成条件。
    #[serde(default)]
    pub(super) criterion: Option<String>,
    /// 可复核的文件、测试、提交或交付记录引用；handoff 不能为空。
    #[serde(default)]
    pub(super) evidence_refs: Vec<String>,
    /// handoff 必填:work_item/batch 只记局部交付;request/goal 需匹配当前委托。内部记录,勿向用户复述。
    #[serde(default)]
    pub(super) handoff_scope: Option<kanzei_harness::handoff::HandoffScope>,
    /// handoff 必填:事项 id、批次名、当前 admitted input id 或当前目标原文。内部记录,勿向用户复述。
    #[serde(default)]
    pub(super) handoff_target: Option<String>,
    /// 偏离默认选择、接管、阻塞/解阻塞、废弃时写入审计。
    #[serde(default)]
    pub(super) reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkChildInput {
    /// Unique within this decomposition; dependencies may use @key for siblings.
    pub(super) key: String,
    pub(super) objective: String,
    #[serde(default)]
    pub(super) scope: Vec<String>,
    #[serde(default)]
    pub(super) dependencies: Vec<String>,
    #[serde(default)]
    pub(super) acceptance: Vec<String>,
    #[serde(default)]
    pub(super) verification: Vec<String>,
    #[serde(default)]
    pub(super) node: kanzei_core::WorkNodeContract,
}
