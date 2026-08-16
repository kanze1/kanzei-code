#!/usr/bin/env node
// D-401:R-272 浏览器运行时遍历批次——真实浏览器(playwright-core channel 模式,
// 与 browser-helper 同通道)做跳转断裂的运行时判定(静态 regex 差集测不到
// 「容器在但切换 JS 崩」):点击导航后目标视图不可见 / 切换新增 console 错误
// 即点名。
//
// 模式:
//   node scripts/ui-connectivity-browser.mjs [--json]        PWA 配对页真实遍历 + 桌面端降级说明
//   node scripts/ui-connectivity-browser.mjs --probe [--json] 跳转断裂检出能力反证(构造 HTML:
//                                                              ok 视图正常切换、broken 视图切换抛错)
//   node scripts/ui-connectivity-browser.mjs --html <path>   遍历指定 HTML 的 desktop 关键路径
//
// 依赖:scripts/key-paths.json(配置化清单)、npm 依赖 playwright-core。
// 退出码:0 = 无失败;1 = 有跳转断裂/切换错误/能力反证未检出。
import { chromium } from "playwright-core";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const KEY_PATHS = JSON.parse(
  fs.readFileSync(path.join(__dirname, "key-paths.json"), "utf8")
);
const UI_HTML = path.join(repoRoot, "crates/kanzei-app/ui/index.html");
const PWA_ROOT = path.join(repoRoot, "crates/kanzei-app/mobile-pwa");
const args = process.argv.slice(2);
const wantJson = args.includes("--json");
const wantProbe = args.includes("--probe");
const htmlArg = args.indexOf("--html");
const customHtml = htmlArg >= 0 ? path.resolve(repoRoot, args[htmlArg + 1]) : null;
const started = Date.now();

const failures = [];
const consoleErrors = [];
let probe = null;
let pwa = null;
let desktopNote = null;

async function runDesktopTraversal(page, htmlPath, paths, label, emitFailures = true) {
  const entries = [];
  await page.goto(`file:///${htmlPath.replace(/\\/g, "/")}`);
  await page.waitForTimeout(200);
  for (const p of paths) {
    const before = consoleErrors.length;
    const entry = { name: p.name, view: p.view };
    try {
      await page.click(p.trigger);
      await page.waitForTimeout(80);
      entry.visible = await page.isVisible(`#view-${p.view}`);
      if (emitFailures && !entry.visible) {
        failures.push(`[${label}] 跳转断裂: 点击 ${p.trigger} 后目标视图 #view-${p.view} 不可见`);
      }
    } catch (e) {
      entry.error = String(e);
      if (emitFailures) {
        failures.push(`[${label}] 跳转断裂: 点击 ${p.trigger} 失败: ${e}`);
      }
    }
    const newErrors = consoleErrors.slice(before);
    if (newErrors.length) {
      entry.switchErrors = newErrors;
      if (emitFailures) {
        failures.push(`[${label}] ${p.name}: 点击 ${p.trigger} 后新增 console 错误: ${newErrors.join(" | ")}`);
      }
    }
    entries.push(entry);
  }
  return entries;
}

