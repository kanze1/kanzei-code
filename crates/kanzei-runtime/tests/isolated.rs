#[cfg(test)]
mod tests {
    use kanzei_harness::*;
    use std::sync::Arc;
    #[test]
    fn isolated_assembly_does_not_install_normal_or_extension_capabilities() {
        let root = kanzei_base::test_support::TestDir::new("kz-isolated-runtime-");
        let mut config = kanzei_harness::KanzeiConfig::default();
        config.permissions.rules.push(rule("*", "*", Effect::Allow));
        let ctx = ResolveCtx {
            profile: ProfileKind::Isolated,
            cwd: root.as_path().into(),
            project_root: root.as_path().into(),
            config: Arc::new(config),
        };
        let harness = kanzei_runtime::build_harness(
            ctx.profile,
            |_| panic!("normal middle extension"),
            |_| panic!("normal tail extension"),
        );
        let snapshot = harness.resolve(&ctx).unwrap();
        let names: Vec<_> = snapshot
            .materialize_tools()
            .iter()
            .map(|tool| tool.name())
            .collect();
        assert_eq!(names.len(), 8);
        for forbidden in [
            "bash",
            "git",
            "process",
            "browser",
            "write",
            "edit",
            "task",
            "skill_read",
            "latex",
            "plot",
            "webfetch",
            "websearch",
        ] {
            assert!(!names.contains(&forbidden), "{forbidden}");
        }
        assert!(names.contains(&"sandbox"));
        assert_eq!(
            snapshot.select_primary_agent(None).unwrap().name,
            "isolated"
        );
    }
}
