#!/usr/bin/env node
// R-272 UI 连通性与跳转评估:可达性/孤岛视图/死链自动巡检。
//
// 数据源(权威):
//   - 桌面端: crates/kanzei-app/ui/index.html——侧栏导航 `button.activity-item[data-view=X]`
//     与目标容器 `#view-X` 的对应关系是可达性的真源(03-shell.js 按 data-view 切 active)。
//   - 移动 PWA: crates/kanzei-app/mobile-pwa/——配对→通知流→发消息→approval 关键路径。
//
// 判定:
//   - 死链 = 有入口按钮(data-view=X)但目标容器(#view-X)不存在/不激活;
//   - 孤岛 = 有容器(#view-X)但没有任何入口按钮指向它(注册了但无入口可达)。
//   - 关键路径 = 按配置文件逐条断言「跳转后目标视图标识存在」(桌面端 #view-* active,
//     PWA 按路径配置断言 DOM 存在)。
//
// 用法: node scripts/ui-connectivity.mjs [--pwa] [--json] [--serve] [--html <path>]
//   --pwa   只巡检移动 PWA
//   --html <path>  对指定 HTML 文件做桌面端可达性扫描(验收①反证用;缺省扫 ui/index.html)
//   --json  输出机器可读报告(可达图 + 失败清单)
//   --serve 自动起一个临时静态服务供 PWA 巡检(测完即停)
// 退出码:0 = 全部通过;1 = 存在死链/孤岛/关键路径失败。

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import http from "node:http";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const UI_HTML = path.join(repoRoot, "crates/kanzei-app/ui/index.html");
const PWA_ROOT = path.join(repoRoot, "crates/kanzei-app/mobile-pwa");

// ---- 关键路径清单:D-401 外置配置文件(scripts/key-paths.json),增删路径不改巡检代码。
const KEY_PATHS = JSON.parse(
  fs.readFileSync(path.join(__dirname, "key-paths.json"), "utf8")
);

// ---- 桌面端静态巡检:扫描 index.html 的 data-view ↔ #view-* 对应关系 ----
function scanDesktop(html) {
  const buttons = [...html.matchAll(/data-view="([^"]+)"/g)].map((m) => m[1]);
  const containers = [...html.matchAll(/id="(view-[^"]+)"/g)].map((m) => m[1]);
  const containerSet = new Set(containers);
  const buttonSet = new Set(buttons);

  const deadLinks = []; // 有入口按钮但目标容器缺失
  const islands = []; // 有容器但无入口按钮
  for (const view of buttonSet) {
    if (!containerSet.has(`view-${view}`)) {
      deadLinks.push(view);
    }
  }
  for (const cid of containerSet) {
    const view = cid.replace(/^view-/, "");
    if (!buttonSet.has(view)) {
      islands.push(view);
    }
  }
  return { buttons: [...buttonSet], containers: [...containerSet], deadLinks, islands };
}

// ---- 关键路径断言(桌面端:静态扫描已覆盖;这里校验配置里声明的视图都存在)----
function checkKeyPaths(scan, report) {
  const failures = [];
  for (const p of KEY_PATHS.desktop) {
    if (!scan.containers.includes(`view-${p.view}`)) {
      failures.push(`关键路径 ${p.name}: 目标视图 #view-${p.view} 缺失`);
    }
    if (!scan.buttons.includes(p.view)) {
      failures.push(`关键路径 ${p.name}: 入口按钮 [data-view=${p.view}] 缺失`);
    }
  }
  report.keyPathFailures = failures;
  return failures;
}