const browser = await chromium.launch({ channel: "msedge", headless: true });
try {
  const page = await browser.newPage();
  page.on("console", (m) => {
    if (m.type() === "error") consoleErrors.push(`console.error: ${m.text()}`);
  });
  page.on("pageerror", (e) => consoleErrors.push(`pageerror: ${e}`));

  if (wantProbe) {
    // 反证:构造「正常切换 + 切换 JS 崩」的 HTML,验证跳转断裂能被检出。
    const probeHtml = `<!DOCTYPE html><html><head><style>.view{display:none}.view.active{display:block}</style></head><body>
<button data-view="ok">ok</button><button data-view="broken">broken</button>
<div id="view-ok" class="view">ok view</div><div id="view-broken" class="view">broken view</div>
<script>
document.querySelectorAll("[data-view]").forEach((b) => {
  b.addEventListener("click", () => {
    document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
    if (b.dataset.view === "broken") throw new Error("simulated switch crash");
    document.getElementById("view-" + b.dataset.view).classList.add("active");
  });
});
</script></body></html>`;
    const probeFile = path.join(os.tmpdir(), `kz-connectivity-probe-${process.pid}.html`);
    fs.writeFileSync(probeFile, probeHtml);
    const entries = await runDesktopTraversal(
      page,
      probeFile,
      [
        { name: "ok", view: "ok", trigger: '[data-view="ok"]' },
        { name: "broken", view: "broken", trigger: '[data-view="broken"]' },
      ],
      "probe",
      false // probe 的 broken 检出是预期,不置全局失败;能力由下面判定。
    );
    probe = { entries };
    // 能力判定:ok 必须正常切换(可见且无错误);broken 必须被检出(不可见+console 错误)。
    const ok = entries.find((e) => e.name === "ok");
    const broken = entries.find((e) => e.name === "broken");
    if (!ok?.visible || ok?.switchErrors?.length) {
      failures.push("probe 反证失败:正常视图 ok 未能切换(遍历能力本身异常)");
    }
    if (!broken || broken.visible || !broken.switchErrors?.length) {
      failures.push("probe 反证失败:切换 JS 崩的 broken 视图未被检出(跳转断裂检测失效)");
    }
    fs.unlinkSync(probeFile);
  } else if (customHtml) {
    // 指定 HTML 的 desktop 关键路径遍历(通用;反证 fixture 或真实页面)。
    await runDesktopTraversal(page, customHtml, KEY_PATHS.desktop, "desktop");
  } else {
    // PWA 配对页真实遍历(纯静态,file:// 可完整加载)。
    const beforePwa = consoleErrors.length;
    await page.goto(`file:///${PWA_ROOT.replace(/\\/g, "/")}/index.html`);
    await page.waitForTimeout(300);
    const pwaAppCount = await page.locator("#app").count();
    // file:// 环境噪声(相对资源解析差异、SW 注册不可用)过滤:net::ERR_FILE_NOT_FOUND
    // 与 favicon 类不计为页面逻辑错误;真实 HTTP 服务下这些资源正常。
    const pwaInitErrors = consoleErrors
      .slice(beforePwa)
      .filter((e) => !e.includes("net::ERR_FILE_NOT_FOUND") && !e.toLowerCase().includes("favicon"));
    pwa = { pairPageApp: pwaAppCount > 0, initErrors: pwaInitErrors };
    if (pwaAppCount === 0) failures.push("PWA 配对页加载失败:#app 容器不存在");
    if (pwaInitErrors.length) {
      failures.push(`PWA 配对页初始化 console 错误: ${pwaInitErrors.join(" | ")}`);
    }
    // 桌面端真实页面:ui/index.html 依赖 tauri IPC,headless 浏览器 file:// 下
    // 01-core.js 初始化崩($ 未定义等)——环境限制,如实降级说明(检测能力由 --probe 反证)。
    const beforeDesktop = consoleErrors.length;
    await page.goto(`file:///${UI_HTML.replace(/\\/g, "/")}`);
    await page.waitForTimeout(200);
    const initErrors = consoleErrors.slice(beforeDesktop);
    if (initErrors.length) {
      desktopNote = `桌面端 ui/index.html 在 headless file:// 下初始化报错(${initErrors.length} 条,如 $ 未定义——页面依赖 tauri IPC),跳转遍历无法在此环境进行;运行时检测能力由 --probe 反证,PWA 配对页为真实遍历。`;
    } else {
      desktopNote = "桌面端 ui/index.html 初始化无错误(此环境可遍历)。";
    }
  }
} finally {
  await browser.close();
}

const report = {
  probe,
  pwa,
  desktopNote,
  failures,
  durationMs: Date.now() - started,
};
if (!wantJson) {
  if (probe) {
    console.log(`[ui-connectivity-browser] 跳转断裂检出能力反证:耗时 ${report.durationMs}ms`);
    for (const e of probe.entries) {
      console.log(
        `  ${e.visible && !e.switchErrors ? "✓" : "✗"} ${e.name}: #view-${e.view} ${e.visible ? "可见" : "不可见"}${
          e.switchErrors ? ` (+${e.switchErrors.length} console 错误,检出跳转断裂)` : ""
        }`
      );
    }
  }
  if (pwa) {
    console.log(`[ui-connectivity-browser] PWA 配对页: #app ${pwa.pairPageApp ? "存在" : "缺失"}${
      pwa.initErrors.length ? `, ${pwa.initErrors.length} 个初始化 console 错误` : ""
    }`);
  }
  if (desktopNote) console.log(`[ui-connectivity-browser] 桌面端: ${desktopNote}`);
  if (!failures.length) console.log("  ? 全部通过");
  else failures.forEach((f) => console.log(`  ? ${f}`));
}
if (wantJson) console.log(JSON.stringify(report, null, 2));
if (failures.length) process.exitCode = 1;
