//! 项目状态、配置、文档快照与缺陷审查测试。

use super::{export_project_data, normalized_project_root, ExportOptions, ProviderPayload, SettingsPayload};
use crate::docs::docs_snapshot;
// R-153 批10:缺陷审查迁到 subagents、模型角色校验迁到 settings。
use crate::subagents::{defect_review, defect_review_report, defect_review_snapshot};
use crate::settings::validate_model_roles;
// R-153 批5:项目隔离/分离已迁到 projects 模块,测试跟着改从模块导入。
use crate::projects::{ensure_project_isolated, project_detach};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn isolation_fixture(tag: &str, with_data: bool) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("kz-iso-{tag}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(base.join(".kanzei/project")).unwrap();
    if with_data { std::fs::write(base.join(".kanzei/project/requirements.md"), "# Requirements\n\n## R-900 上级的需求 [todo]\n").unwrap(); }
    let a = base.join("projA"); let b = base.join("projB");
    std::fs::create_dir_all(&a).unwrap(); std::fs::create_dir_all(&b).unwrap();
    (base, a, b)
}

#[test]
fn 祖先无数据时静默自动隔离_有数据时绝不擅自改根() {
    let (base, a, _) = isolation_fixture("auto", false);
    assert!(ensure_project_isolated(&a));
    assert_eq!(kanzei_harness::config::discover_project_root(&a).unwrap(), a);
    assert!(!ensure_project_isolated(&a));
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn 隔离体检一次报完全部共用项目() {
    let (base, a, b) = isolation_fixture("report", true);
    let dirs = [&a, &b];
    let shared: Vec<_> = dirs.iter().filter(|dir| kanzei_harness::config::discover_project_root(dir).unwrap() != ***dir).collect();
    assert_eq!(shared.len(), 2);
    project_detach(a.display().to_string()).unwrap();
    assert_eq!(kanzei_harness::config::discover_project_root(&a).unwrap(), a);
    assert_ne!(kanzei_harness::config::discover_project_root(&b).unwrap(), b);
    std::fs::remove_dir_all(base).ok();
}

#[test]
fn 同一上级下的两个项目必须各自独立不串数据() {
    let (base, a, b) = isolation_fixture("independent", true);
    assert_eq!(kanzei_harness::config::discover_project_root(&a).unwrap(), kanzei_harness::config::discover_project_root(&b).unwrap());
    project_detach(a.display().to_string()).unwrap();
    assert_eq!(kanzei_harness::config::discover_project_root(&a).unwrap(), a);
    assert_ne!(kanzei_harness::config::discover_project_root(&a).unwrap(), kanzei_harness::config::discover_project_root(&b).unwrap());
    assert!(!a.join(".kanzei/project/requirements.md").exists());
    std::fs::remove_dir_all(base).ok();
}

#[test]
fn 保存前拦住指向不存在_provider_的模型角色() {
    let payload = |primary: &str| SettingsPayload { primary: primary.into(), fast: String::new(), proxy: "env".into(), reasoning: None, codex_fast_mode: false, profile_default: None, profile: None, limits: Default::default(), providers: vec![ProviderPayload { name: "deepseek".into(), protocol: "openai".into(), base_url: "x".into(), api_key_env: None, api_key: None, auth: None, context_limit: None }] };
    assert!(validate_model_roles(&payload("deepsek:chat")).is_err());
    assert!(validate_model_roles(&payload("deepseek:chat")).is_ok());
}

#[test]
fn defect_review_snapshot_is_strictly_read_only() {
    let root = std::env::temp_dir().join(format!("kanzei-defect-review-tools-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let rctx = kanzei_harness::ResolveCtx { profile: kanzei_harness::ProfileKind::Dev, cwd: root.clone(), project_root: root.clone(), config: Arc::new(kanzei_harness::KanzeiConfig::default()) };
    let mut names: Vec<_> = defect_review_snapshot(&rctx).unwrap().materialize_tools().iter().map(|tool| tool.name().to_string()).collect();
    names.sort(); assert_eq!(names, vec!["glob", "grep", "read"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn defect_review_rejects_empty_model_report() {
    let empty = kanzei_core::RunSummary { text: "  ".into(), usage: kanzei_llm::Usage::default(), steps: 1, halted_by_user: false, messages: vec![], context_report: vec![], overflow_traces: vec![] };
    assert!(defect_review_report(&empty).is_err());
}

#[tokio::test]
async fn defect_review_empty_state_returns_without_model_call() {
    let root = std::env::temp_dir().join(format!("kanzei-defect-review-empty-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
    std::fs::write(root.join(".kanzei/project/defects.md"), "# Defects\n").unwrap();
    let result = defect_review(root.display().to_string()).await.unwrap();
    assert!(result.empty); assert_eq!(result.defect_count, 0);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn docs_snapshot_exposes_block_reasons_and_scheduler_order() {
    let root = std::env::temp_dir().join(format!("kanzei-docs-blocked-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
    std::fs::write(root.join(".kanzei/project/requirements.md"), "# Requirements\n\n## R-001 被阻塞 [todo]\n- 阻塞: 等待确认\n\n## R-002 可执行 [todo]\n").unwrap();
    let requirements = docs_snapshot(root.display().to_string())["requirements"].clone();
    assert_eq!(requirements[0]["id"], "R-002"); assert_eq!(requirements[1]["blocked"], true);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn export_project_data_copies_selected_work_materials() {
    let base = std::env::temp_dir().join(format!("kanzei-export-{}", std::process::id()));
    let project = base.join("project"); let output = base.join("output");
    std::fs::create_dir_all(project.join(".kanzei/memory")).unwrap(); std::fs::create_dir_all(project.join(".kanzei/project")).unwrap();
    std::fs::write(project.join(".kanzei/memory/M-001.md"), "记忆").unwrap(); std::fs::write(project.join(".kanzei/project/requirements.md"), "需求").unwrap(); std::fs::write(project.join(".kanzei/kanzei.toml"), "[models]").unwrap();
    let result = export_project_data(ExportOptions { project_dir: project.display().to_string(), output_dir: output.display().to_string(), include_memory: true, include_requirements: true, include_defects: false, include_config: true }).unwrap();
    let export_path = PathBuf::from(result["path"].as_str().unwrap());
    assert!(export_path.join(".kanzei/memory/M-001.md").is_file()); assert!(export_path.join(".kanzei/kanzei.toml").is_file());
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn project_root_normalizes_equivalent_paths() {
    let current = std::env::current_dir().unwrap();
    assert_eq!(normalized_project_root(Path::new(".")), std::fs::canonicalize(current).unwrap());
}
