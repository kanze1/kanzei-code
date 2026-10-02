// Actual Windows WebView2, requirement tools, durable questions and a local model.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, readFile, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Usage: node scripts/ui-registration-research-native-smoke.mjs <fresh kzapp.exe>");
const output = path.resolve("output/playwright/registration-research/native"), run = path.join(output, String(Date.now()));
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
await writeFile(path.join(project, "original.txt"), "original content");
const description = "管理番剧收藏，保存个人评分，并同步到 Windows 和 Android。";
const questionLabel = "番剧收藏需要支持哪些设备？", replyLabel = "Windows 与 Android";
const checks = [], requests = [], errors = [], allowedFixtureOperations = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
let artifactPath, asked = false, preparationTurns = 0;
function answer(res, content, tool = null, toolName = "req") {
  if (res.destroyed || res.writableEnded) return;
  const delta = tool ? { tool_calls: [{ index: 0, id: `native-registration-${requests.length}`, type: "function", function: { name: toolName, arguments: JSON.stringify(tool) } }] } : { content };
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: tool ? "tool_calls" : "stop" }], usage: { prompt_tokens: 80, completion_tokens: 20 } })}\n\n`);
  res.end("data: [DONE]\n\n");
}
const model = http.createServer(async (req, res) => {
  let raw = ""; for await (const part of req) raw += part;
  if (req.url !== "/v1/chat/completions") { res.writeHead(404); res.end(); return; }
  try {
    const data = JSON.parse(raw), messages = data.messages || [];
    const system = JSON.stringify(messages.filter(message => message.role === "system"));
    const lastUser = messages.findLastIndex(message => message.role === "user");
    const prompt = JSON.stringify(messages[lastUser]?.content || "");
    const tail = messages.slice(lastUser + 1).filter(message => message.role === "tool");
    const kind = system.includes("You capture ONE requirement") ? "capture"
      : system.includes("You own one delegated task") || prompt.includes("你是只读勘察代理") || prompt.includes("你是只读复核代理") ? "unexpected-child"
        : prompt.includes("用户回答异步问题") ? "answer" : prompt.includes("先完成 R-001") ? "preparation" : "other";
    requests.push({ kind, messages, model: data.model });
    if (kind === "capture") {
      if (tail.length) answer(res, "R-001 整理番剧收藏与评分");
      else answer(res, null, { action: "add", title: "整理番剧收藏与评分", complexity: "中", tag: "核心", priority: "P1", fields: {
        "原始描述": description, "验收": "收藏和评分可保存并读取，设备同步范围经确认",
        "来源": `用户原话「${description}」`, "发现记录": JSON.stringify({ Intent: "管理番剧收藏和个人评分", Explicit: description,
          Assumptions: "无新增假设", Ambiguities: "同步服务部署位置待用户说明", "领域对象": "番剧、收藏和个人评分",
          "最小成功闭环": "保存一条收藏和评分后读取", "延后决策": "同步服务的具体部署位置" }),
      } });
    } else if (kind === "preparation") {
      artifactPath ||= prompt.match(/再读取调研文件 ([^\s。]+)/)?.[1];
      if (!tail.length) { preparationTurns += 1; answer(res, null, { action: "get", id: "R-001" }); }
      else if (tail.length === 1) answer(res, null, { path: artifactPath }, "read");
      else if (!asked) {
        asked = true;
        answer(res, null, { question: questionLabel, options: [replyLabel, "仅 Windows"], background: true }, "question");
      } else answer(res, "已读取原始需求和待补齐的调研文件，继续独立调研；尚未领取实施或宣称调研完成。");
    } else if (kind === "answer") answer(res, `已收到设备范围：${replyLabel}。调研仍待补齐，尚未领取实施。`);
    else answer(res, "原生验收没有安排子任务或实施。");
  } catch (error) { errors.push(String(error)); answer(res, "LOCAL_SERVER_ERROR"); }
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary = "stub:primary"\nfast = "stub:fast"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n`);
const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env,
  KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"),
  APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port),
} });
let browser, page, owner;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
const entry = async () => (await invoke("docs_snapshot", { projectDir: project })).requirements.find(item => item.id === "R-001");
async function until(fn, label) {
  const end = Date.now() + 45000;
  while (Date.now() < end) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label} timed out`);
}
const idle = () => until(async () => {
  // Approve only this disposable app's actual permission card; keep questions interactive.
  if (await page.locator("#ask-allow").isVisible()) {
    allowedFixtureOperations.push({ action: await page.locator("#ask-action").textContent(), resource: await page.locator("#ask-resource").textContent() });
    await page.locator("#ask-allow").click();
  }
  return !(await invoke("process_list", { projectDir: project })).find(line => line.id === owner.id)?.running;
}, "Main idle");
try {
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1000 }); return true; } catch { return false; } }, "WebView2 start");
  page = browser.contexts()[0].pages()[0]; page.setDefaultTimeout(15000); page.on("pageerror", error => errors.push(error.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true");
  check((await invoke("projects_get")).projects.length === 0, "Native registration runs only in the disposable profile");
  await invoke("projects_init", { path: project, name: "核心登记与调研验收" });
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  owner = (await invoke("process_list", { projectDir: project })).find(line => !["readonly", "research"].includes(line.profile));
  assert(owner, "Main executor exists");
  await invoke("process_update", { processId: owner.id, subagentsEnabled: true, phasePipeline: false });
  await invoke("auto_state_update", { sessionId: owner.session_id, enabled: false });
  await page.evaluate(async () => {
    window.__registrationEvents = [];
    for (const name of ["kz:tool-start", "kz:tool-end", "kz:error", "kz:done"]) await window.__TAURI__.event.listen(name, event => window.__registrationEvents.push({ name, payload: event.payload }));
  });
  await page.locator("#project-register").waitFor({ state: "visible" });
  check(requests.length === 0, "Opening a new project does not start capture or model work");
  await page.locator("#prompt").fill(description);
  await page.locator("#project-register").click();
  await until(async () => { artifactPath = (await entry())?.prior_art?.path; return Boolean(artifactPath); }, "Persisted research path");
  await until(() => requests.some(request => request.kind === "preparation"), "Main research request");
  await idle();
  const saved = await entry(), disk = await readFile(path.join(project, ".kanzei/project/requirements.md"), "utf8");
  check(saved?.status === "todo" && saved.prior_art?.status === "pending", "Core capture is persisted as todo with pending research before implementation");
  check(disk.includes("R-001") && disk.includes(description), "The original request is saved in the actual requirement file");
  check((await readFile(path.join(project, artifactPath), "utf8")).includes("status: pending"), "Capture creates an actual pending prior-art artifact");
  check(requests.filter(request => request.kind === "capture").every(request => request.model === "fast"), "Successful capture does not repeat registration through the primary model");
  check(await page.locator("#prompt").inputValue() === "", "Successful registration clears the submitted draft");
  await page.locator("#project-chat-work .sw-work-entry").first().waitFor({ state: "visible" });
  check((await page.locator("#project-chat-work").innerText()).includes("待调研"), "The native conversation shows the registered requirement and its pending research badge");
  const preparation = requests.find(request => request.kind === "preparation"), prompt = JSON.stringify(preparation.messages.findLast(message => message.role === "user")?.content);
  check(prompt.includes("R-001") && prompt.includes("原始描述") && prompt.includes("外部已有实现") && prompt.includes("仓内既有设计") && prompt.includes("验证通过") && prompt.includes("待我处理"), "Actual main model receives the research instructions and question destination");
  const tools = requests.filter(request => request.kind === "preparation").flatMap(request => request.messages.filter(message => message.role === "tool"));
  check(tools.some(message => JSON.stringify(message.content).includes(description)) && tools.some(message => JSON.stringify(message.content).includes("status: pending")), "The main model reads the real saved requirement and research artifact through tools");
  const pending = (await invoke("softwire_questions")).find(question => question.question === questionLabel);
  check(Boolean(pending?.background) && pending.sessionId === owner.session_id, "A real background question is persisted for the original main conversation");
  check(!await page.locator("#ask-overlay").isVisible(), "Background questions leave the conversation usable without a modal question popup");
  await page.screenshot({ path: path.join(output, "pending-research.png") });
  await assert.rejects(() => invoke("docs_update", { projectDir: project, kind: "req", action: "update", id: "R-001", status: "doing" }), error => /PRIOR_ART_REQUIRED|先行调研尚未通过/.test(String(error)));
  check((await entry()).status === "todo", "Direct native status updates cannot enter implementation before research validation");
  await page.locator("#workbench-attention").click();
  const inbox = page.locator(".sw-inbox-row").filter({ hasText: questionLabel });
  await inbox.waitFor({ state: "visible" });
  check((await inbox.innerText()).includes("待你回复"), "The sidebar Needs attention entry exposes the actual pending research question");
  await inbox.click();
  await page.getByRole("button", { name: replyLabel, exact: true }).click();
  await page.locator(".sw-reply-complete").waitFor();
  await until(() => requests.some(request => request.kind === "answer"), "Original conversation receives answer");
  await idle();
  check(!(await invoke("softwire_questions")).some(question => question.id === pending.id), "Answering through Needs attention removes the persisted question");
  check(JSON.stringify(requests.find(request => request.kind === "answer").messages).includes(replyLabel), "The actual main model receives the selected answer without another Continue click");
  await page.evaluate(async projectDir => { await (await import("./12-workbench.js")).openProjectSpace(projectDir, "chat", { main: true }); await (await import("./26-project-conversations.js")).refreshConversationWork(); }, project);
  await page.locator("#project-chat-work .sw-work-entry").first().click();
  check(await page.getByRole("button", { name: "开始调研", exact: true }).isVisible(), "The saved pending requirement exposes an explicit research action in its details");
  await page.getByRole("button", { name: "查看调研文件", exact: true }).click();
  await until(async () => (await page.locator("#files-preview-path").textContent()).replaceAll("\\", "/").endsWith(artifactPath.replaceAll("\\", "/")), "Native research file editor");
  check(await page.locator("#view-files").evaluate(element => element.classList.contains("active")), "Research file action opens the native file editor on the created artifact");
  check((await invoke("file_preview", { projectDir: project, path: artifactPath })).content.includes("status: pending"), "The file editor reads the real artifact through native IPC");
  await page.evaluate(async projectDir => { await (await import("./12-workbench.js")).openProjectSpace(projectDir, "documents"); await (await import("./14-docs-actions.js")).refreshDocs(); }, project);
  const managed = page.locator('#documents-req-list .doc-item[data-doc-id="R-001"]');
  await managed.locator(".doc-row").waitFor({ state: "visible" });
  check((await managed.locator(".doc-row").innerText()).includes("待调研"), "The full native requirement list retains the research status");
  await managed.locator(".doc-row").click();
  await managed.getByRole("button", { name: "开始调研", exact: true }).click();
  await until(() => preparationTurns >= 2, "Research resumes from requirement management");
  await idle();
  const events = await page.evaluate(() => window.__registrationEvents);
  check(!requests.some(request => request.kind === "unexpected-child") && !events.some(event => event.name === "kz:tool-start" && event.payload.name === "task"), "Registration and research resume never dispatch an automatic execution batch despite enabled subagents");
  check((await entry()).status === "todo" && (await entry()).prior_art.status === "pending", "Research and answer delivery never automatically claim or mark the incomplete requirement done");
  check(await readFile(path.join(project, "original.txt"), "utf8") === "original content", "Research preparation leaves the implementation fixture unchanged");
  check(!events.some(event => event.name === "kz:error"), "Native research runs finish without execution or registration errors");
  check(errors.length === 0, `No native page or model errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, errors, executable: exe,
    modelRequests: requests.map(request => request.kind), allowedFixtureOperations, requirement: await entry(),
    boundary: "Actual Windows WebView2, Rust tools, requirement files and durable questions; isolated project and profile with a deterministic local model. No external prior-art research or real provider quality claim." }, null, 2));
  console.log(`Native registration research PASS: ${checks.length} checks, ${requests.length} local model requests`);
} catch (error) {
  await writeFile(path.join(run, "failure.json"), JSON.stringify({ error: String(error), checks, errors, requests }, null, 2));
  if (page) { await page.screenshot({ path: path.join(output, "failure.png") }).catch(() => {}); console.error(await page.evaluate(() => ({ text: document.body.innerText.slice(-2500), events: window.__registrationEvents?.slice(-10) })).catch(() => null)); }
  console.error({ checks, requests: requests.map(request => request.kind), errors }); throw error;
} finally {
  if (page) await invoke("runtime_shutdown").catch(() => {});
  await browser?.close(); app.kill(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
}
