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
    pub references: Vec<WorkReference>,
    pub blocked: bool,
    pub block_reasons: Vec<String>,
    pub progress_provenance: ProgressProvenance,
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

    let mut executable_wip = Vec::new();
    let mut blocked_items = Vec::new();
    for (kind, scheduled, wip_status) in [
        (&REQUIREMENTS, &scheduled_requirements, "doing"),
        (&DEFECTS, &scheduled_defects, "fixing"),
    ] {
        for scheduled_item in scheduled {
            if kind
                .terminal
                .contains(&scheduled_item.entry.status.as_str())
            {
                continue;
            }
            let view = item(
                kind,
                &scheduled_item.entry,
                scheduled_item.block_reasons.clone(),
                &observation,
                &reference_index,
            );
            if view.blocked {
                blocked_items.push(view);
            } else if view.lifecycle_status == wip_status {
                executable_wip.push(view);
            }
        }
    }

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
                scheduled.iter().find_map(|scheduled_item| {
                    (!kind
                        .terminal
                        .contains(&scheduled_item.entry.status.as_str())
                        && scheduled_item.block_reasons.is_empty())
                    .then(|| {
                        item(
                            kind,
                            &scheduled_item.entry,
                            Vec::new(),
                            &observation,
                            &reference_index,
                        )
                    })
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
                None => (
                    WorkDecision::Empty,
                    "需求与缺陷队列均无活动条目".into(),
                    None,
                ),
            }
        }
    };

    Ok(ResolvedControlState {
        schema_version: 1,
        work_priority: priority_name(priority).into(),
        decision,
        reason,
        selected,
        executable_wip: executable_wip.iter().map(WorkItemSummary::from).collect(),
        blocked_items: blocked_items.iter().map(WorkItemSummary::from).collect(),
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
         re-arbitrate queue priority from tracker prose. Call `work next` to refresh after state changes."
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
         Queue priority comes from the run and cannot be overridden by tool input."
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
                return ToolOutput::error(format!(
                    "当前裁决是 {:?}: {}",
                    state.decision, state.reason
                ));
            }
            WorkDecision::Resume | WorkDecision::Start => {}
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
        if let Err(error) = store.save(&entries) {
            return ToolOutput::error(format!("cannot save claim: {error}"));
        }
        ToolOutput::ok(
            json!({
                "claimed": id,
                "lifecycle_status": wip_status,
                "override": !is_default,
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

        DocStore::open(&dir, &DEFECTS)
            .save(&[entry("D-001", "fixing")])
            .unwrap();
        let state = resolve_work_decision(&dir, &dir, WorkPriority::DefectFirst).unwrap();
        assert_eq!(state.decision, WorkDecision::WipViolation);
        assert_eq!(state.executable_wip.len(), 2);
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
