// Real Edge DOM and production Markdown/navigation/surface owners; isolated IPC only.
// --baseline additionally serves the released navigation module to prove the old failure.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.join(repo, "output/playwright/file-link-menu");
const project = repo.replace(/\\/g, "/");
const reportPath = `${project}/docs/reviews/2026-10-16-product-design-audit.md`;
const relativePath = "output/审查 截图/报告 图 (最终).png";
const imagePath = `${project}/${relativePath}`;
const worktree = `${project}/.kanzei/worktrees/file-link-fixture`;
const tools = [{ id: "vscode", label: "VS Code", available: true, builtin: true, command: null, args: [] }];
const selectedCase = process.argv.find(arg => arg.startsWith("--case="))?.slice("--case=".length);
const result = {
  startedAt: new Date().toISOString(), scenarios: [], checks: [], failures: [], screenshots: [],
  boundary: "Production ESM and actual Edge DOM; only IPC, clipboard and fixture conversation state are mocked. No native application or real project data is opened or changed.",
  baseline: null,
};
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const menu = page => page.locator('[id^="kz-menu-"][role="menu"]');
const item = (page, name) => menu(page).getByRole("menuitem", { name, exact: true });
const calls = (page, cmd) => page.evaluate(cmd => window.__kzPreview.calls.filter(call => call.cmd === cmd), cmd);
const check = (value, name) => { assert.ok(value, name); result.checks.push(name); console.log(`PASS ${name}`); };
const waitPreview = (page, root, path) => page.waitForFunction(({ root, path }) => {
  const { filesDoc, filesEditor } = window.__fileLink.editor;
  return filesDoc?.root === root && filesDoc.path === path
    && filesEditor?.getModel()?.getValue() === `# 文件链接预览\n${root}\n${path}\n`;
}, { root, path });
async function screenshot(page, name) {
  const target = path.join(output, `${name}.png`);
  await page.screenshot({ path: target, fullPage: true });
  result.screenshots.push(target);
}
async function setup(page, { independent = false, baselineSource = null, general = false } = {}) {
  if (baselineSource) await page.route("**/19-structured-navigation.js", route => route.fulfill({ contentType: "text/javascript", body: baselineSource }));
  await page.goto(`${server.origin}/?scene=empty&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async ({ project, reportPath, imagePath, relativePath, worktree, tools, independent, general }) => {
    const shell = await import("/03-shell.js");
    const panes = await import("/01-chat-panes.js");
    const { enterProject } = await import("/09-sessions.js");
    const { renderMarkdownInto } = await import("/04-markdown.js");
    const fixture = {
      id: "file-link-fixture", session_id: "file-link-session", origin_project: project,
      project_dir: project, worktree_path: independent ? worktree : null, profile: "dev", running: false,
      title: "文件链接回归", label: "文件链接回归", branch: null,
    };
    const projects = general ? { current: null, projects: [], names: {} }
      : { current: project, projects: [project], names: { [project]: "文件链接测试项目" } };
    window.__kzPreview.setCommand("projects_get", projects);
    window.__kzPreview.setCommand("projects_select", ({ path }) => {
      if (path !== project) throw new Error(`未登记的测试项目: ${path}`);
      return projects;
    });
    window.__kzPreview.setCommand("process_list", ({ projectDir }) => projectDir === project ? [fixture] : []);
    const workspace = { current: project, observed_at: Date.now(), projects: [{
      path: project, project_id: project, name: projects.names[project], current: true,
      status: "idle", running_lines: 0, pending_count: 0, lines: [fixture], current_items: [],
      work_units: [], decisions: [], work_acceptances: [], verification_jobs: [],
      freshness: "fresh", error: null,
    }] };
    window.__kzPreview.setCommand("workspace_overview", workspace);
    window.__kzPreview.setCommand("workspace_snapshot", ({ projectDir } = {}) => ({
      ...workspace, projects: !projectDir || projectDir === project ? workspace.projects : [],
    }));
    window.__kzPreview.setCommand("open_tools_list", tools);
    window.__kzPreview.setCommand("file_preview", ({ projectDir, path }) => {
      // files_edit::lexical_segments requires a relative path under projectDir.
      // Accepting absolute paths here would conceal a stale-project preview bug.
      if (/^(?:[A-Za-z]:|[/\\])/.test(path) || path.split(/[/\\]/).includes("..")) throw new Error(`路径不合法: ${path}`);
      return {
        content: `# 文件链接预览\n${projectDir}\n${path}\n`, binary: false, truncated: false, size: 128,
        hash: "fixture-preview", bom: false, eol: "lf", mixedEol: false, encoding: "utf-8", mtimeMs: 1, readonly: null,
      };
    });
    window.__kzPreview.setCommand("file_stat", { exists: true, size: 128, mtimeMs: 1 });
    // Activate both browsing and execution owners through the production
    // transaction. Setting only currentProject leaves the preview's old
    // browsingProject selected, so navigate_view would switch back and replace
    // these messages when this script runs outside the preview fixture's path.
    if (general) {
      window.__kzPreview.setCommand("general_chat_open", project);
      await (await import("/29-general-chat.js")).openGeneralChat();
    } else await enterProject(projects);
    if (shell.currentProject !== project || shell.activeProcessId !== fixture.id || shell.activeSessionId !== fixture.session_id) {
      throw new Error("文件链接夹具未进入目标项目与会话");
    }
    const message = document.createElement("div"); message.className = "msg assistant"; message.dataset.sessionId = fixture.session_id;
    const body = document.createElement("div"); body.className = "message-body md"; body.id = "file-link-fixture";
    renderMarkdownInto(body, [
      "## 审计报告与截图", `[审计报告](<${reportPath}>)`, `[中文空格截图](<${imagePath}>)`,
      `[相对截图](<${relativePath}>)`, "[项目相对报告](<output/审查 截图/项目报告.md>)",
      "[外部网页](https://example.com/report)", "普通文本不属于文件链接。",
    ].join("\n\n"));
    message.append(body); panes.paneFor(fixture.session_id).append(message);
    window.__fileLink = { shell, fixture, editor: await import("/17-files-editor.js"), copied: [], contextEvents: [] };
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: {
      writeText: async text => { window.__fileLink.copied.push(text); },
    } });
    document.addEventListener("contextmenu", event => window.__fileLink.contextEvents.push({
      text: event.target.textContent, prevented: event.defaultPrevented,
    }));
  }, { project: general ? "C:/Users/kanzei/.kanzei/conversations/general" : project, reportPath, imagePath, relativePath, worktree, tools, independent, general });
  await page.locator("#file-link-fixture a.md-path").first().waitFor({ state: "visible" });
}
async function delayTools(page) {
  await page.evaluate(() => {
    const deferred = {};
    deferred.promise = new Promise((resolve, reject) => { deferred.resolve = resolve; deferred.reject = reject; });
    window.__fileLink.deferred = deferred;
    window.__kzPreview.setCommand("open_tools_list", () => deferred.promise);
  });
}
async function releaseTools(page) {
  await page.evaluate(async tools => {
    window.__fileLink.deferred.resolve(tools);
    await window.__fileLink.deferred.promise;
    // Cross the rendering boundary after the real IPC promise and its subscribers.
    await new Promise(requestAnimationFrame);
  }, tools);
}
async function scenario(name, run, options = {}) {
  if (selectedCase && selectedCase !== name) return;
  result.scenarios.push(name);
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.setDefaultTimeout(10000);
  try {
    await setup(page, options);
    await run(page);
    assert.deepEqual(errors, [], `${name}: uncaught browser errors`);
  } catch (error) {
    const state = await page.evaluate(() => ({
      project: window.__fileLink?.shell.currentProject,
      processes: window.__fileLink?.shell.processItems,
      fileRoot: window.__fileLink?.editor.filesRoot(),
      fileDoc: window.__fileLink?.editor.filesDoc,
      fileContent: window.__fileLink?.editor.filesEditor?.getModel()?.getValue(),
      calls: window.__kzPreview?.calls.filter(call => ["file_preview", "file_stat", "process_list", "open_with", "reveal_path"].includes(call.cmd)).slice(-20),
    }));
    result.failures.push({ name, error: error.stack || String(error), browserErrors: errors, state });
    console.error(`FAIL ${name}: ${error.message}`);
    await screenshot(page, `failure-${name}`);
  } finally { await page.close(); }
}

