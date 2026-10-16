//! Tests that exercise the global store need a process-owned home. Mutating the
//! parent test process environment would redirect unrelated parallel readers.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) fn run(test_name: &str, test: impl FnOnce(&Path)) {
    const CHILD_CASE: &str = "KANZEI_MEMORY_TEST_CASE";
    if std::env::var(CHILD_CASE).as_deref() == Ok(test_name) {
        let home = PathBuf::from(std::env::var_os("KANZEI_HOME").expect("isolated test home"));
        test(&home);
        return;
    }

    static NEXT_HOME: AtomicU64 = AtomicU64::new(0);
    let home = std::env::temp_dir().join(format!(
        "kz-memory-home-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_HOME.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::create_dir_all(&home).unwrap();
    let parent_home = std::env::var_os("KANZEI_HOME");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD_CASE, test_name)
        .env("KANZEI_HOME", &home);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().expect("start isolated memory test");
    // The parent owns cleanup, after the child and all its worker threads have
    // exited. No parallel test can observe or remove this global store.
    let cleanup = std::fs::remove_dir_all(&home);
    assert_eq!(std::env::var_os("KANZEI_HOME"), parent_home);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && stdout.contains("1 passed; 0 failed;"),
        "isolated test {test_name} failed ({:?})\n{stdout}\n{stderr}",
        output.status.code(),
    );
    cleanup.expect("remove isolated memory test home after process exit");
}
