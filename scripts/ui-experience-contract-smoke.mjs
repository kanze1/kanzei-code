/* global window, requestAnimationFrame */
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const before = process.argv.includes("--before");
const output = `output/audit-WB6/activity/experience-${before ? "before" : "current"}`;
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage();
const errors = [], checks = [];
page.on("pageerror", error => errors.push(String(error)));
try {
  if (before) {
    const body = execFileSync("git", ["show", "26acb7f0:crates/kanzei-app/ui/01-core.js"], { encoding: "utf8" });
    await page.route("**/01-core.js", route => route.fulfill({ contentType: "text/javascript", body }));
  }
  await page.goto(`${server.origin}/?scene=chat`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  const results = await page.evaluate(async () => {
    const core = await import("/01-core.js"), shell = await import("/03-shell.js");
    const visual = await import("/22-neural-flow.js"), activity = await import("/22-activity-state.js");
    const sid = shell.activeSessionId, other = "wb6-background";
    const store = activity.createActivityStateStore({ getSessionId: () => shell.activeSessionId, getRuntime: id => shell.sessionStates.get(id) });
    const previous = visual.neuralFlowEmit;
    let seen = [], sequence = 0;
    const rows = [];
    const capture = (name, actual, expected) => rows.push({ name, actual, expected });
    visual.setNeuralFlowEmit((type, detail) => { seen.push({ type, ...detail }); store.emit(type, detail); });
    const event = (type, klass, payload = {}, sessionId = sid) => core.handleExperienceEvent({
      schema_version: 1, event_id: `wb6-${++sequence}`, session_id: sessionId, event_type: type, class: klass, payload,
    });
    const frame = () => new Promise(resolve => requestAnimationFrame(resolve));
    const reset = () => { core.flushExperienceDeltas(); seen = []; shell.transitionSession(sid, "running"); store.emit("run_started", { session_id: sid }); };
    try {
      reset();
      event("tool_started", "fact", { tool_call_id: "call-a" });
      event("tool_progressed", "delta", { tool_call_id: "call-a", text: "finish" });
      event("tool_completed", "fact", { tool_call_id: "call-a" });
      await frame();
      capture("Queued tool progress precedes completed fact", seen.map(item => item.type), ["tool_started", "tool_progressed", "tool_completed"]);
      capture("Completed tool cannot reappear as executing on the next frame", store.current(), "thinking");

      reset();
      event("text_delta", "delta", { text: "part 1" });
      event("text_delta", "delta", { text: " + part 2" });
      event("run_started", "fact", { step: 2 });
      await frame();
      capture("New turn follows complete coalesced previous text", seen.map(item => [item.type, item.text || "", item.delta_count || 0]), [["assistant_streaming", "part 1 + part 2", 2], ["run_started", "", 0]]);
      capture("Previous text cannot turn new waiting turn into replying", store.current(), "thinking");

      reset();
      event("text_delta", "delta", { text: "background" }, other);
      event("tool_progressed", "delta", { tool_call_id: "call-b" });
      event("tool_completed", "fact", { tool_call_id: "call-b" });
      capture("Fact flush leaves other session queued", [...core.pendingExperienceDeltas.values()].map(item => item.session_id), [other]);
      capture("Partial flush keeps existing frame scheduled", core.experienceDeltaFlushScheduled, true);
      event("text_delta", "delta", { text: "same-frame continuation" });
      await frame();
      capture("Retained scheduled frame delivers subsequent current-session text", seen.filter(item => item.type === "assistant_streaming").map(item => item.text), ["same-frame continuation"]);
      capture("Foreground does not receive another session's progress", seen.every(item => item.session_id === sid), true);
      capture("Frame fully drains queue and scheduler flag", [core.pendingExperienceDeltas.size, core.experienceDeltaFlushScheduled], [0, false]);

      reset();
      event("text_delta", "delta", { text: "foreground after background idle" });
      event("text_delta", "delta", { text: "background before idle" }, other);
      window.__kzPreview.emit("kz:idle", { sessionId: other, reason: "completed" });
      capture("Background terminal drains only its session", [...core.pendingExperienceDeltas.values()].map(item => item.session_id), [sid]);
      capture("Background terminal never emits animation into foreground", [...seen], []);
      await frame();
      capture("Foreground queued text survives background terminal", seen.filter(item => item.type === "assistant_streaming").map(item => item.text), ["foreground after background idle"]);

      reset();
      event("tool_progressed", "delta", { tool_call_id: "last-call" });
      window.__kzPreview.emit("kz:done", { sessionId: sid, steps: 1, history: 1, halted: false, elapsedMs: 20, autoAction: { type: "NoContinue" } });
      await frame();
      capture("Legacy done is also a barrier for pending progress", seen.filter(item => ["tool_progressed", "run_completed"].includes(item.type)).map(item => item.type), ["tool_progressed", "run_completed"]);
      capture("Legacy completion effect survives the pending frame", store.current(), "complete");

      reset();
      event("tool_started", "fact", { tool_call_id: "parallel-1" });
      event("tool_started", "fact", { tool_call_id: "parallel-2" });
      event("tool_progressed", "delta", { tool_call_id: "parallel-1" });
      event("tool_completed", "fact", { tool_call_id: "parallel-1" });
      await frame();
      capture("Other parallel tool keeps executing after fact barrier", store.current(), "executing");
      event("tool_completed", "fact", { tool_call_id: "parallel-2" });
      capture("All completed parallel tools restore thinking", store.current(), "thinking");
    } finally { core.flushExperienceDeltas(); visual.setNeuralFlowEmit(previous); }
    return rows;
  });
  results.push({ name: "Production ESM and event caller have no browser errors", actual: errors, expected: [] });
  for (const item of results) {
    try { assert.deepEqual(item.actual, item.expected); checks.push({ ...item, passed: true }); }
    catch (error) { checks.push({ ...item, passed: false, error: error.message }); }
  }
} finally { await browser.close(); await server.close(); }
await writeFile(`${output}/acceptance.json`, JSON.stringify({ before, checks }, null, 2));
for (const item of checks) console.log(`${item.passed ? "PASS" : "FAIL"} ${item.name}${item.error ? `: ${item.error}` : ""}`);
console.log(`Experience contract: ${checks.filter(item => item.passed).length}/${checks.length} passed.`);
if (checks.some(item => !item.passed)) process.exitCode = 1;
