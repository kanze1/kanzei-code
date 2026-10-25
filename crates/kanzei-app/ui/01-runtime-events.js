// App-level event routing and lifecycle coordination. This is not a foundation module.
import { subscribeRuntimeEvent } from "./00-event-subscriptions.js";
import { applyRuntimeSnapshot, setSessionDetails } from "./00-session-state.js";
import { t } from "./02-i18n.js";
import { beginStreamResponse, chatAbortRunningFor, commitStreamResponse, markPreviousRunErrors } from "./05-chat-render.js";
import {
  activeSessionId,
  currentProject,
  log,
  navigate_view,
  processItems,
  sessionState,
  sessionStates,
  setRunning,
  setStopping,
  toast,
  transitionSession,
  trackRunElapsed,
} from "./03-shell.js";
import { autoContinueTimers, cancelAutoContinueTimer, releaseAutoContinue, takeAwaitingUser, autoStopReason, setAutoStopReason } from "./08-auto.js";
import { handleBackgroundAutoFail, handleBackgroundSessionDone } from "./08-compose-runtime.js";
import "./08-compose.js";
import { refreshParallelTaskProjection, refreshProcesses, switchProcess } from "./09-sessions.js";
import { refreshMemory } from "./13-memory.js";
import { refreshConversationLists } from "./15-views-misc.js";
import { refreshResearch } from "./03-research-view.js";
import { neuralFlowEmit } from "./22-neural-flow.js";
import { subagentSettle } from "./05-subagents.js";
import { updateSessionStage } from "./03-session-stage.js";

import "./00-dom.js";
import "./00-ipc.js";
import { withSessionRender, renderingBackground } from "./01-chat-panes.js";

