// Actual WebView2 + Rust + SQLite, disposable home, local model; no cloud calls.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, readFile, readdir, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Usage: node scripts/ui-general-chat-native-smoke.mjs <fresh kzapp.exe>");
const output = path.resolve("output/playwright/general-harness"), run = path.join(output, String(Date.now()));
const profile = path.join(run, "profile"), home = path.join(profile, ".kanzei"), project = path.join(run, "project");
for (const dir of [home, project, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(home, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
const checks = [], requests = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); console.log(`PASS ${label}`); };
let heldReply, longIndex = 0;
function answer(res, content = "GENERAL_REPLY: 已收到聊天和附件。", tool = null) {
  if (res.destroyed || res.writableEnded) return;
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  const delta = tool ? { tool_calls: [{ index: 0, id: tool.id, type: "function", function: { name: tool.name, arguments: JSON.stringify(tool.input) } }] } : { content };
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: tool ? "tool_calls" : "stop" }], usage: { prompt_tokens: 80, completion_tokens: 20 } })}\n\n`);
  res.end("data: [DONE]\n\n");
}
const model = http.createServer(async (req, res) => {
  if (req.url?.endsWith("/models")) { res.writeHead(200, { "Content-Type": "application/json" }); res.end(JSON.stringify({ data: [{ id: "test" }] })); return; }
  let body = ""; for await (const chunk of req) body += chunk;
  const payload = JSON.parse(body || "{}"); requests.push(payload);
  const text = JSON.stringify([...payload.messages].reverse().find(message => message.role === "user" && !String(message.content).startsWith("(system)")));
  if (payload.messages.some(message => message.role === "system" && JSON.stringify(message).includes("上下文压缩器"))) {
    const files = [...new Set(text.match(/[A-Za-z0-9][A-Za-z0-9_\/-]*\.(?:html|txt)/g) || [])].slice(0, 20);
    const markers = [...new Set(text.match(/\b(?:GENERAL_[A-Z_]+|LONG_GENERAL_BATCH|CREATE_GENERAL_FILE|FORGED_DEV)\b/g) || [])];
    answer(res, `## 目标\n无项目 Harness 验证。\n## 用户指令清单\n${markers.join("、")}。\n## 关键决策与理由\n使用独立会话文件目录。\n## 已完成\n实际文件、终端和子代理检查。\n## 失败尝试\n无。\n## 当前状态\n保留原始历史和文件，继续用户当前请求。\n## 关键文件\n${files.join("\n")}\n## 下一步\n依据当前用户请求继续。`);
    return;
  }
  const calls = payload.messages.flatMap(message => message.tool_calls || []);
  const called = id => calls.some(call => call.id === id);
  const call = (id, name, input) => answer(res, null, { id, name, input });
  if (text.includes("CREATE_GENERAL_FILE")) {
    if (!called("general-file")) call("general-file", "write", { path: "general-artifact.html", content: "<!doctype html><html lang='zh'><meta charset='utf-8'><title>无项目产物</title><h1>GENERAL_ARTIFACT</h1></html>" });
    else {
      const result = [...payload.messages].reverse().find(message => message.role === "tool" && message.tool_call_id === "general-file");
      const file = String(result?.content || "").match(/bytes to (.+)/)?.[1]?.trim();
      answer(res, `GENERAL_FILE_DONE: [打开文件](<${file}>)`);
    }
    return;
  }
  if (text.includes("LONG_GENERAL_BATCH")) {
    const done = longIndex++;
    if (done < 36) call(`general-long-${done}`, "write", { path: `long/file-${done}.txt`, content: `verified artifact ${done}` });
    else answer(res, "GENERAL_LONG_DONE: 36 个文件已创建。");
    return;
  }
  if (text.includes("GENERAL_CHILD_WRITE")) {
    if (!called("child-write")) call("child-write", "write", { path: "child-artifact.txt", content: "GENERAL_CHILD_ARTIFACT" });
    else answer(res, "GENERAL_CHILD_DONE: child-artifact.txt 已完成");
    return;
  }
  if (text.includes("GENERAL_TEAM_PARENT")) {
    if (!called("general-child")) call("general-child", "task", { action: "spawn", agent: "implement", prompt: "GENERAL_CHILD_WRITE: 创建文件并核对", background: false });
    else if (!called("general-adopt")) call("general-adopt", "task", { action: "adopt", id: "general-child" });
    else answer(res, "GENERAL_TEAM_DONE: 已采纳子代理文件");
    return;
  }
  if (text.includes("GENERAL_GIT")) {
    if (!called("general-git-file")) call("general-git-file", "write", { path: "src/main.rs", content: "fn main() {}\n" });
    else if (!called("general-git-stage")) call("general-git-stage", "git", { action: "stage", files: ["src/main.rs"] });
    else if (!called("general-git-commit")) {
      const receipt = payload.messages.find(message => message.role === "tool" && message.tool_call_id === "general-git-stage");
      const hash = String(receipt?.content || "").match(/staged_hash:\s*([^\s]+)/)?.[1];
      call("general-git-commit", "git", { action: "commit", message: "General artifact version", expected_hash: hash });
    } else if (!called("general-git-second")) call("general-git-second", "write", { path: "src/second.rs", content: "pub fn example() {}\n" });
    else if (!called("general-git-finalize")) call("general-git-finalize", "git", { action: "finalize", files: ["src/second.rs"], message: "Another general artifact version" });
    else answer(res, "GENERAL_GIT_DONE");
    return;
  }
  if (text.includes("GENERAL_SHELL")) {
    if (!called("general-command")) call("general-command", "bash", { command: "Write-Output 'GENERAL_SHELL_EXECUTED'" });
    else answer(res, "GENERAL_SHELL_DONE");
    return;
  }
  if (text.includes("GENERAL_INTERACTIVE")) {
    if (!called("general-interactive")) call("general-interactive", "bash", { command: "Write-Output 'GENERAL_READY'; $line = [Console]::ReadLine(); Write-Output ('GENERAL_ECHO:' + $line)", interactive: true });
    else {
      const terminal = payload.messages.find(message => message.role === "tool" && message.tool_call_id === "general-interactive");
      const id = String(terminal?.content || "").match(/process_id:\s*([^\s]+)/)?.[1];
      if (!called("general-input")) call("general-input", "process", { action: "input", id, text: "HELLO_GENERAL\n", close: true });
      else if (!called("general-wait")) call("general-wait", "process", { action: "wait", id, timeout_secs: 10 });
      else answer(res, "GENERAL_INTERACTIVE_DONE");
    }
    return;
  }
  if (text.includes("GENERAL_SCHEDULE")) {
    if (!called("general-schedule-file")) call("general-schedule-file", "write", { path: "schedule-artifact.txt", content: "GENERAL_SCHEDULE_ARTIFACT" });
    else answer(res, "GENERAL_SCHEDULE_DONE");
    return;
  }
  if (text.includes("GENERAL_REWIND_FILE")) {
    if (!called("general-rewind-file")) call("general-rewind-file", "write", { path: "rewind-artifact.txt", content: "GENERAL_REWIND_ARTIFACT" });
    else answer(res, "GENERAL_REWIND_DONE");
    return;
  }
  if (text.includes("HOLD_GENERAL") && !heldReply) heldReply = () => answer(res);
  else answer(res);
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(home, "kanzei.toml"), `[models]\nprimary = "stub:test"\nfast = "stub:test"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n[permissions]\nrules = [{ action = "write", resource = "*", effect = "allow" }]\n`);
let app, browser, page;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
async function until(fn, label, timeoutMs = 45000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label} timed out`);
}
async function start() {
  const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
  const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
  app = spawn(exe, [], { cwd: run, windowsHide: true, stdio: "ignore", env: { ...process.env,
    KANZEI_HOME: home, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"),
    APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: undefined, KANZEI_E2E_CDP: String(port),
    GIT_AUTHOR_NAME: "General Harness Test", GIT_AUTHOR_EMAIL: "general-test@example.invalid", GIT_COMMITTER_NAME: "General Harness Test", GIT_COMMITTER_EMAIL: "general-test@example.invalid",
  } });
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1500 }); return true; } catch { return false; } }, "WebView2 startup");
  page = browser.contexts()[0].pages()[0]; page.on("pageerror", error => errors.push(String(error)));
  await until(() => page.url() !== "about:blank", "Native document navigation");
  console.log(`Native URL: ${page.url()}`);
  await page.waitForFunction(() => document.body.dataset.appReady === "true", { timeout: 45000 });
  await page.locator("#auto-allow").check();
}
async function close() {
  if (page) await invoke("runtime_shutdown").catch(() => {});
  // Startup can fail before CDP becomes available. Shut down only the detached
  // service belonging to this disposable executable/home, through its protocol.
  for (const entry of await readdir(path.join(home, "runtime"), { withFileTypes: true }).catch(() => [])) {
    if (!entry.isDirectory()) continue;
    const endpoint = JSON.parse(await readFile(path.join(home, "runtime", entry.name, "endpoint.json"), "utf8").catch(() => "null"));
    if (!endpoint || path.resolve(endpoint.executable || "").toLowerCase() !== exe.toLowerCase() || !/^127\.0\.0\.1:\d+$/.test(endpoint.addr || "")) continue;
    await new Promise(resolve => {
      const socket = net.createConnection({ host: "127.0.0.1", port: Number(endpoint.addr.split(":").at(-1)) });
      const finish = () => { socket.destroy(); resolve(); };
      socket.setTimeout(1500, finish); socket.on("error", finish); socket.on("data", finish);
      socket.on("connect", () => { const body = Buffer.from(JSON.stringify({ action: "shutdown", token: endpoint.token })), size = Buffer.alloc(4); size.writeUInt32BE(body.length); socket.write(Buffer.concat([size, body])); });
    });
  }
  await browser?.close(); browser = null; page = null;
  if (app && app.exitCode === null) { app.kill(); await new Promise(resolve => app.once("exit", resolve)); }
}
try {
  await start();
  check(await page.locator("body").getAttribute("data-general-chat") === "true", "Zero-project startup opens general chat");
  check(await page.locator("#project-space-name").textContent() === "无项目对话", "Header names the mode without exposing storage paths");
  const root = await invoke("general_chat_open");
  let lines = await invoke("process_list", { projectDir: root });
  const original = lines[0];
  check(original.profile === "dev" && original.subagents_enabled && !original.phase_pipeline && !original.tracker_writes, "General recipient retains Harness without project phases");
  check(await page.locator("#subagent-control").isVisible(), "General chat exposes the subagent switch");
  check((await invoke("projects_get")).projects.length === 0, "Opening chat does not register a fake project");
  const text = "GENERAL_INPUT: 不依赖项目的聊天";
  await page.locator("#attachment-input").setInputFiles({ name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("GENERAL_ATTACHMENT: 独立附件内容") });
  await page.locator("#prompt").fill(text); await page.locator("#send").click();
  await until(async () => (await page.locator("#messages").innerText()).includes("GENERAL_REPLY") && !(await invoke("process_list", { projectDir: root })).some(p => p.running), "General reply");
  const submitted = requests.find(r => JSON.stringify(r.messages).includes("GENERAL_INPUT"));
  check(Boolean(submitted), "UI sends a real model request without selecting a project");
  check((await invoke("process_list", { projectDir: root })).find(p => p.id === original.id).title === text, "Sidebar title uses the actual first user message");
  check((await page.locator("#messages .msg.user").allTextContents()).some(content => content.includes(text)), "User bubble retains the actual submitted text");
  check(JSON.stringify(submitted.messages).includes("GENERAL_ATTACHMENT"), "Uploaded text attachment reaches the model");
  const catalog = submitted.tools.map(tool => tool.function.name);
  check(["read", "write", "edit", "bash", "task", "tool_search", "memory_search"].every(name => catalog.includes(name)), "Model catalog retains files, shell, memory and agents");
  check(!["req", "defect", "work", "test_record", "architecture"].some(name => catalog.includes(name)), "Model catalog excludes project workflow tools");
  check(!JSON.stringify(submitted.messages.filter(m => m.role === "system")).includes("来源判断"), "General prompt excludes project origin and workflow instructions");
  await page.locator("#new-chat").click();
  await until(async () => (await invoke("process_list", { projectDir: root })).length === 2, "New independent chat");
  await page.waitForFunction(() => !document.getElementById("new-chat").hasAttribute("aria-busy"));
  check(!(await page.locator("#messages").innerText()).includes("GENERAL_INPUT"), "New chat has a separate history");
  await page.locator("#workbench-chat-history").click();
  check(await page.locator("#session-history").isVisible(), "General history is available");
  check(!await page.locator("#session-history [data-act='new-task']").isVisible(), "General history hides isolated project tasks");
  await page.keyboard.press("Escape");
  await page.evaluate(async id => { const { switchProcess } = await import("./09-sessions.js"); await switchProcess(id, true); }, original.id);
  check((await page.locator("#messages").innerText()).includes("GENERAL_INPUT"), "Switching history restores the original conversation");
  await invoke("run_prompt", { projectDir: root, processId: original.id, prompt: "FORGED_DEV: 保持聊天权限", profile: "dev", agent: "dev-auto", autonomous: true, executionBatch: true, model: "stub:test" });
  await until(async () => requests.some(r => JSON.stringify(r.messages).includes("FORGED_DEV")) && !(await invoke("process_list", { projectDir: root })).some(p => p.running), "Forged run completes as chat");
  const forged = requests.find(r => JSON.stringify(r.messages).includes("FORGED_DEV"));
  check(forged.tools.some(tool => tool.function.name === "write") && !forged.tools.some(tool => tool.function.name === "work"), "Stale autonomous flags retain general tools without project workflow");
  const runPrompt = async prompt => {
    await invoke("run_prompt", { projectDir: root, processId: original.id, prompt, model: "stub:test", autoAllow: true });
    await until(async () => !(await invoke("process_list", { projectDir: root })).some(p => p.running), `Finish ${prompt}`, prompt.includes("LONG_GENERAL_BATCH") ? 240000 : 45000);
  };
  await runPrompt("CREATE_GENERAL_FILE: 生成真实网页文件和链接");
  const workspaces = await readdir(path.join(root, "artifacts"));
  let artifact;
  for (const workspace of workspaces) {
    const file = path.join(root, "artifacts", workspace, "general-artifact.html");
    if ((await readFile(file, "utf8").catch(() => "")).includes("GENERAL_ARTIFACT")) artifact = file;
  }
  check(Boolean(artifact), "File tool creates a real HTML artifact in the conversation workspace");
  check((await page.locator("#messages").innerText()).includes("GENERAL_FILE_DONE"), "General chat returns the artifact link");
  const link = page.locator("#messages a").filter({ hasText: "打开文件" });
  check((await link.getAttribute("data-path")).replaceAll("\\", "/").toLowerCase() === artifact.replaceAll("\\", "/").toLowerCase(), "Artifact link targets the actual local file");
  await link.click();
  await until(() => page.evaluate(async () => { const { filesDoc, filesEditor } = await import("./17-files-editor.js"); return Boolean(filesDoc && filesEditor?.getValue()?.includes("GENERAL_ARTIFACT")); }), "Open general file link");
  check(true, "Clicking the general artifact link opens the actual file in the application");
  await page.evaluate(async () => { const { navigate_view } = await import("./03-shell.js"); navigate_view("chat"); });
  await runPrompt("LONG_GENERAL_BATCH: 连续创建 36 个独立文件");
  const artifactDir = path.dirname(artifact);
  check((await readdir(path.join(artifactDir, "long"))).filter(name => /^file-\d+\.txt$/.test(name)).length === 36, "General execution crosses the project batch step boundary without BATCH_CLOSING");
  await runPrompt("GENERAL_TEAM_PARENT: 请子代理创建文件并采纳");
  check((await readFile(path.join(artifactDir, "child-artifact.txt"), "utf8")) === "GENERAL_CHILD_ARTIFACT", "Writable general child runs in isolation and adopts its actual artifact");
  const childRequest = requests.find(request => JSON.stringify(request.messages).includes("GENERAL_CHILD_WRITE") && request.messages.some(message => message.role === "system" && JSON.stringify(message).includes("Task messages")));
  check(Boolean(childRequest), "General child preserves its persistent steering inbox");
  await runPrompt("GENERAL_GIT: 为生成的文件建立普通 Git 版本");
  const committed = requests.flatMap(request => request.messages).find(message => message.role === "tool" && message.tool_call_id === "general-git-commit");
  check(String(committed?.content || "").includes("committed"), "General Git commits a source artifact without project test-record gates");
  const finalized = requests.flatMap(request => request.messages).find(message => message.role === "tool" && message.tool_call_id === "general-git-finalize");
  check(String(finalized?.content || "").includes("committed"), "General Git finalize uses ordinary stage and commit without a Cargo project");
  await runPrompt("GENERAL_SHELL: 执行普通本地命令");
  check(requests.some(request => request.messages.some(message => message.role === "tool" && String(message.content).includes("GENERAL_SHELL_EXECUTED"))), "General shell really executes and returns command output");
  await runPrompt("GENERAL_INTERACTIVE: 输入文本并等待终端输出");
  check(requests.some(request => request.messages.some(message => message.role === "tool" && message.tool_call_id === "general-wait" && String(message.content).includes("GENERAL_ECHO:HELLO_GENERAL"))), "General interactive terminal accepts stdin and returns the real reply");
  const schedule = { name: "general-native", enabled: false, when: "每 1 分钟", host: "app", utc_offset_minutes: 480, catch_up: "once", agent: "general", model: "stub:test", timeout_secs: 30, max_steps: 8, steps: [{ prompt: "GENERAL_SCHEDULE: 生成文件" }], writeback: [], body: "" };
  await invoke("schedule_action", { projectDir: root, action: "save", definition: schedule });
  await invoke("schedule_action", { projectDir: root, action: "run", name: schedule.name });
  await until(async () => (await invoke("schedule_action", { projectDir: root, action: "history", name: schedule.name })).some(event => event.type === "schedule.run_finished"), "General schedule completes");
  const scheduleEvents = await invoke("schedule_action", { projectDir: root, action: "history", name: schedule.name });
  check(scheduleEvents.find(event => event.type === "schedule.run_finished")?.data.ok === true, "General scheduled prompt completes without requiring a Git project");
  let scheduledArtifact = false;
  for (const workspace of await readdir(path.join(root, "artifacts"))) {
    scheduledArtifact ||= (await readFile(path.join(root, "artifacts", workspace, "schedule-artifact.txt"), "utf8").catch(() => "")) === "GENERAL_SCHEDULE_ARTIFACT";
  }
  check(scheduledArtifact, "General schedule persists a real file in its run workspace");
  const rewindChat = await invoke("process_create", { projectDir: root, profile: "dev", phasePipeline: false, subagentsEnabled: false });
  const rewindText = "GENERAL_REWIND_FILE: 创建可回退文件";
  await invoke("run_prompt", { projectDir: root, processId: rewindChat.id, prompt: rewindText, model: "stub:test", autoAllow: true });
  await until(async () => !(await invoke("process_list", { projectDir: root })).some(process => process.running), "Rewind file creation");
  let rewindFile;
  for (const workspace of await readdir(path.join(root, "artifacts"))) {
    const file = path.join(root, "artifacts", workspace, "rewind-artifact.txt");
    if ((await readFile(file, "utf8").catch(() => "")) === "GENERAL_REWIND_ARTIFACT") rewindFile = file;
  }
  check(Boolean(rewindFile) && path.dirname(rewindFile) !== artifactDir, "Independent general conversations write to separate directories");
  const rewindArgs = { projectDir: root, processId: rewindChat.id, text: rewindText, occurrenceFromEnd: 0 };
  const rewindPreview = await invoke("conversation_action", { ...rewindArgs, action: "preview" });
  check(rewindPreview.files.some(file => file.path.toLowerCase() === rewindFile.toLowerCase()), "General rewind discovers the actual file checkpoint");
  await invoke("conversation_action", { ...rewindArgs, action: "both", expectedHash: rewindPreview.sourceHash, force: false });
  check(await readFile(rewindFile, "utf8").then(() => false, () => true), "General file and conversation rewind restores the pre-write state");
  check((await readFile(artifact, "utf8")).includes("GENERAL_ARTIFACT"), "Rewinding one general chat preserves another chat's artifact");
  await invoke("run_prompt", { projectDir: root, processId: original.id, prompt: "HOLD_GENERAL", model: "stub:test" });
  await until(() => Boolean(heldReply), "Held response");
  await invoke("stop_run", { projectDir: root, processId: original.id }); heldReply();
  await until(async () => !(await invoke("process_list", { projectDir: root })).some(p => p.running), "Stop general response");
  check(true, "Stop uses the existing session runtime");
  await page.evaluate(async () => { const { flushLayout } = await import("./03-layout.js"); flushLayout(); });
  await until(async () => JSON.parse(await readFile(path.join(home, "app.json"), "utf8")).ui_layout?.prefs?.conversation_mode === "general", "Mode preference persisted");
  await close(); await start();
  check(await page.locator("body").getAttribute("data-general-chat") === "true", "Restart restores general mode");
  check((await page.locator("#messages").innerText()).includes("GENERAL_INPUT"), "Restart recovers actual SQLite conversation history");
  await invoke("process_update", { processId: original.id, subagentsEnabled: false });
  check(!(await invoke("process_list", { projectDir: root })).find(process => process.id === original.id).subagents_enabled, "General subagent preference can be changed");
  await invoke("general_chat_open");
  check(!(await invoke("process_list", { projectDir: root })).find(process => process.id === original.id).subagents_enabled, "Opening general chat preserves the user's subagent preference");
  await invoke("process_update", { processId: original.id, subagentsEnabled: true });
  let denied = false;
  try { await invoke("process_update", { processId: original.id, phasePipeline: true }); } catch { denied = true; }
  check(denied, "General settings reject the project phase pipeline");
  const forkArgs = { projectDir: root, processId: original.id, text: "FORGED_DEV: 保持聊天权限", occurrenceFromEnd: 0 };
  const preview = await invoke("conversation_action", { ...forkArgs, action: "preview" });
  const forked = await invoke("conversation_action", { ...forkArgs, action: "fork", expectedHash: preview.sourceHash, force: false });
  check(JSON.stringify(await invoke("conversation_display_get", { projectDir: root, processId: forked.processId })).includes("GENERAL_INPUT"), "General fork persists its preceding conversation");
  const prefs = await invoke("projects_add", { path: project });
  await page.evaluate(async prefs => { const { enterProject } = await import("./09-sessions.js"); await enterProject(prefs); }, prefs);
  await page.locator("#prompt").fill("PROJECT_DRAFT");
  await page.locator("#workbench-general-chat").click();
  await until(async () => await page.locator("body").getAttribute("data-general-chat") === "true", "Return to general chat");
  check(!(await page.locator("#prompt").inputValue()).includes("PROJECT_DRAFT"), "Project draft stays with its own conversation");
  const after = await invoke("projects_get");
  check(after.projects.length === 1 && after.current === prefs.current, "General chat preserves the real project preference");
  check(!await page.locator("#project-work-switch").isVisible() && !await page.locator("#profile-select").isVisible(), "Project execution controls are hidden");
  await page.locator("#general-chat-link").click(); await page.getByRole("menuitem", { name: "project", exact: true }).click();
  await until(async () => await page.locator("body").getAttribute("data-general-chat") === "false" && (await page.locator("#messages").innerText()).includes("GENERAL_INPUT"), "Link with history");
  const linkedId = await page.evaluate(async () => (await import("./03-shell.js")).activeProcessId);
  check((await invoke("process_list", { projectDir: prefs.current })).find(p => p.id === linkedId)?.profile === "dev", "Project association creates an ordinary conversation");
  await page.locator("#workbench-general-chat").click();
  await until(async () => await page.locator("body").getAttribute("data-general-chat") === "true", "Original remains accessible");
  await page.waitForFunction(() => !document.getElementById("workbench-general-chat").hasAttribute("aria-busy") && !document.getElementById("general-chat-link").disabled);
  check((await page.locator("#messages").innerText()).includes("GENERAL_INPUT"), "Association retains original general history");
  await page.evaluate(async id => { const { switchProcess } = await import("./09-sessions.js"); await switchProcess(id, true); }, original.id);
  const compacted = await invoke("conversation_compact", { projectDir: root, processId: original.id, focus: "保留已生成的文件和当前任务状态" });
  check(compacted.changed === true && compacted.after < compacted.before, "General manual compaction reduces the actual persisted conversation");
  const compactedHistory = await invoke("conversation_display_get", { projectDir: root, processId: original.id });
  check(JSON.stringify(compactedHistory).includes("已压缩为"), "General compaction persists its resumable summary surface");
  for (const theme of ["dark", "light"]) { await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme); await page.screenshot({ path: path.join(output, `${theme}.png`) }); }
  check(errors.length === 0, `No native browser errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, requests: requests.length, errors, executable: exe, boundary: "Actual local WebView2/Rust/SQLite; local stub model, no provider uptime assertion" }, null, 2));
  console.log(`${checks.length} native general-chat checks passed`);
} catch (error) {
  if (page) { await page.screenshot({ path: path.join(output, "failure.png") }).catch(() => {}); await writeFile(path.join(output, "failure-state.json"), JSON.stringify({ url: page.url(), executable: exe, checks, requests }, null, 2)); console.error(await page.evaluate(() => document.body.innerText.slice(-3000)).catch(() => "")); }
  console.error({ checks, errors, requests: requests.map(r => (r.tools || []).map(t => t.function.name)) }); throw error;
} finally { await close(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve)); }
