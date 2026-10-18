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
import { randomUUID } from "node:crypto";
import { isIP } from "node:net";
import { domWalker } from "./browser-dom-walker.mjs";

// The public custom transport keeps a CDP handshake pinned to its authorized URL.
// Playwright's URL overload follows WebSocket redirects to other origins.
function localCdpTransport(url) {
  if (typeof WebSocket !== "function") throw new Error("Connecting an existing browser requires Node.js 22 or newer");
  const socket = new WebSocket(url);
  let closed = false;
  const pending = [];
  const finish = reason => { if (!closed) { closed = true; transport.onclose?.(reason); } };
  const transport = {
    send(message) {
      if (closed) throw new Error("CDP connection is closed");
      const payload = JSON.stringify(message);
      if (socket?.readyState === WebSocket.OPEN) socket.send(payload);
      else pending.push(payload);
    },
    close() { socket?.close(); },
  };
  socket.addEventListener("open", () => { for (const message of pending.splice(0)) socket.send(message); });
  socket.addEventListener("message", event => {
    let message;
    try { message = JSON.parse(event.data); }
    catch { finish("Invalid CDP JSON frame"); socket.close(); return; }
    if (!message || typeof message !== "object" || Array.isArray(message)) { finish("Invalid CDP protocol object"); socket.close(); return; }
    transport.onmessage?.(message);
  });
  socket.addEventListener("error", () => finish("CDP WebSocket handshake failed; redirects are not followed"));
  socket.addEventListener("close", () => finish("CDP connection closed"));
  return transport;
}

