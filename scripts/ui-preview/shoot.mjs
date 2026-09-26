#!/usr/bin/env node
// kanzei UI 预览:headless Edge 逐场景加载 + 零 console 错误校验 + 截图。
//
// 用法:
//   node scripts/ui-preview/shoot.mjs [--out <dir>] [--scenes chat,settings] [--themes dark,light]
//                                     [--dialogs ask,confirm] [--width 1440] [--height 900] [--scale 1] [--json]
//                                     [--query k=v&k2=v2](场景参数,如 memory-graph 的 hover/select/ego)
// 默认输出 output/ui-preview/<scene>-<theme>.png;overlays 的非默认弹窗另存 overlays-<dialog>-<theme>.png。
// 任一页面出现 console.error / 未捕获异常 / 静态资源 4xx-5xx 即退出码 1。
// 服务在脚本内以随机端口启动,结束时关闭(不影响手动开着的 5178)。
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./server.mjs";
import { mockedCommandNames } from "./fixtures.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "../..");
const args = process.argv.slice(2);
const opt = (flag, fallback) => {
  const index = args.indexOf(flag);
  return index >= 0 && args[index + 1] ? args[index + 1] : fallback;
};
const list = (value) => value.split(",").map((item) => item.trim()).filter(Boolean);

const outDir = path.resolve(opt("--out", path.join(REPO, "output/ui-preview")));
const scenes = list(opt("--scenes", "chat,agents,parallel,settings,docs,overlays,lines,empty,memory,memory-graph"));
const themes = list(opt("--themes", "dark,light"));
const dialogs = list(opt("--dialogs", "ask,question,confirm,input,viewer,palette"));
const width = Number(opt("--width", "1440"));
const height = Number(opt("--height", "900"));
// 设备像素比:用户屏幕是 1600@1.25 与 1280@1.5 两档,按真机缩放截图才看得出字号与细线是否发虚。
const scale = Number(opt("--scale", "1"));
const extraQuery = new URLSearchParams(opt("--query", ""));
const wantJson = args.includes("--json");

const shots = [];
for (const scene of scenes) {
  for (const theme of themes) {
    if (scene === "overlays") {
      for (const dialog of dialogs) {
        shots.push({ scene, theme, dialog, file: dialog === "ask" ? `overlays-${theme}.png` : `overlays-${dialog}-${theme}.png` });
      }
    } else {
      shots.push({ scene, theme, file: `${scene}-${theme}.png` });
    }
  }
}

await mkdir(outDir, { recursive: true });
const { origin, close } = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const results = [];
try {
  for (const shot of shots) {
    const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale, colorScheme: shot.theme });
    const page = await context.newPage();
    const errors = [];
    const infos = [];
    page.on("console", (message) => {
      const text = message.text();
      if (message.type() === "error") errors.push(`console.error: ${text}`);
      else if (text.startsWith("[kz-preview]")) infos.push(`${message.type()}: ${text}`);
    });
    page.on("pageerror", (error) => errors.push(`pageerror: ${error.stack || error.message}`));
    page.on("response", (response) => {
      if (response.status() >= 400) errors.push(`HTTP ${response.status()} ${response.url()}`);
    });
    page.on("requestfailed", (request) => errors.push(`requestfailed: ${request.url()} ${request.failure()?.errorText ?? ""}`));
    const query = new URLSearchParams({ theme: shot.theme, scene: shot.scene, ...(shot.dialog ? { dialog: shot.dialog } : {}), ...Object.fromEntries(extraQuery) });
    const url = `${origin}/?${query}`;
    const started = Date.now();
    let ready = false;
    try {
      await page.goto(url, { waitUntil: "load" });
      await page.waitForFunction(() => window.__kzPreview?.ready === true, null, { timeout: 25000 });
      ready = true;
      // 流式渲染合帧、面板过渡动画:给一点时间落定。
      await page.waitForTimeout(400);
    } catch (error) {
      errors.push(`未就绪: ${error.message}`);
    }
    const file = path.join(outDir, shot.file);
    await page.screenshot({ path: file });
    const probe = await page.evaluate(() => ({
      unknown: window.__kzPreview?.unknown?.() ?? [],
      calls: (window.__kzPreview?.calls ?? []).length,
      view: document.body.dataset.view ?? null,
      theme: document.documentElement.getAttribute("data-theme"),
      activeView: document.querySelector(".view.active")?.id ?? null,
    })).catch(() => ({ unknown: [], calls: 0 }));
    results.push({ ...shot, url, file, ready, ms: Date.now() - started, errors, infos, ...probe });
    await context.close();
  }
} finally {
  await browser.close();
  await close();
}

const failed = results.filter((result) => result.errors.length || !result.ready);
const unknownAll = [...new Set(results.flatMap((result) => result.unknown))].sort();
if (wantJson) {
  console.log(JSON.stringify({ outDir, mocked: mockedCommandNames(), unknown: unknownAll, results }, null, 2));
} else {
  console.log(`[ui-preview] ${results.length} 张截图 → ${outDir}`);
  for (const result of results) {
    const mark = result.errors.length || !result.ready ? "✗" : "✓";
    console.log(`  ${mark} ${path.basename(result.file)}  view=${result.activeView} theme=${result.theme} ipc=${result.calls} ${result.ms}ms`);
    for (const error of result.errors) console.log(`      ${error}`);
  }
  console.log(`[ui-preview] 已模拟命令 ${mockedCommandNames().length} 个;本次未模拟而走默认值: ${unknownAll.join(", ") || "无"}`);
}
if (failed.length) process.exitCode = 1;
