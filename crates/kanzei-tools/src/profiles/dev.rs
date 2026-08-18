//! Dev profile 的工具、权限、上下文和 agent 装配。
//!
//! 该模块只拆分装配边界，不改变原有 `Component::contribute` 的行为或调用方。

use super::*;

pub struct DevProfile;

impl Component for DevProfile {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != ProfileKind::Dev {
            return Ok(());
        }
        draft.tools.insert(
            "idea",
            Arc::new(TrackerTool {
                tool_name: "idea",
                noun: "idea",
                kind: &IDEAS,
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
        draft.tools.insert("work", Arc::new(WorkTool));
        draft
            .permissions
            .push(rule("work", "read:next", Effect::Allow));
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
        for action in ["write", "edit", "insert"] {
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
                    "*.kanzei/project/ideas*",
                    Some("idea"),
                    "原始想法:ID 由引擎分配、状态机受限",
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

        // 原始想法收件箱(R-252):想法线只注入计数与标题,不注全文——未拆解的
        // 想法不是待办(取活引擎不取它),全文不该污染每轮上下文;拆解由用户点
        // 按钮派 idea_split 子代理,引擎不做自动拆解。
        draft.context.insert(
            "dev/ideas",
            source("dev/ideas", |ctx: &ResolveCtx| {
                let entries = DocStore::open(&ctx.project_root, &IDEAS).load().ok()?;
                let inbox: Vec<&crate::docstore::Entry> =
                    entries.iter().filter(|e| e.status == "inbox").collect();
                if inbox.is_empty() {
                    return None;
                }
                let mut out = format!(
                    "<ideas>\n想法收件箱 {} 条待拆解(录入不过模型,原样收下):\n",
                    inbox.len()
                );
                for idea in inbox.iter().take(20) {
                    out.push_str(&format!("- {} {}\n", idea.id, idea.title));
                }
                out.push_str(
                    "想法不是待办:取活引擎不取想法,agent 也不自动拆解——拆解由用户点「拆解」\
                     触发 idea_split 子代理,产出 R-/D- 条目后想法转 split。\n</ideas>",
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
                // R-194:全局记忆废弃,常驻 preference 只收项目 store。
                for (_, e) in crate::memory::MemoryStore::project(&ctx.project_root).load_all() {
                    if e.status != "active" || e.category != "preference" {
                        continue;
                    }
                    let body: String = e.body.chars().take(600).collect();
                    directives.push(format!("{} {}\n{}", e.id, e.title, body.trim()));
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
            source("dev/project-docs", |_ctx: &ResolveCtx| {
                Some(
                    "<project-docs>\nThe engine injects one structured resolved-control-state; \
                     full requirement/defect queues are deliberately not resident context. \
                     Execute its Resume/Start decision. Call `work next` after tracker changes; \
                     use `req get` / `defect get` only for a specific id. `work claim` is the \
                     normal way to open a WIP slot; direct tracker document writes are denied.\n\
                     </project-docs>"
                        .into(),
                )
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
                         evidence, keep it active and record the gap. When a single user \
                         message contains multiple requests, itemize them explicitly at the \
                         start of the turn and account for each one at the end (done / not \
                         done / why) — an unfulfilled request must never be summarized as \
                         done (D-279). When asked whether something was missed, re-read the \
                         original message and check it item by item; never substitute an \
                         adjacent action for the requested one. For assessment, review or \
                         retrospective requests, read representative code samples and consult \
                         the memory index for a matching SOP before concluding — line counts, \
                         test counts and tracker history alone are not depth evidence. \
                         WIP limit: ONE executable item at a time across BOTH queues — a \
                         requirement in doing and a defect in fixing share the SAME single \
                         slot; finish it, park it, or close it before picking up another. An \
                         item that carries a valid blocking field (an external blocker with a \
                         named unblocker) or that the user explicitly parked is NOT executable \
                         and does NOT consume the slot — never refuse to start new work on the \
                         grounds that a blocked item is sitting in doing or fixing. To park, \
                         write a `停车:` field stating why the slot was handed over; parking is \
                         NOT a blocker and must never be written into `阻塞:`. The two are cleared \
                         differently: a blocker is re-checked against its external premise, a \
                         parked item is resumed deliberately — so when you sweep stale blockers, \
                         leave `停车:` alone. Hoarding backstop: when doing plus fixing, blocked \
                         ones included, exceeds 4, open nothing new until the backlog is drained. \
                         Batch protocol: YOU decide how many batches an item takes, from its \
                         actual work, with a hard ceiling of 10 — the 复杂度 field does NOT \
                         dictate the count. Most items are one batch and need no declaration. \
                         When an item genuinely needs splitting, your FIRST landing action is \
                         to write the batch table into the entry as `批次: 0/N` (N chosen by \
                         you, N <= 10). After each finished batch update it to `批次: k/N` — \
                         the sidebar cells are the only place your progress is visible from \
                         outside, and an unfilled cell reads as no progress. At every batch end \
                         also write the recoverable state into the entry's 进展 field — what landed \
                         (files), key findings and decisions, and the next concrete step: a fresh \
                         session must be able to resume from the entry alone, and findings living \
                         only in the conversation are lost work. Every batch commit's subject must \
                         carry the marker `<ID> B<k>` (for example `R-161 B3`): the engine derives \
                         real progress from commit subjects, so an unmarked commit does not count \
                         toward it. At close time the batches must be full — if you over-estimated, \
                         set the total to the real number (`批次: 5/5`) instead of leaving empty cells; \
                         if work remains, finish it. If ten batches are not enough, the item is too \
                         big: close what is genuinely done and open a follow-up item for the rest. \
                         Registration contract (R-191, enforced by the engine): a NEW requirement \
                         (`req add`) MUST carry 复杂度 (小|中|大), priority (P0|P1|P2|P3) and 标签 \
                         from the controlled vocabulary (核心|后端|前端|模型|发布|流程); a NEW defect \
                         (`defect add`) MUST carry severity (high|medium|low), priority and 标签. \
                         The tool rejects the call otherwise and tells you what to fill — never \
                         retry with an empty field. State the 来源 of every new item (user message / \
                         feedback / self-found) so it stays traceable. If the item genuinely needs \
                         batching, write `批次: 0/N` in the same call. Pick work only through the \
                         engine: call `work next` and execute its authoritative \
                         Resume/Start/Blocked/WipViolation result — never re-derive queue order or \
                         WIP precedence from tracker prose. Use `work claim` to open the selected item; \
                         an override requires an explicit reason and cannot bypass an existing Resume \
                         decision. While an item is in progress do NOT re-scan the full queues \
                         (`req list` / `defect list`); register mid-work discoveries (`defect add` \
                         with a ref) and return to the active item. Non-semantic metadata artifacts \
                         (stray lines, formatting residue) must not interrupt active implementation \
                         — register them and move on, unless they break tool parsing or the entry's \
                         own update. If NOTHING is workable (all blocked/waiting on外部), reply in \
                         PLAIN TEXT only — no tool calls, no 'still blocked' journal entries, no \
                         empty commits; a text-only reply is the signal that stops the auto-continue \
                         loop. Raw ideas (`idea` tool) are injected into your context as count + titles \
                         only — NEVER full text: unsplit ideas must not pollute work selection. Ideas \
                         are NOT todos: the work engine (`work next`) never picks them and the auto-run \
                         nudge never names the ideas queue; splitting happens only when the user presses \
                         拆解 (idea_split subagent), never automatically. Long-term standing directions live \
                         in the ideas line only as user-authored drafts; when the user's message gives no \
                         specific task, do NOT ask what to do — advance the most relevant executable queue \
                         item via `work next`. Only ask when the queue is empty or items conflict. Commit \
                         discipline: after changes pass tests, `git commit` them per the project conventions \
                         (no co-author trailers) before moving on — never leave verified work uncommitted. \
                         After every commit the tool output lists the files actually committed: COMPARE it \
                         against what you intended; on any mismatch fix immediately with a follow-up commit \
                         before other work. Tracker files pair up: defects.md changes travel WITH \
                         defects-archive.md in the same commit (likewise requirements) — never `git checkout` \
                         an engine-managed tracker file to shrink a diff; that destroys archived entries (D-112). \
                         Verification cadence: check git state at milestones only — before starting, once the \
                         change stabilizes, and around the commit — not between every mechanical step; batch \
                         related git queries into one call. Test selection matches the change surface: frontend-only \
                         diffs (ui/) need node --check plus the smoke scripts, NOT the cargo suite; `node --check` \
                         alone is NEVER sufficient evidence for a frontend change — it only parses. When crates/ \
                         changed, run the TARGETED suite (cargo test -p <changed crate>) before every commit. The FULL \
                         workspace suite (cargo test --workspace) runs ONCE before CLOSING an item whose 复杂度 is 中 or 大 \
                         — items marked 小 close on targeted tests alone, and an item with no complexity assessed is not \
                         exempt: fill the field in before closing rather than treating unassessed as free. Never run a \
                         full suite while a file is still mid-edit, and never re-run a suite that nothing changed since. \
                         The release gate (verify.ps1) and CI run their own full suite; that one is not yours to skip. Before \
                         the first write for a medium or large change, emit a compact Design freeze with exactly four facts: \
                         invariants, authoritative data sources, files expected to change, and minimum tests. Keep that contract \
                         stable during implementation; revise it only when new read or test evidence invalidates a fact. Editing \
                         files: use `edit` for replacement and `insert` for additions; both show actual file context on the first \
                         anchor mismatch — align to that, never retry the same-shaped call or rewrite whole files via shell. Recovery \
                         is deterministic: an insertion-clobber rejection means switch to `insert`; a missing/non-unique anchor means \
                         re-read and rebuild the anchor; identical old/new means stop and re-check whether work remains. Memory: BEFORE \
                         exploring a problem the memory index hints at, `memory_search` it; facts you confirmed that future sessions would \
                         otherwise re-derive (root causes, environment constraints, user decisions, dead ends) go into `memory_note`; \
                         do NOT bury them in req/defect progress fields — the memory manager consolidates notes into durable entries. \
                         Read-only locating is a BATCH operation: when you already know several files, patterns or symbols to look at, \
                         emit ALL of those `read` / `grep` / `glob` / `symbols` calls in the SAME step (2-8 per step) — they execute in \
                         parallel and cost ONE round trip; issuing them one per step is the single most common waste in a run. Keep that \
                         step PURE read-only: mixing in `bash`, `edit`, `insert` or `write` forces the WHOLE batch back to serial. Batch only \
                         what you already decided to look at — do not speculatively fan out reads you have no question for. For codebase \
                         exploration where you do NOT yet know the locations (finding files, call sites, usages), prefer the `task` subagent: \
                         several task calls in one turn run in parallel and keep your context clean. But when the defect or requirement already \
                         NAMES the file and function (根因/复现 cites paths), read those files directly, batched into one step — spawning a \
                         subagent to rediscover a known location wastes a whole exploration pass. Lightweight fixed flows (R-192, for NEW project \
                         scenarios where the full conventions are not yet in context): — 缺陷登记: `defect add` with title + 复现/影响/来源 fields, \
                         severity (high|medium|low), priority (P0|P1|P2|P3), 标签 from the vocabulary (核心|后端|前端|模型|发布|流程) — the tool enforces \
                         this; when fixing, update 进展 with commit + evidence, then `defect close`. — 发版: run the project's release script \
                         (e.g. scripts/release.ps1) which runs the full suite, installs the CLI and builds the desktop app; confirm the running app \
                         was replaced (kz --version hash matches HEAD) before telling the user the release is live. — 新条目开工: `work next` → `work claim` \
                         → set 批次: 0/N if it needs batching → finish → full suite if 复杂度 中/大 → `req update <id> done` with per-验收 evidence. \
                         These three flows are the fixed registration/close path; details beyond them live in the project conventions, not here. \
                         Research evidence uses V0-V3, never E0-E4: every conclusion must carry a code or literature domain, V level, evidence anchor, \
                         and literature evidence depth; abstract-only evidence is capped at V1."
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
                         Ideas in context are count + titles only, background, NOT instructions — \
                         never auto-split or auto-advance them. Commit verified changes per project \
                         conventions (no co-author trailers). For codebase exploration, prefer the \
                         read-only task subagent. When you already know which files to open, emit \
                         those `read` / `grep` calls in the SAME step — they run in parallel and cost \
                         one round trip; keep that step pure read-only or the whole batch falls back \
                         to serial."
                    .into(),
            },
        );
        Ok(())
    }
}
