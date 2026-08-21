//! Shared, bounded memory-inbox consolidation for the CLI and desktop app.
//!
//! The memory crate owns queue slicing and checkpoints; this module owns the
//! manager run. Keeping the runner here prevents the two real callers from
//! drifting on batch limits, fallback behavior, or error reporting.

use kanzei_core::{run_once_with_parts, AskFuture, RunEvent, RunnerConfig};
use kanzei_harness::{Harness, KanzeiConfig, ProfileKind, ResolveCtx, Tool, ToolCtx};
use kanzei_llm::{LlmClient, ProxyConfig};
use serde::Serialize;

use crate::memory::{consolidation_prompt, InboxCheckpoint, MemoryManagerComponent, MemoryStore};

const MAX_BATCH_NOTES: usize = 10;
const MAX_BATCH_BYTES: usize = 32 * 1024;
const MAX_BATCH_TOKENS: usize = 8 * 1024;
const FAILURE_ALERT_THRESHOLD: usize = 3;

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
    consecutive_failures: usize,
) -> anyhow::Result<()> {
    store.write_inbox_checkpoint(&InboxCheckpoint {
        batch_id: batch_id.to_string(),
        status: status.to_string(),
        input_notes,
        input_bytes,
        success_notes,
        pending_after,
        failure_reason,
        consecutive_failures,
        updated_at_ms: now_ms(),
    })
}

/// Discard notes that the manager demonstrably materialized as active entries in this batch.
///
/// This is a conservative safety net for the LLM runner's final tool call: it never promotes
/// entries and never discards a note merely because a candidate exists. The entry must be a
/// changed `memory-manager` entry in `active` state and visibly carry the note summary.
fn reconcile_active_notes(
    store: &MemoryStore,
    batch_text: &str,
    before_entries: &[(std::path::PathBuf, crate::memory::MemoryEntry)],
) -> anyhow::Result<usize> {
    use std::collections::HashMap;

    let before_by_id = before_entries
        .iter()
        .map(|(_, entry)| (entry.id.clone(), entry))
        .collect::<HashMap<_, _>>();
    let changed_active = store
        .load_all()
        .into_iter()
        .filter_map(|(_, entry)| {
            if entry.status != "active" || entry.source != "memory-manager" {
                return None;
            }
            let changed = before_by_id
                .get(&entry.id)
                .is_none_or(|previous| **previous != entry);
            changed.then_some(entry)
        })
        .collect::<Vec<_>>();

    let mut discarded = 0;
    for (_, summary, _) in store.pending_note_list() {
        if summary.is_empty() || !batch_text.contains(&summary) {
            continue;
        }
        let materialized = changed_active.iter().any(|entry| {
            entry.title.contains(&summary)
                || entry.description.contains(&summary)
                || entry.body.contains(&summary)
        });
        if materialized && store.discard_note(&summary)? {
            discarded += 1;
        }
    }
    Ok(discarded)
}

fn explicit_stale_ids(batch_text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in batch_text
        .lines()
        .filter(|line| line.contains("memory_stale"))
    {
        let mut scan_from = 0;
        while let Some(relative) = line[scan_from..].find("project/M-") {
            let start = scan_from + relative + "project/".len();
            let end = line[start..]
                .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
                .map_or(line.len(), |offset| start + offset);
            let id = &line[start..end];
            if id.starts_with("M-")
                && id[2..].chars().all(|ch| ch.is_ascii_digit())
                && !ids.iter().any(|known| known == id)
            {
                ids.push(id.to_string());
            }
            scan_from = end;
            if scan_from >= line.len() {
                break;
            }
        }
        // 兼容第一版 R-216 请求:它写成「对 M-037 执行 memory_stale」而不是
        // `project/M-037`;只取紧邻动作短语前的一个 ID,不误伤同句中的历史五条 ID。
        if ids.is_empty() {
            if let Some(action_at) = line.find("执行 memory_stale") {
                let prefix = &line[..action_at];
                if let Some(id_at) = prefix.rfind("M-") {
                    let end = id_at
                        + 2
                        + prefix[id_at + 2..]
                            .chars()
                            .take_while(|ch| ch.is_ascii_digit())
                            .count();
                    let id = &prefix[id_at..end];
                    if id.len() > 2 && !ids.iter().any(|known| known == id) {
                        ids.push(id.to_string());
                    }
                }
            }
        }
    }
    ids
}

