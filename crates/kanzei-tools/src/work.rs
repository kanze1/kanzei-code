//! 需求/缺陷双队列的确定性取活裁决。
//!
//! 队列模式、WIP、依赖和阻塞由代码合成一个 Resolved Control State；模型只执行
//! Resume/Start，不能再从两份索引和相互冲突的提示词里自行仲裁。

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use kanzei_core::{SessionStore, WorkProjection, WorkUnitStatus};
use kanzei_harness::auto_run::WorkPriority;
use serde::Serialize;
use serde_json::json;

use crate::docstore::{DocKind, DocStore, Entry, DEFECTS, REQUIREMENTS};
use crate::tracker::{
    dependency_states_from_documents, schedule_for_display_with_states, DependencyStates,
    ScheduledEntry,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkDecision {
    Resume,
    Start,
    Blocked,
    WipViolation,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProgressProvenance {
    pub status: String,
    pub reasons: Vec<String>,
    pub recorded_at: Option<String>,
    pub observed_head: Option<String>,
    pub observed_worktree_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub lifecycle_status: String,
    pub severity: Option<String>,
    pub priority: Option<String>,
    /// 保留原文档字段顺序与同名多值；不能收成 map，否则合法的多条验证证据会丢失。
    pub fields: Vec<WorkField>,
    /// R-185:非阻塞前置(「前置:」字段解析出的条目 ID)。可并行,但要在协作
    /// 上下文里对另一条线显式说明——与「依赖」(阻塞,调度跳过)语义分离。
    #[serde(default)]
    pub prerequisites: Vec<String>,
    pub references: Vec<WorkReference>,
    pub blocked: bool,
    pub block_reasons: Vec<String>,
    /// D-434:显式停车(「停车」字段)。与 `blocked` 一样不占 WIP 槽,但语义不同:
    /// 阻塞等外部前提,停车是主动让出单槽的调度决定,复核阻塞时不该被清掉。
    #[serde(default)]
    pub parked: bool,
    /// R-307 批1:停车/阻塞字段带「解除条件:」且所列编号已全部终态时的可观测
    /// 记录(如「停车(解除条件已达成:R-306)」)。该字段已不再计入阻塞;取活方
    /// 认领时顺手把原字段改写为已解除——调度只做动态判定,不自动写回。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub release_notes: Vec<String>,
    pub progress_provenance: ProgressProvenance,
    /// D-354:持有该条目的线(「取得线」字段,claim 时写入)。None = 默认线持有
    /// (含并行化之前的历史 WIP)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    /// work_units_v1 只向模型注入当前执行单元与白名单 Outcome 字段。父需求的批次、
    /// 历史进展、审计锚点不再随每一轮线性累积进入上下文。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_unit_context: Option<WorkUnitContext>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkOutcomeContext {
    pub id: String,
    pub title: String,
    pub fields: Vec<WorkField>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkUnitContext {
    pub outcome: WorkOutcomeContext,
    pub unit: WorkProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkField {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkReference {
    pub id: String,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub lifecycle_status: Option<String>,
    pub archived: bool,
    pub exists: bool,
}

/// D-332:控制面脏数据的明示载体——条目生命周期非法(未知/畸形状态)时,
/// 调度器把它隔离到 integrity_errors,**永不进入** work next 的 WIP/候选/blocked,
/// 不再把「解析失败」静默降级成「非终态、未阻塞、可执行」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntegrityError {
    pub id: String,
    pub kind: String,
    pub field: String,
    pub value: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkItemSummary {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub lifecycle_status: String,
    pub block_reasons: Vec<String>,
}

impl From<&WorkItem> for WorkItemSummary {
    fn from(item: &WorkItem) -> Self {
        Self {
            id: item.id.clone(),
            kind: item.kind.clone(),
            title: item.title.clone(),
            lifecycle_status: item.lifecycle_status.clone(),
            block_reasons: item.block_reasons.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedControlState {
    pub schema_version: u8,
    pub work_priority: String,
    pub decision: WorkDecision,
    pub reason: String,
    pub selected: Option<WorkItem>,
    pub executable_wip: Vec<WorkItemSummary>,
    pub blocked_items: Vec<WorkItemSummary>,
    /// D-434:显式停车的条目。与 blocked_items 分开列,免得复核阻塞的扫荡把
    /// 「主动让出 WIP 槽」当成「过期的自记阻塞」清掉——清完下一轮就撞 wip_violation。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parked_items: Vec<WorkItemSummary>,
    /// D-332 验收⑥:裁决被冻结的标志。Resume/Start 一旦给出且没有新的控制面事实
    /// (队列变化/阻塞解除/用户指示),Agent 不应重新讨论「做不做/做哪个」——
    /// 评估实测:同一 scope decision 反复反刍几千 token,边际信息增益≈0。
    /// 新事实出现后调用 `work next` 刷新,decision_locked 随新裁决更新。
    pub decision_locked: bool,
    /// D-332:生命周期非法的条目(未知/畸形状态)。调度器对其 fail-closed:
    /// 不进 WIP、不进候选、不进 blocked,只在这里明示,等 tracker normalize 修复。
    #[serde(default)]
    pub integrity_errors: Vec<IntegrityError>,
    /// D-354:本次裁决的线身份。None = 主根默认线。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// D-354:其他线正持有的 WIP 条目。对本线是只读背景(协作可见性),
    /// 不参与本线的 Resume/WipViolation/候选裁决。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub foreign_wip: Vec<WorkItemSummary>,
    /// Resume 且进展锚点已陈旧时的**强制前置动作**:先复核已落地范围。
    ///
    /// D-368 实测形态:条目 `fixing`、批次 0/1、进展改动面写着三处待做,而
    /// `observed_head` 已落后于当前 HEAD——三处代码其实早已整块落在一个提交里。
    /// 照着进展字段动手就是把已完成的活重做一遍。当时 agent 是靠自己从文件系统
    /// 反推才没重复实现,这条把「靠自觉」变成裁决面的一等公民。
    ///
    /// 放在裁决顶层而不是拼进 `reason`:reason 会被 claim 原样写进条目的
    /// 「取活依据」字段,那会把整段提示灌进 tracker 文档留下噪音审计行。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_reconcile: Option<String>,
    /// Resume 时的工作树现场——分支、HEAD、未提交改动清单(排除 `.kanzei`)。
    ///
    /// 恢复一条 WIP 的第一件事总是「现在到底改了什么」。这个事实引擎一条 git 命令
    /// 就能给出,却被逐轮外包给模型:实测的恢复开场是 `git status` → `git diff` →
    /// `git log` → `collaboration_status` → 逐个重读改过的文件,四到八步之后才
    /// 写出第一行代码。步数预算被这段吃掉,事务就更容易在中途被切断,下一轮再赔
    /// 一次同样的开场——这是让碎片化自我放大的那条边。
    ///
    /// 只在 Resume 且确实有未提交改动时出现:没有现场就不占篇幅。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_worktree: Option<ResumeWorktree>,
    /// R-349:所有 active tracker 条目的只读机械对账结果；不自动改状态。
    pub reconciliation: ReconciliationReport,
}

#[derive(Debug, Clone)]
pub struct RepoObservation {
    pub recorded_at: String,
    pub observed_head: String,
    pub observed_worktree_hash: String,
}

fn priority_name(priority: WorkPriority) -> &'static str {
    match priority {
        WorkPriority::RequirementFirst => "requirement-first",
        WorkPriority::DefectFirst => "defect-first",
    }
}

fn field<'a>(entry: &'a Entry, key: &str) -> Option<&'a str> {
    entry
        .fields
        .iter()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value.as_str())
}

pub(crate) fn uses_work_units(entry: &Entry) -> bool {
    entry.fields.iter().any(|(key, value)| {
        (key == "执行模型" || key.eq_ignore_ascii_case("execution_model"))
            && value.trim().eq_ignore_ascii_case("work_units_v1")
    })
}

fn outcome_context(entry: &Entry) -> WorkOutcomeContext {
    const CONTEXT_FIELDS: &[&str] = &["目标", "内容", "边界", "验收", "参考", "refs"];
    WorkOutcomeContext {
        id: entry.id.clone(),
        title: entry.title.clone(),
        fields: entry
            .fields
            .iter()
            .filter(|(key, _)| CONTEXT_FIELDS.iter().any(|allowed| key == allowed))
            .map(|(name, value)| WorkField {
                name: name.clone(),
                value: value.clone(),
            })
            .collect(),
    }
}

fn current_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

/// R-247:按「取得线」字段汇总当前活动 WIP，供桌面协作快照和 backlog 共用。
///
/// 无「取得线」字段的 `doing/fixing` 属于默认线；分支线以 git 分支名为身份。
/// 这里只读取 tracker 事实，不从 prompt、排序或运行态推断持有关系。
pub fn active_claims_by_line(
    project_root: &std::path::Path,
) -> Result<BTreeMap<Option<String>, Vec<String>>, String> {
    let mut claims = BTreeMap::<Option<String>, Vec<String>>::new();
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let store = DocStore::open(project_root, kind);
        let entries = store
            .load()
            .map_err(|error| format!("读取 {} 的取得线失败: {error}", kind.rel_path))?;
        let wip_status = kind.statuses[1];
        for entry in entries
            .into_iter()
            .filter(|entry| entry.status == wip_status)
        {
            let owner = field(&entry, "取得线")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            claims.entry(owner).or_default().push(entry.id);
        }
    }
    Ok(claims)
}

/// R-247:释放某条分支线持有的 tracker 条目。
///
/// release 是明确的生命周期动作：`doing/fixing` 回到各自初始态，同时删除「取得线」
/// 并写入一条可审计的最后释放记录。终态或手工异常状态只清持有字段，不倒退状态。
/// 每份文档沿用 DocStore 的跨进程锁和原子保存，不另开旁路写入。
pub fn release_line_claims(
    project_root: &std::path::Path,
    line: &str,
    reason: &str,
) -> Result<Vec<String>, String> {
    let line = line.trim();
    if line.is_empty() {
        return Err("释放取得线时 line 不能为空".into());
    }
    let mut released = Vec::new();
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let store = DocStore::open(project_root, kind);
        let _lock = store
            .lock()
            .map_err(|error| format!("锁定 {} 以释放取得线失败: {error}", kind.rel_path))?;
        let mut entries = store
            .load()
            .map_err(|error| format!("读取 {} 以释放取得线失败: {error}", kind.rel_path))?;
        let mut changed = false;
        for entry in &mut entries {
            let held_here = field(entry, "取得线").is_some_and(|owner| owner.trim() == line);
            if !held_here {
                continue;
            }
            if entry.status == kind.statuses[1] {
                entry.status = kind.statuses[0].to_string();
            }
            entry.fields.retain(|(key, _)| key != "取得线");
            let audit = format!(
                "line={line};reason={};at_ms={}",
                reason.trim(),
                current_unix_ms()
            );
            match entry.fields.iter_mut().find(|(key, _)| key == "取活释放") {
                Some((_, value)) => *value = audit,
                None => entry.fields.push(("取活释放".into(), audit)),
            }
            released.push(entry.id.clone());
            changed = true;
        }
        if changed {
            store
                .save(&entries)
                .map_err(|error| format!("保存 {} 的取得线释放失败: {error}", kind.rel_path))?;
        }
    }
    Ok(released)
}

fn command_output(cwd: &std::path::Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    command.current_dir(cwd).args(args);
    crate::hide_console(&mut command);
    command
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| output.stdout)
        .unwrap_or_default()
}

/// 线身份(D-354):cwd 与主根不同目录 = 并行线(worktree),身份取其 git 分支名,
/// 拿不到分支(裸目录/detached)回落目录名;cwd == 主根 = 默认线,身份为 None。
/// 「取得线」字段与此对齐:条目无该字段 = 默认线持有。这让 WIP 纪律从项目级
/// 收窄为线级——没有它,主线 claim 一条后其他线的裁决永远是 Resume 主线条目
/// 或 WipViolation,任务级并行结构性无法开始。
pub fn line_identity(cwd: &std::path::Path, project_root: &std::path::Path) -> Option<String> {
    let canon =
        |path: &std::path::Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if canon(cwd) == canon(project_root) {
        return None;
    }
    let branch =
        String::from_utf8_lossy(&command_output(cwd, &["rev-parse", "--abbrev-ref", "HEAD"]))
            .trim()
            .to_string();
    if !branch.is_empty() && branch != "HEAD" {
        return Some(branch);
    }
    cwd.file_name()
        .map(|name| name.to_string_lossy().to_string())
}

fn fnv1a64(chunks: &[&[u8]]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for chunk in chunks {
        for byte in *chunk {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("fnv1a64:{hash:016x}")
}

pub fn repo_observation(cwd: &std::path::Path) -> RepoObservation {
    // 托管 tracker 的独立提交不会改变代码事实，不能让刚落的 progress 在配对文档
    // commit 后立刻变 stale。优先锚到最近一次非 `.kanzei` 提交；纯文档仓才回退 HEAD。
    let source_head = command_output(
        cwd,
        &[
            "log",
            "-1",
            "--format=%H",
            "--",
            ".",
            ":(exclude).kanzei/**",
        ],
    );
    let mut head = String::from_utf8_lossy(&source_head).trim().to_string();
    if head.is_empty() {
        head = String::from_utf8_lossy(&command_output(cwd, &["rev-parse", "HEAD"]))
            .trim()
            .to_string();
    }
    let status = command_output(
        cwd,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
            ":(exclude).kanzei/**",
        ],
    );
    let diff = command_output(
        cwd,
        &[
            "diff",
            "--no-ext-diff",
            "--binary",
            "HEAD",
            "--",
            ".",
            ":(exclude).kanzei/**",
        ],
    );
    let untracked = command_output(
        cwd,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
            ":(exclude).kanzei/**",
        ],
    );
    let mut untracked_content = Vec::new();
    for raw in untracked
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        untracked_content.extend_from_slice(raw);
        untracked_content.push(0);
        if let Ok(path) = std::str::from_utf8(raw) {
            if let Ok(bytes) = std::fs::read(cwd.join(path)) {
                untracked_content.extend_from_slice(&bytes);
            }
        }
        untracked_content.push(0xff);
    }
    RepoObservation {
        recorded_at: current_unix_ms().to_string(),
        observed_head: head,
        observed_worktree_hash: fnv1a64(&[&status, &diff, &untracked_content]),
    }
}

