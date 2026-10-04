/* Production frontend, isolated IPC fixtures. Never writes the user's project data. */
/* global window, document, innerWidth */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
import { batchCells, dependencyLayers, runtimeSummary, workChoices } from "../crates/kanzei-app/ui/30-workspace-model.js";

const checks = [], errors = [], output = "output/playwright/familiar-production";
const check = (condition, label) => { assert(condition, label); checks.push(label); };
const now = Date.now();
const summary = runtimeSummary([
  { sessionId: "a", at: now, measured: true, durationMs: 1000, metrics: { total_calls: 4, failed_calls: 1, tool_rejections: 2 } },
  { sessionId: "a", at: now, measured: false, durationMs: null },
  { sessionId: "b", at: now, measured: true, durationMs: 9000, metrics: { total_calls: 20 } },
], [{ at: now, session_id: "a", retrieved_ids: ["M-1", "M-2"], injected_ids: ["M-1"], read_ids: null }], { session: "a" });
check(summary.meanDuration === 1000 && summary.durationSamples === 1, "Missing duration is excluded from the mean");
check(summary.tools === 4 && summary.failures === 1 && summary.rejected === 2, "Failures exclude expected rejections and other conversations");
check(summary.recalls === 2 && summary.injected === 1 && summary.read === null, "Recall, injection and missing read evidence remain distinct");
check(runtimeSummary([], []).tools === null, "No observations are not zero calls");
const graph = dependencyLayers(["app", "core", "tools", "shared"].map(name => ({ name })), [{ from: "app", to: "core" }, { from: "app", to: "tools" }, { from: "core", to: "shared" }, { from: "tools", to: "shared" }]);
check(graph.layers.length === 3 && graph.edges.length === 4, "Shared dependencies survive the graph projection");
check(batchCells({ batches: { done: 2, total: 4 } }).filter(cell => cell.state === "done").length === 2, "Batch cells preserve completion count");
check(!workChoices([{ id: "R-1", status: "doing" }], [{ id: "other", current_item_id: "R-1" }], "main")[0].selectable, "A different owner's work cannot be selected");

check(!workChoices([{ id: "R-2", status: "doing", claimed_by: "feature/other" }], [{ id: "main", branch: "main" }], "main")[0].selectable, "An explicit branch claim is respected even without a live owner");
check(!workChoices([{ id: "R-3", status: "doing", owner_lines: [{ id: "other" }] }], [{ id: "other" }], "main")[0].selectable, "Structured owner records prevent selecting another conversation's item");
check(dependencyLayers(["a", "b"].map(name => ({ name })), [{ from: "a", to: "b" }, { from: "b", to: "a" }]).cyclic.length === 2, "Cycles remain explicit instead of being represented as a false tree");

