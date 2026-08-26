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

import { chromium } from "playwright-core";

// ---- 状态:单 browser / 单 page / 单 context(边界:不做多 tab/多上下文) ----
let browser = null;
let browserChannel = null;
let context = null;
let page = null;
let consoleErrors = [];
let lastError = null; // 上一次操作的错误(供诊断,不吞)

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

async function ensureBrowser(channel) {
  const requestedChannel = resolveChannel(channel ?? "msedge");
  if (browser && browser.isConnected && !browser.isConnected()) {
    browser = null;
    browserChannel = null;
    context = null;
    page = null;
  }
  // 同一辅进程允许调用方显式切换 channel。旧实现一旦先开了 Edge，后续传
  // chrome 也会静默复用 Edge，工具回显与真实执行面不一致。
  if (browser && browserChannel !== requestedChannel) {
    await browser.close().catch(() => {});
    browser = null;
    browserChannel = null;
    context = null;
    page = null;
  }
  if (browser) {
    return;
  }
  browser = await chromium.launch({
    channel: requestedChannel,
    headless: true,
    args: ["--no-first-run", "--disable-features=msEdgeSidebarV2"],
  });
  browserChannel = requestedChannel;
  context = await browser.newContext({
    // 默认桌面 viewport;移动预设由 Rust 侧按需传 viewport 覆盖。
    viewport: { width: 1280, height: 720 },
    deviceScaleFactor: 1,
  });
  page = await context.newPage();
  consoleErrors = [];
  page.on("console", (msg) => {
    if (msg.type() === "error" || msg.type() === "warning") {
      consoleErrors.push({ type: msg.type(), text: msg.text() });
    }
  });
  page.on("pageerror", (err) => {
    consoleErrors.push({ type: "pageerror", text: String(err?.message ?? err) });
  });
}

async function handle(method, params) {
  switch (method) {
    case "shutdown": {
      if (browser) {
        await browser.close().catch(() => {});
        browser = null;
        browserChannel = null;
        context = null;
        page = null;
      }
      return { ok: true };
    }
    case "open": {
      const { url, viewport } = params ?? {};
      await ensureBrowser(params?.channel);
      if (viewport) {
        await page.setViewportSize({ width: viewport.width, height: viewport.height });
      }
      // console 查询只应覆盖本次导航之后的页面。否则先前 URL 的错误会污染
      // 后续页面，模型会把旧页故障归因到当前前端。
      consoleErrors = [];
      await page.goto(url, { waitUntil: "domcontentloaded", timeout: 15000 });
      return { title: await page.title().catch(() => ""), url: page.url() };
    }
    case "screenshot": {
      const { viewport } = params ?? {};
      if (!browser) {
        return { error: "no browser: call open first" };
      }
      if (viewport) {
        await page.setViewportSize({ width: viewport.width, height: viewport.height });
      }
      const png = await page.screenshot({ type: "png", fullPage: false });
      return { png: png.toString("base64") };
    }
    case "dom": {
      const { selector } = params ?? {};
      if (!browser) {
        return { error: "no browser: call open first" };
      }
      const structure = await page.evaluate((sel) => {
        const roots = sel ? Array.from(document.querySelectorAll(sel)) : [document.body];
        const seen = new Set();
        const walk = (el, depth) => {
          if (depth > 12 || seen.has(el)) return [];
          seen.add(el);
          const node = {
            tag: el.tagName ? el.tagName.toLowerCase() : "#text",
            id: el.id || undefined,
            cls: el.className && typeof el.className === "string" ? el.className.split(/\s+/).filter(Boolean).slice(0, 5) : undefined,
            text: el.childElementCount === 0 ? (el.textContent || "").trim().slice(0, 80) : undefined,
            children: [],
          };
          for (const child of el.children) {
            node.children.push(...walk(child, depth + 1));
          }
          return [node];
        };
        const out = [];
        for (const r of roots) out.push(...walk(r, 0));
        return JSON.stringify(out);
      }, selector ?? null);
      return { dom: structure, url: page.url() };
    }
    case "console": {
      if (!browser) return { error: "no browser: call open first" };
      return { errors: consoleErrors, url: page.url() };
    }
    case "click": {
      const { selector } = params ?? {};
      if (!browser) return { error: "no browser: call open first" };
      if (!selector) return { error: "click requires selector" };
      await page.click(selector, { timeout: 5000 });
      return { ok: true, url: page.url() };
    }
    case "type": {
      const { selector, text } = params ?? {};
      if (!browser) return { error: "no browser: call open first" };
      if (!selector || typeof text !== "string") return { error: "type requires selector and text" };
      await page.fill(selector, text);
      return { ok: true, url: page.url() };
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
