// Actual native Kanzei, permission UI, Responses wire protocol and durable search receipts.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, writeFile, readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

assert(process.platform === "win32" && process.argv[2], "Usage: node scripts/ui-web-search-native-smoke.mjs <exe>");
const output = path.resolve("output/playwright/web-search/native"), run = path.join(output, String(Date.now()));
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
const source = "https://docs.example.test/verified", blocked = "https://blocked.example.test/secret";
const requests = [], mainRequests = [], results = {}, errors = [], checks = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
function respond(res, output, text = "") {
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  const send = event => res.write(`data: ${JSON.stringify(event)}\n\n`);
  for (const [index, item] of output.entries()) {
    send({ type: "response.output_item.added", output_index: index, item });
    if (text && item.type === "message") send({ type: "response.output_text.delta", output_index: index, content_index: 0, delta: text });
    send({ type: "response.output_item.done", output_index: index, item });
  }
  send({ type: "response.completed", response: { id: `fixture-response-${Date.now()}`, model: "test", status: "completed", output, usage: { input_tokens: 12, output_tokens: 5 } } });
  res.end();
}
const textReply = (res, text) => respond(res, [{ type: "message", role: "assistant", content: [{ type: "output_text", text, annotations: [] }] }], text);
const toolReply = (res, name, args, mode, step) => respond(res, [{ type: "function_call", id: `item-${mode}-${step}`, call_id: `call-${mode}-${step}`, name, arguments: JSON.stringify(args), status: "completed" }]);
const model = http.createServer(async (req, res) => {
  try {
    if (req.url === "/v1/models") { res.writeHead(200, { "Content-Type": "application/json" }); res.end(JSON.stringify({ data: [{ id: "test" }] })); return; }
    let raw = ""; for await (const chunk of req) raw += chunk;
    if (req.url !== "/v1/responses") { res.writeHead(404); res.end(); return; }
    const body = JSON.parse(raw);
    if (body.tools?.some(tool => tool.type === "web_search")) {
      requests.push(body);
      respond(res, [
        { type: "web_search_call", id: `search-${requests.length}`, status: "completed", action: { type: "search", queries: [body.input[0].content[0].text], sources: [{ type: "url", url: source }, { type: "url", url: blocked }] } },
        { type: "message", content: [{ type: "output_text", text: "中文🦉 verified", annotations: [{ type: "url_citation", title: "Verified source", url: source, start_index: 4, end_index: 12 }, { type: "url_citation", title: "Invented", url: "javascript:alert(1)", start_index: 500, end_index: 600 }] }] }
      ]); return;
    }
    const marker = body.input?.filter(item => item.role === "user").flatMap(item => item.content ?? []).map(part => part.text ?? "").findLast(text => text.startsWith("NATIVE_SEARCH_"));
    if (!marker) { textReply(res, "No memory candidates for this fixture."); return; }
    const mode = marker.split("：")[0], outputs = body.input.filter(item => item.type === "function_call_output");
    mainRequests.push({ mode, outputs: outputs.length, calls: body.input.filter(item => item.type === "function_call").map(item => item.name) });
    console.log(`Native search ${mode}: ${outputs.length} completed tools`);
    if (outputs.length === 0) { toolReply(res, "tool_search", { query: "select:websearch" }, mode, 0); return; }
    if (outputs.length === 1) { toolReply(res, "websearch", { queries: [{ q: `software acceptance ${mode}`, domains: ["example.test"], exclude_domains: ["blocked.example.test"] }] }, mode, 1); return; }
    results[mode] = outputs.at(-1).output;
    const allowed = mode === "NATIVE_SEARCH_ALLOW" || mode === "NATIVE_SEARCH_AUTO_ALLOW";
    textReply(res, allowed ? `SEARCH_COMPLETED：[Verified source](${source})` : "SEARCH_DECLINED：未派发原生搜索。");
  } catch (error) { errors.push(String(error)); textReply(res, `FIXTURE_FAILED: ${error.message}`); }
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary="stub:test"\nfast="stub:test"\n[providers.stub]\nprotocol="openai-responses"\nbase_url="http://127.0.0.1:${model.address().port}/v1"\ncontext_limit=128000\n[web]\nsearch_backend="responses"\n`);
const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve)); const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env, KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"), APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port) } });
let browser, page, database;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
async function until(predicate, label, timeout = 60000) { const deadline = Date.now() + timeout; while (Date.now() < deadline) { if (await predicate()) return; await new Promise(resolve => setTimeout(resolve, 100)); } throw new Error(label); }
try {
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`); return true; } catch { return false; } }, "Native app starts", 30000);
  page = browser.contexts()[0].pages()[0]; page.on("pageerror", error => errors.push(error.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.evaluate(() => { window.nativeSearchClicks = []; document.addEventListener("click", event => window.nativeSearchClicks.push({ id: event.target.id, detail: event.detail, trusted: event.isTrusted }), true); });
  await invoke("projects_init", { path: project, name: "网页搜索原生验收" }); await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.locator(".workbench-project-name").filter({ hasText: "网页搜索原生验收" }).click();
  const lines = [];
  for (const mode of ["NATIVE_SEARCH_DENY", "NATIVE_SEARCH_ALLOW", "NATIVE_SEARCH_UNATTENDED", "NATIVE_SEARCH_AUTO_ALLOW"]) {
    const line = await invoke("process_create", { projectDir: project, profile: "dev", model: "stub:test", subagentMode: "off" }); lines.push(line);
    await page.evaluate(async id => { const sessions = await import("./09-sessions.js"); await sessions.refreshProcesses(); await sessions.switchProcess(id, true); }, line.id);
    const before = requests.length, autonomous = mode === "NATIVE_SEARCH_UNATTENDED" || mode === "NATIVE_SEARCH_AUTO_ALLOW";
    await invoke("run_prompt", { projectDir: project, processId: line.id, prompt: `${mode}：检索指定范围并保留实际来源。`, profile: "dev", agent: "dev-pair", model: "stub:test", autonomous, autoAllow: mode === "NATIVE_SEARCH_AUTO_ALLOW" });
    if (!autonomous) {
      await until(async () => (await invoke("pending_asks_get", { projectDir: project, processId: line.id })).some(ask => ask.action === "websearch"), "Search waits for actual permission");
      check(requests.length === before, `${mode}: zero native HTTP requests before permission`);
      await page.locator("#ask-overlay").waitFor({ state: "visible" });
      await until(() => page.evaluate(async () => (await import("./07-events.js")).askArmed()), "Permission card reaches its actual armed state");
      await page.locator(mode === "NATIVE_SEARCH_DENY" ? "#ask-deny" : "#ask-allow").click();
    }
    await until(async () => { assert.equal(errors.length, 0, errors.join("; ")); const rows = await invoke("collaboration_snapshot", { projectDir: project }); const row = rows.find(row => row.process_id === line.id); return mode === "NATIVE_SEARCH_DENY" ? row?.status === "stopped" : results[mode] && row?.status === "completed"; }, `${mode} reaches its expected terminal state`);
    if (autonomous) await invoke("stop_run", { projectDir: project, processId: line.id });
    const allowed = mode === "NATIVE_SEARCH_ALLOW" || mode === "NATIVE_SEARCH_AUTO_ALLOW";
    check(requests.length === before + (allowed ? 1 : 0), `${mode}: actual native dispatch obeys interactive or unattended permission`);
    if (allowed) {
      const data = JSON.parse(results[mode]);
      check(data.queries[0].results.length === 1 && data.queries[0].results[0].url === source, `${mode}: only retrieved, permitted URLs become results`);
      check(data.queries[0].evidence.search_calls === 1 && data.queries[0].evidence.response_id, `${mode}: provider identity, usage and actual call receipt survive the tool pipeline`);
      const bytes = await readFile(path.join(project, data.receipt.artifact.path));
      check(createHash("sha256").update(bytes).digest("hex") === data.receipt.artifact.sha256, `${mode}: immutable search receipt hash matches actual bytes`);
    } else check(mode === "NATIVE_SEARCH_DENY" || results[mode].includes("[tool error]") || results[mode].includes("declined"), `${mode}: rejected search cancels the turn or returns a rejection, not empty success`);
  }
  check(requests.every(body => body.max_tool_calls === 1 && body.tool_choice.type === "web_search" && body.tools[0].filters.allowed_domains[0] === "example.test" && body.tools[0].filters.blocked_domains[0] === "blocked.example.test"), "Native wire requests enforce one call and both domain filters");
  database = new DatabaseSync(path.join(project, ".kanzei/state.db"), { readOnly: true });
  const facts = database.prepare("SELECT session_id,payload_json FROM session_events WHERE event_type='web.searched'").all();
  check(facts.length === 2 && facts.every(row => JSON.parse(row.payload_json).session_id === row.session_id && JSON.parse(row.payload_json).run_id), "Only dispatched searches produce durable receipts bound to their actual session and run");
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await until(() => page.locator(`a[href='${source}']`).count(), "Source link survives history reload");
  check(true, "Answer source is clickable after actual native history reload");
  check(await page.locator("a[href^='javascript:']").count() === 0, "Invalid provider annotation URLs never become clickable sources");
  await page.screenshot({ path: path.join(output, "native-search.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, requests, facts: facts.map(row => JSON.parse(row.payload_json)), executable: exe, boundary: "Actual native app, Rust runner, permission UI, Responses transport and SQLite; deterministic local provider, not live retrieval quality." }, null, 2));
  console.log(`Web search native PASS: ${checks.length} checks`);
} catch (error) {
  if (page && !page.isClosed()) console.log("Native search failure UI", await page.evaluate(async () => ({ clicks: window.nativeSearchClicks, active: (await import("./07-events.js")).askActive, arming: document.querySelector("#ask-dialog")?.dataset.arming, toast: document.querySelector("#toast")?.textContent })));
  if (page && !page.isClosed()) await page.screenshot({ path: path.join(output, "native-failure.png") });
  await writeFile(path.join(output, "failure.json"), JSON.stringify({ checks, requests, mainRequests, results, errors }, null, 2)); throw error;
} finally {
  database?.close();
  if (page && !page.isClosed()) await invoke("runtime_shutdown").catch(error => console.warn(String(error)));
  await browser?.close(); app.kill(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
}
