// Production UI in Edge; backend receipts/events are isolated deterministic fixtures.
/* global window, document */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/case-reliability";
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
page.setDefaultTimeout(10000);
const checks = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const module = name => page.locator(`[data-module="${name}"]`);
page.on("pageerror", error => errors.push(error.message));
let identity;
const emit = async (name, payload = {}) => {
  await page.evaluate(({ name, payload }) => window.__kzPreview.emit(name, payload), { name, payload: { sessionId: identity.session, ...payload } });
  await settle();
};
try {
  await page.goto(`${server.origin}/?scene=startup&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready); await settle();
  identity = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures(), snapshot = f.commands.workspace_snapshot();
    for (const project of snapshot.projects) for (const line of project.lines) { line.running = false; line.stage = "空闲"; }
    const a = snapshot.projects[0];
    window.__case = { f, snapshot, a, questions: [], traceReadFailed: false, released: false };
    window.__kzPreview.setCommand("workspace_snapshot", () => structuredClone(snapshot));
    window.__kzPreview.setCommand("workspace_overview", () => ({ projects: structuredClone(snapshot.projects) }));
    window.__kzPreview.setCommand("conversation_trace_get", args => {
      if (window.__case.traceReadFailed || args.projectDir !== a.path) throw "trace 暂时不可用";
      const rows = window.__case.raceRows || [{ events: [
        { kind: "tool.started", id: "early-research", name: "webfetch", input: { url: "https://example.com/research" } },
        { kind: "tool.completed", id: "early-research", name: "webfetch", ok: true, content: "进入概览前的真实调研记录" },
      ] }];
      if (!window.__case.released || window.__case.delayTrace) return new Promise(resolve => { window.__case.releaseTrace = () => { window.__case.released = true; window.__case.delayTrace = false; resolve(rows); }; });
      return rows;
    });
    window.__kzPreview.setCommand("softwire_questions", () => structuredClone(window.__case.questions));
    window.__kzPreview.setCommand("batch_evidence", () => [
      { id: "handoff-1", kind: "handoff", entry_id: "R-001", summary: "三份调研文档已交接", run_id: "batch-1", created_at: 1800000000000, status: "declared", source: "work_log", paths: ["docs/research.md"], evidence_refs: ["commit:fd031b5"], test_record_ids: [] },
      { id: "git-1", kind: "git_checkpoint", commit: "fd031b5", run_id: "batch-1", created_at: 1800000000100, status: "recorded", source: "tool_fact", paths: ["docs/research.md"], evidence_refs: [], test_record_ids: [] },
    ]);
    window.__kzPreview.setCommand("softwire_answer_question", () => { window.__case.questions = []; return { status: "delivered" }; });
    window.__kzPreview.setCommand("run_prompt", () => null);
    return { project: a.path, foreign: snapshot.projects[1].path, process: a.lines[0].id, session: a.lines[0].session_id, otherSession: a.lines[1].session_id };
  });
  await emit("kz:turn", { step: 1 });
  await page.evaluate(async project => (await import("/12-workbench.js")).openProjectSpace(project, "project"), identity.project); await settle();
  await page.waitForFunction(() => Boolean(window.__case.releaseTrace));
  check((await module("tools").innerText()).includes("记录未加载"), "First overview distinguishes unloaded tool evidence from no calls");
  await page.evaluate(() => window.__case.releaseTrace()); await settle();
  check((await module("tools").innerText()).includes("webfetch"), "First overview backfills research that completed before the workspace existed");
  check((await module("tools").innerText()).includes("最近调用完成"), "Historical trace backfill is completed evidence without a live tool animation");
  await page.evaluate(() => {
    window.__case.delayTrace = true;
    window.__case.releaseTrace = null;
    window.__case.raceRows = [{ events: Array.from({ length: 75 }, (_, i) => ({ kind: "tool.completed", id: "older-" + i, name: "webfetch", ok: true }))
      .concat({ kind: "tool.completed", id: "current-read", name: "read", ok: true }) }];
  });
  await page.locator("#sw-refresh").click(); await page.waitForFunction(() => Boolean(window.__case.releaseTrace));
  await emit("kz:tool-start", { id: "current-read", name: "read", input: { path: "README.md" } });
  await page.evaluate(() => window.__case.releaseTrace()); await settle();
  check(await module("tools").getAttribute("data-state") === "working" && (await module("tools").innerText()).includes("调用中"), "A delayed history containing 75 older calls never evicts or completes a newer live call");
  await emit("kz:tool-end", { id: "current-read", name: "read", ok: true });
  await emit("kz:status", { stage: "权限", detail: "权限配置警告" });
  check((await page.locator("#sw-runtime").innerText()) === "等待模型", "Permission configuration notices never replace current execution activity");
  await emit("kz:tool-start", { id: "current-read", name: "read", input: { path: "README.md" } });
  check((await page.locator("#sw-runtime").innerText()) === "工具执行中", "Tool activity replaces an earlier permission stage immediately");
  await emit("kz:permission-resolved", { action: "read", result: "allow" });
  check((await page.locator("#sw-runtime").innerText()) === "工具执行中", "Permission resolution preserves an already resumed tool activity");
  await emit("kz:status", { stage: "压缩", detail: "超预算，压缩中段…" });
  check(await module("history").getAttribute("data-state") === "working", "An actual compaction in progress remains visible");
  await emit("kz:text", { text: "已恢复模型输出。" });
  check((await page.locator("#sw-runtime").innerText()) === "生成中" && await module("history").getAttribute("data-state") === "idle", "Model output clears a stale compaction stage and animation");
  await emit("kz:status", { stage: "压缩", detail: "上下文就地压缩为 24k，裁掉 12 条历史" });
  check((await page.locator("#sw-runtime").innerText()) === "等待模型" && await module("history").getAttribute("data-state") === "idle", "Completed compaction reports are historical facts instead of ongoing compaction");
  await page.evaluate(async () => {
    const core = await import("/01-core.js");
    window.__case.beforeWarnings = document.querySelectorAll("#log-lines .warn").length;
    for (let i = 0; i < 180; i++) core.handleExperienceEvent({ schema_version: 1, event_id: "case-task-" + i, session_id: window.__case.a.lines[0].session_id, event_type: "task_progressed", class: "delta", payload: { text: "progress" } });
    for (let i = 0; i < 36; i++) core.handleExperienceEvent({ schema_version: 1, event_id: "case-permission-" + i, session_id: window.__case.a.lines[0].session_id, event_type: "permission_resolved", class: "fact", payload: {} });
  });
  check(await page.evaluate(() => document.querySelectorAll("#log-lines .warn").length === window.__case.beforeWarnings), "216 legal task and permission experience events add no unknown-event warnings");
  check(await page.evaluate(async () => (await import("/01-core.js")).experienceEventIds.has("case-task-179")), "Unanimated legal experience events still enter the fact projection");
  await page.evaluate(async () => {
    (await import("/01-core.js")).handleExperienceEvent({ schema_version: 1, event_id: "case-unknown", session_id: window.__case.a.lines[0].session_id, event_type: "unexpected_future_event", class: "presentation", payload: {} });
  });
  check(await page.locator("#log-lines .warn").filter({ hasText: "unexpected_future_event" }).count() === 1, "A truly unknown event still reports a visible warning");
  await page.evaluate(async () => (await import("/05-chat-render.js")).addErrorMessage("HTTP 401 old-run-evidence", { retryable: true }));
  await emit("kz:turn", { step: 1 });
  check(await page.locator(".historical-error .error-level").filter({ hasText: "上次运行失败" }).count() === 1, "A new run labels the preceding error as a previous-run failure");
  check(await page.locator(".historical-error details").evaluate(el => !el.open) && await page.locator(".historical-error").getAttribute("data-raw").then(raw => raw.includes("old-run-evidence")), "Historical errors fold details while preserving exact source evidence");
  await module("batch").click(); await settle();
  check(await page.locator(".sw-batch-evidence .sw-evidence-link").count() === 2, "Tracker handoff and Git commit evidence appear without a download-file receipt");
  check((await page.locator(".sw-batch-evidence").innerText()).includes("任务验收以实际验证") && (await page.locator(".sw-deliveries").innerText()).includes("暂无交付文件"), "Batch declarations and downloadable-file receipts retain separate meanings");
  await page.locator(".sw-batch-evidence .sw-evidence-link").first().click();
  check((await page.locator(".sw-evidence").innerText()).includes("work_log") && (await page.locator(".sw-evidence").innerText()).includes("fd031b5"), "Batch record details expose the cited commit and source");
  await page.evaluate(() => { window.__case.questions = [{ projectDir: window.__case.a.path, sessionId: window.__case.a.lines[0].session_id, id: 701, revision: "1", question: "旧事项需要选择", options: ["继续"] }]; });
  await page.locator("#workbench-attention").click(); await settle();
  await page.locator(".sw-inbox-row").filter({ hasText: "旧事项需要选择" }).click();
  await page.getByRole("button", { name: "继续", exact: true }).click(); await settle();
  check(await page.locator(".sw-reply-complete").isVisible(), "The answered work message retains its delivery receipt");
  await page.evaluate(() => { window.__case.questions = [{ projectDir: window.__case.a.path, sessionId: window.__case.a.lines[0].session_id, id: 702, revision: "1", question: "新事项等待回答", options: [] }]; });
  await page.locator("#workbench-attention").click(); await settle();
  check(await page.locator(".sw-inbox-row").filter({ hasText: "新事项等待回答" }).count() === 1 && await page.locator(".sw-question").count() === 0, "Sidebar work-message navigation clears an answered interaction and displays new pending items");
  await page.evaluate(async project => (await import("/12-workbench.js")).openProjectSpace(project, "project"), identity.foreign); await settle();
  check((await module("tools").innerText()).includes("记录读取失败") && !(await module("tools").innerText()).includes("尚无调用"), "Unavailable trace never masquerades as an empty tool history");
  await page.evaluate(async project => (await import("/12-workbench.js")).openProjectSpace(project, "chat", { main: true }), identity.project); await settle();
  await page.locator("#project-overview-signal").click(); await settle();
  check(await page.locator("#view-project").getAttribute("class").then(value => value.split(/\s+/).includes("active")), "The dynamic project activity signal opens the actual project overview");
  await page.evaluate(async () => (await import("/28-async-workspace.js")).openAsyncWorkspace()); await settle();
  const floating = page.locator("#async-workspace"), grip = floating.locator(".async-head");
  await floating.waitFor();
  await floating.evaluate(async el => { await Promise.all(el.getAnimations().map(animation => animation.finished.catch(() => {}))); });
  check(await floating.getAttribute("data-kz-frame") === "async-workspace" && await grip.getAttribute("data-kz-frame-grip") === "", "Temporary Q&A uses the shared frame drag interface");
  const firstBox = await floating.boundingBox(), headBox = await grip.boundingBox();
  await page.mouse.move(headBox.x + 30, headBox.y + headBox.height / 2); await page.mouse.down();
  await page.mouse.move(headBox.x - 170, headBox.y + headBox.height / 2 - 120, { steps: 8 }); await page.mouse.up();
  const movedBox = await floating.boundingBox();
  check(Math.abs(movedBox.x - (firstBox.x - 200)) < 2 && Math.abs(movedBox.y - (firstBox.y - 120)) < 2, "Temporary Q&A follows a real pointer drag through the shared frame");
  await page.mouse.move(movedBox.x + 30, movedBox.y + 20); await page.mouse.down(); await page.mouse.move(0, 0, { steps: 8 }); await page.mouse.up();
  const clampedBox = await floating.boundingBox();
  check(clampedBox.x >= 8 && clampedBox.y >= 8 && !await page.evaluate(() => document.documentElement.dataset.kzFrameDrag), "Temporary Q&A clamps to viewport edges and clears the drag state on release");
  await grip.dblclick({ position: { x: 30, y: 20 } }); await settle();
  check(await floating.getAttribute("data-kz-placed") === null, "Double-clicking the temporary Q&A title resets its shared frame position");
  await floating.getByRole("button", { name: "收起", exact: true }).click();
  check(!await floating.isVisible(), "The temporary Q&A title close button remains clickable without starting a drag");
  await page.evaluate(async () => (await import("/28-async-workspace.js")).openAsyncWorkspace()); await settle();
  await page.setViewportSize({ width: 800, height: 700 }); await settle();
  check(await floating.evaluate(el => { const r = el.getBoundingClientRect(); return r.left >= 8 && r.top >= 8 && r.right <= window.innerWidth - 8 && r.bottom <= window.innerHeight - 8; }), "Temporary Q&A remains reachable after a viewport resize");
  await floating.getByRole("button", { name: "收起", exact: true }).click(); await page.setViewportSize({ width: 1440, height: 960 });
  check(errors.length === 0, `No browser errors: ${errors.join("; ")}`);
  await page.screenshot({ path: `${output}/trace-unavailable.png` });
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ checks, errors }, null, 2));
  console.log(`${checks.length} case reliability checks passed`);
} catch (error) {
  await page.screenshot({ path: `${output}/failure.png` }).catch(() => {});
  console.error({ checks, errors }); throw error;
} finally { await browser.close(); await server.close(); }
