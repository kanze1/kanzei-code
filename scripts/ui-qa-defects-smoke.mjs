// UX 手测整改第二轮验收缺陷的真实浏览器回归(预览夹具 + 无头 Edge)。
// 每一节对应一条验收缺陷,断言的是「用户看得到的结果」而不是实现细节:
//   窄窗抽屉点会话/Esc 收起、拖动排序越界或 Esc 取消、对话搜索 Esc 与切换对话重算、概览页 toast 与权限卡不压输入卡、
//   概览等批准转「待你回复」、概览内二级页 Esc 先回概览、状态栏上下文读数不截断、线路卡「停止」即时反馈、
//   设置页成对输入框整对折行与 Provider 表横向滚动、需求页「＋ 新建」展开态文字可读、「新建记忆」清掉目标、
//   ◉ 后台常驻菜单锚在按钮上、自动放行/提示音写 app.json 而不是 localStorage、右键菜单的重命名限长与「添加打开方式」直达设置分区、
//   研究空间命令面板不列做不了的条目、「待我处理」侧栏高亮。
// 文件页编辑器那一组(首次点保存、切树 Canceled、html 404)在 ui-files-editor-smoke.mjs。
/* global window, document, getComputedStyle, localStorage */
import assert from "node:assert/strict";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const passed = [];
const check = (value, label) => { assert(value, label); passed.push(label); };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const PROJECT = "C:/Users/kanzei/Documents/kanzei code";

