import { formatError, invoke } from "../ipc/client";
import type {
  AutoRunStatus,
  EffectiveModelView,
  PendingAskItem,
  PendingInputItem,
  ProcessInfo,
} from "../ipc/types";
import { createStore, useStore } from "./createStore";
import { pushToast } from "./uiStore";

export interface SessionState {
  projectDir: string | null;
  processes: ProcessInfo[];
  activeProcessId: string | null;
  loading: boolean;
  pinnedByProject: Record<string, string[]>;
  pendingInputs: PendingInputItem[];
  asks: PendingAskItem[];
  effectiveModel: EffectiveModelView | null;
  autoRun: AutoRunStatus;
}

export const sessionStore = createStore<SessionState>({
  projectDir: null,
  processes: [],
  activeProcessId: null,
  loading: false,
  pinnedByProject: {},
  pendingInputs: [],
  asks: [],
  effectiveModel: null,
  autoRun: { enabled: false, paused: false, goal: null },
});

export function useSessionStore<S>(selector: (state: SessionState) => S): S {
  return useStore(sessionStore, selector);
}

export function getActiveProcess(): ProcessInfo | null {
  const state = sessionStore.getState();
  return state.processes.find((p) => p.id === state.activeProcessId) ?? null;
}

export function getActiveSessionId(): string | null {
  return getActiveProcess()?.session_id ?? null;
}

export function hydratePinnedFromPrefs(uiLayout: Record<string, unknown> | undefined): void {
  const pins = uiLayout?.session_pins;
  if (pins && typeof pins === "object" && !Array.isArray(pins)) {
    const next: Record<string, string[]> = {};
    for (const [project, list] of Object.entries(pins as Record<string, unknown>)) {
      if (Array.isArray(list)) {
        next[project] = list.filter((x): x is string => typeof x === "string");
      }
    }
    sessionStore.setState({ pinnedByProject: next });
  }
}

export async function togglePinProcess(projectDir: string, processId: string): Promise<void> {
  const current = sessionStore.getState().pinnedByProject[projectDir] ?? [];
  const exists = current.includes(processId);
  const updated = exists
    ? current.filter((id) => id !== processId)
    : [processId, ...current];
  sessionStore.setState((prev) => ({
    ...prev,
    pinnedByProject: { ...prev.pinnedByProject, [projectDir]: updated },
  }));
  try {
    await invoke("ui_prefs_set", {
      ui_layout: { session_pins: { [projectDir]: updated } },
    });
  } catch (error) {
    pushToast(`更新置顶失败：${formatError(error)}`, "error");
  }
}

export async function loadProcessesForProject(
  projectDir: string,
  preferredProcessId?: string | null
): Promise<ProcessInfo[]> {
  sessionStore.setState({ projectDir, loading: true });
  try {
    const list = await invoke<ProcessInfo[]>("process_list", { projectDir });
    if (sessionStore.getState().projectDir !== projectDir) return list;
    const prevId = preferredProcessId ?? sessionStore.getState().activeProcessId;
    const selected =
      (prevId && list.find((p) => p.id === prevId)?.id) ??
      list[0]?.id ??
      null;
    sessionStore.setState({
      processes: list,
      activeProcessId: selected,
      loading: false,
    });
    return list;
  } catch (error) {
    if (sessionStore.getState().projectDir === projectDir) {
      sessionStore.setState({ loading: false });
    }
    pushToast(`读取会话列表失败：${formatError(error)}`, "error");
    return [];
  }
}

export async function refreshActiveSessionMeta(): Promise<void> {
  const state = sessionStore.getState();
  const active = getActiveProcess();
  if (!state.projectDir || !active) {
    sessionStore.setState({
      pendingInputs: [],
      effectiveModel: null,
      autoRun: { enabled: false, paused: false, goal: null },
    });
    return;
  }
  const projectDir = state.projectDir;
  const processId = active.id;
  const sessionId = active.session_id;
  const [pendingInputs, asks, effectiveModel, autoRun] = await Promise.all([
    invoke<PendingInputItem[]>("list_pending_inputs", {
      projectDir,
      processId,
    }).catch(() => []),
    invoke<PendingAskItem[]>("pending_asks_get", {
      projectDir,
      processId,
    }).catch(() => []),
    invoke<EffectiveModelView>("model_effective", {
      projectDir,
      processId,
      profile: active.profile ?? null,
    }).catch(() => null),
    invoke<AutoRunStatus>("auto_state_get", {
      sessionId,
    }).catch(() => ({ enabled: false, paused: false, goal: null })),
  ]);
  if (getActiveProcess()?.id !== processId) return;
  sessionStore.setState({
    pendingInputs: Array.isArray(pendingInputs) ? pendingInputs : [],
    asks: Array.isArray(asks) ? asks : [],
    effectiveModel,
    autoRun,
  });
}

