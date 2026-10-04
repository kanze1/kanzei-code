// 工作台/项目/对话分层的真实浏览器回归。IPC 仅用隔离夹具，不访问用户项目。
/* global window, document, CanvasRenderingContext2D, innerWidth, MouseEvent */
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { chromium } from "playwright-core";
import { MEMORY_GRAPH_FIXTURE } from "./ui-preview/memory-graph-fixture.mjs";
import { mergeWorkspacePrefs } from "./ui-preview/workspace-prefs-fixture.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const uiRoot = path.join(root, "crates/kanzei-app/ui");
const artifactRoot = path.join(root, "output/playwright/workbench");
await mkdir(artifactRoot, { recursive: true });
const source = await readFile(path.join(root, "scripts/ui-runtime-smoke.mjs"), "utf8");
const fixture = source.slice(source.indexOf("const PROJECT = "), source.indexOf("const invokeLog = []"));
const { payloads, project: projectA } = vm.runInNewContext(`${fixture}\n({payloads, project: PROJECT})`, { structuredClone, queueMicrotask });
const projectB = "C:/smoke/second-project";
const projectNames = { [projectA]: "Markdown 阅读器", [projectB]: "独立对照项目" };
const projects = { current: projectA, projects: [projectA, projectB], names: projectNames };
const seedProcess = { ...payloads.process_list[0], profile: "dev", subagent_mode: "off", running: false };
const processes = new Map([
  [projectA, [
    { ...seedProcess, id: "d|reader", session_id: "session-reader", origin_project: projectA, project_dir: projectA },
    { ...seedProcess, id: "p|reader-review", session_id: "session-reader-review", label: "阅读器复核", authority: "parallel", origin_project: projectA, project_dir: projectA },
  ]],
  [projectB, [{ ...seedProcess, id: "d|second", session_id: "session-second", origin_project: projectB, project_dir: projectB }]],
]);
const docs = structuredClone(payloads.docs_snapshot);
let uiPreferences = {
  ...payloads.ui_prefs_get,
  // 旧偏好保留；普通冷启动不应为它进入研究、读取旧对话。
  workspace_state: {
    [projectA]: { dev: { view: "chat", process_id: "d|reader" } },
    "@research-library": { space: "dev", research: { view: "research", page: "overview", topic: "alpha-study" } },
  },
};
let snapshotRevision = 1;
const summary = (project, index) => ({
  path: project, project_id: project, name: projectNames[project], current: projects.current === project,
  status: "idle", running_lines: 0, lines_total: 0, lines: [], pending_count: 0,
  updated_at: 1_796_000_000_000, observed_at: 1_796_000_000_000,
  content_revision: `summary-${snapshotRevision}-${index}`, freshness: "fresh",
  error: null,
  counts: { in_progress: 1, verifying: 0, ready_to_try: 0, decisions: 0, missing_facts: 0 },
  current_items: [{
    project_id: project, kind: "req", id: "R-001", title: `${projectNames[project]}的独立需求`,
    status: "doing", priority: "P1", batches: { done: index ? 1 : 2, total: index ? 3 : 5, source: "git" },
    claimed_by: null, owner_lines: [], running: false,
  }],
  current_items_total: 1, recent_progress: null,
});
payloads.projects_get = () => projects;
payloads.projects_select = ({ path: project }) => {
  assert(projects.projects.includes(project), "项目选择必须携带已登记项目的身份");
  projects.current = project;
  return projects;
};
payloads.project_root_info = ({ projectDir }) => ({ selected: projectDir, resolved: projectDir, shared: false });
payloads.process_list = ({ projectDir }) => processes.get(projectDir) ?? [];
payloads.conversation_display_get = ({ projectDir, processId }) => {
  assert(processes.get(projectDir)?.some((item) => item.id === processId), `对话请求串项目: ${projectDir} / ${processId}`);
  return [{ role: "user", parts: [{ type: "text", text: `历史记录 ${projectNames[projectDir]}` }] }];
};
payloads.conversation_list = () => [];
payloads.docs_snapshot = ({ projectDir }) => ({
  ...docs,
  requirements: [{ ...docs.requirements[0], id: "R-001", title: `${projectNames[projectDir]}的独立需求`, work_units: [] }],
});
payloads.workspace_overview = () => ({
  current: projects.current,
  projects: [summary(projectA, 0), summary(projectB, 1)],
  observed_at: 1_796_000_000_000 + snapshotRevision,
});
payloads.workspace_snapshot = ({ projectDir } = {}) => ({
  ...payloads.workspace_overview(),
  projects: payloads.workspace_overview().projects.filter((project) => !projectDir || project.path === projectDir).map((project) => ({
    ...project, decisions: [], work_units: [], verification_jobs: [], work_acceptances: [], rework: [],
  })),
});
payloads.ui_prefs_get = () => uiPreferences;
payloads.ui_prefs_set = (patch) => {
  const workspace = patch.workspace_state ? mergeWorkspacePrefs(uiPreferences.workspace_state, patch.workspace_state) : uiPreferences.workspace_state;
  uiPreferences = { ...uiPreferences, ...patch, workspace_state: workspace };
  return null;
};
payloads.memory_graph = MEMORY_GRAPH_FIXTURE;
payloads.research_library_list = () => ({
  entries: docs.research_topics.map((entry) => ({ ...entry, id: entry.topic || "legacy", storage_root: projectA, linked_projects: [projectA], available: true, kind: entry.kind || (entry.legacy ? "legacy" : "research") })),
  diagnostics: [],
});
// Keep the lightweight fixture honest against the schema emitted by Rust.
const ipcContract = JSON.parse(await readFile(path.join(root, "scripts/ipc-contract.json"), "utf8"));
function assertShape(value, shape, label = "workspace_overview") {
  if (shape === "nullable") return;
  if (typeof shape === "string") { assert.equal(typeof value, shape === "bool" ? "boolean" : shape, label); return; }
  if (Array.isArray(shape)) {
    assert(Array.isArray(value), `${label} should be an array`);
    for (const item of value) if (shape.length) assertShape(item, shape[0], `${label}[]`);
    return;
  }
  assert.deepEqual(Object.keys(value).sort(), Object.keys(shape).sort(), `${label} keys must match Rust`);
  for (const key of Object.keys(shape)) assertShape(value[key], shape[key], `${label}.${key}`);
}
assertShape(payloads.workspace_overview(), ipcContract.workspace_overview);

