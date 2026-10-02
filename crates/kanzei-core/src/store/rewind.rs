//! R-366: append-only conversation rewind and verified restoration of dedicated-tool writes.
use super::{now_ms, SessionFact, SessionStore, StoreError};
use kanzei_llm::{Message, Part, Role};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RewindFile {
    pub path: String,
    pub tree_root: String,
    pub pre_exists: bool,
    pub pre_blob: Option<String>,
    pub post_hash: Option<String>,
    pub restorable: bool,
    pub external_change: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RewindPlan {
    pub target_sequence: i64,
    pub source_hash: String,
    pub prompt: String,
    pub surface: Vec<Message>,
    pub files: Vec<RewindFile>,
    pub unhandled: Vec<String>,
}

fn message_text(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl SessionStore {
    pub fn latest_rewind_surface(
        &self,
        session: &str,
        start: i64,
        end: i64,
    ) -> Result<Option<(i64, Vec<Message>)>, StoreError> {
        let event = self
            .list_events_by_type(session, start, "conversation.rewind")?
            .into_iter()
            .rev()
            .find(|event| event.sequence <= end);
        event
            .map(|event| {
                Ok((
                    event.sequence,
                    serde_json::from_value(event.payload["surface"].clone())?,
                ))
            })
            .transpose()
    }

    /// Visible transcript ignores compression and starts from the last explicit rewind.
    pub fn visible_transcript(
        &self,
        session: &str,
        start: i64,
        end: i64,
    ) -> Result<Vec<Message>, StoreError> {
        let facts = self
            .list_session_facts(session)
            .map_err(|e| StoreError::InvalidInput(e.to_string()))?;
        let facts: Vec<_> = facts
            .into_iter()
            .filter(|(event, _)| event.sequence > start && event.sequence <= end)
            .collect();
        let projection = match self.latest_rewind_surface(session, start, end)? {
            Some((sequence, surface)) => {
                super::project_session_facts_with_surface(&facts, Some(sequence), Some(surface))
            }
            None => super::project_session_facts(&facts),
        };
        Ok(projection.surface_messages)
    }

    pub fn rewind_plan(
        &self,
        session: &str,
        text: &str,
        occurrence_from_end: usize,
    ) -> Result<RewindPlan, StoreError> {
        if text.trim().is_empty() {
            return Err(StoreError::InvalidInput("请选择实际的用户消息".into()));
        }
        if self
            .list_session_facts(session)
            .map_err(|e| StoreError::InvalidInput(e.to_string()))?
            .is_empty()
        {
            super::prepare_typed_session(self, session)
                .map_err(|e| StoreError::InvalidInput(e.to_string()))?;
        }
        let start = self.conversation_floor(session)?.unwrap_or(0);
        let messages = self.visible_transcript(session, start, i64::MAX)?;
        let target = messages
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, message)| message.role == Role::User && message_text(message) == text)
            .nth(occurrence_from_end)
            .map(|(index, _)| index)
            .ok_or_else(|| {
                StoreError::InvalidInput("所选消息已变化；重新加载当前对话后再选择".into())
            })?;
        let facts = self
            .list_session_facts(session)
            .map_err(|e| StoreError::InvalidInput(e.to_string()))?;
        let reverted: Vec<_> = self
            .list_events_by_type(session, start, "conversation.rewind")?
            .into_iter()
            .filter_map(|event| {
                event.payload["target_sequence"]
                    .as_i64()
                    .map(|target| (target, event.sequence))
            })
            .collect();
        let facts: Vec<_> = facts
            .into_iter()
            .filter(|(event, _)| {
                event.sequence > start
                    && !reverted
                        .iter()
                        .any(|(from, to)| event.sequence >= *from && event.sequence <= *to)
            })
            .collect();
        // Every visible user message carries the sequence of its own fact. Seeded messages
        // share their seed sequence; subsequent rewinds trim the origin list to the kept prefix.
        let all = self
            .list_session_facts(session)
            .map_err(|e| StoreError::InvalidInput(e.to_string()))?;
        let mut origins = Vec::<(String, i64)>::new();
        let rewinds = self.list_events_by_type(session, start, "conversation.rewind")?;
        let mut rewind_cursor = rewinds.iter().peekable();
        for (event, envelope) in all.iter().filter(|(event, _)| event.sequence > start) {
            while rewind_cursor
                .peek()
                .is_some_and(|rewind| rewind.sequence < event.sequence)
            {
                let rewind = rewind_cursor.next().unwrap();
                let surface: Vec<Message> =
                    serde_json::from_value(rewind.payload["surface"].clone())?;
                origins.truncate(
                    surface
                        .iter()
                        .filter(|message| message.role == Role::User)
                        .count(),
                );
            }
            match &envelope.fact {
                SessionFact::LegacySeeded { messages, .. } => {
                    origins = messages
                        .iter()
                        .filter(|message| message.role == Role::User)
                        .map(|message| (message_text(message), event.sequence))
                        .collect();
                }
                SessionFact::UserMessageCommitted { message, .. }
                | SessionFact::SteeringMessageCommitted { message, .. } => {
                    origins.push((message_text(message), event.sequence));
                }
                _ => {}
            }
        }
        for rewind in rewind_cursor {
            let surface: Vec<Message> = serde_json::from_value(rewind.payload["surface"].clone())?;
            origins.truncate(
                surface
                    .iter()
                    .filter(|message| message.role == Role::User)
                    .count(),
            );
        }
        let target_sequence = origins
            .iter()
            .rev()
            .filter(|(content, _)| content == text)
            .nth(occurrence_from_end)
            .map(|(_, sequence)| *sequence)
            .ok_or_else(|| {
                StoreError::InvalidInput("无法对应所选消息的持久事件，请重新加载对话".into())
            })?;
        let runs: BTreeSet<String> = facts
            .iter()
            .filter(|(event, _)| event.sequence >= target_sequence)
            .map(|(_, envelope)| envelope.turn_id.clone())
            .collect();
        let mut rows = Vec::new();
        let mut statement = self.connection.prepare("SELECT abs_path, tree_root, pre_exists, pre_blob, post_hash, captured_at FROM file_checkpoints WHERE run_id=?1 ORDER BY captured_at")?;
        for run in &runs {
            let items = statement.query_map([run], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?;
            rows.extend(items.collect::<Result<Vec<_>, _>>()?);
        }
        rows.sort_by_key(|row| row.5);
        let mut files: BTreeMap<String, RewindFile> = BTreeMap::new();
        for (path, tree_root, existed, blob, post_hash, _) in rows {
            let entry = files
                .entry(super::file_checkpoint_path_key(Path::new(&path)))
                .or_insert(RewindFile {
                    path,
                    tree_root,
                    pre_exists: existed != 0,
                    pre_blob: blob,
                    post_hash: None,
                    restorable: true,
                    external_change: false,
                });
            entry.post_hash = post_hash;
        }
        for file in files.values_mut() {
            file.restorable = !file.pre_exists || file.pre_blob.is_some();
            let current = current_hash(Path::new(&file.path)).unwrap_or(None);
            file.external_change = current != file.post_hash;
        }
        Ok(RewindPlan {
            target_sequence,
            source_hash: super::stable_json_hash(&messages),
            prompt: text.into(),
            surface: messages[..target].to_vec(),
            files: files.into_values().collect(),
            unhandled: vec![
                "bash 与 Git 的改动不在文件检查点内".into(),
                "tracker、记忆、外部服务与子代理历史不回退".into(),
            ],
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_rewind(
        &self,
        project_root: &Path,
        code_root: &Path,
        session: &str,
        plan: &RewindPlan,
        conversation: bool,
        code: bool,
        force: bool,
    ) -> Result<serde_json::Value, StoreError> {
        let start = self.conversation_floor(session)?.unwrap_or(0);
        let current = self.visible_transcript(session, start, i64::MAX)?;
        if super::stable_json_hash(&current) != plan.source_hash {
            return Err(StoreError::InvalidInput(
                "预览后对话已变化，请重新预览".into(),
            ));
        }
        let operation = format!("rewind:{}", now_ms());
        self.append_event(session, "conversation.rewind_started", &json!({"operation":operation,"target_sequence":plan.target_sequence,"conversation":conversation,"code":code}))?;
        let mut restored = Vec::new();
        let mut skipped = Vec::new();
        if code {
            for file in &plan.files {
                match restore_file(project_root, code_root, file, force) {
                    Ok(backup) => {
                        self.append_event(
                            session,
                            "file.reverted",
                            &json!({"operation":operation,"path":file.path,"backup_blob":backup}),
                        )?;
                        restored.push(file.path.clone());
                    }
                    Err(reason) => skipped.push(json!({"path":file.path,"reason":reason})),
                }
            }
        }
        let result = json!({"operation":operation,"target_sequence":plan.target_sequence,"restored":restored,"skipped":skipped,"prompt":plan.prompt});
        if conversation {
            self.append_event(session, "conversation.rewind", &json!({"format_version":1,"operation":operation,"target_sequence":plan.target_sequence,"surface":plan.surface,"result":result}))?;
        } else {
            self.append_event(session, "conversation.code_reverted", &result)?;
        }
        Ok(result)
    }
}

fn current_hash(path: &Path) -> Result<Option<String>, String> {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut digest = Sha256::new();
    let mut chunk = [0u8; 65536];
    loop {
        let count = file.read(&mut chunk).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&chunk[..count]);
    }
    Ok(Some(format!("{:x}", digest.finalize())))
}

fn bounded_read(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(super::FILE_CHECKPOINT_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > super::FILE_CHECKPOINT_MAX_BYTES {
        return Err("文件超过检查点大小上限，已跳过".into());
    }
    Ok(bytes)
}

fn restore_file(
    project_root: &Path,
    code_root: &Path,
    file: &RewindFile,
    force: bool,
) -> Result<Option<String>, String> {
    let path = PathBuf::from(&file.path);
    let root = std::fs::canonicalize(code_root).map_err(|e| e.to_string())?;
    let parent = path
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .ok_or("文件父目录不可用")?;
    if !parent.starts_with(&root)
        || path.file_name().is_none()
        || std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err("文件不在当前代码树内，或为符号链接".into());
    }
    if !file.restorable {
        return Err("前像未采集成功，无法还原".into());
    }
    let _lock =
        kanzei_base::atomic_file::try_lock_exclusive(&path, std::time::Duration::from_secs(2))
            .map_err(|e| e.to_string())?
            .ok_or("文件正在写入，已跳过")?;
    let live_hash = current_hash(&path)?;
    if live_hash != file.post_hash && !force {
        return Err("文件已被外部修改，已跳过；强制回退会先保存当前内容".into());
    }
    let original = if file.pre_exists {
        let blob = file.pre_blob.as_deref().ok_or("原文件缺少前像")?;
        if blob.len() != 64 || !blob.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("前像引用无效".into());
        }
        let bytes = bounded_read(&super::file_checkpoint_blob_path(project_root, blob))
            .map_err(|e| format!("前像无法读取：{e}"))?;
        if format!("{:x}", Sha256::digest(&bytes)) != blob {
            return Err("前像校验失败".into());
        }
        Some(bytes)
    } else {
        None
    };
    let backup = if force && live_hash.is_some() {
        let bytes = bounded_read(&path)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        kanzei_base::atomic_file::write_atomic_bytes(
            &super::file_checkpoint_blob_path(project_root, &hash),
            &bytes,
        )
        .map_err(|e| e.to_string())?;
        Some(hash)
    } else {
        None
    };
    let check = || {
        if current_hash(&path).map_err(std::io::Error::other)? != live_hash {
            return Err(std::io::Error::other("文件在回退期间又被修改，已跳过"));
        }
        Ok(())
    };
    match original {
        Some(bytes) => kanzei_base::atomic_file::write_atomic_bytes_guarded(&path, &bytes, check)
            .map_err(|e| e.to_string())?,
        None if live_hash.is_some() => {
            check().map_err(|e| e.to_string())?;
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        None => {}
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        capture_file_preimage_in, record_file_postimage_in, FileCheckpointTarget,
        SessionFactEnvelope, SessionInvariant,
    };
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "kz-rewind-{}-{}-{}",
                std::process::id(),
                now_ms(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn fact(store: &SessionStore, run: &str, text: &str) {
        store
            .append_session_facts_checked(
                "ses",
                &mut SessionInvariant::default(),
                &[SessionFactEnvelope::new(
                    run,
                    None,
                    SessionFact::UserMessageCommitted {
                        input_id: run.into(),
                        message: Message::user_text(text),
                    },
                )],
            )
            .unwrap();
    }
    #[test]
    fn rewind_is_append_only_and_can_rewind_again_after_new_turn() {
        let root = Temp::new();
        let store = SessionStore::open(&root.path().join("state.db")).unwrap();
        store.create_session("ses", "project", None).unwrap();
        fact(&store, "run1", "first");
        fact(&store, "run2", "second");
        fact(&store, "run3", "third");
        let plan = store.rewind_plan("ses", "third", 0).unwrap();
        store
            .apply_rewind(root.path(), root.path(), "ses", &plan, true, false, false)
            .unwrap();
        assert_eq!(
            store.visible_transcript("ses", 0, i64::MAX).unwrap().len(),
            2
        );
        fact(&store, "run4", "third");
        let next = store.rewind_plan("ses", "second", 0).unwrap();
        store
            .apply_rewind(root.path(), root.path(), "ses", &next, true, false, false)
            .unwrap();
        assert_eq!(
            store.visible_transcript("ses", 0, i64::MAX).unwrap(),
            vec![Message::user_text("first")]
        );
        assert_eq!(store.list_session_facts("ses").unwrap().len(), 4);
        assert!(store
            .apply_rewind(root.path(), root.path(), "ses", &next, true, false, false)
            .is_err());
    }
    #[test]
    fn restore_skips_external_edits_and_force_saves_current_bytes() {
        let root = Temp::new();
        let path = root.path().join("existing.txt");
        std::fs::write(&path, b"before").unwrap();
        let store = SessionStore::open(&root.path().join("state.db")).unwrap();
        store.create_session("ses", "project", None).unwrap();
        fact(&store, "run", "change");
        let target = FileCheckpointTarget {
            project_root: root.path(),
            run_id: "run",
            process_id: None,
            tree_root: root.path(),
            abs_path: &path,
        };
        capture_file_preimage_in(&store, &target).unwrap();
        std::fs::write(&path, b"after").unwrap();
        record_file_postimage_in(&store, &target, b"after").unwrap();
        std::fs::write(&path, b"external").unwrap();
        let plan = store.rewind_plan("ses", "change", 0).unwrap();
        assert!(plan.files[0].external_change);
        let skipped = store
            .apply_rewind(root.path(), root.path(), "ses", &plan, false, true, false)
            .unwrap();
        assert_eq!(skipped["skipped"].as_array().unwrap().len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), b"external");
        let forced = store
            .apply_rewind(root.path(), root.path(), "ses", &plan, false, true, true)
            .unwrap();
        assert_eq!(forced["restored"].as_array().unwrap().len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), b"before");
        let backup = store
            .list_events_by_type("ses", 0, "file.reverted")
            .unwrap()
            .pop()
            .unwrap();
        let blob = backup.payload["backup_blob"].as_str().unwrap();
        assert_eq!(
            std::fs::read(super::super::file_checkpoint_blob_path(root.path(), blob)).unwrap(),
            b"external"
        );
    }
    #[test]
    fn created_file_is_removed_and_corrupt_blob_never_overwrites_current() {
        let root = Temp::new();
        let path = root.path().join("new.txt");
        let store = SessionStore::open(&root.path().join("state.db")).unwrap();
        store.create_session("ses", "project", None).unwrap();
        fact(&store, "run", "new file");
        let target = FileCheckpointTarget {
            project_root: root.path(),
            run_id: "run",
            process_id: None,
            tree_root: root.path(),
            abs_path: &path,
        };
        capture_file_preimage_in(&store, &target).unwrap();
        std::fs::write(&path, b"new").unwrap();
        record_file_postimage_in(&store, &target, b"new").unwrap();
        let plan = store.rewind_plan("ses", "new file", 0).unwrap();
        store
            .apply_rewind(root.path(), root.path(), "ses", &plan, false, true, false)
            .unwrap();
        assert!(!path.exists());
        std::fs::write(&path, b"safe").unwrap();
        let file = RewindFile {
            path: path.display().to_string(),
            tree_root: root.path().display().to_string(),
            pre_exists: true,
            pre_blob: Some("0".repeat(64)),
            post_hash: current_hash(&path).unwrap(),
            restorable: true,
            external_change: false,
        };
        assert!(restore_file(root.path(), root.path(), &file, false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"safe");
    }
    #[test]
    fn legacy_snapshot_and_duplicate_prompts_have_exact_occurrences() {
        let store = SessionStore::open_in_memory().unwrap();
        store.create_session("ses", "project", None).unwrap();
        store
            .append_event(
                "ses",
                "conversation.updated",
                &json!({"messages":[Message::user_text("same"),Message::user_text("same")]}),
            )
            .unwrap();
        let plan = store.rewind_plan("ses", "same", 1).unwrap();
        assert!(plan.surface.is_empty());
        let plan = store.rewind_plan("ses", "same", 0).unwrap();
        assert_eq!(plan.surface.len(), 1);
    }
}
