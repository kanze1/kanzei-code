import React, { useEffect, useState } from "react";
import { formatError, getCurrentWindow, invoke } from "../ipc/client";
import { projectDisplayName, useAppStore } from "../store/appStore";
import { useChatStore } from "../store/chatStore";
import { drawerStore, toggleDrawer, useDrawerStore } from "../store/drawerStore";
import { getActiveProcess, sessionStore, useSessionStore } from "../store/sessionStore";
import {
  confirmDialog,
  toggleSidebar,
  uiStore,
  useUiStore,
} from "../store/uiStore";
import { IconActivity, IconFileCode, IconSearch, IconShield, IconSidebar } from "../components/Icons";

const INTERACTIVE_SELECTOR =
  "button, a, input, select, textarea, summary, [role='button'], [data-no-drag]";

export function WindowChrome(): React.ReactElement {
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const view = useUiStore((s) => s.view);
  const sidebarOpen = useUiStore((s) => s.sidebarOpen);
  const closing = useUiStore((s) => s.closingWindow);
  const windowError = useUiStore((s) => s.windowError);
  const inboxOpen = useUiStore((s) => s.inboxOpen);
  const inboxCount = useUiStore((s) => s.globalAsks.length + s.recentCompleted.length);
  const inboxAskCount = useUiStore((s) => s.globalAsks.length);
  const drawerOpen = useDrawerStore((s) => s.open);
  const drawerMode = useDrawerStore((s) => s.mode);
  const gitChanges = useDrawerStore((s) => s.gitStatus?.changes ?? 0);
  const touchedCount = useDrawerStore((s) => s.touchedFiles.length);
  const liveSubagentsRunning = useChatStore((s) =>
    s.liveSubagents.some((sub) => sub.status === "running")
  );
  const activeProcess = useSessionStore((s) =>
    s.processes.find((p) => p.id === s.activeProcessId) ?? null
  );
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    if (!win) return;
    let unlisten: (() => void) | undefined;
    const sync = () => {
      win.isMaximized().then(setMaximized).catch(() => {});
    };
    sync();
    win
      .onResized(sync)
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      unlisten?.();
    };
  }, []);

  const titleLabel =
    view === "workspace"
      ? "工作台"
      : view === "wiki"
      ? "Repo Wiki 仓库百科"
      : view === "memory"
      ? "记忆管理"
      : view === "skills"
      ? "Skills 技能库"
      : view === "settings"
      ? "全局设置"
      : generalMode
      ? "无项目对话"
      : projectDisplayName(activeProjectDir);

  const handleMinimize = () => {
    const win = getCurrentWindow();
    if (!win) return;
    win.minimize().catch((err) => uiStore.setState({ windowError: formatError(err) }));
  };

  const handleMaximize = async () => {
    const win = getCurrentWindow();
    if (!win) return;
    try {
      await win.toggleMaximize();
      setMaximized(await win.isMaximized());
    } catch (err) {
      uiStore.setState({ windowError: formatError(err) });
    }
  };

  const handleClose = async () => {
    if (uiStore.getState().closingWindow) return;
    const runningCount = sessionStore.getState().processes.filter((p) => p.running).length;
    const editDirty = drawerStore.getState().editDirty;
    if (runningCount > 0 || editDirty) {
      const details = [
        runningCount > 0 ? `当前有 ${runningCount} 条会话正在运行，关闭窗口会优雅停止它们。` : "",
        editDirty ? "右侧文件抽屉还有未保存的修改。" : "",
      ]
        .filter(Boolean)
        .join("\n");
      const accepted = await confirmDialog({
        title: "确认退出 kanzei？",
        message: details,
        confirmLabel: "停止并退出",
        danger: true,
      });
      if (!accepted) return;
    }

    uiStore.setState({ closingWindow: true, windowError: "" });
    try {
      await invoke("app_shutdown").catch((err) => {
        uiStore.setState({ windowError: formatError(err) });
      });
      const win = getCurrentWindow();
      if (win) {
        await win.close();
      }
    } catch (err) {
      uiStore.setState({ closingWindow: false, windowError: formatError(err) });
    }
  };

  const handleMouseDown = (e: React.MouseEvent<HTMLElement>) => {
    if (e.button !== 0 || e.detail > 1) return;
    const target = e.target as HTMLElement | null;
    if (target?.closest(INTERACTIVE_SELECTOR)) return;
    getCurrentWindow()?.startDragging().catch(() => {});
  };

  const handleDoubleClick = (e: React.MouseEvent<HTMLElement>) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest(INTERACTIVE_SELECTOR)) return;
    void handleMaximize();
  };

  const currentProcess = activeProcess ?? getActiveProcess();
  const fileBadgeCount = gitChanges > 0 ? gitChanges : touchedCount;

  return (
    <header
      id="window-chrome"
      role="banner"
      onMouseDown={handleMouseDown}
      onDoubleClick={handleDoubleClick}
    >
      <div className="window-app-mark" aria-hidden="true">
        <img src="./assets/kanzei.svg" alt="" />
      </div>

      <div className="window-header-bar">
        <button
          id="rail-sidebar-toggle"
          type="button"
          className={`btn btn-ghost btn-icon${sidebarOpen ? " active" : ""}`}
          title={sidebarOpen ? "收起侧栏" : "展开侧栏"}
          aria-label={sidebarOpen ? "收起侧栏" : "展开侧栏"}
          aria-pressed={sidebarOpen}
          onClick={toggleSidebar}
        >
          <IconSidebar size={15} />
        </button>

        <div className="window-title-group">
          <span className="window-project-name" title={titleLabel}>
            {titleLabel}
          </span>
          {view === "chat" && currentProcess && (
            <>
              <span className="window-breadcrumb-sep" aria-hidden="true">/</span>
              <span className="window-session-name" title={currentProcess.label}>
                {currentProcess.label}
              </span>
              {currentProcess.running && <span className="status-dot running" title="运行中" />}
            </>
          )}
        </div>

        <div className="window-header-actions">
          <button
            type="button"
            className={`chrome-pill${inboxOpen ? " active" : ""}${inboxAskCount > 0 ? " warn" : ""}`}
            title="全局待办收件箱（跨项目权限审批与后台完工提醒）"
            onClick={() => uiStore.setState({ inboxOpen: !inboxOpen })}
          >
            <IconShield size={13} />
            <span>收件箱</span>
            {inboxCount > 0 && <span className="chrome-pill-count">{inboxCount}</span>}
          </button>

          <button
            type="button"
            className="chrome-cmd-btn"
            title="命令面板 (Ctrl+P)"
            onClick={() => uiStore.setState({ paletteOpen: true })}
          >
            <IconSearch size={13} />
            <span>命令</span>
            <kbd className="kbd">Ctrl+P</kbd>
          </button>

          {view === "chat" && !generalMode && activeProjectDir && (
            <button
              type="button"
              className={`chrome-pill${drawerOpen && (drawerMode === "files" || drawerMode === "worktrees") ? " active" : ""}`}
              title="打开文件上下文与 Diff 抽屉"
              onClick={() => toggleDrawer("files")}
            >
              <IconFileCode size={14} />
              <span>文件上下文</span>
              {fileBadgeCount > 0 && <span className="chrome-pill-count">{fileBadgeCount}</span>}
            </button>
          )}

          {view === "chat" && (
            <button
              type="button"
              className={`chrome-pill${drawerOpen && drawerMode === "activity" ? " active" : ""}`}
              title="运行活动面板"
              onClick={() => toggleDrawer("activity")}
            >
              <IconActivity size={14} />
              <span>活动</span>
              {liveSubagentsRunning && <span className="status-dot running" />}
            </button>
          )}
        </div>

        {windowError && (
          <span id="window-chrome-status" role="status" title={windowError}>
            {windowError}
          </span>
        )}
      </div>

      <div id="window-controls" aria-label="窗口控制">
        <button
          id="window-minimize"
          type="button"
          className="window-control"
          title="最小化"
          aria-label="最小化"
          disabled={closing}
          onClick={handleMinimize}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <line x1="3" y1="8.5" x2="13" y2="8.5" />
          </svg>
        </button>
        <button
          id="window-maximize"
          type="button"
          className="window-control"
          title={maximized ? "还原" : "最大化"}
          aria-label={maximized ? "还原" : "最大化"}
          disabled={closing}
          onClick={() => void handleMaximize()}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            {maximized ? (
              <>
                <rect x="4" y="5" width="7" height="7" />
                <path d="M5.5 5V3.5h7v7H11" />
              </>
            ) : (
              <rect x="3.5" y="3.5" width="9" height="9" />
            )}
          </svg>
        </button>
        <button
          id="window-close"
          type="button"
          className="window-control"
          title="关闭"
          aria-label="关闭"
          disabled={closing}
          onClick={() => void handleClose()}
        >
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M4 4l8 8M12 4l-8 8" />
          </svg>
        </button>
      </div>
    </header>
  );
}
