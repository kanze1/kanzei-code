import { invoke } from "../ipc/client";
import type {
  CompletedSessionNotice,
  LogEntry,
  PendingAskItem,
  ThemeMode,
  ToastItem,
  ViewId,
} from "../ipc/types";
import { createStore, useStore } from "./createStore";

export interface ConfirmDialogRequest {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  resolve: (accepted: boolean) => void;
}

export interface InputDialogRequest {
  title: string;
  label?: string;
  initialValue?: string;
  placeholder?: string;
  confirmLabel?: string;
  resolve: (value: string | null) => void;
}

export interface ViewerDialogState {
  title: string;
  subtitle?: string;
  content: string;
  kind: "text" | "markdown" | "image" | "svg";
  imageSrc?: string;
}

export interface RewindPreviewData {
  sourceHash: string;
  files: Array<{ path?: string; action?: string; [key: string]: unknown } | string>;
  unhandled: Array<string | { path?: string; reason?: string }>;
  prompt: string;
  worktree: boolean;
  keptMessages: number;
}

export interface RewindDialogRequest {
  text: string;
  occurrenceFromEnd: number;
  preview: RewindPreviewData;
  resolve: (choice: { action: "conversation" | "code" | "both"; force?: boolean } | null) => void;
}

export interface UiState {
  view: ViewId;
  viewHistory: ViewId[];
  theme: ThemeMode;
  soundEnabled: boolean;
  sidebarOpen: boolean;
  sidebarWidth: number;
  logPanelOpen: boolean;
  paletteOpen: boolean;
  newProjectOpen: boolean;
  projectModelsOpen: boolean;
  projectModelsTargetDir: string | null;
  contextPopoverOpen: boolean;
  contextInspectorNonce: number;
  traceDialogOpen: boolean;
  inboxOpen: boolean;
  globalAsks: PendingAskItem[];
  recentCompleted: CompletedSessionNotice[];
  btwOpen: boolean;
  btwInitialQuestion: string;
  viewer: ViewerDialogState | null;
  confirmDialog: ConfirmDialogRequest | null;
  inputDialog: InputDialogRequest | null;
  rewindDialog: RewindDialogRequest | null;
  toasts: ToastItem[];
  logs: LogEntry[];
  closingWindow: boolean;
  windowError: string;
}

const SOUND_STORAGE_KEY = "kz-sound-notice";

