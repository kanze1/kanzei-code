use super::*;
use kanzei_harness::{ContextSourceStatus, Harness, HarnessSnapshot, KanzeiConfig};
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "kz-context-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn write(&self, relative: &str, content: impl AsRef<[u8]>) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    fn snapshot(&self, profile: ProfileKind) -> Arc<HarnessSnapshot> {
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevProfile)
            .add(ResearchProfile);
        harness
            .resolve(&ResolveCtx {
                profile,
                cwd: self.0.clone(),
                project_root: self.0.clone(),
                config: Arc::new(KanzeiConfig::default()),
            })
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn optional_document_errors_remain_diagnostic_and_empty_sources_remain_empty() {
    let fixture = Fixture::new("optional");
    let snapshot = fixture.snapshot(ProfileKind::Dev);
    let (_, before) = snapshot.stable_system_baseline_with_report().unwrap();
    for key in ["dev/ideas", "dev/decisions", "dev/design-index"] {
        assert_eq!(
            before
                .iter()
                .find(|report| report.key == key)
                .unwrap()
                .status,
            ContextSourceStatus::Empty
        );
    }
    for path in [
        ".kanzei/project/ideas.md",
        ".kanzei/project/decisions.md",
        ".kanzei/project/architecture/README.md",
        ".kanzei/memory/M-001-invalid.md",
    ] {
        fixture.write(path, [0xff, 0xfe]);
    }
    fixture.write(".kanzei/project-knowledge.json", b"{invalid");
    let (baseline, reports) = snapshot.system_baseline_with_report().unwrap();
    for key in [
        "dev/ideas",
        "dev/decisions",
        "dev/design-index",
        "dev/memory",
        "dev/project-knowledge",
    ] {
        let report = reports.iter().find(|report| report.key == key).unwrap();
        assert_eq!(report.status, ContextSourceStatus::Failed, "{key}");
        assert!(report
            .error
            .as_deref()
            .is_some_and(|error| !error.is_empty()));
        assert_eq!(report.chars, 0);
    }
    assert!(baseline.contains(kanzei_harness::DEFAULT_CONVENTIONS.trim()));
}

#[test]
fn required_conventions_fail_on_initial_and_refreshed_read_for_both_profiles() {
    for profile in [ProfileKind::Dev, ProfileKind::Research] {
        let fixture = Fixture::new("required");
        let snapshot = fixture.snapshot(profile);
        assert!(
            snapshot.system_baseline().is_ok(),
            "missing conventions are valid"
        );
        fixture.write(".kanzei/project/conventions.md", "完整规则");
        assert!(snapshot.system_baseline().unwrap().contains("完整规则"));
        fixture.write(".kanzei/project/conventions.md", [0xff, 0xfe]);
        let error = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap_err();
        assert!(error.to_string().contains("conventions.md"));
        assert!(error
            .sources
            .iter()
            .any(|report| report.key.ends_with("/conventions")
                && report.status == ContextSourceStatus::Failed
                && report.requirement == ContextRequirement::Required));
        assert!(fixture.snapshot(profile).system_baseline().is_err());
    }
}

#[test]
fn research_conventions_are_complete_even_when_optional_topic_read_fails() {
    let fixture = Fixture::new("research");
    let rules = format!(
        "{}\n末尾必须遵守的验收约定",
        "必须保留来源与验证证据。\n".repeat(900)
    );
    assert!(rules.chars().count() > 8000);
    fixture.write(".kanzei/project/conventions.md", &rules);
    fixture.write(".kanzei/research/fixture-topic/sources.md", [0xff, 0xfe]);
    let (baseline, reports) = fixture
        .snapshot(ProfileKind::Research)
        .system_baseline_with_report()
        .unwrap();
    assert!(baseline.contains(&rules));
    let report = reports
        .iter()
        .find(|report| report.key == "research/docs")
        .unwrap();
    assert_eq!(report.status, ContextSourceStatus::Failed);
    assert!(report.error.as_deref().unwrap().contains("fixture-topic"));
    assert_eq!(
        reports
            .iter()
            .find(|report| report.key == "research/conventions")
            .unwrap()
            .status,
        ContextSourceStatus::Loaded
    );
}
