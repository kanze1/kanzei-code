// Regression for dense real-world queues and independent development / research routing.
/* global window, document, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = path.resolve("output/playwright/softwire/spaces");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
page.setDefaultTimeout(10000);
const checks = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
page.on("pageerror", error => errors.push(error.message));
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const calls = cmd => page.evaluate(cmd => window.__kzPreview.calls.filter(c => c.cmd === cmd).map(c => c.args), cmd);
const space = async value => {
  if (!await page.locator(`[data-workspace="${value}"]`).isVisible()) await page.locator("#rail-sidebar-toggle").click();
  await page.locator(`[data-workspace="${value}"]`).click(); await settle();
  await page.waitForFunction(value => document.body.dataset.space === value && !document.querySelector(`[data-workspace="${value}"]`).disabled, value);
};
try {
  await page.goto(`${server.origin}/?scene=workspace&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  const id = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures();
    const snapshot = f.commands.workspace_snapshot(), overview = f.commands.workspace_overview();
    const project = snapshot.projects[0].path;
    const originals = f.state.processes.filter(p => p.origin_project === project);
    originals.forEach((p, index) => { p.profile = index === 1 ? "dev" : "research"; p.running = false; p.research_topic = index === 1 ? null : "old-study"; });
    for (const data of [snapshot, overview]) {
      const p = data.projects[0];
      p.lines.forEach((line, index) => Object.assign(line, { profile: originals[index].profile, running: false, stage: "空闲", label: index === 1 ? "开发执行" : "旧研究会话" }));
      p.current_items = Array.from({length:40}, (_, i) => ({ id:`D-${504+i}`, kind:"defect", title:`配置双真源与自动推进计数器同步：第 ${i+1} 项需要检查和修复的长标题`, status:"fixing", priority:"P2", owner_lines:[], batches:{done:2,total:5} }));
      p.current_items_total = 40;
    }
    const entries = ["alpha", "beta"].map(topic => ({id:topic,topic,label:topic === "alpha" ? "长期记忆研究" : "工具协作研究",kind:"research",storage_root:`C:/research/${topic}`,standalone:true,available:true,linked_projects:[],sources:[],findings:[],runs:[]}));
    for (const e of entries) f.state.processes.push({id:`p|${e.id}`,session_id:`session-${e.id}`,label:e.label,profile:"research",research_topic:e.topic,origin_project:e.storage_root,project_dir:e.storage_root,running:false});
    window.__spaces = { f, snapshot, overview, project };
    // Polls must include processes created during this test, like the real IPC.
    // A frozen snapshot could erase the newly created executor on a timer tick.
    const liveSnapshot = data => {
      const value = structuredClone(data);
      for (const project of value.projects) for (const line of f.state.processes.filter(p => p.origin_project === project.path)) {
        if (!project.lines.some(p => p.id === line.id)) project.lines.push(structuredClone(line));
      }
      return value;
    };
    window.__kzPreview.setCommand("workspace_snapshot", () => liveSnapshot(snapshot));
    window.__kzPreview.setCommand("workspace_overview", () => liveSnapshot(overview));
    window.__kzPreview.setCommand("research_library_list", () => ({entries,diagnostics:[]}));
    window.__kzPreview.setCommand("run_prompt", () => null);
    await (await import("/12-docs-pages.js")).refreshWorkspace();
    return { project, research:originals[0].id, dev:originals[1].id };
  });
  await page.evaluate(async project => (await import("/12-workbench.js")).openProjectSpace(project, "project"), id.project); await settle();
  check((await page.locator(".sw-recipient").innerText()).includes("开发执行"), "Development ignores a research default process and selects a development line");
  check(!(await page.locator(".sw-run-mode").innerText()).includes("研究"), "Development controls never show a research mode");
  check(await page.locator("#sw-requirement").textContent() === "项目概览", "Unclaimed work is never presented as the active line's requirement");
  check(await page.locator("#softwire-workspace .sw-work-entry").count() === 40, "Dense requirement data stays available in the queue");
  check(await page.locator(".sw-network .sw-work-focus").count() === 0, "Requirement queue no longer controls graph geometry");
  for (const theme of ["dark", "light"]) for (const size of [{width:1280,height:720},{width:1280,height:800},{width:1440,height:900}]) {
    await page.setViewportSize(size);
    await page.evaluate(theme => document.documentElement.setAttribute("data-theme",theme),theme);
    await settle(); await page.waitForTimeout(250);
    const geometry = await page.evaluate(() => {
      const rect = el => el.getBoundingClientRect();
      const surface = rect(document.querySelector("#sw-surface")), composer = rect(document.querySelector("#composer"));
      const nodes = [...document.querySelectorAll(".sw-node, .sw-resources button")];
      const rows = document.querySelector("#softwire-workspace .sw-work-rows");
      return { visible:nodes.every(el => { const b=rect(el); return b.top>=surface.top && b.bottom<=surface.bottom+1 && b.bottom<=composer.top && b.left>=surface.left && b.right<=surface.right+1; }), scroll:rows.scrollHeight>rows.clientHeight, outer:document.querySelector("#sw-surface").scrollHeight<=document.querySelector("#sw-surface").clientHeight+1, border:getComputedStyle(document.querySelector("#softwire-workspace .sw-work-entry")).borderBottomStyle };
    });
    check(geometry.visible && geometry.outer, `${theme} ${size.width}x${size.height}: every module and resource stays above the composer without outer scrolling`);
    check(geometry.scroll && geometry.border === "solid", `${theme} ${size.width}: the grouped queue scrolls independently and entries retain visible separators ${JSON.stringify(geometry)}`);
    await page.screenshot({path:path.join(output,`dense-${theme}-${size.width}x${size.height}.png`)});
  }
  await page.locator("#softwire-workspace .sw-work-rows").evaluate(el => el.scrollTop = el.scrollHeight);
  check(await page.locator("#softwire-workspace .sw-work-entry").last().isVisible(), "The last requirement is reachable within its own scroll region");
  await page.locator("#prompt").fill("开发工作草稿");
  await space("research");
  check(await page.locator("#research-section").isVisible() && await page.locator("#research-overview").isVisible(), "Research opens its independent topic workspace and navigation");
  check(!await page.locator("#project-space-nav").isVisible() && !await page.locator("#softwire-workspace").isVisible() && !await page.locator("#sw-run-toggle").isVisible(), "Research excludes development requirements, network, batches and loop controls");
  check(await page.locator('[data-research-page="plan"]').count() === 0 && await page.locator("#research-category-select").count() === 0, "Research has no separate plan page or content-scope dropdown");
  for (const name of ["literature", "experiments", "report", "writing", "overview"]) {
    await page.locator(`[data-research-page="${name}"]`).click(); await settle();
    check(await page.locator(".research-workspace").getAttribute("data-page") === name, `Independent research page: ${name}`);
  }
  await page.screenshot({path:path.join(output,"research-overview.png")});
  await page.locator('[data-research-page="chat"]').click(); await settle();
  check(await page.locator("#prompt").inputValue() === "", "Research starts with its own draft");
  await page.locator("#prompt").fill("Alpha 课题草稿");
  await page.locator("#research-topic-select").selectOption("beta"); await settle();
  await page.locator('[data-research-page="chat"]').click(); await settle();
  check(await page.locator("#prompt").inputValue() === "", "Changing topics does not carry another topic's draft");
  await page.locator("#prompt").fill("Beta 课题草稿");
  await page.locator("#research-topic-select").selectOption("alpha"); await settle();
  await page.locator('[data-research-page="chat"]').click(); await settle();
  check(await page.locator("#prompt").inputValue() === "Alpha 课题草稿", "Returning to a topic restores its draft");
  await space("dev");
  check(await page.locator("body").getAttribute("data-view") === "chat", "Returning to development enters the main conversation");
  await page.locator('[data-work-surface="project"]').click(); await settle();
  check(await page.locator("#softwire-workspace").isVisible() && await page.locator("#prompt").inputValue() === "开发工作草稿", "Returning to development restores its overview and composer draft");
  check((await calls("run_prompt")).length === 0 && !(await calls("auto_state_update")).some(args => args.enabled), "Browsing spaces never starts a task or enables a loop");
  await page.locator("#send").click(); await settle();
  const first = (await calls("run_prompt")).at(-1);
  check(first.processId === id.dev && first.profile === "dev" && first.agent === "dev-pair" && first.projectDir === id.project && !first.researchTopic, "Development send is explicitly bound to the development process");
  // Reproduce the user's project: every existing process, including d|..., is research.
  await page.evaluate(async () => {
    const {f, snapshot, overview, project} = window.__spaces;
    for (const p of f.state.processes.filter(p => p.origin_project === project)) p.profile = "research";
    for (const data of [snapshot, overview]) for (const l of data.projects[0].lines) l.profile = "research";
    await (await import("/12-workbench.js")).openProjectSpace(project, "project");
  }); await settle();
  await page.locator("#sw-refresh").click(); await settle();
  await page.locator('[data-work-surface="project"]').click();
  await page.locator("#prompt").fill("新的开发任务");
  await page.locator("#send").click(); await settle();
  const next = (await calls("run_prompt")).at(-1), created = (await calls("process_create")).at(-1);
  check(created.profile === "dev" && next.processId !== id.research && next.processId !== id.dev && next.profile === "dev", "With only research sessions, send creates a separate development session");
  await page.evaluate(async processId => {
    const {f,snapshot,overview,project} = window.__spaces;
    const p = f.state.processes.find(p => p.id === processId); p.profile = "research";
    for (const data of [snapshot,overview]) data.projects[0].lines.push({...p,stage:"空闲"});
    await (await import("/12-workbench.js")).openProjectSpace(project, "project");
  }, next.processId); await settle();
  await page.locator("#sw-refresh").click(); await settle();
  const runsBeforeStart = (await calls("run_prompt")).length;
  await page.locator("#sw-run-toggle").click(); await settle();
  const enabled = (await calls("auto_state_update")).findLast(args => args.enabled);
  const createdLines = await page.evaluate(() => window.__spaces.f.state.processes.filter(p => p.origin_project === window.__spaces.project && p.profile === "dev"));
  check(createdLines.length === 1 && enabled?.sessionId === createdLines[0].session_id, "One-click loop start creates and enables a separate development session when only research exists");
  await page.locator("#sw-run-toggle").click(); await settle();
  await page.getByRole("button",{name:"关闭鞭挞",exact:true}).click(); await settle();
  await page.waitForTimeout(2300);
  check((await calls("run_prompt")).length === runsBeforeStart, "Pausing a newly created development loop cancels its first round");
  check(await page.evaluate(() => window.__spaces.f.state.processes.filter(p => p.origin_project === window.__spaces.project && p.id.startsWith("d|")).every(p => p.profile === "research")), "Existing research profiles remain unchanged");
  check(errors.length === 0, `No browser errors: ${errors.join("; ")}`);
  await writeFile(path.join(output,"acceptance.json"),JSON.stringify({passed:checks.length,checks,errors,boundary:"Edge browser with isolated IPC fixtures; no model or real project execution."},null,2));
  console.log(`Softwire spaces PASS: ${checks.length} checks`);
} catch (error) {
  console.error("Passed before failure:",checks);
  await page.screenshot({path:path.join(output,"failure.png")}); throw error;
} finally { await browser.close(); await server.close(); }
