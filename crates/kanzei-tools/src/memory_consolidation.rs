//! Shared, bounded memory-inbox consolidation for the CLI and desktop app.
//!
//! The memory crate owns queue slicing and checkpoints; this module owns the
//! manager run. Keeping the runner here prevents the two real callers from
//! drifting on batch limits, fallback behavior, or error reporting.

use kanzei_core::{run_once_with_parts, AskFuture, RunEvent, RunnerConfig};
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx};
use kanzei_llm::{LlmClient, ProxyConfig};
use serde::Serialize;

use crate::memory::{consolidation_prompt, InboxCheckpoint, MemoryManagerComponent, MemoryStore};

const MAX_BATCH_NOTES: usize = 10;
const MAX_BATCH_BYTES: usize = 32 * 1024;
const MAX_BATCH_TOKENS: usize = 8 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ConsolidationBatchReport {
    pub batch_id: String,
    pub status: String,
    pub input_notes: usize,
    pub input_bytes: usize,
    pub estimated_tokens: usize,
    pub success_notes: usize,
    pub pending_after: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsolidationReport {
    pub pending_before: usize,
    pub pending_after: usize,
    pub batches: Vec<ConsolidationBatchReport>,
    pub stopped_reason: Option<String>,
}

impl ConsolidationReport {
    pub fn has_failures(&self) -> bool {
        self.stopped_reason.is_some() || self.batches.iter().any(|batch| batch.error.is_some())
    }

    pub fn summary(&self) -> String {
        let batches = self
            .batches
            .iter()
            .map(|batch| format!("{}:{}", batch.batch_id, batch.status))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "memory inbox: {} -> {} pending; batches=[{}]; stopped_reason={}",
            self.pending_before,
            self.pending_after,
            batches,
            self.stopped_reason.as_deref().unwrap_or("none")
        )
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)] // checkpoint fields mirror the durable audit record; an object would obscure the write contract.
fn checkpoint(
    store: &MemoryStore,
    batch_id: &str,
    status: &str,
    input_notes: usize,
    input_bytes: usize,
    success_notes: usize,
    pending_after: usize,
    failure_reason: Option<String>,
) -> anyhow::Result<()> {
    store.write_inbox_checkpoint(&InboxCheckpoint {
        batch_id: batch_id.to_string(),
        status: status.to_string(),
        input_notes,
        input_bytes,
        success_notes,
        pending_after,
        failure_reason,
        updated_at_ms: now_ms(),
    })
}