async function discoveryDocument(response) {
  const limit = 64 * 1024, chunks = []; let bytes = 0;
  for await (const chunk of response.body) {
    bytes += chunk.byteLength;
    if (bytes > limit) throw new Error("CDP discovery response exceeds 64 KiB");
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

function createSession(attached = false) {
// One isolated browser/context/page per session owner.
let browser = null;
let browserChannel = null;
let context = null;
let page = null, activeFrame = null;
const pageConsole = new WeakMap(), observedPages = new WeakSet();
let chromiumCache = null;
const pageIds = new WeakMap(), frameIds = new WeakMap();
function frameId(frame) { if (!frameIds.has(frame)) frameIds.set(frame, randomUUID()); return frameIds.get(frame); }
function scope() { return attached ? activeFrame : page; }
function currentUrl() { return scope().url(); }
function pageId(target) { if (!pageIds.has(target)) pageIds.set(target, randomUUID()); return pageIds.get(target); }
function allowedPage(url) {
  try { const parsed = new URL(url); return ["http:", "https:"].includes(parsed.protocol) && !parsed.username && !parsed.password && parsed.hostname.replace(/\.$/, "") !== "tauri.localhost"; }
  catch { return false; }
}
function pages() { return browser?.contexts().flatMap(context => context.pages()).filter(target => !target.isClosed() && allowedPage(target.url())) ?? []; }
async function pageList() { return Promise.all(pages().map(async target => ({ page_id: pageId(target), url: target.url(), title: await target.title() }))); }
function choosePage(params) {
  page = pages().find(target => pageId(target) === params.page_id);
  if (!page) throw new Error("PAGE_UNAVAILABLE: refresh pages and explicitly select an authorized page");
  if (page.url() !== params.expected_url) throw new Error("PAGE_CHANGED: the authorized URL no longer matches; refresh pages before acting");
  wirePage(page);
  activeFrame = page.mainFrame();
  if (params.frame_id) {
    activeFrame = page.frames().find(frame => frameId(frame) === params.frame_id && allowedPage(frame.url()));
    if (!activeFrame) throw new Error("FRAME_UNAVAILABLE: refresh frames and explicitly select a current frame");
    if (activeFrame.url() !== params.expected_frame_url) throw new Error("FRAME_CHANGED: frame URL no longer matches its authorization");
  }
}

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

function entriesFor(target) {
  if (!pageConsole.has(target)) pageConsole.set(target, []);
  return pageConsole.get(target);
}
function pushConsole(target, entry) {
  const entries = entriesFor(target); entries.push(entry);
  if (entries.length > CONSOLE_CAP) entries.splice(0, entries.length - CONSOLE_CAP);
}
function wirePage(target) {
  if (observedPages.has(target)) return;
  observedPages.add(target);
  target.on("console", message => {
    const location = message.location();
    pushConsole(target, { type: message.type(), text: message.text(), url: location?.url || undefined,
      line: Number.isInteger(location?.lineNumber) ? location.lineNumber + 1 : undefined,
      column: Number.isInteger(location?.columnNumber) ? location.columnNumber + 1 : undefined });
  });
  target.on("pageerror", error => pushConsole(target, { type: "pageerror", text: String(error?.message ?? error) }));
  target.on("framenavigated", frame => { if (frame === target.mainFrame()) pageConsole.set(target, []); });
}

async function ensureBrowser(channel) {
  if (attached) { requirePage(); return; }
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
  wirePage(page);
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
  if (attached && !["connect", "pages", "shutdown"].includes(method)) choosePage(params);
  switch (method) {
    case "connect": {
      if (!attached || browser) throw new Error("A connection must be created in a fresh connection session");
      const endpoint = new URL(params.endpoint);
      const local = host => host === "localhost" || host === "[::1]" || (isIP(host) === 4 && host.startsWith("127."));
      if (endpoint.protocol !== "http:" || !local(endpoint.hostname) || endpoint.username || endpoint.password || endpoint.pathname !== "/" || endpoint.search || endpoint.hash) throw new Error("Only a local CDP HTTP origin is allowed");
      const discovery = await fetch(new URL("json/version", endpoint), { redirect: "error", signal: AbortSignal.timeout(10000) });
      if (!discovery.ok) throw new Error(`CDP discovery HTTP ${discovery.status}`);
      const websocket = new URL((await discoveryDocument(discovery)).webSocketDebuggerUrl);
      if (websocket.protocol !== "ws:" || websocket.hostname !== endpoint.hostname || websocket.port !== endpoint.port || websocket.username || websocket.password || websocket.search || websocket.hash || !websocket.pathname.startsWith("/devtools/browser/")) throw new Error("CDP discovery returned a different or privileged connection endpoint");
      const chromium = await loadChromium();
      browser = await chromium.connectOverCDP(localCdpTransport(websocket.href), { noDefaults: true, timeout: 15000 });
      browserChannel = "attached";
      return { pages: await pageList() };
    }
    case "pages": {
      if (!attached || !browser?.isConnected()) throw new Error("CONNECTION_UNAVAILABLE: explicitly connect again");
      return { pages: await pageList() };
    }
    case "frames": {
      requirePage();
      return { pages: await pageList(), frames: page.frames().filter(frame => allowedPage(frame.url())).map(frame => ({ frame_id: frameId(frame), url: frame.url(), name: frame.name(), main: frame === page.mainFrame() })) };
    }
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
      pageConsole.set(page, []);
      await scope().goto(url, { waitUntil: "domcontentloaded", timeout: 15000 });
      return { title: await scope().title().catch(() => ""), url: currentUrl() };
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
        png = await scope().locator(params.selector).screenshot({ type: "png", timeout: 5000 });
      } else if (params?.frame_id) {
        if (params.fullPage) throw new Error("fullPage is not supported for an iframe screenshot");
        png = await (await activeFrame.frameElement()).screenshot({ type: "png", timeout: 5000 });
      } else {
        png = await page.screenshot({ type: "png", fullPage: Boolean(params?.fullPage) });
      }
      return { png: png.toString("base64"), url: currentUrl(), title: await scope().title().catch(() => "") };
    }
    case "dom": {
      requirePage();
      const structure = await scope().evaluate(domWalker, params?.selector ?? null);
      return { dom: structure, url: currentUrl() };
    }
    case "console": {
      requirePage();
      if (params?.frame_id) throw new Error("Console observations are page-scoped; omit frame_id to inspect the selected page");
      const entries = params?.all
        ? entriesFor(page).slice(-CONSOLE_ALL_LIMIT)
        : entriesFor(page).filter((e) => e.type === "error" || e.type === "warning" || e.type === "pageerror");
      return { errors: entries, url: currentUrl() };
    }
    case "click": {
      requirePage();
      const { selector } = params ?? {};
      if (!selector) return { error: "click requires selector" };
      const element = await scope().locator(selector).elementHandle({ timeout: 5000 });
      if (!element) throw new Error("No matching click target");
      try {
        if (attached && (page.url() !== params.expected_url || activeFrame.url() !== (params.expected_frame_url ?? params.expected_url))) throw new Error("PAGE_CHANGED before click");
        await element.click({ timeout: 5000 });
      } finally { await element.dispose(); }
      return { ok: true, url: currentUrl() };
    }
    case "press": {
      requirePage();
      const { key } = params ?? {};
      if (!key) return { error: "press requires key" };
      if (params?.frame_id) {
        const focused = await activeFrame.evaluateHandle("document.activeElement");
        try { const element = focused.asElement(); if (!element) throw new Error("The frame has no focused element"); await element.press(key); }
        finally { await focused.dispose(); }
      } else await page.keyboard.press(key);
      return { ok: true, url: currentUrl() };
    }
    case "scroll": {
      requirePage();
      const { selector, dy } = params ?? {};
      if (selector) {
        await scope().locator(selector).scrollIntoViewIfNeeded({ timeout: 5000 });
      } else {
        await scope().evaluate((delta) => globalThis.scrollBy(0, delta), Number.isFinite(dy) ? dy : 600);
      }
      const scrollY = await scope().evaluate(() => globalThis.scrollY);
      return { ok: true, url: currentUrl(), scrollY };
    }
    case "wait": {
      requirePage();
      const { selector, text, ms } = params ?? {};
      const started = Date.now();
      if (selector) {
        await scope().waitForSelector(selector, { timeout: clampWait(ms, MAX_WAIT_MS), state: "attached" });
      } else if (typeof text === "string" && text) {
        await scope().waitForFunction(
          (needle) => Boolean(document.body) && document.body.innerText.includes(needle),
          text,
          { timeout: clampWait(ms, MAX_WAIT_MS), polling: 100 },
        );
      } else {
        await page.waitForTimeout(clampWait(ms, 1000));
      }
      return { ok: true, url: currentUrl(), elapsedMs: Date.now() - started };
    }
    case "eval": {
      requirePage();
      const { expression } = params ?? {};
      if (typeof expression !== "string" || !expression.trim()) return { error: "eval requires expression" };
      const guarded = attached ? `if (location.href !== ${JSON.stringify(params.expected_frame_url ?? params.expected_url)}) throw new Error("PAGE_CHANGED: document changed before evaluation");\n${expression}` : expression;
      const value = await scope().evaluate(guarded);
      if (attached && (page.url() !== params.expected_url || activeFrame.url() !== (params.expected_frame_url ?? params.expected_url))) throw new Error("PAGE_CHANGED: document changed during evaluation; result discarded");
      return { json: JSON.stringify(value === undefined ? null : value) ?? "null", url: currentUrl() };
    }
    default:
      return { error: `unknown method: ${method}` };
  }
}

  return handle;
}

