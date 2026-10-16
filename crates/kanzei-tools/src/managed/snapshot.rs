//! Recovery payload storage and incremental managed-tree snapshots.
use super::MANAGED_ROOTS;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Payloads above this per-snapshot memory budget use temporary disk storage. This is
/// a storage decision, not a limit on whether ordinary shell commands can run.
pub(crate) const MANAGED_SNAPSHOT_MEMORY_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub(super) enum SnapshotData {
    Memory(Vec<u8>),
    Disk(PathBuf),
}
#[derive(Debug)]
pub(super) struct SnapshotContent {
    digest: String,
    size: u64,
    pub(super) fingerprint: String,
    pub(super) data: SnapshotData,
}
impl PartialEq for SnapshotContent {
    fn eq(&self, other: &Self) -> bool {
        self.digest == other.digest
    }
}
impl Drop for SnapshotContent {
    fn drop(&mut self) {
        if let SnapshotData::Disk(path) = &self.data {
            if let Err(error) = std::fs::remove_file(path) {
                tracing::warn!(path=%path.display(), %error, "managed snapshot cleanup failed");
            }
        }
    }
}
fn snapshot_content_cache(
) -> &'static std::sync::Mutex<BTreeMap<String, std::sync::Weak<SnapshotContent>>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<BTreeMap<String, std::sync::Weak<SnapshotContent>>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

