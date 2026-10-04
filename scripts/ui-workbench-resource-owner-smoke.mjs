// Production UI in Edge, isolated IPC: a preview project is not the active root.
/* global window, document */
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = path.resolve(process.env.KANZEI_WB2_OUTPUT || "output/audit-WB2/resources");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
if (process.argv.includes("--before")) {
  const body = execFileSync("git", ["show", "26acb7f0:crates/kanzei-app/ui/12-workbench.js"], { encoding: "utf8" });
  await page.route("**/12-workbench.js", route => route.fulfill({ contentType: "text/javascript", body }));
}
page.setDefaultTimeout(10000);
const checks = [], errors = [];
page.on("pageerror", e => errors.push(e.message));
const check = (value, label) => { assert(value, label); checks.push(label); };
const settle = () => page.evaluate(() => window.__kzPreview.settle());
try {
  await page.goto(`${server.origin}/?scene=workspace&theme=light&keep=1`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.locator(".workspace-card-open").first().click(); await settle();
  const identity = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures(), shell = await import("/03-shell.js");
    const a = shell.currentProject, b = f.commands.workspace_snapshot().projects.find(p => p.path !== a).path;
    window.__resourceTest = { f, a, b, reads: [], saves: [] };
    window.__kzPreview.setCommand("conventions_read", args => {
      window.__resourceTest.reads.push(args.projectDir);
      return { exists: true, content: `Rules for ${args.projectDir}`, hash: "original", proposal: null };
    });
    window.__kzPreview.setCommand("conventions_save", args => { window.__resourceTest.saves.push(args); return "saved"; });
    window.__kzPreview.setCommand("softwire_questions", () => []);
    await (await import("/12-workbench.js")).openProjectSpace(b, "project");
    return { a, b, current: shell.currentProject };
  }); await settle();
  check(identity.current === identity.a, "Browsing B's overview keeps A as the execution root");
  await page.locator('#workspace-sidebar-footer [data-view="settings"]').click();
  await page.locator("#sg-project-tools > summary").click();
  await page.locator('[data-resource="conventions"]').click();
  await page.locator("#conventions-dialog").waitFor({ state: "visible" });
  const opened = await page.evaluate(() => window.__resourceTest.reads.at(-1));
  await writeFile(path.join(output, "observed.json"), JSON.stringify({ identity, opened }, null, 2));
  check(opened === identity.b, "The B overview reads B conventions, never A conventions");
  await page.locator("#conventions-edit").click();
  await page.locator("#conventions-editor").fill("B-only edited rules");
  await page.locator("#conventions-save").click(); await settle();
  const saved = await page.evaluate(() => window.__resourceTest.saves);
  check(saved.length === 1 && saved[0].projectDir === identity.b && saved[0].content === "B-only edited rules", "Saving the previewed project's rules writes only B");
  check(await page.locator("body").getAttribute("data-view") === "project", "Conventions stay on the overview surface");
  await page.locator("#conventions-close").click();
  await page.evaluate(async () => {
    const t = window.__resourceTest;
    window.__kzPreview.setCommand("projects_select", args => args.path === t.a ? new Promise(resolve => {
      t.releaseSelection = () => resolve(t.f.commands.projects_select(args));
    }) : t.f.commands.projects_select(args));
    await (await import("/12-workbench.js")).openProjectSpace(t.a, "project");
  }); await settle();
  await page.locator('#workspace-sidebar-footer [data-view="settings"]').click();
  await page.locator('[data-resource="conventions"]').click();
  await page.waitForFunction(() => Boolean(window.__resourceTest.releaseSelection));
  await page.evaluate(async () => {
    const t = window.__resourceTest, wb = await import("/12-workbench.js");
    await wb.openProjectSpace(t.b, "chat");
    t.releaseSelection();
  }); await settle();
  check(await page.locator("#conventions-dialog").isHidden(), "A cancelled resource activation cannot open its dialog over B");
  check(await page.evaluate(() => window.__kzPreview.calls.filter(c => c.cmd === "run_prompt").length) === 0, "Opening and editing conventions never starts an agent task");
  check(errors.length === 0, `No browser errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ checks, errors }, null, 2));
  console.log(`Workbench resource owner PASS: ${checks.length} checks`);
} catch (error) {
  await writeFile(path.join(output, "failure.json"), JSON.stringify({ checks, errors, error: String(error.stack || error) }, null, 2));
  throw error;
} finally { await browser.close(); await server.close(); }
