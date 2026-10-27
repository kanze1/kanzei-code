// Real keyboard delivery through the production composer, with isolated IPC.
/* global window, document, requestAnimationFrame */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/async-input";
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const proof = { checks: [], errors: [] };
const check = (name, actual, expected) => { proof.checks.push({ name, actual, expected }); assert.deepEqual(actual, expected, name); };

async function open({ selected = false, idle = false, delivery = null } = {}) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.on("pageerror", error => proof.errors.push(error.message));
  await page.goto(`${server.origin}/?scene=composer`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async () => (await import("/03-shell.js")).navigate_view("chat"));
  await page.locator("#prompt").waitFor({ state: "visible" });
  const owner = await page.evaluate(async ({ selected, idle, delivery }) => {
    const shell = await import("/03-shell.js");
    const { invoke } = await import("/00-ipc.js");
    const { readProjectDocuments } = await import("/01-project-resources.js");
    const { workChoices } = await import("/30-workspace-model.js");
    const { selectWork } = await import("/31-work-selection.js");
    const docs = await readProjectDocuments(shell.currentProject, { force: true });
    const snapshot = await invoke("workspace_snapshot", { projectDir: shell.currentProject });
    const normalize = value => String(value || "").replace(/\\/g, "/").toLowerCase();
    const lines = snapshot.projects?.find(row => normalize(row.path) === normalize(shell.currentProject))?.lines || [];
    const choice = workChoices([...(docs.requirements || []), ...(docs.defects || [])], lines, shell.activeProcessId).find(row => row.selectable);
    if (!choice) throw new Error("Missing selectable work fixture");
    selectWork(shell.currentProject, shell.activeProcessId, selected ? choice.id : null);
    if (delivery) document.querySelector("#delivery-select").value = delivery;
    if (idle) window.__kzPreview.emit("kz:idle", { sessionId: shell.activeSessionId, reason: "completed" });
    window.__kzPreview.setCommand("run_prompt", () => "queued");
    return { project: shell.currentProject, process: shell.activeProcessId, session: shell.activeSessionId, selected: selected ? choice.id : null };
  }, { selected, idle, delivery });
  await page.evaluate(() => window.__kzPreview.settle());
  return { page, owner };
}

