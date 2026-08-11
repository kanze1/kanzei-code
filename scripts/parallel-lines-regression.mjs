// 并行线路回归护栏：锁住刷新、切换和设置持久化的关键竞态修复。
// 具体 DOM/IPC 行为由 ui-runtime-smoke 覆盖；这里专门防止后续改动把已修复的
// 高频轮询、无序切换或全局 profile 回写重新引入。
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const readUi = (name) => readFile(resolve(root, "crates", "kanzei-app", "ui", name), "utf8");
const [lines, sessions, compose, views] = await Promise.all([
  readUi("20-lines.js"),
  readUi("09-sessions.js"),
  readUi("08-compose.js"),
  readUi("15-views-misc.js"),
]);

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

assert(!lines.includes("setInterval(() =>"), "多线页面不应恢复固定 setInterval 轮询");
assert(lines.includes("LINES_REFRESH_IDLE_MS = 8000"), "多线空闲刷新间隔护栏缺失");
assert(lines.includes("scheduleLinesRefresh(running ? LINES_REFRESH_RUNNING_MS : LINES_REFRESH_IDLE_MS)"), "多线自适应刷新护栏缺失");
assert(!/open\.addEventListener\([\s\S]{0,220}?await refreshProcesses\(\)/.test(lines), "线路切换前不应额外刷新进程列表");

assert(sessions.includes("processRefreshInFlight"), "进程刷新缺少单飞请求护栏");
assert(sessions.includes("processSwitchGeneration"), "线路切换缺少请求代次护栏");
assert(sessions.includes("tabs.replaceChildren(...nextTabs)"), "顶部 tab 缺少稳定节点复用路径");
assert(sessions.includes("loadConversation(null, switchGeneration)"), "对话恢复未绑定切换代次");

assert(compose.includes("const processUpdateQueues = new Map()"), "进程设置缺少按线路保存队列");
assert(compose.includes("queueProcessUpdate(activeProcessId"), "模型/profile/reasoning 未走保存队列");
const profileFunction = compose.match(/function applyProfileValue\(backendProfile\) \{([\s\S]*?)\n\}/)?.[1] ?? "";
assert(profileFunction && !profileFunction.includes("localStorage.setItem(PROFILE_STORAGE_KEY"), "切换线路不能回写全局 profile");

assert(views.includes("switchGeneration = null"), "对话恢复缺少可选切换代次参数");
assert(views.includes("if (!isCurrent()) return;"), "旧线路对话响应缺少丢弃护栏");

console.log("并行线路回归护栏通过:刷新节流、切换代次、tab 复用、设置串行保存和 profile 隔离");
