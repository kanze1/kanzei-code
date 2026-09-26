import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

// 窗口尺寸 × 左侧栏开合 × 后台任务侧栏开合的真实布局冒烟(无头 Edge 打开静态 index.html,脚本不执行,
// 状态由这里按 06-agent-panel.js reconcileTasksPanel 的同一套判据手动写上)。
//
// UI2-0926 #14:用户推翻 R-334「这两个不用占用一个侧边栏」,活动/子代理两块浮层合成 Claude 式的停靠侧栏。
// 判据随之反转:停靠时侧栏在 #main 网格第 2 列,从顶部到状态栏上沿,对话列(输入区、上下文行)与运行日志
// 都不被它压住,状态栏保持全宽;停靠后对话列不足 600px(主区宽 − 侧栏宽)时改抽屉——absolute 盖在对话上、
// 遮罩可见、主区宽度不变。侧栏关闭时视图占满主区(非对话视图的尺寸与改前一致)。
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const htmlPath = path.join(root, "crates/kanzei-app/ui/index.html");
const viewports = [
  { width: 800, height: 500 },
  { width: 800, height: 600 },
  { width: 1024, height: 720 },
  { width: 1280, height: 840 },
  { width: 1440, height: 900 },
  { width: 1600, height: 1000 },
  { width: 2000, height: 1040 },
];
const states = [
  { sidebar: false, panel: false, log: false },
  { sidebar: true, panel: false, log: false },
  { sidebar: false, panel: true, log: false },
  { sidebar: true, panel: true, log: false },
  { sidebar: true, panel: true, log: true },
];
// 与 06-side-policy.js 同一口径:默认宽 clamp(360, 26vw, 520),夹到 [320, min(760, 主区 − 600)],
// 主区宽 − 侧栏宽 ≥ 600 才停靠。
const SIDE = { chatMin: 600, min: 320, max: 760 };
const url = `file:///${htmlPath.replace(/\\/g, "/")}`;
const browser = await chromium.launch({ channel: "msedge", headless: true });
const failures = [];
const baselineMainWidths = new Map();
let docked = 0;
let drawers = 0;
try {
  const page = await browser.newPage({ viewport: viewports[0] });
  await page.goto(url, { waitUntil: "domcontentloaded" });
  for (const viewport of viewports) {
    await page.setViewportSize(viewport);
    for (const state of states) {
      const result = await page.evaluate(({ sidebar, panel, log, SIDE }) => {
        const $ = (selector) => document.querySelector(selector);
        const main = $("#main");
        const tasks = $("#tasks-panel");
        const scrim = $("#tasks-scrim");
        const logPanel = $("#log-panel");
        const errors = [];
        if (!main || !tasks || !scrim) return { errors: ["#main / #tasks-panel / #tasks-scrim 缺失"], boxes: {} };
        $("#sidebar").classList.toggle("collapsed", !sidebar);
        logPanel.classList.toggle("hidden", !log);
        // 先关着量主区宽(侧栏在 #main 里,开合不改变 #main 本身的宽度)。
        tasks.classList.add("hidden");
        scrim.classList.add("hidden");
        main.dataset.side = "closed";
        main.style.setProperty("--kz-side-col", "0px");
        document.documentElement.style.setProperty("--kz-dock-right", "0px");
        const mainW = main.getBoundingClientRect().width;
        const width = Math.round(Math.min(Math.max(SIDE.min, Math.min(SIDE.max, mainW - SIDE.chatMin)),
          Math.max(SIDE.min, Math.min(520, Math.max(360, globalThis.innerWidth * 0.26)))));
        const dock = mainW - width >= SIDE.chatMin ? "side" : "drawer";
        if (panel) {
          tasks.classList.remove("hidden");
          tasks.dataset.dock = dock;
          tasks.style.setProperty("--kz-tasks-w", `${width}px`);
          main.dataset.side = dock === "side" ? "docked" : "drawer";
          main.style.setProperty("--kz-side-col", dock === "side" ? `${width}px` : "0px");
          document.documentElement.style.setProperty("--kz-dock-right", dock === "side" ? `${width}px` : "0px");
          scrim.classList.toggle("hidden", dock !== "drawer");
        }
        const rect = (selector) => {
          const el = $(selector);
          const box = el?.getBoundingClientRect();
          return box ? { left: box.left, top: box.top, right: box.right, bottom: box.bottom, width: box.width, height: box.height } : null;
        };
        const overlap = (a, b) => Boolean(a && b && a.left < b.right - 0.5 && a.right > b.left + 0.5 && a.top < b.bottom - 0.5 && a.bottom > b.top + 0.5);
        const viewport = { width: globalThis.innerWidth, height: globalThis.innerHeight };
        const content = ["#main", ".empty-state", "#prompt", "#composer-context", "#composer-bar", "#statusbar"];
        const boxes = Object.fromEntries(content.map((selector) => [selector, rect(selector)]));
        for (const [selector, box] of Object.entries(boxes)) {
          if (!box || box.width <= 0 || box.height <= 0) errors.push(`${selector} 不可见`);
          if (box && (box.left < -1 || box.top < -1 || box.right > viewport.width + 1 || box.bottom > viewport.height + 1)) {
            errors.push(`${selector} 越出视口 ${JSON.stringify(box)}`);
          }
        }
        const mainBox = boxes["#main"];
        // ≤900px 时左侧栏是盖在主区上的抽屉,主区用 padding-left 让位(D-712):全宽按主区内容盒算。
        const mainStyle = globalThis.getComputedStyle(main);
        const inner = mainBox && { left: mainBox.left + parseFloat(mainStyle.paddingLeft), right: mainBox.right - parseFloat(mainStyle.paddingRight) };
        const status = boxes["#statusbar"];
        const panelBox = panel ? rect("#tasks-panel") : null;
        const sidebarBox = sidebar ? rect("#sidebar") : null;
        // 状态栏始终全宽(跨网格两列)。
        if (status && inner && (Math.abs(status.left - inner.left) > 1 || Math.abs(status.right - inner.right) > 1)) {
          errors.push(`状态栏不是主区全宽 ${JSON.stringify({ status, main: inner })}`);
        }
        if (!panel) {
          const chat = rect("#view-chat");
          if (chat && inner && Math.abs(chat.width - (inner.right - inner.left)) > 1) errors.push(`侧栏关闭时对话视图没有占满主区(${chat.width} ≠ ${inner.right - inner.left})`);
        }
        if (panelBox) {
          const style = globalThis.getComputedStyle(tasks);
          if (panelBox.width <= 0 || panelBox.height <= 0) errors.push("侧栏不可见");
          if (panelBox.left < -1 || panelBox.top < -1 || panelBox.right > viewport.width + 1 || panelBox.bottom > viewport.height + 1) {
            errors.push(`侧栏越出视口 ${JSON.stringify(panelBox)}`);
          }
          // 两种形态都从主区顶部到状态栏上沿、贴主区右边。
          if (mainBox && Math.abs(panelBox.top - mainBox.top) > 1) errors.push(`侧栏上沿不在主区顶部(${panelBox.top} ≠ ${mainBox.top})`);
          if (status && Math.abs(panelBox.bottom - status.top) > 1) errors.push(`侧栏下沿不在状态栏上沿(${panelBox.bottom} ≠ ${status.top})`);
          if (mainBox && Math.abs(panelBox.right - mainBox.right) > 1) errors.push(`侧栏没有贴主区右边(${panelBox.right} ≠ ${mainBox.right})`);
          if (dock === "side") {
            if (style.position === "absolute") errors.push("停靠态侧栏不该是 absolute(应占网格第 2 列、推开对话列)");
            if (Math.abs(panelBox.width - width) > 1) errors.push(`停靠态侧栏宽 ${panelBox.width} ≠ ${width}`);
            for (const selector of [".empty-state", "#prompt", "#composer-context", "#composer-bar"]) {
              if (overlap(boxes[selector], panelBox)) errors.push(`${selector} 被停靠的侧栏压住`);
            }
            if (log) {
              const logBox = rect("#log-panel");
              if (!logBox || logBox.height <= 0) errors.push("运行日志不可见");
              else if (overlap(logBox, panelBox)) errors.push("运行日志被停靠的侧栏压住(日志应随对话列变窄)");
            }
          } else {
            if (style.position !== "absolute") errors.push(`抽屉态侧栏必须 absolute(盖在对话上),实为 ${style.position}`);
            const scrimBox = rect("#tasks-scrim");
            if (!scrimBox || scrimBox.width <= 0 || globalThis.getComputedStyle(scrim).display === "none") errors.push("抽屉态遮罩不可见");
          }
        }
        for (const selector of [".hint", "#prompt", "#composer-context", "#composer-bar"]) {
          const box = rect(selector);
          if (box && sidebarBox && overlap(box, sidebarBox)) errors.push(`${selector} 与侧栏重叠`);
        }
        if (status && sidebarBox && overlap(status, sidebarBox)) errors.push("状态栏与左侧栏重叠");
        const focusTarget = $("#rail-sidebar-toggle");
        focusTarget.focus();
        const focusBox = focusTarget.getBoundingClientRect();
        if (focusBox.left < 0 || focusBox.top < 0 || focusBox.right > viewport.width || focusBox.bottom > viewport.height) {
          errors.push(`键盘焦点越出视口 ${JSON.stringify(focusBox)}`);
        }
        // 复位,别把状态带进下一轮。
        tasks.classList.add("hidden");
        scrim.classList.add("hidden");
        logPanel.classList.add("hidden");
        main.style.setProperty("--kz-side-col", "0px");
        return { errors, boxes, dock, width, mainW };
      }, { ...state, SIDE });
      const label = `${viewport.width}x${viewport.height} sidebar=${state.sidebar} panel=${state.panel} log=${state.log}`;
      const baselineKey = `${viewport.width}x${viewport.height}:sidebar=${state.sidebar}`;
      const mainWidth = result.boxes["#main"]?.width;
      if (!state.panel) baselineMainWidths.set(baselineKey, mainWidth);
      else {
        if (result.dock === "side") docked += 1;
        else drawers += 1;
        // 侧栏在 #main 里:停靠只把对话列让出来,抽屉盖在上面——两种形态都不改变主区本身的宽度。
        if (mainWidth === undefined || Math.abs(mainWidth - baselineMainWidths.get(baselineKey)) > 0.5) {
          failures.push(`${label}: 主区宽度被侧栏改变 (${mainWidth} != ${baselineMainWidths.get(baselineKey)})`);
        }
      }
      if (result.errors.length) failures.push(`${label} dock=${result.dock} w=${result.width}: ${result.errors.join("; ")}`);
    }
  }
} finally {
  await browser.close();
}
assert.deepEqual(failures, [], `窄窗口布局回归失败:\n${failures.join("\n")}`);
// 判据自证:宽窗口必须出现停靠,窄窗口必须出现抽屉——两种形态都真的被测到。
assert.ok(docked >= 4 && drawers >= 4, `停靠/抽屉两种形态没有都覆盖到(停靠 ${docked} 次、抽屉 ${drawers} 次)`);
console.log(`UI 面板布局冒烟通过：${viewports.length} 个视口 × ${states.length} 个左侧栏/后台任务侧栏/日志状态;停靠 ${docked} 次(不压对话列与日志、到状态栏上沿、状态栏全宽)、抽屉 ${drawers} 次(absolute + 遮罩、主区宽度不变)`);
