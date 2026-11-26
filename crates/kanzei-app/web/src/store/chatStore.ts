import { formatError, invoke } from "../ipc/client";
import type {
  ChatMessage,
  ComposerDraftRecord,
  ConversationMessagePage,
  LiveSubagent,
  LiveToolCall,
  PromptAttachment,
} from "../ipc/types";
import { createStore, useStore } from "./createStore";
import { noteTouchedFile } from "./drawerStore";
import { getActiveProcess, refreshActiveSessionMeta, sessionStore } from "./sessionStore";
import { appendLog, pushToast } from "./uiStore";

export interface ChatState {
  sessionId: string | null;
  projectDir: string | null;
  processId: string | null;
  messages: ChatMessage[];
  total: number;
  startIndex: number;
  hasMore: boolean;
  loading: boolean;
  loadingOlder: boolean;
  streamingText: string;
  streamingReasoning: string;
  liveTools: LiveToolCall[];
  liveSubagents: LiveSubagent[];
  stageText: string;
  lastError: string | null;
  lastUsage: { input: number; output: number; step?: number; maxSteps?: number } | null;
  autoAllow: boolean;
  deliveryMode: "steer" | "queue";
  searchOpen: boolean;
  searchQuery: string;
  drafts: Record<string, { text: string; attachments: PromptAttachment[] }>;
}

const DEFAULT_PAGE_LIMIT = 100;

export const chatStore = createStore<ChatState>({
  sessionId: null,
  projectDir: null,
  processId: null,
  messages: [],
  total: 0,
  startIndex: 0,
  hasMore: false,
  loading: false,
  loadingOlder: false,
  streamingText: "",
  streamingReasoning: "",
  liveTools: [],
  liveSubagents: [],
  stageText: "就绪",
  lastError: null,
  lastUsage: null,
  autoAllow: false,
  deliveryMode: "steer",
  searchOpen: false,
  searchQuery: "",
  drafts: {},
});

export function useChatStore<S>(selector: (state: ChatState) => S): S {
  return useStore(chatStore, selector);
}

export function draftKeyFor(projectDir: string | null, processId: string | null): string {
  return JSON.stringify([projectDir ?? "", processId ?? "", "", "main", "", ""]);
}

export function hydrateComposerDrafts(items: ComposerDraftRecord[]): void {
  const next: Record<string, { text: string; attachments: PromptAttachment[] }> = {};
  for (const row of items) {
    if (row?.key && row?.draft) {
      next[row.key] = {
        text: typeof row.draft.text === "string" ? row.draft.text : "",
        attachments: Array.isArray(row.draft.attachments) ? row.draft.attachments : [],
      };
    }
  }
  chatStore.setState({ drafts: next });
}

let draftSaveTimer: number | null = null;

export function updateDraft(
  projectDir: string | null,
  processId: string | null,
  text: string,
  attachments: PromptAttachment[]
): void {
  const key = draftKeyFor(projectDir, processId);
  chatStore.setState((prev) => ({
    ...prev,
    drafts: {
      ...prev.drafts,
      [key]: { text, attachments },
    },
  }));
  if (typeof window !== "undefined") {
    if (draftSaveTimer !== null) window.clearTimeout(draftSaveTimer);
    draftSaveTimer = window.setTimeout(() => {
      const empty = !text.trim() && attachments.length === 0;
      invoke("composer_draft_save", {
        key,
        draft: empty ? null : { text, attachments },
      }).catch(() => {});
    }, 350);
  }
}

export async function loadConversationMessages(
  projectDir: string,
  processId: string | null,
  sessionId: string
): Promise<void> {
  chatStore.setState({
    projectDir,
    processId,
    sessionId,
    loading: true,
    loadingOlder: false,
    streamingText: "",
    streamingReasoning: "",
    liveTools: [],
    liveSubagents: [],
    lastError: null,
  });
  try {
    const page = await invoke<ConversationMessagePage>("conversation_page_messages", {
      projectDir,
      processId,
      limit: DEFAULT_PAGE_LIMIT,
    });
    if (chatStore.getState().sessionId !== sessionId) return;
    chatStore.setState({
      messages: page.items ?? [],
      total: page.total ?? 0,
      startIndex: page.startIndex ?? 0,
      hasMore: Boolean(page.hasMore),
      loading: false,
    });
  } catch (error) {
    if (chatStore.getState().sessionId !== sessionId) return;
    const msg = formatError(error);
    chatStore.setState({ loading: false, lastError: msg });
    appendLog(`加载对话历史失败: ${msg}`, "error");
  }
}

