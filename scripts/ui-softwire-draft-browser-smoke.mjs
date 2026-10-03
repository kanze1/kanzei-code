// Real UI/ESM owners in Edge; deferred IPC receipts exercise navigation races.
/* global window, document */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = path.resolve("output/audit-WB1/draft-browser");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
page.setDefaultTimeout(9000);
const errors = [], checks = [];
page.on("pageerror", error => errors.push(error.message));
const check = (value, label) => { assert(value, label); checks.push(label); };
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const overview = async () => { await page.locator('[data-work-surface="project"]').click(); await settle(); };
const chat = async () => { await page.evaluate(async () => (await import("/03-shell.js")).navigate_view("chat")); await settle(); };
try {
  await page.goto(`${server.origin}/?scene=workspace&theme=light&keep=1`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.locator(".workspace-card-open").first().click(); await settle();
  await page.locator(".workbench-project-link.active").click(); await settle();
  await overview();
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), fixtures = await window.__kzPreview.fixtures();
    window.__draftRace = { project: shell.currentProject, process: shell.activeProcessId,
      other: fixtures.commands.workspace_snapshot().projects.find(p => p.path !== shell.currentProject).path };
    window.__kzPreview.setCommand("softwire_questions", () => []);
    window.__kzPreview.setCommand("run_prompt", () => new Promise((resolve, reject) => {
      window.__draftRace.receipt = { resolve, reject };
    }));
  });
  await page.locator("#prompt").fill("main draft edited in overview");
  await page.locator('[data-module="tools"]').click();
  await page.locator("#prompt").fill("separate tools draft");
  await chat();
  check(await page.locator("#prompt").inputValue() === "main draft edited in overview", "Leaving a module restores the latest main draft");
  await overview(); await page.locator('[data-module="tools"]').click();
  check(await page.locator("#prompt").inputValue() === "separate tools draft", "The module retains its independent draft");
  await overview();

  async function sendPending(text) {
    await page.evaluate(() => { window.__draftRace.receipt = null; });
    await page.locator("#prompt").fill(text); await page.locator("#send").click();
    await page.waitForFunction(() => Boolean(window.__draftRace.receipt));
  }
  async function acknowledge(failed = false) {
    await page.evaluate(failed => {
      const receipt = window.__draftRace.receipt;
      if (failed) receipt.reject("injected transport failure"); else receipt.resolve(null);
    }, failed);
    await settle();
  }
  await sendPending("send before leaving overview"); await chat(); await acknowledge();
  check(await page.locator("#prompt").inputValue() === "", "Late success clears the restored native draft");
  await overview();
  check(await page.locator("#prompt").inputValue() === "", "Returning to overview does not resurrect an acknowledged draft");

  await sendPending("old submission"); await chat();
  await page.locator("#prompt").fill("new native text"); await acknowledge();
  check(await page.locator("#prompt").inputValue() === "new native text", "Late success preserves newer native input");
  await overview();
  check(await page.locator("#prompt").inputValue() === "new native text", "New native input remains authoritative on remount");

  await sendPending("cross-project submission");
  await page.evaluate(async () => {
    const t = window.__draftRace;
    await (await import("/12-workbench.js")).openProjectSpace(t.other, "chat");
  }); await settle();
  await page.locator("#prompt").fill("other project draft"); await acknowledge();
  check(await page.locator("#prompt").inputValue() === "other project draft", "A background acknowledgement leaves the other project's editor intact");
  await page.evaluate(async () => {
    const t = window.__draftRace;
    await (await import("/12-workbench.js")).openProjectSpace(t.project, "chat");
  }); await settle();
  check(await page.locator("#prompt").inputValue() === "", "Returning to the original project cannot restore its cached submitted draft");
  await overview();
  check(await page.locator("#prompt").inputValue() === "", "Both draft owners agree after a cross-project acknowledgement");

  await sendPending("failed submission retained"); await chat(); await acknowledge(true);
  check(await page.locator("#prompt").inputValue() === "failed submission retained", "A failed receipt retains the native draft");
  await overview();
  check(await page.locator("#prompt").inputValue() === "failed submission retained", "A failed submission remains editable after remount");
  check(errors.length === 0, `No browser errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ passed: checks.length, checks, errors }, null, 2));
  console.log(`Softwire draft browser PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path: path.join(output, "failure.png") });
  console.error("Passed before failure:", checks); throw error;
} finally { await browser.close(); await server.close(); }
