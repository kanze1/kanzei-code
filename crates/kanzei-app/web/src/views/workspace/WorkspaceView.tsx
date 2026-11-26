import React, { useEffect } from "react";
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

export default function WorkspaceView(): React.ReactElement {
  const projects = useAppStore((s) => s.projects);
  const overview = useAppStore((s) => s.workspaceOverview);
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);

  useEffect(() => {
    void refreshWorkspaceOverview();
  }, []);

  const cards = projects.projects.map((path) => {
    const summary = overview?.projects.find((p) => p.path === path);
    const name =
      projects.names[path] ??
      summary?.name ??
      path.split(/[\\/]/).filter(Boolean).pop() ??
      path;
    return { path, name, summary };
  });

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
            管理已登记的代码项目，或随时开启无项目通用对话。
          </div>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button type="button" className="btn" onClick={() => void refreshWorkspaceOverview()}>
            刷新状态
          </button>
          <button type="button" className="btn" onClick={() => void openGeneralChat()}>
            无项目对话
          </button>
          <button type="button" className="btn" onClick={() => void addExistingProject()}>
            添加已有目录
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => uiStore.setState({ newProjectOpen: true })}
          >
            + 新建项目
          </button>
        </div>
      </div>

      <div className="page-scroll">
        {cards.length === 0 ? (
          <div className="empty-state">
            <h3>还没有登记任何项目</h3>
            <p>选择一个本地代码目录加入工作台，或者新建一个空白项目开始协作。</p>
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
                    <div>
                      <div className="project-card-name">{name}</div>
                      <div className="project-card-path">{path}</div>
                    </div>
                    {runningLines > 0 ? (
                      <span className="badge badge-accent">{runningLines} 运行中</span>
                    ) : summary?.error ? (
                      <span className="badge badge-err">不可访问</span>
                    ) : (
                      <span className="badge">空闲</span>
                    )}
                  </div>

                  {summary?.error && (
                    <div style={{ fontSize: 12, color: "var(--err)" }}>{summary.error}</div>
                  )}

                  <div style={{ display: "flex", gap: 12, fontSize: 12, color: "var(--dim)" }}>
                    <span>会话：{totalLines}</span>
                    {typeof summary?.pending_count === "number" && (
                      <span>待办：{summary.pending_count}</span>
                    )}
                  </div>

                  <div className="project-card-footer">
                    <div style={{ display: "flex", gap: 4 }}>
                      <button
                        type="button"
                        className="btn btn-ghost btn-sm"
                        onClick={() => void openExternalPath(path)}
                      >
                        打开目录
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
                      className="btn btn-primary btn-sm"
                      onClick={() => void selectProject(path)}
                    >
                      进入项目
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
