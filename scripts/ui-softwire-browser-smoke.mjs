// Production frontend, isolated IPC fixture. Actual Rust delivery is tested separately.
/* global window, document, innerWidth, innerHeight, getComputedStyle, InputEvent, CompositionEvent */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
import { sameProject, inboxFromProjects, conversationKey } from "../crates/kanzei-app/ui/25-softwire-model.js";

const output = path.resolve("output/playwright/softwire");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
page.setDefaultTimeout(9000);
const errors = [], passed = [];
page.on("pageerror", e => errors.push(e.message));
const check = (value, name) => { assert(value, name); passed.push(name); };
const settle = () => page.evaluate(() => window.__kzPreview.settle());
const last = cmd => page.evaluate(cmd => window.__kzPreview.calls.filter(c => c.cmd === cmd).at(-1)?.args, cmd);
const count = cmd => page.evaluate(cmd => window.__kzPreview.calls.filter(c => c.cmd === cmd).length, cmd);
const network = async () => { await page.locator('[data-work-surface="project"]').click(); await settle(); };
const module = async id => { await network(); if (id !== "main") await page.locator(`[data-module="${id}"]`).click(); await settle(); };
const refresh = async () => { await page.locator("#sw-refresh").click(); await settle(); };
const inbox = async () => { await page.locator('.sw-tabs [data-tab="inbox"]').click(); await settle(); };
try {
  check(sameProject('\\\\?\\C:\\demo\\', 'c:/demo'), "Windows extended paths normalize");
  check(conversationKey({project:"C:/a",module:"tools",sessionId:"s"}) === conversationKey({project:"C:/a",module:"main",sessionId:"s"}), "Management talks share their actual agent conversation");
  const versions = [1,2].map(revision => inboxFromProjects([{path:"C:/a",decisions:[{id:"d",revision}]}])[0].key);
  check(versions[0] !== versions[1], "A new decision revision has a distinct receipt identity");
  await page.goto(`${server.origin}/?scene=workspace&theme=light&keep=1`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  check(await count("conversation_display_get") > 0 && await count("run_prompt") === 0, "Startup restores conversation without starting execution");
  await page.locator(".workspace-card-open").first().click();
  await settle(); await page.locator('.workbench-project-link.active').click();
  await page.locator('[data-work-surface="project"]').click(); await page.locator("#softwire-workspace").waitFor(); await settle();
  const identity = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures();
    const snapshot = f.commands.workspace_snapshot();
    const a = snapshot.projects[0], b = snapshot.projects[1];
    window.__swTest = { snapshot, a, b, questions: [], questionFailure: false, runFailure: false };
    window.__kzPreview.setCommand("workspace_snapshot", () => structuredClone(window.__swTest.snapshot));
    window.__kzPreview.setCommand("softwire_questions", () => structuredClone(window.__swTest.questions));
    window.__kzPreview.setCommand("softwire_answer_question", args => {
      if (window.__swTest.questionFailure) throw "question_changed: 版本已更新";
      window.__swTest.questions = window.__swTest.questions.filter(q => q.id !== args.id || q.sessionId !== args.sessionId);
      return {...args,status:"delivered"};
    });
    window.__kzPreview.setCommand("run_prompt", () => { if (window.__swTest.runFailure) throw "连接中断"; return null; });
    return {a:a.path,b:b.path,process:a.lines[0].id,session:a.lines[0].session_id};
  });
  await refresh();
  await page.evaluate(sessionId => {
    window.__kzPreview.emit("kz:turn", { sessionId, turn: 1 });
    window.__kzPreview.emit("kz:status", { sessionId, stage: "执行" });
  }, identity.session); await settle();
  check((await page.locator("#sw-batch").textContent()).includes("R-379/W1"), "Current requirement and batch are projected together");
  check(await page.locator('#sw-runtime[data-state="running"]').count() === 1 && await page.locator('[data-module="work"][data-state="running"]').count() === 1, "A running line lights the header status and its requirement baseline");
  check(await page.locator(".sw-network .sw-lane").count() === 7 && await page.locator(".sw-network .sw-wave").count() === 6 && await page.locator(".sw-network svg.sw-spark").count() === 1, "Six module baselines and the usage lane are stacked, one straight line each");
  await page.screenshot({path:path.join(output,"workspace-1440-light.png")});
  await page.locator("#prompt").fill("主对话草稿");
  await module("memory");
  check(await page.locator("#prompt").inputValue() === "", "Memory draft is isolated from main draft");
  await page.locator("#prompt").fill("记忆草稿");
  await module("tools"); await page.locator("#prompt").fill("工具草稿");
  await module("memory");
  check(await page.locator("#prompt").inputValue() === "记忆草稿", "Module draft returns intact");
  await module("main");
  check(await page.locator("#prompt").inputValue() === "主对话草稿", "Main draft returns intact");
  const text = "  保留空格\n先检查当前实现  ";
  await page.locator("#prompt").fill(text); await page.locator("#send").click(); await settle();
  const sent = await last("run_prompt");
  check(sent.prompt === text && sent.projectDir === identity.a && sent.processId === identity.process, "Send preserves exact visible text and original recipient");
  check(await page.locator("#prompt").inputValue() === "", "Successful send clears only submitted draft");
  await page.evaluate(() => { window.__swTest.runFailure = true; });
  await page.locator("#prompt").fill("失败后保留的草稿"); await page.locator("#send").click(); await settle();
  check(await page.locator("#prompt").inputValue() === "失败后保留的草稿", "Failed send preserves draft");
  check((await page.locator("#sw-surface").textContent()).includes("连接中断"), "Failed send has a visible reason");
  await page.evaluate(() => { window.__swTest.runFailure = false; });
  await page.locator("#send").click(); await settle();
  await page.locator("#prompt").fill("运行时仍在写的草稿");
  await page.locator("#prompt").focus();
  const imeBefore = await count("run_prompt");
  await page.locator("#prompt").dispatchEvent("keydown", {key:"Enter",isComposing:true,bubbles:true});
  check(await count("run_prompt") === imeBefore, "IME confirmation Enter never sends an unfinished draft");
  await page.evaluate(session => {
    window.__swInput = document.querySelector("#prompt");
    window.__swInput.dispatchEvent(new CompositionEvent("compositionstart", {bubbles:true,data:"中"}));
    window.__kzPreview.emit("kz:text", {sessionId:session,text:"唯一流式回复"});
    window.__kzPreview.emit("kz:tool-start", {sessionId:session,id:"sw-tool",name:"read",summary:"docs/plan.md",input:{path:"docs/plan.md"}});
    window.__kzPreview.emit("kz:tool-end", {sessionId:session,id:"sw-tool",name:"read",ok:true,content:"工具证据"});
  }, identity.session);
  await settle();
  check(await page.evaluate(() => document.querySelector("#prompt") === window.__swInput && document.activeElement === window.__swInput), "Background signals preserve editor node and IME focus");
  check(await page.locator("#prompt").inputValue() === "运行时仍在写的草稿", "Background signals preserve unsent text");
  await page.evaluate(() => window.__swInput.dispatchEvent(new CompositionEvent("compositionend", {bubbles:true,data:"中"})));
  await page.keyboard.press("Control+Shift+C"); await settle();
  const stopped = await last("stop_run");
  check(stopped.projectDir === identity.a && stopped.processId === identity.process, "Stop shortcut addresses the visible receiver");
  await module("tools"); await page.getByRole("button", {name:/read · docs\/plan.md/}).click();
  check((await page.locator(".sw-evidence").textContent()).includes("工具证据"), "Tool details contain the actual result");
  await page.screenshot({path:path.join(output,"tool-evidence.png")});
  await module("main");
  check(await page.locator(".sw-recent-reply").filter({hasText:"唯一流式回复"}).count() === 1, "A tool-management view does not duplicate streamed replies");
  await page.locator("#prompt").fill("处理消息前的草稿");
  await page.evaluate(async () => { (await import("/03-shell.js")).navigate_view("chat"); }); await settle();
  check(await page.locator("body").getAttribute("data-view") === "chat", "The pending foreground question starts in the original conversation surface");
  const foregroundBefore = await count("softwire_answer_question");
  await page.evaluate(sessionId => {
    const t = window.__swTest;
    t.questions = [
      {id:69,projectDir:t.a.path,sessionId,revision:"v1",question:"后台调研问题",background:true,options:[]},
      {id:70,projectDir:t.a.path,sessionId,revision:"v1",question:"当前对话需要你的回答",background:false,options:["继续调研","补充需求"]},
    ];
    window.__kzPreview.emit("kz:ask", {...t.questions[1],kind:"question"});
  }, identity.session);
  await settle();
  check(await page.locator(".sw-question").innerText() === "当前对话需要你的回答" && await page.getByRole("heading",{name:"回复提问",exact:true}).isVisible(), "A foreground question automatically opens its visible reply detail");
  check(await count("softwire_answer_question") === foregroundBefore && await page.locator("#prompt").inputValue() === "", "Opening a question never invents an answer or reuses the conversation draft");
  await page.locator(".sw-detail-head .sw-back").click(); await settle();
  check(await page.locator("body").getAttribute("data-view") === "chat" && await page.locator("#prompt").inputValue() === "处理消息前的草稿", "Closing an automatic question returns to its original native conversation and draft");
  await network(); await refresh();
  check(await page.locator(".sw-question").count() === 0 && await page.locator("#prompt").inputValue() === "处理消息前的草稿", "Closing a question restores the draft and polling does not reopen the same revision");
  await page.evaluate(() => {
    const t = window.__swTest;
    window.__kzPreview.emit("kz:ask", {...t.questions[0],kind:"question"});
  }); await settle();
  check(await page.locator(".sw-question").count() === 0 && await page.locator("#prompt").inputValue() === "处理消息前的草稿", "A background question in the active conversation leaves the current work and draft in place");
  await page.evaluate(() => { window.__swTest.questions[1].revision = "v2"; }); await refresh();
  check(await page.locator(".sw-question").innerText() === "当前对话需要你的回答", "Fetching a pending foreground question recovers its visible reply detail without a live ask event");
  await page.locator(".sw-choices").getByRole("button",{name:"继续调研",exact:true}).click(); await settle();
  const foregroundReply = await last("softwire_answer_question");
  check(foregroundReply.projectDir === identity.a && foregroundReply.sessionId === identity.session && foregroundReply.expectedRevision === "v2" && foregroundReply.reply === "继续调研", "Automatic foreground reply preserves the original project, session, revision and chosen answer");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  check(await page.locator("#prompt").inputValue() === "处理消息前的草稿", "Completing the foreground reply restores the conversation draft");
  await page.evaluate(async () => { const t = window.__swTest; await (await import("/12-workbench.js")).openProjectSpace(t.b.path,"chat"); }); await settle();
  await page.evaluate(sessionId => {
    const t = window.__swTest;
    t.questions = [{id:73,projectDir:t.a.path,sessionId,revision:"v1",question:"切回主对话后恢复的提问",options:["已恢复"]}];
    window.__kzPreview.emit("kz:ask", {...t.questions[0],kind:"question"});
  }, identity.session); await settle();
  check(await page.locator("body").getAttribute("data-view") === "chat" && await page.locator(".sw-question").isVisible() === false, "A foreground question owned by another conversation does not interrupt the active conversation");
  await page.evaluate(async () => { const t = window.__swTest; await (await import("/12-workbench.js")).openProjectSpace(t.a.path,"chat"); }); await settle();
  check(await page.locator(".sw-question").innerText() === "切回主对话后恢复的提问", "Switching back to the original conversation presents its waiting foreground question");
  await page.locator(".sw-choices").getByRole("button",{name:"已恢复",exact:true}).click(); await settle();
  check((await last("softwire_answer_question")).sessionId === identity.session, "Recovered-question replies stay bound to the original session");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  const discussion = await page.evaluate(async () => {
    const t = window.__swTest, core = await import("/01-core.js"), sessions = await import("/09-sessions.js");
    const discussion = await core.invoke("process_create", {projectDir:t.a.path,profile:"readonly",phasePipeline:false});
    t.a.lines.push(discussion);
    await sessions.refreshProcesses(); await sessions.switchProcess(discussion.id);
    return discussion;
  }); await settle();
  await page.locator("#prompt").fill("讨论里还没发出的草稿");
  await page.evaluate(discussion => {
    const t = window.__swTest;
    t.questions = [{id:74,projectDir:t.a.path,sessionId:discussion.session_id,revision:"v1",question:"讨论还需要补充什么？",options:["验收边界"]}];
    window.__kzPreview.emit("kz:ask", {...t.questions[0],kind:"question"});
  }, discussion); await settle();
  check(await page.locator(".sw-question").innerText() === "讨论还需要补充什么？", "A foreground question from a readonly discussion appears despite the overview excluding its work selection");
  check(await page.evaluate(async discussionId => (await import("/03-shell.js")).activeProcessId === discussionId, discussion.id), "Displaying a discussion question keeps its original active discussion");
  await page.locator(".sw-detail-head .sw-back").click(); await settle();
  check(await page.locator("body").getAttribute("data-view") === "chat" && await page.locator("#prompt").inputValue() === "讨论里还没发出的草稿", "Closing a discussion question returns to the original discussion draft");
  await page.evaluate(() => { window.__kzPreview.emit("kz:ask", {...window.__swTest.questions[0],kind:"question"}); }); await settle();
  check(await page.locator("body").getAttribute("data-view") === "chat", "A closed discussion question does not repeatedly interrupt its conversation");
  await page.locator("#workbench-attention").click(); await settle(); await page.locator(".sw-inbox-row").filter({hasText:"讨论还需要补充什么"}).click();
  await page.locator(".sw-choices").getByRole("button",{name:"验收边界",exact:true}).click(); await settle();
  check((await last("softwire_answer_question")).sessionId === discussion.session_id && (await last("softwire_answer_question")).projectDir === identity.a, "A reopened discussion question delivers its answer only to the original discussion");
  await page.getByRole("button",{name:"返回工作",exact:true}).click(); await settle();
  check(await page.locator("body").getAttribute("data-view") === "chat" && await page.locator("#prompt").inputValue() === "讨论里还没发出的草稿", "Completing a reopened discussion question returns to that discussion with its draft intact");
  await page.evaluate(async discussion => {
    const t = window.__swTest;
    t.questions = [{id:75,projectDir:t.a.path,sessionId:discussion.session_id,revision:"v1",question:"恢复讨论的待答问题",options:["恢复回答"]}];
    await (await import("/12-workbench.js")).openProjectSpace(t.a.path,"project");
  }, discussion); await settle();
  check(await page.locator(".sw-question").innerText() === "恢复讨论的待答问题", "An overview refresh recovers a discussion's pending foreground question without a live ask event");
  await page.locator(".sw-choices").getByRole("button",{name:"恢复回答",exact:true}).click(); await settle();
  await page.getByRole("button",{name:"返回工作",exact:true}).click(); await settle();
  check(await page.locator("body").getAttribute("data-view") === "chat" && await page.locator("#prompt").inputValue() === "讨论里还没发出的草稿", "Restored discussion questions preserve their native discussion draft on return");
  await page.evaluate(async processId => { await (await import("/09-sessions.js")).switchProcess(processId); }, identity.process); await settle();
  await page.locator("#prompt").fill("处理消息前的草稿"); await network();
  await page.evaluate(() => {
    const t = window.__swTest;
    t.questions = [{id:71,projectDir:t.b.path,sessionId:"question-session-b",revision:"v1",question:"先检查界面还是运行？",options:["界面","运行"]}];
    window.__kzPreview.emit("kz:ask", {...t.questions[0],kind:"question"});
  });
  await settle();
  check(await page.locator("#ask-overlay").evaluate(el => !el.open), "Incoming question does not open a modal");
  check(await page.locator("#prompt").inputValue() === "处理消息前的草稿", "Incoming question does not steal draft");
  await inbox(); await page.locator(".sw-inbox-row").filter({hasText:"先检查界面还是运行"}).click();
  const before = await count("softwire_answer_question");
  await page.getByRole("button",{name:"补充说明",exact:true}).click();
  await page.locator(".sw-choices").getByRole("button",{name:"界面",exact:true}).click();
  check(await count("softwire_answer_question") === before && await page.locator("#prompt").inputValue() === "界面", "Add a note keeps the choice as a visible draft until explicitly sent");
  await page.locator("#prompt").fill("  界面\n再检查运行  "); await page.locator("#send").click(); await settle();
  const reply = await last("softwire_answer_question");
  check(reply.projectDir === identity.b && reply.sessionId === "question-session-b" && reply.expectedRevision === "v1" && reply.reply === "  界面\n再检查运行  ", "Cross-project reply binds the original question and exact revision");
  check(await page.locator('.sw-reply-complete').isVisible(), "Delivery receipt replaces reply controls");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  check(await page.locator("#prompt").inputValue() === "处理消息前的草稿", "Return restores original draft");
  await page.evaluate(() => { const t = window.__swTest; t.questions = [{id:72,projectDir:t.a.path,sessionId:"question-a",revision:"v1",question:"版本检查",options:[]}]; t.questionFailure=true; });
  await inbox(); await page.locator(".sw-inbox-row").filter({hasText:"版本检查"}).click();
  await page.locator("#prompt").fill("需要保留的回复"); await page.locator("#send").click(); await settle();
  check(await page.locator("#prompt").inputValue() === "需要保留的回复" && await page.getByRole("button",{name:"查看最新事项"}).isVisible(), "Stale question retains text and offers explicit refresh");
  await page.evaluate(() => {window.__swTest.questions[0].revision="v2";window.__swTest.questionFailure=false;});
  await page.getByRole("button",{name:"查看最新事项"}).click(); await settle();
  check(await page.locator("#prompt").inputValue() === "需要保留的回复", "Explicit revision refresh preserves reply draft");
  await page.locator("#send").click(); await settle();
  check((await last("softwire_answer_question")).expectedRevision === "v2", "Retry uses the newly displayed revision");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  // needs_input is persisted as a decision but must be answered like a question.
  await page.evaluate(() => {
    const t=window.__swTest, line=t.b.lines[0];
    t.fact={id:"missing-layout",revision:41,status:"needs_input",session_id:line.session_id,process_id:line.id,
      question:"Agent 入口如何安排？",missing_fact:"需要明确底部导航布局",review:null,resolution:null,
      options:[{label:"新增第六项 Agent",note:"保留原五项"},{label:"Agent 替换 Archive",note:"Archive 迁入其他位置"},{label:"其他布局",note:"请说明具体布局"}]};
    t.b.decisions.push(t.fact);
    window.__kzPreview.setCommand("decision_review", args=>{
      const d=window.__swTest.fact;
      if(args.decisionId!==d.id || args.expectedRevision!==undefined || args.review.expected_revision!==d.revision) throw "original fact identity lost";
      if(d.status==="needs_input" && args.review.action==="accept") throw "invalid store input: missing facts require a reply";
      d.review=args.review;
      return {decision:structuredClone(d),delivery:args.review.action==="correct"?{status:"started",error:null}:null};
    });
  });
  await inbox();
  check((await page.locator(".sw-inbox-row").filter({hasText:"Agent 入口如何安排"}).innerText()).includes("待你回复"), "A missing fact is labeled awaiting reply, not awaiting review");
  await page.locator(".sw-inbox-row").filter({hasText:"Agent 入口如何安排"}).click();
  check(await page.getByRole("heading",{name:"回复提问",exact:true}).isVisible() && await page.getByRole("button",{name:"本次通过",exact:true}).count()===0, "Missing-input detail removes the invalid approval control");
  check(await page.locator("[data-reply-choice]").count()===3 && (await page.locator(".sw-choice-note").first().innerText())==="保留原五项", "Stored original options and notes are restored");
  await page.screenshot({path:path.join(output,"missing-fact-options.png")});
  const factBefore=await count("decision_review");
  await page.locator("#prompt").fill("本次通过"); await page.locator("#send").click(); await settle();
  check(await count("decision_review")===factBefore && (await page.locator(".sw-interaction-receipt").innerText()).includes("这条消息需要具体回答"), "An old approval draft gives actionable feedback without being sent as a fact");
  await page.getByRole("button",{name:"新增第六项 Agent",exact:true}).evaluate(el=>{el.click();el.click();}); await settle();
  const factReply=await last("decision_review");
  check(await count("decision_review")===factBefore+1 && factReply.projectDir===identity.b && factReply.decisionId==="missing-layout" && factReply.review.expected_revision===41 && factReply.review.action==="correct" && factReply.review.feedback==="新增第六项 Agent", "A real option replaces the legacy placeholder and replies once to the original project and revision");
  check((await page.locator(".sw-interaction-receipt").innerText()).includes("已恢复原对话"), "Missing-input reply shows actual resumption");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  await page.evaluate(()=>{const d=window.__swTest.fact;d.revision++;d.review=null;});
  await inbox(); await page.locator(".sw-inbox-row").filter({hasText:"Agent 入口如何安排"}).click();
  const customBefore=await count("decision_review");
  await page.getByRole("button",{name:"其他布局",exact:true}).click(); await settle();
  check(await count("decision_review")===customBefore && await page.locator("#prompt").inputValue()==="其他布局", "An option explicitly requesting an explanation opens the draft before sending");
  await page.locator("#prompt").fill("其他布局\n保留五项，Agent 放入顶部入口。"); await page.locator("#send").click(); await settle();
  check((await last("decision_review")).review.feedback==="其他布局\n保留五项，Agent 放入顶部入口。", "Custom layout and explanation are delivered verbatim");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  await page.evaluate(()=>{const d=window.__swTest.fact;d.revision++;d.review=null;d.options=[];});
  await inbox(); await page.locator(".sw-inbox-row").filter({hasText:"Agent 入口如何安排"}).click();
  check(await page.locator("[data-reply-choice]").count()===0 && (await page.locator("#prompt").getAttribute("placeholder")).includes("填写你的选择"), "A question with no options offers free-text reply without inventing approval");
  await page.locator("#prompt").fill("将 Agent 放到第六项，原五项不动。"); await page.locator("#send").click(); await settle();
  check((await last("decision_review")).review.action==="correct", "Free-text missing-fact reply uses the response channel");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  await page.evaluate(()=>{const d=window.__swTest.fact;d.revision++;d.review=null;d.status="decided";d.resolution={answer:"新增第六项 Agent",rationale:"保留原五项",impact:"导航新增入口"};d.missing_fact=null;});
  await inbox(); await page.locator(".sw-inbox-row").filter({hasText:"Agent 入口如何安排"}).click();
  check(await page.getByRole("heading",{name:"复核决定",exact:true}).isVisible(), "An actual decided item retains the review flow");
  await page.getByRole("button",{name:"本次通过",exact:true}).click(); await settle();
  check((await last("decision_review")).review.action==="accept" && (await last("decision_review")).review.feedback==="", "Approval remains available only for an actual decision");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  await inbox(); await page.locator(".sw-inbox-row").filter({hasText:"普通选择需要停下来问用户"}).click();
  await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures();
    window.__swTest.wakeFailed = false;
    window.__kzPreview.setCommand("decision_review", args => {
      const response = f.commands.decision_review(args);
      response.delivery = { ...response.delivery, status: window.__swTest.wakeFailed ? "started" : "saved",
        error: window.__swTest.wakeFailed ? null : "原执行环境暂时不可用" };
      window.__swTest.wakeFailed = true;
      return response;
    });
  });
  await page.locator("#prompt").fill("不要每次停下来，按批次验证"); await page.locator("#send").click(); await settle();
  const failedReply = await last("decision_review");
  check(await page.locator("#prompt").inputValue() === "不要每次停下来，按批次验证" && (await page.locator("#sw-surface").innerText()).includes("回复已保存，恢复执行失败"), "A saved reply with a failed wake keeps its draft and exposes the failure");
  await page.locator("#send").click(); await settle();
  const decision = await last("decision_review");
  check(decision.review.request_id === failedReply.review.request_id && (await page.locator(".sw-interaction-receipt").innerText()).includes("已恢复原对话"), "Retry reuses the saved input identity and shows actual resumption");
  check(decision.projectDir === identity.a && decision.review.action === "correct" && decision.review.scope === "once" && decision.review.feedback === "不要每次停下来，按批次验证", "Decision correction preserves exact feedback with one-time scope");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  // B28/C10:概览「记忆」小窗只做入口。条目列表、搜索、整理与管理对话只在记忆页,概览里没有自带的列表与传输。
  await module("memory");
  check(await page.getByRole("button",{name:"打开记忆页",exact:true}).isVisible() && await page.locator("#sw-surface .sw-evidence-link").count() === 0, "The overview memory window is an entry only, with no entry list of its own");
  check(await count("memory_chat_send") === 0 && await count("memory_chat_history") === 0, "The overview carries no memory chat transport; the memory page owns the management chat");
  await page.getByRole("button",{name:"打开记忆页",exact:true}).click(); await settle();
  check(await page.locator("body").getAttribute("data-view") === "memory", "The memory window's only button opens the memory page");
  await page.evaluate(async project => (await import("/12-workbench.js")).openProjectSpace(project, "project"), identity.a); await settle();
  // 记忆小窗下面的输入框与别的模块一样发给主对话(run_prompt),不再有独立的记忆管理传输。
  await module("memory"); await page.locator("#prompt").fill("从记忆小窗发出的话"); await page.locator("#send").click(); await settle();
  check((await last("run_prompt")).prompt === "从记忆小窗发出的话" && await count("memory_chat_send") === 0, "A message typed under the memory window goes to the main conversation, not a memory transport");
  await module("main"); await page.locator("#prompt").fill("悬浮草稿");
  await page.getByRole("button",{name:"悬浮对话框",exact:true}).click();
  await page.locator(".sw-drag").focus(); await page.keyboard.press("Shift+ArrowRight");
  check(await page.locator("#composer.sw-floating").count() === 1, "Same editor can float");
  const drag = await page.locator(".sw-drag").boundingBox();
  await page.mouse.move(drag.x+3,drag.y+3); await page.mouse.down(); await page.mouse.move(1900,-100,{steps:5}); await page.mouse.up();
  check(await page.locator("#composer").evaluate(el => {const b=el.getBoundingClientRect();return b.x>=0 && b.y>=0 && b.right<=innerWidth && b.bottom<=innerHeight;}), "Drag is clamped inside viewport");
  await page.getByRole("button",{name:"停靠对话框",exact:true}).click();
  check(await page.locator("#prompt").inputValue() === "悬浮草稿", "Docking preserves text");
  // 任务设置菜单已撤(B22):「自动放行」是随输入框搬进概览输入卡的直接开关,不再有「⚙ 设置」弹层。
  check(await page.locator("#composer .sw-composer-controls #auto-allow-wrap").isVisible(), "Auto-allow switch travels with the actual composer");
  check(await page.locator("#task-options-menu").count() === 0 && await page.getByRole("button",{name:"⚙ 设置",exact:true}).count() === 0, "The task settings popover is gone");
  await page.locator("#auto-allow-wrap").click(); await settle();
  check(await page.locator("#prompt").inputValue() === "悬浮草稿", "Toggling auto-allow does not change draft");
  await page.locator("#auto-allow-wrap").click();
  // A late accepted request must not erase newer edits after the user leaves its module.
  await page.evaluate(() => window.__kzPreview.setCommand("run_prompt", () => new Promise(resolve => {window.__swRelease = resolve;})));
  await page.locator("#prompt").fill("已提交的旧稿"); await page.locator("#send").click();
  await page.waitForFunction(() => Boolean(window.__swRelease));
  await page.locator("#prompt").fill("请求期间补写的新稿"); await module("memory");
  await page.evaluate(() => {window.__swRelease(null);window.__kzPreview.setCommand("run_prompt",null);}); await settle();
  await module("main");
  check(await page.locator("#prompt").inputValue() === "请求期间补写的新稿", "Late success preserves a newer offscreen draft");
  await module("main");
  const beforeDelivery = await count("run_prompt");
  await page.evaluate(() => {
    const t=window.__swTest, u=t.a.work_units[0];
    u.status="done"; u.source_sequence+=1; u.last_checkpoint={summary:"本批测试通过",next_action:"等待试用"};
    t.a.decisions=[];
  });
  await refresh();
  check((await page.locator("#sw-checkpoint").textContent()).includes("已保存检查点"), "Checkpoint uses the persisted last_checkpoint field");
  await inbox(); await page.locator(".sw-inbox-row").filter({hasText:"自主决策与批量复核接入"}).click();
  await page.locator("#prompt").fill("请把右侧按钮再放大一点"); await page.locator("#send").click(); await settle();
  const feedback=await last("run_prompt");
  check(await count("run_prompt")===beforeDelivery+1 && feedback.prompt==="请把右侧按钮再放大一点" && feedback.processId===identity.process, "Delivery feedback reaches the original executor without rewriting it");
  check(await count("work_delivery_accept")===0, "Modification feedback does not mark a delivery accepted");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  await page.evaluate(() => {window.__swTest.a.work_units[0].source_sequence+=1;});
  await inbox(); await page.locator(".sw-inbox-row").filter({hasText:"自主决策与批量复核接入"}).click();
  await page.evaluate(() => window.__kzPreview.setCommand("work_delivery_accept", args=>({...args,accepted_at:Date.now()})));
  await page.getByRole("button",{name:"验收通过",exact:true}).click(); await settle();
  check((await last("work_delivery_accept")).sourceSequence===10, "Acceptance is bound to the displayed delivery version");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  // Live child details reuse the existing renderer, without a hidden main-agent reroute.
  await page.evaluate(session=>window.__kzPreview.emit("kz:tool-start",{sessionId:session,id:"sw-child",name:"task",summary:"核对交付",input:{description:"核对交付",prompt:"核对这批代码的证据"}}),identity.session);
  await module("tasks"); await page.getByRole("button",{name:/核对交付 ·/}).click();
  check((await page.locator("#sw-surface").textContent()).includes("核对这批代码的证据"), "Child task opens its actual instruction and result view");
  check(await page.locator("#prompt").isDisabled(), "Unsupported child resume cannot silently send to main");
  await module("main"); await page.locator("#prompt").fill("刷新后恢复草稿");
  await page.getByRole("button",{name:"悬浮对话框",exact:true}).click();
  await page.reload(); await page.waitForFunction(()=>window.__kzPreview?.ready);
  await page.locator(".workspace-card-open").first().click(); await settle();
  await page.locator('.workbench-project-link.active').click(); await settle();
  await page.locator('[data-work-surface="project"]').click(); await settle();
  check(await page.locator("#prompt").inputValue()==="刷新后恢复草稿", "Reload restores the receiver's draft");
  check(await page.locator("#composer.sw-floating").count()===1, "Reload restores floating position and mode");
  await page.getByRole("button",{name:"停靠对话框",exact:true}).click();
  await network();
  for (const width of [1024,390,320]) {
    await page.setViewportSize({width,height:850}); await settle();
    check(await page.locator("#softwire-workspace").evaluate(el => el.scrollWidth<=el.clientWidth+1), `${width}px workspace has no horizontal overflow`);
    check(await page.locator("#send").evaluate(el=>{const b=el.getBoundingClientRect();return b.x>=0 && b.right<=innerWidth && b.bottom<=innerHeight;}), `${width}px send remains reachable`);
    if (width<760) check(await page.locator("#composer.sw-floating").count() === 0, `${width}px editor is docked`);
    await page.screenshot({path:path.join(output,`workspace-${width}-light.png`)});
  }
  await page.setViewportSize({width:1440,height:960});
  await page.emulateMedia({reducedMotion:"reduce"});
  check(await page.locator('[data-module="work"] .sw-wave').evaluate(el=>getComputedStyle(el,"::after").animationName==="none"), "Reduced motion stops the baseline wave animation");
  await page.evaluate(() => document.documentElement.setAttribute("data-theme","dark"));
  await page.screenshot({path:path.join(output,"workspace-1440-dark.png")});
  // Fresh projects have no session in the overview yet. The first explicit send
  // resolves the real default receiver without dropping its first streamed answer.
  await page.goto(`${server.origin}/?scene=workspace&theme=light`);
  await page.waitForFunction(()=>window.__kzPreview?.ready);
  await page.evaluate(async()=>{
    const f=await window.__kzPreview.fixtures();
    const withoutLines=value=>({...value,projects:value.projects.map(p=>({...p,lines:[],running_lines:0}))});
    window.__kzPreview.setCommand("workspace_overview",()=>withoutLines(f.commands.workspace_overview()));
    window.__kzPreview.setCommand("workspace_snapshot",()=>withoutLines(f.commands.workspace_snapshot()));
    window.__kzPreview.setCommand("run_prompt",(_args,ctx)=>{ctx.emit("kz:text",{sessionId:f.ids.mainSession,text:"新会话第一条回复"});return null;});
    await (await import("/12-docs-pages.js")).refreshWorkspace();
  });
  await page.locator(".workspace-card-open").first().click(); await settle();
  await page.locator('.workbench-project-link.active').click(); await settle();
  await module("memory"); await page.locator("#prompt").fill("创建主对话前的记忆草稿");
  await module("main"); await page.locator("#prompt").fill("首次发送"); await page.locator("#send").click(); await settle();
  check(Boolean((await last("run_prompt")).processId), "First send resolves the real default process");
  check((await page.locator("#sw-surface").textContent()).includes("新会话第一条回复"), "First-session streamed answer reaches the new conversation");
  check(await page.locator("#prompt").inputValue()==="" && !await page.locator("#send").isDisabled(), "First-session send clears once and releases its sending lock");
  await module("memory");
  check(await page.locator("#prompt").inputValue()==="创建主对话前的记忆草稿", "Memory identity is independent of main session creation");
  check(errors.length===0, `No browser errors: ${errors.join("; ")}`);
  await writeFile(path.join(output,"acceptance.json"),JSON.stringify({passed:passed.length,checks:passed,errors,boundary:"Production frontend with mocked IPC; no real model run."},null,2));
  console.log(`Softwire browser PASS: ${passed.length} checks`);
} catch (error) {
  await page.screenshot({path:path.join(output,"failure.png")});
  console.error("Passed before failure:",passed.length,passed.at(-1));
  throw error;
} finally {await browser.close();await server.close();}
