use super::*;
use kanzei_harness::{ContextSourceStatus, Harness, HarnessSnapshot, KanzeiConfig, ProfileKind};
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
fn saved_preferences_refresh_for_normal_profiles_and_fail_explicitly_on_read_error() {
    for profile in [
        ProfileKind::DevAuto,
        ProfileKind::DevPair,
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
                && report.requirement == kanzei_harness::ContextRequirement::Required));
        assert!(fixture.snapshot(profile).system_baseline().is_err());
    }
}
