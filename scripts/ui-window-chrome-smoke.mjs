#!/usr/bin/env node
// 真实 Edge 端到端冒烟：托管 Vite 产物 dist-web/，验证窗口顶栏、三键、视图切换、文件抽屉与关闭保护。
import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { chromium } from "playwright-core";

const root = path.resolve(import.meta.dirname, "..");
const distDir = path.join(root, "crates/kanzei-app/dist-web");
assert.ok(
  fs.existsSync(path.join(distDir, "index.html")),
  "缺少 dist-web/index.html，请先执行 npm run build"
);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "application/javascript; charset=utf-8",
  ".mjs": "application/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".ttf": "font/ttf",
  ".json": "application/json; charset=utf-8",
};

const server = http.createServer((req, res) => {
  const urlPath = (req.url || "/").split("?")[0];
  const rel = urlPath === "/" ? "index.html" : urlPath.replace(/^\/+/, "");
  const filePath = path.join(distDir, rel);
  if (!filePath.startsWith(distDir) || !fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
    res.writeHead(404);
    res.end("Not Found");
    return;
  }
  const ext = path.extname(filePath).toLowerCase();
  res.writeHead(200, { "Content-Type": MIME[ext] || "application/octet-stream" });
  fs.createReadStream(filePath).pipe(res);
});

await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const { port } = server.address();
const baseUrl = `http://127.0.0.1:${port}/`;

