// UI-0926 #9 弹层样例浏览器冒烟(docs/design/ui_surface_stack.md §7.3),由 ui-lint-smoke.mjs 在 ESLint 之后调用,
// 也可单独运行:node scripts/ui-surface-gallery-smoke.mjs。
//
// 运行时冒烟是假 DOM:看不到顶层、样式与颜色。这里用无头 Edge(playwright-core channel msedge,
// 与 ui-narrow-layout-smoke 同一路线)真打开 ui/gallery.html 与 index.html:
//   1. 特性探测:dialog / Popover API / CSS 锚点定位 / position-area / appearance:base-select,缺一即失败;
//   2. 暗/亮两套主题逐个演示:底色/圆角/阴影等于对应 --surface-* token,正文对比度 ≥7、弱化文字 ≥4.5,
//      包围盒在视口内(1280×840;800×500 只查几何);
//   3. 下拉专项(截图 6 回归):点开下拉后截取列表区域求平均相对亮度,暗色 <0.2、亮色 >0.6——
//      列表下面垫着一块反色「金丝雀」,列表没画进页面或画成白底都会露馅;菜单里嵌的下拉也跑一遍;
//   4. Esc 与叠放:卡片上再开确认框,一次 Esc 只关确认框;菜单里开着下拉列表时 Esc 只关列表;
//      弹窗里的 JS/静态菜单真实点击得到(模态外的节点是惰性的)、Esc 先关菜单;补全列表与锚点同宽;
//      键盘打开弹窗时初始焦点不弹 tooltip(另有键盘聚焦出提示的对照);
//   5. index.html 运行时对照:全部 select 为 base-select,全部 dialog/[popover] 带 .k-surface,
//      除 .resize-handle 外没有顶层以外、正在显示的 fixed 元素;
//   6. 截图写入 dist/ui-gallery/(已在 .gitignore)。
// page.evaluate 回调在浏览器里执行,用到的浏览器全局在这里声明给 ESLint(本文件其余部分是 node 环境)。
/* global window, getComputedStyle, CSS, HTMLDialogElement, HTMLElement */
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { inflateSync } from "node:zlib";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// ---------- 颜色与亮度 ----------
function parseColor(text) {
  const m = String(text).match(/rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+))?\s*\)/);
  if (m) return { r: Number(m[1]), g: Number(m[2]), b: Number(m[3]), a: m[4] === undefined ? 1 : Number(m[4]) };
  const c = String(text).match(/color\(srgb\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)(?:\s*\/\s*([\d.]+))?\)/);
  if (c) return { r: Number(c[1]) * 255, g: Number(c[2]) * 255, b: Number(c[3]) * 255, a: c[4] === undefined ? 1 : Number(c[4]) };
  return null;
}
function channel(v) {
  const s = v / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}
function luminance({ r, g, b }) {
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}
function contrast(fg, bg) {
  const a = luminance(fg);
  const b = luminance(bg);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

// 最小 PNG 解码(8 位 RGB/RGBA、非隔行,playwright 截图正是这种),只为求区域平均亮度,不引依赖。
function decodePng(buffer) {
  let offset = 8;
  let width = 0;
  let height = 0;
  let colorType = 0;
  const idat = [];
  while (offset < buffer.length) {
    const length = buffer.readUInt32BE(offset);
    const type = buffer.toString("ascii", offset + 4, offset + 8);
    const data = buffer.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      if (data[8] !== 8 || data[12] !== 0) throw new Error("PNG 解码只支持 8 位非隔行");
      colorType = data[9];
    } else if (type === "IDAT") idat.push(data);
    else if (type === "IEND") break;
    offset += 12 + length;
  }
  const bpp = colorType === 6 ? 4 : colorType === 2 ? 3 : 0;
  if (!bpp) throw new Error(`PNG 解码不支持 colorType=${colorType}`);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * bpp;
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x += 1) {
      const left = x >= bpp ? pixels[y * stride + x - bpp] : 0;
      const up = y > 0 ? pixels[(y - 1) * stride + x] : 0;
      const upLeft = y > 0 && x >= bpp ? pixels[(y - 1) * stride + x - bpp] : 0;
      let value = line[x];
      if (filter === 1) value += left;
      else if (filter === 2) value += up;
      else if (filter === 3) value += Math.floor((left + up) / 2);
      else if (filter === 4) {
        const p = left + up - upLeft;
        const pa = Math.abs(p - left);
        const pb = Math.abs(p - up);
        const pc = Math.abs(p - upLeft);
        value += pa <= pb && pa <= pc ? left : pb <= pc ? up : upLeft;
      }
      pixels[y * stride + x] = value & 0xff;
    }
  }
  return { width, height, bpp, pixels };
}
function meanLuminance(png) {
  const { width, height, bpp, pixels } = decodePng(png);
  let sum = 0;
  for (let i = 0; i < width * height; i += 1) {
    sum += luminance({ r: pixels[i * bpp], g: pixels[i * bpp + 1], b: pixels[i * bpp + 2] });
  }
  return sum / (width * height);
}

// ---------- 期望 ----------
function expected(kind, tokens) {
  switch (kind) {
    case "modal": return { bg: tokens.bg, radius: tokens.radiusLg, shadow: tokens.shadow3 };
    case "tooltip": return { bg: tokens.bgRaised, radius: tokens.radiusSm, shadow: tokens.shadow1, fg: tokens.fgStrong };
    case "card": return { bg: tokens.bg, radius: tokens.radiusMd, shadow: tokens.shadow3 };
    case "chip": return { bg: tokens.bg, radius: tokens.radiusPill, shadow: tokens.shadow2 };
    default: return { bg: tokens.bg, radius: tokens.radiusMd, shadow: tokens.shadow2 };
  }
}

