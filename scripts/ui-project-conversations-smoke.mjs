/* global window, document, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
const output = "output/playwright/conversations";
await mkdir(output, { recursive: true });
const passed = [], errors = [];
const check = (value, text) => { assert(value, text); passed.push(text); };
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const count = cmd => page.evaluate(cmd => window.__kzPreview.calls.filter(c => c.cmd === cmd).length, cmd);
const last = cmd => page.evaluate(cmd => window.__kzPreview.calls.filter(c => c.cmd === cmd).at(-1)?.args, cmd);
page.on("pageerror", error => errors.push(error.message));
page.setDefaultTimeout(10000);
try {
  await page.goto(`${server.origin}/?scene=chat&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready); await settle();
  await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures();
    for (const p of f.state.processes) p.running = false;
    window.__conversationTest = { f, docs: f.commands.docs_snapshot(), captureFailure: false, readFailure: false };
    window.__kzPreview.setCommand("docs_snapshot", () => {
      if (window.__conversationTest.readFailure) throw "读取失败";
      return structuredClone(window.__conversationTest.docs);
    });
    window.__kzPreview.setCommand("run_prompt", args => {
      if (args.handoffSource && window.__conversationTest.handoffFailure) throw "来源对话仍在运行，请等回复完成后重新交接；结论已保留";
      return null;
    });
    await (await import("/09-sessions.js")).refreshProcesses();
    await (await import("/26-project-conversations.js")).refreshConversationWork();
  });
  const owner = await page.evaluate(async () => (await import("/03-workspaces.js")).selected_workspace_process());
  check(await page.locator("#view-chat").evaluate(el => el.classList.contains("active")), "Project enters a real conversation");
  check(await page.locator("#project-chat-work .work-focus-entry").count() > 0, "Conversation includes persisted requirement cards");
  check(!await page.locator("#project-onboarding").isVisible(), "Existing requirements never trigger new-project onboarding");
  check(await count("run_prompt") === 0, "Startup does not start a model run");
  if (await page.locator("#tasks-close").isVisible()) await page.locator("#tasks-close").click();
  await page.locator("#project-chat-work .work-focus-entry").first().waitFor({ state: "visible" });
  check(await page.locator("#project-chat-work").isVisible(), "Closing child details restores the visible requirement rail");
  await page.screenshot({ path: `${output}/conversation-dark.png` });
  await page.evaluate(async sid => {
    (await import("/06-agent-panel.js")).tasksPanelUserRun(sid);
  }, owner.session_id);
  await page.locator("#prompt").focus();
  await page.evaluate(sid => window.__kzPreview.emit("kz:tool-start", { sessionId: sid, id: "acceptance-scout", name: "task", input: { description: "批次勘察", prompt: "只读检查本批文件", phase: "scouting", role: "batch_scout" } }), owner.session_id);
  await settle();
  check(await page.locator("#tasks-panel").isVisible(), "A real task-start event opens the existing subagent panel");
  check(await page.locator("#prompt").evaluate(el => el === document.activeElement), "Automatic subagent panel does not steal typing focus");
  await page.locator("#tasks-close").click();
  await page.evaluate(sid => {
    window.__kzPreview.emit("kz:tool-end", { sessionId: sid, id: "acceptance-scout", name: "task", ok: true, preview: "文件依据已返回" });
    window.__kzPreview.emit("kz:tool-start", { sessionId: sid, id: "acceptance-review", name: "task", input: { description: "批次复核", prompt: "检查实现证据", phase: "review", role: "batch_reviewer" } });
  }, owner.session_id);
  await settle();
  check(!await page.locator("#tasks-panel").isVisible(), "Manual close suppresses another automatic opening in the same run");
  await page.evaluate(sid => { window.__kzPreview.emit("kz:tool-end", { sessionId: sid, id: "acceptance-review", name: "task", ok: true, preview: "NO_ISSUES" }); window.__kzPreview.emit("kz:idle", { sessionId: sid }); }, owner.session_id);
  await page.locator("#prompt").fill("主对话草稿保留");
  await page.locator('[data-work-surface="project"]').click(); await settle();
  check(await page.locator("#management-page").isVisible(), "Overview switch beside composer opens live workspace");
  check(await page.locator("#prompt").inputValue() === "主对话草稿保留", "Entering overview retains the same draft");
  check(!await page.locator("#prompt").isVisible(), "Management has no second conversation composer");
  await page.locator('[data-work-surface="chat"]').click(); await settle();
  await page.locator("#prompt").fill("概览补充的草稿");
  check(await page.locator("#prompt").inputValue() === "概览补充的草稿", "Returning to conversation retains overview edits");
  check(await page.locator("#project-chat-work").isVisible(), "Returning from overview keeps requirements visible on wide screens");
  await page.locator("#send").click(); await settle();
  check(await page.locator("#prompt").inputValue() === "", "Sending the restored main draft clears its editor");
  await page.locator('[data-work-surface="project"]').click(); await settle();
  check(await page.locator("#prompt").inputValue() === "", "Opening overview cannot resurrect a draft already sent from conversation");
  await page.locator('[data-work-surface="chat"]').click(); await settle();
  await page.evaluate(sid => window.__kzPreview.emit("kz:idle", { sessionId: sid }), owner.session_id);
  await page.locator("#prompt").fill("概览补充的草稿");
  await page.locator("#new-chat").click(); await settle();
  check((await last("process_create")).profile === "dev", "New conversation creates an ordinary development recipient");
  check(await count("conversation_clear") === 0, "New discussion never clears the main conversation");
  check(await page.locator("#project-conversation-kind").textContent().then(t => t.trim().length > 0), "Conversation title is visible");
  await page.locator("#composer-more").click();
  check(await page.locator("#auto-continue-wrap").isVisible(), "New conversation offers the same execution controls");
  check(await page.evaluate(async () => (await import("/03-workspaces.js")).selected_workspace_process().id) !== owner.id, "Selection follows the active peer conversation");
  await page.keyboard.press("Escape");
  await page.locator("#prompt").fill("讨论自己的草稿");
  const discussionId = await page.evaluate(async () => (await import("/03-shell.js")).activeProcessId);
  await page.locator('[data-work-surface="project"]').click(); await settle();
  check(await page.evaluate(async expected => (await import("/03-shell.js")).activeProcessId === expected, discussionId), "Overview preserves the selected peer conversation");
  await page.locator('[data-work-surface="chat"]').click(); await settle();
  await page.evaluate(async id => (await import("/09-sessions.js")).switchProcess(id), discussionId);
  check(await page.locator("#prompt").inputValue() === "讨论自己的草稿", "Discussion draft survives viewing main execution");
  await page.locator("#prompt").fill("只分析这个方案"); await page.locator("#send").click(); await settle();
  const peerRequest = await last("run_prompt");
  check(peerRequest.profile === "dev" && ["dev", "dev-pair"].includes(peerRequest.agent), `Conversation sends with its selected development mode: ${JSON.stringify(peerRequest)}`);
  await page.locator("#composer-more").click();
  await page.locator("#project-handoff").click();
  check((await page.locator(".project-handoff-form").innerText()).includes("将附上当前对话上下文"), "Handoff explains that source context accompanies the user's conclusion");
  await page.getByLabel("选择对话", { exact: true }).selectOption(owner.id);
  await page.getByLabel("交给其它对话的结论").fill("按讨论推进，先验收导出功能");
  await page.evaluate(() => { window.__conversationTest.handoffFailure = true; });
  await page.getByRole("button", { name: "发送给对话", exact: true }).click(); await settle();
  check(await page.getByLabel("交给其它对话的结论").inputValue() === "按讨论推进，先验收导出功能", "Rejected snapshot capture keeps the handoff conclusion intact");
  check((await page.locator(".project-form-error").innerText()).includes("来源对话仍在运行"), "Source snapshot failure remains visible in the handoff form");
  check(await page.evaluate(async expected => (await import("/03-shell.js")).activeProcessId === expected, discussionId), "Failed handoff cannot switch away from the source discussion");
  await page.evaluate(() => { window.__conversationTest.handoffFailure = false; });
  await page.getByRole("button", { name: "发送给对话", exact: true }).click(); await settle();
  const forwarded = await last("run_prompt");
  check(forwarded.processId === owner.id && forwarded.executionBatch === true && forwarded.delivery === "queue", "Handoff explicitly queues execution on the main conversation");
  check(forwarded.handoffSource?.processId === discussionId && forwarded.handoffSource?.projectDir === owner.project_dir, "Handoff passes the exact source discussion and project for backend snapshot capture");
  check(forwarded.prompt.includes("来自对话") && forwarded.prompt.endsWith("按讨论推进，先验收导出功能"), "Handoff preserves the user's conclusion independently of source context");
  check(await page.locator("#prompt").inputValue() === "概览补充的草稿", "Returning from discussion restores main draft");
  const beforeCard = await count("run_prompt");
  if (!await page.locator("#project-chat-work").isVisible()) await page.locator(".project-work-toggle").click();
  await page.locator("#project-chat-work .work-focus-entry").first().click(); await settle();
  check(await page.getByRole("button", { name: "继续此需求", exact: true }).isVisible(), "Requirement card opens detail with explicit continue action");
  check(await count("run_prompt") === beforeCard, "Inspecting a requirement does not start work");
  await page.getByRole("button", { name: "← 需求", exact: true }).click();
  await page.locator('[data-work-surface="chat"]').click(); await settle();
  await page.evaluate(async sid => { window.__kzPreview.emit("kz:idle", { sessionId: sid }); }, owner.session_id);
  await page.evaluate(async () => {
    window.__conversationTest.docs = { requirements: [], defects: [], archived: { req: 1, defect: 0 } };
    await (await import("/26-project-conversations.js")).refreshConversationWork();
  });
  check(!await page.locator("#project-onboarding").isVisible(), "Archived completed projects are never treated as newly empty projects");
  check(await page.getByRole("button", { name: "全部工作 ↗", exact: true }).isVisible(), "Completed work stays reachable through All work without adding a third chat slot");
  const beforeArchivedCapture = await count("quick_req");
  await page.locator("#prompt").fill("继续讨论下一步安排"); await page.locator("#send").click(); await settle();
  check(await count("quick_req") === beforeArchivedCapture && (await last("run_prompt")).prompt === "继续讨论下一步安排", "Normal messages after archiving remain conversation messages");
  await page.evaluate(sid => window.__kzPreview.emit("kz:idle", { sessionId: sid }), owner.session_id);
  await page.evaluate(async () => {
    const test = window.__conversationTest;
    test.docs = { requirements: [], defects: [] };
    window.__kzPreview.setCommand("quick_req", args => {
      if (test.captureFailure) throw "登记失败: 模型执行失败；PRIOR_ART_REQUIRED: 外部已有实现未完成；PRIOR_ART_REQUIRED: 外部已有实现未完成";
      const id = test.nextCaptureId || "R-999";
      test.docs.requirements.push({ id, title: args.description, status: "todo", priority: "P1", fields: [], closed: false,
        ...(test.captureResearch ? { prior_art: { status: "pending", path: ".kanzei/research/r1000/prior-art.md", issue: null } } : {}) });
      if (test.failAfterCapture) test.readFailure = true;
      return `${id} ${args.description}`;
    });
    await (await import("/26-project-conversations.js")).refreshConversationWork();
  });
  await settle();
  check(await page.locator("#project-onboarding").isVisible(), "Confirmed empty project asks for a requirement");
  await page.evaluate(async () => { window.__conversationTest.readFailure = true; await (await import("/26-project-conversations.js")).refreshConversationWork(); });
  check(!await page.locator("#project-register").isVisible(), "Failed requirement reads do not masquerade as an empty ready-to-start project");
  await page.evaluate(() => { window.__conversationTest.readFailure = false; });
  await page.locator("#project-onboarding-retry").click(); await settle();
  await page.screenshot({ path: `${output}/onboarding-dark.png` });
  await page.locator("#prompt").fill("建立一个支持导出的本地笔记工具");
  await page.evaluate(() => { window.__conversationTest.captureFailure = true; });
  const beforeCapture = await count("run_prompt");
  await page.locator("#project-register").click(); await settle();
  check(await page.locator("#prompt").inputValue() === "建立一个支持导出的本地笔记工具", "Failed registration retains original input");
  check(await count("run_prompt") === beforeCapture, "Failed registration never starts unowned work");
  check(await page.locator("#project-onboarding-retry").textContent() === "重试登记", "Registration failures explicitly offer retry registration");
  check(await page.locator("#project-onboarding-copy").textContent() === "登记未完成，原文已保留。", "Registration failures show a short actionable summary");
  check(!await page.locator("#project-registration-details").evaluate(el => el.open), "Full registration diagnostics are collapsed by default");
  await page.locator("#project-registration-details summary").click();
  check((await page.locator("#project-registration-detail-copy").textContent()).match(/PRIOR_ART_REQUIRED/g)?.length === 1, "Repeated registration diagnostics are deduplicated in expandable details");
  await page.evaluate(() => { window.__conversationTest.captureFailure = false; window.__conversationTest.failAfterCapture = true; });
  const failedCaptures = await count("quick_req");
  await page.locator("#project-onboarding-retry").click(); await settle();
  check(await count("quick_req") === failedCaptures + 1, "Retry registration reissues quick_req with the retained original description");
  check(await page.locator("#project-onboarding-retry").textContent() === "重试读取", "Only successful registration followed by failed readback offers retry reading");
  await page.locator("#project-register").click(); await settle();
  const captures = await count("quick_req");
  check(await count("run_prompt") === beforeCapture, "Successful capture with a failed readback cannot start an unconfirmed batch");
  await page.evaluate(() => { window.__conversationTest.readFailure = false; window.__conversationTest.failAfterCapture = false; });
  await page.locator("#project-onboarding-retry").click(); await settle();
  check(await page.locator("#project-register").textContent() === "开始此需求", "Recovered registration offers an explicit start for the existing requirement");
  await page.locator("#project-register").click(); await settle();
  check(await count("quick_req") === captures, "Readback retry reuses the persisted requirement instead of duplicating registration");
  check((await last("run_prompt")).prompt.includes("R-999") && (await last("run_prompt")).executionBatch && (await last("run_prompt")).workItemId === "R-999", "Persisted requirement ID is structurally bound to execution");
  check(!await page.locator("#project-onboarding").isVisible(), "Successful registration replaces onboarding with work");
  await page.evaluate(async sid => {
    window.__kzPreview.emit("kz:idle", { sessionId: sid });
    const test = window.__conversationTest;
    test.docs = { requirements: [], defects: [] }; test.captureResearch = true; test.nextCaptureId = "R-1000";
    test.f.state.fileDisk.disk.set(".kanzei/research/r1000/prior-art.md", { text: "# 先行调研\n待补齐外部与仓内对照", bom: false, note: "", mtime: 3 });
    await (await import("/26-project-conversations.js")).refreshConversationWork();
  }, owner.session_id);
  await page.locator("#prompt").fill("整理番剧收藏库，先调研已有实现");
  await page.locator("#project-register").click(); await settle();
  const preparation = await last("run_prompt");
  check(preparation.executionBatch === false && preparation.workItemId == null && preparation.prompt.includes("R-1000"), "Pending research starts in the main conversation without a premature claim");
  check(preparation.prompt.includes("原始描述") && preparation.prompt.includes("外部已有实现") && preparation.prompt.includes("仓内既有设计") && preparation.prompt.includes("检索预算") && preparation.prompt.includes("证据验证通过") && preparation.prompt.includes("待我处理") && !/req get|question|background=true|prior_art validate|work claim|doing/.test(preparation.prompt), "Preparation requests real evidence, validation and Needs attention in concise user-facing language");
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js");
    window.__kzPreview.emit("kz:idle", { sessionId: shell.activeSessionId });
    await (await import("/30-management.js")).openManagementItem(shell.currentProject, window.__conversationTest.docs.requirements[0]);
  }); await settle();
  check(await page.getByRole("button", { name: "开始调研", exact: true }).isVisible(), "Pending requirement detail has an explicit research action");
  await page.getByRole("button", { name: "查看调研文件", exact: true }).click(); await settle();
  check(await page.locator("#view-files").evaluate(el => el.classList.contains("active")) && (await last("file_preview")).path === ".kanzei/research/r1000/prior-art.md", "Research file action opens the real file editor on the selected artifact");
  await page.evaluate(async () => {
    const project = (await import("/03-shell.js")).currentProject;
    await (await import("/12-workbench.js")).openProjectSpace(project, "documents");
    await (await import("/14-docs-actions.js")).refreshDocs();
  }); await settle();
  const managed = page.locator('#documents-req-list .doc-item[data-doc-id="R-1000"]');
  check((await managed.locator(".doc-row").innerText()).includes("待调研"), "Pending research can be found from the full requirement management list");
  await managed.locator(".doc-row").click();
  await managed.getByRole("button", { name: "开始调研", exact: true }).click(); await settle();
  check((await last("run_prompt")).executionBatch === false && (await last("run_prompt")).workItemId == null, "Requirement management resumes preparation without claiming implementation");
  await page.evaluate(async sid => {
    window.__kzPreview.emit("kz:idle", { sessionId: sid });
    window.__conversationTest.docs.requirements[0].prior_art.status = "invalid";
    window.__conversationTest.docs.requirements[0].prior_art.issue = "仓内对照缺少出处";
    await (await import("/26-project-conversations.js")).refreshConversationWork();
  }, owner.session_id);
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js");
    await (await import("/30-management.js")).openManagementItem(shell.currentProject, window.__conversationTest.docs.requirements[0]);
  }); await settle();
  check(await page.getByRole("button", { name: "继续调研", exact: true }).isVisible() && (await page.locator(".management-body").innerText()).includes("仓内对照缺少出处"), "Invalid artifacts retain a follow-up action and their validation issue");
  await page.getByRole("button", { name: "继续调研", exact: true }).click(); await settle();
  check((await last("run_prompt")).workItemId == null, "Invalid research cannot take the implementation branch");
  for (const status of ["complete", "waived"]) {
    await page.evaluate(async ({ sid, status }) => {
      window.__kzPreview.emit("kz:idle", { sessionId: sid });
      window.__conversationTest.docs.requirements[0].prior_art.status = status;
      window.__conversationTest.docs.requirements[0].prior_art.issue = null;
      await (await import("/26-project-conversations.js")).refreshConversationWork();
    }, { sid: owner.session_id, status });
    await page.evaluate(async () => {
      const shell = await import("/03-shell.js");
      await (await import("/30-management.js")).openManagementItem(shell.currentProject, window.__conversationTest.docs.requirements[0]);
    }); await settle();
    await page.getByRole("button", { name: "继续此需求", exact: true }).click(); await settle();
    check((await last("run_prompt")).executionBatch === true && (await last("run_prompt")).workItemId === "R-1000", `${status} artifacts retain the existing explicit implementation binding`);
  }
  for (const width of [1280, 760, 390]) {
    await page.setViewportSize({ width, height: 920 }); await settle();
    check(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), `No horizontal page overflow at ${width}px`);
    check(await page.locator('#project-work-switch [data-work-surface="project"]').isVisible(), `Overview remains accessible at ${width}px`);
  }
  check(await page.locator("#project-chat-work").isVisible(), "Current work and next candidate remain visible on narrow screens");
  await page.screenshot({ path: `${output}/conversation-mobile.png` });
  check(errors.length === 0, `No browser errors: ${errors.join("; ")}`);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ passed, errors }, null, 2));
  console.log(`${passed.length} conversation checks passed`);
} catch (error) {
  console.error(await page.evaluate(() => [...document.querySelectorAll("body *")].filter(el => el.getBoundingClientRect().right > window.innerWidth + 1 && getComputedStyle(el).position !== "fixed").slice(-20).map(el => [el.id || el.className, Math.round(el.getBoundingClientRect().right)])));
  await page.screenshot({ path: `${output}/failure.png` }).catch(() => {});
  console.error({ passed, errors }); throw error;
} finally { await browser.close(); await server.close(); }
