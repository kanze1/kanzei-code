//! Owned directories for engineering tests. Keep the guard alive until stores and workers close.
use std::ffi::OsStr;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct TestDir {
    path: PathBuf,
    owner: tempfile::TempDir,
}

impl TestDir {
    pub fn new(prefix: &str) -> Self {
        let owner = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .expect("create owned test directory");
        Self {
            path: owner.path().to_path_buf(),
            owner,
        }
    }

    /// Call after database/file/worker owners have been released to surface cleanup errors.
    pub fn close(self) -> io::Result<()> {
        self.owner.close()
    }
}

impl Deref for TestDir {
    type Target = PathBuf;
    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<OsStr> for TestDir {
    fn as_ref(&self) -> &OsStr {
        self.path.as_os_str()
    }
}

/// Execute exactly one test in a child with an isolated environment. A completion
/// receipt proves the closure ran; libtest's human-readable stdout is not a protocol.
pub fn in_subprocess(test_name: &str, environment: &[(&str, &OsStr)], test: impl FnOnce()) {
    const CHILD_CASE: &str = "KANZEI_ISOLATED_TEST_CASE";
    const RECEIPT: &str = "KANZEI_ISOLATED_TEST_RECEIPT";
    if std::env::var(CHILD_CASE).as_deref() == Ok(test_name) {
        test();
        std::fs::write(
            std::env::var_os(RECEIPT).expect("test receipt path"),
            test_name,
        )
        .expect("write completed test receipt");
        return;
    }
    let directory = TestDir::new("kz-test-process-");
    let receipt = directory.join("completed");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD_CASE, test_name)
        .env(RECEIPT, &receipt);
    for (key, value) in environment {
        command.env(key, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().expect("start isolated test");
    assert!(
        output.status.success(),
        "isolated {test_name}: {:?}\n{}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(receipt).expect("test closure must complete"),
        test_name
    );
    directory.close().expect("reclaim child test directory");
}

pub fn isolated_test(module: &str, name: &str, test: impl FnOnce()) {
    let (_, module) = module
        .split_once("::")
        .expect("test module below crate root");
    in_subprocess(&format!("{module}::{name}"), &[], test);
}

pub fn is_isolated_test() -> bool {
    std::env::var_os("KANZEI_ISOLATED_TEST_CASE").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_child_with_zero_matching_tests_is_rejected() {
        let result = std::panic::catch_unwind(|| {
            in_subprocess("no_such_isolated_test", &[], || {
                panic!("parent must not run test body")
            });
        });
        assert!(
            result.is_err(),
            "a zero-test libtest exit cannot supply a completion receipt"
        );
    }

    #[test]
    fn unwinding_reclaims_owned_directory_and_explicit_close_reports_completion() {
        let directory = TestDir::new("kz-owned-unwind-");
        let path = directory.to_path_buf();
        let failed = std::panic::catch_unwind(move || {
            std::fs::write(directory.join("failure.txt"), "evidence owned by outer run").unwrap();
            panic!("injected assertion failure");
        });
        assert!(failed.is_err());
        assert!(!path.exists());

        let directory = TestDir::new("kz-owned-close-");
        let path = directory.to_path_buf();
        directory.close().unwrap();
        assert!(!path.exists());
    }
}
