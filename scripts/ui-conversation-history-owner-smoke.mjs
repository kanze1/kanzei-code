// Real Edge and production history controller with isolated IPC fault scheduling.
/* global window, document */
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const before = process.argv.includes("--before");
const viewerOnly = process.argv.includes("--viewer-only");
const documentOnly = process.argv.includes("--document-only");
const rewindOnly = process.argv.includes("--rewind-only");
const reloadOnly = process.argv.includes("--reload-only") || rewindOnly;
const output = path.resolve(process.env.KANZEI_HISTORY_OUTPUT || "output/audit-WB3/history/owner");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
if (before) {
  const body = execFileSync("git", ["show", "26acb7f0:crates/kanzei-app/ui/15-views-misc.js"], { encoding: "utf8" });
  await page.route("**/15-views-misc.js", route => route.fulfill({ status: 200, contentType: "text/javascript", body }));
}
const checks = [], errors = [];
page.on("pageerror", error => errors.push(error.message));
const check = (value, name) => { assert(value, name); checks.push(name); };
try {
  await page.goto(`${server.origin}/?scene=workspace&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.locator(".workspace-card-open").first().click();
  await page.evaluate(() => window.__kzPreview.settle());
  await page.locator(".workbench-project-link.active").click();
  await page.evaluate(() => window.__kzPreview.settle());
  await page.evaluate(async onlyViewer => {
    const shell = await import("/03-shell.js"), seed = shell.processItems[0], project = shell.currentProject;
    const lines = [seed, ...[1, 2].map(n => ({ ...seed, id: `p${950+n}|${project}`, session_id: `wb3-history-${n}`,
      title: `WB3 history ${n}`, kind: "discussion", profile: "readonly", running: false }))];
    window.__history = { project, lines, gates: {}, pending: {}, operations: {}, failures: {} };
    window.__kzPreview.setCommand("process_list", () => structuredClone(lines));
    window.__kzPreview.setCommand("conversation_get", args => {
      const state = window.__history;
      if (state.failures[args.processId]) throw new Error(state.failures[args.processId]);
      if (state.gates[args.processId]) return new Promise((resolve, reject) => { state.pending[args.processId] = { resolve, reject }; });
      return [{ role: "user", parts: [{ type: "text", text: state.rewound ? "WB3_REWOUND_HISTORY" : `OWNER_HISTORY ${args.processId}` }] }];
    });
    await (await import("/09-sessions.js")).refreshProcesses();
    const state = window.__history, first = lines[1].id;
    if (!onlyViewer) {
      state.gates[first] = true;
      state.operations[first] = (await import("/09-sessions.js")).switchProcess(first);
    }
  }, viewerOnly || documentOnly || reloadOnly);
  if (!viewerOnly && !documentOnly && !reloadOnly) {
  await page.waitForFunction(() => Boolean(window.__history.pending[window.__history.lines[1].id]));
  await page.evaluate(async () => {
    const state = window.__history;
    await (await import("/09-sessions.js")).switchProcess(state.lines[2].id);
  });
  const expected = await page.evaluate(() => window.__history.lines[2].id);
  check((await page.locator("#messages").innerText()).includes(`OWNER_HISTORY ${expected}`), "Newer session history renders before the old request fails");
  await page.evaluate(async () => {
    const state = window.__history, first = state.lines[1].id;
    delete state.gates[first]; state.pending[first].reject(new Error("WB3_STALE_HISTORY_FAILURE"));
    await state.operations[first];
  });
  const observed = await page.evaluate(async () => ({ text: document.querySelector("#messages").innerText,
    process: (await import("/03-shell.js")).activeProcessId, toasts: document.body.innerText.includes("WB3_STALE_HISTORY_FAILURE") }));
  await writeFile(path.join(output, "observed-stale-error.json"), JSON.stringify(observed, null, 2));
  check(observed.process === expected && !observed.text.includes("WB3_STALE_HISTORY_FAILURE"), "Late old-session failure cannot append an error to the newer session pane");
  check(!observed.toasts, "Late old-session failure cannot offer a retry owned by the newer session");
  await page.evaluate(async () => {
    const state = window.__history, id = state.lines[2].id;
    state.failures[id] = "WB3_CURRENT_HISTORY_FAILURE";
    await (await import("/15-views-misc.js")).loadConversation(null, null, true);
  });
  check((await page.locator("#messages").innerText()).includes("WB3_CURRENT_HISTORY_FAILURE"), "Current-session failure is still visible");
  const readsBeforeRetry = await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "conversation_get").length);
  await page.evaluate(() => { const state = window.__history; delete state.failures[state.lines[2].id]; });
  check(!await page.locator("#log-panel").isVisible(), "A history error keeps the log collapsed until requested");
  await page.locator("#log-toggle").click();
  await page.locator("#log-retry").click();
  await page.evaluate(() => window.__kzPreview.settle());
  check(await page.evaluate(prior => window.__kzPreview.calls.filter(call => call.cmd === "conversation_get").length > prior, readsBeforeRetry), "Visible retry actually rereads history instead of accepting the cached error pane");
  check((await page.locator("#messages").innerText()).includes(`OWNER_HISTORY ${expected}`)
    && !(await page.locator("#messages").innerText()).includes("WB3_CURRENT_HISTORY_FAILURE"), "Current-session retry replaces the error with its correct history");
  await page.evaluate(async () => {
    const state = window.__history, id = state.lines[2].id;
    state.failures[id] = "WB3_OLD_RETRY_OWNER";
    await (await import("/15-views-misc.js")).loadConversation(null, null, true);
    delete state.failures[id];
    await (await import("/09-sessions.js")).switchProcess(state.lines[0].id);
  });
  const beforeOldRetry = await page.evaluate(async () => ({
    reads: window.__kzPreview.calls.filter(call => call.cmd === "conversation_get").length,
    process: (await import("/03-shell.js")).activeProcessId,
    text: document.querySelector("#messages").innerText,
  }));
  await page.locator("#log-retry").click();
  await page.evaluate(() => window.__kzPreview.settle());
  const afterOldRetry = await page.evaluate(async () => ({
    reads: window.__kzPreview.calls.filter(call => call.cmd === "conversation_get").length,
    process: (await import("/03-shell.js")).activeProcessId,
    text: document.querySelector("#messages").innerText,
  }));
  check(JSON.stringify(beforeOldRetry) === JSON.stringify(afterOldRetry), "An old owner's retry button cannot read or redraw the newer session");
  }
  if (reloadOnly || (!viewerOnly && !documentOnly)) {
    await page.evaluate(async () => {
      const state = window.__history, shell = await import("/03-shell.js"), id = shell.activeProcessId;
      state.reloadId = id; state.gates[id] = true;
      state.oldReload = (await import("/15-views-misc.js")).loadConversation(null, null, true);
    });
    await page.waitForFunction(() => Boolean(window.__history.pending[window.__history.reloadId]));
    await page.evaluate(async () => {
      const state = window.__history; delete state.gates[state.reloadId];
      await (await import("/15-views-misc.js")).loadConversation(null, null, true);
      state.pending[state.reloadId].resolve([{ role: "user", parts: [{ type: "text", text: "WB3_SUPERSEDED_HISTORY" }] }]);
      await state.oldReload;
    });
    const reload = await page.locator("#messages").innerText();
    await writeFile(path.join(output, "observed-reload-order.json"), JSON.stringify({ text: reload }, null, 2));
    if (!rewindOnly) check(!reload.includes("WB3_SUPERSEDED_HISTORY"), "An older same-session forced history load cannot overwrite a newer recovery");
    await page.evaluate(async () => {
      const state = window.__history, id = state.reloadId;
      delete state.pending[id]; state.gates[id] = true;
      window.__kzPreview.setCommand("conversation_action", args => {
        if (args.action === "preview") return { keptMessages: 1, files: [], unhandled: [], sourceHash: "wb3-source" };
        state.rewound = true;
        return { forked: false, processId: id, prompt: "WB3 rewind prompt", skipped: [] };
      });
      state.beforeRewind = (await import("/15-views-misc.js")).loadConversation(null, null, true);
    });
    await page.waitForFunction(() => Boolean(window.__history.pending[window.__history.reloadId]));
    await page.locator('.msg.user button[aria-label="回退或分叉"]').first().click();
    await page.locator("#conversation-action-overlay").waitFor({ state: "visible" });
    await page.evaluate(() => { delete window.__history.gates[window.__history.reloadId]; });
    await page.locator("#conversation-action-overlay").getByRole("button", { name: "只回退对话", exact: true }).click();
    await page.waitForFunction(() => document.querySelector("#messages").innerText.includes("WB3_REWOUND_HISTORY"));
    await page.evaluate(async () => {
      const state = window.__history;
      state.pending[state.reloadId].resolve([{ role: "user", parts: [{ type: "text", text: "WB3_BEFORE_REWIND_HISTORY" }] }]);
      await state.beforeRewind;
    });
    check((await page.locator("#messages").innerText()).includes("WB3_REWOUND_HISTORY")
      && !(await page.locator("#messages").innerText()).includes("WB3_BEFORE_REWIND_HISTORY"), "Actual rewind button recovery cannot be overwritten by an earlier history load");
    await page.evaluate(() => { window.__history.rewound = false; });
  }
  if (!documentOnly && !reloadOnly) {
  await page.evaluate(async () => {
    const state = window.__history, first = state.lines[0].id;
    state.gates[first] = true;
    state.viewerOld = (await import("/15-views-misc.js")).openConversationForProcess(first, 1,
      { project: state.project, title: "WB3 older viewer" });
  });
  await page.waitForFunction(() => Boolean(window.__history.pending[window.__history.lines[0].id]));
  await page.evaluate(async () => {
    const state = window.__history;
    await (await import("/15-views-misc.js")).openConversationForProcess(state.lines[2].id, 2,
      { project: state.project, title: "WB3 newer viewer" });
  });
  check((await page.locator("#viewer-title").innerText()).includes("WB3 newer viewer"), "The newer readonly history opens while the older history is in flight");
  check(await page.evaluate(() => document.activeElement?.id === "viewer-close"), "Viewer lifecycle callbacks preserve initial focus on the close button");
  await page.evaluate(async () => {
    const state = window.__history, first = state.lines[0].id;
    delete state.gates[first];
    state.pending[first].resolve([{ role: "user", parts: [{ type: "text", text: "WB3_OLDER_VIEWER_BODY" }] }]);
    await state.viewerOld;
  });
  const viewer = await page.evaluate(() => ({ title: document.querySelector("#viewer-title").innerText,
    text: document.querySelector("#viewer-body").innerText }));
  await writeFile(path.join(output, "observed-viewer-order.json"), JSON.stringify(viewer, null, 2));
  check(viewer.title.includes("WB3 newer viewer") && !viewer.text.includes("WB3_OLDER_VIEWER_BODY"), "Late older readonly history cannot replace the newer viewer selection");
  await page.evaluate(async () => {
    const state = window.__history, id = state.lines[1].id;
    delete state.pending[id]; state.gates[id] = true;
    window.__kzPreview.setCommand("conversation_list", () => [{ sequence: 1, title: "closed segment" }]);
    state.closedOld = (await import("/15-views-misc.js")).openClosedConversation(state.project,
      { id, title: "WB3 closed history" });
  });
  await page.waitForFunction(() => Boolean(window.__history.pending[window.__history.lines[1].id]));
  await page.evaluate(async () => {
    const views = await import("/15-views-misc.js"), state = window.__history, id = state.lines[1].id;
    views.openRuntimeMarkdown("WB3 newer runtime", "WB3_NEW_RUNTIME_BODY");
    delete state.gates[id];
    state.pending[id].resolve([{ role: "user", parts: [{ type: "text", text: "WB3_OLD_CLOSED_BODY" }] }]);
    await state.closedOld;
  });
  check((await page.locator("#viewer-title").innerText()) === "WB3 newer runtime"
    && (await page.locator("#viewer-body").innerText()).includes("WB3_NEW_RUNTIME_BODY"), "Late closed history cannot replace a newer direct runtime viewer");
  await page.evaluate(async () => {
    const state = window.__history, id = state.lines[1].id;
    delete state.pending[id]; state.gates[id] = true;
    state.closingOld = (await import("/15-views-misc.js")).openConversationForProcess(id, 3,
      { project: state.project, title: "WB3 canceled viewer" });
  });
  await page.waitForFunction(() => Boolean(window.__history.pending[window.__history.lines[1].id]));
  await page.locator("#viewer-close").click();
  await page.evaluate(async () => {
    const state = window.__history, id = state.lines[1].id; delete state.gates[id];
    state.pending[id].resolve([{ role: "user", parts: [{ type: "text", text: "WB3_CLOSE_CANCELED_BODY" }] }]);
    await state.closingOld;
  });
  check(await page.evaluate(() => !document.querySelector("#viewer-overlay").open), "Closing the current viewer cancels a late history request instead of reopening it");
  }
  if (!reloadOnly) {
  const documentOwner = await page.evaluate(async () => {
    const state = window.__history, views = await import("/15-views-misc.js");
    window.__kzPreview.setCommand("docs_read", () => ({ name: "WB3-owned.md", content: "owned by original project" }));
    window.__kzPreview.setCommand("docs_open", () => true);
    const destination = "C:/WB3-other-project";
    window.__kzPreview.setCommand("projects_select", () => new Promise(resolve => { state.selectProject = resolve; }));
    const navigation = (await import("/12-workbench.js")).openProjectSpace(destination, "documents", { activate: true });
    await views.openDocViewer("req");
    state.selectProject({ projects: [state.project, destination], current: destination });
    await navigation;
    document.querySelector("#viewer-external").click();
    await window.__kzPreview.settle();
    return { expected: state.project, call: window.__kzPreview.calls.filter(call => call.cmd === "docs_open").at(-1) };
  });
  await writeFile(path.join(output, "observed-document-owner.json"), JSON.stringify(documentOwner, null, 2));
  check(documentOwner.call?.args.projectDir === documentOwner.expected && documentOwner.call.args.kind === "req", "External document open stays bound to the displayed document's original project");
  }
  check(errors.length === 0, `No browser runtime errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ status: "passed", before, checks, errors,
    boundary: "Real Edge, actual ESM, isolated IPC; no backend/model/user data." }, null, 2));
  console.log(`Conversation history owner PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path: path.join(output, "failure.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ status: "failed", before, checks, errors, error: String(error.stack || error) }, null, 2));
  throw error;
} finally { await browser.close(); await server.close(); }
