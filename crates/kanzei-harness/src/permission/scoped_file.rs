//! Directory capabilities enforce the root boundary during actual file I/O.

use std::io;
use std::path::{Path, PathBuf};

use cap_std::fs::Dir;
use kanzei_base::atomic_file::{write_atomic_bytes_in, AtomicFileSystem};

pub struct ScopedFile {
    root: Dir,
    relative: PathBuf,
    path: PathBuf,
}

pub enum EntryKind {
    File,
    Directory,
    Other,
}
pub struct ScopedEntry {
    pub name: std::ffi::OsString,
    pub kind: EntryKind,
}

impl ScopedFile {
    pub fn resolve(cwd: &Path, project_root: &Path, requested: &Path) -> io::Result<Self> {
        let path = super::filesystem::resolve_target(&cwd.join(requested))?;
        for root in [cwd, project_root] {
            let root = kanzei_base::path_form::canonical(root)?;
            if let Ok(relative) = path.strip_prefix(&root) {
                let relative = Path::new(".").join(relative);
                return Ok(Self {
                    root: Dir::open_ambient_dir(root, cap_std::ambient_authority())?,
                    relative,
                    path,
                });
            }
        }
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "FILESYSTEM_OUTSIDE_WORKSPACE: {} is outside the code tree and project root",
                path.display()
            ),
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn open(&self) -> io::Result<std::fs::File> {
        self.root.open(&self.relative).map(|file| file.into_std())
    }
    pub fn read_to_string(&self) -> io::Result<String> {
        self.root.read_to_string(&self.relative)
    }
    pub fn metadata(&self) -> io::Result<cap_std::fs::Metadata> {
        self.root.metadata(&self.relative)
    }
    pub fn write_atomic_cas(
        &self,
        content: &str,
        expected_hash: &str,
        fingerprint: impl Fn(&str) -> String,
    ) -> io::Result<()> {
        // Only coordination storage is opened by this lock. Document reads and
        // replacements stay on the authorized directory capability below.
        let _lock = kanzei_base::atomic_file::lock_exclusive(&self.path)?;
        self.write_atomic(content.as_bytes(), || {
            let current = match self.read_to_string() {
                Ok(content) => content,
                Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
                Err(error) => return Err(error),
            };
            if fingerprint(&current) != expected_hash {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "file revision changed before replacement",
                ));
            }
            Ok(())
        })
    }
    pub fn create_parent(&self) -> io::Result<()> {
        self.root.create_dir_all(
            self.relative
                .parent()
                .expect("scoped relative path has a parent"),
        )
    }
    pub fn entries(&self) -> io::Result<impl Iterator<Item = io::Result<ScopedEntry>>> {
        Ok(self.root.read_dir(&self.relative)?.map(|entry| {
            let entry = entry?;
            let kind = entry.file_type()?;
            let kind = if kind.is_symlink() {
                EntryKind::Other
            } else if kind.is_file() {
                EntryKind::File
            } else if kind.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::Other
            };
            Ok(ScopedEntry {
                name: entry.file_name(),
                kind,
            })
        }))
    }
    pub fn write_atomic(
        &self,
        bytes: &[u8],
        check: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        write_atomic_bytes_in(self, &self.relative, bytes, check)
    }
}

impl AtomicFileSystem for ScopedFile {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.root.create_dir_all(path)
    }
    fn create_new(&self, path: &Path) -> io::Result<std::fs::File> {
        self.root
            .open_with(
                path,
                cap_std::fs::OpenOptions::new().create_new(true).write(true),
            )
            .map(|file| file.into_std())
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.root.remove_file(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.root.rename(from, &self.root, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_cas_interoperates_with_file_locks_and_rejects_a_replaced_parent() {
        let root = fixture();
        let target = root.join("code/child/cache.txt");
        let file = ScopedFile::resolve(&root.join("code"), &root.join("project"), &target).unwrap();
        file.write_atomic(b"first", || Ok(())).unwrap();
        let fingerprint = |text: &str| kanzei_base::content_hash(text.as_bytes());
        {
            let _outer = kanzei_base::atomic_file::lock_exclusive(&target).unwrap();
            file.write_atomic_cas("second", &fingerprint("first"), fingerprint)
                .unwrap();
            assert!(file
                .write_atomic_cas("stale", &fingerprint("first"), fingerprint)
                .is_err());
        }
        assert_eq!(file.read_to_string().unwrap(), "second");
        assert!(!target.with_extension("lock").exists());
        assert!(!root.join("code/.kanzei").exists());
        std::fs::write(root.join("outside/cache.txt"), "OUTSIDE_CANARY").unwrap();
        std::fs::rename(root.join("code/child"), root.join("code/child-before-link")).unwrap();
        link(&root.join("outside"), &root.join("code/child"));
        assert!(file
            .write_atomic_cas("escaped", &fingerprint("second"), fingerprint)
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("outside/cache.txt")).unwrap(),
            "OUTSIDE_CANARY"
        );
        assert!(!root.join("outside/cache.lock").exists());
    }

    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-scoped-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for dir in ["code/child", "project", "outside"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        root
    }

    fn link(target: &Path, path: &Path) {
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(path.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap()
            .status
            .success());
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, path).unwrap();
    }

    #[test]
    fn code_and_project_roots_are_allowed_but_external_paths_and_links_are_rejected() {
        let root = fixture();
        let code = root.join("code");
        let project = root.join("project");
        for base in [&code, &project, &root.join("outside")] {
            std::fs::write(base.join("file"), "original").unwrap();
        }
        for base in [&code, &project] {
            assert_eq!(
                ScopedFile::resolve(&code, &project, &base.join("file"))
                    .unwrap()
                    .read_to_string()
                    .unwrap(),
                "original"
            );
        }
        assert!(ScopedFile::resolve(&code, &project, &root.join("outside/file")).is_err());
        assert!(ScopedFile::resolve(&code, &project, Path::new("../outside/new/file")).is_err());
        let junction = code.join("escape");
        link(&root.join("outside"), &junction);
        assert!(ScopedFile::resolve(&code, &project, &junction.join("file")).is_err());
        assert!(ScopedFile::resolve(&code, &project, &junction.join("new")).is_err());
        #[cfg(windows)]
        std::fs::remove_dir(&junction).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(&junction).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn actual_read_and_atomic_write_reject_a_parent_replaced_after_resolution() {
        let root = fixture();
        let code = root.join("code");
        std::fs::write(code.join("child/file"), "inside").unwrap();
        std::fs::write(root.join("outside/file"), "outside").unwrap();
        let access =
            ScopedFile::resolve(&code, &root.join("project"), Path::new("child/file")).unwrap();
        std::fs::rename(code.join("child"), code.join("saved")).unwrap();
        link(&root.join("outside"), &code.join("child"));
        assert!(access.open().is_err());
        assert!(access.write_atomic(b"modified", || Ok(())).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("outside/file")).unwrap(),
            "outside"
        );
        assert_eq!(
            std::fs::read_to_string(code.join("saved/file")).unwrap(),
            "inside"
        );
        #[cfg(windows)]
        std::fs::remove_dir(code.join("child")).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(code.join("child")).unwrap();
        drop(access);
        std::fs::remove_dir_all(root).unwrap();
    }
}