// 事件订阅统一入口:注册失败必须可见(D-005 教训——ACL 拒绝时曾静默失联)。
export const SESSION_PROGRESS_EVENTS = new Set([
  "kz:meta", "kz:status", "kz:text", "kz:reasoning",
  "kz:tool-start", "kz:hosted-tool", "kz:tool-progress", "kz:task-progress", "kz:step", "kz:permission-resolved",
]);
// 这些是全局辅助事件,不是某一条运行会话的进度投影:权限询问由自身
// 的队列归属,快速模型安装与 UI 探针也没有运行 session。
// D-381:kz:annotate-progress 是项目级批处理进度(文件标注),同样没有 session 归属。
// 它此前用裸 listen 绕过本函数,于是「没有 sessionId 就丢弃」这条纪律只覆盖了一半的
// 订阅——规则写在代码里,但只写在一条路径上。
// R-267:后台会话也要渲染的事件——它们往消息流里写东西,必须进所属会话的 pane。
// 刻意**不含** kz:status/kz:step/kz:meta/kz:tool-progress:
// 那几条改的是状态栏、轮次显示与工具进度条,都是**全局 UI**,
// 后台会话触发它们才是串线(用户会看到别的线的状态盖在当前线上)。
// UI-0926 #8:kz:task-progress 改为入列——它推进的是所属会话 pane 里的子代理卡片(按
// sessionId|id 找卡,不碰全局状态栏);此前非活动线路的子代理进度整条丢弃,切回去卡片一动不动。
export const BACKGROUND_RENDER_EVENTS = new Set([
  "kz:text",
  "kz:reasoning",
  "kz:response-interrupted",
  "kz:tool-start",
  "kz:hosted-tool",
  "kz:tool-end",
  "kz:task-progress",
  "kz:agent-job", // Child lifecycle does not revive the parent session.
  "kz:input-received",
  "kz:turn-elapsed", // Fixed durable round receipt, including background sessions.
  "kz:permission-resolved",
  "kz:compacted",
  "kz:experience",
]);
export const SESSIONLESS_EVENTS = new Set([
  "kz:native-control", // Desktop ownership spans conversations and auxiliary windows.
  "kz:schedule-run",
  "kz:ask",
  "kz:ask-resolved",
  "kz:fast-setup",
  "kz:ui-probe",
  "kz:annotate-progress",
  "kz:workspace-invalidated", // 项目摘要失效通知不属于某条会话。
  "kz:runtime-resync", // Native transport connection belongs to the desktop window.
  "kz:mobile-message", // D-387:手机消息注入桌面后刷新会话列表(全局,无运行会话)。
  // UI2-0926 #8 网页预览面板:面板状态、控制台批量、批注点选都属于窗口里那一个面板,不归哪条运行会话。
  "kz:preview-state",
  "kz:preview-console",
  "kz:preview-pick",
]);
// R-284 B3:结构化体验事件的前端归并层。旧 kz:* 事件继续由各现有 handler
// 消费；kz:experience 先按 session/topic/entity 归并到 store,再分发到表现层和工作台。
export const experienceEventIds = new Set();
export const experienceProjectionBySession = new Map();
export const pendingExperienceRefreshes = new Set();
export const pendingExperienceDeltas = new Map();
export let experienceDeltaFlushScheduled = false;
export const EXPERIENCE_NEURAL_EVENTS = new Map([
  ["run_started", "run_started"],
  ["text_delta", "assistant_streaming"],
  ["reasoning_delta", "reasoning_active"],
  ["tool_started", "tool_started"],
  ["tool_progressed", "tool_progressed"],
  ["tool_completed", "tool_completed"],
  ["run_status_changed", "run_status_changed"],
  ["stream_restarted", "stream_restarted"],
  ["usage_delta", "usage_delta"],
]);
export const EXPERIENCE_KNOWN_EVENTS = new Set([...EXPERIENCE_NEURAL_EVENTS.keys(), "task_progressed", "permission_resolved", "response_interrupted", "context_request_recorded"]);
export function rememberExperienceEvent(event) {
  if (experienceEventIds.has(event.event_id)) return false;
  experienceEventIds.add(event.event_id);
  if (experienceEventIds.size > 4096) {
    const oldest = experienceEventIds.values().next().value;
    if (oldest) experienceEventIds.delete(oldest);
  }
  const session = experienceProjectionBySession.get(event.session_id) || {
    project_id: event.project_id || "",
    topics: new Map(),
    entities: new Map(),
    facts: new Map(),
    deltas: 0,
    last_event_id: "",
  };
  if (event.project_id) session.project_id = event.project_id;
  if (event.topic_id) session.topics.set(event.topic_id, event);
  if (event.entity_id) session.entities.set(event.entity_id, event);
  if (event.class === "fact") session.facts.set(event.event_type, event);
  if (event.class === "delta") session.deltas += 1;
  session.last_event_id = event.event_id;
  experienceProjectionBySession.set(event.session_id, session);
  return true;
}
export function mergeExperienceDeltaPayload(previous, next) {
  const merged = { ...previous, ...next };
  if (typeof previous.text === "string" && typeof next.text === "string") {
    merged.text = previous.text + next.text;
  }
  return merged;
}
export function flushExperienceDeltas(sessionId = null) {
  // A fact is a barrier for its session; other sessions keep their scheduled frame.
  if (sessionId === null) experienceDeltaFlushScheduled = false;
  const queued = [];
  for (const [key, event] of pendingExperienceDeltas) {
    if (sessionId !== null && event.session_id !== sessionId) continue;
    pendingExperienceDeltas.delete(key);
    queued.push(event);
  }
  for (const event of queued) {
    if (event.session_id !== activeSessionId || renderingBackground) continue;
    if (typeof neuralFlowEmit === "function") {
      neuralFlowEmit(event.neural_event, {
        session_id: event.session_id,
        event_id: event.event_id,
        event_type: event.event_type,
        delta_count: event.delta_count,
        ...(event.payload || {}),
      });
    }
  }
}
export function scheduleExperienceDeltaFlush() {
  if (experienceDeltaFlushScheduled) return;
  experienceDeltaFlushScheduled = true;
  const flush = () => flushExperienceDeltas();
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(flush);
  else setTimeout(flush, 0);
}
export function queueExperienceDelta(event, neuralEvent) {
  const key = `${event.session_id}:${event.event_type}`;
  const queued = pendingExperienceDeltas.get(key);
  if (queued) {
    queued.payload = mergeExperienceDeltaPayload(queued.payload, event.payload || {});
    queued.event_id = event.event_id;
    queued.delta_count += 1;
  } else {
    pendingExperienceDeltas.set(key, {
      session_id: event.session_id,
      event_id: event.event_id,
      event_type: event.event_type,
      neural_event: neuralEvent,
      payload: { ...(event.payload || {}) },
      delta_count: 1,
    });
  }
  scheduleExperienceDeltaFlush();
}
export function replayExperienceFacts(facts) {
  if (!Array.isArray(facts)) return 0;
  let restored = 0;
  for (const event of facts) {
    if (!event || event.class !== "fact" || !event.event_id) continue;
    if (rememberExperienceEvent(event)) restored += 1;
  }
  return restored;
}
export function refreshExperienceWorkbench(event) {
  // B2 的 memory/research 事实使用 canonical project session,不是当前 run
  // session；它们只能刷新同项目工作台，不能触发当前会话神经流。
  if (event.class !== "fact" || typeof currentProject === "undefined" || !currentProject) return;
  if (event.project_id && event.project_id !== currentProject) return;
  const refreshKey = `${event.project_id || currentProject}:${event.event_type}`;
  if (pendingExperienceRefreshes.has(refreshKey)) return;
  pendingExperienceRefreshes.add(refreshKey);
  const refresh = event.event_type.startsWith("memory_")
    ? refreshMemory
    : event.event_type.startsWith("research_") && document.body.dataset.view === "research"
      ? refreshResearch
      : null;
  if (!refresh) {
    pendingExperienceRefreshes.delete(refreshKey);
    return;
  }
  Promise.resolve(event.event_type.startsWith("memory_") ? refresh({ force: true }) : refresh())
    .finally(() => pendingExperienceRefreshes.delete(refreshKey));
}
export function handleExperienceEvent(payload) {
  const event = payload && typeof payload === "object" ? payload : null;
  if (!event?.event_id || !event.session_id || !event.event_type) {
    log(`${t("忽略无效体验事件")}:missing_identity`, "warn");
    return;
  }
  if (event.schema_version !== 1) {
    log(`${t("忽略未知体验事件版本")}:${event.schema_version}`, "warn");
    return;
  }
  if (!rememberExperienceEvent(event)) return;
  refreshExperienceWorkbench(event);
  const neuralEvent = EXPERIENCE_NEURAL_EVENTS.get(event.event_type);
  if (!neuralEvent) {
    // Animation subscriptions are a subset of the event contract. Legal task
    // and permission facts remain in the projection without flooding the log.
    if (!EXPERIENCE_KNOWN_EVENTS.has(event.event_type) && !/^(memory|research)_/.test(event.event_type)) {
      log(`${t("未知体验事件")}:${event.event_type}`, "warn");
    }
    return;
  }
  // 高频 delta 只在一帧内合并一次；事实 store 仍逐事件记录，表现层不反向决定业务状态。
  if (event.class === "delta") {
    queueExperienceDelta(event, neuralEvent);
    return;
  }
  // withSessionRender 会让后台事件进入所属对话 pane，但不允许它驱动当前
  // 会话的 Canvas/音频；动画是表现投影，不是业务状态来源。
  if (event.session_id !== activeSessionId || renderingBackground) return;
  flushExperienceDeltas(event.session_id);
  if (typeof neuralFlowEmit === "function") {
    neuralFlowEmit(neuralEvent, {
      session_id: event.session_id,
      event_id: event.event_id,
      event_type: event.event_type,
      ...(event.payload || {}),
    });
  }
}

