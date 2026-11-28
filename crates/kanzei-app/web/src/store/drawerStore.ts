import { formatError, invoke } from "../ipc/client";
import type {
  FileGitDiffData,
  FilePreviewData,
  FileWriteResponse,
  GitStatusSummary,
} from "../ipc/types";
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
  conflictHash: string | null;
  conflictExists: boolean;
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
  conflictHash: null,
  conflictExists: true,
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
    conflictHash: null,
    conflictExists: true,
    error: null,
  });
}

function decodePathSegments(path: string): string {
  return path
    .split("/")
    .map((segment) => {
      if (!segment.includes("%")) return segment;
      try {
        return decodeURIComponent(segment);
      } catch {
        return segment;
      }
    })
    .join("/");
}

function stripVerbatimPrefix(path: string): string {
  if (path.startsWith("//?/UNC/")) {
    return "//" + path.slice("//?/UNC/".length);
  }
  if (path.startsWith("//?/")) {
    return path.slice("//?/".length);
  }
  return path;
}

function stripLeadingWindowsSlash(path: string): string {
  return /^\/[a-z]:\//i.test(path) ? path.slice(1) : path;
}

function stripWrappingQuotes(path: string): string {
  if (
    (path.startsWith('"') && path.endsWith('"')) ||
    (path.startsWith("'") && path.endsWith("'")) ||
    (path.startsWith("<") && path.endsWith(">"))
  ) {
    return path.slice(1, -1).trim();
  }
  return path;
}

function splitFileAnchor(raw: string): { target: string; line: number | null } {
  const lineAnchor =
    /(?:#L([1-9]\d*)(?:C\d+)?(?:-L?[1-9]\d*(?:C\d+)?)?|:([1-9]\d*)(?:-[1-9]\d*|:\d+)?)$/i.exec(raw);
  if (lineAnchor) {
    return {
      target: raw.slice(0, lineAnchor.index),
      line: Number(lineAnchor[1] ?? lineAnchor[2]),
    };
  }
  const hashIdx = raw.indexOf("#");
  if (hashIdx > 0) {
    return { target: raw.slice(0, hashIdx), line: null };
  }
  return { target: raw, line: null };
}

function normalizeFileInput(file: string, decodeUri: boolean): string {
  let raw = stripWrappingQuotes(file.trim());
  let isFileUri = false;
  if (/^file:\/\//i.test(raw)) {
    isFileUri = true;
    raw = raw.slice("file://".length);
    if (/^localhost\//i.test(raw)) {
      raw = raw.slice("localhost".length);
    } else if (/^[^/]+\//.test(raw) && !/^[a-z](?::|%3a)/i.test(raw)) {
      raw = "//" + raw;
    }
  }
  let normalized = stripVerbatimPrefix(raw.replace(/\\/g, "/"));
  if (isFileUri || decodeUri) {
    normalized = decodePathSegments(normalized);
  }
  return stripLeadingWindowsSlash(normalized);
}

function stripRootPrefix(path: string, rootDir: string): string | null {
  const cleanRoot = stripVerbatimPrefix(rootDir.trim().replace(/\\/g, "/")).replace(/\/+$/, "");
  if (!cleanRoot) return null;
  const prefix = cleanRoot + "/";
  const windowsRoot = /^(?:[a-z]:\/|\/\/)/i.test(prefix);
  const matches = (candidate: string): boolean =>
    windowsRoot
      ? candidate.slice(0, prefix.length).toLowerCase() === prefix.toLowerCase()
      : candidate.startsWith(prefix);
  if (matches(path)) {
    return path.slice(prefix.length);
  }
  if (/%[0-9a-f]{2}/i.test(path)) {
    const decoded = stripLeadingWindowsSlash(decodePathSegments(path));
    if (matches(decoded)) {
      return decoded.slice(prefix.length);
    }
  }
  return null;
}

function isAbsoluteFilePath(path: string): boolean {
  return path.startsWith("/") || path.startsWith("//") || /^[a-z]:\//i.test(path);
}

// File IPC accepts project-relative names. Leave external absolute paths to its root check.
function projectRelativePath(
  file: string,
  projectDir: string,
  worktreePath?: string | null,
  decodeUri = false
): string {
  const path = normalizeFileInput(file, decodeUri);
  for (const root of [worktreePath, projectDir]) {
    if (!root) continue;
    const rel = stripRootPrefix(path, root);
    if (rel !== null) return rel;
  }
  return path;
}

export function noteTouchedFile(path: string): void {
  const state = drawerStore.getState();
  const raw = path.trim();
  if (!raw) return;
  const cleaned = state.projectDir
    ? projectRelativePath(raw, state.projectDir, state.worktreePath)
    : raw.replace(/\\/g, "/");
  if (!cleaned || isAbsoluteFilePath(cleaned)) return;
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
    const cur = drawerStore.getState();
    if (cur.projectDir === projectDir && cur.worktreePath === worktreePath) {
      drawerStore.setState({ gitStatus: status });
    }
    return status;
  } catch {
    return null;
  }
}

