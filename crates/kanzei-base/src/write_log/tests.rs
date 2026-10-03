use super::*;

fn last_entry(root: &Path, path: &str) -> io::Result<Option<WriteLogEntry>> {
    Ok(entries_after(root, 0)?
        .into_iter()
        .rev()
        .find(|entry| entry.path == path))
}

fn temp_root(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("kz-journal-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn entry(at_ms: u128, path: &str, bytes: &[u8]) -> WriteLogEntry {
    WriteLogEntry {
        at_ms,
        path: path.into(),
        fingerprint: crate::content_hash(bytes),
        content: LoggedContent::Stored(bytes.to_vec()),
        run_id: None,
        process_id: None,
    }
}

#[test]
fn roundtrip_and_window_filter_preserve_binary_content() {
    let root = temp_root("roundtrip");
    let first = entry(10, "a/b.md", &[0xff, 0, b'\n']);
    record(&root, &first).unwrap();
    record(&root, &entry(20, "a_b.md", "中文\n正文".as_bytes())).unwrap();
    let all = entries_after(&root, 0).unwrap();
    assert_eq!(all[0], first);
    assert_eq!(entries_after(&root, 11).unwrap().len(), 1);
    assert!(last_entry(&root, "missing.md").unwrap().is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn same_millisecond_paths_and_same_path_writes_never_overwrite() {
    let root = temp_root("identity");
    let mut files = HashSet::new();
    for (path, content) in [
        ("a/b.md", b"one".as_slice()),
        ("a_b.md", b"two"),
        ("a/b.md", b"three"),
    ] {
        assert!(files.insert(record(&root, &entry(100, path, content)).unwrap()));
    }
    assert_eq!(entries_after(&root, 0).unwrap().len(), 3);
    assert_eq!(
        last_entry(&root, "a/b.md").unwrap().unwrap().content,
        LoggedContent::Stored(b"three".to_vec())
    );
    let newest = entry(1, "a/b.md", b"clock moved back");
    record(&root, &newest).unwrap();
    assert_eq!(last_entry(&root, "a/b.md").unwrap().unwrap(), newest);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn empty_file_deleted_file_and_omitted_body_are_distinct() {
    let root = temp_root("states");
    let mut current = entry(1, "rules.md", b"");
    record(&root, &current).unwrap();
    let empty = last_entry(&root, "rules.md").unwrap().unwrap();
    assert!(empty.matches_content(Some(b"")));
    assert!(!empty.matches_content(None));
    assert_eq!(empty.content, LoggedContent::Stored(Vec::new()));
    current.content = LoggedContent::Deleted;
    record(&root, &current).unwrap();
    let deleted = last_entry(&root, "rules.md").unwrap().unwrap();
    assert!(deleted.matches_content(None));
    assert!(!deleted.matches_content(Some(b"")));
    current.content = LoggedContent::FingerprintOnly;
    record(&root, &current).unwrap();
    assert_eq!(
        last_entry(&root, "rules.md").unwrap().unwrap().content,
        LoggedContent::FingerprintOnly
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn quiet_document_checkpoint_survives_more_than_500_hot_writes() {
    let root = temp_root("retention");
    let quiet = entry(1, "quiet.md", b"last legitimate content");
    record(&root, &quiet).unwrap();
    for i in 0..WRITE_LOG_MAX_FILES + 5 {
        record(
            &root,
            &entry(i as u128 + 2, "hot.md", format!("version-{i}").as_bytes()),
        )
        .unwrap();
    }
    assert_eq!(last_entry(&root, "quiet.md").unwrap().unwrap(), quiet);
    assert_eq!(entries_after(&root, 0).unwrap().len(), WRITE_LOG_MAX_FILES);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn retention_target_never_deletes_unique_checkpoints_or_newer_metadata() {
    let root = temp_root("soft-limit");
    record(&root, &entry(1, "a.md", b"old")).unwrap();
    record(&root, &entry(2, "a.md", b"checkpoint")).unwrap();
    let mut omitted = entry(3, "a.md", b"newer without body");
    omitted.content = LoggedContent::FingerprintOnly;
    record(&root, &omitted).unwrap();
    for path in ["b.md", "c.md", "d.md"] {
        record(&root, &entry(4, path, b"")).unwrap();
    }
    let records = read_records(&log_root(&root)).unwrap();
    prune(&records, 2).unwrap();
    let remaining = entries_after(&root, 0).unwrap();
    assert_eq!(remaining.len(), 5);
    assert!(remaining
        .iter()
        .any(|e| e.path == "a.md" && e.content == LoggedContent::Stored(b"checkpoint".to_vec())));
    assert_eq!(last_entry(&root, "a.md").unwrap().unwrap(), omitted);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn existing_six_line_logs_remain_readable_and_untouched() {
    let root = temp_root("legacy");
    std::fs::create_dir_all(log_root(&root)).unwrap();
    let file = log_root(&root).join("100-123-old_path.log");
    let old = format!(
        "100\nrules.md\n{}\n6f6c64\n\n\n",
        crate::content_hash(b"old")
    );
    std::fs::write(&file, &old).unwrap();
    assert_eq!(
        last_entry(&root, "rules.md").unwrap().unwrap().content,
        LoggedContent::Stored(b"old".to_vec())
    );
    record(&root, &entry(99, "rules.md", b"new")).unwrap();
    assert_eq!(
        last_entry(&root, "rules.md").unwrap().unwrap().content,
        LoggedContent::Stored(b"new".to_vec())
    );
    assert_eq!(std::fs::read_to_string(file).unwrap(), old);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_or_ambiguous_legacy_records_are_errors_not_empty_history() {
    let root = temp_root("invalid");
    std::fs::create_dir_all(log_root(&root)).unwrap();
    let file = log_root(&root).join("100-123-invalid.log");
    for text in [
        "broken".to_string(),
        codec::encode(&entry(100, "rules.md", b"old")).replace("data:6f6c64", "data:626164"),
        format!("100\nrules.md\n{}\n\n\n\n", crate::content_hash(b"")),
    ] {
        std::fs::write(&file, &text).unwrap();
        assert_eq!(
            entries_after(&root, 0).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(record(&root, &entry(101, "rules.md", b"new")).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_same_path_same_time_has_no_invented_order() {
    let root = temp_root("legacy-order");
    std::fs::create_dir_all(log_root(&root)).unwrap();
    let encoded = codec::encode(&entry(100, "rules.md", b"old"));
    for pid in [1, 2] {
        std::fs::write(
            log_root(&root).join(format!("100-{pid}-rules.log")),
            &encoded,
        )
        .unwrap();
    }
    assert!(entries_after(&root, 0)
        .unwrap_err()
        .to_string()
        .contains("无法确定"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn journal_child_process() {
    let Some(root) = std::env::var_os("KZ_BASE_JOURNAL_TEST_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let document = root.join("shared.md");
    for index in 0..8 {
        let _lock = crate::atomic_file::lock_exclusive(&document).unwrap();
        let bytes = format!("{}:{index}", std::process::id()).into_bytes();
        crate::atomic_file::write_atomic_bytes(&document, &bytes).unwrap();
        record(root, &entry(100, "shared.md", &bytes)).unwrap();
    }
}

#[test]
fn multiple_processes_order_records_inside_document_transaction() {
    let root = temp_root("process");
    let mut children = Vec::new();
    for _ in 0..2 {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "write_log::tests::journal_child_process"])
            .env("KZ_BASE_JOURNAL_TEST_ROOT", &root)
            .stdout(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        children.push(command.spawn().unwrap());
    }
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let records = entries_after(&root, 0).unwrap();
    assert_eq!(records.len(), 16);
    let actual = std::fs::read(root.join("shared.md")).unwrap();
    assert_eq!(
        records.last().unwrap().content,
        LoggedContent::Stored(actual)
    );
    std::fs::remove_dir_all(root).unwrap();
}