export function on(event, handler) {
  return subscribeRuntimeEvent(event, (eventPayload) => {
    // 旧 kz:* 事件来自 Tauri payload,保留 sessionId；R-284 结构化包络已
    // 在适配层统一为 snake_case,因此直接读取 session_id。
    const sessionId = event === "kz:experience"
      ? eventPayload.payload?.session_id
      : eventPayload.payload?.sessionId;
    // Compatibility lifecycle notifications carry the same version as the
    // supervisor receipt. A delayed old idle must not abort a new answer.
    const lifecycle = eventPayload.payload?.runtime;
    if (sessionId && lifecycle && ["kz:idle", "kz:stopping", "kz:stopped"].includes(event)) {
      const current = sessionState(sessionId).runtime;
      if (current && (current.actor_epoch !== lifecycle.actor_epoch || current.revision > lifecycle.revision)) return;
      applyRuntimeSnapshot(sessionId, lifecycle);
      if (["kz:idle", "kz:stopped"].includes(event) && lifecycle.phase !== "idle") return;
      if (event === "kz:stopping" && !["cancelling", "reaping"].includes(lifecycle.phase)) return;
    }
    // detail 字段只能经 setSessionDetails 落笔:直接赋值跳过 revision 递增与监听通知,
    // 界面会永远停在「不是 runtime_managed」的旧值上。已为真时不重复广播。
    if (sessionId && eventPayload.payload?.runtimeManaged && !sessionState(sessionId).runtime_managed)
      setSessionDetails(sessionId, { runtime_managed: true });
    // 除了权限询问(它有专门的缺省会话归属逻辑),所有运行事件都必须带 sessionId。
    // 没有身份就不能安全地投影到当前对话,宁可只留下后端持久事实也不能串线。
    if (!SESSIONLESS_EVENTS.has(event) && !sessionId) {
      log(`${t("丢弃无 session_id 的运行事件")}:${event}`, "warn");
      return;
    }
    if (event === "kz:lifecycle") { handler(eventPayload); return; }
    // The floating temporary Q&A keeps its own visible recipient even when the
    // main conversation changes. It never changes main-run state or timers.
    // R-086:kz:turn 是每轮开头必发的信号,拿它把会话状态机拨回运行中并解除
    // converged。后端一次运行可以跨多轮(排队输入 promote 后接着跑),轮末的
    // kz:done 之后会话仍在跑,只有这条能把被前一轮 idle 焊住的状态解开。
    // 必须写在下面那句非活动会话 early-return 之前,否则后台会话永远收不到。
    // R-206:状态写入唯一入口 transitionSession,不再手工复刻 6 布尔标志。
    if (event === "kz:turn" && sessionId) {
      withSessionRender(sessionId, beginStreamResponse);
      if (eventPayload.payload?.step === 1) markPreviousRunErrors(sessionId);
      if (takeAwaitingUser(sessionId) && sessionId === activeSessionId && autoStopReason === t("模型在等你回答")) setAutoStopReason("");
      transitionSession(sessionId, "running", {
        roundResult: null,
      });
    }
    if (event === "kz:done" && sessionId) withSessionRender(sessionId, commitStreamResponse);
    // 运行事件不保证每条线路都先收到 kz:turn:并行线路可能先收到 meta、status
    // 或工具进度。任何带会话身份的实时进度都说明该线路仍在运行,否则左侧线路按钮
    // 会在实际执行时显示「空闲」,直到下一次轮询或下一轮 turn 才被纠正。
    if (sessionId && SESSION_PROGRESS_EVENTS.has(event)) {
      // stopping 是用户已发出的控制意图；晚到的进度不能把停止按钮重新翻回运行态。
      const state = sessionState(sessionId);
      if (state.phase === "stopping") return;
      // 已收敛的终态同理:kz:idle 之后**迟到**的进度事件不得把会话复活。
      //
      // 复活的代价不是「按钮闪一下」而是鞭挞卡死:processRunning() 在未收敛时读
      // `state.running || item.running`,被迟到事件置回 running 之后就再没有东西
      // 能把它翻回来(只有 kz:idle 会收敛,而它已经发过了)。armAutoContinue 于是
      // 每 2 秒重试一次、满 15 次放弃,报「上一轮尚未结束」。实测现场:
      // 13:58:31 运行完成 → 13:59:03 放弃,正好 32 秒 = 首次 2s + 15×2s。
      //
      // 解除收敛的权力只留给 kz:turn(上面第一段处理,且**不在**本事件集合里)——
      // 注释写明它是「每轮开头必发的信号」,新一轮必定经它。代价是:若某一轮的
      // 进度事件早于 kz:turn 到达,线路按钮会短暂显示空闲,直到 kz:turn 或下一次
      // process_list 轮询纠正。拿这点显示延迟换掉一个会卡死自动续跑的状态陷阱。
      if (state.converged) return;
      // R-206:状态写入唯一入口 transitionSession,不再手工复刻 6 布尔标志。
      transitionSession(sessionId, "running", {
      });
    }
    if (sessionId) {
      const state = sessionState(sessionId);
      const stageFinished = ["kz:done", "kz:idle", "kz:stopped"].includes(event)
        || event === "kz:error" && eventPayload.payload?.terminal !== false;
      if (stageFinished) flushExperienceDeltas(sessionId);
      if (stageFinished || !state.converged && state.phase !== "stopping") updateSessionStage(state, event, eventPayload.payload);
    }
    // 事件流是线路状态的实时投影入口。不能等 kz:done/kz:idle 或下一次
    // process_list 轮询，否则工具执行期间线路按钮和 stop 会按轮次滞后。
    // 直接调 ESM import:它从未挂到 globalThis,旧写法的 typeof 在真机上恒为 undefined(死调用)。
    trackRunElapsed(event, eventPayload.payload);
    if (sessionId) refreshParallelTaskProjection(sessionId);
    const controlEvent =
      event === "kz:ask" || event === "kz:ask-resolved" ||
      event === "kz:status" ||
      event === "kz:done" ||
      event === "kz:error" ||
      event === "kz:stopped" ||
      event === "kz:stopping" ||
      event === "kz:idle" ||
      event === "kz:mobile-message";
    // R-267:后台会话的渲染事件不再丢弃,改为渲染进**它自己**的 pane。
    //
    // 丢弃是「全局唯一容器」时代的必然:渲染进去就串线。有了 per-session pane
    // 之后,正确做法是把渲染上下文切到所属会话——切回来时 DOM 已经是最新的,
    // 不需要快照、不需要「本轮完成后自动补齐」那句 notice。
    //
    // 只有**渲染类**事件走这条路(SESSION_PROGRESS_EVENTS + tool-end/compacted);
    // 其余非控制事件仍按原样忽略——它们改的是全局 UI(状态栏、活动面板、待办),
    // 不属于任何一条会话的消息流,后台会话触发它们才是真的串线。
    if (!controlEvent && !SESSIONLESS_EVENTS.has(event) && sessionId !== activeSessionId) {
      if (BACKGROUND_RENDER_EVENTS.has(event)) {
        withSessionRender(sessionId, () => handler(eventPayload));
      } else if (event === "kz:auto-fail") {
        // 失败退避重试是**所属线**的自主推进事实,不是渲染:整条丢掉等于后台线一断网
        // 就永久停摆。走专用后台分支(只动该线状态),不能直接调活动线那个 handler——
        // 它会把别人的重试文案写进当前线的鞭挞控制台。
        handleBackgroundAutoFail(eventPayload.payload);
      }
      return;
    }
    if (controlEvent) {
      if (sessionId && event === "kz:stopping") {
        transitionSession(sessionId, "stopping");
        cancelAutoContinueTimer(sessionId);
        releaseAutoContinue(sessionId);
      }
      if (sessionId && event === "kz:done") {
        setSessionDetails(sessionId, {
          roundResult: {
            outcome: eventPayload.payload.outcome || (eventPayload.payload.halted ? "halted" : "completed"),
            reason: eventPayload.payload.incompleteReason || "",
          },
        });
      }
      // R-086:控制事件先按 sessionId 更新对应会话状态机,再决定是否投影视图——
      // 后台会话的终态也必须收敛,不能只靠 refreshProcesses 间接拉后端
      // (事件丢失时切回会卡在错误运行态)。
      // 只有**会话级**终态才收敛:kz:done 是一轮的终点,kz:error 也可能只是本轮
      // 失败(后端随后仍会发 kz:idle),拿它们收敛会让排队输入的第二轮起全程显示空闲。
      const terminalError = event === "kz:error" && eventPayload.payload?.terminal !== false;
      // D-403 的失败退避重试是这条终态错误**自己**排上的那一枪:同一次运行失败,后端先发
      // kz:auto-fail(RetryAfterFailure,前端按 delayMs 排上重试),紧接着才发 kz:error
      // (terminal)。路由层若照常收敛成 failed 并取消定时器,刚排的重试当场被自己人掐掉:
      // 界面停在「失败重试 1/3 · 15s」,那一轮永不到来(用户 2026-08-17 报告:中途断一下网,
      // 鞭挞不再自动重试,手动发一句「继续」才恢复)。有重试在途(定时器带 retryLabel)时
      // 按 auto_pending 收敛并放过那个定时器——kz:stopped、关鞭挞、切线路各有自己的取消
      // 路径,只有「失败自己排的重试」享受这条例外。
      const retryPending = Boolean(sessionId && autoContinueTimers.get(sessionId)?.retryLabel);
      if (sessionId && (event === "kz:idle" || event === "kz:stopped" || terminalError)) {
        const targetPhase =
          event === "kz:stopped"
            ? "stopped"
            : terminalError && !retryPending
              ? "failed"
              : sessionState(sessionId).auto_pending || retryPending
                ? "auto_pending"
                : sessionState(sessionId).roundResult?.outcome === "incomplete" ? "failed"
                  : sessionState(sessionId).roundResult?.outcome === "halted" || sessionState(sessionId).phase === "stopping" ? "stopped" : "idle";
        // R-206:状态写入唯一入口 transitionSession,不再手工复刻 6 布尔标志。
        // terminal_status 由 transitionSession 对 stopped/failed 分支折算,无需重复设置。
        transitionSession(sessionId, targetPhase, {
          stage: targetPhase === "failed" && sessionState(sessionId).roundResult?.outcome === "incomplete" ? "本轮未完成" : "空闲",
          detail: targetPhase === "failed" ? sessionState(sessionId).roundResult?.reason || "" : "",
        });
        refreshParallelTaskProjection(sessionId);
      }
      // UI-0926 #8:整轮停止 / 终态出错 / 轮末仍没收到终态的子代理卡片收尾。活动线与后台线都走
      // 这里(后台线的控制事件下面就被路由走、不进 handler),停止后卡片不会一直停在「运行中」。
      const idleStopped = event === "kz:idle" && sessionId && sessionState(sessionId).phase === "stopped";
      if (sessionId && (event === "kz:stopped" || idleStopped || event === "kz:done" || terminalError)) {
        subagentSettle(sessionId, event === "kz:stopped" || idleStopped ? "cancelled" : "interrupted");
      }
      // D-387:手机消息注入桌面——刷新会话列表(消息已由后端持久化,打开会话可见)。
      if (event === "kz:mobile-message") {
        void refreshConversationLists();
        refreshProcesses();
        handleMobileMessage(eventPayload.payload);
        return;
      }
      // kz:ask 不走路由分支:它必须始终进 handler,按 sessionId 入队
      // (handler 内只在活动会话时弹窗),否则后台 ask 会被丢弃挂死(D-055 根因)。
      if (event !== "kz:ask" && event !== "kz:ask-resolved" && sessionId !== activeSessionId) {        // 控制事件的 UI 副作用不能串到活动线路，但所属线路的历史与自主推进必须执行。
        if (event === "kz:done") handleBackgroundSessionDone(eventPayload.payload);
        if (event === "kz:stopped" || idleStopped || terminalError) {
          // 后台线路的终态错误同样不得掐掉在途的失败退避重试(上面 retryPending 同源):
          // 后台线更没人看着,一次断网就永久停摆。in-flight 标记照旧释放——重试那一枪
          // 到点后才发得出去。
          if (!retryPending) cancelAutoContinueTimer(sessionId);
          releaseAutoContinue(sessionId);
          // #7:该线 pane 里还在转圈的工具行随终态收尾(标「中断」),否则切回去永远在转。
          chatAbortRunningFor(sessionId);
        }
        void refreshConversationLists();
        refreshProcesses();
        log(`${t("后台对话控制事件已路由")}:${event} ${sessionId}`);
        return;
      }
    }
    handler(eventPayload);
  });
}

