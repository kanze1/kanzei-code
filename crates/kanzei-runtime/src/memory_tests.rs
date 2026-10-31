use super::*;
use kanzei_harness::ProfileKind;

#[test]
fn social_channels_have_a_persistent_workspace_without_host_or_deferred_capabilities() {
    kanzei_base::test_support::isolated_test(
        module_path!(),
        "social_channels_have_a_persistent_workspace_without_host_or_deferred_capabilities",
        || {
            let directory = kanzei_base::test_support::TestDir::new("kz-social-scope-");
            let home = directory.join("home");
            std::fs::create_dir_all(home.join("agents")).unwrap();
            std::env::set_var("KANZEI_HOME", &home);
            std::fs::write(
                home.join("agents/dev-pair.md"),
                "---\nname: dev-pair\n---\nUNSAFE_CUSTOM_AGENT_USE_HOST_BASH",
            )
            .unwrap();
            let personal = kanzei_harness::social_conversation_root().unwrap();
            let friend = kanzei_harness::QqConversation::new(
                "100",
                kanzei_harness::QqPeerKind::Friend,
                "200",
            )
            .unwrap()
            .root()
            .unwrap();
            let group = kanzei_harness::QqConversation::new(
                "100",
                kanzei_harness::QqPeerKind::Group,
                "300",
            )
            .unwrap()
            .root()
            .unwrap();
            let other = kanzei_harness::QqConversation::new(
                "100",
                kanzei_harness::QqPeerKind::Group,
                "301",
            )
            .unwrap()
            .root()
            .unwrap();
            let bili = kanzei_harness::bilibili_conversation_root("1000").unwrap();
            for root in [&personal, &friend, &bili] {
                assert_eq!(
                    kanzei_harness::social_workspace_root(root),
                    Some(home.join("social-workspaces/personal"))
                );
                assert_eq!(
                    kanzei_harness::memory_owner_root(root),
                    home.join("conversations/general")
                );
            }
            assert_ne!(
                kanzei_harness::social_workspace_root(&group),
                kanzei_harness::social_workspace_root(&other)
            );
            assert_ne!(
                kanzei_harness::social_workspace_root(&group),
                kanzei_harness::social_workspace_root(&personal)
            );
            assert_eq!(kanzei_harness::memory_owner_root(&group), group);
            assert!(
                kanzei_harness::social_workspace_root(&home.join("conversations/general"))
                    .is_none()
            );
            let mut config = KanzeiConfig::default();
            config.fill_defaults();
            config.permissions.rules.push(kanzei_harness::rule(
                "*",
                "*",
                kanzei_harness::Effect::Allow,
            ));
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let resolved = config.resolve_model("primary").unwrap();
            let route = Route::openai_at("http://127.0.0.1:1/v1", None);
            for root in [&personal, &friend, &group, &bili] {
                std::fs::create_dir_all(root).unwrap();
                for profile in [
                    ProfileKind::DevAuto,
                    ProfileKind::DevPair,
                    ProfileKind::Isolated,
                ] {
                    let snapshot = build_harness(profile, |_| {}, |_| {})
                        .resolve(&ResolveCtx {
                            profile,
                            cwd: root.clone(),
                            project_root: root.clone(),
                            config: Arc::new(config.clone()),
                        })
                        .unwrap();
                    let names: std::collections::BTreeSet<_> = snapshot
                        .materialize_tools()
                        .iter()
                        .map(|t| t.name())
                        .collect();
                    assert_eq!(
                        names,
                        [
                            "sandbox",
                            "websearch",
                            "webfetch",
                            "memory_search",
                            "memory_note"
                        ]
                        .into_iter()
                        .collect()
                    );
                    assert!(snapshot.deferred_tools().is_empty());
                    assert!(snapshot.agent_sources().is_empty());
                    let agent = snapshot
                        .select_primary_agent(Some(profile.as_str()))
                        .unwrap();
                    assert!(!agent.system.contains("UNSAFE_CUSTOM_AGENT"));
                    // The task runtime has its own snapshot. It must not recreate
                    // host-capable children after the primary capability projection.
                    let rctx = ResolveCtx {
                        profile,
                        cwd: root.clone(),
                        project_root: root.clone(),
                        config: Arc::new(config.clone()),
                    };
                    assert!(runtime
                        .block_on(build_subagent_runtime(
                            &rctx,
                            &config,
                            &ProxyConfig::Disabled,
                            &resolved,
                            &route,
                            None,
                            None,
                            None,
                            None
                        ))
                        .unwrap()
                        .is_none());
                }
            }
        },
    );
}

