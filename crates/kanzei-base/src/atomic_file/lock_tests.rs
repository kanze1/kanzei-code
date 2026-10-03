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
    std::fs::create_dir(root.join("bad.lock")).unwrap();
    assert!(try_lock_shared(&root.join("bad.md"), Duration::from_secs(10)).is_err());
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

fn child(mode: &str, path: &Path, hold: bool) -> (Child, bool) {
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
    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if let Some(value) = line.strip_prefix("LOCK_RESULT=") {
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
    let lock_path = lock_path_for(&path);
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
