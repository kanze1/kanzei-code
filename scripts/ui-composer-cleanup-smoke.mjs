// Real production UI with isolated IPC and clipboard, including cross-project ownership.
/* global window, document, navigator, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/composer-cleanup", checks = [], errors = [];
await mkdir(output, { recursive: true });
const check = (value, name) => { assert(value, name); checks.push(name); };
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
page.on("pageerror", error => errors.push(error.message));
try {
  await page.goto(`${server.origin}/?scene=chat`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async text => { window.__cleanupCopied = text; } } }));
  for (const id of ["side-question-open", "runtime-indicator", "terminal-monitor-open", "sop-picker", "sop-picker-panel", "worktree-add", "lines-add", "summarize-btn", "copy-context", "chat-search-toggle"]) {
    check(await page.locator(`#${id}`).count() === 0, `${id} is removed, including its hidden DOM`);
  }
  await page.locator("#composer-more").click();
  for (const id of ["native-desktop-choose", "profile-select", "auto-allow"]) check(await page.locator(`#${id}`).isVisible(), `${id} remains available`);
  check(await page.locator("#composer-more-menu").evaluate(el => el.scrollHeight <= el.clientHeight), "Normal execution options do not need scrolling");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+f");
  check(await page.locator("#chat-search").isVisible() && await page.locator("#chat-search-input").evaluate(el => el === document.activeElement), "Ctrl+F opens and focuses conversation search");
  await page.keyboard.press("Escape");
  check(!await page.locator("#chat-search").isVisible(), "Escape closes conversation search");

  const raw = "## 用户原文\n\n```js\nconst answer = 42;\n```";
  await page.evaluate(async raw => {
    const chat = await import("/05-chat-render.js");
    chat.addUserMessage(raw, [{ file_name: "example.csv", media_type: "text/csv" }]);
  }, raw);
  const userCopy = page.locator('.msg-pane[data-active="1"] .msg.user').last().locator(".copy-btn");
  await userCopy.click();
  await page.waitForFunction(raw => window.__cleanupCopied === raw, raw);
  check(await userCopy.getAttribute("data-copied") === "true", "Message copy preserves Markdown and excludes attachment controls with visible success feedback");
  await page.evaluate(async () => {
    const chat = await import("/05-chat-render.js");
    const message = chat.addMessage("assistant", "Rendered text");
    message.dataset.raw = "**Assistant Markdown**\n\n```text\nexact code\n```";
  });
  await page.locator('.msg-pane[data-active="1"] .msg.assistant').last().locator(".copy-btn").click();
  check((await page.evaluate(() => window.__cleanupCopied)).includes("```text\nexact code\n```"), "Assistant copy retains original code fences");

  const execution = await page.evaluate(async () => {
    const shell = await import("/03-shell.js");
    return { project: shell.currentProject, process: shell.activeProcessId };
  });
  const selected = await page.locator("#workbench-navigation .workbench-session").evaluateAll((rows, active) => rows.filter(el => el.getClientRects().length && el.dataset.processId && el.dataset.processId !== active).map(el => ({ id: el.dataset.processId, project: el.dataset.project }))[0], execution.process);
  check(Boolean(selected), "A different sidebar conversation is available for ownership verification");
  await page.evaluate(() => {
    window.__kzPreview.setCommand("conversation_display_get", args => [{ role: "user", parts: [{ type: "text", text: `EXPORT_SOURCE:${args.processId}` }] }]);
    window.__kzPreview.setCommand("save_conversation_export", () => "C:/fixture/export.md");
    window.__kzPreview.setCommand("summarize_chat", args => ({ summary: `SUMMARY_SOURCE\n${args.transcript}` }));
  });
  const openExport = async () => {
    await page.locator(`#workbench-navigation .workbench-session[data-process-id="${selected.id}"]`).first().click({ button: "right" });
    await page.getByRole("menuitem", { name: "导出…", exact: true }).click();
  };
  await openExport(); await page.getByRole("menuitem", { name: "复制为 Markdown", exact: true }).click();
  await page.waitForFunction(id => window.__cleanupCopied.includes(`EXPORT_SOURCE:${id}`), selected.id);
  check(!(await page.evaluate(() => window.__cleanupCopied)).includes(raw), "Sidebar export reads the selected conversation rather than the current DOM");
  await openExport(); await page.getByRole("menuitem", { name: "保存为 Markdown…", exact: true }).click();
  await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "save_conversation_export"));
  const saved = await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "save_conversation_export").at(-1).args);
  check(saved.content.includes(`EXPORT_SOURCE:${selected.id}`), "Markdown file export uses the same selected-conversation source");
  await openExport(); await page.getByRole("menuitem", { name: "生成摘要并导出…", exact: true }).click();
  await page.waitForFunction(() => window.__kzPreview.calls.filter(call => call.cmd === "save_conversation_export").length === 2);
  const summary = await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "save_conversation_export").at(-1).args);
  check(summary.content.includes("SUMMARY_SOURCE") && summary.content.includes(`EXPORT_SOURCE:${selected.id}`), "Summary export uses full selected-conversation data and saves through the export dialog");

  await page.locator('#workspace-sidebar-footer [data-view="memory"]').click();
  await page.locator("#memory-project-picker").waitFor();
  const otherProject = await page.locator("#memory-project-picker option").evaluateAll((options, current) => options.map(option => option.value).find(value => value !== "@global" && value !== current), execution.project);
  check(Boolean(otherProject), "Memory has a second project for source-isolation verification");
  await page.evaluate(async project => {
    const f = await window.__kzPreview.fixtures();
    window.__kzPreview.setCommand("process_list", args => args.projectDir === project
      ? [{ id: "d|dream-other", session_id: "dream-other", label: "做梦来源对话", profile: "dev-auto", running: false }]
      : f.commands.process_list(args));
    window.__kzPreview.setCommand("memory_dream", () => ({ report: { status: "completed" }, pending: 1 }));
  }, otherProject);
  await page.locator("#memory-project-picker").selectOption(otherProject);
  await page.waitForFunction(async project => (await import("/03-memory-scope.js")).memoryProject === project, otherProject);
  check(await page.locator(".memory-toolbar #memory-dream").isVisible(), "Dream keeps its name and sits next to memory consolidation");
  await page.locator("#memory-dream").click();
  await page.getByRole("menuitem", { name: "做梦来源对话", exact: true }).click();
  await page.waitForFunction(() => window.__kzPreview.calls.some(call => call.cmd === "memory_dream"));
  const dream = await page.evaluate(() => window.__kzPreview.calls.filter(call => call.cmd === "memory_dream").at(-1).args);
  check(dream.projectDir === otherProject && dream.processId === "d|dream-other", "Dream uses the explicitly chosen memory project and conversation");
  check(await page.evaluate(async expected => { const s = await import("/03-shell.js"); return s.currentProject === expected.project && s.activeProcessId === expected.process; }, execution), "Memory work and sidebar exports preserve the execution recipient");
  await page.screenshot({ path: `${output}/memory-dream.png` });
  await page.locator(".memory-toolbar").screenshot({ path: `${output}/dream-toolbar.png` });
  await page.goto(`${server.origin}/?scene=chat`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  for (const theme of ["light", "dark"]) for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.evaluate(theme => document.documentElement.setAttribute("data-theme", theme), theme);
    await page.locator("#composer-more").click();
    await page.evaluate(async () => {
      const finite = document.getAnimations().filter(animation => animation.effect.getTiming().iterations !== Infinity);
      await Promise.allSettled(finite.map(animation => animation.finished));
    });
    check(await page.locator("#composer-more-menu").evaluate(el => el.scrollHeight <= el.clientHeight), `${theme}/${width}: execution menu fits without scrolling`);
    await page.screenshot({ path: `${output}/menu-${theme}-${width}.png` });
    await page.keyboard.press("Escape");
  }
  check(errors.length === 0, `No browser exceptions: ${errors.join("; ")}`);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ checks, errors }, null, 2));
  console.log(`Composer cleanup PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path: `${output}/failure.png` });
  await writeFile(`${output}/failure.json`, JSON.stringify({ checks, errors, error: String(error) }, null, 2));
  throw error;
} finally { await browser.close(); await server.close(); }