// D-387:手机消息到达桌面——打开会话即见持久化消息;会话列表已由 kz:mobile-message 刷新。
// UX-138:此前只写默认收起的日志面板,桌面上完全看不出手机发来了东西。现在弹一条 toast
// (带「打开」直达该对话);不抢焦点、不切视图。
function mobileMessageTarget(sessionId) {
  const line = processItems.find((item) => item.session_id === sessionId);
  if (!line) return { line: null, where: "" };
  const named = typeof line.title === "string" ? line.title.trim() : "";
  const kind = t("对话");
  return { line, where: named || `${kind}${line.ordinal ? ` ${line.ordinal}` : ""}` };
}
export function handleMobileMessage(payload) {
  const sessionId = payload?.session_id;
  if (!sessionId) return;
  const text = String(payload?.text || "").trim();
  const { line, where } = mobileMessageTarget(sessionId);
  log(`${t("手机消息")} → ${where || sessionId}: ${text}`, "info");
  const preview = text.length > 60 ? `${text.slice(0, 60)}…` : text;
  toast(`${t("手机消息")}${where ? ` · ${where}` : ""}:${preview}`, {
    action: line ? {
      label: t("打开"),
      onClick: () => {
        navigate_view("chat");
        void switchProcess(line.id);
      },
    } : undefined,
  });
}

