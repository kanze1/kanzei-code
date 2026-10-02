/* global window, document */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
const output = "output/playwright/agent-team";
await mkdir(output, { recursive: true });
const checks = [], errors = [];
const check = (condition, message) => { assert(condition, message); checks.push(message); };
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const calls = action => page.evaluate(action => window.__kzPreview.calls.filter(c => c.cmd === "agent_team_command" && c.args.input.action === action).map(c => c.args), action);
const update = patch => page.evaluate(patch => {
  Object.assign(window.__teamTest.job, patch, { updated_at: window.__teamTest.job.updated_at + 1 });
  window.__kzPreview.emit("kz:agent-job", { sessionId: window.__teamTest.job.owner, job: structuredClone(window.__teamTest.job) });
}, patch);
page.on("pageerror", e => errors.push(e.message));
page.setDefaultTimeout(10000);
try {
  await page.goto(`${server.origin}/?scene=chat&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready); await settle();
  const owner = await page.evaluate(async () => {
    const shell = await import("/03-shell.js");
    const job = { id: "managed-writer", owner: shell.activeSessionId, process_id: shell.activeProcessId, project_dir: shell.currentProject,
      name: "实现导出功能", role: "implement", model: "primary-test", model_tier: "primary", prompt: "实现并检查导出功能", state: "running", outcome: "pending",
      latest: "正在检查导出路径", result: "", worktree: "C:/trial/child", base: "base", head: null, files: [], depends_on: [],
      created_at: Date.now(), updated_at: Date.now(), messages: [{ id: "initial", from: "main", text: "实现并检查导出功能", state: "received" }], trace: [], trace_seq: 0 };
    window.__teamTest = { job, failure: false, held: null };
    window.__kzPreview.setCommand("agent_team_command", args => {
      const test = window.__teamTest;
      if (args.input.action === "list") return [structuredClone(test.job)];
      if (args.input.action === "get") return { job: structuredClone(test.job) };
      if (args.input.action === "diff") return { diff: "diff --git a/export.js b/export.js\n+export function save() {}" };
      if (args.input.action === "message") {
        if (test.failure) throw "发送失败，请重试";
        if (test.hold) return new Promise(resolve => { test.held = resolve; });
        return { state: "queued" };
      }
      if (args.input.action === "adopt") return { ...test.job, outcome: "adopted", updated_at: test.job.updated_at + 1 };
      return { state: "stopping" };
    });
    (await import("/06-agent-panel.js")).tasksPanelUserRun(job.owner);
    return { id: job.process_id, session_id: job.owner, project: job.project_dir };
  });
  await page.locator("#prompt").fill("主对话草稿"); await page.locator("#prompt").focus();
  await update({}); await settle();
  check(await page.locator("#tasks-panel").isVisible(), "Real job events open the agent panel");
  check(await page.locator("#prompt").evaluate(el => el === document.activeElement), "Agent start does not steal the draft focus");
  const card = page.locator('.tp-card[data-single="true"]').filter({ hasText: "实现导出功能" });
  check(!await card.locator('.tp-phase-head').isVisible(), "Single child removes the redundant delegation level");
  check((await card.locator('.tp-agent-latest').innerText()).includes("正在检查导出路径"), "Latest child activity is visible directly in the list");
  await page.screenshot({ path: `${output}/simplified-list-dark.png` });
  await page.locator('.tp-agent-row').filter({ hasText: "实现导出功能" }).click();
  check(await page.locator(".team-reply-input").isVisible(), "Clicking a child opens its own reply controls");
  check((await page.locator(".team-recipient").innerText()).includes("实现导出功能"), "Reply names the exact recipient");
  await page.evaluate(sid => window.__kzPreview.emit("kz:tool-end", { sessionId: sid, id: "managed-writer", name: "task", ok: true, preview: "已派发" }), owner.session_id);
  check(await page.locator("#agent-detail").getAttribute("data-sa-state") === "running", "Dispatch acknowledgment cannot finish a worker");
  await page.evaluate(sid => window.__kzPreview.emit("kz:idle", { sessionId: sid }), owner.session_id);
  check(await page.locator("#agent-detail").getAttribute("data-sa-state") === "running", "Parent idle does not erase active background jobs");
  await page.evaluate(() => {
    const other = { ...window.__teamTest.job, owner: "background-owner", id: "other-child", name: "其他项目任务" };
    window.__kzPreview.emit("kz:agent-job", { sessionId: other.owner, job: other });
    window.__kzPreview.emit("kz:agent-job", { sessionId: "wrong-owner", job: { ...window.__teamTest.job, state: "failed" } });
  });
  check((await page.locator(".team-recipient").innerText()).includes("实现导出功能"), "Background events never change the visible recipient");
  check(await page.locator("#agent-detail").getAttribute("data-sa-state") === "running", "Mismatched session envelopes are rejected");
  await page.evaluate(() => { window.__teamTest.failure = true; });
  await page.locator(".team-reply-input").fill("先补充导出测试"); await page.locator(".team-reply-input").press("Enter"); await settle();
  check(await page.locator(".team-reply-input").inputValue() === "先补充导出测试", "Failed reply retains its draft");
  check((await page.locator(".team-send-status").innerText()).includes("发送失败"), "Failed reply has an actionable receipt");
  await page.evaluate(() => { window.__teamTest.failure = false; window.__teamTest.hold = true; });
  const before = (await calls("message")).length;
  await page.locator(".team-reply-input").press("Enter"); await page.locator(".team-reply-input").press("Enter");
  check((await calls("message")).length === before + 1, "Repeated Enter while sending cannot duplicate a message");
  const sent = (await calls("message")).at(-1);
  check(sent.projectDir === owner.project && sent.processId === owner.id && sent.input.id === "managed-writer", "Reply preserves project, session and child identity");
  await page.waitForFunction(() => typeof window.__teamTest.held === "function");
  await page.evaluate(() => { window.__teamTest.held({ state: "queued", message_id: "reply-1" }); window.__teamTest.hold = false; }); await settle();
  check(await page.locator(".team-reply-input").inputValue() === "", "Accepted reply clears only its own draft");
  check(await page.locator("#prompt").inputValue() === "主对话草稿", "Child reply leaves the main draft untouched");
  await update({ messages: [{ id: "initial", from: "main", text: "实现并检查导出功能", state: "processed" }, { id: "reply-1", from: "main", text: "先补充导出测试", state: "processed" }] });
  check(await page.locator(".team-send-status").innerText() === "已处理", "Reply receipt advances instead of retaining a stale waiting label");
  await update({ state: "waiting", latest: "等待实现任务", depends_on: ["dependency"] });
  check((await page.locator(".sa-detail-state").innerText()).includes("等待依赖"), "Dependency wait differs from waiting for the user");
  await update({ state: "waiting_user", latest: "等待你的答复" });
  check((await page.locator(".sa-detail-state").innerText()).includes("等待答复"), "Human reply has its own waiting state");
  await page.evaluate(sid => {
    document.getElementById("auto-allow").checked = false;
    window.__kzPreview.emit("kz:ask", { sessionId: sid, id: 9901, agentId: "managed-writer", source: "实现导出功能", kind: "permission", action: "bash", resource: "echo local check" });
  }, owner.session_id);
  check((await page.locator("#ask-source").innerText()).includes("实现导出功能"), "Permission names its actual child recipient");
  await page.locator("#ask-source").click();
  check((await page.locator(".team-recipient").innerText()).includes("实现导出功能"), "Permission source opens the exact child");
  await page.locator("#ask-reopen").click(); await page.locator("#ask-allow").click(); await settle();
  await update({ state: "running", depends_on: [] });
  await page.locator(".sa-detail-stop").click(); await settle();
  check((await calls("stop")).at(-1).input.id === "managed-writer", "Stop targets only the selected child");
  await update({ state: "done", result: "已实现导出并通过定向检查", outcome: "candidate", head: "candidate", files: ["export.js"], latest: "等待整合" });
  check(await page.getByRole("button", { name: "采纳改动", exact: true }).isDisabled(), "Candidate must be inspected before UI adoption");
  check(await page.getByRole("button", { name: "重新派发", exact: true }).isEnabled(), "Completed child exposes a distinct fresh-dispatch action");
  await page.getByRole("button", { name: "重新派发", exact: true }).click(); await settle();
  check((await calls("restart")).at(-1).input.id === "managed-writer", "Fresh dispatch keeps the original task identity as its source");
  check((await page.locator(".team-send-status").innerText()).includes("已派发新任务"), "Fresh dispatch feedback is not overwritten by an earlier message receipt");
  await page.getByRole("button", { name: "查看改动", exact: true }).click();
  await page.waitForFunction(() => document.querySelector(".team-diff")?.textContent.includes("export function"));
  check((await page.locator(".team-diff").innerText()).includes("export function"), "Diff shows the actual candidate patch");
  check(await page.getByRole("button", { name: "采纳改动", exact: true }).isEnabled(), "Inspected completed candidate can be adopted");
  await update({ head: "candidate-v2" });
  check(await page.getByRole("button", { name: "采纳改动", exact: true }).isDisabled(), "A new candidate invalidates the previous diff inspection");
  await page.getByRole("button", { name: "查看改动", exact: true }).click();
  await page.waitForFunction(() => [...document.querySelectorAll(".team-actions button")].some(el => el.textContent === "采纳改动" && !el.disabled));
  await update({ result: "已实现导出并通过定向检查。\n\n".repeat(90) });
  await page.waitForFunction(() => { const el = document.querySelector(".sa-detail-scroll"); return el && el.scrollHeight > el.clientHeight + 500; });
  const replyTop = (await page.locator(".team-reply").boundingBox()).y;
  await page.locator(".sa-detail-scroll").evaluate(el => { el.scrollTop = 0; });
  check(Math.abs((await page.locator(".team-reply").boundingBox()).y - replyTop) < 2, "Reply stays visible at the start of a long transcript");
  await page.locator(".sa-detail-scroll").evaluate(el => { el.scrollTop = el.scrollHeight; });
  check(Math.abs((await page.locator(".team-reply").boundingBox()).y - replyTop) < 2, "Reply stays fixed while inspecting the end of a long transcript");
  check(await page.locator(".sa-detail-head").isVisible(), "Child identity remains visible while reading long results");
  await update({ result: "已实现导出并通过定向检查" });
  await page.screenshot({ path: `${output}/child-dark.png` });
  await page.getByRole("button", { name: "采纳改动", exact: true }).click(); await settle();
  check((await page.locator(".team-facts").innerText()).includes("待主对话验证"), "Adoption is not presented as verified delivery");
  check(await page.locator(".team-reply-input").isDisabled(), "Adopted patches cannot be modified and applied twice");
  const count = await page.evaluate(async sid => (await import("/05-subagents.js")).subagentRunsFor(sid).length, owner.session_id);
  await page.evaluate(sid => {
    window.__kzPreview.emit("kz:tool-start", { sessionId: sid, id: "inspect-team", name: "task", input: { action: "list" } });
    window.__kzPreview.emit("kz:tool-end", { sessionId: sid, id: "inspect-team", name: "task", ok: true, content: "[]" });
  }, owner.session_id); await settle();
  check(await page.evaluate(async ({ sid, count }) => (await import("/05-subagents.js")).subagentRunsFor(sid).length === count, { sid: owner.session_id, count }), "List/control calls never create phantom agents");
  await page.locator("#theme-toggle").click();
  await settle();
  await page.evaluate(async () => { await Promise.all(document.getAnimations().filter(animation => animation.constructor.name === "CSSTransition").map(animation => animation.finished.catch(() => {}))); });
  await page.screenshot({ path: `${output}/child-light.png` });
  await page.setViewportSize({ width: 760, height: 920 }); await settle();
  await page.evaluate(async key => (await import("/06-agent-panel.js")).openSubagentPanel(key), `${owner.session_id}|managed-writer`);
  check(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), "Narrow layout has no horizontal page overflow");
  const narrowReply = await page.locator(".team-reply").boundingBox();
  check(narrowReply.y >= 0 && narrowReply.y + narrowReply.height <= 920, "Narrow drawer keeps the reply inside the visible viewport");
  await page.screenshot({ path: `${output}/child-narrow.png` });
  check(errors.length === 0, `No browser errors: ${errors.join("; ")}`);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ checks, errors }, null, 2));
  console.log(`${checks.length} agent-team browser checks passed`);
} catch (error) {
  console.error({ checks, errors }); await page.screenshot({ path: `${output}/failure.png` }).catch(() => {}); throw error;
} finally { await browser.close(); await server.close(); }
