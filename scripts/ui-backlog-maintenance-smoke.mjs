/* global window, document */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
const output = "output/playwright/backlog-maintenance";
await mkdir(output, { recursive: true });
const passed = [], errors = [];
const check = (value, label) => { assert(value, label); passed.push(label); };
page.on("pageerror", error => errors.push(error.message));
page.setDefaultTimeout(10000);
try {
  await page.goto(`${server.origin}/?scene=chat&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async () => {
    const fixtures = await window.__kzPreview.fixtures();
    for (const process of fixtures.state.processes) process.running = false;
    const make = (id, status, priority, tag, blocked = false, parked = false) => ({
      id, title: `${id} 测试需求的完整标题`, status, priority, closed: false, blocked,
      fields: [["标签", tag], ["验收", "①保留原始验收"], ["进展", "实现与本地验证已完成"], ...(status === "awaiting_external" ? [["外部验收", "真实 SSH 断线恢复"]] : []), ...(parked ? [["停车", "等待设备"]] : [])],
      block_reasons: blocked ? [parked ? "停车:等待设备" : "等待前置需求"] : [],
      nextStatuses: id.startsWith("D-") ? ["fixed", "wontfix"] : ["doing", "done", "dropped"],
      batches: { done: status === "doing" ? 1 : 0, total: 3 },
    });
    const docs = { requirements: [make("R-001", "doing", "P1", "核心"), make("R-002", "todo", "P2", "前端"), make("R-003", "doing", "P1", "流程", true, true), make("R-004", "awaiting_external", "P2", "后端")],
      defects: [make("D-001", "fixing", "P0", "后端"), make("D-002", "open", "P2", "前端", true)],
      ideas: [], archived: { req: 0, defect: 0 }, archived_entries: { req: [], defect: [] } };
    window.__backlogDocs = docs;
    window.__kzPreview.setCommand("docs_snapshot", () => structuredClone(window.__backlogDocs));
    await (await import("/09-sessions.js")).refreshProcesses();
    await (await import("/26-project-conversations.js")).refreshConversationWork();
  });
  const rail = page.locator("#project-chat-work");
  if (await page.locator("#tasks-close").isVisible()) await page.locator("#tasks-close").click();
  await rail.waitFor({ state: "visible" });
  const groups = await rail.locator("[data-work-group]").evaluateAll(nodes => nodes.map(node => [node.dataset.workGroup, node.querySelector(".sw-work-count").textContent]));
  check(JSON.stringify(groups) === JSON.stringify([["active", "2"], ["pending", "1"], ["external", "1"], ["blocked", "1"], ["parked", "1"]]), "State groups show accurate counts for both tracker kinds");
  check(!await rail.locator('[data-work-group="external"]').evaluate(node => node.open), "External acceptance starts collapsed and stays outside development groups");
  check(!await rail.locator('[data-work-group="parked"]').evaluate(node => node.open), "Parked work starts collapsed");
  check(await rail.locator('[data-work-id="D-001"] .sw-work-priority').textContent() === "P0" && (await rail.locator('[data-work-id="D-001"] .sw-work-type').textContent()).includes("缺陷 · 后端"), "Entry shows its kind, priority and tag together");
  await page.screenshot({ path: `${output}/rail-light.png` });
  await rail.locator('[data-work-filter="kind"]').selectOption("defect");
  check(await rail.locator("[data-work-id]").count() === 2, "Kind filter shows only defects");
  await rail.locator('[data-work-filter="priority"]').selectOption("P0");
  check(await rail.locator("[data-work-id]").count() === 1, "Priority combines with kind filter");
  await page.evaluate(async () => (await import("/26-project-conversations.js")).refreshConversationWork());
  check(await rail.locator('[data-work-filter="priority"]').inputValue() === "P0", "Filters survive automatic refresh");
  await rail.locator('[data-work-filter="tag"]').selectOption("前端");
  check(await rail.getByText("暂无匹配条目", { exact: true }).isVisible(), "Empty filters give an honest empty state");
  await rail.getByRole("button", { name: "清除筛选", exact: true }).click();
  check(await rail.locator("[data-work-id]").count() === 6, "Clearing filters restores every active entry");
  await rail.locator('[data-work-group="parked"] > summary').click();
  await page.waitForTimeout(80);
  await page.evaluate(async () => (await import("/26-project-conversations.js")).refreshConversationWork());
  check(await rail.locator('[data-work-group="parked"]').evaluate(node => node.open), "Group expansion survives refresh");
  await rail.locator('[data-work-id="R-002"]').click();
  await rail.getByRole("button", { name: "取消需求", exact: true }).click();
  await page.locator("#input-value").fill("已被新方案替代，取消旧需求");
  await page.locator("#input-ok").click();
  await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "docs_update" && call.args.reason));
  const update = await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "docs_update").at(-1).args);
  check(update.action === "close" && update.status === "dropped" && update.reason.includes("新方案"), "Sidebar cancellation passes an explicit reason through the production IPC");
  await page.evaluate(async () => {
    const { backlogTally } = await import("/12-docs-pages.js");
    window.__externalTally = backlogTally(window.__backlogDocs.requirements.filter(entry => entry.status === "awaiting_external"), "req");
    const { transitionEntryStatus } = await import("/11-docs-list.js");
    window.__externalTransition = transitionEntryStatus(window.__backlogDocs.requirements[1], "req", "awaiting_external");
  });
  await page.locator("#input-value").fill("真实 SSH 运行与断线恢复");
  await page.locator("#input-ok").click();
  await page.evaluate(() => window.__externalTransition);
  const external = await page.evaluate(() => ({ tally: window.__externalTally, args: window.__kzPreview.calls.filter(call => call.cmd === "docs_update").at(-1).args }));
  check(external.tally.blocked === 0 && external.tally.workable === 0 && external.tally.invalid === 0, "External acceptance is visible but excluded from development and blocked counts");
  check(external.args.status === "awaiting_external" && external.args.fields["外部验收"].includes("SSH"), "External acceptance dialog records the outstanding real-host checks");
  // Returning to development must use each tracker's state machine, without starting a run.
  for (const [id, nextStatus] of [["R-004", "doing"], ["D-002", "fixing"]]) {
    await page.evaluate(async id => {
      if (id.startsWith("D-")) window.__backlogDocs.defects.find(entry => entry.id === id).status = "awaiting_external";
      await (await import("/26-project-conversations.js")).refreshConversationWork();
    }, id);
    await rail.locator('[data-work-group="external"]').evaluate(node => { node.open = true; });
    await rail.locator(`[data-work-id="${id}"]`).click();
    check(await rail.getByRole("button", { name: "退回开发", exact: true }).isVisible() && !await rail.getByRole("button", { name: "继续此需求", exact: true }).count(), `${id} external detail offers an explicit return to development`);
    const runCount = await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length);
    await rail.getByRole("button", { name: "退回开发", exact: true }).click();
    await page.waitForFunction(({ id, nextStatus }) => window.__kzPreview.calls.some(call => call.cmd === "docs_update" && call.args.id === id && call.args.status === nextStatus), { id, nextStatus });
    check(await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length) === runCount, `${id} return writes ${nextStatus} without starting a conversation run`);
  }
  // The batch selector exposes the same transition as the single-entry action.
  // Require the actual backend fields in the IPC fixture; missing evidence must fail.
  await page.evaluate(async () => {
    window.__batchExternalCalls = [];
    window.__kzPreview.setCommand("docs_update", args => {
      if (args.status === "awaiting_external") {
        if (!args.fields?.["外部验收"]?.trim()) throw new Error("EXTERNAL_ACCEPTANCE_DETAIL_REQUIRED");
        window.__batchExternalCalls.push(structuredClone(args));
      }
      return "updated";
    });
    const { batchSelection, applyBatch } = await import("/11-docs-list.js");
    const { documentStatusOptions } = await import("/12-docs-pages.js");
    const select = document.getElementById("documents-batch-status");
    select.replaceChildren(...documentStatusOptions.req.map(([value, label]) => new window.Option(label, value)));
    select.value = "awaiting_external";
    document.getElementById("documents-batch-tag").value = "前端";
    batchSelection.clear(); batchSelection.set("R-001", "req"); batchSelection.set("R-002", "req");
    window.__batchExternalTransition = applyBatch();
  });
  await page.locator("#input-value").fill("两项均需真实 SSH 上确认恢复");
  await page.locator("#input-ok").click();
  await page.evaluate(() => window.__batchExternalTransition);
  const batch = await page.evaluate(() => window.__batchExternalCalls);
  check(batch.length === 2 && batch.every(args => args.status === "awaiting_external" && args.fields["外部验收"].includes("SSH") && args.fields["标签"] === "前端"), "Batch external acceptance sends the required evidence and preserves the selected tag for every entry");
  await page.evaluate(async () => {
    const { batchSelection, applyBatch } = await import("/11-docs-list.js");
    document.getElementById("documents-batch-status").innerHTML = '<option value="awaiting_external">待外部验收</option>';
    batchSelection.set("R-001", "req");
    window.__batchExternalTransition = applyBatch();
  });
  await page.locator("#input-cancel").click();
  await page.evaluate(() => window.__batchExternalTransition);
  check(await page.evaluate(() => window.__batchExternalCalls.length) === 2, "Cancelling batch acceptance commits no state transition");
  // A dense sidebar must scroll without moving the conversation or its composer.
  await page.evaluate(async () => {
    const sample = window.__backlogDocs.requirements[0];
    window.__backlogDocs.requirements = Array.from({ length: 60 }, (_, i) => ({ ...structuredClone(sample), id: `R-${100 + i}`, title: `密集列表 ${i}`, status: "doing" }));
    await (await import("/26-project-conversations.js")).refreshConversationWork();
    document.getElementById("project-chat-work").scrollTop = 0;
  });
  const beforeScroll = await page.evaluate(() => ({ chat: document.getElementById("messages").scrollTop, prompt: document.getElementById("prompt").getBoundingClientRect().top }));
  await rail.hover();
  await page.mouse.wheel(0, 1200);
  await page.waitForFunction(() => document.getElementById("project-chat-work").scrollTop > 0);
  const afterScroll = await page.evaluate(() => ({ chat: document.getElementById("messages").scrollTop, prompt: document.getElementById("prompt").getBoundingClientRect().top }));
  check(JSON.stringify(beforeScroll) === JSON.stringify(afterScroll), "Dense sidebar scroll leaves conversation position and composer geometry unchanged");
  await rail.locator('[data-work-id="R-159"]').scrollIntoViewIfNeeded();
  check(await rail.locator('[data-work-id="R-159"]').isVisible(), "The final dense-list entry remains reachable");
  await page.screenshot({ path: `${output}/rail-dense.png` });
  const outcomes = await page.evaluate(async () => {
    const chat = await import("/05-chat-render.js"), activity = await import("/06-activity.js"), summary = await import("/05-tool-summary.js");
    const raw = "[tool_outcome=blocked_by_workflow code=BATCH_CLOSING]\nBATCH_CLOSING: close batch first";
    const history = summary.toolResultSummary("bash", { ok: false, content: raw });
    return { chat: chat.toolOutcomeView(false, "blocked_by_workflow"), activity: activity.activityOutcomeView(false, "blocked_by_workflow"), history };
  });
  check(outcomes.chat.cls === "warn" && outcomes.activity.cls === "warn" && outcomes.history.outcome === "blocked_by_workflow", "Workflow pause has the same non-failure state in live and history views");
  check(outcomes.history.text.includes("流程暂缓"), "Workflow rejection summary explains the next action");
  await page.evaluate(async () => {
    (await import("/02-i18n.js")).setLanguagePreference("en");
    await (await import("/26-project-conversations.js")).refreshConversationWork();
  });
  await page.screenshot({ path: `${output}/rail-en.png` });
  check(errors.length === 0, `No browser runtime errors: ${errors.join("; ")}`);
  await writeFile(`${output}/verification.json`, JSON.stringify({ passed, errors, scope: "Browser rendering and IPC inputs; tracker writes verified by Rust tests" }, null, 2));
  console.log(`Backlog maintenance browser checks passed: ${passed.length}`);
} catch (error) {
  await page.screenshot({ path: `${output}/failure.png` });
  console.error(await page.evaluate(() => ({ view: document.body.dataset.view, space: document.body.dataset.space, errors: document.querySelector("#log")?.textContent?.slice(-400) })));
  throw error;
} finally {
  await browser.close();
  await server.close();
}
