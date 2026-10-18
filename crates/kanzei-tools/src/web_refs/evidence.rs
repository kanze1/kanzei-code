//! Durable acquisition facts refer to concrete bytes, not a URL timestamp.
use kanzei_harness::{permission::ScopedFile, ToolCtx};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceScope {
    pub topic: String,
    pub task_id: String,
}

impl SourceScope {
    pub fn from_input(
        ctx: &ToolCtx,
        topic: Option<&str>,
        task_id: Option<&str>,
    ) -> Result<Option<Self>, String> {
        match (topic, task_id) {
            (None, None) => Ok(None),
            (Some(topic), Some(task_id)) => {
                crate::research_loop::validate_external_task(&ctx.project_root, topic, task_id)?;
                Ok(Some(Self {
                    topic: topic.into(),
                    task_id: task_id.into(),
                }))
            }
            _ => Err("source provenance requires topic and task_id together".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebArtifact {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

impl WebArtifact {
    pub fn from_saved(ctx: &ToolCtx, path: &str, content: &[u8]) -> Result<Self, String> {
        let path = Path::new(path)
            .strip_prefix(&ctx.project_root)
            .map_err(|_| "web artifact is outside its owning project")?;
        let artifact = Self {
            path: path.to_string_lossy().replace('\\', "/"),
            sha256: super::digest_hex(content),
            bytes: content.len() as u64,
        };
        artifact.verify(ctx)?;
        Ok(artifact)
    }
    pub fn verify(&self, ctx: &ToolCtx) -> Result<(), String> {
        let path = Path::new(&self.path);
        if path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || !self.path.starts_with(".kanzei/artifacts/web/")
        {
            return Err("source artifact is not an owned web artifact".into());
        }
        let file = ScopedFile::resolve(
            &ctx.project_root,
            &ctx.project_root,
            &ctx.project_root.join(path),
        )
        .map_err(|error| error.to_string())?;
        if kanzei_harness::permission::FileResource::resolve(
            &ctx.project_root,
            &ctx.project_root.join(path),
        )
        .map_err(|error| error.to_string())?
        .sensitivity()
        .is_some()
        {
            return Err("web evidence cannot refer to a sensitive resource".into());
        }
        let mut file = file
            .open()
            .map_err(|error| format!("source artifact is unavailable: {error}"))?;
        if file.metadata().map_err(|error| error.to_string())?.len() != self.bytes {
            return Err("source artifact length has changed".into());
        }
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        if format!("{:x}", digest.finalize()) != self.sha256 {
            return Err("source artifact content hash has changed".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionMethod {
    StaticText,
    DownloadedPdf,
    RenderedDom,
    UnverifiedHtml,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebAcquisition {
    pub format_version: u32,
    pub id: String,
    pub source_scope: Option<SourceScope>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub original_url: String,
    pub final_url: String,
    pub fetched_at_utc: String,
    pub observed_at_utc: String,
    pub http_status: Option<u16>,
    pub content_type: String,
    pub method: AcquisitionMethod,
    pub truncated: bool,
    pub cache_hit: bool,
    pub artifact: WebArtifact,
    pub raw_artifact: WebArtifact,
}

impl WebAcquisition {
    pub fn verify(&self, ctx: &ToolCtx) -> Result<(), String> {
        if self.format_version != 1 || self.session_id != ctx.session_id {
            return Err("source acquisition belongs to another conversation or format".into());
        }
        let mut identity = self.clone();
        identity.id.clear();
        if self.id
            != super::digest_hex(&serde_json::to_vec(&identity).map_err(|error| error.to_string())?)
        {
            return Err("source acquisition identity has changed".into());
        }
        self.artifact.verify(ctx)?;
        self.raw_artifact.verify(ctx)?;
        Ok(())
    }
    pub fn readable(&self) -> bool {
        self.http_status.is_none_or(|status| status < 400)
            && (self.http_status.is_some() || self.method == AcquisitionMethod::RenderedDom)
            && !self.truncated
            && matches!(
                self.method,
                AcquisitionMethod::StaticText | AcquisitionMethod::RenderedDom
            )
    }
    pub fn ready_for_finding(&self, ctx: &ToolCtx) -> Result<bool, String> {
        self.verify(ctx)?;
        if self.readable() {
            return Ok(true);
        }
        if self.http_status.is_none_or(|status| status >= 400)
            || self.truncated
            || self.method != AcquisitionMethod::DownloadedPdf
        {
            return Ok(false);
        }
        let path = ctx.project_root.join(&self.artifact.path);
        let Some(expected) = ctx
            .read_ledger
            .as_ref()
            .and_then(|ledger| ledger.expected(&path))
        else {
            return Ok(false);
        };
        let file = ScopedFile::resolve(&ctx.project_root, &ctx.project_root, &path)
            .map_err(|error| error.to_string())?
            .open()
            .map_err(|error| error.to_string())?;
        if kanzei_base::reader_content_hash(file).map_err(|error| error.to_string())? != expected {
            return Ok(false);
        }
        let (state, session) = state(ctx)?;
        let payload = serde_json::json!({"acquisition_id":self.id,"artifact_sha256":self.artifact.sha256,"inspection":"read_tool_observed_current_file_version","source_scope":self.source_scope,"run_id":ctx.run_id});
        if state
            .latest_matching_event(&session, "web.inspected", "$.acquisition_id", &self.id)
            .map_err(|error| error.to_string())?
            .is_none_or(|event| event.payload != payload)
        {
            state
                .append_event(&session, "web.inspected", &payload)
                .map_err(|error| error.to_string())?;
        }
        Ok(true)
    }
}

fn owner_session(ctx: &ToolCtx) -> String {
    ctx.session_id
        .clone()
        .unwrap_or_else(|| kanzei_core::project_session_id(&ctx.project_root))
}

fn state(ctx: &ToolCtx) -> Result<(kanzei_core::SessionStore, String), String> {
    let session = owner_session(ctx);
    let state =
        kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&ctx.project_root))
            .map_err(|error| error.to_string())?;
    if ctx.session_id.is_some() {
        if state
            .get_session(&session)
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Err("source owner no longer exists".into());
        }
    } else {
        state
            .create_session(&session, &ctx.project_root.to_string_lossy(), None)
            .map_err(|error| error.to_string())?;
    }
    Ok((state, session))
}

pub fn record_search(
    ctx: &ToolCtx,
    scope: Option<SourceScope>,
    queries: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let (state, session) = state(ctx)?;
    let bytes = serde_json::to_vec_pretty(queries).map_err(|error| error.to_string())?;
    let path = crate::webfetch::save_web_artifact(ctx, "websearch", "json", &bytes)?;
    let artifact = WebArtifact::from_saved(ctx, &path, &bytes)?;
    let event = state
        .append_event(
            &session,
            "web.searched",
            &serde_json::json!({
                "session_id":session,"run_id":ctx.run_id,"process_id":ctx.process_id,
                "source_scope":scope,"queries":queries,"artifact":artifact
            }),
        )
        .map_err(|error| error.to_string())?;
    Ok(serde_json::json!({"event_id":event.event_id,"artifact":artifact}))
}

pub fn record_acquisition(
    ctx: &ToolCtx,
    mut acquisition: WebAcquisition,
) -> Result<String, String> {
    for value in [&mut acquisition.original_url, &mut acquisition.final_url] {
        let url = reqwest::Url::parse(value).map_err(|error| error.to_string())?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("source URL must be an HTTP URL without credentials".into());
        }
        *value = url.to_string();
    }
    acquisition.id.clear();
    acquisition.id =
        super::digest_hex(&serde_json::to_vec(&acquisition).map_err(|error| error.to_string())?);
    acquisition.verify(ctx)?;
    let (state, session) = state(ctx)?;
    let mut urls = vec![acquisition.original_url.as_str()];
    if acquisition.final_url != acquisition.original_url {
        urls.push(&acquisition.final_url);
    }
    for url in urls {
        let key = super::digest_hex(
            &serde_json::to_vec(&serde_json::json!([url, acquisition.source_scope]))
                .map_err(|error| error.to_string())?,
        );
        let topic = acquisition
            .source_scope
            .as_ref()
            .map(|scope| scope.topic.as_str());
        let topic_url_key = super::digest_hex(
            &serde_json::to_vec(&serde_json::json!([url, topic]))
                .map_err(|error| error.to_string())?,
        );
        let payload = serde_json::json!({"lookup_key":key,"topic_url_key":topic_url_key,"url":url,"acquisition":acquisition});
        if state
            .latest_matching_event(&session, "web.acquired", "$.lookup_key", &key)
            .map_err(|error| error.to_string())?
            .is_none_or(|event| event.payload != payload)
        {
            state
                .append_event(&session, "web.acquired", &payload)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(acquisition.id)
}

pub fn acquisition_for_url(
    ctx: &ToolCtx,
    url: &str,
    topic: Option<&str>,
) -> Result<Option<WebAcquisition>, String> {
    if !kanzei_core::project_state_path(&ctx.project_root).is_file() {
        return Ok(None);
    }
    let session = owner_session(ctx);
    let state = kanzei_core::SessionStore::open_read_only(&kanzei_core::project_state_path(
        &ctx.project_root,
    ))
    .map_err(|error| error.to_string())?;
    let url = reqwest::Url::parse(url)
        .map_err(|error| error.to_string())?
        .to_string();
    let key = super::digest_hex(
        &serde_json::to_vec(&serde_json::json!([url, topic])).map_err(|error| error.to_string())?,
    );
    let Some(event) = state
        .latest_matching_event(&session, "web.acquired", "$.topic_url_key", &key)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let acquisition: WebAcquisition = serde_json::from_value(event.payload["acquisition"].clone())
        .map_err(|error| format!("invalid web acquisition: {error}"))?;
    acquisition.verify(ctx)?;
    Ok(Some(acquisition))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn a_real_pdf_download_requires_an_actual_successful_read_before_a_finding() {
        use kanzei_harness::Tool;
        let root = std::env::temp_dir().join(format!(
            "kz-source-pdf-{}-{}",
            std::process::id(),
            super::super::now_ms()
        ));
        std::fs::create_dir_all(root.join(".kanzei/artifacts/web")).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone())
            .with_session_id("pdf-owner".into())
            .with_read_ledger(Default::default());
        let state =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        state
            .create_session("pdf-owner", &root.to_string_lossy(), None)
            .unwrap();
        drop(state);
        let path = root.join(".kanzei/artifacts/web/paper.pdf");
        let bytes = crate::read::test_pdf::bytes();
        std::fs::write(&path, &bytes).unwrap();
        let artifact = WebArtifact::from_saved(&ctx, &path.to_string_lossy(), &bytes).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let original_url = "https://source.example/paper.pdf";
        record_acquisition(
            &ctx,
            WebAcquisition {
                format_version: 1,
                id: String::new(),
                source_scope: None,
                session_id: ctx.session_id.clone(),
                run_id: Some("pdf-run".into()),
                original_url: original_url.into(),
                final_url: original_url.into(),
                fetched_at_utc: now.clone(),
                observed_at_utc: now,
                http_status: Some(200),
                content_type: "application/pdf".into(),
                method: AcquisitionMethod::DownloadedPdf,
                truncated: false,
                cache_hit: false,
                artifact: artifact.clone(),
                raw_artifact: artifact,
            },
        )
        .unwrap();
        let acquisition = acquisition_for_url(&ctx, original_url, None)
            .unwrap()
            .unwrap();
        assert!(!acquisition.ready_for_finding(&ctx).unwrap());
        let output = crate::read::ReadTool
            .execute(
                serde_json::json!({"path":".kanzei/artifacts/web/paper.pdf","pages":"1"}),
                &ctx,
            )
            .await;
        assert!(!output.is_error, "{}", output.content);
        assert!(output.content.contains("PDF smoke"));
        assert!(acquisition.ready_for_finding(&ctx).unwrap());
        let state =
            kanzei_core::SessionStore::open_read_only(&kanzei_core::project_state_path(&root))
                .unwrap();
        assert!(state
            .latest_matching_event(
                "pdf-owner",
                "web.inspected",
                "$.acquisition_id",
                &acquisition.id
            )
            .unwrap()
            .is_some());
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn receipt_validates_materialized_bytes_topic_owner_redirect_and_reopen() {
        let root = std::env::temp_dir().join(format!(
            "kz-source-chain-{}-{}",
            std::process::id(),
            super::super::now_ms()
        ));
        std::fs::create_dir_all(root.join(".kanzei/artifacts/web")).unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone()).with_session_id("owner".into());
        let state =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        state
            .create_session("owner", &root.to_string_lossy(), None)
            .unwrap();
        drop(state);
        let path = root.join(".kanzei/artifacts/web/source.md");
        let bytes = b"concrete source evidence";
        std::fs::write(&path, bytes).unwrap();
        let artifact = WebArtifact::from_saved(&ctx, &path.to_string_lossy(), bytes).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let acquisition = WebAcquisition {
            format_version: 1,
            id: String::new(),
            source_scope: Some(SourceScope {
                topic: "selected-topic".into(),
                task_id: "issued-task".into(),
            }),
            session_id: ctx.session_id.clone(),
            run_id: Some("real-run".into()),
            original_url: "https://source.example/original".into(),
            final_url: "https://source.example/final".into(),
            fetched_at_utc: now.clone(),
            observed_at_utc: now,
            http_status: Some(200),
            content_type: "text/plain".into(),
            method: AcquisitionMethod::StaticText,
            truncated: false,
            cache_hit: false,
            artifact: artifact.clone(),
            raw_artifact: artifact,
        };
        let id = record_acquisition(&ctx, acquisition.clone()).unwrap();
        let loaded = acquisition_for_url(&ctx, &acquisition.final_url, Some("selected-topic"))
            .unwrap()
            .unwrap();
        assert_eq!(loaded.id, id);
        assert!(loaded.ready_for_finding(&ctx).unwrap());
        assert!(
            acquisition_for_url(&ctx, &acquisition.original_url, Some("other-topic"))
                .unwrap()
                .is_none()
        );
        let other = ToolCtx::new(root.clone(), root.clone()).with_session_id("other-owner".into());
        assert!(
            acquisition_for_url(&other, &acquisition.original_url, Some("selected-topic"))
                .unwrap()
                .is_none()
        );
        let missing =
            ToolCtx::new(root.clone(), root.clone()).with_session_id("deleted-owner".into());
        let mut orphan = acquisition.clone();
        orphan.session_id = missing.session_id.clone();
        assert!(record_acquisition(&missing, orphan).is_err());
        assert!(
            kanzei_core::SessionStore::open_read_only(&kanzei_core::project_state_path(&root))
                .unwrap()
                .get_session("deleted-owner")
                .unwrap()
                .is_none()
        );
        std::fs::write(&path, b"modified source evidence").unwrap();
        assert!(
            acquisition_for_url(&ctx, &acquisition.original_url, Some("selected-topic")).is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
