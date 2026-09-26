// UI2-0926 #10 星座背景浏览器冒烟(docs/design/ui_chat_backdrop.md §8),由 ui-lint-smoke.mjs 在弹层样例冒烟之后调用,
// 也可单独运行:node scripts/ui-constellation-browser-smoke.mjs。
//
// 纯函数冒烟(ui-constellation-smoke)与运行时冒烟(假 DOM,渲染器走降级空实现)都看不到绘制与调度。这里用无头 Edge
// (playwright-core channel msedge,与弹层样例冒烟同一路线)打开预览页(scripts/ui-preview,模拟 IPC 跑真实前端),
// 插桩 clearRect 数帧、逐像素读画布,实测渲染器层面的承诺:
//   ① 欢迎舞台空闲 ≤ 9 帧/秒;失败会话(blocked)≤ 9 帧/秒;
//   ② 消息对话、其它视图、窗口隐藏、背景关闭、减少动态效果:0 动画帧;
//   ③ 欢迎舞台每 16ms 一次 assistant_streaming 仍 ≥ 20 帧/秒;
//   ④ 连续 60 次改窗口尺寸期间画布始终非空;
//   ⑤ 欢迎页文案旁构图不压文案;深浅主题消息对话的 SVG 隐藏、Canvas 清空;
//   ⑥ 窄屏欢迎装饰(capped):全图像素合成到 --chat-bg,原本 ≥ 4.5 的字色仍 ≥ 4.5,
//      alpha 不超过 watermarkAlpha;
//   ⑦ 启动:后端存「关闭」时,第一次 kz:backdrop-settings 之前画布上一笔都没画,且第一次发布就是关闭。
// 变异:KZ_SMOKE_MUTATE=<下表 id> 时经 page.route 改写被守护的源码(必须命中,否则直接失败),期望本次运行失败。
// 不认识的 id(其它冒烟的变异)一律忽略。截图写入 dist/ui-constellation/(已在 .gitignore 的 dist 下)。
// page.evaluate 回调在浏览器里执行,用到的浏览器全局在这里声明给 ESLint(本文件其余部分是 node 环境)。
/* global window, document, performance, getComputedStyle, CanvasRenderingContext2D, SVGElement, XMLSerializer, Image, NodeFilter, CustomEvent, Event */
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// ---------- 变异表:{ file: 预览服务上的路径, edits: [{ pattern, replace, all }] } ----------
// all=false 时必须恰好命中一处;all=true 时至少命中一处(同一个判据在渲染器里有几道冗余闸,要一起拆)。
export const BROWSER_MUTATIONS = {
  // 空闲按 125ms 一帧。改成恒按忙帧,空闲也 30 帧/秒(①)。
  bgBrowserIdle: { file: "/22-constellation-core.js", edits: [{ pattern: /return busy \? BUSY_FRAME_MS : IDLE_FRAME_MS;/, replace: "return BUSY_FRAME_MS;" }] },
  // 失败会话不算忙。改回「非 idle 都算忙」,停在失败会话上一直 30 帧/秒(①)。
  bgBrowserFailed: { file: "/22-constellation-core.js", edits: [{ pattern: /return BUSY_ACTIVITIES\.includes\(activity\);/, replace: 'return activity !== "idle";' }] },
  // 不在对话视图就停。拆掉视图判断,切到设置页仍在画(②)。
  bgBrowserView: { file: "/22-constellation.js", edits: [{ pattern: /return !view \|\| view\.classList\.contains\("active"\);/, replace: "return true;" }] },
  // 窗口隐藏就停。渲染器里所有 document.hidden 判断一起拆掉,最小化后仍在画(②)。
  bgBrowserHidden: { file: "/22-constellation.js", edits: [{ pattern: /document\.hidden/g, replace: "false", all: true }] },
  // 背景关闭就停。渲染器里所有 current.enabled 判断一起拆掉,关掉背景后仍在画(②)。
  bgBrowserOff: { file: "/22-constellation.js", edits: [{ pattern: /current\.enabled/g, replace: "true", all: true }] },
  // 减少动态效果只画静帧。忽略系统设置,照常动画(②)。
  bgBrowserReduced: { file: "/22-constellation.js", edits: [{ pattern: /const reduced = \(\) => Boolean\(reducedQuery\?\.matches\);/, replace: "const reduced = () => false;" }] },
  // 流式事件下不饿死有三道:已排好的帧不被新事件取消(shouldHurry)、流式唤醒节流、定时器从上一帧起算。
  // 任一道单独都能扛住 16ms 连发,三道一起拆回复核前的写法,渲染器被饿死(③)。
  bgBrowserStream: {
    file: "/22-constellation.js",
    edits: [
      { pattern: /if \(shouldHurry\(\{ timer: frameTimer, raf: frameRaf, busy: isBusy\(now\), dueIn: dueAt - now \}\)\) \{/, replace: "if (isBusy(now)) { cancelFrame();" },
      { pattern: /if \(changed \|\| spawned \|\| now - lastWake >= WAKE_THROTTLE_MS\) \{/, replace: "if (true) {" },
      { pattern: /const wait = Math\.max\(0, delay - \(now - lastFrameAt\) - FRAME_SLACK_MS\);/, replace: "const wait = delay;" },
    ],
  },
  // 画布换尺寸(被清空)时同步补画一帧。删了它,拖动改窗口尺寸期间画布一直空白(④)。
  bgBrowserResize: { file: "/22-constellation.js", edits: [{ pattern: /if \(still\(\) \|\| resized\) draw\(clock\(\)\);/, replace: "if (still()) draw(clock());" }] },
  // 不压正文的构图用 evenodd 挖掉避让区。拆掉剪裁,星尘 / 连线画进正文列(⑤)。
  bgBrowserClip: { file: "/22-brand-backdrop.js", edits: [{ pattern: /const holes = !layout\.capped \?/, replace: "const holes = false ?" }] },
  // 水印走离屏层 + 单一不透明度合成。拆掉离屏,水印配比直接画上画布,正文压在星上跌破 4.5(⑥)。
  bgBrowserWatermark: { file: "/22-brand-backdrop.js", edits: [{ pattern: /watermarkAlpha\(kit\.watermark, prefs\.opacity\)/, replace: "1" }] },
  // 启动时等第一次偏好发布才开始画。改成立即开始,关掉背景的用户启动时先闪一帧默认星座(⑦)。
  bgBrowserBoot: { file: "/22-neural-flow.js", edits: [{ pattern: /waitForPrefs: true/, replace: "waitForPrefs: false" }] },
};

// ---------- 浏览器端插桩(addInitScript):数帧、记录画笔与偏好发布 ----------
function instrument() {
  const proto = CanvasRenderingContext2D.prototype;
  const isChat = (ctx) => ctx.canvas?.id === "neural-flow-chat";
  window.__bg = { frames: 0, paints: [], prefs: [] };
  const setSvgAttribute = SVGElement.prototype.setAttribute;
  SVGElement.prototype.setAttribute = function (name, value) {
    if (this.id === "neural-flow-brand" && name === "viewBox" && window.__bg.paints.length < 64) window.__bg.paints.push(performance.now());
    return setSvgAttribute.call(this, name, value);
  };
  // Audit the actual SVG output as pixels too; reading its now-empty sibling canvas
  // would silently stop checking clipping and text contrast after the renderer change.
  window.__backdropPixels = async () => {
    const canvas = document.getElementById("neural-flow-chat");
    const svg = document.getElementById("neural-flow-brand");
    if (!svg || getComputedStyle(svg).display === "none") return canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    const copy = svg.cloneNode(true);
    copy.setAttribute("width", String(canvas.width));
    copy.setAttribute("height", String(canvas.height));
    const bitmap = new Image();
    bitmap.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(new XMLSerializer().serializeToString(copy))}`;
    await bitmap.decode();
    const raster = document.createElement("canvas");
    raster.width = canvas.width; raster.height = canvas.height;
    const g = raster.getContext("2d");
    g.drawImage(bitmap, 0, 0);
    return g.getImageData(0, 0, raster.width, raster.height).data;
  };
  const clear = proto.clearRect;
  proto.clearRect = function (...args) {
    if (isChat(this)) window.__bg.frames += 1;
    return clear.apply(this, args);
  };
  for (const name of ["drawImage", "stroke"]) {
    const original = proto[name];
    proto[name] = function (...args) {
      if (isChat(this) && window.__bg.paints.length < 64) window.__bg.paints.push(performance.now());
      return original.apply(this, args);
    };
  }
  document.addEventListener("kz:backdrop-settings", (event) => {
    window.__bg.prefs.push({ at: performance.now(), enabled: event.detail?.enabled, preset: event.detail?.preset });
  }, true);
}

// 预览页固定画静帧(截图可复现);冒烟要测调度,先切到真实动画模式。
async function goLive(page) {
  await page.evaluate(() => {
    delete document.documentElement.dataset.kzPreview;
    document.dispatchEvent(new CustomEvent("kz:view-changed", { detail: { view: "chat" } }));
  });
}
async function fps(page, ms) {
  return page.evaluate(async (ms) => {
    const f0 = window.__bg.frames;
    await new Promise((resolve) => setTimeout(resolve, ms));
    return (window.__bg.frames - f0) / (ms / 1000);
  }, ms);
}
async function snapshot(page) {
  return page.evaluate(async () => (await import("/22-neural-flow.js")).chatBackdrop?.snapshot() ?? null);
}

// 逐像素审计:正文文本矩形(活动 pane / 空态文案 / 语音文案里所有可见文本节点)与避让区下的画布像素。
// 返回 { maxAlphaText, maxAlphaAvoid, minContrast: {token: ratio}, pixels, rects }。
async function pixelAudit(page, wholeCanvas = false) {
  return page.evaluate(async (wholeCanvas) => {
    const canvas = document.getElementById("neural-flow-chat");
    const { width: W, height: H } = canvas;
    const data = await window.__backdropPixels();
    const box = canvas.getBoundingClientRect();
    const scale = W / box.width;
    const css = getComputedStyle(document.documentElement);
    const probe = document.createElement("canvas").getContext("2d");
    const rgb = (value) => {
      probe.fillStyle = "#000";
      probe.fillStyle = value.trim();
      const text = String(probe.fillStyle);
      const hex = text.match(/^#(..)(..)(..)$/);
      return hex ? [1, 2, 3].map((i) => parseInt(hex[i], 16)) : text.match(/[\d.]+/g).slice(0, 3).map(Number);
    };
    const lum = (c) => c.reduce((sum, v, i) => { const s = v / 255; return sum + [0.2126, 0.7152, 0.0722][i] * (s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4); }, 0);
    const ratio = (a, b) => { const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x); return (hi + 0.05) / (lo + 0.05); };
    const bg = rgb(css.getPropertyValue("--chat-bg"));
    const texts = ["--fg", "--fg-strong", "--dim", "--accent-text", "--ok", "--err", "--warn"]
      .map((name) => [name, rgb(css.getPropertyValue(name))])
      .filter(([, color]) => ratio(color, bg) >= 4.5);
    const rects = [];
    const walker = document.createTreeWalker(document.getElementById("chat-area"), NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (!node.textContent.trim()) continue;
      const el = node.parentElement;
      // .sr-only 是只给读屏的隐形文字(如图标化复制键的「复制」),1px 剪裁、屏幕上看不见,不算正文。
      if (!el || el.closest("#agent-panel, #bg-panel, #tasks-panel, #composer, .sr-only") || !el.checkVisibility?.()) continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      for (const r of range.getClientRects()) if (r.width && r.height) rects.push({ x: r.left - box.left, y: r.top - box.top, w: r.width, h: r.height });
    }
    // 窄屏装饰不一定实际碰到文字;全图审计证明任一像素放在文字下仍安全。
    if (wholeCanvas) rects.splice(0, rects.length, { x: 0, y: 0, w: box.width, h: box.height });
    const scan = (list, visit) => {
      for (const r of list) {
        const x0 = Math.max(0, Math.floor(r.x * scale)), x1 = Math.min(W, Math.ceil((r.x + r.w) * scale));
        const y0 = Math.max(0, Math.floor(r.y * scale)), y1 = Math.min(H, Math.ceil((r.y + r.h) * scale));
        for (let y = y0; y < y1; y += 1) for (let x = x0; x < x1; x += 1) visit((y * W + x) * 4);
      }
    };
    let maxAlphaText = 0, pixels = 0;
    const minContrast = Object.fromEntries(texts.map(([name]) => [name, Infinity]));
    scan(rects, (i) => {
      const a = data[i + 3] / 255;
      if (!a) return;
      pixels += 1;
      maxAlphaText = Math.max(maxAlphaText, a);
      // 画布像素是非预乘的 rgba;合成到 chat-bg 上即正文实际的底色
      const px = [0, 1, 2].map((k) => data[i + k] * a + bg[k] * (1 - a));
      for (const [name, color] of texts) minContrast[name] = Math.min(minContrast[name], ratio(color, px));
    });
    return { rects: rects.length, pixels, maxAlphaText, minContrast };
  }, wholeCanvas);
}
// 避让区单独扫(避让区来自渲染器 snapshot,需要 await import,拆成两步免得 evaluate 里混 async)。
async function avoidAudit(page) {
  return page.evaluate(async () => {
    const snap = (await import("/22-neural-flow.js")).chatBackdrop.snapshot();
    const canvas = document.getElementById("neural-flow-chat");
    const { width: W, height: H } = canvas;
    const data = await window.__backdropPixels();
    const scale = W / canvas.getBoundingClientRect().width;
    let max = 0, count = 0;
    // 剪裁边抗锯齿:避让区四边各内缩 1 个设备像素再扫(正文文本矩形另由 pixelAudit 逐像素扫,不内缩)。
    for (const r of snap.avoid ?? []) {
      const x0 = Math.max(0, Math.ceil(r.x * scale) + 1), x1 = Math.min(W, Math.floor((r.x + r.w) * scale) - 1);
      const y0 = Math.max(0, Math.ceil(r.y * scale) + 1), y1 = Math.min(H, Math.floor((r.y + r.h) * scale) - 1);
      for (let y = y0; y < y1; y += 1) for (let x = x0; x < x1; x += 1) {
        const a = data[(y * W + x) * 4 + 3];
        if (a) { count += 1; max = Math.max(max, a / 255); }
      }
    }
    let nonBlank = 0;
    for (let i = 3; i < data.length; i += 4) if (data[i]) nonBlank += 1;
    return { avoid: (snap.avoid ?? []).length, max, count, nonBlank };
  });
}

export async function runConstellationBrowserSmoke({ channel = "msedge", outDir = path.join(root, "dist/ui-constellation"), mutate = process.env.KZ_SMOKE_MUTATE ?? "" } = {}) {
  const failures = [];
  const fail = (message) => failures.push(message);
  const notes = [];
  const mutation = BROWSER_MUTATIONS[mutate] ?? null;
  const mutationHits = new Map();
  await mkdir(outDir, { recursive: true });
  const { origin, close } = await startPreviewServer({ port: 0 });
  const browser = await chromium.launch({ channel, headless: true });

  async function open(scene, { theme = "dark", width = 1600, height = 834, dpr = 1.25, reduced = false, query = "", css = "" } = {}) {
    const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: dpr, colorScheme: theme, reducedMotion: reduced ? "reduce" : "no-preference" });
    if (mutation) {
      await context.route((url) => url.pathname === mutation.file, async (route) => {
        const response = await route.fetch();
        let body = await response.text();
        mutation.edits.forEach((edit, index) => {
          const hits = (body.match(new RegExp(edit.pattern.source, "g")) ?? []).length;
          mutationHits.set(index, hits);
          body = body.replace(edit.all ? new RegExp(edit.pattern.source, "g") : edit.pattern, edit.replace);
        });
        await route.fulfill({ response, body });
      });
    }
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (error) => errors.push(String(error)));
    page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
    await page.addInitScript(instrument);
    await page.goto(`${origin}/?theme=${theme}&scene=${scene}${query}`, { waitUntil: "load" });
    await page.waitForFunction(() => window.__kzPreview?.ready === true, null, { timeout: 25000 });
    if (css) await page.addStyleTag({ content: css });
    return { page, context, errors };
  }
  // 合并后活动/子代理面板变成停靠的 #tasks-panel(chat 场景会自动打开);关掉它,量的是「没有侧栏时」的沟槽构图。
  const closePanel = async (page) => {
    await page.evaluate(() => (document.getElementById("tasks-close") ?? document.getElementById("bg-close"))?.click());
    await page.waitForTimeout(150);
  };
  const settle = async (page, ms = 250) => {
    await page.evaluate(() => document.dispatchEvent(new CustomEvent("kz:view-changed", { detail: { view: "chat" } })));
    await page.waitForTimeout(ms);
  };
  const auditFree = async (page, label) => {
    const snap = await snapshot(page);
    const pixels = await pixelAudit(page);
    const avoid = await avoidAudit(page);
    if (snap.capped) fail(`${label}:构图应不压正文,实际是 capped 水印(${snap.placement})`);
    if (!avoid.avoid) fail(`${label}:没有登记避让区(判据前提)`);
    if (!pixels.rects) fail(`${label}:找不到正文文本矩形(判据定位失效)`);
    if (!avoid.nonBlank) fail(`${label}:画布是空的(判据前提:星座得画出来)`);
    if (pixels.maxAlphaText > 0) fail(`${label}:正文文本下有 ${pixels.pixels} 个画布像素非透明(最大 alpha ${pixels.maxAlphaText.toFixed(3)})`);
    if (avoid.max > 0) fail(`${label}:避让区里有 ${avoid.count} 个画布像素非透明(最大 alpha ${avoid.max.toFixed(3)}),剪裁没挖掉正文列`);
    return { snap, pixels, avoid };
  };
  const auditClean = async (page, label) => {
    const snap = await snapshot(page);
    const pixels = await pixelAudit(page);
    const actual = await page.evaluate(() => {
      const svg = document.getElementById("neural-flow-brand");
      return { display: getComputedStyle(svg).display, chatBackground: getComputedStyle(document.getElementById("chat-area")).backgroundImage };
    });
    if (snap.placement !== "hidden" || snap.capped || actual.display !== "none") fail(`${label}:消息背后仍有装饰`);
    if (pixels.pixels || (await avoidAudit(page)).nonBlank) fail(`${label}:背景画布未清空`);
    if (actual.chatBackground !== "none") fail(`${label}:正文底色不应叠加纹理`);
    const frames = await fps(page, 1000);
    if (frames) fail(`${label}:对话背景仍在刷新(${frames} 帧/秒)`);
    return { snap, pixels };
  };

  try {
    // ---- P1 空态(1600@1.25 暗色,OC 关):文案旁构图不压正文;空闲帧率;其它视图 / 隐藏 / 关闭为 0 帧 ----
    {
      const { page, context, errors } = await open("empty");
      await goLive(page);
      await page.waitForTimeout(2200); // 入场画线 1.6s 走完
      const free = await auditFree(page, "空态");
      const idle = await fps(page, 2000);
      if (idle > 9) fail(`空闲 ${idle.toFixed(1)} 帧/秒 > 9`);
      await page.evaluate(() => document.querySelector('.activity-item[data-view="settings"]')?.click());
      await page.waitForTimeout(400);
      const other = await fps(page, 1200);
      if (other > 0) fail(`切到设置页后仍在画(${other.toFixed(1)} 帧/秒)`);
      await page.evaluate(() => document.querySelector('.activity-item[data-view="chat"]')?.click());
      await page.waitForTimeout(400);
      await page.evaluate(() => {
        Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
        document.dispatchEvent(new Event("visibilitychange"));
      });
      await page.waitForTimeout(400);
      const hidden = await fps(page, 1200);
      if (hidden > 0) fail(`窗口隐藏后仍在画(${hidden.toFixed(1)} 帧/秒)`);
      await page.evaluate(() => {
        Object.defineProperty(document, "hidden", { configurable: true, get: () => false });
        document.dispatchEvent(new Event("visibilitychange"));
      });
      await page.waitForTimeout(400);
      const back = await fps(page, 1000);
      if (!(back > 0)) fail("回到可见后没有恢复作画(判据前提)");
      await page.evaluate(() => {
        const toggle = document.getElementById("set-bg-enabled");
        toggle.checked = false;
        toggle.dispatchEvent(new Event("change"));
      });
      await page.waitForTimeout(400);
      const off = await fps(page, 1200);
      if (off > 0) fail(`关闭背景后仍在画(${off.toFixed(1)} 帧/秒)`);
      if (errors.length) fail(`空态页面异常:${errors.join(" | ")}`);
      notes.push(`空态 ${free.snap.placement}、空闲 ${idle.toFixed(1)} 帧/秒、其它视图 / 隐藏 / 关闭 0 帧`);
      await context.close();
    }

    // ---- P2 减少动态效果:只画静帧 ----
    {
      const { page, context, errors } = await open("empty", { reduced: true });
      await closePanel(page);
      await goLive(page);
      await page.waitForTimeout(600);
      const reduced = await fps(page, 1500);
      if (reduced > 0) fail(`减少动态效果时仍在动画(${reduced.toFixed(1)} 帧/秒)`);
      if (errors.length) fail(`减少动态页面异常:${errors.join(" | ")}`);
      await context.close();
    }

    // ---- P3 欢迎舞台:16ms 流式事件不饿死;失败会话回到空闲帧率 ----
    {
      const { page, context, errors } = await open("empty");
      await closePanel(page);
      await goLive(page);
      await page.waitForTimeout(2000);
      const free = await auditFree(page, "欢迎舞台");
      const stream = await page.evaluate(async () => {
        const flow = await import("/22-neural-flow.js");
        const shell = await import("/03-shell.js");
        const id = shell.activeSessionId;
        shell.sessionStates.set(id, Object.assign(shell.sessionStates.get(id) ?? {}, { phase: "running", running: true, converged: false }));
        const f0 = window.__bg.frames;
        const timer = setInterval(() => flow.neuralFlowEmit("assistant_streaming", { session_id: id, text_length: 4 }), 16);
        await new Promise((resolve) => setTimeout(resolve, 1500));
        clearInterval(timer);
        return { fps: (window.__bg.frames - f0) / 1.5, activity: flow.chatBackdrop.snapshot().activity };
      });
      if (stream.fps < 20) fail(`每 16ms 一次流式事件时只有 ${stream.fps.toFixed(1)} 帧/秒(< 20,渲染器被饿死;活动态 ${stream.activity})`);
      const failed = await page.evaluate(async () => {
        const flow = await import("/22-neural-flow.js");
        const shell = await import("/03-shell.js");
        const id = shell.activeSessionId;
        const state = shell.sessionStates.get(id) ?? {};
        shell.sessionStates.set(id, Object.assign(state, { phase: "failed", running: false, converged: false }));
        flow.neuralFlowEmit("run_failed", { session_id: id, message: "smoke" });
        await new Promise((resolve) => setTimeout(resolve, 1800)); // 涟漪 900ms、光点走完本程
        const f0 = window.__bg.frames;
        await new Promise((resolve) => setTimeout(resolve, 2000));
        return { fps: (window.__bg.frames - f0) / 2, snap: flow.chatBackdrop.snapshot() };
      });
      if (failed.snap.activity !== "blocked") fail(`判据前提:失败会话活动态应为 blocked,实际 ${failed.snap.activity}`);
      if (failed.fps > 9) fail(`停在失败会话上 ${failed.fps.toFixed(1)} 帧/秒 > 9(blocked 没有常驻动画,应按空闲)`);
      if (errors.length) fail(`对话态页面异常:${errors.join(" | ")}`);
      notes.push(`欢迎舞台 ${free.snap.placement}、流式 16ms ${stream.fps.toFixed(1)} 帧/秒、失败会话 ${failed.fps.toFixed(1)} 帧/秒`);
      await page.screenshot({ path: path.join(outDir, "welcome-1600-dark.png") });
      await context.close();
    }

    // ---- P4 拖动改窗口尺寸:60 次、每次 16ms,期间画布始终非空 ----
    {
      const { page, context, errors } = await open("empty", { dpr: 1 });
      await goLive(page);
      await page.waitForTimeout(2000);
      const blank = [];
      for (let k = 0; k < 60; k += 1) {
        await page.setViewportSize({ width: 1600 - (k % 2 ? 6 : 0) - k * 2, height: 834 });
        await page.waitForTimeout(16);
        if (k % 5 === 4) {
          const { nonBlank } = await avoidAudit(page);
          if (!nonBlank) blank.push(k);
        }
      }
      if (blank.length) fail(`拖动改窗口尺寸期间画布空白(第 ${blank.join(", ")} 次采样)`);
      if (errors.length) fail(`改尺寸页面异常:${errors.join(" | ")}`);
      await context.close();
    }

    // ---- P5 消息对话:两套主题都无图案,运行事件也不触发背景帧 ----
    for (const theme of ["dark", "light"]) {
      const COLUMN_768 = "#messages > .msg-pane { max-width: 768px; margin-inline: auto; } #view-chat #messages { padding-left: 0; padding-right: 0; }";
      const { page, context, errors } = await open("chat", { theme, width: 1333, height: 695, dpr: 1.5, css: COLUMN_768 });
      await closePanel(page);
      await goLive(page);
      await page.waitForTimeout(2000);
      const free = await auditClean(page, `${theme} 消息对话`);
      await page.evaluate(async () => {
        const flow = await import("/22-neural-flow.js");
        const shell = await import("/03-shell.js");
        flow.neuralFlowEmit("assistant_streaming", { session_id: shell.activeSessionId, text_length: 4 });
      });
      await auditClean(page, `${theme} 流式对话`);
      if (errors.length) fail(`窄沟页面异常:${errors.join(" | ")}`);
      notes.push(`${theme} 1333×695@1.5 消息对话:${free.snap.placement},0 帧`);
      await page.screenshot({ path: path.join(outDir, `chat-1333-${theme}.png`) });
      await context.close();
    }

    // ---- P6 窄屏欢迎舞台(capped):逐帧审计文案下像素,两套主题 ----
    for (const theme of ["dark", "light"]) {
      const { page, context, errors } = await open("empty", { theme, width: 950, height: 720, dpr: 1.5 });
      await goLive(page);
      await page.waitForTimeout(1200);
      const snap = await snapshot(page);
      if (!snap.capped) {
        fail(`${theme} 判据前提:950×720 欢迎页应落到 capped 装饰,实际 ${snap.placement}`);
        await context.close();
        continue;
      }
      let worst = Infinity, worstName = "", maxAlpha = 0, pixels = 0;
      for (let k = 0; k < 10; k += 1) {
        await page.waitForTimeout(170);
        const audit = await pixelAudit(page, true);
        pixels += audit.pixels;
        maxAlpha = Math.max(maxAlpha, audit.maxAlphaText);
        for (const [name, value] of Object.entries(audit.minContrast)) if (value < worst) { worst = value; worstName = name; }
      }
      if (!pixels) fail(`${theme} 窄屏欢迎装饰未绘制(判据前提)`);
      if (worst < 4.5) fail(`${theme} 水印:正文 ${worstName} 压在水印像素上只有 ${worst.toFixed(2)}:1(< 4.5)`);
      if (maxAlpha > snap.watermarkAlpha + 2 / 255) fail(`${theme} 水印:正文下像素 alpha ${maxAlpha.toFixed(3)} 超过 watermarkAlpha ${snap.watermarkAlpha.toFixed(3)}`);
      if (errors.length) fail(`${theme} 水印页面异常:${errors.join(" | ")}`);
      notes.push(`${theme} 欢迎装饰 ${snap.placement}:全图 ${pixels} 像素,文案压在最坏像素上 ${worstName} ${Number.isFinite(worst) ? worst.toFixed(2) : "—"}:1,alpha ≤ ${maxAlpha.toFixed(3)}(上限 ${snap.watermarkAlpha.toFixed(3)})`);
      await page.screenshot({ path: path.join(outDir, `chat-watermark-${theme}.png`) });
      await context.close();
    }

    // ---- P7 启动:后端存「关闭」,第一次发布之前一笔都不画,第一次发布就是关闭 ----
    {
      const { page, context, errors } = await open("empty", { query: "&backdrop=off" });
      await page.waitForTimeout(300);
      const boot = await page.evaluate(() => ({ prefs: window.__bg.prefs, paints: window.__bg.paints }));
      const first = boot.prefs[0];
      if (!first) fail("启动:没有收到任何 kz:backdrop-settings(判据前提)");
      else {
        if (first.enabled !== false) fail(`启动:后端存「关闭」,第一次发布却是 ${JSON.stringify(first)}(关掉背景的用户会先看到默认星座)`);
        const early = boot.paints.filter((at) => at < first.at);
        if (early.length) fail(`启动:第一次偏好发布之前画布上已经画了 ${early.length} 笔(默认星座闪一下)`);
      }
      if (boot.paints.length) fail(`启动:背景关闭却画了 ${boot.paints.length} 笔`);
      if (errors.length) fail(`启动页面异常:${errors.join(" | ")}`);
      await context.close();
    }
  } finally {
    await browser.close();
    await close();
  }
  if (mutation) {
    mutation.edits.forEach((edit, index) => {
      const hits = mutationHits.get(index) ?? 0;
      if (edit.all ? hits < 1 : hits !== 1) failures.push(`[KZ_SMOKE_MUTATE=${mutate}] 第 ${index + 1} 处改写命中 ${hits} 处:护栏已经失效,先修变异表`);
    });
  }
  return { failures, notes, mutation: mutation ? mutate : null };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const { failures, notes, mutation } = await runConstellationBrowserSmoke();
  for (const note of notes) console.log(`  · ${note}`);
  if (mutation) {
    if (failures.length) {
      console.error(`[KZ_SMOKE_MUTATE=${mutation}] 变异按预期被抓住:\n${failures.join("\n")}`);
      process.exitCode = 1;
    } else {
      console.error(`[KZ_SMOKE_MUTATE=${mutation}] 变异后仍全绿:这道护栏是恒绿的,先修断言`);
      process.exitCode = 1;
    }
  } else {
    assert.deepEqual(failures, [], `星座背景浏览器冒烟失败:\n${failures.join("\n")}`);
    console.log("星座背景浏览器冒烟通过:帧预算、暂停、流式不饿死、改尺寸不空白、正文下像素、水印对比度、启动不闪");
  }
}