export async function loadOlderMessages(): Promise<number> {
  const state = chatStore.getState();
  if (!state.projectDir || !state.hasMore || state.loadingOlder || state.startIndex <= 0) {
    return 0;
  }
  const sessionId = state.sessionId;
  chatStore.setState({ loadingOlder: true });
  try {
    const page = await invoke<ConversationMessagePage>("conversation_page_messages", {
      projectDir: state.projectDir,
      processId: state.processId,
      beforeIndex: state.startIndex,
      limit: DEFAULT_PAGE_LIMIT,
    });
    if (chatStore.getState().sessionId !== sessionId) return 0;
    const added = page.items?.length ?? 0;
    chatStore.setState((prev) => ({
      ...prev,
      messages: [...(page.items ?? []), ...prev.messages],
      startIndex: page.startIndex ?? 0,
      hasMore: Boolean(page.hasMore),
      loadingOlder: false,
    }));
    return added;
  } catch (error) {
    chatStore.setState({ loadingOlder: false });
    pushToast(`加载更早消息失败：${formatError(error)}`, "error");
    return 0;
  }
}

export function appendStreamingText(sessionId: string, chunk: string): void {
  if (chatStore.getState().sessionId !== sessionId || !chunk) return;
  chatStore.setState((prev) => ({
    ...prev,
    streamingText: prev.streamingText + chunk,
  }));
}

export function appendStreamingReasoning(sessionId: string, chunk: string): void {
  if (chatStore.getState().sessionId !== sessionId || !chunk) return;
  chatStore.setState((prev) => ({
    ...prev,
    streamingReasoning: prev.streamingReasoning + chunk,
  }));
}

export function onToolStartEvent(
  sessionId: string,
  payload: { id: string; name: string; summary?: string; input?: unknown }
): void {
  if (chatStore.getState().sessionId !== sessionId) return;
  if (payload.input && typeof payload.input === "object") {
    const input = payload.input as Record<string, unknown>;
    const filePath = input.path ?? input.file_path ?? input.target_path;
    if (typeof filePath === "string") noteTouchedFile(filePath);
  }
  chatStore.setState((prev) => ({
    ...prev,
    liveTools: [
      ...prev.liveTools.filter((t) => t.id !== payload.id),
      {
        id: payload.id,
        name: payload.name,
        summary: payload.summary ?? "",
        input: payload.input,
        done: false,
      },
    ],
  }));
}

export function onToolProgressEvent(
  sessionId: string,
  payload: { id: string; chunk?: string; text?: string }
): void {
  if (chatStore.getState().sessionId !== sessionId) return;
  const piece = payload.chunk ?? payload.text ?? "";
  if (!piece) return;
  chatStore.setState((prev) => ({
    ...prev,
    liveTools: prev.liveTools.map((t) =>
      t.id === payload.id ? { ...t, output: (t.output ?? "") + piece } : t
    ),
  }));
}

export function onToolEndEvent(
  sessionId: string,
  payload: {
    id: string;
    name?: string;
    ok?: boolean;
    preview?: string;
    content?: string;
    durationMs?: number | null;
    display?: Record<string, unknown> | null;
  }
): void {
  if (chatStore.getState().sessionId !== sessionId) return;
  chatStore.setState((prev) => ({
    ...prev,
    liveTools: prev.liveTools.map((t) =>
      t.id === payload.id
        ? {
            ...t,
            done: true,
            ok: payload.ok ?? true,
            preview: payload.preview ?? t.preview,
            output: payload.content ?? t.output,
            durationMs: payload.durationMs,
            display: payload.display,
          }
        : t
    ),
  }));
}

export function onTaskProgressEvent(
  sessionId: string,
  payload: {
    id?: string;
    text?: string;
    trace?: {
      child_id?: string;
      phase?: string;
      agent?: string | null;
      model?: string | null;
      ok?: boolean;
    } | null;
  }
): void {
  if (chatStore.getState().sessionId !== sessionId) return;
  const id = payload.id ?? payload.trace?.child_id ?? "subagent";
  const line = payload.text ?? "";
  const phase = payload.trace?.phase ?? "";
  chatStore.setState((prev) => {
    const existing = prev.liveSubagents.find((s) => s.id === id);
    const status: LiveSubagent["status"] =
      phase === "end" || phase === "done"
        ? payload.trace?.ok === false
          ? "error"
          : "done"
        : "running";
    if (!existing) {
      return {
        ...prev,
        liveSubagents: [
          ...prev.liveSubagents,
          {
            id,
            label: payload.trace?.agent ?? id,
            agent: payload.trace?.agent,
            model: payload.trace?.model,
            status,
            progress: line ? [line] : [],
            startedAt: Date.now(),
          },
        ],
      };
    }
    return {
      ...prev,
      liveSubagents: prev.liveSubagents.map((s) =>
        s.id === id
          ? {
              ...s,
              agent: payload.trace?.agent ?? s.agent,
              model: payload.trace?.model ?? s.model,
              status,
              progress: line ? [...s.progress.slice(-24), line] : s.progress,
            }
          : s
      ),
    };
  });
}

