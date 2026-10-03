// WebView2 + Rust + SQLite. Only disposable homes/projects; no model calls.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { copyFile, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Usage: node scripts/ui-sidebar-native-smoke.mjs <fresh kzapp.exe>");
const output = path.resolve("output/playwright/sidebar-native"), run = path.join(output, String(Date.now()));
const profile = path.join(run, "profile"), home = path.join(profile, ".kanzei"), project = path.join(run, "project");
for (const dir of [home, project, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(home, "app.json"), JSON.stringify({ projects: [], current: null, theme: "light" }));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
let app, browser, page;
const checks = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); console.log(`PASS ${label}`); };
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
async function until(fn, label) {
  const deadline = Date.now() + 45000;
  while (Date.now() < deadline) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label} timed out`);
}
async function start() {
  const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
  const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
  app = spawn(exe, [], { cwd: run, windowsHide: true, stdio: "ignore", env: { ...process.env,
    KANZEI_HOME: home, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"),
    APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: undefined, KANZEI_E2E_CDP: String(port),
  } });
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1500 }); return true; } catch { return false; } }, "WebView2 startup");
  page = browser.contexts()[0].pages()[0]; page.setDefaultTimeout(15000);
  page.on("pageerror", error => errors.push(String(error)));
  await page.waitForFunction(() => document.body.dataset.appReady === "true", { timeout: 45000 });
}
async function close() {
  if (page) await invoke("runtime_shutdown").catch(() => {});
  for (const entry of await readdir(path.join(home, "runtime"), { withFileTypes: true }).catch(() => [])) {
    if (!entry.isDirectory()) continue;
    const endpoint = JSON.parse(await readFile(path.join(home, "runtime", entry.name, "endpoint.json"), "utf8").catch(() => "null"));
    if (!endpoint || path.resolve(endpoint.executable || "").toLowerCase() !== exe.toLowerCase() || !/^127\.0\.0\.1:\d+$/.test(endpoint.addr || "")) continue;
    await new Promise(resolve => {
      const socket = net.createConnection({ host: "127.0.0.1", port: Number(endpoint.addr.split(":").at(-1)) });
      const finish = () => { socket.destroy(); resolve(); };
      socket.setTimeout(1500, finish); socket.on("error", finish); socket.on("data", finish);
      socket.on("connect", () => { const body = Buffer.from(JSON.stringify({ action: "shutdown", token: endpoint.token })), size = Buffer.alloc(4); size.writeUInt32BE(body.length); socket.write(Buffer.concat([size, body])); });
    });
  }
  await browser?.close(); browser = null; page = null;
  if (app && app.exitCode === null) { app.kill(); await new Promise(resolve => app.once("exit", resolve)); }
}
async function closedChat(root, title) {
  const item = await invoke("process_create", { projectDir: root, profile: "readonly" });
  await invoke("process_rename", { projectDir: root, processId: item.id, title });
  await invoke("process_close", { processId: item.id });
  return item;
}
try {
  await start();
  const runtimeDirs = await readdir(path.join(home, "runtime"));
  const profiles = await Promise.all(runtimeDirs.map(dir => readdir(path.join(home, "runtime", dir, "webview")).catch(() => [])));
  check(profiles.some(entries => entries.includes("EBWebView")), "Background WebView uses a separate profile without a test-only UI override");
  const root = await invoke("general_chat_open");
  check(await invoke("general_chat_location") === root, "Existing projectless storage is discoverable without navigation");
  const target = await closedChat(root, "旧对话可管理");
  const neighbor = await closedChat(root, "保留这段对话");
  check((await invoke("process_closed_list", { projectDir: root })).length === 2, "Both closed conversations are retained in SQLite");
  await close(); await start();
  await page.evaluate(async root => {
    const tree = await import("./12-session-tree.js");
    tree.setClosedOpen(root, true); await tree.loadClosedSessions(root, { force: true }); tree.invalidateSessionTree();
  }, root);
  check(await page.locator("#workbench-general-list [data-ctx='closed']").count() === 0, "Closed chats remain hidden after restart");
  check(await page.locator(".workbench-row-menu").count() === 0, "No ellipsis controls in the desktop sidebar");
  await page.locator("#workbench-chat-history").click();
  const targetRow = () => page.locator("#session-history [data-ctx='closed']").filter({ hasText: "旧对话可管理" });
  await targetRow().click({ button: "right" });
  await page.getByRole("menuitem", { name: /重命名/ }).click();
  await page.locator("#input-value").fill("重启后的历史"); await page.locator("#input-ok").click();
  await until(async () => (await invoke("process_closed_list", { projectDir: root })).some(item => item.id === target.id && item.title === "重启后的历史"), "Rename persisted");
  check(true, "Closed history can be renamed through the UI after restart");
  await page.keyboard.press("Escape");
  const prefs = await invoke("projects_add", { path: project });
  const projectHistory = await closedChat(prefs.current, "项目里的验收记录");
  await page.evaluate(async prefs => { const sessions = await import("./09-sessions.js"); await sessions.enterProject(prefs); }, prefs);
  const projectGroup = page.locator(`#workbench-project-list > .workbench-project[data-path=${JSON.stringify(prefs.current)}]`);
  const projectLink = projectGroup.locator(".workbench-project-link");
  check(await page.locator("#workbench-project-list .workbench-project-caret, #workbench-project-list [data-act='toggle']").count() === 0, "Native project rows have no separate expansion marker");
  await page.evaluate(async project => { const tree = await import("./12-session-tree.js"); tree.setSidebarOpen(project, false); tree.invalidateSessionTree(); }, prefs.current);
  await projectLink.click();
  await projectGroup.locator("[data-ctx='session']").first().waitFor();
  await projectLink.click();
  check(await projectLink.getAttribute("aria-expanded") === "true" && await projectGroup.locator(".workbench-session-list").isVisible(), "Native project name opens and keeps the conversation list expanded");
  const liveRows = page.locator("#workbench-project-list [data-ctx='session'], #workbench-general-list [data-ctx='session']");
  check(await liveRows.locator(".workbench-session-dot, .workbench-session-activity, .workbench-session-tag").count() === 0 && await page.locator("#workbench-general-list [data-ctx='session']").count() > 0, "Native project and projectless rows share the same presentation without dots or type badges");
  const projectMain = (await invoke("process_list", { projectDir: prefs.current })).find(item => item.kind === "main");
  const generalMain = (await invoke("process_list", { projectDir: root })).find(item => item.kind === "main");
  await invoke("process_rename", { projectDir: prefs.current, processId: projectMain.id, title: "原生项目标题核验" });
  await invoke("process_rename", { projectDir: root, processId: generalMain.id, title: "原生无项目标题核验" });
  await page.evaluate(async root => {
    const sessions = await import("./09-sessions.js"), tree = await import("./12-session-tree.js");
    await sessions.refreshProcesses(); await tree.loadRemoteSessions(root, { force: true }); tree.invalidateSessionTree();
  }, root);
  check(await projectGroup.locator(`[data-process-id=${JSON.stringify(projectMain.id)}] .workbench-session-name`).innerText() === "原生项目标题核验" && await page.locator(`#workbench-general-list [data-process-id=${JSON.stringify(generalMain.id)}] .workbench-session-name`).innerText() === "原生无项目标题核验", "Native project and projectless main conversations show their stored SQLite titles");
  await page.locator("#workbench-chat-history").click();
  const search = page.getByRole("searchbox", { name: "搜索对话", exact: true });
  await search.fill("重启后的历史");
  await page.locator("#session-history [data-ctx='closed']").filter({ hasText: "重启后的历史" }).waitFor();
  check(true, "Project view can search projectless SQLite history");
  await search.fill("项目里的验收记录");
  await page.locator("#session-history [data-ctx='closed']").filter({ hasText: "项目里的验收记录" }).waitFor();
  check(true, "The same search finds project history");
  await search.fill("重启后的历史");
  const old = page.locator("#session-history [data-ctx='closed']").filter({ hasText: "重启后的历史" });
  await old.click({ button: "right" });
  await page.getByRole("menuitem", { name: /删除对话/ }).click();
  await page.locator("#confirm-ok").click();
  await until(async () => !(await invoke("process_closed_list", { projectDir: root })).some(item => item.id === target.id), "Deletion persisted");
  check(true, "Deleting old history through the UI reaches Rust and SQLite");
  await invoke("process_purge", { projectDir: root, processId: target.id });
  check((await invoke("conversation_get", { projectDir: root, processId: target.id })).length === 0, "Stale reads of deleted history are empty");
  await invoke("conversation_list", { projectDir: root, processId: target.id });
  await invoke("conversation_trace_get", { projectDir: root, processId: target.id });
  check(!(await invoke("process_closed_list", { projectDir: root })).some(item => item.id === target.id), "Repeated deletion and stale reads never recreate a chat");
  const uiPrefs = await invoke("ui_prefs_get");
  await invoke("ui_prefs_set", { ui_layout: { ...(uiPrefs.ui_layout || uiPrefs), preview: { open: true } } });
  await page.keyboard.press("Escape");
  await close(); await start();
  check(await page.evaluate(() => !document.getElementById("view-chat").dataset.preview), "Restart does not reopen the preview browser");
  const remaining = await invoke("process_closed_list", { projectDir: root });
  check(!remaining.some(item => item.id === target.id) && remaining.some(item => item.id === neighbor.id), "Restart cannot resurrect deleted history or lose its neighbor");
  check((await invoke("process_closed_list", { projectDir: prefs.current })).some(item => item.id === projectHistory.id), "Deleting projectless history preserves project history");
  let refused = false;
  try { await invoke("process_rename", { projectDir: root, processId: target.id, title: "不能复活" }); } catch { refused = true; }
  check(refused, "Stale rename cannot recreate a deleted conversation");
  check(errors.length === 0, `No native UI errors: ${errors.join("; ")}`);
  await page.screenshot({ path: path.join(output, "sidebar-native.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ checks, errors, executable: exe, boundary: "Actual WebView2/Rust/SQLite with disposable home; no model calls" }, null, 2));
  console.log(`${checks.length} native sidebar checks passed`);
} catch (error) {
  if (page) await page.screenshot({ path: path.join(output, "failure.png") }).catch(() => {});
  console.error({ checks, errors }); throw error;
} finally { await close(); }