pub fn progress_anchor_fields(cwd: &std::path::Path) -> BTreeMap<String, String> {
    let observation = repo_observation(cwd);
    BTreeMap::from([
        ("recorded_at".into(), observation.recorded_at),
        ("observed_head".into(), observation.observed_head),
        (
            "observed_worktree_hash".into(),
            observation.observed_worktree_hash,
        ),
    ])
}

fn future_legacy_date(entry: &Entry) -> Option<String> {
    let progress = field(entry, "进展")?;
    let bytes = progress.as_bytes();
    let today = crate::memory::today();
    for start in 0..bytes.len().saturating_sub(9) {
        let candidate = &bytes[start..start + 10];
        if candidate[4] == b'-'
            && candidate[7] == b'-'
            && candidate
                .iter()
                .enumerate()
                .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
        {
            let date = String::from_utf8_lossy(candidate).to_string();
            if date > today {
                return Some(date);
            }
        }
    }
    None
}

fn provenance(entry: &Entry, observation: &RepoObservation) -> ProgressProvenance {
    let recorded_at = field(entry, "recorded_at").map(str::to_string);
    let observed_head = field(entry, "observed_head").map(str::to_string);
    let observed_worktree_hash = field(entry, "observed_worktree_hash").map(str::to_string);
    let mut reasons = Vec::new();
    let mut has_future_timestamp = false;
    if let Some(date) = future_legacy_date(entry) {
        has_future_timestamp = true;
        reasons.push(format!(
            "legacy progress date {date} is later than current date"
        ));
    }
    if let Some(recorded) = recorded_at
        .as_deref()
        .and_then(|value| value.parse::<u128>().ok())
    {
        if recorded > current_unix_ms().saturating_add(5 * 60 * 1000) {
            has_future_timestamp = true;
            reasons.push("recorded_at is in the future".into());
        }
    }
    let anchors_complete =
        recorded_at.is_some() && observed_head.is_some() && observed_worktree_hash.is_some();
    if anchors_complete {
        if observed_head.as_deref() != Some(observation.observed_head.as_str()) {
            reasons.push("observed_head differs from current HEAD".into());
        }
        if observed_worktree_hash.as_deref() != Some(observation.observed_worktree_hash.as_str()) {
            reasons.push("observed_worktree_hash differs from current worktree".into());
        }
    }
    let status = if has_future_timestamp {
        "future_timestamp"
    } else if !anchors_complete {
        "unanchored"
    } else if reasons.is_empty() {
        "current"
    } else {
        "stale"
    };
    ProgressProvenance {
        status: status.into(),
        reasons,
        recorded_at,
        observed_head,
        observed_worktree_hash,
    }
}

fn item(
    kind: &'static DocKind,
    entry: &Entry,
    block_reasons: Vec<String>,
    states: &DependencyStates,
    observation: &RepoObservation,
    reference_index: &BTreeMap<String, WorkReference>,
) -> WorkItem {
    WorkItem {
        id: entry.id.clone(),
        kind: if kind.prefix == "R" {
            "requirement".into()
        } else {
            "defect".into()
        },
        title: entry.title.clone(),
        lifecycle_status: entry.status.clone(),
        severity: entry.severity.clone(),
        priority: field(entry, "优先级")
            .or_else(|| field(entry, "priority"))
            .map(str::to_string),
        fields: entry
            .fields
            .iter()
            .map(|(name, value)| WorkField {
                name: name.clone(),
                value: value.clone(),
            })
            .collect(),
        // R-185:前置(非阻塞)解析——「前置:」字段里的条目 ID 单独暴露,供协作
        // 上下文/派发器显式说明;与「依赖」(阻塞)分离,不参与调度跳过。
        prerequisites: entry
            .fields
            .iter()
            .filter(|(key, _)| crate::tracker::is_prerequisite_key(key))
            .flat_map(|(_, value)| crate::tracker::tracker_ids(value))
            .collect(),
        references: entry
            .refs()
            .into_iter()
            .map(|id| {
                reference_index.get(&id).cloned().unwrap_or(WorkReference {
                    id,
                    kind: None,
                    title: None,
                    lifecycle_status: None,
                    archived: false,
                    exists: false,
                })
            })
            .collect(),
        blocked: !block_reasons.is_empty(),
        block_reasons,
        // R-307 批1:解除条件已达成的停车视同恢复(parked=false),doing 条目
        // 直接回到 Resume 通道;达成事实经 release_notes 透出供认领时改写字段。
        parked: crate::tracker::park_reason(entry, states).is_some(),
        release_notes: crate::tracker::scheduling::release_notes(entry, states),
        progress_provenance: provenance(entry, observation),
        claimed_by: field(entry, "取得线").map(str::to_string),
        work_unit_context: None,
    }
}

fn work_unit_provenance(
    projection: &WorkProjection,
    observation: &RepoObservation,
) -> ProgressProvenance {
    let Some(checkpoint) = projection.last_checkpoint.as_ref() else {
        return ProgressProvenance {
            status: "unanchored".into(),
            reasons: vec!["work unit 尚无 checkpoint".into()],
            recorded_at: None,
            observed_head: None,
            observed_worktree_hash: None,
        };
    };
    let mut reasons = Vec::new();
    if checkpoint.observed_head != observation.observed_head {
        reasons.push("observed_head differs from current HEAD".into());
    }
    if checkpoint.observed_worktree_hash != observation.observed_worktree_hash {
        reasons.push("observed_worktree_hash differs from current worktree".into());
    }
    ProgressProvenance {
        status: if reasons.is_empty() {
            "current"
        } else {
            "stale"
        }
        .into(),
        reasons,
        recorded_at: Some(projection.updated_at.to_string()),
        observed_head: Some(checkpoint.observed_head.clone()),
        observed_worktree_hash: Some(checkpoint.observed_worktree_hash.clone()),
    }
}

fn work_unit_item(
    projection: &WorkProjection,
    outcome: &Entry,
    block_reasons: Vec<String>,
    observation: &RepoObservation,
    reference_index: &BTreeMap<String, WorkReference>,
) -> WorkItem {
    WorkItem {
        id: projection.unit_id.clone(),
        kind: "work_unit".into(),
        title: projection.objective.clone(),
        lifecycle_status: projection.status.as_str().into(),
        severity: None,
        priority: field(outcome, "优先级")
            .or_else(|| field(outcome, "priority"))
            .map(str::to_string),
        fields: Vec::new(),
        prerequisites: projection.dependencies.clone(),
        references: outcome
            .refs()
            .into_iter()
            .map(|id| {
                reference_index.get(&id).cloned().unwrap_or(WorkReference {
                    id,
                    kind: None,
                    title: None,
                    lifecycle_status: None,
                    archived: false,
                    exists: false,
                })
            })
            .collect(),
        blocked: !block_reasons.is_empty(),
        block_reasons,
        parked: false,
        release_notes: Vec::new(),
        progress_provenance: work_unit_provenance(projection, observation),
        claimed_by: projection.claimed_by.clone(),
        work_unit_context: Some(WorkUnitContext {
            outcome: outcome_context(outcome),
            unit: projection.clone(),
        }),
    }
}