// ---- PWA 巡检:起临时静态服务,用 browser-helper 打开并断言 DOM ----
async function checkPwa(report, serve) {
  let server = null;
  let port = 0;
  if (serve) {
    server = http.createServer((req, res) => {
      const f = req.url === "/" ? "index.html" : req.url.slice(1);
      const fp = path.join(PWA_ROOT, f);
      fs.readFile(fp, (err, data) => {
        if (err) {
          res.writeHead(404);
          res.end("not found");
          return;
        }
        const types = { ".html": "text/html", ".js": "application/javascript", ".css": "text/css", ".json": "application/json", ".png": "image/png" };
        res.writeHead(200, { "Content-Type": types[path.extname(fp)] || "text/plain" });
        res.end(data);
      });
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    port = server.address().port;
  }
  const url = serve ? `http://127.0.0.1:${port}/` : `file:///${PWA_ROOT.replace(/\\/g, "/")}/index.html`;

  // 用 browser-helper 打开页面读 DOM(与 R-269 同通道)。
  const { spawn } = await import("node:child_process");
  const helper = path.join(__dirname, "browser-helper.mjs");
  const rpc = (reqs) =>
    new Promise((resolve, reject) => {
      const child = spawn(process.execPath, [helper], { stdio: ["pipe", "pipe", "inherit"] });
      let out = "";
      child.stdout.on("data", (d) => (out += d.toString()));
      child.on("close", (code) => (code === 0 ? resolve(out) : reject(new Error(`helper exit ${code}`))));
      // 逐行写 JSON(helper 协议:每行一个请求,id 配对响应)。
      for (const req of reqs) {
        child.stdin.write(JSON.stringify(req) + "\n");
      }
      child.stdin.end();
    });

  try {
    const openReq = { id: 1, method: "open", params: { url, channel: "msedge", viewport: { width: 375, height: 667 } } };
    const domReq = { id: 2, method: "dom", params: { selector: "body" } };
    const shutdownReq = { id: 3, method: "shutdown", params: {} };
    const raw = await rpc([openReq, domReq, shutdownReq]);    const lines = raw.trim().split("\n").filter(Boolean);
    const responses = lines.map((l) => JSON.parse(l));
    const domResp = responses.find((r) => r.id === 2);
    if (!domResp || domResp.result?.error) {
      report.pwaFailures = [`PWA 页面无法打开: ${domResp?.result?.error ?? "无 dom 响应"}`];
      return;
    }
    const domJson = JSON.parse(domResp.result.dom);
    const text = JSON.stringify(domJson);
    const failures = [];
    for (const p of KEY_PATHS.pwa) {
      if (p.needs_pair) {
        // 未配对态不要求已配对视图存在(真机配对后才有)——只记录可达性信息。
        report.pwaUnpaired = report.pwaUnpaired || [];
        report.pwaUnpaired.push(`${p.name}(需配对,未配对态跳过断言)`);
        continue;
      }
      if (!text.includes(p.assert)) {
        failures.push(`PWA 关键路径 ${p.name}: 断言 ${p.assert} 未找到`);
      }
    }
    report.pwaFailures = failures;
  } finally {
    if (server) server.close();
  }
}

// ---- 主流程 ----
const args = process.argv.slice(2);
const wantPwa = args.includes("--pwa");
const json = args.includes("--json");
const serve = args.includes("--serve");
const htmlArg = args.indexOf("--html");
const customHtml = htmlArg >= 0 ? args[htmlArg + 1] : null;

const report = { desktop: null, pwa: null, keyPathFailures: null, pwaFailures: null, pwaUnpaired: null };
const started = Date.now();

// 桌面端(默认,除非 --pwa 只跑 PWA;--html 指定反证文件)。
if (!wantPwa) {
  const htmlPath = customHtml ? path.resolve(repoRoot, customHtml) : UI_HTML;
  const html = fs.readFileSync(htmlPath, "utf8");
  const scan = scanDesktop(html);
  report.desktop = { deadLinks: scan.deadLinks, islands: scan.islands, buttons: scan.buttons.length, containers: scan.containers.length };
  const keyFailures = checkKeyPaths(scan, report);
  const hasDesktopIssues = scan.deadLinks.length > 0 || scan.islands.length > 0 || keyFailures.length > 0;
  if (!json) {
    console.log(`[ui-connectivity] 桌面端(${htmlPath}): ${scan.buttons.length} 个导航入口, ${scan.containers.length} 个视图容器`);
    if (scan.deadLinks.length) console.log(`  ✗ 死链(有入口无目标): ${scan.deadLinks.join(", ")}`);
    if (scan.islands.length) console.log(`  ✗ 孤岛视图(无入口可达): ${scan.islands.join(", ")}`);
    if (keyFailures.length) keyFailures.forEach((f) => console.log(`  ✗ ${f}`));
    if (!scan.deadLinks.length && !scan.islands.length && !keyFailures.length) console.log("  ✓ 桌面端可达性检查通过");
  }
  if (hasDesktopIssues && !wantPwa) {
    if (json) console.log(JSON.stringify({ ...report, durationMs: Date.now() - started }, null, 2));
    process.exit(1);
  }
}

// PWA(默认一起跑;--pwa 只跑 PWA)。
await checkPwa(report, serve);
if (!json) {
  if (report.pwaFailures?.length) report.pwaFailures.forEach((f) => console.log(`[ui-connectivity] ✗ PWA: ${f}`));
  else console.log(`[ui-connectivity] PWA: 配对页可达,关键路径断言通过`);
  if (report.pwaUnpaired?.length) report.pwaUnpaired.forEach((m) => console.log(`[ui-connectivity] · ${m}`));
}
if (json) {
  console.log(JSON.stringify({ ...report, durationMs: Date.now() - started }, null, 2));
}
if (report.pwaFailures?.length) {
  process.exitCode = 1;
}
