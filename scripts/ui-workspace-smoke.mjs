// 开发/研究空间的真实浏览器回归；沿用运行时夹具，IPC 在本地模拟，不启动桌面程序。
import assert from "node:assert/strict";
import { readFile, mkdir } from "node:fs/promises";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { chromium } from "playwright-core";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const ui_root = path.join(root, "crates/kanzei-app/ui");
const artifact_root = path.join(root, "output/playwright/workspaces");
await mkdir(artifact_root, { recursive: true });
const smoke_source = await readFile(path.join(root, "scripts/ui-runtime-smoke.mjs"), "utf8");
const fixture = smoke_source.slice(smoke_source.indexOf('const PROJECT = '), smoke_source.indexOf('const invokeLog = []'));
const { payloads, project } = vm.runInNewContext(`${fixture}\n({payloads, project: PROJECT})`);
const calls = [];
let next_process = 0;
let workspace_state = {};
let fail_create = false;
payloads.project_root_info = { selected: project, resolved: project, shared: false };
payloads.conversation_get = ({ processId }) => [{ role: "user", parts: [{ type: "text", text: `历史对话 ${processId}` }] }];
payloads.process_list[0].running = true;
payloads.process_list[0].profile = "dev";
const source_topics = payloads.docs_snapshot.research_topics;
for (const topic of source_topics) topic.kind = "research";
source_topics.push(
  { topic: "dev-survey", label: "开发勘察", kind: "dev_recon", sources: [], findings: [], runs: [] },
  { topic: "unknown-old", label: "待分类报告", kind: "unclassified", sources: [], findings: [], runs: [] },
  { topic: null, legacy: true, kind: "legacy", label: "旧版平铺", sources: [], findings: [], runs: [] },
);
let delayed_plan = null;
let delayed_snapshot = null;
const original_plan_get = payloads.research_plan_get;
payloads.research_plan_get = (args) => args.topic === "beta-study"
  ? { exists: true, plan: { ...original_plan_get({ topic: "alpha-study" }).plan, topic: "beta-study", title: "Beta 计划", status: "approved" } }
  : original_plan_get(args);
