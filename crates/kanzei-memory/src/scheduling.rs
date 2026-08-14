//! R-203:取活调度链副本——`workable_titles` 及其依赖函数。
//!
//! 2026-08-16 从 kanzei-tools/src/tracker.rs **逐字复制**(行为零变更),起因:
//! memory 的 `prompt_hints` 在自主推进轮用取活条目标题做记忆检索键,依赖
//! tracker 的调度链;kanzei-tools 拆出 kanzei-memory 后 memory 不能再反向依赖
//! tools(R-203 破循环)。原版仍保留在 tracker.rs,本副本只服务 kanzei-memory。
//!
//! **两份必须保持同源演进**:改本文件必须同步 tracker.rs 对应函数,反之亦然;
//! R-204 把取活调度抽成独立可审计模块时在此统一,消除双份。

use std::collections::{BTreeMap, BTreeSet};

use kanzei_harness::ToolCtx;

use crate::docstore::{DocKind, DocStore, Entry, DEFECTS, REQUIREMENTS};

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
    Ok(schedule_for_display_with_states(entries, &states))
}

/// 已由调用方读取完整文档快照时,复用同一份依赖状态,避免 req/defect 各自重新扫盘。
pub fn schedule_for_display_with_states(
    entries: &[Entry],
    states: &DependencyStates,
) -> Vec<ScheduledEntry> {
    let scheduled = schedule_entries(entries, states);
    scheduled
        .into_iter()
        .map(|(entry, block_reasons)| ScheduledEntry {
            entry: entry.clone(),
            block_reasons,
        })
        .collect()
}

/// 当前可推进条目的「ID 标题」,按调度顺序取前 limit 条(阻塞的跳过)。
///
/// 用途:自主推进轮的记忆召回查询键。自动轮的 prompt 是固定模板,拿它去检索
/// 等于每轮都用同一个常量查询——2026-08-12 实测,224 轮召回里 161 轮是自动轮,
/// 反复注入同一批条目(M-006 被注入 101 次只被拉取 18 次),采纳率 22.5%,
/// 而用户真实提问轮是 46.5%。取活条目的标题才是这一轮真正在做的事。
pub fn workable_titles(project_root: &std::path::Path, limit: usize) -> Vec<String> {
    let ctx = ToolCtx::new(project_root.to_path_buf(), project_root.to_path_buf());
    let mut out = Vec::new();
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let Ok(entries) = DocStore::open(project_root, kind).load() else {
            continue;
        };
        let Ok(scheduled) = schedule_for_display(&ctx, kind, &entries) else {
            continue;
        };
        for item in scheduled {
            if out.len() >= limit {
                return out;
            }
            if kind.terminal.contains(&item.entry.status.as_str()) {
                continue;
            }
            // D-332:非法 lifecycle(未知/畸形状态)不参与取活候选——调度器对
            // 控制面脏数据 fail-closed,不让污染条目混进可推进标题。
            if !item.entry.status.is_empty() && !kind.statuses.contains(&item.entry.status.as_str())
            {
                continue;
            }
            if !item.block_reasons.is_empty() {
                continue;
            }
            out.push(format!("{} {}", item.entry.id, item.entry.title));
        }
    }
    out
}

#[derive(Default)]
pub struct DependencyStates {
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

pub fn dependency_states_from_documents(
    requirements: (&[Entry], &[Entry]),
    defects: (&[Entry], &[Entry]),
) -> DependencyStates {
    let mut states = DependencyStates::default();
    for (kind, (active, archived)) in [(&REQUIREMENTS, requirements), (&DEFECTS, defects)] {
        for entry in active.iter().chain(archived.iter()) {
            let deps: Vec<String> = entry
                .fields
                .iter()
                .filter(|(key, _)| is_dependency_key(key))
                .flat_map(|(_, value)| tracker_ids(value))
                .collect();
            states.terminal.insert(
                entry.id.clone(),
                kind.terminal.contains(&entry.status.as_str()),
            );
            if !deps.is_empty() {
                states.deps.insert(entry.id.clone(), deps);
            }
        }
    }
    states
}

fn dependency_states(
    ctx: &ToolCtx,
    current_kind: &DocKind,
    current_entries: &[Entry],
) -> Result<DependencyStates, String> {
    let mut documents: [Vec<Entry>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
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
        let offset = if kind.rel_path == REQUIREMENTS.rel_path {
            0
        } else {
            2
        };
        documents[offset] = active;
        documents[offset + 1] = archived;
    }
    Ok(dependency_states_from_documents(
        (&documents[0], &documents[1]),
        (&documents[2], &documents[3]),
    ))
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

/// 单条目的阻塞理由:「阻塞」字段 + 未完成「依赖」+ 阶段门槛 + 循环依赖。
pub(crate) fn block_reasons(entry: &Entry, states: &DependencyStates) -> Vec<String> {
    let mut reasons = Vec::new();
    // 环上的条目永远等不到依赖完成。只报"未完成依赖"会让 agent 一轮轮空等一个
    // 不可能到来的前置,所以直接点出环并要求断边(D-163)。
    let cycle = states.cycle_from(&entry.id);
    for (key, value) in &entry.fields {
        if is_blocker_key(key) && is_present_blocker(value) {
            reasons.push(format!("阻塞字段: {}", value.trim()));
        }
        if is_dependency_key(key) && cycle.is_none() {
            // R-185:只有「依赖」是阻塞依赖(调度跳过)。「前置」不在此列——
            // 它是可并行但需在协作上下文显式说明的关系,见 is_prerequisite_key。
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
