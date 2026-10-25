use super::*;
use kanzei_harness::{ContextSourceStatus, Harness, HarnessSnapshot, KanzeiConfig};
use std::path::PathBuf;

pub(super) struct Fixture(pub(super) PathBuf);
impl Fixture {
    pub(super) fn new(label: &str) -> Self {
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
    pub(super) fn write(&self, relative: &str, content: impl AsRef<[u8]>) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    pub(super) fn snapshot(&self, profile: ProfileKind) -> Arc<HarnessSnapshot> {
        let mut harness = Harness::default();
        harness
            .add(crate::BaseComponent)
            .add(DevelopmentProfile)
            .add(ResearchProfile)
            .add(ReadonlyProfile);
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
    let snapshot = fixture.snapshot(ProfileKind::DevAuto);
    let (_, before) = snapshot.system_baseline_with_report().unwrap();
    for key in [
        "development/ideas",
        "development/decisions",
        "development/design-index",
    ] {
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
    ] {
        fixture.write(path, [0xff, 0xfe]);
    }
    fixture.write(".kanzei/project-knowledge.json", b"{invalid");
    let (baseline, reports) = snapshot.system_baseline_with_report().unwrap();
    for key in [
        "development/ideas",
        "development/decisions",
        "development/design-index",
        "development/project-knowledge",
    ] {
        let report = reports.iter().find(|report| report.key == key).unwrap();
        assert_eq!(report.status, ContextSourceStatus::Failed, "{key}");
        assert!(report
            .error
            .as_deref()
            .is_some_and(|error| !error.is_empty()));
        assert_eq!(report.chars, 0);
    }
    assert!(!baseline.contains("<project-rules"));
}

#[test]
fn accepted_decision_index_refreshes_status_and_source_revision_without_creating_rules() {
    let fixture = Fixture::new("accepted-decisions");
    let original = "# Decisions\n\n## A-001 当前接受记录 [accepted]\n- 决定: PRIVATE_DECISION_BODY\n- 范围: PRIVATE_DECISION_SCOPE\n\n## A-002 草案不加载 [draft]\n\n## A-003 已替代不加载 [superseded]\n\n## A-004 已拒绝不加载 [rejected]\n";
    for profile in [ProfileKind::DevAuto, ProfileKind::DevPair] {
        fixture.write(DECISIONS.rel_path, original);
        let snapshot = fixture.snapshot(profile);
        let stable = snapshot.stable_system_baseline_with_report().unwrap().0;
        assert!(!stable.contains("当前接受记录"));
        let (text, reports) = snapshot.refreshable_system_baseline_with_report().unwrap();
        let report = reports
            .iter()
            .find(|report| report.key == "development/decisions")
            .unwrap();
        assert_eq!(report.status, ContextSourceStatus::Loaded);
        assert!(report.refresh_each_step);
        assert!(text.contains("A-001 [accepted] 当前接受记录"));
        assert!(text.contains(&fixture.0.join(DECISIONS.rel_path).display().to_string()));
        assert!(text.contains(&crate::content_hash(original.as_bytes())));
        for excluded in [
            "PRIVATE_DECISION_BODY",
            "PRIVATE_DECISION_SCOPE",
            "A-002",
            "A-003",
            "A-004",
            "re-litigate",
            "decision add",
            "tool_search select:decision",
        ] {
            assert!(!text.contains(excluded), "{excluded}");
        }
        assert_eq!(
            std::fs::read_to_string(fixture.0.join(DECISIONS.rel_path)).unwrap(),
            original,
            "context assembly cannot accept, supersede or rewrite a decision"
        );
        let revised = "# Decisions\n\n## A-001 当前接受记录 [superseded]\n\n## A-005 新接受记录 [accepted]\n- 决定: UPDATED_BODY\n";
        fixture.write(DECISIONS.rel_path, revised);
        let refreshed = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0;
        assert!(refreshed.contains("A-005 [accepted] 新接受记录"));
        assert!(refreshed.contains(&crate::content_hash(revised.as_bytes())));
        assert!(!refreshed.contains("A-001") && !refreshed.contains("UPDATED_BODY"));
        fixture.write(
            DECISIONS.rel_path,
            "# Decisions\n\n## A-005 新接受记录 [superseded]\n",
        );
        let (empty, reports) = snapshot.refreshable_system_baseline_with_report().unwrap();
        assert!(!empty.contains("<accepted-decisions>"));
        assert_eq!(
            reports
                .iter()
                .find(|report| report.key == "development/decisions")
                .unwrap()
                .status,
            ContextSourceStatus::Empty
        );
        fixture.write(DECISIONS.rel_path, [0xff, 0xfe]);
        let (failed, reports) = snapshot.refreshable_system_baseline_with_report().unwrap();
        assert!(!failed.contains("<accepted-decisions>"));
        assert_eq!(
            reports
                .iter()
                .find(|report| report.key == "development/decisions")
                .unwrap()
                .status,
            ContextSourceStatus::Failed
        );
    }
}

#[test]
fn design_navigation_is_a_project_file_pointer_without_repository_specific_body_loading() {
    let fixture = Fixture::new("design-pointer");
    fixture.write("notes/Architecture Notes.md", "PROJECT_DESIGN_BODY");
    let index = "# Design navigation\n[Architecture Notes](../../../notes/Architecture Notes.md)\nPRIVATE_INDEX_CONTENT\n";
    fixture.write(crate::architecture::ARCHITECTURE_REL, index);
    for profile in [ProfileKind::DevAuto, ProfileKind::DevPair] {
        let (baseline, reports) = fixture
            .snapshot(profile)
            .stable_system_baseline_with_report()
            .unwrap();
        assert_eq!(
            reports
                .iter()
                .find(|report| report.key == "development/design-index")
                .unwrap()
                .status,
            ContextSourceStatus::Loaded
        );
        assert!(baseline.contains(
            &fixture
                .0
                .join(crate::architecture::ARCHITECTURE_REL)
                .display()
                .to_string()
        ));
        assert!(baseline.contains(&crate::normalized_text_hash(index)));
        for body in [
            "PRIVATE_INDEX_CONTENT",
            "PROJECT_DESIGN_BODY",
            "Architecture Notes.md",
            "docs/design/",
        ] {
            assert!(
                !baseline.contains(body),
                "project navigation cannot preload {body}"
            );
        }
    }
}

#[test]
fn project_overview_resolves_nested_cwd_to_code_root_and_separates_worktree_assets() {
    let project = Fixture::new("overview-assets");
    let worktree = Fixture::new("overview-code");
    project.write(".kanzei/project-knowledge.json", "{\"enabled\":true}");
    project.write(
        "Cargo.toml",
        "[package]\nname='main_product'\nversion='0.1.0'\n",
    );
    project.write("src/lib.rs", "// main\n");
    worktree.write(
        "Cargo.toml",
        "[package]\nname='worktree_product'\nversion='0.1.0'\n",
    );
    worktree.write("src/lib.rs", "// worktree\n");
    for (cwd, expected_root) in [
        (project.0.join("src"), project.0.clone()),
        (worktree.0.clone(), worktree.0.clone()),
    ] {
        let ctx = ResolveCtx {
            profile: ProfileKind::DevPair,
            cwd,
            project_root: project.0.clone(),
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut draft = HarnessDraft::default();
        DevelopmentProfile.contribute(&mut draft, &ctx).unwrap();
        let source = draft.context.get("development/project-knowledge").unwrap();
        assert!(
            !source.refresh_each_step(),
            "the run overview remains a labeled assembly snapshot"
        );
        let text = source.baseline(&ctx).unwrap().unwrap();
        assert!(text.contains(&format!("code_root: {}\n", expected_root.display())));
        assert!(text.contains(&format!("project_root: {}\n", project.0.display())));
        assert!(text.contains("采样概览"));
        assert!(!text.contains("修改前用") && !text.contains("新经验用 memory_note"));
    }
}

#[test]
fn required_conventions_fail_on_initial_and_refreshed_read_for_all_profiles() {
    for profile in [
        ProfileKind::DevAuto,
        ProfileKind::DevPair,
        ProfileKind::Research,
    ] {
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
fn saved_preferences_refresh_for_normal_profiles_and_fail_explicitly_on_read_error() {
    for profile in [
        ProfileKind::DevAuto,
        ProfileKind::DevPair,
        ProfileKind::Research,
        ProfileKind::Readonly,
    ] {
        let fixture = Fixture::new("preferences");
        let snapshot = fixture.snapshot(profile);
        let path = ".kanzei/memory/M-001-preference.md";
        let preference = |body| {
            format!(
            "---\nid: M-001\nscope: project\ncategory: preference\ntitle: 当前项目偏好\nstatus: active\nsource: user\n---\n{body}\n"
        )
        };
        fixture.write(path, preference("项目偏好第一版"));
        assert!(!snapshot
            .stable_system_baseline_with_report()
            .unwrap()
            .0
            .contains("项目偏好第一版"));
        let (text, reports) = snapshot.refreshable_system_baseline_with_report().unwrap();
        assert!(text.contains("项目偏好第一版"));
        assert_eq!(
            reports
                .iter()
                .filter(|report| report.key == "core/preferences")
                .count(),
            1
        );
        fixture.write(path, preference("项目偏好第二版"));
        let refreshed = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap()
            .0;
        assert!(refreshed.contains("项目偏好第二版") && !refreshed.contains("项目偏好第一版"));
        fixture.write(path, [0xff, 0xfe]);
        let error = snapshot
            .refreshable_system_baseline_with_report()
            .unwrap_err();
        assert!(error
            .sources
            .iter()
            .any(|report| report.key == "core/preferences"
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
            .find(|report| report.key == "project/conventions")
            .unwrap()
            .status,
        ContextSourceStatus::Loaded
    );
}