function readInitialSoundEnabled(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem(SOUND_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

let toastSeq = 1;
let logSeq = 1;

export const uiStore = createStore<UiState>({
  view: "chat",
  viewHistory: [],
  theme: "dark",
  soundEnabled: readInitialSoundEnabled(),
  sidebarOpen: true,
  sidebarWidth: 260,
  logPanelOpen: false,
  paletteOpen: false,
  newProjectOpen: false,
  projectModelsOpen: false,
  projectModelsTargetDir: null,
  contextPopoverOpen: false,
  contextInspectorNonce: 0,
  traceDialogOpen: false,
  inboxOpen: false,
  globalAsks: [],
  recentCompleted: [],
  btwOpen: false,
  btwInitialQuestion: "",
  viewer: null,
  confirmDialog: null,
  inputDialog: null,
  rewindDialog: null,
  toasts: [],
  logs: [],
  closingWindow: false,
  windowError: "",
});

export function useUiStore<S>(selector: (state: UiState) => S): S {
  return useStore(uiStore, selector);
}

export function applyThemeDom(theme: ThemeMode): void {
  if (typeof document !== "undefined") {
    document.documentElement.dataset.theme = theme;
  }
}

export function setTheme(theme: ThemeMode, persist = true): void {
  applyThemeDom(theme);
  uiStore.setState({ theme });
  if (persist) {
    invoke("ui_prefs_set", { theme }).catch(() => {});
  }
}

export function toggleTheme(): void {
  const next: ThemeMode = uiStore.getState().theme === "dark" ? "light" : "dark";
  setTheme(next, true);
}

export function navigateView(next: ViewId): void {
  const current = uiStore.getState().view;
  if (current === next) return;
  uiStore.setState((prev) => ({
    ...prev,
    view: next,
    viewHistory: [...prev.viewHistory.slice(-19), prev.view],
    paletteOpen: false,
  }));
}

export function navigateBack(): void {
  const history = uiStore.getState().viewHistory;
  if (history.length === 0) return;
  const target = history[history.length - 1];
  uiStore.setState((prev) => ({
    ...prev,
    view: target,
    viewHistory: prev.viewHistory.slice(0, -1),
  }));
}

export function toggleSidebar(): void {
  const next = !uiStore.getState().sidebarOpen;
  uiStore.setState({ sidebarOpen: next });
}

export function setSidebarWidth(width: number): void {
  const clamped = Math.max(200, Math.min(420, Math.round(width)));
  uiStore.setState({ sidebarWidth: clamped });
}

export function triggerContextInspector(): void {
  uiStore.setState((prev) => ({
    ...prev,
    contextInspectorNonce: prev.contextInspectorNonce + 1,
  }));
}

export function openTraceDialog(): void {
  uiStore.setState({ traceDialogOpen: true });
}

export function openBtwOverlay(initialQuestion = ""): void {
  uiStore.setState({ btwOpen: true, btwInitialQuestion: initialQuestion });
}

export function closeBtwOverlay(): void {
  uiStore.setState({ btwOpen: false, btwInitialQuestion: "" });
}

export async function refreshGlobalInbox(): Promise<void> {
  try {
    const asks = await invoke<PendingAskItem[]>("pending_asks_all");
    uiStore.setState({ globalAsks: Array.isArray(asks) ? asks : [] });
  } catch {
    // 静默忽略非 Tauri 预览或启动早期未就绪
  }
}

export function recordBackgroundCompletion(notice: CompletedSessionNotice): void {
  uiStore.setState((prev) => ({
    ...prev,
    recentCompleted: [
      notice,
      ...prev.recentCompleted.filter((item) => item.sessionId !== notice.sessionId),
    ].slice(0, 20),
  }));
}

export function dismissCompletedNotice(sessionId?: string): void {
  uiStore.setState((prev) => ({
    ...prev,
    recentCompleted: sessionId
      ? prev.recentCompleted.filter((item) => item.sessionId !== sessionId)
      : [],
  }));
}

export function pushToast(text: string, kind: ToastItem["kind"] = "info", durationMs = 3600): void {
  const id = toastSeq++;
  uiStore.setState((prev) => ({
    ...prev,
    toasts: [...prev.toasts.slice(-4), { id, kind, text }],
  }));
  if (durationMs > 0 && typeof window !== "undefined") {
    window.setTimeout(() => dismissToast(id), durationMs);
  }
}

export function dismissToast(id: number): void {
  uiStore.setState((prev) => ({
    ...prev,
    toasts: prev.toasts.filter((item) => item.id !== id),
  }));
}

export function appendLog(text: string, level: LogEntry["level"] = "info"): void {
  const id = logSeq++;
  uiStore.setState((prev) => ({
    ...prev,
    logs: [...prev.logs.slice(-399), { id, at: Date.now(), level, text }],
  }));
}

export function clearLogs(): void {
  uiStore.setState({ logs: [] });
}

export function confirmDialog(
  options: Omit<ConfirmDialogRequest, "resolve">
): Promise<boolean> {
  return new Promise((resolve) => {
    uiStore.setState({
      confirmDialog: { ...options, resolve },
    });
  });
}

export function resolveConfirmDialog(accepted: boolean): void {
  const req = uiStore.getState().confirmDialog;
  uiStore.setState({ confirmDialog: null });
  req?.resolve(accepted);
}

export function promptDialog(
  options: Omit<InputDialogRequest, "resolve">
): Promise<string | null> {
  return new Promise((resolve) => {
    uiStore.setState({
      inputDialog: { ...options, resolve },
    });
  });
}

export function resolveInputDialog(value: string | null): void {
  const req = uiStore.getState().inputDialog;
  uiStore.setState({ inputDialog: null });
  req?.resolve(value);
}

export function openViewer(viewer: ViewerDialogState): void {
  uiStore.setState({ viewer });
}

export function closeViewer(): void {
  uiStore.setState({ viewer: null });
}

export function openProjectModelsDialog(targetDir: string | null = null): void {
  uiStore.setState({ projectModelsOpen: true, projectModelsTargetDir: targetDir });
}

export function closeProjectModelsDialog(): void {
  uiStore.setState({ projectModelsOpen: false, projectModelsTargetDir: null });
}

export function openRewindDialog(
  options: Omit<RewindDialogRequest, "resolve">
): Promise<{ action: "conversation" | "code" | "both"; force?: boolean } | null> {
  return new Promise((resolve) => {
    uiStore.setState({
      rewindDialog: { ...options, resolve },
    });
  });
}

export function resolveRewindDialog(
  choice: { action: "conversation" | "code" | "both"; force?: boolean } | null
): void {
  const req = uiStore.getState().rewindDialog;
  uiStore.setState({ rewindDialog: null });
  req?.resolve(choice);
}

export function setSoundEnabled(enabled: boolean): void {
  uiStore.setState({ soundEnabled: enabled });
  if (typeof window !== "undefined") {
    try {
      window.localStorage.setItem(SOUND_STORAGE_KEY, enabled ? "1" : "0");
    } catch {
      // 忽略受限环境存储异常
    }
  }
}

export function playNotificationChime(kind: "done" | "ask" = "done"): void {
  if (typeof window === "undefined") return;
  try {
    const AudioCtx =
      window.AudioContext ||
      (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioCtx) return;
    const ctx = new AudioCtx();
    const notes = kind === "ask" ? [660, 880] : [523.25, 659.25];
    notes.forEach((freq, idx) => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = freq;
      const start = ctx.currentTime + idx * 0.11;
      gain.gain.setValueAtTime(0.0001, start);
      gain.gain.exponentialRampToValueAtTime(0.06, start + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.18);
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start(start);
      osc.stop(start + 0.2);
    });
    window.setTimeout(() => {
      void ctx.close().catch(() => {});
    }, 500);
  } catch {
    // 无音频设备或权限受限时静默忽略
  }
}
