//! 双模式 Profile 组件:dev(需求/缺陷)与 research(来源/发现)。
//! 组件按当前 profile 决定贡献什么;权限规则是硬门禁的落点。

use std::sync::Arc;

use kanzei_harness::{
    refreshing_source, rule, source, AgentDef, AgentMode, Component, Effect, HarnessDraft,
    ProfileKind, ProfileScope, ResolveCtx,
};

use crate::docstore::{DocStore, DECISIONS, DEFECTS, IDEAS, REQUIREMENTS};
use crate::tracker::TrackerTool;
use crate::work::WorkTool;

mod dev;
mod policy;
mod readonly;
mod research;
pub use dev::{DevProfile, DEV_DEFERRED_TOOLS};
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
            refreshing_source("research/docs", |ctx: &ResolveCtx| {
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
                system: "You are the research agent. If the current topic has an AUTO research workflow, call `research_workflow get` and follow its persisted stage: the user has already authorized its survey budget, so reuse the approved plan and use budget_get rather than creating another plan or setting another budget. Publish the evidence-backed map and wait for the user to choose a direction; then prepare_compute, run the MVP and interpret actual metrics, define the full paired-seed baseline/main/ablation/robustness matrix, execute it, submit_analysis, paper_init, submit_paper, review_paper and compile_paper. Continue until a real PDF and delivery manifest exist; MVP completion is not the end of the research. Use the AUTO workflow paper actions for AUTO topics; the research_write outline/section loop below is for manual research. Stop when the workflow waits, pauses or completes. Otherwise, before searching, use `research_plan` to create an explicit plan tree, record clarification questions, and request user approval; never approve or execute an unapproved plan. After approval, use the `research_loop` tool with start/resume actions to drive the bounded search-read-reflect loop. For each isolated subtask call begin_search first; every websearch/webfetch call MUST pass that topic and returned task_id, then pass the same task_id to add_evidence—the loop gate is mechanical. Before returning any result to the main context, compress it via research_loop add_evidence with relevance, source_ids, and a sourced summary—never pass raw webpage or tool output into the loop. Use reflect to record knowledge gaps and decide whether another round is needed; write findings only through add_finding with source refs. When writing a report, call the `research_write` tool: write_outline first, then write_section once per outline section, assemble_paper for heavy topics, and compile_paper through the LaTeX channel; use repair_paper only after a failed compile and preserve its diagnostics. For manual research use `research_verify` budget_set before starting a loop; AUTO research uses the user's workflow budget. After writing, use verify_claims to mechanically check every FACT source and evidence anchor, and use capture_source for complete literature正文 rather than trusting abstract/要点 fields. Before cross-checking claims, use the `research_index` tool: index_build/index_resume creates or resumes the topic Tantivy index, search uses the same interface for literature and code, and symbols mode performs code symbol reverse lookup. Record every consulted source \
                         (`source add`) and register conclusions as findings citing those \
                         sources. Every conclusion must state its code or literature domain, \
                         V0-V3 level, evidence anchor, and literature evidence depth; use V \
                         evidence, never E0-E4 verification levels, and cap abstract-only \
                         literature evidence at V1. Use `memory_search` for project memory and \
                         `memory_note` for durable research conclusions; do not use the historical \
                         `.kanzei/research/memory.md`. The final report goes to \
                         .kanzei/research/<topic>/report.md."
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

    fn dev_system_prompt(tag: &str) -> String {
        let root = PathBuf::from(format!("C:/kanzei-{tag}-test"));
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
        harness
            .resolve(&ctx)
            .unwrap()
            .select_agent(Some("dev"))
            .unwrap()
            .system
            .clone()
    }

    #[test]
    fn core_contract_is_bounded_and_technology_neutral() {
        let system = dev_system_prompt("compact-contract");
        assert!(system.chars().count() < 5000);
        for required in [
            "逐条对照验收",
            "真实调用方",
            "Start 调用一次",
            "Resume 直接继续",
            "只提交自己负责",
            "独立只读查询合在同一步",
        ] {
            assert!(system.contains(required), "missing {required}");
        }
        for inappropriate in [
            "cargo test",
            "verify.ps1",
            "ui_dom",
            "V0-V3",
            "old_string",
            "Design freeze",
            "<ID> B<k>",
        ] {
            assert!(
                !system.contains(inappropriate),
                "unconditional {inappropriate}"
            );
        }
    }

    #[test]
    fn resolved_context_keeps_project_scope_and_single_effective_cadence() {
        use kanzei_harness::config::FullTestCadence;
        let root = std::env::temp_dir().join(format!("kz-policy-matrix-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(root.join("pubspec.yaml"), "name: reader").unwrap();
        std::fs::write(
            root.join(".kanzei/project/conventions.md"),
            "用户规则: preserve-local-first",
        )
        .unwrap();
        let render = |full_test| {
            let mut config = KanzeiConfig::default();
            config.cadence.full_test = full_test;
            let ctx = ResolveCtx {
                profile: ProfileKind::Dev,
                cwd: root.clone(),
                project_root: root.clone(),
                config: Arc::new(config),
            };
            let mut harness = Harness::default();
            harness
                .add(crate::BaseComponent)
                .add(DevProfile)
                .add(ConfigComponent);
            harness.resolve(&ctx).unwrap().system_baseline()
        };
        let release = render(FullTestCadence::ReleaseOnly);
        assert_eq!(
            release.matches("<effective-verification-policy>").count(),
            1
        );
        assert!(release.contains("仅发布前"));
        assert!(!release.contains("关闭前一次"));
        assert!(release.contains("preserve-local-first"));
        for forbidden in [
            "cargo test",
            "clippy",
            "verify.ps1",
            "ui_dom",
            "V0-V3",
            "<project-docs>",
        ] {
            assert!(!release.contains(forbidden), "{forbidden}");
        }
        assert!(render(FullTestCadence::EntryClose).contains("中/大条目关闭前一次"));
        std::fs::write(root.join("Cargo.toml"), "[workspace]").unwrap();
        assert!(
            !render(FullTestCadence::EntryClose).contains("clippy 四处分工"),
            "generic Rust is not the Kanzei repository"
        );
        std::fs::create_dir_all(root.join("crates/kanzei-app")).unwrap();
        std::fs::write(root.join("crates/kanzei-app/Cargo.toml"), "[package]").unwrap();
        assert!(render(FullTestCadence::EntryClose).contains("clippy 四处分工"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn user_saved_rules_refresh_and_proposals_stay_out_of_context() {
        use kanzei_harness::Tool;
        let root =
            std::env::temp_dir().join(format!("kz-conventions-refresh-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(KanzeiConfig::default()),
        };
        let tools_ctx = ToolCtx::new(root.clone(), root.clone());
        let tool = crate::conventions::ConventionsTool;
        assert!(
            !tool
                .execute(
                    json!({"action":"create", "content":"user-rule-v1"}),
                    &tools_ctx
                )
                .await
                .is_error
        );
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        assert!(snapshot
            .refreshable_system_baseline_with_report()
            .0
            .contains("user-rule-v1"));
        let first_hash = crate::architecture::content_hash("user-rule-v1");
        assert!(!tool.execute(json!({"action":"propose", "content":"unaccepted-draft", "expected_hash":first_hash}), &tools_ctx).await.is_error);
        assert!(!snapshot.system_baseline().contains("unaccepted-draft"));
        crate::conventions::drafts::save_user(&root, "user-rule-v2", &first_hash, None).unwrap();
        let refreshed = snapshot.refreshable_system_baseline_with_report().0;
        assert!(refreshed.contains("user-rule-v2"));
        assert!(!refreshed.contains("user-rule-v1"));
        std::fs::remove_dir_all(root).unwrap();
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
        assert!(
            names.contains(&"prior_art"),
            "dev 档必须有 prior_art: {names:?}"
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
        harness
            .add(crate::BaseComponent)
            .add(DevProfile)
            .add(ConfigComponent);
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
            harness
                .add(crate::BaseComponent)
                .add(DevProfile)
                .add(ConfigComponent);
            harness.resolve(&ctx).unwrap().system_baseline()
        };

        // ① 无项目文件:引擎默认模板必须全量注入(新项目零配置也拿到完整约束)。
        // R-191 验收②:模板生成测试断言关键节存在(§1.1 阻塞口径 / §1.3 批次 /
        // §1.4 节奏 / §1.25 验收证据)——四个关键节必须全部出现在注入后的上下文。
        let bare = PathBuf::from("C:/kanzei-r191-default-test");
        let baseline = baseline_of(&bare);
        assert!(baseline.contains(kanzei_harness::DEFAULT_CONVENTIONS.trim()));
        // §1.4a 提交前 cargo 门禁(D-264)只进 Cargo 工程(UI2-0926 #13):无 Cargo.toml 的项目不该收到,
        // Cargo 工程收到由 dev_conventions_cargo_条款只注入_cargo_工程 守护。
        assert!(
            !baseline.contains("compile_gate"),
            "非 Cargo 项目不该收到 cargo 专属的提交门禁条款"
        );

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
        let default_pos = baseline.find("# 项目约定的来源");
        let project_pos = baseline.find("项目特有规则");
        assert!(
            default_pos.is_some() && project_pos.is_some() && default_pos < project_pos,
            "拼接顺序错误:通用规则必须在项目特有规则之前"
        );
        std::fs::remove_dir_all(root).unwrap();
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
        harness
            .add(crate::BaseComponent)
            .add(DevProfile)
            .add(ConfigComponent);
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
        // UI2-0926 #7:架构图 lint 是只读动作,默认放行(agent 改完图直接自查)。
        assert_eq!(snapshot.evaluate("architecture", "diagrams"), Effect::Allow);
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
        harness
            .add(crate::BaseComponent)
            .add(DevProfile)
            .add(ConfigComponent);
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
        crate::docstore::DocStore::open(&root, &crate::docstore::SOURCES)
            .save(&[crate::docstore::Entry {
                id: "S-901".into(),
                title: "legacy flat source".into(),
                status: "active".into(),
                severity: None,
                fields: vec![],
            }])
            .unwrap();
        crate::docstore::DocStore::open_topic(&root, &crate::docstore::SOURCES, "r221-chain")
            .unwrap()
            .save(&[crate::docstore::Entry {
                id: "S-001".into(),
                title: "topic source visible".into(),
                status: "active".into(),
                severity: None,
                fields: vec![],
            }])
            .unwrap();
        crate::docstore::DocStore::open_topic(&root, &crate::docstore::FINDINGS, "r221-chain")
            .unwrap()
            .save(&[crate::docstore::Entry {
                id: "F-001".into(),
                title: "topic finding visible".into(),
                status: "draft".into(),
                severity: None,
                fields: vec![],
            }])
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
            "[legacy-flat] S-901",
            "legacy flat source",
            "[r221-chain] S-001",
            "topic source visible",
            "[r221-chain] F-001",
            "topic finding visible",
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
        assert_eq!(
            snapshot.evaluate("prior_art", "read:validate"),
            Effect::Allow
        );
        assert_eq!(snapshot.evaluate("prior_art", "write:start"), Effect::Ask);
        crate::docstore::DocStore::open_topic(&root, &crate::docstore::SOURCES, "r221-chain")
            .unwrap()
            .save(&[
                crate::docstore::Entry {
                    id: "S-001".into(),
                    title: "topic source visible".into(),
                    status: "active".into(),
                    severity: None,
                    fields: vec![],
                },
                crate::docstore::Entry {
                    id: "S-002".into(),
                    title: "same session next step source".into(),
                    status: "active".into(),
                    severity: None,
                    fields: vec![],
                },
            ])
            .unwrap();
        let refreshed = snapshot.refreshable_system_baseline_with_report().0;
        assert!(
            refreshed.contains("same session next step source"),
            "research/docs 必须逐模型步骤刷新，刚写入的 topic 来源要在下一步可见"
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
        for name in ["websearch", "webfetch"] {
            let tool = snapshot
                .materialize_tools()
                .into_iter()
                .find(|tool| tool.name() == name)
                .unwrap_or_else(|| panic!("research 档缺少 {name} tool"));
            let required = tool
                .input_schema()
                .pointer("/required")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            assert!(required.contains(&serde_json::json!("topic")));
            assert!(required.contains(&serde_json::json!("task_id")));
        }
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

    /// 研究规则只驻留 research，通用开发不背负该契约。
    #[test]
    fn research_evidence_prompt_uses_v_table_and_literature_depth() {
        let dev = dev_system_prompt("r221-v-table");
        assert!(
            !dev.contains("V0-V3"),
            "research policy must stay out of generic dev"
        );

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
        for action in ["write", "edit", "insert"] {
            for file in ["workflow.json", "loop.json", "plan.json", "budget.json"] {
                assert_eq!(
                    snapshot.evaluate(action, &format!(".kanzei/research/demo/{file}")),
                    Effect::Deny
                );
                if cfg!(windows) {
                    assert_eq!(
                        snapshot.evaluate(
                            action,
                            &format!(".kanzei/research/demo/{}", file.to_uppercase())
                        ),
                        Effect::Deny
                    );
                }
            }
            assert_eq!(
                snapshot.evaluate(action, ".kanzei/research/demo/protocol.md"),
                Effect::Allow
            );
        }
        let bash_hint = snapshot.denial_hint("bash", "anything");
        assert!(
            bash_hint.contains("latex") && bash_hint.contains("plot"),
            "{bash_hint}"
        );
        for subcommand in ["status", "diff", "log"] {
            assert_eq!(snapshot.evaluate("git", subcommand), Effect::Allow);
        }
        // UI2-0926 #13 复核:git 工具新增的 init(建库)同样硬拒绝。
        for subcommand in ["stage", "commit", "merge_ff", "finalize", "init"] {
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

    /// D-663/R-102 批2:readonly 档位权限强制——写入、命令与专用副作用工具硬 deny 且带替代指引。
    #[test]
    fn readonly_profile_hard_denies_writes_commands_and_side_effect_tools() {
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

        // 写入、命令与专用副作用工具:整体 deny(工具会被摘除,模型看不见)。
        for action in [
            "write", "edit", "insert", "bash", "process", "browser", "latex", "plot",
        ] {
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
        // UI2-0926 #13 复核:建库(git init)不落到默认 ask,只读档位硬拒绝。
        assert_eq!(snapshot.evaluate("git", "init"), Effect::Deny);
        assert!(snapshot.denial_hint("git", "init").contains("不建仓库"));
        // 工具物化:所有写入、命令与专用副作用工具从工具表摘除,模型根本拿不到。
        let names: Vec<&str> = snapshot
            .materialize_tools()
            .iter()
            .map(|t| t.name())
            .collect();
        for gone in [
            "write", "edit", "insert", "bash", "process", "browser", "latex", "plot",
        ] {
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
        // 写入、命令与专用副作用工具:Deny 且 fully_denied(工具整体摘除)。
        for action in [
            "write", "edit", "insert", "bash", "process", "browser", "latex", "plot",
        ] {
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
        let before = snapshot.refreshable_system_baseline_with_report().0;
        assert!(
            before.contains("[r221-chain] S-001 [active] first topic source"),
            "baseline was: {before}"
        );

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
        let after = snapshot.refreshable_system_baseline_with_report().0;
        assert!(after.contains("[r221-chain] S-002 [active] new topic source"));
        std::fs::remove_dir_all(root).ok();
    }
}

/// D-662:模型可见工具面的预算门禁。
///
/// # 为什么是「计数」而不是「合并工具」
///
/// 外部评估提的是「工具越多,错误选择概率越高」。但把 req/defect/idea/decision
/// 合成一个 `tracker(kind, ...)` 并**不减少模型要做的判断**——「这是需求还是缺陷」
/// 本来就得判,合并只是把它从工具名挪到参数里,同时还削弱了每个工具 schema
/// 精确描述自己合法动作的能力。真正的问题是这个面**没人盯着,只会涨**。
///
/// 所以第一道防线是把它变成一个有人负责的数字:加工具必须显式抬预算,
/// 顺带在 review 时被看见一次。这是 D-662 的机制修复,不是它的最终解法。
#[cfg(test)]
mod tool_surface_budget {
    use kanzei_harness::{ConfigComponent, KanzeiConfig, ProfileKind, ResolveCtx};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// D-662/R-364 双门禁按工具数计,不是 schema 字符数。
    /// CLI resident=19 个注册工具 + core 单独追加的 task; desktop 再加 collaboration_status。
    const DEV_RESIDENT_TOOL_BUDGET: usize = 20;
    const DEV_DEFERRED_TOOL_BUDGET: usize = 12;

    /// readonly 档:只读分析,面应当明显更小。
    ///
    /// **当前 15 不是「合理值」,是「现状值」**——本预算测试立起来时发现 plot/latex/
    /// process/browser 都还在这个面里,且只被判 Ask 而不是硬拒(D-663)。
    /// D-663 修完后这个数应当降到 11 左右,届时把预算一起收紧。
    const READONLY_TOOL_BUDGET: usize = 15;

    fn visible_tools(profile: ProfileKind) -> Vec<&'static str> {
        let root = PathBuf::from("C:/kanzei-d662-budget");
        let ctx = ResolveCtx {
            profile,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = crate::run::build_harness(
            |h| {
                h.add(crate::ReadonlyProfile);
            },
            |_| {},
        );
        harness.add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let mut names: Vec<&'static str> = snapshot
            .materialize_tools()
            .iter()
            .map(|tool| tool.name())
            .collect();
        names.sort_unstable();
        names
    }

    fn visible_layer_counts(profile: ProfileKind) -> (usize, usize) {
        let root = PathBuf::from("C:/kanzei-d662-budget");
        let ctx = ResolveCtx {
            profile,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = crate::run::build_harness(
            |h| {
                h.add(crate::ReadonlyProfile);
            },
            |_| {},
        );
        harness.add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        (
            snapshot.resident_tools().len(),
            snapshot.deferred_tools().len(),
        )
    }

    fn materialized_tool_specs(profile: ProfileKind) -> Vec<kanzei_llm::ToolSpec> {
        let root = PathBuf::from("C:/kanzei-r364-b1-schema-bill");
        let ctx = ResolveCtx {
            profile,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = crate::run::build_harness(
            |h| {
                h.add(crate::ReadonlyProfile);
            },
            |_| {},
        );
        harness.add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        snapshot
            .materialize_tools()
            .iter()
            .map(|tool| kanzei_llm::ToolSpec {
                name: tool.name().to_string(),
                description: tool.description(),
                input_schema: tool.input_schema(),
            })
            .collect()
    }

    #[test]
    fn 逐工具schema字符账单() {
        let resident = [
            "read",
            "write",
            "edit",
            "insert",
            "bash",
            "glob",
            "grep",
            "symbols",
            "git",
            "req",
            "defect",
            "work",
            "test_record",
            "memory_search",
            "memory_note",
            "question",
            "task",
            "websearch",
            "webfetch",
            "tool_search",
        ];
        let deferred = [
            "process",
            "files",
            "incident",
            "conventions",
            "architecture",
            "prior_art",
            "browser",
            "latex",
            "plot",
            "idea",
            "decision",
            "memory_stats",
        ];
        let specs = materialized_tool_specs(ProfileKind::Dev);
        let expected_materialized: Vec<&str> = resident
            .iter()
            .copied()
            .chain(deferred.iter().copied())
            .filter(|name| *name != "task")
            .collect();
        for expected in expected_materialized {
            assert!(
                specs.iter().any(|spec| spec.name.as_str() == expected),
                "§5.1 当前应物化工具缺失: {expected}"
            );
        }

        let mut rows: Vec<(&str, usize, usize, &str)> = specs
            .iter()
            .map(|spec| {
                let chars = spec.char_len();
                let bytes =
                    spec.name.len() + spec.description.len() + spec.input_schema.to_string().len();
                let layer = if resident.contains(&spec.name.as_str()) {
                    "常驻"
                } else if deferred.contains(&spec.name.as_str()) {
                    "延迟"
                } else {
                    "未分类"
                };
                (spec.name.as_str(), chars, bytes / 4, layer)
            })
            .collect();
        rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
        eprintln!("CLI Dev schema 字符账单:工具\t字符\tbytes/4 估算 tokens\t分层");
        for (name, chars, token_estimate, layer) in &rows {
            eprintln!("{name}\t{chars}\t{token_estimate}\t{layer}");
        }
        let resident_chars: usize = rows
            .iter()
            .filter(|row| row.3 == "常驻")
            .map(|row| row.1)
            .sum();
        let deferred_chars: usize = rows
            .iter()
            .filter(|row| row.3 == "延迟")
            .map(|row| row.1)
            .sum();
        let unclassified_chars: usize = rows
            .iter()
            .filter(|row| row.3 == "未分类")
            .map(|row| row.1)
            .sum();
        let row_sum: usize = rows.iter().map(|row| row.1).sum();
        let spec_sum: usize = specs.iter().map(kanzei_llm::ToolSpec::char_len).sum();
        assert_eq!(row_sum, spec_sum, "账单总数必须等于逐项之和");
        let deferred_ratio = deferred_chars as f64 / spec_sum as f64 * 100.0;
        eprintln!(
            "CLI Dev totals: resident={resident_chars}, deferred={deferred_chars} ({deferred_ratio:.2}%), unclassified={unclassified_chars}, all={spec_sum}; core task_spec(未计入)"
        );
    }

    #[test]
    fn dev档常驻与延迟目录分别符合预算() {
        let names = visible_tools(ProfileKind::Dev);
        let (resident, deferred) = visible_layer_counts(ProfileKind::Dev);
        assert!(
            names.contains(&"tool_search"),
            "tool_search 必须在 Dev 常驻表中"
        );
        assert_eq!(
            resident + 1,
            DEV_RESIDENT_TOOL_BUDGET,
            "resident_tools 的 19 项之外由 core 追加 task_spec"
        );
        assert_eq!(deferred, DEV_DEFERRED_TOOL_BUDGET);
    }

    #[test]
    fn readonly档工具面明显更小() {
        let names = visible_tools(ProfileKind::Readonly);
        assert!(
            names.len() <= READONLY_TOOL_BUDGET,
            "readonly 档可见工具 {} 个,超出预算 {READONLY_TOOL_BUDGET};当前清单: {names:?}",
            names.len()
        );
        // 档位契约的正面断言:写与命令族必须整体摘除(fully_denied),不是判 Ask。
        // 这几条现在是**通过**的;D-663 记的是 plot/latex/process/browser 没进这个名单。
        for denied in ["write", "edit", "insert", "bash"] {
            assert!(
                !names.contains(&denied),
                "readonly 档不得看见 {denied}(应被 action_fully_denied 整体摘除)"
            );
        }
    }

    /// 记忆写路径必须**不在**主 agent 的工具面里(写读分离,R-105)。
    ///
    /// 这条是 D-662 的护栏:memory 一族在仓库里有 10 个工具,主面只该看见 3 个。
    /// 哪天有人图省事把 MemoryManagerComponent 挂进主装配,主面会一次涨 7 个,
    /// 而且**记忆的唯一写路径**这条设计约束会同时失效——两件事一条测试拦住。
    #[test]
    fn 记忆写工具不得进入主agent工具面() {
        let names = visible_tools(ProfileKind::Dev);
        for leaked in [
            "memory_add",
            "memory_promote",
            "memory_update",
            "memory_merge",
            "memory_stale",
            "memory_inbox_clear",
        ] {
            assert!(
                !names.contains(&leaked),
                "{leaked} 泄漏进主 agent 工具面:记忆的唯一写路径是 memory-manager 子代理"
            );
        }
        for expected in ["memory_note", "memory_search", "memory_stats"] {
            assert!(names.contains(&expected), "主 agent 应保留 {expected}");
        }
    }
}
