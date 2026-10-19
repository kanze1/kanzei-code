/* Production HTML/CSS and application entry points with isolated IPC fixtures. */
/* global window, document, innerWidth */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/visual-polish", checks = [], errors = [];
const check = (value, label) => { assert(value, label); checks.push(label); };
const rgb = hex => `rgb(${hex.match(/[a-f\d]{2}/gi).map(part => parseInt(part, 16)).join(", ")})`;
const palette = {
  light: { bg:"#ffffff", sidebar:"#f7f7f7", composer:"#ffffff", code:"#f7f7f7", text:"#2b2b2b", strong:"#0d0d0d", accent:"#c56500", cyan:"#006579" },
  dark: { bg:"#181818", sidebar:"#1f1f1f", composer:"#232323", code:"#1c1c1c", text:"#e5e5e5", strong:"#ffffff", accent:"#ff8700", cyan:"#39d8f6" },
};
await mkdir(output, { recursive:true });
const server = await startPreviewServer({ port:0 });
const browser = await chromium.launch({ channel:"msedge", headless:true });
const page = await browser.newPage({ viewport:{ width:1600, height:1000 }, reducedMotion:"reduce" });
page.setDefaultTimeout(15000);
page.on("pageerror", error => errors.push(error.message));
page.on("response", response => { if (response.status() >= 400) errors.push(`${response.status()} ${response.url()}`); });