await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1500, height: 1000 } });
page.setDefaultTimeout(12000);
page.on("pageerror", error => errors.push(error.stack || error.message));
try {
  await page.goto(`${server.origin}/?scene=chat&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  check(await page.locator("#workspace-header #project-work-switch").isVisible(), "Conversation and management tabs are outside the composer");
  check(await page.locator("#workspace-sidebar-footer [data-view=memory]").isVisible(), "Global memory entry is in the sidebar footer");
  check(!await page.locator('[data-workspace="research"]').isVisible(), "Research workspace entry is removed");
  check(await page.locator("#project-chat-work .work-focus-slot").count() === 2, "Chat work list contains only current work and next candidate");
  check(!await page.locator(".project-work-toggle").isVisible(), "Activity panel does not restore the retired requirements drawer toggle");
  check(await page.locator("#subagent-control").isVisible(), "Subagents are directly reachable in the composer");
  check(!await page.locator("#composer-more").isVisible() && await page.locator("#skills-picker").isVisible() && await page.locator("#goal-picker").isVisible(), "Skills and Goal replace the retired More menu");
  await page.locator("#prompt").fill("切换页面后保留的草稿");
  await page.locator('[data-work-surface="project"]').click();
  await page.locator(".management-row").first().waitFor();
  check(await page.locator(".management-tabs button").count() === 4, "Requirements, defects, deliveries and map are peer pages");
  check((await page.locator(".management-row").first().innerText()).includes("R-"), "Management consumes real document snapshot entries");
  check(!/\d+\s*\/\s*\d+/.test(await page.locator(".management-row").first().innerText()), "Batch progress has no visible numeric fractions");
  await page.locator('.management-tools input[type="search"]').fill("R-365");
  const searchPreserved = await page.evaluate(async () => {
    const input = document.querySelector('.management-tools input[type="search"]');
    input.focus();
    await (await import("/14-docs-actions.js")).refreshDocs();
    return document.activeElement === input && input.isConnected && input.value === "R-365";
  });
  check(searchPreserved, "Background document polling preserves management search focus");
  await page.locator('.management-tools input[type="search"]').fill("");
  await page.locator('.management-tools [aria-label="更多"]').click();
  await page.getByRole("menuitem", { name: "高级筛选与批量管理", exact: true }).click();
  await page.locator("#documents-filter-toggle").waitFor();
  check(await page.locator("#documents-filter-toggle").isVisible(), "Advanced filtering and bulk management remain reachable");
  await page.keyboard.press("Escape");
  await page.locator('[data-work-surface="project"]').click();
  await page.locator(".management-row").first().waitFor();
  await page.screenshot({ path: `${output}/management.png` });
  await page.locator(".management-row").first().click();
  await page.locator(".management-body > .doc-detail").waitFor();
  check(await page.locator(".management-body .doc-full-title").isVisible(), "Work details occupy the main content page");
  await page.locator(".management-body .doc-edit-toggle").click();
  const edit = page.locator(".management-body .doc-edit input").first();
  await edit.fill("尚未保存的完整需求标题");
  await page.evaluate(async () => (await import("/14-docs-actions.js")).refreshDocs());
  check(await edit.inputValue() === "尚未保存的完整需求标题", "Snapshot refresh preserves a requirement edit draft");
  await page.locator('[data-management-tab="defect"]').click();
  await page.locator('[data-management-tab="req"]').click();
  await page.locator(".management-row").first().click();
  check(await page.locator(".management-body .doc-edit input").first().inputValue() === "尚未保存的完整需求标题", "Switching management tabs preserves an unsaved detail draft");
  await page.locator(".management-body .doc-edit-toggle").click();
  await page.locator('[data-management-tab="deliveries"]').click();
  check(await page.locator(".management-deliveries").isVisible(), "Delivered files have their own page");
  await page.locator('[data-management-tab="map"]').click();
  await page.locator(".management-map-node").first().waitFor();
  const nodes = await page.locator(".management-map-node").evaluateAll(nodes => nodes.map(node => node.getBoundingClientRect().x));
  check(new Set(nodes).size > 1, "Project dependencies are laid out left to right");
  await page.locator(".management-map-node").first().click();
  check((await page.locator(".management-map-tools").innerText()).includes("直接关系"), "Module selection isolates direct callers and dependencies");
  check(await page.locator(".management-metrics > div").count() === 13, "Runtime includes tokens, execution, outcomes and memory observations");
  await page.screenshot({ path: `${output}/map.png` });
  await page.locator('[data-work-surface="chat"]').click();
  check(await page.locator("#prompt").inputValue() === "切换页面后保留的草稿", "Conversation draft survives management navigation");
  await page.locator("#profile-select").selectOption("dev-auto");
  await page.getByRole("button", { name: "选择工作", exact: true }).click();
  await page.locator("#work-picker input").fill("R-365");
  await page.locator("#work-picker-list").count();
  await page.locator(".work-picker-list button").filter({ hasText: "R-365" }).click();
  const selected = await page.evaluate(async () => {
    const shell = await import("/03-shell.js"); return (await import("/31-work-selection.js")).selectedWork(shell.currentProject, shell.activeProcessId);
  });
  check(selected === "R-365", "Manual work selection is bound to the selected conversation");
  await page.evaluate(async () => {
    window.__kzPreview.setCommand("docs_snapshot", () => { throw new Error("fixture read failure"); });
    await (await import("/08-compose-runtime.js")).send();
  });
  check(await page.locator("#prompt").inputValue() === "切换页面后保留的草稿", "Failed work validation keeps the unsent prompt");
  await page.evaluate(async () => { const fixtures = await window.__kzPreview.fixtures(); window.__kzPreview.setCommand("docs_snapshot", fixtures.commands.docs_snapshot); });
  const admission = await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), runtime = await import("/08-compose-runtime.js");
    const f = await window.__kzPreview.fixtures();
    for (const item of f.state.processes) item.running = false;
    await (await import("/09-sessions.js")).refreshProcesses();
    shell.transitionSession(shell.activeSessionId, "idle");
    shell.setRunning(false);
    window.__kzPreview.setCommand("run_prompt", args => new Promise(resolve => { window.__pendingSubmission = { args, resolve }; }));
    document.querySelector("#prompt").value = "绑定当前选取的需求";
    window.__pendingSend = runtime.send();
    return { project: shell.currentProject, process: shell.activeProcessId, session: shell.activeSessionId };
  });
  await page.waitForFunction(() => window.__pendingSubmission);
  const queued = await page.evaluate(() => window.__pendingSubmission.args);
  check(queued.projectDir === admission.project && queued.processId === admission.process && queued.workItemId === "R-365" && queued.executionBatch && queued.delivery === "queue", "Manual selection submits a structurally bound requirement to the correct process");
  await page.locator("#prompt").fill("回执尚未返回时写下的新草稿");
  await page.evaluate(async () => { window.__pendingSubmission.resolve(null); await window.__pendingSend; });
  check(await page.locator("#prompt").inputValue() === "回执尚未返回时写下的新草稿", "A late send receipt cannot clear a newer draft");
  const automatic = await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), runtime = await import("/08-compose-runtime.js"), selection = await import("/31-work-selection.js");
    shell.transitionSession(shell.activeSessionId, "idle"); shell.setRunning(false);
    selection.selectWork(shell.currentProject, shell.activeProcessId, "R-365");
    window.__kzPreview.setCommand("run_prompt", () => null);
    await runtime.sendAutoToSession("自动继续", shell.activeSessionId);
    const submitted = window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").at(-1).args;
    const pending = selection.selectedWork(shell.currentProject, shell.activeProcessId);
    (await import("/08-auto.js")).releaseAutoContinue(shell.activeSessionId);
    shell.transitionSession(shell.activeSessionId, "idle"); shell.setRunning(false);
    return { submitted, pending };
  });
  check(automatic.submitted.autonomous && automatic.submitted.workItemId === "R-365" && automatic.pending === null, "Automatic continuation consumes the selected requirement exactly once");
  await page.locator("#prompt").fill("/compact 保留验收约束");
  await page.evaluate(async () => {
    window.__kzPreview.setCommand("conversation_compact", () => ({ changed: true, after: 100, message: "已压缩" }));
    await (await import("/08-compose-runtime.js")).send();
  });
  check(await page.locator("#prompt").inputValue() === "", "A successful compact command clears only its own submitted draft");
  const autoPreview = await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), preview = await import("/24-preview.js");
    const wrong = await preview.prepareAgentPreview("background-process");
    document.querySelector("#prompt").focus();
    const ready = await preview.prepareAgentPreview(shell.activeProcessId);
    const retainedFocus = document.activeElement.id === "prompt";
    return { wrong, ready, retainedFocus };
  });
  check(!autoPreview.wrong.ready && autoPreview.ready.ready, "Only the active conversation's browser call can automatically open preview");
  check(autoPreview.retainedFocus, "Automatic web preview preserves typing focus");
  await page.locator("#preview-close").click();
  const dismissed = await page.evaluate(async () => (await import("/24-preview.js")).prepareAgentPreview((await import("/03-shell.js")).activeProcessId));
  check(!dismissed.ready, "A manually closed preview is not reopened by another agent call");
  await page.locator("#preview-toggle").click();
  const dock = page.locator("#preview-dock"); await dock.waitFor();
  const before = await dock.boundingBox();
  const grip = await page.locator(".pv-window-title strong").boundingBox();
  await page.mouse.move(grip.x + 20, grip.y + 5); await page.mouse.down(); await page.mouse.move(grip.x - 95, grip.y + 50, { steps: 8 }); await page.mouse.up();
  const moved = await dock.boundingBox();
  check(Math.abs(moved.x - before.x) > 70 && moved.y > before.y + 20, "Web preview can be dragged independently");
  const handle = await page.locator('#preview-dock .k-frame-edge[data-edge="se"]').boundingBox();
  await page.mouse.move(handle.x + 8, handle.y + 8); await page.mouse.down(); await page.mouse.move(handle.x - 80, handle.y - 50, { steps: 8 }); await page.mouse.up();
  const resized = await dock.boundingBox(); check(resized.width < moved.width - 40, "Web preview resizes using the shared frame system");
  await page.locator("#preview-maximize").click();
  check((await dock.boundingBox()).width > 1400, "Web preview maximizes");
  await page.locator("#preview-maximize").click();
  check(Math.abs((await dock.boundingBox()).width - resized.width) < 2, "Restore retains the user's floating geometry");
  await page.locator("#preview-device").click();
  check(await page.getByRole("menuitemcheckbox", { name: "125%" }).isVisible().catch(() => false) || (await page.locator("body").innerText()).includes("125%"), "Page zoom is available independently of window resize");
  await page.getByRole("menuitemcheckbox", { name: "125%", exact: true }).click();
  await page.evaluate(() => window.__kzPreview.settle());
  const zoomCall = await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "preview_device").at(-1)?.args);
  check(zoomCall?.zoom === 1.25, "Page zoom reaches the native preview command when a page is alive");
  await page.locator("#preview-close").click();
  if (!await page.locator("#tasks-panel").isVisible()) await page.locator("#tasks-toggle").click();
  check(await page.locator("#tasks-panel").isVisible(), "Activity panel remains available alongside floating preview");
  await page.screenshot({ path: `${output}/conversation.png` });
  const rootBefore = await page.evaluate(async () => (await import("/03-shell.js")).currentProject);
  await page.locator('#workspace-sidebar-footer [data-view="memory"]').click();
  await page.locator("#memory-project-picker").waitFor();
  const options = await page.locator("#memory-project-picker option").evaluateAll(options => options.map(option => option.value));
  await page.locator("#memory-project-picker").selectOption(options.find(value => value !== "@global" && value !== rootBefore));
  const roots = await page.evaluate(async () => ({ execution: (await import("/03-shell.js")).currentProject, memory: (await import("/03-memory-scope.js")).memoryProject }));
  check(roots.execution === rootBefore && roots.memory !== roots.execution, "Browsing another project's memory preserves the execution recipient");
  await page.locator("#memory-project-picker").selectOption("@global");
  await page.waitForFunction(() => document.querySelector("#memory-scope-filter").value === "global");
  check(await page.locator("body").getAttribute("data-app-scope") === "global", "Memory management is a global page");
  await page.locator('#workspace-sidebar-footer [data-view="settings"]').click();
  await page.locator("#sg-project-tools > summary").click();
  check(await page.getByRole("button", { name: "开发规范", exact: true }).isVisible(), "Project conventions and run history are reachable through settings");
  check((await page.locator("#settings-toc").innerText()).includes("项目工具"), "Settings table of contents includes project tools");
  for (const theme of ["dark", "light"]) for (const width of [1440, 1000, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.evaluate(async theme => {
      document.documentElement.setAttribute("data-theme", theme);
      const workbench = await import("/12-workbench.js");
      await workbench.openProjectSpace(workbench.workbenchProject(), "project");
    }, theme);
    await page.locator('[data-management-tab="req"]').click();
    check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `${theme} management has no page overflow at ${width}px`);
    await page.screenshot({ path: `${output}/management-${theme}-${width}.png` });
  }
  check(errors.length === 0, `No browser exceptions: ${errors.join("; ")}`);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ checks, errors }, null, 2));
  console.log(`Production workspace PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path: `${output}/failure.png` });
  await writeFile(`${output}/failure.json`, JSON.stringify({ checks, errors, error: String(error) }, null, 2));
  throw error;
} finally { await browser.close(); await server.close(); }
