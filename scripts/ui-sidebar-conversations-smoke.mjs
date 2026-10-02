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
    f.state.processes.push({ ...owner, id: `d|${general}`, session_id: "ses_general_main", project_dir: general, origin_project: general, title: "日常对话", running: false });
    f.state.processes.push({ ...owner, id: `p1|${general}`, session_id: "ses_general_one", project_dir: general, origin_project: general, kind: "discussion", profile: "readonly", title: "周末的阅读计划", ordinal: 1, running: false });
    const scope = await import("/03-general-scope.js");
    scope.setGeneralChatRoot(general);
    window.__sidebarTest = { f, project, general, target: f.state.closed[0].id, failDelete: false };
    window.__kzPreview.setCommand("process_purge", args => {
      if (window.__sidebarTest.failDelete) throw "记录暂时被占用，请重试";
      return f.commands.process_purge(args);
    });
    const tree = await import("/12-session-tree.js");
    tree.setSidebarOpen(project, true);
    tree.setClosedOpen(project, true);
    await tree.loadClosedSessions(project, { force: true });
    await tree.loadRemoteSessions(general, { force: true });
    tree.invalidateSessionTree();
    return { project, general, target: f.state.closed[0].id, owner: owner.id };
  });
  await settle();
  const row = () => page.locator("#workbench-project-list [data-ctx='closed']").filter({ hasText: "旧对话：待整理的方案" });
  check(await row().count() === 1, "Closed conversation appears once in the sidebar");
  check(await row().locator(".workbench-row-menu").isVisible(), "Closed history has an explicit accessible menu button");
  check(await row().locator(".workbench-session-tag").count() === 0, "Closed rows do not repeat read-only badges");
  check(await page.locator("#workbench-general-list").innerText().then(text => text.includes("周末的阅读计划")), "Projectless history remains visible while a project is selected");
  await row().locator(".workbench-row-menu").click();
  await page.getByRole("menuitem", { name: /重命名/ }).click();
  await page.locator("#input-value").fill("已整理的方案");
  await page.locator("#input-ok").click(); await settle();
  const renamed = () => page.locator("#workbench-project-list [data-ctx='closed']").filter({ hasText: "已整理的方案" });
  check(await renamed().count() === 1, "Renaming a closed chat updates the shared row");
  check(await page.evaluate(id => window.__kzPreview.calls.some(c => c.cmd === "process_rename" && c.args.processId === id), fixture.target), "Closed rename uses the same backend management command");
  await renamed().locator(".workbench-session-link").focus();
  await page.keyboard.press("F2");
  check(await page.locator("#input-dialog").isVisible(), "F2 renames retained history");
  await page.keyboard.press("Escape");
  await page.evaluate(() => { window.__sidebarTest.failDelete = true; });
  await renamed().locator(".workbench-row-menu").click();
  await page.getByRole("menuitem", { name: /删除对话/ }).click();
  await page.locator("#confirm-ok").click(); await settle();
  check(await renamed().count() === 1, "A failed deletion retains the conversation");
  check(await page.locator("body").innerText().then(text => text.includes("记录暂时被占用")), "Deletion failure explains why retry is needed");
  await page.evaluate(() => { window.__sidebarTest.failDelete = false; });
  await renamed().locator(".workbench-row-menu").click();
  await page.getByRole("menuitem", { name: /删除对话/ }).click();
  await page.locator("#confirm-ok").click(); await settle();
  check(await renamed().count() === 0, "Successful deletion removes old history immediately");
  check(await page.evaluate(() => window.__sidebarTest.f.state.closed.length) === 14, "Deleting one history item preserves its neighbors");
  check(await page.locator("#workbench-project-list [data-ctx='closed']").count() === 4, "Long closed history is condensed without hiding its history entry point");
  await page.locator("#workbench-chat-history").click();
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
