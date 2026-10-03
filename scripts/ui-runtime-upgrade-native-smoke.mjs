// Reproduce an installed update while the old detached owner is still alive.
// Usage: node scripts/ui-runtime-upgrade-native-smoke.mjs <old exe> <new exe>
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { copyFile, mkdir, readFile, readdir, rename, writeFile } from "node:fs/promises";
import net from "node:net";
import http from "node:http";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[3]) throw new Error("Provide old and new Windows executables");
const output = path.resolve("output/playwright/runtime-upgrade"), run = path.join(output, String(Date.now()));
const profile = path.join(run, "profile"), home = path.join(profile, ".kanzei"), project = path.join(run, "project");
for (const dir of [home, project, path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(home, "app.json"), JSON.stringify({ projects: [project], current: project, theme: "light" }));
let finishReply;
const model = http.createServer(async (req, res) => {
  for await (const _chunk of req) { /* Drain the local fixture request. */ }
  finishReply = () => {
    res.writeHead(200, { "Content-Type": "text/event-stream" });
    res.end(`data: ${JSON.stringify({ choices: [{ index: 0, delta: { content: "UPGRADE_TASK_FINISHED" }, finish_reason: "stop" }] })}\n\ndata: [DONE]\n\n`);
  };
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(home, "kanzei.toml"), `[models]\nprimary = "stub:primary"\nfast = "stub:primary"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n`);
const exe = path.join(run, "kzapp.exe");
await copyFile(path.resolve(process.argv[2]), exe);
const env = { ...process.env, KANZEI_HOME: home, USERPROFILE: profile, HOME: profile,
  LOCALAPPDATA: path.join(profile, "AppData/Local"), APPDATA: path.join(profile, "AppData/Roaming") };
delete env.KANZEI_EMBEDDED_RUNTIME; delete env.KANZEI_E2E_CDP; delete env.WEBVIEW2_USER_DATA_FOLDER;
const checks = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); console.log(`PASS ${label}`); };
async function until(fn, label) {
  const deadline = Date.now() + 60000;
  while (Date.now() < deadline) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 150)); }
  throw new Error(`Timed out: ${label}`);
}
async function endpoint() {
  for (const entry of await readdir(path.join(home, "runtime"), { withFileTypes: true }).catch(() => [])) {
    const data = await readFile(path.join(home, "runtime", entry.name, "endpoint.json"), "utf8").catch(() => "null");
    const ep = JSON.parse(data);
    if (ep && path.resolve(ep.executable).toLowerCase() === exe.toLowerCase()) return ep;
  }
  return null;
}
function rpc(ep, input) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host: "127.0.0.1", port: Number(ep.addr.split(":").at(-1)) });
    let data = Buffer.alloc(0);
    socket.setTimeout(10000, () => socket.destroy(new Error("runtime timeout")));
    socket.on("error", reject);
    socket.on("connect", () => {
      const body = Buffer.from(JSON.stringify({ ...input, token: ep.token })), size = Buffer.alloc(4);
      size.writeUInt32BE(body.length); socket.write(Buffer.concat([size, body]));
    });
    socket.on("data", chunk => {
      data = Buffer.concat([data, chunk]);
      if (data.length < 4 || data.length < 4 + data.readUInt32BE(0)) return;
      const reply = JSON.parse(data.subarray(4, 4 + data.readUInt32BE(0))); socket.destroy();
      if (reply.ok) resolve(reply.value); else reject(new Error(String(reply.value)));
    });
  });
}
const command = (ep, name, args = {}) => rpc(ep, { action: "invoke", command: name, args });
let old, app, browser, page;
try {
  old = spawn(exe, ["--runtime-service"], { cwd: project, env, windowsHide: true, stdio: "ignore" });
  let previous;
  await until(async () => { previous = await endpoint(); return previous && await rpc(previous, { action: "ping" }).catch(() => null); }, "old runtime");
  check(previous.pid === old.pid, "Old runtime owns the original install path");
  // Legacy builds publish their endpoint before the hidden webview has navigated.
  await until(async () => { try { await command(previous, "process_list", { projectDir: project }); return true; } catch { return false; } }, "legacy IPC ready");
  const original = await command(previous, "process_create", { projectDir: project, profile: "readonly" });
  await command(previous, "process_rename", { projectDir: project, processId: original.id, title: "升级前的对话" });
  await command(previous, "process_close", { processId: original.id });
  let unsupported = false;
  try { await command(previous, "general_chat_location"); } catch (error) { unsupported = /not found/.test(String(error)); }
  console.log(`Old projectless command missing: ${unsupported}`);
  await command(previous, "run_prompt", { projectDir: project, prompt: "UPGRADE_BUSY_INPUT", model: "stub:primary", profile: "readonly", autonomous: false });
  await until(() => Boolean(finishReply), "old model request");
  await rename(exe, path.join(run, "previous.exe"));
  await copyFile(path.resolve(process.argv[3]), exe);
  const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
  const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
  app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore",
    env: { ...env, WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port) } });
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1200 }); return true; } catch { return false; } }, "new desktop");
  page = browser.contexts()[0].pages()[0]; page.on("pageerror", e => errors.push(String(e)));
  await page.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 60000 });
  const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
  await until(async () => { try { await invoke("runtime_status"); return false; } catch (error) { return String(error).includes("仍有任务运行"); } }, "busy runtime protection");
  check(old.exitCode === null, "Upgrade never stops an active old-version task");
  finishReply();
  await until(async () => { try { return (await invoke("runtime_status")).pid !== previous.pid; } catch { return false; } }, "idle runtime handover");
  const status = await invoke("runtime_status");
  check(status.pid !== previous.pid && Boolean(status.build), "New UI retires the stale owner and verifies the new build");
  check(old.exitCode !== null, "Old runtime exits gracefully");
  check((await invoke("process_list", { projectDir: project })).find(p => p.kind === "main")?.title === "UPGRADE_BUSY_INPUT", "Project sidebar title uses the actual user message instead of the main-role label");
  await until(async () => await page.locator("#workbench-project-list .workbench-session-name").filter({ hasText: "UPGRADE_BUSY_INPUT" }).isVisible(), "workspace and title restored in the actual sidebar");
  check(true, "Sidebar automatically restores its project and actual conversation title after the busy owner retires");
  const root = await invoke("general_chat_open");
  check(await invoke("general_chat_location") === root, "Projectless commands work immediately after upgrade");
  check((await invoke("process_closed_list", { projectDir: project })).some(p => p.id === original.id), "Upgrade preserves existing SQLite conversations");
  await invoke("process_purge", { projectDir: project, processId: original.id });
  await invoke("process_purge", { projectDir: project, processId: original.id });
  check(!(await invoke("process_closed_list", { projectDir: project })).some(p => p.id === original.id), "Previously closed history can be deleted after upgrade");
  check(await page.locator("#workbench-project-list [data-ctx='closed'], .workbench-row-menu").count() === 0, "Upgraded sidebar hides closed rows and ellipsis controls");
  check(!/Command .* not found/.test(await page.locator("body").innerText()), "Upgrade does not leak missing-command errors into the UI");
  check(errors.length === 0, "No desktop JavaScript errors during upgrade");
  await page.screenshot({ path: path.join(output, "runtime-upgrade.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ checks, errors, oldCommandMissing: unsupported, oldPid: previous.pid, newPid: status.pid, build: status.build }, null, 2));
} finally {
  const ep = await endpoint(); if (ep) await rpc(ep, { action: "shutdown" }).catch(() => {});
  await browser?.close().catch(() => {});
  for (const child of [old, app]) if (child && child.exitCode === null) child.kill();
  model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
}
