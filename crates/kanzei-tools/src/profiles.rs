//! 双模式 Profile 组件:dev(需求/缺陷)与 research(来源/发现)。
//! 组件按当前 profile 决定贡献什么;权限规则是硬门禁的落点。

use std::sync::Arc;

use kanzei_harness::{
    rule, source, AgentDef, AgentMode, Component, Effect, HarnessDraft, ProfileKind, ProfileScope,
    ResolveCtx,
};

use crate::docstore::{DocStore, DECISIONS, DEFECTS, IDEAS, REQUIREMENTS};
use crate::tracker::TrackerTool;
use crate::work::WorkTool;

mod dev;
mod readonly;
mod research;
pub use dev::DevProfile;
pub use readonly::ReadonlyProfile;

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

/// 记忆注入的字符预算:记忆是常驻上下文,超预算必须显式说明丢了多少,不做静默截断。
// dev/memory 注入预算移入 memory 模块与 prompt_hints 共用(D-216:同一口径)。
use crate::memory::MEMORY_CONTEXT_BUDGET;

pub struct ResearchProfile;

impl Component for ResearchProfile {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != ProfileKind::Research {
            return Ok(());
        }
        research::register_tools(draft);
        research::configure_permissions(draft);

        draft.context.insert(
            "research/docs",
            source("research/docs", |ctx: &ResolveCtx| {
                let src = research::index_of(ctx, &crate::docstore::SOURCES, "Sources");
                let fnd = research::index_of(ctx, &crate::docstore::FINDINGS, "Findings");
                let req = research::index_of(ctx, &REQUIREMENTS, "Requirements");
                let defect = research::index_of(ctx, &DEFECTS, "Defects");
                let conventions = std::fs::read_to_string(
                    ctx.project_root.join(".kanzei/project/conventions.md"),
                )
                .ok()
                .map(|text| text.chars().take(8000).collect::<String>());
                let backlog = [req, defect]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join("\n");
                let conventions = conventions
                    .map(|text| format!("<conventions>\n{text}\n</conventions>\n"))
                    .unwrap_or_default();
                let memory_guidance = "<memory>\nUse the unified `memory_search` tool to retrieve project memory and `memory_note` to submit a durable draft; the historical `.kanzei/research/memory.md` is not a research memory source.\n</memory>\n";
                Some(format!(
                    "<research-docs>\n{}{}<backlog>\n{backlog}\n</backlog>\n{}{}Record sources with `source add` BEFORE citing them; every finding must cite refs. Use the backlog only as a read-only index; req/defect get reads existing entries and add creates a [todo] draft for dev review.\n</research-docs>",
                    src.map(|s| s + "\n").unwrap_or_default(),
                    fnd.map(|s| s + "\n").unwrap_or_default(),
                    conventions,
                    memory_guidance,
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
                system: "You are the research agent. Before searching, use `research_plan` to create an explicit plan tree, record clarification questions, and request user approval; never approve or execute an unapproved plan. After approval, use the `research_loop` tool with start/resume actions to drive the bounded search-read-reflect loop. For each isolated subtask call begin_search first and pass its task_id to add_evidence; the max_concurrency gate is mechanical. For every websearch/webfetch call, also pass the same topic and active task_id; calls without them are rejected while a loop is running. Use websearch/webfetch only as isolated subtasks; before returning any result to the main context, compress it via research_loop add_evidence with relevance, source_ids, and a sourced summary—never pass raw webpage or tool output into the loop. Use reflect to record knowledge gaps and decide whether another round is needed; write findings only through add_finding with source refs. After the loop reaches writing, call the `research_write` tool: write_outline first, then write_section once per outline section, assemble_paper for heavy topics, and compile_paper through the LaTeX channel; use repair_paper only after a failed compile and preserve its diagnostics. Before starting a loop, use the `research_verify` tool budget_set for explicit round/token/concurrency knobs; after writing, use verify_claims to mechanically check every FACT source and evidence anchor, and use capture_source for complete literature正文 rather than trusting abstract/要点 fields. Before cross-checking claims, use the `research_index` tool: index_build/index_resume creates or resumes the topic Tantivy index, search uses the same interface for literature and code, and symbols mode performs code symbol reverse lookup. Record every consulted source \
                         (`source add`) and register conclusions as findings citing those \
                         sources. Every conclusion must state its code or literature domain, \
                         V0-V3 level, evidence anchor, and literature evidence depth; use V \
                         evidence, never E0-E4 verification levels, and cap abstract-only \
                         literature evidence at V1. Use `memory_search` for project memory and \
                         `memory_note` for durable research conclusions; do not use the historical \
                         `.kanzei/research/memory.md`. The final report goes to \
                         .kanzei/research/report.md."
                    .into(),
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DevProfile, ResearchProfile};
    use kanzei_harness::{
        rule, ConfigComponent, Effect, Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx,
    };
    use serde_json::json;
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
            "itemize them explicitly",      // D-279:多项诉求逐项清单
            "re-read the original message", // D-279:追问时回读核对
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
            // D-434:停车与阻塞是两个字段、两种清除方式。提示词里没有这条,
            // 模型只能把停车写进「阻塞」,下一轮复核阻塞时又被当失效自阻塞清掉。
            "write a `停车:` field",
            "must never be written into `阻塞:`",
            "leave `停车:` alone",
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

    /// 只读定位必须教「同一步批量发」。
    ///
    /// 实测(2026-08-15,本仓 state.db 2392 个 step):**85% 的 step 只含一个工具
    /// 调用**,而并行发出的能力早就全线打通(只读工具都是 ToolConcurrency::
    /// shared_worktree,can_parallel_tools 在生产档位恒可并行,协议侧 openai_responses
    /// 显式 parallel_tool_calls:true)。缺的只有提示——全仓唯一教「同轮多发」的文字
    /// 在 `task` 工具描述里,直读分支一个字都没有。连续只读游程可省 267 个往返
    /// (全部 step 的 11.2%),这条提示就是去吃那一块。
    #[test]
    fn dev提示词教只读定位同步批量并行() {
        let system = dev_system_prompt("parallel-locating");
        for required in [
            "SAME step",
            "in parallel",
            // 整批一票否决:混进一个需 Ask 的工具,can_parallel_tools 直接 false,
            // 模型照做却拿不到并行。这句是必须项,不是修饰。
            "PURE read-only",
            // 只批量已决定要看的东西——否则「批量」被理解成投机多读,
            // 上下文膨胀会盖过往返收益(省的是往返,不是 token)。
            "do not speculatively fan out",
        ] {
            assert!(
                system.contains(required),
                "只读并行引导缺失:dev system prompt 里没有 `{required}`"
            );
        }
        // 反向:旧句式把 task 子代理写成勘察的唯一出路,与新引导互斥。
        assert!(
            !system.contains("For codebase exploration (finding files"),
            "旧勘察句式残留:它把 task 写成唯一出路,与「已知位置就同步批量直读」\
             并存会让模型按就近句取其一(D-242 的复发形状)。"
        );
    }

    /// 2026-08-13 用户定调(V4PRO 运行复盘):取活裁决已经下沉 work 工具；
    /// prompt 只保留“执行结果”，不得再复制一套队列算法形成双真源。
    #[test]
    fn dev_system_prompt_enforces_resume_precedence_and_context_discipline() {
        let system = dev_system_prompt("resume-precedence");
        for required in [
            "call `work next`",
            "authoritative Resume/Start/Blocked/WipViolation",
            "cannot bypass an existing Resume decision",
            "do NOT re-scan",
            "Non-semantic metadata artifacts",
            "recoverable state",
            "resume from the entry alone",
            "read representative code samples",
        ] {
            assert!(
                system.contains(required),
                "取活显式序/上下文纪律真源缺失:dev system prompt 里没有 `{required}`"
            );
        }
        for forbidden in ["outranks queue priority", "scan the selected first queue"] {
            assert!(
                !system.contains(forbidden),
                "取活算法仍复制在 prompt 中，Resolved Control State 不是单一真源: {forbidden}"
            );
        }
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

    #[test]
    fn dev_system_prompt_freezes_design_and_maps_edit_recovery() {
        let system = dev_system_prompt("r236-design-freeze");
        assert!(system.contains("Design freeze"));
        for fact in [
            "invariants",
            "authoritative data sources",
            "files expected to change",
            "minimum tests",
        ] {
            assert!(system.contains(fact), "missing design-freeze fact: {fact}");
        }
        assert!(system.contains("insertion-clobber rejection means switch to `insert`"));
        assert!(system.contains("missing/non-unique anchor means re-read"));
        assert!(system.contains("identical old/new means stop"));
    }

    /// 三条定调的两份真源必须同口径:通用规则由引擎模板注入(R-191 单源,
    /// DEFAULT_CONVENTIONS),dev system prompt 常驻,任一侧单方面改口,模型就会
    /// 同时读到两条互斥规则——这正是 D-242/D-128 反复出现的失效模式。
    ///
    /// 只断言三个短 token,不锁整句措辞:规范是用户手写资产,行文随时可改,
    /// 但「1 个槽 / 上限 10 批 / 全量只对中大」这三个判据不能悄悄消失。
    ///
    /// R-191 批5b:真源从项目 conventions.md 迁到引擎模板——通用节已从项目文件
    /// 删除,若仍断言项目文件必然整段缺失;反向断言项目文件不得再含通用节,
    /// 防「复制必然漂移」的旧病复发。
    #[test]
    fn conventions_与提示词对三条定调保持同口径() {
        // 通用规则真源:引擎内置模板(编译期内嵌,所有项目一致)。
        let text = kanzei_harness::DEFAULT_CONVENTIONS;

        // 小节 = 从该二级标题起到下一个二级标题为止(`### ` 不会被误当边界)。
        let section = |heading: &str| -> &str {
            let start = text
                .find(heading)
                .unwrap_or_else(|| panic!("引擎模板里找不到小节 {heading}"));
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
                "引擎模板 {heading} 缺少「{token}」({定调});\
                 提示词已按新口径写,规范这侧沉默就等于半份真源(D-242)。"
            );
        }

        // R-191 单源防回归:项目 conventions.md 只放项目特有规则,不得再复制通用节。
        let project_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.kanzei/project/conventions.md");
        if let Ok(project_text) = std::fs::read_to_string(&project_path) {
            for forbidden in [
                "## 1.1 需求取活与阻塞调度",
                "## 1.25 完成判定与验收证据",
                "## 2. 代码修改原则",
                "## 10. 任务级并行",
            ] {
                assert!(
                    !project_text.contains(forbidden),
                    "项目 conventions.md 仍含通用节「{forbidden}」——\
                     通用规则已由引擎模板单源(R-191),项目文件复制必然漂移。"
                );
            }
        }
    }

    /// R-217:dev 档注册 websearch 且默认 Ask(自主轮 NonInteractive 下即拒,
    /// 交互轮可放行);域名白名单规则可精确放行 webfetch/websearch。
    #[test]
    fn dev档注册websearch默认ask_域名白名单可精确放行() {
        let root = PathBuf::from("C:/kanzei-r217-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness.add(crate::base::BaseComponent).add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let names: Vec<&str> = snapshot
            .materialize_tools()
            .iter()
            .map(|t| t.name())
            .collect();
        assert!(
            names.contains(&"websearch"),
            "dev 档必须有 websearch: {names:?}"
        );
        assert_eq!(
            snapshot.evaluate("websearch", "*"),
            kanzei_harness::Effect::Ask,
            "websearch 默认 Ask"
        );
        assert_eq!(
            snapshot.evaluate("webfetch", "*"),
            kanzei_harness::Effect::Ask,
            "webfetch 默认 Ask"
        );

        // 域名白名单:rule("webfetch", "docs.rs/*", Allow) 匹配规范化资源。
        let mut config = KanzeiConfig::default();
        config
            .permissions
            .rules
            .push(rule("webfetch", "docs.rs/*", Effect::Allow));
        config
            .permissions
            .rules
            .push(rule("websearch", "html.duckduckgo.com/*", Effect::Allow));
        let allow_ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(config),
        };
        let mut allow_harness = Harness::default();
        allow_harness
            .add(crate::base::BaseComponent)
            .add(ConfigComponent);
        let snap2 = allow_harness.resolve(&allow_ctx).unwrap();
        assert_eq!(
            snap2.evaluate("webfetch", "docs.rs/crate/tokio"),
            kanzei_harness::Effect::Allow,
            "docs.rs/* 白名单应放行 docs.rs 域名"
        );
        assert_eq!(
            snap2.evaluate("webfetch", "example.com/x"),
            kanzei_harness::Effect::Ask,
            "白名单外域名仍走 Ask"
        );
        assert_eq!(
            snap2.evaluate("websearch", "html.duckduckgo.com/html"),
            kanzei_harness::Effect::Allow,
            "websearch 域名白名单应放行"
        );
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
        // R-191 验收②:模板生成测试断言关键节存在(§1.1 阻塞口径 / §1.3 批次 /
        // §1.4 节奏 / §1.25 验收证据)——四个关键节必须全部出现在注入后的上下文。
        let bare = PathBuf::from("C:/kanzei-r191-default-test");
        let baseline = baseline_of(&bare);
        for required in [
            "通用开发规范单源",
            "阻塞:` 字段只留给外部阻塞", // §1.1 阻塞口径
            "批次: k/N",                 // §1.3 批次
            "复杂度中/大",               // §1.4 节奏(全量触发点)
            "逐条对照验收原文",          // §1.25 验收证据
            "任务级并行",                // §10
            "可用即关闭",                // §1.2
            "compile_gate",              // §1.4 提交前代码门禁(D-264)
            "多项诉求",                  // §1.25 D-279:用户诉求层逐项清单
            "回读原始消息",              // §1.25 D-279:追问时不得相邻动作顶替
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

    /// R-192:轻量级固定流程(发版/缺陷登记/新条目开工)必须注入 dev system——
    /// 新项目场景下全文规范未在上下文里,agent 靠这段固定流程就能正确完成登记与关闭。
    #[test]
    fn dev_system_prompt_teaches_lightweight_fixed_flows() {
        let system = dev_system_prompt("r192-flow");
        for required in [
            "Lightweight fixed flows (R-192",
            "缺陷登记",
            "defect add",
            "发版",
            "release.ps1",
            "新条目开工",
            "work next",
            "per-验收 evidence",
        ] {
            assert!(
                system.contains(required),
                "R-192 轻量级固定流程缺失:dev system prompt 里没有 `{required}`。\
                 新项目场景无法降低上下文依赖。"
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

    #[test]
    fn dev_design_index_excludes_superseded_rows_and_document_bodies() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r318-design-context-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project/architecture")).unwrap();
        std::fs::create_dir_all(root.join("docs/design")).unwrap();
        std::fs::write(
            root.join(".kanzei/project/architecture/README.md"),
            "## live_design\n- [identity: live_design; last_verified_commit: abcdef1] [`live.md`](../../../docs/design/live.md)\n## superseded\n- [identity: superseded; as_of_commit: abcdef1; superseded_by: live.md] [`old.md`](../../../docs/design/old.md)\n",
        )
        .unwrap();
        std::fs::write(
            root.join("docs/design/live.md"),
            "LIVE_DESIGN_BODY_MUST_NOT_BE_READ\n",
        )
        .unwrap();
        std::fs::write(
            root.join("docs/design/old.md"),
            "SUPERSEDED_BODY_MUST_NOT_BE_READ\n",
        )
        .unwrap();

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
            baseline.contains("live.md"),
            "非 superseded 设计入口应进入默认索引上下文"
        );
        assert!(
            !baseline.contains("old.md"),
            "superseded 设计入口不应进入默认索引上下文"
        );
        assert!(!baseline.contains("LIVE_DESIGN_BODY_MUST_NOT_BE_READ"));
        assert!(!baseline.contains("SUPERSEDED_BODY_MUST_NOT_BE_READ"));
        std::fs::remove_dir_all(root).unwrap();
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
            (".kanzei/project/ideas.md", "idea"),
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

    #[tokio::test]
    async fn research_context_injects_backlog_conventions_and_restricted_tracker_tools() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r221-b4-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::create_dir_all(root.join(".kanzei/research")).unwrap();
        std::fs::write(
            root.join(".kanzei/research/memory.md"),
            "legacy research memory must not be injected\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".kanzei/project/conventions.md"),
            "# project conventions\nB4 convention marker\n",
        )
        .unwrap();
        crate::docstore::DocStore::open(&root, &crate::docstore::REQUIREMENTS)
            .save(&[crate::docstore::Entry {
                id: "R-901".into(),
                title: "研究回流需求".into(),
                status: "todo".into(),
                severity: None,
                fields: vec![],
            }])
            .unwrap();
        crate::docstore::DocStore::open(&root, &crate::docstore::DEFECTS)
            .save(&[crate::docstore::Entry {
                id: "D-901".into(),
                title: "研究回流缺陷".into(),
                status: "open".into(),
                severity: Some("medium".into()),
                fields: vec![],
            }])
            .unwrap();

        let ctx = ResolveCtx {
            profile: ProfileKind::Research,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(super::ResearchProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let baseline = snapshot.system_baseline();
        for required in [
            "<backlog>",
            "R-901",
            "D-901",
            "B4 convention marker",
            "read-only index",
            "unified `memory_search`",
            "memory_note",
        ] {
            assert!(
                baseline.contains(required),
                "research context 缺少 B5 内容: {required}"
            );
        }
        assert!(
            !baseline.contains("legacy research memory must not be injected"),
            "research context 不得注入历史 research/memory.md"
        );
        for tool_name in ["source", "finding", "req", "defect"] {
            assert_eq!(
                snapshot.evaluate(tool_name, "read:get"),
                Effect::Allow,
                "research {tool_name} 应允许单条目读取"
            );
            assert_eq!(
                snapshot.evaluate(tool_name, "write:add"),
                Effect::Allow,
                "research {tool_name} 应允许新增草稿"
            );
            assert_eq!(
                snapshot.evaluate(tool_name, "write:update"),
                Effect::Deny,
                "research {tool_name} 不应允许修改既有条目"
            );
        }
        for resource in [
            "read:get",
            "write:create",
            "write:clarify",
            "write:request_approval",
        ] {
            assert_eq!(
                snapshot.evaluate("research_plan", resource),
                Effect::Allow,
                "research_plan 应允许 {resource}"
            );
        }
        let plan_tool = snapshot
            .materialize_tools()
            .into_iter()
            .find(|tool| tool.name() == "research_plan")
            .expect("research 档缺少 research_plan tool");
        let schema = plan_tool.input_schema();
        let actions = schema
            .pointer("/properties/action/enum")
            .and_then(|value| value.as_array())
            .unwrap();
        assert_eq!(
            serde_json::to_value(actions).unwrap(),
            serde_json::json!(["get", "create", "clarify", "request_approval"])
        );
        assert!(snapshot
            .select_agent(Some("research"))
            .unwrap()
            .system
            .contains("research_plan"));
        for name in ["req", "defect"] {
            let tool = snapshot
                .materialize_tools()
                .into_iter()
                .find(|tool| tool.name() == name)
                .unwrap_or_else(|| panic!("research 档缺少 {name} tool"));
            let schema = tool.input_schema();
            let actions = schema
                .pointer("/properties/action/enum")
                .and_then(|value| value.as_array())
                .unwrap();
            assert_eq!(
                actions,
                &[serde_json::json!("get"), serde_json::json!("add")]
            );
        }
        let tools = snapshot.materialize_tools();
        let memory_search = tools
            .iter()
            .find(|tool| tool.name() == "memory_search")
            .expect("research 档缺少 memory_search");
        let memory_note = tools
            .iter()
            .find(|tool| tool.name() == "memory_note")
            .expect("research 档缺少 memory_note");
        assert_eq!(snapshot.evaluate("memory_search", "*"), Effect::Allow);
        assert_eq!(snapshot.evaluate("memory_note", "*"), Effect::Allow);
        let tool_ctx = ToolCtx::new(root.clone(), root.clone());
        let searched = memory_search
            .execute(
                json!({"query": "B5 unified memory", "scope": "project"}),
                &tool_ctx,
            )
            .await;
        assert!(!searched.is_error, "research memory_search 应可真实调用");
        let noted = memory_note
            .execute(
                json!({
                    "summary": "B5 research memory note",
                    "detail": "统一记忆通道回归",
                    "category_hint": "fact"
                }),
                &tool_ctx,
            )
            .await;
        assert!(
            !noted.is_error,
            "research memory_note 应可真实投递: {}",
            noted.content
        );
        assert!(noted.content.contains("pending notes"));

        std::fs::remove_dir_all(root).unwrap();
    }

    /// R-221 B3:dev/research 都能看到 V 表口径,且不把 E0-E4 当研究证据等级。
    #[test]
    fn research_evidence_prompt_uses_v_table_and_literature_depth() {
        let dev = dev_system_prompt("r221-v-table");
        for required in [
            "Research evidence uses V0-V3",
            "never E0-E4",
            "literature evidence depth",
            "abstract-only evidence is capped at V1",
        ] {
            assert!(
                dev.contains(required),
                "dev prompt 缺少 B3 口径: {required}"
            );
        }

        let root = PathBuf::from("C:/kanzei-r221-v-table-research");
        let ctx = ResolveCtx {
            profile: ProfileKind::Research,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevProfile)
            .add(super::ResearchProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let research = snapshot
            .select_agent(Some("research"))
            .unwrap()
            .system
            .clone();
        for required in [
            "V0-V3 level",
            "never E0-E4 verification levels",
            "literature evidence depth",
            "abstract-only literature evidence at V1",
        ] {
            assert!(
                research.contains(required),
                "research prompt 缺少 B3 口径: {required}"
            );
        }
    }

    /// R-221 B1:research 只保留事实观察与专用科研工具,不允许 shell/git 写入。
    #[test]
    fn research_profile_hard_denies_bash_and_git_writes() {
        let root = PathBuf::from("C:/kanzei-r221-research");
        let ctx = ResolveCtx {
            profile: ProfileKind::Research,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevProfile)
            .add(super::ResearchProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();

        assert_eq!(snapshot.evaluate("bash", "*"), Effect::Deny);
        let bash_hint = snapshot.denial_hint("bash", "anything");
        assert!(
            bash_hint.contains("latex") && bash_hint.contains("plot"),
            "{bash_hint}"
        );
        for subcommand in ["status", "diff", "log"] {
            assert_eq!(snapshot.evaluate("git", subcommand), Effect::Allow);
        }
        for subcommand in ["stage", "commit", "merge_ff", "finalize"] {
            assert_eq!(
                snapshot.evaluate("git", subcommand),
                Effect::Deny,
                "git {subcommand}"
            );
        }
        for action in ["read", "glob", "grep", "files", "webfetch", "websearch"] {
            assert_eq!(snapshot.evaluate(action, "*"), Effect::Allow, "{action}");
        }
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
        for action in ["write", "edit", "insert", "bash"] {
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
        for gone in ["write", "edit", "insert", "bash"] {
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
        for action in ["write", "edit", "insert", "bash"] {
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

    #[test]
    fn research_docs_context_下一轮读取新topic来源() {
        let root = std::env::temp_dir().join(format!(
            "kz-research-context-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let topic =
            crate::docstore::DocStore::open_topic(&root, &crate::docstore::SOURCES, "r221-chain")
                .unwrap();
        topic
            .save(&[crate::docstore::Entry {
                id: "S-001".into(),
                title: "first topic source".into(),
                status: "active".into(),
                severity: None,
                fields: vec![],
            }])
            .unwrap();
        let ctx = ResolveCtx {
            profile: ProfileKind::Research,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness.add(ResearchProfile);
        let snapshot = harness.resolve(&ctx).unwrap();
        let before = snapshot.stable_system_baseline_with_report().0;
        assert!(before.contains("first topic source (topic: r221-chain)"));

        topic
            .save(&[
                crate::docstore::Entry {
                    id: "S-001".into(),
                    title: "first topic source".into(),
                    status: "active".into(),
                    severity: None,
                    fields: vec![],
                },
                crate::docstore::Entry {
                    id: "S-002".into(),
                    title: "new topic source".into(),
                    status: "active".into(),
                    severity: None,
                    fields: vec![],
                },
            ])
            .unwrap();
        let after = snapshot.stable_system_baseline_with_report().0;
        assert!(after.contains("new topic source (topic: r221-chain)"));
        std::fs::remove_dir_all(root).ok();
    }
}
