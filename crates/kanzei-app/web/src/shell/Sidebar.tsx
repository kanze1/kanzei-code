import React, { useEffect, useMemo, useState } from "react";
import { formatError, invoke } from "../ipc/client";
import type { ChatMessage, ConversationMessagePage, ProcessInfo } from "../ipc/types";
import {
  activateProcess,
  addExistingProject,
  openGeneralChat,
  projectDisplayName,
  removeProject,
  renameProject,
  reorderProjects,
  selectProject,
  useAppStore,
} from "../store/appStore";
import { openExternalPath } from "../store/drawerStore";
import {
  closeConversation,
  createConversation,
  purgeConversation,
  renameConversation,
  togglePinProcess,
  useSessionStore,
} from "../store/sessionStore";
import {
  confirmDialog,
  navigateView,
  openViewer,
  promptDialog,
  pushToast,
  setSidebarWidth,
  toggleTheme,
  uiStore,
  useUiStore,
} from "../store/uiStore";
import {
  IconChat,
  IconClose,
  IconFolder,
  IconGrid,
  IconLayers,
  IconMemory,
  IconPlus,
  IconSearch,
  IconSettings,
  IconSkills,
  IconStar,
  IconSun,
  IconTheme,
} from "../components/Icons";

interface ContextMenuState {
  x: number;
  y: number;
  kind: "project" | "session";
  projectDir: string;
  process?: ProcessInfo;
}

interface ClosedProcessSummary {
  id: string;
  session_id: string;
  ordinal?: number | null;
  title?: string | null;
  closed_at?: number | null;
}

