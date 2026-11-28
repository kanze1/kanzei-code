import React, { useEffect, useMemo, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import type { PendingAskItem, WorkspaceLineSummary } from "../../ipc/types";
import {
  addExistingProject,
  openGeneralChat,
  projectDisplayName,
  refreshWorkspaceOverview,
  removeProject,
  renameProject,
  selectProject,
  useAppStore,
} from "../../store/appStore";
import { openExternalPath } from "../../store/drawerStore";
import { createConversation, removePendingAsk } from "../../store/sessionStore";
import {
  confirmDialog,
  dismissCompletedNotice,
  navigateView,
  promptDialog,
  pushToast,
  refreshGlobalInbox,
  uiStore,
  useUiStore,
} from "../../store/uiStore";
import {
  IconBranch,
  IconChat,
  IconExternal,
  IconFolder,
  IconLayers,
  IconPlus,
  IconRefresh,
  IconSearch,
  IconShield,
} from "../../components/Icons";

type WorkspaceViewMode = "projects" | "quests";

interface QuestCardItem {
  key: string;
  projectDir: string;
  projectName: string;
  line: WorkspaceLineSummary;
  bucket: "running" | "action" | "ready" | "error";
  asks: PendingAskItem[];
  completedAt: number | null;
}

export default function WorkspaceView(): React.ReactElement {
  const projects = useAppStore((s) => s.projects);
  const overview = useAppStore((s) => s.workspaceOverview);
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const globalAsks = useUiStore((s) => s.globalAsks);
  const recentCompleted = useUiStore((s) => s.recentCompleted);
  const inboxCount = globalAsks.length + recentCompleted.length;
  const [viewMode, setViewMode] = useState<WorkspaceViewMode>("projects");
  const [query, setQuery] = useState("");
  const [busyAskId, setBusyAskId] = useState<number | null>(null);

  useEffect(() => {
    void refreshWorkspaceOverview();
    void refreshGlobalInbox();
  }, []);

  const cards = useMemo(() => {
    const q = query.trim().toLowerCase();
    return projects.projects
      .map((path) => {
        const summary = overview?.projects.find((p) => p.path === path);
        const name =
          projects.names[path] ??
          summary?.name ??
          path.split(/[\\/]/).filter(Boolean).pop() ??
          path;
        return { path, name, summary };
      })
      .filter((item) => {
        if (!q) return true;
        return item.name.toLowerCase().includes(q) || item.path.toLowerCase().includes(q);
      });
  }, [projects, overview, query]);

  const quests = useMemo<QuestCardItem[]>(() => {
    const q = query.trim().toLowerCase();
    const asksBySession = new Map<string, PendingAskItem[]>();
    for (const ask of globalAsks) {
      if (!ask.sessionId) continue;
      const list = asksBySession.get(ask.sessionId) ?? [];
      list.push(ask);
      asksBySession.set(ask.sessionId, list);
    }
    const completedBySession = new Map<string, number>();
    for (const item of recentCompleted) {
      completedBySession.set(item.sessionId, item.at);
    }

    const all: QuestCardItem[] = [];
    for (const p of overview?.projects ?? []) {
      const projectName = projects.names[p.path] ?? p.name ?? projectDisplayName(p.path);
      for (const line of p.lines ?? []) {
        const lineAsks = asksBySession.get(line.session_id) ?? [];
        const completedAt = completedBySession.get(line.session_id) ?? null;
        const stageLower = (line.stage ?? "").toLowerCase();
        const isError =
          !line.available ||
          Boolean(p.error) ||
          stageLower.includes("错误") ||
          stageLower.includes("失败") ||
          stageLower.includes("不可用");

        let bucket: QuestCardItem["bucket"] = "ready";
        if (lineAsks.length > 0) {
          bucket = "action";
        } else if (isError) {
          bucket = "error";
        } else if (line.running) {
          bucket = "running";
        } else {
          bucket = "ready";
        }

        if (q) {
          const hay = `${projectName} ${p.path} ${line.label} ${line.branch ?? ""} ${line.stage}`.toLowerCase();
          if (!hay.includes(q)) continue;
        }
        all.push({
          key: `${p.path}::${line.id}`,
          projectDir: p.path,
          projectName,
          line,
          bucket,
          asks: lineAsks,
          completedAt,
        });
      }
    }
    return all.sort((a, b) => (b.completedAt ?? b.line.updated_at ?? 0) - (a.completedAt ?? a.line.updated_at ?? 0));
  }, [overview, projects.names, globalAsks, recentCompleted, query]);

  const questBuckets = useMemo(() => {
    return {
      running: quests.filter((q) => q.bucket === "running"),
      action: quests.filter((q) => q.bucket === "action"),
      ready: quests.filter((q) => q.bucket === "ready"),
      error: quests.filter((q) => q.bucket === "error"),
    };
  }, [quests]);

  const totalRunning = useMemo(
    () => (overview?.projects ?? []).reduce((acc, p) => acc + (p.running_lines ?? 0), 0),
    [overview]
  );
  const totalLinesAll = useMemo(
    () =>
      (overview?.projects ?? []).reduce(
        (acc, p) => acc + (p.lines_total ?? p.lines?.length ?? 0),
        0
      ),
    [overview]
  );

  const handleRename = async (path: string, currentName: string) => {
    const next = await promptDialog({
      title: "重命名项目",
      label: "显示名称",
      initialValue: currentName,
      confirmLabel: "保存",
    });
    if (next) await renameProject(path, next);
  };

  const handleRemove = async (path: string, currentName: string) => {
    const ok = await confirmDialog({
      title: "从列表移除项目？",
      message: `确定移除「${currentName}」吗？不会删除磁盘目录。`,
      confirmLabel: "移除",
      danger: true,
    });
    if (ok) await removeProject(path);
  };

  const handleQuickApprove = async (ask: PendingAskItem, reply: "once" | "deny") => {
    setBusyAskId(ask.id);
    try {
      await invoke("answer_ask", { id: ask.id, reply });
      removePendingAsk(ask.id);
      await refreshGlobalInbox();
      await refreshWorkspaceOverview();
      pushToast(reply === "once" ? "已放行本次操作" : "已拒绝该请求", "ok", 1800);
    } catch (err) {
      pushToast(`处理失败：${formatError(err)}`, "error");
    } finally {
      setBusyAskId(null);
    }
  };

  const handleCreateQuestWorktree = async () => {
    const target = activeProjectDir ?? projects.projects[0];
    if (!target) {
      pushToast("请先添加或新建一个项目", "warn");
      return;
    }
    const name = await promptDialog({
      title: `在「${projectDisplayName(target)}」新建隔离并行任务`,
      label: "输入工作树任务名称（字母、数字、连字符）",
      placeholder: "例如：feat-quest-wiki",
      confirmLabel: "创建并进入",
    });
    if (!name) return;
    await selectProject(target);
    await createConversation(target, null, name.trim());
    navigateView("chat");
  };

  const openQuest = async (item: QuestCardItem) => {
    if (item.completedAt) {
      dismissCompletedNotice(item.line.session_id);
    }
    await selectProject(item.projectDir, item.line.id);
    navigateView("chat");
  };

  return (
    <div className="page-shell">
      <div className="page-header">
        <div>
          <h1 className="page-title">工作台</h1>
          <div className="page-subtitle">
            集中管理已登记的代码项目、跨项目 My Quests 全局任务状态看板与快速切换入口。
          </div>
        </div>
        <div className="page-header-actions">
          <div className="segmented" role="tablist" aria-label="工作台视图模式">
            <button
              type="button"
              role="tab"
              aria-selected={viewMode === "projects"}
              className={`segmented-btn${viewMode === "projects" ? " active" : ""}`}
              onClick={() => setViewMode("projects")}
            >
              项目视图 ({projects.projects.length})
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={viewMode === "quests"}
              className={`segmented-btn${viewMode === "quests" ? " active" : ""}`}
              onClick={() => setViewMode("quests")}
            >
              My Quests 任务看板 ({quests.length})
            </button>
          </div>
          <button
            type="button"
            className={`btn${inboxCount > 0 ? " btn-accent" : ""}`}
            onClick={() => uiStore.setState({ inboxOpen: true })}
          >
            <IconShield size={13} />
            <span>收件箱{inboxCount > 0 ? ` (${inboxCount})` : ""}</span>
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => {
              void refreshWorkspaceOverview();
              void refreshGlobalInbox();
            }}
          >
            <IconRefresh size={13} />
            <span>刷新</span>
          </button>
          <button type="button" className="btn" onClick={() => void openGeneralChat()}>
            <IconChat size={13} />
            <span>无项目对话</span>
          </button>
          {viewMode === "quests" ? (
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void handleCreateQuestWorktree()}
            >
              <IconBranch size={13} />
              <span>新建并行任务</span>
            </button>
          ) : (
            <>
              <button type="button" className="btn" onClick={() => void addExistingProject()}>
                <IconFolder size={13} />
                <span>添加已有目录</span>
              </button>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => uiStore.setState({ newProjectOpen: true })}
              >
                <IconPlus size={13} />
                <span>新建项目</span>
              </button>
            </>
          )}
        </div>
      </div>

      <div className="page-scroll">
        <div className="stats-strip">
          <div className="stat-tile">
            <span className="stat-tile-label">已登记项目</span>
            <span className="stat-tile-value">{projects.projects.length}</span>
          </div>
          <div className="stat-tile">
            <span className="stat-tile-label">运行中任务</span>
            <span className={`stat-tile-value${totalRunning > 0 ? " accent" : ""}`}>
              {totalRunning}
            </span>
          </div>
          <div className="stat-tile">
            <span className="stat-tile-label">待确认审批</span>
            <span className={`stat-tile-value${questBuckets.action.length > 0 ? " warn" : ""}`}>
              {questBuckets.action.length}
            </span>
          </div>
          <div className="stat-tile">
            <span className="stat-tile-label">累计会话数</span>
            <span className="stat-tile-value ok">{totalLinesAll}</span>
          </div>
        </div>

        {projects.projects.length > 0 && (
          <div className="view-toolbar">
            <div className="view-search-box">
              <span className="sidebar-search-icon" aria-hidden="true">
                <IconSearch size={13} />
              </span>
              <input
                className="input"
                placeholder={
                  viewMode === "quests"
                    ? "按任务名、项目名、分支或阶段筛选…"
                    : "按项目名称或路径筛选…"
                }
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                aria-label="筛选工作台项目或任务"
              />
            </div>
            <span className="field-hint">
              {viewMode === "quests"
                ? `共 ${quests.length} 条任务线`
                : `显示 ${cards.length} / ${projects.projects.length} 个项目`}
            </span>
          </div>
        )}

        {viewMode === "quests" ? (
          <div className="quests-board" aria-label="My Quests 全局任务状态看板">
            {(
              [
                {
                  id: "running",
                  title: "运行中",
                  badgeCls: "badge-accent",
                  items: questBuckets.running,
                  empty: "当前没有正在后台运行的任务",
                },
                {
                  id: "action",
                  title: "待确认",
                  badgeCls: "badge-warn",
                  items: questBuckets.action,
                  empty: "暂无等待你审批或回答的任务",
                },
                {
                  id: "ready",
                  title: "已完工 / 就绪",
                  badgeCls: "badge-ok",
                  items: questBuckets.ready,
                  empty: "暂无就绪或待验收任务",
                },
                {
                  id: "error",
                  title: "异常 / 不可用",
                  badgeCls: "badge-err",
                  items: questBuckets.error,
                  empty: "一切正常，无异常任务",
                },
              ] as const
            ).map((col) => (
              <div key={col.id} className="quest-column">
                <div className="quest-column-head">
                  <strong>{col.title}</strong>
                  <span className={`badge ${col.badgeCls}`}>{col.items.length}</span>
                </div>
                <div className="quest-column-body">
                  {col.items.length === 0 ? (
                    <div className="quest-empty">{col.empty}</div>
                  ) : (
                    col.items.map((item) => (
                      <article key={item.key} className="quest-card">
                        <div className="quest-card-top">
                          <span className="quest-card-project">{item.projectName}</span>
                          {item.completedAt && (
                            <span className="badge badge-ok">刚完工</span>
                          )}
                          {item.line.worktree_path && (
                            <span className="badge badge-info">工作树</span>
                          )}
                        </div>
                        <div className="quest-card-title">{item.line.label}</div>
                        <div className="quest-card-meta">
                          {item.line.branch && (
                            <span className="mono">
                              <IconBranch size={11} /> {item.line.branch}
                            </span>
                          )}
                          <span>{item.line.stage || "空闲"}</span>
                        </div>

                        {item.asks.length > 0 && (
                          <div className="quest-ask-box">
                            <div className="mono" style={{ fontSize: 11.5, wordBreak: "break-all" }}>
                              {item.asks[0].kind === "question"
                                ? item.asks[0].question || item.asks[0].resource
                                : `${item.asks[0].action}: ${item.asks[0].resource}`}
                            </div>
                            {item.asks[0].kind !== "question" && (
                              <div style={{ display: "flex", gap: 4, marginTop: 6 }}>
                                <button
                                  type="button"
                                  className="btn btn-sm btn-primary"
                                  disabled={busyAskId === item.asks[0].id}
                                  onClick={() => void handleQuickApprove(item.asks[0], "once")}
                                >
                                  本次允许
                                </button>
                                <button
                                  type="button"
                                  className="btn btn-ghost btn-sm"
                                  style={{ color: "var(--err)" }}
                                  disabled={busyAskId === item.asks[0].id}
                                  onClick={() => void handleQuickApprove(item.asks[0], "deny")}
                                >
                                  拒绝
                                </button>
                              </div>
                            )}
                          </div>
                        )}

                        <div className="quest-card-footer">
                          <button
                            type="button"
                            className="btn btn-sm btn-primary"
                            onClick={() => void openQuest(item)}
                          >
                            {item.bucket === "action"
                              ? "前往处理"
                              : item.completedAt
                              ? "前往验收"
                              : "进入会话"}
                          </button>
                          {item.completedAt && (
                            <button
                              type="button"
                              className="btn btn-ghost btn-sm"
                              onClick={() => dismissCompletedNotice(item.line.session_id)}
                            >
                              已读
                            </button>
                          )}
                        </div>
                      </article>
                    ))
                  )}
                </div>
              </div>
            ))}
          </div>
        ) : cards.length === 0 ? (
          <div className="empty-state">
            <div className="empty-state-icon">
              <IconFolder size={20} />
            </div>
            <h3>{projects.projects.length === 0 ? "还没有登记任何项目" : "没有匹配的项目"}</h3>
            <p>
              {projects.projects.length === 0
                ? "选择一个本地代码目录加入工作台，或者新建一个空白项目开始协作。"
                : "尝试清空搜索关键字查看全部项目。"}
            </p>
            {projects.projects.length === 0 && (
              <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                <button type="button" className="btn" onClick={() => void addExistingProject()}>
                  添加已有目录
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => uiStore.setState({ newProjectOpen: true })}
                >
                  新建项目
                </button>
              </div>
            )}
          </div>
        ) : (
          <div className="workspace-grid">
            {cards.map(({ path, name, summary }) => {
              const runningLines = summary?.running_lines ?? 0;
              const totalLines = summary?.lines_total ?? summary?.lines?.length ?? 0;
              const isCurrent = activeProjectDir === path;

              return (
                <article
                  key={path}
                  className={`project-card${isCurrent ? " current" : ""}`}
                >
                  <div className="project-card-head">
                    <div className="project-card-identity">
                      <div className="project-card-icon">
                        <IconFolder size={16} />
                      </div>
                      <div style={{ minWidth: 0 }}>
                        <div className="project-card-name">
                          <span>{name}</span>
                          {isCurrent && <span className="badge badge-accent">当前</span>}
                        </div>
                        <div className="project-card-path">{path}</div>
                      </div>
                    </div>

                    {runningLines > 0 ? (
                      <span className="badge badge-accent">
                        <span className="status-dot running" />
                        {runningLines} 运行中
                      </span>
                    ) : summary?.error ? (
                      <span className="badge badge-err">不可访问</span>
                    ) : (
                      <span className="badge">空闲</span>
                    )}
                  </div>

                  {summary?.error && (
                    <div style={{ fontSize: 12, color: "var(--err)" }}>{summary.error}</div>
                  )}

                  <div className="project-card-metrics">
                    <span>
                      会话<span className="project-card-metric-val">{totalLines}</span>
                    </span>
                    {typeof summary?.pending_count === "number" && (
                      <span>
                        待办<span className="project-card-metric-val">{summary.pending_count}</span>
                      </span>
                    )}
                  </div>

                  <div className="project-card-footer">
                    <div style={{ display: "flex", gap: 3 }}>
                      <button
                        type="button"
                        className="btn btn-ghost btn-sm"
                        onClick={() => void openExternalPath(path)}
                      >
                        <IconExternal size={11} />
                        <span>目录</span>
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-sm"
                        onClick={async () => {
                          await selectProject(path);
                          navigateView("wiki");
                        }}
                      >
                        <IconLayers size={11} />
                        <span>百科</span>
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-sm"
                        onClick={() => void handleRename(path, name)}
                      >
                        重命名
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-sm"
                        onClick={() => void handleRemove(path, name)}
                      >
                        移除
                      </button>
                    </div>
                    <button
                      type="button"
                      className={`btn btn-sm${isCurrent ? " btn-accent" : " btn-primary"}`}
                      onClick={() => void selectProject(path)}
                    >
                      {isCurrent ? "回到对话" : "进入项目"}
                    </button>
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
