//! 双模式 Profile 组件:dev(需求/缺陷)与 research(来源/发现)。
//! 组件按当前 profile 决定贡献什么;权限规则是硬门禁的落点。

use std::sync::Arc;

use kanzei_harness::{
    rule, source, AgentDef, AgentMode, Component, Effect, HarnessDraft, ProfileKind, ProfileScope,
    ResolveCtx,
};

use crate::docstore::{DocStore, DECISIONS, DEFECTS, FINDINGS, GOALS, REQUIREMENTS, SOURCES};
use crate::tracker::TrackerTool;

/// dev agent 的前端自查段。**不写进 dev 的基础提示词**:这段点名的 5 个工具
/// (ui_dom / ui_console / ui_style / frontend_locate / frontend_check)只由桌面端的
/// FrontendToolsComponent 注册,CLI 侧根本不存在。提示词指向不可达的能力正是 D-173
/// 的失效模式——模型试完失败就转去找旁路,而 resolve 末尾的覆盖校验只查 deny 声明的
/// required_tool,管不到提示词点名的工具。装配方注册了这些工具才 append。
pub fn frontend_inspection_guidance() -> &'static str {
    "After touching ui/, inspect what actually rendered: `ui_dom` on the region you \
     changed, `ui_console` for errors the page swallowed, `ui_style` when something is \
     invisible or mis-laid-out. Before editing style.css run `frontend_locate` (the same \
     class is often defined twice — base rule plus a responsive override) and after \
     editing run `frontend_check` (a clobbered `@media ... {` breaks the cascade \
     silently, D-164)."
}

/// 提示词里被反引号点名的工具候选:取每段反引号内的第一个词。
///
/// D-190 抽出 `frontend_inspection_guidance()` 只是把那段挪了个地方,组件注册与提示词
/// 追加仍是两处各写各的;真正让它们同进同退的是以此为基础的两条测试(本文件的
/// CLI 装配线、kanzei-app 的桌面装配线)。提取规则抽成函数是为了两侧共用一套,
/// 不让它们各写一份慢慢漂开。
///
/// 只认 ASCII 标识符形态,所以 `类型: 短期`、`@media ... {`、`{tool}` 这类反引号内容
/// 自然被滤掉;`req update <id> done` 取首词 `req`。
pub fn prompt_tool_mentions(prompt: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = prompt;
    while let Some((_, after)) = rest.split_once('`') {
        let Some((span, tail)) = after.split_once('`') else {
            break;
        };
        rest = tail;
        let Some(first) = span.split_whitespace().next() else {
            continue;
        };
        let identifier = first.starts_with(|c: char| c.is_ascii_alphabetic())
            && first.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if identifier && !out.iter().any(|seen| seen == first) {
            out.push(first.to_string());
        }
    }
    out
}

