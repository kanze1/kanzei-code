// Real browser + production UI; deterministic IPC exercises non-project replies.
/* global window, document */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = path.resolve("output/playwright/chat-question");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
const checks = [], errors = [];
const check = (condition, label) => { assert(condition, label); checks.push(label); console.log(`PASS ${label}`); };
page.on("pageerror", error => errors.push(error.message));
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const emit = async question => {
  await page.evaluate(question => {
    window.__questions.rows = [question];
    window.__kzPreview.emit("kz:ask", { ...question, kind: "question" });
  }, question);
  await page.locator(".chat-question").waitFor();
};
try {
  await page.goto(`${server.origin}/?scene=chat&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  const owner = await page.evaluate(async () => {
    const shell = await import("/03-shell.js");
    const scope = await import("/03-general-scope.js");
    const owner = { projectDir: shell.currentProject, sessionId: shell.activeSessionId };
    scope.setGeneralChatRoot(owner.projectDir);
    window.__questions = { rows: [], answers: [], reject: false };
    window.__kzPreview.setCommand("softwire_questions", () => structuredClone(window.__questions.rows));
    window.__kzPreview.setCommand("softwire_answer_question", args => {
      window.__questions.answers.push(args);
      if (window.__questions.reject) throw "question_expired: 问题已更新";
      window.__questions.rows = window.__questions.rows.filter(question => question.id !== args.id);
    });
    return owner;
  });
  await page.locator("#prompt").fill("原对话的未发送草稿");
  const question = { ...owner, id: 9001, revision: "first", question: "确认第一阶段的沟通方案与模板方式吗？", options: [{ label: "继续制作", note: "采用当前方案" }, "调整方案"] };
  await emit(question);
  check(await page.locator("body").getAttribute("data-view") === "chat", "General question remains in the chat instead of navigating to a forbidden overview");
  check(await page.locator(".chat-question .sw-question").innerText() === question.question, "The complete question and its choices are visible");
  check(await page.locator("#prompt").inputValue() === "原对话的未发送草稿", "Presenting a question preserves the conversation draft");
  check(await page.evaluate(() => window.__questions.answers.length === 0), "No answer is invented when presenting the question");
  await page.screenshot({ path: path.join(output, "general-question-light.png") });
  await page.locator(".chat-question [data-reply-choice='继续制作']").click();
  await page.waitForFunction(() => window.__questions.answers.length === 1 && !document.querySelector(".chat-question"));
  let reply = await page.evaluate(() => window.__questions.answers.at(-1));
  check(reply.reply === "继续制作" && reply.projectDir === owner.projectDir && reply.sessionId === owner.sessionId && reply.expectedRevision === "first", "A selected option goes to the exact original question and revision");
  check(await page.locator("#prompt").inputValue() === "原对话的未发送草稿", "Answering keeps the unsent conversation draft");
  await emit({ ...question, id: 9002, revision: "text", question: "补充你的制作要求", options: [] });
  await page.locator(".chat-question-input").fill("  重点讲清楚并发协调\n保留完整例子  ");
  await page.evaluate(() => { window.__questions.reject = true; });
  await page.locator(".chat-question-send").click();
  await page.locator(".sw-interaction-refresh:visible").waitFor();
  check(await page.locator(".chat-question-input").inputValue() === "  重点讲清楚并发协调\n保留完整例子  ", "A failed reply preserves the complete answer for retry");
  const failedRequest = await page.evaluate(() => window.__questions.answers.at(-1).requestId);
  await page.evaluate(() => { window.__questions.reject = false; });
  await page.locator(".chat-question-send").click();
  await page.waitForFunction(() => !document.querySelector(".chat-question"));
  reply = await page.evaluate(() => window.__questions.answers.at(-1));
  check(reply.reply === "  重点讲清楚并发协调\n保留完整例子  " && reply.requestId === failedRequest, "Retry sends the exact free-text answer with the same request ID");
  await emit({ ...question, id: 9003, revision: "old", options: ["A", "B"], multiple: true });
  await page.locator(".chat-question [data-reply-choice='A']").click();
  check(await page.locator(".chat-question-input").inputValue() === "A", "Multiple choices wait for explicit Send");
  await page.locator(".chat-question [data-reply-choice='B']").click();
  check(await page.locator(".chat-question-input").inputValue() === "A\nB", "Multiple selected labels remain editable");
  await page.evaluate(() => { window.__questions.rows[0].revision = "new"; document.dispatchEvent(new CustomEvent("kz:refresh-work-questions")); });
  await page.waitForFunction(() => document.querySelector(".chat-question")?.dataset.questionKey.includes("new"));
  check(await page.locator(".chat-question-input").inputValue() === "A\nB", "Refreshing a changed revision preserves the pending answer draft");
  await page.locator(".chat-question .sw-back").click();
  await settle();
  await page.locator("#workbench-attention").click();
  await page.locator(".chat-question").waitFor();
  check(await page.locator("body").getAttribute("data-view") === "chat", "Needs attention reopens the general question without a project page");
  check(await page.locator(".chat-question-input").inputValue() === "A\nB", "Reopening restores the question answer draft");
  await page.locator(".chat-question-send").click();
  await page.waitForFunction(() => !document.querySelector(".chat-question"));
  check((await page.evaluate(() => window.__questions.answers.at(-1))).expectedRevision === "new", "The refreshed answer uses the displayed revision");
  await emit({ ...question, id: 9004, revision: "narrow", question: "窄屏长问题：" + "说明制作要求。".repeat(20) });
  await page.setViewportSize({ width: 420, height: 840 });
  await page.locator(".chat-question").scrollIntoViewIfNeeded();
  check(await page.locator(".chat-question").evaluate(element => element.scrollWidth <= element.clientWidth + 1), "A long question and reply controls fit the narrow conversation width");
  await page.screenshot({ path: path.join(output, "general-question-narrow.png") });
  check(!await page.locator("#toast").innerText().catch(() => "").then(text => text.includes("当前空间不提供")), "No workspace-unavailable error is shown");
  check(errors.length === 0, `No runtime errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "checks.json"), JSON.stringify({ checks, errors }, null, 2));
} catch (error) {
  await page.screenshot({ path: path.join(output, "failure.png") });
  throw error;
} finally { await browser.close(); await server.close(); }
