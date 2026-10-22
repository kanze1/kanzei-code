//! Poison recovery for registries and caches whose entries remain independently valid.
//! Transactional state must repair its invariant before opting into this policy.
use std::sync::{Mutex, MutexGuard};

pub trait MutexPoisonExt<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexPoisonExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("recovering poisoned registry mutex");
            self.clear_poison();
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_remains_available_after_unwind() {
        let registry = Mutex::new(std::collections::HashSet::from(["existing"]));
        let result = std::panic::catch_unwind(|| {
            let _guard = registry.lock().unwrap();
            panic!("injected registry owner failure");
        });
        assert!(result.is_err());
        assert!(registry.lock_or_recover().remove("existing"));
        assert!(!registry.is_poisoned());
        assert!(registry.lock().unwrap().is_empty());
    }
}