/// 索引注入的预算上限(条数;超出折叠为计数)。
const INDEX_LIMIT: usize = 30;
/// 记忆注入的字符预算:记忆是常驻上下文,超预算必须显式说明丢了多少,不做静默截断。
// dev/memory 注入预算移入 memory 模块与 prompt_hints 共用(D-216:同一口径)。
use crate::memory::MEMORY_CONTEXT_BUDGET;

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
        draft
            .tools
            .insert("todowrite", Arc::new(crate::todowrite::TodoWriteTool));

        // 测试记录专用写通道(R-080):tests.md 是托管文件,bash 会回滚、write/edit
        // 硬 deny,agent 没有别的合法路径;写仍是写操作,按默认 ask 逐次询问。
        draft
            .tools
            .insert("test_record", Arc::new(crate::test_record::TestRecordTool));

        // 架构索引的专用写通道(D-173):原先这个资源族只有硬 deny 没有工具,
        // 合法路径不可达,模型就去找 shell 旁路。读/校验放行,写仍逐次询问。
        draft.tools.insert(
            "architecture",
            Arc::new(crate::architecture::ArchitectureTool),
        );
        for read_only in ["get", "check", "regenerate"] {
            draft
                .permissions
                .push(rule("architecture", read_only, Effect::Allow));
        }

        // 开发规范 conventions.md 的专用写通道(D-235):与 D-173 同根因——write/edit
        // 硬 deny 而合法路径不可达,模型就去找 shell 旁路。get(读全文+hash)放行,
        // patch(逐字替换)写操作仍逐次询问,和 architecture update 同一保守口径。
        draft
            .tools
            .insert("conventions", Arc::new(crate::conventions::ConventionsTool));
        draft
            .permissions
            .push(rule("conventions", "get", Effect::Allow));

        // 硬 deny:项目文档与记忆文件只能走专用工具(用户手改不受此限——这是模型的门禁)。
        // 每条 deny 都必须挂上它的合法替代路径:resolve 会校验那个工具真的注册了,
        // 拒绝理由也由此推导,不会再固定说一句不存在的 "use the dedicated tool"。
        // 顺序=特化在前、兜底在后;managed_for 取首个命中。
        for action in ["write", "edit"] {
            for (resource, tool, note) in [
                (
                    "*.kanzei/project/architecture/*",
                    Some("architecture"),
                    "架构索引:链接与命名由引擎校验",
                ),
                (
                    "*.kanzei/project/requirements*",
                    Some("req"),
                    "需求条目:ID 由引擎分配、状态机受限",
                ),
                (
                    "*.kanzei/project/defects*",
                    Some("defect"),
                    "缺陷条目:ID 由引擎分配、状态机受限",
                ),
                (
                    "*.kanzei/project/goals*",
                    Some("goal"),
                    "长期目标:ID 由引擎分配、状态机受限",
                ),
                (
                    "*.kanzei/project/decisions*",
                    Some("decision"),
                    "设计决策:ID 由引擎分配、状态机受限",
                ),
                (
                    "*.kanzei/project/tests*",
                    Some("test_record"),
                    "测试记录:终态自动归档,由 test_record 追加",
                ),
                (
                    "*.kanzei/project/conventions*",
                    Some("conventions"),
                    "开发规范:patch 逐字替换,唯一命中才写",
                ),
                (
                    "*.kanzei/memory/*",
                    Some("memory_note"),
                    "记忆库:写路径属 memory-manager 子代理,主 agent 只投草稿",
                ),
                // 兜底族:.kanzei/project 下其余文件(conventions.md 等)是用户手写资产,
                // 模型没有任何合法写通道——如实说成"能力未实现",不要编一个工具名。
                ("*.kanzei/project/*", None, "用户手写的项目资产,模型只读"),
            ] {
                draft.permissions.push_managed_hard_deny(
                    rule(action, resource, Effect::Deny),
                    tool,
                    Some(note),
                );
            }
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
        //
        // **全量注入,不设字符预算**(D-201)。原实现只取前 3000 字符,而本仓库的
        // conventions.md 是 151 行 / 14944 字符——只有 16% 送达,截断点正好切在
        // `## 1.2 关闭边界:可用即关闭` 这个标题上。后果可测,而且是同一份文件的
        // 前后对照:§1.25「关闭前逐条对照验收、给精确代码位置」因为**同时也写进了
        // dev 的 system prompt** 而被严格遵守(近期条目的验收证据都很详实);
        // §1.2「不因缺 E2 夹具等验证增强项长期滞留 fixing」只存在于被截断的部分,
        // 于是 11 条 high 缺陷带着**已经发布的修复**卡在 fixing。被投喂的规则被
        // 遵守,被截断的没有——这不是纪律问题,是投递问题。
        //
        // 规范是用户的常驻定调,不是可按预算取舍的参考资料;口径对齐 CLAUDE.md
        // (全量进上下文,不做字符截断)。要控成本请去精简规范本身,而不是让引擎
        // 悄悄替用户决定哪几条不算数。
        draft.context.insert(
            "dev/conventions",
            source("dev/conventions", |ctx: &ResolveCtx| {
                // R-191:通用规则单源进引擎,所有项目默认注入;项目文件只追加项目特有规则。
                // 通用部分永远在(无项目文件的项目也拿到完整约束),项目文件在其后拼接。
                let mut text = String::from(kanzei_harness::DEFAULT_CONVENTIONS);
                let path = ctx.project_root.join(".kanzei/project/conventions.md");
                if let Ok(project_rules) = std::fs::read_to_string(&path) {
                    let project_rules = project_rules.trim();
                    if !project_rules.is_empty() {
                        text.push_str("\n\n<!-- 以下为本项目特有规则(自动追加) -->\n\n");
                        text.push_str(project_rules);
                    }
                }
                Some(format!("<conventions>\n{}\n</conventions>", text.trim()))
            }),
        );

        // Memory 索引常驻(R-104):只注入 INDEX 行(id+category+title+description),
        // 正文按需 memory_search——description 的质量就是触发器的质量。
        draft.context.insert(
            "dev/memory",
            source("dev/memory", |ctx: &ResolveCtx| {
                // preference = 常驻定调(开发重心、验收口径…),必须全文注入才有约束力;
                // fact/sop 只给索引行,正文按需检索(否则预算爆掉)。
                let mut directives: Vec<String> = Vec::new();
                let mut stores = vec![crate::memory::MemoryStore::project(&ctx.project_root)];
                stores.extend(crate::memory::MemoryStore::global());
                for store in &stores {
                    for (_, e) in store.load_all() {
                        if e.status != "active" || e.category != "preference" {
                            continue;
                        }
                        let body: String = e.body.chars().take(600).collect();
                        directives.push(format!("{} {}\n{}", e.id, e.title, body.trim()));
                    }
                }
                // 索引行预算走查与 prompt_hints 共用同一实现(D-216):
                // 两边口径一致,hints 才知道哪些条目已经在这里、不必重复整行。
                let (lines, _, folded) =
                    crate::memory::resident_index(&ctx.project_root, MEMORY_CONTEXT_BUDGET);
                // 冷启动(D-127):零条目时也必须留声明,否则模型根本不知道记忆系统存在,
                // 于是永不写入 → 永远零条目 → 注入永远为空,自锁成死环。
                if lines.is_empty() && folded == 0 && directives.is_empty() {
                    return Some(
                        "<memory-index>\n(记忆库为空)\nYou have a long-term memory system: \
                         `memory_search` to recall, `memory_note` to record what would change \
                         a future agent's ACTION (root causes, environment constraints, user \
                         decisions, dead ends). Recording costs one call and saves future runs \
                         from re-deriving it.\n</memory-index>"
                            .into(),
                    );
                }
                let mut out = String::from("<memory-index>\n");
                if !directives.is_empty() {
                    out.push_str(
                        "STANDING DIRECTIVES (obey these; they are the user's own words):\n",
                    );
                    let mut budget = MEMORY_CONTEXT_BUDGET;
                    let mut directives_shown = 0usize;
                    for directive in &directives {
                        let cost = directive.chars().count() + 1;
                        // continue 而非 break:放不下的跳过、继续填后面的。break 会让
                        // 一条超长条目把它之后**全部**更短的条目一起挡在外面。
                        if cost > budget {
                            continue;
                        }
                        budget -= cost;
                        directives_shown += 1;
                        out.push_str(directive);
                        out.push_str("\n\n");
                    }
                    // D-196:被丢掉的必须报数。预算注释写的是"超预算必须显式说明丢了
                    // 多少,不做静默截断",而这半边一直没有——改成 continue 之后更要紧:
                    // 丢的不再是尾巴而是中间挑着丢,丢掉的又是标着"obey these; they are
                    // the user's own words"的用户原话,模型完全看不出少了东西。
                    if directives_shown < directives.len() {
                        out.push_str(&format!(
                            "(另有 {} 条常驻指令因预算未列出,memory_search category=preference 可取全文)\n\n",
                            directives.len() - directives_shown
                        ));
                    }
                }
                if !lines.is_empty() || folded > 0 {
                    out.push_str("KNOWN FACTS (index only — fetch bodies with `memory_search`):\n");
                }
                for line in &lines {
                    out.push_str(line);
                    out.push('\n');
                }
                if folded > 0 {
                    out.push_str(&format!("(还有 {folded} 条未列出,memory_search 可检索)\n"));
                }
                out.push_str(
                    "Search a listed fact BEFORE re-deriving it. Record via `memory_note` \
                     ONLY what would change a future agent's action (root cause, environment \
                     constraint, user decision, dead end); narration that changes no future \
                     action is noise — skip it. The memory manager consolidates notes later. \
                     Next steps belong in req/defect, not memory.\n</memory-index>",
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
                         evidence, keep it active and record the gap. \
                         WIP limit: ONE executable item at a time across BOTH queues — a \
                         requirement in doing and a defect in fixing share the SAME single \
                         slot; finish it, park it, or close it before picking up another. An \
                         item that carries a valid blocking field (an external blocker with a \
                         named unblocker) or that the user explicitly parked is NOT executable \
                         and does NOT consume the slot — never refuse to start new work on the \
                         grounds that a blocked item is sitting in doing or fixing. Hoarding \
                         backstop: when doing plus fixing, blocked ones included, exceeds 4, \
                         open nothing new until the backlog is drained. \
                         Batch protocol: YOU decide how many batches an item takes, from its \
                         actual work, with a hard ceiling of 10 — the 复杂度 field does NOT \
                         dictate the count. Most items are one batch and need no declaration. \
                         When an item genuinely needs splitting, your FIRST landing action is \
                         to write the batch table into the entry as `批次: 0/N` (N chosen by \
                         you, N <= 10). After each finished batch update it to `批次: k/N` — \
                         the sidebar cells are the only place your progress is visible from \
                         outside, and an unfilled cell reads as no progress. Every batch \
                         commit's subject must carry the marker `<ID> B<k>` (for example \
                         `R-161 B3`): the engine derives real progress from commit subjects, \
                         so an unmarked commit does not count toward it. At close time the \
                         batches must be full — if you over-estimated, set the total to the \
                         real number (`批次: 5/5`) instead of leaving empty cells; if work \
                         remains, finish it. If ten batches are not enough, the item is too \
                         big: close what is genuinely done and open a follow-up item for the \
                         rest. \
                         Registration contract (R-191, enforced by the engine): a NEW \
                         requirement (`req add`) MUST carry 复杂度 (小|中|大), priority \
                         (P0|P1|P2|P3) and 标签 from the controlled vocabulary \
                         (核心|后端|前端|模型|发布|流程); a NEW defect (`defect add`) MUST \
                         carry severity (high|medium|low), priority and 标签. The tool \
                         rejects the call otherwise and tells you what to fill — never \
                         retry with an empty field. State the 来源 of every new item \
                         (user message / feedback / self-found) so it stays traceable. \
                         If the item genuinely needs batching, write `批次: 0/N` in the \
                         same call.
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
                         change — it only parses. \
                         When crates/ changed, run the TARGETED suite (cargo test -p \
                         <changed crate>) before every commit. The FULL workspace suite \
                         (cargo test --workspace) runs ONCE before CLOSING an item whose \
                         复杂度 is 中 or 大 — items marked 小 close on targeted tests alone, \
                         and an item with no complexity assessed is not exempt: fill the \
                         field in before closing rather than treating unassessed as free. \
                         Never run a full suite while a file is still mid-edit, and never \
                         re-run a suite that nothing changed since. The release gate \
                         (verify.ps1) and CI run their own full suite; that one is not \
                         yours to skip. Editing files: use \
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
        draft
            .tools
            .insert("websearch", Arc::new(crate::websearch::WebSearchTool));
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
        draft
            .permissions
            .push(rule("websearch", "*", Effect::Allow));

        draft.context.insert(
            "research/docs",
            source("research/docs", |ctx: &ResolveCtx| {
                let src = index_of(ctx, &SOURCES, "Sources");
                let fnd = index_of(ctx, &FINDINGS, "Findings");
                let memory = std::fs::read_to_string(ctx.project_root.join(".kanzei/research/memory.md"))
                    .ok()
                    .map(|text| {
                        let capped: String = text.chars().take(5000).collect();
                        let truncated = capped.chars().count() < text.chars().count();
                        format!(
                            "{capped}{}",
                            if truncated {
                                "\n…(来源过长已截断,完整内容 read .kanzei/research/memory.md)"
                            } else {
                                ""
                            }
                        )
                    });
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

pub struct ReadonlyProfile;

impl Component for ReadonlyProfile {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != ProfileKind::Readonly {
            return Ok(());
        }
        // 只读档位(R-102):分析类任务免配权限直接跑。
        // 工具集 = Base 的只读族(read/glob/grep/files/git 只读子命令) + webfetch;
        // 权限强制(read/write/edit 硬 deny、bash 禁用提示替代)在批2 落码,
        // 批1 只建档位概念与 agent,装配必须能解析出这个 profile。
        // 权限强制(批2):write/edit/bash 硬 deny(带替代指引),只读族显式放行。
        for action in ["read", "glob", "grep", "files"] {
            draft.permissions.push(rule(action, "*", Effect::Allow));
        }
        // git 只读子命令放行;状态/差异/日志是分析任务的主干工具。
        for subcommand in ["status", "diff", "log"] {
            draft
                .permissions
                .push(rule("git", subcommand, Effect::Allow));
        }
        // 只读档位下联网抓取放行(分析"外部事实"时的主要只读通道)。
        draft.permissions.push(rule("webfetch", "*", Effect::Allow));
        // 写与命令:硬 deny 且带合法替代指引——硬 deny 只说"不准走这条路",
        // 不说"那该怎么走"就是能力死区,模型会去找旁路(D-173)。
        // 用 ManagedResource 而非裸 push_hard_deny,拒绝理由能点名替代工具。
        for action in ["write", "edit", "bash"] {
            draft.permissions.push_managed_hard_deny(
                rule(action, "*", Effect::Deny),
                None,
                Some("只读档位:write/edit/bash 一律禁止;需要结果请用 read/glob/grep/files/git status|diff|log/webfetch 观察,确需修改则告诉用户手动执行"),
            );
        }
        // task 子代理天然只读(SubagentBase 快照),无需规则——runner 直接放行。
        draft.agents.insert(
            "readonly",
            AgentDef {
                name: "readonly".into(),
                profile: ProfileScope::All,
                model: "primary".into(),
                mode: AgentMode::Primary,
                steps: 0,
                system: "You are the read-only analysis agent. You may READ, SEARCH and \
                         EXPLORE the repository (read/glob/grep/files/git status/diff/log, \
                         webfetch), but you MUST NOT modify anything: no write, no edit, no \
                         bash. Answer the user's question from what you can observe; if an \
                         answer requires writing or running commands, say exactly what would \
                         need to change and let the user do it."
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

    /// 取 dev 档 system prompt 的公共装配(与上面那条同一条装配线)。
    fn dev_system_prompt(tag: &str) -> String {
        let root = PathBuf::from(format!("C:/kanzei-{tag}-test"));
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness.add(DevProfile).add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        snapshot.select_agent(Some("dev")).unwrap().system.clone()
    }

    /// D-242 / D-219(2026-08-10 用户定调):WIP 单槽 + 批次协议必须在提示词里有真源。
    ///
    /// 反向断言是关键:只断言新文本存在的话,旧口径句子留在原地测试照样绿,
    /// 而模型会同时读到两条互斥规则——这正是 D-242 的失效模式。
    #[test]
    fn dev_system_prompt_enforces_wip_and_batch_contract() {
        let system = dev_system_prompt("d242-wip");

        for required in [
            // ① WIP:需求与缺陷合计只有一个可执行槽,阻塞项不占槽。
            "ONE executable item",
            "share the SAME single slot",
            "does NOT consume the slot",
            "exceeds 4",
            // ② 批次:数量由 agent 自定,上限 10,写法与提交标记有明确规矩。
            "hard ceiling of 10",
            "批次: 0/N",
            "批次: k/N",
            "<ID> B<k>",
            "does not count toward it",
            "the item is too big",
        ] {
            assert!(
                system.contains(required),
                "D-242 规则真源缺失:dev system prompt 里没有 `{required}`。\
                 引擎照着批次门禁罚人,提示词却没教规矩,agent 只能撞门。"
            );
        }

        assert!(
            !system.contains("keep at most 2 requirements"),
            "D-219 旧口径残留:WIP 仍写着「最多 2 个 requirements in doing」,\
             与新的单槽口径互斥,模型会按就近句取其一。"
        );
    }

    /// 2026-08-10 用户定调③:全量测试只服务于中/大条目的收口,不再挂在每次提交上。
    #[test]
    fn dev_system_prompt_gates_full_suite_on_complexity() {
        let system = dev_system_prompt("d242-cadence");

        for required in [
            "cargo test --workspace",
            "before CLOSING",
            "复杂度 is 中 or 大",
            // 不可调降的底线必须同时在场,否则「小条目不跑全量」会被外推成「发版也不用跑」。
            "verify.ps1",
        ] {
            assert!(
                system.contains(required),
                "D-242 规则真源缺失:验证节奏段没有 `{required}`,\
                 提示词与 conventions §1.4 会再次漂开。"
            );
        }

        assert!(
            !system.contains("ONCE right before committing"),
            "旧口径残留:提示词仍要求「每次提交前全量一次」,与 §1.4 的\
             「中/大条目关闭前」直接冲突。"
        );
    }

    /// 三条定调的两份真源必须同口径:conventions.md 全量注入(D-201),dev system
    /// prompt 常驻,任一侧单方面改口,模型就会同时读到两条互斥规则——这正是
    /// D-242/D-128 反复出现的失效模式。
    ///
    /// 只断言三个短 token,不锁整句措辞:规范是用户手写资产,行文随时可改,
    /// 但「1 个槽 / 上限 10 批 / 全量只对中大」这三个判据不能悄悄消失。
    #[test]
    fn conventions_与提示词对三条定调保持同口径() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.kanzei/project/conventions.md");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读不到 {}:{e}", path.display()));

        // 小节 = 从该二级标题起到下一个二级标题为止(`### ` 不会被误当边界)。
        // 标题必须带尾空格再查:文件里 §1.35 排在 §1.3 前面,查 "## 1.3" 会先命中它。
        let section = |heading: &str| -> &str {
            let start = text
                .find(heading)
                .unwrap_or_else(|| panic!("conventions.md 里找不到小节 {heading}"));
            let rest = &text[start + heading.len()..];
            &rest[..rest.find("\n## ").unwrap_or(rest.len())]
        };

        for (heading, token, 定调) in [
            (
                "## 1.1 ",
                "1 个可执行活动项",
                "WIP 单槽(需求+缺陷合计 1 个)",
            ),
            ("## 1.3 ", "上限 10 批", "批数由 agent 自定、上限 10"),
            ("## 1.4 ", "复杂度中/大", "全量测试只服务中/大条目的收口"),
        ] {
            assert!(
                section(heading).contains(token),
                "conventions.md {heading} 缺少「{token}」({定调});\
                 提示词已按新口径写,规范这侧沉默就等于半份真源(D-242)。"
            );
        }
    }

    /// D-195:提示词点名的工具必须在同一条装配线上注册。
    ///
    /// D-190 把前端自查段抽成函数,但组件注册(桌面端 5583 行)与提示词追加(5596 行)
    /// 仍是两处各写各的,没有任何东西保证同进同退——这条测试就是那个机制。它守的是
    /// CLI 装配线:谁把前端段(或任何点名工具的文字)写回 dev 基础提示词,这里立刻红。
    /// 桌面装配线由 kanzei-app 侧的同名测试守另一半。
    #[test]
    fn 提示词点名的工具必须在同一条装配线上注册() {
        use super::prompt_tool_mentions;

        // 反引号里不是工具的词。每条都要说得出理由,不许为了让测试变绿往里塞。
        const NOT_TOOLS: &[&str] = &[
            // shell 命令(`node --check`),不是工具。
            "node",
            // 子代理入口:由 runner 在 SubagentRuntime 就位时 push task_spec,
            // 不进 draft.tools,所以 materialize_tools() 里查不到它。
            "task",
        ];

        for profile in [ProfileKind::Dev, ProfileKind::Research] {
            let root = PathBuf::from("C:/kanzei-d195-test");
            let ctx = ResolveCtx {
                profile,
                cwd: root.clone(),
                project_root: root,
                config: Arc::new(KanzeiConfig::default()),
            };
            // CLI 的装配线,但不加 MarkdownComponent:它读真实 ~/.kanzei,
            // 会让这条测试的结果取决于跑测试的机器上放了什么。
            let mut harness = Harness::default();
            harness
                .add(crate::BaseComponent)
                .add(DevProfile)
                .add(super::ResearchProfile)
                .add(ConfigComponent);
            let snapshot = harness.resolve(&ctx).unwrap();
            let tools: Vec<String> = snapshot
                .materialize_tools()
                .iter()
                .map(|t| t.name().to_string())
                .collect();

            for (name, agent) in snapshot.agents().iter() {
                for mentioned in prompt_tool_mentions(&agent.system) {
                    if NOT_TOOLS.contains(&mentioned.as_str()) {
                        continue;
                    }
                    assert!(
                        tools.contains(&mentioned),
                        "{profile:?} 档的 agent `{name}` 提示词点名了 `{mentioned}`,\
                         但这条装配线没注册它——模型试完失败就会转去找旁路(D-173/D-190)。\
                         已注册: {tools:?}"
                    );
                }
            }
        }
    }

    /// D-201:规范必须全量送达。原实现取前 3000 字符,而真实 conventions.md 有
    /// 14944 字符——「1.2 关闭边界」正好落在截断线之外,于是 11 条 high 缺陷带着
    /// 已发布的修复卡在 fixing。送不到的规则等于不存在,所以这条守的是"不截断",
    /// 不是"截得优雅"。
    #[test]
    fn 开发规范全量注入不做字符截断() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-d201-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        // 尾部这条规则只有全量注入才看得见——它正是被旧上限切掉的那一类。
        let tail = "## 关闭边界:可用即关闭,不因验证增强项长期滞留 fixing";
        let filler = "- 填充行,单纯为了越过旧的 3000 字符上限,内容本身不重要。\n".repeat(200);
        let body = format!("# 开发规范\n\n{filler}\n{tail}\n");
        assert!(body.chars().count() > 3000, "夹具没超过旧上限,测不出截断");
        std::fs::write(root.join(".kanzei/project/conventions.md"), &body).unwrap();

        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness.add(DevProfile).add(ConfigComponent);
        let baseline = harness.resolve(&ctx).unwrap().system_baseline();

        assert!(
            baseline.contains(tail),
            "规范尾部没进上下文——又被截断了。送不到的规则等于不存在。"
        );
        assert!(
            !baseline.contains("规范过长已截断"),
            "不该再出现截断提示:全量注入之后它没有意义"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// R-191:通用规则单源注入——无项目文件时引擎默认模板也全量进上下文,
    /// 有项目文件时通用 + 项目特有拼接,且通用在前。conventions 是 context source,
    /// 断言必须看 system_baseline(dev_system_prompt 的 agent.system 不含 context 注入)。
    #[test]
    fn conventions_注入含引擎默认模板与项目特有规则() {
        let baseline_of = |root: &std::path::Path| {
            let ctx = ResolveCtx {
                profile: ProfileKind::Dev,
                cwd: root.to_path_buf(),
                project_root: root.to_path_buf(),
                config: Arc::new(KanzeiConfig::default()),
            };
            let mut harness = Harness::default();
            harness.add(DevProfile).add(ConfigComponent);
            harness.resolve(&ctx).unwrap().system_baseline()
        };

        // ① 无项目文件:引擎默认模板必须全量注入(新项目零配置也拿到完整约束)。
        let bare = PathBuf::from("C:/kanzei-r191-default-test");
        let baseline = baseline_of(&bare);
        for required in [
            "通用开发规范单源",
            "阻塞:` 字段只留给外部阻塞",
            "批次: k/N",
            "任务级并行",
            "可用即关闭",
        ] {
            assert!(
                baseline.contains(required),
                "引擎默认模板未注入 dev 上下文: {required}"
            );
        }

        // ② 有项目文件:两段拼接,通用在前、项目特有在后。
        let root = std::env::temp_dir().join(format!(
            "kanzei-r191-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let project_only = "## 项目特有规则\n- 只在 kanzei 仓库生效";
        std::fs::write(root.join(".kanzei/project/conventions.md"), project_only).unwrap();
        let baseline = baseline_of(&root);
        assert!(
            baseline.contains(project_only),
            "项目特有规则没进上下文——R-191 拼接丢失项目文件"
        );
        let default_pos = baseline.find("通用开发规范单源");
        let project_pos = baseline.find("项目特有规则");
        assert!(
            default_pos.is_some() && project_pos.is_some() && default_pos < project_pos,
            "拼接顺序错误:通用规则必须在项目特有规则之前"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// R-191:登记硬约束必须在提示词里有真源——引擎会拒缺字段的 add,提示词不教
    /// 规矩 agent 只会撞门。断言关键 token,不锁整句措辞。
    #[test]
    fn dev_system_prompt_teaches_registration_contract() {
        let system = dev_system_prompt("r191-reg");
        for required in [
            "Registration contract",
            "复杂度 (小|中|大)",
            "severity (high|medium|low)",
            "controlled vocabulary",
            "rejects the call otherwise",
            "来源",
        ] {
            assert!(
                system.contains(required),
                "R-191 登记契约缺失:dev system prompt 里没有 `{required}`。\
                 引擎现在会拒缺字段的 add,提示词却没教字段清单。"
            );
        }
    }

    #[test]
    fn prompt_tool_mentions_只取反引号里的标识符首词() {
        let mentions = super::prompt_tool_mentions(
            "use `req update <id> done` and `git commit`, not `node --check`; \
             `类型: 短期` and `@media ... {` and `{tool}` are not tools; `req` repeats",
        );
        assert_eq!(mentions, vec!["req", "git", "node"]);
    }

    /// D-173:硬 deny 与专用工具必须闭合。
    /// 有工具的资源族要点名工具;没工具的要如实说"能力未实现"并堵死 shell 绕行。
    #[test]
    fn 每个硬deny资源族都给出真实可达的下一步() {
        let root = PathBuf::from("C:/kanzei-d173-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevProfile)
            .add(ConfigComponent);
        // resolve 本身就是覆盖校验:声明的 required_tool 没注册会直接 bail。
        let snapshot = harness.resolve(&ctx).unwrap();

        for (path, tool) in [
            (".kanzei/project/requirements.md", "req"),
            (".kanzei/project/defects-archive.md", "defect"),
            (".kanzei/project/goals.md", "goal"),
            (".kanzei/project/architecture/README.md", "architecture"),
            (".kanzei/project/tests.md", "test_record"),
            (".kanzei/project/tests-archive.md", "test_record"),
            (".kanzei/memory/M-001-x.md", "memory_note"),
        ] {
            let normalized = kanzei_harness::permission::normalize_resource(path);
            assert_eq!(
                snapshot.evaluate("write", &normalized),
                Effect::Deny,
                "{path}"
            );
            let hint = snapshot.denial_hint("write", &normalized);
            assert!(
                hint.contains(&format!("`{tool}`")),
                "{path} 的指引没点名 {tool}: {hint}"
            );
        }

        // 没有专用工具的资源族:必须如实说能力未实现,并明确堵死 shell 绕行。
        // conventions.md 自 D-235 起有了专用工具,这里用另一个仍无工具的
        // .kanzei/project 文件(如 notes.md)继续守「不得编造工具名」这条底线。
        let uncovered = kanzei_harness::permission::normalize_resource(".kanzei/project/notes.md");
        assert_eq!(snapshot.evaluate("write", &uncovered), Effect::Deny);
        let hint = snapshot.denial_hint("write", &uncovered);
        assert!(hint.contains("unimplemented capability"), "{hint}");
        assert!(hint.contains("WriteAllText"), "{hint}");
        assert!(
            !hint.contains("use the dedicated tool"),
            "不得编造不存在的工具: {hint}"
        );

        // conventions.md 现在有了合法通道,指引必须点名它,不能再说「没有专用工具」。
        let conventions =
            kanzei_harness::permission::normalize_resource(".kanzei/project/conventions.md");
        assert_eq!(snapshot.evaluate("write", &conventions), Effect::Deny);
        let hint = snapshot.denial_hint("write", &conventions);
        assert!(
            hint.contains("`conventions`"),
            "指引没点名 conventions 工具: {hint}"
        );

        // 架构索引现在有了合法通道,而且读/校验默认放行。
        assert_eq!(snapshot.evaluate("architecture", "get"), Effect::Allow);
        assert_eq!(snapshot.evaluate("architecture", "check"), Effect::Allow);
        assert_eq!(snapshot.evaluate("architecture", "update"), Effect::Ask);

        // conventions 的读动作默认放行、写动作(patch)逐次询问——与 architecture 同口径。
        assert_eq!(snapshot.evaluate("conventions", "get"), Effect::Allow);
        assert_eq!(snapshot.evaluate("conventions", "patch"), Effect::Ask);
    }

    /// 覆盖校验必须真的会炸:声明了不存在的工具就不该装配成功。
    #[test]
    fn 声明了未注册的专用工具时装配直接失败() {
        struct BrokenComponent;
        impl kanzei_harness::Component for BrokenComponent {
            fn contribute(
                &self,
                draft: &mut kanzei_harness::HarnessDraft,
                _ctx: &ResolveCtx,
            ) -> anyhow::Result<()> {
                draft.permissions.push_managed_hard_deny(
                    rule("write", "*.kanzei/ledger/*", Effect::Deny),
                    Some("ledger"),
                    None,
                );
                Ok(())
            }
        }
        let root = PathBuf::from("C:/kanzei-d173-gap");
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness.add(BrokenComponent);
        let error = match harness.resolve(&ctx) {
            Ok(_) => panic!("覆盖校验没生效:声明了不存在的工具却装配成功"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("ledger"), "{error}");
    }

    #[test]
    fn dev_project_document_deny_survives_later_user_rules() {
        let mut config = KanzeiConfig::default();
        config
            .permissions
            .rules
            .push(rule("write", "*.kanzei/project/*", Effect::Ask));
        config
            .permissions
            .rules
            .push(rule("write", "*.kanzei/project/*", Effect::Allow));
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

    /// R-102 批1:readonly 档位能装配出只读 agent,且权限快照可见。
    #[test]
    fn readonly_profile_resolves_readonly_agent() {
        let root = PathBuf::from("C:/kanzei-r102-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::Readonly,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevProfile)
            .add(super::ResearchProfile)
            .add(super::ReadonlyProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();

        let agent = snapshot.select_agent(Some("readonly")).unwrap();
        assert_eq!(agent.name, "readonly");
        assert!(agent.system.contains("MUST NOT modify"));
        assert!(agent.system.contains("read/glob/grep"));
        // 档位缺省选 agent:readonly 档位下默认选中 readonly(dev/research 不匹配)。
        let default_agent = snapshot.select_agent(None).unwrap();
        assert_eq!(default_agent.name, "readonly");

        // 权限快照(批1 交付):只读档位下读/检索类默认放行。
        let snap = snapshot.permission_snapshot();
        let read = snap
            .iter()
            .find(|s| s.action == "read")
            .expect("快照里应有 read");
        assert_eq!(read.effect, Effect::Allow);
        let glob = snap
            .iter()
            .find(|s| s.action == "glob")
            .expect("快照里应有 glob");
        assert_eq!(glob.effect, Effect::Allow);
    }

    /// R-102 批2:readonly 档位权限强制——写与命令硬 deny 且带替代指引。
    #[test]
    fn readonly_profile_hard_denies_write_and_bash() {
        let root = PathBuf::from("C:/kanzei-r102-deny");
        let ctx = ResolveCtx {
            profile: ProfileKind::Readonly,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevProfile)
            .add(super::ResearchProfile)
            .add(super::ReadonlyProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();

        // 写与命令:整体 deny(工具会被摘除,模型看不见)。
        for action in ["write", "edit", "bash"] {
            assert_eq!(
                snapshot.evaluate(action, "*"),
                Effect::Deny,
                "{action} 在只读档位必须硬 deny"
            );
            let hint = snapshot.denial_hint(action, "anything");
            assert!(
                hint.contains("read/glob/grep"),
                "{action} 的拒绝理由要点名替代工具: {hint}"
            );
        }
        // 只读族放行。
        for action in ["read", "glob", "grep", "files", "webfetch"] {
            assert_eq!(
                snapshot.evaluate(action, "*"),
                Effect::Allow,
                "{action} 在只读档位必须放行"
            );
        }
        // git 只读子命令放行,其余子命令维持默认 ask。
        for subcommand in ["status", "diff", "log"] {
            assert_eq!(
                snapshot.evaluate("git", subcommand),
                Effect::Allow,
                "git {subcommand} 应放行"
            );
        }
        // 工具物化:write/edit/bash 从工具表摘除,模型根本拿不到。
        let names: Vec<&str> = snapshot
            .materialize_tools()
            .iter()
            .map(|t| t.name())
            .collect();
        for gone in ["write", "edit", "bash"] {
            assert!(
                !names.contains(&gone),
                "{gone} 应被整体摘除,实际工具表: {names:?}"
            );
        }

        // R-102 验收③:档位权限快照测试。快照语义 = 每个工具在 `*` 资源上的
        // 最终决策 + 是否整体摘除。只读档位快照必须反映全部强制规则。
        let snap = snapshot.permission_snapshot();
        let by_action = |action: &str| {
            snap.iter()
                .find(|s| s.action == action)
                .unwrap_or_else(|| panic!("快照里缺少 {action}"))
        };
        // 写/命令:Deny 且 fully_denied(工具整体摘除)。
        for action in ["write", "edit", "bash"] {
            let item = by_action(action);
            assert_eq!(item.effect, Effect::Deny, "{action} 快照应为 Deny");
            assert!(item.fully_denied, "{action} 快照应标记 fully_denied");
        }
        // 只读族:Allow 且不摘除。
        for action in ["read", "glob", "grep", "files", "webfetch"] {
            let item = by_action(action);
            assert_eq!(item.effect, Effect::Allow, "{action} 快照应为 Allow");
            assert!(!item.fully_denied, "{action} 不应被摘除");
        }
        // task 补进快照(runner 内建只读子代理),档位下默认 ask 即放行无副作用。
        let task = by_action("task");
        assert!(!task.fully_denied, "task 在只读档位不应被摘除");
    }
}