async function open({ scene, theme = "light", width = 1333, height = 695, dpr = 1, init = null }) {
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: dpr, colorScheme: theme });
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));
  page.on("console", (message) => { if (message.type() === "error") errors.push(`console.error: ${message.text()}`); });
  if (init) await page.addInitScript(init);
  await page.goto(`${server.origin}/?${new URLSearchParams({ theme, scene })}`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.waitForTimeout(500);
  return {
    page, errors,
    settle: (ms) => page.evaluate((value) => window.__kzPreview.settle(value), ms),
    calls: (cmd) => page.evaluate((name) => window.__kzPreview.calls.filter((call) => !name || call.cmd === name).map((call) => ({ cmd: call.cmd, args: call.args })), cmd ?? null),
    emit: (event, payload) => page.evaluate(([name, value]) => window.__kzPreview.emit(name, value), [event, payload]),
    nav: (view) => page.evaluate(async (name) => (await import("/03-shell.js")).navigate_view(name), view),
    view: () => page.evaluate(() => document.body.dataset.view),
    rect: (selector) => page.evaluate((sel) => {
      const element = document.querySelector(sel);
      if (!element) return null;
      const box = element.getBoundingClientRect();
      return { x: box.x, y: box.y, w: box.width, h: box.height, bottom: box.bottom, right: box.right };
    }, selector),
    close: () => context.close(),
  };
}
/// 每节一个独立页面;节内断言完后,整节再断言「零 console / 未捕获错误」。
async function section(name, options, body) {
  const app = await open(options);
  try {
    await body(app);
    check(app.errors.length === 0, `${name}: zero console errors (${app.errors.join("; ")})`);
  } finally {
    await app.close();
  }
}
const rgb = (text) => String(text).match(/[\d.]+/g).slice(0, 3).map(Number);
const luminance = ([r, g, b]) => {
  const channel = (value) => { const v = value / 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
};
const contrast = (a, b) => { const [hi, lo] = [luminance(rgb(a)), luminance(rgb(b))].sort((x, y) => y - x); return (hi + 0.05) / (lo + 0.05); };
const permissionAsk = (id, sessionId) => ({
  id, kind: "permission", sessionId, source: "primary", action: "bash",
  resource: JSON.stringify({ command: `cargo test -p kanzei-tools tool_search_${id}`, workdir: PROJECT }),
  remember: JSON.stringify({ command: `cargo test -p kanzei-tools tool_search_${id}`, workdir: PROJECT }),
});

try {
  // ---- 窄窗悬浮抽屉:点会话行收起;Esc 先收抽屉、再退页(UX-152 验收缺陷) ----
  await section("drawer", { scene: "startup", width: 800, height: 700 }, async (app) => {
    const { page } = app;
    const expanded = () => page.getAttribute("#rail-sidebar-toggle", "aria-expanded");
    await page.click("#rail-sidebar-toggle");
    await page.waitForFunction(() => document.querySelector("#rail-sidebar-toggle").getAttribute("aria-expanded") === "true");
    const row = page.locator('.workbench-session-link[aria-current="false"]').first();
    const name = (await row.locator(".workbench-session-name").textContent()).trim();
    await row.click();
    await page.waitForFunction(() => document.querySelector("#rail-sidebar-toggle").getAttribute("aria-expanded") === "false", null, { timeout: 3000 });
    const current = (await page.locator('.workbench-session-link[aria-current="true"] .workbench-session-name').first().textContent()).trim();
    check(current === name, `drawer: clicking a conversation row switches to it (${name}) and collapses the drawer`);

    await sleep(1000); // 切会话的收尾(回到对话页)是异步的;人不会在它之前再点下一步
    await app.nav("documents");
    await page.waitForFunction(() => document.body.dataset.view === "documents");
    await page.click("#rail-sidebar-toggle");
    await page.waitForFunction(() => document.querySelector("#rail-sidebar-toggle").getAttribute("aria-expanded") === "true");
    await sleep(400);
    await page.keyboard.press("Escape");
    await sleep(400);
    const afterFirst = { drawer: await expanded(), view: await app.view() };
    check(afterFirst.drawer === "false" && afterFirst.view === "documents", `drawer: the first Esc closes the open drawer and leaves the page where it is (${JSON.stringify(afterFirst)})`);
    await page.keyboard.press("Escape");
    await page.waitForFunction(() => document.body.dataset.view === "chat", null, { timeout: 3000 });
    check(true, "drawer: the second Esc goes back one page");
  });

  // ---- 拖动排序:拖出侧栏松手 / 拖动中 Esc 都是取消,不写排序、不留拖动标记 ----
  await section("drag", { scene: "startup" }, async (app) => {
    const { page } = app;
    for (let i = 0; i < 3; i += 1) {
      await page.evaluate(async (index) => (await import("/01-core.js")).invoke("process_create", { projectDir: "C:/Users/kanzei/Documents/kanzei code", profile: index === 0 ? "readonly" : "dev" }), i);
    }
    await page.evaluate(async () => (await import("/09-sessions.js")).refreshProcesses());
    const rows = page.locator("#workbench-project-list .workbench-session-list .workbench-session-link");
    await page.waitForFunction(() => document.querySelectorAll("#workbench-project-list .workbench-session-list .workbench-session-link").length >= 5);
    const orderWrites = async () => (await app.calls("ui_prefs_set")).filter((call) => call.args?.ui_layout?.session_order).length;
    const before = await orderWrites();
    const last = await rows.nth(await rows.count() - 1).boundingBox();
    await page.mouse.move(last.x + 40, last.y + last.height / 2);
    await page.mouse.down();
    await page.mouse.move(last.x + 60, last.y - 20, { steps: 4 });
    await page.mouse.move(900, 300, { steps: 8 });
    await page.mouse.up();
    await sleep(500);
    check(await orderWrites() === before, "drag: releasing the drag outside the sidebar cancels it (no order is saved)");
    const again = await rows.nth(await rows.count() - 1).boundingBox();
    await page.mouse.move(again.x + 40, again.y + again.height / 2);
    await page.mouse.down();
    await page.mouse.move(again.x + 40, again.y - 40, { steps: 6 });
    await page.keyboard.press("Escape");
    await sleep(200);
    check(await page.evaluate(() => document.querySelectorAll(".drop-before, .drop-after, .is-dragging").length) === 0, "drag: Esc mid-drag clears the drag marks");
    await page.mouse.up();
    await sleep(500);
    check(await orderWrites() === before && await app.view() === "chat", "drag: Esc cancels the reorder and is not taken as \"go back\"");
  });

  // ---- 对话搜索条:Esc 在对话区任意位置关;切换对话后命中与计数按新对话重算 ----
  await section("search", { scene: "column" }, async (app) => {
    const { page } = app;
    const state = () => page.evaluate(() => ({ hidden: document.getElementById("chat-search").classList.contains("hidden"), count: document.getElementById("chat-search-count").textContent, hits: document.querySelectorAll(".search-hit").length }));
    await page.click("#prompt");
    await page.keyboard.press("Control+f");
    await page.keyboard.type("R-001");
    await page.waitForFunction(() => document.querySelectorAll(".search-hit").length > 0);
    await page.mouse.click(420, 250); // 点进消息区:焦点离开搜索框
    await page.keyboard.press("Escape");
    await sleep(200);
    check((await state()).hidden, "search: Esc closes the search bar even when the focus is in the message area");
    await page.keyboard.press("Control+f");
    await page.keyboard.type("R-001");
    await page.waitForFunction(() => document.querySelectorAll(".search-hit").length > 0);
    await page.locator("#sidebar").getByText("R-366 B2 回退事件").first().click();
    await page.waitForFunction(() => document.getElementById("chat-search-count").textContent === "0/0", null, { timeout: 4000 });
    const after = await state();
    check(!after.hidden && after.hits === 0, "search: switching conversation recounts the hits for the new one (no stale \"1/4\" and no leftover highlights)");
  });

  // ---- 概览页:toast 与权限卡锚在输入卡上沿,不压输入卡;等批准时「工具」行转琥珀「待你回复」 ----
  for (const [theme, width, height] of [["light", 1333, 695], ["dark", 1000, 700]]) {
    await section(`overview ${theme} ${width}`, { scene: "column", theme, width, height }, async (app) => {
      const { page } = app;
      const meta = await page.evaluate(async () => (await window.__kzPreview.fixtures()).events.meta);
      await page.click('#project-work-switch [data-work-surface="project"]');
      await page.waitForFunction(() => document.body.dataset.view === "project" && document.querySelector("#softwire-workspace .sw-network"));
      await page.evaluate(async () => (await import("/03-shell.js")).toast("位置测试(概览页)"));
      await page.waitForSelector(".k-toast");
      const toast = await app.rect(".k-toast"), composer = await app.rect("#composer");
      check(toast.bottom <= composer.y + 1, `overview ${width}: a toast sits above the composer card, not on its tool row (${Math.round(toast.bottom)} <= ${Math.round(composer.y)})`);
      await page.waitForTimeout(3600);
      await app.emit("kz:meta", meta);
      await app.emit("kz:turn", { sessionId: meta.sessionId, step: 1, maxSteps: 0 });
      await app.emit("kz:tool-start", { sessionId: meta.sessionId, id: "b1", name: "bash", summary: "cargo test", input: { command: "cargo test" } });
      await app.emit("kz:status", { sessionId: meta.sessionId, stage: "权限", detail: "等待你批准 bash" });
      await app.emit("kz:ask", permissionAsk(301, meta.sessionId));
      await page.waitForFunction(() => document.querySelector("#ask-overlay")?.matches(":popover-open"));
      await sleep(400);
      const card = await app.rect("#ask-overlay"), send = await app.rect("#send"), after = await app.rect("#composer");
      check(card.bottom <= after.y + 8, `overview ${width}: the permission card docks above the composer card (${Math.round(card.bottom)} <= ${Math.round(after.y)})`);
      check(card.bottom <= send.y || card.x >= send.right || card.right <= send.x, `overview ${width}: the permission card never covers the send / stop buttons`);
      const tools = await page.evaluate(() => {
        const node = document.querySelector('.sw-lane [data-module="tools"]');
        return { state: node?.dataset.state, text: node?.closest(".sw-lane")?.querySelector("small")?.textContent };
      });
      check(tools.state === "attention" && /待你回复/.test(tools.text), `overview ${width}: waiting on a permission shows the amber "needs you" state on the tools row (${tools.state} / ${tools.text})`);
    });
  }

  // ---- 概览内二级页:Esc 先回概览、再按一次回对话 ----
  await section("overview-back", { scene: "column" }, async (app) => {
    const { page } = app;
    const where = () => page.evaluate(() => ({ view: document.body.dataset.view, network: Boolean(document.querySelector("#softwire-workspace .sw-network")) }));
    await page.click('#project-work-switch [data-work-surface="project"]');
    await page.waitForFunction(() => document.querySelector("#softwire-workspace .sw-network"));
    await page.click('[data-module="work"]');
    await page.waitForFunction(() => !document.querySelector("#softwire-workspace .sw-network"));
    await page.keyboard.press("Escape");
    await sleep(500);
    const first = await where();
    check(first.view === "project" && first.network, "overview-back: Esc inside a module page returns to the overview first");
    await page.keyboard.press("Escape");
    await sleep(500);
    check((await where()).view === "chat", "overview-back: the next Esc leaves the overview for the conversation");
  });

  // ---- 状态栏上下文读数:1000 宽也完整显示「占用/上限(百分比)」 ----
  await section("status-tokens", { scene: "chat", width: 1000, height: 700 }, async (app) => {
    const info = await app.page.evaluate(() => {
      const button = document.querySelector("#status-tokens");
      const main = button.querySelector(".ctx-main");
      return { text: main?.textContent ?? "", clipped: !main || main.getBoundingClientRect().right > button.getBoundingClientRect().right + 1 };
    });
    check(/\d+(?:\.\d+)?k\/\d+k \(\d+%\)/.test(info.text) && !info.clipped, `status-tokens: at 1000px the context reading keeps used/limit and the percentage (${info.text})`);
  });

  // ---- 线路卡「停止」:点击立刻进「停止中…」、按钮禁用、只发一次 stop_run ----
  await section("lines-stop", { scene: "lines" }, async (app) => {
    const { page } = app;
    const lane = "p|thread-r366-b2";
    await page.evaluate(async (id) => {
      const fixtures = await window.__kzPreview.fixtures();
      const original = fixtures.commands.collaboration_snapshot;
      let stopped = false;
      // 真实后端在停止完成前一直报 running;完成后才有终态事件。
      window.__kzPreview.setCommand("collaboration_snapshot", (args, ctx) => original(args, ctx).map((row) => (row.process_id === id ? { ...row, running: !stopped, status: stopped ? "stopped" : "running" } : row)));
      window.__kzPreview.setCommand("stop_run", (args, ctx) => {
        setTimeout(() => { stopped = true; ctx.emit("kz:stopped", { sessionId: "ses_01J8Q9V1R2T3Y4U5I6O7P8A9S0" }); }, 1500);
        return null;
      });
    }, lane);
    // 状态用 .line-running-state 的状态类(running / stopping / stopped),不依赖界面用词。
    const state = () => page.evaluate((id) => {
      const card = document.querySelector(`.line-lane[data-process-id="${id}"]`);
      const stop = card.querySelector('[data-focus-key="stop"]');
      return { status: [...(card.querySelector(".line-running-state")?.classList ?? [])].filter((name) => name !== "line-running-state").join(" "), stop: stop ? { disabled: stop.disabled } : null };
    }, lane);
    const idle = await state();
    check(idle.status === "running" && idle.stop?.disabled === false, `lines-stop: a running line shows the running state with an enabled stop button (${idle.status})`);
    await page.click(`.line-lane[data-process-id="${lane}"] [data-focus-key="stop"]`);
    await sleep(500);
    const pressed = await state();
    check(pressed.status === "stopping" && pressed.stop?.disabled === true, `lines-stop: the card turns into the stopping state with a disabled button right away (${pressed.status})`);
    await page.waitForFunction((id) => document.querySelector(`.line-lane[data-process-id="${id}"] .line-running-state`)?.classList.contains("stopped"), lane, { timeout: 8000 });
    check((await app.calls("stop_run")).length === 1, "lines-stop: exactly one stop_run is sent and the terminal event refreshes the card to stopped");
  });

  // ---- 设置页:成对输入框整对折行(1000 宽);Provider 表横向滚动,名称列够宽 ----
  await section("settings-1000", { scene: "settings", width: 1000, height: 700 }, async (app) => {
    const { page } = app;
    await page.evaluate(() => document.querySelectorAll("#view-settings details.settings-group").forEach((group) => { group.open = true; }));
    await sleep(300);
    const pairs = await page.evaluate(() => [...document.querySelectorAll("#sg-limits .form-pair")].map((pair) => {
      const label = pair.querySelector("label").getBoundingClientRect(), input = pair.querySelector("input").getBoundingClientRect();
      return { apart: Math.abs((label.top + label.height / 2) - (input.top + input.height / 2)), id: pair.querySelector("input").id };
    }));
    check(pairs.length >= 5 && pairs.every((pair) => pair.apart < 20), `settings-1000: every label + input pair of 高级运行参数 stays together on one row (${pairs.map((pair) => Math.round(pair.apart)).join(",")})`);
    check(await page.evaluate(() => { const view = document.querySelector("#view-settings"); return view.scrollWidth <= view.clientWidth + 1; }), "settings-1000: the settings page has no horizontal scroll");
    const providers = await page.evaluate(() => {
      const scroll = document.querySelector(".providers-scroll");
      const name = document.querySelector("#providers-table tbody tr input");
      return { overflow: scroll ? getComputedStyle(scroll).overflowX : "", nameWidth: name?.getBoundingClientRect().width ?? 0 };
    });
    check(providers.overflow === "auto" && providers.nameWidth >= 100, `settings-1000: the provider table scrolls sideways and keeps the name column readable (${Math.round(providers.nameWidth)}px)`);
  });

  // ---- 需求页「＋ 新建」展开态(含悬停)文字对比度 ≥ 4.5 ----
  for (const theme of ["light", "dark"]) {
    await section(`documents-new ${theme}`, { scene: "docs", theme }, async (app) => {
      const { page } = app;
      await page.click("#documents-new-toggle");
      await sleep(250);
      const box = await page.locator("#documents-new-toggle").boundingBox();
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await sleep(250);
      const colors = await page.evaluate(() => { const style = getComputedStyle(document.querySelector("#documents-new-toggle")); return { fg: style.color, bg: style.backgroundColor, expanded: document.querySelector("#documents-new-toggle").getAttribute("aria-expanded") }; });
      check(colors.expanded === "true" && contrast(colors.fg, colors.bg) >= 4.5, `documents-new ${theme}: the expanded + hovered New button stays readable (${colors.fg} on ${colors.bg} = ${contrast(colors.fg, colors.bg).toFixed(2)})`);
    });
  }

  // ---- 决策复核「待复核」状态字在白底上 ≥ 4.5:1(琥珀小字用 --accent-text,不用填充色 --accent) ----
  await section("decision-status-contrast", { scene: "workspace", theme: "light" }, async (app) => {
    const { page } = app;
    await page.click('.console-tab[data-console-tab="decisions"]');
    await sleep(400);
    const worst = await page.evaluate(() => {
      const rows = [...document.querySelectorAll(".console-status.decided")].map((element) => {
        let background = "rgb(255, 255, 255)";
        for (let node = element; node; node = node.parentElement) {
          const color = getComputedStyle(node).backgroundColor;
          if (color && !/rgba\(.*,\s*0\)$|transparent/.test(color)) { background = color; break; }
        }
        return { fg: getComputedStyle(element).color, bg: background };
      });
      return rows;
    });
    check(worst.length > 0 && worst.every((row) => contrast(row.fg, row.bg) >= 4.5), `decision-status-contrast: the amber 待复核 status text reads at 4.5:1 or better on white (${worst.map((row) => `${row.fg} on ${row.bg}`).slice(0, 2).join("; ")})`);
  });

  // ---- 「新建记忆」:先清掉选中条目,管理对话发出去的 target 为空 ----
  await section("memory-new", { scene: "memory" }, async (app) => {
    const { page } = app;
    await page.locator("#memory-list .memory-row").nth(4).click();
    await sleep(300);
    await page.click("#memory-new-btn");
    await sleep(300);
    await page.keyboard.type("记住:提交前跑 verify");
    await page.click("#memory-chat-send");
    await page.waitForFunction(() => window.__kzPreview.calls.some((call) => call.cmd === "memory_chat_send"));
    const sent = (await app.calls("memory_chat_send")).at(-1);
    check(sent.args.target === null || sent.args.target === undefined, `memory-new: starting a new memory does not target the previously selected one (${JSON.stringify(sent.args.target)})`);
  });

  // ---- ◉ 后台常驻:菜单锚在按钮上方,不落在窗口左上角 ----
  await section("runtime-chip", {
    scene: "composer",
    init: () => {
      let real;
      Object.defineProperty(window, "__TAURI__", {
        configurable: true,
        get() { return real; },
        set(value) { const invoke = value.core.invoke; value.core.invoke = (cmd, args) => (cmd === "runtime_status" ? Promise.resolve({ mode: "detached", connected: true }) : invoke(cmd, args)); real = value; },
      });
    },
  }, async (app) => {
    const { page } = app;
    await page.waitForFunction(() => { const chip = document.querySelector("#runtime-indicator"); return chip && !chip.hidden && chip.getBoundingClientRect().width > 0; });
    await page.click("#runtime-indicator");
    await page.waitForSelector(".k-menu:popover-open");
    await sleep(250);
    const chip = await app.rect("#runtime-indicator"), menu = await page.evaluate(() => { const box = document.querySelector(".k-menu:popover-open").getBoundingClientRect(); return { x: box.x, y: box.y, bottom: box.bottom, right: box.right }; });
    check(menu.x > 100 && menu.bottom <= chip.y + 8 && chip.y - menu.bottom < 80 && Math.abs(menu.right - chip.right) < 300, `runtime-chip: the menu opens right above the chip, not at the window corner (menu ${Math.round(menu.x)},${Math.round(menu.y)}..${Math.round(menu.right)},${Math.round(menu.bottom)} / chip ${Math.round(chip.x)},${Math.round(chip.y)})`);
  });

  // ---- 存储整理结果:人读的大小(KB/MB),不是「18432 bytes」 ----
  await section("storage-size", { scene: "settings" }, async (app) => {
    const { page } = app;
    await page.click("#settings-toc >> text=存储整理");
    await sleep(300);
    await page.click("#storage-cleanup");
    await page.waitForFunction(() => /\d/.test(document.querySelector("#storage-cleanup-result")?.textContent ?? ""));
    const text = await page.textContent("#storage-cleanup-result");
    check(/\d+(?:\.\d+)? ?(?:KB|MB|GB)/.test(text) && !/bytes/i.test(text), `storage-size: the cleanup result shows a human-readable size (${text.trim()})`);
  });

  // ---- 自动放行 / 提示音:写 app.json 的 ui_layout.prefs,不再只写 localStorage ----
  await section("prefs", { scene: "composer" }, async (app) => {
    const { page } = app;
    await page.evaluate(() => document.querySelector("#auto-allow").click());
    await sleep(900);
    const writes = (await app.calls("ui_prefs_set")).filter((call) => call.args?.ui_layout?.prefs && "auto_allow" in call.args.ui_layout.prefs);
    check(writes.length >= 1 && writes.at(-1).args.ui_layout.prefs.auto_allow === true, "prefs: turning auto-allow on is saved through ui_prefs_set (app.json)");
    check(await page.evaluate(() => localStorage.getItem("kz-auto-allow")) === null, "prefs: auto-allow is not parked in localStorage (it would be lost on restart)");
  });
  await section("prefs-sound", { scene: "settings" }, async (app) => {
    const { page } = app;
    await page.click("#settings-toc >> text=提示音");
    await sleep(300);
    await page.click('label[for="set-sound-completed"]');
    await sleep(900);
    const writes = (await app.calls("ui_prefs_set")).filter((call) => call.args?.ui_layout?.prefs?.sound);
    check(writes.length >= 1, "prefs: a notification-sound change is saved through ui_prefs_set (app.json)");
    check(await page.evaluate(() => localStorage.getItem("kz-sound-settings")) === null, "prefs: notification-sound settings are not parked in localStorage");
  });

  // ---- 右键菜单:重命名输入限长;无可用打开方式时「添加打开方式…」直达设置分区 ----
  await section("context-menu", { scene: "startup" }, async (app) => {
    const { page } = app;
    const mainRow = '.workbench-session[data-kind="main"] .workbench-session-link';
    await page.click(mainRow, { button: "right" });
    await page.waitForSelector(".k-menu:popover-open");
    await page.locator(".k-menu:popover-open .k-menu-item", { hasText: "重命名" }).first().click();
    await page.waitForSelector("dialog[open] #input-value");
    check(await page.getAttribute("dialog[open] #input-value", "maxlength") === "60", "context-menu: the rename box is capped at 60 characters (the backend limit) instead of failing after submit");
    await page.keyboard.press("Escape");
    await sleep(400);
    check(await page.evaluate(() => document.activeElement?.classList.contains("workbench-session-link") === true), "context-menu: cancelling a dialog opened from the menu puts the focus back on the row (not on <body>)");
    await page.evaluate(() => window.__kzPreview.setCommand("open_tools_list", []));
    await page.evaluate(async () => (await import("/12-session-menus.js")).loadOpenTools({ force: true }));
    await page.click(mainRow, { button: "right" });
    await page.waitForSelector(".k-menu:popover-open");
    await page.locator(".k-menu:popover-open .k-menu-item", { hasText: /添加打开方式/ }).click();
    await page.waitForFunction(() => document.body.dataset.view === "settings");
    await sleep(700);
    const anchor = await page.evaluate(() => { const section = document.querySelector("#sg-open-tools"); const box = section.getBoundingClientRect(); return { open: section.open !== false, top: box.top, visible: box.top >= 0 && box.top < window.innerHeight }; });
    check(anchor.open && anchor.visible, `context-menu: "add an open-with tool" lands on the 打开方式 section, not the top of settings (top ${Math.round(anchor.top)})`);
  });

  // ---- 研究空间的命令面板不列做不了的条目 ----
  await section("palette-research", { scene: "startup" }, async (app) => {
    const { page } = app;
    await page.click('.workspace-switcher [data-workspace="research"]');
    await sleep(1000);
    await page.keyboard.press("Control+p");
    await sleep(250);
    const listed = {};
    for (const query of ["新讨论", "记需求", "记缺陷", "总结"]) {
      await page.fill("#palette-input", query);
      await sleep(200);
      listed[query] = await page.evaluate((word) => [...document.querySelectorAll("#palette-list .palette-row")].some((row) => row.innerText.split("\n").map((part) => part.trim()).includes(word)), query);
    }
    check(Object.values(listed).every((value) => value === false), `palette-research: the research workspace palette does not offer development-only actions (${JSON.stringify(listed)})`);
  });

  // ---- 侧栏「待我处理」:进入后它自己高亮,离开后取消 ----
  await section("attention-nav", { scene: "chat" }, async (app) => {
    const { page } = app;
    const marks = () => page.evaluate(() => ({ attention: document.querySelector("#workbench-attention").classList.contains("active"), home: document.querySelector("#workbench-home").classList.contains("active") }));
    await page.click("#workbench-attention");
    await sleep(600);
    check((await marks()).attention === true, "attention-nav: opening 待我处理 highlights that sidebar link");
    await page.click("#workbench-home");
    await sleep(600);
    const after = await marks();
    check(after.attention === false && after.home === true, "attention-nav: leaving it moves the highlight to the page you are on");
  });

  console.log(`QA defects PASS: ${passed.length} checks`);
} finally {
  await browser.close();
  await server.close();
}
