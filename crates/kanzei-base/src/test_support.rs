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

#[cfg(test)]
mod tests {
    use super::*;

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
