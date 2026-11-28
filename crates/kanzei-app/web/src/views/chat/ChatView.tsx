import React, { useCallback, useEffect, useMemo, useState } from "react";
import { profileDisplayLabel } from "../../configOptions";
import { formatError, invoke } from "../../ipc/client";
import type { ChatMessage } from "../../ipc/types";
import {
  createConversation,
  projectDisplayName,
  selectProject,
  useAppStore,
} from "../../store/appStore";
import {
  clearCurrentConversation,
  compactCurrentConversation,
  loadConversationMessages,
} from "../../store/chatStore";
import { ControlPopover } from "../../components/ControlPopover";
import { refreshGitStatus, toggleDrawer, useDrawerStore } from "../../store/drawerStore";
import { useSessionStore } from "../../store/sessionStore";
import { ErrorBoundary } from "../../components/ErrorBoundary";
import {
  confirmDialog,
  navigateView,
  openBtwOverlay,
  openTraceDialog,
  openViewer,
  promptDialog,
  pushToast,
  uiStore,
  useUiStore,
} from "../../store/uiStore";
import {
  IconActivity,
  IconBranch,
  IconClose,
  IconCompact,
  IconDownload,
  IconFolder,
  IconLayers,
  IconPlus,
  IconSparkles,
  IconTrash,
} from "../../components/Icons";
import { AskOverlay } from "./AskOverlay";
import { BtwOverlay } from "./BtwOverlay";
import { Composer } from "./Composer";
import { ContextDrawer } from "./ContextDrawer";
import { MessageList } from "./MessageList";

interface HistorySegmentItem {
  sequence: number;
  preview?: string;
  count?: number;
  updated_at?: number;
}

