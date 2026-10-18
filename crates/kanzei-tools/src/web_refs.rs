//! R-365 B3/B4:session-scoped websearch result references used by webfetch(ref=...).
//! The registry stores only URL/title metadata under the project artifact directory.

use kanzei_harness::ToolCtx;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_REFERENCES: usize = 500;
mod evidence;
pub use evidence::{
    acquisition_for_url, record_acquisition, record_search, AcquisitionMethod, SourceScope,
    WebAcquisition, WebArtifact,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredReference {
    url: String,
    title: String,
    created_at_ms: u128,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ReferenceStore {
    references: BTreeMap<String, StoredReference>,
}

/// Stable, short ID; URL stays in the project-local registry, not in the visible ref string.
pub fn reference_id(url: &str) -> String {
    format!("w{}", &digest_hex(url.as_bytes())[..16])
}

/// Store one result reference atomically. Repeated results for the same URL reuse the ID.
pub fn remember(ctx: &ToolCtx, url: &str, title: &str) -> Result<String, String> {
    let parsed =
        reqwest::Url::parse(url).map_err(|error| format!("invalid result URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("web result URL must use http or https".into());
    }
    let id = reference_id(parsed.as_str());
    let path = registry_path(
        &ctx.project_root,
        ctx.session_id.as_deref().unwrap_or("anonymous"),
    );
    std::fs::create_dir_all(path.parent().expect("registry path has parent"))
        .map_err(|error| error.to_string())?;
    let _lock = crate::atomic_file::lock_exclusive(&path).map_err(|error| error.to_string())?;
    let mut store = read_store(&path)?;
    store.references.insert(
        id.clone(),
        StoredReference {
            url: parsed.to_string(),
            title: title.to_string(),
            created_at_ms: now_ms(),
        },
    );
    if store.references.len() > MAX_REFERENCES {
        let mut oldest: Vec<_> = store
            .references
            .iter()
            .map(|(id, entry)| (entry.created_at_ms, id.clone()))
            .collect();
        oldest.sort_unstable();
        for (_, old_id) in oldest
            .into_iter()
            .take(store.references.len() - MAX_REFERENCES)
        {
            store.references.remove(&old_id);
        }
    }
    let contents = serde_json::to_string_pretty(&store).map_err(|error| error.to_string())?;
    crate::atomic_file::write_atomic(&path, &contents).map_err(|error| error.to_string())?;
    Ok(id)
}

/// Resolve only a reference written for this exact project and session.
pub fn resolve(ctx: &ToolCtx, reference: &str) -> Result<String, String> {
    if reference.trim().is_empty() {
        return Err("ref must not be empty".into());
    }
    let path = registry_path(
        &ctx.project_root,
        ctx.session_id.as_deref().unwrap_or("anonymous"),
    );
    let store = read_store(&path)?;
    let entry = store
        .references
        .get(reference)
        .ok_or_else(|| format!("unknown web reference `{reference}` for this session"))?;
    let parsed = reqwest::Url::parse(&entry.url).map_err(|error| error.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("stored web reference is not an http(s) URL".into());
    }
    Ok(parsed.to_string())
}

fn read_store(path: &Path) -> Result<ReferenceStore, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ReferenceStore::default());
        }
        Err(error) => return Err(error.to_string()),
    };
    serde_json::from_str(&text).map_err(|error| format!("invalid web reference registry: {error}"))
}

fn registry_path(project_root: &Path, session_id: &str) -> PathBuf {
    let session_hash = digest_hex(session_id.as_bytes());
    project_root
        .join(".kanzei/artifacts/web/refs")
        .join(format!("{session_hash}.json"))
}

fn digest_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{remember, resolve, ToolCtx};

    #[test]
    fn a_legacy_timestamp_without_an_artifact_is_not_verified_source_evidence() {
        let root = std::env::temp_dir().join(format!(
            "kz-web-source-receipt-{}-{}",
            std::process::id(),
            super::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            session_id: Some("selected".into()),
            ..Default::default()
        };
        let url = "https://source.example/security";
        let id = remember(&ctx, url, "Security source").unwrap();
        let path = super::registry_path(&root, "selected");
        let mut registry: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        registry["references"][&id]["fetched_at_ms"] = serde_json::json!(super::now_ms());
        std::fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
        assert!(
            super::acquisition_for_url(&ctx, url, None)
                .unwrap()
                .is_none(),
            "a timestamp alone cannot prove that source evidence exists"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reference_is_project_and_session_scoped() {
        let root = std::env::temp_dir().join(format!(
            "kz-web-ref-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone()).with_session_id("session-a".into());
        let reference = remember(&ctx, "https://example.org/article?q=1", "Example").unwrap();
        assert_eq!(
            resolve(&ctx, &reference).unwrap(),
            "https://example.org/article?q=1"
        );
        let other_session =
            ToolCtx::new(root.clone(), root.clone()).with_session_id("session-b".into());
        assert!(resolve(&other_session, &reference).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_non_http_reference_urls() {
        let root = std::env::temp_dir().join(format!("kz-web-ref-invalid-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone());
        assert!(remember(&ctx, "javascript:alert(1)", "bad").is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
