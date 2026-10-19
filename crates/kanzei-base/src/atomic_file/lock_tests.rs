use super::*;
use crate::atomic_file::tests::temp_root;
use std::io::{BufRead, Write};
use std::process::{Child, Command, Stdio};

#[test]
fn thread_admission_matrix() {
    for held in [Mode::Shared, Mode::Exclusive] {
        for requested in [Mode::Shared, Mode::Exclusive] {
            let root = temp_root("matrix");
            let path = root.join("doc.md");
            let guard = try_lock(&path, held, Duration::ZERO).unwrap().unwrap();
            let other = path.clone();
            let allowed = std::thread::spawn(move || {
                try_lock(&other, requested, Duration::ZERO)
                    .unwrap()
                    .is_some()
            })
            .join()
            .unwrap();
            assert_eq!(
                allowed,
                held == Mode::Shared && requested == Mode::Shared,
                "{held:?} x {requested:?}"
            );
            drop(guard);
            assert!(try_lock(&path, requested, Duration::ZERO)
                .unwrap()
                .is_some());
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn reentrant_depth_and_last_shared_reader_control_release() {
    for mode in [Mode::Shared, Mode::Exclusive] {
        let root = temp_root("reenter");
        let path = root.join("doc.md");
        let outer = try_lock(&path, mode, Duration::ZERO).unwrap().unwrap();
        let inner = try_lock(&path, mode, Duration::ZERO).unwrap().unwrap();
        let shared = try_lock_shared(&path, Duration::ZERO).unwrap().unwrap();
        drop(outer);
        drop(inner);
        let other = path.clone();
        assert!(
            std::thread::spawn(move || try_lock_exclusive(&other, Duration::ZERO)
                .unwrap()
                .is_none())
            .join()
            .unwrap()
        );
        drop(shared);
        assert!(try_lock_exclusive(&path, Duration::ZERO).unwrap().is_some());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn shared_to_exclusive_reports_deadlock() {
    let root = temp_root("upgrade");
    let path = root.join("doc.md");
    let shared = lock_shared(&path).unwrap();
    let result = try_lock_exclusive(&path, Duration::from_secs(10));
    assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::Deadlock));
    drop(shared);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn release_wakes_waiter_and_waiting_does_not_hold_other_slots() {
    let root = temp_root("wakeup");
    let path = root.join("doc.md");
    let guard = lock_exclusive(&path).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        try_lock_shared(&path, Duration::from_secs(2))
            .unwrap()
            .is_some()
    });
    started_rx.recv().unwrap();
    assert!(try_lock_exclusive(&root.join("other.md"), Duration::ZERO)
        .unwrap()
        .is_some());
    drop(guard);
    assert!(waiter.join().unwrap());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn contention_times_out_but_io_errors_are_not_contention() {
    let root = temp_root("timeout");
    let path = root.join("doc.md");
    let guard = lock_exclusive(&path).unwrap();
    let started = Instant::now();
    let other = path.clone();
    assert!(std::thread::spawn(
        move || try_lock_exclusive(&other, Duration::from_millis(40))
            .unwrap()
            .is_none()
    )
    .join()
    .unwrap());
    assert!(started.elapsed() >= Duration::from_millis(40));
    drop(guard);
    let bad = root.join("bad.md");
    let bad_lock = lock_path_for(&bad).unwrap();
    std::fs::create_dir(&bad_lock).unwrap();
    assert!(try_lock_shared(&bad, Duration::from_secs(10)).is_err());
    std::fs::remove_dir(bad_lock).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn path_aliases_reenter_the_same_slot() {
    let root = temp_root("aliases");
    std::fs::create_dir(root.join("child")).unwrap();
    let path = root.join("doc.md");
    let guard = lock_exclusive(&path).unwrap();
    let alias = root.join("child/../doc.md");
    assert!(try_lock_exclusive(&alias, Duration::ZERO)
        .unwrap()
        .is_some());
    #[cfg(windows)]
    {
        let lowercase = PathBuf::from(path.to_str().unwrap().to_lowercase());
        assert!(try_lock_exclusive(&lowercase, Duration::ZERO)
            .unwrap()
            .is_some());
    }
    drop(guard);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn file_and_directory_symlinks_share_lock_identity() {
    let root = temp_root("symlink");
    let real = root.join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("doc.md"), "content").unwrap();
    std::os::unix::fs::symlink(&real, root.join("alias")).unwrap();
    std::os::unix::fs::symlink(real.join("doc.md"), root.join("file.md")).unwrap();
    let guard = lock_exclusive(&real.join("doc.md")).unwrap();
    for path in [root.join("alias/doc.md"), root.join("file.md")] {
        assert!(try_lock_exclusive(&path, Duration::ZERO).unwrap().is_some());
    }
    drop(guard);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn relative_and_absolute_paths_share_identity_before_and_after_creation() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = PathBuf::from(format!(".kz-lock-test-{}-{nanos}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let relative = root.join("doc.md");
    let absolute = std::env::current_dir().unwrap().join(&relative);
    let guard = lock_exclusive(&relative).unwrap();
    for created in [false, true] {
        if created {
            std::fs::write(&absolute, "created under lock").unwrap();
        }
        assert!(try_lock_exclusive(&absolute, Duration::ZERO)
            .unwrap()
            .is_some());
    }
    drop(guard);
    std::fs::remove_dir_all(root).unwrap();
}

fn child_command(mode: &str, path: &Path, hold: bool) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "atomic_file::lock::tests::child_process",
            "--nocapture",
        ])
        .env("KZ_BASE_LOCK_TARGET", path)
        .env("KZ_BASE_LOCK_MODE", mode)
        .env("KZ_BASE_LOCK_HOLD", if hold { "1" } else { "0" })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn child(mode: &str, path: &Path, hold: bool) -> (Child, bool) {
    spawn_child(child_command(mode, path, hold))
}

fn spawn_child(mut command: Command) -> (Child, bool) {
    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let line = line.unwrap();
            // The serial Rust harness can prefix this line with `test ...`.
            // The child's explicit marker frames the result in either mode.
            if let Some((_, value)) = line.split_once("LOCK_RESULT=") {
                tx.send(value == "true").unwrap();
                // Keep draining so the child's test harness can finish normally.
            }
        }
    });
    let acquired = match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(value) => value,
        Err(error) => {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("child did not report lock result: {error}");
        }
    };
    (child, acquired)
}

