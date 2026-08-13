// R-101 E2 harness 基座验证:通过 CDP 连接真实 WebView2(kzapp)。
// 用法:node scripts/e2e-smoke.mjs [端口]
// 前置:已构建 target/debug/kzapp.exe;启动时注入 KANZEI_E2E_CDP=<端口>。
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";
import { chromium } from "playwright-core";

const root = resolve(import.meta.dirname, "..");
const exe = join(root, "target", "debug", "kzapp.exe");
const PORT = Number(process.argv[2] ?? 9335);

function assert(cond, msg) {
  if (!cond) throw new Error(`断言失败: ${msg}`);
}

// 独立沙箱:用户数据目录与 HOME 都指向临时目录,避免污染真实配置。
const sandbox = mkdtempSync(join(tmpdir(), "kz-e2e-"));
const userData = join(sandbox, "webview");
const home = join(sandbox, "home");
const env = {
  ...process.env,
  USERPROFILE: home,
  HOME: home,
  // R-200:全局根隔离三连缺一即退回读开发者真实 ~/.kanzei(Windows dirs::home_dir()
  // 不认 USERPROFILE,KANZEI_HOME 是官方隔离通道),kzapp 会读取它。
  KANZEI_HOME: join(home, ".kanzei"),
  WEBVIEW2_USER_DATA_FOLDER: userData,
  KANZEI_E2E_CDP: String(PORT),
};

const child = spawn(exe, [], { env, stdio: ["ignore", "pipe", "pipe"] });
let logs = "";
child.stdout.on("data", (d) => (logs += d.toString()));
child.stderr.on("data", (d) => (logs += d.toString()));

let browser;
try {
  // 等待 CDP 端口就绪(WebView2 启动 + 注入参数生效,最长 20s)。
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) {
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      break;
    } catch {
      await new Promise((r) => setTimeout(r, 500));
    }
  }
  assert(browser, `CDP 端口 ${PORT} 20 秒内未就绪`);

  const contexts = browser.contexts();
  assert(contexts.length >= 1, "CDP 未暴露任何 browser context");
  // 取默认 context 的第一个页面(WebView2 会有一个默认 page)。
  let page = null;
  for (const ctx of contexts) {
    const pages = ctx.pages();
    if (pages.length > 0) {
      page = pages[0];
      break;
    }
  }
  assert(page, "CDP 连接成功但无页面");
  await page.waitForLoadState("domcontentloaded").catch(() => {});

  const title = await page.title().catch(() => "");
  const url = page.url();
  console.log(`[e2e-smoke] 连接成功 pid=${child.pid}`);
  console.log(`[e2e-smoke] title="${title}"`);
  console.log(`[e2e-smoke] url=${url}`);

  // 断言:页面可交互(存在根容器),证明真实 UI 已渲染。
  const hasApp = await page
    .evaluate(() => !!document.querySelector("#app, #main, #messages"))
    .catch(() => false);
  assert(hasApp, "真实 UI 未找到根容器(#app/#main/#messages)");
  console.log("[e2e-smoke] 真实 UI 已渲染,harness 基座可行 ✔");

  // 截图留证。
  const shot = join(root, "output", "e2e", "smoke.png");
  const { mkdirSync } = await import("node:fs");
  mkdirSync(join(root, "output", "e2e"), { recursive: true });
  await page.screenshot({ path: shot });
  console.log(`[e2e-smoke] 截图:${shot}`);

  // 收尾:退出码 0 表示成功。
  await browser.close().catch(() => {});
  console.log("[e2e-smoke] PASS");
  process.exitCode = 0;
} catch (e) {
  console.error(`[e2e-smoke] FAIL: ${e.message}`);
  if (logs) console.error(`--- kzapp 日志 ---\n${logs.slice(-3000)}`);
  process.exitCode = 1;
} finally {
  if (browser) await browser.close().catch(() => {});
  if (child.exitCode === null) {
    // 给退出一点时间;强杀兜底。
    child.kill();
    await new Promise((r) => setTimeout(r, 500));
    if (child.exitCode === null) child.kill("SIGKILL");
  }
  rmSync(sandbox, { recursive: true, force: true });
}
