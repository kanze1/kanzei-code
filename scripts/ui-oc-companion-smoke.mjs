import assert from "node:assert/strict";
import { createOcStateStore } from "../crates/kanzei-app/ui/22-oc-companion.js";

let active = "a";
let clock = 1000;
const runtimes = new Map([["a", { phase: "idle", running: false }]]);
const store = createOcStateStore({
  getSessionId: () => active,
  getRuntime: (id) => runtimes.get(id),
  now: () => clock,
});
const emit = (type, id = "a", detail = {}) => store.emit(type, { session_id: id, ...detail });

assert.equal(store.current(), "idle");
runtimes.set("a", { phase: "running", running: true });
assert.equal(store.current(), "thinking", "恢复运行中的会话应有思考反馈，即使还没收到新动画事件");
emit("run_started");
emit("tool_started", "a", { tool_call_id: "read" });
emit("tool_started", "a", { tool_call_id: "search" });
emit("tool_completed", "a", { tool_call_id: "read" });
assert.equal(store.current(), "executing", "并行工具尚未全部结束时，光点应继续流动");
emit("assistant_streaming");
assert.equal(store.current(), "executing", "文本流不应掩盖仍在执行的工具");
emit("tool_completed", "a", { tool_call_id: "search" });
assert.equal(store.current(), "thinking");
emit("assistant_streaming");
assert.equal(store.current(), "replying");

runtimes.set("b", { phase: "running", running: true });
emit("tool_started", "b", { tool_call_id: "build" });
assert.equal(store.current(), "replying", "后台会话不得改变当前人物状态");
active = "b";
assert.equal(store.current(), "executing", "切回后台会话应恢复它自己的执行光效");
runtimes.set("b", { phase: "idle", running: false, converged: true });
assert.equal(store.current(), "idle", "即使没有动画终态事件，运行真源也应让人物收住");
emit("tool_started", "b", { tool_call_id: "late" });
assert.equal(store.current(), "idle", "迟到进度不得让终态人物重新活动");

active = "a";
emit("run_completed");
runtimes.set("a", { phase: "auto_pending", running: false, converged: true });
assert.equal(store.current(), "complete");
clock += 1801;
assert.equal(store.current(), "idle", "轮间等待必须在收尾后恢复待机");
runtimes.set("a", { phase: "running", running: true });
emit("run_started");
emit("reasoning_active");
assert.equal(store.current(), "thinking");
runtimes.set("a", { phase: "stopping", running: true });
assert.equal(store.current(), "idle", "发出停止意图后立即收住动画");
emit("assistant_streaming");
assert.equal(store.current(), "idle");
runtimes.set("a", { phase: "failed", running: false, converged: true });
assert.equal(store.current(), "blocked");
runtimes.set("a", { phase: "running", running: true });
emit("run_started");
assert.equal(store.current(), "thinking", "新一轮任务必须清除上一轮失败表现");
active = null;
assert.equal(store.current(), "idle");
assert.deepEqual(runtimes.get("a"), { phase: "running", running: true }, "动画不得改写业务状态");
console.log("OC 联动冒烟通过：并行工具、会话隔离、恢复、完成、停止和迟到事件收敛");