/// Process the current inbox in bounded, checkpointed manager runs.
///
/// A batch is considered successful only when the manager run reduces the
/// pending-note count. A provider/tool failure or a non-progressing run is
/// returned to the caller and stops the loop; already discarded notes remain
/// committed, so the next invocation resumes from the remaining queue.
pub async fn consolidate_memory_inbox(
    config: &KanzeiConfig,
    proxy: &ProxyConfig,
    client: &LlmClient,
    rctx: &ResolveCtx,
    ctx: &ToolCtx,
    current_episode_id: Option<i64>,
) -> ConsolidationReport {
    let store = MemoryStore::project(&ctx.project_root);
    let pending_before = store.pending_notes();
    let mut report = ConsolidationReport {
        pending_before,
        pending_after: pending_before,
        batches: Vec::new(),
        stopped_reason: None,
    };
    if pending_before == 0 {
        return report;
    }

    let mut harness = Harness::default();
    harness.add(MemoryManagerComponent);
    let snapshot = match harness.resolve(rctx) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            report.stopped_reason = Some(format!("manager harness resolve failed: {error}"));
            return report;
        }
    };
    let agent = crate::memory::manager_agent();

    loop {
        let Some(batch) =
            store.read_inbox_batch(MAX_BATCH_NOTES, MAX_BATCH_BYTES, MAX_BATCH_TOKENS)
        else {
            report.stopped_reason = Some("inbox could not be read as a batch".into());
            break;
        };
        let batch_id = format!("inbox-{}", now_ms());
        let pending_at_start = store.pending_notes();
        if let Err(error) = checkpoint(
            &store,
            &batch_id,
            "processing",
            batch.note_count,
            batch.bytes,
            0,
            pending_at_start,
            None,
        ) {
            report.stopped_reason = Some(format!("checkpoint write failed: {error}"));
            break;
        }

        let prompt = consolidation_prompt(&batch.text, current_episode_id);
        let mut last_error = None;
        let mut attempted = false;
        for role in ["primary", "fast"] {
            let resolved = match config.resolve_model(role) {
                Ok(resolved) => resolved,
                Err(error) => {
                    last_error = Some(format!("resolve {role} model failed: {error}"));
                    continue;
                }
            };
            let route = match kanzei_core::build_route(&resolved, proxy).await {
                Ok(route) => route,
                Err(error) => {
                    last_error = Some(format!("build {role} route failed: {error}"));
                    continue;
                }
            };
            let runner_config = RunnerConfig {
                model: resolved.model.clone(),
                max_tokens: 4096,
                reasoning: kanzei_llm::ReasoningEffort::Off,
                service_tier: config.service_tier_for(&resolved),
                context_limit: resolved.provider.context_limit,
                limits: config.limits.clone(),
                recall: None,
                execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
                ask_policy: kanzei_core::AskPolicy::NonInteractive,
                halt: None,
            };
            let mut on_event = |_event: RunEvent| {};
            let mut ask = |request: kanzei_core::AskRequest| -> AskFuture {
                Box::pin(async move {
                    match request {
                        kanzei_core::AskRequest::Permission { .. } => {
                            kanzei_core::AskResponse::Permission(kanzei_core::AskReply::AllowOnce)
                        }
                        kanzei_core::AskRequest::Question { .. } => {
                            kanzei_core::AskResponse::Cancelled
                        }
                    }
                })
            };
            attempted = true;
            match run_once_with_parts(
                client,
                &route,
                &snapshot,
                &agent,
                &runner_config,
                ctx,
                &prompt,
                None,
                None,
                &[],
                None,
                None,
                None,
                &mut on_event,
                &mut ask,
            )
            .await
            {
                Ok(_) => {
                    last_error = None;
                    break;
                }
                Err(error) => {
                    last_error = Some(format!("{role} manager run failed: {error}"));
                }
            }
        }
        if !attempted && last_error.is_none() {
            last_error = Some("no manager model route was available".into());
        }

        let pending_after = store.pending_notes();
        let success_notes = pending_at_start.saturating_sub(pending_after);
        let status = if success_notes == 0 {
            "failed"
        } else if success_notes < batch.note_count {
            "partial"
        } else {
            "completed"
        };
        let error = if success_notes == 0 {
            last_error.or_else(|| Some("manager run made no inbox progress".into()))
        } else {
            last_error
        };
        let checkpoint_error = checkpoint(
            &store,
            &batch_id,
            status,
            batch.note_count,
            batch.bytes,
            success_notes,
            pending_after,
            error.clone(),
        )
        .err()
        .map(|write_error| format!("checkpoint finalization failed: {write_error}"));
        let batch_error = error.or(checkpoint_error);
        report.batches.push(ConsolidationBatchReport {
            batch_id,
            status: status.to_string(),
            input_notes: batch.note_count,
            input_bytes: batch.bytes,
            estimated_tokens: batch.estimated_tokens,
            success_notes,
            pending_after,
            error: batch_error.clone(),
        });
        report.pending_after = pending_after;
        if batch_error.is_some() || pending_after == 0 {
            if batch_error.is_some() && report.stopped_reason.is_none() {
                report.stopped_reason = Some("manager batch failed or made no progress".into());
            }
            break;
        }
    }
    report
}

/// Shared dependency setup for the desktop command's manual consolidation.
pub async fn consolidate_memory_for_project(
    project_dir: &str,
    current_episode_id: Option<i64>,
) -> anyhow::Result<ConsolidationReport> {
    let cwd = std::path::PathBuf::from(project_dir);
    let project_root =
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or_else(|| cwd.clone());
    let config = KanzeiConfig::load(&cwd)?;
    let config = std::sync::Arc::new(config);
    let rctx = ResolveCtx {
        profile: ProfileKind::Dev,
        cwd: cwd.clone(),
        project_root: project_root.clone(),
        config: config.clone(),
    };
    let proxy = match config.proxy.as_deref() {
        Some("off") => ProxyConfig::Disabled,
        Some("env") | None => ProxyConfig::Env,
        Some(value) => ProxyConfig::Explicit(value.to_string()),
    };
    let client = LlmClient::new(&proxy)?;
    let ctx = ToolCtx {
        cwd,
        project_root,
        ..Default::default()
    };
    Ok(consolidate_memory_inbox(&config, &proxy, &client, &rctx, &ctx, current_episode_id).await)
}
