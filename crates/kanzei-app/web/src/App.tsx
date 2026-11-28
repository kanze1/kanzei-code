import React, { Suspense, lazy, useEffect } from "react";
import { installEventBridge } from "./ipc/events";
import { bootstrapApp, useAppStore } from "./store/appStore";
import { useSessionStore } from "./store/sessionStore";
import { refreshGlobalInbox, useUiStore } from "./store/uiStore";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { WindowChrome } from "./shell/WindowChrome";
import { Sidebar } from "./shell/Sidebar";
import { StatusBar } from "./shell/StatusBar";
import { LogPanel } from "./shell/LogPanel";
import { InboxDrawer } from "./shell/InboxDrawer";
import { CommandPalette, GlobalShortcuts } from "./shell/CommandPalette";
import {
  ConfirmDialogHost,
  InputDialogHost,
  NewProjectDialogHost,
  ProjectModelsDialogHost,
  RewindDialogHost,
  ToastRegion,
  ViewerDialogHost,
} from "./components/Dialogs";
import { ChatView } from "./views/chat/ChatView";

const WorkspaceView = lazy(() => import("./views/workspace/WorkspaceView"));
const RepoWikiView = lazy(() => import("./views/wiki/RepoWikiView"));
const MemoryView = lazy(() => import("./views/memory/MemoryView"));
const SkillsView = lazy(() => import("./views/skills/SkillsView"));
const SettingsView = lazy(() => import("./views/settings/SettingsView"));

function ViewFallback(): React.ReactElement {
  return (
    <div className="empty-state" style={{ flex: 1 }}>
      <p>正在加载视图…</p>
    </div>
  );
}

export function App(): React.ReactElement {
  const view = useUiStore((s) => s.view);
  const bootstrapped = useAppStore((s) => s.bootstrapped);
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const activeProcessId = useSessionStore((s) => s.activeProcessId);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    void installEventBridge().then((fn) => {
      dispose = fn;
    });
    void bootstrapApp().then(() => refreshGlobalInbox());
    return () => {
      dispose?.();
    };
  }, []);

  return (
    <div className="app-shell">
      <WindowChrome />

      <div className="app-body">
        <ErrorBoundary scope="侧边导航栏" compact>
          <Sidebar />
        </ErrorBoundary>

        <main className="main-area">
          <div className="main-content-row">
            <div className="view-host">
              <ErrorBoundary
                scope="当前视图"
                resetKeys={[view, activeProjectDir, activeProcessId]}
              >
                {!bootstrapped ? (
                  <ViewFallback />
                ) : view === "chat" ? (
                  <ChatView />
                ) : (
                  <Suspense fallback={<ViewFallback />}>
                    {view === "workspace" && <WorkspaceView />}
                    {view === "wiki" && <RepoWikiView />}
                    {view === "memory" && <MemoryView />}
                    {view === "skills" && <SkillsView />}
                    {view === "settings" && <SettingsView />}
                  </Suspense>
                )}
              </ErrorBoundary>
            </div>
          </div>

          <ErrorBoundary scope="运行日志面板" compact>
            <LogPanel />
          </ErrorBoundary>
          <ErrorBoundary scope="状态栏" compact>
            <StatusBar />
          </ErrorBoundary>
        </main>
      </div>

      <GlobalShortcuts />
      <ErrorBoundary scope="全局弹窗" compact>
        <CommandPalette />
        <InboxDrawer />
        <ConfirmDialogHost />
        <InputDialogHost />
        <NewProjectDialogHost />
        <ProjectModelsDialogHost />
        <RewindDialogHost />
        <ViewerDialogHost />
      </ErrorBoundary>
      <ToastRegion />
    </div>
  );
}
