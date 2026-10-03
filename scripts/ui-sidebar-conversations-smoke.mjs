// Real browser, production modules, disposable IPC fixtures. Native storage is tested separately.
/* global window, document */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
const output = "output/playwright/sidebar-conversations";
await mkdir(output, { recursive: true });
const passed = [], errors = [];
const check = (value, text) => { assert(value, text); passed.push(text); };
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const showHistory = async () => {
  if (!await page.locator("#session-history").isVisible()) await page.locator("#workbench-chat-history").click();
  await settle();
};
page.on("pageerror", error => errors.push(error.message));
page.setDefaultTimeout(10000);
try {
  await page.goto(`${server.origin}/?scene=chat&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready); await settle();
  const fixture = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures();
    const shell = await import("/03-shell.js");
    const project = shell.currentProject, general = "C:/smoke/general";
    for (const item of f.state.processes) item.running = false;
    f.state.closed = Array.from({ length: 15 }, (_, i) => ({
      id: `p${90 + i}|${project}`, session_id: `ses_closed_${i}`, ordinal: 90 + i,
      title: i === 0 ? "旧对话：待整理的方案" : `历史讨论 ${i}`,
      title_custom: true, closed_at: Date.now() - i * 1000, updated_at: Date.now() - i * 1000,
    }));
    const owner = f.state.processes.find(item => item.kind === "main");
    Object.assign(owner, { title: "侧栏标题显示修复", title_custom: false, label: "主对话" });
    f.state.processes.push({ ...owner, id: `p4|${project}`, session_id: "ses_project_legacy", title: null, label: "讨论 4", kind: "discussion", profile: "readonly", ordinal: 4 });
    f.state.processes.push({ ...owner, id: `d|${general}`, session_id: "ses_general_main", project_dir: general, origin_project: general, title: "日常对话", running: false });
    f.state.processes.push({ ...owner, id: `p1|${general}`, session_id: "ses_general_one", project_dir: general, origin_project: general, kind: "discussion", profile: "readonly", title: "周末的阅读计划", ordinal: 1, running: false });
    f.state.processes.push({ ...owner, id: `p2|${general}`, session_id: "ses_general_legacy", project_dir: general, origin_project: general, title: null, label: "对话 4", kind: "discussion", profile: "readonly", ordinal: 2 });
    const scope = await import("/03-general-scope.js");
    scope.setGeneralChatRoot(general);
    window.__sidebarTest = { f, project, general, target: f.state.closed[0].id, failDelete: false };
    window.__kzPreview.setCommand("process_purge", args => {
      if (window.__sidebarTest.failDelete) throw "记录暂时被占用，请重试";
      return f.commands.process_purge(args);
    });
    await (await import("/09-sessions.js")).refreshProcesses();
    const tree = await import("/12-session-tree.js");
    tree.setSidebarOpen(project, false);
    tree.setClosedOpen(project, true);
    await tree.loadClosedSessions(project, { force: true });
    await tree.loadRemoteSessions(general, { force: true });
    tree.invalidateSessionTree();
    return { project, general, target: f.state.closed[0].id, owner: owner.id, generalOwner: `d|${general}` };
  });
  await settle();
  const projectGroup = page.locator(`#workbench-project-list > .workbench-project[data-path=${JSON.stringify(fixture.project)}]`);
  const projectLink = projectGroup.locator(".workbench-project-link");
  const projectRows = () => projectGroup.locator("[data-ctx='session']");
  const generalRows = () => page.locator("#workbench-general-list [data-ctx='session']");
  const rowIds = locator => locator.evaluateAll(rows => rows.map(row => row.dataset.processId));
  const projectOwner = () => projectGroup.locator(`[data-ctx='session'][data-process-id=${JSON.stringify(fixture.owner)}]`);
  const generalOwner = () => page.locator(`#workbench-general-list [data-ctx='session'][data-process-id=${JSON.stringify(fixture.generalOwner)}]`);
  check(await page.locator("#workbench-project-list .workbench-project-caret, #workbench-project-list [data-act='toggle']").count() === 0, "Project rows have no separate expansion marker");
  check(await projectLink.getAttribute("aria-expanded") === "false" && await projectRows().count() === 0, "A collapsed project has no conversation rows");
  await projectLink.click(); await settle();
  check(await projectLink.getAttribute("aria-expanded") === "true" && await projectGroup.locator(".workbench-session-list").isVisible() && await projectRows().count() === 4, "Clicking the project name opens its conversation list");
  await projectLink.click(); await settle();
  check(await projectLink.getAttribute("aria-expanded") === "true", "Clicking an open project keeps its conversations expanded");
  const sidebarRows = page.locator("#workbench-project-list [data-ctx='session'], #workbench-general-list [data-ctx='session']");
  check(await sidebarRows.locator(".workbench-session-dot, .workbench-session-activity, .workbench-session-tag").count() === 0, "Project and projectless conversation rows have no leading dot or type badge");
  check(await projectOwner().locator(".workbench-session-name").innerText() === "侧栏标题显示修复" && await generalOwner().locator(".workbench-session-name").innerText() === "日常对话", "Stored automatic titles are shown for project and projectless main conversations");
  check(await projectRows().filter({ hasText: /^新对话$/ }).count() === 1 && await generalRows().filter({ hasText: /^新对话$/ }).count() === 1, "Legacy numbered titles use the same new conversation fallback in both scopes");
  await page.evaluate(async () => {
    const { f, project, general } = window.__sidebarTest;
    Object.assign(f.state.processes.find(item => item.id === `p4|${project}`), { title: "主对话", title_custom: false });
    Object.assign(f.state.processes.find(item => item.id === `p2|${general}`), { title: "对话 4", title_custom: false });
    const sessions = await import("/09-sessions.js"), tree = await import("/12-session-tree.js");
    await sessions.refreshProcesses();
    await tree.loadRemoteSessions(general, { force: true });
    tree.invalidateSessionTree();
  }); await settle();
  check(await projectRows().locator(`.workbench-session-name`).allTextContents().then(names => names.includes("主对话")) && await generalRows().locator(".workbench-session-name").allTextContents().then(names => names.includes("对话 4")), "Real automatic titles matching legacy type labels remain visible in both scopes");
  check((await rowIds(projectRows())).at(-1) === fixture.owner && (await rowIds(generalRows())).at(-1) === fixture.generalOwner, "Main conversations follow the same ordering as other conversations");
  for (const [label, ownerRow, rows, ownerId] of [
    ["Project", projectOwner, projectRows, fixture.owner],
    ["Projectless", generalOwner, generalRows, fixture.generalOwner],
  ]) {
    await ownerRow().click({ button: "right" });
    await page.getByRole("menuitem", { name: "置顶", exact: true }).click(); await settle();
    check((await rowIds(rows()))[0] === ownerId && await ownerRow().getAttribute("data-pinned") === "true", `${label} main conversation can be pinned like any conversation`);
    await ownerRow().click({ button: "right" });
    await page.getByRole("menuitem", { name: "取消置顶", exact: true }).click(); await settle();
    const beforeMove = await rowIds(rows());
    await ownerRow().click({ button: "right" });
    await page.getByRole("menuitem", { name: /更多操作/ }).click();
    check(await page.getByRole("menuitem", { name: /^上移/ }).isEnabled(), `${label} main conversation offers enabled manual ordering`);
    await page.getByRole("menuitem", { name: /^上移/ }).click(); await settle();
    check((await rowIds(rows())).indexOf(ownerId) === beforeMove.indexOf(ownerId) - 1, `${label} main conversation moves within the unified conversation order`);
  }
  check(await page.locator("#workbench-project-list [data-ctx='closed'], #workbench-general-list [data-ctx='closed'], #workbench-project-list .workbench-closed-toggle").count() === 0, "Closed and archived chats stay out of the sidebar");
  check(await page.locator(".workbench-row-menu").count() === 0, "Sidebar has no ellipsis buttons");
  const active = page.locator("#workbench-project-list [data-ctx='session']").first();
  await active.click({ button: "right" });
  check(await page.getByRole("menuitem").count() <= 6, "Conversation context menu stays compact");
  await page.keyboard.press("Escape");
  await page.locator("#workbench-project-list [data-ctx='project']").first().click({ button: "right" });
  check(await page.getByRole("menuitem").count() <= 6, "Project context menu stays compact");
  await page.keyboard.press("Escape");
  await page.evaluate(async () => { const layout = await import("/03-layout.js"); layout.setLayoutPref("preview", "open", true); });
  check(await page.evaluate(() => !document.getElementById("view-chat").dataset.preview), "Persisted preview preference never opens the browser");
  await page.locator("#workbench-chat-history").click();
  await settle();
  const row = () => page.locator("#session-history [data-ctx='closed']").filter({ hasText: "旧对话：待整理的方案" });
  check(await row().count() === 1, "Closed conversation is available only in history search");

  check(await row().locator(".workbench-session-tag").count() === 0, "Closed rows do not repeat read-only badges");
  check(await page.locator("#workbench-general-list").innerText().then(text => text.includes("周末的阅读计划")), "Projectless history remains visible while a project is selected");
  await row().click({ button: "right" });
  await page.getByRole("menuitem", { name: /重命名/ }).click();
  await page.locator("#input-value").fill("已整理的方案");
  await page.locator("#input-ok").click(); await showHistory();
  const renamed = () => page.locator("#session-history [data-ctx='closed']").filter({ hasText: "已整理的方案" });
  check(await renamed().count() === 1, "Renaming a closed chat updates the shared row");
  check(await page.evaluate(id => window.__kzPreview.calls.some(c => c.cmd === "process_rename" && c.args.processId === id), fixture.target), "Closed rename uses the same backend management command");
  await renamed().locator(".workbench-session-link").focus();
  await page.keyboard.press("F2");
  check(await page.locator("#input-dialog").isVisible(), "F2 renames retained history");
  await page.keyboard.press("Escape");
  await showHistory();
  await page.evaluate(() => { window.__sidebarTest.failDelete = true; });
  await renamed().click({ button: "right" });
  await page.getByRole("menuitem", { name: /删除对话/ }).click();
  await page.locator("#confirm-ok").click(); await showHistory();
  check(await renamed().count() === 1, "A failed deletion retains the conversation");
  check(await page.locator("body").innerText().then(text => text.includes("记录暂时被占用")), "Deletion failure explains why retry is needed");
  await page.evaluate(() => { window.__sidebarTest.failDelete = false; });
  await renamed().click({ button: "right" });
  await page.getByRole("menuitem", { name: /删除对话/ }).click();
  await page.locator("#confirm-ok").click(); await showHistory();
  check(await renamed().count() === 0, "Successful deletion removes old history immediately");
  check(await page.evaluate(() => window.__sidebarTest.f.state.closed.length) === 14, "Deleting one history item preserves its neighbors");
  check(await page.locator("#workbench-project-list [data-ctx='closed']").count() === 0, "History management cannot reinsert closed chats into the sidebar");
  const search = page.getByRole("searchbox", { name: "搜索对话", exact: true });
  check(await search.evaluate(node => node === document.activeElement), "Global search opens with input focus");
  await search.fill("周末 阅读"); await settle();
  check(await page.locator("#session-history .workbench-session").count() === 1, "Global search finds projectless conversations by all search terms");
  await search.fill("历史讨论 10"); await settle();
  check(await page.locator("#session-history [data-ctx='closed']").count() === 1, "Global search includes retained closed history");
  await page.evaluate(async () => { const tree = await import("/12-session-tree.js"); tree.invalidateSessionTree(); tree.syncSessionActivity(); });
  check(await search.inputValue() === "历史讨论 10" && await search.evaluate(node => node === document.activeElement), "Polling preserves search text and focus");
  await search.fill("已整理的方案");
  check(await page.locator("#session-history").innerText().then(text => text.includes("未找到对话")), "Deleted history is absent from search");
  await page.keyboard.press("Escape");
  await page.screenshot({ path: `${output}/sidebar-light.png` });
  await page.locator("#sidebar").screenshot({ path: `${output}/sidebar-detail-light.png` });
  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));
  await page.screenshot({ path: `${output}/sidebar-dark.png` });
  await page.setViewportSize({ width: 800, height: 900 });
  await page.locator("#rail-sidebar-toggle").click();
  await page.screenshot({ path: `${output}/sidebar-narrow.png` });
  check(errors.length === 0, `No browser errors: ${errors.join("; ")}`);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ passed, errors }, null, 2));
  console.log(`${passed.length} sidebar conversation checks passed`);
} catch (error) {
  await page.screenshot({ path: `${output}/failure.png` }).catch(() => {});
  console.error({ passed, errors }); throw error;
} finally { await browser.close(); await server.close(); }
