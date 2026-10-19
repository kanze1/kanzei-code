// Actual WebView2 + Rust runner + two sessions and a deterministic local model.
/* global window, document */
import assert from "node:assert/strict";

import { mkdir, writeFile, readFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";
import { withTask, spawnOwned as spawn } from "./artifact-lifecycle.mjs";

await withTask({ suite: "ui-run-concurrency-native-smoke" }, async task => {

assert(process.platform === "win32" && process.argv[2], "Usage: node scripts/ui-run-concurrency-native-smoke.mjs <exe>");
const output = task.evidence, run = task.environment;
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(project, "known.txt"), "NATIVE_SOURCE_EVIDENCE");
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
const requests = [], checks = [], errors = [];
let heldResponse;
const reply = (res, delta) => {
  const tool = Boolean(delta.tool_calls);
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: tool ? "tool_calls" : "stop" }], usage: { prompt_tokens: 100, completion_tokens: 20 } })}\n\n`);
  res.end("data: [DONE]\n\n");
};
const tool = (res, id, name, args) => reply(res, { tool_calls: [{ index: 0, id, type: "function", function: { name, arguments: JSON.stringify(args) } }] });
const model = task.server(http.createServer(async (req, res) => {
  let raw = ""; for await (const part of req) raw += part;
  if (req.url !== "/v1/chat/completions") { res.writeHead(404); res.end(); return; }
  const data = JSON.parse(raw), messages = data.messages || [];
  const userIndex = messages.findLastIndex(message => message.role === "user"), prompt = JSON.stringify(messages[userIndex]?.content);
  const kind = prompt.includes("NATIVE_PARALLEL_A") ? "a" : prompt.includes("NATIVE_PARALLEL_B") ? "b" : "other";
  const results = messages.slice(userIndex + 1).filter(message => message.role === "tool");
  requests.push({ kind, messages, model: data.model });
  if (kind === "a") { heldResponse = res; return; }
  if (kind !== "b") { errors.push(`unexpected model request: ${kind}`); reply(res, { content: "unexpected" }); return; }
  if (!results.length) tool(res, "native-read", "read", { path: "known.txt" });
  else if (results.length === 1) {
    assert(JSON.stringify(results[0]).includes("NATIVE_SOURCE_EVIDENCE"), "Actual Rust read returns the fixture bytes");
    tool(res, "native-write", "write", { path: "b-result.txt", content: "NATIVE_COMPLETED_B" });
  } else reply(res, { content: "NATIVE_COMPLETED_B: 已读取来源并写出结果。" });
}));
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary="stub:test"\nfast="stub:test"\n[providers.stub]\nprotocol="openai"\nbase_url="http://127.0.0.1:${model.address().port}/v1"\ncontext_limit=64000\n`);
const probe = task.server(net.createServer()); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve)); const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await task.copyInput(path.resolve(process.argv[2]), exe);
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env, KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile,
  LOCALAPPDATA: path.join(profile, "AppData/Local"), APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port) } });