try {
  if (process.argv.includes("--baseline")) {
    const baselineSource = execFileSync("git", ["show", "3938210f:crates/kanzei-app/ui/19-structured-navigation.js"], { cwd: repo, encoding: "utf8" });
    await scenario("released-baseline", async page => {
      await page.getByRole("link", { name: "审计报告", exact: true }).click({ button: "right" });
      const state = await page.evaluate(() => ({ menus: document.querySelectorAll('[id^="kz-menu-"][role="menu"]').length, event: window.__fileLink.contextEvents.at(-1) }));
      assert.equal(state.menus, 0); assert.equal(state.event.prevented, false);
      assert.throws(() => assert.equal(state.menus, 1), "the new menu assertion must fail on the released implementation");
      result.baseline = { commit: "3938210f", expectedRegressionObserved: true, ...state };
      check(true, "released navigation reproduces the original missing right-click menu");
      await screenshot(page, "released-baseline-no-menu");
    }, { baselineSource });
  }
  await scenario("inline-artifact", async page => {
    const identity = await page.evaluate(async () => {
      const { renderMarkdownInto } = await import("/04-markdown.js");
      const body = document.getElementById("file-link-fixture");
      const root = window.__fileLink.fixture.origin_project;
      const path = "artifacts/fnv-37febed659b71441/monty-hall-两对话整理.md";
      const absolute = `${root}/${path}`.replaceAll("/", "\\");
      const reply = `两份对话都用表格和分隔线整理，便于阅读。文件已保存到 \`${absolute}\`。`;
      renderMarkdownInto(body, reply.slice(0, -2), { streaming: true });
      const incompleteLinks = body.querySelectorAll("a.md-path").length;
      renderMarkdownInto(body, reply, { streaming: true });
      return { root, path, absolute, reply, incompleteLinks };
    });
    const link = page.locator("#file-link-fixture a.md-path");
    check(identity.incompleteLinks === 0 && await link.count() === 1,
      "streaming artifact path becomes a link only after its inline code closes, without a delivery receipt");
    check(await link.getAttribute("data-path") === identity.absolute && await link.innerText() === identity.absolute,
      "the screenshot's Windows/Chinese artifact path is preserved as a clickable link");
    check(await link.locator(".resource-icon").count() === 1, "automatic file links share the existing resource icon");
    await screenshot(page, "inline-artifact-general-chat");
    await link.click();
    await waitPreview(page, identity.root, identity.path);
    assert.deepEqual((await calls(page, "file_preview")).at(-1).args, { projectDir: identity.root, path: identity.path });
    check(true, "clicking the artifact in a projectless conversation opens its exact file in the real editor");
    await page.evaluate(async reply => {
      window.__fileLink.shell.navigate_view("chat");
      const body = document.getElementById("file-link-fixture");
      (await import("/04-markdown.js")).renderMarkdownInto(body, reply);
      const saved = body.innerHTML;
      body.innerHTML = saved;
    }, identity.reply);
    await link.click({ button: "right" }); await item(page, "复制路径").click();
    check(await page.evaluate(expected => window.__fileLink.copied.at(-1) === expected, identity.absolute.replaceAll("\\", "/")),
      "restored history links reuse the file menu and copy the complete absolute path");
    await link.focus(); await page.keyboard.press("Enter");
    await waitPreview(page, identity.root, identity.path);
    check(true, "Enter opens restored artifact links through the delegated navigation owner");
    await screenshot(page, "inline-artifact-preview");
  }, { general: true });
  await scenario("immediate-menu", async page => {
    await delayTools(page);
    const before = (await calls(page, "file_preview")).length;
    await page.getByRole("link", { name: "审计报告", exact: true }).click({ button: "right" });
    await menu(page).waitFor({ state: "visible" });
    check(await item(page, "在应用内打开").isVisible() && await item(page, "在资源管理器中显示").isVisible() && await item(page, "复制路径").isVisible(), "right-click immediately exposes basic actions before tool discovery resolves");
    check((await calls(page, "file_preview")).length === before && await page.locator("#view-chat").evaluate(el => el.classList.contains("active")), "right-click does not require or trigger left-click preview");
    check(await item(page, "正在读取打开方式…").getAttribute("aria-disabled") === "true", "pending discovery has a visible disabled status");
    await screenshot(page, "immediate-report-menu");
    await page.keyboard.press("ArrowDown");
    await releaseTools(page);
    await item(page, "用 VS Code 打开").waitFor({ state: "visible" });
    check(await page.evaluate(() => document.activeElement.textContent === "在资源管理器中显示"), "async tool insertion preserves the selected basic action");
    await screenshot(page, "loaded-report-menu");
    await item(page, "用 VS Code 打开").click();
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "open_with"));
    assert.deepEqual((await calls(page, "open_with")).at(-1).args, { projectDir: project, processId: "file-link-fixture", path: reportPath, toolId: "vscode" });
    check(true, "VS Code receives the concrete report file and its project/process identity");
  });
  await scenario("unicode-explorer-copy", async page => {
    const link = page.getByRole("link", { name: "中文空格截图", exact: true });
    check(await link.getAttribute("data-path") === imagePath, "Markdown retains Chinese characters, spaces and parentheses in the image path");
    await link.click({ button: "right" }); await item(page, "在资源管理器中显示").click();
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "reveal_path"));
    assert.deepEqual((await calls(page, "reveal_path")).at(-1).args, { projectDir: project, processId: "file-link-fixture", path: imagePath });
    check(true, "Explorer receives the selected image file rather than the project directory");
    await link.click({ button: "right" }); await item(page, "复制路径").click();
    check(await page.evaluate(expected => window.__fileLink.copied.at(-1) === expected, imagePath), "copy writes the absolute Unicode path to the isolated clipboard");
  });
  await scenario("worktree-relative", async page => {
    const link = page.getByRole("link", { name: "相对截图", exact: true });
    await link.click({ button: "right" }); await item(page, "用 VS Code 打开").click();
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "open_with"));
    assert.deepEqual((await calls(page, "open_with")).at(-1).args, { projectDir: project, processId: "file-link-fixture", path: relativePath, toolId: "vscode" });
    check(true, "worktree relative links retain process identity for backend root resolution");
    await link.click({ button: "right" }); await item(page, "复制路径").click();
    check(await page.evaluate(expected => window.__fileLink.copied.at(-1) === expected, `${worktree}/${relativePath}`), "relative path copy resolves against the owning worktree");
    await link.focus(); await page.keyboard.press("Enter");
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "file_preview"));
    assert.deepEqual((await calls(page, "file_preview")).at(-1).args, { projectDir: worktree, path: relativePath });
    await waitPreview(page, worktree, relativePath);
    check(true, "Enter previews a relative link inside its independent worktree");
  }, { independent: true });
  await scenario("project-document-with-active-worktree", async page => {
    const documentPath = "docs/项目 文档.md";
    await page.evaluate(() => window.__fileLink.shell.navigate_view("documents", { persist: false }));
    await page.locator("#documents-req-list .doc-row").first().waitFor({ state: "visible" });
    await page.evaluate(async documentPath => {
      const { renderMarkdownInto } = await import("/04-markdown.js");
      const body = document.createElement("section"); body.id = "project-doc-file-link"; body.className = "md";
      // The project document host is stable; the owner's live requirement list
      // is independently rerendered from its resource and is not fixture storage.
      document.querySelector("#documents-scroll").append(body);
      renderMarkdownInto(body, `## 项目文档附件\n\n[项目文档链接](<${documentPath}>)`);
      window.__kzPreview.setCommand("docs_read_custom", ({ projectDir, relPath }) => ({
        name: relPath, content: `# 主项目文档语义\n\n项目：${projectDir}\n\n文档：${relPath}`,
      }));
    }, documentPath);
    const link = page.locator("#project-doc-file-link a.md-path");
    check(await link.evaluate(el => !el.closest(".msg-pane")
      && window.__fileLink.shell.activeProcessId === window.__fileLink.fixture.id
      && window.__fileLink.shell.processItems.some(item => item.id === window.__fileLink.fixture.id && item.worktree_path)),
    "project document fixture has no conversation pane while the active process owns an independent worktree");
    await link.click({ button: "right" }); await item(page, "用 VS Code 打开").click();
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "open_with"));
    assert.deepEqual((await calls(page, "open_with")).at(-1).args, { projectDir: project, path: documentPath, toolId: "vscode" });
    check(true, "project document VS Code action omits the unrelated active process identity");
    await link.click({ button: "right" }); await item(page, "在资源管理器中显示").click();
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "reveal_path"));
    assert.deepEqual((await calls(page, "reveal_path")).at(-1).args, { projectDir: project, path: documentPath });
    check(true, "project document Explorer action remains rooted in the main project without a process ID");
    await link.click({ button: "right" }); await item(page, "复制路径").click();
    check(await page.evaluate(expected => window.__fileLink.copied.at(-1) === expected, `${project}/${documentPath}`), "project document copy resolves under the main project rather than the active worktree");
    const beforePreview = (await calls(page, "file_preview")).length;
    await link.focus(); await page.keyboard.press("Enter");
    await page.locator("#viewer-overlay").waitFor({ state: "visible" });
    assert.deepEqual((await calls(page, "docs_read_custom")).at(-1).args, { projectDir: project, relPath: documentPath });
    check(await page.locator("#viewer-body").getByRole("heading", { name: "主项目文档语义", exact: true }).isVisible()
      && await page.locator("#viewer-body").getByText(`项目：${project}`, { exact: true }).isVisible()
      && await page.locator("#viewer-body").getByText(`文档：${documentPath}`, { exact: true }).isVisible()
      && (await calls(page, "file_preview")).length === beforePreview, "Enter preserves the project docs viewer and its original read contract");
    await screenshot(page, "project-document-enter-viewer");
    await page.locator("#viewer-close").click();
    await link.click({ button: "right" }); await item(page, "在应用内打开").click();
    await waitPreview(page, project, documentPath);
    assert.deepEqual((await calls(page, "file_preview")).at(-1).args, { projectDir: project, path: documentPath });
    check(true, "project document in-app menu previews the main-project file with actual editor content");
  }, { independent: true });
  await scenario("delivered-file-link", async page => {
    await page.evaluate(async ({ project, reportPath }) => {
      const { registerDelivery } = await import("/06-deliveries.js");
      registerDelivery({
        id: "file-link-delivery", project_dir: project, process_id: "file-link-fixture", session_id: "file-link-session",
        path: reportPath, name: "2026-10-16-product-design-audit.md", status: "available", bytes: 128,
      }, "file-link-session");
      window.__kzPreview.setCommand("open_delivered_path", null);
    }, { project, reportPath });
    const link = page.locator("#file-link-fixture button.delivery-inline");
    check(await link.count() === 1 && await page.getByRole("link", { name: "审计报告", exact: true }).count() === 0, "delivery owner replaces the actual Markdown report link with its file button");
    // Even when its process has left the active list, the receipt owns its target.
    await page.evaluate(() => {
      window.__fileLink.shell.setCurrentProject("C:/other-project");
      window.__fileLink.shell.setProcessItems([]);
      window.__fileLink.shell.setActiveProcessId("other-process");
    });
    await link.click({ button: "right" });
    await item(page, "用 VS Code 打开").waitFor({ state: "visible" });
    await screenshot(page, "delivered-report-menu");
    await item(page, "用 VS Code 打开").click();
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "open_with"));
    assert.deepEqual((await calls(page, "open_with")).at(-1).args, { projectDir: project, processId: "file-link-fixture", path: reportPath, toolId: "vscode" });
    check(true, "delivered file right-click retains receipt project/process even after the active process changes");
    await link.focus(); await page.keyboard.press("Shift+F10");
    await menu(page).waitFor({ state: "visible" }); await page.keyboard.press("Escape");
    check(await link.evaluate(el => document.activeElement === el), "delivered file keyboard menu restores focus to its native button");
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "open_delivered_path"));
    const opens = await calls(page, "open_delivered_path");
    assert.equal(opens.length, 1);
    assert.deepEqual(opens[0].args, { projectDir: project, path: reportPath, mode: "open" });
    check(true, "delivered file Enter preserves its original file-card action exactly once");
  });
  await scenario("project-switch-external", async page => {
    await delayTools(page);
    await page.getByRole("link", { name: "审计报告", exact: true }).click({ button: "right" });
    await page.evaluate(() => { window.__fileLink.shell.setCurrentProject("C:/other-project"); window.__fileLink.shell.setActiveProcessId("other-process"); });
    await releaseTools(page); await item(page, "用 VS Code 打开").click();
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "open_with"));
    assert.deepEqual((await calls(page, "open_with")).at(-1).args, { projectDir: project, processId: "file-link-fixture", path: reportPath, toolId: "vscode" });
    check(true, "project changes while discovery waits cannot retarget the external file action");
  });
  await scenario("project-switch-preview", async page => {
    await page.getByRole("link", { name: "项目相对报告", exact: true }).click({ button: "right" });
    await page.evaluate(() => { window.__fileLink.shell.setCurrentProject("C:/other-project"); window.__fileLink.shell.setActiveProcessId("other-process"); });
    await item(page, "在应用内打开").click();
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "file_preview"));
    const actual = (await calls(page, "file_preview")).at(-1).args;
    assert.deepEqual(actual, { projectDir: project, path: "output/审查 截图/项目报告.md" });
    await waitPreview(page, project, "output/审查 截图/项目报告.md");
    check(true, "project changes cannot retarget the in-app relative file preview");
  });
  await scenario("project-switch-worktree-preview", async page => {
    await page.getByRole("link", { name: "相对截图", exact: true }).click({ button: "right" });
    await page.evaluate(() => { window.__fileLink.shell.setCurrentProject("C:/other-project"); window.__fileLink.shell.setActiveProcessId("other-process"); });
    await item(page, "在应用内打开").click();
    await waitPreview(page, worktree, relativePath);
    assert.deepEqual((await calls(page, "file_preview")).at(-1).args, { projectDir: worktree, path: relativePath });
    check(true, "cross-project menu preview restores the owning project and independent worktree with real editor content");
    await screenshot(page, "worktree-preview-after-project-switch");
  }, { independent: true });
  await scenario("escape-late-discovery", async page => {
    await delayTools(page);
    const link = page.getByRole("link", { name: "审计报告", exact: true });
    await link.focus(); await page.keyboard.press("Shift+F10");
    await menu(page).waitFor({ state: "visible" });
    check(await page.evaluate(() => document.activeElement.textContent === "在应用内打开"), "Shift+F10 opens the menu and focuses its first action");
    await page.keyboard.press("Escape");
    check(await link.evaluate(el => document.activeElement === el), "Escape restores focus to the file link");
    await releaseTools(page);
    check(await menu(page).count() === 0, "late tool discovery never reopens a menu dismissed with Escape");
  });
  await scenario("discovery-failure", async page => {
    await page.evaluate(() => window.__kzPreview.setCommand("open_tools_list", () => { throw new Error("tool discovery unavailable"); }));
    await page.getByRole("link", { name: "审计报告", exact: true }).click({ button: "right" });
    await menu(page).getByText("tool discovery unavailable", { exact: true }).waitFor({ state: "visible" });
    check(await item(page, "在应用内打开").isVisible() && await item(page, "在资源管理器中显示").isVisible() && await item(page, "复制路径").isVisible(), "tool discovery failure is visible while all basic operations remain available");
    await screenshot(page, "tool-discovery-failure");
    await item(page, "在资源管理器中显示").click();
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "reveal_path"));
    check((await calls(page, "reveal_path")).at(-1).args.path === reportPath, "Explorer still works after tool discovery fails");
  });
  await scenario("non-file-context", async page => {
    const before = (await calls(page, "open_tools_list")).length;
    await page.locator('#file-link-fixture a[href="https://example.com/report"]').click({ button: "right" });
    await page.locator("#file-link-fixture p").filter({ hasText: "普通文本不属于文件链接。" }).click({ button: "right" });
    check(await page.evaluate(() => window.__fileLink.contextEvents.length === 2 && window.__fileLink.contextEvents.every(event => !event.prevented)), "external links and ordinary text retain their native context-menu behavior");
    check(await menu(page).count() === 0 && (await calls(page, "open_tools_list")).length === before, "non-file context menus do not discover tools or create app menus");
  });
  await scenario("context-key-and-enter", async page => {
    const link = page.getByRole("link", { name: "审计报告", exact: true });
    await link.focus(); await page.keyboard.press("ContextMenu");
    await menu(page).waitFor({ state: "visible" });
    check(await page.evaluate(() => document.activeElement.textContent === "在应用内打开"), "ContextMenu key opens and focuses the same menu");
    await page.keyboard.press("Escape");
    check(await link.evaluate(el => document.activeElement === el), "keyboard menu close returns focus to the original link");
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "file_preview"));
    const actual = (await calls(page, "file_preview")).at(-1).args;
    assert.deepEqual(actual, { projectDir: project, path: "docs/reviews/2026-10-16-product-design-audit.md" });
    await waitPreview(page, project, "docs/reviews/2026-10-16-product-design-audit.md");
    await page.locator("#files-editor .monaco-editor").waitFor({ state: "visible" });
    check(await page.locator("#view-files").evaluate(el => el.classList.contains("active")), "Enter opens the report in the real in-app file preview");
    await screenshot(page, "keyboard-enter-preview");
  });
  assert.ok(result.scenarios.length > 0, `No matching scenario: ${selectedCase}`);
} catch (error) {
  result.failures.push({ name: "suite", error: error.stack || String(error) });
  throw error;
} finally {
  await browser.close(); await server.close();
  result.finishedAt = new Date().toISOString(); result.passed = result.failures.length === 0;
  await writeFile(path.join(output, "results.json"), `${JSON.stringify(result, null, 2)}\n`, "utf8");
}
assert.equal(result.failures.length, 0, result.failures.map(failure => `${failure.name}: ${failure.error}`).join("\n"));
console.log(`File link menu passed: ${result.checks.length} checks${result.baseline ? "; released baseline regression reproduced" : ""}.`);
