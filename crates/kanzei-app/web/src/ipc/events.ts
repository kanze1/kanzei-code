import { invoke, listenEvent } from "./client";
import type { PendingAskItem } from "./types";
import { appStore, refreshWorkspaceOverview } from "../store/appStore";
import {
  appendStreamingReasoning,
  appendStreamingText,
  chatStore,
  loadConversationMessages,
  onTaskProgressEvent,
  onToolEndEvent,
  onToolProgressEvent,
  onToolStartEvent,
  sendPrompt,
} from "../store/chatStore";
import { drawerStore, refreshGitStatus } from "../store/drawerStore";
import {
  getActiveProcess,
  pushPendingAsk,
  refreshActiveSessionMeta,
  removePendingAsk,
  sessionStore,
  updateProcessRuntime,
} from "../store/sessionStore";
import {
  appendLog,
  playNotificationChime,
  pushToast,
  recordBackgroundCompletion,
  refreshGlobalInbox,
  uiStore,
} from "../store/uiStore";

let installed = false;
let unlisteners: Array<() => void> = [];

function sessionIdOf(payload: Record<string, unknown>): string {
  return typeof payload.sessionId === "string" ? payload.sessionId : "";
}

export async function installEventBridge(): Promise<() => void> {
  if (installed) {
    return () => {};
  }
  installed = true;

  const regs: Array<Promise<() => void>> = [
    listenEvent<Record<string, unknown>>("kz:text", (payload) => {
      const sid = sessionIdOf(payload);
      if (typeof payload.text === "string") {
        appendStreamingText(sid, payload.text);
      }
    }),

    listenEvent<Record<string, unknown>>("kz:reasoning", (payload) => {
      const sid = sessionIdOf(payload);
      if (typeof payload.text === "string") {
        appendStreamingReasoning(sid, payload.text);
      }
    }),

    listenEvent<Record<string, unknown>>("kz:turn", (payload) => {
      const sid = sessionIdOf(payload);
      updateProcessRuntime(sid, { running: true });
      if (chatStore.getState().sessionId === sid) {
        const step = typeof payload.step === "number" ? payload.step : undefined;
        const maxSteps = typeof payload.maxSteps === "number" ? payload.maxSteps : undefined;
        chatStore.setState((prev) => ({
          ...prev,
          stageText: step && maxSteps ? `第 ${step}/${maxSteps} 步` : "思考中…",
          lastUsage: {
            input: prev.lastUsage?.input ?? 0,
            output: prev.lastUsage?.output ?? 0,
            step,
            maxSteps,
          },
        }));
      }
    }),

    listenEvent<Record<string, unknown>>("kz:status", (payload) => {
      const sid = sessionIdOf(payload);
      const stage = typeof payload.stage === "string" ? payload.stage : "";
      const detail = typeof payload.detail === "string" ? payload.detail : "";
      if (stage) updateProcessRuntime(sid, { stage });
      if (chatStore.getState().sessionId === sid) {
        chatStore.setState({
          stageText: detail ? `${stage} · ${detail}` : stage || "运行中",
        });
      }
      if (stage || detail) {
        appendLog(`[${stage || "状态"}] ${detail}`);
      }
    }),

    listenEvent<Record<string, unknown>>("kz:tool-start", (payload) => {
      const sid = sessionIdOf(payload);
      onToolStartEvent(sid, {
        id: String(payload.id ?? ""),
        name: String(payload.name ?? "tool"),
        summary: typeof payload.summary === "string" ? payload.summary : "",
        input: payload.input,
      });
    }),

    listenEvent<Record<string, unknown>>("kz:tool-progress", (payload) => {
      const sid = sessionIdOf(payload);
      onToolProgressEvent(sid, {
        id: String(payload.id ?? ""),
        chunk: typeof payload.chunk === "string" ? payload.chunk : undefined,
        text: typeof payload.text === "string" ? payload.text : undefined,
      });
    }),

    listenEvent<Record<string, unknown>>("kz:tool-end", (payload) => {
      const sid = sessionIdOf(payload);
      onToolEndEvent(sid, {
        id: String(payload.id ?? ""),
        name: typeof payload.name === "string" ? payload.name : undefined,
        ok: typeof payload.ok === "boolean" ? payload.ok : true,
        preview: typeof payload.preview === "string" ? payload.preview : undefined,
        content: typeof payload.content === "string" ? payload.content : undefined,
        durationMs: typeof payload.durationMs === "number" ? payload.durationMs : null,
        display: (payload.display as Record<string, unknown> | null) ?? null,
      });
    }),

    listenEvent<Record<string, unknown>>("kz:task-progress", (payload) => {
      const sid = sessionIdOf(payload);
      onTaskProgressEvent(sid, {
        id: typeof payload.id === "string" ? payload.id : undefined,
        text: typeof payload.text === "string" ? payload.text : undefined,
        trace: (payload.trace as {
          child_id?: string;
          phase?: string;
          agent?: string | null;
          model?: string | null;
          ok?: boolean;
        } | null) ?? null,
      });
    }),

    listenEvent<Record<string, unknown>>("kz:step", (payload) => {
      const sid = sessionIdOf(payload);
      if (chatStore.getState().sessionId === sid) {
        chatStore.setState((prev) => ({
          ...prev,
          lastUsage: {
            ...prev.lastUsage,
            input: typeof payload.input === "number" ? payload.input : prev.lastUsage?.input ?? 0,
            output: typeof payload.output === "number" ? payload.output : prev.lastUsage?.output ?? 0,
          },
        }));
      }
    }),

    listenEvent<Record<string, unknown>>("kz:stream-restart", (payload) => {
      const sid = sessionIdOf(payload);
      if (chatStore.getState().sessionId === sid) {
        chatStore.setState({ streamingText: "", streamingReasoning: "" });
      }
    }),

    listenEvent<Record<string, unknown>>("kz:compacted", (payload) => {
      const sid = sessionIdOf(payload);
      if (chatStore.getState().sessionId === sid) {
        appendLog("[上下文] 已自动压缩历史上下文");
      }
    }),

    listenEvent<Record<string, unknown>>("kz:stopping", (payload) => {
      const sid = sessionIdOf(payload);
      updateProcessRuntime(sid, { stage: "停止中" });
      if (chatStore.getState().sessionId === sid) {
        chatStore.setState({ stageText: "正在停止…" });
      }
    }),

    listenEvent<Record<string, unknown>>("kz:stopped", (payload) => {
      const sid = sessionIdOf(payload);
      updateProcessRuntime(sid, { running: false, stage: "已停止" });
      if (chatStore.getState().sessionId === sid) {
        chatStore.setState({ stageText: "已停止" });
      }
    }),

    listenEvent<Record<string, unknown>>("kz:idle", (payload) => {
      const sid = sessionIdOf(payload);
      updateProcessRuntime(sid, { running: false, stage: "空闲" });
    }),

    listenEvent<Record<string, unknown>>("kz:auto-fail", (payload) => {
      const sid = sessionIdOf(payload);
      const reason = typeof payload.reason === "string" ? payload.reason : "自动推进已暂停";
      appendLog(`[自动推进] ${reason}`, "warn");
      if (chatStore.getState().sessionId === sid) {
        pushToast(reason, "warn");
      }
    }),

    listenEvent<Record<string, unknown>>("kz:response-interrupted", (payload) => {
      const sid = sessionIdOf(payload);
      if (chatStore.getState().sessionId === sid) {
        chatStore.setState({ streamingText: "", streamingReasoning: "" });
      }
    }),

    listenEvent<Record<string, unknown>>("kz:input-received", (payload) => {
      const sid = sessionIdOf(payload);
      if (chatStore.getState().sessionId === sid) {
        void refreshActiveSessionMeta();
      }
    }),

    listenEvent<Record<string, unknown>>("kz:ask", (payload) => {
      const ask: PendingAskItem = {
        id: Number(payload.id ?? 0),
        sessionId: sessionIdOf(payload),
        source: typeof payload.source === "string" ? payload.source : undefined,
        agentId: typeof payload.agentId === "string" ? payload.agentId : null,
        kind: payload.kind === "question" ? "question" : "permission",
        action: String(payload.action ?? "permission"),
        resource: String(payload.resource ?? ""),
        question: typeof payload.question === "string" ? payload.question : undefined,
        options: Array.isArray(payload.options)
          ? payload.options.filter((v): v is string => typeof v === "string")
          : undefined,
        projectRoot: typeof payload.projectRoot === "string" ? payload.projectRoot : undefined,
      };
      pushPendingAsk(ask);
      void refreshGlobalInbox();
      if (uiStore.getState().soundEnabled) {
        playNotificationChime("ask");
      }
      const activeSid = getActiveProcess()?.session_id;
      if (ask.sessionId && activeSid && ask.sessionId !== activeSid) {
        pushToast("另一条后台会话正在等待你的权限/提问确认（可在顶栏「收件箱」直接处理）", "warn", 5000);
      }
    }),

    listenEvent<Record<string, unknown>>("kz:ask-resolved", (payload) => {
      const id = Number(payload.id ?? 0);
      if (id) removePendingAsk(id);
      void refreshGlobalInbox();
    }),

    listenEvent<Record<string, unknown>>("kz:error", (payload) => {
      const sid = sessionIdOf(payload);
      const message = typeof payload.message === "string" ? payload.message : "发生未知错误";
      appendLog(`[错误] ${message}`, "error");
      if (!sid || chatStore.getState().sessionId === sid) {
        chatStore.setState({ lastError: message, stageText: "发生错误" });
        pushToast(message, "error", 5000);
      }
    }),

    listenEvent<Record<string, unknown>>("kz:lifecycle", (payload) => {
      const sid = sessionIdOf(payload);
      const phase = typeof payload.phase === "string" ? payload.phase : "";
      const running = phase !== "" && phase !== "idle";
      updateProcessRuntime(sid, { running, runtime: payload });
    }),

    listenEvent<Record<string, unknown>>("kz:done", (payload) => {
      const sid = sessionIdOf(payload);
      updateProcessRuntime(sid, { running: false, stage: "空闲" });
      const active = getActiveProcess();
      const projectDir = sessionStore.getState().projectDir;
      if (uiStore.getState().soundEnabled) {
        playNotificationChime("done");
      }
      if (active && projectDir && active.session_id === sid) {
        const snap = chatStore.getState();
        const doneTools = snap.liveTools;
        const autoObj = payload.auto as
          | {
              type?: string;
              action?: string;
              prompt?: string;
              reason?: string;
              usedTokens?: number;
              budgetTokens?: number;
              handoff?: { summary?: string };
            }
          | undefined;
        const usageObj = payload.usage as { input?: number; output?: number } | undefined;
        chatStore.setState({
          stageText: "已完成",
          lastActionReport: {
            sessionId: sid,
            completedAt: Date.now(),
            elapsedMs:
              typeof payload.elapsedMs === "number"
                ? payload.elapsedMs
                : typeof payload.elapsed_ms === "number"
                ? payload.elapsed_ms
                : null,
            inputTokens: usageObj?.input ?? snap.lastUsage?.input ?? 0,
            outputTokens: usageObj?.output ?? snap.lastUsage?.output ?? 0,
            toolsTotal: doneTools.length,
            toolsFailed: doneTools.filter((t) => t.done && t.ok === false).length,
            toolNames: Array.from(new Set(doneTools.map((t) => t.name))),
            touchedFiles: [...drawerStore.getState().touchedFiles],
            autoReason: autoObj?.reason ?? null,
            handoffSummary:
              typeof autoObj?.handoff?.summary === "string" ? autoObj.handoff.summary : null,
          },
        });
        void loadConversationMessages(projectDir, active.id, sid);
        void refreshActiveSessionMeta();
        if (!appStore.getState().generalMode) {
          void refreshGitStatus(projectDir, active.worktree_path);
        }
        // 单进程自动推进续跑：若后端判定需要继续，则在短暂延迟后投递下一轮
        const auto = payload.auto as
          | {
              type?: string;
              action?: string;
              prompt?: string;
              reason?: string;
              usedTokens?: number;
              budgetTokens?: number;
            }
          | undefined;
        if (
          auto &&
          (auto.action === "continue" ||
            auto.action === "nudge" ||
            auto.type === "Continue" ||
            auto.type === "Nudge" ||
            auto.type === "GoalPending") &&
          typeof auto.prompt === "string" &&
          auto.prompt.trim()
        ) {
          const nextPrompt = auto.prompt;
          window.setTimeout(() => {
            const cur = sessionStore.getState().autoRun;
            if (cur.enabled && !cur.paused && getActiveProcess()?.session_id === sid) {
              void sendPrompt(nextPrompt, [], { autoRound: true });
            }
          }, 800);
        } else if (auto?.reason === "TokenBudgetExceeded") {
          const used = auto.usedTokens?.toLocaleString() ?? "-";
          const budget = auto.budgetTokens?.toLocaleString() ?? "-";
          const msg = `自主推进已触达 Token 预算上限（已用 ${used} / 上限 ${budget}），自动停止`;
          appendLog(`[预算熔断] ${msg}`, "warn");
          pushToast(msg, "warn", 6000);
        } else if (auto?.reason) {
          appendLog(`[自动推进结束] ${auto.reason}`);
        }
      } else if (sid) {
        const overview = appStore.getState().workspaceOverview;
        let matchedProject = projectDir ?? "";
        let matchedProcessId: string | undefined;
        let matchedLabel = "后台会话";
        for (const p of overview?.projects ?? []) {
          const line = (p.lines ?? []).find((l) => l.session_id === sid);
          if (line) {
            matchedProject = p.path;
            matchedProcessId = line.id;
            matchedLabel = `${p.name} / ${line.label}`;
            break;
          }
        }
        recordBackgroundCompletion({
          sessionId: sid,
          projectDir: matchedProject,
          processId: matchedProcessId,
          label: matchedLabel,
          at: Date.now(),
        });
      }
      void refreshGlobalInbox();
      void refreshWorkspaceOverview();
    }),

    listenEvent<Record<string, unknown>>("kz:workspace-invalidated", () => {
      void refreshWorkspaceOverview();
    }),

    listenEvent<Record<string, unknown>>("kz:fast-setup", (payload) => {
      if (typeof payload.text === "string") {
        appendLog(`[快速模型] ${payload.text}`);
      }
    }),

    listenEvent<Record<string, unknown>>("kz:ui-probe", (payload) => {
      const id = Number(payload.id ?? 0);
      if (!id) return;
      const snapshot = {
        view: uiStore.getState().view,
        theme: uiStore.getState().theme,
        project: appStore.getState().activeProjectDir,
        session: sessionStore.getState().activeProcessId,
        messageCount: chatStore.getState().messages.length,
      };
      invoke("ui_probe_result", { id, result: snapshot }).catch(() => {});
    }),
  ];

  unlisteners = await Promise.all(regs);
  return () => {
    for (const unlisten of unlisteners) {
      unlisten();
    }
    unlisteners = [];
    installed = false;
  };
}