export async function sendPrompt(
  prompt: string,
  attachments: PromptAttachment[] = [],
  options?: { autoRound?: boolean }
): Promise<boolean> {
  const active = getActiveProcess();
  const projectDir = sessionStore.getState().projectDir;
  if (!projectDir || !active) {
    pushToast("请先选择项目或无项目对话", "warn");
    return false;
  }
  const trimmed = prompt.trim();
  if (!trimmed && attachments.length === 0) return false;
  const chat = chatStore.getState();
  const auto = sessionStore.getState().autoRun;
  const delivery = active.running ? chat.deliveryMode : "queue";

  chatStore.setState({ lastError: null });
  if (!active.running) {
    const optimisticParts: ChatMessage["parts"] = [];
    if (trimmed) optimisticParts.push({ type: "text", text: trimmed });
    for (const att of attachments) {
      optimisticParts.push({
        type: att.media_type.startsWith("image/") ? "image" : "document",
        media_type: att.media_type,
        data: att.data,
        file_name: att.file_name,
      });
    }
    chatStore.setState((prev) => ({
      ...prev,
      messages: [
        ...prev.messages,
        { role: "user", origin: "human", parts: optimisticParts },
      ],
      streamingText: "",
      streamingReasoning: "",
      liveTools: [],
      liveSubagents: [],
      stageText: "正在发送…",
    }));
  }

  try {
    await invoke("run_prompt", {
      projectDir,
      prompt: trimmed,
      processId: active.id,
      profile: active.profile,
      model: active.model,
      autonomous: auto.enabled && !auto.paused,
      autoAllow: chat.autoAllow,
      autoRound: options?.autoRound ?? false,
      delivery,
      attachments: attachments.length > 0 ? attachments : null,
    });
    updateDraft(projectDir, active.id, "", []);
    await refreshActiveSessionMeta();
    return true;
  } catch (error) {
    const msg = formatError(error);
    chatStore.setState({ lastError: msg, stageText: "发送失败" });
    pushToast(`发送失败：${msg}`, "error");
    return false;
  }
}

export async function stopCurrentRun(): Promise<void> {
  const active = getActiveProcess();
  const projectDir = sessionStore.getState().projectDir;
  if (!projectDir || !active) return;
  try {
    await invoke("stop_run", { projectDir, processId: active.id });
    chatStore.setState({ stageText: "正在停止…" });
  } catch (error) {
    pushToast(`停止失败：${formatError(error)}`, "error");
  }
}

export async function cancelPendingInput(inputId: string): Promise<void> {
  const active = getActiveProcess();
  const projectDir = sessionStore.getState().projectDir;
  if (!projectDir || !active) return;
  try {
    await invoke("cancel_input", {
      projectDir,
      processId: active.id,
      inputId,
    });
    await refreshActiveSessionMeta();
  } catch (error) {
    pushToast(`撤销排队失败：${formatError(error)}`, "error");
  }
}

export async function clearCurrentConversation(): Promise<void> {
  const active = getActiveProcess();
  const projectDir = sessionStore.getState().projectDir;
  if (!projectDir || !active) return;
  try {
    await invoke("conversation_clear", { projectDir, processId: active.id });
    await loadConversationMessages(projectDir, active.id, active.session_id);
    pushToast("已开启新对话段", "ok");
  } catch (error) {
    pushToast(`清空对话失败：${formatError(error)}`, "error");
  }
}

export async function compactCurrentConversation(): Promise<void> {
  const active = getActiveProcess();
  const projectDir = sessionStore.getState().projectDir;
  if (!projectDir || !active) return;
  try {
    await invoke("conversation_compact", { projectDir, processId: active.id });
    await loadConversationMessages(projectDir, active.id, active.session_id);
    pushToast("已完成上下文压缩", "ok");
  } catch (error) {
    pushToast(`压缩失败：${formatError(error)}`, "error");
  }
}

export async function performConversationAction(
  action: "rewind" | "fork",
  text: string
): Promise<void> {
  const active = getActiveProcess();
  const projectDir = sessionStore.getState().projectDir;
  if (!projectDir || !active) return;
  try {
    await invoke("conversation_action", {
      projectDir,
      processId: active.id,
      action,
      text,
      occurrenceFromEnd: 1,
    });
    await loadConversationMessages(projectDir, active.id, active.session_id);
    pushToast(action === "rewind" ? "已回退到此节点" : "已分叉出新对话", "ok");
  } catch (error) {
    pushToast(`操作失败：${formatError(error)}`, "error");
  }
}
