import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import { useAppStore } from "../../store/appStore";
import { confirmDialog, pushToast } from "../../store/uiStore";
import { IconMemory, IconPlus, IconSearch, IconSend } from "../../components/Icons";
import { MarkdownView } from "../../components/MarkdownView";

type MemoryTab = "entries" | "inbox" | "chat" | "graph" | "diagnostics";

interface MemoryListEntry {
  id: string;
  scope: string;
  category: string;
  title: string;
  description?: string;
  status?: string;
  updated?: string;
  body?: string;
  refs?: string[];
  hits?: number;
  archived?: boolean;
  revision?: string;
}

interface NoteCandidate {
  id?: string;
  scope?: string;
  fingerprint?: string;
  header?: string;
  title?: string;
  summary?: string;
  content?: string;
  body?: string;
  index?: number;
}

interface RecallHitItem {
  id: string;
  title?: string;
  scope?: string;
  category?: string | null;
  injected?: boolean;
  read?: boolean | null;
}

interface RecallRoundItem {
  recall_id?: string;
  at?: number;
  created_at?: number;
  prompt_head?: string;
  query?: string;
  injected_bytes?: number;
  hits?: RecallHitItem[];
}

interface MemoryChatTurn {
  role: string;
  text: string;
  changes?: Array<{ tool?: string; ok?: boolean; summary?: string }>;
}

interface GraphPayload {
  nodes?: Array<{ id: string; label?: string; category?: string; kind?: string }>;
  links?: Array<{ source: string; target: string; kind?: string }>;
  edges?: Array<{ source: string; target: string; kind?: string }>;
}

let forceGraphPromise: Promise<unknown> | null = null;
function ensureForceGraph(): Promise<(el: HTMLElement) => Record<string, (...args: unknown[]) => unknown>> {
  if (!forceGraphPromise) {
    forceGraphPromise = new Promise((resolve, reject) => {
      const w = window as unknown as { ForceGraph?: unknown };
      if (w.ForceGraph) {
        resolve(w.ForceGraph);
        return;
      }
      const script = document.createElement("script");
      script.src = "./vendor/force-graph/force-graph-1.51.4.min.js";
      script.onload = () => resolve((window as unknown as { ForceGraph?: unknown }).ForceGraph);
      script.onerror = reject;
      document.head.appendChild(script);
    });
  }
  return forceGraphPromise as Promise<(el: HTMLElement) => Record<string, (...args: unknown[]) => unknown>>;
}

function categoryBadgeClass(cat: string): string {
  switch (cat) {
    case "fact":
      return "badge badge-fact";
    case "sop":
      return "badge badge-sop";
    case "habit":
      return "badge badge-habit";
    case "preference":
      return "badge badge-preference";
    default:
      return "badge";
  }
}

