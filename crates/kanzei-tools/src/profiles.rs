//! 开发与研究 Profile 组件；开发包含 dev-auto 与 dev-pair 两种模式。
//! 组件按当前 profile 决定贡献什么;权限规则是硬门禁的落点。

use std::sync::Arc;

use kanzei_harness::{
    rule, source, AgentDef, AgentMode, Component, Effect, HarnessDraft, ProfileScope, ResolveCtx,
};

#[cfg(test)]
mod context_tests;
mod development;
mod general;
mod isolated;
pub use isolated::IsolatedProfile;
mod policy;
mod readonly;
pub use development::{DevelopmentProfile, DEVELOPMENT_DEFERRED_TOOLS};
pub use general::{GeneralChatProfile, GeneralChatScope};
pub use readonly::ReadonlyProfile;

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

#[cfg(test)]
mod tests {
    use super::DevelopmentProfile;
    use kanzei_harness::{
        rule, ConfigComponent, Effect, Harness, KanzeiConfig, ProfileKind, ResolveCtx,
    };
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

    /// D-173:硬 deny 与专用工具必须闭合。
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
        let snapshot = harness.resolve(&ctx).unwrap();

        let normalized = kanzei_harness::permission::normalize_resource(".kanzei/memory/M-001-x.md");
        assert_eq!(snapshot.evaluate("write", &normalized), Effect::Deny);
        let hint = snapshot.denial_hint("write", &normalized);
        assert!(
            hint.contains("`memory_note`"),
            "memory 路径的指引没点名 memory_note: {hint}"
        );
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
            .add(super::ReadonlyProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&ctx).unwrap();

        // 写入、命令与专用副作用工具:整体 deny(工具会被摘除,模型看不见)。
        for action in [
            "write", "edit", "insert", "bash", "sandbox", "process", "browser",
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
            "write", "edit", "insert", "bash", "sandbox", "process", "browser",
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
            "write", "edit", "insert", "bash", "sandbox", "process", "browser",
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
}
