//! 取活调度域(R-204 从 tracker.rs 拆出):req/defect 的阻塞判定、稳定后置排序、
//! 可推进标题、backlog 判定、耦合证伪与派发留痕。供 auto_run/CLI/docs/memory
//! 四方统一消费;纯函数优先,唯一读盘点 `dependency_states` 经 `ToolCtx`。

use std::collections::{BTreeMap, BTreeSet};

use kanzei_harness::{auto_run::BacklogStatus, ToolCtx};

use crate::docstore::{DocKind, DocStore, Entry, DEFECTS, REQUIREMENTS};

type DependencyMap = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone)]
pub struct ScheduledEntry {
    pub entry: Entry,
    pub block_reasons: Vec<String>,
}

/// R-184 批5(收活格5):合并成功后,把线的交付回写**主根** tracker。
///
/// 追加「进展」与仓库 provenance 锚点、不改状态、不改标题——收活回写是事实记录,
/// 条目该不该 done/open 仍由取活判定负责,这里不越权。走与 `TrackerTool::execute` 相同
/// 的跨进程锁与完整性门禁(load … find … save 整段罩在锁里,两个 worktree
/// 各自登记撞 D-267 的教训),所以绝不绕过 docstore 直接改文件。
///
/// 返回更新后的条目,让调用方能复核回写是否落在预期的行上。
pub fn append_progress(
    project_root: &std::path::Path,
    kind: &'static DocKind,
    id: &str,
    note: &str,
) -> Result<Entry, String> {
    let store = DocStore::open(project_root, kind);
    let _lock = store
        .lock()
        .map_err(|e| format!("cannot lock {} for writing: {e}", store.path.display()))?;
    let mut entries = store
        .load()
        .map_err(|e| format!("cannot read {}: {e}", store.path.display()))?;
    let issues = store.integrity_issues(&entries);
    if !issues.is_empty() {
        return Err(format!(
            "REFUSING to write {}: tracker integrity is broken.\n{}",
            kind.rel_path,
            issues.join("\n")
        ));
    }
    let Some(pos) = entries.iter().position(|e| e.id == id) else {
        return Err(unknown_id(id, &entries));
    };
    let entry = &mut entries[pos];
    let now = crate::memory::today();
    let line = format!("{now} 收活回写: {note}");
    match entry.fields.iter_mut().find(|(k, _)| k == "进展") {
        Some((_, slot)) => {
            slot.push('\n');
            slot.push_str(&line);
        }
        None => entry.fields.push(("进展".into(), line)),
    }
    for (key, value) in crate::work::progress_anchor_fields(project_root) {
        match entry
            .fields
            .iter_mut()
            .find(|(candidate, _)| *candidate == key)
        {
            Some((_, slot)) => *slot = value,
            None => entry.fields.push((key, value)),
        }
    }
    store
        .save(&entries)
        .map_err(|e| format!("cannot write {}: {e}", store.path.display()))?;
    Ok(entries[pos].clone())
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

/// R-169:自主推进(鞭挞)的 backlog 判定——桌面端与 CLI 共用同一实现
/// (D-229 类「能力只在桌面端」的架构债消除)。活动条目里存在可推进项 →
/// Workable;无活动条目 → Empty;全部阻塞 → AllBlocked。
/// block_reasons 与 docs_snapshot 的 `blocked` 字段同源(schedule_for_display)。
pub fn backlog_status(project_root: &std::path::Path) -> BacklogStatus {
    // R-141:调用方给的就是主根,不再从它二次发现(worktree 线传下来的主根
    // 与代码树不同,发现一次就会拐回分支副本)。
    let ctx = ToolCtx::new(project_root.to_path_buf(), project_root.to_path_buf());
    let mut active = 0usize;
    let mut workable = false;
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let entries = match DocStore::open(project_root, kind).load() {
            Ok(entries) => entries,
            // 自动推进宁可继续等下一轮，也不能把暂时的读取故障伪装成“已清空”。
            Err(_) => return BacklogStatus::Unknown,
        };
        let scheduled = match schedule_for_display(&ctx, kind, &entries) {
            Ok(scheduled) => scheduled,
            Err(_) => return BacklogStatus::Unknown,
        };
        for item in scheduled {
            if kind.terminal.contains(&item.entry.status.as_str()) {
                continue;
            }
            // D-332:非法 lifecycle 不算活动条目——它已被隔离为 integrity 错误,
            // 不能计入「active」,否则 backlog 判定会把它误算成可推进项。
            if !item.entry.status.is_empty() && !kind.statuses.contains(&item.entry.status.as_str())
            {
                continue;
            }
            active += 1;
            if item.block_reasons.is_empty() {
                workable = true;
            }
        }
    }
    if workable {
        BacklogStatus::Workable
    } else if active == 0 {
        BacklogStatus::Empty
    } else {
        BacklogStatus::AllBlocked
    }
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

pub(crate) fn dependency_states(
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

/// 反向依赖图(R-111 验收②「条目详情含正反向链接」):id → 依赖它的条目 id 列表。
/// 与 dependency_states 共用同一份「依赖:」字段解析(依赖=阻塞关系,refs 不在此列),
/// 供桌面端 docs_snapshot 输出反向链接与文档页依赖视图使用。
/// 返回 (正向图 id→deps, 反向图 id→dependents)。
pub fn dependents_map(
    ctx: &ToolCtx,
    current_kind: &DocKind,
    current_entries: &[Entry],
) -> Result<(DependencyMap, DependencyMap), String> {
    let states = dependency_states(ctx, current_kind, current_entries)?;
    Ok(dependents_map_with_states(&states))
}

/// 从已缓存的依赖状态生成正向/反向链接,不再触发任何文件读取。
pub fn dependents_map_with_states(states: &DependencyStates) -> (DependencyMap, DependencyMap) {
    let mut dependents: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (from, deps) in &states.deps {
        for dep in deps {
            dependents
                .entry(dep.clone())
                .or_default()
                .push(from.clone());
        }
    }
    let mut deps_map: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (from, deps) in &states.deps {
        deps_map.insert(from.clone(), deps.clone());
    }
    for list in dependents.values_mut().chain(deps_map.values_mut()) {
        list.sort();
        list.dedup();
    }
    (deps_map, dependents)
}

pub(crate) fn schedule_entries<'a>(
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
/// pub(crate):work.rs 的 R-185 测试断言「前置」不阻塞、依赖照常阻塞。
pub(crate) fn block_reasons(entry: &Entry, states: &DependencyStates) -> Vec<String> {
    let mut reasons = Vec::new();
    // 环上的条目永远等不到依赖完成。只报"未完成依赖"会让 agent 一轮轮空等一个
    // 不可能到来的前置,所以直接点出环并要求断边(D-163)。
    let cycle = states.cycle_from(&entry.id);
    for (key, value) in &entry.fields {
        if is_blocker_key(key) && is_present_blocker(value) {
            reasons.push(format!("阻塞字段: {}", value.trim()));
        }
        // D-434:停车与阻塞语义分离。两者都让条目不可执行、不占 WIP 槽,但
        // 「阻塞」= 外部前提未满足(有具名解除人),复核时该逐条查前提是否还成立;
        // 「停车」= 显式让出单 WIP 槽的调度决定,前提本来就不存在,复核阻塞的
        // 扫荡不该碰它。混用同一个字段的代价实测过:一轮把停车当「失效自阻塞」
        // 清掉,下一轮四个条目同时可执行撞 wip_violation,取活直接停摆。
        if is_park_key(key) && is_present_blocker(value) {
            reasons.push(format!("停车: {}", value.trim()));
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

/// 全部条目被判阻塞时置顶的横幅。措辞是刻意的:先要求复核既有阻塞(多数是自记的),
/// 复核后仍全阻塞才允许升级给用户,并且必须点名缺哪个决策——"无可执行条目"不是合格收尾。
pub(crate) fn deadlock_banner(total: usize, noun: &str) -> String {
    format!(
        "[调度死锁] {total} 条{noun}全部带阻塞标记,可执行数为 0。这是队列异常,不是没活干。\n\
         1. 先逐条复核阻塞是否仍成立:依赖已归档、条件已满足、方案其实早已确认的,\
         清空该条的「阻塞」字段再取活——自己历轮写下的「需先确认方案」不算外部阻塞。\n\
         2. 复核后仍全阻塞,才回复用户,并逐条点名缺的是哪一个具体决策。\n\
         注意:带「停车」字段的条目是显式让出 WIP 槽的调度决定,不是待复核的阻塞——\
         第 1 步的扫荡不要清它,恢复由用户或停车的那条线自己做(D-434)。\n\
         禁止以「没有可执行条目」「本轮停止」之类的纯文本收尾。"
    )
}

pub(crate) fn structured_entry(
    entry: &Entry,
    reasons: &[String],
    archived: bool,
) -> serde_json::Value {
    let priority = entry
        .fields
        .iter()
        .find(|(key, _)| key == "优先级" || key.eq_ignore_ascii_case("priority"))
        .map(|(_, value)| value.clone());
    let fields: Vec<serde_json::Value> = entry
        .fields
        .iter()
        .map(|(name, value)| serde_json::json!({"name": name, "value": value}))
        .collect();
    serde_json::json!({
        "id": entry.id,
        "title": entry.title,
        "lifecycle_status": entry.status,
        "archived": archived,
        "severity": entry.severity,
        "priority": priority,
        "blocked": !reasons.is_empty(),
        "block_reasons": reasons,
        "fields": fields,
    })
}

fn is_blocker_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    key.contains("阻塞") || matches!(lower.as_str(), "blocked" | "blocker" | "blocking")
}

/// D-434:显式停车。与 [`is_blocker_key`] 严格分开——`is_blocker_key` 走
/// `contains("阻塞")`,而停车字段名不含「阻塞」,两者不会互相命中。
pub(crate) fn is_park_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    key.trim() == "停车" || matches!(lower.as_str(), "parked" | "park")
}

/// D-434:条目是否被显式停车。停车理由原样返回,供裁决面与协作上下文区分展示。
pub(crate) fn park_reason(entry: &Entry) -> Option<String> {
    entry
        .fields
        .iter()
        .find(|(key, value)| is_park_key(key) && is_present_blocker(value))
        .map(|(_, value)| value.trim().to_string())
}

fn is_dependency_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    key.trim() == "依赖" || matches!(lower.as_str(), "dependency" | "dependencies" | "depends_on")
}

/// R-185:非阻塞前置。「依赖」= 阻塞依赖(调度器必须跳过),「前置」= 可并行但要在
/// 协作上下文里对另一条线显式说明。两者语义分离后,「前置」不再需要用
/// 「前置(不写进依赖字段,按 D-239 教训)」这种注释绕过字段缺陷(R-177/R-182 的
/// 既有注释即该惯例的实证)。调度器只对「依赖」跳过,「前置」不进阻塞判定。
pub(crate) fn is_prerequisite_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    key.trim() == "前置" || matches!(lower.as_str(), "prerequisite" | "prereq" | "before")
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
/// pub(crate):work.rs 解析「前置」字段时复用(R-185)。
pub(crate) fn tracker_ids(value: &str) -> Vec<String> {
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

/// R-185 验收①:同批派发前的**耦合证伪信号**——两条目能否同时并行开,从推断
/// 变成可机械计算的判定。当前信号(按成本排序):
/// 1. **refs 互指**:A.refs 含 B 或 B.refs 含 A → 强耦合(语义上互相依赖)。
/// 2. **文件/路径面求交**:正文(含全部字段值)中点名的路径(如 `crates/kanzei-tools/
///    src/shell.rs`、`docs/design/xxx.md`)有重叠 → 文本层可能撞车。
///
/// 返回 `(是否耦合, 留痕说明)`。纯函数,不读盘——调用方传 Entry 即可。
///
/// 边界(R-185):这是**文本层**信号,不承诺语义撞车检测(R-182 已明确不做事后
/// 检测);信号用来收窄和提醒,最终判定仍可由人拍板,但拍板必须留痕(验收③)。
pub fn coupling_signals(a: &Entry, b: &Entry) -> (bool, Vec<String>) {
    let mut reasons = Vec::new();
    // 信号 1:refs 互指——A.refs 含 B.id 或 B.refs 含 A.id
    let a_refs = a.refs();
    let b_refs = b.refs();
    for id in &a_refs {
        if *id == b.id {
            reasons.push(format!("refs 互指: {} 引用 {}", a.id, id));
        }
    }
    for id in &b_refs {
        if *id == a.id {
            reasons.push(format!("refs 互指: {} 引用 {}", b.id, id));
        }
    }
    // 信号 2:文件/路径面求交(正文所有字段值里提取 crates/ 与 docs/ 路径)
    let a_paths = entry_paths(a);
    let b_paths = entry_paths(b);
    let overlap: Vec<&String> = a_paths.iter().filter(|p| b_paths.contains(*p)).collect();
    for p in overlap {
        reasons.push(format!("路径重叠: {p}"));
    }
    reasons.sort();
    reasons.dedup();
    (!reasons.is_empty(), reasons)
}

/// R-185 验收③④:同批派发的**判定留痕 + 可执行处置**。对两条目跑 coupling_signals,
/// 输出一段自包含的留痕文本(「当时凭什么判定这两条能否并行」),耦合时给出可执行的
/// 处置(串行化/合并/指定先后),而不是一句警告。留痕文本可直接进协作上下文/
/// 派发记录,合并后若真出语义问题能回查当初的判据。
pub fn dispatch_verdict(a: &Entry, b: &Entry) -> String {
    let (coupled, reasons) = coupling_signals(a, b);
    let date = crate::memory::today();
    if !coupled {
        format!(
            "[dispatch {date}] {a_id} ∥ {b_id} 可并行:耦合证伪通过,无 refs 互指、无路径重叠。\
             留痕判据:{}",
            if reasons.is_empty() {
                "refs 不相指且路径面不相交".to_string()
            } else {
                reasons.join("; ")
            },
            a_id = a.id,
            b_id = b.id,
        )
    } else {
        format!(
            "[dispatch {date}] {a_id} 与 {b_id} 判定为耦合,不得同批并行。信号:{}。\
             处置(选一,拍板必须留痕):\
             ①串行化——按文件顺序先后做,后做的基于前做的结果;\
             ②合并成一条——两条并成同一条需求/缺陷由同一线完成;\
             ③指定先后——明确谁先落地,后落地者按前者的新接口重新适配。",
            reasons.join("; "),
            a_id = a.id,
            b_id = b.id,
        )
    }
}

/// 从条目正文(标题 + 全部字段值)提取代码/文档路径(crates/ 与 docs/ 开头的 token),
/// 供 coupling_signals 的路径面求交使用。
fn entry_paths(entry: &Entry) -> Vec<String> {
    let mut text = entry.title.clone();
    for (_, value) in &entry.fields {
        text.push('\n');
        text.push_str(value);
    }
    // 匹配 crates/<...>/ 与 docs/<...> 形式的路径 token(到空白/标点为止)。
    let mut paths = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("crates/").or_else(|| rest.find("docs/")) {
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| {
                c.is_whitespace()
                    || matches!(c, '，' | '。' | '；' | '、' | ')' | '(' | '"' | '\'' | '`')
            })
            .unwrap_or(tail.len());
        let path = tail[..end].trim_end_matches([',', '.', ':', ';']);
        if !path.is_empty() && !paths.contains(&path.to_string()) {
            paths.push(path.to_string());
        }
        rest = &tail[end.max(1)..];
    }
    paths
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
