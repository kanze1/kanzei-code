// Behavioral acceptance for explicit message admission, session truth and paged history.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const result = { at: new Date().toISOString(), checks: [], errors: [], boundary: "Current production ESM in Edge; mocked native IPC" };
const check = (name, value) => { assert.ok(value, name); result.checks.push(name); console.log(name); };
async function open(failListen = false) {
  const page = await browser.newPage(); page.on("pageerror", error => result.errors.push(String(error)));
  await page.addInitScript(fail => {
    window.failListen = fail; let tauri;
    Object.defineProperty(window, "__TAURI__", { configurable: true, get: () => tauri, set: value => {
      const listen = value.event.listen;
      value.event.listen = (name, handler) => window.failListen && name === "kz:turn" ? Promise.reject(new Error("subscription unavailable")) : listen(name, handler);
      tauri = value;
    } });
  }, failListen);
  await page.goto(server.origin + "/?scene=chat"); await page.waitForFunction(() => window.__kzPreview?.ready);
  return page;
}
try {
  const page = await open();
  check("UI08: research presentation not loaded on normal startup", await page.evaluate(() => !performance.getEntriesByType("resource").some(item => /\/19-research(?:-auto|-navigation)?\.js/.test(item.name))));
  check("session phase determines all compatibility properties and rejects overrides atomically", await page.evaluate(async () => {
    const { sessionState, reduceSession, SESSION_PHASES } = await import("/00-session-state.js");
    for (const phase of SESSION_PHASES) {
      const state = reduceSession("test-invariants", phase);
      if (state.running !== ["starting", "running", "stopping"].includes(phase) || state.local_start_pending !== (phase === "starting")) return false;
      const expected = state.running; try { state.running = !expected; } catch (error) { if (!(error instanceof TypeError)) return false; }
      if (state.running !== expected) return false;
      try { reduceSession("test-invariants", "running", { phase: "failed" }); return false; } catch (error) { if (!(error instanceof TypeError)) return false; }
      if (sessionState("test-invariants").phase !== phase) return false;
    }
    return true;
  }));
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js"); shell.transitionSession(shell.activeSessionId, "idle");
    window.copyCount = 0; Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async () => { window.copyCount++; } } });
    window.__kzPreview.setCommand("conversation_action", { keptMessages: 1, files: [], unhandled: [] });
    const { addUserMessage } = await import("/05-chat-render.js"); addUserMessage("rollback target");
  });
  await page.locator(".conversation-action-btn").last().click();
  await page.waitForFunction(() => document.getElementById("conversation-action-overlay").open);
  check("rollback action opens its dialog without copying", await page.evaluate(() => window.copyCount === 0));
  await page.keyboard.press("Escape");
  await page.evaluate(async () => {
    window.__kzPreview.setCommand("run_prompt", () => new Promise((resolve, reject) => { window.admissionCount = (window.admissionCount || 0) + 1; window.admission = { resolve, reject }; }));
    const { sendText } = await import("/08-compose-runtime.js"); window.sendResult = sendText("admission failure test");
  });
  await page.waitForFunction(() => window.admission && document.querySelector('[data-delivery-state="pending"]'));
  check("before admission bubble is visibly pending and rollback disabled", await page.locator('[data-delivery-state="pending"] .conversation-action-btn').isDisabled());
  await page.evaluate(() => window.admission.reject(new Error("admission rejected")));
  await page.waitForFunction(() => document.querySelector('[data-delivery-state="failed"]'));
  check("admission failure keeps one explicit failed bubble", await page.locator("[data-delivery-state]").count() === 1 && await page.locator('[data-delivery-state="failed"]').innerText().then(text => text.includes("admission rejected")));
  await page.evaluate(() => { document.querySelector('[data-delivery-state="failed"] .outgoing-status button').click(); document.querySelector('[data-delivery-state="failed"] .outgoing-status button').click(); });
  await page.waitForFunction(() => window.admissionCount === 2 && document.querySelector('[data-delivery-state="pending"]'));
  await page.evaluate(() => window.admission.resolve(null));
  await page.waitForFunction(() => document.querySelector('[data-delivery-state="accepted"]'));
  check("retry reuses one bubble and sends one request", await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length === 2) && await page.locator("[data-delivery-state]").count() === 1);
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), runtime = await import("/03-session-runtime.js");
    const items = shell.processItems.map(item => ({ ...item, running: false }));
    window.__kzPreview.setCommand("process_list", args => args.projectDir === "C:/deadline" ? [{ id: "deadline-process", running: false }] : items); window.deadlineSid = "deadline-session"; window.deadlineAt = Date.now(); window.deadlineState = shell.sessionState(window.deadlineSid);
    shell.transitionSession(window.deadlineSid, "starting");
    runtime.awaitRunConfirmation(window.deadlineSid, { projectDir: "C:/deadline", processId: "deadline-process" });
  });
  await page.waitForFunction(() => window.deadlineState.phase === "idle", null, { timeout: 15000 });
  check("accepted startup deadline performs fresh idle reconciliation without a run retry", await page.evaluate(() => Date.now() - window.deadlineAt >= 10000 && window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length === 2));
  check("fresh backend idle closes starting without resending or success claim", await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), runtime = await import("/03-session-runtime.js");
    const before = window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length;
    shell.transitionSession(shell.activeSessionId, "starting"); window.__kzPreview.setCommand("process_list", [{ id: shell.activeProcessId, running: false }]);
    await runtime.confirmSessionRuntime(shell.activeSessionId, shell.currentProject, shell.activeProcessId);
    return shell.sessionState(shell.activeSessionId).phase === "idle" && window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length === before;
  }));
  check("failed state probe is explicit; later runtime fact defeats an older snapshot", await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), runtime = await import("/03-session-runtime.js");
    const sid = shell.activeSessionId; shell.transitionSession(sid, "starting");
    window.__kzPreview.setCommand("process_list", () => { throw new Error("probe unavailable"); });
    await runtime.confirmSessionRuntime(sid, shell.currentProject, shell.activeProcessId);
    if (shell.sessionState(sid).phase !== "unconfirmed") return false;
    let resolve, started; const ready = new Promise(done => { started = done; }); window.__kzPreview.setCommand("process_list", () => new Promise(done => { resolve = done; started(); }));
    const checking = runtime.confirmSessionRuntime(sid, shell.currentProject, shell.activeProcessId);
    await ready;
    shell.transitionSession(sid, "running"); resolve([]); await checking;
    return shell.sessionState(sid).phase === "running";
  }));
  check("history pages preserve owner, stop at end and expose failed reads", await page.evaluate(async () => {
    const history = await import("/00-conversation-history.js"); const all = Array.from({ length: 121 }, (_, index) => ({ sequence: index + 1, title: `history ${index + 1}`, message_count: 2 }));
    window.__kzPreview.setCommand("conversation_page", args => {
      const available = all.filter(item => args.beforeSequence == null || item.sequence < args.beforeSequence);
      const items = available.slice(-args.limit), hasMore = available.length > args.limit;
      return { items, hasMore, nextBeforeSequence: hasMore ? items[0].sequence : null };
    });
    const first = await history.readConversationHistory("C:/history", "p1"); if (first.items.length !== 50 || first.nextBeforeSequence !== 72) return false;
    await Promise.all([history.readConversationHistory("C:/history", "p1", { older: true }), history.readConversationHistory("C:/history", "p1", { older: true })]);
    if (first.items.length !== 100 || history.conversationHistory("C:/other", "p1").items.length) return false;
    await history.readConversationHistory("C:/history", "p1", { older: true }); if (first.items.length !== 121 || first.hasMore) return false;
    window.__kzPreview.setCommand("conversation_page", () => { throw new Error("history unavailable"); });
    const failed = await history.readConversationHistory("C:/history", "p2"); return failed.error.includes("history unavailable") && !failed.loaded;
  }));
  await page.evaluate(async () => {
    const fixtures = await window.__kzPreview.fixtures(); window.__kzPreview.setCommand("process_list", fixtures.state.processes);
    const shell = await import("/03-shell.js"), history = await import("/00-conversation-history.js");
    window.historyProcess = shell.activeProcessId;
    const all = Array.from({ length: 121 }, (_, index) => ({ sequence: index + 1, title: `visible history ${index + 1}`, message_count: 2 }));
    window.__kzPreview.setCommand("conversation_page", args => {
      const rows = all.filter(item => args.beforeSequence == null || item.sequence < args.beforeSequence), items = rows.slice(-args.limit), hasMore = rows.length > args.limit;
      return { items, hasMore, nextBeforeSequence: hasMore ? items[0].sequence : null };
    });
    history.invalidateConversationHistory(shell.currentProject, shell.activeProcessId);
    await history.readConversationHistory(shell.currentProject, shell.activeProcessId);
    (await import("/12-session-tree.js")).openSessionHistory(document.getElementById("workbench-chat-history"), shell.currentProject);
  });
  const historyOwner = await page.evaluate(() => window.historyProcess);
  await page.locator(`[data-history-owner="${historyOwner}"]`).click();
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), history = await import("/00-conversation-history.js");
    window.visibleHistory = history.conversationHistory(shell.currentProject, window.historyProcess);
  });
  await page.waitForFunction(() => window.visibleHistory.items.length === 100);
  check("visible history popup loads the next page for its conversation", true);
  await page.keyboard.press("Escape");
  check("resource caches and merges invalidation, supports real disposal and explicit errors", await page.evaluate(async () => {
    const { createReadResource } = await import("/00-resource.js"); let reads = 0, resolve, failure = false;
    const resource = createReadResource(() => { reads++; if (failure) throw new Error("resource unavailable"); return new Promise(done => { resolve = done; }); });
    let observed = 0; const dispose = resource.subscribe(() => observed++);
    const first = resource.load(); await Promise.resolve(); resource.invalidate(); resource.invalidate(); resolve("old");
    while (reads < 2) await Promise.resolve(); resolve("new"); await first;
    if (reads !== 2 || resource.data !== "new") return false;
    await resource.load(); if (reads !== 2) return false;
    dispose(); const before = observed; failure = true;
    try { await resource.load({ force: true }); return false; } catch { return resource.error.includes("resource unavailable") && observed === before; }
  }));
  await page.close();
  const disconnected = await open(true);
  check("subscription failure remains visible", await disconnected.locator("#runtime-connection-state").isVisible() && (await disconnected.locator("#runtime-connection-state").innerText()).includes("subscription unavailable"));
  check("failed subscription blocks new admission", await disconnected.evaluate(async () => {
    const { sendText } = await import("/08-compose-runtime.js"); const before = window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length;
    await sendText("must not submit"); return window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length === before;
  }));
  await disconnected.evaluate(() => { window.failListen = false; });
  await disconnected.locator("#runtime-connection-state button").click();
  await disconnected.waitForFunction(() => document.getElementById("runtime-connection-state").hidden);
  check("explicit recovery restores failed subscriptions", await disconnected.evaluate(async () => (await import("/00-event-subscriptions.js")).runtimeEventsReady()));
  await disconnected.close();
  check("no uncaught browser failures", result.errors.length === 0);
} finally {
  const artifactPath = "output/design-audit-20261016/ui-round2-behavior-result.json";
  await mkdir(dirname(artifactPath), { recursive: true });
  await writeFile(artifactPath, JSON.stringify(result, null, 2));
  await browser.close(); await server.close();
}
console.log(`UI round 2 acceptance passed: ${result.checks.length} checks.`);
