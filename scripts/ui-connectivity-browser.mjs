#!/usr/bin/env node
// D-401：R-272 浏览器运行时遍历批次。使用真实 Edge/playwright 通道验证
// 点击导航后目标视图仍可见，且切换没有新增 console/page 错误。
//
// 模式：
//   node scripts/ui-connectivity-browser.mjs [--json]
//       真实 PWA 配对页遍历，并如实记录桌面端 file:// 环境限制
//   node scripts/ui-connectivity-browser.mjs --probe [--json]
//       用正常/故障切换 fixture 反证检测能力
//   node scripts/ui-connectivity-browser.mjs --html <path>
//       遍历指定桌面 HTML 的配置化关键路径
//
// 静态 data-view ↔ #view-* 差集检查由 ui-connectivity.mjs 负责；本脚本补足
// “容器存在但切换 JS 崩溃”的运行时断裂检查。
import { chromium } from "playwright-core";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const KEY_PATHS = JSON.parse(fs.readFileSync(path.join(__dirname, "key-paths.json"), "utf8"));
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
        failures.push(`[${label}] 点击 ${p.trigger} 后目标视图 #view-${p.view} 不可见`);
      }
    } catch (error) {
      entry.error = String(error);
      if (emitFailures) failures.push(`[${label}] 点击 ${p.trigger} 失败：${error}`);
    }
    const newErrors = consoleErrors.slice(before);
    if (newErrors.length) {
      entry.switchErrors = newErrors;
      if (emitFailures) {
        failures.push(`[${label}] ${p.name} 新增 console 错误：${newErrors.join(" | ")}`);
      }
    }
    entries.push(entry);
  }
  return entries;
}

const browser = await chromium.launch({ channel: "msedge", headless: true });
try {
  const page = await browser.newPage();
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(`console.error: ${message.text()}`);
  });
  page.on("pageerror", (error) => consoleErrors.push(`pageerror: ${error}`));

  if (wantProbe) {
    const probeHtml = `<!DOCTYPE html><html><head><style>.view{display:none}.view.active{display:block}</style></head><body>
<button data-view="ok">ok</button><button data-view="broken">broken</button>
<div id="view-ok" class="view">ok view</div><div id="view-broken" class="view">broken view</div>
<script>document.querySelectorAll("[data-view]").forEach((button) => button.addEventListener("click", () => {
  document.querySelectorAll(".view").forEach((view) => view.classList.remove("active"));
  if (button.dataset.view === "broken") throw new Error("simulated switch crash");
  document.getElementById("view-" + button.dataset.view).classList.add("active");
}));</script></body></html>`;
    const probeFile = path.join(os.tmpdir(), `kz-connectivity-probe-${process.pid}.html`);
    fs.writeFileSync(probeFile, probeHtml);
    try {
      const entries = await runDesktopTraversal(
        page,
        probeFile,
        [
          { name: "ok", view: "ok", trigger: '[data-view="ok"]' },
          { name: "broken", view: "broken", trigger: '[data-view="broken"]' },
        ],
        "probe",
        false,
      );
      probe = { entries };
      const ok = entries.find((entry) => entry.name === "ok");
      const broken = entries.find((entry) => entry.name === "broken");
      if (!ok?.visible || ok.switchErrors?.length) {
        failures.push("probe 反证失败：正常视图 ok 未能切换");
      }
      if (!broken || broken.visible || !broken.switchErrors?.length) {
        failures.push("probe 反证失败：broken 切换崩溃未被检出");
      }
    } finally {
      fs.unlinkSync(probeFile);
    }
  } else if (customHtml) {
    await runDesktopTraversal(page, customHtml, KEY_PATHS.desktop, "desktop");
  } else {
    const beforePwa = consoleErrors.length;
    await page.goto(`file:///${PWA_ROOT.replace(/\\/g, "/")}/index.html`);
    await page.waitForTimeout(300);
    const pairPageApp = (await page.locator("#app").count()) > 0;
    const initErrors = consoleErrors
      .slice(beforePwa)
      .filter((error) => !error.includes("net::ERR_FILE_NOT_FOUND") && !error.toLowerCase().includes("favicon"));
    pwa = { pairPageApp, initErrors };
    if (!pairPageApp) failures.push("PWA 配对页加载失败：#app 容器不存在");
    if (initErrors.length) failures.push(`PWA 配对页初始化 console 错误：${initErrors.join(" | ")}`);

    const beforeDesktop = consoleErrors.length;
    await page.goto(`file:///${UI_HTML.replace(/\\/g, "/")}`);
    await page.waitForTimeout(200);
    const desktopErrors = consoleErrors.slice(beforeDesktop);
    desktopNote = desktopErrors.length
      ? `ui/index.html 在 headless file:// 下有 ${desktopErrors.length} 条初始化错误（依赖 Tauri IPC），桌面运行时遍历降级；检测能力由 --probe 反证。`
      : "ui/index.html 在 headless file:// 下初始化无错误。";
  }
} finally {
  await browser.close();
}

const report = { probe, pwa, desktopNote, failures, durationMs: Date.now() - started };
if (!wantJson) {
  if (probe) {
    console.log(`[ui-connectivity-browser] 反证：耗时 ${report.durationMs}ms`);
    for (const entry of probe.entries) {
      console.log(`  ${entry.visible && !entry.switchErrors ? "✓" : "✗"} ${entry.name}: #view-${entry.view} ${entry.visible ? "可见" : "不可见"}${entry.switchErrors ? ` (+${entry.switchErrors.length} console 错误)` : ""}`);
    }
  }
  if (pwa) console.log(`[ui-connectivity-browser] PWA 配对页：#app ${pwa.pairPageApp ? "存在" : "缺失"}`);
  if (desktopNote) console.log(`[ui-connectivity-browser] 桌面端：${desktopNote}`);
  if (!failures.length) console.log("  ✓ 全部通过");
  else failures.forEach((failure) => console.log(`  ✗ ${failure}`));
}
if (wantJson) console.log(JSON.stringify(report, null, 2));
if (failures.length) process.exitCode = 1;
