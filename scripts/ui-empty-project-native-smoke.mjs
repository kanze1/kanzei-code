// Actual WebView2, Rust IPC and SQLite, with a disposable home/local model.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Usage: node scripts/ui-empty-project-native-smoke.mjs <fresh kzapp.exe>");
const output = path.resolve("dist/sidebar-draft-20261007/native"), run = path.join(output, String(Date.now()));
const project = path.join(run, "project"), profile = path.join(run, "profile"), home = path.join(profile, ".kanzei");
for (const dir of [project, home, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(home, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
const checks = [], requests = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); console.log(`PASS ${label}`); };
const description = "核对空项目第一次发送的对话身份，保留输入内容。";
const reply = (res, delta, finish = "stop") => {
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: finish }], usage: { prompt_tokens: 80, completion_tokens: 20 } })}\n\n`);
  res.end("data: [DONE]\n\n");
};
const model = http.createServer(async (req, res) => {
  if (req.url !== "/v1/chat/completions") { res.writeHead(404); res.end(); return; }
  let raw = ""; for await (const part of req) raw += part;
  const data = JSON.parse(raw), messages = data.messages || [];
  const capture = messages.some(message => message.role === "system" && String(message.content).startsWith("将用户意图整理为简洁、可追踪、可验收的中文需求。"));
  requests.push({ model: data.model, capture, messages });
  const capturedTools = messages.filter(message => message.role === "tool").length;
  if (capture && capturedTools < 2) {
    reply(res, { tool_calls: [{ index: 0, id: "first-requirement", type: "function", function: { name: "req", arguments: JSON.stringify({
      ...(capturedTools === 0 ? { action: "list", reason: "deduplicate_registration" } : {
        action: "add", title: "核对首次发送的身份", requirement: {
          statement: description, acceptance: [{ text: "第一次发送仅创建一条对话，输入内容保留" }],
        },
      }),
    }) } }] }, "tool_calls");
  } else reply(res, { content: capture ? "R-001" : "本地验收回复：首次发送的身份已绑定。" });
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(home, "kanzei.toml"), `[models]\nprimary = "stub:base"\nfast = "stub:base"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n`);
const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env,
  KANZEI_HOME: home, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"),
  APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port),
} });
let browser, page;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
async function until(fn, label) {
  const deadline = Date.now() + 45000;
  while (Date.now() < deadline) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label} timed out`);
}
try {
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1500 }); return true; } catch { return false; } }, "WebView2 startup");
  page = browser.contexts()[0].pages()[0]; page.setDefaultTimeout(15000);
  page.on("pageerror", error => errors.push(error.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 45000 });
  const general = await invoke("general_chat_open");
  check((await invoke("process_list", { projectDir: general })).length === 0, "Cold startup and general navigation create no stored conversation");
  const prefs = await invoke("projects_add", { path: project });
  await page.evaluate(async prefs => (await import("./09-sessions.js")).enterProject(prefs), prefs);
  await page.evaluate(async project => (await import("./12-workbench.js")).openProjectSpace(project), prefs.current);
  check((await invoke("process_list", { projectDir: prefs.current })).length === 0, "Repeated native empty-project navigation keeps SQLite conversation list empty");
  await page.evaluate(async () => {
    const models = await import("./08-models.js");
    await models.setLineModel("stub:draft"); await models.setLineReasoning("medium");
  });
  const effective = await page.evaluate(async () => (await import("./08-models.js")).effectiveModel);
  check(effective.model.resolved === "stub:draft" && effective.reasoning.value === "medium"
    && (await invoke("process_list", { projectDir: prefs.current })).length === 0, "Rust resolves draft model/reasoning without allocating a conversation");
  await page.locator("#prompt").fill(description);
  await page.evaluate(async () => (await import("./26-project-conversations.js")).refreshConversationWork());
  await page.locator("#send").click();
  await until(async () => requests.some(request => !request.capture && request.model === "draft")
    && (await invoke("process_list", { projectDir: prefs.current })).some(item => !item.running), "First project task completed");
  const items = await invoke("process_list", { projectDir: prefs.current });
  check(items.length === 1 && items[0].model === "stub:draft" && items[0].reasoning === "medium", "Actual first send creates exactly one conversation with draft settings");
  const history = await invoke("conversation_display_get", { projectDir: prefs.current, processId: items[0].id });
  check(history.some(message => message.role === "user") && history.some(message => message.role === "assistant"), "The first native task and local reply are persisted in that conversation");
  check((await invoke("docs_snapshot", { projectDir: prefs.current })).requirements.length === 1, "Empty-project onboarding registers the real submitted requirement once");
  await page.locator("#prompt").fill("第二次实际任务"); await page.locator("#send").click();
  await until(async () => requests.filter(request => !request.capture && request.model === "draft").length >= 2
    && (await invoke("process_list", { projectDir: prefs.current })).every(item => !item.running), "Second task completed");
  check((await invoke("process_list", { projectDir: prefs.current })).length === 1, "Subsequent native sends reuse the same stored identity");
  await page.evaluate(async () => (await import("./29-general-chat.js")).openGeneralChat());
  check((await invoke("process_list", { projectDir: general })).length === 0, "Opening empty projectless chat still creates nothing");
  await page.locator("#prompt").fill("无项目实际任务"); await page.locator("#send").click();
  await until(async () => (await invoke("process_list", { projectDir: general })).some(item => !item.running)
    && requests.some(request => !request.capture && request.model === "base"), "First general task completed");
  check((await invoke("process_list", { projectDir: general })).length === 1, "The first native projectless send persists exactly one conversation");
  check(errors.length === 0, "No WebView2 JavaScript runtime errors");
  await page.screenshot({ path: path.join(output, "native-general.png") });
  await writeFile(path.join(output, "report.json"), JSON.stringify({ checks, errors, requests: requests.map(({ model, capture }) => ({ model, capture })),
    executable: exe, boundary: "Windows WebView2, real Rust IPC/SQLite, isolated test home and local deterministic model; no external model." }, null, 2));
  console.log(`Native empty project PASS: ${checks.length} checks`);
} catch (error) {
  if (page) await page.screenshot({ path: path.join(output, "failure.png") });
  await writeFile(path.join(output, "failure.json"), JSON.stringify({ checks, errors, requests }, null, 2));
  throw error;
} finally {
  if (page) await invoke("runtime_shutdown").catch(() => {});
  await browser?.close(); app.kill();
  model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
}