impl SnapshotContent {
    fn temporary_payload() -> std::io::Result<(PathBuf, std::fs::File)> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let target = std::env::temp_dir().join(format!(
            "kanzei-snapshot-{}-{now}-{sequence}",
            std::process::id()
        ));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        Ok((target, file))
    }

    fn fit_budget(content: Arc<Self>, memory_budget: u64) -> std::io::Result<Arc<Self>> {
        use std::io::Write;
        let SnapshotData::Memory(bytes) = &content.data else {
            return Ok(content);
        };
        if bytes.len() as u64 <= memory_budget {
            return Ok(content);
        }
        // An unchanged file can move later in the traversal after another file
        // is added. Storage selection applies to reused payloads too; it does
        // not require reopening or hashing the managed source.
        let (target, mut file) = Self::temporary_payload()?;
        let spilled = Arc::new(Self {
            digest: content.digest.clone(),
            size: content.size,
            fingerprint: content.fingerprint.clone(),
            data: SnapshotData::Disk(target),
        });
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(spilled)
    }

    pub(super) fn bytes(&self) -> std::io::Result<Vec<u8>> {
        match &self.data {
            SnapshotData::Memory(bytes) => Ok(bytes.clone()),
            SnapshotData::Disk(path) => std::fs::read(path),
        }
    }
    fn capture(path: &Path, memory_budget: u64) -> std::io::Result<std::sync::Arc<Self>> {
        use sha2::{Digest, Sha256};
        use std::io::{Read, Seek, Write};
        use std::sync::{Arc, Weak};
        let mut file = std::fs::File::open(path)?;
        let mut sha = Sha256::new();
        let mut fnv = 0xcbf2_9ce4_8422_2325u64;
        let mut buffer = [0u8; 64 * 1024];
        let mut memory = Vec::new();
        let mut total = 0u64;
        let mut in_memory = file.metadata()?.len() <= memory_budget;
        loop {
            let size = file.read(&mut buffer)?;
            if size == 0 {
                break;
            }
            total += size as u64;
            sha.update(&buffer[..size]);
            for byte in &buffer[..size] {
                fnv ^= u64::from(*byte);
                fnv = fnv.wrapping_mul(0x100_0000_01b3);
            }
            if in_memory {
                if memory.len() as u64 + size as u64 <= memory_budget {
                    memory.extend_from_slice(&buffer[..size]);
                } else {
                    // A concurrently growing file also moves to disk storage.
                    in_memory = false;
                    memory = Vec::new();
                }
            }
        }
        let digest = format!("{:x}", sha.finalize());
        let cached = snapshot_content_cache()
            .lock()
            .map_err(|_| std::io::Error::other("snapshot cache lock poisoned"))?
            .get(&digest)
            .and_then(Weak::upgrade);
        if let Some(content) = cached {
            return Ok(content);
        }
        let data = if in_memory {
            SnapshotData::Memory(memory)
        } else {
            let (target, mut copy) = Self::temporary_payload()?;
            // The RAII owner cleans the file even when copying or validating fails.
            let owner = SnapshotContent {
                digest: digest.clone(),
                size: total,
                fingerprint: String::new(),
                data: SnapshotData::Disk(target.clone()),
            };
            file.rewind()?;
            let mut copied = Sha256::new();
            loop {
                let size = file.read(&mut buffer)?;
                if size == 0 {
                    break;
                }
                copy.write_all(&buffer[..size])?;
                copied.update(&buffer[..size]);
            }
            copy.sync_all()?;
            drop(copy);
            if format!("{:x}", copied.finalize()) != digest {
                return Err(std::io::Error::other(
                    "managed source changed while taking snapshot",
                ));
            }
            // Move ownership without duplicating a potentially large payload.
            let mut owner = owner;
            std::mem::replace(&mut owner.data, SnapshotData::Memory(Vec::new()))
        };
        let content = Arc::new(SnapshotContent {
            digest: digest.clone(),
            size: total,
            fingerprint: format!("fnv-{fnv:016x}"),
            data,
        });
        snapshot_content_cache()
            .lock()
            .map_err(|_| std::io::Error::other("snapshot cache lock poisoned"))?
            .insert(digest, Arc::downgrade(&content));
        Ok(content)
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::{Backend, Lease};
#[cfg(not(windows))]
mod portable {
    pub(super) struct Backend;
    pub(super) struct Lease;
    impl Backend {
        pub(super) fn new() -> std::io::Result<Self> {
            Ok(Self)
        }
        pub(super) fn acquire(&self, _: &std::path::Path) -> std::io::Result<Option<Lease>> {
            Ok(None)
        }
    }
    impl Lease {
        pub(super) fn valid(&self) -> bool {
            false
        }
    }
}
#[cfg(not(windows))]
use portable::{Backend, Lease};

#[derive(Debug, Default, Clone)]
pub(crate) struct SnapshotStats {
    pub files_read: u64,
    pub bytes_read: u64,
    pub files_reused: u64,
    pub directories_read: u64,
    pub directories_reused: u64,
    pub uncached_paths: u64,
}
#[derive(Clone)]
struct Child {
    path: PathBuf,
    directory: bool,
}
struct CachedDirectory {
    lease: Option<Lease>,
    children: Vec<Child>,
}
struct CachedFile {
    lease: Option<Lease>,
    content: Arc<SnapshotContent>,
}
#[derive(Default)]
struct CollectorState {
    root: Option<PathBuf>,
    directories: BTreeMap<PathBuf, CachedDirectory>,
    files: BTreeMap<PathBuf, CachedFile>,
    backend: Option<Backend>,
}
/// Owned by one tool/run and any background snapshots transferred from it.
/// No process-global strong project cache keeps kernel handles alive forever.
#[derive(Default)]
pub(crate) struct SnapshotCollector {
    state: Mutex<CollectorState>,
}

#[derive(Clone)]
pub(crate) struct ManagedSnapshot {
    pub(super) files: BTreeMap<String, Option<Arc<SnapshotContent>>>,
    pub(super) error: Option<String>,
    collector: Arc<SnapshotCollector>,
    pub(crate) stats: SnapshotStats,
}
impl std::fmt::Debug for ManagedSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedSnapshot")
            .field("files", &self.files)
            .field("error", &self.error)
            .field("stats", &self.stats)
            .finish()
    }
}
impl PartialEq for ManagedSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.files == other.files && self.error == other.error
    }
}
impl Default for ManagedSnapshot {
    fn default() -> Self {
        Self {
            files: BTreeMap::new(),
            error: None,
            collector: Arc::new(SnapshotCollector::default()),
            stats: SnapshotStats::default(),
        }
    }
}
impl ManagedSnapshot {
    pub(crate) fn capture(project_root: &Path) -> Self {
        Arc::new(SnapshotCollector::default()).capture(project_root)
    }
    pub(crate) fn refresh(&self, project_root: &Path) -> Self {
        self.collector.capture(project_root)
    }
    pub(crate) fn ensure_complete(&self) -> Result<(), String> {
        self.error
            .as_ref()
            .map_or(Ok(()), |error| Err(error.clone()))
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.error.is_none() && self.files.values().all(Option::is_some)
    }

