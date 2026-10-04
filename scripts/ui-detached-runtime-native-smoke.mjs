// Real desktop/UI exit + independently surviving runtime + model/terminal callbacks.
/* global window, document */
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { mkdir, copyFile, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

const output = path.resolve("output/playwright/detached-runtime");
const run = path.join(output, String(Date.now())), project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "dark" }));
await writeFile(path.join(project, "original.txt"), "unchanged");
const git = (...args) => execFileSync("git", ["-C", project, ...args], { windowsHide: true, encoding: "utf8" }).trim();
git("init"); git("config", "user.name", "Runtime acceptance"); git("config", "user.email", "test@local"); git("add", "."); git("commit", "-m", "fixture");
const checks = [], requests = [], errors = [], held = new Map(), counts = new Map();
const check = (value, label) => { assert(value, label); checks.push(label); console.log(label); };
function answer(res, text, calls = []) {
  if (res.destroyed || res.writableEnded) return;
  if (!res.headersSent) res.writeHead(200, { "Content-Type": "text/event-stream" });
  const delta = calls.length ? { tool_calls: calls.map((call, index) => ({ index, id: call.id, type: "function", function: { name: call.name, arguments: JSON.stringify(call.input) } })) } : { content: text };
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: calls.length ? "tool_calls" : "stop" }], usage: { prompt_tokens: 60, completion_tokens: 15 } })}\n\n`);
  res.end("data: [DONE]\n\n");
}
const call = (id, name, input) => ({ id, name, input });
const model = http.createServer(async (req, res) => {
  let raw = ""; for await (const chunk of req) raw += chunk;
  if (req.url === "/preview") { res.writeHead(200, {"Content-Type":"text/html; charset=utf-8"}); res.end("<title>Native preview</title><h1>VISIBLE_PANE_CONTENT</h1>"); return; }
  if (req.url !== "/v1/chat/completions") { res.writeHead(404); res.end(); return; }
  try {
    const data = JSON.parse(raw), messages = data.messages || [], history = JSON.stringify(messages);
    const system = JSON.stringify(messages.filter(m => m.role === "system")), last = JSON.stringify(messages.findLast(m => m.role === "user")?.content || "");
    const child = system.includes("You own one delegated task"), side = system.includes("独立的临时问题");
    const boundWork = last.includes("MAIN_WORK_ITEM");
    const kind = side ? "side" : boundWork ? last.includes("只读勘察代理") ? "work-scout" : last.includes("只读复核代理") ? "work-review" : "work-main"
      : child ? history.includes("CHILD_DURABLE") ? "child-question" : history.includes("CHILD_ONE") ? "child-one" : "child-two"
      : last.includes("用户回答异步问题") ? "answer" : last.includes("日志订阅更新") ? "monitor" : last.includes("后台终端完成") ? "terminal-callback"
        : last.includes("MAIN_HOLD") ? "main-hold" : last.includes("MAIN_DURABLE") ? "main-question" : last.includes("MAIN_LOGS") ? "terminal"
          : last.includes("MAIN_BROWSER") ? "browser" : last.includes("MAIN_SCREENSHOT") ? "screenshot" : last.includes("AUTO_GOAL") ? "auto" : "callback";
    const step = (counts.get(kind) || 0) + 1; counts.set(kind, step); requests.push({ kind, child, messages, tools: data.tools, model: data.model });
    if (side) { res.writeHead(200, { "Content-Type": "text/event-stream" }); res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: { content: "独立回答正在流式显示" }, finish_reason: null }] })}\n\n`); held.set("side", () => answer(res, "。临时问答完成。")); }
    else if (["main-hold", "child-one", "child-two", "work-main"].includes(kind)) held.set(kind, () => answer(res, `${kind.toUpperCase()}_DONE`));
    else if (kind === "work-scout") answer(res, "original.txt:1 contains the isolated fixture. No product changes are requested by this IPC check.");
    else if (kind === "work-review") answer(res, "NO_ISSUES");
    else if (kind === "auto") { if (step === 1) held.set("auto", () => answer(res, "继续推进，目标尚未达成。")); else held.set("auto-next", () => answer(res, "已暂停验收模型。")); }
    else if (kind === "main-question" && step === 1) answer(res, null, [call("durable-main", "question", { question: "MAIN_DURABLE_CHOICE", options: ["A", "B"], background: true })]);
    else if (kind === "child-question" && step === 1) answer(res, null, [call("durable-child", "question", { question: "CHILD_DURABLE_CHOICE", options: ["A", "B"], background: true })]);
    else if (kind === "browser" && step === 1) answer(res, null, [call("pane", "browser", { action:"dom" })]);
    else if (kind === "screenshot" && step === 1) answer(res, null, [call("screen", "ui_screenshot", {})]);
    else if (kind === "terminal" && step === 1) answer(res, null, [call("logs", "bash", { command: "Write-Output LOG_ONE; Start-Sleep -Seconds 6; Write-Output LOG_TWO; Start-Sleep -Seconds 6; Write-Output LOG_THREE; Start-Sleep -Seconds 10; Write-Output LOG_DONE", background: true })]);
    else answer(res, `RESULT_${kind}_${step}`);
  } catch (e) { errors.push(String(e)); answer(res, "LOCAL_MODEL_ERROR"); }
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(appHome, "kanzei.toml"), `[models]\nprimary = "stub:primary"\nfast = "stub:fast"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n`);
const exe = path.join(run, "kzapp.exe"); await copyFile(path.resolve(process.argv[2]), exe);
let app, browser, page, owner, servicePid;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
const command = input => invoke("agent_team_command", { projectDir: project, processId: owner.id, input });
const prompt = (text, extra = {}) => invoke("run_prompt", { projectDir: project, processId: owner.id, profile: "dev", agent: "dev-pair", model: "stub:chosen", prompt: text, autonomous: false, ...extra });
const running = async () => (await invoke("process_list", { projectDir: project })).find(p => p.id === owner.id)?.running;
async function until(fn, label, ms = 50000) { const end = Date.now() + ms; while (Date.now() < end) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 150)); } throw new Error(`${label} timed out`); }
async function idle() { await until(async () => !await running(), "main idle"); }
async function open() {
  const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve)); const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
  app = spawn(exe, [], { cwd: project, windowsHide: true, stdio: "ignore", env: { ...process.env, KANZEI_HOME: appHome, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"), APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port) } });
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 800 }); return true; } catch { return false; } }, "WebView2");
  page = browser.contexts()[0].pages()[0]; page.setDefaultTimeout(25000); page.on("pageerror", e => errors.push(e.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true", null, { timeout: 60000 });
  servicePid = (await invoke("runtime_status")).pid;
}
async function closeUi() {
  if (app.exitCode === null) { const exited = new Promise(resolve => app.once("exit", resolve)); app.kill(); await exited; }
  await browser?.close().catch(() => {}); browser = null;
}
const alive = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };
try {
  await open(); check(servicePid !== app.pid && alive(servicePid), "Execution runs in a separate OS process");
  await invoke("projects_init", { path: project, name: "后台运行验收" }); await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await Promise.all(Array.from({length:24},(_,i)=>Promise.all([
    invoke("projects_rename",{path:project,name:"后台运行验收"}),
    invoke("ui_prefs_set",{ui_layout:{detachedAcceptance:{revision:i}}}),
  ])));
  const prefs=await invoke("projects_get");
  check(prefs.projects.length===1&&prefs.names[project.replaceAll("/","\\")]==="后台运行验收","Concurrent project and UI preference saves preserve project identity");
  owner = (await invoke("process_list", { projectDir: project })).find(p => !["readonly", "research"].includes(p.profile));
  await invoke("process_update", { processId: owner.id, subagentMode: "auto", });
  await invoke("docs_update", { projectDir: project, kind: "req", action: "add", id: "", title: "原生条目绑定验收", status: "todo", priority: "P1", fields: { "复杂度": "小", "标签": "前端", "验收": "工作台选择的 R-001 在真实 IPC 中认领并绑定本轮" } });
  await prompt("MAIN_WORK_ITEM validate this native start/continue binding", { executionBatch: true, workItemId: "R-001" });
  await until(() => held.has("work-main"), "bound work main request");
  const boundDocs = await invoke("docs_snapshot", { projectDir: project });
  check(boundDocs.requirements.find(item => item.id === "R-001")?.status === "doing", "Native workItemId IPC claims the selected requirement before model execution");
  const boundProject = (await invoke("workspace_overview")).projects.find(item => item.path === project.replaceAll("/", "\\"));
  check(boundProject?.lines.find(line => line.id === owner.id)?.current_item_id === "R-001", "Native running overview binds the exact selected item to the current run");
  check(requests.filter(r => r.kind === "work-scout").length === 2, "Native execution batch runs both required scouts before the main request");
  held.get("work-main")(); await idle();
  check(requests.some(r => r.kind === "work-review"), "Native selected-item execution retains the independent review gate");
  await prompt("MAIN_HOLD keep the original run active"); await until(() => held.has("main-hold"), "main request");
  await page.getByRole("button", { name: "旁路提问", exact: true }).click();
  await page.getByRole("textbox", { name: "临时问题", exact: true }).fill("SIDE_ISOLATED explain the current task");
  await page.locator("#async-workspace").getByRole("button", { name: "发送", exact: true }).click();
  await until(() => held.has("side"), "independent model request");
  await page.locator(".async-answer-text").filter({ hasText: "独立回答正在流式显示" }).waitFor();
  check(await running(), "Side Q&A streams while the main model request is still held");
  check(!requests.find(r => r.kind === "side").tools?.length, "Side Q&A has no executable tools");
  held.get("side")(); await until(async () => (await invoke("side_question_list", { projectDir: project, processId: owner.id }))[0]?.status === "done", "side complete");
  check(!JSON.stringify(await invoke("conversation_display_get", { projectDir: project, processId: owner.id })).includes("SIDE_ISOLATED"), "Temporary question never enters the main conversation");
  await page.screenshot({ path: path.join(output, "side-question-dark.png") });
  await page.evaluate(() => document.documentElement.dataset.theme = "light");
  await page.screenshot({ path: path.join(output, "side-question-light.png") });
  await page.setViewportSize({width:800,height:700});
  await page.evaluate(async () => { await Promise.all(document.getAnimations().filter(a => a.effect?.getComputedTiming().iterations !== Infinity).map(a => a.finished.catch(() => {}))); });
  check(await page.locator("#async-workspace").evaluate(el => { const r=el.getBoundingClientRect(); return r.left>=0&&r.top>=0&&r.right<=window.innerWidth+1&&r.bottom<=window.innerHeight+1; }), "Temporary Q&A remains reachable in a narrow desktop window");
  await page.screenshot({ path: path.join(output, "side-question-narrow.png") });
  await page.setViewportSize({width:1440,height:1000});
  await page.evaluate(() => document.documentElement.dataset.theme = "dark");
  await page.locator("#async-workspace").getByRole("button", { name: "收起", exact: true }).click();
  const firstChild = await command({ action: "spawn", agent: "explore", description: "第一项", prompt: "CHILD_ONE" });
  const secondChild = await command({ action: "spawn", agent: "plan", description: "第二项", prompt: "CHILD_TWO" });
  check(firstChild.id && secondChild.id && firstChild.id !== secondChild.id, "Native team IPC dispatches two distinct child tasks");
  await until(() => held.has("child-one") && held.has("child-two"), "both children");
  const originalPid = servicePid; await closeUi(); check(alive(originalPid), "Killing the desktop process leaves the runtime alive");
  held.get("main-hold")(); held.get("child-one")(); held.get("child-two")();
  await until(() => requests.some(r => r.kind === "callback" && JSON.stringify(r.messages).includes("CHILD-TWO_DONE")), "callback without UI");
  check(true, "Child completions wake the main actor with no desktop process running");
  await open(); check(servicePid === originalPid, "Reopening reconnects to the same runtime instead of dispatching again"); await idle();
  check((await command({ action: "list" })).filter(j => j.state === "done").length === 2, "Completed child results remain visible after reconnect");
  await prompt("MAIN_DURABLE ask a question asynchronously"); await idle();
  const child = await command({ action: "spawn", agent: "plan", description: "持久提问", prompt: "CHILD_DURABLE" });
  await until(async () => (await invoke("softwire_questions")).length === 2, "both durable questions");
  await until(async () => (await command({ action: "get", id: child.id })).job.state === "done", "child waiting asynchronously"); await idle();
  const beforeQuestions = await invoke("softwire_questions"); await closeUi(); process.kill(servicePid);
  await until(() => !alive(servicePid), "runtime crash"); await open();
  const restored = await invoke("softwire_questions");
  check(restored.length === 2 && restored.every(q => beforeQuestions.some(old => old.id === q.id && old.revision === q.revision)), `Main and child questions survive runtime loss with stable identity and revision (${restored.length})`);
  const main = restored.find(q => !q.agentId), childQ = restored.find(q => q.agentId === child.id);
  const response = { projectDir: project, sessionId: main.sessionId, id: main.id, expectedRevision: main.revision, requestId: "main-recovery-answer", reply: "B" };
  await invoke("softwire_answer_question", response); await until(() => counts.has("answer"), "main recovered answer"); await idle();
  const count = requests.length; await invoke("softwire_answer_question", response); await new Promise(resolve => setTimeout(resolve, 500));
  check(requests.length === count, "Retrying the same recovered answer does not run it twice");
  await invoke("softwire_answer_question", { ...response, id: childQ.id, expectedRevision: childQ.revision, requestId: "child-recovery-answer", reply: "A" });
  await until(async () => { const job = (await command({ action: "get", id: child.id })).job; return job.attempt === 2 && job.state === "done"; }, "recovered child continuation"); await idle();
  check(!(await invoke("softwire_questions")).length, "Answering recovered questions resumes the original main and original child and clears the inbox");
  counts.delete("main-question");
  await prompt("MAIN_DURABLE stop retires the question"); await idle();
  const cancelled=(await invoke("softwire_questions"))[0];
  await invoke("stop_run",{projectDir:project,processId:owner.id});
  check(!(await invoke("softwire_questions")).length, "Explicit stop retires persisted pending questions");
  await assert.rejects(()=>invoke("softwire_answer_question",{...response,id:cancelled.id,expectedRevision:cancelled.revision,requestId:"after-stop",reply:"A"}));
  check(true,"A late reply cannot restart a stopped task");
  await invoke("preview_open",{target:`http://127.0.0.1:${model.address().port}/preview`,processId:owner.id,bounds:{x:800,y:80,w:580,h:450}});
  await invoke("preview_set_visible",{visible:true,processId:owner.id});
  await prompt("MAIN_BROWSER inspect the visible preview"); await idle();
  const browserResult=JSON.stringify(requests.filter(r=>r.kind==="browser").at(-1)?.messages);
  check(browserResult.includes("backend: pane")&&browserResult.includes("VISIBLE_PANE_CONTENT"),"Background browser tool uses the visible desktop preview");
  await invoke("preview_close");
  await prompt("MAIN_SCREENSHOT capture current UI"); await idle();
  check(requests.some(r=>r.messages.some(m=>m.tool_call_id==="screen"&&String(m.content).includes("[screenshot]"))&&JSON.stringify(r.messages).includes("data:image/png;base64,")),"Background screenshot tool returns pixels from the visible UI process");
  await prompt("MAIN_LOGS start a terminal"); await page.getByRole("button", { name: "允许一次", exact: true }).click(); await idle();
  const listArgs = { projectDir: project, processId: owner.id, action: "list" }; let terminal;
  await until(async () => (terminal = (await invoke("terminal_monitor", listArgs)).find(p => p.running)), "terminal");
  await invoke("terminal_monitor", { ...listArgs, action: "watch", id: terminal.id });
  await until(() => (counts.get("monitor") || 0) >= 2, "two incremental callbacks");
  check(true, "Continuous terminal output produces multiple asynchronous actor callbacks before exit");
  const updates=requests.filter(r=>r.kind==="monitor").map(r=>r.messages.findLast(m=>m.role==="user")?.content);
  check(String(updates[0]).includes("LOG_ONE")&&String(updates[1]).includes("LOG_TWO")&&!String(updates[1]).includes("LOG_ONE"),"Incremental log callbacks do not replay old lines");
  await invoke("terminal_monitor", { ...listArgs, action: "unwatch", id: terminal.id });
  check((await invoke("terminal_monitor", listArgs)).find(p => p.id === terminal.id).running, "Cancelling the subscription leaves the terminal running");
  await page.evaluate(async () => (await import("./28-async-workspace.js")).openAsyncWorkspace("", "logs"));
  await page.evaluate(async () => { await Promise.all(document.getAnimations().filter(a => a.effect?.getComputedTiming().iterations !== Infinity).map(a => a.finished.catch(() => {}))); });
  await page.screenshot({ path: path.join(output, "terminal-logs-dark.png") });
  await until(() => counts.has("terminal-callback"), "terminal final callback"); await idle();
  check(requests.some(r => r.kind === "terminal-callback" && JSON.stringify(r.messages).includes("LOG_DONE")), "Final terminal callback contains the complete final output");
  await page.locator("#async-workspace").getByRole("button", { name: "收起", exact: true }).click();
  await invoke("auto_state_update", { sessionId: owner.session_id, enabled: true, paused: false, goal: "AUTO_GOAL verify continued execution after closing the UI" });
  await prompt("AUTO_GOAL initial round", { agent: "dev", autonomous: true }); await until(() => held.has("auto"), "auto first request");
  await closeUi(); held.get("auto")(); await until(() => held.has("auto-next"), "backend next round", 65000);
  check(true, "Autonomous next-round scheduling continues while the desktop process is closed");
  await open(); await invoke("stop_run", { projectDir: project, processId: owner.id });
  check(errors.length === 0, `No UI errors: ${errors.join("; ")}`);
  await page.getByRole("button",{name:"后台运行",exact:true}).click();
  await page.getByRole("button",{name:"停止后台任务并退出",exact:true}).click();
  await until(()=>!alive(servicePid),"explicit background exit");
  check(true,"Explicit exit stops the background process");
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ checks, errors, executable: exe, requests: requests.map(({ kind, model }) => ({ kind, model })), boundary: "Real independent Windows runtime, WebView2, SQLite, filesystem, terminal. Deterministic local model." }, null, 2));
  console.log(`${checks.length} detached runtime checks passed`);
} catch (error) {
  await page?.screenshot({ path: path.join(output, "failure.png") }).catch(() => {});
  const jobs = page && owner ? await command({ action: "list" }).catch(e => ({ error: String(e) })) : [];
  await writeFile(path.join(output, "failure.json"), JSON.stringify({ error: String(error), checks, errors, requests, jobs, run, servicePid }, null, 2)); throw error;
} finally {
  await closeUi().catch(() => {});
  if (servicePid && alive(servicePid)) process.kill(servicePid);
  model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
}
