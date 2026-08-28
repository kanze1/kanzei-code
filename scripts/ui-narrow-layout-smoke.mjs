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
  { width: 1440, height: 900 },
];
const states = [
  { sidebar: false, panel: "none" },
  { sidebar: true, panel: "none" },
  { sidebar: false, panel: "activity" },
  { sidebar: false, panel: "agent" },
  { sidebar: true, panel: "activity" },
  { sidebar: true, panel: "agent" },
];
const url = `file:///${htmlPath.replace(/\\/g, "/")}`;
const browser = await chromium.launch({ channel: "msedge", headless: true });
const failures = [];
const baselineMainWidths = new Map();
try {
  const page = await browser.newPage({ viewport: viewports[0] });
  await page.goto(url, { waitUntil: "domcontentloaded" });
  for (const viewport of viewports) {
    await page.setViewportSize(viewport);
    for (const state of states) {
      const result = await page.evaluate(({ sidebar, panel }) => {
        const sidebarEl = document.querySelector("#sidebar");
        const bgPanel = document.querySelector("#bg-panel");
        const agentPanel = document.querySelector("#agent-panel");
        sidebarEl.classList.toggle("collapsed", !sidebar);
        bgPanel.classList.toggle("hidden", panel !== "activity");
        agentPanel.classList.toggle("hidden", panel !== "agent");
        const rect = (selector) => {
          const el = document.querySelector(selector);
          const box = el?.getBoundingClientRect();
          return box ? { left: box.left, top: box.top, right: box.right, bottom: box.bottom, width: box.width, height: box.height } : null;
        };
        const overlap = (a, b) => Boolean(a && b && a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top);
        const viewport = { width: globalThis.innerWidth, height: globalThis.innerHeight };
        const content = ["#main", ".empty-state", "#prompt", "#composer-context", "#composer-bar", "#statusbar"];
        const boxes = Object.fromEntries(content.map((selector) => [selector, rect(selector)]));
        const panelSelector = panel === "activity" ? "#bg-panel" : panel === "agent" ? "#agent-panel" : null;
        const panelBox = panelSelector ? rect(panelSelector) : null;
        const sidebarBox = sidebar ? rect("#sidebar") : null;
        const errors = [];
        for (const [selector, box] of Object.entries(boxes)) {
          if (!box || box.width <= 0 || box.height <= 0) errors.push(`${selector} 不可见`);
          if (box && (box.left < -1 || box.top < -1 || box.right > viewport.width + 1 || box.bottom > viewport.height + 1)) {
            errors.push(`${selector} 越出视口 ${JSON.stringify(box)}`);
          }
        }
        if (panelBox) {
          const style = globalThis.getComputedStyle(document.querySelector(panelSelector));
          if (style.position !== "absolute") errors.push(`${panelSelector} 未脱离主区布局: position=${style.position}`);
          if (panelBox.width <= 0 || panelBox.height <= 0) errors.push(`${panelSelector} 浮层不可见`);
          if (panelBox.left < -1 || panelBox.top < -1 || panelBox.right > viewport.width + 1 || panelBox.bottom > viewport.height + 1) {
            errors.push(`${panelSelector} 浮层越出视口 ${JSON.stringify(panelBox)}`);
          }
        }
        for (const selector of [".hint", "#prompt", "#composer-context", "#composer-bar"]) {
          const box = rect(selector);
          if (box && sidebarBox && overlap(box, sidebarBox)) errors.push(`${selector} 与侧栏重叠`);
        }
        if (boxes["#statusbar"] && sidebarBox && overlap(boxes["#statusbar"], sidebarBox)) errors.push("状态栏与侧栏重叠");
        const visiblePanels = [bgPanel, agentPanel].filter((el) => !el.classList.contains("hidden"));
        if (visiblePanels.length > 1) errors.push("活动与子代理浮层同时可见");
        const focusTarget = document.querySelector("#rail-sidebar-toggle");
        focusTarget.focus();
        const focusBox = focusTarget.getBoundingClientRect();
        if (focusBox.left < 0 || focusBox.top < 0 || focusBox.right > viewport.width || focusBox.bottom > viewport.height) {
          errors.push(`键盘焦点越出视口 ${JSON.stringify(focusBox)}`);
        }
        return { errors, boxes, panelBox, panelSelector, sidebarBox };
      }, state);
      const baselineKey = `${viewport.width}x${viewport.height}:sidebar=${state.sidebar}`;
      const mainWidth = result.boxes["#main"]?.width;
      if (state.panel === "none") baselineMainWidths.set(baselineKey, mainWidth);
      else if (mainWidth === undefined || Math.abs(mainWidth - baselineMainWidths.get(baselineKey)) > 0.5) {
        failures.push(`${viewport.width}x${viewport.height} sidebar=${state.sidebar} panel=${state.panel}: 主区宽度被浮层改变 (${mainWidth} != ${baselineMainWidths.get(baselineKey)})`);
      }
      if (result.errors.length) failures.push(`${viewport.width}x${viewport.height} sidebar=${state.sidebar} panel=${state.panel}: ${result.errors.join("; ")}`);
    }
  }
} finally {
  await browser.close();
}
assert.deepEqual(failures, [], `窄窗口布局回归失败:\n${failures.join("\n")}`);
console.log(`UI 面板布局冒烟通过：${viewports.length} 个视口 × ${states.length} 个侧栏/浮层状态，主区宽度保持不变，浮层无越界且互斥`);
