//! Bounded read model for a multi-project workbench. No recovery, event replay or writes.

use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use super::{SessionStore, StoreError};

impl SessionStore {
    /// A workbench read must not silently migrate an older database or interpret a newer
    /// schema. Startup recovery owns migration and retries; this check performs no writes.
    pub fn check_workspace_schema(&self) -> Result<(), StoreError> {
        let has_meta: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_meta')",
            [],
            |row| row.get(0),
        )?;
        let version: Option<i64> = if has_meta {
            self.connection
                .query_row(
                    "SELECT value FROM schema_meta WHERE key='schema_version'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .and_then(|value| value.parse().ok())
        } else {
            None
        };
        match version {
            Some(found) if found == super::SCHEMA_VERSION => Ok(()),
            Some(found) if found > super::SCHEMA_VERSION => Err(StoreError::UnsupportedSchema {
                found,
                supported: super::SCHEMA_VERSION,
            }),
            Some(found) => Err(StoreError::InvalidInput(format!(
                "状态库需要从 schema {found} 升级至 {}；应用启动恢复会处理，工作台查询保持只读",
                super::SCHEMA_VERSION
            ))),
            None => Err(StoreError::InvalidInput(
                "状态库缺少 schema 版本，等待应用恢复；工作台查询保持只读".into(),
            )),
        }
    }

    /// Count pending inputs in SQL; lightweight read-only summary for the workbench.
    pub fn workspace_summary(&self, _journal: &str) -> Result<Value, StoreError> {
        self.check_workspace_schema()?;
        let pending: u64 = self.connection.query_row(
            "SELECT COUNT(*) FROM session_inputs WHERE status='pending'",
            [],
            |row| row.get(0),
        )?;
        Ok(json!({
            "pending": pending,
            "recent_progress": null
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_schema_check_reports_old_or_new_versions_without_migrating() {
        let store = SessionStore::open_in_memory().unwrap();
        for version in [
            super::super::SCHEMA_VERSION - 1,
            super::super::SCHEMA_VERSION + 1,
        ] {
            store
                .connection
                .execute(
                    "UPDATE schema_meta SET value=?1 WHERE key='schema_version'",
                    [version.to_string()],
                )
                .unwrap();
            assert!(store.check_workspace_schema().is_err());
            let still: String = store
                .connection
                .query_row(
                    "SELECT value FROM schema_meta WHERE key='schema_version'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(still, version.to_string());
        }
    }
}