let browser;
try {
  browser = await chromium.launch({ channel: "msedge", headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();

  const pageErrors = [];
  const consoleErrors = [];
  page.on("pageerror", (err) => pageErrors.push(String(err)));
  page.on("console", (msg) => {
    if (msg.type() === "error") consoleErrors.push(msg.text());
  });

  await page.addInitScript(() => {
    window.__E2E_CALLS__ = [];
    window.__E2E_WINDOW_ACTIONS__ = [];
    let maximized = false;
    let fileWriteAttempt = 0;
    window.__TAURI__ = {
      core: {
        async invoke(command, args) {
          window.__E2E_CALLS__.push({ command, args });
          switch (command) {
            case "app_bootstrap":
              return {
                appInfo: { version: "0.1.0", build: "e2e" },
                uiPrefs: { theme: "dark", ui_layout: {} },
                projects: {
                  projects: ["C:/demo-repo"],
                  current: "C:/demo-repo",
                  names: { "C:/demo-repo": "Demo Repo" },
                },
                generalChatRoot: null,
                composerDrafts: { items: [], error: null },
                target: { kind: "project", projectDir: "C:/demo-repo" },
              };
            case "projects_select":
              return {
                projects: ["C:/demo-repo"],
                current: "C:/demo-repo",
                names: { "C:/demo-repo": "Demo Repo" },
              };
            case "process_list":
              return [
                {
                  id: "d|C:/demo-repo",
                  origin_project: "C:/demo-repo",
                  project_dir: "C:/demo-repo",
                  worktree_path: null,
                  branch: "main",
                  session_id: "ses-e2e",
                  model: null,
                  profile: "dev-auto",
                  research_topic: null,
                  reasoning: null,
                  manual_models: [],
                  subagent_mode: "auto",
                  tracker_writes: false,
                  stage: "空闲",
                  running: false,
                  label: "主对话",
                  title: null,
                  title_custom: false,
                  kind: "main",
                  ordinal: null,
                  updated_at: Date.now(),
                },
              ];
            case "conversation_page_messages":
              return {
                items: [
                  { role: "user", parts: [{ type: "text", text: "帮我检查 src/main.rs:10" }] },
                  { role: "assistant", parts: [{ type: "text", text: "已定位 `src/main.rs:10`，可点击查看 Diff。" }] },
                ],
                total: 2,
                startIndex: 0,
                endIndex: 2,
                hasMore: false,
              };
            case "git_status":
              return {
                repo: "own",
                branch: "main",
                changes: 1,
                last: "abc123 init",
                additions: 5,
                deletions: 1,
                files: [{ path: "src/main.rs", additions: 5, deletions: 1 }],
              };
            case "file_preview":
              return {
                path: args?.path || "src/main.rs",
                content: "new\n",
                hash: "h1",
                bom: false,
                readonly: null,
                binary: false,
              };
            case "file_git_diff":
              return { path: args?.path || "src/main.rs", diff: "@@ -1 +1 @@\n-old\n+new\n" };
            case "file_write":
              fileWriteAttempt += 1;
              if (fileWriteAttempt === 1) {
                return { status: "conflict", hash: "h-disk-v2", size: 12, mtimeMs: 1000, exists: true, evidence: null };
              }
              return { status: "saved", hash: `h-saved-${fileWriteAttempt}`, size: 14, mtimeMs: 2000, exists: true, evidence: null };
            case "context_snapshot":
              return {
                sessionId: "ses-e2e",
                requests: [{ sequence: 1, step: 1, model: "openai:gpt-5" }],
                selected: {
                  sequence: 1,
                  manifest: {
                    model: "openai:gpt-5",
                    estimated_tokens: 1200,
                    blocks: [
                      {
                        position: 0,
                        kind: "system",
                        role: "system",
                        label: "agent/system",
                        chars: 320,
                        bytes: 320,
                        sha256: "sha-sys",
                        inspectable: true,
                        source: { system_index: 0, offset_bytes: 0, length_bytes: 320 },
                      },
                    ],
                  },
                },
                web: [],
              };
            case "workspace_overview":
              return {
                current: "C:/demo-repo",
                projects: [{ path: "C:/demo-repo", project_id: "C:/demo-repo", name: "Demo Repo", current: true, lines_total: 1, running_lines: 0 }],
                observed_at: Date.now(),
              };
            case "models_list":
              return { models: [{ id: "openai:gpt-5", label: "openai:gpt-5" }], errors: [] };
            case "model_effective":
              return { model: { value: "openai:gpt-5", source: "global" } };
            case "auto_state_get":
              return { enabled: false, paused: false, goal: null };
            case "settings_get":
              return {
                path: "~/.kanzei/kanzei.toml",
                primary: "openai:gpt-5",
                fast: "ollama:qwen3",
                compact: "",
                proxy: "env",
                reasoning: "medium",
                codexFastMode: false,
                profileDefault: "dev-auto",
                providers: [{ name: "openai", protocol: "openai", baseUrl: "https://api.openai.com/v1", builtin: true, source: "global" }],
                limits: {},
              };
            case "skills_list":
              return [{ id: "builtin:review", name: "review", displayName: "代码评审", description: "评审代码变更", source: "builtin", editable: false, enabled: true }];
            case "memory_entries":
              return [{ id: "M-001", scope: "project", category: "fact", title: "架构单进程", description: "关闭窗口即退出" }];
            case "fast_model_status":
              return { managed: false };
            case "app_shutdown":
              return { stopped: true, state: "quiescent" };
            default:
              return [];
          }
        },
      },
      event: {
        async listen() {
          return () => {};
        },
      },
      window: {
        getCurrentWindow() {
          return {
            async minimize() { window.__E2E_WINDOW_ACTIONS__.push("minimize"); },
            async toggleMaximize() { maximized = !maximized; window.__E2E_WINDOW_ACTIONS__.push("maximize"); },
            async isMaximized() { return maximized; },
            async startDragging() { window.__E2E_WINDOW_ACTIONS__.push("drag"); },
            async close() { window.__E2E_WINDOW_ACTIONS__.push("close"); },
            async onResized() { return () => {}; },
          };
        },
      },
    };
  });

  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.waitForSelector("#window-chrome");
  await page.waitForSelector("#window-minimize");
  await page.waitForSelector("#window-maximize");
  await page.waitForSelector("#window-close");

  // 1. 验证 44px 顶部窗口栏与三键边界无重叠、无裁切
  const chromeRect = await page.locator("#window-chrome").boundingBox();
  assert.ok(chromeRect && Math.abs(chromeRect.height - 44) <= 1, `顶栏高度应为 44px，实测 ${chromeRect?.height}`);
  const buttons = ["#window-minimize", "#window-maximize", "#window-close"];
  const rects = [];
  for (const sel of buttons) {
    const r = await page.locator(sel).boundingBox();
    assert.ok(r && r.width >= 32 && r.height >= 32, `${sel} 按钮不可见或尺寸异常`);
    assert.ok(r.x >= 0 && r.x + r.width <= 1280.5, `${sel} 超出视口右边界`);
    rects.push(r);
  }
  assert.ok(rects[0].x + rects[0].width <= rects[1].x + 0.5, "最小化与最大化按钮重叠");
  assert.ok(rects[1].x + rects[1].width <= rects[2].x + 0.5, "最大化与关闭按钮重叠");

  // Exercise the redesigned controls in both actual Edge color schemes.
  for (const theme of ["dark", "light"]) {
    if (theme === "light") await page.getByRole("button", {name:"切换明暗主题"}).click();
    await page.waitForFunction(value => document.documentElement.dataset.theme === value, theme);
    const model = page.getByRole("button", {name:"模型选择"});
    await model.focus();
    await page.keyboard.press("Enter");
    const popup = page.getByRole("dialog", {name:"模型选择"});
    await popup.waitFor();
    const rect = await popup.boundingBox();
    assert.ok(rect && rect.x >= 0 && rect.y >= 0 && rect.x + rect.width <= 1281 && rect.y + rect.height <= 801);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Escape");
    assert.ok(await model.evaluate(el => document.activeElement === el));
    await page.getByRole("button", {name:"更多对话操作"}).click();
    await page.getByRole("button", {name:"轨迹",exact:true}).waitFor();
    await page.keyboard.press("Escape");
  }

  // 2. 点击对话中的文件链接打开右侧文件上下文抽屉，验证 Diff 回退此块、轻编辑冲突保护与强制覆盖
  await page.getByRole("button", { name: "src/main.rs:10" }).click();
  await page.waitForSelector(".side-drawer");
  await page.getByRole("button", { name: "轻编辑" }).click();
  await page.locator(".drawer-editor").fill("edited-in-e2e\n");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await page.waitForSelector(".drawer-conflict-bar");
  await page.getByRole("button", { name: "强制覆盖" }).click();
  await page.waitForSelector(".drawer-conflict-bar", { state: "detached" });
  await page.getByRole("button", { name: "Diff" }).click();
  await page.waitForSelector(".diff-line-add");
  await page.getByRole("button", { name: "回退此块" }).click();
  await page.waitForTimeout(120);

  // 2b. 点击底栏「上下文构成」验证真实结构化 source 对象渲染不触发白屏
  await page.getByRole("button", { name: "上下文构成" }).click();
  await page.waitForSelector('[role="dialog"][aria-label="本轮请求上下文构成"]');
  await page.locator(".context-group summary").filter({hasText:"系统提示与规则"}).click();
  await page.locator('[role="dialog"][aria-label="本轮请求上下文构成"] >> text=agent/system').waitFor();
  await page.locator('[role="dialog"][aria-label="本轮请求上下文构成"]').getByRole("button", { name: "关闭" }).click();
  await page.waitForSelector('[role="dialog"][aria-label="本轮请求上下文构成"]', { state: "detached" });

  // 3. 依次切换四个懒加载二级视图，确认无横向溢出与报错
  for (const navLabel of ["工作台", "记忆", "Skills", "设置"]) {
    await page.locator("#workspace-sidebar-footer").getByRole("button", { name: navLabel }).click();
    await page.waitForTimeout(180);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    assert.ok(overflow <= 1, `${navLabel} 视图发生横向溢出: ${overflow}px`);
  }

  // 4. 验证设置页吸底保存栏完整在视口内
  const savebar = await page.locator(".settings-savebar").boundingBox();
  assert.ok(savebar && savebar.x >= 0 && savebar.x + savebar.width <= 1280.5, "设置页保存栏超出视口");

  // 5. 测试窗口最小化、最大化与优雅关闭链路
  await page.locator("#window-minimize").click();
  await page.locator("#window-maximize").click();
  await page.locator("#window-close").click();
  await page.waitForTimeout(150);

  const actions = await page.evaluate(() => window.__E2E_WINDOW_ACTIONS__);
  const calls = await page.evaluate(() => window.__E2E_CALLS__.map((c) => c.command));
  assert.ok(actions.includes("minimize"), "最小化按钮未触发");
  assert.ok(actions.includes("maximize"), "最大化按钮未触发");
  assert.ok(actions.includes("close"), "关闭按钮未触发 window.close");
  assert.ok(calls.includes("app_shutdown"), "关闭窗口前未调用单进程 app_shutdown");

  assert.deepEqual(pageErrors, [], `页面存在未捕获异常: ${pageErrors.join("; ")}`);
  assert.deepEqual(consoleErrors, [], `控制台存在错误: ${consoleErrors.join("; ")}`);

  console.log("Edge E2E smoke PASS: window chrome, chat, context drawer, 4 lazy views, graceful shutdown verified with 0 errors");
} finally {
  if (browser) await browser.close();
  server.close();
}
