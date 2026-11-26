export type ViewId = "chat" | "workspace" | "memory" | "skills" | "settings";
export type ThemeMode = "dark" | "light";

export interface AppInfo {
  version: string;
  build: string;
}

export interface UiPrefs {
  theme?: ThemeMode | null;
  backdrop?: Record<string, unknown> | null;
  work_priority?: Record<string, string>;
  continue_prompt?: string | null;
  process_auto_state?: Record<string, AutoPrefsEntry>;
  workspace_state?: Record<string, Record<string, unknown>>;
  ui_layout?: Record<string, unknown>;
  memory_view?: "list" | "graph" | null;
}

export interface AutoPrefsEntry {
  enabled?: boolean;
  paused?: boolean;
  stopAfterRound?: boolean;
  mode?: string;
}

export interface ProjectsPrefs {
  projects: string[];
  current: string | null;
  names: Record<string, string>;
}

export interface ComposerDraftRecord {
  key: string;
  draft: {
    text?: string;
    attachments?: PromptAttachment[];
  };
}

export interface AppBootstrapPayload {
  appInfo: AppInfo;
  uiPrefs: UiPrefs;
  projects: ProjectsPrefs;
  generalChatRoot: string | null;
  composerDrafts: {
    items: ComposerDraftRecord[];
    error: string | null;
  };
  target: {
    kind: "project" | "general" | "workspace";
    projectDir: string | null;
  };
}

export interface WorkspaceLineSummary {
  id: string;
  label: string;
  title: string | null;
  title_custom: boolean;
  kind: string;
  ordinal: number | null;
  updated_at: number | null;
  session_id: string;
  running: boolean;
  stage: string;
  branch: string | null;
  worktree_path: string | null;
  profile: string | null;
  available: boolean;
}

export interface WorkspaceProjectSummary {
  path: string;
  project_id: string;
  name: string;
  current: boolean;
  status?: "running" | "idle";
  updated_at?: number | null;
  pending_count?: number;
  lines?: WorkspaceLineSummary[];
  lines_total?: number;
  running_lines?: number;
  freshness?: "fresh" | "stale" | "unavailable";
  error?: string | null;
}

export interface WorkspaceOverview {
  current: string | null;
  projects: WorkspaceProjectSummary[];
  observed_at: number;
}

export interface SessionLifecycleSnapshot {
  existence?: string;
  phase?: string;
  activity?: string | null;
  revision?: number;
  round_generation?: number;
  error?: string | null;
}

export interface ProcessInfo {
  id: string;
  origin_project: string;
  project_dir: string;
  worktree_path: string | null;
  branch: string | null;
  session_id: string;
  model: string | null;
  profile: string | null;
  research_topic: string | null;
  reasoning: string | null;
  manual_models: string[];
  subagent_mode: string;
  tracker_writes: boolean;
  stage: string;
  running: boolean;
  runtime?: SessionLifecycleSnapshot;
  label: string;
  title: string | null;
  title_custom: boolean;
  kind: string;
  ordinal: number | null;
  updated_at: number | null;
}

export interface MessagePart {
  type: string;
  text?: string;
  id?: string;
  name?: string;
  input?: Record<string, unknown>;
  tool_use_id?: string;
  content?: string;
  is_error?: boolean;
  media_type?: string;
  data?: string;
  [key: string]: unknown;
}

export interface ChatMessage {
  role: "user" | "assistant" | "system" | "tool";
  origin?: string;
  parts: MessagePart[];
  reasoning?: string | null;
}

export interface ConversationMessagePage {
  items: ChatMessage[];
  total: number;
  startIndex: number;
  endIndex: number;
  hasMore: boolean;
}

export interface PromptAttachment {
  file_name: string;
  media_type: string;
  data: string;
}

export interface LiveToolCall {
  id: string;
  name: string;
  summary: string;
  input?: unknown;
  output?: string;
  preview?: string;
  ok?: boolean;
  done: boolean;
  durationMs?: number | null;
  display?: Record<string, unknown> | null;
}

export interface LiveSubagent {
  id: string;
  label: string;
  agent?: string | null;
  model?: string | null;
  status: "running" | "done" | "error";
  progress: string[];
  startedAt: number;
}

export interface PendingAskItem {
  id: number;
  sessionId: string;
  source?: string;
  agentId?: string | null;
  kind?: "permission" | "question";
  action: string;
  resource: string;
  question?: string;
  options?: string[];
  projectRoot?: string;
}

export interface PendingInputItem {
  input_id: string;
  prompt: string;
  delivery: string;
  created_at?: number;
}

export interface GitStatusSummary {
  repo: "own" | "none" | "parent";
  toplevel?: string;
  branch: string | null;
  changes: number;
  last: string | null;
  additions: number;
  deletions: number;
  files: Array<{
    path: string;
    additions: number;
    deletions: number;
    binary?: boolean;
    untracked?: boolean;
  }>;
}

export interface FilePreviewData {
  path: string;
  content?: string | null;
  hash?: string | null;
  readonly?: string | null;
  binary?: boolean;
  truncated?: boolean;
  lines?: number;
  size?: number;
}

export interface FileGitDiffData {
  path: string;
  status?: string;
  diff?: string;
  binary?: boolean;
  untracked?: boolean;
  message?: string;
}

export interface ModelCatalogItem {
  id: string;
  label: string;
}

export interface EffectiveModelView {
  model?: { value: string; source: string; provider?: string; model?: string };
  reasoning?: { value: string; source: string };
  codexFastMode?: { value: boolean; source: string };
  effective?: string;
  [key: string]: unknown;
}

export interface AutoRunStatus {
  enabled: boolean;
  paused: boolean;
  goal: string | null;
}

export interface LogEntry {
  id: number;
  at: number;
  level: "info" | "warn" | "error";
  text: string;
}

export interface ToastItem {
  id: number;
  kind: "info" | "ok" | "warn" | "error";
  text: string;
}

export interface OpenToolInfo {
  id: string;
  label: string;
  builtin?: boolean;
  available?: boolean;
  command?: string;
  args?: string[];
}
