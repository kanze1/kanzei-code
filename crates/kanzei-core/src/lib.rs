//! kanzei-core: session 运行时。
//! M0:内存态一次性 runner;M2 起换成 SQLite 事件溯源 + steer/queue 调度。

pub mod assemble;
pub mod experience_events;
pub mod history;
pub mod notification;
pub mod orchestration;
pub mod phase;
pub mod replay;
pub mod runner;
pub mod store;

pub use assemble::build_route;
pub use history::filter_message_history;
pub use notification::AgentNotification;
pub use phase::{PhaseOrchestrator, ScoutTask};
pub use runner::{
    compact_conversation, compaction_budget, completed_entry, estimate_conversation_tokens,
    is_usable_failure_kind, mask_volatile_payload, normalize_fp_marker,
    pending_background_subagents, prune_conversation, run_once, run_once_with_parts,
    run_read_agent, summarize_failures, summarize_metrics, summarize_tools, AskFuture, AskOption,
    AskPolicy, AskReply, AskRequest, AskResponse, BackgroundEventSink, CancellationToken,
    CompletedEntry, FailureSignal, RecallHit, RecallOutcome, RecallPolicy, RecallRunOutcome,
    RecallTrigger, RecallWatch, RunEvent, RunMetrics, RunSummary, RunnerConfig, SubagentRuntime,
    SubagentTranscriptProvider, TaskCancellationGuard, TaskCancellations, TaskTrace,
};
pub use store::{
    compare_shadow, prepare_typed_session, project_session_facts,
    project_session_facts_with_surface, project_session_id, project_state_path, store_open_count,
    summarize_shadow_reports, AdmittedInput, ArtifactCleanupPlan, ArtifactFileReport, Delivery,
    EpisodeRecord, FunnelCounts, RecallEvent, RecallLinkStats, RecallMetrics, Session, SessionFact,
    SessionFactEnvelope, SessionFactError, SessionInvariant, SessionProjection, SessionStore,
    SessionTurnTerminal, ShadowComparison, ShadowVerdictStats, StorageReport, StoreError,
    StoredEvent, StoredProcess, StoredWorkEvent, TypedSessionWriter, WorkCheckpoint, WorkEvidence,
    WorkFact, WorkProjection, WorkUnitSpec, WorkUnitStatus, MAX_CHECKPOINT_SUMMARY_CHARS,
    MAX_WORK_ITEM_CHARS, MAX_WORK_LIST_ITEMS, MAX_WORK_OBJECTIVE_CHARS, SUBAGENT_TRANSCRIPT,
    WORK_PROJECTION_FORMAT_VERSION,
};
