// Real Kanzei runner, desktop bridge, WebView2, React 18 and a deterministic local provider.
/* global window, document, DOMParser */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, writeFile, readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";
import { actions } from "./fixtures/browser-form-actions.mjs";

assert(process.platform === "win32" && process.argv[2], "Usage: node scripts/ui-browser-forms-native-smoke.mjs <exe>");
const output = path.resolve("output/playwright/browser-forms/native"), run = path.join(output, String(Date.now()));
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await copyFile("scripts/fixtures/browser-form.html", path.join(project, "form.html"));
await copyFile("node_modules/react/umd/react.development.js", path.join(project, "react.js"));
await copyFile("node_modules/react-dom/umd/react-dom.development.js", path.join(project, "react-dom.js"));
await writeFile(path.join(project, "attachment.txt"), "ACTUAL_AUTHORIZED_UPLOAD\n");
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));

const rounds = { pane: [], headless: [], explicit: [], setup: [], stale: [] }, errors = [], checks = [];
let heldResponse, heldBackground;
const check = (value, label) => { assert(value, label); checks.push(label); };
function reply(res, delta) {
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: delta.tool_calls ? "tool_calls" : "stop" }], usage: { prompt_tokens: 100, completion_tokens: 20 } })}\n\n`);
  res.end("data: [DONE]\n\n");
}
const model = http.createServer(async (req, res) => {
  try {
    if (req.url === "/v1/models") { res.writeHead(200, { "Content-Type": "application/json" }); res.end(JSON.stringify({ data: [{ id: "test" }] })); return; }
    let raw = ""; for await (const part of req) raw += part;
    if (req.url !== "/v1/chat/completions") { res.writeHead(404); res.end(); return; }
    const messages = JSON.parse(raw).messages, user = messages.findLastIndex(message => message.role === "user" && JSON.stringify(message.content).includes("NATIVE_FORM_ACCEPTANCE"));
    if (user < 0) { reply(res, { content: "No memory candidates for this fixture." }); return; }
    const marker = JSON.stringify(messages[user].content);
    const mode = marker.includes("NATIVE_FORM_ACCEPTANCE_STALE") ? "stale" : marker.includes("NATIVE_FORM_ACCEPTANCE_SETUP") ? "setup" : marker.includes("NATIVE_FORM_ACCEPTANCE_EDGE") ? "explicit" : marker.includes("NATIVE_FORM_ACCEPTANCE_BG") ? "headless" : "pane";
    const results = rounds[mode];
    const outputs = messages.slice(user + 1).filter(message => message.role === "tool");
    if (outputs.length) {
      const last = outputs.at(-1).content;
      results[outputs.length - 1] = last;
      if (mode !== "stale") {
        assert(!last.startsWith("[tool error]"), `Tool failed at step ${outputs.length}: ${last}`);
        assert(!last.includes("[tool_outcome="), `Unexpected tool rejection at step ${outputs.length}: ${last}`);
        assert(!last.includes("浏览器工具失败"), `Browser operation failed at step ${outputs.length}: ${last}`);
      }
    }
    if (mode === "setup" || mode === "stale") {
      const plan = [["tool_search", { query: "select:browser" }], ["browser", mode === "setup" ? { action: "open", backend: "pane", path: "form.html" } : { action: "type", selector: "#name", text: "MUST_NOT_REACH_REPLACEMENT" }]];
      if (outputs.length >= plan.length) { reply(res, { content: `${mode.toUpperCase()}_DONE` }); return; }
      const [name, args] = plan[outputs.length];
      reply(res, { tool_calls: [{ index: 0, id: `native-${mode}-${outputs.length}`, type: "function", function: { name, arguments: JSON.stringify(args) } }] }); return;
    }
    if (mode === "explicit") {
      if (outputs.length === 1) { reply(res, { content: "EXPLICIT_EDGE_DONE" }); return; }
      reply(res, { tool_calls: [{ index: 0, id: "native-explicit-edge", type: "function", function: { name: "browser", arguments: JSON.stringify({ action: "open", backend: "headless", path: "form.html" }) } }] }); return;
    }
    if (outputs.length === actions.length && mode === "pane") { heldResponse = res; return; }
    if (outputs.length === actions.length && mode === "headless") { heldBackground = res; return; }
    if (outputs.length >= actions.length) { reply(res, { content: "NATIVE_FORM_DONE：已校验、纠正并提交，保存了渲染正文。" }); return; }
    const [name, args] = actions[outputs.length];
    const input = mode === "headless" && args.text === "最后确认" ? { ...args, text: "后台确认" } : args;
    console.log(`Native form ${mode} step ${outputs.length + 1}/${actions.length}: ${name} ${args.action ?? args.query}`);
    reply(res, { tool_calls: [{ index: 0, id: `native-form-${mode}-${outputs.length}`, type: "function", function: { name, arguments: JSON.stringify(input) } }] });
  } catch (error) { errors.push(String(error)); reply(res, { content: `NATIVE_FORM_FAILED: ${error.message}` }); }
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary="stub:test"\nfast="stub:test"\n[providers.stub]\nprotocol="openai"\nbase_url="http://127.0.0.1:${model.address().port}/v1"\ncontext_limit=128000\n`);
const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve)); const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env, KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile,
  LOCALAPPDATA: path.join(profile, "AppData/Local"), APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port) } });