let browser, page;
const check = (value, label) => { assert(value, label); checks.push(label); };
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
async function until(fn, label) {
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(label);
}
try {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) { try { browser = await task.browser(chromium.connectOverCDP(`http://127.0.0.1:${port}`)); break; } catch { await new Promise(resolve => setTimeout(resolve, 250)); } }
  assert(browser, "Isolated native app starts"); page = browser.contexts()[0].pages()[0];
  page.on("pageerror", error => errors.push(error.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 30000 });
  await invoke("projects_init", { path: project, name: "并发调查原生验收" });
  const create = () => invoke("process_create", { projectDir: project, profile: "dev", model: "stub:test", subagentMode: "off" });
  const a = await create(), b = await create();
  const start = (line, prompt) => invoke("run_prompt", { projectDir: project, processId: line.id, prompt, profile: "dev", agent: "dev-pair", model: "stub:test", autonomous: false, autoAllow: false });
  await start(a, "NATIVE_PARALLEL_A: 保持当前调查请求，等待本机测试服务完成。");
  await until(() => Boolean(heldResponse), "First dev run must reach the actual provider");
  await start(b, "NATIVE_PARALLEL_B: 读取 known.txt 并把完成结果写入 b-result.txt。");
  await until(async () => (await invoke("collaboration_snapshot", { projectDir: project })).find(row => row.process_id === b.id)?.status === "awaiting_approval", "Second run reaches its actual write approval while first provider is held");
  const pending = await invoke("pending_asks_get", { projectDir: project, processId: b.id });
  check(pending.length === 1, "Interactive write waits for one real approval and does not publish completion");
  await invoke("answer_ask", { id: pending[0].id, reply: "once" });
  await until(async () => { const rows = await invoke("process_list", { projectDir: project }); return requests.filter(request => request.kind === "b").length === 3 && !rows.find(row => row.id === b.id)?.running; }, "Second dev run must complete while the first provider is held");
  check((await readFile(path.join(project, "b-result.txt"), "utf8")) === "NATIVE_COMPLETED_B", "Second session performed actual Rust file read and write while first session was thinking");
  let rows = await invoke("collaboration_snapshot", { projectDir: project });
  check(rows.find(row => row.process_id === a.id)?.running && rows.find(row => row.process_id === b.id)?.status === "completed", "Native collaboration reports first run active and second durably completed");
  check(rows.every(row => !(row.waiting_writers || []).length), "Thinking run does not reserve or queue a mutation lease");
  reply(heldResponse, { content: "NATIVE_COMPLETED_A" }); heldResponse = null;
  await until(async () => (await invoke("process_list", { projectDir: project })).every(row => !row.running), "First run finishes after its provider responds");
  const facts = requests.filter(request => request.kind === "a" || request.kind === "b").map(request => {
    const system = request.messages.filter(message => message.role === "system").map(message => typeof message.content === "string" ? message.content : message.content.map(part => part.text || "").join("\n")).join("\n");
    const match = system.match(/<engine_run_facts>\s*(\{[^\n]*\})\s*<\/engine_run_facts>/); assert(match, "Engine facts reach actual provider request"); return JSON.parse(match[1]);
  });
  check(facts.every(fact => fact.provider === "stub" && fact.model === "test" && fact.local_date === new Date(fact.started_at_utc).toLocaleDateString("en-CA") && fact.utc_offset && fact.input_id), "Actual provider request has runtime date, timezone, input identity and resolved model");
  check(new Set(facts.map(fact => fact.run_id)).size === 2, "Concurrent sessions keep distinct engine-owned run identities");
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  rows = await invoke("collaboration_snapshot", { projectDir: project });
  const completed = rows.filter(row => row.process_id === a.id || row.process_id === b.id);
  check(completed.length === 2 && completed.every(row => row.status === "completed"), "Native UI reload preserves the completed outcomes");
  check(errors.length === 0, `No native page or model protocol errors: ${errors.join("; ")}`);
  await page.screenshot({ path: path.join(output, "native-concurrency.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, requests: requests.map(request => ({ kind: request.kind, model: request.model })), facts, executable: exe,
    boundary: "Actual native Rust runner, SQLite and WebView2; two dev conversations on the same tree; deterministic local provider, isolated fixture." }, null, 2));
  console.log(`Run concurrency native PASS: ${checks.length} checks`);
} catch (error) {
  const lines = page && !page.isClosed() ? await invoke("collaboration_snapshot", { projectDir: project }) : [];
  const diagnostic = { checks, requests: requests.map(request => request.kind), errors, lines };
  await writeFile(path.join(output, "failure.json"), JSON.stringify(diagnostic, null, 2));
  console.error(diagnostic); throw error;
}
finally { heldResponse?.destroy(); await browser?.close(); app.kill(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve)); }
});
