//! 双模式 Profile 组件:dev(需求/缺陷)与 research(来源/发现)。
//! 组件按当前 profile 决定贡献什么;权限规则是硬门禁的落点。

use std::sync::Arc;

use kanzei_harness::{
    rule, source, AgentDef, AgentMode, Component, Effect, HarnessDraft, ProfileKind, ProfileScope,
    ResolveCtx,
};

use crate::docstore::{DocStore, DECISIONS, DEFECTS, FINDINGS, GOALS, REQUIREMENTS, SOURCES};
use crate::tracker::TrackerTool;

/// 索引注入的预算上限(条数;超出折叠为计数)。
const INDEX_LIMIT: usize = 30;
/// 记忆注入的字符预算:记忆是常驻上下文,超预算必须显式说明丢了多少,不做静默截断。
const MEMORY_CONTEXT_BUDGET: usize = 3000;

pub struct DevProfile;

impl Component for DevProfile {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != ProfileKind::Dev {
            return Ok(());
        }
        draft.tools.insert(
            "goal",
            Arc::new(TrackerTool {
                tool_name: "goal",
                noun: "goal",
                kind: &GOALS,
                requires_refs: None,
            }),
        );
        draft.tools.insert(
            "req",
            Arc::new(TrackerTool {
                tool_name: "req",
                noun: "requirement",
                kind: &REQUIREMENTS,
                requires_refs: None,
            }),
        );
        draft.tools.insert(
            "defect",
            Arc::new(TrackerTool {
                tool_name: "defect",
                noun: "defect",
                kind: &DEFECTS,
                requires_refs: None,
            }),
        );
        // 设计决策沉淀(R-110):讨论定下的方案与取舍像需求/缺陷一样落条目。
        draft.tools.insert(
            "decision",
            Arc::new(TrackerTool {
                tool_name: "decision",
                noun: "decision",
                kind: &DECISIONS,
                requires_refs: None,
            }),
        );

        // Memory 系统(R-104,文件优先分级记忆):主 agent 只有 检索/草稿投递/概览,
        // 写路径(add/update/merge/stale)属 M2 的 memory-manager 子代理。
        draft
            .tools
            .insert("memory_search", Arc::new(crate::memory::MemorySearchTool));
        draft
            .tools
            .insert("memory_note", Arc::new(crate::memory::MemoryNoteTool));
        draft
            .tools
            .insert("memory_stats", Arc::new(crate::memory::MemoryStatsTool));
        for tool in ["memory_search", "memory_note", "memory_stats"] {
            draft.permissions.push(rule(tool, "*", Effect::Allow));
        }
        draft.tools.insert("todowrite", Arc::new(crate::todowrite::TodoWriteTool));

        // 硬 deny:项目文档与记忆文件只能走专用工具(用户手改不受此限——这是模型的门禁)。
        for action in ["write", "edit"] {
            draft
                .permissions
                .push_hard_deny(rule(action, "*.kanzei/project/*", Effect::Deny));
            draft
                .permissions
                .push_hard_deny(rule(action, "*.kanzei/memory/*", Effect::Deny));
        }

        // 长期目标(R-019):活跃目标全文注入——"没有明确任务时推进目标"的信息基础。
        draft.context.insert(
            "dev/goals",
            source("dev/goals", |ctx: &ResolveCtx| {
                let entries = DocStore::open(&ctx.project_root, &GOALS).load().ok()?;
                let active: Vec<&crate::docstore::Entry> =
                    entries.iter().filter(|e| e.status == "active").collect();
                if active.is_empty() {
                    return None;
                }
                let mut out = String::from("<goals>\n");
                for goal in active.iter().take(5) {
                    out.push_str(&format!("{} {}\n", goal.id, goal.title));
                    for (key, value) in &goal.fields {
                        let v: String = value.chars().take(300).collect();
                        out.push_str(&format!("  - {key}: {v}\n"));
                    }
                }
                let paused = entries.iter().filter(|e| e.status == "paused").count();
                if paused > 0 {
                    out.push_str(&format!("(另有 {paused} 个 paused 目标,goal list 可见)\n"));
                }
                out.push_str(
                    "When the user gives no specific task, advance these goals and record \
                     progress via `goal update`. Goals with field `类型: 短期` are \
                     finishable: the moment their acceptance is met, CLOSE them with \
                     `goal update <id> achieved` — never leave a completed short-term \
                     goal active. Goals with `类型: 长期` are standing directions; do \
                     not close them yourself.\n</goals>",
                );
                Some(out)
            }),
        );

