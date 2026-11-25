// Windows project identity and runtime activity helpers shared by navigation and sessions.
export const pathKey = value => String(value || "").replaceAll("\\", "/").replace(/^\/\/\?\//, "").replace(/\/$/, "").toLowerCase();
export const sameProject = (a, b) => Boolean(a && b) && pathKey(a) === pathKey(b);
export const activityLabels = Object.freeze({
  idle: "空闲", starting: "启动中", running: "运行中", stopping: "停止中",
  waiting: "等待下一轮", attention: "待你回复", stopped: "已停止", failed: "运行失败", unknown: "状态未确认",
});
export function executionActivity(line, live, { waiting = false, unavailable = false } = {}) {
  const authoritative = live && (live.converged || live.running || live.auto_pending || live.local_start_pending);
  const phase = authoritative ? live.phase : unavailable ? "unknown" : line?.running ? "running" : "idle";
  const state = ["stopping", "starting"].includes(phase) ? phase : waiting ? "attention"
    : phase === "auto_pending" ? "waiting" : activityLabels[phase] ? phase : "idle";
  const stage = authoritative ? live.stage : line?.stage;
  return { state, label: state === "running" && stage && stage !== "空闲" && !/权限|permission/i.test(stage) ? stage : activityLabels[state] };
}
