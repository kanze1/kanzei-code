// R-269 浏览器工具辅进程:playwright-core channel 模式自 launch 本机 Edge/Chrome headless。
//
// Rust 侧(browser_tool.rs)经 stdio 与本脚本通信,协议为 JSON-RPC over stdio:
//   请求:   {"id": 1, "method": "open", "params": {...}}\n
//   响应:   {"id": 1, "result": {...}}\n  或  {"id": 1, "error": "..."}\n
// 每行一个 JSON(无换行嵌入),请求与响应一一对应(id 配对)。
//
// 自 launch 实例不碰 WebView2,天然绕开 D-319(WebView2 DevTools 端口不监听的
// 环境问题)——browser 由本脚本用 channel 模式启动本机已安装的 Edge/Chrome,
// 与 e2e-smoke 的 connectOverCDP 路线完全无关。
//
// 生命周期:stdin 关闭(或收到 "shutdown" 请求)即关闭 browser 并退出——不留
// 僵尸 headless 实例。空闲回收由 Rust 侧空闲超时触发 shutdown。
//
// UI2-0926 #8:本脚本随安装包放在 <安装目录>/scripts/ 下(tauri bundle resource),
// 那里没有 node_modules;playwright-core 先按常规解析,失败再从 KANZEI_PLAYWRIGHT_ROOT
// (Rust 侧传入的仓库根)解析。DOM walker 与面板后端共用 browser-dom-walker.mjs 一份。

import { createRequire } from "node:module";
import path from "node:path";
import { domWalker } from "./browser-dom-walker.mjs";

// ---- 状态:单 browser / 单 page / 单 context(边界:不做多 tab/多上下文) ----
let browser = null;
let browserChannel = null;
let context = null;
let page = null;
let consoleEntries = [];
let chromiumCache = null;

// console 条目在内存里最多留这么多(按到达顺序丢头)。
const CONSOLE_CAP = 500;
// console all=true 时返回最近这么多条。
const CONSOLE_ALL_LIMIT = 200;
// wait 的上限(与 Rust 侧 MAX_WAIT_MS 一致)。
const MAX_WAIT_MS = 10000;

async function loadChromium() {
  if (chromiumCache) return chromiumCache;
  try {
    chromiumCache = (await import("playwright-core")).chromium;
    return chromiumCache;
  } catch (first) {
    const extra = process.env.KANZEI_PLAYWRIGHT_ROOT;
    if (extra) {
      try {
        const require = createRequire(path.join(extra, "package.json"));
        chromiumCache = require("playwright-core").chromium;
        return chromiumCache;
      } catch {
        // 落到下面的统一报错。
      }
    }
    throw new Error(
      `找不到 playwright-core(${String(first?.message ?? first)})。无头浏览器需要 playwright-core:` +
        `在 kanzei 仓库根执行 npm install,或设置 KANZEI_PLAYWRIGHT_ROOT 指向含 node_modules/playwright-core 的目录。` +
        `桌面端打开「网页预览」面板时 browser 走面板,不需要它。`,
    );
  }
}

// 从 channel 名解析浏览器可执行文件:msedge -> Edge(channel: "msedge"),
// chrome -> Chrome(channel: "chrome")。playwright-core 会找系统安装路径。
// 未知 channel 明确报错(验收⑤:无浏览器/非法配置诊断明确,不静默降级)。
function resolveChannel(channel) {
  if (channel === "msedge" || channel === "chrome") {
    return channel;
  }
  throw new Error(
    `未知浏览器 channel: ${channel ?? "(空)"}。支持 msedge(默认)或 chrome;` +
      `若本机未安装 Edge/Chrome,请安装其一后重试。`,
  );
}

function resetState() {
  browser = null;
  browserChannel = null;
  context = null;
  page = null;
}

function pushConsole(entry) {
  consoleEntries.push(entry);
  if (consoleEntries.length > CONSOLE_CAP) consoleEntries.splice(0, consoleEntries.length - CONSOLE_CAP);
}

async function ensureBrowser(channel) {
  const requestedChannel = resolveChannel(channel ?? "msedge");
  if (browser && browser.isConnected && !browser.isConnected()) {
    resetState();
  }
  // 同一辅进程允许调用方显式切换 channel。旧实现一旦先开了 Edge，后续传
  // chrome 也会静默复用 Edge，工具回显与真实执行面不一致。
  if (browser && browserChannel !== requestedChannel) {
    await browser.close().catch(() => {});
    resetState();
  }
  if (browser) {
    return;
  }
  const chromium = await loadChromium();
  browser = await chromium.launch({
    channel: requestedChannel,
    headless: true,
    args: ["--no-first-run", "--disable-features=msEdgeSidebarV2"],
  });
  browserChannel = requestedChannel;
  context = await browser.newContext({
    // 默认桌面 viewport;移动/平板/桌面预设由 Rust 侧按需传 viewport 覆盖。
    viewport: { width: 1280, height: 720 },
    deviceScaleFactor: 1,
  });
  page = await context.newPage();
  consoleEntries = [];
  page.on("console", (msg) => {
    const location = msg.location();
    // 行列统一 1 起算(playwright 给 0 起算),与面板后端的 CDP 条目同口径。
    pushConsole({
      type: msg.type(),
      text: msg.text(),
      url: location?.url || undefined,
      line: Number.isInteger(location?.lineNumber) ? location.lineNumber + 1 : undefined,
      column: Number.isInteger(location?.columnNumber) ? location.columnNumber + 1 : undefined,
    });
  });
  page.on("pageerror", (err) => {
    pushConsole({ type: "pageerror", text: String(err?.message ?? err) });
  });
}

