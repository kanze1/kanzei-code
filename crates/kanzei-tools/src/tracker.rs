//! 通用追踪工具:req / defect / source / finding 共用一套 CRUD。
//! 硬门禁:ID 引擎分配、状态机受限、格式引擎序列化、引用必须存在——模型只提供字段值。

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::docstore::{DocKind, DocStore, Entry, DEFECTS, REQUIREMENTS};

pub struct TrackerTool {
    pub tool_name: &'static str,
    pub noun: &'static str,
    pub kind: &'static DocKind,
    /// Some(kind) = add/update 时 refs 必须非空且全部存在于该文档(finding → sources)。
    pub requires_refs: Option<&'static DocKind>,
}

#[derive(Deserialize, JsonSchema)]
struct TrackerInput {
    /// list | get | add | update | close | archive | reorder | repair_reused_id
    action: String,
    /// reorder 用:完整的 ID 新顺序(必须恰好覆盖当前全部条目)
    #[serde(default)]
    order: Vec<String>,
    /// get/update/close 必填,如 "R-012"
    #[serde(default)]
    id: Option<String>,
    /// add 必填
    #[serde(default)]
    title: Option<String>,
    /// update 用;close 可指定终态
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    /// 自由字段,如 {"验收": "...", "复现": "..."}
    #[serde(default)]
    fields: BTreeMap<String, String>,
    /// 引用的条目 ID(finding 必须引用 source)
    #[serde(default)]
    refs: Vec<String>,
}

