import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const htmlPath = path.join(root, "crates/kanzei-app/ui/index.html");
const viewports = [
  { width: 800, height: 500 },
  { width: 800, height: 600 },
  { width: 1024, height: 720 },
  { width: 1280, height: 840 },
];
const states = [
  { sidebar: false, drawer: false },
  { sidebar: true, drawer: false },
  { sidebar: false, drawer: true },
];
const url = `file:///${htmlPath.replace(/\\/g, "/")}`;
const browser = await chromium.launch({ channel: "msedge", headless: true });
const failures = [];
try {
  const page = await browser.newPage({ viewport: viewports[0] });
  await page.goto(url, { waitUntil: "domcontentloaded" });
  for (const viewport of viewports) {
    await page.setViewportSize(viewport);
    for (const state of states) {
      const result = await page.evaluate(({ sidebar, drawer }) => {
        const sidebarEl = document.querySelector("#sidebar");
        const bgPanel = document.querySelector("#bg-panel");
        const agentPanel = document.querySelector("#agent-panel");
        sidebarEl.classList.toggle("collapsed", !sidebar);
        bgPanel.classList.toggle("hidden", !drawer);
        agentPanel.classList.add("hidden");
        const rect = (selector) => {
          const el = document.querySelector(selector);
          const box = el?.getBoundingClientRect();
          return box ? { left: box.left, top: box.top, right: box.right, bottom: box.bottom, width: box.width, height: box.height } : null;
        };
        const overlap = (a, b) => Boolean(a && b && a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top);
        const viewport = { width: globalThis.innerWidth, height: globalThis.innerHeight };
        const content = [".empty-state", "#prompt", "#composer-context", "#composer-bar", "#statusbar"];
        const boxes = Object.fromEntries(content.map((selector) => [selector, rect(selector)]));
        const drawerBox = drawer ? rect("#bg-panel") : null;
        const sidebarBox = sidebar ? rect("#sidebar") : null;
        const errors = [];
        for (const [selector, box] of Object.entries(boxes)) {
          if (!box || box.width <= 0 || box.height <= 0) errors.push(`${selector} 不可见`);
          if (box && (box.left < -1 || box.top < -1 || box.right > viewport.width + 1 || box.bottom > viewport.height + 1)) {
            errors.push(`${selector} 越出视口 ${JSON.stringify(box)}`);
          }
        }
        for (const selector of [".hint", "#prompt", "#composer-context", "#composer-bar"]) {
          const box = rect(selector);
          if (box && sidebarBox && overlap(box, sidebarBox)) errors.push(`${selector} 与侧栏重叠`);
          if (box && drawerBox && overlap(box, drawerBox)) errors.push(`${selector} 与右抽屉重叠`);
        }
        if (boxes["#statusbar"] && sidebarBox && overlap(boxes["#statusbar"], sidebarBox)) errors.push("状态栏与侧栏重叠");
        if (boxes["#statusbar"] && drawerBox && overlap(boxes["#statusbar"], drawerBox)) errors.push("状态栏与右抽屉重叠");
        const focusTarget = document.querySelector("#rail-sidebar-toggle");
        focusTarget.focus();
        const focusBox = focusTarget.getBoundingClientRect();
        if (focusBox.left < 0 || focusBox.top < 0 || focusBox.right > viewport.width || focusBox.bottom > viewport.height) {
          errors.push(`键盘焦点越出视口 ${JSON.stringify(focusBox)}`);
        }
        return { errors, boxes, sidebarBox, drawerBox };
      }, state);
      if (result.errors.length) failures.push(`${viewport.width}x${viewport.height} sidebar=${state.sidebar} drawer=${state.drawer}: ${result.errors.join("; ")}`);
    }
  }
} finally {
  await browser.close();
}
assert.deepEqual(failures, [], `窄窗口布局回归失败:\n${failures.join("\n")}`);
console.log(`UI 窄窗口布局冒烟通过：${viewports.length} 个视口 × ${states.length} 个侧栏/右抽屉状态，0 重叠/截断/越界`);