const calls = [];
const errors = [];
const heldRequests = [];
function holdNext(command, predicate = () => true) {
  let release, markSeen;
  const held = { command, predicate, used: false,
    wait: new Promise((resolve) => { release = resolve; }),
    seen: new Promise((resolve) => { markSeen = resolve; }),
    release: () => release(), markSeen: () => markSeen(),
  };
  heldRequests.push(held);
  return held;
}
const server = http.createServer(async (request, response) => {
  try {
    if (request.url === "/ipc") {
      let body = "";
      for await (const chunk of request) body += chunk;
      const { cmd, args = {} } = JSON.parse(body);
      calls.push({ cmd, args });
      const value = typeof payloads[cmd] === "function" ? await payloads[cmd](args) : payloads[cmd] ?? null;
      const held = heldRequests.find((item) => !item.used && item.command === cmd && item.predicate(args));
      if (held) { held.used = true; held.markSeen(); await held.wait; }
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ value }));
      return;
    }
    const relative = decodeURIComponent(new URL(request.url, "http://localhost").pathname).replace(/^\/+/, "") || "index.html";
    const file = path.resolve(uiRoot, relative);
    if (!file.startsWith(uiRoot + path.sep)) throw new Error("静态资源越界");
    const bytes = await readFile(file);
    const mime = { ".js": "text/javascript", ".css": "text/css", ".html": "text/html", ".svg": "image/svg+xml" }[path.extname(file)] || "application/octet-stream";
    response.writeHead(200, { "Content-Type": `${mime}; charset=utf-8` });
    response.end(bytes);
  } catch (error) {
    response.writeHead(request.url === "/ipc" ? 200 : 404, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ error: String(error) }));
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch({ channel: "msedge", headless: true });
const heavyCommands = new Set(["process_list", "conversation_display_get", "conversation_list", "models_list", "model_effective", "docs_snapshot", "research_library_list", "memory_graph"]);
let observationStart = 0;
const heavySince = (at = observationStart) => calls.slice(at).filter((call) => heavyCommands.has(call.cmd));
const count = (command) => calls.filter((call) => call.cmd === command).length;
let activePage;
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
  activePage = page;
  page.setDefaultTimeout(10000);
  page.on("pageerror", (error) => errors.push(error.stack || error.message));
  await page.addInitScript(() => {
    window.__graphFrames = 0;
    const clear = CanvasRenderingContext2D.prototype.clearRect;
    CanvasRenderingContext2D.prototype.clearRect = function (...args) {
      if (this.canvas?.closest("#memory-graph-canvas")) window.__graphFrames += 1;
      return clear.apply(this, args);
    };
    const subscriptions = new Map();
    const intervals = new Map();
    const originalInterval = window.setInterval;
    window.setInterval = (callback, delay, ...args) => {
      if (!intervals.has(delay)) intervals.set(delay, []);
      intervals.get(delay).push(() => callback(...args));
      return originalInterval(callback, delay, ...args);
    };
    window.__workbenchTick = (delay, count = 1) => {
      for (let index = 0; index < count; index++) for (const callback of intervals.get(delay) || []) callback();
    };
    window.__workbenchEmit = (event, payload) => {
      for (const listener of subscriptions.get(event) || []) listener({ payload });
    };
    globalThis.__TAURI__ = {
      core: { invoke: async (cmd, args) => {
        const response = await fetch("/ipc", { method: "POST", body: JSON.stringify({ cmd, args }) });
        const result = await response.json();
        if (result.error) throw new Error(result.error);
        return result.value;
      } },
      event: { listen: async (event, listener) => {
        if (!subscriptions.has(event)) subscriptions.set(event, new Set());
        subscriptions.get(event).add(listener);
        return () => subscriptions.get(event).delete(listener);
      } },
    };
  });
  const settle = async () => { await page.waitForTimeout(100); await page.waitForLoadState("networkidle"); };
  const view = async (name) => page.waitForFunction((name) => document.querySelector(`#view-${name}`)?.classList.contains("active"), name);
  const sidebar = async () => {
    if (await page.locator("#sidebar").evaluate(el => el.classList.contains("collapsed"))) await page.locator("#rail-sidebar-toggle").click();
  };
  const home = async () => {
    await sidebar();
    await page.locator("#workbench-home").click();
    await view("workspace");
    await settle();
    if (page.viewportSize().width <= 900 && !await page.locator("#sidebar").evaluate((el) => el.classList.contains("collapsed"))) {
      await page.locator("#rail-sidebar-toggle").click();
    }
  };
  const openOverview = async (project) => {
    await home();
    await page.evaluate(async project => (await import("./12-workbench.js")).openProjectSpace(project, "project"), project);
    await view("project");
    await settle();
  };
  const openChat = async () => {
    if (!["chat", "project"].includes(await page.locator("body").getAttribute("data-view"))) {
      await sidebar();
      const project = page.locator(".workbench-project-link.active");
      if (await project.getAttribute("aria-expanded") !== "true") await project.click();
      else await project.locator("../..").locator(".workbench-session-link").first().click();
      await settle();
    }
    if (await page.locator("body").getAttribute("data-view") !== "chat") {
      await page.locator('[data-work-surface="chat"]').click();
      await view("chat"); await settle();
    }
  };

  await page.goto(origin, { waitUntil: "networkidle" });
  await view("chat");
  assert(count("process_list") > 0, "冷启动应恢复主对话但不开始执行");
  assert.equal(count("run_prompt"), 0, "恢复概览不得启动模型任务");
  await home();
  observationStart = calls.length;
  assert.equal(await page.locator("body").getAttribute("data-app-scope"), "global");
  assert.equal(await page.locator("#new-chat").isVisible(), true, "工作台保留无项目新对话入口");
  assert.equal(await page.locator("#workbench-chat-history").isVisible(), true, "工作台保留全局搜索入口");
  assert.equal(await page.locator("#prompt").isVisible(), false, "工作台不应先打开聊天框");
  assert.deepEqual(heavySince(), [], "返回所有项目后不再装配对话或研究页");
  await page.evaluate(() => window.__workbenchTick(3000, 5));
  await settle();
  assert.deepEqual(heavySince(), [], "工作台停留不能由后台轮询激活项目");
  const firstCard = page.locator(`.workspace-card[data-path="${projectA}"]`);
  const secondCard = page.locator(`.workspace-card[data-path="${projectB}"]`);
  assert.match(await firstCard.locator(".workbench-item").innerText(), /R-001[\s\S]*Markdown 阅读器的独立需求/);
  assert.match(await firstCard.locator(".workbench-batch").innerText(), /2\/5/);
  assert.match(await secondCard.locator(".workbench-item").innerText(), /R-001[\s\S]*独立对照项目的独立需求/);
  assert.match(await secondCard.locator(".workbench-batch").innerText(), /1\/3/);
  assert.notEqual(await firstCard.locator(".workbench-item").getAttribute("data-key"), await secondCard.locator(".workbench-item").getAttribute("data-key"), "同 R-001 必须有不同项目键");
  assert.equal(await page.locator("#workbench-new-goal").isVisible(), false, "想法入口已退出工作台");
  assert.equal(count("docs_update"), 0, "浏览工作台不能登记已移除的想法");
  await page.screenshot({ path: path.join(artifactRoot, "01-workbench.png"), fullPage: true });

  await openOverview(projectA);
  assert.equal(await page.locator("body").getAttribute("data-app-scope"), "project");
  assert.deepEqual(heavySince().filter(call => !["docs_snapshot", "conversation_display_get"].includes(call.cmd)), [], "概览可读取需求统计和最近回复，但不得装配执行会话");
  await page.evaluate(() => window.__workbenchTick(3000, 5));
  await settle();
  assert.deepEqual(heavySince().filter(call => !["docs_snapshot", "conversation_display_get"].includes(call.cmd)), [], "未进入执行视图的项目不能被轮询装配会话");
  await page.screenshot({ path: path.join(artifactRoot, "02-project.png"), fullPage: true });

  await home();
  const beforeItemHistory = count("conversation_display_get");
  await secondCard.locator(".workbench-item").click();
  await view("project");
  await page.locator(".management-detail-nav").waitFor();
  await settle();
  assert(calls.some((call) => call.cmd === "docs_snapshot" && call.args.projectDir === projectB), "同编号进度标签应下钻原项目");
  assert.equal(count("conversation_display_get"), beforeItemHistory, "进度标签下钻不能顺手读取对话");
  await openOverview(projectA);
  const beforeWorkHistory = count("conversation_display_get");
  await page.locator('[data-management-tab="req"]').click();
  await view("project");
  await settle();
  assert(calls.some((call) => call.cmd === "docs_snapshot" && call.args.projectDir === projectA), "工作页应按需读取本项目文档");
  assert.equal(count("conversation_display_get"), beforeWorkHistory, "查看项目工作不能顺手读取对话");
  await openChat();
  await page.getByText(`历史记录 ${projectNames[projectA]}`, { exact: true }).waitFor({ state: "visible" });
  await page.locator("#prompt").fill("阅读器的中文草稿，尚未发送");
  await page.evaluate(async () => {
    const shell = await import("./03-shell.js");
    const composer = await import("./08-compose-runtime.js");
    shell.setAttachments([{ name: "reader.png", media_type: "image/png", data: "cmVhZGVy" }]);
    composer.renderAttachments();
  });
  assert.equal(await page.locator("#process-subagents").isVisible(), true, "子代理开关在对话工具行直接可用");
  await page.locator("#workbench-chat-history").click();
  await page.locator('.project-session-menu .workbench-session[data-process-id="p|reader-review"] .workbench-session-link').click();
  await settle();
  assert.equal(await page.locator("#prompt").inputValue(), "", "同项目第二对话不得带入主线草稿");
  await page.locator("#prompt").fill("阅读器复核对话的草稿");
  await page.locator("#workbench-chat-history").click();
  await page.locator('.project-session-menu .workbench-session[data-process-id="d|reader"] .workbench-session-link').click();
  await settle();
  assert.equal(await page.locator("#prompt").inputValue(), "阅读器的中文草稿，尚未发送", "同项目切回主线应恢复原草稿");
  await page.keyboard.press("Escape");

  const beforeSecondOverview = calls.length;
  await openOverview(projectB);
  assert.deepEqual(heavySince(beforeSecondOverview).filter(call => call.cmd !== "docs_snapshot"), [], "跨项目预览不能加载第二项目对话");
  await page.evaluate(() => window.__workbenchTick(3000, 5));
  await settle();
  assert.deepEqual(heavySince(beforeSecondOverview).filter(call => call.cmd !== "docs_snapshot"), [], "保留旧执行根时预览另一项目仍不能触发 process 轮询");
  await openChat();
  await page.getByText(`历史记录 ${projectNames[projectB]}`, { exact: true }).waitFor({ state: "visible" });
  assert.equal(await page.locator("#prompt").inputValue(), "", "第二项目不得带入阅读器草稿");
  assert.deepEqual(await page.evaluate(async () => (await import("./03-shell.js")).attachments), [], "第二项目不得带入阅读器附件");
  await page.locator("#prompt").fill("第二项目的独立草稿");
  await openOverview(projectA);
  await openChat();
  assert.equal(await page.locator("#prompt").inputValue(), "阅读器的中文草稿，尚未发送", "回到阅读器应恢复原草稿");
  assert.equal(await page.evaluate(async () => (await import("./03-shell.js")).attachments[0]?.name), "reader.png", "回到阅读器应恢复原附件");

  const beforeHidden = count("process_list");
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
    document.dispatchEvent(new Event("visibilitychange"));
    window.__workbenchTick(3000, 5);
  });
  await settle();
  assert.equal(count("process_list"), beforeHidden, "隐藏窗口应暂停 process 轮询");
  await page.evaluate(() => {
    delete document.hidden;
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await home();
  const beforeGlobalPoll = count("process_list");
  await page.evaluate(() => window.__workbenchTick(3000, 5));
  await settle();
  assert.equal(count("process_list"), beforeGlobalPoll, "回到工作台不能继续按聊天周期轮询");
  await page.evaluate(() => {
    window.__workbenchEmit("kz:status", { sessionId: "unselected-background", stage: "实现", detail: "后台继续" });
    window.__workbenchEmit("kz:idle", { sessionId: "unselected-background" });
  });
  assert.equal(await page.evaluate(async () => (await import("./03-shell.js")).sessionState("unselected-background").phase), "idle", "停轮询不能阻断后台终态事件");
  await view("workspace");
  assert.equal(calls.some((call) => ["process_create", "run_start", "run_stop", "send", "stop"].includes(call.cmd)), false, "仅浏览导航不能启动或停止模型任务");

  snapshotRevision += 1;
  await page.evaluate(async () => (await import("./12-docs-pages.js")).refreshWorkspace());
  await view("workspace");
  const additional = [];
  const check = async (name, action) => {
    try { await action(); additional.push({ name, passed: true }); }
    catch (error) { additional.push({ name, passed: false, error: String(error) }); }
  };
  const seen = (gate) => Promise.race([gate.seen, new Promise((_, reject) => setTimeout(() => reject(new Error(`未收到待延迟的 ${gate.command}`)), 5000))]);
  await check("sessionless-overview-invalidation", async () => {
    const before = count("workspace_overview");
    await page.evaluate(() => window.__workbenchEmit("kz:workspace-invalidated", { source: "lifecycle" }));
    await page.waitForTimeout(350);
    await settle();
    assert(count("workspace_overview") > before, "全局摘要失效事件没有 sessionId 也必须触发更新");
  });
  await check("home-during-project-selection", async () => {
    await openOverview(projectA);
    const gate = holdNext("projects_select", (args) => args.path === projectA);
    try {
      await page.locator('[data-work-surface="chat"]').click();
      await seen(gate);
      await home();
      gate.release();
      await settle();
      assert.equal(await page.locator("body").getAttribute("data-view"), "workspace", "迟到的 projects_select 不得把工作台抢回聊天");
    } finally { gate.release(); }
  });
  await check("rapid-project-selection", async () => {
    await openOverview(projectA);
    const gate = holdNext("projects_select", (args) => args.path === projectA);
    try {
      await page.locator('[data-work-surface="chat"]').click();
      await seen(gate);
      await page.evaluate(async path => { const tree = await import("./12-session-tree.js"); tree.setSidebarOpen(path, false); tree.invalidateSessionTree(); }, projectB);
      await page.locator(`.workbench-project-link[data-path="${projectB}"]`).click();
      gate.release();
      await settle();
      assert.equal(await page.locator("body").getAttribute("data-view"), "chat");
      assert.equal(await page.locator("#project-space-name").textContent(), projectNames[projectB], "迟到 A 请求不能覆盖 B 项目身份");
    } finally { gate.release(); }
  });
  await check("home-during-conversation-recovery", async () => {
    await openOverview(projectB);
    processes.get(projectB)[0].session_id = "session-second-delayed";
    const gate = holdNext("conversation_display_get", (args) => args.projectDir === projectB);
    try {
      await page.locator('[data-work-surface="chat"]').click();
      await seen(gate);
      await home();
      gate.release();
      await settle();
      assert.equal(await page.locator("body").getAttribute("data-view"), "workspace", "迟到的历史恢复不得把工作台抢回聊天");
    } finally { gate.release(); }
  });
  await check("document-browse-does-not-activate-execution", async () => {
    await openOverview(projectA);
    await openChat();
    await home();
    const requests = calls.length;
    await secondCard.locator(".workbench-item").click();
    await view("project");
    await page.locator(".management-detail-nav").waitFor();
    await settle();
    assert.equal(await page.evaluate(async () => (await import("./03-shell.js")).currentProject), projectA, "浏览B需求不改变A执行根");
    assert.equal(calls.slice(requests).some(call => call.cmd === "projects_select"), false);
    assert.match(await page.locator(".management-body").innerText(), /独立对照项目的独立需求/);
  });
  await check("same-view-project-work-switch", async () => {
    await openOverview(projectA);
    await page.evaluate(async project => (await import("./12-workbench.js")).openProjectSpace(project, "documents"), projectA);
    await view("documents");
    await settle();
    const requests = calls.length;
    await page.locator("#documents-project-select").selectOption(projectB);
    await settle();
    assert(calls.slice(requests).some((call) => call.cmd === "docs_snapshot" && call.args.projectDir === projectB), "工作页直接切项目必须读取新项目，不能仅切 currentProject 留着旧表单");
    assert.equal(await page.evaluate(async () => (await import("./12-docs-pages.js")).latestDocsSnapshot?.requirements[0]?.title), `${projectNames[projectB]}的独立需求`);
  });
  await check("old-work-form-cannot-write-new-project", async () => {
    await openOverview(projectA);
    await page.evaluate(async project => (await import("./12-workbench.js")).openProjectSpace(project, "documents"), projectA);
    await view("documents");
    await settle();
    await page.evaluate(async () => (await import("./11-docs-list.js")).jumpToEntry("R-001", { expand: true }));
    const update = page.locator('#view-documents [data-doc-id="R-001"] .doc-detail-actions button').first();
    await update.waitFor({ state: "visible" });
    const originalButton = await update.elementHandle();
    const gate = holdNext("docs_snapshot", (args) => args.projectDir === projectB);
    const before = calls.length;
    try {
      await page.locator("#documents-project-select").selectOption(projectB);
      await seen(gate);
      // The transition blocks real interaction. Exercise the old handler too,
      // proving an already queued click still cannot rebind A's entry to B.
      assert(await originalButton.evaluate((el) => Boolean(el.closest("[inert]"))), "跨项目读取期间旧工作表单必须不可交互");
      await originalButton.evaluate((el) => el.dispatchEvent(new MouseEvent("click", { bubbles: true })));
      await page.waitForTimeout(100);
      const wrong = calls.slice(before).filter((call) => call.cmd === "docs_update" && call.args.projectDir === projectB && call.args.id === "R-001");
      assert.deepEqual(wrong, [], "B快照未到时点击旧A详情会误写B的R-001");
    } finally { gate.release(); await settle(); }
  });
  await check("same-project-draft-return", async () => {
    await openOverview(projectA);
    await openChat();
    await page.locator("#prompt").fill("同项目返回，草稿原样保留");
    await page.locator('[data-work-surface="project"]').click();
    await view("project");
    await openChat();
    assert.equal(await page.locator("#prompt").inputValue(), "同项目返回，草稿原样保留");
  });
  await check("graph-pauses-on-home", async () => {
    await openOverview(projectA);
    await page.locator('#workspace-sidebar-footer [data-view="memory"]').click();
    await view("memory");
    await page.locator("#memory-view-graph").click();
    await page.waitForFunction(() => window.__kzMemoryGraph?.ready && window.__kzMemoryGraph.mode === "canvas");
    await page.waitForTimeout(150);
    assert(await page.evaluate(() => window.__graphFrames > 0), "图谱确实进入绘制后再检查暂停");
    await home();
    const frames = await page.evaluate(() => window.__graphFrames);
    await page.waitForTimeout(400);
    assert.equal(await page.evaluate(() => window.__graphFrames), frames, "回工作台之后隐藏图谱必须停止画帧");
  });
  await check("retired-research-workspace", async () => {
    const requests = count("research_library_list");
    await page.evaluate(async () => (await import("./03-workspaces.js")).switch_workspace("research"));
    assert.equal(await page.locator('[data-workspace="research"]').isVisible(), false);
    assert.equal(count("research_library_list"), requests, "已移除的研究工作区不能被旧偏好重新加载");
  });
  for (const width of [1000, 390]) {
    await check(`width-${width}`, async () => {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 760 });
      for (const route of ["workspace", "project"]) {
        if (route === "workspace") await home();
        else await openOverview(projectA);
        const overflow = await page.evaluate((route) => {
          const main = document.querySelector(route === "workspace" ? "#workspace-scroll" : "#project-overview-content");
          return { window: innerWidth, body: document.documentElement.scrollWidth, content: main.clientWidth, scroll: main.scrollWidth };
        }, route);
        assert(overflow.body <= overflow.window + 1 && overflow.scroll <= overflow.content + 1, `${route} ${width}px 横向溢出: ${JSON.stringify(overflow)}`);
        await page.screenshot({ path: path.join(artifactRoot, `${route}-${width}.png`), fullPage: true });
      }
    });
  }
  await writeFile(path.join(artifactRoot, "additional.json"), JSON.stringify(additional, null, 2));
  assert.deepEqual(additional.filter((item) => !item.passed), [], "竞态/窄屏/重视图回归失败");
  assert.deepEqual(errors, [], "工作台迁移不应产生浏览器运行错误");
  await writeFile(path.join(artifactRoot, "result.json"), JSON.stringify({ passed: true, checks: ["cold-start", "scoped-progress", "goal-registration", "lazy-project-preview", "item-navigation", "project-work", "conversation-drafts", "attachments", "hidden-poll", "global-poll", "background-events"], additional, calls: calls.map(({ cmd, args }) => ({ cmd, args })) }, null, 2));
  console.log("工作台浏览器回归通过：冷启动、惰性预览、按需工作/对话、草稿附件隔离、轮询和后台终态。");
} catch (error) {
  await activePage?.screenshot({ path: path.join(artifactRoot, "failure.png"), fullPage: true }).catch(() => {});
  await writeFile(path.join(artifactRoot, "failure.json"), JSON.stringify({ error: String(error), errors, calls }, null, 2));
  if (errors.length) console.error("Browser errors:", errors);
  throw error;
} finally {
  await browser.close();
  await new Promise((resolve) => server.close(resolve));
}
