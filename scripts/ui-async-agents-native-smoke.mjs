// Actual WebView2 + Rust scheduler + deterministic local model and terminal processes.
/* global window, document */
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdir, copyFile, writeFile, readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Pass a freshly built kzapp.exe");
const output = path.resolve("output/playwright/async-agents");
const run = path.join(output, String(Date.now())), project = path.join(run, "project");
const profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
await writeFile(path.join(project, "original.txt"), "unchanged");
const git = (...args) => execFileSync("git", ["-C", project, ...args], { windowsHide: true, encoding: "utf8" }).trim();
git("init"); git("config", "user.name", "Async acceptance"); git("config", "user.email", "test@local"); git("add", "."); git("commit", "-m", "fixture");
const checks = [], requests = [], errors = [], held = new Map(), counts = new Map();
const check = (value, label) => { assert(value, label); checks.push(label); };
function answer(res, content, calls = []) {
  if (res.destroyed || res.writableEnded) return;
  const delta = calls.length ? { tool_calls: calls.map((call, index) => ({ index, id: call.id, type: "function", function: { name: call.name, arguments: JSON.stringify(call.input) } })) } : { content };
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: calls.length ? "tool_calls" : "stop" }], usage: { prompt_tokens: 60, completion_tokens: 15 } })}\n\n`);
  res.end("data: [DONE]\n\n");
}
const call = (id, name, input) => ({ id, name, input });
const model = http.createServer(async (req, res) => {
  let raw = ""; for await (const part of req) raw += part;
  if (req.url !== "/v1/chat/completions") { res.writeHead(404); res.end(); return; }
  try {
    const data = JSON.parse(raw), messages = data.messages || [];
    const system = JSON.stringify(messages.filter(m => m.role === "system"));
    const last = JSON.stringify(messages.findLast(m => m.role === "user")?.content || "");
    const child = system.includes("You own one delegated task");
    const assignment = String(messages.findLast(m => m.role === "user" && typeof m.content === "string" && m.content.startsWith("main: CHILD_"))?.content || "");
    const kind = child ? assignment.includes("CHILD_ASK_ASYNC") ? "child-question" : assignment.includes("CHILD_TERMINAL") ? "child-terminal" : assignment.includes("CHILD_ALPHA") ? "alpha" : "beta"
      : last.includes("后台终端完成") ? "terminal-callback" : last.includes("MAIN_PARALLEL") ? "parallel" : last.includes("MAIN_ASK_ASYNC") ? "main-question"
        : last.includes("MAIN_HANDOFF") ? "handoff" : last.includes("HANDOFF_STEER") ? "handoff-steered"
          : last.includes("MAIN_TERMINAL") ? "main-terminal" : last.includes("MAIN_STEER") ? "steer"
          : last.includes("STEERING_BOUNDARY") ? "steered" : last.includes("用户回答异步问题") ? "main-answer"
            : last.includes("STATUS_WHILE_BACKGROUND") ? "status" : "callback";
    const step = (counts.get(kind) || 0) + 1; counts.set(kind, step);
    requests.push({ kind, child, messages, model: data.model, at: Date.now() });
    if (kind === "parallel" && step === 1) answer(res, null, [
      call("async-alpha", "task", { agent: "explore", description: "实现检查", prompt: "CHILD_ALPHA inspect original.txt" }),
      call("async-beta", "task", { agent: "plan", description: "验收检查", prompt: "CHILD_BETA inspect test boundaries" }),
    ]);
    else if (kind === "alpha" || kind === "beta") held.set(kind, () => answer(res, kind.toUpperCase() + "_RESULT"));
    else if (kind === "steer") held.set("steer", () => answer(res, null, [call("steer-read", "read", { path: "original.txt" })]));
    else if (kind === "main-question" && step === 1) answer(res, null, [call("main-async-ask", "question", { question: "MAIN_ASYNC_CHOICE", options: ["A", "B"], background: true })]);
    else if (kind === "child-question" && !last.includes("用户回答异步问题") && step === 1) answer(res, null, [call("child-async-ask", "question", { question: "CHILD_ASYNC_CHOICE", options: ["A", "B"], background: true })]);
    else if (kind === "main-terminal" && step === 1) answer(res, null, [call("terminal-main", "bash", { command: "Start-Sleep -Milliseconds 1600; Write-Output MAIN_TERMINAL_RESULT", background: true })]);
    else if (kind === "handoff" && step === 1) answer(res, null, [call("terminal-handoff", "bash", {
      command: 'Write-Output "HANDOFF_PREFIX:$PID"; while (-not (Test-Path handoff-release.txt)) { Start-Sleep -Milliseconds 20 }; Write-Output HANDOFF_SUFFIX', timeout_ms: 30000,
    })]);
    else if (kind === "child-terminal" && step === 1) answer(res, null, [call("terminal-child", "bash", { command: "Start-Sleep -Milliseconds 1600; Write-Output CHILD_TERMINAL_RESULT", background: true })]);
    else answer(res, `RESULT_${kind}_${step}: 已处理实际输入，可以继续独立工作。`);
  } catch (error) { errors.push(String(error)); answer(res, "LOCAL_SERVER_ERROR"); }
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary = "stub:primary"\nfast = "stub:fast"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n`);
const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
const executableSha256 = createHash("sha256").update(await readFile(exe)).digest("hex");
const app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env,
  KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"),
  APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port),
} });
let browser, page, owner;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
const command = input => invoke("agent_team_command", { projectDir: project, processId: owner.id, input });
const jobs = () => command({ action: "list" });
const prompt = (text, extra = {}) => invoke("run_prompt", { projectDir: project, processId: owner.id, profile: "dev", agent: "dev-pair", model: "stub:interactive-main", prompt: text, autoAllow: false, autonomous: false, ...extra });
const running = async () => (await invoke("process_list", { projectDir: project })).find(p => p.id === owner.id)?.running;
async function until(fn, label) {
  const end = Date.now() + 45000;
  while (Date.now() < end) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label} timed out`);
}
async function idle() { await until(async () => !await running(), "Main idle"); }
async function question(label) { let q; await until(async () => (q = (await invoke("softwire_questions")).find(q => q.question === label)), label); return q; }
async function allowOnce() {
  const button = page.getByRole("button", { name: "允许一次", exact: true });
  await button.waitFor({ state: "visible" });
  // Keyboard approval respects the product's protection against accidental
  // pointer clicks immediately after a permission card appears.
  await button.press("Enter");
}
try {
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1000 }); return true; } catch { return false; } }, "WebView2");
  page = browser.contexts()[0].pages()[0]; page.setDefaultTimeout(15000); page.on("pageerror", e => errors.push(e.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true");
  const prefs = await invoke("projects_init", { path: project, name: "异步协作验收" });
  await page.evaluate(async prefs => (await import("./09-sessions.js")).enterProject(prefs), prefs);
  check((await invoke("process_list", { projectDir: project })).length === 0, "Project initialization creates no empty conversation");
  owner = await invoke("process_create", { projectDir: project, profile: "dev", model: "stub:interactive-main" });
  await page.evaluate(async id => { const sessions = await import("./09-sessions.js"); await sessions.refreshProcesses(); await sessions.switchProcess(id, true); }, owner.id);
  await page.evaluate(async () => {
    window.asyncProgress = [];
    window.asyncJobs = [];
    await window.__TAURI__.event.listen("kz:tool-progress", event => window.asyncProgress.push(event.payload));
    await window.__TAURI__.event.listen("kz:agent-job", event => window.asyncJobs.push(event.payload));
  });
  await invoke("process_update", { processId: owner.id, subagentMode: "auto", });
  await prompt("MAIN_PARALLEL: dispatch two independent tasks");
  await until(() => held.has("alpha") && held.has("beta"), "Two simultaneous model requests");
  await idle();
  check((await jobs()).filter(j => j.state === "running").length === 2, "Two real child requests remain active after the main turn returns");
  check(!await running(), "Main conversation does not wait for all background children");
  check(requests.find(r => r.kind === "parallel").messages.filter(m => m.role === "user" && JSON.stringify(m).includes("MAIN_PARALLEL")).length === 1, "Initial input is fed once rather than repeated in restored history");
  if (!await page.locator("#tasks-panel").isVisible()) await page.locator("#tasks-toggle").click();
  await page.locator("#tasks-panel").waitFor({ state: "visible" });
  await page.screenshot({ path: path.join(output, "two-children-dark.png") });
  await prompt("STATUS_WHILE_BACKGROUND: answer while both children are busy");
  await until(() => counts.has("status"), "Main follow-up"); await idle();
  check((await jobs()).filter(j => j.state === "running").length === 2, "Main accepts a new conversation turn while two children keep running");
  const snapshot = await command({ action: "collect" });
  check(snapshot.length === 0, "Collect returns immediately without waiting for running tasks");
  held.get("alpha")(); held.get("beta")();
  await until(async () => (await jobs()).every(j => j.state === "done"), "Child completion");
  await until(() => requests.some(r => !r.child && JSON.stringify(r.messages).includes("ALPHA_RESULT") && JSON.stringify(r.messages).includes("BETA_RESULT")), "Automatic completion delivery"); await idle();
  check(true, "Completion callbacks wake the idle original main conversation");
  check(requests.filter(r => r.kind === "callback").every(r => r.model === "interactive-main"), "Idle callbacks retain the main conversation's selected model");

  await prompt("MAIN_STEER: hold this request before a read");
  await until(() => held.has("steer"), "Main held");
  await prompt("STEERING_BOUNDARY: use my updated acceptance criterion", { delivery: "steer" });
  held.get("steer")();
  await until(() => counts.has("steered"), "Steering consumed"); await idle();
  check(requests.find(r => r.kind === "steered").messages.some(m => m.role === "tool" && JSON.stringify(m).includes("unchanged")), "Steering is consumed between complete tool/result batches");

  await prompt("MAIN_HANDOFF: run a foreground terminal and wait for my release file");
  await allowOnce();
  await until(() => page.evaluate(() => window.asyncProgress.some(p => p.chunk.includes("HANDOFF_PREFIX:"))), "Foreground shell has actually produced output");
  const prefix = await page.evaluate(() => window.asyncProgress.find(p => p.chunk.includes("HANDOFF_PREFIX:")).chunk);
  await page.locator("#delivery-select").selectOption("queue");
  await page.locator("#prompt").fill("HANDOFF_STEER: handle this input while the same terminal keeps running");
  await page.locator("#prompt").press("Control+Enter");
  await until(() => counts.has("handoff-steered"), "Ctrl+Enter foreground handoff"); await idle();
  const handoffResult = requests.find(r => r.kind === "handoff-steered").messages.findLast(m => m.role === "tool" && String(m.content).includes("background: true"));
  check(Boolean(handoffResult), "Ctrl+Enter bypasses queue mode and admits steering in the current tool batch");
  const processId = /process_id: (\S+)/.exec(handoffResult.content)[1];
  check(/pid: (\d+)/.exec(handoffResult.content)[1] === /HANDOFF_PREFIX:(\d+)/.exec(prefix)[1], "The handed-off shell keeps the original OS process, with no rerun");
  check(!requests.some(r => r.kind === "terminal-callback" && JSON.stringify(r.messages).includes(processId)), "Main answers the new input before the blocked shell finishes");
  const other = await invoke("process_create", { projectDir: project, profile: "dev" });
  await page.evaluate(async id => { const sessions = await import("./09-sessions.js"); await sessions.refreshProcesses(); await sessions.switchProcess(id, true); }, other.id);
  await writeFile(path.join(project, "handoff-release.txt"), "release the existing process");
  await until(() => requests.some(r => r.kind === "terminal-callback" && JSON.stringify(r.messages.findLast(m => m.role === "user")).includes(processId)), "Handoff completion callback"); await idle();
  const handoffCallback = requests.filter(r => r.kind === "terminal-callback" && JSON.stringify(r.messages.findLast(m => m.role === "user")).includes(processId));
  check(handoffCallback.length === 1 && JSON.stringify(handoffCallback[0].messages.findLast(m => m.role === "user")).includes("HANDOFF_SUFFIX")
    && JSON.stringify(handoffCallback[0].messages.findLast(m => m.role === "user")).includes("HANDOFF_PREFIX:"), "One completion callback contains both the pre-handoff and final output");
  check((await invoke("conversation_display_get", { projectDir: project, processId: other.id })).length === 0, "Switching conversations cannot deliver terminal output to the displayed unrelated conversation");
  await page.evaluate(async id => (await import("./09-sessions.js")).switchProcess(id, true), owner.id);

  await prompt("MAIN_ASK_ASYNC: ask and continue independent work");
  const mainQuestion = await question("MAIN_ASYNC_CHOICE"); await idle();
  check(mainQuestion.background && !mainQuestion.agentId, "Main async question keeps its original ownership");
  check(!await running(), "An unanswered async question does not keep the main turn occupied");
  await invoke("answer_ask", { id: mainQuestion.id, reply: "B" });
  await until(() => counts.has("main-answer"), "Main answer callback"); await idle();
  check(!(await invoke("softwire_questions")).some(q => q.id === mainQuestion.id), "Answer removes the pending question and wakes its owner");

  const child = await command({ action: "spawn", agent: "plan", description: "异步提问", prompt: "CHILD_ASK_ASYNC ask then work independently" });
  const childQuestion = await question("CHILD_ASYNC_CHOICE");
  await until(async () => (await jobs()).find(j => j.id === child.id)?.state === "done", "Child independent result"); await idle();
  check(childQuestion.agentId === child.id && childQuestion.background, "Child question names the exact child and remains answerable after its turn ends");
  await invoke("answer_ask", { id: childQuestion.id, reply: "A" });
  await until(() => requests.some(r => r.kind === "child-question" && JSON.stringify(r.messages).includes("用户回答异步问题")), "Child answer resume");
  await until(async () => (await jobs()).find(j => j.id === child.id)?.attempt === 2 && (await jobs()).find(j => j.id === child.id)?.state === "done", "Same child second attempt"); await idle();
  check(true, "Answer resumes the same child with its saved history");

  await prompt("MAIN_TERMINAL: run a background terminal");
  await allowOnce();
  await until(() => requests.some(r => r.kind === "terminal-callback" && JSON.stringify(r.messages.findLast(m => m.role === "user")).includes("MAIN_TERMINAL_RESULT")), "Main terminal callback"); await idle();
  check(true, "Real terminal final output and exit callback reach the original main conversation");
  const terminalChild = await command({ action: "spawn", agent: "implement", description: "终端回调", prompt: "CHILD_TERMINAL start a background command" });
  await allowOnce();
  await until(() => requests.some(r => r.kind === "child-terminal" && JSON.stringify(r.messages.findLast(m => m.role === "user")).includes("后台终端完成")), "Child terminal callback");
  await until(async () => (await jobs()).find(j => j.id === terminalChild.id)?.attempt === 2 && (await jobs()).find(j => j.id === terminalChild.id)?.state === "done", "Child terminal second attempt"); await idle();
  check(true, "Child terminal completion resumes the child rather than posting a raw result to a different conversation");

  const alphaBeforeStop = counts.get("alpha") || 0;
  const old = await command({ action: "spawn", agent: "explore", description: "停止与重派", prompt: "CHILD_ALPHA hold for explicit stop" });
  await until(() => (counts.get("alpha") || 0) > alphaBeforeStop, "Stop target's actual in-flight request");
  await command({ action: "stop", id: old.id });
  await until(async () => (await jobs()).find(j => j.id === old.id)?.state === "stopped", "Stopped");
  const oldRelease = held.get("alpha"); const before = requests.length;
  oldRelease(); await new Promise(resolve => setTimeout(resolve, 400));
  check(requests.length === before, "Late response cannot wake a child explicitly stopped by the user");
  const restarted = await command({ action: "restart", id: old.id });
  await until(async () => (await jobs()).find(j => j.id === restarted.id)?.state === "running", "Fresh dispatch");
  check(restarted.id !== old.id && (await jobs()).find(j => j.id === restarted.id)?.replaces === old.id, "Restart creates a fresh task and preserves the old task record");
  await command({ action: "stop", id: restarted.id }); await idle();
  const closing = await command({ action: "spawn", agent: "plan", description: "关闭会话", prompt: "CHILD_BETA hold while closing the main session" });
  await until(async () => (await jobs()).find(j => j.id === closing.id)?.state === "running", "Close target");
  await invoke("process_close", { processId: owner.id });
  await until(() => page.evaluate(id => window.asyncJobs.some(event => event.job.id === id && event.job.state === "stopped"), closing.id), "Closed owner stops children");
  check(true, "Closing an idle main session also stops its background children");
  await page.screenshot({ path: path.join(output, "completed-dark.png") });
  check(errors.length === 0, `No browser/runtime errors: ${errors.join("; ")}`);
  const report = { checks, errors, requests: requests.map(({ kind, at }) => ({ kind, at })), executable: exe, executableSha256, boundary: "Real Windows UI, Rust scheduler, SQLite, Git and terminal; model responses supplied by an isolated local server." };
  await writeFile(path.join(run, "acceptance.json"), JSON.stringify(report, null, 2));
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify(report, null, 2));
  console.log(`${checks.length} async native checks passed (${requests.length} model requests)`);
} catch (error) {
  await page?.screenshot({ path: path.join(output, "failure.png") }).catch(() => {});
  const report = { error: String(error), checks, errors, requests, executable: exe, executableSha256 };
  await writeFile(path.join(run, "failure.json"), JSON.stringify(report, null, 2));
  await writeFile(path.join(output, "failure.json"), JSON.stringify(report, null, 2));
  throw error;
} finally {
  if (page) await invoke("runtime_shutdown").catch(() => {});
  await browser?.close();
  if (app.exitCode === null) { const stopped = new Promise(resolve => app.once("exit", resolve)); app.kill(); await stopped; }
  model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
}
