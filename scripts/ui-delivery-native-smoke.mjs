// Actual WebView2, Rust deliver tool, SQLite receipts and reload. Isolated profile and local model.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, writeFile, unlink } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";
if (process.platform!=="win32"||!process.argv[2]) throw new Error("Usage: node scripts/ui-delivery-native-smoke.mjs <exe> [read-only-project]");
const output=path.resolve("output/playwright/delivery/native"), run=path.join(output,String(Date.now()));
const project=path.join(run,"project"),profile=path.join(run,"profile"),appHome=path.join(profile,".kanzei");
for(const dir of [project,path.join(project,"build"),appHome,path.join(run,"webview"),path.join(profile,"AppData/Local"),path.join(profile,"AppData/Roaming")]) await mkdir(dir,{recursive:true});
const artifact=path.join(project,"build/android-r1-current-debug.apk");
await writeFile(artifact,"Delivery test fixture. Not an installable APK.");
await writeFile(path.join(appHome,"app.json"),JSON.stringify({projects:[],current:null,theme:"dark"}));
const requests=[],checks=[],errors=[];
const model=http.createServer(async(req,res)=>{
  let body="";for await(const part of req) body+=part;
  if(req.url!=="/v1/chat/completions"){res.writeHead(404);res.end();return;}
  const data=JSON.parse(body);requests.push(data);
  const completed=data.messages?.some(m=>m.role==="tool");
  const delta=completed?{content:"已交付 `android-r1-current-debug.apk`。请打开 [安装包](build/android-r1-current-debug.apk) 验收。"}
    :{tool_calls:[{index:0,id:"native-delivery",type:"function",function:{name:"deliver",arguments:JSON.stringify({path:"build/android-r1-current-debug.apk",caption:"Android 验收文件 · 尚未真机验收"})}}]};
  res.writeHead(200,{"Content-Type":"text/event-stream"});
  res.write(`data: ${JSON.stringify({id:"local-test",object:"chat.completion.chunk",choices:[{index:0,delta,finish_reason:null}]})}\n\n`);
  res.write(`data: ${JSON.stringify({choices:[{index:0,delta:{},finish_reason:completed?"stop":"tool_calls"}],usage:{prompt_tokens:100,completion_tokens:20}})}\n\n`);
  res.end("data: [DONE]\n\n");
});
await new Promise(resolve=>model.listen(0,"127.0.0.1",resolve));
await writeFile(path.join(appHome,"kanzei.toml"),`[models]\nprimary = "stub:test"\nfast = "stub:test"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n`);
const probe=net.createServer();await new Promise(resolve=>probe.listen(0,"127.0.0.1",resolve));const port=probe.address().port;await new Promise(resolve=>probe.close(resolve));
const exe=path.join(run,"kzapp.exe");await copyFile(path.resolve(process.argv[2]),exe);
const app=spawn(exe,[],{cwd:project,windowsHide:true,stdio:"ignore",env:{...process.env,KANZEI_HOME:appHome,USERPROFILE:profile,HOME:profile,
  LOCALAPPDATA:path.join(profile,"AppData/Local"),APPDATA:path.join(profile,"AppData/Roaming"),WEBVIEW2_USER_DATA_FOLDER:path.join(run,"webview"),KANZEI_E2E_CDP:String(port)}});