#[test]
fn readonly_composition_preserves_the_boundary_with_custom_instructions_and_broad_rules() {
    kanzei_base::test_support::isolated_test(
        module_path!(),
        "readonly_composition_preserves_the_boundary_with_custom_instructions_and_broad_rules",
        || {
            let directory = kanzei_base::test_support::TestDir::new("kz-readonly-composition-");
            let home = directory.join("home");
            let root = directory.join("project");
            std::fs::create_dir_all(home.join("agents")).unwrap();
            std::fs::create_dir_all(&root).unwrap();
            std::env::set_var("KANZEI_HOME", &home);
            std::fs::write(
                home.join("agents/readonly.md"),
                "---\nname: readonly\n---\nCUSTOM_READONLY_INSTRUCTIONS",
            )
            .unwrap();
            let mut config = KanzeiConfig::default();
            config.fill_defaults();
            config.permissions.rules.push(kanzei_harness::rule(
                "*",
                "*",
                kanzei_harness::Effect::Allow,
            ));
            let resolved = config.resolve_model("primary").unwrap();
            let runner = build_runner_config(
                &resolved,
                &config,
                None,
                &root,
                ProfileKind::Readonly,
                kanzei_core::AskPolicy::AutoAllow,
                None,
            );
            assert_eq!(
                runner.execution_policy,
                kanzei_harness::ExecutionPolicy::ReadOnly
            );
            let snapshot = build_harness(ProfileKind::Readonly, |_| {}, |_| {})
                .resolve(&ResolveCtx {
                    profile: ProfileKind::Readonly,
                    cwd: root.clone(),
                    project_root: root,
                    config: Arc::new(config),
                })
                .unwrap();
            assert_eq!(
                snapshot.select_primary_agent(None).unwrap().system,
                "CUSTOM_READONLY_INSTRUCTIONS"
            );
            let tools = snapshot.materialize_tools();
            assert!(tools.iter().all(|tool| tool.read_only_support()
                != kanzei_harness::tool::ReadOnlySupport::Unsupported));
            for name in ["read", "webfetch", "websearch", "skill_read", "skill_list"] {
                assert!(
                    tools.iter().any(|tool| tool.name() == name),
                    "missing observation: {name}"
                );
            }
            for name in ["write", "bash", "sandbox", "process", "browser"] {
                assert!(
                    !tools.iter().any(|tool| tool.name() == name),
                    "mutable capability exposed: {name}"
                );
            }
        },
    );
}

#[test]
fn isolated_memory_preparation_does_not_read_project_sources_or_embedding_configuration() {
    let directory = kanzei_base::test_support::TestDir::new("isolated-memory-context");
    let root = directory.join("unused-project");
    let mut config = KanzeiConfig::default();
    config.fill_defaults();
    config.embeddings.provider = Some("unknown-provider".into());
    config.embeddings.model = Some("unknown-model".into());
    let resolved = config.resolve_model("primary").unwrap();
    let runner = build_runner_config(
        &resolved,
        &config,
        None,
        &root,
        ProfileKind::Isolated,
        kanzei_core::AskPolicy::Interactive,
        None,
    );
    assert!(runner.recall.is_none());
    assert!(
        task_memory_hints(&config, ProfileKind::Isolated, &root, "query", None)
            .unwrap()
            .is_none()
    );
    assert!(
        !root.exists(),
        "isolated preparation cannot initialize project memory"
    );
    assert!(
        task_memory_hints(&config, ProfileKind::DevAuto, &root, "query", None).is_err(),
        "explicit configuration errors cannot silently disable embeddings"
    );
}

#[test]
fn projectless_templates_preserve_global_overrides_and_isolated_sources_stay_empty() {
    kanzei_base::test_support::isolated_test(
        module_path!(),
        "projectless_templates_preserve_global_overrides_and_isolated_sources_stay_empty",
        || {
            let directory = kanzei_base::test_support::TestDir::new("kz-general-agent-order-");
            let home = directory.join("home");
            let root = home.join("conversations/general");
            std::fs::create_dir_all(root.join(".kanzei/agents")).unwrap();
            std::fs::create_dir_all(home.join("agents")).unwrap();
            std::env::set_var("KANZEI_HOME", &home);
            for name in ["dev-auto", "dev-pair"] {
                std::fs::write(
                    home.join(format!("agents/{name}.md")),
                    format!("---\nname: {name}\n---\nGLOBAL_CUSTOM_{name}"),
                )
                .unwrap();
            }
            std::fs::write(
                root.join(".kanzei/agents/ignored.md"),
                "PROJECT_SOURCE_MUST_NOT_LOAD",
            )
            .unwrap();
            // A global-only view does not consult the project approval store.
            std::fs::write(home.join("project-trust.json"), "invalid JSON").unwrap();
            for profile in [
                ProfileKind::DevAuto,
                ProfileKind::DevPair,
                ProfileKind::Isolated,
            ] {
                let snapshot = build_harness(profile, |_| {}, |_| {})
                    .resolve(&ResolveCtx {
                        profile,
                        cwd: root.clone(),
                        project_root: root.clone(),
                        config: Arc::new(KanzeiConfig::default()),
                    })
                    .unwrap();
                if profile == ProfileKind::Isolated {
                    assert!(snapshot.agent_sources().is_empty());
                    continue;
                }
                let agent = snapshot
                    .select_primary_agent(Some(profile.as_str()))
                    .unwrap();
                assert_eq!(agent.system, format!("GLOBAL_CUSTOM_{}", profile.as_str()));
                assert!(snapshot.agent_sources().iter().all(|source| source.scope
                    == kanzei_harness::agent_sources::AgentSourceScope::Global));
                let report = snapshot.registry_report().agents;
                let origin = report
                    .iter()
                    .find(|entry| entry.name == profile.as_str())
                    .unwrap();
                assert_eq!(
                    origin.winner.path.as_ref().unwrap(),
                    &home.join(format!("agents/{}.md", profile.as_str()))
                );
                assert!(!snapshot
                    .materialize_tools()
                    .iter()
                    .any(|tool| tool.name() == "req"));
            }
        },
    );
}
