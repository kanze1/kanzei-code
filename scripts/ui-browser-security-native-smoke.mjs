// Actual Kanzei, WebView2, local static service and IPC capability boundaries.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, writeFile } from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

assert(process.platform === "win32" && process.argv[2], "Usage: node scripts/ui-browser-security-native-smoke.mjs <exe>");
const output = path.resolve("output/playwright/browser-security/native");
const run = path.join(output, String(Date.now()));
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
await writeFile(path.join(appHome, "kanzei.toml"), '[models]\nprimary = "stub:test"\nfast = "stub:test"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:1/v1"\ncontext_limit = 64000\n');
await writeFile(path.join(project, "first.html"), `<!doctype html><title>Preview security acceptance</title><h1>Native preview</h1><div id="ready">pending</div><script type="module">import { label } from './module.mjs'; const result=await fetch('private.txt'); document.querySelector('#ready').textContent=label+':'+result.status;</script>`);
await writeFile(path.join(project, "second.html"), '<!doctype html><title>Second origin</title><h1>Second origin</h1>');
await writeFile(path.join(project, "module.mjs"), 'export const label="module-ok";');
await writeFile(path.join(project, "private.txt"), "SIBLING_PRIVATE_MARKER");
const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env,
  KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"),
  APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port),
} });
let browser, main, preview;
const checks = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
const invoke = (command, args = {}) => main.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
try {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`); break; }
    catch { await new Promise(resolve => setTimeout(resolve, 250)); }
  }
  assert(browser, "Isolated native app starts");
  main = browser.contexts()[0].pages()[0];
  main.on("pageerror", error => errors.push(error.message));
  await main.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 30000 });
  await invoke("projects_init", { path: project, name: "浏览器安全原生验收" });
  await main.reload();
  await main.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 30000 });
  const line = await invoke("process_create", { projectDir: project, profile: "dev" });
  const open = target => invoke("preview_open", { target, processId: line.id, bounds: { x: 500, y: 100, w: 650, h: 600 } });
  const previewCreated = browser.contexts()[0].waitForEvent("page", { timeout: 15000 });
  const first = await open(path.join(project, "first.html"));
  check(first.url.includes(".localhost:"), "Local page receives an isolated origin from the real Rust server");
  preview = await previewCreated;
  await preview.waitForURL(first.url);
  assert(preview, "Native preview has a separate actual WebView2 target");
  preview.setDefaultTimeout(15000);
  await preview.locator("#ready").filter({ hasText: "module-ok:403" }).waitFor();
  check(true, "Native module loads and arbitrary sibling data returns 403");
  const ipc = await preview.evaluate(async () => {
    if (typeof window.__TAURI_INTERNALS__?.invoke !== "function") return { exposed: false };
    try { return { exposed: true, value: await window.__TAURI_INTERNALS__.invoke("app_info", {}) }; }
    catch (error) { return { exposed: true, denied: String(error) }; }
  });
  check(!ipc.exposed || Boolean(ipc.denied), "Untrusted preview cannot invoke app_info through Tauri IPC");
  await preview.evaluate(() => { localStorage.setItem("private", "first"); document.cookie = "private=first; SameSite=Lax"; });
  const second = await open(path.join(project, "second.html"));
  await preview.waitForURL(second.url);
  const isolated = await preview.evaluate(() => ({ storage: localStorage.getItem("private"), cookie: document.cookie }));
  check(isolated.storage === null && isolated.cookie === "", "Native page origins isolate localStorage and cookies");
  const cross = await preview.evaluate(async url => { try { await fetch(url); return "read"; } catch { return "blocked"; } }, first.url);
  check(cross === "blocked", "Native preview cannot read the other page origin");
  await open(path.join(project, "first.html")); await preview.waitForURL(first.url);
  check(await preview.evaluate(() => localStorage.getItem("private")) === "first", "Returning to the authorized page restores its own storage");
  let denied = false;
  try { await open("http://tauri.localhost/"); } catch { denied = true; }
  check(denied, "Registered application protocol origin remains denied");
  await main.screenshot({ path: path.join(output, "native-preview.png") });
  check(errors.length === 0, `Main UI has no page errors: ${errors.join("; ")}`);
  const info = await invoke("app_info");
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, ipc, info, executable: exe,
    boundary: "Actual native Kanzei and WebView2; local disposable project/profile; no model or user data accessed." }, null, 2));
  console.log(`Browser security native PASS: ${checks.length} checks`);
} catch (error) {
  console.error({ checks, pages: browser?.contexts().flatMap(context => context.pages().map(page => page.url())) });
  throw error;
} finally {
  if (main && !main.isClosed()) await invoke("preview_close").catch(() => {});
  await browser?.close(); app.kill();
}