let browser,page;
const check=(value,label)=>{assert(value,label);checks.push(label);};
const invoke=(command,args)=>page.evaluate(({command,args})=>window.__TAURI__.core.invoke(command,args),{command,args});
async function waitBackend(fn,arg) {
  const deadline=Date.now()+45000;
  while(Date.now()<deadline) { if(await page.evaluate(fn,arg)) return; await new Promise(resolve=>setTimeout(resolve,150)); }
  throw new Error("Backend condition timed out");
}
try {
  const deadline=Date.now()+30000;
  while(Date.now()<deadline){try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);break;}catch{await new Promise(resolve=>setTimeout(resolve,300));}}
  assert(browser,"Native WebView2 starts");page=browser.contexts()[0].pages()[0];page.setDefaultTimeout(20000);
  page.on("pageerror",e=>errors.push(e.message));
  await page.waitForFunction(()=>document.body.dataset.appReady==="true",null,{timeout:30000});
  check(!(await invoke("projects_get",{})).projects.length,"Only the disposable profile is active");
  await invoke("projects_init",{path:project,name:"交付验收"});
  await page.reload();await page.waitForFunction(()=>document.body.dataset.appReady==="true",null,{timeout:30000});
  const line=(await invoke("process_list",{projectDir:project}))[0];
  await invoke("run_prompt",{projectDir:project,processId:line.id,prompt:"交付 build/android-r1-current-debug.apk 并提供可点击文件",profile:"dev",agent:"dev-pair",model:"stub:test",autonomous:false,autoAllow:false});
  await waitBackend(async projectDir=>{
    const rows=await window.__TAURI__.core.invoke("delivered_files",{projectDir});
    const lines=await window.__TAURI__.core.invoke("process_list",{projectDir});
    return rows.length===1&&!lines[0]?.running;
  },project);
  check(requests.length===2,"Real model turn called the Rust deliver tool and returned a reply");
  let rows=await invoke("delivered_files",{projectDir:project});
  check(rows[0].status==="available"&&rows[0].bytes===46,"Receipt has the actual file size and availability");
  check(rows[0].session_id===line.session_id&&rows[0].project_dir.replaceAll("/","\\").toLowerCase()===project.toLowerCase(),"Receipt binds the actual project and session");
  const receiptId=rows[0].id;
  const snapshot=await invoke("workspace_snapshot",{projectDir:project});
  check(!snapshot.projects[0].work_units?.length,"File delivery does not create a completed work unit");
  await page.locator('[data-work-surface="project"]').click();
  await page.locator("#sw-refresh").click();
  await page.locator('.sw-node[data-module="batch"]').click();
  await page.waitForFunction(()=>document.querySelectorAll(".sw-deliveries .file-card").length===1);
  check((await page.locator(".sw-deliveries").innerText()).includes("android-r1-current-debug.apk"),"Native delivery page shows the persisted file without a work unit");
  await page.screenshot({path:path.join(output,"delivery-dark.png")});
  await page.locator('[data-work-surface="chat"]').click();
  await page.waitForFunction(()=>document.body.dataset.view==="chat"&&document.querySelectorAll(".message-deliveries .file-card").length===1);
  check(await page.locator(".delivery-inline").count()===2,"Live reply exposes filename and Markdown references as clickable links");
  check(await page.locator(".tool-group .file-card").count()===0,"User delivery cards are outside collapsed tool groups");
  check(await page.locator(".tool-msg > .file-card").count()===0,"The final reply replaces the earlier visible card instead of duplicating it");
  await page.screenshot({path:path.join(output,"reply-dark.png")});
  await page.reload();await page.waitForFunction(()=>document.body.dataset.appReady==="true",null,{timeout:30000});
  await page.locator('[data-work-surface="chat"]').click();
  await page.waitForFunction(()=>document.querySelectorAll(".message-deliveries .file-card").length===1);
  rows=await invoke("delivered_files",{projectDir:project});
  check(rows.length===1&&rows[0].id===receiptId,"Reload recovers one durable receipt without duplicating the legacy tool result");
  check(await page.locator(".delivery-inline").count()===2,"Historical conversation recovers both clickable references");
  await writeFile(artifact,"Updated artifact");
  check((await invoke("delivered_files",{projectDir:project}))[0].status==="changed","Backend detects a replaced delivery file");
  await unlink(artifact);
  check((await invoke("delivered_files",{projectDir:project}))[0].status==="unavailable","Backend detects a removed delivery file");
  let denied=false;try{await invoke("open_delivered_path",{projectDir:project,path:artifact,mode:"reveal"});}catch{denied=true;}
  check(denied,"Missing file action fails explicitly without launching a shell");
  let actualProject;
  if(process.argv[3]) {
    const started=Date.now(); const actual=await invoke("delivered_files",{projectDir:path.resolve(process.argv[3])});
    actualProject={files:actual.map(r=>({name:r.name,status:r.status,bytes:r.bytes})),elapsedMs:Date.now()-started};
    check(actual.some(r=>r.name==="android-r1-current-debug.apk"&&r.status==="available"),"Read-only inspection recovers the user's previously delivered APK from old typed facts");
  }
  check(errors.length===0,`No native page errors: ${errors.join("; ")}`);
  await writeFile(path.join(output,"acceptance.json"),JSON.stringify({passed:checks.length,checks,requests:requests.length,errors,actualProject,executable:exe,boundary:"Actual Rust deliver + SQLite + native WebView2 reload; local deterministic model; no installation or external model. Optional existing project query is read-only."},null,2));
  console.log(`Delivery native PASS: ${checks.length} checks, ${requests.length} local requests`);
} catch(error) {if(page){await page.screenshot({path:path.join(output,"failure.png")});console.error((await page.locator("body").innerText()).slice(-2000));}console.error({checks,requests:requests.length,errors});throw error;}
finally {await browser?.close();app.kill();model.closeAllConnections();await new Promise(resolve=>model.close(resolve));}
