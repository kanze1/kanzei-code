// Actual native memory UI, versioned Rust feedback and durable SQLite facts.
/* global window, document */
import assert from "node:assert/strict";

import { mkdir, writeFile } from "node:fs/promises";
import net from "node:net";
import http from "node:http";
import path from "node:path";
import { chromium } from "playwright-core";
import { withTask, spawnOwned as spawn } from "./artifact-lifecycle.mjs";

await withTask({ suite: "ui-memory-feedback-native-smoke" }, async task => {

assert(process.platform === "win32" && process.argv[2], "Usage: node scripts/ui-memory-feedback-native-smoke.mjs <exe>");
const output = task.evidence, run = task.environment;
const project = path.join(run, "project"), other = path.join(run, "other"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [path.join(project, ".kanzei/memory"), path.join(other, ".kanzei/memory"), appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
const memoryFile = root => path.join(root, ".kanzei/memory/M-001-feedback.md");
const memory = "---\nid: M-001\nscope: project\ncategory: fact\ntitle: 保留反馈来源与版本\ndescription: 评价记忆内容时检查版本\nstatus: active\ncreated: 2026-10-18\nupdated: 2026-10-18\nsource: user\n---\n\n明确区分检索、注入、读取和用户采用。\n";
await writeFile(memoryFile(project), memory); await writeFile(memoryFile(other), memory);
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
const model = task.server(http.createServer((req, res) => {
  if (req.url !== "/v1/models") { res.writeHead(404); res.end(); return; }
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ object: "list", data: [{ id: "test", object: "model", owned_by: "fixture" }] }));
}));
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary="stub:test"\nfast="stub:test"\n[providers.stub]\nprotocol="openai"\nbase_url="http://127.0.0.1:${model.address().port}/v1"\ncontext_limit=64000\n`);
const probe = task.server(net.createServer()); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve)); const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await task.copyInput(path.resolve(process.argv[2]), exe);
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env, KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile,
  LOCALAPPDATA: path.join(profile, "AppData/Local"), APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port) } });
let browser, page; const checks = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
const entry = (root = project) => invoke("memory_entry_get", { projectDir: root, scope: "project", id: "M-001" });
try {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) { try { browser = await task.browser(chromium.connectOverCDP(`http://127.0.0.1:${port}`)); break; } catch { await new Promise(resolve => setTimeout(resolve, 250)); } }
  assert(browser, "Isolated native app starts"); page = browser.contexts()[0].pages()[0]; page.setDefaultTimeout(15000);
  page.on("pageerror", error => errors.push(error.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 30000 });
  await invoke("projects_init", { path: project, name: "记忆反馈原生验收" });
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.locator('button.activity-item[data-view="memory"]').click();
  await page.locator("#memory-project-picker").selectOption(project);
  await page.locator('#memory-list .memory-row[data-memory-id="M-001"]').click();
  const adopted = page.locator('[data-memory-feedback="adopted"]');
  await adopted.click();
  await page.locator(".memory-feedback-status").filter({ hasText: "采用反馈 1" }).waitFor();
  let value = await entry();
  check(value.usage.adopted === 1 && value.usage.injected === 0, "Actual UI adoption is durable and independent of injection counts");
  await adopted.click();
  await page.waitForFunction(() => !document.querySelector('[data-memory-feedback="adopted"]').disabled);
  check((await entry()).usage.adopted === 1, "Repeated native click on the same revision does not duplicate feedback");
  await page.locator('[data-memory-feedback="corrected"]').click();
  await page.locator(".memory-feedback-status").filter({ hasText: "纠正反馈 1" }).waitFor();
  value = await entry(); check(value.usage.last_feedback === "corrected", "Native correction records explicit user feedback");
  await page.locator('[data-memory-feedback="cleared"]').click();
  await page.waitForFunction(() => !document.querySelector('[data-memory-feedback="cleared"]').disabled);
  check((await entry()).usage.last_feedback === "cleared", "Feedback can be withdrawn without deleting its history");
  check((await entry(other)).usage.adopted === 0, "Same memory ID in another project does not inherit this project's feedback");
  const oldRevision = value.revision;
  await writeFile(memoryFile(project), memory.replace("明确区分检索", "新版本：明确区分检索"));
  let refused = false;
  try { await invoke("memory_entry_feedback", { projectDir: project, scope: "project", id: "M-001", expectedRevision: oldRevision, kind: "adopted" }); } catch { refused = true; }
  check(refused, "Actual Rust command rejects feedback from a stale detail revision");
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  check((await entry()).usage.last_feedback === "cleared", "Native reload retains the durable withdrawal and review history");
  check(errors.length === 0, `No native page errors: ${errors.join("; ")}`);
  await page.locator('button.activity-item[data-view="memory"]').click();
  await page.locator("#memory-project-picker").selectOption(project);
  await page.locator('#memory-list .memory-row[data-memory-id="M-001"]').click();
  await page.screenshot({ path: path.join(output, "native-memory-feedback.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, executable: exe,
    boundary: "Actual native memory UI + Rust commands + SQLite; isolated profile and two disposable projects; no model or user-memory corpus accessed." }, null, 2));
  console.log(`Memory feedback native PASS: ${checks.length} checks`);
} catch (error) { console.error({ checks, errors }); if (page && !page.isClosed()) await page.screenshot({ path: path.join(output, "failure.png") }); throw error; }
finally { await browser?.close(); app.kill(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve)); }
});