#[async_trait]
impl Tool for TrackerTool {
    fn name(&self) -> &'static str {
        self.tool_name
    }

    fn description(&self) -> String {
        let mut d = format!(
            "Track {}s in the project doc. Actions: list, get(id), add(title, fields), update(id, status/fields), close(id), archive (move terminal entries to the archive file), reorder(order: complete id list — file order IS the user's dev order), repair_reused_id(id) (renumber a semantically different archived entry when its ID was reused by an active entry). Statuses: {}.",
            self.noun,
            self.kind.statuses.join("→"),
        );
        if self.requires_refs.is_some() {
            d.push_str(" `refs` (source IDs) is REQUIRED on add.");
        }
        d
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(TrackerInput)).unwrap()
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: TrackerInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let store = DocStore::open(&ctx.project_root, self.kind);
        let mut entries = match store.load() {
            Ok(e) => e,
            Err(e) => {
                return ToolOutput::error(format!("cannot read {}: {e}", store.path.display()))
            }
        };

        // D-140:完整性告警原先只是拼在输出末尾的一行 warn,模型可以完全忽略——
        // 实测 R-104/107/108/110 从活动与归档同时消失后,告警连响 5 个提交无人处理,
        // 最后靠旧副本偶然捞回。改为:发现缺号/重复时**拒绝一切写操作**,读操作照常放行,
        // 迫使当轮先把数据找回来。错误文本直接给出可执行的恢复路径,避免变成死锁。
        const WRITE_ACTIONS: &[&str] = &[
            "add",
            "update",
            "close",
            "archive",
            "reorder",
            "repair_reused_id",
        ];
        if WRITE_ACTIONS.contains(&input.action.as_str()) && input.action != "repair_reused_id" {
            let issues = store.integrity_issues(&entries);
            if !issues.is_empty() {
                return ToolOutput::error(format!(
                    "REFUSING to write {}: tracker integrity is broken.\n{}\n\
                     Fix it first (reads still work): for a reused ID whose active/archive entries have different meanings, call `{} repair_reused_id <id>`; otherwise find lost entries with \
                     `git log -S \"## <id>\" -- {}` and restore them, or add an explicit \
                     tombstone entry for ids that are genuinely unrecoverable. \
                     Writing now would bury the loss under unrelated changes.",
                    self.kind.rel_path,
                    issues.join("\n"),
                    self.tool_name,
                    self.kind.rel_path,
                ));
            }
        }

        let mut output = match input.action.as_str() {
            "list" => {
                if entries.is_empty() {
                    return ToolOutput::ok(format!("(no {}s yet)", self.noun));
                }
                let dependency_states = match dependency_states(ctx, self.kind, &entries) {
                    Ok(states) => states,
                    Err(e) => return ToolOutput::error(format!("cannot read scheduler dependencies: {e}")),
                };
                let scheduled = schedule_entries(&entries, &dependency_states);
                let mut lines: Vec<String> = scheduled
                    .iter()
                    .map(|(entry, reasons)| render_scheduled_line(entry, reasons))
                    .collect();
                // 饥饿保护:一条可执行都没有是队列的异常状态,不是"没活干"。不加这条横幅时
                // agent 只看到满屏 [blocked:...] 就会判定无可推进项并停住,而阻塞理由多半是
                // 它自己历轮写下的"需先确认方案"(D-147)。
                if scheduled.iter().all(|(_, reasons)| !reasons.is_empty()) {
                    lines.insert(0, deadlock_banner(scheduled.len(), self.noun));
                }
                ToolOutput::ok(lines.join("\n"))
            }
            "get" => {
                let Some(id) = &input.id else {
                    return ToolOutput::error("`id` is required for get");
                };
                match entries.iter().find(|e| &e.id == id) {
                    Some(e) => ToolOutput::ok(render_full(e)),
                    // 已归档条目仍可读:回落到 archive 文件(只读,不可 update)。
                    None => match store
                        .load_archive()
                        .ok()
                        .and_then(|arch| arch.into_iter().find(|e| &e.id == id))
                    {
                        Some(e) => ToolOutput::ok(format!("{} (archived)", render_full(&e))),
                        None => ToolOutput::error(unknown_id(id, &entries)),
                    },
                }
            }
            "repair_reused_id" => {
                let Some(id) = &input.id else {
                    return ToolOutput::error("`id` is required for repair_reused_id");
                };
                match store.repair_reused_archived_id(id) {
                    Ok(new_id) => ToolOutput::ok(format!(
                        "repaired reused ID: archived {id} → {new_id}; active {id} kept unchanged. Commit `{}` and its archive file together.",
                        self.kind.rel_path,
                    )),
                    Err(e) => ToolOutput::error(format!("repair_reused_id failed: {e}")),
                }
            }
            "archive" => match store.archive_terminal() {
                Ok(moved) if moved.is_empty() => {
                    ToolOutput::ok("nothing to archive (no terminal entries)")
                }
                Ok(moved) => {
                    // 归档后回读校验(D-112):移动的 ID 必须真的落在归档文件里。
                    let archived = store.load_archive().unwrap_or_default();
                    let lost: Vec<&String> = moved
                        .iter()
                        .filter(|id| !archived.iter().any(|e| &&e.id == id))
                        .collect();
                    if !lost.is_empty() {
                        return ToolOutput::error(format!(
                            "archive verification FAILED: {} missing from {} after the move — \
                             do NOT commit; the entries may be lost, investigate immediately",
                            lost.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                            store.archive_file().display()
                        ));
                    }
                    ToolOutput::ok(format!(
                        "archived {} terminal {}(s): {} → {}\n\
                         IMPORTANT: `{}` and its archive file were BOTH modified — commit them \
                         together in the SAME commit. Committing only one (or reverting the \
                         archive) permanently loses these entries (D-112).",
                        moved.len(),
                        self.noun,
                        moved.join(", "),
                        store.archive_file().display(),
                        self.kind.rel_path,
                    ))
                }
                Err(e) => ToolOutput::error(format!("archive failed: {e}")),
            },
            "add" => {
                let Some(title) = input.title.as_deref().filter(|t| !t.trim().is_empty()) else {
                    return ToolOutput::error("`title` is required for add");
                };
                if let Some(sev_err) = self.check_severity(&input.severity) {
                    return ToolOutput::error(sev_err);
                }
                if let Some(priority_err) = self.check_priority(&input.priority) {
                    return ToolOutput::error(priority_err);
                }
                if let Err(e) = self.check_refs(ctx, &input.refs, true) {
                    return ToolOutput::error(e);
                }
                let id = store.next_id(&entries);
                let mut fields: Vec<(String, String)> = input.fields.into_iter().collect();
                if !input.refs.is_empty() {
                    fields.push(("refs".into(), input.refs.join(" ")));
                }
                if let Some(priority) = input.priority {
                    fields.push(("优先级".into(), priority));
                }
                let severity = input
                    .severity
                    .or_else(|| self.kind.severities.map(|s| s[s.len() / 2].to_string()));
                entries.push(Entry {
                    id: id.clone(),
                    title: title.trim().to_string(),
                    status: self.kind.statuses[0].to_string(),
                    severity: if self.kind.severities.is_some() {
                        severity
                    } else {
                        None
                    },
                    fields,
                });
                if let Err(e) = store.save(&entries) {
                    return ToolOutput::error(format!(
                        "cannot write {}: {e}",
                        store.path.display()
                    ));
                }
                ToolOutput::ok(format!("added {id} [{}] {title}", self.kind.statuses[0]))
            }
            "update" | "close" => {
                let Some(id) = &input.id else {
                    return ToolOutput::error("`id` is required");
                };
                let Some(pos) = entries.iter().position(|e| &e.id == id) else {
                    return ToolOutput::error(unknown_id(id, &entries));
                };
                if let Some(sev_err) = self.check_severity(&input.severity) {
                    return ToolOutput::error(sev_err);
                }
                if let Some(priority_err) = self.check_priority(&input.priority) {
                    return ToolOutput::error(priority_err);
                }
                if let Err(e) = self.check_refs(ctx, &input.refs, false) {
                    return ToolOutput::error(e);
                }
                let target_status = if input.action == "close" {
                    let status = input
                        .status
                        .clone()
                        .unwrap_or_else(|| self.kind.terminal[0].to_string());
                    if !self.kind.terminal.contains(&status.as_str()) {
                        return ToolOutput::error(format!(
                            "close target must be terminal: {}",
                            self.kind.terminal.join(" | ")
                        ));
                    }
                    Some(status)
                } else {
                    input.status.clone()
                };
                let entry = &mut entries[pos];
                if let Some(status) = target_status {
                    if let Err(e) = store.transition_allowed(&entry.status, &status) {
                        return ToolOutput::error(e);
                    }
                    entry.status = status;
                }
                if let Some(title) = input.title.filter(|t| !t.trim().is_empty()) {
                    entry.title = title.trim().to_string();
                }
                if input.severity.is_some() && self.kind.severities.is_some() {
                    entry.severity = input.severity;
                }
                if let Some(priority) = input.priority {
                    match entry.fields.iter_mut().find(|(key, _)| {
                        key == "优先级" || key.eq_ignore_ascii_case("priority")
                    }) {
                        Some((_, value)) => *value = priority,
                        None => entry.fields.push(("优先级".into(), priority)),
                    }
                }
                for (key, value) in input.fields {
                    match entry.fields.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, slot)) => *slot = value,
                        None => entry.fields.push((key, value)),
                    }
                }
                if !input.refs.is_empty() {
                    let joined = input.refs.join(" ");
                    match entry.fields.iter_mut().find(|(k, _)| k == "refs") {
                        Some((_, slot)) => *slot = joined,
                        None => entry.fields.push(("refs".into(), joined)),
                    }
                }
                let line = render_line(&entries[pos]);
                if let Err(e) = store.save(&entries) {
                    return ToolOutput::error(format!(
                        "cannot write {}: {e}",
                        store.path.display()
                    ));
                }
                ToolOutput::ok(format!("updated: {line}"))
            }
            // R-054:整表重排(文件顺序 = 开发顺序)。要求 order 是现有条目的完整置换,
            // 缺一多一都拒绝——引擎整读整写,天然与并发的状态更新互斥。
            "reorder" => {
                if input.order.is_empty() {
                    return ToolOutput::error("`order` (complete id list) is required for reorder");
                }
                let mut seen = std::collections::HashSet::new();
                for id in &input.order {
                    if !seen.insert(id.as_str()) {
                        return ToolOutput::error(format!("duplicate id `{id}` in order"));
                    }
                    if !entries.iter().any(|e| &e.id == id) {
                        return ToolOutput::error(unknown_id(id, &entries));
                    }
                }
                if input.order.len() != entries.len() {
                    let missing: Vec<&str> = entries
                        .iter()
                        .filter(|e| !input.order.iter().any(|id| id == &e.id))
                        .map(|e| e.id.as_str())
                        .collect();
                    return ToolOutput::error(format!(
                        "order must cover ALL {} entries; missing: {}",
                        entries.len(),
                        missing.join(", ")
                    ));
                }
                let mut reordered = Vec::with_capacity(entries.len());
                for id in &input.order {
                    if let Some(pos) = entries.iter().position(|e| &e.id == id) {
                        reordered.push(entries.remove(pos));
                    }
                }
                if let Err(e) = store.save(&reordered) {
                    return ToolOutput::error(format!(
                        "cannot write {}: {e}",
                        store.path.display()
                    ));
                }
                ToolOutput::ok(format!(
                    "reordered {} {}s: {}",
                    reordered.len(),
                    self.noun,
                    input.order.join(" → ")
                ))
            }
            other => ToolOutput::error(format!(
                "unknown action `{other}`; valid: list | get | add | update | close | archive | reorder"
            )),
        };
        // D-112 门禁:每次调用后对活动∪归档做缺号/重复检测,数据丢失立刻可见,
        // 而不是等 requirements 的依赖引用悬空才被发现。
        if !output.is_error {
            if let Ok(current) = store.load() {
                let issues = store.integrity_issues(&current);
                if !issues.is_empty() {
                    output.content.push_str(&format!(
                        "\n⚠ tracker integrity ({}): {}",
                        self.kind.rel_path,
                        issues.join("; ")
                    ));
                }
            }
        }
        output
    }
}

