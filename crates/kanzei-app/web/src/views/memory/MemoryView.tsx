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
  const [recalls, setRecalls] = useState<unknown[]>([]);
  const [jobStatus, setJobStatus] = useState<Record<string, unknown> | null>(null);
  const [chatTurns, setChatTurns] = useState<Array<{ role: string; text: string }>>([]);
  const [chatInput, setChatInput] = useState("");
  const [chatBusy, setChatBusy] = useState(false);
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
        const res = await invoke<{ items?: MemoryListEntry[] }>("memory_search_page", {
          projectDir,
          query: query.trim(),
          limit: 100,
        });
        setEntries(res?.items ?? []);
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
      Promise.all([
        invoke<Record<string, unknown>>("memory_overview", { projectDir }).catch(() => null),
        invoke<Record<string, unknown>>("memory_control_plane", { projectDir }).catch(() => null),
        invoke<Record<string, unknown>>("memory_context_bill", { projectDir }).catch(() => null),
        invoke<unknown[]>("memory_recalls", { projectDir, limit: 30 }).catch(() => []),
      ]).then(([ov, cp, bill, rec]) => {
        setOverview(ov);
        setControlPlane(cp);
        setContextBill(bill);
        setRecalls(Array.isArray(rec) ? rec : []);
      });
    } else if (tab === "chat") {
      invoke<{ turns?: Array<{ role: string; text: string }> }>("memory_chat_history", {
        projectDir,
      })
        .then((res) => setChatTurns(res?.turns ?? []))
        .catch(() => {});
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
      await invoke("memory_entry_save", {
        projectDir,
        scope: editScope,
        id: selected?.id ?? "",
        title: editTitle.trim(),
        description: editDesc.trim(),
        body: editBody,
      });
      pushToast("已保存记忆条目", "ok");
      setEditMode(false);
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
    setChatBusy(true);
    setChatTurns((prev) => [...prev, { role: "user", text: prompt }]);
    setChatInput("");
    try {
      const requestId = `mem-chat-${Date.now()}`;
      const TauriChannel = (
        window.__TAURI__?.core as unknown as { Channel?: new () => { onmessage?: (ev: unknown) => void } }
      )?.Channel;
      const onEvent = TauriChannel ? new TauriChannel() : null;
      const res = await invoke<{ reply?: string; turns?: Array<{ role: string; text: string }> }>(
        "memory_chat_send",
        { projectDir, message: prompt, requestId, onEvent }
      );
      if (Array.isArray(res?.turns)) {
        setChatTurns(res.turns);
      } else if (res?.reply) {
        setChatTurns((prev) => [...prev, { role: "assistant", text: res.reply! }]);
      }
    } catch (error) {
      pushToast(`管理对话失败：${formatError(error)}`, "error");
    } finally {
      setChatBusy(false);
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
                      <select className="select" value={editScope} onChange={(e) => setEditScope(e.target.value)}>
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
                <span className="badge badge-info">作业状态：{String(jobStatus.status ?? "idle")}</span>
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
                  </div>
                ))
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
                if (e.key === "Enter") void sendMemoryChat();
              }}
            />
            <div style={{ display: "flex", gap: 6, flex: "none" }}>
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() =>
                  void invoke("memory_chat_reset", { projectDir }).then(() => {
                    setChatTurns([]);
                    pushToast("已重置管理对话", "ok");
                  })
                }
              >
                重置
              </button>
              <button
                type="button"
                className="btn btn-accent"
                disabled={chatBusy || !chatInput.trim()}
                onClick={() => void sendMemoryChat()}
              >
                <IconSend size={13} />
                <span>{chatBusy ? "执行中…" : "发送"}</span>
              </button>
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

      {tab === "diagnostics" && (
        <div className="page-scroll">
          <div style={{ maxWidth: 860, display: "flex", flexDirection: "column", gap: 14 }}>
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

            <details className="diag-card" open>
              <summary className="diag-card-summary">
                <span>记忆库概览（原始快照）</span>
                <span className="badge">JSON</span>
              </summary>
              <div className="diag-card-body">
                <pre className="tool-pre">{JSON.stringify(overview, null, 2)}</pre>
              </div>
            </details>

            <details className="diag-card">
              <summary className="diag-card-summary">
                <span>控制面诊断</span>
                <span className="badge">JSON</span>
              </summary>
              <div className="diag-card-body">
                <pre className="tool-pre">{JSON.stringify(controlPlane, null, 2)}</pre>
              </div>
            </details>

            <details className="diag-card">
              <summary className="diag-card-summary">
                <span>上下文注入账单</span>
                <span className="badge">JSON</span>
              </summary>
              <div className="diag-card-body">
                <pre className="tool-pre">{JSON.stringify(contextBill, null, 2)}</pre>
              </div>
            </details>

            <details className="diag-card">
              <summary className="diag-card-summary">
                <span>近期召回记录</span>
                <span className="badge mono">{recalls.length} 条</span>
              </summary>
              <div className="diag-card-body">
                <pre className="tool-pre">{JSON.stringify(recalls, null, 2)}</pre>
              </div>
            </details>
          </div>
        </div>
      )}
    </div>
  );
}
