use super::*;
use kanzei_harness::ProfileKind;

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
