// Production ESM/CSS in Edge; disposable IPC exercises first-send ownership.
/* global window, document, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const before = process.argv.includes("--before");
const output = path.resolve("dist/sidebar-draft-20261007", before ? "before" : "after");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
const checks = [], errors = [];
const check = (value, text) => { assert(value, text); checks.push(text); };
page.on("pageerror", error => errors.push(error.message));
if (before) {
  for (const file of ["09-sessions.js", "29-general-chat.js", "03-workspaces.js", "08-compose-runtime.js", "08-models.js", "12-workbench.js", "26-project-conversations.js", "workbench.css", "familiar-workspace.css"]) {
    const source = execFileSync("git", ["show", `f99b5d7d:crates/kanzei-app/ui/${file}`], { encoding: "utf8" });
    await page.route(`**/${file}`, route => route.fulfill({ status: 200, contentType: file.endsWith("css") ? "text/css" : "text/javascript", body: source }));
  }
}
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const open = project => page.evaluate(async project => (await import("/12-workbench.js")).openProjectSpace(project, "chat"), project);
const state = () => page.evaluate(async () => {
  const shell = await import("/03-shell.js");
  return { project: shell.currentProject, process: shell.activeProcessId, session: shell.activeSessionId,
    prompt: document.getElementById("prompt").value, creates: window.__empty.creates, runs: window.__empty.runs,
    messages: document.querySelector(".msg-pane[data-active='1']")?.textContent || "",
    messageCount: document.querySelectorAll(".msg-pane[data-active='1'] .msg").length };
});
const edit = text => page.locator("#prompt").fill(text);
const send = () => page.evaluate(async () => (await import("/08-compose-runtime.js")).send());
try {
  await page.goto(`${server.origin}/?scene=workspace&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  const projects = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures();
    const seed = structuredClone(f.state.processes[0]);
    const projects = ["C:/smoke/empty-a", "C:/smoke/empty-b", "C:/smoke/empty-c"];
    const general = "C:/smoke/empty-general";
    f.state.processes = [];
    window.__empty = { f, seed, projects, general, creates: [], runs: [], failCreate: false, gate: false };
    window.__kzPreview.setCommand("projects_select", ({ path }) => ({ current: path, projects, names: {} }));
    window.__kzPreview.setCommand("projects_get", { current: projects[0], projects, names: {} });
    window.__kzPreview.setCommand("process_create", async args => {
      const state = window.__empty;
      state.creates.push(args);
      if (state.failCreate) throw "测试：创建失败";
      if (state.gate) await new Promise(resolve => { state.release = resolve; });
      return f.commands.process_create(args);
    });
    window.__kzPreview.setCommand("run_prompt", (args, ctx) => {
      window.__empty.runs.push(args);
      const process = f.state.processes.find(item => item.id === args.processId);
      f.state.conversations.set(args.processId, [...(f.state.conversations.get(args.processId) || []),
        { role: "user", parts: [{ type: "text", text: args.prompt }] },
        { role: "assistant", parts: [{ type: "text", text: "测试回复：任务已收到。" }] }]);
      ctx.emit("kz:idle", { sessionId: process.session_id, reason: "completed" });
      return null;
    });
    window.__kzPreview.setCommand("workspace_snapshot", { projects: projects.map(path => ({ path, lines: [], error: "测试：状态尚未加载", freshness: "unavailable" })) });
    window.__kzPreview.setCommand("general_chat_open", general);
    const sessions = await import("/09-sessions.js"), tree = await import("/12-session-tree.js");
    projects.forEach(path => tree.setSidebarOpen(path, false));
    sessions.renderProjects({ current: projects[0], projects, names: {} }, { activate: false });
    await (await import("/12-docs-pages.js")).refreshWorkspace();
    return { projects, general };
  });
  const [a, b, c] = projects.projects;
  const indicator = () => page.evaluate(() => [...document.querySelectorAll(".workbench-project-link")].map(link => {
    const dot = link.querySelector(".workbench-project-activity");
    return { state: link.dataset.activity, text: dot.textContent, hidden: dot.hidden, width: dot.getBoundingClientRect().width,
      animation: getComputedStyle(dot).animationName, after: getComputedStyle(link, "::after").content };
  }));
  const unknown = await indicator();
  await page.locator(`.workbench-project-link[data-path=${JSON.stringify(a)}]`).click();
  await settle();
  const opened = await state();
  if (before) {
    await page.screenshot({ path: path.join(output, "empty-project.png") });
    await writeFile(path.join(output, "observed.json"), JSON.stringify({ opened, unknown, errors }, null, 2));
    console.log(JSON.stringify({ before: true, createsOnOpen: opened.creates.length, unknown }));
  } else {
    check(opened.project === a && !opened.process && !opened.session && opened.creates.length === 0, "Opening an empty project does not persist a conversation");
    check(opened.messageCount === 0, "An empty project does not display the previous project's conversation");
    check(unknown.every(row => row.text === "" && row.hidden && row.width === 0 && row.animation === "none"), "Unavailable projects have no pale bar or loading animation");
    await page.screenshot({ path: path.join(output, "empty-project.png") });
    await page.evaluate(async () => {
      const { projects, seed } = window.__empty;
      window.__kzPreview.setCommand("workspace_snapshot", { projects: projects.map((path, index) => ({ path,
        lines: index === 0 ? [{ ...seed, session_id: "stale-session", running: true }] : [], freshness: "fresh" })) });
      await (await import("/12-docs-pages.js")).refreshWorkspace();
      (await import("/12-workbench.js")).renderProjectActivity();
    });
    check((await indicator())[0].state === "idle", "An empty current process list wins over stale running jobs in the overview snapshot");
    await open(a); await send();
    check((await state()).creates.length === 0, "Repeated navigation and empty send create nothing");
    await edit("A 的未发送草稿"); await open(b);
    check((await state()).prompt === "", "An empty project has its own blank draft");
    await edit("B 的草稿"); await open(a);
    check((await state()).prompt === "A 的未发送草稿" && (await state()).creates.length === 0, "Unsent drafts survive switching between empty projects without creation");
    await page.evaluate(async () => {
      const models = await import("/08-models.js");
      await models.setLineModel("codex:gpt-6-sol"); await models.setLineReasoning("medium");
    });
    check((await state()).creates.length === 0 && await page.evaluate(async () => {
      const { effectiveModel } = await import("/08-models.js");
      return effectiveModel.model.resolved === "codex:gpt-6-sol" && effectiveModel.reasoning.value === "medium";
    }), "Model and reasoning can be selected before the first persisted conversation");
    await open(b); await open(a);
    check(await page.evaluate(async () => (await import("/08-models.js")).effectiveModel.model.resolved === "codex:gpt-6-sol"), "An unsent project's model choice survives navigation");
    await edit("/compact"); await send();
    check((await state()).creates.length === 0, "A local compact command does not create an empty conversation");
    await edit("首次实际任务");
    await page.evaluate(async () => { const { send } = await import("/08-compose-runtime.js"); await Promise.all([send(), send()]); });
    let sent = await state();
    check(sent.creates.length === 1 && sent.runs.length === 1 && sent.runs[0].processId === sent.process
      && sent.runs[0].projectDir === a && sent.runs[0].prompt === "首次实际任务", "Concurrent first sends create one conversation and send once to that identity");
    check(sent.prompt === "" && Boolean(sent.session), "Successful first send clears the submitted draft and binds session identity");
    check(sent.creates[0].model === "codex:gpt-6-sol" && sent.creates[0].reasoning === "medium" && sent.runs[0].model === "codex:gpt-6-sol", "The first stored conversation and task use the selected draft model and reasoning");
    await edit("第二次任务"); await send();
    check((await state()).creates.length === 1 && (await state()).runs.length === 2, "Later sends reuse the persisted conversation");
    await edit("已有对话的未发送草稿"); await open(b); await open(a);
    check((await state()).prompt === "已有对话的未发送草稿", "Restoring a stored conversation preserves its own draft instead of a temporary empty identity");
    await open(b); await page.evaluate(() => { window.__empty.failCreate = true; }); await send();
    check((await state()).prompt === "B 的草稿" && !(await state()).process && (await state()).runs.length === 2, "Creation failure preserves the draft and sends nothing");
    await page.evaluate(() => { window.__empty.failCreate = false; window.__empty.gate = true; });
    await page.evaluate(async () => { window.__empty.pendingSend = (await import("/08-compose-runtime.js")).send(); });
    await page.waitForFunction(() => Boolean(window.__empty.release));
    await open(c); await edit("C 的新草稿");
    await page.evaluate(async () => { window.__empty.gate = false; window.__empty.release(); await window.__empty.pendingSend; });
    sent = await state();
    check(sent.project === c && !sent.process && sent.prompt === "C 的新草稿" && sent.runs.length === 2, "A late creation cannot send into a newly selected project or overwrite its draft");
    await open(b); await send();
    check((await state()).runs.at(-1).projectDir === b && (await state()).runs.at(-1).prompt === "B 的草稿", "The original draft remains sendable after canceled navigation and reuses the created conversation");
    await open(c);
    await page.evaluate(async () => { window.__empty.gate = true; window.__empty.release = null; window.__empty.pendingSend = (await import("/08-compose-runtime.js")).send(); });
    await page.waitForFunction(() => Boolean(window.__empty.release));
    await edit("创建期间继续输入的新草稿");
    await page.evaluate(async () => { window.__empty.gate = false; window.__empty.release(); await window.__empty.pendingSend; });
    check((await state()).runs.at(-1).prompt === "C 的新草稿" && (await state()).prompt === "创建期间继续输入的新草稿", "A delayed first send submits its snapshot and preserves newly typed text");
    const beforeGeneral = (await state()).creates.length;
    await page.evaluate(async () => (await import("/29-general-chat.js")).openGeneralChat());
    check(!(await state()).process && (await state()).creates.length === beforeGeneral, "Opening empty general chat also defers persistence");
    check(await page.locator("#general-chat-link").isDisabled(), "An empty general draft cannot be linked as a persisted conversation");
    await edit("无项目第一条任务"); await send();
    check((await state()).runs.at(-1).projectDir === projects.general && (await state()).creates.length === beforeGeneral + 1, "General chat creates its first identity when a task is sent");
    check(await page.locator("#general-chat-link").isEnabled(), "A stored general conversation becomes linkable after first send");
    await open(a);
    await page.evaluate(async () => {
      const shell = await import("/03-shell.js");
      window.__empty.f.state.processes.find(item => item.id === shell.activeProcessId).running = true;
      window.__kzPreview.emit("kz:turn", { sessionId: shell.activeSessionId, step: 1, maxSteps: 0 });
      await (await import("/09-sessions.js")).refreshProcesses();
      (await import("/12-workbench.js")).renderProjectActivity();
    });
    const running = await indicator();
    check(running.some(row => row.state === "running" && !row.hidden && row.animation === "none"), "Running projects have a stable indicator without pulsing");
    check(await page.locator(".workbench-session-link[data-activity='running'] .workbench-session-activity").evaluateAll(dots => dots.length > 0 && dots.every(dot => getComputedStyle(dot).animationName === "none")), "Running task indicators are also static");
    for (const theme of ["dark", "light"]) {
      await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
      check((await indicator()).every(row => row.after === "none"), `${theme} sidebar has no second busy pseudo-element`);
      await page.screenshot({ path: path.join(output, `sidebar-${theme}.png`) });
    }
    check(errors.length === 0, "Browser reports no JavaScript runtime errors");
    await writeFile(path.join(output, "report.json"), JSON.stringify({ checks, errors, final: await state() }, null, 2));
    console.log(JSON.stringify({ passed: checks.length, output }));
  }
} finally { await browser.close(); await server.close(); }
