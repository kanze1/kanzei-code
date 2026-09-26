#!/usr/bin/env node
// 桌面端 UI 的浏览器预览服务:不启动 Tauri,在普通浏览器里用模拟 IPC 跑真实前端。
//
// 用法:
//   node scripts/ui-preview/server.mjs [--port 5178] [--host 127.0.0.1]
//   然后打开 http://127.0.0.1:5178/?theme=dark&scene=chat
//
// URL 参数(由 mock-ipc.js / scenes.mjs 解释):
//   theme=dark|light                 主题(默认 dark)
//   scene=chat|agents|parallel|settings|docs|overlays|lines|empty   打开哪个视图/弹窗(默认 chat)
//   dialog=ask|question|confirm|input|viewer|palette        overlays 场景里显示哪个弹窗(默认 ask)
//   lang=zh|en                       界面语言(默认 zh)
//   anchor=<CSS 选择器>              场景就绪后把该元素滚进视口
//   keep=1                           保留上次的 localStorage(默认每次加载清空 kz* 键,保证可复现)
//
// 页面内 API:window.__kzPreview(emit / calls / unknown / setCommand / ready),见 mock-ipc.js 头注释。
//
// 服务只做两件事:
//   1. 原样托管 crates/kanzei-app/ui(禁止目录穿越);
//   2. 给 index.html 注入 `<script src="/__preview/mock-ipc.js">`(经典脚本,先于全部 ESM 模块执行),
//      并在 /__preview/ 下托管本目录的 mock-ipc.js / fixtures.mjs / scenes.mjs。
// 不改动 ui/ 下任何文件,不引入 npm 依赖。
import http from "node:http";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const UI_ROOT = path.resolve(HERE, "../../crates/kanzei-app/ui");
export const PREVIEW_ROOT = HERE;
const PREVIEW_FILES = new Set(["mock-ipc.js", "fixtures.mjs", "scenes.mjs", "memory-graph-fixture.mjs"]);
const MOCK_TAG = '<script src="/__preview/mock-ipc.js"></script>';

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".webp": "image/webp",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".ttf": "font/ttf",
  ".wasm": "application/wasm",
  ".map": "application/json; charset=utf-8",
  ".wav": "audio/wav",
  ".mp3": "audio/mpeg",
  ".mp4": "video/mp4",
  ".webm": "video/webm",
  ".txt": "text/plain; charset=utf-8",
};

/// 把模拟 IPC 注入 index.html:放在 <head> 最前面,保证它在任何 module 脚本求值前装好 window.__TAURI__。
export function injectMock(html) {
  if (html.includes(MOCK_TAG)) return html;
  const head = /<head[^>]*>/i.exec(html);
  if (!head) return `${MOCK_TAG}\n${html}`;
  const at = head.index + head[0].length;
  return `${html.slice(0, at)}\n  ${MOCK_TAG}${html.slice(at)}`;
}

function send(res, status, body, type = "text/plain; charset=utf-8") {
  res.writeHead(status, { "Content-Type": type, "Cache-Control": "no-store" });
  res.end(body);
}

async function handle(req, res) {
  const url = new URL(req.url, "http://127.0.0.1");
  let pathname;
  try {
    pathname = decodeURIComponent(url.pathname);
  } catch {
    return send(res, 400, "bad path");
  }
  if (pathname === "/favicon.ico") return send(res, 204, "");
  if (pathname === "/__preview/health") {
    return send(res, 200, JSON.stringify({ ok: true, ui: UI_ROOT }), TYPES[".json"]);
  }
  if (pathname.startsWith("/__preview/")) {
    const name = pathname.slice("/__preview/".length);
    if (!PREVIEW_FILES.has(name)) return send(res, 404, "not found");
    const body = await readFile(path.join(PREVIEW_ROOT, name));
    return send(res, 200, body, TYPES[path.extname(name)]);
  }
  if (pathname === "/" || pathname === "/index.html") {
    const html = await readFile(path.join(UI_ROOT, "index.html"), "utf8");
    return send(res, 200, injectMock(html), TYPES[".html"]);
  }
  const file = path.resolve(UI_ROOT, `.${pathname}`);
  const relative = path.relative(UI_ROOT, file);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) return send(res, 403, "forbidden");
  try {
    const body = await readFile(file);
    return send(res, 200, body, TYPES[path.extname(file).toLowerCase()] || "application/octet-stream");
  } catch {
    return send(res, 404, "not found");
  }
}

/// 起服务。port=0 取随机端口(截图脚本用,避免与手动开的 5178 冲突)。
export function startPreviewServer({ port = 5178, host = "127.0.0.1" } = {}) {
  const server = http.createServer((req, res) => {
    handle(req, res).catch((error) => send(res, 500, String(error?.stack || error)));
  });
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, host, () => {
      const address = server.address();
      const origin = `http://${host}:${address.port}`;
      resolve({
        server,
        origin,
        close: () => new Promise((done) => server.close(() => done())),
      });
    });
  });
}

function argValue(flag, fallback) {
  const index = process.argv.indexOf(flag);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

const invokedDirectly = process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;
if (invokedDirectly) {
  const port = Number(argValue("--port", process.env.KZ_PREVIEW_PORT || "5178"));
  const host = argValue("--host", "127.0.0.1");
  const { origin } = await startPreviewServer({ port, host });
  console.log(`kanzei UI 预览: ${origin}/?theme=dark&scene=chat`);
  console.log("场景: chat | agents | settings | docs | overlays(&dialog=ask|question|confirm|input|viewer|palette) | lines | empty");
  console.log("Ctrl+C 退出");
}