        // 开发规范(用户手写,agent 只读遵守;write/edit 对 project 目录本就硬 deny)。
        draft.context.insert(
            "dev/conventions",
            source("dev/conventions", |ctx: &ResolveCtx| {
                let path = ctx.project_root.join(".kanzei/project/conventions.md");
                let text = std::fs::read_to_string(path).ok()?;
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                let capped: String = text.chars().take(3000).collect();
                let truncated = capped.len() < text.len();
                Some(format!(
                    "<conventions>\n{capped}{}\n</conventions>",
                    if truncated {
                        "\n…(规范过长已截断,完整内容 read .kanzei/project/conventions.md)"
                    } else {
                        ""
                    }
                ))
            }),
        );

        // Memory 索引常驻(R-104):只注入 INDEX 行(id+category+title+description),
        // 正文按需 memory_search——description 的质量就是触发器的质量。
        draft.context.insert(
            "dev/memory",
            source("dev/memory", |ctx: &ResolveCtx| {
                let mut lines: Vec<String> = Vec::new();
                // preference = 常驻定调(开发重心、验收口径…),必须全文注入才有约束力;
                // fact/sop 只给索引行,正文按需检索(否则预算爆掉)。
                let mut directives: Vec<String> = Vec::new();
                let mut stores = vec![crate::memory::MemoryStore::project(&ctx.project_root)];
                stores.extend(crate::memory::MemoryStore::global());
                for store in &stores {
                    for (_, e) in store.load_all() {
                        if e.status != "active" {
                            continue;
                        }
                        if e.category == "preference" {
                            let body: String = e.body.chars().take(600).collect();
                            directives.push(format!(
                                "{} {}\n{}",
                                e.id,
                                e.title,
                                body.trim()
                            ));
                        } else {
                            lines.push(format!(
                                "{} [{}/{}] {} — {}",
                                e.id, e.scope, e.category, e.title, e.description
                            ));
                        }
                    }
                }
                // 冷启动(D-127):零条目时也必须留声明,否则模型根本不知道记忆系统存在,
                // 于是永不写入 → 永远零条目 → 注入永远为空,自锁成死环。
                if lines.is_empty() && directives.is_empty() {
                    return Some(
                        "<memory-index>\n(记忆库为空)\nYou have a long-term memory system: \
                         `memory_search` to recall, `memory_note` to record anything reusable \
                         you confirm this run (root causes, environment constraints, user \
                         decisions, dead ends). Recording costs one call and saves future runs \
                         from re-deriving it.\n</memory-index>"
                            .into(),
                    );
                }
                let mut out = String::from("<memory-index>\n");
                if !directives.is_empty() {
                    out.push_str("STANDING DIRECTIVES (obey these; they are the user's own words):\n");
                    let mut budget = MEMORY_CONTEXT_BUDGET;
                    for directive in &directives {
                        let cost = directive.chars().count() + 1;
                        if cost > budget {
                            break;
                        }
                        budget -= cost;
                        out.push_str(directive);
                        out.push_str("\n\n");
                    }
                }
                if !lines.is_empty() {
                    out.push_str("KNOWN FACTS (index only — fetch bodies with `memory_search`):\n");
                }
                let mut budget = MEMORY_CONTEXT_BUDGET;
                let mut shown = 0usize;
                for line in &lines {
                    let cost = line.chars().count() + 1;
                    if cost > budget {
                        break;
                    }
                    budget -= cost;
                    out.push_str(line);
                    out.push('\n');
                    shown += 1;
                }
                if shown < lines.len() {
                    out.push_str(&format!(
                        "(还有 {} 条未列出,memory_search 可检索)\n",
                        lines.len() - shown
                    ));
                }
                out.push_str(
                    "Search a listed fact BEFORE re-deriving it. When you confirm something \
                     reusable this run (root cause, environment constraint, user decision, \
                     dead end), drop it via `memory_note`; the memory manager consolidates \
                     notes later. Facts only — next steps belong in req/defect.\n</memory-index>",
                );
                Some(out)
            }),
        );