export async function openFileInDrawer(
  file: string,
  options?: {
    line?: number | null;
    tab?: FileViewTab;
    projectDir?: string | null;
    worktreePath?: string | null;
    decodeUri?: boolean;
  }
): Promise<void> {
  const state = drawerStore.getState();
  const projectDir = options?.projectDir ?? state.projectDir;
  const worktreePath =
    options?.worktreePath !== undefined ? options.worktreePath : state.worktreePath;
  if (!projectDir) {
    pushToast("请先选择一个项目以查看文件上下文", "warn");
    return;
  }
  const { target: strippedFile, line: parsedLine } = splitFileAnchor(
    stripWrappingQuotes(file.trim())
  );
  const path = projectRelativePath(
    strippedFile,
    projectDir,
    worktreePath,
    Boolean(options?.decodeUri)
  );
  const highlightLine = options?.line ?? parsedLine;
  const tab = options?.tab ?? "preview";
  drawerStore.setState({
    open: true,
    mode: "files",
    projectDir,
    worktreePath,
    activeFile: path,
    fileTab: tab,
    highlightLine,
    loadingFile: true,
    conflictMessage: null,
    conflictHash: null,
    conflictExists: true,
    error: null,
  });

  try {
    const [preview, diff] = await Promise.all([
      invoke<FilePreviewData>("file_preview", { projectDir, path, worktreePath }),
      invoke<FileGitDiffData>("file_git_diff", {
        projectDir,
        path,
        worktreePath,
      }).catch(() => null),
    ]);
    const latest = drawerStore.getState();
    if (
      latest.activeFile !== path ||
      latest.projectDir !== projectDir ||
      latest.worktreePath !== worktreePath
    ) {
      return;
    }
    noteTouchedFile(path);
    drawerStore.setState({
      loadingFile: false,
      preview: { ...preview, path },
      diff,
      editContent: preview.content ?? "",
      editHash: preview.hash ?? null,
      editDirty: false,
      conflictMessage: null,
      conflictHash: null,
      conflictExists: true,
    });
  } catch (error) {
    const latest = drawerStore.getState();
    if (
      latest.activeFile !== path ||
      latest.projectDir !== projectDir ||
      latest.worktreePath !== worktreePath
    ) {
      return;
    }
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
  const { projectDir, worktreePath, activeFile, editContent } = state;
  const expectedHash = force
    ? state.conflictExists === false
      ? null
      : (state.conflictHash ?? state.editHash)
    : state.editHash;
  drawerStore.setState({ saving: true, conflictMessage: null });
  try {
    const res = await invoke<FileWriteResponse>("file_write", {
      projectDir,
      path: activeFile,
      content: editContent,
      expectedHash,
      bom: Boolean(state.preview?.bom),
      evidence: Boolean(force && state.conflictExists !== false),
      worktreePath,
    });
    const isSameFile = (cur: DrawerState) =>
      cur.projectDir === projectDir &&
      cur.worktreePath === worktreePath &&
      cur.activeFile === activeFile;

    if (res?.status === "conflict") {
      const exists = res.exists !== false;
      const conflictMessage = exists
        ? "磁盘文件已被外部修改，尚未写入。可点击「重新加载」放弃当前编辑，或点击「强制覆盖」（覆盖前自动留证）。"
        : "磁盘文件已被外部删除，尚未写入。可点击「重新创建」按当前编辑重建文件，或重新加载。";
      drawerStore.setState((prev) =>
        isSameFile(prev)
          ? {
              ...prev,
              saving: false,
              editDirty: true,
              conflictMessage,
              conflictHash: res.hash ?? null,
              conflictExists: exists,
            }
          : { ...prev, saving: false }
      );
      pushToast(exists ? "保存冲突：磁盘文件已变化，未写入" : "保存冲突：磁盘文件已删除，未写入", "warn");
      return false;
    }
    if (res?.status !== "saved") {
      const msg = `后端未确认文件已保存（status=${String(res?.status ?? "unknown")}）`;
      drawerStore.setState((prev) =>
        isSameFile(prev)
          ? {
              ...prev,
              saving: false,
              editDirty: true,
              conflictMessage: msg,
            }
          : { ...prev, saving: false }
      );
      pushToast(`保存失败：${msg}`, "error");
      return false;
    }
    const nextHash = res.hash ?? state.editHash;
    drawerStore.setState((prev) => {
      if (!isSameFile(prev)) {
        return { ...prev, saving: false };
      }
      const hasNewerEdits = prev.editContent !== editContent;
      return {
        ...prev,
        saving: false,
        editDirty: hasNewerEdits,
        editHash: nextHash,
        conflictMessage: null,
        conflictHash: null,
        conflictExists: true,
        preview: prev.preview
          ? { ...prev.preview, content: editContent, hash: nextHash }
          : prev.preview,
      };
    });
    pushToast(`已保存 ${activeFile}`, "ok");
    void invoke<FileGitDiffData>("file_git_diff", {
      projectDir,
      path: activeFile,
      worktreePath,
    })
      .then((nextDiff) => {
        if (isSameFile(drawerStore.getState())) {
          drawerStore.setState({ diff: nextDiff });
        }
      })
      .catch(() => {});
    void refreshGitStatus(projectDir, worktreePath);
    return true;
  } catch (error) {
    const msg = formatError(error);
    drawerStore.setState((prev) =>
      prev.projectDir === projectDir &&
      prev.worktreePath === worktreePath &&
      prev.activeFile === activeFile
        ? { ...prev, saving: false, conflictMessage: msg }
        : { ...prev, saving: false }
    );
    pushToast(`保存失败：${msg}`, "error");
    return false;
  }
}

export async function openExternalPath(
  path: string,
  toolId?: string,
  options?: { processId?: string | null }
): Promise<void> {
  const state = drawerStore.getState();
  const projectDir = state.projectDir ?? path;
  const processId = options?.processId ?? null;
  try {
    if (toolId) {
      await invoke("open_with", {
        toolId,
        projectDir,
        processId,
        path: state.projectDir ? path : null,
      });
    } else {
      await invoke("reveal_path", {
        projectDir,
        processId,
        path: state.projectDir && path !== state.projectDir ? path : null,
      });
    }
  } catch (error) {
    pushToast(`外部打开失败：${formatError(error)}`, "error");
  }
}
