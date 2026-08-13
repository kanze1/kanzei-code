// kanzei 桌面端前端逻辑(静态,无构建步骤)。
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// R-126:自加载起累积 console 错误与未捕获异常,供 ui_console 工具取样。
// 必须在最前面装:晚一步就漏掉初始化阶段的错误,而那正是最要命的一段。
const uiConsoleLog = [];
const UI_CONSOLE_MAX = 200;
function recordConsole(level, args) {
  if (uiConsoleLog.length >= UI_CONSOLE_MAX) uiConsoleLog.shift();
  uiConsoleLog.push({
    level,
    at: Date.now(),
    text: args.map((a) => (a instanceof Error ? `${a.message}\n${a.stack ?? ""}` : String(a))).join(" "),
  });
}
for (const level of ["error", "warn"]) {
  const original = console[level].bind(console);
  console[level] = (...args) => {
    recordConsole(level, args);
    original(...args);
  };
}
window.addEventListener("error", (event) => {
  recordConsole("uncaught", [event.message, event.filename ? `${event.filename}:${event.lineno}` : ""]);
});
window.addEventListener("unhandledrejection", (event) => {
  recordConsole("unhandled-rejection", [event.reason]);
});

// 事件订阅统一入口:注册失败必须可见(D-005 教训——ACL 拒绝时曾静默失联)。
const SESSION_PROGRESS_EVENTS = new Set([
  "kz:meta", "kz:status", "kz:text", "kz:reasoning",
  "kz:tool-start", "kz:tool-progress", "kz:task-progress", "kz:step",
]);
function on(event, handler) {
  listen(event, (eventPayload) => {
    const sessionId = eventPayload.payload?.sessionId;
    // R-086:kz:turn 是每轮开头必发的信号,拿它把会话状态机拨回运行中并解除
    // converged。后端一次运行可以跨多轮(排队输入 promote 后接着跑),轮末的
    // kz:done 之后会话仍在跑,只有这条能把被前一轮 idle 焊住的状态解开。
    // 必须写在下面那句非活动会话 early-return 之前,否则后台会话永远收不到。
    // R-206:状态写入唯一入口 transitionSession,不再手工复刻 6 布尔标志。
    if (event === "kz:turn" && sessionId) {
      transitionSession(sessionId, "running", {
        converged: false,
        auto_pending: false,
        local_start_pending: false,
        terminal_status: "",
      });
    }
    if (event === "kz:status" && sessionId) {
      const state = sessionState(sessionId);
      state.stage = eventPayload.payload?.stage || state.stage || "空闲";
      state.detail = eventPayload.payload?.detail || "";
    }
    // 运行事件不保证每条线路都先收到 kz:turn:并行线路可能先收到 meta、status
    // 或工具进度。任何带会话身份的实时进度都说明该线路仍在运行,否则左侧线路按钮
    // 会在实际执行时显示「空闲」,直到下一次轮询或下一轮 turn 才被纠正。
    if (sessionId && SESSION_PROGRESS_EVENTS.has(event)) {
      // stopping 是用户已发出的控制意图；晚到的进度不能把停止按钮重新翻回运行态。
      const state = sessionState(sessionId);
      if (state.phase === "stopping") return;
      // R-206:状态写入唯一入口 transitionSession,不再手工复刻 6 布尔标志。
      transitionSession(sessionId, "running", {
        converged: false,
        auto_pending: false,
        local_start_pending: false,
        terminal_status: "",
      });
    }
    // 事件流是线路状态的实时投影入口。不能等 kz:done/kz:idle 或下一次
    // process_list 轮询，否则工具执行期间线路按钮和 stop 会按轮次滞后。
    if (sessionId && typeof refreshParallelTaskProjection === "function") {
      refreshParallelTaskProjection(sessionId);
    }
    const controlEvent =
      event === "kz:ask" ||
      event === "kz:status" ||
      event === "kz:done" ||
      event === "kz:error" ||
      event === "kz:stopped" ||
      event === "kz:idle";
    if (!controlEvent && sessionId && activeSessionId && sessionId !== activeSessionId) return;
    if (controlEvent) {
      // R-086:控制事件先按 sessionId 更新对应会话状态机,再决定是否投影视图——
      // 后台会话的终态也必须收敛,不能只靠 refreshProcesses 间接拉后端
      // (事件丢失时切回会卡在错误运行态)。
      // 只有**会话级**终态才收敛:kz:done 是一轮的终点,kz:error 也可能只是本轮
      // 失败(后端随后仍会发 kz:idle),拿它们收敛会让排队输入的第二轮起全程显示空闲。
      const terminalError = event === "kz:error" && eventPayload.payload?.terminal !== false;
      if (sessionId && (event === "kz:idle" || event === "kz:stopped" || terminalError)) {
        const targetPhase =
          event === "kz:stopped" ? "stopped" : terminalError ? "failed" : sessionState(sessionId).auto_pending ? "auto_pending" : "idle";
        // R-206:状态写入唯一入口 transitionSession,不再手工复刻 6 布尔标志。
        // terminal_status 由 transitionSession 对 stopped/failed 分支折算,无需重复设置。
        transitionSession(sessionId, targetPhase, {
          stage: "空闲",
          detail: "",
        });
        if (typeof refreshParallelTaskProjection === "function") {
          refreshParallelTaskProjection(sessionId);
        }
      }
      // kz:ask 不走路由分支:它必须始终进 handler,按 sessionId 入队
      // (handler 内只在活动会话时弹窗),否则后台 ask 会被丢弃挂死(D-055 根因)。
      if (event !== "kz:ask" && sessionId && activeSessionId && sessionId !== activeSessionId) {
        // 控制事件的 UI 副作用不能串到活动线路，但所属线路的历史与自主推进必须执行。
        if (event === "kz:done" && typeof handleBackgroundSessionDone === "function") {
          handleBackgroundSessionDone(eventPayload.payload);
        }
        if ((event === "kz:stopped" || terminalError) && typeof cancelAutoContinueTimer === "function") {
          cancelAutoContinueTimer(sessionId);
        }
        if (typeof refreshConversationLists === "function") void refreshConversationLists();
        refreshProcesses();
        log(`后台会话控制事件已路由:${event} ${sessionId}`);
        return;
      }
    }
    handler(eventPayload);
  }).catch((err) => {
    log(`事件订阅失败 ${event}: ${err} — 界面将收不到运行事件,请反馈`, "err");
    $("log-panel").classList.remove("hidden");
  });
}

const $ = (id) => document.getElementById(id);
// localStorage 里的 JSON 可能被手改坏;读不出来就当没有,绝不让偏好读取抛异常
// 把整个初始化带崩。
function readJson(key, fallback) {
  try {
    const parsed = JSON.parse(localStorage.getItem(key) || "null");
    return parsed && typeof parsed === "object" ? parsed : fallback;
  } catch {
    return fallback;
  }
}
function writeJson(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* 配额满等情况:偏好丢失可以接受,不该打断当前操作 */
  }
}

// R-184 P2:子代理角色名 → 确定性强调色(0-4)。活动面板角色色点与主对话折叠组共用,
// 只做确定性映射(同一角色刷新不变),不承诺语义排序;角色名文本是主标识,颜色是辅助
// (design §2.2 不得只靠颜色区分)。
function agentRoleAccent(role) {
  if (!role) return 0;
  let sum = 0;
  for (let i = 0; i < role.length; i += 1) sum = (sum * 31 + role.charCodeAt(i)) >>> 0;
  return (sum % 4) + 1;
}

const messages = $("messages");
const promptBox = $("prompt");
