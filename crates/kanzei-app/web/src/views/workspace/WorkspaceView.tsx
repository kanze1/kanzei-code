import React, { useEffect, useMemo, useState } from "react";
import {
  addExistingProject,
  openGeneralChat,
  refreshWorkspaceOverview,
  removeProject,
  renameProject,
  selectProject,
  useAppStore,
} from "../../store/appStore";
import { openExternalPath } from "../../store/drawerStore";
import { confirmDialog, promptDialog, uiStore } from "../../store/uiStore";
import {
  IconChat,
  IconExternal,
  IconFolder,
  IconPlus,
  IconRefresh,
  IconSearch,
} from "../../components/Icons";

export default function WorkspaceView(): React.ReactElement {
  const projects = useAppStore((s) => s.projects);
  const overview = useAppStore((s) => s.workspaceOverview);
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const [query, setQuery] = useState("");

  useEffect(() => {
    void refreshWorkspaceOverview();
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

  return (
    <div className="page-shell">
      <div className="page-header">
        <div>
          <h1 className="page-title">工作台</h1>
          <div className="page-subtitle">
            集中管理已登记的代码项目、会话运行态与快速切换入口。
          </div>
        </div>
        <div className="page-header-actions">
          <button type="button" className="btn" onClick={() => void refreshWorkspaceOverview()}>
            <IconRefresh size={13} />
            <span>刷新</span>
          </button>
          <button type="button" className="btn" onClick={() => void openGeneralChat()}>
            <IconChat size={13} />
            <span>无项目对话</span>
          </button>
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
        </div>
      </div>

      <div className="page-scroll">
        <div className="stats-strip">
          <div className="stat-tile">
            <span className="stat-tile-label">已登记项目</span>
            <span className="stat-tile-value">{projects.projects.length}</span>
          </div>
          <div className="stat-tile">
            <span className="stat-tile-label">运行中会话</span>
            <span className={`stat-tile-value${totalRunning > 0 ? " accent" : ""}`}>
              {totalRunning}
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
                placeholder="按项目名称或路径筛选…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                aria-label="筛选工作台项目"
              />
            </div>
            <span className="field-hint">
              显示 {cards.length} / {projects.projects.length} 个项目
            </span>
          </div>
        )}

        {cards.length === 0 ? (
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
