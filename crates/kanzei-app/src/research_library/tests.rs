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
    assert!(root.starts_with(home.canonicalize().unwrap()));
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
