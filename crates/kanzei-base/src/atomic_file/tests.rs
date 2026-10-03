use super::*;

pub(super) fn temp_root(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kz-base-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn hash(text: &str) -> String {
    crate::content_hash(text.as_bytes())
}

#[test]
fn atomic_replacement_preserves_bytes_and_creates_parents() {
    let root = temp_root("replace");
    let path = root.join("nested/doc.md");
    write_atomic(&path, "old").unwrap();
    write_atomic_bytes(&path, &[0, 0xff, 0x80, b'A']).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), [0, 0xff, 0x80, b'A']);
    assert_eq!(
        std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
        1
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn temp_names_are_unique_siblings() {
    let root = temp_root("names");
    let path = root.join("doc.md");
    let names: std::collections::HashSet<_> = (0..100)
        .map(|_| temp_sibling(&path, &root).unwrap())
        .collect();
    assert_eq!(names.len(), 100);
    assert!(names
        .iter()
        .all(|name| name.parent() == Some(root.as_path())));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_check_leaves_target_and_removes_candidate() {
    let root = temp_root("guard");
    let path = root.join("doc.md");
    write_atomic(&path, "old").unwrap();
    let error = write_atomic_bytes_guarded(&path, b"replacement", || {
        std::fs::write(&path, "external edit")?;
        Err(io::Error::other("conflict"))
    })
    .unwrap_err();
    assert_eq!(error.to_string(), "conflict");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "external edit");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cas_matches_rejects_stale_and_reenters_outer_transaction() {
    let root = temp_root("cas");
    let path = root.join("doc.md");
    {
        let _guard = lock_exclusive(&path).unwrap();
        write_atomic_cas(&path, "old", &hash(""), hash).unwrap();
        write_atomic_cas(&path, "new", &hash("old"), hash).unwrap();
        assert!(write_atomic_cas(&path, "stale", &hash("old"), hash)
            .unwrap_err()
            .contains("stale expected_hash"));
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry
        .unwrap()
        .path()
        .extension()
        .is_some_and(|e| e == "tmp")));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_cas_has_exactly_one_winner() {
    let root = temp_root("cas-race");
    let path = root.join("doc.md");
    write_atomic(&path, "base").unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|next| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                write_atomic_cas(&path, next, &hash("base"), hash).is_ok()
            })
        })
        .collect();
    let winners: usize = handles
        .into_iter()
        .map(|handle| usize::from(handle.join().unwrap()))
        .sum();
    assert_eq!(winners, 1);
    assert!(matches!(
        std::fs::read_to_string(&path).unwrap().as_str(),
        "one" | "two"
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn replacement_error_is_immediate_and_keeps_complete_candidate() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = temp_root("occupied");
    let path = root.join("doc.md");
    write_atomic(&path, "original").unwrap();
    let occupied = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let error = write_atomic(&path, "replacement").unwrap_err();
    assert!(error.to_string().contains("原文件未被破坏"));
    drop(occupied);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    let candidates: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "tmp"))
        .collect();
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        std::fs::read_to_string(&candidates[0]).unwrap(),
        "replacement"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cas_serializes_competing_writers() {
    let dir = temp_root("cas-writers");
    let path = dir.join("doc.md");
    write_atomic(&path, "before").unwrap();
    let (checking_tx, checking_rx) = std::sync::mpsc::channel();
    let (continue_tx, continue_rx) = std::sync::mpsc::channel();
    let first_path = path.clone();
    let first = std::thread::spawn(move || {
        write_atomic_cas(&first_path, "first", "before", |live| {
            checking_tx.send(()).unwrap();
            continue_rx.recv().unwrap();
            live.to_owned()
        })
    });
    checking_rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    let second_path = path.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let second = std::thread::spawn(move || {
        let result = write_atomic_cas(&second_path, "second", "before", str::to_owned);
        done_tx.send(result).unwrap();
    });
    let early = done_rx.recv_timeout(std::time::Duration::from_millis(100));
    continue_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    second.join().unwrap();
    assert!(
        early.is_err(),
        "second CAS committed while first was checking"
    );
    assert!(done_rx
        .recv()
        .unwrap()
        .unwrap_err()
        .contains("stale expected_hash"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
    std::fs::remove_dir_all(dir).unwrap();
}
