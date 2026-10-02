// Windows WebView2 + actual Tauri commands. Uses a disposable project and profile;
// never starts a model run. Pass the freshly built kzapp.exe as the first argument.
/* global window, document */
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, copyFile, writeFile } from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

if (process.platform !== "win32" || !process.argv[2]) throw new Error("Usage: node scripts/ui-softwire-native-smoke.mjs <fresh kzapp.exe>");
const output = path.resolve("output/playwright/softwire/native");
const run = path.join(output, String(Date.now()));
const project = path.join(run, "project");
const profile = path.join(run, "profile");
const appHome = path.join(profile, ".kanzei");
const webview = path.join(run, "webview");
for (const dir of [project,appHome,webview,path.join(profile,"AppData/Local"),path.join(profile,"AppData/Roaming")]) await mkdir(dir,{recursive:true});
await writeFile(path.join(appHome,"app.json"),JSON.stringify({projects:[],current:null,theme:"light"}));
const exe = path.join(run,"kzapp.exe");
// Only the GUI is copied: no CLI sidecar can update the user's installed CLI.
await copyFile(path.resolve(process.argv[2]),exe);
const probe = net.createServer();
await new Promise(resolve=>probe.listen(0,"127.0.0.1",resolve));
const port=probe.address().port;
await new Promise(resolve=>probe.close(resolve));
const app = spawn(exe,[],{cwd:project,windowsHide:true,stdio:"ignore",env:{...process.env,
  KANZEI_HOME:appHome,USERPROFILE:profile,HOME:profile,
  LOCALAPPDATA:path.join(profile,"AppData/Local"),APPDATA:path.join(profile,"AppData/Roaming"),
  WEBVIEW2_USER_DATA_FOLDER:webview,KANZEI_E2E_CDP:String(port),
}});
const passed=[], errors=[];
const check=(value,label)=>{assert(value,label);passed.push(label);};
let browser,page,servicePid;
try {
  const deadline=Date.now()+25000;
  while(Date.now()<deadline) {
    try {browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:1500});break;}
    catch {await new Promise(resolve=>setTimeout(resolve,400));}
  }
  assert(browser,"Native WebView2 CDP did not become ready");
  const context=browser.contexts()[0];
  page=context.pages()[0] || await context.waitForEvent("page");
  page.setDefaultTimeout(12000);page.on("pageerror",e=>errors.push(e.message));
  await page.waitForFunction(()=>document.body.dataset.appReady==="true",null,{timeout:30000});
  servicePid=(await page.evaluate(()=>window.__TAURI__.core.invoke("runtime_status"))).pid;
  check(!(await page.evaluate(()=>Boolean(window.__kzPreview))),"Desktop uses real IPC, no preview bridge");
  const prefs=await page.evaluate(()=>window.__TAURI__.core.invoke("projects_get"));
  check(prefs.projects.length===0,"Isolated desktop profile has no user projects");
  await page.evaluate(async path=>{
    await window.__TAURI__.core.invoke("projects_init",{path,name:"Softwire 原生验收"});
    await window.__TAURI__.core.invoke("docs_update",{projectDir:path,kind:"req",action:"add",id:"",title:"工作台原生交互验收",status:"doing",priority:"P1",fields:{"复杂度":"小","标签":"前端","描述":"仅用于隔离验收"}});
    await window.__TAURI__.core.invoke("docs_update",{projectDir:path,kind:"req",action:"update",id:"R-001",status:"doing"});
  },project);
  await page.addInitScript(() => {
    window.__activityBoot = [];
    new window.MutationObserver(() => {
      for (const el of document.querySelectorAll('.workbench-project-link[aria-busy="true"]')) {
        const indicator = el.querySelector(".workbench-project-activity");
        if (indicator) window.__activityBoot.push({ state: el.dataset.activity, pseudo: window.getComputedStyle(el,"::after").content,
          animation: window.getComputedStyle(indicator).animationName });
      }
    }).observe(document,{subtree:true,childList:true,attributes:true,attributeFilter:["aria-busy","data-activity","data-loading"]});
  });
  await page.reload();await page.waitForFunction(()=>document.body.dataset.appReady==="true");
  check(await page.locator("body").getAttribute("data-view")==="chat","Native startup restores the main conversation");
  check(await page.evaluate(()=>window.__activityBoot.length>0 && window.__activityBoot.every(frame=>frame.state!=="starting" && frame.animation!=="workbench-spin" && ["none","normal"].includes(frame.pseudo))),"Actual native startup loading frames never draw either spinner for restoring history");
  check(await page.locator(".workbench-project-link").first().evaluate(el => window.getComputedStyle(el,"::after").content === "none" && window.getComputedStyle(el.querySelector(".workbench-project-activity")).animationName === "none"),"Native cold project restore has no spinners or false execution activity");
  await page.locator('[data-work-surface="project"]').click();
  await page.locator("#softwire-workspace").waitFor();
  await page.waitForFunction(()=>document.querySelector("#softwire-workspace .sw-work-entry")?.textContent.includes("工作台原生交互验收"));
  check((await page.locator("#sw-title").textContent()).includes("Softwire 原生验收"),"Project heading stays stable above its work queue");
  check((await page.locator("#sw-requirement").textContent()).includes("项目概览"),"Unclaimed requirements never impersonate the current session's work");
  check((await page.locator("#softwire-workspace .sw-work-entry").first().textContent()).includes("R-001"),"Native overview includes the compact requirement list");
  check(await page.locator("#sw-run-toggle").isVisible(),"Native overview exposes one-click loop start");
  check(await page.locator("#composer #sw-run-toggle").count()===1,"Native loop control belongs to the movable composer");
  check(await page.locator("[data-resource]").count()===7 && await page.locator("#project-tools").count()===0,"Native development resources stay separate from research");
  const questions=await page.evaluate(()=>window.__TAURI__.core.invoke("softwire_questions"));
  check(Array.isArray(questions)&&questions.length===0,"Native non-modal question command is registered");
  const rejected=await page.evaluate(async projectDir=>{
    try {await window.__TAURI__.core.invoke("softwire_answer_question",{projectDir,sessionId:"nonexistent",id:9,expectedRevision:"old",requestId:"native-expired",reply:"验收回复"});return "unexpected success";}
    catch(error){return typeof error === "string" ? error : JSON.stringify(error);}
  },project);
  check(rejected.includes("question_expired"),"Native stale answer rejects without inventing delivery: " + rejected);
  await page.locator("#prompt").fill("原生主对话草稿");
  await page.locator('[data-module="memory"]').click();
  check(await page.locator("#prompt").inputValue()==="","Native memory target has a separate draft");
  await page.locator("#prompt").fill("原生记忆草稿");
  await page.locator('[data-work-surface="project"]').click();
  check(await page.locator("#prompt").inputValue()==="原生主对话草稿","Native draft returns intact");
  await page.getByRole("button",{name:"悬浮对话框",exact:true}).click();
  check(await page.locator("#composer.sw-floating").count()===1,"Native WebView2 supports the floating editor");
  await page.getByRole("button",{name:"停靠对话框",exact:true}).click();
  // 任务设置菜单已撤(B22):自动放行是随输入框搬进工作台输入卡的直接开关。
  await page.locator("#composer .sw-composer-controls #auto-allow-wrap").waitFor();
  check(await page.locator("#composer .sw-composer-controls #auto-allow-wrap").isVisible(),"Native auto-allow switch is at the work desk");
  check(await page.locator("#prompt").inputValue()==="原生主对话草稿","Resolving a new project's first session preserves the draft");
  await page.getByRole("button",{name:"鞭挞设置",exact:true}).click();
  await page.locator("#autorun-menu").waitFor();
  check(await page.locator("#autorun-menu").isVisible(),"Native loop settings open without leaving the overview");
  await page.locator("#continue-toggle").click();
  check(await page.locator("#continue-prompt").isVisible(),"Native continuation text can be edited in place");
  await page.keyboard.press("Escape");
  await page.locator('[data-work-surface="project"]').click();
  await page.screenshot({path:path.join(output,"desktop-light.png")});
  await page.evaluate(async projectDir => {
    for (let i=2;i<=12;i++) {
      await window.__TAURI__.core.invoke("docs_update",{projectDir,kind:"req",action:"add",id:"",title:`密集需求 ${i}：确认长标题不会把执行网络和工具模块挤出视野`,status:"doing",priority:"P2",fields:{"复杂度":"小","标签":"前端","描述":"隔离布局验收"}});
      await window.__TAURI__.core.invoke("docs_update",{projectDir,kind:"req",action:"update",id:`R-${String(i).padStart(3,"0")}`,status:"doing"});
    }
    await window.__TAURI__.core.invoke("research_library_create",{topic:"native-study",title:"原生研究空间验收"});
  },project);
  await page.locator("#sw-refresh").click();
  await page.getByRole("button",{name:"查看全部进行中的工作 ›",exact:true}).waitFor();
  check(await page.locator("#softwire-workspace .sw-work-entry").count()===2,"Native bounded summary retains a full-list entry for the remaining requirements");
  await page.evaluate(()=>document.documentElement.setAttribute("data-theme","dark"));
  await page.waitForTimeout(250);
  check(await page.evaluate(()=>{
    const surface=document.querySelector("#sw-surface").getBoundingClientRect(), composer=document.querySelector("#composer").getBoundingClientRect();
    return [...document.querySelectorAll(".sw-node, .sw-resources button")].every(el=>{const b=el.getBoundingClientRect();return b.top>=surface.top&&b.bottom<=surface.bottom+1&&b.bottom<=composer.top;});
  }),"Native dark desktop keeps every network module above the composer");
  await page.screenshot({path:path.join(output,"desktop-dark.png")});
  await page.locator('[data-workspace="research"]').click();
  await page.waitForFunction(()=>document.body.dataset.space==="research"&&!document.querySelector('[data-workspace="research"]').disabled);
  check(await page.locator("#research-section").isVisible()&&!await page.locator("#sw-run-toggle").isVisible(),"Native research has independent navigation and no development loop controls");
  check((await page.locator("#research-heading").innerText()).includes("原生研究空间验收"),"Native research library loads its real topic");
  await page.locator('[data-research-page="chat"]').click();
  await page.waitForFunction(()=>document.body.dataset.view==="chat");
  await page.locator("#prompt").fill("原生研究草稿");
  const researchIdentity=await page.evaluate(async()=>{
    const s=await import("./03-shell.js");return {root:s.currentProject,process:s.processItems.find(p=>p.id===s.activeProcessId)};
  });
  check(researchIdentity.root!==project&&researchIdentity.process.profile==="research"&&researchIdentity.process.research_topic==="native-study","Native research owns a separate storage root and bound session");
  await page.locator('[data-research-page="writing"]').click();
  await page.reload();await page.waitForFunction(()=>document.body.dataset.appReady==="true");
  check(await page.locator("body").getAttribute("data-space")==="research"&&await page.locator(".research-workspace").getAttribute("data-page")==="writing","Desktop reload restores the selected research topic and page");
  await page.locator('[data-research-page="overview"]').click();
  await page.waitForFunction(()=>document.querySelector(".research-workspace").dataset.page==="overview"&&document.querySelector('[data-research-page="overview"]').getAttribute("aria-current")==="page");
  await page.evaluate(()=>document.documentElement.setAttribute("data-theme","dark"));await page.waitForTimeout(250);
  await page.screenshot({path:path.join(output,"research-dark.png")});
  await page.locator('[data-workspace="dev"]').click();
  await page.waitForFunction(()=>document.body.dataset.space==="dev");
  await page.locator('[data-work-surface="project"]').click();
  await page.locator("#softwire-workspace").waitFor();
  check(await page.locator("#prompt").inputValue()==="原生主对话草稿","Native return from research restores the persisted development draft");
  check(errors.length===0,`No native browser errors: ${errors.join("; ")}`);
  await writeFile(path.join(output,"acceptance.json"),JSON.stringify({passed:passed.length,checks:passed,errors,run,exe,boundary:"Real desktop and local IPC in an isolated profile; no model execution."},null,2));
  console.log(`Softwire native PASS: ${passed.length} checks`);
} catch(error) {
  if(page) await page.screenshot({path:path.join(output,"failure.png")}).catch(()=>{});
  if(page) console.error("Native diagnostics:",await page.evaluate(()=>({state:{...document.body.dataset},text:document.body.innerText.slice(-6000)})).catch(()=>null),errors);
  console.error("Native checks passed:",passed);
  throw error;
} finally {
  if(page) await page.evaluate(()=>window.__TAURI__.window.getCurrentWindow().close()).catch(()=>{});
  if(browser) await browser.close().catch(()=>{});
  if(app.exitCode===null) app.kill();
  // This exact PID belongs to the runtime started in this test's disposable profile.
  if(servicePid) { try { process.kill(servicePid); } catch {} }
}