// ── 分区:后台任务侧栏与可调框 ──
// 浏览器变异守卫(与 ui-runtime-smoke 的 KZ_SMOKE_MUTATE 同一约定):把被守护的那一处源码改坏,经 page.route
// 喂给步骤 8 的页面,期望本冒烟失败。变异没有恰好命中一处就直接报错——守卫悄悄失效比断言变红更危险。
// 不认识的 id 不理会(那是 ui-runtime-smoke 的变异)。
const TASKS_BROWSER_MUTATIONS = {
  // 权限卡锚在输入区上(anchor-name)。删掉,卡片退回右下停靠,与输入区不同宽。
  askComposerAnchor: { file: "surface.css", pattern: /#composer \{ anchor-name: --kz-composer; \}/, replace: "" },
  // 「重新打开询问」芯片按停靠宽度让开侧栏。去掉 --kz-dock-right,芯片压在侧栏上。
  chipDockRight: {
    file: "surface.css",
    pattern: /(\.k-chip-float \{\r?\n[ \t]*position: fixed; inset: auto )calc\(22px \+ var\(--kz-dock-right, 0px\)\)( 18px auto;)/,
    replace: "$122px$2",
  },
  // 弹层里的 Esc 不算关闭侧栏。删掉守卫,下拉列表开着时 Esc 整个侧栏收起。
  escPopoverBrowser: {
    file: "06-agent-panel.js",
    pattern: /[ \t]*if \(event\.defaultPrevented \|\| event\.target\?\.closest\?\.\("\[popover\]"\) \|\| nativePickerOpen\(event\.target\)\) return;\r?\n/,
    replace: "",
  },
  // 窄侧栏隐去模型列(容器查询)。删掉,1280 宽时子代理列只剩几个字。
  tasksNarrowModel: { file: "style.css", pattern: /@container tasks \(max-width: [\d.]+px\) \{[\s\S]*?\r?\n\}\r?\n/, replace: "" },
};
async function tasksBrowserMutation() {
  const id = process.env.KZ_SMOKE_MUTATE ?? "";
  const mutation = TASKS_BROWSER_MUTATIONS[id];
  if (!mutation) return null;
  const source = await readFile(path.join(root, "crates/kanzei-app/ui", mutation.file), "utf8");
  const hits = (source.match(new RegExp(mutation.pattern.source, "g")) ?? []).length;
  if (hits !== 1) throw new Error(`变异 ${id} 没有恰好命中一处被守护的源码(实得 ${hits} 处):护栏已经失效,先修变异表`);
  const type = mutation.file.endsWith(".css") ? "text/css; charset=utf-8" : "text/javascript; charset=utf-8";
  return { id, file: mutation.file, type, body: source.replace(mutation.pattern, mutation.replace) };
}

export async function runSurfaceGallerySmoke({ channel = "msedge", outDir = path.join(root, "dist/ui-gallery") } = {}) {
  const failures = [];
  const fail = (message) => failures.push(message);
  const notes = [];
  await mkdir(outDir, { recursive: true });
  const { origin, close } = await startPreviewServer({ port: 0 });
  const browser = await chromium.launch({ channel, headless: true });
  try {
    const context = await browser.newContext({ viewport: { width: 1280, height: 840 }, deviceScaleFactor: 1 });
    const page = await context.newPage();
    const pageErrors = [];
    page.on("console", (message) => { if (message.type() === "error") pageErrors.push(message.text()); });
    page.on("pageerror", (error) => pageErrors.push(String(error)));
    await page.goto(`${origin}/gallery.html`, { waitUntil: "load" });

    // 1 特性探测
    const features = await page.evaluate(() => ({
      dialog: typeof HTMLDialogElement !== "undefined" && typeof HTMLDialogElement.prototype.showModal === "function",
      popover: typeof HTMLElement !== "undefined" && "showPopover" in HTMLElement.prototype,
      anchorName: CSS.supports("anchor-name: --a"),
      positionArea: CSS.supports("position-area: block-end"),
      baseSelect: CSS.supports("appearance: base-select"),
      closedBy: typeof HTMLDialogElement !== "undefined" && "closedBy" in HTMLDialogElement.prototype,
      hint: (() => { const d = document.createElement("div"); d.setAttribute("popover", "hint"); return d.popover === "hint"; })(),
      version: navigator.userAgent.match(/Edg\/([\d.]+)/)?.[1] ?? navigator.userAgent,
    }));
    const missing = ["dialog", "popover", "anchorName", "positionArea", "baseSelect"].filter((key) => !features[key]);
    if (missing.length) {
      fail(`运行时缺少弹层所需能力 ${missing.join(", ")}:需要 WebView2/Edge ≥ 135(本机运行时 ${features.version})`);
      return { failures, notes };
    }
    notes.push(`Edge ${features.version};closedby=${features.closedBy} popover=hint=${features.hint}`);
    await page.waitForFunction(() => window.__gallery?.ready === true, null, { timeout: 15000 });
    const demos = await page.evaluate(() => window.__gallery.demos());

    for (const theme of ["dark", "light"]) {
      await page.setViewportSize({ width: 1280, height: 840 });
      await page.evaluate((value) => window.__gallery.setTheme(value), theme);
      await page.mouse.move(2, 2);
      const tokens = await page.evaluate(() => window.__gallery.tokens());
      // 2 逐个演示:token、对比度、包围盒、截图
      for (const { id, kind } of demos) {
        if (kind === "select") continue;
        await page.evaluate(() => window.__gallery.closeAll());
        if (kind === "tooltip") {
          const anchor = id === "tooltip" ? "#demo-tip-short" : "#demo-tip-long";
          await page.locator(anchor).scrollIntoViewIfNeeded();
          await page.hover(anchor);
          await page.waitForTimeout(600);
        } else {
          await page.evaluate((demoId) => window.__gallery.open(demoId), id);
          await page.waitForTimeout(kind === "toast" ? 250 : 120);
        }
        const m = await page.evaluate((demoId) => window.__gallery.measure(demoId), id);
        const where = `${theme}/${id}`;
        if (!m) { fail(`${where}:演示没有产出可测量的元素`); continue; }
        if (!m.open) fail(`${where}:弹层没有打开(不在顶层)`);
        const want = expected(kind, tokens);
        if (m.bg !== want.bg) fail(`${where}:底色 ${m.bg} ≠ token ${want.bg}(外观必须来自 --surface-*)`);
        if (m.radius !== want.radius) fail(`${where}:圆角 ${m.radius} ≠ token ${want.radius}`);
        if (m.shadow !== want.shadow) fail(`${where}:阴影 ${m.shadow} ≠ token ${want.shadow}`);
        const bg = parseColor(m.bg);
        const fg = parseColor(m.fg);
        const muted = parseColor(tokens.muted);
        if (bg && fg) {
          const ratio = contrast(fg, bg);
          if (ratio < 7) fail(`${where}:正文对比度 ${ratio.toFixed(2)} < 7(${m.fg} on ${m.bg})`);
          if (kind !== "tooltip" && muted && contrast(muted, bg) < 4.5) fail(`${where}:弱化文字对比度 ${contrast(muted, bg).toFixed(2)} < 4.5`);
        } else fail(`${where}:无法解析颜色 fg=${m.fg} bg=${m.bg}`);
        const b = m.box;
        if (kind !== "panel" && (b.left < -1 || b.top < -1 || b.right > m.viewport.width + 1 || b.bottom > m.viewport.height + 1)) {
          fail(`${where}:包围盒越出视口 ${JSON.stringify(b)}`);
        }
        if (b.width <= 0 || b.height <= 0) fail(`${where}:包围盒为空 ${JSON.stringify(b)}`);
        await page.screenshot({ path: path.join(outDir, `${theme}-${id}.png`) });
      }
      await page.evaluate(() => window.__gallery.closeAll());
      await page.mouse.move(2, 2);

      // 3 下拉专项:列表区域平均亮度(截图 6 的白色列表)
      const pickerCases = [
        { id: "#demo-select" },
        { id: "#demo-select-chip" },
        { id: "#demo-select-long" },
        { id: "#demo-select-in-menu", menu: "menu-select" },
      ];
      for (const pick of pickerCases) {
        await page.evaluate(() => window.__gallery.closeAll());
        await page.evaluate(() => window.scrollTo(0, 0));
        if (pick.menu) {
          await page.evaluate((demoId) => window.__gallery.open(demoId), pick.menu);
          await page.waitForTimeout(120);
        }
        const select = page.locator(pick.id);
        const appearance = await select.evaluate((el) => getComputedStyle(el).appearance);
        if (appearance !== "base-select") fail(`${theme}/${pick.id}:appearance=${appearance},不是 base-select`);
        await select.click();
        await page.waitForTimeout(200);
        const state = await select.evaluate((el) => {
          let open = false;
          try { open = el.matches(":open"); } catch { /* 旧运行时 */ }
          const r = el.getBoundingClientRect();
          return { open, left: r.left, bottom: r.bottom, width: r.width };
        });
        if (!state.open) fail(`${theme}/${pick.id}:点击后下拉列表没有打开(:open 为假)`);
        // 取样区在列表内部:列表至少与下拉同宽(min-width: anchor-size(width)),向下弹时紧贴下拉下沿;
        // 左右各缩 6px、只取前 44px 高(至少覆盖两行选项),不会截到列表外的金丝雀底块。
        const clip = { x: Math.round(state.left + 6), y: Math.round(state.bottom + 10), width: Math.max(8, Math.round(state.width - 12)), height: 44 };
        const png = await page.screenshot({ clip });
        const lum = meanLuminance(png);
        const ok = theme === "dark" ? lum < 0.2 : lum > 0.6;
        if (!ok) fail(`${theme}/${pick.id}:下拉列表区域平均亮度 ${lum.toFixed(3)}(暗色须 <0.2、亮色须 >0.6)——列表没跟主题,或没画进页面`);
        await page.screenshot({ path: path.join(outDir, `${theme}-picker-${pick.id.slice(1)}.png`) });
        // 4b 菜单里的下拉:Esc 只关列表,菜单还在,栈深度不变
        if (pick.menu) {
          const before = await page.evaluate(() => window.__gallery.stackDepth());
          await page.keyboard.press("Escape");
          await page.waitForTimeout(120);
          const after = await page.evaluate((selector) => {
            const el = document.querySelector(selector);
            let open = false;
            try { open = el.matches(":open"); } catch { /* 忽略 */ }
            return { selectOpen: open, menuOpen: document.querySelector("#demo-menu-select").matches(":popover-open"), depth: window.__gallery.stackDepth() };
          }, pick.id);
          if (after.selectOpen || !after.menuOpen || after.depth !== before) {
            fail(`${theme}/菜单内下拉:一次 Esc 应只关列表(列表 ${after.selectOpen ? "仍开" : "已关"}、菜单 ${after.menuOpen ? "仍开" : "被关"}、栈深 ${before}→${after.depth})`);
          }
          await page.keyboard.press("Escape");
          await page.waitForTimeout(80);
          const depth = await page.evaluate(() => window.__gallery.stackDepth());
          if (depth !== before - 1) fail(`${theme}/菜单内下拉:第二次 Esc 应关菜单(栈深 ${before}→${depth})`);
        } else {
          await page.keyboard.press("Escape");
          await page.waitForTimeout(80);
        }
      }

      // 4a 叠放:卡片上再开确认框,一次 Esc 只关确认框
      await page.evaluate(() => window.__gallery.closeAll());
      await page.evaluate(() => window.__gallery.open("card"));
      await page.evaluate(() => window.__gallery.open("confirm"));
      await page.waitForTimeout(120);
      const depthBefore = await page.evaluate(() => window.__gallery.stackDepth());
      await page.keyboard.press("Escape");
      await page.waitForTimeout(120);
      const stacked = await page.evaluate(() => ({
        confirmOpen: document.querySelector("#confirm-overlay").open,
        cardOpen: document.querySelector("#demo-card").matches(":popover-open"),
        depth: window.__gallery.stackDepth(),
      }));
      if (stacked.confirmOpen || !stacked.cardOpen || stacked.depth !== depthBefore - 1) {
        fail(`${theme}/叠放:卡片+确认框时一次 Esc 应只关确认框(确认框 ${stacked.confirmOpen ? "仍开" : "已关"}、卡片 ${stacked.cardOpen ? "仍在" : "被关"}、栈深 ${depthBefore}→${stacked.depth})`);
      }
      await page.keyboard.press("Escape");
      await page.waitForTimeout(80);
      await page.evaluate(() => window.__gallery.closeAll());

      // 4c 排队:同一个确认框并发两次,点确认后第二个接着打开且**保持**打开
      //    (close 事件异步派发,迟到的 close 不能把排队的第二问当成被取消)。
      await page.evaluate(() => { window.__queueA = window.__gallery.open("confirm"); window.__gallery.open("confirm-safe"); });
      await page.waitForTimeout(80);
      await page.click("#confirm-ok");
      await page.waitForTimeout(200);
      const queued = await page.evaluate(() => ({
        open: document.querySelector("#confirm-overlay").open,
        hidden: document.querySelector("#confirm-overlay").classList.contains("hidden"),
        safeVisible: !document.querySelector("#confirm-safe").classList.contains("hidden"),
        depth: window.__gallery.stackDepth(),
      }));
      if (!queued.open || queued.hidden || !queued.safeVisible || queued.depth !== 1) {
        fail(`${theme}/排队:第一个确认框确认后,第二个没有接着打开并保持打开 ${JSON.stringify(queued)}`);
      }
      await page.keyboard.press("Escape");
      await page.waitForTimeout(80);
      await page.evaluate(() => window.__gallery.closeAll());

      if (theme === "dark") {
        // 4d 弹窗里的菜单:模态开着时 dialog 子树之外全是惰性的(点不到、拿不到焦点)。
        //    真实鼠标点击菜单项必须走到 onSelect;静态 data-kz-menu 菜单里的勾选框必须勾得上;Esc 先关菜单再关弹窗。
        await page.mouse.move(2, 2);
        await page.click('[data-demo="dialog-menu"]');
        await page.waitForTimeout(150);
        const mount = await page.evaluate(() => {
          const menu = document.querySelector(".k-menu[id^='kz-menu-']");
          return { inDialog: Boolean(menu?.closest("#demo-dialog-menu")), open: Boolean(menu?.matches(":popover-open")) };
        });
        if (!mount.open || !mount.inDialog) fail(`dark/弹窗内菜单:openMenu 的菜单${mount.open ? "" : "没打开、"}${mount.inDialog ? "" : "没挂进锚点所在的 <dialog>(模态开着时 body 下的节点是惰性的)"}`);
        const picksBefore = (await page.evaluate(() => window.__gallery.picks())).length;
        try {
          await page.locator(".k-menu[id^='kz-menu-'] .k-menu-item").first().click({ timeout: 3000 });
        } catch (error) {
          fail(`dark/弹窗内菜单:真实点击菜单项失败(惰性节点收不到指针事件):${String(error).split("\n")[0]}`);
        }
        await page.waitForTimeout(120);
        const afterPick = await page.evaluate(() => ({
          picks: window.__gallery.picks(),
          dialogOpen: document.querySelector("#demo-dialog-menu").open,
          depth: window.__gallery.stackDepth(),
        }));
        if (afterPick.picks.length !== picksBefore + 1 || !afterPick.dialogOpen || afterPick.depth !== 1) {
          fail(`dark/弹窗内菜单:点菜单项应恰好调一次 onSelect、只关菜单不关弹窗 ${JSON.stringify(afterPick)}`);
        }
        try {
          await page.click("#demo-dialog-menu-static-trigger", { timeout: 3000 });
          await page.waitForTimeout(100);
          await page.click("#demo-dialog-menu-check", { timeout: 3000 });
        } catch (error) {
          fail(`dark/弹窗内静态菜单:真实点击失败:${String(error).split("\n")[0]}`);
        }
        await page.waitForTimeout(100);
        const staticState = await page.evaluate(() => ({
          checked: document.querySelector("#demo-dialog-menu-check").checked,
          menuOpen: document.querySelector("#demo-dialog-menu-static").matches(":popover-open"),
          depth: window.__gallery.stackDepth(),
        }));
        if (!staticState.checked || !staticState.menuOpen || staticState.depth !== 2) {
          fail(`dark/弹窗内静态菜单:勾选框应勾上且菜单仍开(栈深 2)${JSON.stringify(staticState)}`);
        }
        await page.keyboard.press("Escape");
        await page.waitForTimeout(100);
        const escOnce = await page.evaluate(() => ({
          menuOpen: document.querySelector("#demo-dialog-menu-static").matches(":popover-open"),
          dialogOpen: document.querySelector("#demo-dialog-menu").open,
        }));
        if (escOnce.menuOpen || !escOnce.dialogOpen) fail(`dark/弹窗内菜单:第一次 Esc 应只关菜单 ${JSON.stringify(escOnce)}`);
        await page.keyboard.press("Escape");
        await page.waitForTimeout(100);
        if (await page.evaluate(() => document.querySelector("#demo-dialog-menu").open)) fail("dark/弹窗内菜单:第二次 Esc 应关弹窗");
        await page.evaluate(() => window.__gallery.closeAll());

        // 4e 补全列表与锚点同宽(不被菜单/浮层的 420px 上限截窄)。锚点在 1280 宽下远超 420px。
        await page.evaluate(() => window.__gallery.open("completion"));
        await page.waitForTimeout(120);
        const widths = await page.evaluate(() => {
          const list = document.querySelector("#demo-completion").getBoundingClientRect();
          const anchor = document.querySelector("#demo-completion-anchor").getBoundingClientRect();
          return { list: list.width, anchor: anchor.width, listLeft: list.left, anchorLeft: anchor.left };
        });
        if (widths.anchor <= 420 || Math.abs(widths.list - widths.anchor) > 1 || Math.abs(widths.listLeft - widths.anchorLeft) > 1) {
          fail(`dark/补全列表:应与输入框同宽且左缘对齐,实为宽 ${widths.list.toFixed(1)}/${widths.anchor.toFixed(1)}、左缘 ${widths.listLeft.toFixed(1)}/${widths.anchorLeft.toFixed(1)}(前置:锚点须宽于 420px)`);
        }
        await page.evaluate(() => window.__gallery.closeAll());

        // 4f 键盘打开弹窗:初始焦点是程序化聚焦,不弹提示(否则「关闭」提示立刻盖住弹窗角);
        //    对照:随后键盘态下聚焦带 title 的按钮,提示照常立即出现(证明本用例看得见提示)。
        //    样例里的弹窗与应用查看器同构:第一个可聚焦元素不是 initialFocus,焦点会被程序化地挪一次。
        //    用一个鼠标从没进过的新页面:鼠标停在页面上时,弹窗的遮罩一出现就会派发 pointerover 把提示收掉,
        //    用例会恒绿(纯键盘用户、鼠标在窗口外时,问题才露出来)。
        const kbd = await context.newPage();
        kbd.on("pageerror", (error) => pageErrors.push(String(error)));
        await kbd.goto(`${origin}/gallery.html`, { waitUntil: "load" });
        await kbd.waitForFunction(() => window.__gallery?.ready === true, null, { timeout: 15000 });
        await kbd.focus('[data-demo="dialog-lg"]');
        await kbd.waitForTimeout(250); // 聚焦触发器会滚动页面,滚动事件异步派发且会收起提示:先让它落定
        await kbd.keyboard.press("Enter");
        await kbd.waitForTimeout(150);
        const quiet = await kbd.evaluate(() => ({
          focused: document.activeElement?.id ?? "",
          tip: Boolean(document.getElementById("kz-tip")?.matches(":popover-open")),
        }));
        if (quiet.focused !== "demo-dialog-lg-close") fail(`dark/键盘打开弹窗:前置失败,初始焦点应在 #demo-dialog-lg-close,实为 #${quiet.focused}`);
        if (quiet.tip) fail("dark/键盘打开弹窗:初始焦点按钮上立刻弹出了 tooltip(程序化聚焦不该触发提示)");
        await kbd.keyboard.press("Escape");
        await kbd.waitForTimeout(100);
        await kbd.focus("#demo-tip-short");
        await kbd.waitForTimeout(250);
        const control = await kbd.evaluate(() => Boolean(document.getElementById("kz-tip")?.matches(":popover-open")));
        if (!control) fail("dark/键盘打开弹窗:对照失败——键盘态聚焦带 title 的按钮也没出提示,本用例看不见 tooltip");
        await kbd.close();
      }

      // ── 分区:后台任务侧栏与可调框 ──
      // 7 可调框(00-frame.js,UI2-0926 #4):真实鼠标拖边(从框外 3px 处按下)、拖标题栏、一步甩动、上边夹紧、
      //   关掉再开保留几何、窗口缩小夹紧与恢复、双击标题复位、点遮罩照常关闭、Alt+Shift+→ 加宽、卡片拖标题与左缘加宽;
      //   菜单/浮层/提示里没有手柄。
      if (theme === "dark") {
        await page.setViewportSize({ width: 1280, height: 840 });
        await page.evaluate(() => { window.__gallery.closeAll(); window.__gallery.resetFrames(); });
        await page.mouse.move(2, 2);
        const frameBox = (selector) => page.evaluate((sel) => {
          const r = document.querySelector(sel).getBoundingClientRect();
          return { left: r.left, top: r.top, width: r.width, height: r.height, right: r.right, bottom: r.bottom };
        }, selector);
        const near = (a, b) => Math.abs(a - b) <= 1;
        const drag = async (x0, y0, x1, y1, steps = 6) => {
          await page.mouse.move(x0, y0);
          await page.mouse.down();
          await page.mouse.move(x1, y1, { steps });
          await page.mouse.up();
          await page.waitForTimeout(60);
        };
        const frames = await page.evaluate(() => window.__gallery.frames());
        if (frames.join(",") !== "demo-lg,demo-card") fail(`dark/可调框:样例页的框清单应为 demo-lg,demo-card,实为 ${frames.join(",")}`);
        await page.evaluate(() => window.__gallery.open("dialog-lg"));
        await page.waitForTimeout(150);
        const r0 = await frameBox("#demo-dialog-lg");
        const depth0 = await page.evaluate(() => window.__gallery.stackDepth());
        await drag(r0.right + 3, r0.top + r0.height / 2, r0.right + 103, r0.top + r0.height / 2);
        const r1 = await frameBox("#demo-dialog-lg");
        const alive = await page.evaluate(() => ({ open: document.querySelector("#demo-dialog-lg").open, depth: window.__gallery.stackDepth() }));
        if (!near(r1.width, r0.width + 100) || !near(r1.left, r0.left) || !alive.open || alive.depth !== depth0) {
          fail(`dark/可调框:从右边框外 3px 拖 +100 应只加宽 100、左缘不动、弹窗不被轻关闭 ${JSON.stringify({ r0, r1, alive })}`);
        }
        const head = await frameBox("#demo-dialog-lg .viewer-head");
        await drag(head.left + 80, head.top + head.height / 2, head.left - 40, head.top + head.height / 2 - 50);
        const r2 = await frameBox("#demo-dialog-lg");
        if (!near(r2.width, r1.width) || !near(r2.height, r1.height) || !near(r2.left, r1.left - 120) || !near(r2.top, r1.top - 50)) {
          fail(`dark/可调框:拖标题栏 (-120,-50) 应只移动不变尺寸 ${JSON.stringify({ r1, r2 })}`);
        }
        const head2 = await frameBox("#demo-dialog-lg .viewer-head");
        await drag(head2.left + 80, head2.top + 10, head2.left + 280, head2.top + 110, 1);
        const r3 = await frameBox("#demo-dialog-lg");
        if (!near(r3.left, r2.left + 200) || !near(r3.top, r2.top + 100)) fail(`dark/可调框:一步甩动 (+200,+100) 没有移动到位 ${JSON.stringify({ r2, r3 })}`);
        await drag(r3.left + r3.width / 2, r3.top - 3, r3.left + r3.width / 2, r3.top - 2003);
        const r4 = await frameBox("#demo-dialog-lg");
        if (!near(r4.top, 8) || !near(r4.bottom, r3.bottom)) fail(`dark/可调框:上边拖到窗外应夹在 8px、下缘不动 ${JSON.stringify({ r3, r4 })}`);
        await page.keyboard.press("Escape");
        await page.waitForTimeout(100);
        await page.evaluate(() => window.__gallery.open("dialog-lg"));
        await page.waitForTimeout(150);
        const r5 = await frameBox("#demo-dialog-lg");
        const stored = await page.evaluate(() => localStorage.getItem("kz-frame:demo-lg"));
        if (!near(r5.left, r4.left) || !near(r5.width, r4.width) || !stored) fail(`dark/可调框:关掉再开应保留几何并记在存储里 ${JSON.stringify({ r4, r5, stored })}`);
        await page.setViewportSize({ width: 800, height: 500 });
        await page.waitForTimeout(150);
        const small = await frameBox("#demo-dialog-lg");
        if (small.left < 7 || small.top < 7 || small.right > 793 || small.bottom > 493) fail(`dark/可调框:窗口缩到 800×500 后框应夹进 [8,792]×[8,492] ${JSON.stringify(small)}`);
        await page.setViewportSize({ width: 1280, height: 840 });
        await page.waitForTimeout(150);
        const r6 = await frameBox("#demo-dialog-lg");
        if (!near(r6.left, r5.left) || !near(r6.width, r5.width) || !near(r6.height, r5.height)) fail(`dark/可调框:窗口恢复后应回到用户摆的几何 ${JSON.stringify({ r5, r6 })}`);
        const head3 = await frameBox("#demo-dialog-lg .viewer-head");
        await page.mouse.dblclick(head3.left + 80, head3.top + head3.height / 2);
        await page.waitForTimeout(100);
        const r7 = await frameBox("#demo-dialog-lg");
        const cleared = await page.evaluate(() => localStorage.getItem("kz-frame:demo-lg"));
        if (!near(r7.left, r0.left) || !near(r7.top, r0.top) || !near(r7.width, r0.width) || cleared) fail(`dark/可调框:双击标题栏应回到默认居中并清掉存储 ${JSON.stringify({ r0, r7, cleared })}`);
        await page.focus("#demo-dialog-lg-close");
        await page.keyboard.press("Alt+Shift+ArrowRight");
        await page.waitForTimeout(80);
        const r8 = await frameBox("#demo-dialog-lg");
        if (!near(r8.width, r7.width + 24)) fail(`dark/可调框:Alt+Shift+→ 应加宽 24,实为 ${r7.width}→${r8.width}`);
        await page.mouse.click(4, 4);
        await page.waitForTimeout(150);
        if (await page.evaluate(() => document.querySelector("#demo-dialog-lg").open)) fail("dark/可调框:点遮罩应照常关闭弹窗(closedby=any)");
        await page.screenshot({ path: path.join(outDir, "dark-frame-dialog.png") });
        await page.evaluate(() => { window.__gallery.resetFrames(); window.__gallery.open("card"); });
        await page.waitForTimeout(150);
        const c0 = await frameBox("#demo-card");
        const cardHead = await frameBox("#demo-card .ask-head");
        await drag(cardHead.left + 30, cardHead.top + cardHead.height / 2, cardHead.left - 170, cardHead.top + cardHead.height / 2 - 100);
        const c1 = await frameBox("#demo-card");
        if (!near(c1.left, c0.left - 200) || !near(c1.top, c0.top - 100) || !near(c1.width, c0.width)) fail(`dark/可调卡片:拖标题栏应移动且保持宽度 ${JSON.stringify({ c0, c1 })}`);
        await drag(c1.left - 3, c1.top + c1.height / 2, c1.left - 83, c1.top + c1.height / 2);
        const c2 = await frameBox("#demo-card");
        if (!near(c2.width, c1.width + 80) || !near(c2.right, c1.right)) fail(`dark/可调卡片:左缘加宽 80 应右缘不动 ${JSON.stringify({ c1, c2 })}`);
        const titleBox = await frameBox("#demo-card-title");
        const clickPromise = page.evaluate(() => new Promise((resolve) => {
          document.querySelector("#demo-card").addEventListener("click", (event) => resolve(event.target.closest(".ask-title") ? "title" : String(event.target.className || event.target.tagName)), { once: true });
          setTimeout(() => resolve("none"), 800);
        }));
        await page.mouse.click(titleBox.left + 10, titleBox.top + titleBox.height / 2);
        const clicked = await clickPromise;
        if (clicked !== "title") fail(`dark/可调卡片:标题上的普通点击 target 应仍在标题内(越过阈值前不捕获指针),实为 ${clicked}`);
        const stray = await page.evaluate(() => document.querySelectorAll(".k-menu .k-frame-edge, .k-popover .k-frame-edge, .k-tooltip .k-frame-edge, .k-toast .k-frame-edge").length);
        if (stray) fail(`dark/可调框:菜单/浮层/提示/toast 里出现了 ${stray} 个调尺寸手柄(它们不是框)`);
        await page.screenshot({ path: path.join(outDir, "dark-frame-card.png") });
        await page.evaluate(() => { window.__gallery.closeAll(); window.__gallery.resetFrames(); });
        notes.push("可调框:拖边/拖标题/甩动/夹紧/复位/键盘/卡片加宽均通过");
      }

      // 静态矩阵(暗色页面下两套主题同屏)
      if (theme === "dark") {
        await page.locator("#demo-matrix").scrollIntoViewIfNeeded();
        await page.locator("#demo-matrix").screenshot({ path: path.join(outDir, "matrix.png") });
        // 亮色矩阵是嵌套在暗色页面里的 [data-theme="light"] 区块::root 上以 var() 定义的 token(--danger/--alert/
        // --dot-idle/组件层 --surface-*)在 :root 就求好了值,区块里不重声明就拿到暗色值(曾经危险菜单项
        // #ff7b72 在白底上 2.52:1)。判据:区块上每个主题 token 的计算值 == html[data-theme=light] 上的计算值。
        const drift = await page.evaluate(() => {
          const names = new Set();
          const walk = (rules) => {
            for (const rule of rules) {
              if (rule.styleSheet) walk(rule.styleSheet.cssRules);
              else if (rule.style && (rule.selectorText === ":root" || rule.selectorText === '[data-theme="light"]')) {
                for (const name of rule.style) if (name.startsWith("--")) names.add(name);
              } else if (rule.cssRules) walk(rule.cssRules);
            }
          };
          for (const sheet of document.styleSheets) walk(sheet.cssRules);
          const section = document.querySelector('[data-matrix="light"]');
          const html = document.documentElement;
          const previous = html.getAttribute("data-theme");
          html.setAttribute("data-theme", "light");
          const want = Object.fromEntries([...names].map((name) => [name, getComputedStyle(html).getPropertyValue(name).trim()]));
          html.setAttribute("data-theme", previous);
          const got = Object.fromEntries([...names].map((name) => [name, getComputedStyle(section).getPropertyValue(name).trim()]));
          return {
            checked: names.size,
            aliases: ["--danger", "--alert", "--dot-idle"].map((name) => `${name}=${got[name]}`),
            mismatched: [...names].filter((name) => want[name] !== got[name]).map((name) => `${name}: 矩阵 ${got[name]} ≠ 亮色 ${want[name]}`),
          };
        });
        if (drift.checked < 50) fail(`亮色矩阵 token 对照:只找到 ${drift.checked} 个主题 token,判据定位失效`);
        if (drift.mismatched.length) fail(`亮色矩阵拿到了暗色 token(嵌套主题区块未重声明 :root 的 var() 值):\n${drift.mismatched.join("\n")}`);
        notes.push(`亮色矩阵 ${drift.checked} 个主题 token 与 html[data-theme=light] 一致(${drift.aliases.join(" ")})`);
      }

      // 800×500 只查几何(与主题无关,只跑一遍):菜单/浮层/卡片/模态不越出小窗口
      if (theme !== "dark") continue;
      await page.setViewportSize({ width: 800, height: 500 });
      for (const { id, kind } of demos) {
        if (kind === "select" || kind === "tooltip" || kind === "panel" || kind === "toast") continue;
        await page.evaluate(() => window.__gallery.closeAll());
        await page.evaluate((demoId) => window.__gallery.open(demoId), id);
        await page.waitForTimeout(100);
        const m = await page.evaluate((demoId) => window.__gallery.measure(demoId), id);
        const b = m?.box;
        if (!b || b.left < -1 || b.top < -1 || b.right > 801 || b.bottom > 501) fail(`${theme}/${id}@800×500:包围盒越出视口 ${JSON.stringify(b)}`);
      }
      await page.evaluate(() => window.__gallery.closeAll());
    }
    if (pageErrors.length) fail(`gallery.html 控制台错误:${pageErrors.join(" | ")}`);
    await context.close();

    // 5 index.html 运行时对照(经预览服务注入模拟 IPC)
    for (const theme of ["dark", "light"]) {
      const appContext = await browser.newContext({ viewport: { width: 1280, height: 840 }, colorScheme: theme });
      const app = await appContext.newPage();
      const appErrors = [];
      app.on("pageerror", (error) => appErrors.push(String(error)));
      await app.goto(`${origin}/?theme=${theme}&scene=chat`, { waitUntil: "load" });
      await app.waitForFunction(() => window.__kzPreview?.ready === true, null, { timeout: 25000 });
      await app.waitForTimeout(300);
      const audit = await app.evaluate(() => {
        const selects = [...document.querySelectorAll("select")].filter((s) => getComputedStyle(s).appearance !== "base-select").map((s) => s.id || s.className);
        const bare = [...document.querySelectorAll("dialog, [popover]")].filter((el) => !el.classList.contains("k-surface")).map((el) => el.id || el.className);
        const fixed = [...document.querySelectorAll("body *")].filter((el) => {
          const s = getComputedStyle(el);
          if (s.position !== "fixed" || s.display === "none") return false;
          if (el.matches(".resize-handle")) return false;
          try { if (el.matches(":popover-open, dialog:modal")) return false; } catch { /* 忽略 */ }
          return true;
        }).map((el) => el.id || el.className);
        // ── 分区:后台任务侧栏与可调框 ── 应用里的框清单;可调尺寸的框只有一个内层容器(手柄与挂进来的菜单除外)。
        const frameIds = [...document.querySelectorAll("[data-kz-frame]")].map((el) => el.dataset.kzFrame).sort();
        const multiChild = [...document.querySelectorAll("[data-kz-frame-edges]")]
          .filter((el) => [...el.children].filter((c) => !c.classList.contains("k-frame-edge") && !c.hasAttribute("popover")).length !== 1)
          .map((el) => el.id);
        return { selects, bare, fixed, selectCount: document.querySelectorAll("select").length, frameIds, multiChild };
      });
      if (audit.frameIds.join(",") !== "ask,confirm,input,project-models,viewer") fail(`index.html(${theme}):可调框清单应为 ask,confirm,input,project-models,viewer,实为 ${audit.frameIds.join(",")}`);
      if (audit.multiChild.length) fail(`index.html(${theme}):可调尺寸的框必须只有一个内层容器(裁剪与滚动下放到它):${audit.multiChild.join(", ")}`);
      if (audit.selects.length) fail(`index.html(${theme}):以下 select 不是 base-select:${audit.selects.join(", ")}`);
      if (audit.bare.length) fail(`index.html(${theme}):以下 dialog/[popover] 缺 .k-surface:${audit.bare.join(", ")}`);
      if (audit.fixed.length) fail(`index.html(${theme}):顶层以外出现 fixed 浮层(除 .resize-handle):${audit.fixed.join(", ")}`);
      if (appErrors.length) fail(`index.html(${theme}):页面异常 ${appErrors.join(" | ")}`);
      notes.push(`index.html ${theme}:${audit.selectCount} 个 select 全为 base-select`);
      await app.screenshot({ path: path.join(outDir, `${theme}-index.png`) });
      await appContext.close();
    }

    // ── 分区:对话单列与输入区 ──
    // 6 输入区控件几何(UI2-0926 #11,scripts/ui-composer-geometry.mjs):控件等高、全部 select 居中、同行无叠压且不越界、
    //   模式芯片墨迹居中、满列单行;每次运行自带三种注入回归的自检。
    const { runComposerGeometry } = await import("./ui-composer-geometry.mjs");
    const geometry = await runComposerGeometry(browser, origin);
    for (const failure of geometry.failures) fail(failure);
    notes.push(...geometry.notes);

    // ── 分区:后台任务侧栏与可调框 ──
    // 8 停靠侧栏的真实布局与键盘(UI2-0926 #14 复核修复;假 DOM 看不到几何与原生下拉):
    //   a) 1600×900 侧栏停靠时,权限卡左右边与输入区重合(同宽、锚在输入区上方)、不压侧栏;
    //      收起成「重新打开询问」芯片后,芯片右缘 ≤ 侧栏左缘;
    //   b) 「筛选与清理」菜单里原生下拉的列表开着时按 Esc:只收列表,侧栏与菜单都还在;再按收菜单;
    //      焦点回到侧栏里之后 Esc 才关侧栏;
    //   c) 侧栏不宽于 440(1280 宽的 352)时委派卡小表隐去模型列、子代理列拿回宽度;2000 宽(520)保留四列。
    // KZ_SMOKE_MUTATE=<id>(TASKS_BROWSER_MUTATIONS)经 page.route 把被守护的源码改坏后再跑,这里必须变红。
    {
      const mutation = await tasksBrowserMutation();
      if (mutation) notes.push(`[KZ_SMOKE_MUTATE=${mutation.id}] 已改坏 ${mutation.file},期望本次失败`);
      const tasksContext = await browser.newContext({ viewport: { width: 1600, height: 900 }, colorScheme: "dark" });
      if (mutation) {
        await tasksContext.route(`**/${mutation.file}`, (route) => route.fulfill({ status: 200, contentType: mutation.type, body: mutation.body }));
      }
      const tasksErrors = [];
      const boxOf = (page, selector) => page.evaluate((sel) => {
        const el = document.querySelector(sel);
        if (!el || el.classList.contains("hidden")) return null;
        const r = el.getBoundingClientRect();
        return { left: r.left, right: r.right, top: r.top, bottom: r.bottom, width: r.width, height: r.height };
      }, selector);
      // a) 权限卡与芯片
      const askPage = await tasksContext.newPage();
      askPage.on("pageerror", (error) => tasksErrors.push(String(error)));
      await askPage.goto(`${origin}/?theme=dark&scene=overlays&dialog=ask`, { waitUntil: "load" });
      await askPage.waitForFunction(() => window.__kzPreview?.ready === true, null, { timeout: 25000 });
      await askPage.waitForFunction(() => !document.getElementById("ask-overlay")?.classList.contains("hidden"), null, { timeout: 5000 }).catch(() => {});
      await askPage.click("#tasks-toggle");
      await askPage.waitForTimeout(300);
      const dock = await askPage.evaluate(() => document.getElementById("tasks-panel")?.dataset.dock);
      const panelBox = await boxOf(askPage, "#tasks-panel");
      const askBox = await boxOf(askPage, "#ask-overlay");
      const composerBox = await boxOf(askPage, "#composer");
      if (dock !== "side" || !panelBox || !askBox || !composerBox) {
        fail(`后台任务侧栏/权限卡:1600 宽手动打开应停靠、权限卡可见 ${JSON.stringify({ dock, panelBox, askBox, composerBox })}`);
      } else {
        if (Math.abs(askBox.left - composerBox.left) > 1 || Math.abs(askBox.right - composerBox.right) > 1) {
          fail(`后台任务侧栏/权限卡:侧栏停靠时权限卡应与输入区同宽(左右边重合)${JSON.stringify({ askBox, composerBox })}`);
        }
        if (askBox.bottom > composerBox.top + 1) fail(`后台任务侧栏/权限卡:权限卡应在输入区上方 ${JSON.stringify({ askBox, composerBox })}`);
        if (askBox.right > panelBox.left + 0.5) fail(`后台任务侧栏/权限卡:权限卡压住了停靠的侧栏 ${JSON.stringify({ askBox, panelBox })}`);
      }
      await askPage.click("#ask-collapse");
      await askPage.waitForTimeout(200);
      const chipBox = await boxOf(askPage, "#ask-reopen");
      const panelAfter = await boxOf(askPage, "#tasks-panel");
      if (!chipBox || !panelAfter) fail(`后台任务侧栏/芯片:收起权限卡后「重新打开询问」芯片或侧栏不可见 ${JSON.stringify({ chipBox, panelAfter })}`);
      else if (chipBox.right > panelAfter.left + 0.5) fail(`后台任务侧栏/芯片:「重新打开询问」芯片压住了停靠的侧栏(芯片右缘 ${chipBox.right} > 侧栏左缘 ${panelAfter.left})`);
      await askPage.screenshot({ path: path.join(outDir, "tasks-ask-chip.png") });
      await askPage.close();

      // b) 弹层里的 Esc;c) 窄侧栏的小表
      const chatPage = await tasksContext.newPage();
      chatPage.on("pageerror", (error) => tasksErrors.push(String(error)));
      await chatPage.goto(`${origin}/?theme=dark&scene=chat`, { waitUntil: "load" });
      await chatPage.waitForFunction(() => window.__kzPreview?.ready === true, null, { timeout: 25000 });
      await chatPage.waitForTimeout(300);
      if (await chatPage.evaluate(() => document.getElementById("tasks-panel").classList.contains("hidden"))) await chatPage.click("#tasks-toggle");
      await chatPage.waitForTimeout(200);
      const escState = () => chatPage.evaluate(() => {
        let picker = null;
        try { picker = document.getElementById("bg-status-filter").matches(":open"); } catch { /* 忽略 */ }
        return {
          panel: !document.getElementById("tasks-panel").classList.contains("hidden"),
          menu: document.getElementById("tasks-filter-menu").matches(":popover-open"),
          picker,
        };
      });
      await chatPage.click("#tasks-filter");
      await chatPage.waitForTimeout(150);
      await chatPage.click("#bg-status-filter");
      await chatPage.waitForTimeout(200);
      const escOpen = await escState();
      await chatPage.keyboard.press("Escape");
      await chatPage.waitForTimeout(200);
      const esc1 = await escState();
      await chatPage.keyboard.press("Escape");
      await chatPage.waitForTimeout(200);
      const esc2 = await escState();
      if (!escOpen.menu || escOpen.picker !== true) fail(`后台任务侧栏/Esc:前置——筛选菜单与其中的下拉列表应都打开 ${JSON.stringify(escOpen)}`);
      else if (!esc1.panel || !esc1.menu || esc1.picker) fail(`后台任务侧栏/Esc:下拉列表开着时按 Esc 应只收列表(侧栏与菜单都还在)${JSON.stringify(esc1)}`);
      else if (!esc2.panel || esc2.menu) fail(`后台任务侧栏/Esc:再按 Esc 应只收菜单 ${JSON.stringify(esc2)}`);
      const modelLayout = () => chatPage.evaluate(() => {
        const row = document.querySelector("#tasks-panel .tp-agent-row");
        if (!row) return null;
        const name = row.querySelector(".tp-agent-name").getBoundingClientRect().width;
        return {
          panel: Math.round(document.getElementById("tasks-panel").getBoundingClientRect().width),
          modelShown: getComputedStyle(row.querySelector(".tp-agent-model")).display !== "none",
          nameShare: name / row.getBoundingClientRect().width,
        };
      });
      await chatPage.setViewportSize({ width: 2000, height: 1040 });
      await chatPage.waitForTimeout(400);
      const wide = await modelLayout();
      await chatPage.setViewportSize({ width: 1280, height: 840 });
      await chatPage.waitForTimeout(400);
      const narrow = await modelLayout();
      if (!wide || !narrow) fail(`后台任务侧栏/小表:场景里应有委派卡的子代理行 ${JSON.stringify({ wide, narrow })}`);
      else {
        if (!(wide.panel > 440 && wide.modelShown)) fail(`后台任务侧栏/小表:2000 宽(侧栏 ${wide.panel})应保留模型列 ${JSON.stringify(wide)}`);
        if (!(narrow.panel <= 440 && !narrow.modelShown && narrow.nameShare >= 0.55)) {
          fail(`后台任务侧栏/小表:侧栏不宽于 440 时应隐去模型列、子代理列拿回宽度(≥55%)${JSON.stringify(narrow)}`);
        }
      }
      await chatPage.screenshot({ path: path.join(outDir, "tasks-narrow-1280.png") });
      await chatPage.close();
      if (tasksErrors.length) fail(`后台任务侧栏:页面异常 ${tasksErrors.join(" | ")}`);
      await tasksContext.close();
      notes.push(`后台任务侧栏:权限卡与输入区同宽不压侧栏、芯片让开侧栏、弹层 Esc 只关栈顶、窄侧栏隐去模型列(2000 → ${wide?.panel}px 四列 / 1280 → ${narrow?.panel}px 三列)`);
    }
  } finally {
    await browser.close();
    await close();
  }
  return { failures, notes };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const { failures, notes } = await runSurfaceGallerySmoke();
  for (const note of notes) console.log(`  · ${note}`);
  assert.deepEqual(failures, [], `弹层样例浏览器冒烟失败:\n${failures.join("\n")}`);
  console.log("弹层样例浏览器冒烟通过:特性探测、双主题 token/对比度/包围盒、下拉亮度、Esc 叠放、index.html 运行时对照");
}
