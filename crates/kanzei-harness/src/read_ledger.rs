//! R-367: conversation-local file versions. Cloning shares receipts; a child creates its own.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Default)]
pub struct ReadLedger(Arc<Mutex<HashMap<PathBuf, String>>>);

impl ReadLedger {
    fn key(path: &Path) -> PathBuf {
        let canonical = path
            .parent()
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .zip(path.file_name())
            .map(|(parent, name)| parent.join(name))
            .unwrap_or_else(|| path.to_path_buf());
        #[cfg(windows)]
        let canonical = PathBuf::from(canonical.to_string_lossy().to_lowercase());
        canonical
    }
    pub fn record(&self, path: &Path, hash: String) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(Self::key(path), hash);
    }
    pub fn expected(&self, path: &Path) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&Self::key(path))
            .cloned()
    }
    pub fn forget(&self, path: &Path) {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&Self::key(path));
    }
    pub fn clear(&self) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }
}
