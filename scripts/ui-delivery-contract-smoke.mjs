/* global window, document */
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const before = process.argv.includes("--before");
const output = `output/audit-WB6/deliveries/${before ? "before" : "current"}`;
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage();
const errors = [], checks = [];
page.on("pageerror", error => errors.push(String(error)));
const check = (name, actual, expected) => {
  try { assert.deepEqual(actual, expected); checks.push({ name, passed: true }); }
  catch (error) { checks.push({ name, passed: false, actual, expected, message: error.message }); }
};
try {
  if (before) {
    for (const file of ["06-deliveries.js", "05-chat-render.js"]) {
      const source = execFileSync("git", ["show", `26acb7f0:crates/kanzei-app/ui/${file}`], { encoding: "utf8" });
      await page.route(`**/${file}`, route => route.fulfill({ contentType: "text/javascript", body: source }));
    }
  }
  await page.goto(`${server.origin}/?scene=chat`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  const result = await page.evaluate(async () => {
    const delivery = await import("/06-deliveries.js"), core = await import("/01-core.js");
    const chat = await import("/05-chat-render.js"), md = await import("/04-markdown.js");
    const sa = await import("/05-subagents.js");
    const project = "C:/WB6/deliveries", session = "wb6-delivery-session";
    const row = { id: "receipt-1", kind: "file", name: "report.csv", path: `${project}/out/report.csv`, project_dir: project,
      worktree_root: project, session_id: session, bytes: 10, created_at: 1000, status: "available", caption: "first receipt" };
    core.showPane(session);
    let rows = [row];
    window.__kzPreview.setCommand("delivered_files", () => structuredClone(rows));
    await delivery.loadDeliveredFiles(project, { force: true });
    const state = delivery.deliveryState(project);
    const snapshot = () => ({ id: state.rows[0]?.id, status: state.rows[0]?.status, bytes: state.rows[0]?.current_bytes });
    // The tool trace is stored while its subagent details remain collapsed.
    const run = sa.subagentStart({ sessionId: session, id: "wb6-agent", input: { prompt: "Produce report" } });
    sa.subagentProgress({ sessionId: session, id: "wb6-agent", trace: { phase: "start", child_id: "deliver-1", name: "deliver", input: { path: row.path } } });
    sa.subagentProgress({ sessionId: session, id: "wb6-agent", trace: { phase: "end", child_id: "deliver-1", name: "deliver", ok: true, outcome: "success", preview: "[delivered] report.csv", display: row } });
    rows = [{ ...row, status: "unavailable", current_bytes: 10 }];
    await delivery.loadDeliveredFiles(project, { force: true });
    const host = document.createElement("div"); document.body.append(host);
    sa.renderSubagentTimeline(sa.createSubagentView(host, run));
    const replay = { cache: snapshot(), cardDisabled: host.querySelector(".file-card-name")?.disabled, cardStatus: host.querySelector(".file-card")?.dataset.status };
    rows = [{ ...row, id: "receipt-2", created_at: 2000, caption: "new receipt", status: "changed", current_bytes: 20 }];
    await delivery.loadDeliveredFiles(project, { force: true });
    const olderHost = document.createElement("div"); document.body.append(olderHost);
    sa.renderSubagentTimeline(sa.createSubagentView(olderHost, run));
    const oldReceipt = { cache: snapshot(), caption: olderHost.querySelector(".file-card-caption")?.textContent };

    rows = [{ ...row, status: "changed", current_bytes: 20 }];
    await delivery.loadDeliveredFiles(project, { force: true });
    const message = chat.addMessage("assistant", "");
    md.renderMarkdownInto(message.querySelector(".message-body"), "Delivered `report.csv`.");
    const initialSize = message.querySelector(".file-card-size")?.textContent;
    rows = [{ ...row, status: "changed", current_bytes: 40 }];
    await delivery.loadDeliveredFiles(project, { force: true });
    const updatedSize = message.querySelector(".file-card-size")?.textContent;
    const duplicateCards = message.querySelectorAll(".message-deliveries .file-card").length;

    const otherProject = "C:/WB6/other";
    const other = { ...row, project_dir: otherProject, worktree_root: otherProject, path: `${otherProject}/out/report.csv`, session_id: "wb6-other-session" };
    delivery.registerDelivery(other, other.session_id);
    const scoped = delivery.deliveredFileFor(session, "report.csv")?.path;
    const ambiguity = delivery.matchDeliveredFile("report.csv", [row, { ...row, path: `${project}/other/report.csv` }]);
    const relative = delivery.matchDeliveredFile("out/report.csv", [row])?.path;
    const alias = delivery.deliveryState("\\\\?\\C:\\WB6\\deliveries\\") === state;
    const sameTime = { ...row, id: "receipt-same-ms", created_at: 1000, caption: "same millisecond valid receipt" };
    delivery.registerDelivery(sameTime, session);
    const tieAccepted = state.rows[0].id;

    let release, entered;
    const started = new Promise(resolve => { entered = resolve; });
    window.__kzPreview.setCommand("delivered_files", () => { entered(); return new Promise(resolve => { release = resolve; }); });
    const loading = delivery.loadDeliveredFiles(project, { force: true });
    await started;
    const deduped = delivery.loadDeliveredFiles(project, { force: true }) === loading;
    const live = { ...row, id: "receipt-live", created_at: 3000, caption: "live" };
    delivery.registerDelivery(live, session);
    release([{ ...row, status: "unavailable" }]); await loading;
    const mergedLive = state.rows[0].id;
    window.__kzPreview.setCommand("delivered_files", () => { throw new Error("WB6 receipt read failure"); });
    await delivery.loadDeliveredFiles(project, { force: true });
    const failure = { retained: state.rows[0].id, error: state.error.includes("WB6 receipt read failure"), pending: state.request !== null };
    window.__kzPreview.setCommand("delivered_files", [live]);
    await delivery.loadDeliveredFiles(project, { force: true });
    const recovered = { id: state.rows[0].id, error: state.error, loaded: state.loaded };
    return { replay, oldReceipt, initialSize, updatedSize, duplicateCards, scoped, ambiguity, relative, alias, tieAccepted, deduped, mergedLive, failure, recovered };
  });
  check("Opening saved subagent trace retains current backend receipt status", result.replay.cache, { id: "receipt-1", status: "unavailable", bytes: 10 });
  check("Subagent trace card uses the selected current receipt", [result.replay.cardDisabled, result.replay.cardStatus], [true, "unavailable"]);
  check("Older subagent receipt cannot replace a newer delivery", result.oldReceipt.cache, { id: "receipt-2", status: "changed", bytes: 20 });
  check("Older trace card displays the selected latest receipt", result.oldReceipt.caption, "new receipt");
  check("Initial reply card displays current file size", result.initialSize, "20 B");
  check("Changed file size refreshes even when receipt id and status stay unchanged", result.updatedSize, "40 B");
  check("A matching reply reference produces one delivery card", result.duplicateCards, 1);
  check("Other session and project receipts do not hijack filename resolution", result.scoped, "C:/WB6/deliveries/out/report.csv");
  check("Ambiguous filename never chooses a path", result.ambiguity, null);
  check("Worktree relative path matches exact artifact", result.relative, "C:/WB6/deliveries/out/report.csv");
  check("Existing Windows project aliases share one state owner", result.alias, true);
  check("Different valid receipt in the same millisecond is accepted", result.tieAccepted, "receipt-same-ms");
  check("Concurrent refresh requests share one pending load", result.deduped, true);
  check("Live receipt registered during load wins over older snapshot", result.mergedLive, "receipt-live");
  check("Failed load retains receipts and exposes error with pending request cleared", result.failure, { retained: "receipt-live", error: true, pending: false });
  check("Retry clears the old error and completes loading", result.recovered, { id: "receipt-live", error: "", loaded: true });
  check("No browser runtime errors", errors, []);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ before, checks, errors }, null, 2));
  console.log(`Delivery contracts: ${checks.filter(c => c.passed).length} passed, ${checks.filter(c => !c.passed).length} failed`);
  assert(checks.every(c => c.passed), JSON.stringify(checks.filter(c => !c.passed)));
} finally { await browser.close(); await server.close(); }