impl TrackerTool {
    fn check_severity(&self, severity: &Option<String>) -> Option<String> {
        let (Some(sev), Some(valid)) = (severity.as_deref(), self.kind.severities) else {
            return None;
        };
        if valid.contains(&sev) {
            None
        } else {
            Some(format!(
                "invalid severity `{sev}`; valid: {}",
                valid.join(" | ")
            ))
        }
    }

    fn check_priority(&self, priority: &Option<String>) -> Option<String> {
        let (Some(value), Some(valid)) = (priority.as_deref(), self.kind.priorities) else {
            return None;
        };
        if valid.contains(&value) {
            None
        } else {
            Some(format!(
                "invalid priority `{value}`; valid: {}",
                valid.join(" | ")
            ))
        }
    }

    fn check_refs(&self, ctx: &ToolCtx, refs: &[String], adding: bool) -> Result<(), String> {
        let Some(ref_kind) = self.requires_refs else {
            return Ok(());
        };
        if refs.is_empty() {
            if adding {
                let available = DocStore::open(&ctx.project_root, ref_kind)
                    .load()
                    .map(|entries| {
                        entries
                            .iter()
                            .map(render_line)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                return Err(format!(
                    "every {} MUST cite at least one source via `refs`. Existing sources:\n{}",
                    self.noun,
                    if available.is_empty() {
                        "(none — record a source first)"
                    } else {
                        &available
                    },
                ));
            }
            return Ok(());
        }
        let existing = DocStore::open(&ctx.project_root, ref_kind)
            .load()
            .map_err(|e| e.to_string())?;
        for id in refs {
            if !existing.iter().any(|e| &e.id == id) {
                return Err(format!(
                    "ref `{id}` does not exist. {}",
                    unknown_id(id, &existing)
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ScheduledEntry {
    pub entry: Entry,
    pub block_reasons: Vec<String>,
}

/// 为桌面端文档快照提供与 req/defect list 相同的阻塞判断和稳定后置顺序。
pub fn schedule_for_display(
    ctx: &ToolCtx,
    kind: &'static DocKind,
    entries: &[Entry],
) -> Result<Vec<ScheduledEntry>, String> {
    let states = dependency_states(ctx, kind, entries)?;
    let scheduled = schedule_entries(entries, &states);
    Ok(scheduled
        .into_iter()
        .map(|(entry, block_reasons)| ScheduledEntry {
            entry: entry.clone(),
            block_reasons,
        })
        .collect())
}

#[derive(Default)]
struct DependencyStates {
    terminal: BTreeMap<String, bool>,
    deps: BTreeMap<String, Vec<String>>,
}

impl DependencyStates {
    fn get(&self, id: &str) -> Option<&bool> {
        self.terminal.get(id)
    }

    fn is_terminal(&self, id: &str) -> bool {
        self.terminal.get(id).copied().unwrap_or(false)
    }

    /// 沿**未完成**依赖从 start 出发,能走回 start 就返回环路径。已归档依赖不构成
    /// 阻塞,自然也不参与成环。返回的路径首尾都是 start,方便直接打印。
    fn cycle_from(&self, start: &str) -> Option<Vec<String>> {
        let mut path = vec![start.to_string()];
        let mut visited = BTreeSet::new();
        self.walk(start, start, &mut path, &mut visited)
            .then_some(path)
    }

    fn walk(
        &self,
        node: &str,
        start: &str,
        path: &mut Vec<String>,
        visited: &mut BTreeSet<String>,
    ) -> bool {
        let Some(deps) = self.deps.get(node) else {
            return false;
        };
        for dep in deps {
            if self.is_terminal(dep) {
                continue;
            }
            path.push(dep.clone());
            if dep == start {
                return true;
            }
            if visited.insert(dep.clone()) && self.walk(dep, start, path, visited) {
                return true;
            }
            path.pop();
        }
        false
    }
}

fn dependency_states(
    ctx: &ToolCtx,
    current_kind: &DocKind,
    current_entries: &[Entry],
) -> Result<DependencyStates, String> {
    let mut states = DependencyStates::default();
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let active = if kind.rel_path == current_kind.rel_path {
            current_entries.to_vec()
        } else {
            DocStore::open(&ctx.project_root, kind)
                .load()
                .map_err(|e| format!("{}: {e}", kind.rel_path))?
        };
        let archived = DocStore::open(&ctx.project_root, kind)
            .load_archive()
            .map_err(|e| format!("{} archive: {e}", kind.rel_path))?;
        for entry in active.into_iter().chain(archived) {
            let deps: Vec<String> = entry
                .fields
                .iter()
                .filter(|(key, _)| is_dependency_key(key))
                .flat_map(|(_, value)| tracker_ids(value))
                .collect();
            states
                .terminal
                .insert(entry.id.clone(), kind.terminal.contains(&entry.status.as_str()));
            if !deps.is_empty() {
                states.deps.insert(entry.id, deps);
            }
        }
    }
    Ok(states)
}

fn schedule_entries<'a>(
    entries: &'a [Entry],
    states: &DependencyStates,
) -> Vec<(&'a Entry, Vec<String>)> {
    // 稳定分区:不改写 Markdown,只在取活输出中把当前不可执行项后置；
    // 因此阻塞解除后会自动回到原文档顺序。
    let mut executable = Vec::with_capacity(entries.len());
    let mut blocked = Vec::new();
    for entry in entries {
        let reasons = block_reasons(entry, states);
        if reasons.is_empty() {
            executable.push((entry, reasons));
        } else {
            blocked.push((entry, reasons));
        }
    }
    executable.extend(blocked);
    executable
}

fn block_reasons(entry: &Entry, states: &DependencyStates) -> Vec<String> {
    let mut reasons = Vec::new();
    // 环上的条目永远等不到依赖完成。只报"未完成依赖"会让 agent 一轮轮空等一个
    // 不可能到来的前置,所以直接点出环并要求断边(D-147)。
    let cycle = states.cycle_from(&entry.id);
    for (key, value) in &entry.fields {
        if is_blocker_key(key) && is_present_blocker(value) {
            reasons.push(format!("阻塞字段: {}", value.trim()));
        }
        if is_dependency_key(key) && cycle.is_none() {
            for id in tracker_ids(value) {
                match states.get(&id) {
                    Some(true) => {}
                    Some(false) => reasons.push(format!("未完成依赖: {id}")),
                    None => reasons.push(format!("依赖不存在: {id}")),
                }
            }
        }
        if is_stage_key(key) && is_deferred_stage(value) {
            reasons.push(format!("阶段门槛: {}", value.trim()));
        }
    }
    if let Some(path) = cycle {
        reasons.push(format!(
            "循环依赖: {} —— 环上没有条目能先完成,必须断掉其中一条边(把不成立的依赖移入 refs)",
            path.join(" → ")
        ));
    }
    reasons
}

/// 全部条目被判阻塞时置顶的横幅。措辞是刻意的:先要求复核既有阻塞(多数是自记的),
/// 复核后仍全阻塞才允许升级给用户,并且必须点名缺哪个决策——"无可执行条目"不是合格收尾。
fn deadlock_banner(total: usize, noun: &str) -> String {
    format!(
        "[调度死锁] {total} 条{noun}全部带阻塞标记,可执行数为 0。这是队列异常,不是没活干。\n\
         1. 先逐条复核阻塞是否仍成立:依赖已归档、条件已满足、方案其实早已确认的,\
         清空该条的「阻塞」字段再取活——自己历轮写下的「需先确认方案」不算外部阻塞。\n\
         2. 复核后仍全阻塞,才回复用户,并逐条点名缺的是哪一个具体决策。\n\
         禁止以「没有可执行条目」「本轮停止」之类的纯文本收尾。"
    )
}

fn render_scheduled_line(entry: &Entry, reasons: &[String]) -> String {
    let line = render_line(entry);
    if reasons.is_empty() {
        line
    } else {
        format!("{line} [blocked: {}]", reasons.join("；"))
    }
}

fn is_blocker_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    key.contains("阻塞") || matches!(lower.as_str(), "blocked" | "blocker" | "blocking")
}

fn is_dependency_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    key.trim() == "依赖" || matches!(lower.as_str(), "dependency" | "dependencies" | "depends_on")
}

fn is_stage_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    key.trim() == "阶段" || matches!(lower.as_str(), "stage" | "phase")
}

fn is_present_blocker(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "无" | "否" | "none" | "no" | "false" | "未阻塞" | "暂无"
        )
}

fn is_deferred_stage(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    value.contains("后")
        || value.contains("以后")
        || lower.contains("after")
        || lower.contains("later")
}

/// 从自由文本中提取 R-001/D-002 形式的追踪 ID,兼容中文标点和说明文字。
fn tracker_ids(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut ids = Vec::new();
    let mut i = 0;
    while i + 2 < chars.len() {
        let prefix = chars[i].to_ascii_uppercase();
        if !matches!(prefix, 'R' | 'D') || chars[i + 1] != '-' || !chars[i + 2].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut end = i + 2;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
        let id: String = chars[i..end].iter().collect();
        if !ids.contains(&id) {
            ids.push(id);
        }
        i = end;
    }
    ids
}

fn render_line(e: &Entry) -> String {
    let sev = e
        .severity
        .as_ref()
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    format!("{} [{}]{sev} {}", e.id, e.status, e.title)
}

fn render_full(e: &Entry) -> String {
    let mut out = render_line(e);
    for (key, value) in &e.fields {
        out.push_str(&format!("\n- {key}: {value}"));
    }
    out
}

fn unknown_id(id: &str, entries: &[Entry]) -> String {
    let known: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    format!(
        "unknown id `{id}`; existing: {}",
        if known.is_empty() {
            "(none)".into()
        } else {
            known.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::TrackerTool;
    use crate::docstore::{DocStore, Entry, GOALS, REQUIREMENTS};
    use kanzei_harness::{Tool, ToolCtx};
    use serde_json::json;

    fn entry(id: &str) -> Entry {
        Entry {
            id: id.into(),
            title: format!("t-{id}"),
            status: "todo".into(),
            severity: None,
            fields: vec![],
        }
    }

    #[tokio::test]
    async fn reorder_rewrites_file_order_and_rejects_partial() {
        let dir = std::env::temp_dir().join(format!("kz-reorder-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        store
            .save(&[entry("R-001"), entry("R-002"), entry("R-003")])
            .unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        // 完整置换:成功且文件顺序改变。
        let out = tool
            .execute(json!({"action": "reorder", "order": ["R-003", "R-001", "R-002"]}), &ctx)
            .await;
        assert!(!out.is_error, "{}", out.content);
        let ids: Vec<String> = store.load().unwrap().iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["R-003", "R-001", "R-002"]);

        // 不完整清单:拒绝且顺序不变。
        let out = tool
            .execute(json!({"action": "reorder", "order": ["R-001"]}), &ctx)
            .await;
        assert!(out.is_error);
        let ids: Vec<String> = store.load().unwrap().iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids, vec!["R-003", "R-001", "R-002"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn add_preserves_handwritten_free_text_and_unknown_blocks() {
        let dir = std::env::temp_dir().join(format!("kz-preserve-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let path = dir.join(REQUIREMENTS.rel_path);
        std::fs::write(
            &path,
            "# Requirements\n\n手写说明: 不应被引擎删除\n- 就是个备注\n\n## R-001 已有条目 [todo]\n### 子标题\n```text\n用户代码块\n```\n- 验收: 原字段\n",
        )
        .unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let output = tool
            .execute(json!({"action": "add", "title": "新条目"}), &ToolCtx::new(dir.clone()))
            .await;
        assert!(!output.is_error, "{}", output.content);
        let saved = std::fs::read_to_string(path).unwrap();
        for line in ["手写说明: 不应被引擎删除", "- 就是个备注", "### 子标题", "用户代码块"] {
            assert!(saved.contains(line), "missing preserved line: {line}");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn archive_preserves_handwritten_free_text_and_unknown_blocks() {
        let dir = std::env::temp_dir().join(format!("kz-archive-preserve-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let path = dir.join(REQUIREMENTS.rel_path);
        std::fs::write(
            &path,
            "# Requirements\n\n## R-001 已完成条目 [done]\n终态说明: 归档时也不能丢\n- 手写备注\n### 归档子标题\n```text\n归档代码块\n```\n- 验收: 原字段\n\n## R-002 进行中条目 [doing]\n- 验收: 保留在活动文档\n",
        )
        .unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let output = tool
            .execute(json!({"action": "archive"}), &ToolCtx::new(dir.clone()))
            .await;
        assert!(!output.is_error, "{}", output.content);

        let archive = std::fs::read_to_string(
            dir.join(".kanzei/project/requirements-archive.md"),
        )
        .unwrap();
        for line in ["终态说明: 归档时也不能丢", "- 手写备注", "### 归档子标题", "归档代码块"] {
            assert!(archive.contains(line), "missing preserved line: {line}");
        }
        let active = std::fs::read_to_string(path).unwrap();
        assert!(!active.contains("终态说明: 归档时也不能丢"));
        assert!(active.contains("## R-002 进行中条目 [doing]"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn 完整性破损时拒绝写操作但读操作放行() {
        // D-140:告警是 warn-only 时无人处理,实测缺号连响 5 个提交。
        let dir = std::env::temp_dir().join(format!(
            "kz-integrity-gate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        // 制造缺号:R-001 与 R-003 存在,R-002 缺失。
        store.save(&[entry("R-001"), entry("R-003")]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        // 读操作必须仍然可用——否则模型无法诊断,就成了死锁。
        for action in ["list", "get"] {
            let out = tool
                .execute(json!({"action": action, "id": "R-001"}), &ctx)
                .await;
            assert!(!out.is_error, "读操作不该被拦: {action} -> {}", out.content);
        }
        // 写操作全部被拒,且错误里给出可执行的恢复路径。
        for input in [
            json!({"action": "add", "title": "新条目"}),
            json!({"action": "update", "id": "R-001", "status": "doing"}),
            json!({"action": "close", "id": "R-001"}),
            json!({"action": "archive"}),
            json!({"action": "reorder", "order": ["R-003", "R-001"]}),
        ] {
            let action = input["action"].as_str().unwrap().to_string();
            let out = tool.execute(input, &ctx).await;
            assert!(out.is_error, "写操作必须被拒: {action}");
            assert!(out.content.contains("R-002"), "要指名缺失的 id: {}", out.content);
            assert!(out.content.contains("git log -S"), "要给恢复路径: {}", out.content);
        }
        // 补齐缺号后写操作恢复正常。
        store
            .save(&[entry("R-001"), entry("R-002"), entry("R-003")])
            .unwrap();
        let out = tool.execute(json!({"action": "add", "title": "恢复后可写"}), &ctx).await;
        assert!(!out.is_error, "完整性恢复后应放行: {}", out.content);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn 复用历史_id_可改号修复且不会丢归档自由内容() {
        let dir = std::env::temp_dir().join(format!(
            "kz-reused-id-repair-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = dir.join(".kanzei/project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("goals.md"),
            "# Goals\n\n## G-001 长期目标 [active]\n- 进展: 旧\n\n## G-002 新长期目标 [active]\n- 来源: R-093\n",
        )
        .unwrap();
        std::fs::write(
            project.join("goals-archive.md"),
            "# Goals Archive\n\n## G-002 旧短期目标 [achieved]\n- 验收: 达成即 `goal update G-002 achieved`\n\n手写归档说明 G-002 不能丢\n\n## G-003 另一目标 [achieved]\n",
        )
        .unwrap();
        let tool = TrackerTool {
            tool_name: "goal",
            noun: "goal",
            kind: &GOALS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        let repaired = tool
            .execute(json!({"action": "repair_reused_id", "id": "G-002"}), &ctx)
            .await;
        assert!(!repaired.is_error, "{}", repaired.content);
        assert!(repaired.content.contains("G-004"), "{}", repaired.content);
        let active = std::fs::read_to_string(project.join("goals.md")).unwrap();
        let archive = std::fs::read_to_string(project.join("goals-archive.md")).unwrap();
        assert!(active.contains("## G-002 新长期目标 [active]"));
        assert!(archive.contains("## G-004 旧短期目标 [achieved]"));
        assert!(archive.contains("goal update G-004 achieved"));
        assert!(archive.contains("手写归档说明 G-004 不能丢"));

        let updated = tool
            .execute(
                json!({"action": "update", "id": "G-001", "fields": {"进展": "新"}}),
                &ctx,
            )
            .await;
        assert!(!updated.is_error, "修复后普通写操作应恢复: {}", updated.content);
        let store = DocStore::open(&dir, &GOALS);
        let entries = store.load().unwrap();
        assert!(store.integrity_issues(&entries).is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn integrity_warning_surfaces_in_tool_output() {
        // D-112:缺号(R-002)必须出现在每次成功调用的输出里。
        let dir = std::env::temp_dir().join(format!(
            "kz-tracker-integrity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        store.save(&[entry("R-001"), entry("R-003")]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let out = tool
            .execute(json!({"action": "list"}), &ToolCtx::new(dir.clone()))
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("tracker integrity"), "{}", out.content);
        assert!(out.content.contains("R-002"), "{}", out.content);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn archive_reports_moved_ids_and_requires_paired_commit() {
        // D-112:归档输出必须列出移动的 ID 并要求两文件同一提交。
        let dir = std::env::temp_dir().join(format!(
            "kz-tracker-archive-msg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut done = entry("R-001");
        done.status = "done".into();
        store.save(&[done, entry("R-002")]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let out = tool
            .execute(json!({"action": "archive"}), &ToolCtx::new(dir.clone()))
            .await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("R-001"), "{}", out.content);
        assert!(out.content.contains("SAME commit"), "{}", out.content);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn priority_update_reuses_existing_english_field() {
        let dir = std::env::temp_dir().join(format!("kz-priority-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut item = entry("R-001");
        item.fields.push(("priority".into(), "P2".into()));
        store.save(&[item]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let output = tool
            .execute(
                json!({"action": "update", "id": "R-001", "priority": "P0"}),
                &ToolCtx::new(dir.clone()),
            )
            .await;
        assert!(!output.is_error, "{}", output.content);
        let fields = &store.load().unwrap()[0].fields;
        assert_eq!(fields, &[("priority".into(), "P0".into())]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn list_stably_postpones_blocked_entries_and_restores_order() {
        let dir = std::env::temp_dir().join(format!(
            "kz-scheduler-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut blocked = entry("R-001");
        blocked.fields.push(("阻塞".into(), "等待用户确认".into()));
        let mut dependency_blocked = entry("R-003");
        dependency_blocked
            .fields
            .push(("依赖".into(), "R-002".into()));
        let mut stage_blocked = entry("R-004");
        stage_blocked.fields.push(("阶段".into(), "5 后".into()));
        store
            .save(&[blocked, entry("R-002"), dependency_blocked, stage_blocked])
            .unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        let lines: Vec<&str> = out.content.lines().collect();
        assert!(lines[0].starts_with("R-002"), "{}", out.content);
        assert!(lines[1].contains("R-001") && lines[1].contains("阻塞字段"));
        assert!(lines[2].contains("R-003") && lines[2].contains("未完成依赖: R-002"));
        assert!(lines[3].contains("R-004") && lines[3].contains("阶段门槛"));

        // 解除两个阻塞后，原文档顺序自动恢复；没有持久化一份临时排序。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"阻塞": ""}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-002", "status": "done"}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        let lines: Vec<&str> = out.content.lines().collect();
        assert!(lines[0].starts_with("R-001"), "{}", out.content);
        assert!(lines[1].starts_with("R-002"), "{}", out.content);
        assert!(lines[2].starts_with("R-003"), "{}", out.content);
        assert!(lines[3].contains("R-004") && lines[3].contains("阶段门槛"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn list_banners_deadlock_when_nothing_is_executable() {
        let dir = std::env::temp_dir().join(format!(
            "kz-deadlock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut first = entry("R-001");
        first.fields.push(("阻塞".into(), "等待用户确认方案".into()));
        let mut second = entry("R-002");
        second.fields.push(("阻塞".into(), "依赖外部服务".into()));
        store.save(&[first, second]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.starts_with("[调度死锁] 2 条requirement"), "{}", out.content);
        // 横幅必须堵死"没有可执行条目"这条收尾话术,否则鞭挞照旧静默停。
        assert!(out.content.contains("禁止以「没有可执行条目」"), "{}", out.content);
        assert!(out.content.contains("R-001") && out.content.contains("R-002"));

        // 只要有一条恢复可执行,横幅就消失——它只在真死锁时出现,不构成日常噪音。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-001", "fields": {"阻塞": ""}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(!out.content.contains("[调度死锁]"), "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn list_names_dependency_cycles_instead_of_endless_waiting() {
        let dir = std::env::temp_dir().join(format!(
            "kz-cycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
        let store = DocStore::open(&dir, &REQUIREMENTS);
        let mut a = entry("R-001");
        a.fields.push(("依赖".into(), "R-002".into()));
        let mut b = entry("R-002");
        b.fields.push(("依赖".into(), "R-001".into()));
        // 环外的普通未完成依赖仍应照旧报"未完成依赖",不被环检测吞掉。
        let mut c = entry("R-003");
        c.fields.push(("依赖".into(), "R-004".into()));
        store.save(&[a, b, c, entry("R-004")]).unwrap();
        let tool = TrackerTool {
            tool_name: "req",
            noun: "requirement",
            kind: &REQUIREMENTS,
            requires_refs: None,
        };
        let ctx = ToolCtx::new(dir.clone());

        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        let cycle_line = out
            .content
            .lines()
            .find(|l| l.starts_with("R-001"))
            .unwrap_or_default();
        assert!(
            cycle_line.contains("循环依赖: R-001 → R-002 → R-001"),
            "{}",
            out.content
        );
        assert!(cycle_line.contains("断掉其中一条边"), "{}", out.content);
        let plain = out
            .content
            .lines()
            .find(|l| l.starts_with("R-003"))
            .unwrap_or_default();
        assert!(plain.contains("未完成依赖: R-004"), "{}", out.content);
        assert!(!plain.contains("循环依赖"), "{}", out.content);

        // 断边后环消失,两条都回到可执行。
        let out = tool
            .execute(
                json!({"action": "update", "id": "R-002", "fields": {"依赖": ""}}),
                &ctx,
            )
            .await;
        assert!(!out.is_error, "{}", out.content);
        let out = tool.execute(json!({"action": "list"}), &ctx).await;
        assert!(!out.is_error, "{}", out.content);
        assert!(!out.content.contains("循环依赖"), "{}", out.content);
        std::fs::remove_dir_all(dir).ok();
    }
}
