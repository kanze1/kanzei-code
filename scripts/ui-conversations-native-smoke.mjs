// Real Tauri, scoped registration, FIFO inputs and independent conversations.
/* global window, document */
import assert from "node:assert/strict";

import { mkdir, readFile, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";
import { withTask, spawnOwned as spawn } from "./artifact-lifecycle.mjs";

await withTask({ suite: "ui-conversations-native-smoke" }, async task => {

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Usage: node scripts/ui-conversations-native-smoke.mjs <fresh kzapp.exe>");
const output = task.evidence, run = task.environment;
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
await writeFile(path.join(project, "original.txt"), "original content");
const checks = [], requests = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
let mainReply, servedLinkPages = 0;
const captureDescription = "核对 original.txt，并保留主对话、只读讨论和排队执行的关系。";
function answer(res, content, tool, toolName = "write") {
  if (res.destroyed || res.writableEnded) return;
  const delta = tool ? { tool_calls: [{ index: 0, id: "native-tool", type: "function", function: { name: toolName, arguments: JSON.stringify(tool) } }] } : { content };
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  res.write(`data: ${JSON.stringify({ id: "local", object: "chat.completion.chunk", choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: tool ? "tool_calls" : "stop" }], usage: { prompt_tokens: 80, completion_tokens: 20 } })}\n\n`);
  res.end("data: [DONE]\n\n");
}
const model = task.server(http.createServer(async (req, res) => {
  if (req.method === "GET" && req.url === "/input-link") {
    servedLinkPages += 1; res.writeHead(200, { "Content-Type":"text/html; charset=utf-8" });
    res.end("<!doctype html><title>Input link acceptance</title><h1>Native link opened</h1>"); return;
  }
  let body = ""; for await (const part of req) body += part;
  if (req.url !== "/v1/chat/completions") { res.writeHead(404); res.end(); return; }
  const data = JSON.parse(body), messages = data.messages || [];
  // Chat Completions flattens attachment parts into additional user messages.
  const text = JSON.stringify(messages.findLast(m => m.role === "user" && typeof m.content === "string" && !m.content.startsWith("[external_source_data]\n"))?.content || "");
  const tools = data.tools || [];
  const kind = tools.length === 1 && tools[0].function?.name === "req" ? "capture"
    : text.includes("READONLY_ATTEMPT") ? "discussion" : text.includes("HOLD_MAIN") ? "hold" : text.includes("ATTACHMENT_INPUT") ? "attachment"
      : text.includes("QUEUED_EXECUTION") ? "queued" : "implementation";
  requests.push({ kind, messages });
  if (kind === "capture") {
    if (messages.some(m => m.role === "tool")) answer(res, "R-001");
    else answer(res, null, { action: "add", title: "确认独立对话与排队身份", tag: "前端", priority: "P1", requirement: {
      statement: captureDescription, acceptance: [{ text: "文件内容不变，对话身份独立且排队输入按顺序执行" }],
    } }, "req");
  } else if (kind === "hold") mainReply = () => answer(res, "主对话本轮结束。");
  else if (kind === "discussion") {
    if (messages.some(m => m.role === "tool")) answer(res, "只读讨论不能修改文件，结论应交给主对话。");
    else answer(res, null, { path: "original.txt", content: "must not be written" });
  } else answer(res, "本轮已核对输入范围，没有修改代码，没有声明交付。");
}));
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary = "stub:test"\nfast = "stub:test"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n`);
const probe = task.server(net.createServer()); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await task.copyInput(path.resolve(process.argv[2]), exe);
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env,
  KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"),
  APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port),
} });
let browser, page;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
async function until(fn, label = "Native condition") {
  const deadline = Date.now() + 45000;
  while (Date.now() < deadline) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label} timed out`);
}
try {
  await until(async () => { try { browser = await task.browser(chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1500 })); return true; } catch { return false; } }, "WebView2 start");
  page = browser.contexts()[0].pages()[0]; page.setDefaultTimeout(15000);
  page.on("pageerror", e => errors.push(e.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 30000 });
  check((await invoke("projects_get")).projects.length === 0, "Native tests use only the disposable profile");
  await invoke("projects_init", { path: project, name: "对话协作验收" });
  const receipt = await invoke("quick_req", { projectDir: project, kind: "req", description: captureDescription });
  const captured = (await invoke("docs_snapshot", { projectDir: project })).requirements.find(r => r.id === "R-001");
  check(receipt.startsWith("R-001") && captured, "Explicit registration returns a real persisted requirement");
  check(requests.some(request => request.kind === "capture" && request.messages.some(m => m.role === "tool" && JSON.stringify(m).includes("R-001"))), "Registration finishes with a tool receipt instead of a model-only claim");
  let captureCalls = requests.length;
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 30000 });
  await page.evaluate(async () => { const prefs = await window.__TAURI__.core.invoke("projects_get"); await (await import("./09-sessions.js")).enterProject(prefs); });
  const created = await invoke("process_create", { projectDir: project, profile: "dev-auto", model: "stub:test", subagentMode: "auto" });
  await page.evaluate(async id => { const sessions = await import("./09-sessions.js"); await sessions.refreshProcesses(); await sessions.switchProcess(id, true); }, created.id);
  const owner = (await invoke("process_list", { projectDir: project })).find(p => !["readonly", "research"].includes(p.profile));
  assert(owner, "Main executor exists");
  check(await page.locator("body").getAttribute("data-view") === "chat", "Native project enters its main conversation");
  await page.locator("#composer-work-next").waitFor({ state: "visible" });
  check((await page.locator("#composer-work-next").innerText()).includes("R-001"), "Registered requirements are visible beside the native conversation");
  await page.screenshot({ path: path.join(output, "conversation-dark.png") });
  await invoke("process_update", { processId: owner.id, subagentMode: "auto", });
  await page.evaluate(async () => {
    window.__conversationEvents = [];
    for (const name of ["kz:tool-start", "kz:tool-end", "kz:error", "kz:done"]) await window.__TAURI__.event.listen(name, event => window.__conversationEvents.push({ name, payload: event.payload }));
  });
  const prompt = { projectDir: project, processId: owner.id, agent: "dev-auto", profile: "dev-auto", model: "stub:test", autonomous: false, autoAllow: false };
  await page.locator("#preview-toggle").click();
  const header = await page.locator("#workspace-header").boundingBox(), preview = await page.locator("#preview-dock").boundingBox();
  check(header.y + header.height <= preview.y + 1 || header.x + header.width <= preview.x + 1, "Native preview does not cover conversation controls");
  check(await page.locator("#new-chat").isVisible(), "Native project sidebar includes new conversation");
  await page.locator("#preview-close").click();
  const localPage = `http://127.0.0.1:${model.address().port}/input-link`;
  await page.locator("#prompt").fill(localPage);
  await page.locator("#composer-links a").click();
  await until(() => servedLinkPages > 0, "Composer URL opens native child webview");
  check(await page.locator("#view-chat").getAttribute("data-preview") === "open", "Composer link loads a real page in the native preview");
  await page.locator("#preview-close").click();
  await page.locator("#attachment-input").setInputFiles(path.resolve("tests/fixtures/attachments/budget.xlsx"));
  await page.locator('#attachments [data-kind="sheet"]').waitFor();
  await page.locator("#prompt").fill(`ATTACHMENT_INPUT: 查看表格，以及 https://github.com/acme/repo 和 ${localPage}`);
  check(await page.locator('#composer-links [data-kind="git"]').isVisible(), "Native composer recognizes the repository link");
  await page.locator("#send").click();
  await until(async () => requests.some(r=>r.kind==="attachment") && !(await invoke("process_list",{projectDir:project})).find(p=>p.id===owner.id)?.running, "Parsed spreadsheet reaches model");
  const spreadsheetInput = JSON.stringify(requests.find(r=>r.kind==="attachment").messages);
  check(spreadsheetInput.includes("苹果") && spreadsheetInput.includes("12.5") && spreadsheetInput.includes("[=B2*C2]") && spreadsheetInput.includes("工作表：备注"), "Excel sheet names, values and formulas reach the actual model request");
  check(await page.locator('.msg.user a[href="https://github.com/acme/repo"]').isVisible(), "Native sent URL remains clickable");
  const beforeLink = servedLinkPages;
  await page.locator(`.msg.user a[href="${localPage}"]`).click();
  await until(() => servedLinkPages > beforeLink, "Sent URL opens native child webview");
  check(await page.locator("#view-chat").getAttribute("data-preview") === "open", "Sent message link loads a real page in the native preview");
  await page.locator("#preview-close").click();
  captureCalls = requests.length;
  await invoke("run_prompt", { ...prompt, prompt: "HOLD_MAIN: 等待本地验收放行" });
  await until(() => Boolean(mainReply), "First main request");
  check(requests.length === captureCalls + 1 && requests.at(-1).kind === "hold", "Ordinary conversation does not force an execution batch");
  await invoke("run_prompt", { ...prompt, prompt: "QUEUED_EXECUTION: 仅核对 R-001 的原始文件，不修改文件", delivery: "queue", executionBatch: true });
  check(requests.length === captureCalls + 1, "Execution input is queued while the main turn is running");
  await page.evaluate(async () => { window.__nativeConversationShell = await import("./03-shell.js"); });
  await page.locator("#new-chat").click();
  await page.waitForFunction(id => window.__nativeConversationShell.activeProcessId !== id, owner.id);
  check(await page.locator("#prompt").inputValue() === "", "New conversation opens a fresh composer before admission");
  const discussion = await invoke("process_create", { projectDir: project, profile: "dev-auto", model: "stub:test", subagentMode: "off" });
  await page.evaluate(async id => { const sessions = await import("./09-sessions.js"); await sessions.refreshProcesses(); await sessions.switchProcess(id, true); }, discussion.id);
  check(discussion?.profile === "dev-auto", "New conversation has the same development mode");
  await invoke("process_update", { processId: discussion.id, profile: "readonly" });
  check(Boolean(discussion) && (await invoke("process_list", { projectDir: project })).find(p => p.id === owner.id).running, "New discussion does not stop or replace the main execution owner");
  // A client asking for a writable development profile cannot upgrade readonly ownership.
  await invoke("run_prompt", { ...prompt, processId: discussion.id, profile: "dev-pair", agent: "dev-pair", prompt: "READONLY_ATTEMPT: 改写 original.txt", executionBatch: true, autonomous: true, autoAllow: true });
  await until(async () => requests.filter(r => r.kind === "discussion").length >= 2 && !(await invoke("process_list", { projectDir: project })).find(p => p.id === discussion.id)?.running, "Readonly discussion while main owns write lease");
  check(await readFile(path.join(project, "original.txt"), "utf8") === "original content", "Readonly ownership blocks writes despite dev, auto and execution overrides");
  check(!(await invoke("agent_team_command", { projectDir: project, processId: owner.id, input: { action: "list" } })).length, "An independent discussion does not dispatch children on the held conversation");
  await page.evaluate(async id => (await import("./09-sessions.js")).switchProcess(id), owner.id);
  await page.waitForFunction(() => document.body.dataset.conversationKind === "conversation");
  await page.locator("#prompt").fill("继续输入中的草稿"); await page.locator("#prompt").focus();
  mainReply();
  await until(async () => requests.some(r => r.kind === "queued") && !(await invoke("process_list", { projectDir: project })).find(p => p.id === owner.id)?.running, "Queued input completion");
  check(await page.locator("#prompt").inputValue() === "继续输入中的草稿", "Queued execution preserves the unsent conversation draft");
  check(await page.locator("#composer-work-context").isVisible(), "Registered requirements remain reachable after queued execution");
  await page.screenshot({ path: path.join(output, "queued-dark.png") });
  const events = await page.evaluate(() => window.__conversationEvents);
  const children = events.filter(e => e.name === "kz:tool-start" && e.payload.name === "task");
  check(children.length === 0, "Execution intent leaves delegation to the agent instead of forcing scouts or review");
  check(requests.filter(r => r.kind === "queued").length === 1 && requests.findIndex(r => r.kind === "queued") > requests.findIndex(r => r.kind === "hold"), "The queued input executes exactly once after the held input");
  const history = JSON.stringify(await invoke("conversation_display_get", { projectDir: project, processId: owner.id }));
  check(history.includes("QUEUED_EXECUTION") && !history.includes("READONLY_ATTEMPT"), "Queued scope stays on its owner and the independent discussion stays separate");
  check((await invoke("delivered_files", { projectDir: project })).length === 0, "An inspection does not invent a delivery receipt");
  check(errors.length === 0, `No native browser errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ checks, passed: checks.length, modelRequests: requests.map(r => r.kind), errors, executable: exe,
    boundary: "Actual Windows WebView2, Rust scheduler and child model requests with a local deterministic server; no external model or user project." }, null, 2));
  console.log(`Native conversations PASS: ${checks.length} checks, ${requests.length} local requests`);
} catch (error) {
  await writeFile(path.join(run, "failed-requests.json"), JSON.stringify(requests, null, 2));
  if (page) { await page.screenshot({ path: path.join(output, "failure.png") }).catch(() => {}); console.error(await page.evaluate(() => ({ text: document.body.innerText.slice(-2200), events: window.__conversationEvents?.slice(-8) })).catch(() => null)); }
  console.error({ checks, requests: requests.map(r => r.kind), errors,
    captureResults: requests.filter(r => r.kind === "capture").flatMap(r => r.messages.filter(m => m.role === "tool").map(m => m.content)),
  }); throw error;
} finally { if (page) await invoke("runtime_shutdown").catch(() => {}); await browser?.close(); app.kill(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve)); }
});