async function openScene(scene, theme) {
  await page.goto(`${server.origin}/?scene=${["work", "skills"].includes(scene) ? "chat" : scene}&theme=${theme}`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async scene => {
    const shell = await import("/03-shell.js"), fixtures = await window.__kzPreview.fixtures();
    for (const process of fixtures.state.processes) process.running = false;
    if (shell.activeSessionId) window.__kzPreview.emit("kz:idle", { sessionId:shell.activeSessionId });
    await (await import("/09-sessions.js")).refreshProcesses();
    if (scene === "chat") {
      (await import("/15-views-misc.js")).renderRecoveredMessages([
        { role:"user", parts:[{ type:"text", text:"检查对话中的代码与复制操作。" }] },
        { role:"assistant", parts:[{ type:"text", text:"## 检查结果\n\n正文与代码分层排版。\n\n```powershell\nnode scripts/ui-project-conversations-smoke.mjs\n```" }] },
      ]);
    } else if (scene === "work") await (await import("/12-workbench.js")).openProjectSpace(shell.currentProject, "project");
    else if (scene === "skills") shell.navigate_view("skills");
    document.getElementById("tasks-close")?.click();
    document.activeElement?.blur();
    await window.__kzPreview.settle(); await document.fonts.ready;
  }, scene);
  if (scene === "work") await page.locator(".management-row").first().waitFor();
  if (scene === "skills") await page.locator(".skill-library-row").first().waitFor();
}
async function checkHover(selector, label) {
  const tab = page.locator(selector).nth(1);
  const before = await tab.boundingBox();
  await tab.hover();
  const hover = await tab.evaluate(element => {
    const css = window.getComputedStyle(element), patch = window.getComputedStyle(element, "::before");
    return { background:css.backgroundColor, patch:parseFloat(patch.height), height:element.getBoundingClientRect().height };
  });
  assert.deepEqual(await tab.boundingBox(), before, `${label}: hover must not shift the tab`);
  check(hover.background === "rgba(0, 0, 0, 0)" && hover.patch < hover.height * .65 && hover.height >= 44, `${label}: compact hover keeps the full click target`);
  await page.mouse.move(0, 0);
}
async function checkActivity(label) {
  check(await page.locator("#tasks-badge").isVisible(), `${label}: real activity count is visible`);
  const layout = await page.locator("#tasks-toggle").evaluate(button => {
    const badge = button.querySelector("#tasks-badge"), text = badge.textContent;
    const cases = [text, "12", "120"].map(value => {
      badge.textContent = value;
      const a = button.querySelector("svg").getBoundingClientRect(), b = badge.getBoundingClientRect(), target = button.getBoundingClientRect();
      return b.left - a.right >= 4 && b.right <= target.right && b.top >= target.top && b.bottom <= target.bottom && target.right <= innerWidth;
    });
    badge.classList.add("hidden");
    const hidden = badge.getBoundingClientRect().width === 0;
    badge.textContent = text; badge.classList.remove("hidden");
    return { cases, hidden };
  });
  check(layout.cases.every(Boolean), `${label}: icon and multi-digit count have separate space`);
  check(layout.hidden, `${label}: hidden count does not occupy space`);
}
try {
  for (const theme of ["light", "dark"]) {
    const colors = palette[theme];
    for (const scene of ["chat", "empty", "work", "memory", "skills", "settings"]) {
      await page.setViewportSize({ width:1600, height:1000 });
      await openScene(scene, theme);
      check(!await page.locator('link[href*="proposal"],link[href*="visual-balance"]').count(), `${scene} ${theme}: production owns all styles`);
      check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${scene} ${theme}: page has no horizontal overflow`);
      if (scene === "chat") {
        const actual = await page.evaluate(() => {
          const color = (selector, property) => window.getComputedStyle(document.querySelector(selector))[property];
          return { bg:color("body", "backgroundColor"), sidebar:color("#sidebar", "backgroundColor"), composer:color("#composer", "backgroundColor"), code:color(".md-code-block", "backgroundColor"), text:color("body", "color"), strong:color("#prompt", "color"), accent:color("#send", "backgroundColor") };
        });
        for (const [key, value] of Object.entries(actual)) check(value === rgb(colors[key]), `${theme}: published ${key} color remains unchanged`);
        check(await page.locator("#sidebar").evaluate(el => window.getComputedStyle(el).backgroundImage !== "none"), `${theme}: sidebar retains its existing texture`);
        const width = await page.locator("#composer").evaluate(el => el.clientWidth);
        await page.locator("#prompt").focus();
        check(await page.locator("#composer").evaluate(el => el.clientWidth) === width, `${theme}: composer focus preserves geometry`);
        await page.locator("#prompt").blur();
        await checkActivity(`${theme} desktop`);
        await page.locator("#tasks-toggle").click();
        check(await page.locator("#tasks-panel").isVisible(), `${theme}: activity panel opens`);
        await page.locator("#tasks-close").click();
        check(await page.locator("#tasks-toggle").getAttribute("aria-expanded") === "false", `${theme}: activity panel closes`);
        await checkHover(".workspace-header [data-work-surface]", `${theme}: workspace tabs`);
        const sidebarWidth = await page.locator("#sidebar").evaluate(el => el.getBoundingClientRect().width);
        await page.evaluate(() => {
          document.documentElement.style.setProperty("--kz-split-sidebar", "310px");
        });
        await page.waitForFunction(() => document.getElementById("sidebar").getBoundingClientRect().width === 310);
        check(await page.locator("#sidebar").evaluate(el => el.getBoundingClientRect().width) === 310, `${theme}: sidebar still respects the saved or dragged width`);
        await page.evaluate(() => document.documentElement.style.removeProperty("--kz-split-sidebar"));
        await page.waitForFunction(width => document.getElementById("sidebar").getBoundingClientRect().width === width, sidebarWidth);
      }
      if (scene === "empty") {
        check(await page.locator(".empty-auto-hint").isVisible(), `${theme}: automatic mode has one welcome hint`);
        check(!await page.locator("#project-onboarding").isVisible(), `${theme}: routine onboarding has no persistent button panel`);
      }
      if (scene === "work") {
        await checkHover(".management-tabs button", `${theme}: management tabs`);
        check(await page.locator('.workspace-header [aria-pressed="true"]').evaluate(el => window.getComputedStyle(el, "::after").backgroundColor) === rgb(colors.accent), `${theme}: workspace uses an orange marker`);
        check(await page.locator('.management-tabs [aria-current="page"]').evaluate(el => window.getComputedStyle(el, "::after").backgroundColor) === rgb(colors.cyan), `${theme}: content uses a cyan marker`);
      }
      if (scene === "skills") check(await page.locator(".skill-switch input:checked+span").first().evaluate(el => window.getComputedStyle(el).backgroundColor) === rgb(colors.cyan), `${theme}: enabled skills have cyan accents`);
      await page.screenshot({ path:`${output}/${scene}-${theme}.png`, animations:"disabled" });
      await page.setViewportSize({ width:420, height:900 });
      await page.screenshot({ path:`${output}/${scene}-${theme}-narrow.png`, animations:"disabled" });
      check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${scene} ${theme}: 420px page has no overflow`);
      if (["chat", "empty"].includes(scene)) check(await page.locator("#send").isVisible(), `${scene} ${theme}: narrow composer retains its action`);
      if (scene === "chat") await checkActivity(`${theme} narrow`);
      if (scene === "settings") {
        const toc = page.locator("#settings-toc");
        check(await toc.isVisible() && await toc.evaluate(el => el.clientHeight <= 64 && el.scrollWidth > el.clientWidth), `${theme}: narrow settings retains a compact scrollable outline`);
        await toc.locator('[data-settings-target="sg-update"]').click();
        check(await page.locator("#sg-update").evaluate(el => el.open), `${theme}: narrow outline reaches version and updates`);
        check(await page.locator("#settings-save").isVisible(), `${theme}: narrow settings keeps Save visible`);
        await page.screenshot({ path:`${output}/settings-${theme}-narrow.png`, animations:"disabled" });
      }
      if (scene === "memory") check(await page.locator(".memory-manager-toolbar label>span").evaluateAll(labels => labels.every(el => el.clientHeight < 2 * parseFloat(window.getComputedStyle(el).fontSize))), `${theme}: narrow filter labels stay horizontal`);
    }
  }
  check(errors.length === 0, `No browser or resource errors: ${errors.join("; ")}`);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ checks, passed:checks.length, errors }, null, 2));
  console.log(`Visual polish PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path:`${output}/failure.png`, animations:"disabled" });
  throw error;
} finally { await browser.close(); await server.close(); }
