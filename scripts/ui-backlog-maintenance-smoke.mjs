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
  await page.locator("#composer-work-next").click();
  await rail.waitFor({ state: "visible" });
  check(await rail.locator(".work-focus-slot").count() === 2, "Conversation keeps only current work and next candidate");
  check(!/R-003|R-004|D-002/.test(await rail.innerText()), "Parked, external acceptance and blocked items are excluded from conversation candidates");
  await page.screenshot({ path: `${output}/rail-light.png` });
  await page.locator('[data-work-surface="project"]').click();
  await page.locator(".management-work-views").getByRole("button", { name: "列表", exact: true }).click();
  await page.locator('.management-row[data-work-id="R-002"]').waitFor();
  check(await page.locator(".management-row").count() === 4, "Management retains every open requirement, including non-executable states");
  await page.locator('.management-tools select').selectOption("awaiting_external");
  check(await page.locator(".management-row").count() === 1 && await page.locator('[data-work-id="R-004"]').isVisible(), "External acceptance has a dedicated management filter");
  await page.locator('.management-tools select').selectOption("open");
  await page.locator('[data-management-tab="defect"]').click();
  check(await page.locator(".management-row").count() === 2, "Defects are shown in their own peer page");
  await page.locator('.management-tools [aria-label="更多"]').click();
  await page.getByRole("menuitem", { name: "高级筛选与批量管理", exact: true }).click();
  await page.locator("#documents-filter-toggle").click();
  await page.locator("#documents-priority-filter").selectOption("P0");
  check(await page.locator("#documents-defect-list .doc-item").count() === 1, "Advanced priority filter is still functional");
  await page.evaluate(async () => (await import("/14-docs-actions.js")).refreshDocs());
  check(await page.locator("#documents-priority-filter").inputValue() === "P0", "Advanced filters survive snapshot refresh");
  await page.locator("#documents-tag-filter").selectOption("前端");
  check(await page.locator("#documents-defect-list .doc-item").count() === 0, "Tag and priority combine without inventing matches");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.locator('[data-work-surface="project"]').click();
  await page.locator('[data-management-tab="req"]').click();
  await page.locator('.management-row[data-work-id="R-002"]').click();
  await page.locator(".management-body").getByRole("button", { name: "→ 转 已放弃", exact: true }).click();
  await page.locator("#input-value").fill("已被新方案替代，取消旧需求");
  await page.locator("#input-ok").click();
  await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "docs_update" && call.args.reason));
  const update = await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "docs_update").at(-1).args);
  check(update.action === "close" && update.status === "dropped" && update.reason.includes("新方案"), "Full-page cancellation passes an explicit reason through the production IPC");
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
      if (id.startsWith("D-")) {
        const entry = window.__backlogDocs.defects.find(entry => entry.id === id);
        entry.status = "awaiting_external"; entry.nextStatuses = ["fixing", "fixed", "wontfix"];
      }
      await (await import("/14-docs-actions.js")).refreshDocs();
    }, id);
    await page.locator(`[data-management-tab="${id.startsWith("D-") ? "defect" : "req"}"]`).click();
    await page.locator(`.management-row[data-work-id="${id}"]`).click();
    await page.locator(".management-body").getByRole("button", { name: "退回开发", exact: true }).waitFor();
    check(await page.locator(".management-body").getByRole("button", { name: "退回开发", exact: true }).isVisible() && !await page.locator(".management-body").getByRole("button", { name: "继续此需求", exact: true }).count(), `${id} external detail offers an explicit return to development`);
    const runCount = await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length);
    await page.locator(".management-body").getByRole("button", { name: "退回开发", exact: true }).click();
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
  // A long management list remains reachable without disturbing the conversation.
  const beforeScroll = await page.evaluate(() => ({ chat: document.getElementById("messages").scrollTop, draft: document.getElementById("prompt").value }));
  await page.evaluate(async () => {
    const sample = window.__backlogDocs.requirements[0];
    window.__backlogDocs.requirements = Array.from({ length: 60 }, (_, i) => ({ ...structuredClone(sample), id: `R-${100 + i}`, title: `密集列表 ${i}`, status: "doing" }));
    await (await import("/14-docs-actions.js")).refreshDocs();
  });
  await page.locator('[data-management-tab="req"]').click();
  await page.locator(".management-list").hover();
  await page.mouse.wheel(0, 1200);
  await page.waitForFunction(() => document.getElementById("view-project").scrollTop > 0);
  const afterScroll = await page.evaluate(() => ({ chat: document.getElementById("messages").scrollTop, draft: document.getElementById("prompt").value }));
  check(JSON.stringify(beforeScroll) === JSON.stringify(afterScroll), "Management scrolling preserves conversation position and draft");
  await page.locator('.management-row[data-work-id="R-159"]').scrollIntoViewIfNeeded();
  check(await page.locator('.management-row[data-work-id="R-159"]').isVisible(), "The final dense-list entry remains reachable");
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
  // Confirmed archive writes own retirement, even if the next read fails or an
  // older read is still in flight. Keep the raw last-good snapshot as evidence.
  await page.evaluate(async () => {
    (await import("/02-i18n.js")).setLanguagePreference("zh");
    const { currentProject } = await import("/03-shell.js");
    const entry = { id: "R-401", title: "归档后关闭旧编辑器", status: "doing", priority: "P1", closed: false, blocked: false,
      fields: [["标签", "前端"], ["验收", "完成后返回列表"]], nextStatuses: ["done", "dropped"] };
    const survivor = { ...structuredClone(entry), id: "R-402", title: "保留其他活动条目" };
    const test = window.__archiveCheck = { project: currentProject, other: `${currentProject}/other-project`, entry,
      committed: false, rejectClose: true, hold: false, recover: false,
      docs: { requirements: [entry, survivor], defects: [], ideas: [], archived: { req: 0, defect: 0 } } };
    window.__kzPreview.setCommand("docs_snapshot", args => {
      if (args.projectDir === test.other) return { ...structuredClone(test.docs), requirements: [structuredClone(entry)] };
      if (test.hold && !test.release) {
        const old = structuredClone(test.docs);
        return new Promise(resolve => { test.release = () => resolve(old); });
      }
      if (test.committed && !test.recover) throw new Error("CONTROLLED_POST_COMMIT_SNAPSHOT_READ_ERROR");
      return structuredClone(test.docs);
    });
    window.__kzPreview.setCommand("docs_update", args => {
      if (args.projectDir !== test.project || args.id !== entry.id || args.action !== "close") throw new Error("Unexpected archive mutation");
      if (test.rejectClose) throw new Error("CONTROLLED_ARCHIVE_WRITE_ERROR");
      test.docs.requirements = test.docs.requirements.filter(row => row.id !== entry.id);
      test.docs.archived.req += 1; test.committed = true;
      return `${entry.id} 已完成并归档`;
    });
    const { readProjectDocuments, projectDocuments } = await import("/01-project-resources.js");
    await readProjectDocuments(test.other, { force: true });
    await readProjectDocuments(test.project, { force: true });
    test.lastGood = projectDocuments(test.project).data;
  });
  await page.locator('[data-work-surface="project"]').click();
  await page.locator('[data-management-tab="req"]').click();
  await page.locator('.management-row[data-work-id="R-401"]').click();
  await page.locator(".management-body .doc-edit-toggle").click();
  await page.locator('.management-body .doc-edit [data-field=":title"]').fill("保留未提交草稿");
  await page.locator(".management-body").getByRole("button", { name: "→ 转 已完成", exact: true }).click();
  await page.locator("#confirm-cancel").click();
  check(await page.locator('.management-body .doc-edit [data-field=":title"]').inputValue() === "保留未提交草稿" && !await page.evaluate(() => window.__archiveCheck.committed), "Cancelling archive preserves the active detail and unsaved draft");
  await page.evaluate(async () => {
    const { transitionEntryStatus } = await import("/11-docs-list.js");
    window.__archiveTransition = transitionEntryStatus(window.__archiveCheck.entry, "req", "done");
  });
  await page.locator("#confirm-ok").click();
  check(!await page.evaluate(() => window.__archiveTransition) && await page.locator('.management-body .doc-edit [data-field=":title"]').inputValue() === "保留未提交草稿", "A failed archive write preserves the editor and its draft");
  check(!await page.evaluate(async () => {
    const test = window.__archiveCheck;
    return (await import("/01-project-resources.js")).projectDocumentIsClosed(test.project, test.entry.id);
  }), "A failed write publishes no confirmed closure");
  await page.evaluate(async () => {
    const test = window.__archiveCheck; test.rejectClose = false; test.hold = true;
    test.pendingRead = (await import("/01-project-resources.js")).readProjectDocuments(test.project, { force: true }).catch(String);
  });
  await page.waitForFunction(() => Boolean(window.__archiveCheck.release));
  await page.locator(".management-body").getByRole("button", { name: "→ 转 已完成", exact: true }).click();
  await page.locator("#confirm-ok").click();
  await page.waitForFunction(() => window.__archiveCheck.committed && !document.querySelector(".management-detail-nav"));
  check(!await page.locator('.management-row[data-work-id="R-401"]').count() && await page.locator('.management-row[data-work-id="R-402"]').isVisible(), "Successful archive closes the editor and removes only its active row before the in-flight read settles");
  const failedRead = await page.evaluate(async () => {
    const test = window.__archiveCheck; test.release(); await test.pendingRead;
    const { projectDocuments, projectDocumentsView, projectDocumentIsClosed } = await import("/01-project-resources.js");
    const resource = projectDocuments(test.project);
    return { status: resource.status, error: resource.error, sameSnapshot: resource.data === test.lastGood,
      rawIds: resource.data.requirements.map(row => row.id), viewIds: projectDocumentsView(test.project, resource.data).requirements.map(row => row.id),
      otherOpen: !projectDocumentIsClosed(test.other, test.entry.id) && projectDocumentsView(test.other, projectDocuments(test.other).data).requirements.some(row => row.id === test.entry.id) };
  });
  check(failedRead.status === "error" && failedRead.error.includes("CONTROLLED_POST_COMMIT_SNAPSHOT_READ_ERROR") && await page.locator(".management-feedback").innerText().then(text => text.includes("CONTROLLED_POST_COMMIT_SNAPSHOT_READ_ERROR")), "The post-commit read failure remains explicit in the resource and management page");
  check(failedRead.sameSnapshot && failedRead.rawIds.includes("R-401") && !failedRead.viewIds.includes("R-401"), "An obsolete in-flight read cannot revive the archived row or mutate the last-good backend snapshot");
  check(failedRead.otherOpen, "A confirmed closure does not affect the same document ID in another project");
  await page.evaluate(async () => {
    const test = window.__archiveCheck;
    await (await import("/30-management.js")).openManagementItem(test.project, test.entry);
  });
  check(!await page.locator(".management-detail-nav").count(), "A stale external link cannot reopen the confirmed archived editor after a read failure");
  await page.evaluate(async () => {
    const test = window.__archiveCheck;
    await (await import("/30-management.js")).openManagementItem(test.project, { ...test.entry, status: "done", closed: true, nextStatuses: [] });
  });
  check(await page.locator(".management-detail-nav").isVisible() && !await page.locator(".management-body .doc-edit-toggle").count(), "Explicit read-only archive history remains accessible during the read failure");
  await page.locator('[data-management-tab="req"]').click();
  await page.locator('.management-tools [aria-label="更多"]').click();
  await page.getByRole("menuitem", { name: "高级筛选与批量管理", exact: true }).click();
  check(!await page.locator('#documents-req-list .doc-item[data-doc-id="R-401"]').count() && await page.locator("#documents-error").innerText().then(text => text.includes("CONTROLLED_POST_COMMIT_SNAPSHOT_READ_ERROR")), "Advanced document lists exclude confirmed closures while preserving the read error");
  await page.evaluate(async () => {
    await (await import("/12-workbench.js")).openProjectSpace(window.__archiveCheck.project, "chat");
  });
  check(!(await rail.innerText()).includes("R-401"), "Conversation candidates cannot offer confirmed archived work from the old snapshot");
  const lifecycle = await page.evaluate(async () => {
    const test = window.__archiveCheck;
    const { projectDocuments, projectDocumentClosed, projectDocumentIsClosed, projectDocumentsView, readProjectDocuments } = await import("/01-project-resources.js");
    const uncached = `${test.project}/uncached`;
    projectDocumentClosed(uncached, test.entry.id);
    const resource = projectDocuments(uncached);
    const before = resource.data === null && projectDocumentsView(uncached, resource.data) === null;
    let failure = "";
    try { await resource.load(); } catch (error) { failure = String(error); }
    const retained = resource.data === null && resource.status === "error" && projectDocumentIsClosed(uncached, test.entry.id);
    // A later successful authoritative read may reflect a manual restoration.
    test.recover = true; test.docs.requirements.unshift(structuredClone(test.entry)); test.docs.archived.req = 0;
    await readProjectDocuments(test.project, { force: true });
    const current = projectDocuments(test.project);
    return { before, retained, failure, current: current.isCurrent(), cleared: !projectDocumentIsClosed(test.project, test.entry.id), exact: projectDocumentsView(test.project, current.data) === current.data };
  });
  check(lifecycle.before && lifecycle.retained && lifecycle.failure.includes("CONTROLLED_POST_COMMIT_SNAPSHOT_READ_ERROR"), "A confirmed closure without cached data preserves null and survives read errors");
  check(lifecycle.current && lifecycle.cleared && lifecycle.exact, "Only a current successful snapshot supersedes pending confirmed closures");
  await page.evaluate(async () => {
    const test = window.__archiveCheck;
    test.docs.requirements = ["R-801", "R-802"].map(id => ({ ...structuredClone(test.entry), id }));
    test.docs.archived.req = 0; test.committed = false;
    window.__kzPreview.setCommand("docs_snapshot", () => {
      if (test.committed) throw new Error("CONTROLLED_POST_COMMIT_SNAPSHOT_READ_ERROR");
      return structuredClone(test.docs);
    });
    window.__kzPreview.setCommand("docs_update", args => {
      if (args.projectDir !== test.project || args.action !== "update" || args.status !== "done") throw new Error("Unexpected batch mutation");
      if (args.id === "R-802") throw new Error("CONTROLLED_BATCH_ITEM_WRITE_ERROR");
      if (args.id !== "R-801") throw new Error("Unexpected batch document");
      test.docs.requirements = test.docs.requirements.filter(entry => entry.id !== args.id);
      test.docs.archived.req += 1; test.committed = true;
      return `${args.id} archived`;
    });
    await (await import("/01-project-resources.js")).readProjectDocuments(test.project, { force: true });
    await (await import("/30-management.js")).openManagementItem(test.project, test.docs.requirements[0]);
    const { batchSelection, applyBatch } = await import("/11-docs-list.js");
    document.getElementById("documents-batch-status").innerHTML = '<option value="done">已完成</option>';
    document.getElementById("documents-batch-tag").value = "";
    batchSelection.clear(); batchSelection.set("R-801", "req"); batchSelection.set("R-802", "req");
    await applyBatch();
  });
  await page.waitForFunction(async () => {
    const test = window.__archiveCheck;
    return (await import("/01-project-resources.js")).projectDocuments(test.project).status === "error";
  });
  check(!await page.locator(".management-detail-nav").count() && !await page.locator('.management-row[data-work-id="R-801"]').count() && await page.locator('.management-row[data-work-id="R-802"]').isVisible(), "A partly successful terminal batch retires its successful item and keeps the failed item active");
  check(await page.evaluate(async () => {
    const test = window.__archiveCheck;
    const { projectDocumentIsClosed } = await import("/01-project-resources.js");
    return projectDocumentIsClosed(test.project, "R-801") && !projectDocumentIsClosed(test.project, "R-802");
  }), "Batch retirement facts follow individual successful backend acknowledgements");
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
