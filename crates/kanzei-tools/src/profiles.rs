//! 开发与研究 Profile 组件；开发包含 dev-auto 与 dev-pair 两种模式。
//! 组件按当前 profile 决定贡献什么;权限规则是硬门禁的落点。

use anyhow::Context as _;
use std::sync::Arc;

use kanzei_harness::{
    fallible_source, refreshing_fallible_source, rule, source, AgentDef, AgentMode, Component,
    ContextRequirement, Effect, HarnessDraft, ProfileKind, ProfileScope, ResolveCtx,
};

use crate::tracker::TrackerTool;
use crate::work::WorkTool;
use kanzei_memory::docstore::{DECISIONS, DEFECTS, IDEAS, REQUIREMENTS};

#[cfg(test)]
mod context_tests;
mod development;
mod general;
mod isolated;
pub use isolated::IsolatedProfile;
mod policy;
mod project_indexes;
mod readonly;
mod research;
mod research_docs;
pub use development::{DevelopmentProfile, DEVELOPMENT_DEFERRED_TOOLS};
pub use general::{GeneralChatProfile, GeneralChatScope};
pub use readonly::ReadonlyProfile;

/// Missing project documents are valid; unreadable or invalid UTF-8 documents are not empty.
fn read_optional_context_file(path: &std::path::Path) -> anyhow::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("读取上下文文件 {}", path.display())),
    }
}

/// User-owned project rules are a separate, refreshable context source.
fn register_project_conventions(draft: &mut HarnessDraft) {
    draft.context.insert(
        "project/conventions",
        refreshing_fallible_source(
            "project/conventions",
            ContextRequirement::Required,
            |ctx: &ResolveCtx| {
                Ok(read_optional_context_file(
                    &ctx.project_root.join(crate::conventions::CONVENTIONS_REL),
                )?
                .filter(|text| !text.trim().is_empty())
                .map(|text| format!("<project-rules source=\".kanzei/project/conventions.md\">\n{text}\n</project-rules>")))
            },
        ),
    );
}