        draft.context.insert(
            "dev/decisions",
            source("dev/decisions", |ctx: &ResolveCtx| {
                let entries = DocStore::open(&ctx.project_root, &DECISIONS).load().ok()?;
                let standing: Vec<String> = entries
                    .iter()
                    .filter(|e| e.status == "accepted")
                    .map(|e| format!("{} {}", e.id, e.title))
                    .collect();
                if standing.is_empty() {
                    return None;
                }
                Some(format!(
                    "<decisions>\n{}\nAccepted decisions are standing constraints — do not \
                     re-litigate them; `decision get <id>` for rationale. Record newly agreed \
                     designs/tradeoffs with `decision add` (status draft until the user accepts).\n</decisions>",
                    standing.join("\n")
                ))
            }),
        );

        draft.context.insert(
            "dev/project-docs",
            source("dev/project-docs", |ctx: &ResolveCtx| {
                let def = index_of(ctx, &DEFECTS, "Defects");
                let req = index_of(ctx, &REQUIREMENTS, "Requirements");
                if req.is_none() && def.is_none() {
                    return Some(
                        "<project-docs>\n(empty — record requirements with `req add`, defects with `defect add`)\n</project-docs>".into(),
                    );
                }
                Some(format!(
                    "<project-docs>\n{}{}Use the selected work-priority mode from the run instruction to choose between the requirements and defects queues; when no mode is supplied, use defects-first. Use req/defect tools to read or update; direct writes are denied.\n</project-docs>",
                    def.map(|s| s + "\n").unwrap_or_default(),
                    req.map(|s| s + "\n").unwrap_or_default(),
                ))
            }),
        );

