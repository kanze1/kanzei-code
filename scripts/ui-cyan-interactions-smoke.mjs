// Real production DOM in Edge with isolated IPC. No user project writes.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const output = "output/playwright/cyan-interactions";
await mkdir(output, { recursive: true });
const result = { boundary: "Production frontend in Edge; isolated IPC, not native backend acceptance", checks: [], failures: [], errors: [] };
const check = (label, pass) => { assert.ok(pass, label); result.checks.push(label); };
async function open(scene = "empty", theme = "light", width = 1440) {
  const page = await browser.newPage({ viewport: { width, height: 960 } });
  page.setDefaultTimeout(15000);
  page.on("pageerror", error => result.errors.push(error.message));
  await page.goto(`${server.origin}/?scene=${scene}&theme=${theme}`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  return page;
}
async function scenario(name, run) {
  try { await run(); } catch (error) { result.failures.push({ name, error: String(error.stack || error) }); }
}
const colorIsOk = locator => locator.evaluate(el => {
  const reference = document.createElement("span"); reference.style.color = "var(--ok)"; el.append(reference);
  const matches = getComputedStyle(el).color === getComputedStyle(reference).color; reference.remove(); return matches;
});
try {
  await scenario("model rejection", async () => {
    const page = await open();
    const original = await page.evaluate(async () => { const s = await import("/03-shell.js"); return s.processItems.find(x => x.id === s.activeProcessId).model || null; });
    await page.evaluate(() => window.__kzPreview.setCommand("process_update", () => { throw "audit rejected model save"; }));
    await page.locator("#model-picker").click(); await page.locator('[data-action="show-all"]').click();
    await page.locator('[data-value="ollama:qwen3.5:4b"]').click();
    await page.waitForFunction(() => document.body.innerText.includes("audit rejected model save") && !document.querySelector("#model-picker").classList.contains("is-pending"));
    check("Rejected selection restores the displayed confirmed model", !(await page.locator("#model-picker").innerText()).includes("ollama:qwen3.5:4b"));
    await page.evaluate(() => window.__kzPreview.setCommand("run_prompt", () => null));
    await page.locator("#prompt").fill("send after rejected setting"); await page.locator("#send").click();
    await page.waitForFunction(() => window.__kzPreview.calls.some(x => x.cmd === "run_prompt"));
    check("Next send uses the confirmed model after a rejected save", await page.evaluate(original => window.__kzPreview.calls.findLast(x => x.cmd === "run_prompt").args.model === original, original));
    await page.close();
  });
  await scenario("pending settings and send", async () => {
    const page = await open();
    await page.evaluate(async () => {
      const p = window.__kzPreview, f = await p.fixtures();
      p.setCommand("process_update", args => new Promise(resolve => { window.finishModelSave = async () => { resolve(await f.commands.process_update(args)); }; }));
      p.setCommand("run_prompt", () => null);
      const models = await import("/08-models.js"); window.modelSaving = models.setLineModel("ollama:qwen3.5:4b");
    });
    await page.waitForFunction(() => window.finishModelSave);
    check("Saving model exposes a pending state", await page.locator("#model-picker").getAttribute("aria-busy") === "true");
    await page.locator("#prompt").fill("send while setting saves"); await page.locator("#send").click();
    check("Send waits for the setting commit", await page.evaluate(() => !window.__kzPreview.calls.some(x => x.cmd === "run_prompt")));
    await page.evaluate(() => window.finishModelSave());
    await page.waitForFunction(() => window.__kzPreview.calls.some(x => x.cmd === "run_prompt"));
    check("Accepted setting is used by the waiting send", await page.evaluate(() => window.__kzPreview.calls.findLast(x => x.cmd === "run_prompt").args.model === "ollama:qwen3.5:4b"));
    await page.close();
  });
  await scenario("queued edits", async () => {
    const page = await open();
    await page.evaluate(async () => {
      const p = window.__kzPreview, f = await p.fixtures(); window.saveGates = [];
      p.setCommand("process_update", args => new Promise((resolve, reject) => window.saveGates.push({ resolve: async () => resolve(await f.commands.process_update(args)), reject })));
      const m = await import("/08-models.js"); window.saveOne = m.setLineModel("ollama:rejected"); window.saveTwo = m.setLineModel("ollama:qwen3.5:4b");
    });
    await page.waitForFunction(() => window.saveGates.length === 1);
    await page.evaluate(() => window.saveGates[0].reject("first save rejected"));
    await page.waitForFunction(() => window.saveGates.length === 2);
    check("Earlier failure cannot clear a newer pending choice", (await page.locator("#model-picker").innerText()).includes("ollama:qwen3.5:4b") && await page.locator("#model-picker").getAttribute("aria-busy") === "true");
    await page.evaluate(() => window.saveGates[1].resolve());
    await page.waitForFunction(() => document.querySelector("#model-picker").getAttribute("aria-busy") === "false");
    check("Last successful queued edit converges local and displayed values", await page.evaluate(async () => { const s = await import("/03-shell.js"); return s.processItems.find(x => x.id === s.activeProcessId).model === "ollama:qwen3.5:4b" && document.querySelector("#model-picker").textContent.includes("ollama:qwen3.5:4b"); }));
    await page.close();
  });
  await scenario("cold editor", async () => {
    const page = await open();
    await page.evaluate(async () => {
      const e = await import("/17-files-editor.js");
      e.setMonacoLoader(() => new Promise((resolve, reject) => { window.editorGate = { resolve, reject }; }));
      (await import("/03-shell.js")).navigate_view("files");
    });
    await page.locator("#files-tree .files-file").first().click();
    await page.waitForFunction(() => window.editorGate);
    check("Cold editor displays a loading message", await page.locator("#files-placeholder").isVisible() && (await page.locator("#files-placeholder").innerText()).includes("正在加载编辑器"));
    check("Cold editor never exposes an empty editing surface", await page.locator("#files-editor").isHidden());
    await page.evaluate(() => window.editorGate.reject("audit editor unavailable"));
    await page.waitForFunction(() => document.querySelector("#files-placeholder").textContent.includes("audit editor unavailable"));
    check("Editor load rejection has visible feedback", await page.locator("#files-placeholder").isVisible());
    await page.close();
  });
  await scenario("reasoning rejection", async () => {
    const page = await open();
    const state = await page.evaluate(async () => {
      const s = await import("/03-shell.js"), original = s.processItems.find(x => x.id === s.activeProcessId).reasoning || null;
      window.__kzPreview.setCommand("process_update", () => { throw "audit rejected reasoning save"; });
      await (await import("/08-models.js")).setLineReasoning("low");
      return { original, confirmed: s.processItems.find(x => x.id === s.activeProcessId).reasoning || null };
    });
    check("Rejected reasoning edit preserves the confirmed process value", state.original === state.confirmed);
    check("Rejected reasoning edit clears its pending state", await page.locator("#reasoning-picker").getAttribute("aria-busy") === "false");
    await page.close();
  });
  await scenario("copy feedback", async () => {
    const page = await open("chat");
    await page.evaluate(async () => {
      Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: () => new Promise(resolve => { window.finishCopy = resolve; }) } });
      window.copying = (await import("/07-events.js")).copyReadable(document.querySelector(".msg.assistant"));
    });
    await page.waitForFunction(() => window.finishCopy);
    check("Copy has no premature success receipt", await page.locator('.k-toast[data-kind="ok"]').count() === 0);
    await page.evaluate(() => window.finishCopy());
    await page.locator('.k-toast[data-kind="ok"]').waitFor();
    check("Confirmed copy exposes the shared success feedback", (await page.locator('.k-toast[data-kind="ok"]').innerText()).includes("已复制"));
    await page.close();
  });
  for (const theme of ["light", "dark"]) await scenario(`management ${theme}`, async () => {
    const page = await open("empty", theme);
    await page.locator('[data-work-surface="project"]').click(); await page.locator(".management-row").first().waitFor();
    check(`${theme}: requirement identifiers use the cyan token`, await colorIsOk(page.locator(".management-id").first()));
    check(`${theme}: list counts use the cyan token`, await colorIsOk(page.locator(".management-list-count strong")));
    const total = await page.locator(".management-row").count();
    await page.locator(".management-tools input").fill("no matching requirement");
    check(`${theme}: filtering updates counts and the empty state`, await page.locator(".management-row").count() === 0 && (await page.locator(".management-list-count").innerText()).includes("0 /") && await page.locator(".management-empty").isVisible());
    check(`${theme}: search keeps keyboard focus`, await page.locator(".management-tools input").evaluate(el => el === document.activeElement));
    await page.locator(".management-tools input").fill("");
    check(`${theme}: clearing search restores rows`, await page.locator(".management-row").count() === total);
    await page.screenshot({ path: `${output}/management-${theme}.png` });
    await page.evaluate(async () => {
      const p = window.__kzPreview, f = await p.fixtures();
      p.setCommand("docs_snapshot", () => new Promise(resolve => { window.finishRefresh = () => resolve(f.commands.docs_snapshot()); }));
    });
    await page.locator(".management-tools").getByRole("button", { name: "刷新", exact: true }).click();
    await page.waitForFunction(() => window.finishRefresh);
    check(`${theme}: refresh exposes disabled/busy feedback`, await page.locator('.management-tools button[aria-busy="true"]').isDisabled());
    await page.evaluate(() => window.finishRefresh());
    await page.locator('.management-tools button[aria-busy="false"]').waitFor();
    check(`${theme}: refresh settles back to an enabled action`, !await page.locator('.management-tools button[aria-busy="false"]').isDisabled());
    await page.locator('[data-management-tab="defect"]').click();
    check(`${theme}: selected tab updates after click`, await page.locator('[data-management-tab="defect"]').getAttribute("aria-current") === "page" && await page.locator('[data-management-tab="req"]').getAttribute("aria-current") === "false");
    await page.evaluate(async () => {
      const s = await import("/03-shell.js");
      window.__kzPreview.setCommand("run_metrics", { rounds: [{ at: Date.now(), sessionId: s.activeSessionId, measured: true, durationMs: 42000, inputTokens: 12840, outputTokens: 926, steps: 5, outcome: "completed", metrics: { total_calls: 9, failed_calls: 1, tool_rejections: 2 }, tools: { read: 6, bash: 3 } }] });
      window.__kzPreview.setCommand("memory_recalls", { rounds: [] });
    });
    await page.locator('[data-management-tab="map"]').click(); await page.locator(".management-metrics dd").first().waitFor();
    check(`${theme}: known operational counts are cyan`, await colorIsOk(page.locator('.management-metrics [data-tone="value"] dd').first()));
    check(`${theme}: large counts use readable thousands separators`, (await page.locator(".management-metrics").innerText()).includes("12,840"));
    check(`${theme}: failures retain an error tone`, await page.locator(".management-metrics > div").filter({ has: page.locator("dt", { hasText: "失败调用" }) }).getAttribute("data-tone") === "error");
    check(`${theme}: unobserved metrics remain unknown`, await page.locator('.management-metrics [data-tone="unknown"] dd').first().innerText() === "—");
    await page.screenshot({ path: `${output}/metrics-${theme}.png` });
    await page.setViewportSize({ width: 800, height: 900 });
    check(`${theme}: management fits a narrow desktop viewport`, await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await page.screenshot({ path: `${output}/metrics-${theme}-800.png` });
    await page.close();
  });
  await scenario("attachment lifecycle", async () => {
    const page = await open("chat");
    await page.locator("#attachment-input").setInputFiles({ name: "audit.txt", mimeType: "text/plain", buffer: Buffer.from("audit") });
    await page.locator("#attachment-send-hint").waitFor();
    check("Running attachment restriction is persistent before clicking Send", await page.locator("#attachment-send-hint").isVisible());
    await page.locator("#prompt").fill("retain my attachment draft"); await page.locator("#send").click();
    check("Blocked attachment send preserves text and attachment", await page.locator("#prompt").inputValue() === "retain my attachment draft" && await page.locator(".attachment-chip").count() === 1);
    check("Attachment icon uses the cyan token", await colorIsOk(page.locator(".attachment-chip .resource-mark")));
    await page.evaluate(async () => { const s = await import("/03-shell.js"); window.__kzPreview.emit("kz:stopped", { sessionId: s.activeSessionId }); });
    await page.locator("#attachment-send-hint").waitFor({ state: "hidden" });
    check("Terminal state removes the attachment restriction", true);
    await page.close();
  });
  for (const scene of ["workspace", "settings", "memory", "arch", "lines", "docs", "files"]) await scenario(`surface ${scene}`, async () => {
    const page = await open(scene);
    check(`${scene}: first render has no document overflow`, await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    check(`${scene}: active content is present`, await page.locator(".view.active").count() > 0);
    await page.close();
  });
} finally {
  await browser.close(); await server.close();
  await writeFile(`${output}/results.json`, JSON.stringify(result, null, 2) + "\n");
  console.log(JSON.stringify(result, null, 2));
}
assert.equal(result.failures.length, 0, "Cyan interaction scenarios failed");
assert.equal(result.errors.length, 0, "Unexpected frontend runtime errors");
