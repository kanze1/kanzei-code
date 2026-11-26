import { invoke } from "../ipc/client";
import type { LogEntry, ThemeMode, ToastItem, ViewId } from "../ipc/types";
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

export interface UiState {
  view: ViewId;
  viewHistory: ViewId[];
  theme: ThemeMode;
  sidebarOpen: boolean;
  sidebarWidth: number;
  logPanelOpen: boolean;
  paletteOpen: boolean;
  newProjectOpen: boolean;
  projectModelsOpen: boolean;
  contextPopoverOpen: boolean;
  viewer: ViewerDialogState | null;
  confirmDialog: ConfirmDialogRequest | null;
  inputDialog: InputDialogRequest | null;
  toasts: ToastItem[];
  logs: LogEntry[];
  closingWindow: boolean;
  windowError: string;
}

let toastSeq = 1;
let logSeq = 1;

export const uiStore = createStore<UiState>({
  view: "chat",
  viewHistory: [],
  theme: "dark",
  sidebarOpen: true,
  sidebarWidth: 260,
  logPanelOpen: false,
  paletteOpen: false,
  newProjectOpen: false,
  projectModelsOpen: false,
  contextPopoverOpen: false,
  viewer: null,
  confirmDialog: null,
  inputDialog: null,
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
