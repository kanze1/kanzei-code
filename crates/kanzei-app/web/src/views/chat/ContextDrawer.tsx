import React, { useEffect, useMemo, useRef, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import { useAppStore } from "../../store/appStore";
import { appendQuoteToComposer, useChatStore } from "../../store/chatStore";
import {
  closeDrawer,
  drawerStore,
  openExternalPath,
  openFileInDrawer,
  refreshGitStatus,
  saveDrawerFile,
  setDrawerEditContent,
  setDrawerFileTab,
  setDrawerWidth,
  useDrawerStore,
} from "../../store/drawerStore";
import { getActiveProcess, useSessionStore } from "../../store/sessionStore";
import { confirmDialog, openViewer, promptDialog, pushToast } from "../../store/uiStore";
import {
  IconClose,
  IconExternal,
  IconFileCode,
  IconPlus,
  IconRefresh,
} from "../../components/Icons";

interface TerminalProcessInfo {
  id: string;
  command?: string;
  running?: boolean;
  exit?: number | null;
  owner?: string;
  subscribed?: boolean;
  output?: string;
}

interface WorktreeItem {
  path: string;
  branch: string;
  clean: boolean;
  files: string[];
  diff: string;
  bound_process?: string | null;
}

interface GateStepResult {
  name: string;
  ok: boolean;
  summary: string;
}

interface SnapshotFileEntry {
  path: string;
  size?: number;
  lines?: number;
}

export interface DiffHunk {
  index: number;
  header: string;
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  lines: string[];
}

export function parseUnifiedDiffHunks(diffText: string): { metaLines: string[]; hunks: DiffHunk[] } {
  const all = diffText.split("\n");
  const metaLines: string[] = [];
  const hunks: DiffHunk[] = [];
  let current: DiffHunk | null = null;
  for (const line of all) {
    const match = line.match(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/);
    if (match) {
      if (current) hunks.push(current);
      current = {
        index: hunks.length,
        header: line,
        oldStart: Number(match[1]),
        oldCount: match[2] !== undefined ? Number(match[2]) : 1,
        newStart: Number(match[3]),
        newCount: match[4] !== undefined ? Number(match[4]) : 1,
        lines: [],
      };
      continue;
    }
    if (!current) {
      metaLines.push(line);
    } else {
      current.lines.push(line);
    }
  }
  if (current) hunks.push(current);
  return { metaLines, hunks };
}

export function applyReverseHunk(content: string, hunk: DiffHunk): string | null {
  const eol = content.includes("\r\n") ? "\r\n" : "\n";
  const hasTrailingEol = content.endsWith("\r\n") || content.endsWith("\n");
  const bodyWithoutTrailingEol = hasTrailingEol
    ? content.slice(0, content.endsWith("\r\n") ? -2 : -1)
    : content;
  const fileLines = content.length === 0 ? [] : bodyWithoutTrailingEol.split(/\r?\n/);

  const expectedNew: string[] = [];
  const restoredOld: string[] = [];
  let lastLineKind: "+" | "-" | " " | null = null;
  let oldNoNewline = false;
  let newNoNewline = false;

  for (const raw of hunk.lines) {
    if (raw.startsWith("\\ ")) {
      if (lastLineKind === "-") {
        oldNoNewline = true;
      } else if (lastLineKind === "+") {
        newNoNewline = true;
      } else if (lastLineKind === " ") {
        oldNoNewline = true;
        newNoNewline = true;
      }
      continue;
    }
    if (raw.startsWith("+")) {
      expectedNew.push(raw.slice(1));
      lastLineKind = "+";
    } else if (raw.startsWith("-")) {
      restoredOld.push(raw.slice(1));
      lastLineKind = "-";
    } else if (raw.startsWith(" ")) {
      const body = raw.slice(1);
      expectedNew.push(body);
      restoredOld.push(body);
      lastLineKind = " ";
    }
  }

  const startIdx =
    hunk.newCount === 0
      ? Math.min(fileLines.length, hunk.newStart)
      : Math.max(0, hunk.newStart - 1);
  if (startIdx + expectedNew.length > fileLines.length) {
    return null;
  }
  for (let i = 0; i < expectedNew.length; i += 1) {
    if (fileLines[startIdx + i] !== expectedNew[i]) {
      return null;
    }
  }

  const touchesEnd = startIdx + expectedNew.length === fileLines.length;
  if (touchesEnd && newNoNewline && hasTrailingEol) {
    return null;
  }

  const nextLines = [
    ...fileLines.slice(0, startIdx),
    ...restoredOld,
    ...fileLines.slice(startIdx + expectedNew.length),
  ];
  if (nextLines.length === 0) {
    return "";
  }

  let nextHasTrailingEol = hasTrailingEol;
  if (touchesEnd) {
    if (oldNoNewline) {
      nextHasTrailingEol = false;
    } else if (newNoNewline) {
      nextHasTrailingEol = true;
    } else if (fileLines.length === 0) {
      nextHasTrailingEol = true;
    }
  }
  return nextLines.join(eol) + (nextHasTrailingEol ? eol : "");
}

interface SplitDiffRow {
  leftNo: string;
  leftText: string;
  leftKind: "del" | "ctx" | "hunk" | "empty";
  rightNo: string;
  rightText: string;
  rightKind: "add" | "ctx" | "hunk" | "empty";
}

const IMAGE_EXT_RE = /\.(png|jpe?g|gif|webp|bmp|ico)$/i;

function splitFilePath(full: string): { name: string; dir: string } {
  const norm = full.replace(/\\/g, "/");
  const idx = norm.lastIndexOf("/");
  if (idx < 0) return { name: norm, dir: "" };
  return { name: norm.slice(idx + 1), dir: norm.slice(0, idx) };
}

function parseUnifiedDiffToSplit(diffText: string): SplitDiffRow[] {
  const lines = diffText.split("\n");
  const rows: SplitDiffRow[] = [];
  let leftLine = 0;
  let rightLine = 0;
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (
      line.startsWith("diff --git") ||
      line.startsWith("index ") ||
      line.startsWith("--- ") ||
      line.startsWith("+++ ")
    ) {
      i += 1;
      continue;
    }
    const hunk = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
    if (hunk) {
      leftLine = Number(hunk[1]);
      rightLine = Number(hunk[2]);
      rows.push({
        leftNo: "@@",
        leftText: line,
        leftKind: "hunk",
        rightNo: "@@",
        rightText: line,
        rightKind: "hunk",
      });
      i += 1;
      continue;
    }
    if (line.startsWith("-") || line.startsWith("+")) {
      const dels: Array<{ no: number; text: string }> = [];
      const adds: Array<{ no: number; text: string }> = [];
      while (i < lines.length && (lines[i].startsWith("-") || lines[i].startsWith("+"))) {
        if (lines[i].startsWith("-")) {
          dels.push({ no: leftLine++, text: lines[i].slice(1) });
        } else {
          adds.push({ no: rightLine++, text: lines[i].slice(1) });
        }
        i += 1;
      }
      const maxLen = Math.max(dels.length, adds.length);
      for (let k = 0; k < maxLen; k += 1) {
        const d = dels[k];
        const a = adds[k];
        rows.push({
          leftNo: d ? String(d.no) : "",
          leftText: d ? d.text : "",
          leftKind: d ? "del" : "empty",
          rightNo: a ? String(a.no) : "",
          rightText: a ? a.text : "",
          rightKind: a ? "add" : "empty",
        });
      }
      continue;
    }
    if (line.startsWith(" ") || line === "") {
      const content = line.startsWith(" ") ? line.slice(1) : "";
      rows.push({
        leftNo: leftLine > 0 ? String(leftLine++) : "",
        leftText: content,
        leftKind: "ctx",
        rightNo: rightLine > 0 ? String(rightLine++) : "",
        rightText: content,
        rightKind: "ctx",
      });
    }
    i += 1;
  }
  return rows;
}

