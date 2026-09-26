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
let fail_library = false;
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
const project_b = "C:/project-b";
const library_entries = source_topics.map((entry) => ({ ...entry, id: entry.topic || "legacy", storage_root: project, linked_projects: [project], available: true }));
library_entries.push({ id: "other-alpha", topic: "alpha-study", label: "另一个 Alpha", kind: "research", storage_root: project_b, linked_projects: [project_b], available: true });
payloads.research_library_list = () => {
  if (fail_library) throw new Error("课题登记表读取失败");
  return { entries: library_entries, diagnostics: [] };
};
let delayed_plan = null;
let delayed_snapshot = null;
const workflows = new Map();
payloads.research_workflow_get = ({ topic }) => workflows.get(topic) ?? null;
payloads.file_preview = ({ path: file }) => {
  assert.ok(file?.startsWith(".kanzei/research/new-topic/"), "研究产物必须传递当前课题的真实文件路径");
  return { binary: false, size: 24, content: `Artifact ${file}`, truncated: false };
};
payloads.research_workflow_start = ({ topic, budget, maxMvpRuns }) => {
  assert.ok(budget.max_rounds > 0 && maxMvpRuns >= 2);
  const state = { topic, revision: 1, stage: "survey", paused: false, waiting_reason: null,
    directions: [], selected_direction: null, mvp: null, result_ids: [], budget, max_mvp_runs: maxMvpRuns };
  workflows.set(topic, state);
  return state;
};
payloads.research_workflow_update = ({ topic, revision, action, direction, maxMvpRuns }) => {
  const state = workflows.get(topic);
  assert.equal(revision, state.revision);
  if (action === "select") { state.selected_direction = direction; state.stage = "design_mvp"; }
  if (action === "pause") state.paused = true;
  if (action === "resume") { state.paused = false; state.waiting_reason = null; }
  if (action === "budget") state.max_mvp_runs = maxMvpRuns;
  if (action === "revise_full") {
    state.full_rounds = [...(state.full_rounds || []), { round: 1, artifact_root: "rounds/full-001-r20", reason: "用户要求补充完整实验", plan: state.full_plan, results: state.full_results, analysis: state.analysis, paper: state.paper }];
    state.stage = "plan_full";
    state.full_plan = null;
    state.full_results = [];
    state.analysis = null;
    state.paper = null;
  }
  state.revision += 1;
  return state;
};
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
      } else if (cmd === "research_library_create") {
        if (fail_create) throw new Error("课题标识已存在");
        value = { id: args.topic, topic: args.topic, label: args.title, kind: "research", storage_root: `C:/research-workspaces/${args.topic}`, linked_projects: [], standalone: true, available: true, sources: [], findings: [], runs: [], report: false };
        source_topics.push(value);
        library_entries.push(value);
      } else if (cmd === "research_library_link_projects") {
        value = library_entries.find((entry) => entry.id === args.id);
        value.linked_projects = args.projects;
      } else if (cmd === "process_list") {
        value = payloads.process_list.filter((item) => (item.origin_project || item.project_dir || project) === args.projectDir);
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
      } else if (cmd === "docs_snapshot") {
        value = { ...payloads.docs_snapshot, research_topics: args.projectDir === project_b
          ? [{ topic: "alpha-study", label: "另一个 Alpha", kind: "research", sources: [], findings: [], runs: [], report: false }]
          : source_topics.filter((entry) => (entry.storage_root || project) === args.projectDir) };
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
  page.on("pageerror", (error) => errors.push(error.stack || error.message));
  page.on("response", (response) => {
    if (response.status() >= 400 && response.url().includes("/vendor/monaco/")) errors.push(`HTTP ${response.status()} ${response.url()}`);
  });
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
  fail_library = true;
  await page.locator('[data-workspace="research"]').click();
  await page.waitForFunction(() => document.querySelector("#log-panel").textContent.includes("课题登记表读取失败"));
  assert.equal(await page.locator("body").getAttribute("data-space"), "dev");
  fail_library = false;
  await space("research");
  await page.waitForFunction(() => document.querySelector("#research-topic-select").options.length === 3);
  assert.equal(await page.locator("#project-switch").isVisible(), false);
  // UI2-0926 #1:侧栏「项目」分区已删(项目切换只剩项目卡菜单);D-170 隔离告警住在 dev 专属的 .project-warn-slot,研究空间里整个槽隐藏。
  assert.equal(await page.locator("#projects-section").count(), 0);
  assert.equal(await page.locator(".project-warn-slot").evaluate((el) => el.classList.contains("hidden")), true);
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
  for (const [category, expected] of [["dev_recon", "dev-survey"], ["unclassified", "unknown-old"], ["legacy", "legacy"], ["research", "alpha-study"]]) {
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
  assert.equal(await page.evaluate(async () => (await import("./03-shell.js")).currentProject), "C:/research-workspaces/new-topic");
  assert.equal(calls.filter(({ cmd }) => ["projects_add", "projects_init", "projects_select"].includes(cmd)).length, 0);
  await page.locator(".research-topic-links summary").click();
  await page.locator(".research-topic-links input[type=checkbox]").check();
  await page.waitForTimeout(1100);
  assert.equal(await page.locator(".research-topic-links input[type=checkbox]").isChecked(), true);
  await page.getByRole("button", { name: "保存关联", exact: true }).click();
  await page.waitForFunction(() => document.querySelector(".research-topic-links p")?.textContent.includes("smoke"));
  assert.equal(await page.evaluate(async () => (await import("./03-shell.js")).currentProject), "C:/research-workspaces/new-topic");
  await research_page("plan");
  assert.equal(await page.locator("#research-plan-panel").isVisible(), true);
  await research_page("report");
  assert.match(await page.locator("#research-report").textContent(), /尚未生成报告/);
  // AUTO research: user launch, durable direction wait, explicit choice and topic-bound continuation.
  await research_page("overview");
  await page.locator('#research-auto-panel input[name="rounds"]').fill("2");
  await page.waitForTimeout(1200);
  assert.equal(await page.locator('#research-auto-panel input[name="rounds"]').inputValue(), "2", "轮询不能重置用户预算草稿");
  await page.getByRole("button", { name: "启动 AUTO research", exact: true }).click();
  await page.waitForFunction(() => document.querySelector("#view-chat").classList.contains("active"));
  await page.waitForTimeout(100);
  assert.ok(calls.some(({ cmd, args }) => cmd === "research_workflow_start" && args.topic === "new-topic" && args.budget.max_rounds === 2));
  assert.ok(calls.some(({ cmd, args }) => cmd === "run_prompt" && args.profile === "research" && args.researchTopic === "new-topic" && args.prompt.includes("AUTO research")));
  const auto_state = workflows.get("new-topic");
  Object.assign(auto_state, { stage: "choose_direction", revision: 3, survey: "survey.md", map: "research-map.md",
    directions: [{ id: "staleness", title: "过时记忆的影响", question: "固定预算下过时记忆是否降低成功率？", rationale: "已有证据待对照", uncertainty: "需要最小实验", cost: "两次本地运行", validation: "固定预算对照", source_ids: ["S-001"] }] });
  await page.reload({ waitUntil: "networkidle" });
  await research_page("overview");
  await page.waitForFunction(() => document.querySelector('[data-direction="staleness"]'));
  assert.equal(workflows.get("new-topic").selected_direction, null);
  await page.screenshot({ path: path.join(artifact_root, "auto-research-map.png") });
  const before_selection = calls.filter(({ cmd }) => cmd === "run_prompt").length;
  await page.getByRole("button", { name: "选择并继续", exact: true }).click();
  await page.waitForFunction(() => document.querySelector("#view-chat").classList.contains("active"));
  await page.waitForTimeout(100);
  assert.equal(workflows.get("new-topic").selected_direction, "staleness");
  assert.ok(calls.filter(({ cmd }) => cmd === "run_prompt").length > before_selection);
  assert.equal(calls.filter(({ cmd }) => cmd === "run_prompt").at(-1).args.researchTopic, "new-topic");
  await research_page("overview");
  await page.getByRole("button", { name: "本轮后暂停研究", exact: true }).click();
  await page.waitForFunction(() => document.querySelector("#research-auto-panel")?.textContent.includes("研究已暂停"));
  await page.locator("#research-auto-panel details summary").click();
  await page.locator('#research-auto-panel details input[type="number"]').fill("6");
  const before_budget = calls.filter(({ cmd }) => cmd === "run_prompt").length;
  await page.getByRole("button", { name: "更新实验预算", exact: true }).click();
  await page.waitForFunction(() => document.querySelector("#research-auto-panel details summary")?.textContent.endsWith(": 6"));
  assert.equal(workflows.get("new-topic").max_mvp_runs, 6);
  assert.equal(calls.filter(({ cmd }) => cmd === "run_prompt").length, before_budget, "只调预算不能启动实验");
  await page.reload({ waitUntil: "networkidle" });
  await page.waitForFunction(() => document.querySelector("#research-auto-panel")?.textContent.includes("研究已暂停"));
  assert.equal(workflows.get("new-topic").stage, "design_mvp");
  await topic("beta-study");
  assert.equal(await page.locator('[data-direction="staleness"]').count(), 0);
  await topic("new-topic");
  assert.match(await page.locator("#research-auto-panel").textContent(), /研究已暂停/);
  await page.getByRole("button", { name: "继续研究", exact: true }).click();
  await page.waitForTimeout(100);
  assert.equal(workflows.get("new-topic").paused, false);
  // Full workflow: later phases, experiment progress and delivery entry points stay visible.
  Object.assign(auto_state, { stage: "run_full", revision: 12, compute: { kind: "local", snapshot: { gpus: [{ name: "RTX test GPU" }] } }, full_plan: { protocol: "full-protocol.md", experiments: [{ id: "b0" }, { id: "m0" }] }, full_results: [{ experiment_id: "b0", result_id: "E-001-03" }] });
  await research_page("overview");
  await page.waitForFunction(() => document.querySelector("#research-auto-panel")?.textContent.includes("1 / 2"));
  assert.match(await page.locator("#research-auto-panel").textContent(), /RTX test GPU/);
  Object.assign(auto_state, { stage: "completed", revision: 20, verdict: "rejected", full_results: [{ experiment_id: "b0", result_id: "E-001-03" }, { experiment_id: "m0", result_id: "E-001-04" }], analysis: "analysis.md", paper: { tex: "latex/paper.tex", pdf: "latex/paper.pdf", manifest: "delivery.json" } });
  await page.reload({ waitUntil: "networkidle" });
  await research_page("overview");
  await page.getByRole("button", { name: "打开论文 PDF", exact: true }).waitFor();
  assert.equal(await page.getByRole("button", { name: "论文源码", exact: true }).count(), 1);
  assert.equal(await page.getByRole("button", { name: "交付清单", exact: true }).count(), 1);
  assert.equal(await page.getByRole("button", { name: "继续研究", exact: true }).count(), 0);
  await page.screenshot({ path: path.join(artifact_root, "auto-research-completed.png") });
  await page.getByRole("button", { name: "打开论文 PDF", exact: true }).click();
  await page.waitForFunction(() => document.querySelector(".research-workspace").dataset.page === "writing" && !document.querySelector("#research-latex-pdf").hidden);
  assert.equal(calls.filter(({ cmd }) => cmd === "research_latex_pdf").at(-1).args.pdfPath, ".kanzei/research/new-topic/latex/paper.pdf");
  for (const [label, file] of [["阅读调研", "survey.md"], ["论文源码", "latex/paper.tex"], ["交付清单", "delivery.json"]]) {
    await research_page("overview");
    await page.getByRole("button", { name: label, exact: true }).click();
    await page.waitForFunction((path) => document.body.dataset.view === "files" && document.querySelector("#files-preview-path").textContent === path, `.kanzei/research/new-topic/${file}`);
    await page.waitForFunction(() => document.querySelector("#files-editor .monaco-editor"));
    await page.waitForFunction((content) => globalThis.monaco.editor.getModels().some((model) => model.getValue() === content), `Artifact .kanzei/research/new-topic/${file}`);
    assert.equal(calls.filter(({ cmd }) => cmd === "file_preview").at(-1).args.path, `.kanzei/research/new-topic/${file}`);
  }
  await research_page("overview");
  await page.getByRole("button", { name: "补充实验", exact: true }).click();
  await page.waitForFunction(() => document.querySelector("#view-chat").classList.contains("active"));
  assert.equal(workflows.get("new-topic").stage, "plan_full");
  assert.equal(workflows.get("new-topic").full_rounds[0].results.length, 2);
  await research_page("overview");
  await page.getByText("历次完整实验", { exact: true }).click();
  assert.match(await page.locator("#research-auto-panel").textContent(), /完整实验轮次: 2/);
  await page.locator(".research-auto-history").getByRole("button", { name: "综合分析", exact: true }).click();
  await page.waitForFunction(() => document.querySelector("#files-preview-path").textContent.endsWith("rounds/full-001-r20/analysis.md"));
  await research_page("overview");
  const history = page.locator(".research-auto-history");
  if (!(await history.evaluate((element) => element.open))) await history.locator("summary").click();
  await history.getByRole("button", { name: "打开论文 PDF", exact: true }).click();
  await page.waitForFunction(() => document.querySelector(".research-workspace").dataset.page === "writing" && !document.querySelector("#research-latex-pdf").hidden);
  assert.equal(calls.filter(({ cmd }) => cmd === "research_latex_pdf").at(-1).args.pdfPath, ".kanzei/research/new-topic/rounds/full-001-r20/latex/paper.pdf");
  await research_page("overview");
  await topic("alpha-study");
  // 旧项目完整快照迟到时，不得覆盖新项目的材料、标题或报告。
  await page.evaluate(async () => (await import("./03-shell.js")).navigate_view("memory"));
  let release_snapshot;
  delayed_snapshot = { promise: new Promise((resolve) => { release_snapshot = resolve; }) };
  const stale_snapshot = page.evaluate(async () => (await import("./19-research.js")).refreshResearch());
  await page.waitForTimeout(80);
  await topic("other-alpha");
  assert.equal(await page.evaluate(async () => (await import("./03-shell.js")).currentProject), project_b);
  release_snapshot();
  await stale_snapshot;
  assert.equal(await page.locator("#research-topic-select").inputValue(), "other-alpha");
  assert.equal(await page.locator("#research-cards .research-card").count(), 0);
  await research_page("chat");
  assert.equal(calls.filter(({ cmd }) => cmd === "process_create").at(-1).args.projectDir, project_b);
  await space("dev");
  assert.equal(await page.evaluate(async () => (await import("./03-shell.js")).currentProject), project);
  await space("research");
  assert.equal(await page.locator("#research-topic-select").inputValue(), "other-alpha");
  await topic("alpha-study");
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
  // ── 分区:侧栏与需求页 ──
  // UI2-0926 #1#5 复核:假 DOM 量不到版面,这三处对齐/落点在真浏览器里量。新开一页(独立 localStorage),开发空间。
  //  ① 需求页状态列:英文 In progress 比中文状态词宽,列宽不够时 flex 项被内容撑宽,这一行标题右移——各行标题左缘
  //     必须全等、状态词不被截断,展开的详情内容与标题左缘对齐(--doc-title-inset 与状态列同一个变量);
  //  ② 项目菜单与项目卡两缘对齐(surface 层的横向偏移归零);
  //  ③ 侧栏「查看全部隔离工作树 →」落在线路页的工作树清单顶端,而不是被随后画出的线路卡推到半路;
  //  ④ 侧栏「各线当前在做」一行式的线:线路名很长时让位的是左边的身份,右边「未取得条目」(英文更长)不被截断。
  {
    const saved_workspace_state = workspace_state;
    const saved_worktrees = payloads.worktree_list;
    const saved_settings = payloads.settings_get;
    const bg_process = payloads.process_list.find((item) => item.id === "p|bg");
    const saved_bg_label = bg_process?.label;
    if (bg_process) bg_process.label = "一条名字很长很长的并行线路,用来挤占焦点区一行式的宽度";
    workspace_state = {};
    payloads.worktree_list = Array.from({ length: 9 }, (_, index) => ({
      path: `C:/smoke/wt-${index}`, branch: `kanzei/wt-${index}`, clean: index % 3 !== 0,
      files: index % 3 ? [] : ["a.rs"], diff: "", bound_process: null,
    }));
    const sd_page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
    sd_page.on("pageerror", (error) => errors.push(error.stack || error.message));
    await sd_page.addInitScript(() => {
      globalThis.__TAURI__ = { core: { invoke: async (cmd, args) => {
        const result = await (await fetch("/ipc", { method: "POST", body: JSON.stringify({ cmd, args }) })).json();
        if (result.error) throw new Error(result.error);
        return result.value;
      } }, event: { listen: async () => () => {} } };
    });
    try {
      for (const language of ["en", "zh"]) {
        // 语言的持久化真源是全局配置(settings_get.language),启动时以它为准。
        payloads.settings_get = { ...saved_settings, language };
        if (language === "zh") await sd_page.reload({ waitUntil: "networkidle" });
        else await sd_page.goto(origin, { waitUntil: "networkidle" });
        await sd_page.waitForFunction((lang) => document.documentElement.lang === lang, language === "en" ? "en" : "zh-CN");
        await sd_page.locator('.activity-item[data-view="documents"]').click();
        await sd_page.waitForFunction(() => document.querySelectorAll("#documents-req-list .doc-row .title").length >= 2);
        for (const width of [1280, 1600, 2000]) {
          await sd_page.setViewportSize({ width, height: 720 });
          const rows = await sd_page.evaluate(() => [...document.querySelectorAll("#documents-req-list .doc-row")].map((row) => {
            const st = row.querySelector(".st");
            return { status: st?.textContent, title: row.querySelector(".title")?.getBoundingClientRect().left,
              stWidth: st?.getBoundingClientRect().width, truncated: st ? st.scrollWidth > st.clientWidth : null };
          }));
          assert.ok(rows.some((row) => /In progress|doing/.test(row.status)) && rows.some((row) => /To do|todo/.test(row.status)), `需求页夹具应同时有在做与待做:${JSON.stringify(rows)}`);
          assert.ok(rows.every((row) => Math.abs(row.title - rows[0].title) < 0.5),
            `${language} ${width}px 需求页各行标题左缘必须全等(状态列被状态词撑宽了):${JSON.stringify(rows)}`);
          assert.ok(rows.every((row) => row.truncated === false), `${language} ${width}px 状态词被截断(状态列宽不够):${JSON.stringify(rows)}`);
        }
        const compact = await sd_page.evaluate(() => [...document.querySelectorAll("#focus-body .line-focus-compact")].map((section) => {
          const head = section.querySelector(".line-focus-head");
          const empty = section.querySelector(".line-focus-empty");
          return { head: head?.textContent, headTruncated: head ? head.scrollWidth > head.clientWidth : null,
            empty: empty?.textContent, emptyTruncated: empty ? empty.scrollWidth > empty.clientWidth : null };
        }));
        assert.ok(compact.some((row) => row.headTruncated), `④ 前置:应有一条线路名长到被截断的一行式线:${JSON.stringify(compact)}`);
        assert.ok(compact.every((row) => row.emptyTruncated === false), `${language} 一行式线的「未取得条目」被截断了(该让位的是左边的线路身份):${JSON.stringify(compact)}`);
        const row = sd_page.locator('#documents-req-list .doc-item[data-doc-id="R-001"] .doc-row');
        if (await row.getAttribute("aria-expanded") !== "true") await row.click();
        const inset = await sd_page.evaluate(() => {
          const item = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]');
          return { title: item.querySelector(".doc-row .title").getBoundingClientRect().left, detail: item.querySelector(".doc-detail").getBoundingClientRect().left };
        });
        assert.ok(Math.abs(inset.title - inset.detail) < 0.5, `${language} 详情内容应与标题左缘对齐:${JSON.stringify(inset)}`);
        // 键盘:Tab 到勾选框按空格要真勾上(行的 keydown 不吞),详情不跟着开合。
        const pick = sd_page.locator('#documents-req-list .doc-item[data-doc-id="R-002"] .doc-pick');
        const expanded_before = await sd_page.locator('#documents-req-list .doc-item[data-doc-id="R-002"] .doc-row').getAttribute("aria-expanded");
        await pick.focus();
        await sd_page.keyboard.press("Space");
        assert.equal(await pick.isChecked(), true, `${language} 勾选框聚焦后按空格没勾上(被行的 keydown 吞了)`);
        assert.equal(await sd_page.locator('#documents-req-list .doc-item[data-doc-id="R-002"] .doc-row').getAttribute("aria-expanded"), expanded_before, `${language} 勾选框上按空格不该开合详情`);
        assert.equal(await sd_page.evaluate(() => document.querySelector("#documents-req-list").classList.contains("has-selection")), true);
        await sd_page.keyboard.press("Space");
        assert.equal(await pick.isChecked(), false);
      }
      await sd_page.setViewportSize({ width: 1280, height: 720 });
      // ② 项目菜单两缘与项目卡对齐。
      await sd_page.locator("#project-switch").click();
      await sd_page.waitForFunction(() => document.querySelector(".k-menu.project-menu")?.matches(":popover-open"));
      const menu_box = await sd_page.evaluate(() => {
        const card = document.querySelector("#project-switch").getBoundingClientRect();
        const menu = document.querySelector(".k-menu.project-menu").getBoundingClientRect();
        return { card: [card.left, card.right, card.bottom], menu: [menu.left, menu.right, menu.top] };
      });
      assert.ok(Math.abs(menu_box.menu[0] - menu_box.card[0]) <= 1 && Math.abs(menu_box.menu[1] - menu_box.card[1]) <= 1,
        `项目菜单左右缘应与项目卡对齐:${JSON.stringify(menu_box)}`);
      assert.ok(menu_box.menu[2] >= menu_box.card[2], `项目菜单应在项目卡下方:${JSON.stringify(menu_box)}`);
      await sd_page.keyboard.press("Escape");
      // ③ 「查看全部隔离工作树 →」落点。
      await sd_page.locator('.activity-item[data-view="chat"]').click();
      await sd_page.waitForFunction(() => document.querySelector("#view-chat").classList.contains("active"));
      await sd_page.locator("#worktree-list .worktree-more").click();
      await sd_page.waitForFunction(() => document.querySelector("#view-lines").classList.contains("active") && document.querySelectorAll("#lines-list .line-lane").length > 0);
      await sd_page.waitForTimeout(400);
      const landing = await sd_page.evaluate(() => {
        const scroller = document.querySelector("#lines-scroll");
        return { target: document.querySelector("#lines-worktrees").getBoundingClientRect().top, scroller: scroller.getBoundingClientRect().top,
          scrollTop: scroller.scrollTop, maxScroll: scroller.scrollHeight - scroller.clientHeight };
      });
      assert.ok(landing.maxScroll > 200, `落点前置:线路页应足够长,能滚:${JSON.stringify(landing)}`);
      assert.ok(Math.abs(landing.target - landing.scroller) < 80, `「查看全部隔离工作树」应落在线路页工作树清单顶端,停在了半路:${JSON.stringify(landing)}`);
      await sd_page.screenshot({ path: path.join(artifact_root, "sidebar-worktrees-landing.png") });
    } finally {
      await sd_page.close();
      workspace_state = saved_workspace_state;
      payloads.settings_get = saved_settings;
      if (bg_process) bg_process.label = saved_bg_label;
      if (saved_worktrees === undefined) delete payloads.worktree_list;
      else payloads.worktree_list = saved_worktrees;
    }
  }
  // ── 分区:侧栏与需求页(完) ──
  // 没有开发项目也能进入研究并创建独立课题。
  payloads.projects_get = { current: null, projects: [], names: {} };
  library_entries.length = 0;
  workspace_state = {};
  const empty_page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  empty_page.on("pageerror", (error) => errors.push(error.stack || error.message));
  await empty_page.addInitScript(() => {
    globalThis.__TAURI__ = { core: { invoke: async (cmd, args) => {
      const result = await (await fetch("/ipc", { method: "POST", body: JSON.stringify({ cmd, args }) })).json();
      if (result.error) throw new Error(result.error);
      return result.value;
    } }, event: { listen: async () => () => {} } };
  });
  await empty_page.goto(origin, { waitUntil: "networkidle" });
  await empty_page.locator('[data-workspace="research"]').click();
  await empty_page.waitForFunction(() => document.body.dataset.space === "research" && document.querySelector("#research-overview").textContent.includes("开始一个研究课题"));
  const processes_before_empty = calls.filter(({ cmd }) => cmd === "process_create").length;
  await empty_page.locator("#new-chat").click();
  assert.equal(calls.filter(({ cmd }) => cmd === "process_create").length, processes_before_empty);
  await empty_page.locator("#research-topic-title").fill("无需项目的课题");
  await empty_page.locator("#research-topic-slug").fill("standalone-case");
  await empty_page.locator("#research-topic-submit").click();
  await empty_page.waitForFunction(() => document.querySelector("#research-topic-select").value === "standalone-case");
  await empty_page.locator('[data-research-page="chat"]').click();
  await empty_page.waitForFunction(() => document.querySelector("#view-chat").classList.contains("active"));
  assert.equal(calls.filter(({ cmd }) => cmd === "process_create").at(-1).args.projectDir, "C:/research-workspaces/standalone-case");
  assert.equal(calls.filter(({ cmd }) => ["projects_add", "projects_init", "projects_select"].includes(cmd)).length, 0);
  await empty_page.reload({ waitUntil: "networkidle" });
  await empty_page.waitForFunction(() => document.body.dataset.space === "research" && document.querySelector("#research-topic-select").value === "standalone-case");
  await empty_page.waitForFunction(() => document.querySelector("#view-chat").classList.contains("active"));
  await empty_page.locator('[data-research-page="overview"]').click();
  await empty_page.waitForFunction(() => document.querySelector("#view-research").classList.contains("active"));
  await empty_page.screenshot({ path: path.join(artifact_root, "independent-topic.png") });
  await empty_page.close();
  assert.deepEqual(errors, [], "浏览器未捕获错误");
  console.log("工作空间浏览器回归通过：无项目独立课题创建/重载、同名课题跨根隔离、可选关联、失败回退、AUTO research 与交付预览、开发会话恢复、迟到响应及 3 视口 × 6 页面布局。");
} finally {
  await browser.close();
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
}
