//! Namespace migration preserves the adopted file state and its original evidence;
//! it does not invent a new user instruction or a new promotion decision.
use super::{render_entry, MemoryScope, MemoryStore};
use std::path::Path;
impl MemoryStore {
    pub(crate) fn import_personal_namespace(&self, source_root: &Path) -> anyhow::Result<()> {
        let original = source_root.join(".kanzei/memory");
        if original == self.root || !original.is_dir() {
            return Ok(());
        }
        let source = MemoryStore::open(MemoryScope::QqFriend, original);
        let entries = source.load_all_checked()?;
        if entries.is_empty() && source.pending_note_ids_checked()?.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(
            self.project_root
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("personal memory has no owner"))?,
        )?;
        let _lock = self.tree_lock()?;
        let source_db = kanzei_core::project_state_path(source_root);
        let target_db = super::memory_state_path(source_root);
        let mut mapping = std::collections::BTreeMap::new();
        let mut source_paths = std::collections::BTreeMap::new();
        if source.root.join("sources").is_dir() {
            for item in std::fs::read_dir(source.root.join("sources"))? {
                let path = item?.path();
                if !path.is_file() {
                    continue;
                }
                let name = format!(
                    "personal-source-{}.json",
                    kanzei_base::content_hash(path.to_string_lossy().as_bytes())
                );
                self.write_source_artifact(&name, &std::fs::read_to_string(&path)?)?;
                source_paths.insert(
                    format!(
                        ".kanzei/memory/sources/{}",
                        path.file_name()
                            .ok_or_else(|| anyhow::anyhow!("source has no file name"))?
                            .to_string_lossy()
                    ),
                    format!(".kanzei/memory/sources/{name}"),
                );
            }
        }
        // Reserve every destination identity before resolving cross-entry references.
        for (path, entry) in &entries {
            let origin = path.to_string_lossy().to_string();
            let all = self.load_all_checked()?;
            let imported = all.iter().find(|(_, e)| {
                e.extras
                    .iter()
                    .any(|(k, v)| k == "imported_from" && v == &origin)
            });
            let id = if let Some((_, imported)) = imported {
                imported.id.clone()
            } else {
                let mut reserved = entry.clone();
                reserved.id = self.next_id(&all);
                reserved.scope = MemoryScope::Personal.label().into();
                reserved.status = "candidate".into();
                reserved
                    .extras
                    .push(("imported_from".into(), origin.clone()));
                reserved
                    .extras
                    .push(("import_pending".into(), "true".into()));
                self.write_source_artifact(
                    &format!(
                        "personal-{}.json",
                        kanzei_base::content_hash(origin.as_bytes())
                    ),
                    &serde_json::json!({"path":origin,"entry":render_entry(entry)}).to_string(),
                )?;
                self.write_entry(&reserved, None)?;
                reserved.id
            };
            mapping.insert(entry.id.clone(), id);
        }
        for (path, mut entry) in entries {
            let origin = path.to_string_lossy().to_string();
            let id = &mapping[&entry.id];
            let all = self.load_all_checked()?;
            let (destination, adopted) = all
                .iter()
                .find(|(_, e)| &e.id == id)
                .ok_or_else(|| anyhow::anyhow!("reserved memory disappeared"))?;
            if !adopted.extras.iter().any(|(k, _)| k == "import_pending") {
                continue;
            }
            let old_id = entry.id.clone();
            let reviewed = entry.requires_review() && entry.status == "active";
            if reviewed {
                anyhow::ensure!(
                    entry.field("reviewed_content_revision")
                        == Some(super::memory_content_revision(&entry).as_str()),
                    "legacy approval does not match its content"
                );
            }
            entry.id = id.clone();
            entry.scope = MemoryScope::Personal.label().into();
            for (key, refs) in &mut entry.extras {
                if key == "refs" {
                    *refs = refs
                        .split_whitespace()
                        .map(|r| {
                            mapping
                                .get(r)
                                .or_else(|| source_paths.get(r))
                                .map(String::as_str)
                                .unwrap_or(r)
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                }
            }
            entry.extras.push(("imported_from".into(), origin));
            if reviewed {
                let revision = super::memory_content_revision(&entry);
                for (key, value) in &mut entry.extras {
                    if key == "reviewed_content_revision" {
                        *value = revision.clone();
                    }
                }
            }
            let mut sources = Vec::new();
            if source_db.is_file() {
                let old = kanzei_core::SessionStore::open(&source_db)?;
                let target = kanzei_core::SessionStore::open(&target_db)?;
                for evidence in old.memory_source_evidence(&old_id)? {
                    let imported = target.import_memory_episode(&source_db, evidence.episode_id)?;
                    target.record_memory_source(
                        &entry.id,
                        imported,
                        evidence.event_start,
                        evidence.event_end,
                        &evidence.source_hash,
                    )?;
                    sources.push(imported);
                }
            }
            self.write_entry(&entry, Some(destination))?;
            super::record_memory_lifecycle_event(
                self.project_root.as_deref(),
                "memory_namespace_imported",
                Some(&entry.id),
                &sources,
                Some(&old_id),
                &[],
                "personal_namespace_unified",
                Some(&entry.status),
                Some(&entry.status),
            );
        }
        self.import_personal_inbox(&source, &mapping, &source_paths)?;
        self.refresh_derived()?;
        Ok(())
    }
}