        draft.agents.insert(
            "dev",
            AgentDef {
                name: "dev".into(),
                profile: ProfileScope::Dev,
                model: "primary".into(),
                mode: AgentMode::Primary,
                // 0 = 无轮数上限(用户定调)。
                steps: 0,
                system: "You are the dev agent. Workflow contract: before starting work set the \
                         requirement to doing (`req update`); when you find a bug record it \
                         (`defect add`) before fixing; the moment acceptance is met, mark it \
                         done (`req update <id> done`) — an unmarked finished requirement is a \
                         bug in your process. Before closing a requirement or defect, compare \
                         every acceptance item and cite its exact implementation location. A \
                         claimed capability must have a real caller or consumer; dead commands \
                         and display-only shells do not count. Mark reused behavior explicitly as \
                         existing capability rather than this delivery, and never narrow platform \
                         or scope qualifiers from the original acceptance text. If any item lacks \
                         evidence, keep it active and record the gap. WIP limit: keep at most 2 requirements in doing; \
                         finish and close existing doing items before starting new ones. \
                         Pick work according to the selected work-priority mode appended for this run: \
                         scan the selected first queue top-down, then the other queue only when the \
                         first has no workable item. When no mode is supplied, use defect-first. Priority labels are background info, not the ordering. If NOTHING is workable \
                         (all blocked/waiting on外部), reply in PLAIN TEXT only — no tool \
                         calls, no 'still blocked' journal entries in goals, no empty commits; \
                         a text-only reply is the signal that stops the auto-continue loop. \
                         Long-term goals (`goal` tool) are injected into your context: when \
                         the user's message gives no specific task, do NOT ask what to do — \
                         pick the most relevant active goal, advance its next concrete step, \
                         and record progress with `goal update` (e.g. field 进展). Only ask \
                         when goals conflict or none exist. Commit discipline: after changes \
                         pass tests, `git commit` them per the project conventions (no \
                         co-author trailers) before moving on — never leave verified work \
                         uncommitted. After every commit the tool output lists the files \
                         actually committed: COMPARE it against what you intended; on any \
                         mismatch fix immediately with a follow-up commit before other work. \
                         Tracker files pair up: defects.md changes travel WITH \
                         defects-archive.md in the same commit (likewise requirements) — \
                         never `git checkout` an engine-managed tracker file to shrink a \
                         diff; that destroys archived entries (D-112). Verification cadence: \
                         check git state at milestones only — before starting, once the \
                         change stabilizes, and around the commit — not between every \
                         mechanical step; batch related git queries into one call. Test \
                         selection matches the change surface: frontend-only diffs (ui/) \
                         need node --check plus the smoke scripts, NOT the cargo suite; \
                         `node --check` alone is NEVER sufficient evidence for a frontend \
                         change — it only parses. After touching ui/, inspect what actually \
                         rendered: `ui_dom` on the region you changed, `ui_console` for errors \
                         the page swallowed, `ui_style` when something is invisible or \
                         mis-laid-out. Before editing style.css run `frontend_locate` (the same \
                         class is often defined twice — base rule plus a responsive override) \
                         and after editing run `frontend_check` (a clobbered `@media ... {` \
                         breaks the cascade silently, D-164). \
                         when crates/ changed, run the TARGETED test first and the full \
                         workspace suite ONCE right before committing — never while a \
                         file is still mid-edit, and never re-run a suite that nothing \
                         changed since. Editing files: use \
                         `edit`; if it misses twice it shows the file's real content — align \
                         to that, never rewrite whole files via shell. Memory: BEFORE \
                         exploring a problem the memory index hints at, `memory_search` it; \
                         facts you confirmed that future sessions would otherwise re-derive \
                         (root causes, environment constraints, user decisions, dead ends) \
                         go into `memory_note`; do NOT bury them in req/defect progress \
                         fields — the memory manager consolidates notes into durable entries. \
                         For codebase exploration (finding files, call sites, \
                         usages), prefer the `task` subagent: several task calls in one turn run \
                         in parallel and keep your context clean. But when the defect or \
                         requirement already NAMES the file and function (根因/复现 cites \
                         paths), read those files directly — spawning a subagent to rediscover \
                         a known location wastes a whole exploration pass."
                    .into(),
            },
        );
        draft.agents.insert(
            "dev-pair",
            AgentDef {
                name: "dev-pair".into(),
                profile: ProfileScope::Dev,
                model: "primary".into(),
                mode: AgentMode::Primary,
                steps: 0,
                system: "You are the pair-programming agent working WITH the user in conversation. \
                         Follow the user's direction — their latest message defines the task. \
                         Answer questions directly; do NOT start coding when the user is only asking \
                         or discussing. Before non-trivial changes, state a one-line plan first. \
                         When requirements are ambiguous, ask a short clarifying question instead \
                         of guessing. Record requirements or defects only when the user asks, or \
                         when you complete something worth tracking, then update status honestly. \
                         Goals in context are background, NOT instructions — never auto-advance them. \
                         Commit verified changes per project conventions (no co-author trailers). \
                         For codebase exploration, prefer the read-only task subagent."
                    .into(),
            },
        );
        Ok(())
    }
}

pub struct ResearchProfile;

impl Component for ResearchProfile {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != ProfileKind::Research {
            return Ok(());
        }
        draft.tools.insert(
            "source",
            Arc::new(TrackerTool {
                tool_name: "source",
                noun: "source",
                kind: &SOURCES,
                requires_refs: None,
            }),
        );
        draft.tools.insert(
            "websearch",
            Arc::new(crate::websearch::WebSearchTool),
        );
        draft.tools.insert(
            "finding",
            Arc::new(TrackerTool {
                tool_name: "finding",
                noun: "finding",
                kind: &FINDINGS,
                requires_refs: Some(&SOURCES),
            }),
        );

        // 写权限收窄:仅 .kanzei/research/** 可写(report.md 等自由写作);其余 deny。
        for action in ["write", "edit"] {
            draft.permissions.push(rule(action, "*", Effect::Deny));
            draft
                .permissions
                .push(rule(action, "*.kanzei/research/*", Effect::Allow));
        }
        // 研究模式下 bash 全程 ask(默认即 ask,这里显式声明意图);联网抓取放行(主力工具)。
        draft.permissions.push(rule("bash", "*", Effect::Ask));
        draft.permissions.push(rule("webfetch", "*", Effect::Allow));
        draft.permissions.push(rule("websearch", "*", Effect::Allow));