fn reference_index(
    requirements: (&[Entry], &[Entry]),
    defects: (&[Entry], &[Entry]),
) -> BTreeMap<String, WorkReference> {
    let mut index = BTreeMap::new();
    for (kind, (active, archived)) in [(&REQUIREMENTS, requirements), (&DEFECTS, defects)] {
        let kind_name = if kind.prefix == "R" {
            "requirement"
        } else {
            "defect"
        };
        // 先归档后活动：若历史脏数据复用了 id，当前活动状态应覆盖旧快照。
        for (entries, is_archived) in [(archived, true), (active, false)] {
            for entry in entries {
                index.insert(
                    entry.id.clone(),
                    WorkReference {
                        id: entry.id.clone(),
                        kind: Some(kind_name.into()),
                        title: Some(entry.title.clone()),
                        lifecycle_status: Some(entry.status.clone()),
                        archived: is_archived,
                        exists: true,
                    },
                );
            }
        }
    }
    index
}

/// Resume 前的复核提示。只在进展锚点与当前仓库对不上时给出——锚点新鲜就不啰嗦,
/// 免得每轮都念一遍把它念成背景噪音。
///
/// 判据直接复用 `provenance()` 已经算好的 status(current/stale/unanchored/
/// future_timestamp),不重算锚点比对:同一个判断在一个文件里只该有一处实现。
fn resume_reconcile_hint(item: &WorkItem) -> Option<String> {
    let provenance = &item.progress_provenance;
    if !matches!(provenance.status.as_str(), "stale" | "unanchored") {
        return None;
    }
    let why = if provenance.reasons.is_empty() {
        format!("进展锚点状态为 {}", provenance.status)
    } else {
        provenance.reasons.join(";")
    };
    Some(format!(
        "恢复 {} 的第一步是复核已落地范围,不是接着写:{why}。\
         进展字段可能落后于代码——先用 git log / git log -S <符号> 与实际文件确认哪些批次\
         已经实现并提交,把真实进度写回条目,再决定还剩什么要做。\
         D-368 实测:条目写着批次 0/1、改动面三处待做,而三处代码早已整块落在一个提交里。",
        item.id
    ))
}

fn compact_for_context(mut state: ResolvedControlState) -> ResolvedControlState {
    if state.decision != WorkDecision::Blocked {
        state.blocked_items.clear();
    }
    if !matches!(
        state.decision,
        WorkDecision::Resume | WorkDecision::WipViolation
    ) {
        state.executable_wip.clear();
    }
    // R-349:完整对账仍由结构化 resolve 输出保留；注入 prompt 只带 selected 的判据，
    // 不把其它 active 条目的标题/进展灌进模型上下文，延续 prompt 隔离契约。
    let selected_id = state.selected.as_ref().map(|item| item.id.as_str());
    state
        .reconciliation
        .items
        .retain(|item| Some(item.id.as_str()) == selected_id);
    state
}

