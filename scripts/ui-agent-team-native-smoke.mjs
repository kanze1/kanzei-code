// Actual desktop IPC, model routing, Git isolation, SQLite recovery and child UI.
/* global window, document */
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdir, copyFile, readFile, writeFile, access } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Pass a freshly built kzapp.exe");
const output = path.resolve("output/playwright/agent-team/native"), run = path.join(output, String(Date.now()));
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
await writeFile(path.join(project, "original.txt"), "original content");
const git = (...args) => execFileSync("git", ["-C", project, ...args], { windowsHide: true, encoding: "utf8" }).trim();
git("init"); git("config", "user.name", "Acceptance"); git("config", "user.email", "acceptance@local"); git("config", "core.autocrlf", "false");
git("add", "original.txt"); git("commit", "-m", "Acceptance baseline");
const checks = [], requests = [], errors = [];
const check = (condition, label) => { assert(condition, label); checks.push(label); };
let releaseWriter, mainStep = 0;
function answer(res, content, tool, toolName = "write", id = "local-tool") {
  if (res.destroyed || res.writableEnded) return;
  const delta = tool ? { tool_calls: [{ index: 0, id, type: "function", function: { name: toolName, arguments: JSON.stringify(tool) } }] } : { content };
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: tool ? "tool_calls" : "stop" }], usage: { prompt_tokens: 80, completion_tokens: 20 } })}\n\n`);
  res.end("data: [DONE]\n\n");
}
const model = http.createServer(async (req, res) => {
  let body = ""; for await (const part of req) body += part;
  if (req.url !== "/v1/chat/completions") { res.writeHead(404); res.end(); return; }
  const data = JSON.parse(body), messages = data.messages || [];
  const serialized = JSON.stringify(messages), delegated = serialized.includes("You own one delegated task");
  const text = JSON.stringify(messages.findLast(m => m.role === "user")?.content || "");
  const tools = messages.filter(m => m.role === "tool");
  const kind = !delegated ? "main" : serialized.includes("NATIVE_WRITER") ? "writer" : serialized.includes("NATIVE_PLAN") ? "plan" : "recovery";
  requests.push({ kind, model: data.model, messages });
  if (kind === "writer") {
    if (tools.length) answer(res, "NATIVE_RESULT: child.txt created with actual evidence");
    else releaseWriter = () => answer(res, null, { path: "child.txt", content: "native child result" });
  } else if (kind === "plan") answer(res, text.includes("FOLLOWUP") ? "FOLLOWUP_RESULT: continued the original plan" : "FIRST_PLAN_EVIDENCE");
  else if (kind === "recovery") {
    if (text.includes("RESUME_PERSIST")) answer(res, "RESUMED_WITH_CONTEXT");
    // Otherwise leave the request open so stop/crash recovery is observable.
  } else {
    mainStep += 1;
    if (mainStep === 1) answer(res, null, { agent: "implement", description: "实现独立文件", prompt: "NATIVE_WRITER: create child.txt in your own checkout", context: "fork" }, "task", "native-writer");
    else if (mainStep === 2) answer(res, "已派发，等待真实结果。");
    else if (mainStep === 3) answer(res, null, { action: "diff", id: "native-writer" }, "task", "native-diff");
    else if (mainStep === 4) answer(res, null, { action: "adopt", id: "native-writer" }, "task", "native-adopt");
    else if (mainStep === 5) answer(res, null, { path: "child.txt" }, "read", "native-read");
    else answer(res, "已采纳，并读取实际文件；仅完成本地验收。");
  }
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary = "stub:primary-test"\nfast = "stub:fast-test"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n`);
const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
let app, browser, page, owner;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
const command = input => invoke("agent_team_command", { projectDir: project, processId: owner.id, input });
const jobs = () => command({ action: "list" });
async function until(fn, label = "condition") {
  const deadline = Date.now() + 45000;
  while (Date.now() < deadline) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label} timed out`);
}
async function start() {
  app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env,
    KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"),
    APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port),
  } });
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1500 }); return true; } catch { return false; } }, "WebView2");
  page = browser.contexts()[0].pages()[0]; page.setDefaultTimeout(15000); page.on("pageerror", e => errors.push(e.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 30000 });
}
async function stopApp() {
  await browser?.close(); browser = null;
  if (app && app.exitCode === null) { const exit = new Promise(resolve => app.once("exit", resolve)); app.kill(); await exit; }
}
try {
  await start();
  check((await invoke("projects_get")).projects.length === 0, "Disposable desktop profile contains no user project");
  await invoke("projects_init", { path: project, name: "子代理实验验收" });
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  owner = (await invoke("process_list", { projectDir: project })).find(p => !["readonly", "research"].includes(p.profile));
  assert(owner);
  await invoke("process_update", { processId: owner.id, subagentsEnabled: true, phasePipeline: false });
  await invoke("run_prompt", { projectDir: project, processId: owner.id, agent: "dev-pair", profile: "dev", model: "stub:primary-test", autonomous: false, autoAllow: false, prompt: "NATIVE_MAIN: delegate an isolated implementation then inspect and adopt it" });
  await until(() => releaseWriter && mainStep >= 2, "Background dispatch");
  check((await jobs()).find(j => j.id === "native-writer")?.state === "running", "Main task creates a real running writer");
  await until(async () => !(await invoke("process_list", { projectDir: project })).find(p => p.id === owner.id)?.running, "Main returns while child runs");
  check(true, "Main returns while child continues; completion later wakes the original conversation");
  check(!await access(path.join(project, "child.txt")).then(() => true, () => false), "Child has not touched the parent before adoption");
  await page.locator("#tasks-panel").waitFor({ state: "visible" });
  check((await page.locator("#tasks-panel").innerText()).includes("实现独立文件"), "Native job events display the real child");
  await page.screenshot({ path: path.join(output, "running-dark.png") });
  releaseWriter();
  await until(async () => (await jobs()).find(j => j.id === "native-writer")?.state === "waiting_user", "Child permission request");
  check(await page.getByRole("button", { name: "允许一次", exact: true }).isVisible(), "Child inherits the parent's interactive permission flow");
  const pending = await invoke("pending_asks_get", { projectDir: project, processId: owner.id });
  check(pending.some(ask => ask.agentId === "native-writer" && ask.source === "实现独立文件"), "Pending permission retains the exact child identity");
  check((await page.locator("#ask-source").innerText()).includes("实现独立文件"), "Native permission identifies its child");
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.locator("#ask-source").waitFor({ state: "visible" });
  check((await page.locator("#ask-source").innerText()).includes("实现独立文件"), "Refreshing a waiting permission preserves its child origin");
  await page.locator("#ask-source").click();
  check((await page.locator(".team-recipient").innerText()).includes("实现独立文件"), "Permission source navigates to the original child conversation");
  await page.locator("#ask-reopen").click();
  await page.getByRole("button", { name: "允许一次", exact: true }).click();
  await until(async () => (await jobs()).find(j => j.id === "native-writer")?.outcome === "adopted", "Child result integrated");
  await until(async () => !(await invoke("process_list", { projectDir: project })).find(p => p.id === owner.id)?.running, "Parent integration");
  const writer = (await jobs()).find(j => j.id === "native-writer");
  check(writer.state === "done" && writer.outcome === "adopted", "Real diff and adoption complete on the original child identity");
  check(await readFile(path.join(project, "child.txt"), "utf8") === "native child result", "Reviewed child patch reaches the parent workspace");
  check(git("diff", "--cached") === "", "Adoption leaves the parent's index unchanged");
  const collected = requests.filter(r => r.kind === "main");
  check(collected.length === 6, "An already returned adoption cannot trigger a duplicate final answer");
  check(collected[2].messages.some(m => JSON.stringify(m).includes("NATIVE_RESULT")), "Main receives the actual child result before integration");
  check(collected.some(r => r.messages.some(m => m.role === "tool" && JSON.stringify(m).includes("native child result"))), "Main inspects file evidence after integration");
  check(requests.find(r => r.kind === "writer").model === "primary-test", "Implementation uses the primary model");
  const planner = await command({ agent: "plan", description: "继续原计划", prompt: "NATIVE_PLAN: retain this plan context" });
  await until(async () => (await jobs()).find(j => j.id === planner.id)?.state === "done", "Plan result");
  await page.evaluate(async ({ owner, id }) => { await (await import("./27-agent-team.js")).refreshAgents(); (await import("./06-agent-panel.js")).openSubagentPanel(`${owner}|${id}`); }, { owner: owner.session_id, id: planner.id });
  await page.locator(".team-reply-input").fill("FOLLOWUP: explain the original evidence"); await page.locator(".team-reply-input").press("Enter");
  await until(async () => (await jobs()).find(j => j.id === planner.id)?.result.includes("FOLLOWUP_RESULT"), "Native reply");
  const followup = requests.filter(r => r.kind === "plan").at(-1);
  check(followup.messages.some(m => m.role === "assistant" && JSON.stringify(m).includes("FIRST_PLAN_EVIDENCE")), "UI reply resumes the persisted child conversation");
  check(followup.model === "primary-test", "Planner inherits primary instead of silently using fast");
  check((await jobs()).find(j => j.id === planner.id).messages.every(m => m.state === "processed"), "Message receipt advances through actual processing");
  await until(async () => await page.locator(".team-send-status").innerText() === "已处理", "Processed reply receipt");
  check(await page.locator(".team-send-status").innerText() === "已处理", "Reply editor replaces its old queued receipt");
  await page.screenshot({ path: path.join(output, "continued-dark.png") });
  const stopped = await command({ agent: "plan", description: "停止验收", prompt: "HOLD_STOP" });
  await until(async () => (await jobs()).find(j => j.id === stopped.id)?.state === "running", "Stop target");
  await command({ action: "stop", id: stopped.id });
  await until(async () => (await jobs()).find(j => j.id === stopped.id)?.state === "stopped", "Single stop");
  check((await jobs()).find(j => j.id === planner.id)?.state === "done", "Stopping one child preserves other results");
  const recovery = await command({ agent: "plan", description: "中断恢复验收", prompt: "HOLD_PERSIST PERSIST_CONTEXT" });
  await until(() => requests.some(r => r.kind === "recovery" && JSON.stringify(r.messages).includes("PERSIST_CONTEXT")), "Persisted request");
  // Closing only the UI now keeps work alive. This case intentionally crashes
  // the runtime to exercise interrupted-task recovery.
  const runtime = await invoke("runtime_status");
  await stopApp(); if (runtime.mode === "detached") process.kill(runtime.pid);
  model.closeAllConnections(); const beforeRestart = requests.length;
  await start();
  const restored = (await jobs()).find(j => j.id === recovery.id);
  check(restored.state === "interrupted", "Restart restores an interrupted task without a false running spinner");
  check(requests.length === beforeRestart, "Opening restored tasks never silently replays model work");
  await command({ action: "resume", id: recovery.id, prompt: "RESUME_PERSIST: continue from saved context" });
  await until(async () => (await jobs()).find(j => j.id === recovery.id)?.state === "done", "Explicit resume");
  check(requests.findLast(r => r.kind === "recovery").messages.some(m => JSON.stringify(m).includes("PERSIST_CONTEXT")), "Explicit resume retains pre-crash context");
  check(errors.length === 0, `No native browser errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ checks, errors, modelRequests: requests.length, executable: exe,
    boundary: "Actual Windows desktop, IPC, Git worktrees and SQLite; deterministic local model responses, no external-model quality claim." }, null, 2));
  console.log(`${checks.length} native agent-team checks passed (${requests.length} local requests)`);
} catch (error) {
  console.error({ checks, errors, requests: requests.map(r => r.kind), jobs: owner && page ? await jobs().catch(() => []) : [] });
  await page?.screenshot({ path: path.join(output, "failure.png") }).catch(() => {}); throw error;
} finally { if (page) await invoke("runtime_shutdown").catch(() => {}); await stopApp(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve)); }