export default function MemoryView(): React.ReactElement {
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const [tab, setTab] = useState<MemoryTab>("entries");
  const [scope, setScope] = useState("all");
  const [showArchived, setShowArchived] = useState(false);
  const [category, setCategory] = useState("");
  const [query, setQuery] = useState("");
  const [entries, setEntries] = useState<MemoryListEntry[]>([]);
  const [selected, setSelected] = useState<MemoryListEntry | null>(null);
  const [editMode, setEditMode] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [editDesc, setEditDesc] = useState("");
  const [editBody, setEditBody] = useState("");
  const [editCategory, setEditCategory] = useState("fact");
  const [editScope, setEditScope] = useState("project");
  const [candidates, setCandidates] = useState<NoteCandidate[]>([]);
  const [overview, setOverview] = useState<Record<string, unknown> | null>(null);
  const [controlPlane, setControlPlane] = useState<Record<string, unknown> | null>(null);
  const [contextBill, setContextBill] = useState<Record<string, unknown> | null>(null);
  const [recallsPayload, setRecallsPayload] = useState<{
    rounds_total?: number;
    rounds?: RecallRoundItem[];
  } | null>(null);
  const [diagErrors, setDiagErrors] = useState<string[]>([]);
  const [jobStatus, setJobStatus] = useState<Record<string, unknown> | null>(null);
  const [chatTurns, setChatTurns] = useState<MemoryChatTurn[]>([]);
  const [chatHistoryError, setChatHistoryError] = useState<string | null>(null);
  const [chatInput, setChatInput] = useState("");
  const [chatBusy, setChatBusy] = useState(false);
  const [chatRequestId, setChatRequestId] = useState<string | null>(null);
  const [chatStreamText, setChatStreamText] = useState("");
  const [chatStreamTool, setChatStreamTool] = useState<string | null>(null);
  const [graphData, setGraphData] = useState<GraphPayload | null>(null);
  const graphContainerRef = useRef<HTMLDivElement>(null);

  const loadEntries = useCallback(async () => {
    if (!projectDir) return;
    try {
      const targetScopes = scope === "all" ? ["project", "global"] : [scope];
      if (showArchived) {
        const lists = await Promise.all(
          targetScopes.map((sc) =>
            invoke<MemoryListEntry[]>("memory_archived_entries", {
              projectDir,
              scope: sc,
            }).catch(() => [])
          )
        );
        setEntries(lists.flat());
      } else if (query.trim()) {
        const res = await invoke<MemoryListEntry[] | { items?: MemoryListEntry[] }>(
          "memory_search_page",
          {
            projectDir,
            query: query.trim(),
            limit: 100,
          }
        );
        const rawList = Array.isArray(res) ? res : Array.isArray(res?.items) ? res.items : [];
        const filtered = rawList.filter((item) => {
          if (scope !== "all" && item.scope !== scope) return false;
          if (category && item.category !== category) return false;
          return true;
        });
        setEntries(filtered);
      } else {
        const lists = await Promise.all(
          targetScopes.map((sc) =>
            invoke<MemoryListEntry[]>("memory_entries", {
              projectDir,
              scope: sc,
              category: category || null,
            }).catch(() => [])
          )
        );
        setEntries(lists.flat());
      }
    } catch (error) {
      pushToast(`加载记忆条目失败：${formatError(error)}`, "error");
    }
  }, [projectDir, scope, showArchived, category, query]);

  useEffect(() => {
    if (tab === "entries") void loadEntries();
  }, [tab, loadEntries]);

  useEffect(() => {
    if (!projectDir) return;
    if (tab === "inbox") {
      Promise.all([
        invoke<{ notes?: NoteCandidate[] } | NoteCandidate[]>("memory_note_candidates", {
          projectDir,
        }).catch(() => []),
        invoke<Record<string, unknown>>("memory_job_status", { projectDir }).catch(() => null),
      ]).then(([cands, job]) => {
        setCandidates(Array.isArray(cands) ? cands : cands?.notes ?? []);
        setJobStatus(job);
      });
    } else if (tab === "diagnostics") {
      const nextErrors: string[] = [];
      Promise.all([
        invoke<Record<string, unknown>>("memory_overview", { projectDir }).catch((err) => {
          nextErrors.push(`记忆库概览读取失败：${formatError(err)}`);
          return null;
        }),
        invoke<Record<string, unknown>>("memory_control_plane", { projectDir }).catch((err) => {
          nextErrors.push(`控制面诊断读取失败：${formatError(err)}`);
          return null;
        }),
        invoke<Record<string, unknown>>("memory_context_bill", { projectDir }).catch((err) => {
          nextErrors.push(`上下文账单读取失败：${formatError(err)}`);
          return null;
        }),
        invoke<{ rounds_total?: number; rounds?: RecallRoundItem[] } | RecallRoundItem[]>(
          "memory_recalls",
          { projectDir, limit: 30 }
        ).catch((err) => {
          nextErrors.push(`召回记录读取失败：${formatError(err)}`);
          return null;
        }),
      ]).then(([ov, cp, bill, rec]) => {
        setOverview(ov);
        setControlPlane(cp);
        setContextBill(bill);
        if (Array.isArray(rec)) {
          setRecallsPayload({ rounds_total: rec.length, rounds: rec });
        } else {
          setRecallsPayload(rec);
        }
        setDiagErrors(nextErrors);
      });
    } else if (tab === "chat") {
      setChatHistoryError(null);
      invoke<{
        messages?: MemoryChatTurn[];
        turns?: MemoryChatTurn[];
        requestId?: string | null;
      }>("memory_chat_history", {
        projectDir,
      })
        .then((res) => {
          const list = Array.isArray(res?.messages)
            ? res.messages
            : Array.isArray(res?.turns)
            ? res.turns
            : [];
          setChatTurns(list);
          if (typeof res?.requestId === "string" && res.requestId) {
            setChatRequestId(res.requestId);
          }
        })
        .catch((err) => {
          const msg = formatError(err);
          setChatHistoryError(msg);
          pushToast(`读取管理对话历史失败：${msg}`, "error");
        });
    } else if (tab === "graph") {
      invoke<GraphPayload>("memory_graph", { projectDir })
        .then(setGraphData)
        .catch((err) => pushToast(`加载记忆图谱失败：${formatError(err)}`, "error"));
    }
  }, [tab, projectDir]);

  useEffect(() => {
    if (tab !== "graph" || !graphData || !graphContainerRef.current) return;
    const container = graphContainerRef.current;
    const nodes = (graphData.nodes ?? []).map((n) => ({ ...n }));
    const links = (graphData.links ?? graphData.edges ?? []).map((l) => ({ ...l }));
    if (nodes.length === 0) {
      container.innerHTML = "";
      return;
    }
    let disposed = false;
    let observer: ResizeObserver | undefined;
    ensureForceGraph()
      .then((Factory) => {
        if (disposed || !Factory || !container) return;
        container.innerHTML = "";
        const style = getComputedStyle(document.documentElement);
        const colorByCategory: Record<string, string> = {
          fact: style.getPropertyValue("--graph-fact").trim() || "#4cc2ff",
          sop: style.getPropertyValue("--graph-sop").trim() || "#c084fc",
          habit: style.getPropertyValue("--graph-habit").trim() || "#6382f6",
          preference: style.getPropertyValue("--graph-preference").trim() || "#f472b6",
        };
        const defaultNodeColor = style.getPropertyValue("--graph-module").trim() || "#7e8291";
        const edgeColor = style.getPropertyValue("--graph-edge").trim() || "#565656";

        const graph = Factory(container);
        const resize = () => {
          graph.width(container.clientWidth || 720);
          graph.height(container.clientHeight || 480);
        };
        resize();
        graph.graphData({ nodes, links });
        graph.nodeLabel((n: unknown) => {
          const item = n as { id?: string; label?: string; category?: string };
          return `${item.id ?? ""} ${item.label ?? ""}`.trim();
        });
        graph.nodeColor((n: unknown) => {
          const item = n as { category?: string };
          return (item.category && colorByCategory[item.category]) || defaultNodeColor;
        });
        graph.linkColor(() => edgeColor);

        if (typeof ResizeObserver !== "undefined") {
          observer = new ResizeObserver(resize);
          observer.observe(container);
        }
      })
      .catch(() => {});
    return () => {
      disposed = true;
      observer?.disconnect();
    };
  }, [tab, graphData]);

  const overviewNumbers = useMemo(() => {
    if (!overview) return [];
    return Object.entries(overview)
      .filter(([, v]) => typeof v === "number" || typeof v === "string")
      .slice(0, 6);
  }, [overview]);

  if (!projectDir) {
    return (
      <div className="empty-state" style={{ flex: 1 }}>
        <div className="empty-state-icon">
          <IconMemory size={20} />
        </div>
        <h3>请先选择一个项目</h3>
        <p>记忆库按当前项目与全局范围组织，请先在侧栏选择项目。</p>
      </div>
    );
  }

  const openEntry = async (item: MemoryListEntry) => {
    try {
      const full = await invoke<MemoryListEntry>("memory_entry_get", {
        projectDir,
        scope: item.scope,
        id: item.id,
      });
      setSelected(full);
      setEditMode(false);
    } catch (error) {
      pushToast(`读取记忆详情失败：${formatError(error)}`, "error");
    }
  };

  const startNewEntry = () => {
    setSelected(null);
    setEditScope("project");
    setEditCategory("fact");
    setEditTitle("");
    setEditDesc("");
    setEditBody("");
    setEditMode(true);
    setTab("entries");
  };

  const startEditSelected = () => {
    if (!selected) return;
    setEditScope(selected.scope);
    setEditCategory(selected.category);
    setEditTitle(selected.title);
    setEditDesc(selected.description ?? "");
    setEditBody(selected.body ?? "");
    setEditMode(true);
  };

  const saveEntry = async () => {
    if (!editTitle.trim() || !editBody.trim()) {
      pushToast("请填写记忆标题与正文", "warn");
      return;
    }
    try {
      const saved = await invoke<MemoryListEntry | null>("memory_entry_save", {
        projectDir,
        scope: selected ? selected.scope : editScope,
        id: selected?.id ?? "",
        category: editCategory,
        title: editTitle.trim(),
        description: editDesc.trim() || editTitle.trim(),
        body: editBody,
      });
      pushToast(selected ? "已保存记忆条目" : "已创建新记忆条目", "ok");
      setEditMode(false);
      if (saved && typeof saved === "object" && saved.id) {
        setSelected(saved);
      }
      await loadEntries();
    } catch (error) {
      pushToast(`保存失败：${formatError(error)}`, "error");
    }
  };

  const deleteSelected = async () => {
    if (!selected) return;
    const ok = await confirmDialog({
      title: "删除记忆条目？",
      message: `确定删除「${selected.title}」(${selected.id}) 吗？`,
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await invoke("memory_entry_delete", {
        projectDir,
        scope: selected.scope,
        id: selected.id,
      });
      setSelected(null);
      pushToast("已删除记忆条目", "ok");
      await loadEntries();
    } catch (error) {
      pushToast(`删除失败：${formatError(error)}`, "error");
    }
  };

  const sendMemoryChat = async () => {
    const prompt = chatInput.trim();
    if (!prompt || chatBusy) return;
    const requestId = `mem-chat-${Date.now()}`;
    setChatBusy(true);
    setChatRequestId(requestId);
    setChatStreamText("");
    setChatStreamTool(null);
    setChatTurns((prev) => [...prev, { role: "user", text: prompt }]);
    setChatInput("");
    try {
      const TauriChannel = (
        window.__TAURI__?.core as unknown as {
          Channel?: new () => { onmessage?: (ev: Record<string, unknown>) => void };
        }
      )?.Channel;
      const onEvent = TauriChannel ? new TauriChannel() : null;
      if (onEvent) {
        onEvent.onmessage = (ev) => {
          if (ev?.type === "text" && typeof ev.text === "string") {
            setChatStreamText((prev) => prev + ev.text);
          } else if (ev?.type === "tool") {
            setChatStreamTool(`${String(ev.name ?? "tool")}：${String(ev.text ?? "")}`);
          } else if (ev?.type === "tool_done") {
            setChatStreamTool(null);
          }
        };
      }
      const res = await invoke<{
        text?: string;
        reply?: string;
        changes?: Array<{ tool?: string; ok?: boolean; summary?: string }>;
        messages?: MemoryChatTurn[];
        turns?: MemoryChatTurn[];
        stopped?: boolean;
        error?: string | null;
      }>("memory_chat_send", {
        projectDir,
        message: prompt,
        requestId,
        target: selected ? { scope: selected.scope, id: selected.id } : null,
        onEvent,
      });
      if (Array.isArray(res?.messages)) {
        setChatTurns(res.messages);
      } else if (Array.isArray(res?.turns)) {
        setChatTurns(res.turns);
      } else {
        const finalText = res?.text ?? res?.reply ?? "";
        if (finalText) {
          setChatTurns((prev) => [
            ...prev,
            { role: "assistant", text: finalText, changes: res?.changes },
          ]);
        }
      }
      if (res?.error) {
        pushToast(`管理对话提示：${res.error}`, "warn");
      }
      if (res?.changes && res.changes.length > 0) {
        void loadEntries();
      }
    } catch (error) {
      pushToast(`管理对话失败：${formatError(error)}`, "error");
    } finally {
      setChatBusy(false);
      setChatRequestId(null);
      setChatStreamText("");
      setChatStreamTool(null);
    }
  };

  const stopMemoryChat = async () => {
    if (!projectDir || !chatRequestId) return;
    try {
      await invoke("memory_chat_stop", { projectDir, requestId: chatRequestId });
      pushToast("已发送中止请求", "info");
    } catch (error) {
      pushToast(`停止失败：${formatError(error)}`, "error");
    }
  };

  const stopMemoryJob = async () => {
    const jobId = typeof jobStatus?.jobId === "string" ? jobStatus.jobId : typeof jobStatus?.id === "string" ? jobStatus.id : "";
    if (!projectDir || !jobId) return;
    try {
      await invoke("memory_job_stop", { projectDir, jobId });
      pushToast("已发送作业停止请求", "ok");
      const nextStatus = await invoke<Record<string, unknown>>("memory_job_status", { projectDir }).catch(() => null);
      setJobStatus(nextStatus);
    } catch (error) {
      pushToast(`停止作业失败：${formatError(error)}`, "error");
    }
  };

  const graphNodeCount = graphData?.nodes?.length ?? 0;
  const graphEdgeCount = (graphData?.links ?? graphData?.edges ?? []).length;

  return (
    <div className="page-shell">
      <div className="page-header">
        <div>
          <h1 className="page-title">记忆管理</h1>
          <div className="page-subtitle">浏览、检索、整理与图谱化项目与全局经验记忆。</div>
        </div>
        <div className="page-header-actions">
          <button type="button" className="btn btn-primary" onClick={startNewEntry}>
            <IconPlus size={13} />
            <span>新建记忆</span>
          </button>
        </div>
      </div>

      <div className="page-tabs" role="tablist">
        {(
          [
            ["entries", "记忆列表"],
            ["inbox", `待整理 (${candidates.length})`],
            ["chat", "管理对话"],
            ["graph", "记忆图谱"],
            ["diagnostics", "概览与诊断"],
          ] as Array<[MemoryTab, string]>
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            className={`page-tab-btn${tab === id ? " active" : ""}`}
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {tab === "entries" && (
        <div className="split-page">
          <div className="split-left">
            <div className="split-left-filters">
              <div className="sidebar-search-wrap">
                <span className="sidebar-search-icon" aria-hidden="true">
                  <IconSearch size={12} />
                </span>
                <input
                  className="input sidebar-search"
                  placeholder="搜索记忆标题或内容…"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
              </div>
              <div className="split-left-row">
                <select className="select" value={scope} onChange={(e) => setScope(e.target.value)}>
                  <option value="all">全部范围</option>
                  <option value="project">项目记忆</option>
                  <option value="global">全局记忆</option>
                </select>
                <select className="select" value={category} onChange={(e) => setCategory(e.target.value)}>
                  <option value="">全部类别</option>
                  <option value="fact">fact 事实</option>
                  <option value="sop">sop 流程</option>
                  <option value="habit">habit 习惯</option>
                  <option value="preference">preference 偏好</option>
                </select>
              </div>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", paddingTop: 2 }}>
                <span className="field-hint">查看已归档记忆</span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={showArchived}
                  aria-label="查看已归档记忆"
                  className="switch"
                  onClick={() => setShowArchived(!showArchived)}
                />
              </div>
            </div>

            <div className="split-left-scroll">
              {entries.length === 0 ? (
                <div className="sidebar-empty-hint">暂无匹配的记忆条目</div>
              ) : (
                entries.map((item, idx) => (
                  <button
                    key={`${item.scope}:${item.id}:${idx}`}
                    type="button"
                    className={`list-card-item cat-${item.category}${
                      selected?.id === item.id && selected?.scope === item.scope ? " active" : ""
                    }`}
                    onClick={() => void openEntry(item)}
                  >
                    <div className="list-card-title">
                      <span className="list-card-title-text">{item.title}</span>
                      <span className={categoryBadgeClass(item.category)}>{item.category}</span>
                    </div>
                    {item.description && <div className="list-card-desc">{item.description}</div>}
                    <div className="list-card-meta">
                      <span>{item.id}</span>
                      <span>·</span>
                      <span>{item.scope}</span>
                    </div>
                  </button>
                ))
              )}
            </div>
          </div>

          <div className="split-right">
            <div className="split-right-scroll">
              {editMode ? (
                <div className="detail-container">
                  <div className="detail-header">
                    <h2 className="detail-title">
                      {selected ? `编辑记忆 ${selected.id}` : "新建记忆条目"}
                    </h2>
                  </div>
                  <div className="settings-grid-2">
                    <div className="field-group">
                      <label className="field-label">范围</label>
                      <select
                        className="select"
                        value={editScope}
                        disabled={Boolean(selected)}
                        onChange={(e) => setEditScope(e.target.value)}
                      >
                        <option value="project">项目</option>
                        <option value="global">全局</option>
                      </select>
                    </div>
                    <div className="field-group">
                      <label className="field-label">类别</label>
                      <select className="select" value={editCategory} onChange={(e) => setEditCategory(e.target.value)}>
                        <option value="fact">fact 事实</option>
                        <option value="sop">sop 流程</option>
                        <option value="habit">habit 习惯</option>
                        <option value="preference">preference 偏好</option>
                      </select>
                    </div>
                  </div>
                  <div className="field-group">
                    <label className="field-label">标题</label>
                    <input className="input" value={editTitle} onChange={(e) => setEditTitle(e.target.value)} />
                  </div>
                  <div className="field-group">
                    <label className="field-label">简短说明</label>
                    <input className="input" value={editDesc} onChange={(e) => setEditDesc(e.target.value)} />
                  </div>
                  <div className="field-group">
                    <label className="field-label">正文（Markdown）</label>
                    <textarea
                      className="textarea mono"
                      style={{ minHeight: 240 }}
                      value={editBody}
                      onChange={(e) => setEditBody(e.target.value)}
                    />
                  </div>
                  <div style={{ display: "flex", gap: 8 }}>
                    <button type="button" className="btn btn-primary" onClick={() => void saveEntry()}>
                      保存
                    </button>
                    <button type="button" className="btn" onClick={() => setEditMode(false)}>
                      取消
                    </button>
                  </div>
                </div>
              ) : selected ? (
                <div className="detail-container">
                  <div className="detail-header">
                    <div>
                      <h2 className="detail-title">{selected.title}</h2>
                      <div className="detail-meta-row">
                        <span className="badge mono">{selected.id}</span>
                        <span className="badge">{selected.scope}</span>
                        <span className={categoryBadgeClass(selected.category)}>{selected.category}</span>
                        <span className="badge badge-ok">{selected.status ?? "active"}</span>
                      </div>
                    </div>
                    <div className="detail-actions">
                      {selected.archived ? (
                        <button
                          type="button"
                          className="btn btn-sm btn-primary"
                          onClick={() =>
                            void invoke("memory_entry_restore", {
                              projectDir,
                              scope: selected.scope,
                              id: selected.id,
                              status: "active",
                            })
                              .then(() => {
                                pushToast("已从归档恢复记忆", "ok");
                                setSelected(null);
                                void loadEntries();
                              })
                              .catch((e) => pushToast(formatError(e), "error"))
                          }
                        >
                          恢复为有效
                        </button>
                      ) : (
                        <>
                          {selected.revision && (
                            <>
                              <button
                                type="button"
                                className="btn btn-sm"
                                onClick={() =>
                                  void invoke("memory_entry_feedback", {
                                    projectDir,
                                    scope: selected.scope,
                                    id: selected.id,
                                    expectedRevision: selected.revision!,
                                    kind: "adopted",
                                  })
                                    .then(() => pushToast("已记录「有帮助」反馈", "ok"))
                                    .catch((e) => pushToast(formatError(e), "error"))
                                }
                              >
                                有帮助
                              </button>
                              <button
                                type="button"
                                className="btn btn-sm"
                                onClick={() =>
                                  void invoke("memory_entry_feedback", {
                                    projectDir,
                                    scope: selected.scope,
                                    id: selected.id,
                                    expectedRevision: selected.revision!,
                                    kind: "corrected",
                                  })
                                    .then(() => pushToast("已记录「需修正」反馈", "warn"))
                                    .catch((e) => pushToast(formatError(e), "error"))
                                }
                              >
                                需修正
                              </button>
                            </>
                          )}
                          <button type="button" className="btn btn-sm btn-primary" onClick={startEditSelected}>
                            编辑
                          </button>
                          <button type="button" className="btn btn-danger btn-sm" onClick={() => void deleteSelected()}>
                            删除
                          </button>
                        </>
                      )}
                    </div>
                  </div>

                  {selected.description && <p className="field-hint">{selected.description}</p>}
                  <div className="selectable">
                    <MarkdownView text={selected.body ?? ""} />
                  </div>
                </div>
              ) : (
                <div className="empty-state">
                  <div className="empty-state-icon">
                    <IconMemory size={20} />
                  </div>
                  <h3>选择一条记忆查看详情</h3>
                  <p>从左侧浏览项目与全局经验记忆，或点击右上角「新建记忆」沉淀新规则。</p>
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      {tab === "inbox" && (
        <div className="page-scroll">
          <div className="view-toolbar">
            <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() =>
                  void invoke("memory_consolidate", {
                    projectDir,
                    requestId: `cons-${Date.now()}`,
                  })
                    .then(() => pushToast("已启动记忆整理任务", "ok"))
                    .catch((e) => pushToast(`整理失败：${formatError(e)}`, "error"))
                }
              >
                整理收件箱
              </button>
              <button
                type="button"
                className="btn"
                onClick={() =>
                  void invoke("memory_dream", {
                    projectDir,
                    requestId: `dream-${Date.now()}`,
                  })
                    .then(() => pushToast("已启动记忆重构（Dream）", "ok"))
                    .catch((e) => pushToast(`启动失败：${formatError(e)}`, "error"))
                }
              >
                后台做梦重构
              </button>
              {jobStatus && (
                <>
                  <span className="badge badge-info">
                    作业状态：{String(jobStatus.status ?? "idle")}
                  </span>
                  {String(jobStatus.status ?? "") === "running" && (
                    <button
                      type="button"
                      className="btn btn-danger btn-sm"
                      onClick={() => void stopMemoryJob()}
                    >
                      停止作业
                    </button>
                  )}
                </>
              )}
            </div>
          </div>

          {candidates.length === 0 ? (
            <div className="empty-state">
              <h3>待整理收件箱为空</h3>
              <p>对话过程中自动沉淀的候选经验笔记会汇集在此，等待审核吸收。</p>
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 12, maxWidth: 820 }}>
              {candidates.map((cand, idx) => (
                <div key={cand.fingerprint ?? cand.id ?? idx} className="provider-card">
                  <div className="provider-card-head">
                    <strong style={{ fontSize: 13.5 }}>{cand.title ?? cand.header ?? `笔记 #${idx + 1}`}</strong>
                    <div style={{ display: "flex", gap: 6 }}>
                      <button
                        type="button"
                        className="btn btn-sm btn-primary"
                        onClick={() =>
                          void invoke("memory_consolidate_note", {
                            projectDir,
                            requestId: `note-${Date.now()}`,
                            scope: cand.scope ?? "project",
                            summary: cand.summary ?? cand.content ?? cand.body ?? "",
                          })
                            .then(() => pushToast("已吸收该条笔记", "ok"))
                            .catch((e) => pushToast(formatError(e), "error"))
                        }
                      >
                        吸收
                      </button>
                      {cand.fingerprint && (
                        <button
                          type="button"
                          className="btn btn-sm btn-ghost"
                          onClick={() =>
                            void invoke("memory_note_discard", {
                              projectDir,
                              scope: cand.scope ?? "project",
                              fingerprint: cand.fingerprint!,
                            })
                              .then(() => {
                                setCandidates((prev) =>
                                  prev.filter((x) => x.fingerprint !== cand.fingerprint)
                                );
                                pushToast("已丢弃笔记", "ok");
                              })
                              .catch((e) => pushToast(formatError(e), "error"))
                          }
                        >
                          丢弃
                        </button>
                      )}
                    </div>
                  </div>
                  <pre className="tool-pre">{cand.content ?? cand.body ?? ""}</pre>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {tab === "chat" && (
        <div className="page-shell">
          <div className="memory-chat-stream">
            <div className="memory-chat-inner">
              {chatHistoryError && (
                <div className="provider-card" role="alert" style={{ borderColor: "var(--err)" }}>
                  <div style={{ color: "var(--err)", fontSize: 12.5 }}>
                    读取管理对话历史失败：{chatHistoryError}
                  </div>
                </div>
              )}
              {chatTurns.length === 0 ? (
                <div className="empty-state">
                  <h3>记忆管理对话</h3>
                  <p>可以直接对话让记忆管理员帮你检索、合并、修正或清理项目记忆。</p>
                </div>
              ) : (
                chatTurns.map((turn, idx) => (
                  <div key={idx} className={`memory-chat-bubble ${turn.role}`}>
                    <div className="field-label">{turn.role === "user" ? "你" : "记忆管理员"}</div>
                    <MarkdownView text={turn.text} />
                    {turn.changes && turn.changes.length > 0 && (
                      <div style={{ display: "flex", flexWrap: "wrap", gap: 4, marginTop: 4 }}>
                        {turn.changes.map((c, ci) => (
                          <span
                            key={ci}
                            className={`badge ${c.ok === false ? "badge-err" : "badge-ok"}`}
                            title={c.summary ?? ""}
                          >
                            {c.tool ?? "修改"}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                ))
              )}
              {chatBusy && (chatStreamText || chatStreamTool) && (
                <div className="memory-chat-bubble assistant">
                  <div className="field-label">
                    记忆管理员 {chatStreamTool ? `· ${chatStreamTool}` : "· 正在处理…"}
                  </div>
                  {chatStreamText && <MarkdownView text={chatStreamText} />}
                </div>
              )}
            </div>
          </div>
          <div className="settings-savebar">
            <input
              className="input"
              placeholder="输入对记忆库的管理指令…"
              value={chatInput}
              onChange={(e) => setChatInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.nativeEvent.isComposing) void sendMemoryChat();
              }}
            />
            <div style={{ display: "flex", gap: 6, flex: "none" }}>
              <button
                type="button"
                className="btn btn-ghost"
                disabled={chatBusy}
                onClick={() =>
                  void invoke("memory_chat_reset", { projectDir }).then(() => {
                    setChatTurns([]);
                    pushToast("已重置管理对话", "ok");
                  })
                }
              >
                重置
              </button>
              {chatBusy ? (
                <button
                  type="button"
                  className="btn btn-danger"
                  onClick={() => void stopMemoryChat()}
                >
                  停止
                </button>
              ) : (
                <button
                  type="button"
                  className="btn btn-accent"
                  disabled={!chatInput.trim()}
                  onClick={() => void sendMemoryChat()}
                >
                  <IconSend size={13} />
                  <span>发送</span>
                </button>
              )}
            </div>
          </div>
        </div>
      )}

      {tab === "graph" && (
        <div className="memory-graph-stage">
          <div className="memory-graph-legend">
            <span className="legend-item">
              <span className="legend-dot" style={{ background: "var(--graph-fact)" }} />
              fact 事实
            </span>
            <span className="legend-item">
              <span className="legend-dot" style={{ background: "var(--graph-sop)" }} />
              sop 流程
            </span>
            <span className="legend-item">
              <span className="legend-dot" style={{ background: "var(--graph-habit)" }} />
              habit 习惯
            </span>
            <span className="legend-item">
              <span className="legend-dot" style={{ background: "var(--graph-preference)" }} />
              preference 偏好
            </span>
            <span className="badge mono">
              {graphNodeCount} 节点 · {graphEdgeCount} 关联
            </span>
          </div>
          {graphNodeCount === 0 ? (
            <div className="empty-state" style={{ height: "100%" }}>
              <div className="empty-state-icon">
                <IconMemory size={20} />
              </div>
              <h3>暂无可展示的记忆图谱节点</h3>
              <p>当项目积累了带交叉引用的记忆条目后，此处将自动渲染关系网络。</p>
            </div>
          ) : (
            <div ref={graphContainerRef} style={{ width: "100%", height: "100%" }} />
          )}
        </div>
      )}

      {tab === "diagnostics" && (() => {
        const recallRounds = recallsPayload?.rounds ?? [];
        const latestRound = recallRounds[0] ?? null;
        const latestHits = latestRound?.hits ?? [];
        const latestInjectedCount = latestHits.filter((h) => h.injected).length;
        const latestReadCount = latestHits.filter((h) => h.read === true).length;
        const cpRecall = (controlPlane?.recall as Record<string, number> | undefined) ?? {};
        const scopes = Array.isArray(overview?.scopes)
          ? (overview.scopes as Array<{
              scope?: string;
              total?: number;
              archived?: number;
              integrity?: unknown[];
            }>)
          : [];
        const integrityIssues = scopes.flatMap((sc) =>
          Array.isArray(sc.integrity)
            ? sc.integrity.map((issue) => `${sc.scope ?? "memory"}: ${typeof issue === "string" ? issue : JSON.stringify(issue)}`)
            : []
        );
        const promotionGaps = Number(controlPlane?.promotion_gaps ?? 0);
        const orphanedEvents = Number(cpRecall.events_orphaned ?? 0);
        const hasFailures = diagErrors.length > 0 || integrityIssues.length > 0;

        return (
          <div className="page-scroll">
            <div style={{ maxWidth: 860, display: "flex", flexDirection: "column", gap: 14 }}>
              <div className="provider-card" aria-label="记忆诊断结论摘要">
                <div className="provider-card-head">
                  <div className="provider-card-title">最近一次召回与健康摘要</div>
                  <span className={`badge ${hasFailures ? "badge-err" : "badge-ok"}`}>
                    {hasFailures ? "存在读取/完整性异常" : "读取正常"}
                  </span>
                </div>

                <div className="stats-strip" style={{ marginBottom: 4 }}>
                  <div className="stat-tile">
                    <span className="stat-tile-label">最近召回命中</span>
                    <span className="stat-tile-value">{latestHits.length} 条</span>
                  </div>
                  <div className="stat-tile">
                    <span className="stat-tile-label">已注入上下文</span>
                    <span className="stat-tile-value accent">{latestInjectedCount} 条</span>
                  </div>
                  <div className="stat-tile">
                    <span className="stat-tile-label">累计召回 / 注入 / 读取</span>
                    <span className="stat-tile-value mono" style={{ fontSize: 15 }}>
                      {cpRecall.recalled ?? 0} / {cpRecall.injected ?? 0} / {cpRecall.read ?? 0}
                    </span>
                  </div>
                  <div className="stat-tile">
                    <span className="stat-tile-label">读取失败与完整性问题</span>
                    <span className={`stat-tile-value${hasFailures ? " warn" : " ok"}`}>
                      {diagErrors.length + integrityIssues.length}
                    </span>
                  </div>
                </div>

                {latestRound ? (
                  <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                    <div className="field-hint">
                      最近一轮召回（共记录 {recallsPayload?.rounds_total ?? recallRounds.length} 轮）：
                      {latestHits.length === 0
                        ? "未命中任何记忆条目"
                        : `命中 ${latestHits.length} 条，其中 ${latestInjectedCount} 条已注入，${latestReadCount} 条已确认被读取`}
                    </div>
                    {latestHits.length > 0 && (
                      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                        {latestHits.map((hit) => (
                          <div key={hit.id} className="perm-rule-row">
                            <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                              <span className="mono">{hit.id}</span>
                              <span>{hit.title ?? hit.id}</span>
                              {hit.category && (
                                <span className={categoryBadgeClass(hit.category)}>
                                  {hit.category}
                                </span>
                              )}
                            </span>
                            <span style={{ display: "inline-flex", gap: 6 }}>
                              <span className={`badge ${hit.injected ? "badge-ok" : ""}`}>
                                {hit.injected ? "已注入" : "未注入"}
                              </span>
                              {hit.injected && (
                                <span className={`badge ${hit.read ? "badge-accent" : ""}`}>
                                  {hit.read === true ? "已读取" : hit.read === false ? "未读取" : "待观测"}
                                </span>
                              )}
                            </span>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                ) : (
                  <div className="field-hint">
                    当前项目还没有记录到记忆召回事件（累计召回 {cpRecall.recalled ?? 0} 次）。
                  </div>
                )}

                {diagErrors.length > 0 && (
                  <div role="alert" style={{ color: "var(--err)", fontSize: 12.5 }}>
                    {diagErrors.map((err, idx) => (
                      <div key={idx}>• {err}</div>
                    ))}
                  </div>
                )}
                {integrityIssues.length > 0 && (
                  <div style={{ color: "var(--warn)", fontSize: 12.5 }}>
                    {integrityIssues.slice(0, 6).map((issue, idx) => (
                      <div key={idx}>• 完整性告警：{issue}</div>
                    ))}
                  </div>
                )}
                {!hasFailures && (
                  <div className="field-hint">
                    无读取失败与索引完整性异常
                    {promotionGaps > 0 ? `；另有 ${promotionGaps} 条待补来源晋升条目` : ""}
                    {orphanedEvents > 0 ? `；另有 ${orphanedEvents} 条未关联事件` : ""}。
                  </div>
                )}
              </div>

              {overviewNumbers.length > 0 && (
                <div className="stats-strip" style={{ marginBottom: 4 }}>
                  {overviewNumbers.map(([k, v]) => (
                    <div key={k} className="stat-tile">
                      <span className="stat-tile-label">{k}</span>
                      <span className="stat-tile-value">{String(v)}</span>
                    </div>
                  ))}
                </div>
              )}

              <details className="diag-card">
                <summary className="diag-card-summary">
                  <span>技术详情 · 记忆库概览（原始快照）</span>
                  <span className="badge">{overviewNumbers.length} 项统计</span>
                </summary>
                <div className="diag-card-body">
                  <pre className="tool-pre">{JSON.stringify(overview, null, 2)}</pre>
                </div>
              </details>

              <details className="diag-card">
                <summary className="diag-card-summary">
                  <span>技术详情 · 控制面诊断</span>
                  <span className="badge">
                  {typeof controlPlane?.backlog === "number"
                    ? "待处理 " + controlPlane.backlog
                    : "暂无待处理统计"}
                </span>
                </summary>
                <div className="diag-card-body">
                  <pre className="tool-pre">{JSON.stringify(controlPlane, null, 2)}</pre>
                </div>
              </details>

              <details className="diag-card">
                <summary className="diag-card-summary">
                  <span>技术详情 · 上下文注入账单</span>
                  <span className="badge">
                  {Array.isArray(contextBill?.episodes)
                    ? "近期 " + contextBill.episodes.length + " 轮"
                    : "暂无轮次统计"}
                </span>
                </summary>
                <div className="diag-card-body">
                  <pre className="tool-pre">{JSON.stringify(contextBill, null, 2)}</pre>
                </div>
              </details>

              <details className="diag-card">
                <summary className="diag-card-summary">
                  <span>技术详情 · 近期召回记录</span>
                  <span className="badge mono">{recallRounds.length} 轮</span>
                </summary>
                <div className="diag-card-body">
                  <pre className="tool-pre">{JSON.stringify(recallsPayload, null, 2)}</pre>
                </div>
              </details>
            </div>
          </div>
        );
      })()}
    </div>
  );
}
