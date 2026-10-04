// Real Edge, production ESM and isolated IPC: late session opens cannot own newer navigation.
/* global window, document */
/* global window, document */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = path.resolve(process.env.KANZEI_SESSION_OUTPUT || "output/audit-WB2/tree/navigation-owner");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
const before = process.argv.includes("--before");
if (before) {
  const source = execFileSync("git", ["show", "26acb7f0:crates/kanzei-app/ui/12-session-tree.js"], { encoding: "utf8" });
  await page.route("**/12-session-tree.js", route => route.fulfill({ status: 200, contentType: "text/javascript", body: source }));
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
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js");
    const seed = shell.processItems[0], project = shell.currentProject;
    const lines = [seed, ...[1, 2, 3].map(n => ({ ...seed, id: `p${900+n}|${project}`,
      session_id: `wb2-navigation-${n}`, title: `WB2 session ${n}`, kind: "discussion", profile: "readonly", running: false }))];
    window.__nav = { project, lines, gates: {}, results: {}, pending: {} };
    window.__kzPreview.setCommand("process_list", () => structuredClone(lines));
    window.__kzPreview.setCommand("conversation_display_get", args => {
      if (window.__nav.gates[args.processId]) return new Promise(resolve => { window.__nav.pending[args.processId] = resolve; });
      return [{ role: "user", parts: [{ type: "text", text: `History ${args.processId}` }] }];
    });
    await (await import("/09-sessions.js")).refreshProcesses();
  });
  // Each delayed target is new, so the real history loader cannot use an existing pane cache.
  const begin = async index => page.evaluate(async n => {
    const state = window.__nav, id = state.lines[n].id;
    state.gates[id] = true;
    state.operations ??= {};
    state.operations[id] = (await import("/12-session-tree.js")).openSession(state.project, id)
      .then(value => { state.results[id] = value; });
    return id;
  }, index);
  const release = async id => page.evaluate(async key => {
    const state = window.__nav; delete state.gates[key];
    state.pending[key]([{ role: "user", parts: [{ type: "text", text: `Delayed ${key}` }] }]);
    await state.operations[key];
  }, id);
  const waitGate = id => page.waitForFunction(key => Boolean(window.__nav.pending[key]), id);
  const first = await begin(1); await waitGate(first);
  const second = await begin(2); await waitGate(second);
  await release(first);
  check(await page.evaluate(async key => (await import("/03-shell.js")).activeProcessId === key, second), "A late older switch preserves the newer session identity");
  await page.evaluate(async () => (await import("/03-shell.js")).navigate_view("settings"));
  await release(second);
  const canceled = await page.evaluate(() => ({ view: document.body.dataset.view, result: window.__nav.results[window.__nav.lines[2].id] }));
  await writeFile(path.join(output, "observed-cancellation.json"), JSON.stringify(canceled, null, 2));
  check(canceled.view === "settings", "Late session completion preserves newer Settings navigation");
  check(canceled.result === false, "Canceled session open reports false to follow-up action callers");
  const third = await begin(3); await waitGate(third);
  await page.evaluate(async () => (await import("/03-shell.js")).navigate_view("workspace"));
  await release(third);
  check(await page.evaluate(() => document.body.dataset.view === "workspace"), "Late session completion preserves newer Workspace navigation");
  await page.evaluate(async () => {
    const state = window.__nav;
    state.normal = await (await import("/12-session-tree.js")).openSession(state.project, state.lines[0].id);
  });
  check(await page.evaluate(async () => window.__nav.normal === true && document.body.dataset.view === "chat"
    && (await import("/03-shell.js")).activeProcessId === window.__nav.lines[0].id), "An uncanceled session open returns true and opens its chat");
  const missing = await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), tree = await import("/12-session-tree.js");
    shell.navigate_view("settings");
    const prior = { view: document.body.dataset.view, project: shell.currentProject,
      process: shell.activeProcessId, session: shell.activeSessionId };
    const result = await tree.openSession(window.__nav.project, `p999999|${window.__nav.project}`);
    return { result, prior, after: { view: document.body.dataset.view, project: shell.currentProject,
      process: shell.activeProcessId, session: shell.activeSessionId } };
  });
  check(missing.result === false, "A missing process returns false instead of reporting success");
  check(JSON.stringify(missing.prior) === JSON.stringify(missing.after), "A missing process preserves page and project/process/session target");
  check(errors.length === 0, `No browser runtime errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ status: "passed", before, checks, errors,
    boundary: "Real Edge and production controllers; isolated IPC, no real backend/model/user data." }, null, 2));
  console.log(`Session navigation owner PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path: path.join(output, "failure.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ status: "failed", before, checks, errors, error: String(error.stack || error) }, null, 2));
  throw error;
} finally { await browser.close(); await server.close(); }
