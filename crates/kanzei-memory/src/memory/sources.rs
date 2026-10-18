//! Engine-owned, immutable source artifacts share the memory tree transaction.
use super::{MemoryScope, MemoryStore};
use std::path::{Component, Path, PathBuf};

impl MemoryStore {
    pub fn write_source_artifact(&self, name: &str, content: &str) -> anyhow::Result<PathBuf> {
        anyhow::ensure!(
            self.scope == MemoryScope::Project,
            "会话来源必须保存在所属项目中"
        );
        let mut components = Path::new(name).components();
        anyhow::ensure!(
            matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
            "来源文件名必须是单个普通名称"
        );
        super::privacy::reject_credentials(content)?;
        let directory = self.root.join("sources");
        let path = directory.join(name);
        let root = self
            .project_root
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("来源文件缺少所属项目"))?;
        let file = kanzei_harness::permission::ScopedFile::resolve(root, root, &path)?;
        anyhow::ensure!(
            kanzei_harness::permission::FileResource::resolve(root, &path)?
                .sensitivity()
                .is_none(),
            "来源文件不能指向敏感资源"
        );
        let _lock = self.tree_lock()?;
        match file.read_to_string() {
            Ok(existing) => {
                anyhow::ensure!(
                    existing == content,
                    "来源文件与冻结快照不一致，不能覆盖原始证据"
                );
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        file.write_atomic(content.as_bytes(), || match file.read_to_string() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
            Ok(_) => Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "来源文件归属已变化",
            )),
        })?;
        self.record_write_log(&path, Some(content.as_bytes().to_vec()))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanitized_structured_source_is_not_reinterpreted_as_a_secret_by_json_delimiters() {
        let root = std::env::temp_dir().join(format!(
            "kz-memory-json-source-{}-{}",
            std::process::id(),
            super::super::store::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = MemoryStore::project(&root);
        let safe = super::super::redact_credentials("喜欢简洁。api_key=opaque-source-secret");
        let serialized = serde_json::json!({"text":safe}).to_string();
        assert!(store
            .write_source_artifact("redacted.json", &serialized)
            .is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn immutable_source_creation_is_idempotent_and_rejects_tampering_escape_and_credentials() {
        let root = std::env::temp_dir().join(format!(
            "kz-memory-source-{}-{}",
            std::process::id(),
            super::super::store::now_ms()
        ));
        let store = MemoryStore::project(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = store
            .write_source_artifact("snapshot.json", r#"{"preference":"简洁排版"}"#)
            .unwrap();
        assert_eq!(
            store
                .write_source_artifact("snapshot.json", r#"{"preference":"简洁排版"}"#)
                .unwrap(),
            path
        );
        assert!(store
            .write_source_artifact("snapshot.json", "different source")
            .is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"preference":"简洁排版"}"#
        );
        assert!(store
            .write_source_artifact("../outside.json", "data")
            .is_err());
        assert!(store
            .write_source_artifact("secret.json", "api_key=sk-proj-abcdefghijklmnop0123456789")
            .is_err());
        assert!(!store.root.join("sources/secret.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
