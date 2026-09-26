use super::*;

fn temporary_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kanzei-library-{label}-{}-{}",
        std::process::id(),
        crate::run::now_ms()
    ));
    std::fs::create_dir_all(root.join(".kanzei")).unwrap();
    root
}

#[tokio::test]
async fn standalone_topic_owns_storage_and_restores_its_session_without_a_project() {
    let home = temporary_root("standalone");
    let entry = create_entry(&home, "agentmem", "Agent memory").unwrap();
    let second = create_entry(&home, "agentmem", "Another study").unwrap();
    assert_ne!(entry.id, second.id);
    assert_ne!(entry.storage_root, second.storage_root);
    assert!(entry.linked_projects.is_empty());
    assert!(!home.join("app.json").exists());
    let root = Path::new(&entry.storage_root);
    // UI2-0926 #13:存储根是 path_form::canonical 形态(不带 `\\?\` 前缀)。
    assert!(root.starts_with(kanzei_tools::path_form::canonical(&home).unwrap()));
    assert!(root.join(".kanzei/research/agentmem/topic.json").is_file());
    let state = crate::AppState::default();
    let process = crate::processes::lifecycle::create_process_with_tracker(
        &state,
        &entry.storage_root,
        None,
        Some("research".into()),
        None,
        Some(false),
        Some(true),
        Some(false),
        None,
        None,
        entry.topic.clone(),
    )
    .await
    .unwrap();
    let restored = crate::AppState::default();
    crate::processes::restore_processes_from_store(&restored, root).unwrap();
    let restored_process = restored
        .processes
        .lock()
        .unwrap()
        .get(&process.id)
        .unwrap()
        .clone();
    assert_eq!(
        restored_process.research_topic.lock().unwrap().as_deref(),
        Some("agentmem")
    );
    assert_eq!(
        list_library(&home, &[]).unwrap()["entries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let project = home.join("code");
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();
    let linked = link_projects(&home, &entry.id, vec![project.display().to_string()]).unwrap();
    assert_eq!(linked.storage_root, entry.storage_root);
    assert_eq!(linked.linked_projects.len(), 1);
    assert!(link_projects(
        &home,
        &entry.id,
        vec![home.join("missing").display().to_string()]
    )
    .is_err());
    drop(restored_process);
    drop(restored);
    drop(state);
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn imports_duplicate_legacy_slugs_without_moving_files_and_keeps_missing_entries() {
    let home = temporary_root("legacy");
    let projects = [home.join("a"), home.join("b")];
    for (index, root) in projects.iter().enumerate() {
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        crate::research_topics::create_topic(root, "agentmem", &format!("Memory {index}")).unwrap();
        std::fs::write(
            root.join(".kanzei/research/agentmem/report.md"),
            format!("Original {index}"),
        )
        .unwrap();
    }
    let paths = projects
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>();
    let snapshot = list_library(&home, &paths).unwrap();
    let entries = snapshot["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["topic"] == "agentmem")
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0]["id"], entries[1]["id"]);
    assert_eq!(snapshot, list_library(&home, &paths).unwrap());
    for (index, root) in projects.iter().enumerate() {
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei/research/agentmem/report.md")).unwrap(),
            format!("Original {index}")
        );
    }
    std::fs::remove_dir_all(&projects[0]).unwrap();
    let next = list_library(&home, &[]).unwrap();
    let missing = next["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == entries[0]["id"])
        .unwrap();
    assert_eq!(missing["available"], false);
    assert_eq!(next["entries"].as_array().unwrap().len(), 4);
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn concurrent_creates_keep_both_entries_and_corrupt_registry_is_not_overwritten() {
    let home = temporary_root("concurrent");
    std::thread::scope(|scope| {
        let a = scope.spawn(|| create_entry(&home, "one", "One").unwrap());
        let b = scope.spawn(|| create_entry(&home, "two", "Two").unwrap());
        assert_ne!(a.join().unwrap().id, b.join().unwrap().id);
    });
    assert_eq!(
        list_library(&home, &[]).unwrap()["entries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    std::fs::write(home.join("research-library.json"), "broken").unwrap();
    assert!(create_entry(&home, "three", "Three").is_err());
    assert_eq!(
        std::fs::read_to_string(home.join("research-library.json")).unwrap(),
        "broken"
    );
    std::fs::remove_dir_all(home).unwrap();
}

// ── 分区:工作目录管理(UI2-0926 #13 复核)──

/// 登记表里存的是升级前 `\?\C:\…` 形态:第一次进研究空间不能把每个课题以新编号再登记一遍。
/// 迁移后一个课题一条、编号不变、存储根与关联项目是 simplify 形态;dev 构建期间已经多出来的
/// 重复登记合并进编号最小的那条。
#[cfg(windows)]
#[test]
fn verbatim_登记表迁移后不重复登记_编号不变() {
    let home = temporary_root("verbatim");
    let project = home.join("proj");
    std::fs::create_dir_all(project.join(".kanzei")).unwrap();
    crate::research_topics::create_topic(&project, "agentmem", "Agent memory").unwrap();
    let plain = kanzei_tools::path_form::canonical(&project)
        .unwrap()
        .display()
        .to_string();
    let verbatim = format!(r"\\?\{plain}");
    let entry = |id: &str, topic: Option<&str>, kind: &str, root: &str| {
        serde_json::json!({
            "id": id, "topic": topic, "label": "x", "kind": kind,
            "storage_root": root, "linked_projects": [root], "standalone": false,
        })
    };
    let seeded = serde_json::json!({
        "next_id": 3,
        "entries": [
            entry("topic-00000001", Some("agentmem"), "research", &verbatim),
            entry("topic-00000002", None, "unbound", &verbatim),
            // dev 构建跑过一次留下的重复登记(simplify 形态、大写盘符之外完全同一目录)。
            entry("topic-00000003", Some("agentmem"), "research", &plain.to_uppercase()),
        ],
    });
    std::fs::write(
        home.join("research-library.json"),
        serde_json::to_string(&seeded).unwrap(),
    )
    .unwrap();
    let projects = vec![verbatim.clone()];
    let first = list_library(&home, &projects).unwrap();
    let second = list_library(&home, &projects).unwrap();
    assert_eq!(first, second, "第二次进入不得再变");
    let entries = second["entries"].as_array().unwrap();
    let ids: Vec<&str> = entries.iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["topic-00000001", "topic-00000002"], "{entries:#?}");
    for entry in entries {
        let root = entry["storage_root"].as_str().unwrap();
        assert!(!root.starts_with(r"\\?\"), "{root}");
        for linked in entry["linked_projects"].as_array().unwrap() {
            assert!(!linked.as_str().unwrap().starts_with(r"\\?\"), "{linked}");
        }
    }
    assert_eq!(
        entries[0]["linked_projects"].as_array().unwrap().len(),
        1,
        "同一目录的两种写法只算一个关联项目"
    );
    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("research-library.json")).unwrap())
            .unwrap();
    assert_eq!(stored["next_id"], 3, "迁移不消耗新编号");
    assert!(!stored.to_string().contains(r"\\\\?\\"), "{stored}");
    std::fs::remove_dir_all(home).unwrap();
}
