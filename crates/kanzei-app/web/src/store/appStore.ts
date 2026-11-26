import { formatError, hasDesktopRuntime, invoke } from "../ipc/client";
import type {
  AppBootstrapPayload,
  AppInfo,
  ModelCatalogError,
  ModelCatalogItem,
  OpenToolInfo,
  ProjectsPrefs,
  UiPrefs,
  WorkspaceOverview,
} from "../ipc/types";
import { hydrateComposerDrafts, loadConversationMessages } from "./chatStore";
import { createStore, useStore } from "./createStore";
import { refreshGitStatus, setDrawerContext } from "./drawerStore";
import {
  getActiveProcess,
  hydratePinnedFromPrefs,
  loadProcessesForProject,
  refreshActiveSessionMeta,
  sessionStore,
} from "./sessionStore";
import { appendLog, applyThemeDom, navigateView, pushToast, uiStore } from "./uiStore";

export interface ProjectFacts {
  git?: string;
  has_head?: boolean;
  has_git?: boolean;
  [key: string]: unknown;
}

export interface AppState {
  bootstrapped: boolean;
  bootstrapError: string | null;
  appInfo: AppInfo;
  uiPrefs: UiPrefs;
  projects: ProjectsPrefs;
  generalChatRoot: string | null;
  generalMode: boolean;
  activeProjectDir: string | null;
  projectFacts: ProjectFacts | null;
  workspaceOverview: WorkspaceOverview | null;
  modelsCatalog: ModelCatalogItem[];
  modelsErrors: ModelCatalogError[];
  modelsLoading: boolean;
  openTools: OpenToolInfo[];
  fastModelReady: boolean | null;
}

export const appStore = createStore<AppState>({
  bootstrapped: false,
  bootstrapError: null,
  appInfo: { version: "0.1.0", build: "dev" },
  uiPrefs: {},
  projects: { projects: [], current: null, names: {} },
  generalChatRoot: null,
  generalMode: false,
  activeProjectDir: null,
  projectFacts: null,
  workspaceOverview: null,
  modelsCatalog: [],
  modelsErrors: [],
  modelsLoading: false,
  openTools: [],
  fastModelReady: null,
});

export function useAppStore<S>(selector: (state: AppState) => S): S {
  return useStore(appStore, selector);
}

