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
function on(event, handler) {
  listen(event, (eventPayload) => {
    const sessionId = eventPayload.payload?.sessionId;
    // R-086:kz:turn 是每轮开头必发的信号,拿它把会话状态机拨回运行中并解除
    // converged。后端一次运行可以跨多轮(排队输入 promote 后接着跑),轮末的
    // kz:done 之后会话仍在跑,只有这条能把被前一轮 idle 焊住的状态解开。
    // 必须写在下面那句非活动会话 early-return 之前,否则后台会话永远收不到。
    if (event === "kz:turn" && sessionId) {
      const state = sessionState(sessionId);
      state.running = true;
      state.converged = false;
    }
    if (event === "kz:status" && sessionId) {
      const state = sessionState(sessionId);
      state.stage = eventPayload.payload?.stage || state.stage || "空闲";
      state.detail = eventPayload.payload?.detail || "";
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
      if (sessionId && (event === "kz:idle" || event === "kz:stopped")) {
        const state = sessionState(sessionId);
        state.running = false;
        // 终态一经收敛,后续轮询的旧值(发出事件前采样的 running=true)不得把它
        // 翻回——这是"不依赖当前视图"的最后一环;下一轮的 kz:turn 才能解除。
        state.converged = true;
      }
      // kz:ask 不走路由分支:它必须始终进 handler,按 sessionId 入队
      // (handler 内只在活动会话时弹窗),否则后台 ask 会被丢弃挂死(D-055 根因)。
      if (event !== "kz:ask" && sessionId && activeSessionId && sessionId !== activeSessionId) {
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
const messages = $("messages");
const promptBox = $("prompt");