function requirePage() {
  if (!browser || !page) {
    throw new Error("no browser: call open first");
  }
}

async function applyViewport(viewport) {
  if (viewport) {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
  }
}

function clampWait(ms, fallback) {
  const value = Number.isFinite(ms) ? ms : fallback;
  return Math.max(0, Math.min(MAX_WAIT_MS, value));
}

async function handle(method, params) {
  switch (method) {
    case "shutdown": {
      if (browser) {
        await browser.close().catch(() => {});
        resetState();
      }
      return { ok: true };
    }
    case "open": {
      const { url, viewport } = params ?? {};
      await ensureBrowser(params?.channel);
      await applyViewport(viewport);
      // console 查询只应覆盖本次导航之后的页面。否则先前 URL 的错误会污染
      // 后续页面，模型会把旧页故障归因到当前前端。
      consoleEntries = [];
      await page.goto(url, { waitUntil: "domcontentloaded", timeout: 15000 });
      return { title: await page.title().catch(() => ""), url: page.url() };
    }
    case "emulateMedia": {
      requirePage();
      const scheme = params?.colorScheme;
      await page.emulateMedia({ colorScheme: scheme === "light" || scheme === "dark" ? scheme : null });
      return { ok: true };
    }
    case "screenshot": {
      requirePage();
      await applyViewport(params?.viewport);
      let png;
      if (params?.selector) {
        png = await page.locator(params.selector).first().screenshot({ type: "png", timeout: 5000 });
      } else {
        png = await page.screenshot({ type: "png", fullPage: Boolean(params?.fullPage) });
      }
      return { png: png.toString("base64"), url: page.url(), title: await page.title().catch(() => "") };
    }
    case "dom": {
      requirePage();
      const structure = await page.evaluate(domWalker, params?.selector ?? null);
      return { dom: structure, url: page.url() };
    }
    case "console": {
      requirePage();
      const entries = params?.all
        ? consoleEntries.slice(-CONSOLE_ALL_LIMIT)
        : consoleEntries.filter((e) => e.type === "error" || e.type === "warning" || e.type === "pageerror");
      return { errors: entries, url: page.url() };
    }
    case "click": {
      requirePage();
      const { selector } = params ?? {};
      if (!selector) return { error: "click requires selector" };
      await page.click(selector, { timeout: 5000 });
      return { ok: true, url: page.url() };
    }
    case "type": {
      requirePage();
      const { selector, text } = params ?? {};
      if (!selector || typeof text !== "string") return { error: "type requires selector and text" };
      await page.fill(selector, text);
      return { ok: true, url: page.url() };
    }
    case "press": {
      requirePage();
      const { key } = params ?? {};
      if (!key) return { error: "press requires key" };
      await page.keyboard.press(key);
      return { ok: true, url: page.url() };
    }
    case "scroll": {
      requirePage();
      const { selector, dy } = params ?? {};
      if (selector) {
        await page.locator(selector).first().scrollIntoViewIfNeeded({ timeout: 5000 });
      } else {
        await page.evaluate((delta) => globalThis.scrollBy(0, delta), Number.isFinite(dy) ? dy : 600);
      }
      const scrollY = await page.evaluate(() => globalThis.scrollY);
      return { ok: true, url: page.url(), scrollY };
    }
    case "wait": {
      requirePage();
      const { selector, text, ms } = params ?? {};
      const started = Date.now();
      if (selector) {
        await page.waitForSelector(selector, { timeout: clampWait(ms, MAX_WAIT_MS), state: "attached" });
      } else if (typeof text === "string" && text) {
        await page.waitForFunction(
          (needle) => Boolean(document.body) && document.body.innerText.includes(needle),
          text,
          { timeout: clampWait(ms, MAX_WAIT_MS), polling: 100 },
        );
      } else {
        await page.waitForTimeout(clampWait(ms, 1000));
      }
      return { ok: true, url: page.url(), elapsedMs: Date.now() - started };
    }
    case "eval": {
      requirePage();
      const { expression } = params ?? {};
      if (typeof expression !== "string" || !expression.trim()) return { error: "eval requires expression" };
      const value = await page.evaluate(expression);
      return { json: JSON.stringify(value === undefined ? null : value) ?? "null", url: page.url() };
    }
    default:
      return { error: `unknown method: ${method}` };
  }
}

// ---- stdio 循环:逐行读 JSON,响应逐行写。请求**串行**处理(单 browser/page,
// 并发 RPC 会让后到的截图在 open 完成前执行)——用 promise 链把处理排成队列。
let queue = Promise.resolve();
let stdinEnded = false;
let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  input += chunk;
  let idx;
  while ((idx = input.indexOf("\n")) >= 0) {
    const line = input.slice(0, idx).trim();
    input = input.slice(idx + 1);
    if (!line) continue;
    let req;
    try {
      req = JSON.parse(line);
    } catch {
      process.stdout.write(JSON.stringify({ id: null, error: "invalid JSON" }) + "\n");
      continue;
    }
    // 排进队列:前一个请求完成(含 browser launch)后才处理当前请求。
    queue = queue.then(async () => {
      let result;
      try {
        result = await handle(req.method, req.params ?? {});
      } catch (err) {
        result = { error: String(err?.message ?? err) };
      }
      process.stdout.write(JSON.stringify({ id: req.id, result }) + "\n");
    });
  }
});
function maybeExit() {
  if (stdinEnded) {
    queue = queue.then(async () => {
      try {
        if (browser) await browser.close().catch(() => {});
      } finally {
        process.exit(0);
      }
    });
  }
}
process.stdin.on("end", () => {
  stdinEnded = true;
  maybeExit();
});