export function updateProcessRuntime(
  sessionId: string,
  patch: Partial<Pick<ProcessInfo, "running" | "stage" | "runtime">>
): void {
  sessionStore.setState((prev) => {
    const idx = prev.processes.findIndex((p) => p.session_id === sessionId);
    if (idx < 0) return prev;
    const next = [...prev.processes];
    next[idx] = { ...next[idx], ...patch };
    return { ...prev, processes: next };
  });
}

export function pushPendingAsk(ask: PendingAskItem): void {
  sessionStore.setState((prev) => {
    if (prev.asks.some((a) => a.id === ask.id)) return prev;
    return { ...prev, asks: [...prev.asks, ask] };
  });
}

export function removePendingAsk(askId: number): void {
  sessionStore.setState((prev) => ({
    ...prev,
    asks: prev.asks.filter((a) => a.id !== askId),
  }));
}

export async function createConversation(
  projectDir: string,
  profile: string | null = null,
  worktreeName: string | null = null
): Promise<ProcessInfo | null> {
  try {
    const created = await invoke<ProcessInfo>("process_create", {
      projectDir,
      profile,
      worktreeName: worktreeName?.trim() ? worktreeName.trim() : null,
    });
    await loadProcessesForProject(projectDir, created.id);
    return created;
  } catch (error) {
    pushToast(`新建对话失败：${formatError(error)}`, "error");
    return null;
  }
}

export async function renameConversation(
  projectDir: string,
  processId: string,
  title: string
): Promise<void> {
  try {
    await invoke("process_rename", { projectDir, processId, title });
    await loadProcessesForProject(projectDir, processId);
    pushToast("已更新对话名称", "ok");
  } catch (error) {
    pushToast(`重命名失败：${formatError(error)}`, "error");
  }
}

export async function closeConversation(
  projectDir: string,
  processId: string
): Promise<void> {
  try {
    await invoke("process_close", { processId });
    await loadProcessesForProject(projectDir);
    pushToast("已关闭对话", "ok");
  } catch (error) {
    pushToast(`关闭对话失败：${formatError(error)}`, "error");
  }
}

export async function purgeConversation(
  projectDir: string,
  processId: string
): Promise<void> {
  try {
    await invoke("process_purge", { projectDir, processId });
    await loadProcessesForProject(projectDir);
    pushToast("已删除对话", "ok");
  } catch (error) {
    pushToast(`删除对话失败：${formatError(error)}`, "error");
  }
}

export async function updateProcessSettings(
  _projectDir: string,
  processId: string,
  patch: {
    model?: string | null;
    profile?: string | null;
    reasoning?: string | null;
    manualModels?: string[] | null;
    subagentMode?: string | null;
  }
): Promise<void> {
  try {
    const args: {
      processId: string;
      model?: string | null;
      profile?: string | null;
      reasoning?: string | null;
      manualModels?: string[] | null;
      subagentMode?: string | null;
    } = { processId };
    if (patch.model !== undefined) args.model = patch.model;
    if (patch.profile !== undefined) args.profile = patch.profile;
    if (patch.reasoning !== undefined) args.reasoning = patch.reasoning;
    if (patch.manualModels !== undefined) args.manualModels = patch.manualModels;
    if (patch.subagentMode !== undefined) args.subagentMode = patch.subagentMode;

    const updated = await invoke<ProcessInfo>("process_update", args);
    sessionStore.setState((prev) => ({
      ...prev,
      processes: prev.processes.map((p) => (p.id === updated.id ? updated : p)),
    }));
    await refreshActiveSessionMeta();
  } catch (error) {
    pushToast(`更新会话设置失败：${formatError(error)}`, "error");
  }
}

export async function updateAutoRun(patch: {
  enabled?: boolean;
  paused?: boolean;
  stopAfterRound?: boolean;
  goal?: string;
}): Promise<void> {
  const sessionId = getActiveSessionId();
  if (!sessionId) return;
  try {
    const res = await invoke<{ ok?: boolean; goal?: string | null }>("auto_state_update", {
      sessionId,
      enabled: patch.enabled,
      paused: patch.paused,
      stopAfterRound: patch.stopAfterRound,
      goal: patch.goal,
    });
    sessionStore.setState((prev) => ({
      ...prev,
      autoRun: {
        enabled: patch.enabled ?? prev.autoRun.enabled,
        paused: patch.paused ?? prev.autoRun.paused,
        goal: res?.goal !== undefined ? res.goal : prev.autoRun.goal,
      },
    }));
  } catch (error) {
    pushToast(`更新自动推进设置失败：${formatError(error)}`, "error");
  }
}
