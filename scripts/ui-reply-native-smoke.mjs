// Actual Tauri scheduler + a local, deterministic model. No user profile or external model.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Usage: node scripts/ui-reply-native-smoke.mjs <fresh kzapp.exe>");
const output = path.resolve("output/playwright/softwire/reply-native"), run = path.join(output, String(Date.now()));
const project = path.join(run,"project"), profile = path.join(run,"profile"), appHome = path.join(profile,".kanzei");
for (const dir of [project,appHome,path.join(run,"webview"),path.join(profile,"AppData/Local"),path.join(profile,"AppData/Roaming")]) await mkdir(dir,{recursive:true});
await writeFile(path.join(appHome,"app.json"),JSON.stringify({projects:[],current:null,theme:"dark"}));
const requests = [], checks = [], errors = [];
let releaseReply;
const answer = (res,content,tool) => {
  if (res.writableEnded || res.destroyed) return;
  res.writeHead(200,{"Content-Type":"text/event-stream"});
  const delta = tool ? {tool_calls:[{index:0,id:"native-question",type:"function",function:{name:"question",arguments:JSON.stringify(tool)}}]} : {content};
  res.write(`data: ${JSON.stringify({id:"local-test",object:"chat.completion.chunk",choices:[{index:0,delta,finish_reason:null}]})}\n\n`);
  res.write(`data: ${JSON.stringify({choices:[{index:0,delta:{},finish_reason:tool?"tool_calls":"stop"}],usage:{prompt_tokens:100,completion_tokens:20}})}\n\n`);
  res.end("data: [DONE]\n\n");
};
const model = http.createServer(async (req,res) => {
  let body=""; for await (const part of req) body+=part;
  if (req.url !== "/v1/chat/completions") {res.writeHead(404);res.end();return;}
  const data=JSON.parse(body); requests.push(data);
  const latest=[...(data.messages||[])].reverse().find(m=>m.role==="user");
  const text=JSON.stringify(latest?.content||"");
  if (text.includes("直接选择验收")) {
    const tail=data.messages.slice(data.messages.findLastIndex(m=>m.role==="user")+1);
    if (tail.some(m=>m.role==="tool")) answer(res,"收到选择：检查界面。直接回复已经继续执行。");
    else answer(res,null,{question:"直接选择验收：先检查什么？",options:[{label:"检查界面",note:"只验收当前界面和状态"},"检查运行"]});
  } else if (text.includes("第二条补充事实")) answer(res,"已处理第二条补充事实，验收结束。");
  else if (text.includes("仅验收现有版本")) releaseReply=()=>answer(res,"已收到：仅验收现有版本。回复已执行，验收结束。");
  else if ((data.messages||[]).some(m=>m.role==="tool")) answer(res,"需要你补充验收范围，等待你的回答。");
  else answer(res,null,{question:"请补充本次验收范围",missing_fact:"需要用户给出验收范围",
    options:[{label:"仅验收现有版本",note:"保留当前环境，不安装额外依赖"},"需要重新验证"]});
});
await new Promise(resolve=>model.listen(0,"127.0.0.1",resolve));
const modelPort=model.address().port;
await writeFile(path.join(appHome,"kanzei.toml"),`[models]\nprimary = "stub:test"\nfast = "stub:test"\n\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${modelPort}/v1"\ncontext_limit = 64000\n`);
const probe=net.createServer(); await new Promise(resolve=>probe.listen(0,"127.0.0.1",resolve));
const port=probe.address().port; await new Promise(resolve=>probe.close(resolve));
const exe=path.join(run,"kzapp.exe");await copyFile(path.resolve(process.argv[2]),exe);
const app=spawn(exe,[],{cwd:project,windowsHide:true,stdio:"ignore",env:{...process.env,KANZEI_HOME:appHome,USERPROFILE:profile,HOME:profile,
  LOCALAPPDATA:path.join(profile,"AppData/Local"),APPDATA:path.join(profile,"AppData/Roaming"),WEBVIEW2_USER_DATA_FOLDER:path.join(run,"webview"),KANZEI_E2E_CDP:String(port)}});