    /// 把 `current` 里 `paths` 指定的这些路径吸收进本基线(其余路径保持原样)。
    ///
    /// D-258:吸收粒度从「前缀内全部路径」收窄为「窗口期间实际变化的路径」。
    /// 整前缀吸收会让后台进程在窗口内偷写的文件(专用工具没碰、窗口前后却有变化)
    /// 也被固化进基线;按路径吸收只认「打开/关闭两次镜像之间的差异」,后台进程
    /// 必须和专用工具写同一批文件才能蒙混——写别的文件会在窗口关闭后留痕,
    /// 被守卫下一轮对账判成越界回滚。
    ///
    /// `paths` 里在 `current` 中不存在的路径视为被删除(窗口内被删的托管文件同样
    /// 要吸收,否则守卫会把"专用工具的归档移动"当成越界删除再给写回去)。
    pub(crate) fn absorb_paths(&mut self, current: &ManagedSnapshot, paths: &[&str]) {
        for &path in paths {
            match current.files.get(path) {
                Some(content) => {
                    self.files.insert(path.to_string(), content.clone());
                }
                None => {
                    self.files.remove(path);
                }
            }
        }
    }
}

impl SnapshotCollector {
    pub(crate) fn capture(self: &Arc<Self>, project_root: &Path) -> ManagedSnapshot {
        let mut snapshot = ManagedSnapshot {
            collector: self.clone(),
            ..Default::default()
        };
        let result = (|| -> std::io::Result<()> {
            snapshot_content_cache()
                .lock()
                .map_err(|_| std::io::Error::other("snapshot cache lock poisoned"))?
                .retain(|_, content| content.strong_count() > 0);
            let mut state = self
                .state
                .lock()
                .map_err(|_| std::io::Error::other("snapshot collector lock poisoned"))?;
            if state.root.as_deref() != Some(project_root) {
                state.files.clear();
                state.directories.clear();
                state.root = Some(project_root.to_path_buf());
            }
            if state.backend.is_none() {
                state.backend = Some(Backend::new()?);
            }
            let mut visited_files = BTreeSet::new();
            let mut visited_directories = BTreeSet::new();
            let mut memory_budget = MANAGED_SNAPSHOT_MEMORY_LIMIT;
            for root in MANAGED_ROOTS {
                collect_directory(
                    &project_root.join(root),
                    project_root,
                    &mut state,
                    &mut snapshot,
                    &mut memory_budget,
                    &mut visited_files,
                    &mut visited_directories,
                )?;
            }
            state.files.retain(|path, _| visited_files.contains(path));
            state
                .directories
                .retain(|path, _| visited_directories.contains(path));
            Ok(())
        })();
        if let Err(error) = result {
            snapshot.error = Some(error.to_string());
        }
        tracing::debug!(?snapshot.stats, complete=snapshot.is_complete(), "managed snapshot captured");
        snapshot
    }
}
fn collect_directory(
    directory: &Path,
    project_root: &Path,
    state: &mut CollectorState,
    snapshot: &mut ManagedSnapshot,
    memory_budget: &mut u64,
    visited_files: &mut BTreeSet<PathBuf>,
    visited_directories: &mut BTreeSet<PathBuf>,
) -> std::io::Result<()> {
    let children = if let Some(cached) = state
        .directories
        .get(directory)
        .filter(|cached| cached.lease.as_ref().is_some_and(Lease::valid))
    {
        snapshot.stats.directories_reused += 1;
        cached.children.clone()
    } else {
        // Arm before reading: a change during enumeration invalidates this
        // listing rather than creating an unobserved gap between read and watch.
        let lease = state
            .backend
            .as_ref()
            .expect("collector backend initialized")
            .acquire(directory)?;
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        snapshot.stats.directories_read += 1;
        let mut children = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("lock") {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_file() || kind.is_dir() {
                children.push(Child {
                    path,
                    directory: kind.is_dir(),
                });
            }
        }
        children.sort_by(|a, b| a.path.cmp(&b.path));
        let lease = lease.filter(Lease::valid);
        if lease.is_none() {
            snapshot.stats.uncached_paths += 1;
        }
        state.directories.insert(
            directory.to_path_buf(),
            CachedDirectory {
                lease,
                children: children.clone(),
            },
        );
        children
    };
    visited_directories.insert(directory.to_path_buf());
    for child in children {
        if child.directory {
            collect_directory(
                &child.path,
                project_root,
                state,
                snapshot,
                memory_budget,
                visited_files,
                visited_directories,
            )?;
        } else {
            let content = if let Some(cached) = state
                .files
                .get(&child.path)
                .filter(|cached| cached.lease.as_ref().is_some_and(Lease::valid))
            {
                snapshot.stats.files_reused += 1;
                cached.content.clone()
            } else {
                let lease = state
                    .backend
                    .as_ref()
                    .expect("collector backend initialized")
                    .acquire(&child.path)?;
                let content = SnapshotContent::capture(&child.path, *memory_budget)?;
                snapshot.stats.files_read += 1;
                snapshot.stats.bytes_read += content.size;
                let lease = lease.filter(Lease::valid);
                if lease.is_none() {
                    snapshot.stats.uncached_paths += 1;
                }
                state.files.insert(
                    child.path.clone(),
                    CachedFile {
                        lease,
                        content: content.clone(),
                    },
                );
                content
            };
            let content = SnapshotContent::fit_budget(content, *memory_budget)?;
            state
                .files
                .get_mut(&child.path)
                .expect("selected file is cached")
                .content = content.clone();
            if let SnapshotData::Memory(bytes) = &content.data {
                *memory_budget = memory_budget.saturating_sub(bytes.len() as u64);
            }
            let relative = child
                .path
                .strip_prefix(project_root)
                .map_err(std::io::Error::other)?;
            snapshot.files.insert(
                relative.display().to_string().replace('\\', "/"),
                Some(content),
            );
            visited_files.insert(child.path);
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "kz-incremental-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(root.join(".kanzei/memory")).unwrap();
            for id in 0..24 {
                std::fs::write(
                    root.join(format!(".kanzei/memory/M-{id:03}.md")),
                    format!("entry {id}: original"),
                )
                .unwrap();
            }
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn unchanged_capture_reads_zero_bytes_and_one_edit_only_rehashes_that_file() {
        let fixture = Fixture::new();
        let collector = Arc::new(SnapshotCollector::default());
        let before = collector.capture(&fixture.0);
        before.ensure_complete().unwrap();
        assert_eq!(before.stats.files_read, 24);
        assert_eq!(
            before.stats.uncached_paths, 0,
            "local test volume must grant read-cache leases"
        );
        let clean = collector.capture(&fixture.0);
        clean.ensure_complete().unwrap();
        assert_eq!(clean.stats.bytes_read, 0);
        assert_eq!(clean.stats.directories_read, 0);
        assert_eq!(clean.stats.files_reused, 24);
        let path = fixture.0.join(".kanzei/memory/M-007.md");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, b"entry 7: changed!").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let changed = collector.capture(&fixture.0);
        changed.ensure_complete().unwrap();
        assert_eq!(changed.stats.files_read, 1);
        assert_eq!(changed.stats.files_reused, 23);
        let diff = crate::managed::diff(&before, &changed).unwrap();
        assert_eq!(diff.modified, [".kanzei/memory/M-007.md"]);
        assert_eq!(
            std::fs::metadata(path).unwrap().modified().unwrap(),
            modified
        );
    }
    #[test]
    fn atomic_replacement_renames_and_new_directories_invalidate_leases_without_blocking_writers() {
        let fixture = Fixture::new();
        let before = ManagedSnapshot::capture(&fixture.0);
        before.ensure_complete().unwrap();
        let path = fixture.0.join(".kanzei/memory/M-007.md");
        let writer = std::thread::spawn(move || {
            crate::atomic_file::write_atomic(&path, "replacement").unwrap()
        });
        writer.join().unwrap();
        let after = before.refresh(&fixture.0);
        after.ensure_complete().unwrap();
        assert_eq!(after.stats.files_read, 1);
        assert_eq!(
            after.files[".kanzei/memory/M-007.md"]
                .as_ref()
                .unwrap()
                .bytes()
                .unwrap(),
            b"replacement"
        );
        let folder = fixture.0.join(".kanzei/memory/archive");
        std::fs::create_dir(&folder).unwrap();
        std::fs::rename(
            fixture.0.join(".kanzei/memory/M-009.md"),
            folder.join("renamed.md"),
        )
        .unwrap();
        let renamed = after.refresh(&fixture.0);
        renamed.ensure_complete().unwrap();
        let change = crate::managed::diff(&after, &renamed).unwrap();
        assert_eq!(change.deleted, [".kanzei/memory/M-009.md"]);
        assert_eq!(change.created, [".kanzei/memory/archive/renamed.md"]);
        std::fs::remove_file(folder.join("renamed.md")).unwrap();
        let deleted = renamed.refresh(&fixture.0);
        deleted.ensure_complete().unwrap();
        assert!(!deleted
            .files
            .contains_key(".kanzei/memory/archive/renamed.md"));
    }
    #[test]
    fn live_open_writer_and_background_window_baseline_cannot_hide_changes() {
        use std::io::{Seek, Write};
        let fixture = Fixture::new();
        let mut before = ManagedSnapshot::capture(&fixture.0);
        let path = fixture.0.join(".kanzei/memory/M-001.md");
        let mut writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        writer.write_all(b"unauthorized").unwrap();
        writer.flush().unwrap();
        let changed = before.refresh(&fixture.0);
        changed.ensure_complete().unwrap();
        assert!(crate::managed::diff(&before, &changed).is_some());
        // Absorbing a legal window changes the baseline, not the live collector.
        before.absorb_paths(&changed, &[".kanzei/memory/M-001.md"]);
        writer.rewind().unwrap();
        writer.write_all(b"next-illegal").unwrap();
        writer.flush().unwrap();
        let next = before.refresh(&fixture.0);
        next.ensure_complete().unwrap();
        assert!(crate::managed::diff(&before, &next).is_some());
        drop(writer);
    }
    #[test]
    fn reused_payloads_obey_memory_budget_after_earlier_file_is_added() {
        let fixture = Fixture::new();
        let last = fixture.0.join(".kanzei/memory/Z-large.md");
        let size = (MANAGED_SNAPSHOT_MEMORY_LIMIT * 3 / 4) as usize;
        std::fs::write(&last, vec![b'z'; size]).unwrap();
        let before = ManagedSnapshot::capture(&fixture.0);
        before.ensure_complete().unwrap();
        assert!(matches!(
            before.files[".kanzei/memory/Z-large.md"]
                .as_ref()
                .unwrap()
                .data,
            SnapshotData::Memory(_)
        ));
        std::fs::write(
            fixture.0.join(".kanzei/memory/A-large.md"),
            vec![b'a'; size],
        )
        .unwrap();
        let after = before.refresh(&fixture.0);
        after.ensure_complete().unwrap();
        assert_eq!(after.stats.files_read, 1);
        assert_eq!(after.stats.files_reused, 25);
        let memory: usize = after
            .files
            .values()
            .flatten()
            .map(|content| match &content.data {
                SnapshotData::Memory(bytes) => bytes.len(),
                SnapshotData::Disk(_) => 0,
            })
            .sum();
        assert!(memory as u64 <= MANAGED_SNAPSHOT_MEMORY_LIMIT);
        assert!(matches!(
            after.files[".kanzei/memory/Z-large.md"]
                .as_ref()
                .unwrap()
                .data,
            SnapshotData::Disk(_)
        ));
        assert_eq!(
            after.files[".kanzei/memory/Z-large.md"]
                .as_ref()
                .unwrap()
                .bytes()
                .unwrap(),
            vec![b'z'; size]
        );
    }
    #[test]
    fn releasing_last_owner_cancels_pending_requests_and_releases_all_file_handles() {
        let fixture = Fixture::new();
        let collector = Arc::new(SnapshotCollector::default());
        let snapshot = collector.capture(&fixture.0);
        snapshot.ensure_complete().unwrap();
        let weak = Arc::downgrade(&collector);
        drop(collector);
        assert!(
            weak.upgrade().is_some(),
            "background/baseline snapshot owns collector"
        );
        drop(snapshot);
        assert!(weak.upgrade().is_none());
        std::fs::rename(
            fixture.0.join(".kanzei/memory"),
            fixture.0.join(".kanzei/renamed"),
        )
        .unwrap();
    }
}