pub fn resolve_work_decision(
    cwd: &std::path::Path,
    project_root: &std::path::Path,
    priority: WorkPriority,
) -> Result<ResolvedControlState, String> {
    let req_store = DocStore::open(project_root, &REQUIREMENTS);
    let def_store = DocStore::open(project_root, &DEFECTS);
    let requirements = req_store.load().map_err(|error| error.to_string())?;
    let defects = def_store.load().map_err(|error| error.to_string())?;
    let req_archive = req_store
        .load_archive()
        .map_err(|error| error.to_string())?;
    let def_archive = def_store
        .load_archive()
        .map_err(|error| error.to_string())?;
    let states =
        dependency_states_from_documents((&requirements, &req_archive), (&defects, &def_archive));
    let reference_index = reference_index((&requirements, &req_archive), (&defects, &def_archive));
    let observation = repo_observation(cwd);
    let reconciliation = reconcile_active(
        project_root,
        &requirements,
        REQUIREMENTS.terminal,
        &defects,
        DEFECTS.terminal,
        &observation,
    );
    let scheduled_requirements = schedule_for_display_with_states(&requirements, &states);
    let scheduled_defects = schedule_for_display_with_states(&defects, &states);
    // D-354:WIP 纪律按线圈定。他线持有的 WIP 不进本线的 Resume/WipViolation,
    // 也不进本线候选——每条线在自己的 WIP 集合里遵守单 WIP。
    let me = line_identity(cwd, project_root);

    let work_unit_requirement_ids = requirements
        .iter()
        .filter(|entry| uses_work_units(entry))
        .map(|entry| entry.id.clone())
        .collect::<BTreeSet<_>>();
    let known_work_unit_requirement_ids = work_unit_requirement_ids
        .iter()
        .cloned()
        .chain(
            req_archive
                .iter()
                .filter(|entry| uses_work_units(entry))
                .map(|entry| entry.id.clone()),
        )
        .collect::<BTreeSet<_>>();
    let state_path = kanzei_core::project_state_path(project_root);
    let work_units = if state_path.is_file() {
        SessionStore::open(&state_path)
            .map_err(|error| format!("cannot open work unit store: {error}"))?
            .list_work_units(None)
            .map_err(|error| format!("cannot list work units: {error}"))?
    } else {
        Vec::new()
    };

    let mut executable_wip = Vec::new();
    let mut blocked_items = Vec::new();
    let mut parked_items: Vec<WorkItem> = Vec::new();
    let mut foreign_wip: Vec<WorkItemSummary> = Vec::new();
    let mut integrity_errors = Vec::new();
    let mut work_unit_candidates = Vec::new();

    for projection in &work_units {
        if !known_work_unit_requirement_ids.contains(&projection.requirement_id) {
            integrity_errors.push(IntegrityError {
                id: projection.unit_id.clone(),
                kind: "work_unit".into(),
                field: "requirement_id".into(),
                value: projection.requirement_id.clone(),
                message: "work unit 的父需求不存在、已归档或未启用 work_units_v1".into(),
            });
        }
    }

    // work_units_v1 的父 Requirement 只提供 Outcome 顺序与边界；执行/WIP/验证
    // 全由 unit 投影驱动。旧 Requirement 保留原调度语义。
    for scheduled_outcome in &scheduled_requirements {
        let outcome = &scheduled_outcome.entry;
        if !uses_work_units(outcome) || REQUIREMENTS.terminal.contains(&outcome.status.as_str()) {
            continue;
        }
        let outcome_units = work_units
            .iter()
            .filter(|unit| unit.requirement_id == outcome.id)
            .collect::<Vec<_>>();
        if outcome_units.is_empty() {
            let mut view = item(
                &REQUIREMENTS,
                outcome,
                vec!["已启用 work_units_v1，但尚未拆分 Work Unit".into()],
                &states,
                &observation,
                &reference_index,
            );
            view.blocked = true;
            view.block_reasons = vec!["使用 `work create_unit` 建立首个执行单元".into()];
            blocked_items.push(view);
            continue;
        }
        if outcome_units.iter().all(|unit| unit.status.is_terminal()) {
            let mut view = item(
                &REQUIREMENTS,
                outcome,
                vec!["所有 Work Unit 已终态，等待 Outcome 验收关闭".into()],
                &states,
                &observation,
                &reference_index,
            );
            view.blocked = true;
            view.block_reasons = vec!["运行 `req close` 完成 Outcome 级验收".into()];
            blocked_items.push(view);
            continue;
        }
        let outcome_parked = crate::tracker::park_reason(outcome, &states).is_some();
        for projection in outcome_units {
            if projection.status.is_terminal() {
                continue;
            }
            let mut reasons = scheduled_outcome.block_reasons.clone();
            if outcome_parked {
                reasons.push("父 Outcome 已停车".into());
            }
            if !projection.dependencies_satisfied(&work_units) {
                reasons.push(format!(
                    "Work Unit 依赖未完成: {}",
                    projection.dependencies.join("、")
                ));
            }
            if projection.status == WorkUnitStatus::Blocked {
                reasons.push(
                    projection
                        .blocked_reason
                        .clone()
                        .unwrap_or_else(|| "Work Unit 已阻塞".into()),
                );
            }
            let view = work_unit_item(projection, outcome, reasons, &observation, &reference_index);
            match projection.status {
                WorkUnitStatus::Active | WorkUnitStatus::Verifying if view.blocked => {
                    blocked_items.push(view)
                }
                WorkUnitStatus::Active | WorkUnitStatus::Verifying
                    if projection.claimed_by.as_deref() != me.as_deref() =>
                {
                    foreign_wip.push(WorkItemSummary::from(&view))
                }
                WorkUnitStatus::Active | WorkUnitStatus::Verifying => executable_wip.push(view),
                WorkUnitStatus::Ready if view.blocked => blocked_items.push(view),
                WorkUnitStatus::Ready => work_unit_candidates.push(view),
                WorkUnitStatus::Blocked => blocked_items.push(view),
                WorkUnitStatus::Done | WorkUnitStatus::Superseded => unreachable!(),
            }
        }
    }

    for (kind, scheduled, wip_status) in [
        (&REQUIREMENTS, &scheduled_requirements, "doing"),
        (&DEFECTS, &scheduled_defects, "fixing"),
    ] {
        for scheduled_item in scheduled {
            if kind.prefix == "R" && work_unit_requirement_ids.contains(&scheduled_item.entry.id) {
                continue;
            }
            let status = scheduled_item.entry.status.as_str();
            // D-332 fail-closed:状态非空但不在合法枚举 = 控制面脏数据。
            // 隔离到 integrity_errors,永不进 WIP/候选/blocked——不再把解析失败
            // 静默当成「非终态、未阻塞、可执行」(曾让 [open]/[fixed] 污染的需求
            // 重新被取活)。
            if !status.is_empty() && !kind.statuses.contains(&status) {
                integrity_errors.push(IntegrityError {
                    id: scheduled_item.entry.id.clone(),
                    kind: if kind.prefix == "R" {
                        "requirement".into()
                    } else {
                        "defect".into()
                    },
                    field: "lifecycle".into(),
                    value: status.to_string(),
                    message: format!(
                        "invalid {} lifecycle `{}`; valid: {}",
                        if kind.prefix == "R" {
                            "requirement"
                        } else {
                            "defect"
                        },
                        status,
                        kind.statuses.join(" | ")
                    ),
                });
                continue;
            }
            if kind.terminal.contains(&status) {
                continue;
            }
            let view = item(
                kind,
                &scheduled_item.entry,
                scheduled_item.block_reasons.clone(),
                &states,
                &observation,
                &reference_index,
            );
            if view.lifecycle_status == wip_status && view.claimed_by.as_deref() != me.as_deref() {
                // 他线的 WIP:只作背景可见,不进本线任何裁决(blocked 也不进——
                // 它的阻塞该由持有线处理)。
                foreign_wip.push(WorkItemSummary::from(&view));
            } else if reconciliation.already_committed(&view.id) {
                let mut view = view;
                if let Some(reason) = reconciliation.classification_reason(&view.id) {
                    view.blocked = true;
                    view.block_reasons
                        .push(format!("机械对账禁止重复取活：{reason}"));
                }
                blocked_items.push(view);
            } else if view.parked {
                // D-434:停车先于阻塞判定——停车条目照样不可执行,但要落在自己的
                // 清单里,裁决面才分得清「等外部前提」和「主动让槽」。
                parked_items.push(view);
            } else if view.blocked {
                blocked_items.push(view);
            } else if view.lifecycle_status == wip_status {
                executable_wip.push(view);
            }
        }
    }

    // D-332:非法条目存在时,调度器即使有候选也要在原因里点名,让修复通道
    // (tracker normalize)和被隔离的条目对用户可见。
    let integrity_banner = if integrity_errors.is_empty() {
        String::new()
    } else {
        format!(
            "\n[tracker integrity degraded]\n{}\n",
            integrity_errors
                .iter()
                .map(|e| format!("  {}: invalid {} lifecycle [{}]", e.id, e.kind, e.value))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };

    let (decision, reason, selected) = match executable_wip.as_slice() {
        [only] => (
            WorkDecision::Resume,
            format!("唯一可执行 WIP 是 {}，必须先恢复它", only.id),
            Some(only.clone()),
        ),
        [_, _, ..] => (
            WorkDecision::WipViolation,
            format!(
                "检测到 {} 个可执行 WIP；先关闭或 park 到只剩一个，禁止新取活",
                executable_wip.len()
            ),
            None,
        ),
        [] => {
            let legacy_candidate = |kind: &'static DocKind, scheduled: &[ScheduledEntry]| {
                let wip_status = if kind.prefix == "R" {
                    "doing"
                } else {
                    "fixing"
                };
                scheduled.iter().find_map(|scheduled_item| {
                    let status = scheduled_item.entry.status.as_str();
                    // D-332:非法 lifecycle 同样排除出候选(已在 integrity_errors 隔离)。
                    // D-354:WIP 态条目也不是候选——本线的 WIP 走 Resume,他线的
                    // WIP 归持有线,都轮不到 Start。
                    let invalid = !status.is_empty() && !kind.statuses.contains(&status);
                    if invalid
                        || status == wip_status
                        || reconciliation.already_committed(&scheduled_item.entry.id)
                        || (kind.prefix == "R"
                            && work_unit_requirement_ids.contains(&scheduled_item.entry.id))
                    {
                        None
                    } else if !kind.terminal.contains(&status)
                        && scheduled_item.block_reasons.is_empty()
                    {
                        Some(item(
                            kind,
                            &scheduled_item.entry,
                            Vec::new(),
                            &states,
                            &observation,
                            &reference_index,
                        ))
                    } else {
                        None
                    }
                })
            };
            let requirement_candidate = legacy_candidate(&REQUIREMENTS, &scheduled_requirements);
            let defect_candidate = legacy_candidate(&DEFECTS, &scheduled_defects);
            let unit_candidate = work_unit_candidates.first().cloned();
            let candidate = match priority {
                WorkPriority::RequirementFirst => unit_candidate
                    .or(requirement_candidate)
                    .or(defect_candidate),
                WorkPriority::DefectFirst => defect_candidate
                    .or(unit_candidate)
                    .or(requirement_candidate),
            };
            // R-307 批2:全员不可执行时,对存量自由文本停车/阻塞做复核提醒——
            // 字段里点名的 R-/D- 编号全部已终态的条目逐条点名。只是提醒通道,
            // 不改变阻塞状态(R-281 的停车原因消失一天才被人工对账发现的教训)。
            let stale_hints: Vec<String> = blocked_items
                .iter()
                .chain(parked_items.iter())
                .filter_map(|item| {
                    crate::tracker::scheduling::stale_blocker_evidence(
                        item.fields
                            .iter()
                            .map(|field| (field.name.as_str(), field.value.as_str())),
                        &states,
                    )
                    .map(|(label, ids)| {
                        format!(
                            "{} 的{label}前提({})可能已达成,请复核",
                            item.id,
                            ids.join("、")
                        )
                    })
                })
                .collect();
            let stale_banner = if stale_hints.is_empty() {
                String::new()
            } else {
                format!("\n[停车/阻塞复核提醒] {}", stale_hints.join(";"))
            };
            match candidate {
                Some(candidate) => {
                    // R-307 批2:取活依据点名反向依赖权重;批1:解除条件已达成的
                    // 字段随依据透出,让认领方顺手改写。
                    let unblocks =
                        crate::tracker::scheduling::unblocks_count(&states, &candidate.id);
                    let mut reason = format!(
                        "无可执行 WIP，按 {} 选择队首 {}(unblocks={unblocks})",
                        priority_name(priority),
                        candidate.id
                    );
                    if !candidate.release_notes.is_empty() {
                        reason.push_str(&format!(
                            ";{}——认领时顺手把该字段改写为已解除",
                            candidate.release_notes.join("、")
                        ));
                    }
                    (WorkDecision::Start, reason, Some(candidate))
                }
                None if !blocked_items.is_empty() => (
                    WorkDecision::Blocked,
                    if parked_items.is_empty() {
                        format!(
                            "所有非终态条目都带有效阻塞；需要复核阻塞或请求外部解锁{stale_banner}"
                        )
                    } else {
                        // D-434:两类不可执行的处置方式相反——阻塞要复核前提,停车要
                        // 显式恢复。合并成一句会让 agent 拿复核阻塞的手法去动停车条目。
                        format!(
                            "所有非终态条目都不可执行:{} 条带有效阻塞(复核前提或请求外部解锁)，\
                             {} 条显式停车(恢复它们才取活,不要当失效阻塞清掉):{}{stale_banner}",
                            blocked_items.len(),
                            parked_items.len(),
                            parked_items
                                .iter()
                                .map(|item| item.id.as_str())
                                .collect::<Vec<_>>()
                                .join("、")
                        )
                    },
                    None,
                ),
                None if !parked_items.is_empty() => (
                    WorkDecision::Blocked,
                    format!(
                        "所有非终态条目都被显式停车({});恢复其中一条再取活——\
                         停车是主动让出 WIP 槽,不是待复核的阻塞{stale_banner}",
                        parked_items
                            .iter()
                            .map(|item| item.id.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                    None,
                ),
                None if !integrity_errors.is_empty() => (
                    WorkDecision::Blocked,
                    format!(
                        "所有非终态条目都因生命周期非法被隔离(D-332 fail-closed)；\
                         先修复 integrity 再取活：{}",
                        integrity_errors
                            .iter()
                            .map(|e| format!("{} [{}]", e.id, e.value))
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                    None,
                ),
                None if !foreign_wip.is_empty() => (
                    WorkDecision::Empty,
                    format!(
                        "本线({})无可取条目:{} 个活动条目由其他线持有({});\
                         等待他线交付或由用户另行派发",
                        me.as_deref().unwrap_or("主线"),
                        foreign_wip.len(),
                        foreign_wip
                            .iter()
                            .map(|item| item.id.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                    None,
                ),
                None => (
                    WorkDecision::Empty,
                    "需求与缺陷队列均无活动条目".into(),
                    None,
                ),
            }
        }
    };

    let decision_locked = matches!(decision, WorkDecision::Resume | WorkDecision::Start);
    // D-368:Resume 且进展锚点陈旧时,恢复的第一步是复核而不是接着写。
    let resume_reconcile = match (&decision, &selected) {
        (WorkDecision::Resume, Some(item)) => resume_reconcile_hint(item),
        _ => None,
    };
    // 只在 Resume 时采集。Start 是从干净起点开始,工作树里的东西不是它的现场;
    // Blocked/WipViolation 也不需要——那两种裁决的下一步不是写代码。
    let resume_worktree = match decision {
        WorkDecision::Resume => collect_resume_worktree(cwd),
        _ => None,
    };
    Ok(ResolvedControlState {
        schema_version: 2,
        work_priority: priority_name(priority).into(),
        decision,
        reason: format!("{reason}{integrity_banner}"),
        selected,
        executable_wip: executable_wip.iter().map(WorkItemSummary::from).collect(),
        blocked_items: blocked_items.iter().map(WorkItemSummary::from).collect(),
        parked_items: parked_items.iter().map(WorkItemSummary::from).collect(),
        decision_locked,
        integrity_errors,
        line: me,
        foreign_wip,
        resume_reconcile,
        resume_worktree,
        reconciliation,
    })
}

pub fn resolved_control_prompt(
    cwd: &std::path::Path,
    project_root: &std::path::Path,
    priority: WorkPriority,
) -> String {
    resolved_control_prompt_of(resolve_work_decision(cwd, project_root, priority))
}

/// 把**已算好**的裁决渲染成注入块。
///
/// 拆出来是为了让一轮之内只算一次:`resolve_work_decision` 内部有 4 次 git 调用
/// (含 `git diff --binary HEAD`),而同一份裁决既要进 system prompt,也要作为
/// 任务上下文灌给勘察/复核角色。算两次除了浪费,还会出现主代理与角色看到不同
/// 条目的可能——尤其复核发生在实现段之后,重算会选到下一条。
pub fn resolved_control_prompt_of(state: Result<ResolvedControlState, String>) -> String {
    let state = state
        .map(compact_for_context)
        .map(|state| serde_json::to_string_pretty(&state).unwrap_or_else(|_| "{}".into()))
        .unwrap_or_else(|error| json!({"decision": "error", "reason": error}).to_string());
    format!(
        "\n\n<resolved-control-state>\n{state}\n</resolved-control-state>\n\
         This block is the engine's authoritative work decision for the turn. Execute it; do not \
         re-arbitrate queue priority from tracker prose. Call `work next` to refresh after state changes.\n\
         decision_locked=true 时该裁决已冻结:没有新的控制面事实(队列变化/阻塞解除/用户指示)就\
         不要重新讨论做哪个、做不做——直接执行 selected。\n\
         resume_reconcile 非空时:冻结的是「做哪个」,不是「已经做到哪」。先按该字段复核代码与\
         提交、确认哪些批次已落地并把真实进度写回条目,再继续实现——否则会把已完成的批次重做一遍。\n\
         resume_worktree 是引擎已经替你跑过的 git status/diff 结果(已排除 .kanzei 托管文档)。\
         不要再跑一遍 git status / git diff --stat / git log 去问同一个问题——直接从这份清单\
         接着干:先读清单里点到的文件,而不是从头重新勘察。清单为空(字段不存在)= 工作树干净。\n"
    )
}

pub(crate) mod log;
mod reconcile;
pub use reconcile::{reconcile_active, ReconcileClass, ReconcileItem, ReconciliationReport};

mod resume;
use resume::collect_resume_worktree;
pub use resume::ResumeWorktree;

mod tool;
pub use tool::WorkTool;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docstore::Entry;
    use crate::tracker::DependencyStates;
    use kanzei_harness::{Tool, ToolCtx};

    fn entry(id: &str, status: &str) -> Entry {
        Entry {
            id: id.into(),
            title: format!("title {id}"),
            status: status.into(),
            severity: id.starts_with("D-").then(|| "medium".into()),
            fields: vec![],
        }
    }

    fn fixture(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kanzei-work-{tag}-{}-{}",
            std::process::id(),
            current_unix_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 建一个带一次提交的真 git 仓库,供恢复现场用例使用。
    fn git_fixture(tag: &str) -> std::path::PathBuf {
        let dir = fixture(tag);
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .current_dir(&dir)
                .args(args)
                .output()
                .expect("git 可用")
        };
        run(&["init", "--quiet"]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(dir.join("base.txt"), "one\n").unwrap();
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "base"]);
        dir
    }

    /// 验收①:Resume 时把工作树现场直接交给模型,省掉
    /// `git status` → `git diff` → `git log` 这段逐轮重跑的恢复开场。
    #[tokio::test]
    async fn 恢复现场_resume带出未提交现场() {
        let dir = git_fixture("resume-worktree");
        std::fs::write(dir.join("base.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join("extra.txt"), "new\n").unwrap();
        let mut doing = entry("R-001", "doing");
        doing.fields.push(("进展".into(), "批次 1/3".into()));
        DocStore::open(&dir, &REQUIREMENTS).save(&[doing]).unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Resume, "{}", state.reason);
        let worktree = state.resume_worktree.expect("Resume 必须带出现场");
        assert_eq!(worktree.uncommitted_files, 2, "{:?}", worktree.uncommitted);
        assert!(
            worktree
                .uncommitted
                .iter()
                .any(|f| f.starts_with("base.txt +1/-0")),
            "{:?}",
            worktree.uncommitted
        );
        assert!(
            worktree
                .uncommitted
                .iter()
                .any(|f| f == "extra.txt (untracked)"),
            "{:?}",
            worktree.uncommitted
        );
        assert!(worktree.head.contains("base"), "{}", worktree.head);
        std::fs::remove_dir_all(dir).ok();
    }

    /// 干净树不占篇幅;Start 裁决不带现场——工作树里的东西不是它的现场。
    #[tokio::test]
    async fn 恢复现场_干净树与start裁决不带现场() {
        let clean = git_fixture("resume-clean");
        let mut doing = entry("R-001", "doing");
        doing.fields.push(("进展".into(), "批次 1/3".into()));
        DocStore::open(&clean, &REQUIREMENTS)
            .save(&[doing])
            .unwrap();
        let state = resolve_work_decision(&clean, &clean, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Resume, "{}", state.reason);
        assert!(
            state.resume_worktree.is_none(),
            "干净树没有现场要交代: {:?}",
            state.resume_worktree
        );
        std::fs::remove_dir_all(clean).ok();

        let start = git_fixture("resume-start");
        std::fs::write(start.join("base.txt"), "one\ntwo\n").unwrap();
        DocStore::open(&start, &REQUIREMENTS)
            .save(&[entry("R-001", "todo")])
            .unwrap();
        let state = resolve_work_decision(&start, &start, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Start, "{}", state.reason);
        assert!(
            state.resume_worktree.is_none(),
            "Start 不带现场: {:?}",
            state.resume_worktree
        );
        std::fs::remove_dir_all(start).ok();
    }

    /// D-680:handoff 缺少完整完成证据时必须拒绝，避免把临时批次收尾送进鞭挞停机路径。
    #[tokio::test]
    async fn d680_handoff_requires_completion_criterion_and_evidence() {
        // handoff 会把完成条件与证据落成过程事实;ToolCtx::default() 的 project_root
        // 是当前工作目录(跑 cargo test 时就是 crate 目录),会把 .kanzei/artifacts 写进
        // 源码树。用临时根隔离。
        let dir = fixture("handoff");
        let ctx = ToolCtx::new(dir.clone(), dir.clone());
        let missing_criterion = WorkTool
            .execute(
                serde_json::json!({
                    "action": "handoff",
                    "summary": "当前批次暂时没有更多动作，等待真实样本"
                }),
                &ctx,
            )
            .await;
        assert!(missing_criterion.is_error);
        assert_eq!(
            missing_criterion.code,
            Some("HANDOFF_COMPLETION_CRITERION_REQUIRED")
        );
        assert!(
            !dir.join(crate::work::log::WORK_LOG_REL).exists(),
            "被拦下的 handoff 不该留痕——留痕的是发生过的事,不是被拒绝的意图"
        );

        let missing_evidence = WorkTool
            .execute(
                serde_json::json!({
                    "action": "handoff",
                    "summary": "任务完成",
                    "criterion": "D-680 验收条款全部满足"
                }),
                &ctx,
            )
            .await;
        assert!(missing_evidence.is_error);
        assert_eq!(missing_evidence.code, Some("HANDOFF_EVIDENCE_REQUIRED"));

        let accepted = WorkTool
            .execute(
                serde_json::json!({
                    "action": "handoff",
                    "summary": "D-680 验收完成",
                    "criterion": "鞭挞模式不因阶段性收尾交换控制权",
                    "evidence_refs": ["crates/kanzei-tools/src/work/tool.rs:493"]
                }),
                &ctx,
            )
            .await;
        assert!(!accepted.is_error, "{}", accepted.content);
        assert!(accepted.content.contains("model completion declared"));
        assert!(accepted.content.contains("auto-run controller"));
        assert!(!accepted.content.contains("control returned to the user"));

        // 工具为了拿到 criterion/evidence_refs 硬拦了两次,拿到就必须留痕——
        // 否则是"逼模型干活然后扔掉":停机理由恰恰是最该可追溯的。
        let work_log = std::fs::read_to_string(dir.join(crate::work::log::WORK_LOG_REL)).unwrap();
        assert!(
            work_log.contains("鞭挞模式不因阶段性收尾交换控制权"),
            "{work_log}"
        );
        assert!(work_log.contains("work/tool.rs:493"), "{work_log}");
        assert!(work_log.contains("\"event\":\"handoff\""), "{work_log}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// D-368:Resume 且进展锚点已陈旧时,裁决必须把「先复核已落地范围」作为前置动作给出。
    ///
    /// 事故形态:条目 fixing、批次 0/1、改动面写着三处待做,而 observed_head 已落后于
    /// 当前 HEAD——三处代码其实早已整块落在一个提交里。当时 agent 是靠自己从文件系统
    /// 反推才没重复实现一遍;这条把它变成裁决面强制输出。
    #[test]
    fn d368_resume时锚点陈旧给出复核前置() {
        let dir = fixture("d368-stale");
        let mut stale = entry("D-368", "fixing");
        stale.fields.push(("进展".into(), "批次 0/1".into()));
        // 三个锚点齐全(否则算 unanchored),但 head/worktree 都对不上当前仓库 → stale
        stale
            .fields
            .push(("recorded_at".into(), current_unix_ms().to_string()));
        stale.fields.push((
            "observed_head".into(),
            "0000000000000000000000000000000000000000".into(),
        ));
        stale.fields.push((
            "observed_worktree_hash".into(),
            "not-the-current-hash".into(),
        ));
        DocStore::open(&dir, &DEFECTS).save(&[stale]).unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Resume, "{}", state.reason);
        let hint = state
            .resume_reconcile
            .expect("锚点陈旧的 Resume 必须给出复核前置动作");
        assert!(hint.contains("D-368"), "提示要点名条目: {hint}");
        assert!(hint.contains("复核"), "{hint}");
        assert!(
            hint.contains("git log"),
            "提示要给出可执行的复核手段而不是空喊: {hint}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 锚点与当前仓库一致时不给复核提示——每轮都念会把它念成背景噪音,
    /// 真正陈旧那次就没人当回事了。
    #[test]
    fn d368_锚点新鲜时不给复核前置() {
        let dir = fixture("d368-fresh");
        let observation = repo_observation(&dir);
        let mut fresh = entry("D-368", "fixing");
        fresh.fields.push(("进展".into(), "批次 1/1".into()));
        fresh
            .fields
            .push(("recorded_at".into(), observation.recorded_at.clone()));
        fresh
            .fields
            .push(("observed_head".into(), observation.observed_head.clone()));
        fresh.fields.push((
            "observed_worktree_hash".into(),
            observation.observed_worktree_hash.clone(),
        ));
        DocStore::open(&dir, &DEFECTS).save(&[fresh]).unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Resume, "{}", state.reason);
        assert!(
            state.resume_reconcile.is_none(),
            "锚点新鲜不该给复核提示: {:?}",
            state.resume_reconcile
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 非 Resume 裁决不给复核提示——它是「恢复」这个动作的前置,不是通用告示。
    #[test]
    fn d368_非resume裁决不给复核前置() {
        let dir = fixture("d368-start");
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-001", "open")])
            .unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_ne!(state.decision, WorkDecision::Resume, "{}", state.reason);
        assert!(state.resume_reconcile.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn r247_tracker_claim_snapshot_and_release_are_line_scoped() {
        let dir = fixture("r247-release");
        let mut requirement = entry("R-247", "doing");
        requirement
            .fields
            .push(("取得线".into(), "kanzei/line-r247".into()));
        let mut other = entry("D-001", "fixing");
        other.fields.push(("取得线".into(), "kanzei/other".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[requirement])
            .unwrap();
        DocStore::open(&dir, &DEFECTS).save(&[other]).unwrap();

        let before = active_claims_by_line(&dir).unwrap();
        assert_eq!(
            before.get(&Some("kanzei/line-r247".into())),
            Some(&vec!["R-247".into()])
        );
        assert_eq!(
            release_line_claims(&dir, "kanzei/line-r247", "test-close").unwrap(),
            ["R-247"]
        );

        let released = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(released[0].status, "todo", "release 必须回到可领取态");
        assert_eq!(field(&released[0], "取得线"), None);
        assert!(
            field(&released[0], "取活释放").is_some_and(|value| {
                value.contains("kanzei/line-r247") && value.contains("test-close")
            }),
            "release 必须留下线身份与原因审计"
        );
        let untouched = DocStore::open(&dir, &DEFECTS).load().unwrap();
        assert_eq!(untouched[0].status, "fixing");
        assert_eq!(field(&untouched[0], "取得线"), Some("kanzei/other"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unique_wip_resumes_and_multiple_wip_violates() {
        let dir = fixture("wip");
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[entry("R-001", "doing")])
            .unwrap();
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-001", "open")])
            .unwrap();
        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Resume);
        assert_eq!(state.selected.unwrap().id, "R-001");
        // D-332 验收⑥:Resume 裁决一旦给出,decision_locked 必须为 true——
        // Agent 不该再重新讨论「做不做 R-001」(评估实测反复反刍已冻结决策)。
        assert!(state.decision_locked, "Resume 决策必须冻结");

        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-001", "fixing")])
            .unwrap();
        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::WipViolation);
        assert_eq!(state.executable_wip.len(), 2);
        // WipViolation 不是有效裁决,不冻结——必须先收敛 WIP 再取活。
        assert!(!state.decision_locked, "WipViolation 不得冻结决策");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn no_wip_starts_from_selected_priority_queue() {
        let dir = fixture("priority");
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[entry("R-001", "todo")])
            .unwrap();
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-001", "open")])
            .unwrap();
        let defects = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(defects.decision, WorkDecision::Start);
        assert_eq!(defects.selected.unwrap().id, "D-001");
        let requirements =
            resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(requirements.selected.unwrap().id, "R-001");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn all_nonterminal_items_blocked_returns_blocked() {
        let dir = fixture("blocked");
        let mut requirement = entry("R-001", "todo");
        requirement
            .fields
            .push(("阻塞".into(), "等待外部凭证".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[requirement])
            .unwrap();
        let mut defect = entry("D-001", "open");
        defect.fields.push(("阻塞".into(), "等待用户决定".into()));
        DocStore::open(&dir, &DEFECTS).save(&[defect]).unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Blocked);
        assert!(state.selected.is_none());
        assert_eq!(state.blocked_items.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// D-434 验收①:停车的 WIP 不占单槽——同队列另一条 WIP 照常 Resume。
    /// 反例是本条修复前的真实形态:R-221/R-216/R-281/D-349 四个可执行 WIP
    /// 撞 wip_violation,work next 直接拒绝取活。
    #[test]
    fn parked_wip_does_not_consume_the_single_slot() {
        let dir = fixture("parked-slot");
        let mut parked = entry("R-001", "doing");
        parked
            .fields
            .push(("停车".into(), "单 WIP 槽让给 D-001".into()));
        DocStore::open(&dir, &REQUIREMENTS).save(&[parked]).unwrap();
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-001", "fixing")])
            .unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(
            state.decision,
            WorkDecision::Resume,
            "停车条目仍占槽,四个 WIP 撞单槽纪律的老毛病回来了: {}",
            state.reason
        );
        assert_eq!(state.selected.unwrap().id, "D-001");
        assert_eq!(
            state
                .parked_items
                .iter()
                .map(|i| i.id.as_str())
                .collect::<Vec<_>>(),
            vec!["R-001"],
            "停车条目必须落在 parked_items,而不是 blocked_items"
        );
        assert!(
            state.blocked_items.is_empty(),
            "停车不是阻塞:混进 blocked_items 就会被复核阻塞的扫荡清掉"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// D-434 验收②:全员不可执行时,裁决面必须把「等外部前提」和「主动让槽」
    /// 分开说——两者的处置方式相反,合并成一句会让复核阻塞的手法去动停车条目。
    #[test]
    fn parked_and_blocked_are_reported_separately() {
        let dir = fixture("parked-vs-blocked");
        let mut parked = entry("R-001", "doing");
        parked.fields.push(("停车".into(), "让槽给别的活".into()));
        let mut blocked = entry("R-002", "todo");
        blocked.fields.push(("阻塞".into(), "等待外部凭证".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[parked, blocked])
            .unwrap();
        DocStore::open(&dir, &DEFECTS).save(&[]).unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Blocked);
        assert_eq!(state.parked_items.len(), 1, "{:?}", state.parked_items);
        assert_eq!(state.blocked_items.len(), 1, "{:?}", state.blocked_items);
        assert!(
            state.reason.contains("停车") && state.reason.contains("R-001"),
            "理由里必须点名停车条目,否则恢复动作无处着手: {}",
            state.reason
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn resolved_prompt_only_embeds_selected_item_details() {
        let dir = fixture("bounded-prompt");
        let mut selected = entry("D-001", "open");
        selected.title = "selected defect details".into();
        let mut unselected = entry("D-002", "open");
        unselected.title = "private unselected details".into();
        DocStore::open(&dir, &DEFECTS)
            .save(&[selected, unselected])
            .unwrap();

        let prompt = resolved_control_prompt(&dir, &dir, WorkPriority::DefectFirst);
        assert!(prompt.contains("selected defect details"));
        assert!(!prompt.contains("private unselected details"));
        assert!(!prompt.contains("blocked_items\": [\n    {"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn selected_item_resolves_reference_metadata() {
        let dir = fixture("references");
        let mut selected = entry("R-001", "todo");
        selected.fields.push(("refs".into(), "D-009 D-404".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[selected])
            .unwrap();
        let mut archived = entry("D-009", "fixed");
        archived.title = "archived dependency evidence".into();
        let defect_store = DocStore::open(&dir, &DEFECTS);
        defect_store.save(&[archived]).unwrap();
        defect_store.archive_terminal().unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        let references = state.selected.unwrap().references;
        assert_eq!(references[0].id, "D-009");
        assert!(references[0].exists);
        assert!(references[0].archived);
        assert_eq!(
            references[0].title.as_deref(),
            Some("archived dependency evidence")
        );
        assert_eq!(references[1].id, "D-404");
        assert!(!references[1].exists);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_future_progress_is_marked_untrustworthy() {
        let dir = fixture("future-progress");
        let mut requirement = entry("R-001", "doing");
        requirement
            .fields
            .push(("进展".into(), "2099-01-01 已经完成尚未发生的验证".into()));
        assert_eq!(
            future_legacy_date(&requirement).as_deref(),
            Some("2099-01-01")
        );
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[requirement])
            .unwrap();
        let loaded = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(
            future_legacy_date(&loaded[0]).as_deref(),
            Some("2099-01-01"),
            "loaded fields: {:?}",
            loaded[0].fields
        );

        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        let selected = state.selected.expect("unique WIP should be selected");
        assert_eq!(selected.progress_provenance.status, "future_timestamp");
        assert!(selected
            .progress_provenance
            .reasons
            .iter()
            .any(|reason| reason.contains("2099-01-01")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prerequisites_do_not_block_but_dependencies_do() {
        // R-185 验收②:「前置」与「依赖」语义分离——调度器只对「依赖」(阻塞)跳过,
        // 「前置」(非阻塞)不阻塞,并在 WorkItem.prerequisites 里显式暴露供协作上下文消费。
        let dir = fixture("prereq");
        let mut with_prereq = entry("R-001", "todo");
        with_prereq
            .fields
            .push(("前置".into(), "R-002 R-003".into()));
        let mut with_dep = entry("R-004", "todo");
        with_dep.fields.push(("依赖".into(), "R-999".into()));
        let mut pending_dep = entry("R-005", "todo");
        // 未完成的依赖:R-006 存在但非终态 → 阻塞
        pending_dep.fields.push(("依赖".into(), "R-006".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[
                with_prereq.clone(),
                with_dep.clone(),
                pending_dep.clone(),
                entry("R-006", "todo"),
            ])
            .unwrap();
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-001", "open")])
            .unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        // R-001 带「前置」但不阻塞 → 应被选中(requirement-first 队首)
        let selected = state.selected.expect("R-001 不应被前置阻塞");
        assert_eq!(selected.id, "R-001", "前置不得阻塞调度");
        assert!(
            selected.block_reasons.is_empty(),
            "{:?}",
            selected.block_reasons
        );
        // WorkItem.prerequisites 显式暴露(R-002/R-003)
        assert_eq!(
            selected.prerequisites,
            vec!["R-002".to_string(), "R-003".to_string()],
            "前置应解析进 prerequisites: {:?}",
            selected.prerequisites
        );

        // R-004 依赖不存在的 R-999 → 阻塞;R-005 依赖未完成的 R-006 → 阻塞
        // (R-001 是队首被选,但 R-004/R-005 的阻塞应在 blocked 判定可见——
        // 用 schedule 单测直接断言 block_reasons)
        let loaded = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        let states = dependency_states_from_documents((&loaded, &[]), (&[], &[]));
        let reasons_004 = loaded
            .iter()
            .find(|e| e.id == "R-004")
            .map(|e| block_reasons_for_test(e, &states))
            .unwrap();
        assert!(
            reasons_004.iter().any(|r| r.contains("依赖不存在")),
            "依赖不存在应阻塞: {reasons_004:?}"
        );
        let reasons_005 = loaded
            .iter()
            .find(|e| e.id == "R-005")
            .map(|e| block_reasons_for_test(e, &states))
            .unwrap();
        assert!(
            reasons_005.iter().any(|r| r.contains("未完成依赖")),
            "未完成依赖应阻塞: {reasons_005:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn block_reasons_for_test(entry: &Entry, states: &DependencyStates) -> Vec<String> {
        crate::tracker::block_reasons(entry, states)
    }

    #[test]
    fn invalid_lifecycle_is_quarantined_and_never_selected() {
        // D-332:非法 lifecycle(requirement 上的 [open])被隔离进 integrity_errors,
        // 永不进 WIP/候选/blocked——不再像以前那样被当成可执行重新取活。
        let dir = fixture("invalid-lifecycle");
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[entry("R-208", "open"), entry("R-001", "todo")])
            .unwrap();
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-001", "open")])
            .unwrap();

        // 无 WIP 时,非法条目不得被选为 Start 候选(defect-first 应选 D-001)
        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Start);
        assert_eq!(state.selected.unwrap().id, "D-001");
        assert_eq!(state.integrity_errors.len(), 1);
        assert_eq!(state.integrity_errors[0].id, "R-208");
        assert_eq!(state.integrity_errors[0].value, "open");
        assert!(state.reason.contains("[tracker integrity degraded]"));
        assert!(state.reason.contains("R-208"));

        // requirement-first 时应跳过 R-208,选合法的 R-001
        let state = resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(state.selected.unwrap().id, "R-001");

        // 全队列只剩非法条目时 → Blocked(fail-closed,不是 Empty 也不是 Start)
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[entry("R-208", "open")])
            .unwrap();
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-999", "fixed")])
            .unwrap();
        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Blocked);
        assert!(state.reason.contains("生命周期非法被隔离"));
        assert!(state.selected.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reconcile_committed_entry_is_not_selected_again() {
        let dir = git_fixture("reconcile-scheduler");
        let mut committed = entry("D-900", "open");
        committed
            .fields
            .push(("observed_head".into(), repo_observation(&dir).observed_head));
        DocStore::open(&dir, &DEFECTS).save(&[committed]).unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Blocked, "{}", state.reason);
        assert!(state.selected.is_none());
        assert!(state.reconciliation.already_committed("D-900"));
        assert!(state.blocked_items.iter().any(|item| item.id == "D-900"
            && item
                .block_reasons
                .iter()
                .any(|reason| reason.contains("机械对账"))));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn managed_tracker_commit_does_not_invalidate_source_head_anchor() {
        let dir = fixture("managed-head");
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .current_dir(&dir)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "--quiet"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Kanzei Test"]);
        std::fs::write(dir.join("source.txt"), "source").unwrap();
        git(&["add", "source.txt"]);
        git(&["commit", "--quiet", "-m", "source"]);
        let source_observation = repo_observation(&dir);

        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        std::fs::write(
            dir.join(".kanzei/project/requirements.md"),
            "# Requirements\n",
        )
        .unwrap();
        git(&["add", ".kanzei/project/requirements.md"]);
        git(&["commit", "--quiet", "-m", "tracker"]);
        let tracker_observation = repo_observation(&dir);

        assert_eq!(
            source_observation.observed_head,
            tracker_observation.observed_head
        );
        let actual_head = String::from_utf8_lossy(&command_output(&dir, &["rev-parse", "HEAD"]))
            .trim()
            .to_string();
        assert_ne!(tracker_observation.observed_head, actual_head);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// D-354:任务级并行的取活底座。他线持有的 WIP 不进本线裁决,本线能 Start
    /// 未被持有的队首;claim 落「取得线」事实;主根默认线行为完全不变。
    #[tokio::test]
    async fn 并行线取活_他线wip不挡本线start_claim落取得线() {
        let main = fixture("line-main");
        let line_a = fixture("line-par-a");
        // 主线(默认线)已持有 D-001:无「取得线」字段 = 默认线持有。
        DocStore::open(&main, &DEFECTS)
            .save(&[entry("D-001", "fixing"), entry("D-002", "open")])
            .unwrap();
        DocStore::open(&main, &REQUIREMENTS)
            .save(&[entry("R-001", "todo")])
            .unwrap();

        // 线视角:D-001 是他线 WIP → 只进 foreign_wip;本线无 WIP → Start(D-002)。
        // 旧引擎在这里给 Resume(D-001),第二条线永远无法开工——正是 D-354 的病根。
        let state = resolve_work_decision(&line_a, &main, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Start, "{}", state.reason);
        assert_eq!(state.selected.as_ref().unwrap().id, "D-002");
        assert_eq!(state.executable_wip.len(), 0, "他线 WIP 不得进本线 WIP");
        assert_eq!(state.foreign_wip.len(), 1);
        assert_eq!(state.foreign_wip[0].id, "D-001");
        let line_name = state.line.clone().expect("并行线必须有线身份");

        // 线 claim 引擎选中的 D-002:成功,落「取得线」。
        let ctx = ToolCtx::new(line_a.clone(), main.clone())
            .with_work_priority(WorkPriority::DefectFirst);
        let claimed = WorkTool
            .execute(json!({ "action": "claim", "id": "D-002" }), &ctx)
            .await;
        assert!(!claimed.is_error, "{}", claimed.content);
        let defects = DocStore::open(&main, &DEFECTS).load().unwrap();
        let d002 = defects.iter().find(|entry| entry.id == "D-002").unwrap();
        assert_eq!(d002.status, "fixing");
        assert_eq!(field(d002, "取得线"), Some(line_name.as_str()));

        // 线视角复查:Resume 自己的 D-002。
        let state = resolve_work_decision(&line_a, &main, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Resume);
        assert_eq!(state.selected.unwrap().id, "D-002");

        // 主线视角完全不变:Resume 自己的 D-001,不是 WipViolation。
        let main_state = resolve_work_decision(&main, &main, WorkPriority::DefectFirst).unwrap();
        assert_eq!(
            main_state.decision,
            WorkDecision::Resume,
            "{}",
            main_state.reason
        );
        assert_eq!(main_state.selected.unwrap().id, "D-001");
        assert_eq!(
            main_state.foreign_wip.len(),
            1,
            "线持有的 D-002 应作背景可见"
        );
        assert!(main_state.line.is_none(), "主根默认线身份为 None");
        let _ = std::fs::remove_dir_all(main);
        let _ = std::fs::remove_dir_all(line_a);
    }

    /// D-354:他线持有的条目 claim 必须被拒(除非带接管 reason);全部活动条目
    /// 被他线持有时本线裁决为 Empty 并说明持有方,不误报队列已清空可停机。
    #[tokio::test]
    async fn 并行线取活_他线条目拒绝顺手claim_全被持有时明示() {
        let main = fixture("line-guard-main");
        let line_b = fixture("line-guard-b");
        let mut held = entry("D-001", "fixing");
        held.fields.push(("取得线".into(), "par/other".into()));
        DocStore::open(&main, &DEFECTS).save(&[held]).unwrap();
        DocStore::open(&main, &REQUIREMENTS).save(&[]).unwrap();

        // 全部活动条目被他线持有 → Empty,reason 点名持有情况。
        let state = resolve_work_decision(&line_b, &main, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Empty);
        assert!(
            state.reason.contains("其他线持有"),
            "reason 应点名被持有: {}",
            state.reason
        );

        // 无 reason 的 claim 被拒,错误信息指向持有事实。
        let ctx = ToolCtx::new(line_b.clone(), main.clone())
            .with_work_priority(WorkPriority::DefectFirst);
        let rejected = WorkTool
            .execute(json!({ "action": "claim", "id": "D-001" }), &ctx)
            .await;
        assert!(rejected.is_error);
        assert!(
            rejected.content.contains("其他线持有"),
            "{}",
            rejected.content
        );
        let defects = DocStore::open(&main, &DEFECTS).load().unwrap();
        assert_eq!(field(&defects[0], "取得线"), Some("par/other"));

        // 带 reason 的接管:改写「取得线」为本线。
        let takeover = WorkTool
            .execute(
                json!({
                    "action": "claim",
                    "id": "D-001",
                    "reason": "par/other 线已停机,用户指示本线接管",
                }),
                &ctx,
            )
            .await;
        assert!(!takeover.is_error, "{}", takeover.content);
        let defects = DocStore::open(&main, &DEFECTS).load().unwrap();
        let owner = field(&defects[0], "取得线").unwrap().to_string();
        assert_ne!(owner, "par/other", "接管必须改写取得线");
        let _ = std::fs::remove_dir_all(main);
        let _ = std::fs::remove_dir_all(line_b);
    }

    #[tokio::test]
    async fn claim_override_records_reason_but_cannot_bypass_resume() {
        let dir = fixture("claim");
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[entry("R-001", "todo")])
            .unwrap();
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-001", "open")])
            .unwrap();
        let ctx =
            ToolCtx::new(dir.clone(), dir.clone()).with_work_priority(WorkPriority::DefectFirst);
        let tool = WorkTool;
        let claimed = tool
            .execute(
                json!({
                    "action": "claim",
                    "id": "R-001",
                    "reason": "用户本轮明确要求先做这个需求",
                }),
                &ctx,
            )
            .await;
        assert!(!claimed.is_error, "{}", claimed.content);
        let requirements = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(requirements[0].status, "doing");
        // 取活理由留痕,但**不落条目**:它是调度器每轮重算的机制产物,写进 fields 会被
        // structured_entry 序列化进控制状态,变成永久占上下文的过期快照。
        assert!(
            field(&requirements[0], "取活依据").is_none(),
            "机制产物不该腌进条目字段"
        );
        let work_log = std::fs::read_to_string(dir.join(crate::work::log::WORK_LOG_REL)).unwrap();
        assert!(
            work_log.contains("用户本轮明确要求"),
            "override 理由必须留在过程事实里: {work_log}"
        );
        assert!(work_log.contains("\"source\":\"override\""), "{work_log}");

        let rejected = tool
            .execute(
                json!({
                    "action": "claim",
                    "id": "D-001",
                    "reason": "尝试绕过已有 WIP",
                }),
                &ctx,
            )
            .await;
        assert!(rejected.is_error);
        assert!(rejected.content.contains("必须 Resume"));
        let defects = DocStore::open(&dir, &DEFECTS).load().unwrap();
        assert_eq!(defects[0].status, "open");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// R-307 批1:「解除条件:」所列编号全部终态 → 停车条目变为可执行并被取活,
    /// 达成事实经 release_notes/取活依据透出,供认领时顺手改写字段(引擎不写回)。
    #[test]
    fn r307_解除条件达成的停车条目可执行并透出改写提示() {
        let dir = fixture("r307-release-start");
        let mut parked = entry("R-001", "todo");
        parked
            .fields
            .push(("停车".into(), "排队等收编;解除条件:D-001".into()));
        DocStore::open(&dir, &REQUIREMENTS).save(&[parked]).unwrap();
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-001", "fixed")])
            .unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Start, "{}", state.reason);
        let selected = state.selected.expect("解除条件达成的停车条目应可取活");
        assert_eq!(selected.id, "R-001");
        assert!(!selected.parked, "解除条件达成后不再是停车条目");
        assert_eq!(
            selected.release_notes,
            vec!["停车(解除条件已达成:D-001)"],
            "达成事实必须可观测"
        );
        assert!(
            state.reason.contains("解除条件已达成") && state.reason.contains("改写"),
            "取活依据要提示认领方顺手改写字段: {}",
            state.reason
        );
        assert!(
            state.parked_items.is_empty(),
            "已解除的停车不得再进 parked_items: {:?}",
            state.parked_items
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// R-307 批1:停车的 WIP 解除条件达成后直接回 Resume——R-306 停车等 D-565,
    /// D-565 修复归档后 R-306 仍停车无人恢复,正是本条要杀死的实证形态。
    #[test]
    fn r307_解除条件达成的停车wip直接resume() {
        let dir = fixture("r307-release-resume");
        let mut parked_wip = entry("R-306", "doing");
        parked_wip
            .fields
            .push(("停车".into(), "等 D-565 修复;解除条件:D-565".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[parked_wip])
            .unwrap();
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-565", "fixed")])
            .unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(
            state.decision,
            WorkDecision::Resume,
            "停车解除后的 WIP 必须回到 Resume: {}",
            state.reason
        );
        assert_eq!(state.selected.unwrap().id, "R-306");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// R-307 批1:未达成(编号未终态/不在册)仍停车;「解除条件:用户」永不达成。
    #[test]
    fn r307_解除条件未达成仍停车_用户永不达成() {
        let dir = fixture("r307-release-pending");
        let mut pending = entry("R-001", "todo");
        pending
            .fields
            .push(("停车".into(), "排队;解除条件:R-002".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[pending, entry("R-002", "todo")])
            .unwrap();
        DocStore::open(&dir, &DEFECTS).save(&[]).unwrap();

        // R-002 未终态 → R-001 仍停车,候选只剩 R-002。
        let state = resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Start, "{}", state.reason);
        assert_eq!(state.selected.unwrap().id, "R-002");
        assert_eq!(
            state
                .parked_items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["R-001"],
            "解除条件未达成必须维持停车"
        );

        // 「解除条件:用户」:即使全队列只剩它,也维持停车等用户,不被机械放行。
        let mut user_parked = entry("R-010", "todo");
        user_parked
            .fields
            .push(("停车".into(), "等改派;解除条件:用户".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[user_parked])
            .unwrap();
        let state = resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Blocked, "{}", state.reason);
        assert_eq!(state.parked_items.len(), 1, "{:?}", state.parked_items);
        assert!(
            !state.reason.contains("复核提醒"),
            "解除条件:用户 是明确在等,不该被复核提醒误报: {}",
            state.reason
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// R-307 批2:全员不可执行时,存量自由文本停车/阻塞里点名的编号全部终态的
    /// 条目被点名复核;提取不到编号的不误报。阻塞状态本身不变。
    #[test]
    fn r307_blocked诊断点名前提可能已达成的存量停车() {
        let dir = fixture("r307-stale-hint");
        let mut legacy_parked = entry("R-001", "todo");
        legacy_parked
            .fields
            .push(("停车".into(), "排队等 D-486 收口".into()));
        let mut vague_blocked = entry("R-002", "todo");
        vague_blocked
            .fields
            .push(("阻塞".into(), "等用户拍板".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[legacy_parked, vague_blocked])
            .unwrap();
        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-486", "fixed")])
            .unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Blocked, "{}", state.reason);
        assert!(
            state.reason.contains("[停车/阻塞复核提醒]")
                && state.reason.contains("R-001 的停车前提")
                && state.reason.contains("D-486"),
            "存量文本停车的前提已终态必须被点名复核: {}",
            state.reason
        );
        assert!(
            !state.reason.contains("R-002 的"),
            "提取不到编号的条目不得误报: {}",
            state.reason
        );
        assert_eq!(
            state.parked_items.len(),
            1,
            "复核提醒不改变阻塞状态: {:?}",
            state.parked_items
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// R-307 批2:同优先级内 unblocks(直接反向依赖数)大者优先取活,
    /// 取活依据点名权重。
    #[test]
    fn r307_同优先级内unblocks大者优先取活() {
        let dir = fixture("r307-unblocks");
        let mut plain = entry("R-001", "todo");
        plain.fields.push(("优先级".into(), "P1".into()));
        let mut hub = entry("R-002", "todo");
        hub.fields.push(("优先级".into(), "P1".into()));
        let mut dep_a = entry("R-003", "todo");
        dep_a.fields.push(("依赖".into(), "R-002".into()));
        let mut dep_b = entry("R-004", "todo");
        dep_b.fields.push(("依赖".into(), "R-002".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[plain, hub, dep_a, dep_b])
            .unwrap();
        DocStore::open(&dir, &DEFECTS).save(&[]).unwrap();

        let state = resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::Start, "{}", state.reason);
        assert_eq!(
            state.selected.unwrap().id,
            "R-002",
            "unblocks=2 的 R-002 应排到同优先级的 R-001 前面: {}",
            state.reason
        );
        assert!(
            state.reason.contains("unblocks=2"),
            "取活依据必须点名反向依赖权重: {}",
            state.reason
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn work_units_v1_按单元取活并以证据完成() {
        let dir = fixture("work-units-v1");
        let mut outcome = entry("R-001", "todo");
        outcome
            .fields
            .push(("执行模型".into(), "work_units_v1".into()));
        outcome
            .fields
            .push(("目标".into(), "交付可恢复的长程执行底座".into()));
        outcome
            .fields
            .push(("进展".into(), "这段历史不应进入单元上下文".repeat(100)));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[outcome])
            .unwrap();
        DocStore::open(&dir, &DEFECTS).save(&[]).unwrap();
        let ctx = ToolCtx::new(dir.clone(), dir.clone())
            .with_work_priority(WorkPriority::RequirementFirst);
        let tool = WorkTool;

        let before_split =
            resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(before_split.decision, WorkDecision::Blocked);
        assert!(before_split.reason.contains("有效阻塞"));

        let created = tool
            .execute(
                json!({
                    "action": "create_unit",
                    "requirement_id": "R-001",
                    "objective": "实现事件存储",
                    "scope": ["crates/kanzei-core"],
                    "acceptance": ["事件可回放"],
                    "verification": ["cargo test -p kanzei-core"],
                    "base_revision": "base-head"
                }),
                &ctx,
            )
            .await;
        assert!(!created.is_error, "{}", created.content);

        let ready = resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(ready.decision, WorkDecision::Start, "{}", ready.reason);
        let selected = ready.selected.expect("应该选择首个 Work Unit");
        assert_eq!(selected.id, "R-001/W1");
        assert_eq!(selected.kind, "work_unit");
        assert!(selected.fields.is_empty(), "不得注入父需求自由字段历史");
        let context = selected.work_unit_context.expect("必须有有界上下文胶囊");
        assert_eq!(context.outcome.fields.len(), 1);
        assert_eq!(context.outcome.fields[0].name, "目标");
        assert!(
            serde_json::to_vec(&context).unwrap().len() < 8_000,
            "父需求的长进展不得让当前单元上下文随历史线性增长"
        );

        let claimed = tool
            .execute(json!({"action": "claim", "id": "R-001/W1"}), &ctx)
            .await;
        assert!(!claimed.is_error, "{}", claimed.content);
        let req_tool = crate::tracker::TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let early_close = req_tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(early_close.is_error);
        assert!(
            early_close.content.contains("非终态 Work Unit"),
            "{}",
            early_close.content
        );

        for input in [
            json!({
                "action": "checkpoint",
                "id": "R-001/W1",
                "summary": "事件表与投影表已实现",
                "next_action": "运行回放测试"
            }),
            json!({"action": "verify", "id": "R-001/W1"}),
        ] {
            let output = tool.execute(input, &ctx).await;
            assert!(!output.is_error, "{}", output.content);
        }
        let rejected = tool
            .execute(json!({"action": "complete", "id": "R-001/W1"}), &ctx)
            .await;
        assert!(rejected.is_error);
        assert!(rejected.content.contains("未覆盖 acceptance"));

        let evidence = tool
            .execute(
                json!({
                    "action": "evidence",
                    "id": "R-001/W1",
                    "criterion": "事件可回放",
                    "evidence_refs": ["cargo-test:work-events-replay"]
                }),
                &ctx,
            )
            .await;
        assert!(!evidence.is_error, "{}", evidence.content);
        let completed = tool
            .execute(json!({"action": "complete", "id": "R-001/W1"}), &ctx)
            .await;
        assert!(!completed.is_error, "{}", completed.content);

        let requirements = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
        assert_eq!(requirements[0].status, "doing");
        let after = resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(after.decision, WorkDecision::Blocked);
        assert!(after
            .blocked_items
            .iter()
            .any(|item| item.id == "R-001" && item.block_reasons[0].contains("req close")));
        let closed = req_tool
            .execute(json!({"action": "close", "id": "R-001"}), &ctx)
            .await;
        assert!(!closed.is_error, "{}", closed.content);
        let final_state =
            resolve_work_decision(&dir, &dir, WorkPriority::RequirementFirst).unwrap();
        assert_eq!(
            final_state.decision,
            WorkDecision::Empty,
            "{}",
            final_state.reason
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn work_unit接管不能绕过本线已有resume() {
        let dir = fixture("work-unit-takeover-resume");
        let mut outcome = entry("R-001", "doing");
        outcome
            .fields
            .push(("执行模型".into(), "work_units_v1".into()));
        DocStore::open(&dir, &REQUIREMENTS)
            .save(&[outcome])
            .unwrap();
        DocStore::open(&dir, &DEFECTS).save(&[]).unwrap();
        let store = SessionStore::open(&kanzei_core::project_state_path(&dir)).unwrap();
        for index in 1..=2 {
            store
                .create_work_unit(kanzei_core::WorkUnitSpec {
                    unit_id: format!("R-001/W{index}"),
                    requirement_id: "R-001".into(),
                    objective: format!("单元 {index}"),
                    scope: vec![],
                    dependencies: vec![],
                    acceptance: vec![format!("验收 {index}")],
                    verification: vec![],
                    base_revision: "base".into(),
                })
                .unwrap();
        }
        store
            .append_work_fact(
                "R-001/W1",
                kanzei_core::WorkFact::Claimed { claimed_by: None },
            )
            .unwrap();
        store
            .append_work_fact(
                "R-001/W2",
                kanzei_core::WorkFact::Claimed {
                    claimed_by: Some("other-line".into()),
                },
            )
            .unwrap();

        let ctx = ToolCtx::new(dir.clone(), dir.clone())
            .with_work_priority(WorkPriority::RequirementFirst);
        let rejected = WorkTool
            .execute(
                json!({
                    "action": "claim",
                    "id": "R-001/W2",
                    "reason": "尝试接管他线"
                }),
                &ctx,
            )
            .await;
        assert!(rejected.is_error);
        assert!(
            rejected.content.contains("必须 Resume"),
            "{}",
            rejected.content
        );
        assert_eq!(
            store
                .get_work_unit("R-001/W2")
                .unwrap()
                .unwrap()
                .claimed_by
                .as_deref(),
            Some("other-line")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
