// Real browser + production UI. Loop requests use an isolated IPC fixture.
/* global window, document */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
import { selectWork, plainText, visibleReply, latestSentence, sparkPath, usageSeries, usageSummary, readableText } from "../crates/kanzei-app/ui/25-softwire-model.js";

const output = path.resolve("output/playwright/softwire/overview");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
page.setDefaultTimeout(10000);
const passed = [], errors = [];
const check = (value, label) => { assert(value, label); passed.push(label); };
page.on("pageerror", error => errors.push(error.message));
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const count = cmd => page.evaluate(cmd => window.__kzPreview.calls.filter(c => c.cmd === cmd).length, cmd);
const last = cmd => page.evaluate(cmd => window.__kzPreview.calls.filter(c => c.cmd === cmd).at(-1)?.args, cmd);
const overview = async () => {
  if (!await page.locator('.workbench-project-link.active').isVisible()) await page.locator("#rail-sidebar-toggle").click();
  await page.locator('.workbench-project-link.active').click(); await settle();
  await page.locator('[data-work-surface="project"]').click(); await settle();
};
try {
  const unclaimedLine = selectWork({lines:[{id:"a"},{id:"b"}],current_items:[{id:"R-1",owner_lines:[{id:"a"}]}],work_units:[{requirement_id:"R-1",claimed_by:"a",status:"active"}]},"b");
  check(unclaimedLine.item === null && unclaimedLine.unit === null, "Selecting an idle line never borrows another line's requirement or batch");
  check(plainText("## 标题\n**重点**与`代码`、[链接](docs/x.md)\n| a | b |\n|---|---|\n```js\nlet x\n```\n结论。") === "标题\n重点与代码、链接\n结论。", "Reply previews strip Markdown markers, tables and fenced code");
  check(plainText("已完成。\n\n<!-- handoff: scope=work_item -->\n**交接范围**:work_item\n**交接目标**:R-1\n\n结论:通过。") === "已完成。\n结论:通过。" && latestSentence("做完了。\n\n1. 交接范围:work_item\n2. 交接目标:R-1\n3. 证据:abc") === "做完了。" && visibleReply("<!-- handoff -->\n- 交接范围:work_item\n- 交接目标:R-1") === "", "Overview reply helpers share the chat page's handoff filter (comments, numbered fields, whole-reply blocks)");
  check(!/[CQ]/.test(sparkPath([1,3,2],100,20)) && sparkPath([1,3,2],100,20).split(" L").length === 3, "Usage waveforms are straight segments, never curves");
  check(usageSeries([{at:2,steps:5,inputTokens:10,outputTokens:1,tools:{read:2}},{at:1,steps:3,metrics:{}}]).map(row => row.at).join() === "1,2" && usageSummary(usageSeries([{at:1,steps:4},{at:2,steps:6}])).steps === 5, "Usage rounds are drawn oldest to newest and averaged");
  check(!/[{}"]/.test(readableText({summary:"完成",paths:["a.md"],nested:{handoff_target:"x"}})) && readableText({kind:"handoff"}) === "类别：交付记录", "Evidence details are readable text, not JSON");
  await page.goto(`${server.origin}/?scene=workspace&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  const identity = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures();
    for (const p of f.state.processes) p.running = false;
    const snapshot = f.commands.workspace_snapshot(), overview = f.commands.workspace_overview();
    for (const data of [snapshot, overview]) for (const p of data.projects) for (const line of p.lines) { line.running = false; line.stage = "空闲"; }
    window.__overviewTest = { f, snapshot, overview, fail: false };
    window.__kzPreview.setCommand("workspace_snapshot", () => structuredClone(window.__overviewTest.snapshot));
    window.__kzPreview.setCommand("workspace_overview", () => structuredClone(window.__overviewTest.overview));
    window.__kzPreview.setCommand("run_prompt", () => null);
    window.__kzPreview.setCommand("auto_state_update", args => {
      if (window.__overviewTest.fail && args.enabled) throw "无法保存鞭挞状态";
      return {ok:true};
    });
    await (await import("/12-docs-pages.js")).refreshWorkspace();
    const a = snapshot.projects[0];
    return {project:a.path, process:a.lines[0].id, session:a.lines[0].session_id};
  });
  const processesBefore = await count("process_list");
  await page.evaluate(async project => (await import("/12-workbench.js")).openProjectSpace(project, "project"), identity.project); await settle();
  check(await count("run_prompt") === 0 && await count("process_list") === processesBefore, "Browsing the overview never activates or starts a session");
  check(await page.locator("#softwire-workspace .sw-work-entry").first().innerText().then(s => s.includes("R-379") && s.includes("2/5")), "Requirement, priority and batch progress live in the overview");
  check(await page.locator("#softwire-workspace .sw-backlog tr").count() === 3, "Requirements and defects each show actionable and blocked counts");
  // E1:运行画像并成概览第 7 行「运行用量」,数据来自 run_metrics(夹具里是 14 轮造数据)。
  const usage = page.locator('[data-module="usage"]');
  check((await usage.innerText()).includes("近 14 轮") && await count("run_metrics") >= 1, "The usage lane summarizes the latest rounds from run_metrics");
  check(/^M[\d. -]+( L[\d. -]+)+$/.test(await page.locator(".sw-spark path").first().getAttribute("d")), "The usage waveform is straight segments, never curves");
  check(await page.locator("#sw-usage-detail").isHidden() && await usage.getAttribute("aria-expanded") === "false", "Usage details start collapsed");
  await usage.click();
  check(await page.locator("#sw-usage-detail").isVisible() && await page.locator("#sw-usage-detail .sw-usage-chart").count() === 3 && await page.locator("#sw-usage-detail .sw-usage-rounds li").count() === 8 && await usage.getAttribute("aria-expanded") === "true", "Opening the usage lane shows steps, token and call curves plus the last eight rounds");
  await usage.click();
  check(await page.locator("#sw-usage-detail").isHidden(), "The usage lane collapses again");
  // UX-008/037:概览的「最新一句」「最近回复」与对话页同源过滤内部交接字段(HTML 注释、交接范围/目标/验收标准/证据),
  // 中间带字段块与以字段块结尾两种形态都不得露出,留下的人话与代码块/正常 Markdown 保持原样。
  const leaky = [
    "已完成 R-364。\n\n<!-- handoff: scope=work_item target=R-364 -->\n**交接范围**:work_item\n**交接目标**:R-364\n**验收标准**:常驻层 20 个工具\n**证据**:commit 21c36e9d\n\n结论:测试通过。",
    "已完成 R-365。\n\n1. 交接范围:work_item\n2. 交接目标:R-365\n3. 验收标准:ok\n4. 证据:commit 21c36e9e",
  ];
  const leakSession = identity.session;
  for (const text of leaky) {
    await page.evaluate(async ({session, text}) => {
      const emit = window.__kzPreview.emit;
      await emit("kz:turn", {sessionId: session, step: 1, maxSteps: 0});
      await emit("kz:text", {sessionId: session, text});
      await emit("kz:done", {sessionId: session, steps: 1, halted: false, history: 1, elapsedMs: 10, input: 1, output: 1, cacheRead: 0, cacheWrite: 0, tools: {}, autoAction: {type: "NoContinue"}});
      await emit("kz:idle", {sessionId: session, reason: "completed"});
    }, {session: leakSession, text}); await settle();
    const shown = await page.locator("#sw-surface .sw-recent").innerText();
    check(!/交接范围|交接目标|验收标准|证据|handoff|<!--/.test(shown) && /已完成 R-36[45]/.test(shown), "The overview's recent replies hide the internal handoff fields but keep the human sentence");
  }
  check(await page.locator("#sw-surface .sw-recent-reply").first().innerText().then(s => s.includes("已完成 R-365")), "A reply that ends with the field block still previews its human part");
  // UX-056:1333x695 下待办表不再压到下面的回复,每个按钮热区不小于 24px 且点得到。
  await page.setViewportSize({width:1333,height:695}); await settle();
  check(await page.evaluate(() => {
    const table = document.querySelector("#sw-surface .sw-backlog"), recent = document.querySelector("#sw-surface .sw-recent").getBoundingClientRect(), buttons = [...table.querySelectorAll("button")];
    buttons.at(-1).scrollIntoView({block:"nearest"});
    const box = buttons.at(-1).getBoundingClientRect(), hit = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2);
    return table.getBoundingClientRect().bottom <= recent.top + 1 && buttons.every(button => button.getBoundingClientRect().height >= 24) && buttons.at(-1).contains(hit);
  }), "At 1333x695 the backlog table neither overlaps the replies nor loses its click target");
  await page.setViewportSize({width:1440,height:960}); await settle();
  check(await page.locator("#project-tools").count() === 0, "No project-tools catch-all remains");
  check(await page.locator("[data-resource]").count() === 7, "Development resources have contextual links (conventions, memory graph, files, architecture, history, lines, usage profile); research has its own workspace");
  // UX-007:开发规范的入口在概览「需求」行右侧,点开就是规范对话框(不跳页)。
  check(await page.locator('[data-lane="work"] [data-resource="conventions"]').count() === 1, "The conventions entry sits on the requirement row of the overview");
  await page.locator('[data-resource="conventions"]').click(); await settle();
  check(await page.locator("#conventions-dialog").isVisible() && await count("conventions_read") >= 1 && await page.locator("body").getAttribute("data-view") === "project", "The conventions link opens the conventions dialog in place");
  await page.keyboard.press("Escape");
  check(await page.locator("#conventions-dialog").isHidden(), "Esc closes the conventions dialog");
  await page.locator("#prompt").fill("启动不应该带走这个草稿");
  await page.locator(".sw-run-mode").click();
  await page.getByRole("menuitem", {name:"自主推进",exact:true}).click(); await settle();
  check((await page.locator(".sw-run-mode").textContent()).includes("自主推进"), "Explicit loop mode selection stays visible");
  check(await page.locator("body").getAttribute("data-view") === "project", "Preparing execution keeps the user on the overview");
  check(await page.locator("#prompt").inputValue() === "启动不应该带走这个草稿", "Preparing a loop preserves the composer draft");
  await page.evaluate(() => { window.__overviewTest.fail = true; });
  await page.locator("#sw-run-toggle").click(); await settle();
  check((await page.locator(".sw-run-status").textContent()).includes("无法保存"), "Failed loop activation has a visible error");
  check(await count("run_prompt") === 0 && (await page.locator("#sw-run-toggle").textContent()).includes("启动"), "Failed activation neither starts nor falsely shows an enabled loop");
  await page.evaluate(() => { window.__overviewTest.fail = false; });
  await page.locator("#sw-run-toggle").click(); await settle();
  check((await last("auto_state_update")).sessionId === identity.session && (await last("auto_state_update")).enabled === true, "One-click start enables only the displayed session");
  await page.locator("#sw-run-toggle").click(); await settle();
  check((await last("auto_state_update")).paused === true, "Pause uses the existing per-line backend controller");
  await page.waitForTimeout(2300);
  check(await count("run_prompt") === 0, "Pausing during countdown cancels the scheduled first round");
  await page.locator("#sw-run-toggle").click();
  await page.waitForFunction(() => window.__kzPreview.calls.some(c => c.cmd === "run_prompt"));
  const run = await last("run_prompt");
  check(run.projectDir === identity.project && run.processId === identity.process && run.autonomous === true && run.agent === "dev", "Resume sends a real autonomous request for the visible project and mode");
  check(await count("run_prompt") === 1, "A single start produces one first-round request");
  check(await page.locator("#prompt").inputValue() === "启动不应该带走这个草稿", "The loop does not send or clear a user draft");
  await page.getByRole("button",{name:"关闭鞭挞",exact:true}).click(); await settle();
  check((await last("auto_state_update")).enabled === false, "Close disables future loop rounds");
  await page.getByRole("button",{name:"鞭挞设置",exact:true}).click(); await settle();
  check(await page.locator("#autorun-menu").isVisible(), "Loop settings open in place");
  await page.locator("#continue-toggle").click();
  check(await page.locator("#continue-prompt").isVisible(), "Continuation text remains editable in overview settings");
  await page.keyboard.press("Escape");
  for (const resource of ["files", "arch", "metrics", "memory", "lines", "chat"]) {
    await page.locator(`[data-resource="${resource}"]`).click(); await settle();
    check(await page.locator("body").getAttribute("data-view") === resource, `Contextual ${resource} link opens the correct project page`);
    await overview();
  }
  await page.getByRole("button", {name:"打开完整需求与缺陷列表",exact:true}).click(); await settle();
  check(await page.locator("body").getAttribute("data-view") === "documents", "The compact list opens the full requirement list");
  await overview();
  await page.getByRole("button",{name:"＋ 记缺陷",exact:true}).click();
  await page.locator(".sw-capture-form textarea").fill("概览按钮在窄屏下需要更清楚");
  await page.locator(".sw-capture-form").getByRole("button",{name:"记录",exact:true}).click(); await settle();
  const captured = await last("quick_req");
  check(captured.projectDir === identity.project && captured.kind === "defect" && captured.description === "概览按钮在窄屏下需要更清楚", "Quick capture addresses the overview project and selected document type");
  const runsBeforeSwitch = await count("run_prompt");
  const foreign = await page.evaluate(() => {
    const t = window.__overviewTest;
    window.__kzPreview.setCommand("projects_select", args => args.path !== t.snapshot.projects[0].path ? t.f.commands.projects_select(args) : new Promise(resolve => {
      t.releaseSelection = () => resolve(t.f.commands.projects_select(args));
    }));
    return t.snapshot.projects[1].path;
  });
  await page.locator("#sw-run-toggle").click();
  await page.waitForFunction(() => Boolean(window.__overviewTest.releaseSelection));
  if (!await page.locator(".workbench-project-link").first().isVisible()) await page.locator("#rail-sidebar-toggle").click();
  await page.locator(`.workbench-project-link[data-path="${foreign}"]`).click();
  await page.evaluate(() => window.__overviewTest.releaseSelection()); await settle();
  check(await page.evaluate(async path => document.body.dataset.view === "chat" && (await import("/12-workbench.js")).workbenchProject() === path, foreign), "Late execution preparation cannot pull the user back to a previous project");
  check(await count("run_prompt") === runsBeforeSwitch, "Leaving during start preparation does not launch the old or newly viewed project");
  await page.evaluate(() => window.__kzPreview.setCommand("projects_select", args => window.__overviewTest.f.commands.projects_select(args)));
  await page.locator(`.workbench-project-link[data-path="${identity.project}"]`).click(); await settle();
  await overview();
  await page.locator('[data-work-surface="project"]').click(); await settle();
  await page.screenshot({path:path.join(output,"desktop-light.png")});
  for (const width of [1024,390,320]) {
    await page.setViewportSize({width,height:850}); await settle();
    check(await page.locator("#softwire-workspace").evaluate(el => el.scrollWidth <= el.clientWidth + 1), `${width}px overview has no horizontal overflow`);
    check(await page.locator("#sw-run-toggle").isVisible(), `${width}px loop control remains reachable`);
    await page.screenshot({path:path.join(output,`width-${width}.png`)});
  }
  await page.setViewportSize({width:1440,height:960});
  await page.evaluate(() => document.documentElement.setAttribute("data-theme","dark"));
  await page.screenshot({path:path.join(output,"desktop-dark.png")});
  check(errors.length === 0, `No browser errors: ${errors.join("; ")}`);
  await writeFile(path.join(output,"acceptance.json"), JSON.stringify({passed:passed.length,checks:passed,errors,boundary:"Actual UI in Edge; isolated IPC fixtures; no real model or user-project execution."},null,2));
  console.log(`Softwire overview PASS: ${passed.length} checks`);
} catch (error) {
  await page.screenshot({path:path.join(output,"failure.png")});
  console.error("Passed before failure:",passed); throw error;
} finally { await browser.close(); await server.close(); }