const server = http.createServer(async (request, response) => {
  try {
    if (request.url === "/ipc") {
      let body = "";
      for await (const chunk of request) body += chunk;
      const { cmd, args = {} } = JSON.parse(body);
      calls.push({ cmd, args });
      let value = null;
      if (cmd === "process_create") {
        value = { id: `p|workspace-${++next_process}`, session_id: `session-workspace-${next_process}`, label: args.researchTopic || "研究对话", profile: args.profile, research_topic: args.researchTopic || null, running: false, project_dir: args.projectDir };
        payloads.process_list.push(value);
      } else if (cmd === "research_topic_create") {
        if (fail_create) throw new Error("课题标识已存在");
        value = { topic: args.topic, label: args.title, kind: "research", sources: [], findings: [], runs: [], report: false };
        source_topics.push(value);
      } else if (cmd === "ui_prefs_set") {
        if (args.workspace_state) workspace_state = args.workspace_state;
      } else if (cmd === "ui_prefs_get") {
        value = { ...payloads.ui_prefs_get, workspace_state };
      } else if (cmd === "docs_snapshot" && delayed_snapshot) {
        const pending = delayed_snapshot;
        delayed_snapshot = null;
        const captured = structuredClone(payloads.docs_snapshot);
        await pending.promise;
        value = captured;
      } else if (cmd === "docs_snapshot" && args.projectDir !== project) {
        value = { ...payloads.docs_snapshot, sources: [], findings: [], research_topics: [{ topic: "project-b-topic", label: "项目 B 课题", kind: "research", sources: [], findings: [], runs: [], report: false }] };
      } else if (cmd === "research_plan_get" && delayed_plan) {
        await delayed_plan.promise;
        throw new Error("旧课题请求失败");
      } else if (cmd in payloads) {
        const entry = payloads[cmd];
        value = typeof entry === "function" ? entry(args) : entry;
      }
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ value }));
      return;
    }
    const relative = decodeURIComponent(new URL(request.url, "http://localhost").pathname).replace(/^\/+/, "") || "index.html";
    const file = path.resolve(ui_root, relative);
    if (!file.startsWith(ui_root + path.sep)) throw new Error("无效静态资源路径");
    const bytes = await readFile(file);
    const mime = { ".js": "text/javascript", ".css": "text/css", ".html": "text/html", ".svg": "image/svg+xml" }[path.extname(file)] || "application/octet-stream";
    response.writeHead(200, { "Content-Type": `${mime}; charset=utf-8` });
    response.end(bytes);
  } catch (error) {
    response.writeHead(request.url === "/ipc" ? 200 : 404, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ error: String(error) }));
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch({ channel: "msedge", headless: true });
const errors = [];
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript(() => {
    globalThis.__TAURI__ = {
      core: { invoke: async (cmd, args) => {
        const response = await fetch("/ipc", { method: "POST", body: JSON.stringify({ cmd, args }) });
        const result = await response.json();
        if (result.error) throw new Error(result.error);
        return result.value;
      } },
      event: { listen: async () => () => {} },
    };
  });
  await page.goto(origin, { waitUntil: "networkidle" });
  const space = async (value) => {
    await page.locator(`[data-workspace="${value}"]`).click();
    await page.waitForFunction((value) => document.body.dataset.space === value && !document.querySelector(`[data-workspace="${value}"]`).disabled, value);
  };
  const topic = async (value) => {
    await page.locator("#research-topic-select").selectOption(value);
    await page.waitForFunction((value) => document.querySelector("#research-heading").textContent.includes(value.split("-")[0]) || document.querySelector("#research-topic-select").value === value, value);
    await page.waitForFunction(() => document.querySelector(".research-workspace").dataset.page === "overview");
  };
  const research_page = async (value) => {
    if (!await page.locator(`[data-research-page="${value}"]`).isVisible()) await page.locator("#rail-sidebar-toggle").click();
    await page.locator(`[data-research-page="${value}"]`).click();
    if (value === "chat") await page.waitForFunction(() => document.querySelector("#view-chat").classList.contains("active"));
    else await page.waitForFunction((value) => document.querySelector(".research-workspace").dataset.page === value, value);
  };
  assert.deepEqual(await page.locator("#profile-select option").evaluateAll((items) => items.map((item) => item.value)), ["dev-pair", "dev-auto"]);
  await page.locator("#prompt").fill("开发任务草稿");
  await page.evaluate(async () => { const s = await import("./03-shell.js"); s.setAttachments([{ name: "dev.png", media_type: "image/png", data: "dev-attachment" }]); });
  const original_dev = JSON.stringify(payloads.process_list.slice(0, 2));
  await space("research");
  await page.waitForFunction(() => document.querySelector("#research-topic-select").options.length === 2);
  assert.equal(await page.locator('#profile-select').isVisible(), false);
  assert.equal(await page.locator('#focus-section').isVisible(), false);
  assert.equal(await page.locator('.activity-item[data-view="lines"]').isVisible(), false);
  assert.equal(await page.locator('.activity-item[data-view="files"]').isVisible(), true);
  assert.equal(await page.locator('#research-overview').isVisible(), true);
  assert.equal(await page.locator('#view-research').evaluate((el) => el.parentElement.id), "main");
  await page.screenshot({ path: path.join(artifact_root, "research-overview.png") });
  await research_page("literature");
  await page.locator('#research-cards .research-card[data-doc-id="S-101"] .research-card-actions button').filter({ hasText: /^→/ }).click();
  await page.waitForTimeout(50);
  assert.ok(calls.some(({ cmd, args }) => cmd === "docs_update" && args.id === "S-101" && args.topic === "alpha-study"));
  await research_page("chat");
  assert.equal(await page.locator("#prompt").inputValue(), "");
  await page.locator("#prompt").fill("Alpha 课题草稿");
  await topic("beta-study");
  await research_page("chat");
  assert.equal(await page.locator("#prompt").inputValue(), "");
  await page.locator("#prompt").fill("Beta 课题草稿");
  await topic("alpha-study");
  await research_page("chat");
  assert.equal(await page.locator("#prompt").inputValue(), "Alpha 课题草稿");
  assert.match(await page.locator("#research-chat-context").textContent(), /alpha-study/);
  const alpha_process = payloads.process_list.find((item) => item.research_topic === "alpha-study");
  assert.match(await page.locator('#messages [data-active]').textContent(), new RegExp(alpha_process.id.replaceAll("|", "\\|")));
  const before_new = payloads.process_list.length;
  await page.locator("#new-chat").click();
  await page.waitForFunction(() => document.querySelector("#prompt").value === "");
  assert.equal(payloads.process_list.length, before_new + 1);
  assert.equal(payloads.process_list.at(-1).research_topic, "alpha-study");
  assert.equal(calls.filter(({ cmd }) => cmd === "conversation_clear").length, 0);
  await page.locator("#prompt").fill("请围绕当前课题继续研究");
  await page.locator("#send").click();
  await page.waitForFunction(() => document.querySelector("#prompt").value === "");
  assert.ok(calls.some(({ cmd, args }) => cmd === "run_prompt" && args.researchTopic === "alpha-study" && args.profile === "research"));
  await space("dev");
  assert.equal(await page.locator("#prompt").inputValue(), "开发任务草稿");
  assert.equal(await page.evaluate(async () => (await import("./03-shell.js")).attachments[0]?.data), "dev-attachment");
  assert.equal(await page.locator("#stop").isVisible(), true);
  assert.equal(JSON.stringify(payloads.process_list.slice(0, 2)), original_dev);
  assert.equal(calls.filter(({ cmd, args }) => cmd === "process_update" && args.profile).length, 0);
  assert.equal(calls.filter(({ cmd }) => cmd === "stop_run").length, 0);
  await space("research");
  await research_page("writing");
  await page.waitForFunction(() => document.querySelector("#research-latex-template").options.length === 4);
  await page.waitForFunction(() => document.querySelector("#research-latex-history").children.length > 0);
  await page.reload({ waitUntil: "networkidle" });
  await page.waitForFunction(() => document.body.dataset.space === "research" && document.querySelector(".research-workspace").dataset.page === "writing");
  assert.equal(await page.locator("#research-topic-select").inputValue(), "alpha-study");
  for (const [category, expected] of [["dev_recon", "dev-survey"], ["unclassified", "unknown-old"], ["legacy", ""], ["research", "alpha-study"]]) {
    await page.locator("#research-category-select").selectOption(category);
    await page.waitForFunction((expected) => document.querySelector("#research-topic-select").value === expected, expected);
  }
  // 请求失败晚于课题切换时，不得清掉新课题的计划。
  let release_plan;
  delayed_plan = { promise: new Promise((resolve) => { release_plan = resolve; }) };
  const old_plan = page.evaluate(async () => (await import("./19-research.js")).refreshResearchPlan());
  await page.waitForTimeout(80);
  delayed_plan = null;
  await topic("beta-study");
  await page.waitForFunction(() => document.querySelector("#research-report").textContent.includes("Beta report"));
  release_plan();
  await old_plan;
  assert.equal(await page.locator("#research-topic-select").inputValue(), "beta-study");
  assert.equal(await page.locator("#research-plan-status").textContent(), "已批准");
  await page.locator("#research-topic-new").click();
  await page.locator("#research-topic-title").fill("新建验证课题");
  await page.locator("#research-topic-slug").fill("new-topic");
  fail_create = true;
  await page.locator("#research-topic-submit").click();
  await page.waitForFunction(() => document.querySelector("#research-topic-error").textContent.includes("标识已存在"));
  fail_create = false;
  await page.locator("#research-topic-submit").click();
  await page.waitForFunction(() => document.querySelector("#research-topic-select").value === "new-topic" && document.querySelector("#research-topic-form").classList.contains("hidden"));
  await research_page("plan");
  assert.equal(await page.locator("#research-plan-panel").isVisible(), true);
  await research_page("report");
  assert.match(await page.locator("#research-report").textContent(), /尚未生成报告/);
  await topic("alpha-study");
  // 旧项目完整快照迟到时，不得覆盖新项目的材料、标题或报告。
  await page.evaluate(async () => (await import("./03-shell.js")).navigate_view("memory"));
  let release_snapshot;
  delayed_snapshot = { promise: new Promise((resolve) => { release_snapshot = resolve; }) };
  const stale_snapshot = page.evaluate(async () => (await import("./19-research.js")).refreshResearch());
  await page.waitForTimeout(80);
  await page.evaluate(async () => {
    const sessions = await import("./09-sessions.js");
    sessions.renderProjects({ current: "C:/project-b", projects: ["C:/project-b"], names: {} });
    await (await import("./19-research.js")).refreshResearch();
  });
  assert.equal(await page.locator("#research-topic-select").inputValue(), "project-b-topic");
  release_snapshot();
  await stale_snapshot;
  assert.equal(await page.locator("#research-topic-select").inputValue(), "project-b-topic");
  assert.equal(await page.locator("#research-report").textContent().then((text) => text.includes("Alpha report")), false);
  await page.evaluate(async (project) => {
    const sessions = await import("./09-sessions.js");
    sessions.renderProjects({ current: project, projects: [project], names: { [project]: "smoke" } });
    await sessions.refreshProcesses();
    await (await import("./19-research.js")).refreshResearch();
  }, project);
  for (const viewport of [{ width: 800, height: 600 }, { width: 1024, height: 720 }, { width: 1440, height: 960 }, { width: 800, height: 600 }, { width: 1440, height: 960 }]) {
    const was_overlay = await page.evaluate(() => globalThis.matchMedia("(max-width: 900px)").matches);
    await page.setViewportSize(viewport);
    // setViewportSize 返回时 matchMedia 的 change 事件仍可能在途；先验证窄屏自动收栏，
    // 再走用户展开侧栏的操作，避免刚检查可见就被断点处理收起。
    if (!was_overlay && viewport.width <= 900) {
      await page.waitForFunction(() => document.querySelector("#sidebar").classList.contains("collapsed")
        && document.querySelector("#rail-sidebar-toggle").getAttribute("aria-expanded") === "false");
    }
    for (const section of ["overview", "literature", "plan", "experiments", "report", "writing"]) {
      await research_page(section);
      const result = await page.evaluate(() => {
        const content = document.querySelector("#view-research");
        const box = content.getBoundingClientRect();
        const visible_sections = ["#research-overview", ".research-side", "#research-plan-panel", "#research-roadmap", "#research-report", "#research-latex"].filter((selector) => document.querySelector(selector).getBoundingClientRect().height > 0);
        return { width: box.width, right: box.right, bottom: box.bottom, overflow: document.documentElement.scrollWidth > globalThis.innerWidth, visible_sections };
      });
      assert.equal(result.overflow, false, `${section} 横向溢出 ${JSON.stringify(viewport)}`);
      assert.ok(result.width > 300 && result.right <= viewport.width + 1 && result.bottom <= viewport.height + 1, JSON.stringify(result));
      assert.equal(result.visible_sections.length, 1, `${section} 页面叠放: ${result.visible_sections}`);
    }
    await page.screenshot({ path: path.join(artifact_root, `writing-${viewport.width}.png`) });
  }
  assert.deepEqual(errors, [], "浏览器未捕获错误");
  console.log("工作空间浏览器回归通过：开发运行保持、课题会话/草稿/附件隔离、刷新恢复、内容分类、延迟响应、创建成功/失败及 3 视口 × 6 页面布局。");
} finally {
  await browser.close();
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}