try {
  const queued = await open();
  await queued.page.locator("#prompt").fill("尚未发送的真实用户草稿");
  const queueProjection = await queued.page.evaluate(async owner => {
    const sessions = await import("/09-sessions.js");
    const inputs = [
      { input_id: "human-steer", origin: "human", delivery: "steer", prompt: "用户补充方向" },
      { input_id: "child-result", origin: "notification", delivery: "steer", prompt: "子任务返回结果" },
      { input_id: "human-reply", origin: "human", delivery: "queue", prompt: "用户回答后台问题" },
      { input_id: "automatic", origin: "engine", delivery: "queue", prompt: "自动继续" },
      { input_id: "agent-message", origin: "agent", delivery: "steer", prompt: "代理消息" },
      { input_id: "tool-message", origin: "tool", delivery: "steer", prompt: "工具完成" },
      { input_id: "summary", origin: "summary", delivery: "queue", prompt: "历史摘要" },
      { input_id: "legacy", delivery: "queue", prompt: "来源未记录" },
    ].map(input => ({ ...input, session_id: owner.session, created_at: 1 }));
    window.__kzPreview.setCommand("list_pending_inputs", () => inputs);
    window.__kzPreview.setCommand("cancel_input", ({ inputId }) => {
      const index = inputs.findIndex(input => input.input_id === inputId);
      if (index < 0) return false;
      inputs.splice(index, 1); return true;
    });
    await sessions.refreshPendingInputs();
    return {
      prompts: [...document.querySelectorAll("#queue-list .queue-prompt")].map(row => row.textContent),
      count: document.querySelector("#queue-count").textContent,
      cancellable: document.querySelectorAll("#queue-list .queue-cancel").length,
      draft: document.querySelector("#prompt").value,
    };
  }, queued.owner);
  check("Scheduler callbacks never appear as cancellable user input", queueProjection,
    { prompts: ["用户补充方向", "用户回答后台问题"], count: "(2)", cancellable: 2, draft: "尚未发送的真实用户草稿" });
  await queued.page.locator("#queue-list .queue-cancel").first().click();
  await queued.page.waitForFunction(() => document.querySelectorAll("#queue-list .queue-entry").length === 1);
  check("Cancel targets only the real user input", await queued.page.evaluate(() =>
    window.__kzPreview.calls.filter(call => call.cmd === "cancel_input").map(call => call.args.inputId)), ["human-steer"]);
  await queued.page.locator("#queue-list .queue-cancel").click();
  await queued.page.waitForFunction(() => document.querySelector("#composer-queue").classList.contains("hidden"));
  check("Only background messages pending leaves the user queue empty", await queued.page.locator("#queue-count").textContent(), "");
  check("Background delivery preserves the unsent draft", await queued.page.locator("#prompt").inputValue(), "尚未发送的真实用户草稿");
  await queued.page.screenshot({ path: `${output}/callback-queue.png`, fullPage: true });
  await queued.page.close();
  const stopping = await open();
  const stopState = await stopping.page.evaluate(async owner => {
    const { sessionState } = await import("/00-session-state.js");
    window.__kzPreview.emit("kz:stopping", { sessionId: owner.session, cancelled_queue: 2 });
    return { phase: sessionState(owner.session).phase, running: sessionState(owner.session).running };
  }, stopping.owner);
  check("Stop acknowledgement retains running ownership until finalization", stopState, { phase: "stopping", running: true });
  check("Stop finalization leaves the send button available", await stopping.page.locator("#send").isEnabled(), true);
  await stopping.page.locator("#prompt").fill("停止收尾期间继续补充需求");
  await stopping.page.locator("#send").click();
  await stopping.page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "run_prompt"));
  const followup = await stopping.page.evaluate(() => window.__kzPreview.calls.findLast(call => call.cmd === "run_prompt").args);
  check("Input during stop finalization still goes to the same conversation", followup.processId, stopping.owner.process);
  check("Input during stop finalization keeps steering delivery", followup.delivery, "steer");
  const finalStop = await stopping.page.evaluate(async owner => {
    const { sessionState } = await import("/00-session-state.js");
    window.__kzPreview.emit("kz:idle", { sessionId: owner.session, reason: "completed" });
    return { phase: sessionState(owner.session).phase, running: sessionState(owner.session).running };
  }, stopping.owner);
  check("Only scheduler idle completes the stop", finalStop, { phase: "stopped", running: false });
  await stopping.page.close();
  const planning = await open({ selected: true, idle: true });
  await planning.page.evaluate(async () => (await import("/08-compose-runtime.js")).sendText("补齐节点理解，不领取实施任务", { workItemId: null, executionBatch: false }));
  const planningRequest = await planning.page.evaluate(() => window.__kzPreview.calls.findLast(call => call.cmd === "run_prompt").args);
  check("Explicit planning does not inherit the selected execution target", planningRequest.workItemId, null);
  check("Explicit planning is not promoted to an execution batch", planningRequest.executionBatch, false);
  check("Planning preserves the next work selection", await planning.page.evaluate(async owner => (await import("/31-work-selection.js")).selectedWork(owner.project, owner.process), planning.owner), planning.owner.selected);
  await planning.page.close();
  for (const scenario of [
    { name: "Ctrl+Enter with selected work", selected: true, delivery: "queue", key: "Control+Enter", expected: "steer" },
    { name: "Enter defaults to conversation delivery", key: "Enter", expected: "steer" },
    { name: "Enter keeps selected work separate", selected: true, key: "Enter", expected: "steer" },
    { name: "Explicit queue remains a queued message", selected: true, delivery: "queue", key: "Enter", expected: "queue" },
    { name: "Ctrl+Enter preserves the queue preference", delivery: "queue", key: "Control+Enter", expected: "steer" },
    { name: "Idle selected work starts its execution batch", selected: true, idle: true, key: "Enter", expected: "queue" },
  ]) {
    const { page, owner } = await open(scenario);
    await page.locator("#prompt").fill(`ASYNC_INPUT: ${scenario.name}`);
    await page.locator("#prompt").press(scenario.key);
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "run_prompt"));
    const request = await page.evaluate(() => window.__kzPreview.calls.findLast(call => call.cmd === "run_prompt").args);
    check(`${scenario.name}: delivery`, request.delivery, scenario.expected);
    check(`${scenario.name}: work binding`, request.workItemId, scenario.idle ? owner.selected : null);
    check(`${scenario.name}: execution contract`, request.executionBatch, Boolean(scenario.idle));
    check(`${scenario.name}: exactly one submission`, await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length), 1);
    if (scenario.delivery) check(`${scenario.name}: preference stays unchanged`, await page.locator("#delivery-select").inputValue(), scenario.delivery);
    if (scenario.selected && !scenario.idle) check(`${scenario.name}: work selection remains available`, await page.evaluate(async owner => (await import("/31-work-selection.js")).selectedWork(owner.project, owner.process), owner), owner.selected);
    await page.close();
  }
  const { page, owner } = await open({ selected: true });
  await page.evaluate(async owner => (await import("/31-work-selection.js")).selectWork(owner.project, owner.process, "R-NO-LONGER-SELECTABLE"), owner);
  await page.locator("#prompt").fill("新问题与已经失效的工作选择无关");
  await page.locator("#prompt").press("Control+Enter");
  await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "run_prompt"));
  check("A stale work selection cannot block a running conversation", await page.evaluate(() => window.__kzPreview.calls.findLast(call => call.cmd === "run_prompt").args.workItemId), null);
  const stream = await page.evaluate(async owner => {
    const { activePane } = await import("/01-chat-panes.js");
    const { addMessage } = await import("/05-chat-render.js");
    const shell = await import("/03-shell.js");
    const committed = addMessage("assistant md", "COMMITTED_BEFORE_INPUT");
    const emit = (name, payload = {}) => window.__kzPreview.emit(name, { sessionId: owner.session, ...payload });
    emit("kz:turn", { step: 2, maxSteps: 5 });
    emit("kz:reasoning", { text: "PROVISIONAL_THINKING_ONE" });
    emit("kz:text", { text: "PROVISIONAL_TEXT" });
    emit("kz:reasoning", { text: "PROVISIONAL_THINKING_TWO" });
    emit("kz:response-interrupted");
    // The frame has not flushed yet: discarded nodes must not be resurrected.
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const cleared = !activePane.textContent.includes("PROVISIONAL_");
    const committedKept = committed.isConnected;
    const stillRunning = shell.running;
    emit("kz:turn", { step: 2, maxSteps: 5 });
    emit("kz:text", { text: "NEW_RESPONSE_AFTER_INPUT" });
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return { cleared, committedKept, stillRunning, response: shell.currentAssistant.dataset.raw,
      pendingErrors: activePane.textContent.includes("连接中断") };
  }, owner);
  check("All provisional reasoning segments and text are discarded", stream.cleared, true);
  check("Previously committed answer survives interruption", stream.committedKept, true);
  check("Interruption keeps the owning session running", stream.stillRunning, true);
  check("New response starts without old text", stream.response, "NEW_RESPONSE_AFTER_INPUT");
  check("Human input is not presented as a connection failure", stream.pendingErrors, false);
  const background = await page.evaluate(async () => {
    const { activePane, paneFor } = await import("/01-chat-panes.js");
    const emit = (name, payload = {}) => window.__kzPreview.emit(name, { sessionId: "async-background-owner", ...payload });
    const visible = activePane.textContent;
    const status = document.querySelector("#status-text").textContent;
    emit("kz:turn", { step: 1, maxSteps: 5 });
    emit("kz:text", { text: "BACKGROUND_COMMITTED_STEP" });
    emit("kz:turn", { step: 2, maxSteps: 5 });
    emit("kz:reasoning", { text: "BACKGROUND_PROVISIONAL_REASONING" });
    emit("kz:text", { text: "BACKGROUND_PROVISIONAL_TEXT" });
    emit("kz:response-interrupted");
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const pane = paneFor("async-background-owner");
    return { committedKept: pane.textContent.includes("BACKGROUND_COMMITTED_STEP"),
      provisionalCleared: !pane.textContent.includes("BACKGROUND_PROVISIONAL_"),
      visibleUntouched: activePane.textContent === visible,
      statusUntouched: document.querySelector("#status-text").textContent === status };
  });
  for (const [name, value] of Object.entries(background)) check(`Background interruption: ${name}`, value, true);
  await page.close();
  check("No browser runtime errors", proof.errors, []);
  console.log(`Async composer: ${proof.checks.length} checks passed`);
} finally {
  await writeFile(`${output}/acceptance.json`, JSON.stringify(proof, null, 2));
  await browser.close();
  await server.close();
}
