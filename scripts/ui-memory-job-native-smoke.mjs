// Actual WebView2, manager HTTP stream, cancellation, checkpoint and resumed queue.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, writeFile, readFile } from "node:fs/promises";
import net from "node:net";
import http from "node:http";
import path from "node:path";
import { chromium } from "playwright-core";

assert(process.platform === "win32" && process.argv[2], "Usage: node scripts/ui-memory-job-native-smoke.mjs <exe>");
const output = path.resolve("output/playwright/memory-job/native"), run = path.join(output, String(Date.now()));
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [path.join(project, ".kanzei/memory"), appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
const inbox = path.join(project, ".kanzei/memory/inbox.md");
await writeFile(inbox, "# Memory Inbox\n\n## note 2026-10-18 [fact]\n- note_id: native-note-1\n- summary: native-memory-job-source\nNo durable change is needed.\n");
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
const requests = []; let holding = true, stage = "starting"; const states = [], clicks = [], ipcRequests = [];
const reply = (res, delta) => {
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: delta.tool_calls ? "tool_calls" : "stop" }], usage: { prompt_tokens: 100, completion_tokens: 20 } })}\n\n`);
  res.end("data: [DONE]\n\n");
};
const model = http.createServer(async (req, res) => {
  if (req.url === "/v1/models") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ object: "list", data: [{ id: "test", object: "model" }, { id: "memory-role", object: "model" }] })); return;
  }
  if (req.url !== "/v1/chat/completions") { res.writeHead(404); res.end(); return; }
  let raw = ""; for await (const part of req) raw += part;
  const data = JSON.parse(raw); requests.push({ stage, model: data.model, tools: data.tools?.map(tool => tool.function.name) || [] });
  if (holding) return;
  if (!data.messages.some(message => message.role === "tool")) {
    reply(res, { tool_calls: [{ index: 0, id: "job-discard-1", type: "function", function: {
      name: "memory_inbox_discard", arguments: JSON.stringify({ fingerprint: "native-note-1", decision: "noop" }) } }] });
  } else reply(res, { content: "整理完成：这一条没有需要长期保留的信息。" });
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary="stub:test"\nfast="stub:memory-role"\n[providers.stub]\nprotocol="openai"\nbase_url="http://127.0.0.1:${model.address().port}/v1"\ncontext_limit=64000\n`);
const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve)); const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env, KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile,
  LOCALAPPDATA: path.join(profile, "AppData/Local"), APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port) } });