async fn process_explicit_stale_requests(
    store: &MemoryStore,
    batch: &crate::memory::InboxBatch,
    ctx: &ToolCtx,
) -> anyhow::Result<usize> {
    let ids = explicit_stale_ids(&batch.text);
    if ids.is_empty() {
        return Ok(0);
    }
    for id in ids {
        if store.has_archived_id(&id) {
            continue;
        }
        let output = kanzei_harness::managed_fence::tool_scope(
            "memory_stale",
            crate::memory::MemoryStaleTool.execute(
                serde_json::json!({
                    "scope": "project",
                    "id": id,
                    "reason": "R-216 tracker 交付状态已由 tracker/refs 取代；保留可追溯墓碑，错误重复候选同步退役。"
                }),
                ctx,
            ),
        )
        .await;
        if output.is_error {
            anyhow::bail!("explicit memory_stale failed: {}", output.content);
        }
    }
    let mut discarded = 0;
    for (_, summary, _) in store.pending_note_list() {
        if batch.text.contains(&summary) {
            let removed =
                kanzei_harness::managed_fence::tool_scope("memory_inbox_discard", async {
                    store.discard_note(&summary)
                })
                .await?;
            discarded += usize::from(removed);
        }
    }
    Ok(discarded)
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
        let previous_failures = store
            .read_inbox_checkpoint()
            .filter(|checkpoint| checkpoint.status == "failed")
            .map(|checkpoint| checkpoint.consecutive_failures)
            .unwrap_or(0);
        let before_entries = store.load_all();
        if let Err(error) = checkpoint(
            &store,
            &batch_id,
            "processing",
            batch.note_count,
            batch.bytes,
            0,
            pending_at_start,
            None,
            previous_failures,
        ) {
            report.stopped_reason = Some(format!("checkpoint write failed: {error}"));
            break;
        }

        if !explicit_stale_ids(&batch.text).is_empty() {
            let explicit_error = process_explicit_stale_requests(&store, &batch, ctx)
                .await
                .err()
                .map(|error| error.to_string());
            let pending_after = store.pending_notes();
            let success_notes = pending_at_start.saturating_sub(pending_after);
            let status = if explicit_error.is_some() {
                "failed"
            } else if success_notes < batch.note_count {
                "partial"
            } else {
                "completed"
            };
            let failure_streak = if status == "failed" {
                previous_failures.saturating_add(1)
            } else {
                0
            };
            let checkpoint_error = checkpoint(
                &store,
                &batch_id,
                status,
                batch.note_count,
                batch.bytes,
                success_notes,
                pending_after,
                explicit_error.clone(),
                failure_streak,
            )
            .err()
            .map(|error| format!("checkpoint finalization failed: {error}"));
            let batch_error = explicit_error.or(checkpoint_error);
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
            if batch_error.is_some() && failure_streak >= FAILURE_ALERT_THRESHOLD {
                report.stopped_reason = Some(format!(
                    "ALERT: memory inbox manager failed for {failure_streak} consecutive batches"
                ));
            }
            if batch_error.is_some() || pending_after == 0 {
                if batch_error.is_some() {
                    report.stopped_reason =
                        Some("explicit stale request failed or made no progress".into());
                }
                break;
            }
            continue;
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
                intensity: kanzei_harness::HarnessIntensity::Autonomous,
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

        if let Err(error) = reconcile_active_notes(&store, &batch.text, &before_entries) {
            last_error = Some(format!(
                "deterministic inbox reconciliation failed: {error}"
            ));
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
        let failure_streak = if status == "failed" {
            previous_failures.saturating_add(1)
        } else {
            0
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
            failure_streak,
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
        if batch_error.is_some() && failure_streak >= FAILURE_ALERT_THRESHOLD {
            report.stopped_reason = Some(format!(
                "ALERT: memory inbox manager failed for {failure_streak} consecutive batches"
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::{explicit_stale_ids, reconcile_active_notes};
    use crate::memory::{AddOutcome, MemoryStore};

    fn temp_project(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-memory-consolidation-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        root
    }

    #[test]
    fn explicit_stale_parser_only_selects_requested_ids() {
        let first = "M-032 M-033 M-035 M-036 M-040 均已归档，请对 M-037 执行 memory_stale";
        assert_eq!(explicit_stale_ids(first), vec!["M-037"]);
        let second = "请对 project/M-037、project/M-150、project/M-151 都使用 memory_stale";
        assert_eq!(explicit_stale_ids(second), vec!["M-037", "M-150", "M-151"]);
        assert!(explicit_stale_ids("普通 memory_note，不做退役").is_empty());
    }

    #[test]
    fn deterministic_reconciliation_discards_only_changed_active_manager_entry() {
        let active_root = temp_project("active");
        let active_store = MemoryStore::project(&active_root);
        active_store
            .append_note("active sentinel", "detail", "fact", &[])
            .unwrap();
        let before = active_store.load_all();
        let added = active_store
            .add(
                "fact",
                "active sentinel 规则",
                "处理 active sentinel 时必读",
                "active sentinel detail",
                "memory-manager",
                &[],
                None,
                true,
            )
            .unwrap();
        let id = match added {
            AddOutcome::Added(entry) => entry.id,
            other => panic!("expected added candidate, got {other:?}"),
        };
        active_store
            .update(&id, None, None, None, Some("active"), None, false)
            .unwrap();
        let batch = active_store.read_inbox();
        assert_eq!(
            reconcile_active_notes(&active_store, &batch, &before).unwrap(),
            1
        );
        assert_eq!(active_store.pending_notes(), 0);
        std::fs::remove_dir_all(active_root).ok();

        let candidate_root = temp_project("candidate");
        let candidate_store = MemoryStore::project(&candidate_root);
        candidate_store
            .append_note("candidate sentinel", "detail", "fact", &[])
            .unwrap();
        candidate_store
            .add(
                "fact",
                "candidate sentinel 规则",
                "处理 candidate sentinel 时必读",
                "candidate sentinel detail",
                "memory-manager",
                &[],
                None,
                true,
            )
            .unwrap();
        let batch = candidate_store.read_inbox();
        assert_eq!(
            reconcile_active_notes(&candidate_store, &batch, &[]).unwrap(),
            0
        );
        assert_eq!(candidate_store.pending_notes(), 1);
        std::fs::remove_dir_all(candidate_root).ok();
    }
}
