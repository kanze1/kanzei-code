// Actual Kanzei -> permission -> Rust -> helper -> an already authenticated Edge.
/* global window, document */
import assert from "node:assert/strict";

import { mkdir, writeFile, readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";
import { actions } from "./fixtures/browser-form-actions.mjs";
import { decodeExternalSource } from "./fixtures/tool-source.mjs";
import { withTask, spawnOwned as spawn } from "./artifact-lifecycle.mjs";

await withTask({ suite: "ui-browser-connection-native-smoke" }, async task => {

assert(process.platform === "win32" && process.argv[2], "Usage: node scripts/ui-browser-connection-native-smoke.mjs <exe>");
const output = task.evidence, run = task.environment;
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(project, "attachment.txt"), "ACTUAL_AUTHORIZED_UPLOAD\n");
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
const frameSite = task.server(http.createServer((_req, res) => { res.writeHead(200, { "Content-Type": "text/html" }); res.end('<title>Selected iframe</title><p>Frame evidence from the selected existing browser</p><input id="frame-value" value="original-frame">'); }));
await new Promise(resolve => frameSite.listen(0, "127.0.0.1", resolve));
const frameUrl = `http://127.0.0.1:${frameSite.address().port}/embedded`;
const html = (await readFile("scripts/fixtures/browser-form.html", "utf8")).replace('<div id="root">', `<p id="authenticated">Existing login preserved</p><iframe id="embedded" src="${frameUrl}"></iframe><div id="root">`);
const react = await readFile("node_modules/react/umd/react.development.js"), reactDom = await readFile("node_modules/react-dom/umd/react-dom.development.js");
const site = task.server(http.createServer((req, res) => {
  if (req.url === "/react.js" || req.url === "/react-dom.js") { res.writeHead(200, { "Content-Type": "text/javascript" }); res.end(req.url === "/react.js" ? react : reactDom); return; }
  res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
  if (req.url === "/unrelated") res.end('<title>Unrelated tab</title><input id="unrelated" value="unchanged">');
  else res.end(req.headers.cookie?.includes("owned-login=ready") ? html : "<p>Login required</p>");
}));
await new Promise(resolve => site.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${site.address().port}`;
async function freePort() { const server = task.server(net.createServer()); await new Promise(resolve => server.listen(0, "127.0.0.1", resolve)); const port = server.address().port; await new Promise(resolve => server.close(resolve)); return port; }
const edgePort = await freePort(), appPort = await freePort(), endpoint = `http://127.0.0.1:${edgePort}/`;
const results = [], errors = [], checks = [], formActions = actions.slice(2);
let target, frameTarget;
const check = (value, label) => { assert(value, label); checks.push(label); };
function reply(res, delta) {
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: delta.tool_calls ? "tool_calls" : "stop" }], usage: { prompt_tokens: 100, completion_tokens: 20 } })}\n\n`);
  res.end("data: [DONE]\n\n");
}
const model = task.server(http.createServer(async (req, res) => {
  try {
    if (req.url === "/v1/models") { res.end(JSON.stringify({ data: [{ id: "test" }] })); return; }
    let raw = ""; for await (const chunk of req) raw += chunk;
    const messages = JSON.parse(raw).messages, user = messages.findLastIndex(message => message.role === "user" && JSON.stringify(message.content).includes("NATIVE_CONNECTED_FORM"));
    if (user < 0) { reply(res, { content: "No candidates for this fixture." }); return; }
    const outputs = messages.slice(user + 1).filter(message => message.role === "tool").map(message => ({ ...message, ...decodeExternalSource(message.content) }));
    if (outputs.length) {
      const last = outputs.at(-1).content; results[outputs.length - 1] = last;
      if (last.startsWith("backend:")) assert.equal(outputs.at(-1).source?.kind, "browser", "Existing browser observations carry an external source policy");
      assert(!last.startsWith("[tool error]") && !last.includes("[tool_outcome="), `Connected browser action failed: ${last}`);
    }
    if (outputs.length === 2) {
      const connected = JSON.parse(outputs[1].content.split("\n").find(line => line.startsWith("{")));
      const page = connected.pages.find(page => page.url === `${origin}/form`); assert(page);
      target = { connection_id: connected.connection_id, page_id: page.page_id, expected_url: page.url };
    }
    if (outputs.length === 4) {
      const observed = JSON.parse(outputs[3].content.split("\n").find(line => line.startsWith("{")));
      const frame = observed.frames.find(frame => !frame.main && frame.url === frameUrl); assert(frame);
      frameTarget = { ...target, frame_id: frame.frame_id, expected_frame_url: frame.url };
    }
    let name = "browser", input;
    if (!outputs.length) { name = "tool_search"; input = { query: "select:browser" }; }
    else if (outputs.length === 1) input = { action: "connect", endpoint };
    else if (outputs.length === 2) input = { action: "screenshot", ...target };
    else if (outputs.length === 3) input = { action: "frames", ...target };
    else if (outputs.length === 4) input = { action: "type", selector: "#frame-value", text: "native-frame-edit", ...frameTarget };
    else if (outputs.length === 5) input = { action: "source", ...frameTarget };
    else if (outputs.length < formActions.length + 6) input = { ...formActions[outputs.length - 6][1], ...target };
    else if (outputs.length === formActions.length + 6) input = { action: "disconnect", connection_id: target.connection_id };
    else { reply(res, { content: "NATIVE_CONNECTED_FORM_DONE" }); return; }
    console.log(`Connected native step ${outputs.length + 1}: ${input.action ?? input.query}`);
    reply(res, { tool_calls: [{ index: 0, id: `connected-${outputs.length}`, type: "function", function: { name, arguments: JSON.stringify(input) } }] });
  } catch (error) { errors.push(String(error)); reply(res, { content: `NATIVE_CONNECTED_FORM_FAILED: ${error.message}` }); }
}));
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary="stub:test"\nfast="stub:test"\n[providers.stub]\nprotocol="openai"\nbase_url="http://127.0.0.1:${model.address().port}/v1"\ncontext_limit=128000\n`);
const exe = path.join(run, "kzapp.exe"); await task.copyInput(path.resolve(process.argv[2]), exe);
let existing, original, unrelated, app, browser, page, database;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
async function until(predicate, label, timeout = 60000) { const deadline = Date.now() + timeout; while (Date.now() < deadline) { if (await predicate()) return; await new Promise(resolve => setTimeout(resolve, 100)); } throw new Error(label); }
try {
  existing = await task.browser(chromium.launchPersistentContext(path.join(run, "existing-edge"), { channel: "msedge", headless: true, args: [`--remote-debugging-port=${edgePort}`] }));
  await existing.addCookies([{ name: "owned-login", value: "ready", url: origin, httpOnly: true, sameSite: "Lax" }]);
  original = existing.pages()[0]; await original.goto(`${origin}/form`); await original.locator("#name").waitFor();
  unrelated = await existing.newPage(); await unrelated.goto(`${origin}/unrelated`);
  check(await original.locator("#authenticated").textContent() === "Existing login preserved", "An authenticated Edge tab exists before Kanzei starts");
  app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env, KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile,
    LOCALAPPDATA: path.join(profile, "AppData/Local"), APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(appPort) } });
  await until(async () => { try { browser = await task.browser(chromium.connectOverCDP(`http://127.0.0.1:${appPort}`)); return true; } catch { return false; } }, "Native app starts", 30000);
  page = browser.contexts()[0].pages()[0]; page.on("pageerror", error => errors.push(error.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await invoke("projects_init", { path: project, name: "现有 Edge 接管原生验收" }); await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.locator(".workbench-project-name").filter({ hasText: "现有 Edge 接管原生验收" }).click();
  const line = await invoke("process_create", { projectDir: project, profile: "dev-auto", model: "stub:test", subagentMode: "off" });
  await invoke("run_prompt", { projectDir: project, processId: line.id, prompt: `NATIVE_CONNECTED_FORM：使用我已授权的现有 Edge CDP ${endpoint}，只在 ${origin}/form 标签页填写 React 表单，授权上传 attachment.txt，校验更正、提交并保存正文，然后断开连接。`, profile: "dev-pair", agent: "dev-pair", model: "stub:test", autonomous: false, autoAllow: true });
  await until(async () => {
    assert.equal(errors.length, 0, errors.join("; "));
    for (const ask of await invoke("pending_asks_get", { projectDir: project, processId: line.id })) { assert.equal(ask.action, "browser"); await invoke("answer_ask", { id: ask.id, reply: "once" }); }
    return results.length === formActions.length + 7 && (await invoke("collaboration_snapshot", { projectDir: project })).find(row => row.process_id === line.id)?.status === "completed";
  }, "Actual native connection and form workflow completes", 90000);
  check(results[1].startsWith("backend: attached") && results[2].startsWith("backend: attached"), "The native runner connects and captures the selected existing tab");
  check(await original.frameLocator("#embedded").locator("#frame-value").inputValue() === "native-frame-edit", "Explicit iframe identity controls the intended cross-origin frame");
  check(results[5].includes("Frame evidence from the selected existing browser") && results[5].includes(frameUrl), "Cross-origin iframe source is captured from its authorized document");
  const submitted = JSON.parse(await original.locator("#result").textContent());
  assert.deepEqual(submitted.state, { name: "最后确认", email: "kanzei@example.test", date: "2026-10-18", city: "sh", tags: ["code", "music"], agreed: true, kind: "b", files: ["attachment.txt"] });
  check(true, "Existing-tab React state contains all actual form edits");
  check(submitted.submitted.file_text === "ACTUAL_AUTHORIZED_UPLOAD\n", "Existing-tab submission contains the exact authorized file bytes");
  check(await unrelated.locator("#unrelated").inputValue() === "unchanged", "The other pre-existing Edge tab remains unchanged");
  check(results.at(-1).includes('"disconnected":true') && !original.isClosed(), "Native disconnect leaves the original Edge browser and tabs open");
  database = new DatabaseSync(path.join(project, ".kanzei/state.db"), { readOnly: true });
  const row = database.prepare("SELECT payload_json FROM session_events WHERE session_id=? AND event_type='web.acquired' ORDER BY created_at DESC LIMIT 1").get(line.session_id); assert(row);
  const receipt = JSON.parse(row.payload_json).acquisition, bytes = await readFile(path.join(project, receipt.artifact.path));
  check(receipt.session_id === line.session_id && Boolean(receipt.run_id) && receipt.final_url === target.expected_url, "Connected source evidence preserves the selected page, conversation and run");
  check(createHash("sha256").update(bytes).digest("hex") === receipt.artifact.sha256 && bytes.toString().includes("Existing login preserved"), "Connected rendered source is saved with verifiable bytes and authentication-state evidence");
  await original.reload(); await original.locator("#name").waitFor();
  check(await original.locator("#authenticated").textContent() === "Existing login preserved", "Original login remains valid after native disconnect");
  await page.screenshot({ path: path.join(output, "native-connection.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, target, acquisition: receipt, actions: results.length, executable: exe, boundary: "Actual native Kanzei, permissions, Rust, Node helper and pre-existing authenticated Edge; disposable owned profiles and local site." }, null, 2));
  console.log(`Browser connection native PASS: ${checks.length} checks`);
} catch (error) { await writeFile(path.join(output, "failure.json"), JSON.stringify({ checks, results, errors }, null, 2)); throw error; }
finally {
  database?.close(); if (page && !page.isClosed()) await invoke("runtime_shutdown").catch(error => console.warn(String(error)));
  await browser?.close(); app?.kill(); await existing?.close();
  for (const server of [site, frameSite, model]) { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
}
});
