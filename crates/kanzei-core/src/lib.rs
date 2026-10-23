//! kanzei-core: session 运行时。
//! M0:内存态一次性 runner;M2 起换成 SQLite 事件溯源 + steer/queue 调度。

pub mod assemble;
pub mod experience_events;
pub mod history;
pub mod notification;
pub mod orchestration;
pub mod replay;
pub mod research;
pub mod research_callback;
pub mod runner;
mod store;

pub use assemble::build_route;
pub use history::filter_message_history;
pub use notification::AgentNotification;
pub use research::{
    ensure_result_artifact_skeleton, load_research_topic, parse_exploration_markdown,
    ExperimentResult, ExperimentResultStatus, ExplorationDocument, ExplorationFrontmatter,
    ExplorationStatus, ParsedExploration, ResearchDiagnostic, ResearchError, ResearchTopic,
    ResultArtifactSkeleton,
};
pub use research_callback::{
    encode_callback_line, parse_callback_line, CallbackEncodeError, CallbackMessage, CallbackStats,
    ParsedCallbackLine, ResearchCallbackEvent, CALLBACK_PREFIX, MAX_CALLBACK_LINE_BYTES,
};
pub use runner::{
    compact_conversation, compact_conversation_with_model, compaction_budget, completed_entry,
    estimate_conversation_tokens, is_usable_failure_kind, mask_volatile_payload,
    normalize_fp_marker, pending_background_subagents, project_observation_context,
    prune_conversation, prune_conversation_with_archive, run_once, run_once_with_parts,
    run_read_agent, run_subagent, summarize_failures, summarize_metrics, summarize_tools,
    ui_tool_content, AskFuture, AskOption, AskPolicy, AskReply, AskRequest, AskResponse,
    BackgroundEventSink, CancellationToken, CompletedEntry, DelegationFuture, DelegationHost,
    DigestModel, FailureSignal, ObservationProjection, RecallHit, RecallOutcome, RecallPolicy,
    RecallRunOutcome, RecallTrigger, RecallWatch, RunEvent, RunMetrics, RunSummary, RunnerConfig,
    SubagentOptions, SubagentRuntime, SubagentTranscriptProvider, TaskCancellationGuard,
    TaskCancellations, TaskTrace, TOOL_END_UI_CONTENT_MAX,
};
pub use store::{
    compare_shadow, prepare_typed_session, project_conversation_facts, project_session_facts,
    project_session_facts_with_surface, project_session_id, project_state_path, store_open_count,
    summarize_shadow_reports, AdmittedInput, ArtifactCleanupPlan, ArtifactFileReport,
    AvailableMemory, Delivery, EpisodeRecord, FunnelCounts, MemoryFeedbackKind,
    MemoryRecallObservation, MemoryRetentionCounts, MemoryUsageCounts, RecallEvent,
    RecallLinkStats, RecallMetrics, ResearchRunEvent, ResearchRunRecord, Session,
    SessionArtifactReport, SessionDeletionPlan, SessionDeletionResult, SessionFact,
    SessionFactEnvelope, SessionFactError, SessionInvariant, SessionProjection, SessionStore,
    SessionTurnTerminal, ShadowComparison, ShadowVerdictStats, StorageBackupReport,
    StorageCleanupPlan, StorageCleanupResult, StorageReport, StoreError, StoredEvent, StoredInput,
    StoredProcess, StoredWorkEvent, TypedSessionWriter, WorkCheckpoint, WorkEvidence, WorkFact,
    WorkNodeContract, WorkNodeKind, WorkNodeReadiness, WorkNodeStep, WorkProjection, WorkUnitSpec,
    WorkUnitStatus, MAX_CHECKPOINT_SUMMARY_CHARS, MAX_WORK_ITEM_CHARS, MAX_WORK_LIST_ITEMS,
    MAX_WORK_OBJECTIVE_CHARS, SUBAGENT_TRANSCRIPT, WORK_PROJECTION_FORMAT_VERSION,
};

// Public domain services. The SQLite implementation stays private.
pub use store::{artifact_liveness, context_manifest, decisions, session_execution};
pub use store::{
    capture_file_preimage_in, decode_session_fact, file_checkpoint_blob_path,
    file_checkpoint_path_key, file_checkpoint_tree_root, record_file_postimage_in,
    stable_json_hash, stable_message_hash, FileCheckpointTarget, TaskOutcome,
    FILE_CHECKPOINT_UNKNOWN_PRE_BYTES,
};

pub use store::{
    capture_file_preimage, file_checkpoint_rel_path, project_work_events, record_file_postimage,
    EffectEstimate, EvalCaseSet, InterruptedAssistant, OriginalConversation, RewindFile,
    RewindPlan, TaskCompatibilityAudit, TaskLegacyProjection, TaskMetricsProjection,
    TaskProjection, TaskRoundProjection, TaskStatus, ASSISTANT_DRAFT_APPENDED,
    ASSISTANT_MESSAGE_COMMITTED, ASSISTANT_MESSAGE_INTERRUPTED, FILE_CHECKPOINT_MAX_BYTES,
    LEGACY_SEEDED, SESSION_EVENT_FORMAT_VERSION, TASK_CLOSED_EVENT_TYPE, TASK_EVENT_SCHEMA_VERSION,
    TASK_MEMBERSHIP_ADDED_EVENT_TYPE, TASK_STARTED_EVENT_TYPE, TOOL_CALLED, TOOL_RESULT_COMMITTED,
    TOOL_RESULT_INTERRUPTED, TURN_COMPLETED, TURN_FAILED, TURN_STARTED, TURN_STOPPED,
    USER_MESSAGE_COMMITTED,
};
