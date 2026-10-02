// Real browser: verify default startup and first-use lazy interfaces.
import assert from "node:assert/strict";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage();
const requests = new Set(), errors = [];
page.on("request", request => requests.add(new URL(request.url()).pathname));
page.on("pageerror", error => errors.push(String(error)));
try {
  await page.goto(`${server.origin}/?scene=empty`);
  await page.waitForFunction(() => globalThis.__kzPreview?.ready);
  for (const file of ["13-memory-chat.js", "24-memory-graph.js", "24-graph-view.js", "23-voice.js", "16-mobile.js", "22-visual-runtime.js"]) {
    assert(!requests.has(`/${file}`), `${file} loaded on default startup`);
  }
  assert(!(await page.evaluate(() => globalThis.__kzPreview.calls.some(call => ["voice_settings_get", "voice_start", "mobile_service_start", "memory_graph", "memory_chat_history"].includes(call.cmd)))), "optional IPC on startup");
  await page.evaluate(async () => (await import("/03-shell.js")).navigate_view("memory"));
  await page.locator("#memory-view-graph").click();
  await page.waitForFunction(() => document.querySelector("#memory-graph-pane")?.classList.contains("hidden") === false);
  assert(requests.has("/24-memory-graph.js"));
  await page.locator("#memory-chat-tab").click();
  await page.waitForFunction(() => globalThis.__kzPreview.calls.some(call => call.cmd === "memory_chat_history"));
  assert(requests.has("/13-memory-chat.js"));
  await page.evaluate(async () => (await import("/03-shell.js")).navigate_view("settings"));
  const outer = page.locator("#mobile-settings").locator("xpath=ancestor::details[1]");
  if (await outer.count()) await outer.locator(":scope > summary").click();
  await page.locator("#mobile-settings > summary").click();
  await page.waitForFunction(() => performance.getEntriesByType("resource").some(entry => entry.name.endsWith("/16-mobile.js")));
  assert(!requests.has("/23-voice.js"));
  assert(!requests.has("/22-visual-runtime.js"));
  await page.locator("#set-visuals-enabled").check();
  await page.waitForFunction(async () => (await import("/22-neural-flow.js")).chatBackdrop !== null);
  assert(requests.has("/22-visual-runtime.js"));
  await page.locator("#set-visuals-enabled").uncheck();
  assert(await page.evaluate(async () => (await import("/22-neural-flow.js")).chatBackdrop === null));
  assert.equal(await page.locator("#oc-toggle, #oc-settings, .oc-figure").count(), 0);
  assert(![...requests].some(url => /22-oc-|assets\/oc\/|vendor\/pixi\//.test(url)), "removed character resources requested");
  assert.equal(await page.locator("html").getAttribute("data-backdrop"), "off");
  await page.evaluate(async () => (await import("/03-shell.js")).navigate_view("chat"));
  await page.locator("#voice-toggle").click();
  await page.waitForFunction(() => globalThis.__kzPreview.calls.some(call => call.cmd === "voice_settings_get"));
  assert(requests.has("/23-voice.js"));
  assert.deepEqual(errors, [], "optional interface runtime errors");
  console.log("Optional modules passed: no default imports/IPC; graph, memory chat, mobile and voice first use; visual master teardown.");
} finally {
  await browser.close();
  await server.close();
}
