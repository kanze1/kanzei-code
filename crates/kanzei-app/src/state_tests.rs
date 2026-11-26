//! 项目状态、配置与隔离测试。

use super::{
    export_project_data, normalized_project_root, ExportOptions, ProviderPayload, SettingsPayload,
};
use crate::projects::{ensure_project_isolated, project_detach};
use crate::settings::settings_save_at_path;
use std::path::{Path, PathBuf};

fn isolation_fixture(tag: &str, with_data: bool) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "kz-iso-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(base.join(".kanzei/project")).unwrap();
    if with_data {
        std::fs::write(
            base.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-900 上级的需求 [todo]\n",
        )
        .unwrap();
    }
    let a = base.join("projA");
    let b = base.join("projB");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    (base, a, b)
}

#[test]
fn 祖先无数据时静默自动隔离_有数据时绝不擅自改根() {
    let (base, a, _) = isolation_fixture("auto", false);
    assert!(ensure_project_isolated(&a));
    assert_eq!(
        kanzei_harness::config::discover_project_root(&a).unwrap(),
        a
    );
    assert!(!ensure_project_isolated(&a));
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn 隔离体检一次报完全部共用项目() {
    let (base, a, b) = isolation_fixture("report", true);
    let dirs = [&a, &b];
    let shared: Vec<_> = dirs
        .iter()
        .filter(|dir| kanzei_harness::config::discover_project_root(dir).unwrap() != ***dir)
        .collect();
    assert_eq!(shared.len(), 2);
    project_detach(a.display().to_string()).unwrap();
    assert_eq!(
        kanzei_harness::config::discover_project_root(&a).unwrap(),
        a
    );
    assert_ne!(
        kanzei_harness::config::discover_project_root(&b).unwrap(),
        b
    );
    std::fs::remove_dir_all(base).ok();
}

#[test]
fn 同一上级下的两个项目必须各自独立不串数据() {
    let (base, a, b) = isolation_fixture("independent", true);
    assert_eq!(
        kanzei_harness::config::discover_project_root(&a).unwrap(),
        kanzei_harness::config::discover_project_root(&b).unwrap()
    );
    project_detach(a.display().to_string()).unwrap();
    assert_eq!(
        kanzei_harness::config::discover_project_root(&a).unwrap(),
        a
    );
    assert_ne!(
        kanzei_harness::config::discover_project_root(&a).unwrap(),
        kanzei_harness::config::discover_project_root(&b).unwrap()
    );
    assert!(!a.join(".kanzei/project/requirements.md").exists());
    std::fs::remove_dir_all(base).ok();
}

#[test]
fn 保存前拦住指向不存在_provider_的模型角色() {
    let payload = |primary: &str| SettingsPayload {
        language: None,
        primary: primary.into(),
        fast: String::new(),
        compact: String::new(),
        proxy: "env".into(),
        reasoning: None,
        codex_fast_mode: false,
        profile_default: None,
        profile: None,
        limits: Default::default(),
        cadence: None,
        providers: vec![ProviderPayload {
            name: "deepseek".into(),
            protocol: "openai".into(),
            base_url: "x".into(),
            api_key_env: None,
            api_key: None,
            auth: None,
            context_limit: None,
        }],
    };
    let path = std::env::temp_dir().join(format!(
        "kanzei-role-validation-{}-{}.toml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    assert!(settings_save_at_path(payload("deepsek:chat"), &path).is_err());
    assert!(!path.exists(), "非法角色不能提交配置");
    assert!(settings_save_at_path(payload("deepseek:chat"), &path).is_ok());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn export_project_data_copies_selected_work_materials() {
    let base = std::env::temp_dir().join(format!("kanzei-export-{}", std::process::id()));
    let project = base.join("project");
    let output = base.join("output");
    std::fs::create_dir_all(project.join(".kanzei/memory")).unwrap();
    std::fs::create_dir_all(project.join(".kanzei/project")).unwrap();
    std::fs::write(project.join(".kanzei/memory/M-001.md"), "记忆").unwrap();
    std::fs::write(project.join(".kanzei/project/requirements.md"), "需求").unwrap();
    std::fs::write(project.join(".kanzei/kanzei.toml"), "[models]").unwrap();
    let result = export_project_data(ExportOptions {
        project_dir: project.display().to_string(),
        output_dir: output.display().to_string(),
        include_memory: true,
        include_requirements: true,
        include_defects: false,
        include_config: true,
    })
    .unwrap();
    let export_path = PathBuf::from(result["path"].as_str().unwrap());
    assert!(export_path.join(".kanzei/memory/M-001.md").is_file());
    assert!(export_path.join(".kanzei/kanzei.toml").is_file());
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn project_root_normalizes_equivalent_paths() {
    let current = std::env::current_dir().unwrap();
    // UI2-0926 #13:身份根是去掉 `\\?\` 前缀的 canonical 形态(path_form::canonical)。
    assert_eq!(
        normalized_project_root(Path::new(".")),
        kanzei_tools::path_form::canonical(&current).unwrap()
    );
}

#[test]
fn explicitly_selected_empty_projects_keep_their_own_identity_and_state() {
    let (base, a, b) = isolation_fixture("selected-state", true);
    let parent = normalized_project_root(&base);
    // This is deliberately a different contract from CLI cwd discovery: neither child
    // has a marker, and discovery would select the parent containing user material.
    assert_eq!(
        kanzei_harness::config::discover_project_root(&a),
        Some(base.clone())
    );
    let roots = [normalized_project_root(&a), normalized_project_root(&b)];
    assert_eq!(roots[0], kanzei_tools::path_form::canonical(&a).unwrap());
    assert_eq!(roots[1], kanzei_tools::path_form::canonical(&b).unwrap());
    let parent_id = crate::process_session_id(&parent, None);
    let parent_store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&parent)).unwrap();
    parent_store
        .create_session(&parent_id, &parent.display().to_string(), None)
        .unwrap();
    parent_store
        .append_event(
            &parent_id,
            "user.material",
            &serde_json::json!({"text":"parent draft"}),
        )
        .unwrap();
    let state = crate::AppState::default();
    for root in &roots {
        crate::conversation::clear_conversation(&state, &root.display().to_string(), None).unwrap();
        let process = crate::ensure_default_process(&state, root);
        assert_eq!(process.origin_project.0, *root);
        let id = crate::process_session_id(root, None);
        assert_ne!(id, parent_id);
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(root)).unwrap();
        assert!(store.get_session(&id).unwrap().is_some());
        assert!(store.get_session(&parent_id).unwrap().is_none());
        assert!(parent_store.get_session(&id).unwrap().is_none());
        assert_eq!(
            store
                .list_events_by_type(&id, 0, "conversation.reset")
                .unwrap()
                .len(),
            1
        );
    }
    assert_ne!(
        crate::process_session_id(&roots[0], None),
        crate::process_session_id(&roots[1], None)
    );
    assert!(parent_store.get_session(&parent_id).unwrap().is_some());
    assert!(parent_store
        .list_events_by_type(&parent_id, 0, "conversation.reset")
        .unwrap()
        .is_empty());
    assert_eq!(
        std::fs::read_to_string(base.join(".kanzei/project/requirements.md")).unwrap(),
        "# Requirements\n\n## R-900 上级的需求 [todo]\n"
    );
    drop(parent_store);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn selected_child_permissions_and_memory_keep_parent_material_untouched() {
    let (base, _unused_child, permission_child) = isolation_fixture("selected-surfaces", true);
    let memory_child = base.join("memory-child");
    std::fs::create_dir_all(&memory_child).unwrap();
    let parent_config = "[permissions]\n[[permissions.rules]]\naction = \"bash\"\nresource = \"parent-only\"\neffect = \"allow\"\n";
    std::fs::write(base.join(".kanzei/kanzei.toml"), parent_config).unwrap();
    let parent_memory = "---\nid: M-900\nscope: project\ncategory: fact\ntitle: Parent draft\ndescription: Parent material\nstatus: verified\ncreated: 2026-09-30\nupdated: 2026-09-30\nsource: user\n---\n\nDo not change parent material.\n";
    std::fs::create_dir_all(base.join(".kanzei/memory")).unwrap();
    std::fs::write(base.join(".kanzei/memory/M-900.md"), parent_memory).unwrap();

    let rules =
        crate::settings::permission_rules_get(permission_child.display().to_string()).unwrap();
    assert!(rules["rules"].as_array().unwrap().is_empty());
    assert_eq!(
        PathBuf::from(rules["path"].as_str().unwrap()),
        kanzei_harness::kanzei_home()
            .unwrap()
            .join("project-trust.json")
    );
    assert!(crate::settings::permission_rule_delete(
        permission_child.display().to_string(),
        0,
        kanzei_harness::permission::Rule {
            action: "bash".into(),
            resource: "cargo test".into(),
            effect: kanzei_harness::permission::Effect::Allow,
        },
    )
    .is_err());
    assert_eq!(
        std::fs::read_to_string(base.join(".kanzei/kanzei.toml")).unwrap(),
        parent_config
    );

    let entries =
        crate::memory::memory_entries(memory_child.display().to_string(), "project".into(), None)
            .unwrap();
    assert!(entries.as_array().unwrap().is_empty());
    assert!(crate::memory::memory_entry_delete(
        memory_child.display().to_string(),
        "project".into(),
        "M-900".into()
    )
    .is_err());
    assert_eq!(
        std::fs::read_to_string(base.join(".kanzei/memory/M-900.md")).unwrap(),
        parent_memory
    );
    let parent_store =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&base)).unwrap();
    parent_store
        .record_recall_event(&kanzei_core::RecallEvent {
            recall_id: "parent-recall",
            episode_id: None,
            step_id: None,
            trigger_type: "memory_search",
            trigger_payload: "{}",
            policy_action: "lexical",
            query: "parent",
            candidate_ids: "[\"M-900\"]",
            retrieved_ids: "[\"M-900\"]",
            injected_ids: "[]",
            lexical_ms: 0,
            embed_ms: 0,
            vector_ms: 0,
            total_ms: 0,
        })
        .unwrap();
    let recalls = crate::memory::memory_recalls(memory_child.display().to_string(), None).unwrap();
    assert_eq!(recalls["rounds_total"], 0);
    assert_eq!(
        parent_store.memory_recall_observations(20).unwrap().len(),
        1
    );
    assert!(memory_child.join(".kanzei/state.db").is_file());
    drop(parent_store);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn d506_hot_path_mutex_locks_use_poison_recovery() {
    let files = [
        "src/state.rs",
        "src/processes/registry.rs",
        "src/run/coordinator.rs",
        "src/run/persistence.rs",
    ];
    for file in files {
        let source =
            std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file))
                .unwrap_or_else(|error| panic!("读取 {file} 失败: {error}"));
        let compact: String = source.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert!(
            !compact.contains(".lock().unwrap()"),
            "D-506 热路径仍存在 poisoned mutex panic 写法: {file}"
        );
    }
}

/// UI2-0926 #13:项目身份根是进程 id、ProcessInfo.project_dir 与鞭挞轮 projectDir 的唯一来源,
/// 必须是去掉 verbatim 前缀的经典形态;传入 verbatim 与裸路径两种写法得到同一个进程 id。
#[test]
fn 项目身份根不带_verbatim_前缀且两种入参同一进程id() {
    let dir = std::env::temp_dir().join(format!(
        "kz-root-form-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei")).unwrap();
    let bare = crate::normalized_project_root(&dir);
    assert!(
        !bare.display().to_string().starts_with(r"\\?\"),
        "{}",
        bare.display()
    );
    let verbatim = std::fs::canonicalize(&dir).unwrap();
    assert_eq!(
        crate::state::legacy_process_id(&crate::normalized_project_root(&verbatim)),
        crate::state::legacy_process_id(&bare)
    );
    std::fs::remove_dir_all(&dir).ok();
}
