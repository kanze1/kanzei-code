/* Production UI in Edge with isolated IPC; never touches the user's projects or clipboard. */
/* global window, document, navigator, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
import { conversationMarkdown } from "../crates/kanzei-app/ui/31-conversation-export.js";
import { runtimeSummary } from "../crates/kanzei-app/ui/30-workspace-model.js";

const output = "output/playwright/conversation-redesign", checks = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
const md = conversationMarkdown({ name: "Conversation", project: "fixture", sessionId: "one", messages: [
  { role: "user", parts: [{ type: "text", text: "User instruction" }] },
  { role: "assistant", parts: [{ type: "reasoning", text: "Reasoning" }, { type: "tool_call", name: "bash", id: "call-1", input: { command: "echo example" } }] },
  { role: "user", parts: [{ type: "tool_result", call_id: "call-1", content: "```code```", is_error: false }, { type: "image", media_type: "image/png", data: "PRIVATE_BASE64" }] },
] });
check(md.includes("## User") && md.includes("## Assistant") && md.includes("## Tool results") && md.includes("call-1"), "Markdown preserves roles and tool call/result identities");
check(md.includes("````\n```code```\n````") && !md.includes("PRIVATE_BASE64"), "Markdown fences embedded code safely and omits image data");
const now = Date.now();
const observed = runtimeSummary([
  { sessionId: "one", at: now, outcome: "completed", measured: true, durationMs: 1000, inputTokens: 1200, outputTokens: 300, steps: 4, tools: { read: 2 }, metrics: { total_calls: 2, failed_calls: 0, tool_rejections: 0 } },
  { sessionId: "one", at: now - 1000, outcome: "halted", measured: false, durationMs: null, inputTokens: null, outputTokens: null, steps: 2 },
  { sessionId: "other", at: now, measured: true, durationMs: 10000, inputTokens: 9000, outputTokens: 9000, steps: 99 },
], [], { session: "one" });
check(observed.rounds === 2 && observed.inputTokens === 1200 && observed.tokenSamples === 1 && observed.steps === 6 && observed.p95Duration === 1000, "Runtime filters owners and excludes missing token and duration observations");
check(observed.toolNames[0][0] === "read" && observed.toolNames[0][1] === 2 && observed.read === null, "Runtime tool distribution uses recorded calls and missing memory reads stay unknown");

await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1500, height: 980 } });
page.setDefaultTimeout(10000);
page.on("pageerror", error => errors.push(error.stack || error.message));
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const last = command => page.evaluate(cmd => window.__kzPreview.calls.filter(call => call.cmd === cmd).at(-1)?.args, command);
const idle = () => page.evaluate(async () => {
  const shell = await import("/03-shell.js"); shell.transitionSession(shell.activeSessionId, "idle"); shell.setRunning(false);
});
try {
  await page.goto(`${server.origin}/?scene=chat&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready); await settle();
  const ids = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures(), shell = await import("/03-shell.js");
    const bind = new Map(), submitted = [];
    for (const process of f.state.processes) { process.running = false; shell.transitionSession(process.session_id, "idle"); }
    window.__redesign = { f, bind, submitted, failSkill: false, failReply: false, replies: [], questions: [] };
    window.__kzPreview.setCommand("run_prompt", args => { submitted.push(structuredClone(args)); return null; });
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async text => { window.__redesign.clipboard = text; } } });
    await (await import("/09-sessions.js")).refreshProcesses(); shell.setRunning(false);
    return { project: shell.currentProject, process: shell.activeProcessId, session: shell.activeSessionId, peer: f.state.processes.find(p => p.id !== shell.activeProcessId && p.profile === "dev") };
  });
  check(!await page.locator("#composer-more").isVisible() && !await page.locator("#side-question-open").isVisible(), "Obsolete composer operations are removed from the visible flow");
  check(await page.locator("#skills-nav").isVisible() && !await page.locator("#skills-picker").count() && await page.locator("#goal-picker").isVisible() && await page.locator("#process-subagents").isVisible(), "Global Skills stays in the rail while Goal and subagents remain conversation controls");
  await page.locator("#skills-nav").click(); await settle();
  check(await page.locator("#view-skills").isVisible() && await page.locator("#skills-create").isVisible(), "Skills has an independent management page");
  await page.evaluate(async () => (await import("/03-shell.js")).navigate_view("chat")); await settle();
  await page.evaluate(async peer => (await import("/09-sessions.js")).switchProcess(peer), ids.peer.id); await settle();
  check(await page.locator("#skills-nav").isVisible(), "A second conversation retains the same global Skills entry");
  await page.evaluate(async process => (await import("/09-sessions.js")).switchProcess(process), ids.process); await settle();
  check(!await page.locator("#skills-picker").count(), "Returning to a conversation does not restore per-conversation skill bindings");

  await page.locator("#profile-select").selectOption("dev-pair"); await idle(); await settle();
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), selection = await import("/31-work-selection.js");
    selection.selectWork(shell.currentProject, shell.activeProcessId, "missing-requirement");
    window.__kzPreview.setCommand("docs_snapshot", () => { throw "tracker unavailable"; });
    await (await import("/08-compose-runtime.js")).sendText("Ordinary paired request");
  }); await settle();
  const paired = await last("run_prompt");
  check(paired.agent === "dev-pair" && !paired.executionBatch && !paired.workItemId, "Paired sends ignore autonomous work selection and do not acquire batch gates");
  check(!await page.locator("#autorun-bar").isVisible(), "An ordinary paired conversation has no automatic loop controls");
  await page.evaluate(async () => { const f = window.__redesign.f; window.__kzPreview.setCommand("docs_snapshot", f.commands.docs_snapshot); });
  await idle();
  await page.locator("#profile-select").selectOption("dev-auto"); await settle();
  check(await page.locator("#autorun-bar").isVisible() && await page.locator("#auto-continue").isChecked(), "Autonomous mode binds automatic advancement");
  await page.locator("#profile-select").selectOption("dev-pair"); await settle();
  check(!await page.locator("#auto-continue").isChecked(), "Switching back to paired mode cancels an ordinary loop");
  await page.evaluate(async process => {
    await (await import("/08-compose-runtime.js")).setLineAutoState(process, { enabled: true });
    (await import("/08-auto.js")).cancelAutoContinueTimer();
  }, ids.process); await settle();
  check(await page.locator("#profile-select").inputValue() === "dev-auto", "Starting automatic work from another control selects autonomous mode even with a saved paired mode");
  await page.locator("#profile-select").selectOption("dev-pair"); await settle();
  await page.locator("#goal-picker").click(); await page.locator("#auto-goal").fill("Finish this user-defined goal");
  await page.locator("#goal-start").click(); await settle();
  const goalRun = await last("run_prompt");
  check(goalRun.agent === "dev-pair" && !goalRun.executionBatch && await page.locator("#profile-select").inputValue() === "dev-pair", "Goal starts a paired loop without changing its mode or task gates");
  check(await page.locator("#auto-continue").isChecked(), "A paired Goal enables continuation");
  await page.evaluate(async process => {
    const runtime = await import("/08-compose-runtime.js");
    await runtime.setLineAutoState(process, { paused: true });
    await runtime.setLineAutoState(process, { enabled: true, paused: false });
  }, ids.process); await settle();
  check(await page.locator("#profile-select").inputValue() === "dev-pair", "Resuming an existing paired Goal retains its conversation mode");
  await page.evaluate(async peer => (await import("/09-sessions.js")).switchProcess(peer), ids.peer.id); await settle();
  check(await page.locator("#auto-goal").inputValue() === "", "Goal does not leak to a different conversation");
  await page.evaluate(async process => (await import("/09-sessions.js")).switchProcess(process), ids.process); await settle();
  check(await page.locator("#auto-goal").inputValue() === "Finish this user-defined goal", "The original conversation restores its saved Goal");
  await page.keyboard.press("Escape"); await page.locator("#goal-picker").click(); await page.locator("#goal-clear").click(); await settle(); await page.keyboard.press("Escape"); await idle();
  check(!await page.locator("#auto-continue").isChecked() && !await page.locator("#autorun-bar").isVisible(), "Ending a paired Goal removes its automatic loop");

  await page.evaluate(async () => {
    const activity = await import("/06-activity.js"); activity.diffSummary.clear();
    activity.diffSummary.set("src/one.rs", { path: "src/one.rs", additions: 2, deletions: 1 }); activity.renderDiffSummary();
    const core = await import("/01-core.js"); if (core.$("tasks-toggle").getAttribute("aria-expanded") !== "true") core.$("tasks-toggle").click();
  }); await settle();
  check(!await page.locator("#diff-summary").isVisible(), "Activity file changes start collapsed");
  await page.locator("#diff-summary-toggle").click();
  check(await page.locator("#diff-summary").isVisible(), "The file change summary explicitly expands");
  await page.locator("#diff-summary-toggle").click();
  await page.evaluate(async () => { const activity = await import("/06-activity.js"); activity.diffSummary.set("src/two.rs", { path: "src/two.rs", additions: 3, deletions: 0 }); activity.renderDiffSummary(); });
  check(!await page.locator("#diff-summary").isVisible() && await page.locator("#diff-summary-toggle").getAttribute("aria-expanded") === "false", "New diffs preserve the user's collapsed state");
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), f = window.__redesign.f;
    for (const process of f.state.processes.filter(p => p.profile === "dev").slice(0, 2)) { process.running = true; shell.transitionSession(process.session_id, "running"); }
    await (await import("/09-sessions.js")).refreshProcesses();
  }); await settle();
  const dots = await page.locator('.workbench-session-link[data-activity="running"] .workbench-session-activity').evaluateAll(nodes => nodes.filter(n => n.getBoundingClientRect().width).map(n => getComputedStyle(n).animationName));
  check(dots.length >= 2 && dots.every(animation => animation === "none"), "Every active conversation has its own static running indicator");
  await page.emulateMedia({ reducedMotion: "reduce" });
  check(await page.locator('.workbench-session-link[data-activity="running"] .workbench-session-activity').first().evaluate(node => getComputedStyle(node).animationName) === "none", "Running indicators respect reduced motion");
  await page.emulateMedia({ reducedMotion: "no-preference" });
  const peerRow = page.locator(`.workbench-session[data-process-id="${ids.peer.id}"]`).first();
  await peerRow.click({ button: "right" });
  check(await page.getByRole("menuitem", { name: "清空对话…", exact: true }).count() === 0, "Sidebar context menus no longer offer Clear conversation");
  await page.getByRole("menuitem", { name: "复制为 Markdown", exact: true }).click(); await settle();
  const copy = await page.evaluate(async () => ({ text: window.__redesign.clipboard, active: (await import("/03-shell.js")).activeProcessId }));
  check(copy.text.includes(`- Session: ${ids.peer.session_id}`) && copy.active === ids.process, "Markdown copy reads its sidebar recipient without changing the active conversation");
  await page.screenshot({ path: `${output}/conversation-light.png` });

  await page.evaluate(async () => {
    const f = window.__redesign.f, shell = await import("/03-shell.js");
    for (const process of f.state.processes) { process.running = false; shell.transitionSession(process.session_id, "idle"); } shell.setRunning(false);
    window.__redesign.deliveries = [{ id: "delivery-owned", name: "report.md", path: shell.currentProject + "/report.md", project_dir: shell.currentProject, worktree_root: shell.currentProject, session_id: shell.activeSessionId, status: "available", bytes: 120 }];
    window.__kzPreview.setCommand("delivered_files", () => window.__redesign.deliveries);
    window.__kzPreview.setCommand("delivery_manage", ({ id, action }) => {
      const row = window.__redesign.deliveries.find(row => row.id === id); if (!row) throw "foreign receipt";
      if (action === "archive") row.archived = true;
      if (action === "remove") row.removed = true;
      if (action === "restore") { row.archived = false; row.removed = false; }
      return window.__redesign.deliveries;
    });
    await (await import("/06-deliveries.js")).loadDeliveredFiles(shell.currentProject, { force: true });
    window.__kzPreview.setCommand("run_metrics", { rounds: [{ at: Date.now(), sessionId: shell.activeSessionId, outcome: "completed", steps: 5, durationMs: 2100, inputTokens: 4000, outputTokens: 800, measured: true, metrics: { total_calls: 4, failed_calls: 1, tool_rejections: 0 }, tools: { read: 3, bash: 1 } }] });
  });
  await page.locator('[data-work-surface="project"]').click(); await page.locator('[data-management-tab="deliveries"]').click(); await settle();
  await page.locator(".management-delivery button").filter({ hasText: "整理" }).click(); await page.getByRole("menuitem", { name: "归档", exact: true }).click(); await settle();
  check(await page.locator(".management-delivery").count() === 0, "Archived deliveries leave the current list");
  await page.getByRole("combobox", { name: "交付整理", exact: true }).selectOption("archived");
  check(await page.locator(".management-delivery").count() === 1, "Archived deliveries remain available for organization");
  await page.locator(".management-delivery button").filter({ hasText: "整理" }).click(); await page.getByRole("menuitem", { name: "移除记录", exact: true }).click(); await settle();
  await page.getByRole("combobox", { name: "交付整理", exact: true }).selectOption("removed");
  check(await page.locator(".management-delivery").count() === 1 && (await last("delivery_manage")).id === "delivery-owned", "Record removal keeps a scoped and recoverable receipt");
  await page.locator('[data-management-tab="map"]').click(); await settle();
  check(await page.locator(".management-metrics > div").count() === 13 && await page.locator(".runtime-tools").isVisible(), "Runtime exposes execution, tokens, outcomes and tool distribution");
  await page.locator(".runtime-tools > summary").click(); await page.locator(".runtime-rounds > summary").click();
  check(await page.locator(".runtime-table-scroll table").innerText().then(text => text.includes("4,000") || text.includes("4000")), "Round records show the actual observed input tokens");
  await page.screenshot({ path: `${output}/runtime-light.png` });
  await page.evaluate(() => { document.querySelector("#prompt").value = "Keep this draft"; });
  await page.getByRole("button", { name: "让模型生成项目地图", exact: true }).click(); await settle();
  check(await page.locator("#prompt").inputValue().then(text => text.startsWith("Keep this draft") && text.includes("生成项目地图")), "Generate project map opens the same project chat and preserves its draft");

  await page.evaluate(async () => {
    const f = window.__redesign.f, snapshot = f.commands.workspace_snapshot();
    for (const project of snapshot.projects) { project.decisions = []; project.work_units = []; }
    const a = snapshot.projects[0], b = snapshot.projects[1];
    window.__redesign.questions = [
      { id: 9801, projectDir: a.path, sessionId: a.lines[0].session_id, revision: "a-1", question: "First project decision", background: true, options: [] },
      { id: 9802, projectDir: a.path, sessionId: a.lines[0].session_id, revision: "a-2", question: "Next project decision", background: true, options: [] },
      { id: 9803, projectDir: b.path, sessionId: b.lines[0].session_id, revision: "b-1", question: "Other project decision", background: true, options: [] },
    ];
    window.__kzPreview.setCommand("workspace_snapshot", snapshot);
    window.__kzPreview.setCommand("softwire_questions", () => window.__redesign.questions);
    window.__kzPreview.setCommand("softwire_answer_question", args => {
      if (window.__redesign.failReply) throw "fixture answer failure";
      window.__redesign.replies.push(structuredClone(args)); window.__redesign.questions = window.__redesign.questions.filter(q => q.id !== args.id); return { status: "delivered" };
    });
  });
  await page.locator("#workbench-attention").click(); await settle(); await page.locator("#sw-refresh").click(); await settle();
  check(await page.locator(".sw-inbox-project").count() === 2 && await page.locator("#workbench-attention-count").innerText() === "3", "Inbox groups pending messages by project and shows their total count");
  await page.screenshot({ path: `${output}/inbox-projects.png` });
  await page.locator(".sw-inbox-row").filter({ hasText: "First project decision" }).click();
  await page.locator(".sw-reply-explain").click(); await page.locator("#prompt").fill("first response");
  await page.evaluate(() => { window.__redesign.failReply = true; }); await page.locator("#send").click(); await settle();
  check(await page.locator("#sw-surface").innerText().then(text => text.includes("First project decision")) && await page.locator("#prompt").inputValue() === "first response", "A failed decision reply keeps the original item and draft");
  await page.evaluate(() => { window.__redesign.failReply = false; }); await page.locator("#send").click(); await settle();
  check(await page.locator("#sw-surface").innerText().then(text => text.includes("Next project decision")) && await page.locator("#workbench-attention-count").innerText() === "2", "Successful reply advances to the next item in the same project and decrements counts");
  await page.locator(".sw-reply-explain").click(); await page.locator("#prompt").fill("second response"); await page.locator("#send").click(); await settle();
  check(await page.locator("#sw-surface").innerText().then(text => text.includes("Other project decision")), "After one project finishes, continuous decisions move to the next project");
  const replies = await page.evaluate(() => window.__redesign.replies);
  check(replies.map(reply => reply.id).join() === "9801,9802" && replies[0].expectedRevision === "a-1" && replies[0].reply === "first response", "Continuous decisions preserve original identities, revisions and reply content");
  for (const [theme, width] of [["dark", 1500], ["light", 900]]) {
    await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
    await page.setViewportSize({ width, height: 980 }); await settle();
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1);
    check(!overflow, `${theme} ${width}px: The redesigned workspace has no horizontal overflow`);
    await page.screenshot({ path: `${output}/decision-${theme}-${width}.png` });
  }
  check(errors.length === 0, `No browser runtime errors: ${errors.join("; ")}`);
  console.log(`Conversation redesign PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path: `${output}/failure.png` }); throw error;
} finally {
  await writeFile(`${output}/verification.json`, JSON.stringify({ checks, errors, boundary: "Production frontend in Edge with isolated IPC; backend filesystem behavior is covered by Rust tests." }, null, 2));
  await browser.close(); await server.close();
}