// D-387:订阅手机消息事件(与 SESSIONLESS_EVENTS 手动同源,冒烟校验要求 on() 调用)。
on("kz:mobile-message", (eventPayload) => {
  handleMobileMessage(eventPayload.payload);
});
// R-284 B1:结构化事件统一进入归并层；未知事件只记诊断，不得让 UI 崩溃。
on("kz:experience", (eventPayload) => {
  handleExperienceEvent(eventPayload.payload);
});

// R-260:process_list 定时轮询。01-core 事件处理与 09-sessions 的 renderProcesses
// 校正逻辑都假定「下一次 process_list 轮询」会兜底事件丢失与列表结构变化(外部创建/
// 注销进程、Tauri 事件偶发丢失),但轮询定时器从未实现——侧边栏任务列表只能靠事件
// 投影 + 用户操作刷新,事件一丢或列表结构一变就滞留到下一次手动操作。3s 一轮:
// 工具执行期间能及时看到运行状态变化;process_list 后端是内存 + stat 轻量查询,
// refreshProcesses 内部已按项目单飞去重(processRefreshInFlight),频繁调用安全。
// D-376:节律随运行态自适应。3s 是**运行中**需要的分辨率(工具执行期间要及时看到
// 状态变化);全空闲时它变成纯消耗——每 3 秒一次 IPC + 一次 renderProcesses 全量
// 重建侧栏任务列表,一天两万八千次,而这段时间里列表根本不会变。空闲降到 15s 仍然
// 保住这条兜底的本意(事件丢失、外部创建/注销进程最迟 15 秒被纠正),代价只是
// 「别的进程刚建了一条线」这种低频事件晚几秒出现在列表里。
// 实现上保留**单个** setInterval 跳拍,而不是按需重排的递归 setTimeout:后者每次
// 触发都新建一个定时器,冒烟 harness 的「排空待处理定时器」会因此自我续命(实测三条
// 断言被搅红)。跳拍方案对外只是"少调几次",定时器身份与节拍都不变。
export const PROCESS_POLL_MS = 3000;
export const PROCESS_POLL_IDLE_EVERY = 5; // 空闲时每 5 拍拉一次 = 15s
export let processPollTick = 0;
export function anySessionBusy() {
  // 取状态机而不是上一次轮询的 item.running:新一轮由 kz:turn 立刻拨到 running,
  // 不必等下一次轮询才把节律提上来。auto_pending 也算忙——鞭挞正等着下一轮。
  if (typeof sessionStates === "undefined") return false;
  for (const state of sessionStates.values()) {
    if (["starting", "running", "stopping", "auto_pending"].includes(state.phase)) return true;
  }
  return false;
}
setInterval(() => {
  // 工作台只消费项目摘要；预览项目或隐藏窗口不应顺手恢复会话。
  // 已运行任务仍由上面的会话事件路由维护状态和续跑，这里只关掉视图轮询。
  if (document.hidden || document.body?.dataset.appScope === "global"
    || document.body?.dataset.projectPreview === "true" || !currentProject || !activeSessionId) return;
  processPollTick += 1;
  if (!anySessionBusy() && processPollTick % PROCESS_POLL_IDLE_EVERY !== 0) return;
  refreshProcesses();
}, PROCESS_POLL_MS);

on("kz:lifecycle", ({ payload }) => {
  if (!payload?.sessionId || !payload.runtime) return;
  if (!applyRuntimeSnapshot(payload.sessionId, payload.runtime)) return;
  if (payload.sessionId === activeSessionId) {
    const state = sessionState(payload.sessionId);
    if (state.phase === "stopping") setStopping(state.stage);
    else setRunning(state.running, state.stage);
  }
  refreshParallelTaskProjection(payload.sessionId);
});
