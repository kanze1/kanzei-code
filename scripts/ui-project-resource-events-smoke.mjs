// Real Tauri event input through production subscribers and shared resource ownership.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const result = { checks: [], errors: [], boundary: "Production ESM and actual subscribed event handler; mocked docs IPC" };
const check = (name, value) => { assert.ok(value, name); result.checks.push(name); console.log(name); };
try {
  const page = await browser.newPage();
  page.on("pageerror", error => result.errors.push(String(error)));
  await page.goto(server.origin + "/?scene=chat");
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.locator('[data-work-surface="project"]').click();
  await page.locator(".management-work-views").getByRole("button", { name: "列表", exact: true }).click();
  await page.locator(".management-row").first().waitFor();
  await page.evaluate(async () => {
    const resources = await import("/01-project-resources.js");
    const shell = await import("/03-shell.js");
    window.eventProject = shell.currentProject;
    window.eventSession = shell.activeSessionId;
    window.eventResource = resources.projectDocuments(shell.currentProject);
    window.eventDocs = structuredClone(await resources.readProjectDocuments(shell.currentProject));
    window.eventReads = () => window.__kzPreview.calls.filter(call => call.cmd === "docs_snapshot").length;
    window.eventEmit = (ok = true) => window.__kzPreview.emit("kz:tool-end", { sessionId: window.eventSession, id: "resource-event-regression", name: "req", ok, preview: "updated: R-001" });
    window.eventGeneration = 1;
    window.__kzPreview.setCommand("docs_snapshot", () => ({ ...structuredClone(window.eventDocs), eventGeneration: window.eventGeneration }));
  });
  check("project tool mutation immediately refreshes the subscribed resource", await page.evaluate(async () => {
    const before = window.eventReads();
    window.eventEmit();
    const request = window.eventResource.request;
    if (!request) return false;
    await request;
    return window.eventReads() === before + 1 && window.eventResource.data.eventGeneration === 1;
  }));
  check("failed tool results do not invalidate a successful snapshot", await page.evaluate(() => {
    const previous = window.eventResource.data, before = window.eventReads();
    window.eventEmit(false);
    return window.eventResource.status === "ready" && window.eventResource.data === previous && window.eventReads() === before;
  }));
  await page.evaluate(async () => {
    window.__kzPreview.setCommand("docs_snapshot", () => { throw new Error("event docs unavailable"); });
    window.eventEmit();
    await Promise.allSettled([window.eventResource.request]);
  });
  check("event refresh failures remain visible in the management page", (await page.locator(".management-error").allTextContents()).join(" ").includes("event docs unavailable"));
  await page.evaluate(async () => {
    await (await import("/03-shell.js")).navigate_view("settings", { persist: false });
    window.eventGeneration = 2;
    window.__kzPreview.setCommand("docs_snapshot", () => ({ ...structuredClone(window.eventDocs), eventGeneration: window.eventGeneration }));
    window.eventHiddenReads = window.eventReads();
    window.eventEmit();
  });
  // Existing projection debounce is 400ms. Cross the real callback boundary to
  // prove it cannot read documents after the owner has been unmounted.
  await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "settings_get"));
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 450)));
  const hidden = await page.evaluate(() => ({ status: window.eventResource.status, pending: !!window.eventResource.request, reads: window.eventReads() - window.eventHiddenReads, view: document.body.dataset.view }));
  result.hidden = hidden;
  check(`unmounted views keep the resource stale without background document reads: ${JSON.stringify(hidden)}`, hidden.status === "stale" && !hidden.pending && hidden.reads === 0 && hidden.view === "settings");
  check("a later read retrieves the changed version", await page.evaluate(async () => {
    const resources = await import("/01-project-resources.js");
    const data = await resources.readProjectDocuments(window.eventProject);
    return data.eventGeneration === 2 && window.eventReads() === window.eventHiddenReads + 1;
  }));
  await page.evaluate(async () => { await (await import("/03-shell.js")).navigate_view("documents"); });
  await page.locator("#documents-req-list .doc-row").first().click();
  await page.locator("#documents-req-list .doc-edit-toggle").first().click();
  const title = page.locator('#documents-req-list .doc-edit [data-field=":title"]').first();
  await title.fill("draft while composing");
  check("resource updates preserve the exact editing node, draft, focus and IME host", await page.evaluate(async () => {
    const input = document.querySelector('#documents-req-list .doc-edit [data-field=":title"]');
    window.editingNode = input; input.focus(); input.setSelectionRange(3, 8);
    input.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "拼" }));
    window.eventDocs.requirements[0].title = "event-new-visible-title";
    window.eventGeneration = 3; window.eventEmit(); await window.eventResource.request;
    return window.eventResource.data.eventGeneration === 3 && input.isConnected && document.activeElement === input
      && input.value === "draft while composing" && input.selectionStart === 3 && input.selectionEnd === 8;
  }));
  await title.dispatchEvent("compositionend", { data: "拼" });
  await page.locator("#documents-req-list .doc-edit-actions .ghost").first().click();
  check("ending editing projects the newest cached version without a timer", (await page.locator("#documents-req-list").innerText()).includes("event-new-visible-title"));
  await page.evaluate(async () => {
    const docs = await import("/12-docs-pages.js");
    docs.documentFilters.req.grouped = false; docs.documentFilters.req.sort = "manual";
    docs.renderDocsSnapshot(window.eventResource.data);
  });
  check("resource updates keep a dragged row attached until drag end", await page.evaluate(async () => {
    const item = document.querySelector('#documents-req-list .doc-item[draggable="true"]');
    if (!item) return false;
    window.draggedNode = item;
    item.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: new DataTransfer() }));
    window.eventDocs.requirements.find(entry => entry.id === item.dataset.docId).title = "event-after-drag";
    window.eventGeneration = 4; window.eventEmit(); await window.eventResource.request;
    return item.isConnected && item.classList.contains("dragging") && window.eventResource.data.eventGeneration === 4;
  }));
  await page.evaluate(() => window.draggedNode.dispatchEvent(new DragEvent("dragend", { bubbles: true })));
  check("drag end projects the latest snapshot without replacing a live drag", (await page.locator("#documents-req-list").innerText()).includes("event-after-drag"));
  check("no uncaught browser errors", result.errors.length === 0);
} finally {
  await browser.close(); await server.close();
  const artifactPath = "output/design-audit-20261016/round2/project-resource-events.json";
  await mkdir(dirname(artifactPath), { recursive: true });
  await writeFile(artifactPath, JSON.stringify(result, null, 2));
}
console.log(`Project resource event acceptance passed: ${result.checks.length} checks.`);
