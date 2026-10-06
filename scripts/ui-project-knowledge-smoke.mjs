// 真浏览器验证三个 UI 消费同一层级数据，IPC 使用项目自带隔离预览。
/* global window, document */
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const errors = [];
const passed = [];
const check = (value, label) => { assert(value, label); passed.push(label); };
await mkdir("dist/ui-project-knowledge", { recursive: true });
try {
  for (const theme of ["light", "dark"]) {
    const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
    page.setDefaultTimeout(12000);
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
    await page.goto(`${server.origin}/?scene=arch&theme=${theme}`);
    await page.waitForFunction(() => window.__kzPreview?.ready);
    await page.locator('#arch-knowledge [data-area="kanzei-tools"] > summary').waitFor();
    check(await page.locator('#arch-knowledge [data-area="kanzei-tools/edit"]').count() === 0, `${theme}: module DOM loads on expansion`);
    await page.locator('#arch-knowledge [data-area="kanzei-tools"] > summary').click();
    await page.locator('#arch-knowledge [data-area="kanzei-tools/edit"] > summary').click();
    check((await page.locator("#arch-knowledge").innerText()).includes("推断关联"), `${theme}: inferred links are visible`);
    check((await page.locator("#arch-knowledge").innerText()).includes("kanzei-harness"), `${theme}: dependencies are visible`);
    await page.evaluate(async () => { const ui = await import("/02-i18n.js"); ui.setLanguagePreference("en", { persist: true }); });
    await page.waitForFunction(() => document.querySelector("#arch-knowledge")?.textContent.includes("Inferred link"));
    check(await page.locator('#arch-knowledge [data-area="kanzei-tools/edit"][open]').count() === 1, `${theme}: language switch preserves expanded module`);
    await page.evaluate(async () => { const ui = await import("/02-i18n.js"); ui.setLanguagePreference("zh", { persist: true }); });
    await page.screenshot({ path: `dist/ui-project-knowledge/architecture-${theme}.png`, fullPage: true });
    const memoryButton = page.locator('#arch-knowledge [data-area="kanzei-tools/edit"] .knowledge-memory').first();
    const id = (await memoryButton.innerText()).split(" · ")[0];
    await memoryButton.click();
    await page.waitForFunction(id => document.querySelector("#memory-detail")?.dataset.memoryId === id, id);
    check(await page.locator("#view-memory.active").count() === 1, `${theme}: architecture memory opens original detail`);
    await page.locator("#view-memory > #memory-scroll > details.project-knowledge-wrap > summary").click();
    await page.locator('#memory-arch [data-area="kanzei-tools"] > summary').click();
    await page.locator('#memory-arch [data-area="kanzei-tools/edit"] > summary').click();
    await page.locator('#memory-arch [data-area="kanzei-tools/edit"] .knowledge-memory').first().waitFor();
    check(await page.locator('#memory-arch [data-area="kanzei-tools/edit"] .knowledge-memory').count() === 2, `${theme}: memory page uses the same module links`);
    await page.setViewportSize({ width: 800, height: 650 });
    await page.screenshot({ path: `dist/ui-project-knowledge/memory-${theme}-800.png`, fullPage: true });
    check(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `${theme}: narrow viewport has no document overflow`);
    await page.close();
  }
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${server.origin}/?scene=files&state=open&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready && document.querySelector("#files-preview-path")?.textContent);
  await page.locator("#files-knowledge-wrap:not(.hidden) > summary").click();
  check(await page.locator('#files-knowledge [data-area="kanzei-tools/registry"] .knowledge-memory').count() === 2, "file explorer shows module experience");
  check((await page.locator("#files-knowledge").innerText()).includes("AI 用途摘要"), "file purposes retain summary provenance");
  check(await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "files_snapshot")
    .every(call => Boolean(call.args.knowledgeProjectDir))), "file explorer passes the project asset root separately");
  await page.screenshot({ path: "dist/ui-project-knowledge/files-light.png", fullPage: true });
  const fileMemory = page.locator("#files-knowledge .knowledge-memory").first();
  const fileMemoryId = (await fileMemory.innerText()).split(" · ")[0];
  await fileMemory.click();
  await page.waitForFunction(id => document.querySelector("#memory-detail")?.dataset.memoryId === id, fileMemoryId);
  check(await page.locator("#view-memory.active").count() === 1, "file explorer opens the original project memory");
  await page.close();
  const off = await browser.newPage({ viewport: { width: 1280, height: 840 } });
  off.on("pageerror", error => errors.push(error.message));
  await off.goto(`${server.origin}/?scene=arch&knowledge=off&theme=light`);
  await off.waitForFunction(() => window.__kzPreview?.ready);
  const switcher = off.locator("#arch-knowledge .knowledge-switch input");
  await switcher.waitFor();
  check(!await switcher.isChecked(), "project switch defaults to off without initialized knowledge");
  check(await off.locator("#arch-knowledge [data-area]").count() === 0, "disabled knowledge does not render module hierarchy");
  await switcher.check();
  await off.locator('#arch-knowledge [data-area="kanzei-tools"] > summary').waitFor();
  check(await off.locator("#arch-knowledge .knowledge-switch input").isChecked(), "enabling initializes the hierarchy");
  await off.locator("#arch-knowledge .knowledge-switch input").uncheck();
  await off.waitForFunction(() => document.querySelectorAll("#arch-knowledge [data-area]").length === 0);
  await off.locator("#arch-knowledge .knowledge-switch input").check();
  await off.locator('#arch-knowledge [data-area="kanzei-tools"] > summary').click();
  await off.locator('#arch-knowledge [data-area="kanzei-tools/edit"] > summary').click();
  check(await off.locator('#arch-knowledge [data-area="kanzei-tools/edit"] .knowledge-memory').count() === 2, "disable and re-enable preserve original memories");
  await off.evaluate(async () => { const ui = await import("/09-sessions.js"); ui.openNewProjectDialog(); });
  check(!await off.locator("#new-project-knowledge").isChecked(), "new project offers an explicit unchecked initialization option");
  await off.locator("#new-project-knowledge").check();
  await off.keyboard.press("Escape");
  await off.evaluate(async () => { const ui = await import("/09-sessions.js"); ui.openNewProjectDialog(); });
  check(await off.locator("#new-project-knowledge").isChecked(), "new project draft retains initialization choice");
  await off.close();
  check(errors.length === 0, `browser errors: ${errors.join("; ")}`);
  console.log(`Project knowledge browser PASS: ${passed.length} checks`);
} finally {
  await browser.close();
  await server.close();
}
