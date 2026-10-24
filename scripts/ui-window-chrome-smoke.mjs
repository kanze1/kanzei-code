// Real production DOM in Edge with explicit native-window IPC fixtures. This
// does not prove Windows frame geometry or move/minimize behavior; run native QA.
/* global window, innerWidth, MouseEvent */
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/window-chrome";
const result = { boundary: "Production ESM/Edge + mocked window API; not a native window test", checks: [], errors: [] };
const check = (condition, label) => { assert.ok(condition, label); result.checks.push(label); };
const config = JSON.parse(await readFile("crates/kanzei-app/tauri.conf.json", "utf8"));
check(config.app.windows[0].decorations === false, "Main window opts out of native title decorations");
const capability = JSON.parse(await readFile("crates/kanzei-app/capabilities/main-window-chrome.json", "utf8"));
check(JSON.stringify(capability.windows) === '["main"]', "Window-control capability is main-window-only");
check(JSON.stringify([...capability.permissions].sort()) === JSON.stringify(["core:window:allow-close", "core:window:allow-minimize", "core:window:allow-start-dragging", "core:window:allow-toggle-maximize"].sort()), "Only the four required new native capabilities are granted");
const chromeSource = await readFile("crates/kanzei-app/ui/30-window-chrome.js", "utf8");
check(!/\.destroy\(|app\.exit\(|process\.exit\(/.test(chromeSource), "Close uses no force-destroy or app-exit path");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
page.setDefaultTimeout(12000);
page.on("pageerror", error => result.errors.push(String(error)));
const calls = () => page.evaluate(() => [...window.__kzPreview.windowCalls]);
const ready = () => page.waitForFunction(() => !document.querySelector("#window-close").disabled);
const navigate = async view => {
  await page.evaluate(async view => (await import("/03-shell.js")).navigate_view(view), view);
  await page.waitForFunction(view => document.body.dataset.view === view, view);
};
async function checkLayout(label) {
  const layout = await page.evaluate(() => {
    const rect = selector => { const r = document.querySelector(selector).getBoundingClientRect(); return { x: r.x, y: r.y, right: r.right, bottom: r.bottom, height: r.height }; };
    return { chrome: rect("#window-chrome"), app: rect("#app"), active: rect(".view.active"), controls: rect("#window-controls"), header: rect("#workspace-header"), width: innerWidth, scrollWidth: document.documentElement.scrollWidth };
  });
  check(layout.chrome.y === 0 && layout.chrome.height === 44 && layout.app.y === 44, `${label}: one compact 44px top row`);
  check(layout.active.y === 44, `${label}: no retired workspace-header row remains in main`);
  check(layout.controls.right <= layout.width && layout.header.right <= layout.controls.x + 1 && layout.scrollWidth <= layout.width, `${label}: window controls stay in bounds without overlap`);
  for (const id of ["minimize", "maximize", "close"]) check(await page.locator(`#window-${id}`).isVisible(), `${label}: ${id} remains reachable`);
}
try {
  await page.goto(`${server.origin}/?scene=chat&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready); await ready();
  check(await page.locator("#workspace-header").count() === 1 && await page.locator("#main > #workspace-header").count() === 0, "Workspace actions are moved, not duplicated");
  check(await page.locator("#window-chrome #project-work-switch").isVisible() && await page.locator("#window-chrome #tasks-toggle").isVisible() && await page.locator("#window-chrome #preview-toggle").isVisible(), "Existing workspace navigation, activity and preview live in the same row");
  await checkLayout("dark chat");
  await page.screenshot({ path: `${output}/chat-dark-1440.png` });
  await page.locator("#window-maximize").click();
  await page.waitForFunction(() => document.querySelector("#window-maximize").getAttribute("aria-label") === "还原");
  check((await calls()).at(-1) === "toggleMaximize", "Maximize button calls the current native window");
  await ready(); await page.locator("#window-maximize").click(); await ready();
  check(await page.locator("#window-maximize").getAttribute("aria-label") === "最大化", "Restoring updates its accessible label and icon");
  await page.evaluate(() => window.__TAURI__.window.getCurrentWindow().toggleMaximize());
  await page.waitForFunction(() => document.querySelector("#window-maximize").getAttribute("aria-label") === "还原");
  check(true, "An external native resize/maximize updates the chrome state");
  await page.evaluate(() => window.__TAURI__.window.getCurrentWindow().toggleMaximize());
  await ready(); await page.locator("#window-minimize").click(); await ready();
  check((await calls()).at(-1) === "minimize", "Minimize calls the window API");

  let before = (await calls()).length;
  await page.locator("#workspace-project-name").dispatchEvent("mousedown", { button: 0, detail: 1 });
  await ready(); check((await calls()).slice(before).join() === "startDragging", "Noninteractive title starts native dragging");
  before = (await calls()).length;
  await page.locator("#workspace-project-name").dispatchEvent("mousedown", { button: 0, detail: 2 });
  await ready(); check((await calls()).slice(before).join() === "toggleMaximize", "Title double-mousedown toggles maximize without a second drag");
  before = (await calls()).length;
  for (const selector of ["#window-minimize", "#window-maximize", "#window-close", "#project-work-switch", "#tasks-toggle", "#preview-toggle"]) {
    await page.locator(selector).dispatchEvent("mousedown", { button: 0, detail: 1 });
    await page.locator(selector).dispatchEvent("mousedown", { button: 0, detail: 2 });
  }
  await page.evaluate(() => {
    for (const tag of ["input", "select", "a"]) {
      const element = document.createElement(tag); document.querySelector("#workspace-header").append(element);
      element.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0, detail: 1 })); element.remove();
    }
  });
  await page.locator("#workspace-project-name").dispatchEvent("mousedown", { button: 2, detail: 1 });
  check((await calls()).length === before, "Navigation, buttons, inputs, links, selects and right-click never drag or maximize");

  await page.locator('[data-work-surface="project"]').click(); await page.locator(".management-tabs").waitFor();
  await checkLayout("project overview");
  await page.screenshot({ path: `${output}/project-dark-1440.png` });
  for (const [view, title] of [["settings", "设置"], ["memory", "记忆"], ["skills", "Skills"], ["workspace", "所有项目"]]) {
    await navigate(view); await checkLayout(view);
    check(await page.locator("#workspace-project-name").innerText() === title, `${view}: global page title comes from existing view metadata`);
  }
  await navigate("settings");
  await page.screenshot({ path: `${output}/settings-dark-1440.png` });
  await page.evaluate(async () => (await import("/02-i18n.js")).setLanguagePreference("en"));
  check(await page.locator("#window-close").getAttribute("aria-label") === "Close window" && await page.locator("#workspace-project-name").innerText() === "Settings", "Page titles and window controls follow the selected language");

  for (const theme of ["dark", "light"]) {
    await page.goto(`${server.origin}/?scene=chat&theme=${theme}`);
    await page.waitForFunction(() => window.__kzPreview?.ready); await ready();
    await page.setViewportSize({ width: 800, height: 600 }); await checkLayout(`${theme} minimum native size`);
    await page.screenshot({ path: `${output}/chat-${theme}-800.png` });
    await page.locator("#rail-sidebar-toggle").click(); await checkLayout(`${theme} collapsed sidebar`);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.screenshot({ path: `${output}/chat-${theme}-1440.png` });
  }
  // Explicitly exercise the normal close event against the shared guard owner.
  await page.evaluate(async () => {
    const { registerLeaveGuard } = await import("/00-leave-guard.js");
    window.chromeGuard = registerLeaveGuard({ dirty: () => true, flush: async () => {}, error: error => { throw error; }, leave: async () => { window.chromeGuardAsked = true; return false; } });
    await window.chromeGuard.ready();
  });
  await page.locator("#window-close").click(); await ready();
  check(await page.evaluate(() => window.chromeGuardAsked && !window.__kzPreview.windowState.closed), "Close button enters shared leave guard; cancellation keeps the window open");
  await page.evaluate(() => { window.chromeGuard.dispose(); const win = window.__TAURI__.window.getCurrentWindow(); win.minimize = async () => { throw new Error("fixture native refusal"); }; });
  await page.locator("#window-minimize").click(); await ready();
  check((await page.locator("#window-chrome-status").innerText()).includes("fixture native refusal"), "Rejected native operations surface an error rather than failing silently");
  await page.locator("#window-close").focus(); await page.keyboard.press("Enter"); await ready();
  check(await page.evaluate(() => window.__kzPreview.windowState.closed && window.__kzPreview.windowCalls.at(-1) === "close"), "Keyboard close requests the normal native close when guards permit it");
  check(result.errors.length === 0, "No page errors in production modules");
  console.log(JSON.stringify(result, null, 2));
} finally {
  await writeFile(`${output}/result.json`, JSON.stringify(result, null, 2));
  await browser.close(); await server.close();
}
