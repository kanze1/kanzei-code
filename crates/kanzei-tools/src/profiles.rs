//! 双模式 Profile 组件:dev(需求/缺陷)与 research(来源/发现)。
//! 组件按当前 profile 决定贡献什么;权限规则是硬门禁的落点。

use std::sync::Arc;

use kanzei_harness::{
    rule, source, AgentDef, AgentMode, Component, Effect, HarnessDraft, ProfileKind, ProfileScope,
    ResolveCtx,
};

use crate::docstore::{DocStore, DEFECTS, FINDINGS, GOALS, REQUIREMENTS, SOURCES};
use crate::tracker::TrackerTool;

/// 索引注入的预算上限(条数;超出折叠为计数)。
const INDEX_LIMIT: usize = 30;

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
        draft.tools.insert("todowrite", Arc::new(crate::todowrite::TodoWriteTool));

        // 硬 deny:项目文档只能走专用工具(用户手改不受此限——这是模型的门禁)。
        for action in ["write", "edit"] {
            draft
                .permissions
                .push(rule(action, "*.kanzei/project/*", Effect::Deny));
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
                    "<project-docs>\n{}{}Defects are the first development queue; inspect and resolve them before starting new requirements. Use req/defect tools to read or update; direct writes are denied.\n</project-docs>",
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
                         bug in your process. WIP limit: keep at most 2 requirements in doing; \
                         finish and close existing doing items before starting new ones. \
                         Pick work defect-first: inspect and resolve the first workable open \
                         defect before selecting a requirement. After defects are clear, pick \
                         work TOP-DOWN from the requirements list: the list order IS the user's \
                         intent (R-054). Priority labels are background info, not the ordering. If NOTHING is workable \
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
                         uncommitted. For codebase exploration (finding files, call sites, \
                         usages), prefer the `task` subagent: several task calls in one turn run \
                         in parallel and keep your context clean."
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