/// 提示词里被反引号点名的工具候选:取每段反引号内的第一个词。
///
/// 供装配合同测试核对 Agent 提示词引用的工具与当前工具目录。
///
/// 只认 ASCII 标识符形态,所以 `类型: 短期`、`@media ... {`、`{tool}` 这类反引号内容
/// 自然被滤掉;`req update <id> done` 取首词 `req`。
#[cfg(test)]
fn prompt_tool_mentions(prompt: &str) -> Vec<String> {
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

// 项目概览复用记忆模块的字符预算；已保存偏好由基础组件单独装配。
use kanzei_memory::memory::MEMORY_CONTEXT_BUDGET;

pub struct ResearchProfile;

impl Component for ResearchProfile {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        if ctx.profile != ProfileKind::Research {
            return Ok(());
        }
        research::register_tools(draft);
        research::configure_permissions(draft);

        research_docs::register(draft);
        register_project_conventions(draft);

        draft.agents.insert(
            "research",
            AgentDef {
                name: "research".into(),
                profile: ProfileScope::Research,
                model: "primary".into(),
                mode: AgentMode::Primary,
                // 0 = 无轮数上限(用户定调)。
                steps: 0,
                system: include_str!("profiles/research_system.md").into(),
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DevelopmentProfile, ResearchProfile};
    use kanzei_harness::{
        rule, ConfigComponent, Effect, Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx,
    };
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn dev_auto_system_prompt(tag: &str) -> String {
        let root = PathBuf::from(format!("C:/kanzei-{tag}-test"));
        let ctx = ResolveCtx {
            profile: ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevelopmentProfile)
            .add(ConfigComponent);
        harness
            .resolve(&ctx)
            .unwrap()
            .select_primary_agent(Some("dev-auto"))
            .unwrap()
            .system
            .clone()
    }

    #[test]
    fn core_contract_is_bounded_and_technology_neutral() {
        let system = dev_auto_system_prompt("compact-contract");
        assert!(system.chars().count() < 5000);
        for required in [
            "逐条对照验收",
            "真实调用方",
            "自主选择",
            "不以登记作为修复前提",
            "只提交自己负责",
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
            "Start 调用一次",
            "Resume 直接继续",
            "node.covers",
            "source_sequence",
            "awaiting_external",
            "backlog_maintenance",
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
        let render = |profile, full_test| {
            let mut config = KanzeiConfig::default();
            config.cadence.full_test = full_test;
            let ctx = ResolveCtx {
                profile,
                cwd: root.clone(),
                project_root: root.clone(),
                config: Arc::new(config),
            };
            let mut harness = Harness::default();
            harness
                .add(crate::BaseComponent)
                .add(DevelopmentProfile)
                .add(ConfigComponent);
            harness.resolve(&ctx).unwrap().system_baseline().unwrap()
        };
        let release = render(ProfileKind::DevAuto, FullTestCadence::ReleaseOnly);
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
        assert!(render(ProfileKind::DevAuto, FullTestCadence::EntryClose)
            .contains("全量测试：条目关闭前"));
        let paired = render(ProfileKind::DevPair, FullTestCadence::EveryCommit);
        assert!(paired.contains("Pair 按当前请求选择验证范围"));
        assert!(paired.contains("仅在任务包含这些操作时适用"));
        assert!(paired.contains("全量测试：每次提交前"));
        assert!(paired.contains("提交：按实际工作批次"));
        assert!(paired.contains("推送：每条目完成后"));
        assert!(!paired.contains("Auto 自主划分"));
        let unspecified = render(ProfileKind::DevAuto, FullTestCadence::EveryNBatches);
        assert!(unspecified.contains("full_test_batches 尚未设置"));
        assert!(
            !unspecified.contains("每 1"),
            "missing intervals cannot silently impose testing every batch"
        );
        std::fs::write(root.join("Cargo.toml"), "[workspace]").unwrap();
        std::fs::create_dir_all(root.join("crates/kanzei-app")).unwrap();
        std::fs::write(root.join("crates/kanzei-app/Cargo.toml"), "[package]").unwrap();
        let with_repository_marker = render(ProfileKind::DevAuto, FullTestCadence::EntryClose);
        assert!(with_repository_marker.contains("preserve-local-first"));
        for forbidden in ["cargo test", "cargo clippy", "verify.ps1", "compile_gate"] {
            assert!(!with_repository_marker.contains(forbidden), "{forbidden}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn user_saved_rules_refresh_and_proposals_stay_out_of_context() {
        use kanzei_harness::Tool;
        let root =
            std::env::temp_dir().join(format!("kz-conventions-refresh-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ResolveCtx {
            profile: ProfileKind::DevAuto,
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
            .add(DevelopmentProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        assert!(snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0
            .contains("user-rule-v1"));
        let first_hash = crate::normalized_text_hash("user-rule-v1");
        assert!(!tool.execute(json!({"action":"propose", "content":"unaccepted-draft", "expected_hash":first_hash}), &tools_ctx).await.is_error);
        assert!(!snapshot
            .system_baseline()
            .unwrap()
            .contains("unaccepted-draft"));
        crate::conventions::drafts::save_user(&root, "user-rule-v2", &first_hash, None).unwrap();
        let refreshed = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0;
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
            profile: ProfileKind::DevAuto,
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
            profile: ProfileKind::DevAuto,
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

    /// CLI 提示词引用的工具必须在对应档位的常驻或延迟目录中可达。
    #[test]
    fn 提示词点名的工具必须在同一条装配线上注册() {
        use super::prompt_tool_mentions;

        // 反引号里不是工具的词。每条都要说得出理由,不许为了让测试变绿往里塞。
        const NOT_TOOLS: &[&str] = &[
            // shell 命令(`node --check`),不是工具。
            "node",
            // 子代理入口:由 runner 在 SubagentRuntime 就位时 push task_spec,
            // 不进 draft.tools,由 runner 单独提供。
            "task",
        ];

        for profile in [
            ProfileKind::DevAuto,
            ProfileKind::DevPair,
            ProfileKind::Research,
        ] {
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
                .add(DevelopmentProfile)
                .add(super::ResearchProfile)
                .add(ConfigComponent);
            let snapshot = harness.resolve(&ctx).unwrap();
            let resident: Vec<String> = snapshot
                .resident_tools()
                .iter()
                .map(|tool| tool.name().to_string())
                .collect();
            let deferred: Vec<String> = snapshot
                .deferred_tools()
                .iter()
                .map(|tool| tool.name().to_string())
                .collect();

            for (name, agent) in snapshot.agents().iter() {
                for mentioned in prompt_tool_mentions(&agent.system) {
                    if NOT_TOOLS.contains(&mentioned.as_str()) || resident.contains(&mentioned) {
                        continue;
                    }
                    assert!(
                        deferred.contains(&mentioned),
                        "{profile:?} 档的 agent `{name}` 提示词点名了 `{mentioned}`,\
                         但它既非常驻工具也不在延迟目录中"
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
            profile: ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevelopmentProfile)
            .add(ConfigComponent);
        let baseline = harness.resolve(&ctx).unwrap().system_baseline().unwrap();

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

    #[test]
    fn project_conventions_load_only_saved_content_for_each_supported_profile() {
        let fixture = super::context_tests::Fixture::new("project-rules");
        for profile in [
            ProfileKind::DevAuto,
            ProfileKind::DevPair,
            ProfileKind::Research,
        ] {
            let baseline = fixture.snapshot(profile).system_baseline().unwrap();
            assert!(!baseline.contains("<project-rules"));
        }
        assert!(!fixture.0.join(".kanzei/project/conventions.md").exists());
        fixture.write(".kanzei/project/conventions.md", "   ");
        assert!(!fixture
            .snapshot(ProfileKind::DevPair)
            .system_baseline()
            .unwrap()
            .contains("<project-rules"));
        let rules = "## 项目特有规则\n- 保留用户指定的架构边界";
        fixture.write(".kanzei/project/conventions.md", rules);
        for profile in [
            ProfileKind::DevAuto,
            ProfileKind::DevPair,
            ProfileKind::Research,
        ] {
            let (baseline, reports) = fixture
                .snapshot(profile)
                .system_baseline_with_report()
                .unwrap();
            assert!(baseline.contains(&format!("<project-rules source=\".kanzei/project/conventions.md\">\n{rules}\n</project-rules>")));
            assert!(reports
                .iter()
                .any(|report| report.key == "project/conventions"));
            for forbidden in ["# 项目约定的来源", "compile_gate", "cargo clippy"] {
                assert!(!baseline.contains(forbidden), "{forbidden}");
            }
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
    fn design_documents_remain_available_without_a_resident_index() {
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
            profile: ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevelopmentProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();
        let baseline = snapshot.system_baseline().unwrap();
        assert!(snapshot
            .materialize_tools()
            .iter()
            .any(|tool| tool.name() == "architecture"));
        assert!(!baseline.contains("live.md"));
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
            profile: ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevelopmentProfile)
            .add(ConfigComponent);
        // resolve 本身就是覆盖校验:声明的 required_tool 没注册会直接 bail。
        let snapshot = harness.resolve(&ctx).unwrap();

        for (path, tool) in [
            (".kanzei/project/requirements.md", "req"),
            (".kanzei/project/defects-archive.md", "defect"),
            (".kanzei/project/ideas.md", "idea"),
            (".kanzei/project/decisions.md", "decision"),
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
            if snapshot.is_deferred(tool) {
                assert!(
                    hint.contains("tool_search") && hint.contains(&format!("select:{tool}")),
                    "对延迟工具 `{tool}` 的拒绝必须指向 tool_search select:{tool}: {hint}"
                );
            }
        }

        // 没有专用写入工具的资源族只报告能力缺失，不编造工具或追加工作。
        // conventions.md 自 D-235 起有了专用工具,这里用另一个仍无工具的
        // .kanzei/project 文件(如 notes.md)继续守「不得编造工具名」这条底线。
        let uncovered = kanzei_harness::permission::normalize_resource(".kanzei/project/notes.md");
        assert_eq!(snapshot.evaluate("write", &uncovered), Effect::Deny);
        let hint = snapshot.denial_hint("write", &uncovered);
        assert!(
            hint.contains("no owning write tool is registered"),
            "{hint}"
        );
        assert!(!hint.contains("defect add"), "{hint}");
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
        assert!(
            hint.contains("tool_search") && hint.contains("select:conventions"),
            "延迟 conventions 拒绝必须指向 tool_search select:conventions: {hint}"
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
            profile: ProfileKind::DevAuto,
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
            profile: ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(config),
        };
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevelopmentProfile)
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
        kanzei_memory::docstore::DocStore::open(&root, &kanzei_memory::docstore::SOURCES)
            .save(&[kanzei_memory::docstore::Entry {
                id: "S-901".into(),
                title: "legacy flat source".into(),
                status: "active".into(),
                severity: None,
                fields: vec![],
            }])
            .unwrap();
        kanzei_memory::docstore::DocStore::open_topic(
            &root,
            &kanzei_memory::docstore::SOURCES,
            "r221-chain",
        )
        .unwrap()
        .save(&[kanzei_memory::docstore::Entry {
            id: "S-001".into(),
            title: "topic source visible".into(),
            status: "active".into(),
            severity: None,
            fields: vec![],
        }])
        .unwrap();
        kanzei_memory::docstore::DocStore::open_topic(
            &root,
            &kanzei_memory::docstore::FINDINGS,
            "r221-chain",
        )
        .unwrap()
        .save(&[kanzei_memory::docstore::Entry {
            id: "F-001".into(),
            title: "topic finding visible".into(),
            status: "draft".into(),
            severity: None,
            fields: vec![],
        }])
        .unwrap();
        kanzei_memory::docstore::DocStore::open(&root, &kanzei_memory::docstore::REQUIREMENTS)
            .save(&[kanzei_memory::docstore::Entry {
                id: "R-901".into(),
                title: "研究回流需求".into(),
                status: "todo".into(),
                severity: None,
                fields: vec![],
            }])
            .unwrap();
        kanzei_memory::docstore::DocStore::open(&root, &kanzei_memory::docstore::DEFECTS)
            .save(&[kanzei_memory::docstore::Entry {
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
        let baseline = snapshot.system_baseline().unwrap();
        for required in [
            "R-901",
            "D-901",
            "B4 convention marker",
            "非终态记录导航",
            "revision:",
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
        assert!(!baseline.contains("legacy flat source") && !baseline.contains("S-901"));
        assert_eq!(
            snapshot.evaluate("prior_art", "read:validate"),
            Effect::Allow
        );
        assert_eq!(snapshot.evaluate("prior_art", "write:start"), Effect::Ask);
        kanzei_memory::docstore::DocStore::open_topic(
            &root,
            &kanzei_memory::docstore::SOURCES,
            "r221-chain",
        )
        .unwrap()
        .save(&[
            kanzei_memory::docstore::Entry {
                id: "S-001".into(),
                title: "topic source visible".into(),
                status: "active".into(),
                severity: None,
                fields: vec![],
            },
            kanzei_memory::docstore::Entry {
                id: "S-002".into(),
                title: "same session next step source".into(),
                status: "active".into(),
                severity: None,
                fields: vec![],
            },
        ])
        .unwrap();
        let refreshed = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0;
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
            .materialize_tools()
            .iter()
            .any(|tool| tool.name() == "research_plan"));
        let system = &snapshot
            .select_primary_agent(Some("research"))
            .unwrap()
            .system;
        assert!(!system.contains("research_plan") && !system.contains("research_workflow"));
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
            .add(DevelopmentProfile)
            .add(super::ResearchProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();

        assert_eq!(snapshot.evaluate("bash", "*"), Effect::Deny);
        for action in ["write", "edit", "insert"] {
            for file in ["loop.json", "plan.json", "budget.json"] {
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

    /// Readonly installs its primary definition and exposes observation permissions.
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
            .add(DevelopmentProfile)
            .add(super::ResearchProfile)
            .add(super::ReadonlyProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();

        let agent = snapshot.select_primary_agent(Some("readonly")).unwrap();
        assert_eq!(agent.name, "readonly");
        assert!(!agent.system.trim().is_empty());
        // 档位缺省选 agent:readonly 档位下默认选中 readonly(dev-auto/dev-pair/research 不匹配)。
        let default_agent = snapshot.select_primary_agent(None).unwrap();
        assert_eq!(default_agent.name, "readonly");

        // Read access remains available independently of prompt wording.
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

    /// Known effectful tools remain hard denied, including under custom config.
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
            .add(DevelopmentProfile)
            .add(super::ResearchProfile)
            .add(super::ReadonlyProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();

        // 写入、命令与专用副作用工具:整体 deny(工具会被摘除,模型看不见)。
        for action in [
            "write", "edit", "insert", "bash", "sandbox", "process", "browser", "latex", "plot",
        ] {
            assert_eq!(
                snapshot.evaluate(action, "*"),
                Effect::Deny,
                "{action} 在只读档位必须硬 deny"
            );
            let hint = snapshot.denial_hint(action, "anything");
            assert!(
                hint.contains("只读"),
                "{action} must explain its readonly boundary: {hint}"
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
        // Git observations are allowed; mixed-tool mutations are rejected at admission.
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
            "write", "edit", "insert", "bash", "sandbox", "process", "browser", "latex", "plot",
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
            "write", "edit", "insert", "bash", "sandbox", "process", "browser", "latex", "plot",
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
        // Runner-owned task availability still depends on runtime installation,
        // delegation mode and role eligibility; a wildcard row is not authorization.
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
        let topic = kanzei_memory::docstore::DocStore::open_topic(
            &root,
            &kanzei_memory::docstore::SOURCES,
            "r221-chain",
        )
        .unwrap();
        topic
            .save(&[kanzei_memory::docstore::Entry {
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
        let before = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0;
        assert!(
            before.contains("[r221-chain] S-001 [active] first topic source"),
            "baseline was: {before}"
        );

        topic
            .save(&[
                kanzei_memory::docstore::Entry {
                    id: "S-001".into(),
                    title: "first topic source".into(),
                    status: "active".into(),
                    severity: None,
                    fields: vec![],
                },
                kanzei_memory::docstore::Entry {
                    id: "S-002".into(),
                    title: "new topic source".into(),
                    status: "active".into(),
                    severity: None,
                    fields: vec![],
                },
            ])
            .unwrap();
        let after = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0;
        assert!(after.contains("[r221-chain] S-002 [active] new topic source"));
        std::fs::remove_dir_all(root).ok();
    }
}