let browser, page, database;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
async function until(predicate, label, timeout = 60000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await predicate()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(label);
}
async function validateRound(line, mode) {
  const results = rounds[mode], name = mode === "pane" ? "最后确认" : "后台确认";
  check(results.length >= actions.length && errors.length === 0, `${mode}: all ${actions.length} actual runner actions completed`);
  check(results[1].startsWith(`backend: ${mode}`), `${mode}: Rust runner selected the intended browser backend`);
  check(results[3].includes('"type":"email"'), `${mode}: DOM identifies typed form fields`);
  check(results[4].includes('"valid":false') && results[10].includes('"valid":false') && results[20].includes('"valid":true'), `${mode}: browser validation distinguishes missing, malformed and corrected fields`);
  check(results[5].includes('"value":""'), `${mode}: controlled text is actually cleared`);
  check(results[7].includes("Browser validation") && results[22].includes("Server validation"), `${mode}: browser and application validation errors are observed`);
  database ??= new DatabaseSync(path.join(project, ".kanzei/state.db"), { readOnly: true });
  const rows = database.prepare("SELECT session_id,payload_json FROM session_events WHERE event_type='web.acquired' AND session_id=? ORDER BY created_at DESC").all(line.session_id);
  assert(rows.length > 0); const receipt = JSON.parse(rows[0].payload_json).acquisition;
  check(rows.every(row => row.session_id === line.session_id) && receipt.session_id === line.session_id && Boolean(receipt.run_id), `${mode}: source receipt preserves the actual conversation and run`);
  const body = await readFile(path.join(project, receipt.artifact.path)), raw = await readFile(path.join(project, receipt.raw_artifact.path));
  check(createHash("sha256").update(body).digest("hex") === receipt.artifact.sha256 && createHash("sha256").update(raw).digest("hex") === receipt.raw_artifact.sha256, `${mode}: saved text and rendered DOM have independently verified SHA-256`);
  const submitted = await page.evaluate(html => JSON.parse(new DOMParser().parseFromString(html, "text/html").querySelector("#result").textContent), raw.toString());
  assert.deepEqual(submitted.state, { name, email: "kanzei@example.test", date: "2026-10-18", city: "sh", tags: ["code", "music"], agreed: true, kind: "b", files: ["attachment.txt"] });
  check(true, `${mode}: actual React state retains text/date/select/multiselect/checkbox/radio/file changes`);
  check(submitted.submitted.file_text === "ACTUAL_AUTHORIZED_UPLOAD\n" && submitted.submitted.name === name, `${mode}: submitted FormData contains the exact authorized file bytes and corrected state`);
  check(receipt.method === "rendered_dom" && receipt.http_status === null, `${mode}: rendered DOM declares its unknown HTTP status`);
  return { submitted, acquisition: receipt };
}
async function progress(lines) {
  for (const line of lines) for (const ask of await invoke("pending_asks_get", { projectDir: project, processId: line.id })) {
    assert(ask.action === "browser", `Only fixture browser approvals are expected: ${JSON.stringify(ask)}`);
    await invoke("answer_ask", { id: ask.id, reply: "once" });
  }
  assert.equal(errors.length, 0, errors.join("; "));
  return invoke("collaboration_snapshot", { projectDir: project });
}
try {
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`); return true; } catch { return false; } }, "Native app starts", 30000);
  page = browser.contexts()[0].pages()[0]; page.on("pageerror", error => errors.push(error.message));
  browser.contexts()[0].on("page", target => target.on("framenavigated", frame => { if (frame === target.mainFrame()) console.log(`Native preview navigation: ${frame.url()}`); }));
  await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await invoke("projects_init", { path: project, name: "表单与动态正文原生验收" });
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.locator(".workbench-project-name").filter({ hasText: "表单与动态正文原生验收" }).click();
  const create = () => invoke("process_create", { projectDir: project, profile: "dev", model: "stub:test", subagentMode: "off" });
  const select = id => page.evaluate(async id => { const sessions = await import("./09-sessions.js"); await sessions.refreshProcesses(); await sessions.switchProcess(id, true); }, id);
  const start = (line, marker) => invoke("run_prompt", { projectDir: project, processId: line.id, prompt: `${marker}：填写本地 React 表单，检查错误、纠正后提交，授权上传 attachment.txt，并保存当前渲染正文。`, profile: "dev", agent: "dev-pair", model: "stub:test", autonomous: false, autoAllow: true });
  const foreground = await create(); await select(foreground.id); await start(foreground, "NATIVE_FORM_ACCEPTANCE");
  await until(async () => { await progress([foreground]); return rounds.pane.length === actions.length && heldResponse; }, "Foreground form and source complete", 90000);
  const foregroundEvidence = await validateRound(foreground, "pane");
  const preview = browser.contexts()[0].pages().find(target => target.url().includes(".localhost:")); assert(preview, "Actual WebView2 target exists");
  assert.equal(JSON.parse(await preview.locator("#result").textContent()).state.name, "最后确认");
  await page.locator("#preview-close").click();
  const background = await create(); await select(foreground.id); await start(background, "NATIVE_FORM_ACCEPTANCE_BG");
  await until(async () => { await progress([foreground, background]); return rounds.headless.length === actions.length && heldBackground; }, "Background Edge form completes while foreground provider is held", 90000);
  const backgroundEvidence = await validateRound(background, "headless");
  await select(background.id);
  reply(heldBackground, { tool_calls: [{ index: 0, id: "native-switch-open", type: "function", function: { name: "browser", arguments: JSON.stringify({ action: "open", path: "form.html" }) } }] }); heldBackground = null;
  await until(async () => { const rows = await progress([foreground, background]); return rounds.headless.length === actions.length + 1 && rows.find(row => row.process_id === background.id)?.status === "completed"; }, "Switched conversation keeps its original browser");
  check(rounds.headless.at(-1).startsWith("backend: headless"), "Switching a background conversation to the foreground does not migrate its next navigation into WebView2");
  const rows = await invoke("collaboration_snapshot", { projectDir: project });
  check(rows.find(row => row.process_id === foreground.id)?.running && rows.find(row => row.process_id === background.id)?.status === "completed", "Two real conversations run concurrently and background Edge completes independently");
  check(JSON.parse(await preview.locator("#result").textContent()).state.name === "最后确认", "Background Edge does not change the hidden foreground WebView2 form");
  await select(foreground.id);
  reply(heldResponse, { tool_calls: [{ index: 0, id: "native-hidden-source", type: "function", function: { name: "browser", arguments: JSON.stringify({ action: "source" }) } }] }); heldResponse = null;
  await until(async () => { const rows = await progress([foreground]); return rounds.pane.length === actions.length + 1 && rows.find(row => row.process_id === foreground.id)?.status === "completed"; }, "Hidden foreground page remains operable on the same backend");
  check(rounds.pane.at(-1).startsWith("backend: pane") && rounds.pane.at(-1).includes("最后确认"), "A hidden page is captured through its original pane without switching browser or losing its form");
  const explicit = await create(); await select(explicit.id); await start(explicit, "NATIVE_FORM_ACCEPTANCE_EDGE");
  await until(async () => { const rows = await progress([explicit]); return rounds.explicit.length === 1 && rows.find(row => row.process_id === explicit.id)?.status === "completed"; }, "Foreground explicitly selects an independent Edge browser");
  check(rounds.explicit[0].startsWith("backend: headless"), "An explicit headless backend opens independent Edge even in the foreground");
  await until(() => page.evaluate(async () => (await import("./00-surface.js")).surfaceElements().every(surface => surface.type === "tooltip")), "Visible-browser capture begins after background approval notifications are dismissed");
  await start(explicit, "NATIVE_FORM_ACCEPTANCE_SETUP");
  await until(async () => { const rows = await progress([explicit]); return rounds.setup.length === 2 && rows.find(row => row.process_id === explicit.id)?.status === "completed"; }, "Explicit selection switches this conversation into the internal pane");
  check(rounds.setup[1].startsWith("backend: pane"), "Explicit backend selection switches an existing headless conversation into its own internal pane");
  const stalePage = browser.contexts()[0].pages().find(target => target.url().includes(".localhost:") && !target.isClosed()); assert(stalePage);
  const staleUrl = stalePage.url();
  await invoke("run_prompt", { projectDir: project, processId: explicit.id, prompt: "NATIVE_FORM_ACCEPTANCE_STALE：修改姓名字段。", profile: "dev", agent: "dev-pair", model: "stub:test", autonomous: false, autoAllow: false });
  let staleAsk;
  await until(async () => { staleAsk = (await invoke("pending_asks_get", { projectDir: project, processId: explicit.id })).find(ask => ask.action === "browser"); return Boolean(staleAsk); }, "Browser action waits for actual approval");
  await invoke("preview_close");
  await invoke("preview_open", { target: staleUrl, processId: explicit.id, bounds: { x: 420, y: 80, w: 800, h: 600 } });
  let replacement;
  await until(() => { replacement = browser.contexts()[0].pages().find(target => target.url() === staleUrl && !target.isClosed()); return Boolean(replacement); }, "Replacement WebView2 target finishes navigation to the same URL");
  await replacement.locator("#name").waitFor();
  await invoke("answer_ask", { id: staleAsk.id, reply: "once" });
  await until(async () => { const rows = await progress([explicit]); return rounds.stale.length === 2 && rows.find(row => row.process_id === explicit.id)?.status === "completed"; }, "Stale approved action returns without modifying replacement page");
  check(rounds.stale[1].includes("预览页面已变化"), "Actual approval rejects a replaced WebView2 page at the same URL");
  check(await replacement.locator("#name").inputValue() !== "MUST_NOT_REACH_REPLACEMENT", "The replacement page receives zero stale field mutations");
  await page.screenshot({ path: path.join(output, "native-form.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, actions: actions.map(([tool, args]) => ({ tool, action: args.action })), foreground: foregroundEvidence, background: backgroundEvidence, executable: exe, boundary: "Actual native app, runner, WebView2, controlled Edge, React 18.3.1 and SQLite; two concurrent conversations, deterministic local provider and disposable project/profile." }, null, 2));
  console.log(`Browser forms native PASS: ${checks.length} checks`);
} catch (error) {
  if (page && !page.isClosed()) console.log("Native UI failure state", await page.evaluate(async () => ({ preview: (await import("./24-preview.js")).previewState(), surfaces: (await import("./00-surface.js")).surfaceElements().map(surface => ({ type: surface.type, id: surface.el.id, text: surface.el.textContent.slice(0, 100) })) })));
  if (page && !page.isClosed()) await page.screenshot({ path: path.join(output, "native-failure.png") });
  await writeFile(path.join(output, "failure.json"), JSON.stringify({ checks, rounds, errors }, null, 2)); throw error;
} finally {
  heldResponse?.destroy();
  heldBackground?.destroy();
  database?.close();
  if (page && !page.isClosed()) await invoke("runtime_shutdown").catch(error => console.warn(String(error)));
  await browser?.close(); app.kill(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
}
