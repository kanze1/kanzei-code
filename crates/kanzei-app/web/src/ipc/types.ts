export type ViewId = "chat" | "workspace" | "wiki" | "memory" | "skills" | "settings";
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
  projectDir?: string;
  processId?: string;
  source?: string;
  agentId?: string | null;
  kind?: "permission" | "question";
  action: string;
  resource: string;
  remember?: string;
  question?: string;
  options?: string[];
  projectRoot?: string;
}

export interface CompletedSessionNotice {
  sessionId: string;
  projectDir: string;
  processId?: string;
  label: string;
  at: number;
}

export interface QuickShellResult {
  command: string;
  cwd: string;
  exitCode: number;
  stdout: string;
  stderr: string;
  durationMs: number;
  truncated: boolean;
  timedOut: boolean;
}

export interface SideQuestionTurn {
  role: "user" | "assistant";
  text: string;
  model?: string;
}

export interface HookRuleItem {
  event: "pre_tool" | "post_tool" | "on_stop";
  toolMatcher?: string | null;
  command: string;
  timeoutSecs?: number | null;
  enabled: boolean;
}

export interface RepoWikiPageSummary {
  areaId: string;
  title: string;
  kind: string;
  parent: string | null;
  band: number;
  bandLabel: string;
  depth: number;
  status: "fresh" | "stale" | "missing";
  fileCount: number;
  sourceFingerprint: string;
  generatedAt: number | null;
  model: string | null;
}

export interface RepoWikiOverview {
  projectDir: string;
  wikiDir: string;
  totalPages: number;
  freshCount: number;
  staleCount: number;
  missingCount: number;
  deps: Array<{ from: string; to: string }>;
  pages: RepoWikiPageSummary[];
}

export interface RepoWikiDetail {
  areaId: string;
  title: string;
  path: string;
  status: "fresh" | "stale" | "missing";
  content: string | null;
  files: string[];
  outboundDeps: string[];
  inboundDeps: string[];
  sourceFingerprint: string;
  generatedAt: number | null;
  model: string | null;
}

export interface ActionReportData {
  sessionId: string;
  completedAt: number;
  elapsedMs: number | null;
  inputTokens: number;
  outputTokens: number;
  toolsTotal: number;
  toolsFailed: number;
  toolNames: string[];
  touchedFiles: string[];
  modifiedFiles?: string[];
  autoReason?: string | null;
  handoffSummary?: string | null;
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

export interface ModelCatalogError {
  provider: string;
  message: string;
}

export interface EffectiveModelChoice {
  ref?: string;
  resolved?: string | null;
  value?: string;
  source?: string;
  role?: string | null;
  followsPrimary?: boolean;
  error?: string | null;
}

export interface EffectiveReasoningChoice {
  value: string;
  source: string;
}

export interface EffectiveFastModeView {
  enabled: boolean;
  applies?: boolean;
  active?: boolean;
  source?: string;
}

export interface EffectiveModelView {
  agent?: string | null;
  agentError?: string | null;
  model?: EffectiveModelChoice;
  defaultModel?: EffectiveModelChoice;
  reasoning?: EffectiveReasoningChoice;
  defaultReasoning?: EffectiveReasoningChoice;
  codexFastMode?: EffectiveFastModeView;
  contextLimit?: number | null;
  effective?: string;
  [key: string]: unknown;
}

export interface ProjectModelFieldView<T = string | null> {
  project: T;
  global: T;
  inherited: T;
  inheritedSource: string;
  inheritedFollowsPrimary: boolean;
  effective: T;
  source: string;
  followsPrimary: boolean;
}

export interface ProjectModelsResponse {
  projectDir: string;
  configPath: string;
  exists: boolean;
  fields: {
    primary?: ProjectModelFieldView<string | null>;
    fast?: ProjectModelFieldView<string | null>;
    compact?: ProjectModelFieldView<string | null>;
    reasoning?: ProjectModelFieldView<string | null>;
    codexFastMode?: ProjectModelFieldView<boolean | null>;
  };
}

export interface ProjectModelOverrideSummary {
  project: string;
  name: string;
  configPath?: string;
  keys: string[];
  current?: boolean;
}

export interface PermissionRuleRecord {
  index: number;
  action: string;
  resource: string;
  effect: string;
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