        draft.context.insert(
            "research/docs",
            source("research/docs", |ctx: &ResolveCtx| {
                let src = index_of(ctx, &SOURCES, "Sources");
                let fnd = index_of(ctx, &FINDINGS, "Findings");
                let memory = std::fs::read_to_string(ctx.project_root.join(".kanzei/research/memory.md"))
                    .ok()
                    .map(|text| text.chars().take(5000).collect::<String>());
                Some(format!(
                    "<research-docs>\n{}{}{}Record sources with `source add` BEFORE citing them; every finding must cite refs. Persist reusable conclusions in .kanzei/research/memory.md and include source IDs next to each claim.\n</research-docs>",
                    src.map(|s| s + "\n").unwrap_or_default(),
                    fnd.map(|s| s + "\n").unwrap_or_default(),
                    memory.map(|text| format!("<memory>\n{text}\n</memory>\n")).unwrap_or_default(),
                ))
            }),
        );

        draft.agents.insert(
            "research",
            AgentDef {
                name: "research".into(),
                profile: ProfileScope::Research,
                model: "primary".into(),
                mode: AgentMode::Primary,
                // 0 = 无轮数上限(用户定调)。
                steps: 0,
                system: "You are the research agent. Record every consulted source \
                         (`source add`) and register conclusions as findings citing those \
                         sources. The final report goes to .kanzei/research/report.md."
                    .into(),
            },
        );
        Ok(())
    }
}

/// 文档索引:非终态条目一行一个,预算封顶。
fn index_of(
    ctx: &ResolveCtx,
    kind: &'static crate::docstore::DocKind,
    label: &str,
) -> Option<String> {
    let store = DocStore::open(&ctx.project_root, kind);
    let entries = store.load().ok()?;
    if entries.is_empty() {
        return None;
    }
    let open: Vec<&crate::docstore::Entry> = entries
        .iter()
        .filter(|e| !kind.terminal.contains(&e.status.as_str()))
        .collect();
    // 已完成的会被移入归档文件,closed 计数要把两处都算上。
    let closed = entries.len() - open.len() + store.load_archive().map_or(0, |a| a.len());
    let mut lines: Vec<String> = open
        .iter()
        .take(INDEX_LIMIT)
        .map(|e| {
            let sev = e
                .severity
                .as_ref()
                .map(|s| format!("/{s}"))
                .unwrap_or_default();
            format!("{} [{}{sev}] {}", e.id, e.status, e.title)
        })
        .collect();
    if open.len() > INDEX_LIMIT {
        lines.push(format!("… +{} more open", open.len() - INDEX_LIMIT));
    }
    Some(format!(
        "{label} ({} open, {closed} closed):\n{}",
        open.len(),
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::DevProfile;
    use kanzei_harness::{
        rule, ConfigComponent, Effect, Harness, KanzeiConfig, ProfileKind, ResolveCtx,
    };
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn dev_system_prompt_enforces_acceptance_evidence_contract() {
        let root = PathBuf::from("C:/kanzei-r085-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness.add(DevProfile).add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let system = &snapshot.select_agent(Some("dev")).unwrap().system;

        for required in [
            "compare every acceptance item",
            "exact implementation location",
            "real caller or consumer",
            "existing capability rather than this delivery",
            "never narrow platform or scope qualifiers",
            "keep it active and record the gap",
        ] {
            assert!(
                system.contains(required),
                "dev system prompt 缺少 R-085 完成判定约束: {required}"
            );
        }
    }

    #[test]
    fn dev_project_document_deny_survives_later_user_rules() {
        let mut config = KanzeiConfig::default();
        config.permissions.rules.push(rule(
            "write",
            "*.kanzei/project/*",
            Effect::Ask,
        ));
        config.permissions.rules.push(rule(
            "write",
            "*.kanzei/project/*",
            Effect::Allow,
        ));
        let root = PathBuf::from("C:/kanzei-d050-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(config),
        };
        let mut harness = Harness::default();
        harness.add(DevProfile).add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();

        assert_eq!(
            snapshot.evaluate("write", r".KANZEI\project\requirements.md"),
            Effect::Deny
        );
    }
}
