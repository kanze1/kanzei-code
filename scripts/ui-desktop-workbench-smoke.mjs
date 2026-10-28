// Production pages in Edge with the shared IPC fixture. No user data or model.
/* global document, getComputedStyle, innerWidth, window, CompositionEvent, KeyboardEvent */
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
import clipAudit from "./ui-preview/clip-audit.mjs";

const output = "output/desktop-workbench/acceptance";
await mkdir(output, { recursive: true });
const receipt = { boundary: "Production ESM and CSS in Edge; fixture IPC", checks: [], pages: [], errors: [] };
const check = (value, label) => { receipt.checks.push({ label, passed: Boolean(value) }); };
const scenes = [
  ["workspace", "workspace"], ["chat", "chat"], ["empty", "chat"], ["voice", "chat"],
  ["management", "project"], ["management&tab=defect", "project"], ["management&tab=deliveries", "project"], ["management&tab=map", "project"],
  ["doclist", "documents"], ["docs", "documents"], ["lines", "lines"], ["memory", "memory"],
  ["memory-graph", "memory"], ["files", "files"], ["arch", "arch"], ["metrics", "metrics"],
  ["skills", "skills"], ["settings", "settings"], ["overlays", "chat"], ["overlays&dialog=question", "chat"],
  ["overlays&dialog=confirm", "chat"], ["overlays&dialog=input", "chat"], ["overlays&dialog=viewer", "chat"], ["overlays&dialog=palette", "chat"],
];
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
try {
  for (const [width, height, themes, selected] of [[1440, 900, ["dark", "light"], scenes], [420, 800, ["dark"], scenes.filter(([scene]) => !["voice", "empty", "docs", "memory-graph", "arch"].includes(scene))]]) {
    for (const theme of themes) for (const [scene, expected] of selected) {
      const page = await browser.newPage({ viewport: { width, height }, reducedMotion: "reduce" });
      const label = `${scene}-${theme}-${width}`, errors = [];
      page.on("pageerror", error => errors.push(error.message));
      page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
      page.on("response", response => { if (response.status() >= 400) errors.push(`HTTP ${response.status()} ${response.url()}`); });
      try {
        await page.goto(`${server.origin}/?theme=${theme}&scene=${scene}`);
        await page.waitForFunction(() => window.__kzPreview?.ready === true);
        const observed = await page.evaluate(() => {
          const fonts = new Set(), violations = [];
          for (const node of document.body.querySelectorAll("*")) {
            if (!node.getClientRects().length || node.closest(".katex, .monaco-editor, .kz-diagram")) continue;
            if (![...node.childNodes].some(child => child.nodeType === 3 && child.textContent.trim())) continue;
            const size = parseFloat(getComputedStyle(node).fontSize);
            fonts.add(size);
            if (![12, 14, 16, 24].includes(size)) violations.push({ tag: node.tagName, id: node.id, className: String(node.className), size });
          }
          return { view: document.querySelector(".view.active")?.id, fonts: [...fonts], typography: violations.slice(0, 12), overflow: document.documentElement.scrollWidth > innerWidth };
        });
        const audit = await page.evaluate(clipAudit, { viewport: true });
        check(observed.view === `view-${expected}`, `${label}: intended page`);
        check(!observed.overflow, `${label}: no global horizontal overflow`);
        check(observed.typography.length === 0, `${label}: four canonical text sizes`);
        check(audit.cut === 0, `${label}: shadow cut=0`);
        check(errors.length === 0, `${label}: no browser errors`);
        receipt.pages.push({ label, ...observed, audit, errors });
        await page.screenshot({ path: `${output}/${label.replaceAll("&", "-").replaceAll("=", "-")}.png` });
      } catch (error) {
        receipt.errors.push({ label, message: error.stack || error.message, errors });
      } finally { await page.close(); }
    }
  }
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, reducedMotion: "reduce" });
  await page.goto(`${server.origin}/?scene=chat&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  check(await page.locator("#workspace-header #rail-sidebar-toggle").isVisible(), "Sidebar control remains outside collapsed navigation");
  for (const id of ["skills-nav", "theme-toggle"]) check(await page.locator(`#workspace-sidebar-footer #${id}`).isVisible(), `${id} is in global navigation`);
  await page.locator("#rail-sidebar-toggle").click();
  check(await page.locator("#rail-sidebar-toggle").isVisible(), "Collapsed sidebar keeps its restore control");
  await page.locator("#rail-sidebar-toggle").click();
  await page.setViewportSize({ width: 420, height: 800 });
  await page.locator("#rail-sidebar-toggle").click();
  check(await page.locator("#rail-sidebar-toggle").getAttribute("aria-expanded") === "true", "Narrow chrome toggle opens its drawer without outside-click dismissal");
  check(await page.locator("#workbench-home").isVisible(), "Narrow drawer keeps its navigation readable");
  await page.locator("#rail-sidebar-toggle").click();
  check(await page.locator("#rail-sidebar-toggle").getAttribute("aria-expanded") === "false", "Narrow chrome toggle closes the drawer");
  await page.setViewportSize({ width: 1280, height: 800 });
  const ime = await page.evaluate(() => {
    const prompt = document.querySelector("#prompt"); prompt.value = "拼音确认不应发送";
    const before = window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length;
    prompt.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "pin" }));
    prompt.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Enter", isComposing: true }));
    prompt.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "拼" }));
    const companion = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Enter" }); prompt.dispatchEvent(companion);
    return { prevented: companion.defaultPrevented, unchanged: prompt.value === "拼音确认不应发送", callsUnchanged: before === window.__kzPreview.calls.filter(call => call.cmd === "run_prompt").length };
  });
  check(ime.prevented && ime.unchanged && ime.callsUnchanged, "Composing and companion Enter preserve the draft without submission");
  await page.locator("#prompt").fill(Array.from({ length: 30 }, (_, i) => `Line ${i}`).join("\n"));
  check(await page.locator("#prompt").evaluate(node => node.clientHeight <= 200 && node.scrollHeight > node.clientHeight), "Long composer scrolls within its 200px cap");
  await page.close();
  // All feature styles resolve typography from the same four tokens.
  const css = await readFile("crates/kanzei-app/ui/style.css", "utf8");
  check(new Set([...css.matchAll(/--fs-([\d-]+):/g)].map(match => match[1])).size === 4, "Exactly four application typography tokens");
} finally {
  await browser.close(); await server.close();
  receipt.passed = receipt.errors.length === 0 && receipt.checks.every(result => result.passed);
  await writeFile(`${output}/verification.json`, JSON.stringify(receipt, null, 2));
}
const failed = receipt.checks.filter(result => !result.passed);
for (const page of receipt.pages.filter(result => result.typography.length || result.audit.cut)) console.error(JSON.stringify(page));
for (const error of receipt.errors) console.error(error);
console.log(JSON.stringify({ pages: receipt.pages.length, checks: receipt.checks.length, failed, errors: receipt.errors.length, passed: receipt.passed }));
assert.equal(receipt.passed, true, "Desktop workbench acceptance failed; see output/desktop-workbench/acceptance/verification.json");
