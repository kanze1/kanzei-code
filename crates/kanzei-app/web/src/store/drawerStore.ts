import { formatError, invoke } from "../ipc/client";
import type { FileGitDiffData, FilePreviewData, GitStatusSummary } from "../ipc/types";
import { createStore, useStore } from "./createStore";
import { pushToast } from "./uiStore";

export type DrawerMode = "files" | "activity" | "worktrees";
export type FileViewTab = "preview" | "diff" | "edit";

export interface DrawerState {
  open: boolean;
  mode: DrawerMode;
  width: number;
  projectDir: string | null;
  worktreePath: string | null;
  gitStatus: GitStatusSummary | null;
  touchedFiles: string[];
  activeFile: string | null;
  fileTab: FileViewTab;
  highlightLine: number | null;
  loadingFile: boolean;
  preview: FilePreviewData | null;
  diff: FileGitDiffData | null;
  editContent: string;
  editHash: string | null;
  editDirty: boolean;
  saving: boolean;
  conflictMessage: string | null;
  error: string | null;
}

export const drawerStore = createStore<DrawerState>({
  open: false,
  mode: "files",
  width: 440,
  projectDir: null,
  worktreePath: null,
  gitStatus: null,
  touchedFiles: [],
  activeFile: null,
  fileTab: "preview",
  highlightLine: null,
  loadingFile: false,
  preview: null,
  diff: null,
  editContent: "",
  editHash: null,
  editDirty: false,
  saving: false,
  conflictMessage: null,
  error: null,
});

export function useDrawerStore<S>(selector: (state: DrawerState) => S): S {
  return useStore(drawerStore, selector);
}

export function setDrawerWidth(width: number): void {
  drawerStore.setState({ width: Math.max(300, Math.min(680, Math.round(width))) });
}

export function toggleDrawer(mode: DrawerMode): void {
  const cur = drawerStore.getState();
  if (cur.open && cur.mode === mode) {
    drawerStore.setState({ open: false });
  } else {
    drawerStore.setState({ open: true, mode });
    if (mode === "files" && cur.projectDir) {
      void refreshGitStatus(cur.projectDir, cur.worktreePath);
    }
  }
}

export function closeDrawer(): void {
  drawerStore.setState({ open: false });
}

export function setDrawerContext(projectDir: string | null, worktreePath: string | null): void {
  const prev = drawerStore.getState();
  if (prev.projectDir === projectDir && prev.worktreePath === worktreePath) return;
  drawerStore.setState({
    projectDir,
    worktreePath,
    gitStatus: null,
    touchedFiles: [],
    activeFile: null,
    preview: null,
    diff: null,
    editContent: "",
    editHash: null,
    editDirty: false,
    conflictMessage: null,
    error: null,
  });
}

export function noteTouchedFile(path: string): void {
  const cleaned = path.trim().replace(/\\/g, "/");
  if (!cleaned) return;
  drawerStore.setState((prev) => {
    if (prev.touchedFiles.includes(cleaned)) return prev;
    return { ...prev, touchedFiles: [cleaned, ...prev.touchedFiles].slice(0, 60) };
  });
}

export async function refreshGitStatus(
  projectDir: string,
  worktreePath: string | null = null
): Promise<GitStatusSummary | null> {
  try {
    const status = await invoke<GitStatusSummary>("git_status", {
      projectDir,
      worktreePath,
    });
    if (drawerStore.getState().projectDir === projectDir) {
      drawerStore.setState({ gitStatus: status });
    }
    return status;
  } catch {
    return null;
  }
}

export async function openFileInDrawer(
  path: string,
  options?: { line?: number | null; tab?: FileViewTab; projectDir?: string | null }
): Promise<void> {
  const state = drawerStore.getState();
  const projectDir = options?.projectDir ?? state.projectDir;
  if (!projectDir) {
    pushToast("请先选择一个项目以查看文件上下文", "warn");
    return;
  }
  const tab = options?.tab ?? "preview";
  drawerStore.setState({
    open: true,
    mode: "files",
    projectDir,
    activeFile: path,
    fileTab: tab,
    highlightLine: options?.line ?? null,
    loadingFile: true,
    conflictMessage: null,
    error: null,
  });
  noteTouchedFile(path);

  try {
    const [preview, diff] = await Promise.all([
      invoke<FilePreviewData>("file_preview", { projectDir, path }),
      invoke<FileGitDiffData>("file_git_diff", {
        projectDir,
        path,
        worktreePath: drawerStore.getState().worktreePath,
      }).catch(() => null),
    ]);
    if (drawerStore.getState().activeFile !== path) return;
    drawerStore.setState({
      loadingFile: false,
      preview,
      diff,
      editContent: preview.content ?? "",
      editHash: preview.hash ?? null,
      editDirty: false,
    });
  } catch (error) {
    if (drawerStore.getState().activeFile !== path) return;
    drawerStore.setState({
      loadingFile: false,
      error: formatError(error),
    });
  }
}

export function setDrawerFileTab(tab: FileViewTab): void {
  drawerStore.setState({ fileTab: tab, conflictMessage: null });
}

export function setDrawerEditContent(content: string): void {
  const cur = drawerStore.getState();
  drawerStore.setState({
    editContent: content,
    editDirty: content !== (cur.preview?.content ?? ""),
  });
}

export async function saveDrawerFile(force = false): Promise<boolean> {
  const state = drawerStore.getState();
  if (!state.projectDir || !state.activeFile) return false;
  drawerStore.setState({ saving: true, conflictMessage: null });
  try {
    const res = await invoke<{
      ok?: boolean;
      conflict?: boolean;
      hash?: string;
      message?: string;
    }>("file_write", {
      projectDir: state.projectDir,
      path: state.activeFile,
      content: state.editContent,
      expectedHash: force ? null : state.editHash,
      bom: false,
      evidence: true,
    });
    if (res?.conflict) {
      drawerStore.setState({
        saving: false,
        conflictMessage: res.message ?? "磁盘文件已变化，请确认是否强制覆盖或重新加载",
      });
      return false;
    }
    const nextHash = res?.hash ?? state.editHash;
    drawerStore.setState((prev) => ({
      ...prev,
      saving: false,
      editDirty: false,
      editHash: nextHash,
      preview: prev.preview
        ? { ...prev.preview, content: prev.editContent, hash: nextHash }
        : prev.preview,
    }));
    pushToast(`已保存 ${state.activeFile}`, "ok");
    void refreshGitStatus(state.projectDir, state.worktreePath);
    return true;
  } catch (error) {
    const msg = formatError(error);
    drawerStore.setState({ saving: false, conflictMessage: msg });
    pushToast(`保存失败：${msg}`, "error");
    return false;
  }
}

export async function openExternalPath(
  path: string,
  toolId?: string
): Promise<void> {
  const state = drawerStore.getState();
  const projectDir = state.projectDir ?? path;
  try {
    if (toolId) {
      await invoke("open_with", {
        toolId,
        projectDir,
        path: state.projectDir ? path : null,
      });
    } else {
      await invoke("reveal_path", {
        projectDir,
        path: state.projectDir && path !== state.projectDir ? path : null,
      });
    }
  } catch (error) {
    pushToast(`外部打开失败：${formatError(error)}`, "error");
  }
}
