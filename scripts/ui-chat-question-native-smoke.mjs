// Actual WebView2 + Rust question suspension/resumption, with an isolated home
// and deterministic local model. No installed profile or external model calls.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Usage: node scripts/ui-chat-question-native-smoke.mjs <fresh kzapp.exe>");
const output = path.resolve("output/playwright/chat-question/native"), run = path.join(output, String(Date.now()));
const profile = path.join(run, "profile"), home = path.join(profile, ".kanzei"), exe = path.join(run, "kzapp.exe");
for (const dir of [home, path.join(run, "webview"), path.join(profile, "AppData/Local"), path.join(profile, "AppData/Roaming")]) await mkdir(dir, { recursive: true });
await writeFile(path.join(home, "app.json"), JSON.stringify({ projects: [], current: null, theme: "light" }));
await copyFile(path.resolve(process.argv[2]), exe);
const checks = [], errors = [], requests = [];
const check = (value, label) => { assert(value, label); checks.push(label); console.log(`PASS ${label}`); };
function answer(res, content, tool) {
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  const delta = tool ? { tool_calls: [{ index: 0, id: "native-general-question", type: "function", function: { name: "question", arguments: JSON.stringify(tool) } }] } : { content };
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
  res.write(`data: ${JSON.stringify({ choices: [{ index: 0, delta: {}, finish_reason: tool ? "tool_calls" : "stop" }], usage: { prompt_tokens: 100, completion_tokens: 20 } })}\n\n`);
  res.end("data: [DONE]\n\n");
}
const model = http.createServer(async (req, res) => {
  if (req.url?.endsWith("/models")) { res.writeHead(200, { "Content-Type": "application/json" }); res.end(JSON.stringify({ data: [{ id: "test" }] })); return; }
  let body = ""; for await (const chunk of req) body += chunk;
  const payload = JSON.parse(body || "{}"); requests.push(payload);
  const userIndex = payload.messages.findLastIndex(message => message.role === "user");
  const user = JSON.stringify(payload.messages[userIndex]?.content);
  if (payload.messages.slice(userIndex + 1).some(message => message.role === "tool")) answer(res, "NATIVE_QUESTION_RESUMED：收到回答，继续制作。");
  else answer(res, null, { question: user.includes("FREE_TEXT") ? "请补充制作要求" : "确认第一阶段的沟通方案与模板方式吗？", options: user.includes("FREE_TEXT") ? [] : [{ label: "继续制作", note: "采用当前方案" }, "修改方案"] });
});
await new Promise(resolve => model.listen(0, "127.0.0.1", resolve));
await writeFile(path.join(home, "kanzei.toml"), `[models]\nprimary = "stub:test"\nfast = "stub:test"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n`);
const probe = net.createServer(); await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
const port = probe.address().port; await new Promise(resolve => probe.close(resolve));
const app = spawn(exe, [], { cwd: run, windowsHide: true, stdio: "ignore", env: { ...process.env,
  KANZEI_HOME: home, USERPROFILE: profile, HOME: profile, LOCALAPPDATA: path.join(profile, "AppData/Local"),
  APPDATA: path.join(profile, "AppData/Roaming"), WEBVIEW2_USER_DATA_FOLDER: path.join(run, "webview"), KANZEI_E2E_CDP: String(port),
} });
let browser, page;
async function until(fn, label, timeout = 45000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await fn()) return; await new Promise(resolve => setTimeout(resolve, 100)); }
  throw new Error(`${label} timed out`);
}
const invoke = (command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI__.core.invoke(command, args), { command, args });
try {
  await until(async () => { try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1500 }); return true; } catch { return false; } }, "WebView2 startup");
  page = browser.contexts()[0].pages()[0];
  page.setDefaultTimeout(12000); page.on("pageerror", error => errors.push(error.message));
  await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.locator("#workbench-general-chat").click();
  await page.waitForFunction(() => document.body.dataset.generalChat === "true" && !document.getElementById("workbench-general-chat").hasAttribute("aria-busy"));
  const root = await invoke("general_chat_open");
  const owner = (await invoke("process_list", { projectDir: root }))[0];
  check((await invoke("projects_get")).projects.length === 0, "The native test has no project and uses an isolated user profile");
  await page.locator("#auto-allow").check();
  const ask = async marker => {
    await page.locator("#prompt").fill(marker); await page.locator("#send").click();
    await page.locator(".chat-question").waitFor();
    await page.waitForFunction(() => document.getElementById("status-mode").textContent.includes("待你回复"));
    return (await invoke("softwire_questions"))[0];
  };
  const question = await ask("NATIVE_GENERAL_CHOICE");
  check(question.projectDir === root && question.sessionId === owner.session_id, "The backend persists the waiting question in its original general conversation");
  check(await page.locator("body").getAttribute("data-view") === "chat", "A real general question displays in chat without the workspace-unavailable route");
  await page.locator("#prompt").fill("保留未发出的对话草稿");
  await page.screenshot({ path: path.join(output, "native-general-question.png") });
  await page.locator(".chat-question [data-reply-choice='继续制作']").click();
  await until(async () => (await invoke("softwire_questions")).length === 0 && !(await invoke("process_list", { projectDir: root })).some(process => process.running), "Original question resumes");
  check(requests.some(request => request.messages.some(message => message.role === "tool" && message.tool_call_id === "native-general-question" && JSON.stringify(message.content).includes("继续制作"))), "The actual model receives the selected answer as its original question tool result");
  check((await page.locator("#messages").innerText()).includes("NATIVE_QUESTION_RESUMED"), "The original task continues and displays its model response");
  check(await page.locator("#prompt").inputValue() === "保留未发出的对话草稿", "Native question submission preserves the conversation draft");
  await ask("NATIVE_GENERAL_FREE_TEXT");
  await page.locator(".chat-question-input").fill("  请保留完整并发例子\n使用中文  ");
  await page.locator(".chat-question-send").click();
  await until(async () => (await invoke("softwire_questions")).length === 0 && !(await invoke("process_list", { projectDir: root })).some(process => process.running), "Free-text task resumes");
  check(requests.some(request => request.messages.some(message => message.role === "tool" && JSON.stringify(message.content).includes("请保留完整并发例子"))), "A free-text question without options also resumes the real task");
  await ask("NATIVE_GENERAL_RELOAD");
  await page.locator(".chat-question-input").fill("切换对话时保留这份回复");
  await page.locator("#new-chat").click();
  await until(async () => (await invoke("process_list", { projectDir: root })).length === 2, "New independent general chat");
  await page.waitForFunction(() => !document.getElementById("new-chat").hasAttribute("aria-busy"));
  check(await page.locator(".chat-question:visible").count() === 0, "A pending question does not appear in a different general conversation");
  await page.locator("#workbench-attention").click();
  await page.locator(".chat-question").waitFor();
  check(await page.evaluate(async id => (await import("./03-shell.js")).activeProcessId === id, owner.id), "Needs attention switches to the original question owner before opening the answer");
  check(await page.locator(".chat-question-input").inputValue() === "切换对话时保留这份回复", "Switching away and back preserves the pending answer draft");
  await page.reload();
  await page.waitForFunction(() => document.body.dataset.appReady === "true");
  await page.locator(".chat-question").waitFor();
  check((await invoke("softwire_questions"))[0].sessionId === owner.session_id, "Reload restores the persisted waiting question without a new live ask event");
  await page.locator(".chat-question .sw-back").click();
  await page.locator("#workbench-attention").click();
  await page.locator(".chat-question").waitFor();
  check(await page.locator("body").getAttribute("data-view") === "chat", "Native Needs attention reopens a general question in the original chat");
  await page.locator(".chat-question [data-reply-choice='继续制作']").click();
  await until(async () => (await invoke("softwire_questions")).length === 0, "Restored question answers");
  check(errors.length === 0, `No native UI errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "checks.json"), JSON.stringify({ checks, errors, requestCount: requests.length }, null, 2));
} catch (error) {
  await page?.screenshot({ path: path.join(output, "failure.png") }).catch(() => {});
  throw error;
} finally {
  await invoke("runtime_shutdown").catch(() => {});
  await browser?.close();
  if (app.exitCode === null) { app.kill(); await new Promise(resolve => app.once("exit", resolve)); }
  await new Promise(resolve => model.close(resolve));
}
