// Production UI with deterministic IPC/time; native question delivery has its own acceptance.
/* global window, document, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = path.resolve("output/playwright/reply-timer");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
page.setDefaultTimeout(12000);
const checks = [], errors = [];
page.on("pageerror", e => errors.push(e.message));
const check = (value, label) => { assert(value, label); checks.push(label); };
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const calls = () => page.evaluate(() => window.__kzPreview.calls.filter(c => c.cmd === "softwire_answer_question").map(c => c.args));
const elapsed = async () => {
  const text = (await page.locator("#status-elapsed").textContent()).replace(/^· /, "");
  return text.endsWith("s") ? Number.parseInt(text) : text.split(":").reduce((n, part) => n * 60 + Number(part), 0);
};
let identity;
const emit = async (event, data = {}) => {
  await page.evaluate(({ event, data, sid }) => window.__kzPreview.emit(event, { sessionId: sid, ...data }), { event, data, sid: identity.session });
  await settle();
};
const advance = async ms => page.evaluate(async ms => {
  window.__replyClock.offset += ms;
  (await import("/03-shell.js")).renderTurnElapsed();
}, ms);
const question = async (id, extra = {}) => {
  if (await page.locator(".sw-reply-complete").isVisible()) {
    await page.getByRole("button", { name: "返回工作", exact: true }).click(); await settle();
  }
  await page.evaluate(({ id, extra, identity }) => {
    const q = { id, projectDir: identity.project, sessionId: identity.session, revision: "v1",
      kind: "question", question: "工作消息 " + id, options: ["检查界面", "检查运行"], ...extra };
    window.__replyClock.questions = [q];
    window.__kzPreview.emit("kz:ask", q);
  }, { id, extra, identity });
  await settle();
  await page.locator(".sw-question").filter({ hasText: "工作消息 " + id }).waitFor({ state: "visible" });
  check(await page.getByRole("heading", { name: "回复提问", exact: true }).isVisible(), "Foreground question " + id + " opens its reply detail automatically");
};
try {
  await page.goto(server.origin + "/?scene=workspace&theme=dark");
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.locator(".workspace-card-open").first().click(); await settle();
  await page.locator('[data-work-surface="chat"]').click(); await settle();
  identity = await page.evaluate(async () => {
    const shell = await import("/03-shell.js");
    const f = await window.__kzPreview.fixtures(), snapshot = f.commands.workspace_snapshot();
    for (const project of snapshot.projects) { project.decisions = []; project.work_units = []; }
    window.__kzPreview.setCommand("workspace_snapshot", snapshot);

    const now = performance.now.bind(performance);
    window.__replyClock = { offset: 0, questions: [], hold: false, fail: false };
    Object.defineProperty(performance, "now", { configurable: true, value: () => now() + window.__replyClock.offset });
    window.__kzPreview.setCommand("softwire_questions", () => {
      const rows = structuredClone(window.__replyClock.questions);
      if (window.__replyClock.holdQuestions) {
        window.__replyClock.holdQuestions = false;
        return new Promise(resolve => { window.__replyClock.releaseQuestions = () => resolve(rows); });
      }
      return rows;
    });
    window.__kzPreview.setCommand("softwire_answer_question", args => {
      const done = () => {
        if (window.__replyClock.fail) throw new Error("回复暂未送达");
        window.__replyClock.questions = window.__replyClock.questions.filter(q => q.id !== args.id);
        window.__kzPreview.emit("kz:question-replied", args);
        return { ...args, status: "delivered" };
      };
      return window.__replyClock.hold ? new Promise(resolve => { window.__replyClock.release = () => resolve(done()); }) : done();
    });
    return { project: shell.currentProject, session: shell.activeSessionId };
  });
  check(Boolean(identity.session), "Timer has the actual active session identity");
  check(await page.locator("#status-elapsed").textContent() === "", "Opening the app does not fabricate elapsed time");
  await emit("kz:turn", { step: 1 }); await emit("kz:tool-start", { id: "q-call", name: "question" });
  await advance(65000);
  check((await page.locator("#turn-activity-elapsed").textContent()).startsWith("1:05"), "Active time is formatted as minutes and seconds");
  await question(301);
  check(await page.locator("#status-mode").textContent() === "待你回复", "A suspended question is waiting for the user, not executing");
  check(await page.locator("#status-elapsed").textContent() === "" && await page.locator("#turn-activity-elapsed").textContent() === "", "Both elapsed displays stop during human wait");
  await advance(13 * 3600000);
  await emit("kz:status", { stage: "执行", detail: "question" });
  check(await page.locator("#status-mode").textContent() === "待你回复" && await page.locator("#status-elapsed").textContent() === "", "Thirteen hours and a late progress event cannot count waiting as work");
  check(await page.locator("#status-dot").evaluate(el => getComputedStyle(el).animationName === "none"), "The waiting mark stays still");
  await page.evaluate(() => { window.__replyClock.hold = true; });
  const before = (await calls()).length;
  await page.getByRole("button", { name: "检查界面", exact: true }).evaluate(el => { el.click(); el.click(); });
  await page.waitForFunction(() => Boolean(window.__replyClock.release));
  check((await calls()).length === before + 1, "A double click sends one reply without pressing Send");
  check(await page.getByRole("button", { name: "检查运行", exact: true }).isDisabled(), "Other choices are disabled until delivery finishes");
  check(await page.locator("#status-elapsed").textContent() === "", "Sending a reply does not resume the clock before acknowledgement");
  await page.evaluate(() => { window.__replyClock.hold = false; window.__replyClock.release(); }); await settle();
  check((await calls()).at(-1).reply === "检查界面" && (await calls()).at(-1).sessionId === identity.session && (await calls()).at(-1).expectedRevision === "v1", "One-click reply preserves exact choice, owner and revision");
  await emit("kz:tool-end", { id: "q-call", name: "question", ok: true }); await advance(3000);
  check(await elapsed() >= 68 && await elapsed() < 75, "Reply resumes accumulated work time without adding thirteen hours");
  check(!await page.locator(".sw-reply-complete").isVisible(), "Successful quick reply leaves the completed item and advances automatically");
  await page.screenshot({ path: path.join(output, "quick-reply-dark.png") });
  await emit("kz:idle");
  check(await page.locator("#status-elapsed").textContent() === "", "Idle alone clears the timer even if Done is missing");
  await emit("kz:turn", { step: 1 }); await advance(1000);
  check(await elapsed() >= 1 && await elapsed() < 4, "The next run starts with a fresh clock");
  await question(302);
  await page.getByRole("button", { name: "补充说明", exact: true }).click();
  const notedBefore = (await calls()).length;
  await page.getByRole("button", { name: "检查运行", exact: true }).click();
  check((await calls()).length === notedBefore && await page.locator("#prompt").inputValue() === "检查运行", "Add a note stages a choice without prematurely sending");
  await page.locator("#prompt").fill("检查运行\n只检查现有环境，不安装依赖。");
  await page.getByRole("button", { name: "检查界面", exact: true }).click();
  check(await page.locator("#prompt").inputValue() === "检查界面\n只检查现有环境，不安装依赖。", "Changing a single choice replaces it while preserving the explanation");
  await page.locator("#prompt").fill("检查运行\n只检查现有环境，不安装依赖。");
  await page.evaluate(() => { window.__replyClock.holdQuestions = true; });
  await page.locator("#sw-refresh").click();
  await page.waitForFunction(() => Boolean(window.__replyClock.releaseQuestions));
  await page.locator("#send").click();
  await page.waitForFunction(() => !window.__replyClock.questions.some(question => question.id === 302));
  await page.evaluate(() => window.__replyClock.releaseQuestions()); await settle();
  check((await calls()).at(-1).reply === "检查运行\n只检查现有环境，不安装依赖。", "The manually sent explanation is delivered verbatim");
  check(await page.locator("#status-mode").textContent() !== "待你回复", "A delayed old question poll cannot undo the reply acknowledgement");
  await question(303, { multiple: true });
  const multiBefore = (await calls()).length;
  await page.getByRole("button", { name: "检查界面", exact: true }).click();
  await page.getByRole("button", { name: "检查运行", exact: true }).click();
  check((await calls()).length === multiBefore, "Explicit multi-select waits until all choices are ready");
  await page.getByRole("button", { name: "检查界面", exact: true }).click();
  check(await page.locator("#prompt").inputValue() === "检查运行", "A selected multi-choice can be removed");
  await page.locator("#send").click(); await settle();
  await question(304);
  await page.locator("#prompt").fill("先保留我的说明");
  const draftBefore = (await calls()).length;
  await page.getByRole("button", { name: "检查界面", exact: true }).click();
  check((await calls()).length === draftBefore && (await page.locator("#prompt").inputValue()).startsWith("先保留我的说明"), "An existing draft prevents instant send and is never overwritten");
  await page.locator("#send").click(); await settle();
  await question(305);
  await page.evaluate(() => { window.__replyClock.fail = true; });
  await page.getByRole("button", { name: "检查运行", exact: true }).click(); await settle();
  const failed = (await calls()).at(-1);
  check(await page.locator("#prompt").inputValue() === "检查运行" && (await page.locator(".sw-interaction-receipt").innerText()).includes("回复暂未送达"), "Failed instant delivery preserves the reply and exposes the error");
  check(await page.locator("#status-mode").textContent() === "待你回复", "Failure does not pretend that the question resumed");
  await page.evaluate(() => { window.__replyClock.fail = false; });
  await page.locator("#send").click(); await settle();
  check((await calls()).at(-1).requestId === failed.requestId, "Retry reuses the same receipt identity");
  await page.evaluate(async () => (await import("/03-shell.js")).applyTheme("light"));
  await question(306, { options: [{ label: "检查界面", note: "保留当前颜色，只检查状态和交互" }, "检查运行"] });
  check((await page.locator(".sw-choice-note").textContent()).includes("保留当前颜色"), "Option notes remain visible before choosing");
  await page.screenshot({ path: path.join(output, "question-light.png") });
  await emit("kz:stopped");
  check(await page.locator("#status-elapsed").textContent() === "" && await page.locator("#status-mode").textContent() !== "待你回复", "Stop clears a suspended question's activity");
  const scoped = await page.evaluate(async () => {
    const s = await import("/03-shell.js"), before = s.activeSessionId;
    s.setActiveSessionId("timer-background"); s.setRunning(true); s.startElapsed();
    window.__replyClock.offset += 9000; s.renderTurnElapsed();
    const other = document.querySelector("#status-elapsed").textContent;
    s.setActiveSessionId(before); s.setRunning(false);
    return { other, restored: document.querySelector("#status-elapsed").textContent };
  });
  check(scoped.other === "· 9s" && scoped.restored === "", "Switching sessions never carries the other session's timer");
  check(errors.length === 0, "No browser errors: " + errors.join("; "));
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, errors }, null, 2));
  console.log("Reply/timer browser PASS: " + checks.length + " checks");
} catch (error) {
  await page.screenshot({ path: path.join(output, "failure.png") });
  console.error({ checks, errors }); throw error;
} finally { await browser.close(); await server.close(); }
