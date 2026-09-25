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
//   5. index.html 运行时对照:全部 select 为 base-select,全部 dialog/[popover] 带 .k-surface,
//      除 .resize-handle 外没有顶层以外、正在显示的 fixed 元素;
//   6. 截图写入 dist/ui-gallery/(已在 .gitignore)。
// page.evaluate 回调在浏览器里执行,用到的浏览器全局在这里声明给 ESLint(本文件其余部分是 node 环境)。
/* global window, getComputedStyle, CSS, HTMLDialogElement, HTMLElement */
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
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

      // 静态矩阵(暗色页面下两套主题同屏)
      if (theme === "dark") {
        await page.locator("#demo-matrix").scrollIntoViewIfNeeded();
        await page.locator("#demo-matrix").screenshot({ path: path.join(outDir, "matrix.png") });
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
        return { selects, bare, fixed, selectCount: document.querySelectorAll("select").length };
      });
      if (audit.selects.length) fail(`index.html(${theme}):以下 select 不是 base-select:${audit.selects.join(", ")}`);
      if (audit.bare.length) fail(`index.html(${theme}):以下 dialog/[popover] 缺 .k-surface:${audit.bare.join(", ")}`);
      if (audit.fixed.length) fail(`index.html(${theme}):顶层以外出现 fixed 浮层(除 .resize-handle):${audit.fixed.join(", ")}`);
      if (appErrors.length) fail(`index.html(${theme}):页面异常 ${appErrors.join(" | ")}`);
      notes.push(`index.html ${theme}:${audit.selectCount} 个 select 全为 base-select`);
      await app.screenshot({ path: path.join(outDir, `${theme}-index.png`) });
      await appContext.close();
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