export function Sidebar(): React.ReactElement {
  const sidebarOpen = useUiStore((s) => s.sidebarOpen);
  const sidebarWidth = useUiStore((s) => s.sidebarWidth);
  const view = useUiStore((s) => s.view);
  const theme = useUiStore((s) => s.theme);
  const projects = useAppStore((s) => s.projects);
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const workspaceOverview = useAppStore((s) => s.workspaceOverview);
  const processes = useSessionStore((s) => s.processes);
  const activeProcessId = useSessionStore((s) => s.activeProcessId);
  const pinnedByProject = useSessionStore((s) => s.pinnedByProject);
  const asks = useSessionStore((s) => s.asks);
  const [filter, setFilter] = useState("");
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const [closedOpen, setClosedOpen] = useState(false);
  const [closedList, setClosedList] = useState<ClosedProcessSummary[]>([]);

  useEffect(() => {
    setClosedOpen(false);
    setClosedList([]);
  }, [activeProjectDir, generalMode]);

  useEffect(() => {
    if (!closedOpen || !activeProjectDir || generalMode) return;
    invoke<ClosedProcessSummary[]>("process_closed_list", { projectDir: activeProjectDir })
      .then((list) => setClosedList(Array.isArray(list) ? list : []))
      .catch(() => setClosedList([]));
  }, [closedOpen, activeProjectDir, generalMode, processes.length]);

  const askCountBySession = useMemo(() => {
    const map = new Map<string, number>();
    for (const ask of asks) {
      if (!ask.sessionId) continue;
      map.set(ask.sessionId, (map.get(ask.sessionId) ?? 0) + 1);
    }
    return map;
  }, [asks]);

  const openClosedReadonly = async (item: ClosedProcessSummary) => {
    if (!activeProjectDir) return;
    try {
      const page = await invoke<ConversationMessagePage>("conversation_page_messages", {
        projectDir: activeProjectDir,
        processId: item.id,
        limit: 200,
      });
      const md = (page?.items ?? [])
        .filter((m: ChatMessage) => m.role === "user" || m.role === "assistant")
        .map((m: ChatMessage) => {
          const text = (m.parts ?? [])
            .filter((p) => p.type === "text" && typeof p.text === "string")
            .map((p) => p.text ?? "")
            .join("\n")
            .trim();
          if (!text) return "";
          return `${m.role === "user" ? "### 用户" : "### 助手"}\n\n${text}`;
        })
        .filter(Boolean)
        .join("\n\n---\n\n");
      openViewer({
        title: item.title || `已关闭对话 ${item.ordinal ?? item.id}`,
        subtitle: "只读历史记录（已关闭会话）",
        content: md || "该会话暂无可显示的文本消息。",
        kind: "markdown",
      });
    } catch (error) {
      pushToast(`读取已关闭对话失败：${formatError(error)}`, "error");
    }
  };

  const handleCreateWorktreeChat = async (dir: string) => {
    setMenu(null);
    const name = await promptDialog({
      title: "新建独立工作树并行对话",
      label: "工作树 / 分支名称（仅字母、数字、连字符，将从当前 HEAD 创建隔离分支）",
      placeholder: "例如：feat-ui-polish",
      confirmLabel: "创建并进入",
    });
    if (!name) return;
    if (activeProjectDir !== dir || generalMode) {
      await selectProject(dir);
    }
    const created = await createConversation(dir, null, name);
    if (created) {
      navigateView("chat");
      await activateProcess(created.id);
      pushToast(`已创建工作树对话 ${created.branch ?? name}`, "ok");
    }
  };

  const moveProject = (dir: string, delta: -1 | 1) => {
    setMenu(null);
    const list = [...projects.projects];
    const idx = list.indexOf(dir);
    const target = idx + delta;
    if (idx < 0 || target < 0 || target >= list.length) return;
    const [item] = list.splice(idx, 1);
    list.splice(target, 0, item);
    void reorderProjects(list);
  };

  const openContextMenu = (
    e: React.MouseEvent,
    payload: Omit<ContextMenuState, "x" | "y">
  ) => {
    e.preventDefault();
    const maxW = typeof window !== "undefined" ? window.innerWidth : 1280;
    const maxH = typeof window !== "undefined" ? window.innerHeight : 800;
    const x = Math.min(e.clientX, maxW - 196);
    const y = Math.min(e.clientY, maxH - 200);
    setMenu({ ...payload, x: Math.max(8, x), y: Math.max(8, y) });
  };

  const filteredProjects = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return projects.projects.filter((dir) => {
      if (!q) return true;
      const name = (projects.names[dir] ?? dir).toLowerCase();
      return name.includes(q) || dir.toLowerCase().includes(q);
    });
  }, [projects, filter]);

  const sortedActiveProcesses = useMemo(() => {
    if (!activeProjectDir) return [];
    const pins = new Set(pinnedByProject[activeProjectDir] ?? []);
    return [...processes].sort((a, b) => {
      const pinDiff = Number(pins.has(b.id)) - Number(pins.has(a.id));
      if (pinDiff !== 0) return pinDiff;
      const runDiff = Number(b.running) - Number(a.running);
      if (runDiff !== 0) return runDiff;
      return (b.updated_at ?? 0) - (a.updated_at ?? 0);
    });
  }, [processes, activeProjectDir, pinnedByProject]);

  const startResize = (e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = sidebarWidth;
    const onMove = (ev: MouseEvent) => setSidebarWidth(startWidth + (ev.clientX - startX));
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  const handleRenameProject = async (dir: string) => {
    setMenu(null);
    const next = await promptDialog({
      title: "重命名项目",
      label: "显示名称（不改变磁盘文件夹名）",
      initialValue: projectDisplayName(dir),
      confirmLabel: "保存",
    });
    if (next) await renameProject(dir, next);
  };

  const handleRemoveProject = async (dir: string) => {
    setMenu(null);
    const ok = await confirmDialog({
      title: "从列表移除项目？",
      message: `将从侧栏移除「${projectDisplayName(dir)}」，磁盘上的源码与历史文件不会被删除。`,
      confirmLabel: "移除",
      danger: true,
    });
    if (ok) await removeProject(dir);
  };

  const handleRenameSession = async (projectDir: string, proc: ProcessInfo) => {
    setMenu(null);
    const next = await promptDialog({
      title: "重命名对话",
      label: "留空恢复默认标题",
      initialValue: proc.title ?? proc.label,
      confirmLabel: "保存",
    });
    if (next !== null) await renameConversation(projectDir, proc.id, next);
  };

  const handlePurgeSession = async (projectDir: string, proc: ProcessInfo) => {
    setMenu(null);
    const ok = await confirmDialog({
      title: "永久删除对话？",
      message: `确定永久删除对话「${proc.label}」及其历史消息？此操作不可恢复。`,
      confirmLabel: "删除",
      danger: true,
    });
    if (ok) await purgeConversation(projectDir, proc.id);
  };

  return (
    <aside
      className={`sidebar${sidebarOpen ? "" : " collapsed"}`}
      style={{ width: sidebarWidth }}
      aria-label="项目与会话导航"
      onClick={() => menu && setMenu(null)}
    >
      <div className="sidebar-top">
        <div className="sidebar-top-row">
          <button
            type="button"
            className={`btn btn-sm${generalMode && view === "chat" ? " btn-primary" : ""}`}
            style={{ flex: 1 }}
            onClick={() => void openGeneralChat()}
            title="开启或切换到无项目通用对话"
          >
            <IconChat size={13} />
            <span>无项目对话</span>
          </button>
          <button
            type="button"
            className="btn btn-sm btn-icon"
            title="添加已有项目文件夹"
            aria-label="添加已有项目文件夹"
            onClick={() => void addExistingProject()}
          >
            <IconFolder size={13} />
          </button>
          <button
            type="button"
            className="btn btn-sm btn-icon"
            title="新建项目"
            aria-label="新建项目"
            onClick={() => uiStore.setState({ newProjectOpen: true })}
          >
            <IconPlus size={13} />
          </button>
        </div>

        <div className="sidebar-search-wrap">
          <span className="sidebar-search-icon" aria-hidden="true">
            <IconSearch size={12} />
          </span>
          <input
            className="input sidebar-search"
            placeholder="筛选项目…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            aria-label="筛选项目"
          />
          {filter && (
            <button
              type="button"
              className="sidebar-search-clear"
              onClick={() => setFilter("")}
              aria-label="清空筛选"
            >
              <IconClose size={11} />
            </button>
          )}
        </div>
      </div>

      <div className="sidebar-scroll">
        <div className="sidebar-section-header">
          <span>项目 · {filteredProjects.length}</span>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => navigateView("workspace")}
          >
            总览
          </button>
        </div>

        {filteredProjects.length === 0 ? (
          <div className="sidebar-empty-hint">
            {filter.trim() ? "没有匹配的项目名称。" : "还没有项目，点击上方按钮添加或新建。"}
          </div>
        ) : (
          filteredProjects.map((dir) => {
            const isActive = !generalMode && activeProjectDir === dir && view === "chat";
            const summary = workspaceOverview?.projects.find((p) => p.path === dir);
            const runningCount = summary?.running_lines ?? 0;
            const label = projects.names[dir] ?? dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;

            return (
              <div key={dir} className="project-node">
                <div
                  className={`project-row${isActive ? " active" : ""}`}
                  role="button"
                  tabIndex={0}
                  onClick={() => void selectProject(dir)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void selectProject(dir);
                  }}
                  onContextMenu={(e) => openContextMenu(e, { kind: "project", projectDir: dir })}
                >
                  <span className={`status-dot${runningCount > 0 ? " running" : ""}`} />
                  <span className="project-row-name" title={dir}>
                    {label}
                  </span>
                  <span className="project-row-actions">
                    <button
                      type="button"
                      className="btn btn-ghost btn-icon"
                      style={{ width: 22, height: 22 }}
                      title="在此项目新建对话"
                      aria-label={`在 ${label} 新建对话`}
                      onClick={(e) => {
                        e.stopPropagation();
                        void (async () => {
                          if (activeProjectDir !== dir || generalMode) {
                            await selectProject(dir);
                          }
                          await createConversation(dir);
                        })();
                      }}
                    >
                      <IconPlus size={12} />
                    </button>
                  </span>
                </div>

                {!generalMode && activeProjectDir === dir && sortedActiveProcesses.length > 0 && (
                  <div className="session-list">
                    {sortedActiveProcesses.map((proc) => {
                      const pinned = (pinnedByProject[dir] ?? []).includes(proc.id);
                      const selected = proc.id === activeProcessId && view === "chat";
                      const pendingAskCount = askCountBySession.get(proc.session_id) ?? 0;
                      return (
                        <button
                          key={proc.id}
                          type="button"
                          className={`session-row${selected ? " active" : ""}`}
                          onClick={() => {
                            navigateView("chat");
                            void activateProcess(proc.id);
                          }}
                          onContextMenu={(e) =>
                            openContextMenu(e, {
                              kind: "session",
                              projectDir: dir,
                              process: proc,
                            })
                          }
                        >
                          <span className={`status-dot${proc.running ? " running" : ""}`} />
                          {pinned && (
                            <span className="session-row-pin" title="已置顶">
                              <IconStar size={10} />
                            </span>
                          )}
                          <span className="session-row-label" title={proc.label}>
                            {proc.label}
                          </span>
                          {pendingAskCount > 0 && (
                            <span className="badge badge-warn" title="等待权限或提问确认">
                              待确认 {pendingAskCount}
                            </span>
                          )}
                          {proc.branch && (
                            <span className="session-row-meta" title={proc.branch}>
                              {proc.branch}
                            </span>
                          )}
                        </button>
                      );
                    })}
                    <button
                      type="button"
                      className="session-row session-row-closed-toggle"
                      onClick={() => setClosedOpen((v) => !v)}
                    >
                      <span className="field-hint">
                        {closedOpen ? "▾ 收起已关闭对话" : "▸ 查看已关闭对话"}
                      </span>
                    </button>
                    {closedOpen &&
                      (closedList.length === 0 ? (
                        <div className="field-hint" style={{ padding: "4px 10px" }}>
                          暂无已关闭的历史对话
                        </div>
                      ) : (
                        closedList.map((closed) => (
                          <button
                            key={closed.id}
                            type="button"
                            className="session-row session-row-closed"
                            onClick={() => void openClosedReadonly(closed)}
                            title="点击只读查看该已关闭对话历史"
                          >
                            <span className="session-row-label">
                              {closed.title || `已关闭对话 ${closed.ordinal ?? closed.id}`}
                            </span>
                            <span className="badge">只读</span>
                          </button>
                        ))
                      ))}
                  </div>
                )}
              </div>
            );
          })
        )}

        {generalMode && activeProjectDir && sortedActiveProcesses.length > 0 && (
          <>
            <div className="sidebar-section-header" style={{ marginTop: 8 }}>
              <span>无项目对话</span>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => void createConversation(activeProjectDir)}
              >
                <IconPlus size={11} />
                <span>新建</span>
              </button>
            </div>
            <div className="session-list session-list-flat">
              {sortedActiveProcesses.map((proc) => (
                <button
                  key={proc.id}
                  type="button"
                  className={`session-row${proc.id === activeProcessId && view === "chat" ? " active" : ""}`}
                  onClick={() => {
                    navigateView("chat");
                    void activateProcess(proc.id);
                  }}
                  onContextMenu={(e) =>
                    openContextMenu(e, {
                      kind: "session",
                      projectDir: activeProjectDir,
                      process: proc,
                    })
                  }
                >
                  <span className={`status-dot${proc.running ? " running" : ""}`} />
                  <span className="session-row-label">{proc.label}</span>
                </button>
              ))}
            </div>
          </>
        )}
      </div>

      <div id="workspace-sidebar-footer" className="workspace-sidebar-footer">
        <nav className="sidebar-nav-list" aria-label="全局功能">
          <button
            type="button"
            data-view="workspace"
            className={`sidebar-nav-btn${view === "workspace" ? " active" : ""}`}
            onClick={() => navigateView("workspace")}
          >
            <IconGrid size={14} />
            <span>工作台</span>
          </button>
          <button
            type="button"
            data-view="wiki"
            className={`sidebar-nav-btn${view === "wiki" ? " active" : ""}`}
            onClick={() => navigateView("wiki")}
          >
            <IconLayers size={14} />
            <span>仓库百科</span>
          </button>
          <button
            type="button"
            data-view="memory"
            className={`sidebar-nav-btn${view === "memory" ? " active" : ""}`}
            onClick={() => navigateView("memory")}
          >
            <IconMemory size={14} />
            <span>记忆</span>
          </button>
          <button
            type="button"
            data-view="skills"
            className={`sidebar-nav-btn${view === "skills" ? " active" : ""}`}
            onClick={() => navigateView("skills")}
          >
            <IconSkills size={14} />
            <span>Skills</span>
          </button>
          <button
            type="button"
            data-view="settings"
            className={`sidebar-nav-btn${view === "settings" ? " active" : ""}`}
            onClick={() => navigateView("settings")}
          >
            <IconSettings size={14} />
            <span>设置</span>
          </button>
        </nav>

        <div className="sidebar-footer-meta">
          <button
            type="button"
            className="sidebar-theme-btn"
            title={theme === "dark" ? "切换到浅色主题" : "切换到暗色主题"}
            aria-label="切换明暗主题"
            onClick={toggleTheme}
          >
            {theme === "dark" ? <IconSun size={13} /> : <IconTheme size={13} />}
            <span>{theme === "dark" ? "浅色外观" : "暗色外观"}</span>
          </button>
        </div>
      </div>

      <div className="sidebar-resizer" onMouseDown={startResize} aria-hidden="true" />

      {menu && (
        <div
          className="context-menu"
          style={{ left: menu.x, top: menu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          {menu.kind === "project" ? (
            <>
              <button
                type="button"
                className="context-menu-item"
                onClick={() => {
                  const dir = menu.projectDir;
                  setMenu(null);
                  void createConversation(dir);
                }}
              >
                新建对话
              </button>
              <button
                type="button"
                className="context-menu-item"
                onClick={() => void handleCreateWorktreeChat(menu.projectDir)}
              >
                新建工作树并行对话…
              </button>
              <button
                type="button"
                className="context-menu-item"
                onClick={() => {
                  const dir = menu.projectDir;
                  setMenu(null);
                  void openExternalPath(dir);
                }}
              >
                在文件管理器打开
              </button>
              <button
                type="button"
                className="context-menu-item"
                onClick={() => void handleRenameProject(menu.projectDir)}
              >
                重命名显示名
              </button>
              {projects.projects.length > 1 && (
                <>
                  <button
                    type="button"
                    className="context-menu-item"
                    disabled={projects.projects.indexOf(menu.projectDir) <= 0}
                    onClick={() => moveProject(menu.projectDir, -1)}
                  >
                    上移项目
                  </button>
                  <button
                    type="button"
                    className="context-menu-item"
                    disabled={
                      projects.projects.indexOf(menu.projectDir) >= projects.projects.length - 1
                    }
                    onClick={() => moveProject(menu.projectDir, 1)}
                  >
                    下移项目
                  </button>
                </>
              )}
              <div className="context-menu-sep" />
              <button
                type="button"
                className="context-menu-item danger"
                onClick={() => void handleRemoveProject(menu.projectDir)}
              >
                从列表移除
              </button>
            </>
          ) : menu.process ? (
            <>
              <button
                type="button"
                className="context-menu-item"
                onClick={() => {
                  const { projectDir, process } = menu;
                  setMenu(null);
                  if (process) void togglePinProcess(projectDir, process.id);
                }}
              >
                {(pinnedByProject[menu.projectDir] ?? []).includes(menu.process.id)
                  ? "取消置顶"
                  : "置顶对话"}
              </button>
              <button
                type="button"
                className="context-menu-item"
                onClick={() => void handleRenameSession(menu.projectDir, menu.process!)}
              >
                重命名
              </button>
              <button
                type="button"
                className="context-menu-item"
                onClick={() => {
                  const { projectDir, process } = menu;
                  setMenu(null);
                  if (process) void closeConversation(projectDir, process.id);
                }}
              >
                关闭对话
              </button>
              <div className="context-menu-sep" />
              <button
                type="button"
                className="context-menu-item danger"
                onClick={() => void handlePurgeSession(menu.projectDir, menu.process!)}
              >
                永久删除
              </button>
            </>
          ) : null}
        </div>
      )}
    </aside>
  );
}