const sessions = new Map();
const SESSION_IDLE_MS = 120000;
async function handle(method, params) {
  if (method === "shutdown") {
    for (const session of sessions.values()) await session.handle("shutdown", {});
    sessions.clear();
    return { ok: true };
  }
  if (typeof params?.owner !== "string" || !params.owner) {
    throw new Error("browser session owner is required");
  }
  const attached = method === "connect" || typeof params.connection_id === "string";
  const connectionId = method === "connect" ? randomUUID() : params.connection_id;
  const key = JSON.stringify([params.owner, attached ? connectionId : null]);
  const now = Date.now();
  for (const [owner, session] of sessions) {
    if (now - session.lastUsed > SESSION_IDLE_MS) {
      await session.handle("shutdown", {});
      sessions.delete(owner);
    }
  }
  let session = sessions.get(key);
  if (!session) {
    if (attached && method !== "connect") throw new Error("CONNECTION_UNAVAILABLE: connection expired or belongs to another conversation");
    session = { handle: createSession(attached), lastUsed: now, endpoint: params.endpoint };
    sessions.set(key, session);
  }
  try {
    if (method === "disconnect") {
      await session.handle("shutdown", {}); sessions.delete(key);
      return { disconnected: true, connection_id: connectionId };
    }
    const result = await session.handle(method, params);
    return attached ? { ...result, connection_id: connectionId, endpoint: session.endpoint } : result;
  } finally {
    session.lastUsed = Date.now();
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
        await handle("shutdown", {});
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
