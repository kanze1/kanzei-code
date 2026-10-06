// Production UI with disposable IPC: startup, last-chat deletion, and inbox browsing.
/* global window, document, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const output = "output/playwright/deleted-general-inbox";
await mkdir(output, { recursive: true });
const checks = [], errors = [];
const check = (value, text) => { assert(value, text); checks.push(text); console.log(`PASS ${text}`); };
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
page.on("pageerror", error => errors.push(error.message));
try {
  const cold = await browser.newPage({ javaScriptEnabled: false });
  await cold.goto(server.origin);
  check(await cold.locator("#workspace-bar, #sidebar > .sidebar-section").evaluateAll(nodes => nodes.every(node => getComputedStyle(node).display === "none")), "Legacy sidebar sections are hidden before JavaScript starts");
  check(await cold.locator(".workspace-switcher, .rail-section-separator, .rail-global-separator").evaluateAll(nodes => nodes.every(node => getComputedStyle(node).display === "none")), "Retired navigation and separator lines are hidden on the first paint");
  await cold.close();
  await page.goto(`${server.origin}/?scene=chat&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(() => window.__kzPreview.settle());
  check(await page.locator("#process-subagents-wrap .composer-control-label").innerText() === "子代理管理", "The delegation control has an explicit subagent management label");
  check(await page.locator("#profile-mode-wrap .composer-control-label").innerText() === "无监管模式"
    && JSON.stringify(await page.locator("#profile-select option").allTextContents()) === JSON.stringify(["关闭", "开启"]), "Unsupervised mode displays only off/on choices");
  await page.screenshot({ path: `${output}/composer-controls.png` });
  const ids = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures();
    const shell = await import("/03-shell.js"), sessions = await import("/09-sessions.js");
    const project = shell.currentProject, general = "C:/smoke/general";
    const item = { ...f.state.processes[0], id: `p77|${general}`, session_id: "ses_delete_last", kind: "discussion", profile: "readonly", origin_project: general, project_dir: general, title: "待删除的最后一个对话", running: false };
    window.__deleteProbe = { rows: [item], project, general, purged: false, delayed: false };
    (await import("/03-general-scope.js")).setGeneralChatRoot(general);
    shell.setCurrentProject(general);
    window.__kzPreview.setCommand("process_list", async ({ projectDir }) => {
      if (projectDir !== general) return f.commands.process_list({ projectDir });
      const probe = window.__deleteProbe, snapshot = structuredClone(probe.rows);
      if (probe.delayNext) {
        probe.delayNext = false; probe.delayed = true;
        await new Promise(resolve => { probe.release = resolve; });
      }
      return snapshot;
    });
    window.__kzPreview.setCommand("process_purge", () => { window.__deleteProbe.rows = []; window.__deleteProbe.purged = true; return "对话已删除"; });
    sessions.renderProcesses([item]);
    const core = await import("/01-core.js");
    core.showPane(item.session_id);
    (await import("/05-chat-render.js")).addMessage("user", "DELETED_MESSAGE_MUST_DISAPPEAR");
    (await import("/12-workbench.js")).setBrowsingProject(null);
    shell.navigate_view("chat", { prepared: true });
    return { project, general, id: item.id, sessionId: item.session_id };
  });
  check(await page.locator("#messages").innerText().then(text => text.includes("DELETED_MESSAGE_MUST_DISAPPEAR")), "The deleted-chat reproduction starts with visible content");
  await page.evaluate(async ids => {
    const sessions = await import("/09-sessions.js");
    window.__deleteProbe.delayNext = true;
    void sessions.refreshProcesses();
    window.__deletePromise = (await import("/12-session-menus.js")).deleteSession(ids.general, ids.id);
  }, ids);
  await page.locator("#confirm-ok").click();
  await page.waitForFunction(() => window.__deleteProbe.purged && window.__deleteProbe.delayed);
  check(!await page.locator("#messages").innerText().then(text => text.includes("DELETED_MESSAGE_MUST_DISAPPEAR")), "Deletion clears visible content while an older poll is still pending");
  await page.evaluate(async () => { window.__deleteProbe.release(); await window.__deletePromise; });
  check(await page.evaluate(async () => {
    const shell = await import("/03-shell.js");
    return shell.activeProcessId === null && shell.activeSessionId === null && shell.processItems.length === 0;
  }), "A fresh post-delete snapshot wins over the delayed stale poll");
  check(!await page.locator("#messages").innerText().then(text => text.includes("DELETED_MESSAGE_MUST_DISAPPEAR")), "Deleting the last chat leaves no old messages in the main area");
  check(await page.locator(`[data-ctx='session'][data-process-id=${JSON.stringify(ids.id)}]`).count() === 0, "The deleted chat disappears from the sidebar");
  check(!await page.locator("#profile-mode-wrap").isVisible(), "General chat hides the complete project-mode control including its label");
  await page.evaluate(async () => {
    window.__kzPreview.setCommand("softwire_questions", () => []);
    document.dispatchEvent(new CustomEvent("kz:refresh-work-questions"));
    await window.__kzPreview.settle();
  });
  await page.locator("#workbench-attention").click();
  await page.waitForFunction(() => document.body.dataset.view === "project");
  await page.evaluate(() => window.__kzPreview.settle());
  check(await page.evaluate(async ids => {
    const shell = await import("/03-shell.js");
    return shell.currentProject === ids.general && shell.activeSessionId === null;
  }, ids), "Opening project inbox preserves the general execution root and creates no conversation");
  check(!await page.locator("#project-space-name").innerText().then(text => text.includes("无项目对话")), "Project inbox header uses its browsing project rather than the general chat name");
  check(!await page.locator("#toast").innerText().then(text => text.includes("当前空间不提供")), "Needs attention opens from an empty general chat without a workspace error");
  await page.screenshot({ path: `${output}/inbox-from-general.png` });
  check(errors.length === 0, `No browser errors: ${errors.join("; ")}`);
  await writeFile(`${output}/checks.json`, JSON.stringify({ checks, errors }, null, 2));
} catch (error) {
  await page.screenshot({ path: `${output}/failure.png` });
  throw error;
} finally { await browser.close(); await server.close(); }
