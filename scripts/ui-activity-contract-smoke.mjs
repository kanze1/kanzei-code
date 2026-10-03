import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";

const before = process.argv.includes("--before");
const file = "crates/kanzei-app/ui/22-activity-state.js";
const source = before
  ? execFileSync("git", ["show", `26acb7f0:${file}`], { encoding: "utf8" })
  : await readFile(file, "utf8");
const { createActivityStateStore } = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
const checks = [];
function check(name, run) {
  try { run(); checks.push({ name, passed: true }); }
  catch (error) { checks.push({ name, passed: false, error: error.message }); }
}
function fixture() {
  let selected = "a", clock = 1000;
  const states = new Map([["a", { phase: "running", running: true, converged: false }]]);
  const store = createActivityStateStore({ getSessionId: () => selected, getRuntime: id => states.get(id), now: () => clock });
  return {
    store, states,
    select(id) { selected = id; },
    tick(ms) { clock += ms; },
    phase(phase, id = "a") { states.set(id, { phase, running: ["starting", "running", "stopping"].includes(phase), converged: ["idle", "stopped", "failed", "auto_pending"].includes(phase) }); },
    emit(type, id = "a", detail = {}) { store.emit(type, { session_id: id, ...detail }); },
  };
}

for (const [terminal, phase] of [["run_failed", "failed"], ["run_completed", "idle"], ["run_stopped", "stopped"]]) {
  check(`Explicit starting overrides cached ${terminal} before backend turn`, () => {
    const f = fixture();
    f.emit(terminal); f.phase(phase);
    // 08-compose-runtime transitions to starting before awaiting run_prompt.
    f.phase("starting");
    assert.equal(f.store.current(), "thinking");
    f.phase("running");
    assert.equal(f.store.current(), "thinking", "poll recovery before turn must not restore old cache");
  });
}
check("Starting clears unfinished tool presentation from the previous run", () => {
  const f = fixture();
  f.emit("tool_started", "a", { tool_call_id: "old" });
  f.phase("idle");
  // No current() was requested while the session was in the background.
  f.phase("starting");
  assert.equal(f.store.current(), "thinking");
  f.phase("running"); f.emit("assistant_streaming");
  assert.equal(f.store.current(), "replying");
});
check("Completion effect remains valid until real next start or expiry", () => {
  const f = fixture();
  f.emit("run_completed");
  assert.equal(f.store.current(), "complete", "done precedes session idle");
  f.phase("auto_pending");
  assert.equal(f.store.current(), "complete");
  f.tick(1800);
  assert.equal(f.store.current(), "idle");
});
check("Failure and stopping runtime override earlier presentation", () => {
  const f = fixture(); f.emit("run_completed");
  f.phase("failed"); assert.equal(f.store.current(), "blocked");
  f.phase("stopping"); assert.equal(f.store.current(), "idle");
  f.emit("tool_started"); assert.equal(f.store.current(), "idle");
});
check("Parallel tool completion and streamed response retain correct state", () => {
  const f = fixture();
  for (const id of ["read", "build"]) f.emit("tool_started", "a", { tool_call_id: id });
  f.emit("tool_completed", "a", { tool_call_id: "read" });
  f.emit("assistant_streaming"); assert.equal(f.store.current(), "executing");
  f.emit("tool_completed", "a", { tool_call_id: "build" });
  f.emit("assistant_streaming"); assert.equal(f.store.current(), "replying");
});
check("New start on a background session preserves foreground tool state", () => {
  const f = fixture();
  f.emit("tool_started", "a", { tool_call_id: "a-tool" });
  f.phase("running", "b"); f.emit("run_failed", "b"); f.phase("starting", "b");
  assert.equal(f.store.current(), "executing");
  f.select("b"); assert.equal(f.store.current(), "thinking");
  f.select("a"); assert.equal(f.store.current(), "executing");
});
check("Terminal source rejects late progress and runtime state stays untouched", () => {
  const f = fixture(); f.phase("idle");
  const original = structuredClone(f.states.get("a"));
  for (const type of ["tool_started", "tool_progressed", "tool_completed", "assistant_streaming", "reasoning_active", "context_compacted"]) f.emit(type);
  assert.equal(f.store.current(), "idle");
  assert.deepEqual(f.states.get("a"), original);
  f.select(null); assert.equal(f.store.current(), "idle");
});
check("Run started event resets presentation and tools without mutating runtime", () => {
  const f = fixture(); f.emit("tool_started", "a", { tool_call_id: "old" });
  const original = structuredClone(f.states.get("a"));
  f.emit("run_started"); f.emit("assistant_streaming");
  assert.equal(f.store.current(), "replying");
  assert.deepEqual(f.states.get("a"), original);
});

const output = `output/audit-WB6/activity/${before ? "before" : "current"}`;
await mkdir(output, { recursive: true });
await writeFile(`${output}/acceptance.json`, JSON.stringify({ before, checks }, null, 2));
for (const item of checks) console.log(`${item.passed ? "PASS" : "FAIL"} ${item.name}${item.error ? `: ${item.error}` : ""}`);
console.log(`Activity contract: ${checks.filter(item => item.passed).length}/${checks.length} passed.`);
if (checks.some(item => !item.passed)) process.exitCode = 1;