export function ContextDrawer(): React.ReactElement | null {
  const open = useDrawerStore((s) => s.open);
  const mode = useDrawerStore((s) => s.mode);
  const width = useDrawerStore((s) => s.width);
  const projectDir = useDrawerStore((s) => s.projectDir);
  const worktreePath = useDrawerStore((s) => s.worktreePath);
  const gitStatus = useDrawerStore((s) => s.gitStatus);
  const touchedFiles = useDrawerStore((s) => s.touchedFiles);
  const activeFile = useDrawerStore((s) => s.activeFile);
  const fileTab = useDrawerStore((s) => s.fileTab);
  const highlightLine = useDrawerStore((s) => s.highlightLine);
  const loadingFile = useDrawerStore((s) => s.loadingFile);
  const preview = useDrawerStore((s) => s.preview);
  const diff = useDrawerStore((s) => s.diff);
  const editContent = useDrawerStore((s) => s.editContent);
  const editDirty = useDrawerStore((s) => s.editDirty);
  const saving = useDrawerStore((s) => s.saving);
  const conflictMessage = useDrawerStore((s) => s.conflictMessage);
  const conflictExists = useDrawerStore((s) => s.conflictExists);
  const error = useDrawerStore((s) => s.error);

  const openTools = useAppStore((s) => s.openTools);
  const generalMode = useAppStore((s) => s.generalMode);
  const activeProcessId = useSessionStore((s) => s.activeProcessId);
  const liveSubagents = useChatStore((s) => s.liveSubagents);
  const liveTools = useChatStore((s) => s.liveTools);

  const [fileListScope, setFileListScope] = useState<"changed" | "all">("changed");
  const [allFiles, setAllFiles] = useState<SnapshotFileEntry[]>([]);
  const [loadingTree, setLoadingTree] = useState(false);
  const [fileFilter, setFileFilter] = useState("");
  const [diffViewMode, setDiffViewMode] = useState<"unified" | "split">("unified");
  const [imagePreviewSrc, setImagePreviewSrc] = useState<string | null>(null);
  const [selectedLines, setSelectedLines] = useState<{ start: number; end: number } | null>(null);
  const [selectedEditorId, setSelectedEditorId] = useState<string>("");

  const [filesExpanded, setFilesExpanded] = useState(true);
  const [terminals, setTerminals] = useState<TerminalProcessInfo[]>([]);
  const [expandedTerminalId, setExpandedTerminalId] = useState<string | null>(null);
  const [expandedSubagentId, setExpandedSubagentId] = useState<string | null>(null);

  const [worktrees, setWorktrees] = useState<WorktreeItem[]>([]);
  const [worktreeBusy, setWorktreeBusy] = useState(false);
  const [gateResults, setGateResults] = useState<Record<string, GateStepResult[]>>({});
  const [postMergeGate, setPostMergeGate] = useState<GateStepResult[] | null>(null);
  const [manualPath, setManualPath] = useState("");
  const codeViewRef = useRef<HTMLDivElement>(null);

  const availableEditors = useMemo(
    () => openTools.filter((t) => t.available !== false),
    [openTools]
  );

  useEffect(() => {
    if (!selectedEditorId && availableEditors[0]?.id) {
      setSelectedEditorId(availableEditors[0].id);
    }
  }, [availableEditors, selectedEditorId]);

  const loadFilesSnapshot = () => {
    if (!projectDir || generalMode) return;
    setLoadingTree(true);
    invoke<{ files?: SnapshotFileEntry[] }>("files_snapshot", { projectDir, worktreePath })
      .then((res) => setAllFiles(Array.isArray(res?.files) ? res.files : []))
      .catch(() => setAllFiles([]))
      .finally(() => setLoadingTree(false));
  };

  useEffect(() => {
    if (open && mode === "files" && fileListScope === "all" && projectDir && !generalMode) {
      loadFilesSnapshot();
    }
  }, [open, mode, fileListScope, projectDir, worktreePath, generalMode]);

  useEffect(() => {
    setSelectedLines(null);
    setImagePreviewSrc(null);
    if (!open || !projectDir || !activeFile || !IMAGE_EXT_RE.test(activeFile)) return;
    let cancelled = false;
    invoke<{ png?: string }>("file_image", { projectDir, path: activeFile, worktreePath })
      .then((res) => {
        if (!cancelled && res?.png) {
          setImagePreviewSrc(`data:image/png;base64,${res.png}`);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [open, projectDir, worktreePath, activeFile]);

  const loadWorktrees = () => {
    if (!projectDir || generalMode) return;
    invoke<WorktreeItem[]>("worktree_list", { projectDir })
      .then((list) => setWorktrees(Array.isArray(list) ? list : []))
      .catch(() => setWorktrees([]));
  };

  useEffect(() => {
    if (!open || mode !== "worktrees" || !projectDir || generalMode) return;
    loadWorktrees();
  }, [open, mode, projectDir, generalMode]);

  useEffect(() => {
    if (!open || mode !== "activity" || !projectDir || !activeProcessId) return;
    const loadTerminals = () => {
      invoke<{ processes?: TerminalProcessInfo[] } | TerminalProcessInfo[]>("terminal_monitor", {
        projectDir,
        processId: activeProcessId,
        action: "list",
      })
        .then((res) => {
          const list = Array.isArray(res) ? res : res?.processes ?? [];
          setTerminals(list);
        })
        .catch(() => {});
    };
    loadTerminals();
    const timer = window.setInterval(loadTerminals, 3000);
    return () => window.clearInterval(timer);
  }, [open, mode, projectDir, activeProcessId]);

  useEffect(() => {
    if (!open || mode !== "files" || fileTab !== "preview" || loadingFile || !highlightLine) return;
    const target = codeViewRef.current?.querySelector(".code-line.highlight");
    target?.scrollIntoView({ block: "center" });
  }, [open, mode, fileTab, highlightLine, loadingFile]);

  const splitDiffRows = useMemo(
    () => (diff?.diff ? parseUnifiedDiffToSplit(diff.diff) : []),
    [diff?.diff]
  );
  const parsedHunks = useMemo(
    () => (diff?.diff ? parseUnifiedDiffHunks(diff.diff) : { metaLines: [], hunks: [] }),
    [diff?.diff]
  );
  const [revertingHunk, setRevertingHunk] = useState<number | null>(null);

  const handleRevertHunk = async (hunk: DiffHunk) => {
    if (!projectDir || !activeFile) return;
    setRevertingHunk(hunk.index);
    try {
      const latest = await invoke<{
        content?: string | null;
        hash?: string | null;
        readonly?: string | null;
        bom?: boolean;
      }>("file_preview", {
        projectDir,
        path: activeFile,
        worktreePath,
      });
      if (latest?.readonly) {
        pushToast(`该文件不可编辑：${latest.readonly}`, "warn");
        return;
      }
      if (typeof latest?.content !== "string" || !latest?.hash) {
        pushToast("无法读取最新文件内容与指纹，请刷新后重试", "error");
        return;
      }
      const nextContent = applyReverseHunk(latest.content, hunk);
      if (nextContent === null) {
        pushToast("磁盘文件相对当前 Diff 已变化，请先刷新 Diff 再回退该块", "warn");
        return;
      }
      const writeRes = await invoke<{ status?: string }>("file_write", {
        projectDir,
        path: activeFile,
        content: nextContent,
        expectedHash: latest.hash,
        bom: Boolean(latest.bom),
        evidence: false,
        worktreePath,
      });
      if (writeRes?.status === "conflict") {
        pushToast("磁盘文件在回退期间发生变化，未写入，请刷新 Diff 后重试", "warn");
        return;
      }
      if (writeRes?.status !== "saved") {
        pushToast("后端未确认回退写入，请刷新后检查", "error");
        return;
      }
      pushToast(`已回退代码块 #${hunk.index + 1}`, "ok", 2000);
      await openFileInDrawer(activeFile, { tab: "diff", projectDir, worktreePath });
      await refreshGitStatus(projectDir, worktreePath);
    } catch (err) {
      pushToast(`回退代码块失败：${formatError(err)}`, "error");
    } finally {
      setRevertingHunk(null);
    }
  };

  if (!open) return null;

  const startResize = (e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = width;
    const onMove = (ev: MouseEvent) => setDrawerWidth(startW + (startX - ev.clientX));
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  const gitFiles = gitStatus?.files ?? [];
  const mergedChangedFiles = Array.from(new Set([...gitFiles.map((f) => f.path), ...touchedFiles]));
  const filterText = fileFilter.trim().toLowerCase();
  const visibleFiles =
    fileListScope === "all"
      ? allFiles
          .map((f) => f.path)
          .filter((p) => !filterText || p.toLowerCase().includes(filterText))
          .slice(0, 300)
      : mergedChangedFiles.filter((p) => !filterText || p.toLowerCase().includes(filterText));

  const stopTerminal = async (id: string) => {
    if (!projectDir) return;
    try {
      await invoke("run_tool_process_stop", { projectDir, processId: id });
      setTerminals((prev) => prev.filter((t) => t.id !== id));
      pushToast("已停止后台终端进程", "ok");
    } catch (err) {
      pushToast(`停止终端失败：${formatError(err)}`, "error");
    }
  };

  const toggleWatchTerminal = async (term: TerminalProcessInfo) => {
    if (!projectDir || !activeProcessId) return;
    try {
      await invoke("terminal_monitor", {
        projectDir,
        processId: activeProcessId,
        action: term.subscribed ? "unwatch" : "watch",
        id: term.id,
      });
      setTerminals((prev) =>
        prev.map((t) => (t.id === term.id ? { ...t, subscribed: !term.subscribed } : t))
      );
      pushToast(term.subscribed ? "已取消终端事件监听" : "已开启终端输出监听", "ok");
    } catch (err) {
      pushToast(`切换监控失败：${formatError(err)}`, "error");
    }
  };

  const stopSubagent = async (taskId: string) => {
    const proc = getActiveProcess();
    if (!projectDir || !proc) return;
    try {
      await invoke("stop_task", { projectDir, processId: proc.id, taskId });
      pushToast("已发送子代理取消信号", "ok");
    } catch (err) {
      pushToast(`取消子代理失败：${formatError(err)}`, "error");
    }
  };

  const handleLineClick = (lineNo: number, shiftKey: boolean) => {
    setSelectedLines((prev) => {
      if (shiftKey && prev) {
        return {
          start: Math.min(prev.start, lineNo),
          end: Math.max(prev.start, lineNo),
        };
      }
      if (prev && prev.start === lineNo && prev.end === lineNo) return null;
      return { start: lineNo, end: lineNo };
    });
  };

  const quoteSelectedLines = async (withComment: boolean) => {
    if (!activeFile || !selectedLines) return;
    const all = (preview?.content ?? "").split("\n");
    const snippet = all.slice(selectedLines.start - 1, selectedLines.end).join("\n");
    const rangeTag =
      selectedLines.start === selectedLines.end
        ? `${activeFile}:${selectedLines.start}`
        : `${activeFile}:${selectedLines.start}-${selectedLines.end}`;
    let comment = "";
    if (withComment) {
      const entered = await promptDialog({
        title: `对 ${rangeTag} 添加批注`,
        label: "批注意见将连同引用代码块一起插入对话输入框",
        placeholder: "例如：这里缺少边界空值判断，请重构一下",
        confirmLabel: "插入输入框",
      });
      if (entered === null) return;
      comment = entered.trim();
    }
    const block = [
      `参考 \`${rangeTag}\`：`,
      "```",
      snippet.slice(0, 2000),
      "```",
      comment,
    ]
      .filter(Boolean)
      .join("\n");
    await appendQuoteToComposer(block);
    pushToast(`已将 ${rangeTag} 引用至输入框`, "ok", 1800);
    setSelectedLines(null);
  };

  const handleCreateWorktree = async () => {
    if (!projectDir) return;
    const name = await promptDialog({
      title: "新建并行 Git 工作树",
      label: "分支/工作树名称（字母、数字、连字符）",
      placeholder: "例如：feat-refactor",
      confirmLabel: "创建",
    });
    if (!name) return;
    setWorktreeBusy(true);
    try {
      await invoke("worktree_create", { projectDir, name });
      pushToast(`已创建工作树 ${name}`, "ok");
      loadWorktrees();
    } catch (err) {
      pushToast(`创建工作树失败：${formatError(err)}`, "error");
    } finally {
      setWorktreeBusy(false);
    }
  };

  const handleRunWorktreeGate = async (wt: WorktreeItem) => {
    if (!projectDir) return;
    setWorktreeBusy(true);
    try {
      const steps = await invoke<GateStepResult[]>("worktree_gate", {
        projectDir,
        worktreePath: wt.path,
      });
      const list = Array.isArray(steps) ? steps : [];
      setGateResults((prev) => ({ ...prev, [wt.path]: list }));
      const failed = list.filter((s) => !s.ok).length;
      if (list.length === 0) {
        pushToast(`工作树 ${wt.branch} 验证未执行：当前未检测到适用门禁检查（0 项）`, "info");
      } else {
        pushToast(
          failed === 0
            ? `工作树 ${wt.branch} 门禁全部通过 (${list.length} 项)`
            : `工作树 ${wt.branch} 有 ${failed}/${list.length} 项门禁未通过`,
          failed === 0 ? "ok" : "warn"
        );
      }
    } catch (err) {
      pushToast(`运行工作树门禁失败：${formatError(err)}`, "error");
    } finally {
      setWorktreeBusy(false);
    }
  };

  const handlePostMergeGate = async () => {
    if (!projectDir) return;
    setWorktreeBusy(true);
    try {
      const steps = await invoke<GateStepResult[]>("worktree_post_merge_gate", { projectDir });
      const list = Array.isArray(steps) ? steps : [];
      setPostMergeGate(list);
      const failed = list.filter((s) => !s.ok).length;
      if (list.length === 0) {
        pushToast("主分支合并后验证未执行：当前未检测到适用门禁检查（0 项）", "info");
      } else {
        pushToast(
          failed === 0
            ? `主分支合并后全量门禁通过 (${list.length} 项)`
            : `合并后门禁有 ${failed}/${list.length} 项失败`,
          failed === 0 ? "ok" : "error"
        );
      }
    } catch (err) {
      pushToast(`运行合并后门禁失败：${formatError(err)}`, "error");
    } finally {
      setWorktreeBusy(false);
    }
  };

  const handleMergeWorktree = async (wt: WorktreeItem) => {
    if (!projectDir) return;
    setWorktreeBusy(true);
    try {
      const conflicts = await invoke<string[]>("worktree_merge_preview", {
        projectDir,
        worktreePath: wt.path,
      });
      const msg =
        conflicts && conflicts.length > 0
          ? `合并预检发现以下冲突文件：\n${conflicts.join("\n")}\n\n确认仍尝试合并分支 ${wt.branch} 吗？`
          : `合并预检无冲突。确认将分支 ${wt.branch} 合并回主分支？`;
      const ok = await confirmDialog({
        title: `合并工作树 ${wt.branch}`,
        message: msg,
        confirmLabel: "确认合并",
      });
      if (!ok) return;
      const result = await invoke<string>("worktree_merge", {
        projectDir,
        worktreePath: wt.path,
      });
      pushToast(result || `已合并 ${wt.branch}`, "ok");
      loadWorktrees();
      void refreshGitStatus(projectDir, worktreePath);
    } catch (err) {
      pushToast(`合并失败：${formatError(err)}`, "error");
    } finally {
      setWorktreeBusy(false);
    }
  };

  const handleDiscardWorktree = async (wt: WorktreeItem) => {
    if (!projectDir) return;
    const ok = await confirmDialog({
      title: `放弃工作树 ${wt.branch}？`,
      message: `将移除工作树 ${wt.path} 并删除其分支，未合并的改动会丢失。`,
      confirmLabel: "放弃并删除",
      danger: true,
    });
    if (!ok) return;
    setWorktreeBusy(true);
    try {
      await invoke("worktree_discard", { projectDir, worktreePath: wt.path });
      pushToast(`已移除工作树 ${wt.branch}`, "ok");
      loadWorktrees();
    } catch (err) {
      pushToast(`移除工作树失败：${formatError(err)}`, "error");
    } finally {
      setWorktreeBusy(false);
    }
  };

  const activeEditor =
    availableEditors.find((t) => t.id === selectedEditorId) ?? availableEditors[0];

  return (
    <aside className="side-drawer" style={{ width }} aria-label="右侧伴生抽屉">
      <div className="side-drawer-resizer" onMouseDown={startResize} aria-hidden="true" />

      <div className="drawer-header">
        <div className="drawer-main-tabs" aria-label="抽屉面板切换">
          <button
            type="button"
            aria-pressed={mode === "files"}
            className={`drawer-main-tab${mode === "files" ? " active" : ""}`}
            onClick={() => drawerStore.setState({ mode: "files" })}
          >
            <span>文件</span>
            <span className="drawer-tab-count">{mergedChangedFiles.length}</span>
          </button>
          <button
            type="button"
            aria-pressed={mode === "activity"}
            className={`drawer-main-tab${mode === "activity" ? " active" : ""}`}
            onClick={() => drawerStore.setState({ mode: "activity" })}
          >
            <span>活动</span>
            <span className="drawer-tab-count">{liveSubagents.length + terminals.length}</span>
          </button>
          {!generalMode && (
            <button
              type="button"
              aria-pressed={mode === "worktrees"}
              className={`drawer-main-tab${mode === "worktrees" ? " active" : ""}`}
              onClick={() => drawerStore.setState({ mode: "worktrees" })}
            >
              <span>工作树</span>
              <span className="drawer-tab-count">{worktrees.length}</span>
            </button>
          )}
        </div>
        <button
          type="button"
          className="btn btn-ghost btn-icon btn-sm"
          onClick={closeDrawer}
          aria-label="关闭抽屉"
          title="关闭抽屉"
        >
          <IconClose size={13} />
        </button>
      </div>

      {mode === "files" ? (
        <div className="drawer-body">
          <div className="drawer-path-bar">
            <input
              className="input mono"
              placeholder="输入相对路径回车打开，或筛选下方文件…"
              value={manualPath}
              onChange={(e) => {
                setManualPath(e.target.value);
                setFileFilter(e.target.value);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" && manualPath.trim()) {
                  void openFileInDrawer(manualPath.trim());
                }
              }}
            />
            <button
              type="button"
              className="btn btn-icon"
              title="刷新文件与 Git 状态"
              aria-label="刷新文件状态"
              onClick={() => {
                if (projectDir) {
                  void refreshGitStatus(projectDir, worktreePath);
                  if (fileListScope === "all") loadFilesSnapshot();
                }
              }}
            >
              <IconRefresh size={13} />
            </button>
          </div>

          <div className="drawer-file-summary">
            <div className="segmented" aria-label="文件列表视图">
              <button
                type="button"
                className={`segmented-btn${fileListScope === "changed" ? " active" : ""}`}
                onClick={() => setFileListScope("changed")}
              >
                变更 ({mergedChangedFiles.length})
              </button>
              {!generalMode && (
                <button
                  type="button"
                  className={`segmented-btn${fileListScope === "all" ? " active" : ""}`}
                  onClick={() => setFileListScope("all")}
                >
                  全部文件{allFiles.length > 0 ? ` (${allFiles.length})` : ""}
                </button>
              )}
            </div>
            {gitStatus && (gitStatus.additions > 0 || gitStatus.deletions > 0) && (
              <span className="mono">
                <span style={{ color: "var(--diff-add)" }}>+{gitStatus.additions}</span>{" "}
                <span style={{ color: "var(--diff-del)" }}>-{gitStatus.deletions}</span>
              </span>
            )}
            <button type="button" className="btn btn-xs btn-ghost" aria-expanded={filesExpanded}
              onClick={() => setFilesExpanded(!filesExpanded)}>{filesExpanded ? "收起列表" : "展开列表"}</button>
          </div>

          <div className="drawer-file-list" hidden={!filesExpanded}>
            {fileListScope === "all" && loadingTree ? (
              <div className="field-hint" style={{ padding: 10 }}>
                正在扫描项目文件树…
              </div>
            ) : visibleFiles.length === 0 ? (
              <div className="field-hint" style={{ padding: 10 }}>
                {fileListScope === "all"
                  ? "没有匹配的项目文件。"
                  : "本轮暂无触及或未提交变动的文件。切换到「全部文件」浏览整个仓库，或点击对话中的文件路径。"}
              </div>
            ) : (
              visibleFiles.map((file) => {
                const stat = gitFiles.find((f) => f.path === file);
                const { name, dir } = splitFilePath(file);
                return (
                  <button
                    key={file}
                    type="button"
                    className={`drawer-file-item${activeFile === file ? " active" : ""}`}
                    onClick={() => void openFileInDrawer(file)}
                    title={file}
                  >
                    <span className="drawer-file-name-wrap">
                      <span className="drawer-file-basename">{name}</span>
                      {dir && <span className="drawer-file-dir">{dir}</span>}
                    </span>
                    {stat && (
                      <span className="drawer-file-stats">
                        {stat.untracked ? (
                          <span className="badge badge-ok">新增</span>
                        ) : (
                          <>
                            <span style={{ color: "var(--diff-add)" }}>+{stat.additions}</span>
                            <span style={{ color: "var(--diff-del)" }}>-{stat.deletions}</span>
                          </>
                        )}
                      </span>
                    )}
                  </button>
                );
              })
            )}
          </div>

          {activeFile ? (
            <>
              <div className="drawer-subbar">
                <div className="segmented" aria-label="文件查看模式">
                  <button
                    type="button"
                    aria-pressed={fileTab === "preview"}
                    className={`segmented-btn${fileTab === "preview" ? " active" : ""}`}
                    onClick={() => setDrawerFileTab("preview")}
                  >
                    原文
                  </button>
                  <button
                    type="button"
                    aria-pressed={fileTab === "diff"}
                    className={`segmented-btn${fileTab === "diff" ? " active" : ""}`}
                    onClick={() => setDrawerFileTab("diff")}
                  >
                    Diff
                  </button>
                  <button
                    type="button"
                    aria-pressed={fileTab === "edit"}
                    className={`segmented-btn${fileTab === "edit" ? " active" : ""}`}
                    disabled={Boolean(preview?.readonly || preview?.binary)}
                    onClick={() => setDrawerFileTab("edit")}
                  >
                    轻编辑{editDirty ? " •" : ""}
                  </button>
                </div>

                <div className="drawer-subbar-actions">
                  {fileTab === "diff" && diff?.diff && (
                    <div className="segmented" aria-label="Diff 展示模式">
                      <button
                        type="button"
                        className={`segmented-btn${diffViewMode === "unified" ? " active" : ""}`}
                        onClick={() => setDiffViewMode("unified")}
                      >
                        统一
                      </button>
                      <button
                        type="button"
                        className={`segmented-btn${diffViewMode === "split" ? " active" : ""}`}
                        onClick={() => setDiffViewMode("split")}
                      >
                        并排
                      </button>
                    </div>
                  )}
                  {fileTab === "edit" && (
                    <button
                      type="button"
                      className="btn btn-accent btn-sm"
                      disabled={saving || !editDirty}
                      onClick={() => void saveDrawerFile(false)}
                    >
                      {saving ? "保存中…" : "保存"}
                    </button>
                  )}
                  {availableEditors.length > 1 && (
                    <select
                      className="composer-select"
                      aria-label="选择外部打开方式"
                      value={selectedEditorId}
                      onChange={(e) => setSelectedEditorId(e.target.value)}
                      style={{ maxWidth: 100 }}
                    >
                      {availableEditors.map((tool) => (
                        <option key={tool.id} value={tool.id}>
                          {tool.label}
                        </option>
                      ))}
                    </select>
                  )}
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    onClick={() =>
                      void openExternalPath(activeFile, activeEditor?.id, {
                        processId: activeProcessId,
                      })
                    }
                    title={activeEditor ? `用 ${activeEditor.label} 打开` : "在资源管理器显示"}
                  >
                    <IconExternal size={12} />
                    <span>外部打开</span>
                  </button>
                </div>
              </div>

              {fileTab === "preview" && selectedLines && (
                <div className="drawer-selection-bar">
                  <span className="mono">
                    已选 L{selectedLines.start}
                    {selectedLines.end !== selectedLines.start ? `–L${selectedLines.end}` : ""}（Shift+点击选范围）
                  </span>
                  <div style={{ display: "flex", gap: 4 }}>
                    <button
                      type="button"
                      className="btn btn-sm"
                      onClick={() => void quoteSelectedLines(false)}
                    >
                      引用到对话
                    </button>
                    <button
                      type="button"
                      className="btn btn-sm btn-accent"
                      onClick={() => void quoteSelectedLines(true)}
                    >
                      批注并引用
                    </button>
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      onClick={() => setSelectedLines(null)}
                    >
                      清除
                    </button>
                  </div>
                </div>
              )}

              {conflictMessage && (
                <div className="drawer-conflict-bar">
                  <span>{conflictMessage}</span>
                  <div style={{ display: "flex", gap: 4, flex: "none" }}>
                    <button
                      type="button"
                      className="btn btn-sm"
                      onClick={() => void openFileInDrawer(activeFile, { tab: "edit" })}
                    >
                      重新加载
                    </button>
                    <button
                      type="button"
                      className="btn btn-danger btn-sm"
                      onClick={() => void saveDrawerFile(true)}
                    >
                      {conflictExists ? "强制覆盖" : "重新创建"}
                    </button>
                  </div>
                </div>
              )}

              {loadingFile ? (
                <div className="empty-state">
                  <p>正在读取 {activeFile}…</p>
                </div>
              ) : error ? (
                <div className="empty-state">
                  <p style={{ color: "var(--err)" }}>{error}</p>
                </div>
              ) : fileTab === "edit" ? (
                <textarea
                  className="drawer-editor"
                  value={editContent}
                  onChange={(e) => setDrawerEditContent(e.target.value)}
                  spellCheck={false}
                />
              ) : fileTab === "diff" ? (
                <div className="drawer-code-view">
                  {diff?.diff ? (
                    diffViewMode === "split" ? (
                      <div className="split-diff-table">
                        {splitDiffRows.map((row, idx) => (
                          <div key={idx} className="split-diff-row">
                            <div className={`split-diff-cell split-${row.leftKind}`}>
                              <span className="code-line-no">{row.leftNo}</span>
                              <span>{row.leftText || " "}</span>
                            </div>
                            <div className={`split-diff-cell split-${row.rightKind}`}>
                              <span className="code-line-no">{row.rightNo}</span>
                              <span>{row.rightText || " "}</span>
                            </div>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="drawer-code-inner">
                        {parsedHunks.metaLines.map((line, i) => (
                          <div key={`meta-${i}`} className="diff-line">
                            {line || " "}
                          </div>
                        ))}
                        {parsedHunks.hunks.map((hunk) => (
                          <div key={`hunk-${hunk.index}`} className="diff-hunk-block">
                            <div className="diff-line diff-line-hunk diff-hunk-head">
                              <span>{hunk.header}</span>
                              {!diff.untracked && (
                                <button
                                  type="button"
                                  className="btn btn-ghost btn-sm diff-hunk-revert"
                                  disabled={revertingHunk === hunk.index}
                                  onClick={() => void handleRevertHunk(hunk)}
                                  title="仅撤销这一块改动（保留文件内其余修改）"
                                >
                                  {revertingHunk === hunk.index ? "回退中…" : "回退此块"}
                                </button>
                              )}
                            </div>
                            {hunk.lines.map((line, i) => (
                              <div
                                key={`${hunk.index}-${i}`}
                                className={`diff-line ${
                                  line.startsWith("+") && !line.startsWith("+++")
                                    ? "diff-line-add"
                                    : line.startsWith("-") && !line.startsWith("---")
                                    ? "diff-line-del"
                                    : ""
                                }`}
                              >
                                {line || " "}
                              </div>
                            ))}
                          </div>
                        ))}
                      </div>
                    )
                  ) : (
                    <div className="empty-state" style={{ padding: "28px 16px" }}>
                      <p>{diff?.message ?? "该文件相对 HEAD 暂无文本差异"}</p>
                    </div>
                  )}
                </div>
              ) : (
                <div ref={codeViewRef} className="drawer-code-view">
                  {imagePreviewSrc ? (
                    <div className="empty-state" style={{ padding: 16 }}>
                      <img
                        src={imagePreviewSrc}
                        alt={activeFile}
                        style={{ maxWidth: "100%", borderRadius: 8, cursor: "zoom-in" }}
                        onClick={() =>
                          openViewer({
                            title: activeFile,
                            content: "",
                            kind: "image",
                            imageSrc: imagePreviewSrc,
                          })
                        }
                      />
                      <span className="field-hint">点击图片查看大图</span>
                    </div>
                  ) : preview?.binary ? (
                    <div className="empty-state">
                      <p>二进制文件不支持文本预览，请使用外部程序打开。</p>
                    </div>
                  ) : (
                    <div className="drawer-code-inner">
                      {(preview?.content ?? "").split("\n").map((line, idx) => {
                        const no = idx + 1;
                        const inSelection =
                          selectedLines !== null &&
                          no >= selectedLines.start &&
                          no <= selectedLines.end;
                        return (
                          <div
                            key={no}
                            className={`code-line${highlightLine === no ? " highlight" : ""}${
                              inSelection ? " selected-range" : ""
                            }`}
                            onClick={(e) => handleLineClick(no, e.shiftKey)}
                            title="点击选中该行（Shift+点击选区间），可批注并引用到对话"
                          >
                            <span className="code-line-no">{no}</span>
                            <span>{line || " "}</span>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              )}
            </>
          ) : (
            <div className="empty-state" style={{ flex: 1 }}>
              <div className="empty-state-icon">
                <IconFileCode size={18} />
              </div>
              <p>选择上方文件以查看源码、Diff（支持统一/并排对照与行级批注）或进行轻量修改。</p>
            </div>
          )}
        </div>
      ) : mode === "worktrees" ? (
        <div className="drawer-scroll-body">
          <div className="drawer-section-head">
            <span className="field-label">并行 Git 工作树 ({worktrees.length})</span>
            <div style={{ display: "flex", gap: 6 }}>
              <button
                type="button"
                className="btn btn-sm"
                title="在主分支运行合并后全量验证门禁"
                disabled={worktreeBusy}
                onClick={() => void handlePostMergeGate()}
              >
                合并后门禁
              </button>
              <button
                type="button"
                className="btn btn-icon btn-sm"
                title="刷新工作树列表"
                onClick={loadWorktrees}
                disabled={worktreeBusy}
              >
                <IconRefresh size={12} />
              </button>
              <button
                type="button"
                className="btn btn-sm btn-primary"
                onClick={() => void handleCreateWorktree()}
                disabled={worktreeBusy}
              >
                <IconPlus size={12} />
                <span>新建</span>
              </button>
            </div>
          </div>

          {postMergeGate && (
            <div className="drawer-card">
              <div className="drawer-card-head">
                <strong style={{ fontSize: 12 }}>主分支合并后门禁结果</strong>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => setPostMergeGate(null)}
                >
                  清除
                </button>
              </div>
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                {postMergeGate.map((step) => (
                  <button
                    key={step.name}
                    type="button"
                    className={`chip-btn ${step.ok ? "badge-ok" : "badge-err"}`}
                    style={{ border: 0, cursor: "pointer" }}
                    onClick={() =>
                      openViewer({
                        title: `合并后门禁：${step.name}`,
                        subtitle: step.ok ? "通过" : "失败",
                        content: step.summary || (step.ok ? "无输出（通过）" : "失败"),
                        kind: "text",
                      })
                    }
                  >
                    {step.ok ? "✓" : "✗"} {step.name}
                  </button>
                ))}
              </div>
            </div>
          )}

          {worktrees.length === 0 ? (
            <div className="empty-state">
              <p>当前项目没有独立的并行工作树。新建工作树可在隔离分支上并行开发、运行门禁并预览合并。</p>
            </div>
          ) : (
            worktrees.map((wt) => {
              const steps = gateResults[wt.path];
              return (
                <div key={wt.path} className="drawer-card">
                  <div className="drawer-card-head">
                    <strong className="mono" style={{ fontSize: 12.5 }}>
                      {wt.branch}
                    </strong>
                    <span className={`badge ${wt.clean ? "badge-ok" : "badge-warn"}`}>
                      {wt.clean ? "干净" : `${wt.files.length} 改动`}
                    </span>
                  </div>
                  <div className="project-card-path">{wt.path}</div>
                  {steps && steps.length > 0 && (
                    <div style={{ display: "flex", gap: 5, flexWrap: "wrap" }}>
                      {steps.map((step) => (
                        <button
                          key={step.name}
                          type="button"
                          className={`chip-btn ${step.ok ? "badge-ok" : "badge-err"}`}
                          style={{ border: 0, cursor: "pointer" }}
                          onClick={() =>
                            openViewer({
                              title: `门禁 ${wt.branch} · ${step.name}`,
                              subtitle: step.ok ? "通过" : "失败",
                              content: step.summary || (step.ok ? "通过" : "失败"),
                              kind: "text",
                            })
                          }
                        >
                          {step.ok ? "✓" : "✗"} {step.name}
                        </button>
                      ))}
                    </div>
                  )}
                  <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                    <button
                      type="button"
                      className="btn btn-sm"
                      onClick={() =>
                        openViewer({
                          title: `工作树差异：${wt.branch}`,
                          subtitle: wt.path,
                          content: wt.diff || "该工作树相对主分支暂无差异",
                          kind: "text",
                        })
                      }
                    >
                      查看差异
                    </button>
                    <button
                      type="button"
                      className="btn btn-sm"
                      disabled={worktreeBusy}
                      onClick={() => void handleRunWorktreeGate(wt)}
                    >
                      运行门禁
                    </button>
                    <button
                      type="button"
                      className="btn btn-sm btn-primary"
                      disabled={worktreeBusy}
                      onClick={() => void handleMergeWorktree(wt)}
                    >
                      预检并合并
                    </button>
                    <button
                      type="button"
                      className="btn btn-sm btn-danger"
                      disabled={worktreeBusy}
                      onClick={() => void handleDiscardWorktree(wt)}
                    >
                      放弃
                    </button>
                  </div>
                </div>
              );
            })
          )}
        </div>
      ) : (
        <div className="drawer-scroll-body">
          <div>
            <div className="drawer-section-head">
              <span className="field-label">子代理活动 ({liveSubagents.length})</span>
            </div>
            {liveSubagents.length === 0 ? (
              <div className="field-hint">当前会话没有活跃或近期的子代理任务。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {liveSubagents.map((sub) => {
                  const isExpanded = expandedSubagentId === sub.id;
                  const elapsedSec = Math.max(0, Math.round((Date.now() - sub.startedAt) / 1000));
                  return (
                    <div key={sub.id} className="drawer-card">
                      <div className="drawer-card-head">
                        <button
                          type="button"
                          className="btn btn-ghost btn-sm"
                          style={{ padding: "0 4px", fontWeight: 600 }}
                          aria-expanded={isExpanded}
                          onClick={() => setExpandedSubagentId(isExpanded ? null : sub.id)}
                        >
                          {isExpanded ? "▾" : "▸"} {sub.agent ?? sub.label}
                        </button>
                        <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                          <span className="field-hint mono" style={{ fontSize: 10.5 }}>
                            {elapsedSec}s
                          </span>
                          <span
                            className={`badge ${
                              sub.status === "running"
                                ? "badge-accent"
                                : sub.status === "error"
                                ? "badge-err"
                                : "badge-ok"
                            }`}
                          >
                            {sub.status === "running"
                              ? "运行中"
                              : sub.status === "error"
                              ? "失败"
                              : "完成"}
                          </span>
                          {sub.status === "running" && (
                            <button
                              type="button"
                              className="btn btn-danger btn-sm"
                              onClick={() => void stopSubagent(sub.id)}
                            >
                              取消
                            </button>
                          )}
                        </div>
                      </div>
                      {sub.model && (
                        <div className="field-hint mono" style={{ fontSize: 11 }}>
                          模型：{sub.model} · 轨迹 {sub.progress.length} 条
                        </div>
                      )}
                      {sub.progress.length > 0 && (
                        <pre className="tool-pre">
                          {(isExpanded ? sub.progress : sub.progress.slice(-1)).join("\n")}
                        </pre>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          <div>
            <div className="drawer-section-head">
              <span className="field-label">后台终端进程 ({terminals.length})</span>
            </div>
            {terminals.length === 0 ? (
              <div className="field-hint">暂无运行中的后台终端进程。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {terminals.map((term) => {
                  const isOpen = expandedTerminalId === term.id;
                  return (
                    <div key={term.id} className="drawer-card">
                      <div className="drawer-card-head">
                        <button
                          type="button"
                          className="btn btn-ghost btn-sm mono"
                          style={{
                            fontSize: 11.5,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            maxWidth: "56%",
                            justifyContent: "flex-start",
                          }}
                          title={term.command ?? term.id}
                          aria-expanded={isOpen}
                          onClick={() => setExpandedTerminalId(isOpen ? null : term.id)}
                        >
                          {isOpen ? "▾" : "▸"} {term.command ?? term.id}
                        </button>
                        <div style={{ display: "flex", gap: 4, alignItems: "center" }}>
                          {typeof term.exit === "number" && (
                            <span className={`badge ${term.exit === 0 ? "badge-ok" : "badge-err"}`}>
                              exit {term.exit}
                            </span>
                          )}
                          <button
                            type="button"
                            className={`btn btn-sm${term.subscribed ? " btn-accent" : ""}`}
                            onClick={() => void toggleWatchTerminal(term)}
                          >
                            {term.subscribed ? "监控中" : "监控"}
                          </button>
                          {term.running !== false && (
                            <button
                              type="button"
                              className="btn btn-danger btn-sm"
                              onClick={() => void stopTerminal(term.id)}
                            >
                              停止
                            </button>
                          )}
                        </div>
                      </div>
                      {!isOpen && term.output?.trim() && (
                        <div className="activity-tail mono">{term.output.trimEnd().split("\n").at(-1)}</div>
                      )}
                      {isOpen && (
                        <pre className="tool-pre" style={{ maxHeight: 220 }}>
                          {term.output?.trim() ? term.output : "（暂无终端输出）"}
                        </pre>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          <div>
            <div className="drawer-section-head">
              <span className="field-label">本轮工具轨迹 ({liveTools.length})</span>
            </div>
            {liveTools.length === 0 ? (
              <div className="field-hint">本轮暂无工具调用。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
                {liveTools.map((t) => (
                  <div key={t.id} className="drawer-trace-row">
                    <span className="mono">{t.name}</span>
                    <span
                      className={`badge ${!t.done ? "badge-accent" : t.ok ? "badge-ok" : "badge-err"}`}
                    >
                      {t.done ? (t.ok ? "成功" : "失败") : "运行中"}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </aside>
  );
}