#[test]
fn child_process() {
    let Some(target) = std::env::var_os("KZ_BASE_LOCK_TARGET") else {
        return;
    };
    let mode = if std::env::var("KZ_BASE_LOCK_MODE").unwrap() == "shared" {
        Mode::Shared
    } else {
        Mode::Exclusive
    };
    let guard = try_lock(Path::new(&target), mode, Duration::ZERO).unwrap();
    println!("LOCK_RESULT={}", guard.is_some());
    std::io::stdout().flush().unwrap();
    if std::env::var("KZ_BASE_LOCK_HOLD").unwrap() == "1" {
        std::io::stdin().read_line(&mut String::new()).unwrap();
    }
    drop(guard);
}

#[test]
fn real_process_matrix_and_crash_release_ignore_lock_file_age() {
    let root = temp_root("process");
    let path = root.join("doc.md");
    let shared = lock_shared(&path).unwrap();
    for (mode, expected) in [("shared", true), ("exclusive", false)] {
        let (mut process, acquired) = child(mode, &path, false);
        assert_eq!(acquired, expected);
        assert!(process.wait().unwrap().success());
    }
    drop(shared);
    let (mut process, acquired) = child("exclusive", &path, true);
    assert!(acquired);
    let lock_path = lock_path_for(&path).unwrap();
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1))
        .unwrap();
    assert!(try_lock_shared(&path, Duration::ZERO).unwrap().is_none());
    assert!(try_lock_exclusive(&path, Duration::ZERO).unwrap().is_none());
    process.kill().unwrap();
    process.wait().unwrap();
    assert!(lock_path.exists());
    assert!(try_lock_exclusive(&path, Duration::ZERO).unwrap().is_some());
    assert!(try_lock_exclusive(&path, Duration::ZERO).unwrap().is_some());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn regression_locking_does_not_create_workspace_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("missing/child/script.js");
    let _guard = lock_exclusive(&target).unwrap();
    assert!(
        !root.path().join("missing").exists(),
        "coordination must not create target directories or sibling lock files"
    );
}

#[test]
fn regression_different_extensions_have_independent_locks() {
    let root = tempfile::tempdir().unwrap();
    let _guard = lock_exclusive(&root.path().join("script.js")).unwrap();
    let other = root.path().join("script.py");
    assert!(
        std::thread::spawn(move || try_lock_exclusive(&other, Duration::ZERO)
            .unwrap()
            .is_some())
        .join()
        .unwrap()
    );
}

