//! 需求/缺陷双队列的确定性取活裁决。
//!
//! 队列模式、WIP、依赖和阻塞由代码合成一个 Resolved Control State；模型只执行
//! Resume/Start，不能再从两份索引和相互冲突的提示词里自行仲裁。

use std::collections::BTreeMap;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use kanzei_harness::auto_run::WorkPriority;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::docstore::{DocKind, DocStore, Entry, DEFECTS, REQUIREMENTS};
use crate::tracker::{dependency_states_from_documents, schedule_for_display_with_states};

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
    pub progress_provenance: ProgressProvenance,
    /// D-354:持有该条目的线(「取得线」字段,claim 时写入)。None = 默认线持有
    /// (含并行化之前的历史 WIP)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
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
        progress_provenance: provenance(entry, observation),
        claimed_by: field(entry, "取得线").map(str::to_string),
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
    let scheduled_requirements = schedule_for_display_with_states(&requirements, &states);
    let scheduled_defects = schedule_for_display_with_states(&defects, &states);
    // D-354:WIP 纪律按线圈定。他线持有的 WIP 不进本线的 Resume/WipViolation,
    // 也不进本线候选——每条线在自己的 WIP 集合里遵守单 WIP。
    let me = line_identity(cwd, project_root);

    let mut executable_wip = Vec::new();
    let mut blocked_items = Vec::new();
    let mut foreign_wip: Vec<WorkItemSummary> = Vec::new();
    let mut integrity_errors = Vec::new();
    for (kind, scheduled, wip_status) in [
        (&REQUIREMENTS, &scheduled_requirements, "doing"),
        (&DEFECTS, &scheduled_defects, "fixing"),
    ] {
        for scheduled_item in scheduled {
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
                &observation,
                &reference_index,
            );
            if view.lifecycle_status == wip_status && view.claimed_by.as_deref() != me.as_deref() {
                // 他线的 WIP:只作背景可见,不进本线任何裁决(blocked 也不进——
                // 它的阻塞该由持有线处理)。
                foreign_wip.push(WorkItemSummary::from(&view));
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
            let queues = match priority {
                WorkPriority::RequirementFirst => [
                    (&REQUIREMENTS, &scheduled_requirements),
                    (&DEFECTS, &scheduled_defects),
                ],
                WorkPriority::DefectFirst => [
                    (&DEFECTS, &scheduled_defects),
                    (&REQUIREMENTS, &scheduled_requirements),
                ],
            };
            let candidate = queues.into_iter().find_map(|(kind, scheduled)| {
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
                    if invalid || status == wip_status {
                        None
                    } else if !kind.terminal.contains(&status)
                        && scheduled_item.block_reasons.is_empty()
                    {
                        Some(item(
                            kind,
                            &scheduled_item.entry,
                            Vec::new(),
                            &observation,
                            &reference_index,
                        ))
                    } else {
                        None
                    }
                })
            });
            match candidate {
                Some(candidate) => (
                    WorkDecision::Start,
                    format!(
                        "无可执行 WIP，按 {} 选择队首 {}",
                        priority_name(priority),
                        candidate.id
                    ),
                    Some(candidate),
                ),
                None if !blocked_items.is_empty() => (
                    WorkDecision::Blocked,
                    "所有非终态条目都带有效阻塞；需要复核阻塞或请求外部解锁".into(),
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
    Ok(ResolvedControlState {
        schema_version: 1,
        work_priority: priority_name(priority).into(),
        decision,
        reason: format!("{reason}{integrity_banner}"),
        selected,
        executable_wip: executable_wip.iter().map(WorkItemSummary::from).collect(),
        blocked_items: blocked_items.iter().map(WorkItemSummary::from).collect(),
        decision_locked,
        integrity_errors,
        line: me,
        foreign_wip,
        resume_reconcile,
    })
}

pub fn resolved_control_prompt(
    cwd: &std::path::Path,
    project_root: &std::path::Path,
    priority: WorkPriority,
) -> String {
    let state = resolve_work_decision(cwd, project_root, priority)
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
         提交、确认哪些批次已落地并把真实进度写回条目,再继续实现——否则会把已完成的批次重做一遍。\n"
    )
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WorkInput {
    /// next: 只读刷新裁决；claim: 原子占用选中条目。
    action: String,
    /// claim 必填。
    #[serde(default)]
    id: Option<String>,
    /// 只有偏离默认 Start 选择时必填，写入条目供审计。
    #[serde(default)]
    reason: Option<String>,
}

pub struct WorkTool;

#[async_trait]
impl Tool for WorkTool {
    fn name(&self) -> &'static str {
        "work"
    }

    fn description(&self) -> String {
        "Resolve the authoritative requirement/defect work decision. `next` returns structured \
         Resume/Start/Blocked/WipViolation; `claim(id)` atomically starts the selected item. \
         WIP discipline is per line: items held by other lines appear as foreign_wip (read-only \
         background) and are never selected for this line; claiming one requires an explicit \
         takeover reason. Queue priority comes from the run and cannot be overridden by tool input."
            .into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(WorkInput)).unwrap();
        schema["properties"]["action"]["enum"] = json!(["next", "claim"]);
        schema
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        let action = input
            .get("action")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        vec![format!(
            "{}:{action}",
            if action == "claim" { "write" } else { "read" }
        )]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: WorkInput = match crate::parse_input(self, input) {
            Ok(input) => input,
            Err(output) => return output,
        };
        if input.action == "next" {
            return match resolve_work_decision(&ctx.cwd, &ctx.project_root, ctx.work_priority) {
                Ok(state) => ToolOutput::ok(
                    serde_json::to_string_pretty(&compact_for_context(state)).unwrap(),
                ),
                Err(error) => ToolOutput::error(error),
            };
        }
        if input.action != "claim" {
            return ToolOutput::error("unknown action; valid: next | claim");
        }
        let Some(id) = input.id.as_deref() else {
            return ToolOutput::error("`id` is required for claim");
        };

        let work_lock_path = ctx.project_root.join(".kanzei/project/work-selection");
        let _work_lock = match crate::atomic_file::lock_exclusive(&work_lock_path) {
            Ok(lock) => lock,
            Err(error) => return ToolOutput::error(format!("cannot lock work selection: {error}")),
        };
        let state = match resolve_work_decision(&ctx.cwd, &ctx.project_root, ctx.work_priority) {
            Ok(state) => state,
            Err(error) => return ToolOutput::error(error),
        };
        match state.decision {
            WorkDecision::WipViolation => {
                return ToolOutput::error(format!(
                    "{}；当前 WIP: {}",
                    state.reason,
                    state
                        .executable_wip
                        .iter()
                        .map(|item| item.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            WorkDecision::Resume if state.selected.as_ref().is_some_and(|item| item.id != id) => {
                return ToolOutput::error(format!(
                    "已有可执行 WIP {}，必须 Resume；claim override 不能再开第二个 WIP",
                    state.selected.as_ref().unwrap().id
                ));
            }
            WorkDecision::Blocked | WorkDecision::Empty => {
                // D-354:Empty/Blocked 里仍可能有他线持有的条目——带非空 reason 的
                // 接管(线停机/用户改派)要能走通,不能被"无可取条目"一票否决。
                let foreign_takeover = state.foreign_wip.iter().any(|item| item.id == id)
                    && input
                        .reason
                        .as_deref()
                        .is_some_and(|reason| !reason.trim().is_empty());
                if !foreign_takeover {
                    return ToolOutput::error(format!(
                        "当前裁决是 {:?}: {}",
                        state.decision, state.reason
                    ));
                }
            }
            WorkDecision::Resume | WorkDecision::Start => {}
        }
        // D-354:他线持有的条目不能顺手 claim——报错要指明「被谁持有」,而不是
        // 笼统的"偏离默认选择"。接管(线死了/用户改派)走 override 通道:带非空
        // reason,接管成功会改写「取得线」并把依据留在审计字段。
        if state.foreign_wip.iter().any(|item| item.id == id)
            && input
                .reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
        {
            return ToolOutput::error(format!(
                "{id} 正被其他线持有(见 foreign_wip),不能重复 claim;\
                 确要接管须提供非空 reason(留取活覆盖审计,接管会改写取得线)"
            ));
        }
        let is_default = state.selected.as_ref().is_some_and(|item| item.id == id);
        if !is_default
            && input
                .reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
        {
            return ToolOutput::error(
                "claim 偏离引擎默认选择时必须提供非空 reason，供取活覆盖审计",
            );
        }

        let (kind, wip_status) = if id.starts_with("R-") {
            (&REQUIREMENTS, "doing")
        } else if id.starts_with("D-") {
            (&DEFECTS, "fixing")
        } else {
            return ToolOutput::error("claim id 必须是 R-xxx 或 D-xxx");
        };
        let store = DocStore::open(&ctx.project_root, kind);
        let _doc_lock = match store.lock() {
            Ok(lock) => lock,
            Err(error) => return ToolOutput::error(format!("cannot lock tracker: {error}")),
        };
        let mut entries = match store.load() {
            Ok(entries) => entries,
            Err(error) => return ToolOutput::error(format!("cannot read tracker: {error}")),
        };
        let Some(position) = entries.iter().position(|entry| entry.id == id) else {
            return ToolOutput::error(format!("unknown id `{id}`"));
        };
        if kind.terminal.contains(&entries[position].status.as_str()) {
            return ToolOutput::error(format!("{id} 已是终态，不能 claim"));
        }
        let me = line_identity(&ctx.cwd, &ctx.project_root);
        if !is_default {
            let refreshed = resolve_work_decision(&ctx.cwd, &ctx.project_root, ctx.work_priority)
                .ok()
                .and_then(|control| control.blocked_items.into_iter().find(|item| item.id == id));
            if let Some(blocked) = refreshed {
                return ToolOutput::error(format!(
                    "{id} 当前被阻塞，不能覆盖 claim: {}",
                    blocked.block_reasons.join("；")
                ));
            }
        }
        if entries[position].status != wip_status {
            if let Err(error) = store.transition_allowed(&entries[position].status, wip_status) {
                return ToolOutput::error(error);
            }
            entries[position].status = wip_status.into();
        }
        let audit = if is_default {
            format!("engine:{}", state.reason)
        } else {
            format!(
                "override:{}",
                input.reason.as_deref().unwrap_or_default().trim()
            )
        };
        match entries[position]
            .fields
            .iter_mut()
            .find(|(key, _)| key == "取活依据")
        {
            Some((_, value)) => *value = audit,
            None => entries[position].fields.push(("取活依据".into(), audit)),
        }
        // D-354:落「取得线」事实(设计 parallel_lines_ui §1.2:被取得是事实不是推断)。
        // 默认线不写字段(无字段 = 默认线),接管时清掉他线残留。
        match &me {
            Some(line) => {
                match entries[position]
                    .fields
                    .iter_mut()
                    .find(|(key, _)| key == "取得线")
                {
                    Some((_, value)) => *value = line.clone(),
                    None => entries[position]
                        .fields
                        .push(("取得线".into(), line.clone())),
                }
            }
            None => entries[position].fields.retain(|(key, _)| key != "取得线"),
        }
        if let Err(error) = store.save(&entries) {
            return ToolOutput::error(format!("cannot save claim: {error}"));
        }
        ToolOutput::ok(
            json!({
                "claimed": id,
                "lifecycle_status": wip_status,
                "override": !is_default,
                "line": me,
                "work_priority": priority_name(ctx.work_priority),
            })
            .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docstore::Entry;
    use crate::tracker::DependencyStates;

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
        assert!(field(&requirements[0], "取活依据")
            .unwrap()
            .contains("用户本轮明确要求"));

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
}