let browser, page; const checks = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
const waitStatus = status => page.locator("#memory-job-progress").filter({ hasText: {
  cancelled: "整理已停止，可继续", completed: "整理已完成",
}[status] }).waitFor({ state: "visible", timeout: 25000 });
try {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`); break; } catch { await new Promise(resolve => setTimeout(resolve, 250)); } }
  assert(browser, "Isolated native app starts");
  const context = browser.contexts()[0]; page = context.pages()[0] || await context.waitForEvent("page"); page.setDefaultTimeout(20000);
  await context.addInitScript(() => document.addEventListener("click", event => console.log(`fixture-click:${event.target.closest("button")?.id}`), true));
  page.on("console", message => { if (message.text().startsWith("fixture-click:")) clicks.push(message.text()); });
  page.on("request", request => { if (request.url().includes("memory_consolidate")) ipcRequests.push({ stage, url: request.url(), method:request.method() }); });
  page.on("pageerror", error => errors.push(error.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 30000 });
  await invoke("projects_init", { path: project, name: "记忆作业原生验收" });
  states.push({ stage: "before", runtime: await invoke("runtime_status"), appPid: app.pid });
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.locator('button.activity-item[data-view="memory"]').click();
  await page.locator("#memory-project-picker").selectOption(project);
  await page.locator("#memory-consolidate-btn").click();
  await page.locator("#memory-consolidate-stop").waitFor({ state: "visible" });
  const first = (await invoke("memory_job_status", { projectDir: project })).job;
  states.push(first);
  check(first.status === "running" && first.model === "memory-role" && first.steps === 1, "Native progress identifies the actual fast model and in-flight step");
  check(requests.length === 1 && !requests[0].tools.includes("memory_inbox_clear"), "One actual manager request has only the captured batch discard capability");
  const busy = await invoke("memory_consolidate", { projectDir: project, requestId: "native-contention" });
  check(busy.report.status === "busy" && requests.length === 1, "A second actual entry point reports busy without another model request");
  let staleRefused = false;
  try { await invoke("memory_job_stop", { projectDir: project, jobId: "previous-job" }); } catch { staleRefused = true; }
  check(staleRefused && (await invoke("memory_job_status", { projectDir: project })).job.status === "running", "A stale job ID cannot stop the current job");
  stage = "stopping"; await page.locator("#memory-consolidate-stop").click();
  await waitStatus("cancelled");
  states.push((await invoke("memory_job_status", { projectDir: project })).job);
  check((await readFile(inbox, "utf8")).includes("native-note-1"), "Actual stop preserves the unprocessed source note");
  const replay = await invoke("memory_consolidate", { projectDir: project, requestId: first.request_id });
  states.push({ replay: replay.report });
  check(replay.report.status === "cancelled" && replay.report.job.id === first.id && requests.length === 1,
    "An actual replay of the stopped request returns its durable receipt without starting another manager");
  stage = "reload"; await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  states.push({ stage: "reload", runtime: await invoke("runtime_status"), appPid: app.pid });
  await page.locator('button.activity-item[data-view="memory"]').click();
  await page.locator("#memory-project-picker").selectOption(project);
  check((await invoke("runtime_status")).pid === first.owner_pid, "Stopping a memory job and reloading leave the same detached runtime alive");
  await page.locator("#memory-job-progress").filter({ hasText: "整理已停止，可继续" }).waitFor();
  check((await invoke("memory_job_status", { projectDir: project })).job.id === first.id, "Native reload retains the previous cancellation checkpoint");
  holding = false;
  await page.locator("#memory-consolidate-btn").click();
  await waitStatus("completed");
  const last = (await invoke("memory_job_status", { projectDir: project })).job;
  check(last.id !== first.id && last.usage_reported && last.input_tokens === 200 && last.output_tokens === 40, "Resume uses a new owned job and persists actual reported usage");
  check(!(await readFile(inbox, "utf8")).includes("## note ") && requests.length === 3, "Resume consumes the preserved note exactly once and completes the actual tool round trip");
  const delayedReplay = await invoke("memory_consolidate", { projectDir: project, requestId: first.request_id });
  check(delayedReplay.report.status === "cancelled" && delayedReplay.report.job.id === first.id && requests.length === 3,
    "An older request remains idempotent after a later job has completed");
  check(errors.length === 0, `No native page errors: ${errors.join("; ")}`);
  await page.screenshot({ path: path.join(output, "native-memory-job.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, requests, first, last, executable: exe,
    boundary: "Actual native UI, Rust manager, HTTP streaming/cancellation and persistent checkpoint; disposable project and profile." }, null, 2));
  console.log(`Memory job native PASS: ${checks.length} checks`);
} catch (error) { if (page && !page.isClosed()) states.push((await invoke("memory_job_status", { projectDir: project })).job); console.error(JSON.stringify({ checks, errors, requests, states, clicks, ipcRequests },null,2)); if (page && !page.isClosed()) await page.screenshot({ path: path.join(output, "failure.png") }); throw error; }
finally {
  if (page && !page.isClosed()) {
    try {
      const active = await invoke("memory_job_status", { projectDir: project });
      if (active.can_stop) { await invoke("memory_job_stop", { projectDir: project, jobId: active.job.id }); await waitStatus("cancelled"); }
      await invoke("runtime_shutdown");
    } catch (error) { console.warn(`Isolated runtime cleanup: ${String(error)}`); }
  }
  await browser?.close(); app.kill(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
}
