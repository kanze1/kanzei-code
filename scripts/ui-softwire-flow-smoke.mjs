// Interaction-order and live-state regression, production UI with isolated IPC.
/* global window, document, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = path.resolve("output/playwright/softwire/flow");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
page.setDefaultTimeout(10000);
const checks = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
page.on("pageerror", error => errors.push(error.message));
const settle = () => page.evaluate(() => window.__kzPreview.settle());
try {
  await page.goto(`${server.origin}/?scene=startup&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  check(await page.locator("body").getAttribute("data-view") === "chat", "Startup enters the last project's main conversation");
  const id = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures();
    const snapshot = f.commands.workspace_snapshot();
    const a = snapshot.projects[0], b = snapshot.projects[1];
    window.__flow = { f, snapshot, a, b };
    window.__kzPreview.setCommand("workspace_snapshot", () => structuredClone(snapshot));
    window.__kzPreview.setCommand("conversation_get", args => {
      if (window.__flow.failHistory) throw new Error("历史暂时不可用");
      return args.projectDir === a.path ? [
      {role:"assistant",text:"第一条：已读取需求，正在确认验收范围。"},
      {role:"user",text:"不要把用户消息混入最近回复。"},
      {role:"assistant",text:"第二条：**界面结构**已调整，`消息流`待验证。"},
      {role:"assistant",text:"第三条：当前批次验证通过，等待体验反馈。"},
    ] : [{role:"assistant",text:"另一个项目的独立回复。"}]; });
    return {a:a.path,b:b.path,session:a.lines[0].session_id,process:a.lines[0].id,other:a.lines[1].id};
  });
  const projectButton = p => page.locator(`.workbench-project-link[data-path="${p}"]`);
  await page.evaluate(() => {
    window.__kzPreview.setCommand("projects_select", args => window.__flow.gated ? window.__flow.f.commands.projects_select(args) : new Promise(resolve => {
      window.__flow.gated = true;
      window.__flow.release = () => resolve(window.__flow.f.commands.projects_select(args));
    }));
  });
  await projectButton(id.b).click();
  await page.waitForFunction(() => Boolean(window.__flow.release));
  check(await projectButton(id.b).getAttribute("data-loading") === "true" && await projectButton(id.b).getAttribute("data-activity") !== "starting", "Opening a project marks navigation busy without inventing an execution start");
  check(await projectButton(id.b).evaluate(el => getComputedStyle(el,"::after").content === "none" && getComputedStyle(el.querySelector(".workbench-project-activity")).animationName !== "workbench-spin"), "Page restoration never produces either the generic or the execution spinner");
  await page.locator("#workbench-home").click();
  await page.evaluate(() => window.__flow.release()); await settle();
  check(await page.locator("body").getAttribute("data-view") === "workspace" && await projectButton(id.b).getAttribute("aria-busy") === "false", "Late loading cannot pull navigation back or leave a stuck spinner");
  await page.evaluate(() => window.__kzPreview.setCommand("projects_select", args => window.__flow.f.commands.projects_select(args)));
  await projectButton(id.a).click(); await settle();
  check(await page.locator("body").getAttribute("data-view") === "chat", "Clicking a project enters its main conversation");
  await page.locator('[data-work-surface="project"]').click(); await settle();
  await page.locator('#sw-refresh').click(); await settle();
  check(!await page.locator("#sidebar #project-space-nav").count() && await page.locator("#sidebar #focus-section").count() === 0 && !await page.locator("#sidebar #live-section").isVisible(), "The development sidebar contains projects without duplicate local navigation, cards or conversations");
  check(!await page.locator("#statusbar .status-left").isVisible(), "Overview does not repeat another activated conversation's footer state");
  check(await page.locator(".sw-recent-reply").count() === 3, "Overview shows the last three assistant replies");
  check((await page.locator(".sw-recent-reply").first().innerText()).includes("第三条") && !(await page.locator(".sw-recent").innerText()).includes("用户消息"), "Newest answer comes first and user messages stay out of the preview");
  check((await page.locator(".sw-recent").innerText()).includes("第二条：界面结构已调整，消息流待验证。") && !/[*`#|]/.test(await page.locator(".sw-recent-text").nth(1).innerText()), "Markdown source is stripped from the reply preview");
  check(await page.locator(".sw-live-reply").isHidden(), "The live sentence only appears while the model is streaming, so it never repeats the newest reply");
  check(await page.locator("#composer #sw-run-toggle").count() === 1 && await page.locator(".sw-heading #sw-run-toggle").count() === 0, "Loop control is inside the composer instead of the distant header");
  await page.locator("#prompt").fill("状态变化时保留的草稿");
  await page.evaluate(sessionId => {
    window.__flow.main = document.querySelector('[data-module="tools"]');
    window.__flow.wire = document.querySelector('[data-module="tools"] .sw-wave');
    window.__kzPreview.emit("kz:turn", {sessionId,turn:1});
    window.__kzPreview.emit("kz:status", {sessionId,stage:"执行",detail:"检查文件"});
    window.__kzPreview.emit("kz:tool-start", {sessionId,id:"flow-read",name:"read",input:{path:"README.md"}});
  }, id.session); await settle();
  check(await page.locator("#sw-runtime").getAttribute("data-state") === "running" && await page.locator('[data-module="work"]').getAttribute("data-state") === "running" && await projectButton(id.a).getAttribute("data-activity") === "running", "Overview header, requirement baseline and project list all reflect the real running session");
  check(await page.locator('[data-module="tools"]').getAttribute("data-state") === "working", "Only an unfinished live tool call activates the tools module");
  check(await page.evaluate(() => window.__flow.main === document.querySelector('[data-module="tools"]') && window.__flow.wire === document.querySelector('[data-module="tools"] .sw-wave')), "Status changes preserve lane nodes and baselines so animations do not restart");
  check(await page.locator('[data-module="tools"] .sw-wave').evaluate(el => getComputedStyle(el,"::after").animationName === "sw-wave-run") && await page.locator('[data-module="memory"] .sw-wave').evaluate(el => getComputedStyle(el,"::after").animationName === "none"), "Only lanes with real activity run the orange wave");
  await page.evaluate(sessionId => {
    window.__kzPreview.emit("kz:tool-end", {sessionId,id:"flow-read",name:"read",ok:true});
    window.__kzPreview.emit("kz:text", {sessionId,text:"第四条：实时"});
    window.__kzPreview.emit("kz:text", {sessionId,text:"回复已更新。"});
  },id.session); await settle();
  check(await page.locator(".sw-recent-reply").count() === 3 && (await page.locator(".sw-live-text").innerText()).includes("第四条：实时回复已更新。"), "Streaming chunks update the live sentence while three completed replies remain visible");
  await page.evaluate(sessionId => {
    window.__flow.live = document.querySelector(".sw-live-reply");
    window.__flow.history = document.querySelector(".sw-recent-reply");
    window.__kzPreview.emit("kz:text", {sessionId,text:"正在核对最后一项"});
  },id.session); await settle();
  check(await page.locator(".sw-live-text").innerText() === "正在核对最后一项" && await page.evaluate(() => window.__flow.live === document.querySelector(".sw-live-reply") && window.__flow.history === document.querySelector(".sw-recent-reply")), "The live sentence follows the latest fragment without remounting the live row or previous replies");
  check(await page.locator("#prompt").inputValue() === "状态变化时保留的草稿", "Live updates leave the draft intact");
  await page.evaluate(sessionId => window.__kzPreview.emit("kz:text", {sessionId,text:"\n\n## 结论：**全部通过**，详见 [报告](docs/x.md)。"}),id.session); await settle();
  check(await page.locator(".sw-live-text").innerText() === "结论：全部通过，详见 报告。", "The live sentence strips Markdown markers instead of showing source text");
  await page.locator(".sw-live-reply").click(); await settle();
  check(await page.locator("body").getAttribute("data-view") === "chat" && await page.locator("#messages").isVisible(), "Clicking the live sentence opens the complete native conversation");
  await page.setViewportSize({width:390,height:844}); await settle();
  check(await page.locator("#project-space-nav").evaluate(el => el.scrollWidth <= el.clientWidth + 1) && await page.locator("#workbench-chat-history").count() === 1, "Full conversation header stays within one line on a narrow screen (history and new-discussion live only in the sidebar)");
  await page.setViewportSize({width:1440,height:900}); await settle();
  if (await page.locator("#sidebar").evaluate(el => el.classList.contains("collapsed"))) await page.locator("#rail-sidebar-toggle").click();
  await page.locator('[data-work-surface="project"]').click(); await settle();
  for (const theme of ["dark", "light"]) for (const size of [{width:1280,height:720},{width:1440,height:900}]) {
    await page.setViewportSize(size);
    await page.evaluate(theme => document.documentElement.setAttribute("data-theme",theme),theme);
    await page.mouse.move(2,2); await page.waitForTimeout(450);
    const result = await page.evaluate(() => {
      const surface=document.querySelector("#sw-surface"), box=surface.getBoundingClientRect(), composer=document.querySelector("#composer").getBoundingClientRect();
      const items=[...surface.querySelectorAll(".sw-node, .sw-resources button, .sw-recent-reply, .sw-live-reply:not([hidden]), .sw-work-entry")];
      const wire=document.querySelector('[data-module="history"] .sw-wave'), ink=getComputedStyle(wire,"::before"), paper=getComputedStyle(document.querySelector(".sw-workspace"));
      // color-mix() 的计算值是 color(srgb 0.4 0.4 0.4)(0-1),普通颜色是 rgb(102, 102, 102)(0-255)。
      const luminance = color => color.match(/[\d.]+/g).slice(0,3).map(Number).map(n => { n = color.startsWith("color(") ? n : n/255; return n<=.04045?n/12.92:((n+.055)/1.055)**2.4; }).reduce((n,c,i)=>n+c*[.2126,.7152,.0722][i],0);
      const a=luminance(ink.borderTopColor), b=luminance(paper.backgroundColor);
      const recent=document.querySelector(".sw-recent").getBoundingClientRect();
      const separate=[...document.querySelectorAll(".sw-node, .sw-resources button")].every(el=>el.getBoundingClientRect().bottom<=recent.top-3);
      return { fits:separate&&items.every(el=>{const r=el.getBoundingClientRect();return r.top>=box.top&&r.bottom<=box.bottom+1&&r.bottom<=composer.top&&r.right<=box.right+1;})&&surface.scrollHeight<=surface.clientHeight+1,
        contrast:(Math.max(a,b)+.05)/(Math.min(a,b)+.05), stroke:Number(ink.borderTopWidth.replace("px","")) };
    });
    check(result.fits, `${theme} ${size.width}x${size.height}: graph and three replies fit above the composer`);
    check(result.contrast >= 3 && result.stroke >= 1, `${theme}: idle baselines are straight neutral lines with at least 3:1 contrast`);
    await page.screenshot({path:path.join(output,`${theme}-${size.width}.png`)});
  }
  await page.evaluate(sessionId => window.__kzPreview.emit("kz:idle", {sessionId}),id.session); await settle();
  check(await page.locator("#sw-runtime").getAttribute("data-state") === "idle" && await page.locator('[data-module="tools"]').getAttribute("data-state") === "result", "Completion removes activity while retaining the last tool result");
  await page.evaluate(sessionId => window.__kzPreview.emit("kz:status", {sessionId,stage:"迟到事件"}),id.session); await settle();
  check(await page.locator("#sw-runtime").getAttribute("data-state") === "idle", "Late progress cannot revive a finished session");
  await page.emulateMedia({reducedMotion:"reduce"});
  await page.evaluate(sessionId => window.__kzPreview.emit("kz:turn", {sessionId,turn:2}),id.session); await settle();
  check(await page.locator('[data-module="work"] .sw-wave').evaluate(el=>getComputedStyle(el,"::after").animationName === "none") && await page.locator("#sw-runtime").getAttribute("data-state") === "running" && await page.locator('[data-module="work"]').getAttribute("data-state") === "running", "Reduced motion retains an explicit active state without animation");
  await page.locator("#softwire-workspace .sw-idle-lines summary").click();
  await page.locator("#softwire-workspace .sw-idle-lines .sw-line-select").first().click(); await settle();
  await page.getByRole("button", {name:"完整对话 ↗",exact:true}).click(); await settle();
  check(await page.evaluate(async expected => (await import("/03-shell.js")).activeProcessId === expected, id.other), "Full conversation opens the selected graph line, not the previously active chat");
  if (await page.locator("#sidebar").evaluate(el => el.classList.contains("collapsed"))) await page.locator("#rail-sidebar-toggle").click();
  await projectButton(id.b).click(); await settle();
  await page.locator('[data-work-surface="project"]').click(); await settle();
  await page.locator('#sw-refresh').click(); await settle();
  check((await page.locator(".sw-recent").innerText()).includes("另一个项目") && !(await page.locator(".sw-recent").innerText()).includes("第四条"), "Recent replies never leak between projects");
  await page.evaluate(() => {window.__flow.failHistory = true;});
  await page.locator("#sw-refresh").click(); await settle();
  check(await page.locator(".sw-recent-retry").isVisible(), "History failures stay local and offer an explicit retry");
  const historyCalls = await page.evaluate(() => window.__kzPreview.calls.filter(c=>c.cmd==="conversation_get").length);
  for(let i=0;i<3;i++) await page.locator('[data-work-surface="project"]').click(); await settle();
  check(await page.evaluate(() => window.__kzPreview.calls.filter(c=>c.cmd==="conversation_get").length) === historyCalls, "Status paints do not repeatedly fetch failed history");
  await page.evaluate(() => {window.__flow.failHistory = false;});
  await page.locator(".sw-recent-retry").click(); await settle();
  check(await page.locator(".sw-recent-retry").count() === 0, "Explicit history retry recovers the reply preview");
  await page.evaluate(() => {
    window.__flow.historyGates = [];
    window.__kzPreview.setCommand("conversation_get", () => new Promise(resolve => window.__flow.historyGates.push(resolve)));
  });
  await page.locator("#sw-refresh").click();
  await page.waitForFunction(() => window.__flow.historyGates.length === 1);
  await page.locator("#sw-refresh").click();
  await page.waitForFunction(() => window.__flow.historyGates.length === 2);
  await page.evaluate(() => window.__flow.historyGates[1]([{role:"assistant",text:"较新的历史快照"}]));
  await page.waitForFunction(() => document.querySelector(".sw-recent")?.textContent.includes("较新的历史快照"));
  await page.evaluate(() => window.__flow.historyGates[0]([{role:"assistant",text:"过期的历史快照"}])); await settle();
  check((await page.locator(".sw-recent").innerText()).includes("较新的历史快照") && !(await page.locator(".sw-recent").innerText()).includes("过期"), "Late history cannot overwrite a newer reply snapshot");
  check(await page.evaluate(() => window.__kzPreview.calls.every(c=>c.cmd!=="run_prompt")), "Navigation and observation never start a model task");
  check(errors.length===0,`No runtime errors: ${errors.join("; ")}`);
  await writeFile(path.join(output,"acceptance.json"),JSON.stringify({passed:checks.length,checks,errors},null,2));
  console.log(`Softwire flow PASS: ${checks.length} checks`);
} catch(error) {
  await page.screenshot({path:path.join(output,"failure.png")}); console.error("Passed:",checks); throw error;
} finally { await browser.close(); await server.close(); }