let browser,page;
const check=(condition,label)=>{assert(condition,label);checks.push(label);};
const waitBackend=async (fn,arg) => {
  const deadline=Date.now()+30000;
  while(Date.now()<deadline){if(await page.evaluate(fn,arg))return;await new Promise(resolve=>setTimeout(resolve,100));}
  throw new Error("Backend condition timed out");
};
try {
  const deadline=Date.now()+30000;
  while(Date.now()<deadline){try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);break;}catch{await new Promise(resolve=>setTimeout(resolve,300));}}
  assert(browser,"Native WebView2 starts");page=browser.contexts()[0].pages()[0];page.setDefaultTimeout(15000);
  page.on("pageerror",e=>errors.push(e.message));
  await page.waitForFunction(()=>document.body.dataset.appReady==="true",null,{timeout:30000});
  check(await page.evaluate(async()=>!(await window.__TAURI__.core.invoke("projects_get")).projects.length),"Only the disposable profile is open");
  await page.evaluate(async path=>{await window.__TAURI__.core.invoke("projects_init",{path,name:"回复恢复验收"});},project);
  await page.reload();await page.waitForFunction(()=>document.body.dataset.appReady==="true",null,{timeout:30000});
  check(await page.locator("body").getAttribute("data-view")==="chat","Native startup opens the main conversation");
  await page.locator('[data-work-surface="project"]').click();
  const line=await page.evaluate(async projectDir=>(await window.__TAURI__.core.invoke("process_list",{projectDir}))[0],project);
  await page.evaluate(async () => {
    window.__nativeErrors=[];
    await window.__TAURI__.event.listen("kz:error",event=>window.__nativeErrors.push(event.payload));
  });
  await page.evaluate(async ({projectDir,line})=>{
    await window.__TAURI__.core.invoke("process_update",{processId:line.id,subagentMode:false});
    await window.__TAURI__.core.invoke("auto_state_update",{sessionId:line.session_id,enabled:true});
    await window.__TAURI__.core.invoke("run_prompt",{projectDir,processId:line.id,prompt:"提出验收范围问题并等待回复",profile:"dev",agent:"dev-pair",model:"stub:test",autonomous:true,autoAllow:false});
  },{projectDir:project,line});
  await waitBackend(async projectDir=>{
    const snap=await window.__TAURI__.core.invoke("workspace_snapshot",{projectDir});window.__replySnapshot=snap;
    return snap.projects?.[0]?.decisions?.some(d=>d.status==="needs_input")&&!(await window.__TAURI__.core.invoke("process_list",{projectDir}))[0]?.running;
  },project);
  check(requests.length===2,"A real local model turn persisted a question and stopped");
  check(await page.evaluate(async sessionId=>(await import("./08-auto.js")).awaitingUserSessions.has(sessionId),line.session_id),"The waiting session is marked as requiring a user answer");
  await page.locator("#sw-refresh").click();await page.locator('[data-tab="inbox"]').click();
  check((await page.locator(".sw-inbox-row").filter({hasText:"请补充本次验收范围"}).innerText()).includes("待你回复"),"A persisted missing fact appears as a question in the inbox");
  await page.locator(".sw-inbox-row").filter({hasText:"请补充本次验收范围"}).click();
  check(await page.getByRole("heading",{name:"回复提问",exact:true}).isVisible() && await page.getByRole("button",{name:"本次通过",exact:true}).count()===0,"A missing fact has reply controls instead of an invalid approval button");
  check((await page.locator(".sw-choice-note").innerText()).includes("保留当前环境"),"The original stored options and notes survive projection");
  const rejected=await page.evaluate(async projectDir=>{
    const d=(await window.__TAURI__.core.invoke("workspace_snapshot",{projectDir})).projects[0].decisions.find(d=>d.status==="needs_input");
    try {
      await window.__TAURI__.core.invoke("decision_review",{projectDir,decisionId:d.id,agent:"dev-pair",
        review:{request_id:"invalid-approval-test",expected_revision:d.revision,action:"accept",feedback:"",scope:"once"}});
      return false;
    } catch(error) { return String(error).includes("missing facts require a reply"); }
  },project);
  check(rejected && requests.length===2,"The backend still rejects approval of a missing fact without resuming the model");
  await page.locator("#prompt").fill("本次通过");await page.locator("#send").click();
  await page.waitForFunction(()=>document.querySelector(".sw-interaction-receipt").textContent.includes("这条消息需要具体回答"));
  check(requests.length===2 && await page.locator("#prompt").inputValue()==="本次通过","A legacy approval draft stays visible with a useful explanation and cannot accidentally resume work");
  await page.getByRole("button",{name:"仅验收现有版本",exact:true}).click();
  await page.locator(".sw-reply-complete").waitFor();
  await waitBackend(async projectDir=>(await window.__TAURI__.core.invoke("process_list",{projectDir}))[0]?.running,project);
  const gateDeadline=Date.now()+15000;while(!releaseReply&&Date.now()<gateDeadline)await new Promise(resolve=>setTimeout(resolve,100));
  check(Boolean(releaseReply)&&requests.length===3,"Reply restarts the idle owner and reaches the model without another Continue");
  check(!await page.evaluate(async sessionId=>(await import("./08-auto.js")).awaitingUserSessions.has(sessionId),line.session_id),"The resumed model turn clears the old waiting-for-answer state");
  check((await page.locator(".sw-interaction-receipt").textContent()).includes("已恢复原对话"),"The receipt distinguishes resumed execution from a queue");
  const decision=await page.evaluate(async projectDir=>(await window.__TAURI__.core.invoke("workspace_snapshot",{projectDir})).projects[0].decisions[0],project);
  const repeat=await page.evaluate(async ({projectDir,d})=>window.__TAURI__.core.invoke("decision_review",{projectDir,decisionId:d.id,agent:"dev-pair",review:{...d.review,expected_revision:d.revision}}),{projectDir:project,d:decision});
  check(repeat.delivery.status==="consumed"&&requests.length===3,"Retrying the same receipt does not duplicate the prompt or scheduler");
  const queued=await page.evaluate(async ({projectDir,d})=>window.__TAURI__.core.invoke("decision_review",{projectDir,decisionId:d.id,agent:"dev-pair",review:{request_id:"second-reply",expected_revision:d.revision,action:"correct",feedback:"第二条补充事实",scope:"once"}}),{projectDir:project,d:decision});
  check(queued.delivery.status==="queued"&&requests.length===3,"A correction during execution waits for the current turn");
  releaseReply();
  await waitBackend(async projectDir=>!(await window.__TAURI__.core.invoke("process_list",{projectDir}))[0]?.running,project);
  const pending=await page.evaluate(async ({projectDir,processId})=>window.__TAURI__.core.invoke("list_pending_inputs",{projectDir,processId}),{projectDir:project,processId:line.id});
  check(requests.length===4&&pending.length===0,"Both saved replies execute once and the queue drains");
  await page.getByRole("button",{name:"返回工作",exact:true}).click();
  await page.locator('[data-work-surface="chat"]').click();await page.waitForFunction(()=>document.body.dataset.view==="chat");
  check((await page.locator("#messages").innerText()).includes("已处理第二条补充事实"),"The full native conversation contains the resumed result");
  check(await page.locator("#sidebar #focus-section").count() === 0,"Development sidebar no longer duplicates requirement cards (section removed from the DOM, UX-069)");
  await page.locator("#workbench-chat-history").click();check(await page.locator(".project-session-menu").isVisible(),"Session and history remain available beside the full conversation");
  await page.keyboard.press("Escape");await page.locator('[data-work-surface="project"]').click();await page.locator("#sw-refresh").click();
  await page.screenshot({path:path.join(output,"reply-resumed.png")});
  // The interactive question remains a live suspended tool, not a persisted decision.
  await page.evaluate(async ({sessionId,projectDir,processId})=>{
    await window.__TAURI__.core.invoke("auto_state_update",{sessionId,enabled:false});
    await window.__TAURI__.core.invoke("run_prompt",{projectDir,processId,prompt:"直接选择验收",profile:"dev",agent:"dev-pair",model:"stub:test",autonomous:false,autoAllow:false});
  },{sessionId:line.session_id,projectDir:project,processId:line.id});
  await waitBackend(async()=> (await window.__TAURI__.core.invoke("softwire_questions")).some(q=>q.question.startsWith("直接选择验收")));
  await page.waitForFunction(()=>document.querySelector("#status-mode").textContent==="待你回复");
  check(await page.locator("#status-elapsed").textContent()==="","A real suspended question hides elapsed execution time");
  await new Promise(resolve=>setTimeout(resolve,1300));
  check(await page.locator("#status-elapsed").textContent()==="" && await page.locator("#turn-activity-elapsed").textContent()==="","Waiting for a human never advances either native timer");
  await page.locator('[data-work-surface="chat"]').click();
  await page.waitForFunction(()=>document.body.dataset.view==="chat");
  check(await page.locator("#turn-activity").isVisible() && await page.locator("#turn-activity-label").textContent()==="待你回复","The visible conversation activity row shows human wait instead of tool execution");
  await page.screenshot({path:path.join(output,"question-timer-chat.png")});
  await page.locator('[data-work-surface="project"]').click();
  await page.locator('[data-tab="inbox"]').click();
  await page.locator(".sw-inbox-row").filter({hasText:"直接选择验收"}).click();
  await page.screenshot({path:path.join(output,"quick-reply-awaiting.png")});
  await page.getByRole("button",{name:"检查界面",exact:true}).evaluate(el=>{el.click();el.click();});
  await page.locator(".sw-reply-complete").waitFor();
  await waitBackend(async projectDir=>!(await window.__TAURI__.core.invoke("process_list",{projectDir}))[0]?.running,project);
  check(requests.length===6,"One option click delivers once and the real model continues without pressing Send");
  const replyTool=requests.at(-1).messages.filter(m=>m.role==="tool").at(-1);
  check(JSON.stringify(replyTool?.content).includes("检查界面"),"The model receives the selected label through the original question tool");
  check(await page.evaluate(async()=>!(await window.__TAURI__.core.invoke("softwire_questions")).length),"The acknowledged question leaves the pending inbox");
  check(await page.locator("#status-elapsed").textContent()==="" && await page.locator("#status-mode").textContent()!=="待你回复","Native completion clears the timer and waiting state");
  await page.screenshot({path:path.join(output,"quick-reply-complete.png")});
  check(errors.length===0,`No native page errors: ${errors.join("; ")}`);
  await writeFile(path.join(output,"acceptance.json"),JSON.stringify({passed:checks.length,checks,requests:requests.length,errors,executable:exe,boundary:"Real Tauri + WebView2 scheduler; deterministic local HTTP model, isolated project and profile."},null,2));
  console.log(`Native reply PASS: ${checks.length} checks, ${requests.length} local model requests`);
}catch(error){if(page){await page.screenshot({path:path.join(output,"failure.png")});console.error(await page.evaluate(()=>window.__nativeErrors));console.error((await page.locator("body").innerText()).slice(-1800));}console.error({checks,requests:requests.length,errors});throw error;}
finally{releaseReply?.();await browser?.close();app.kill();model.closeAllConnections();await new Promise(resolve=>model.close(resolve));}
