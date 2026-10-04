// Real Windows WebView2, isolated conversations, durable questions and a local model.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, readFile, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Usage: node scripts/ui-discussion-handoff-native-smoke.mjs <fresh kzapp.exe>");
const output = path.resolve("output/playwright/discussion-handoff/native"), run = path.join(output, String(Date.now()));
const project = path.join(run, "project"), profile = path.join(run, "profile"), appHome = path.join(profile, ".kanzei");
for (const dir of [project, appHome, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(appHome, "app.json"), JSON.stringify({ projects: [], current: null, theme: "light" }));
await writeFile(path.join(project, "original.txt"), "original content");
const libraryScope = "NATIVE_SCOPE_LIBRARY：番剧、轻小说和 Galgame 使用统一作品库；个人评分必须与来源评分分开保存。";
const syncScope = "NATIVE_SCOPE_SYNC：Windows 是完整客户端，Android 是离线轻量客户端；自托管服务只允许邀请码注册，同步冲突不能静默覆盖。";
const handoffText = "按我们的讨论拆分登记需求";
const mainDraft = "主对话待续草稿 MAIN_SCOPE_DRAFT";
const discussionQuestion = "讨论需要确认：小说卷级收藏要独立记录进度吗？";
const discussionReply = "每卷独立记录（NATIVE_DISCUSSION_REPLY）";
const discussionDraft = "讨论中待补充的卷级规则 DISCUSSION_SCOPE_DRAFT";
const foregroundQuestion = "番剧整理默认登记需要先支持哪个客户端？";
const foregroundReply = "Windows 优先（NATIVE_MAIN_REPLY）";
const backgroundQuestion = "后台核对：是否保留手动导出的备份？";
const backgroundReply = "保留备份（NATIVE_BACKGROUND_REPLY）";
const backgroundDraft = "暂存中的主对话草稿 BACKGROUND_SCOPE_DRAFT";
const checks = [], requests = [], errors = [], allowedFixtureOperations = [], capturedEvents = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
let foregroundAsked = false, backgroundAsked = false, discussionAsked = false, releaseBackground;

function answer(res, content, tool = null) {
  if (res.destroyed || res.writableEnded) return;
  const delta = tool ? { tool_calls: [{ index: 0, id: `native-handoff-${requests.length}`, type: "function", function: { name: "question", arguments: JSON.stringify(tool) } }] } : { content };
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
    const prompt = JSON.stringify(messages.findLast(message => message.role === "user")?.content || "");
    const kind = system.includes("You capture ONE requirement") ? "unexpected-capture"
      : system.includes("You own one delegated task") || prompt.includes("你是只读勘察代理") || prompt.includes("你是只读复核代理") ? "unexpected-child"
        : prompt.includes("用户回答异步问题") ? "answer"
          : prompt.includes("NATIVE_BACKGROUND_START") ? "background-main"
            : prompt.includes("用户明确交给其它对话的结论") && prompt.includes(handoffText) ? "handoff-main"
              : prompt.includes("NATIVE_DISCUSSION_QUESTION") ? "discussion-question"
                : prompt.includes("NATIVE_SCOPE_SYNC") ? "discussion-sync"
                  : prompt.includes("NATIVE_SCOPE_LIBRARY") ? "discussion-library" : "other";
    requests.push({ kind, messages, model: data.model });
    if (kind === "discussion-library") answer(res, "已确认作品库范围和评分边界 DISCUSSION_LIBRARY_ACK。");
    else if (kind === "discussion-sync") answer(res, "已确认客户端、邀请制和冲突边界 DISCUSSION_SYNC_ACK。");
    else if (kind === "discussion-question" && !discussionAsked) {
      discussionAsked = true;
      answer(res, null, { question: discussionQuestion, options: [discussionReply, "只记录系列总进度"] });
    }
    else if (kind === "handoff-main" && !foregroundAsked) {
      foregroundAsked = true;
      // Deliberately omit background: ordinary questions must automatically appear.
      answer(res, null, { question: foregroundQuestion, options: [foregroundReply, "Android 优先"] });
    } else if (kind === "background-main" && !backgroundAsked) {
      backgroundAsked = true;
      // Hold arrival until the real composer has focus and an unsent draft.
      releaseBackground = () => answer(res, null, { question: backgroundQuestion, options: [backgroundReply, "暂不保留"], background: true });
    } else if (["answer", "handoff-main", "background-main", "discussion-question"].includes(kind)) answer(res, "已收到原会话的答复，测试范围确认完成。");
    else answer(res, "原生验收没有安排登记模型、子任务或实施。");
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
let browser, page, owner, discussion;
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
const conversation = processId => invoke("conversation_display_get", { projectDir: project, processId });
const pending = async label => (await invoke("softwire_questions")).find(question => question.question === label);
const receivedReply = (messages, label) => messages.some(message => ["user", "tool"].includes(message.role) && JSON.stringify(message.content ?? message.parts ?? []).includes(label));
const messageText = message => typeof message?.content === "string" ? message.content : (message?.content || []).map(part => part.text || "").join("\n");
async function until(fn, label) {
  const end = Date.now() + 45000;
  while (Date.now() < end) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label} timed out`);
}
const idle = processId => until(async () => {
  // Approve only permissions of this disposable fixture; questions remain interactive.
  if (await page.locator("#ask-allow").isVisible()) {
    allowedFixtureOperations.push({ action: await page.locator("#ask-action").textContent(), resource: await page.locator("#ask-resource").textContent() });
    await page.locator("#ask-allow").click();
  }
  return !(await invoke("process_list", { projectDir: project })).find(line => line.id === processId)?.running;
}, "Conversation idle");
const selected = () => page.evaluate(async () => {
  const shell = await import("./03-shell.js");
  return { project: shell.currentProject, processId: shell.activeProcessId, sessionId: shell.activeSessionId, view: document.body.dataset.view };
});
async function openMainChat() {
  const back = page.locator(".sw-reply-complete").getByRole("button", { name: "返回工作", exact: true });
  if (await back.isVisible()) await back.click();
  await page.locator('#project-work-switch [data-work-surface="chat"]').click();
  await until(async () => { const active = await selected(); return active.view === "chat" && active.processId === owner.id; }, "Main chat selection");
}
async function answerChoice(label) {
  await page.locator(".sw-choices").getByRole("button", { name: label, exact: true }).click();
  await page.locator(".sw-reply-complete").waitFor({ state: "visible" });
}
const listenEvents = () => page.evaluate(async () => {
  window.__handoffEvents = [];
  for (const name of ["kz:tool-start", "kz:tool-end", "kz:work-question", "kz:ask", "kz:error", "kz:done"]) await window.__TAURI__.event.listen(name, event => window.__handoffEvents.push({ name, payload: event.payload }));
});

try {
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1000 }); return true; } catch { return false; } }, "WebView2 start");
  page = browser.contexts()[0].pages()[0]; page.setDefaultTimeout(15000); page.on("pageerror", error => errors.push(error.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true");
  check((await invoke("projects_get")).projects.length === 0, "Native handoff uses only the disposable app profile");
  await invoke("projects_init", { path: project, name: "番剧讨论交接与提问验收" });
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.evaluate(async () => { const prefs = await window.__TAURI__.core.invoke("projects_get"); await (await import("./09-sessions.js")).enterProject(prefs); });
  owner = (await invoke("process_list", { projectDir: project })).find(line => !["readonly", "research"].includes(line.profile));
  assert(owner, "Main executor exists");
  await invoke("process_update", { processId: owner.id, subagentMode: "off", });
  await invoke("auto_state_update", { sessionId: owner.session_id, enabled: false });
  await listenEvents();
  await page.locator("#prompt").fill(mainDraft);
  await page.locator("#new-chat").click();
  await until(async () => { discussion = (await invoke("process_list", { projectDir: project })).find(line => line.id !== owner.id); return discussion && (await selected()).processId === discussion.id; }, "UI-created discussion");
  await invoke("auto_state_update", { sessionId: discussion.session_id, enabled: false });
  check(discussion.session_id !== owner.session_id && discussion.profile === "dev", "New conversation has an ordinary development identity");
  // This scenario exercises an explicitly read-only discussion, not a secondary rank.
  await invoke("process_update", { processId: discussion.id, profile: "readonly" });
  await page.evaluate(async id => { const sessions = await import("./09-sessions.js"); await sessions.refreshProcesses(); await sessions.switchProcess(id, true); }, discussion.id);
  check(await page.locator("#prompt").inputValue() === "", "Creating a discussion preserves the main draft and starts a separate composer");
  for (const [scope, kind] of [[libraryScope, "discussion-library"], [syncScope, "discussion-sync"]]) {
    await page.locator("#prompt").fill(scope); await page.locator("#send").click();
    await until(() => requests.some(request => request.kind === kind), "Discussion model request");
    await idle(discussion.id);
  }
  const sourceHistory = JSON.stringify(await conversation(discussion.id));
  check(sourceHistory.includes(libraryScope) && sourceHistory.includes(syncScope) && sourceHistory.includes("DISCUSSION_LIBRARY_ACK") && sourceHistory.includes("DISCUSSION_SYNC_ACK"), "Both actual discussion turns and assistant conclusions are persisted before handoff");
  check(!JSON.stringify(await conversation(owner.id)).includes("NATIVE_SCOPE_LIBRARY"), "Discussion scope is absent from the main conversation before explicit handoff");
  await page.locator("#prompt").fill(discussionDraft);
  await invoke("run_prompt", { projectDir: project, processId: discussion.id, profile: "readonly", agent: "readonly", prompt: "NATIVE_DISCUSSION_QUESTION 确认小说卷级收藏边界。", autonomous: false });
  const discussionShown = page.locator(".sw-question").filter({ hasText: discussionQuestion });
  await discussionShown.waitFor({ state: "visible" });
  const discussionPending = await pending(discussionQuestion), discussionActive = await selected();
  check(discussionPending && !discussionPending.background && discussionPending.sessionId === discussion.session_id, "An ordinary question asked by the real read-only discussion retains that discussion's durable identity");
  check(discussionActive.processId === discussion.id && discussionActive.sessionId === discussion.session_id, "The discussion question automatically appears while preserving the active discussion recipient");
  check(await page.locator(".sw-choices").getByRole("button", { name: discussionReply, exact: true }).isVisible(), "Automatically presented discussion question exposes its actual reply choices");
  await page.screenshot({ path: path.join(output, "discussion-foreground-question.png") });
  await page.locator(".sw-detail-head .sw-back").click();
  await until(async () => { const active = await selected(); return active.view === "chat" && active.sessionId === discussion.session_id; }, "Collapse question to original discussion");
  check(await page.locator("#prompt").inputValue() === discussionDraft, "Collapsing the unanswered discussion question restores its original discussion draft");
  check((await pending(discussionQuestion))?.id === discussionPending.id && !await discussionShown.isVisible(), "Collapsing a discussion question leaves the same durable question pending without immediately reopening it");
  await page.locator("#workbench-attention").click();
  await page.locator(".sw-inbox-row").filter({ hasText: discussionQuestion }).click();
  await discussionShown.waitFor({ state: "visible" });
  check((await pending(discussionQuestion))?.revision === discussionPending.revision, "Needs attention reopens the same discussion question revision after it was collapsed");
  await answerChoice(discussionReply);
  await until(() => requests.some(request => ["answer", "discussion-question"].includes(request.kind) && receivedReply(request.messages, discussionReply)), "Discussion model receives its own reply");
  await idle(discussion.id);
  check(!await pending(discussionQuestion), "Answering the automatically shown discussion question clears its own durable record");
  check(receivedReply(await conversation(discussion.id), discussionReply) && !receivedReply(await conversation(owner.id), discussionReply), "Discussion reply is committed only to the original discussion and never to the main conversation");
  await page.locator(".sw-reply-complete").getByRole("button", { name: "返回工作", exact: true }).click();
  await until(async () => { const active = await selected(); return active.view === "chat" && active.processId === discussion.id && active.sessionId === discussion.session_id; }, "Return to original discussion");
  check(await page.locator("#prompt").inputValue() === discussionDraft, "Returning from the answered discussion question restores that discussion's unsent draft");
  check(await page.locator("#project-handoff").isVisible(), "After the question reply the original discussion remains available for handoff");
  await page.screenshot({ path: path.join(output, "discussion-returned-draft.png") });
  await page.locator("#prompt").fill(handoffText);
  await page.locator("#project-handoff").click();
  const form = page.locator(".project-handoff-form"), conclusion = form.getByRole("textbox", { name: "交给其它对话的结论", exact: true });
  check(await conclusion.inputValue() === handoffText, "Native handoff form contains only the user's short instruction");
  await page.screenshot({ path: path.join(output, "discussion-handoff.png") });
  await form.getByRole("button", { name: "发送给对话", exact: true }).click();
  await until(() => requests.some(request => request.kind === "handoff-main"), "Actual main model handoff request");
  const handed = requests.find(request => request.kind === "handoff-main");
  const handedPrompt = messageText(handed.messages.findLast(message => message.role === "user"));
  check(handedPrompt.includes(handoffText) && handedPrompt.includes(libraryScope) && handedPrompt.includes(syncScope), "Actual main model receives both business scopes when only the short handoff instruction was entered");
  check(handedPrompt.includes(discussion.session_id) && handedPrompt.includes(discussion.id) && handedPrompt.includes("来源项目") && handedPrompt.includes("来源会话"), "The forwarded context names the exact source project, process and durable session");
  check(handedPrompt.includes("DISCUSSION_LIBRARY_ACK") && handedPrompt.includes("DISCUSSION_SYNC_ACK"), "The source snapshot includes persisted assistant conclusions as well as original user turns");
  const shown = page.locator(".sw-question").filter({ hasText: foregroundQuestion });
  await shown.waitFor({ state: "visible" });
  const firstPending = await pending(foregroundQuestion), active = await selected();
  check(firstPending && !firstPending.background && firstPending.sessionId === owner.session_id, "A question with omitted background is persisted as a foreground question for the original main session");
  check(active.processId === owner.id && active.sessionId === owner.session_id, "The default question automatically opens in the original main conversation without an inbox click");
  check(await page.locator(".sw-choices").getByRole("button", { name: foregroundReply, exact: true }).isVisible(), "The automatically shown question exposes its actual reply options");
  check(!await page.locator("#ask-overlay").isVisible(), "The question reply surface is independent from permission approval controls");
  await page.screenshot({ path: path.join(output, "foreground-question.png") });
  capturedEvents.push(...await page.evaluate(() => window.__handoffEvents || []));
  await page.reload(); await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await listenEvents();
  await shown.waitFor({ state: "visible" });
  const recovered = await pending(foregroundQuestion);
  check(recovered?.id === firstPending.id && recovered?.revision === firstPending.revision, "Reload restores the same unanswered foreground question and revision");
  check((await selected()).sessionId === owner.session_id, "Restored question presentation retains the original recipient");
  await page.screenshot({ path: path.join(output, "foreground-question-reloaded.png") });
  await answerChoice(foregroundReply);
  await until(() => requests.some(request => ["answer", "handoff-main"].includes(request.kind) && receivedReply(request.messages, foregroundReply)), "Main model receives foreground reply");
  await idle(owner.id);
  check(!await pending(foregroundQuestion), "Answering the visible default question clears its durable pending record");
  check(receivedReply(await conversation(owner.id), foregroundReply), "The chosen answer is committed to the original main conversation");
  check(!JSON.stringify(await conversation(discussion.id)).includes("NATIVE_MAIN_REPLY"), "The main answer does not leak into the source discussion");
  await openMainChat();
  check(await page.locator("#prompt").inputValue() === mainDraft, "Automatic question presentation and reload preserve the original unsent main draft");
  await invoke("run_prompt", { projectDir: project, processId: owner.id, profile: "dev", agent: "dev-pair", prompt: "NATIVE_BACKGROUND_START 验证后台提问保留草稿。", autonomous: false });
  await until(() => Boolean(releaseBackground), "Held background request");
  await page.locator("#prompt").fill(backgroundDraft); await page.locator("#prompt").focus();
  releaseBackground();
  await until(async () => Boolean(await pending(backgroundQuestion)), "Durable background question");
  await idle(owner.id);
  await until(async () => (await page.locator("#workbench-attention-count").textContent()).trim() === "1", "Background attention badge");
  check((await pending(backgroundQuestion)).background, "Explicit background true remains a durable background question");
  check((await selected()).view === "chat" && !await page.locator(".sw-question").filter({ hasText: backgroundQuestion }).isVisible(), "Explicit background question updates Needs attention without opening a reply surface");
  check(await page.locator("#prompt").inputValue() === backgroundDraft && await page.locator("#prompt").evaluate(element => document.activeElement === element), "Background question preserves the active draft and composer focus");
  await page.screenshot({ path: path.join(output, "background-question-no-popup.png") });
  await page.locator("#workbench-attention").click();
  await page.locator(".sw-inbox-row").filter({ hasText: backgroundQuestion }).click();
  await page.locator(".sw-question").filter({ hasText: backgroundQuestion }).waitFor({ state: "visible" });
  await answerChoice(backgroundReply);
  await until(() => requests.some(request => ["answer", "background-main"].includes(request.kind) && receivedReply(request.messages, backgroundReply)), "Main model receives background reply");
  await idle(owner.id);
  check(!await pending(backgroundQuestion), "An explicitly opened background question can be answered and cleared normally");
  check(receivedReply(await conversation(owner.id), backgroundReply) && !JSON.stringify(await conversation(discussion.id)).includes("NATIVE_BACKGROUND_REPLY"), "Background answer also resumes only its original main conversation");
  await openMainChat();
  check(await page.locator("#prompt").inputValue() === backgroundDraft, "Replying through Needs attention preserves the separate main composer draft");
  check(!requests.some(request => ["unexpected-child", "unexpected-capture"].includes(request.kind)), "Handoff and question acceptance do not dispatch an unrelated capture model or child task");
  check(await readFile(path.join(project, "original.txt"), "utf8") === "original content", "The native regression leaves the implementation fixture unchanged");
  const events = [...capturedEvents, ...await page.evaluate(() => window.__handoffEvents || [])];
  check(!events.some(event => event.name === "kz:error"), "Native handoff and question runs finish without execution errors");
  check(errors.length === 0, `No native page or local model errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, errors, executable: exe, project, owner, discussion,
    modelRequests: requests.map(request => request.kind), allowedFixtureOperations,
    boundary: "Actual Windows WebView2, Rust handoff IPC, persisted conversation snapshots, durable questions and model request bodies; isolated profile and project using a deterministic local model. No real user session or external provider was accessed." }, null, 2));
  await writeFile(path.join(run, "model-requests.json"), JSON.stringify(requests, null, 2));
  console.log(`Native discussion handoff PASS: ${checks.length} checks, ${requests.length} local model requests`);
} catch (error) {
  await writeFile(path.join(run, "failure.json"), JSON.stringify({ error: String(error), checks, errors, requests }, null, 2));
  if (page) { await page.screenshot({ path: path.join(output, "failure.png") }).catch(() => {}); console.error(await page.evaluate(() => ({ text: document.body.innerText.slice(-3000), events: window.__handoffEvents?.slice(-10) })).catch(() => null)); }
  console.error({ checks, requests: requests.map(request => request.kind), errors }); throw error;
} finally {
  if (page) await invoke("runtime_shutdown").catch(() => {});
  await browser?.close(); app.kill(); model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
}