export function ChatView(): React.ReactElement {
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const projects = useAppStore((s) => s.projects);
  const projectFacts = useAppStore((s) => s.projectFacts);
  const activeProcess = useSessionStore((s) =>
    s.processes.find((p) => p.id === s.activeProcessId) ?? null
  );
  const [segmentsOpen, setSegmentsOpen] = useState(false);
  const [segments, setSegments] = useState<HistorySegmentItem[]>([]);
  const [segmentsLoading, setSegmentsLoading] = useState(false);
  const traceOpen = useUiStore((s) => s.traceDialogOpen);
  const [traceItems, setTraceItems] = useState<Array<Record<string, unknown>>>([]);
  const [traceLoading, setTraceLoading] = useState(false);
  const [traceFilter, setTraceFilter] = useState("");

  const loadTraces = useCallback(async () => {
    if (!activeProjectDir || !activeProcess) return;
    setTraceLoading(true);
    try {
      const list = await invoke<Array<Record<string, unknown>>>("conversation_trace_get", {
        projectDir: activeProjectDir,
        processId: activeProcess.id,
      });
      setTraceItems(Array.isArray(list) ? list : []);
    } catch (err) {
      pushToast(`读取执行轨迹失败：${formatError(err)}`, "error");
      setTraceItems([]);
    } finally {
      setTraceLoading(false);
    }
  }, [activeProjectDir, activeProcess]);

  useEffect(() => {
    if (traceOpen) {
      void loadTraces();
    }
  }, [traceOpen, loadTraces]);

  const flatTraceRows = useMemo(() => {
    const q = traceFilter.trim().toLowerCase();
    const rows: Array<{ key: string; kind: string; title: string; detail: string; raw: unknown }> = [];
    traceItems.forEach((entry, idx) => {
      const events = Array.isArray(entry.events)
        ? (entry.events as Array<Record<string, unknown>>)
        : null;
      if (events) {
        const runId = typeof entry.run_id === "string" ? entry.run_id : `batch-${idx}`;
        events.forEach((ev, evIdx) => {
          const kind = String(ev.type ?? ev.kind ?? ev.event ?? "trace");
          const name = String(ev.name ?? ev.stage ?? ev.tool ?? "");
          const dur =
            typeof ev.duration_ms === "number"
              ? `${ev.duration_ms}ms`
              : typeof ev.durationMs === "number"
              ? `${ev.durationMs}ms`
              : "";
          const title = [kind, name, dur].filter(Boolean).join(" · ");
          const detail = String(ev.summary ?? ev.detail ?? ev.preview ?? ev.outcome ?? "");
          rows.push({ key: `${runId}-${evIdx}`, kind, title, detail, raw: ev });
        });
      } else {
        const kind = String(entry.type ?? entry.kind ?? "timing");
        const title = [
          kind,
          typeof entry.model === "string" ? entry.model : "",
          typeof entry.duration_ms === "number" ? `${entry.duration_ms}ms` : "",
        ]
          .filter(Boolean)
          .join(" · ");
        const detail =
          typeof entry.outcome === "string"
            ? `结果：${entry.outcome}`
            : JSON.stringify(entry).slice(0, 140);
        rows.push({ key: `item-${idx}`, kind, title, detail, raw: entry });
      }
    });
    if (!q) return rows;
    return rows.filter(
      (r) => r.title.toLowerCase().includes(q) || r.detail.toLowerCase().includes(q)
    );
  }, [traceItems, traceFilter]);

  if (!activeProjectDir) {
    return (
      <div className="empty-state" style={{ flex: 1 }}>
        <div className="empty-state-icon">
          <IconFolder size={20} />
        </div>
        <h3>尚未选择项目</h3>
        <p>前往工作台添加或新建项目，或点击左侧「无项目对话」直接开始。</p>
        <button type="button" className="btn btn-primary" onClick={() => navigateView("workspace")}>
          打开工作台
        </button>
      </div>
    );
  }

  const handleInitGit = async () => {
    try {
      await invoke("project_git_init", { projectDir: activeProjectDir, initialCommit: true });
      await refreshGitStatus(activeProjectDir, activeProcess?.worktree_path ?? null);
      pushToast("已初始化独立 Git 仓库", "ok");
    } catch (error) {
      pushToast(`初始化 Git 失败：${formatError(error)}`, "error");
    }
  };

  const handleLinkToProject = async (targetDir: string) => {
    if (!activeProcess || !targetDir) return;
    try {
      const created = await invoke<{ id: string }>("general_chat_link", {
        processId: activeProcess.id,
        projectDir: targetDir,
      });
      await selectProject(targetDir, created.id);
      pushToast("已将对话复制到目标项目", "ok");
    } catch (error) {
      pushToast(`关联项目失败：${formatError(error)}`, "error");
    }
  };

  const handleCompact = async () => {
    const focus = await promptDialog({
      title: "压缩当前对话上下文",
      label: "可选：填写希望在摘要中重点保留的信息（留空则全面压缩）",
      placeholder: "例如：保留 API 契约结论、已修改文件路径和未完成测试",
      confirmLabel: "开始压缩",
    });
    if (focus === null) return;
    await compactCurrentConversation(focus);
  };

  const openHistorySegments = async () => {
    if (!activeProjectDir || !activeProcess) return;
    setSegmentsOpen(true);
    setSegmentsLoading(true);
    try {
      const list = await invoke<HistorySegmentItem[]>("conversation_list", {
        projectDir: activeProjectDir,
        processId: activeProcess.id,
        limit: 50,
      });
      setSegments(Array.isArray(list) ? list : []);
    } catch (err) {
      pushToast(`读取历史段失败：${formatError(err)}`, "error");
    } finally {
      setSegmentsLoading(false);
    }
  };

  const inspectSegment = async (sequence: number) => {
    if (!activeProjectDir || !activeProcess) return;
    try {
      const page = await invoke<{ items?: Array<{ role: string; parts?: Array<{ type: string; text?: string }> }> }>(
        "conversation_page_messages",
        {
          projectDir: activeProjectDir,
          processId: activeProcess.id,
          sequence,
          limit: 200,
        }
      );
      const md = (page?.items ?? [])
        .filter((m) => m.role === "user" || m.role === "assistant")
        .map((m) => {
          const text = (m.parts ?? [])
            .filter((p) => p.type === "text" && typeof p.text === "string")
            .map((p) => p.text ?? "")
            .join("\n")
            .trim();
          return text ? `${m.role === "user" ? "### 用户" : "### 助手"}\n\n${text}` : "";
        })
        .filter(Boolean)
        .join("\n\n---\n\n");
      openViewer({
        title: `历史对话段 #${sequence}`,
        subtitle: activeProcess.label,
        content: md || "该历史段暂无文本消息。",
        kind: "markdown",
      });
    } catch (err) {
      pushToast(`读取历史段详情失败：${formatError(err)}`, "error");
    }
  };

  const deleteSegment = async (sequence: number) => {
    if (!activeProjectDir || !activeProcess) return;
    const ok = await confirmDialog({
      title: `删除历史对话段 #${sequence}？`,
      message: "删除后将永久移除该对话段记录，不可恢复。",
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await invoke("conversation_delete", {
        projectDir: activeProjectDir,
        processId: activeProcess.id,
        sequences: [sequence],
      });
      setSegments((prev) => prev.filter((s) => s.sequence !== sequence));
      await loadConversationMessages(activeProjectDir, activeProcess.id, activeProcess.session_id);
      pushToast("已删除历史对话段", "ok");
    } catch (err) {
      pushToast(`删除历史段失败：${formatError(err)}`, "error");
    }
  };

  const handleClear = async () => {
    const ok = await confirmDialog({
      title: "重开上下文？",
      message: "开始一段空白对话，不再使用当前上下文。原记录仍可在「历史段」查看。",
      confirmLabel: "开启新段",
    });
    if (ok) await clearCurrentConversation();
  };

  const handleExport = async () => {
    if (!activeProjectDir || !activeProcess) return;
    let msgs: ChatMessage[];
    try {
      msgs = await invoke<ChatMessage[]>("conversation_display_get", {
        projectDir: activeProjectDir,
        processId: activeProcess.id,
      });
    } catch (err) {
      pushToast(`读取对话失败：${formatError(err)}`, "error");
      return;
    }
    const transcript = msgs
      .filter((m) => m.role === "user" || m.role === "assistant")
      .map((m) => {
        const header = m.role === "user" ? "## 用户" : "## 助手";
        const body = (m.parts ?? [])
          .filter((p) => p.type === "text" && typeof p.text === "string")
          .map((p) => p.text ?? "")
          .join("\n")
          .trim();
        return body ? `${header}\n\n${body}` : "";
      })
      .filter(Boolean)
      .join("\n\n---\n\n");
    if (!transcript) {
      pushToast("当前对话暂无可导出的文本", "info");
      return;
    }
    const withSummary = await confirmDialog({
      title: "导出对话为 Markdown",
      message: "是否先调用快速模型在文件顶部生成简明纪要？选择「取消」则直接导出原始对话。",
      confirmLabel: "生成纪要并导出",
      cancelLabel: "直接导出原文",
    });
    let content = `# ${activeProcess?.label ?? projectDisplayName(activeProjectDir)}\n\n${transcript}\n`;
    if (withSummary) {
      try {
        const res = await invoke<{ summary?: string }>("summarize_chat", {
          projectDir: activeProjectDir,
          transcript,
        });
        if (res?.summary) {
          content = `# ${activeProcess?.label ?? projectDisplayName(activeProjectDir)}\n\n## 纪要\n\n${res.summary}\n\n---\n\n${transcript}\n`;
        }
      } catch (err) {
        pushToast(`生成纪要失败，将直接导出原文：${formatError(err)}`, "warn");
      }
    }
    try {
      const saved = await invoke<string | null>("save_conversation_export", {
        name: activeProcess?.label ?? "conversation",
        content,
      });
      if (saved) pushToast(`已导出到 ${saved}`, "ok");
    } catch (err) {
      pushToast(`导出失败：${formatError(err)}`, "error");
    }
  };

  const gitStatus = useDrawerStore(state => state.gitStatus);
  const needsGitBanner = !generalMode && projectFacts && projectFacts.has_git === false;
  const profileLabel = profileDisplayLabel(activeProcess?.profile);

  return (
    <div className="chat-layout">
      <div className="chat-column">
        <div className="chat-topbar">
          <div className="chat-topbar-left">
            {!generalMode && gitStatus?.branch && (
              <button
                className="btn btn-ghost btn-sm mono"
                onClick={() => toggleDrawer("files")}
                title="查看分支与改动"
              >
                <IconBranch size={12} />
                {gitStatus.branch} <span>{gitStatus.changes} 改动</span>
              </button>
            )}
            {generalMode ? (
              <span className="badge badge-accent">无项目模式</span>
            ) : (
              <span className="badge">{profileLabel}</span>
            )}
            {activeProcess?.worktree_path && (
              <span className="badge badge-info" title={activeProcess.worktree_path}>
                隔离工作树
              </span>
            )}
          </div>

          <div className="chat-topbar-right">
            {generalMode && projects.projects.length > 0 && (
              <select
                className="composer-select"
                defaultValue=""
                aria-label="关联到项目"
                onChange={(e) => {
                  const dir = e.target.value;
                  e.target.value = "";
                  if (dir) void handleLinkToProject(dir);
                }}
              >
                <option value="">关联到项目…</option>
                {projects.projects.map((dir) => (
                  <option key={dir} value={dir}>
                    {projects.names[dir] ?? dir}
                  </option>
                ))}
              </select>
            )}
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              title="在当前项目开启新对话 (Ctrl+Shift+N)"
              onClick={() => void createConversation(activeProjectDir)}
            >
              <IconPlus size={12} />
              <span>新对话</span>
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              title="打开 /btw 旁路只读提问（不写入主对话历史）"
              disabled={!activeProcess}
              onClick={() => openBtwOverlay("")}
            >
              <IconSparkles size={12} />
              <span>/btw</span>
            </button>
            <ControlPopover label="更多对话操作" trigger="···">
              {(close) => (
                <>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    title="导出当前对话段的全部文本为 Markdown"
                    disabled={!activeProcess}
                    onClick={() => {
                      close();
                      void handleExport();
                    }}
                  >
                    <IconDownload size={12} />
                    <span>导出</span>
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    title="查看当前会话的运行轨迹与工具耗时（/trace）"
                    disabled={!activeProcess}
                    onClick={() => {
                      close();
                      openTraceDialog();
                    }}
                  >
                    <IconActivity size={12} />
                    <span>轨迹</span>
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    title="查看或清理当前会话的历次对话段"
                    disabled={!activeProcess}
                    onClick={() => {
                      close();
                      void openHistorySegments();
                    }}
                  >
                    <IconLayers size={12} />
                    <span>历史段</span>
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    title="压缩当前上下文（可指定保留重点）"
                    onClick={() => {
                      close();
                      void handleCompact();
                    }}
                  >
                    <IconCompact size={12} />
                    <span>压缩</span>
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    title="开启新的空白对话段"
                    onClick={() => {
                      close();
                      void handleClear();
                    }}
                  >
                    <IconTrash size={12} />
                    <span>重开上下文</span>
                  </button>
                </>
              )}
            </ControlPopover>
          </div>
        </div>

        {needsGitBanner && (
          <div className="facts-banner">
            <span>当前项目尚未初始化独立 Git 仓库，建议初始化以启用 Diff 对照与安全回滚。</span>
            <div className="facts-banner-actions">
              <button type="button" className="btn btn-sm btn-primary" onClick={() => void handleInitGit()}>
                一键初始化 Git
              </button>
            </div>
          </div>
        )}

        <MessageList />
        <BtwOverlay />
        <AskOverlay />
        <Composer />
      </div>

      <ErrorBoundary
        scope="文件与活动抽屉"
        resetKeys={[activeProjectDir, activeProcess?.id]}
        compact
      >
        <ContextDrawer />
      </ErrorBoundary>

      {traceOpen && (
        <div
          className="modal-scrim"
          role="dialog"
          aria-modal="true"
          aria-label="会话执行轨迹 Trace"
          onClick={() => uiStore.setState({ traceDialogOpen: false })}
        >
          <div className="modal-panel modal-lg" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <div className="modal-title">执行轨迹 Trace（{flatTraceRows.length} 条）</div>
              <button
                type="button"
                className="btn btn-ghost btn-icon"
                onClick={() => uiStore.setState({ traceDialogOpen: false })}
                aria-label="关闭"
              >
                <IconClose size={14} />
              </button>
            </div>
            <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              <div style={{ display: "flex", gap: 8 }}>
                <input
                  className="input"
                  placeholder="过滤事件类型、工具名或详情…"
                  value={traceFilter}
                  onChange={(e) => setTraceFilter(e.target.value)}
                />
                <button type="button" className="btn btn-sm" onClick={() => void loadTraces()}>
                  刷新
                </button>
              </div>
              {traceLoading ? (
                <div className="field-hint">正在读取执行轨迹…</div>
              ) : flatTraceRows.length === 0 ? (
                <div className="empty-state" style={{ padding: "24px 12px" }}>
                  <p>当前对话段还没有持久化的运行轨迹事件。</p>
                </div>
              ) : (
                <div className="trace-list">
                  {flatTraceRows
                    .slice(-200)
                    .reverse()
                    .map((row) => (
                      <details key={row.key} className="trace-row">
                        <summary className="trace-row-summary">
                          <span className="badge">{row.kind}</span>
                          <span className="mono trace-row-title">{row.title}</span>
                          {row.detail && <span className="field-hint trace-row-detail">{row.detail}</span>}
                        </summary>
                        <pre className="trace-row-json mono">{JSON.stringify(row.raw, null, 2)}</pre>
                      </details>
                    ))}
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      {segmentsOpen && (
        <div
          className="modal-scrim"
          role="dialog"
          aria-modal="true"
          aria-label="会话历史段管理"
          onClick={() => setSegmentsOpen(false)}
        >
          <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <div className="modal-title">会话历史段（{segments.length}）</div>
              <button
                type="button"
                className="btn btn-ghost btn-icon"
                onClick={() => setSegmentsOpen(false)}
                aria-label="关闭"
              >
                <IconClose size={14} />
              </button>
            </div>
            <div className="modal-body">
              {segmentsLoading ? (
                <div className="field-hint">正在读取历史段…</div>
              ) : segments.length === 0 ? (
                <div className="empty-state" style={{ padding: "24px 12px" }}>
                  <p>当前会话暂无已归档的历史对话段。</p>
                </div>
              ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                  {segments.map((seg) => (
                    <div key={seg.sequence} className="perm-rule-row">
                      <div style={{ minWidth: 0, flex: 1 }}>
                        <div className="mono" style={{ fontWeight: 600 }}>
                          段 #{seg.sequence}
                          {typeof seg.count === "number" ? ` · ${seg.count} 条消息` : ""}
                        </div>
                        {seg.preview && (
                          <div
                            className="field-hint"
                            style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                          >
                            {seg.preview}
                          </div>
                        )}
                      </div>
                      <div style={{ display: "flex", gap: 4, flex: "none" }}>
                        <button
                          type="button"
                          className="btn btn-sm"
                          onClick={() => void inspectSegment(seg.sequence)}
                        >
                          查看
                        </button>
                        <button
                          type="button"
                          className="btn btn-ghost btn-sm"
                          style={{ color: "var(--err)" }}
                          onClick={() => void deleteSegment(seg.sequence)}
                        >
                          删除
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