export function projectDisplayName(path: string | null): string {
  if (!path) return "未选择项目";
  const state = appStore.getState();
  if (state.generalMode) return "无项目对话";
  if (state.projects.names[path]) return state.projects.names[path];
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function scheduleIdle(task: () => void): void {
  if (typeof window !== "undefined" && "requestIdleCallback" in window) {
    (window as unknown as { requestIdleCallback: (cb: () => void, opts?: { timeout: number }) => void }).requestIdleCallback(
      task,
      { timeout: 600 }
    );
  } else {
    setTimeout(task, 60);
  }
}

export async function refreshWorkspaceOverview(): Promise<void> {
  try {
    const overview = await invoke<WorkspaceOverview>("workspace_overview");
    appStore.setState({ workspaceOverview: overview });
  } catch (error) {
    appendLog(`加载工作区总览失败: ${formatError(error)}`, "warn");
  }
}

let modelsInFlight: Promise<void> | null = null;

export async function loadModelsCatalog(
  projectDir?: string | null,
  force = false
): Promise<void> {
  if (modelsInFlight && !force) return modelsInFlight;
  const target = projectDir ?? appStore.getState().activeProjectDir;
  appStore.setState({ modelsLoading: true });
  const task = (async () => {
    try {
      const res = await invoke<{ models?: ModelCatalogItem[]; errors?: ModelCatalogError[] }>(
        "models_list",
        {
          projectDir: target,
          force,
        }
      );
      const errors = Array.isArray(res?.errors) ? res.errors : [];
      appStore.setState({
        modelsCatalog: Array.isArray(res?.models) ? res.models : [],
        modelsErrors: errors,
        modelsLoading: false,
      });
      for (const err of errors) {
        appendLog(`探测 ${err.provider} 模型失败: ${err.message}`, "warn");
      }
    } catch (error) {
      appStore.setState({ modelsLoading: false });
      appendLog(`探测模型清单失败: ${formatError(error)}`, "warn");
    } finally {
      modelsInFlight = null;
    }
  })();
  modelsInFlight = task;
  return task;
}

export async function loadOpenTools(): Promise<void> {
  try {
    const list = await invoke<OpenToolInfo[]>("open_tools_list");
    appStore.setState({ openTools: Array.isArray(list) ? list : [] });
  } catch {
    // 非关键后台信息
  }
}

export async function activateProcess(processId: string): Promise<void> {
  const state = sessionStore.getState();
  const target = state.processes.find((p) => p.id === processId);
  if (!target || !state.projectDir) return;
  sessionStore.setState({ activeProcessId: target.id });
  setDrawerContext(state.projectDir, target.worktree_path);
  await Promise.all([
    loadConversationMessages(state.projectDir, target.id, target.session_id),
    refreshActiveSessionMeta(),
  ]);
  if (!appStore.getState().generalMode) {
    void refreshGitStatus(state.projectDir, target.worktree_path);
  }
}

export async function selectProject(
  projectDir: string,
  preferredProcessId?: string | null
): Promise<void> {
  const prevGeneral = appStore.getState().generalMode;
  appStore.setState({
    generalMode: false,
    activeProjectDir: projectDir,
    projectFacts: null,
  });
  navigateView("chat");
  if (prevGeneral) {
    invoke("ui_prefs_set", { ui_layout: { conversation_mode: "project" } }).catch(() => {});
  }
  invoke<ProjectsPrefs>("projects_select", { path: projectDir })
    .then((prefs) => {
      if (prefs && Array.isArray(prefs.projects)) {
        appStore.setState({ projects: prefs });
      }
    })
    .catch(() => {});

  const list = await loadProcessesForProject(projectDir, preferredProcessId);
  const active = getActiveProcess() ?? list[0] ?? null;
  setDrawerContext(projectDir, active?.worktree_path ?? null);
  if (active) {
    await Promise.all([
      loadConversationMessages(projectDir, active.id, active.session_id),
      refreshActiveSessionMeta(),
    ]);
  }
  scheduleIdle(() => {
    void refreshGitStatus(projectDir, active?.worktree_path ?? null);
    invoke<ProjectFacts>("project_facts", { projectDir })
      .then((facts) => {
        if (appStore.getState().activeProjectDir === projectDir) {
          appStore.setState({ projectFacts: facts });
        }
      })
      .catch(() => {});
    void loadModelsCatalog(projectDir, false);
  });
}

export async function openGeneralChat(): Promise<void> {
  try {
    const root = await invoke<string>("general_chat_open");
    appStore.setState({
      generalChatRoot: root,
      generalMode: true,
      activeProjectDir: root,
      projectFacts: null,
    });
    navigateView("chat");
    invoke("ui_prefs_set", { ui_layout: { conversation_mode: "general" } }).catch(() => {});
    const list = await loadProcessesForProject(root);
    const active = getActiveProcess() ?? list[0] ?? null;
    setDrawerContext(root, null);
    if (active) {
      await Promise.all([
        loadConversationMessages(root, active.id, active.session_id),
        refreshActiveSessionMeta(),
      ]);
    }
    scheduleIdle(() => {
      void loadModelsCatalog(root, false);
    });
  } catch (error) {
    pushToast(`打开无项目对话失败：${formatError(error)}`, "error");
  }
}

export async function addExistingProject(): Promise<void> {
  try {
    const picked = await invoke<string | null>("projects_pick");
    if (!picked) return;
    const prefs = await invoke<ProjectsPrefs>("projects_add", { path: picked });
    appStore.setState({ projects: prefs });
    await selectProject(prefs.current ?? picked);
    void refreshWorkspaceOverview();
    pushToast("已添加项目", "ok");
  } catch (error) {
    pushToast(`添加项目失败：${formatError(error)}`, "error");
  }
}

export async function createNewProject(args: {
  path: string;
  name: string;
  gitInit: boolean;
  description?: string;
}): Promise<boolean> {
  try {
    const res = await invoke<{ prefs?: ProjectsPrefs; path?: string }>("projects_create", {
      path: args.path,
      name: args.name,
      gitInit: args.gitInit,
      description: args.description || null,
      knowledgeEnabled: true,
    });
    const prefs = res?.prefs ?? (await invoke<ProjectsPrefs>("projects_get"));
    appStore.setState({ projects: prefs });
    const target = res?.path ?? prefs.current ?? args.path;
    await selectProject(target);
    void refreshWorkspaceOverview();
    pushToast(`已创建项目 ${args.name}`, "ok");
    return true;
  } catch (error) {
    pushToast(`新建项目失败：${formatError(error)}`, "error");
    return false;
  }
}

export async function renameProject(path: string, name: string): Promise<void> {
  try {
    const prefs = await invoke<ProjectsPrefs>("projects_rename", { path, name });
    appStore.setState({ projects: prefs });
    void refreshWorkspaceOverview();
    pushToast("已重命名项目", "ok");
  } catch (error) {
    pushToast(`重命名项目失败：${formatError(error)}`, "error");
  }
}

export async function removeProject(path: string): Promise<void> {
  try {
    const prefs = await invoke<ProjectsPrefs>("projects_remove", { path });
    appStore.setState({ projects: prefs });
    if (appStore.getState().activeProjectDir === path) {
      if (prefs.current) {
        await selectProject(prefs.current);
      } else {
        appStore.setState({ activeProjectDir: null, projectFacts: null });
        navigateView("workspace");
      }
    }
    void refreshWorkspaceOverview();
    pushToast("已从列表移除项目（未删除磁盘文件）", "ok");
  } catch (error) {
    pushToast(`移除项目失败：${formatError(error)}`, "error");
  }
}

export async function reorderProjects(paths: string[]): Promise<void> {
  const prev = appStore.getState().projects;
  appStore.setState({ projects: { ...prev, projects: paths } });
  try {
    await invoke("projects_reorder", { paths });
    void refreshWorkspaceOverview();
  } catch (error) {
    appStore.setState({ projects: prev });
    pushToast(`调整项目顺序失败：${formatError(error)}`, "error");
  }
}

export async function bootstrapApp(): Promise<void> {
  if (!hasDesktopRuntime()) {
    applyThemeDom("dark");
    appStore.setState({ bootstrapped: true });
    return;
  }
  try {
    // Stage 1: 单次聚合 IPC 获取全部引导配置
    const boot = await invoke<AppBootstrapPayload>("app_bootstrap");
    const theme = boot.uiPrefs?.theme === "light" ? "light" : "dark";
    applyThemeDom(theme);
    uiStore.setState({ theme });
    hydratePinnedFromPrefs(boot.uiPrefs?.ui_layout);
    hydrateComposerDrafts(boot.composerDrafts?.items ?? []);
    if (boot.composerDrafts?.error) {
      appendLog(`部分草稿恢复失败: ${boot.composerDrafts.error}`, "warn");
    }

    appStore.setState({
      bootstrapped: true,
      bootstrapError: null,
      appInfo: boot.appInfo,
      uiPrefs: boot.uiPrefs ?? {},
      projects: boot.projects,
      generalChatRoot: boot.generalChatRoot,
    });

    // Stage 2: 首屏聚焦（仅加载当前激活目标的会话与尾窗消息）
    if (boot.target.kind === "general") {
      await openGeneralChat();
    } else if (boot.target.kind === "project" && boot.target.projectDir) {
      await selectProject(boot.target.projectDir);
    } else {
      navigateView("workspace");
    }

    // Stage 3: 空闲期延迟水合侧栏总览、外部打开工具与快速模型状态
    scheduleIdle(() => {
      void refreshWorkspaceOverview();
      void loadOpenTools();
      invoke<{ managed?: boolean; ready?: boolean }>("fast_model_status")
        .then((status) => {
          appStore.setState({
            fastModelReady: status?.managed ? Boolean(status.ready) : true,
          });
        })
        .catch(() => {});
    });
  } catch (error) {
    const msg = formatError(error);
    appStore.setState({ bootstrapped: true, bootstrapError: msg });
    appendLog(`应用引导失败: ${msg}`, "error");
    pushToast(`启动初始化失败：${msg}`, "error");
  }
}
