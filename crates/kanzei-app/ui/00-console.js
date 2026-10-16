// R-126:自加载起累积 console 错误与未捕获异常,供 ui_console 工具取样。
// 必须在最前面装:晚一步就漏掉初始化阶段的错误,而那正是最要命的一段。
export const uiConsoleLog = [];
export const UI_CONSOLE_MAX = 200;
export function recordConsole(level, args) {
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
  // Monaco 销毁/换掉 model 时会取消它内部还在跑的异步计算,没人接的 CancelablePromise 以 Canceled 拒绝:
  // 这是它自己的取消信号而不是故障(切文件树选「不保存」时必现)——不当未捕获错误上报,也不进 ui_console。
  const reason = event.reason;
  if (reason?.name === "Canceled" && reason?.message === "Canceled") {
    event.preventDefault();
    return;
  }
  recordConsole("unhandled-rejection", [event.reason]);
});