#[test]
fn regression_lock_named_document_stays_locked_across_atomic_replacement() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("package.lock");
    std::fs::write(&target, "original dependency data").unwrap();
    let _guard = lock_exclusive(&target).unwrap();
    crate::atomic_file::write_atomic(&target, "new dependency data").unwrap();
    let (mut process, acquired) = child("exclusive", &target, false);
    assert!(process.wait().unwrap().success());
    assert!(!acquired, "atomic replacement must preserve lock identity");
}

#[test]
fn regression_slots_are_reclaimed_after_the_last_holder() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("script.js");
    let outer = lock_exclusive(&target).unwrap();
    let key = outer.path.clone();
    let slot = Arc::downgrade(&outer.slot);
    let inner = lock_exclusive(&target).unwrap();
    drop(outer);
    assert!(slot.upgrade().is_some());
    drop(inner);
    assert!(
        slot.upgrade().is_none(),
        "idle paths must not retain lock slots"
    );
    assert!(!slots().lock().unwrap().contains_key(&key));
}

#[test]
fn slot_reclamation_preserves_exclusion_with_contending_waiters() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("counter.txt");
    let active = AtomicUsize::new(0);
    let start = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                start.wait();
                for _ in 0..64 {
                    let _guard = lock_exclusive(&target).unwrap();
                    assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                    std::thread::yield_now();
                    assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                }
            });
        }
    });
    assert!(!slots()
        .lock()
        .unwrap()
        .contains_key(&lock_path_for(&target).unwrap()));
}

#[test]
fn distinct_completed_paths_leave_no_registry_entries() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..128 {
        let target = root.path().join(format!("script-{index}.js"));
        let guard = lock_exclusive(&target).unwrap();
        let key = guard.path.clone();
        drop(guard);
        assert!(!slots().lock().unwrap().contains_key(&key));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn failed_acquisition_reclaims_its_slot_without_releasing_another_owner() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("contended.txt");
    let (key, owner) = open_lock(&target).unwrap();
    owner.lock().unwrap();
    for _ in 0..2 {
        assert!(try_lock_exclusive(&target, Duration::ZERO)
            .unwrap()
            .is_none());
        assert!(!slots().lock().unwrap().contains_key(&key));
    }
    drop(owner);
    assert!(try_lock_exclusive(&target, Duration::ZERO)
        .unwrap()
        .is_some());
    assert!(!slots().lock().unwrap().contains_key(&key));
}

#[test]
fn isolated_configuration_does_not_split_cross_process_lock_identity() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("script.js");
    let _guard = lock_exclusive(&target).unwrap();
    let mut command = child_command("exclusive", &target, false);
    command
        .env("HOME", root.path())
        .env("USERPROFILE", root.path())
        .env("APPDATA", root.path().join("AppData/Roaming"))
        .env("LOCALAPPDATA", root.path().join("AppData/Local"))
        .env("XDG_CACHE_HOME", root.path().join(".cache"))
        .env("KANZEI_HOME", root.path().join(".kanzei"));
    let (mut process, acquired) = spawn_child(command);
    assert!(process.wait().unwrap().success());
    assert!(!acquired);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn missing_paths_preserve_identity_after_creation_with_different_case() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("新目录/ÜScript.js");
    let key = lock_path_for(&target).unwrap();
    let _guard = lock_exclusive(&target).unwrap();
    let created = root.path().join("新目录/üscript.JS");
    std::fs::create_dir_all(created.parent().unwrap()).unwrap();
    std::fs::write(&created, "contents").unwrap();
    let alias = root.path().join("新目录/ÜSCRIPT.Js");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "contents");
    assert_eq!(key, lock_path_for(&alias).unwrap());
    assert!(
        std::thread::spawn(move || try_lock_exclusive(&alias, Duration::ZERO)
            .unwrap()
            .is_none())
        .join()
        .unwrap()
    );
}

#[cfg(windows)]
#[test]
fn missing_long_paths_keep_identity_after_creation_adds_a_verbatim_prefix() {
    let root = tempfile::tempdir().unwrap();
    let target = root
        .path()
        .join("a".repeat(150))
        .join("b".repeat(100))
        .join("script.js");
    let key = lock_path_for(&target).unwrap();
    let _guard = lock_exclusive(&target).unwrap();
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "contents").unwrap();
    assert_eq!(key, lock_path_for(&target).unwrap());
    assert!(
        std::thread::spawn(move || try_lock_exclusive(&target, Duration::ZERO)
            .unwrap()
            .is_none())
        .join()
        .unwrap()
    );
}
