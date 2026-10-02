// Real browser and production UI; desktop IPC actions are recorded, never execute an installer.
/* global window, document */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
const output = path.resolve("output/playwright/delivery");
await mkdir(output,{recursive:true});
const server = await startPreviewServer({port:0});
const browser = await chromium.launch({channel:"msedge",headless:true});
const page = await browser.newPage({viewport:{width:1440,height:960}});
page.setDefaultTimeout(10000);
const checks=[], errors=[];
const check=(value,label)=>{assert(value,label);checks.push(label);};
page.on("pageerror",e=>errors.push(e.message));
const settle=()=>page.evaluate(()=>window.__kzPreview.settle());
const last=cmd=>page.evaluate(cmd=>window.__kzPreview.calls.filter(c=>c.cmd===cmd).at(-1)?.args,cmd);
try {
  await page.goto(`${server.origin}/?scene=workspace&theme=light`);
  await page.waitForFunction(()=>window.__kzPreview?.ready);
  const identity=await page.evaluate(async()=>{
    const f=await window.__kzPreview.fixtures();
    const snapshot=f.commands.workspace_snapshot(), project=snapshot.projects[0];
    project.work_units=[]; for(const l of project.lines) {l.running=false;l.stage="空闲";}
    const row={id:"delivery-1",kind:"file",name:"android-r1-current-debug.apk",path:project.path+"/build/android-r1-current-debug.apk",
      bytes:230506809,caption:"R-001 Android · 等待真机验收",project_dir:project.path,worktree_root:project.path,session_id:project.lines[0].session_id,created_at:Date.now(),status:"available"};
    window.__deliveryTest={rows:[row],snapshot,project};
    window.__kzPreview.setCommand("workspace_snapshot",()=>structuredClone(snapshot));
    window.__kzPreview.setCommand("delivered_files",()=>structuredClone(window.__deliveryTest.rows));
    window.__kzPreview.setCommand("open_delivered_path",()=>null);
    window.__kzPreview.setCommand("save_delivered_file",()=>null);
    await (await import("/12-workbench.js")).openProjectSpace(project.path,"project");
    await (await import("/06-deliveries.js")).loadDeliveredFiles(project.path,{force:true});
    return {project:project.path,session:project.lines[0].session_id,path:row.path};
  }); await settle();
  await page.locator('#sw-checkpoint').click();await settle();
  check(await page.locator(".sw-deliveries .file-card").count()===1,"Delivery page shows real files without a work unit");
  check((await page.locator("#sw-checkpoint").innerText()).includes("1 个交付文件"),"Header no longer falsely says no delivery");
  check((await page.locator(".sw-deliveries .file-card-size").innerText()).includes("219.8 MB"),"File size is displayed");
  await page.locator(".sw-deliveries .file-card-name").click();await settle();
  check((await last("open_delivered_path")).path===identity.path&&(await last("open_delivered_path")).mode==="reveal","APK filename reveals the exact artifact rather than executing an installer");
  await page.locator(".sw-deliveries").getByRole("button",{name:"另存为",exact:true}).click();await settle();
  check((await last("save_delivered_file")).projectDir===identity.project,"Save as stays bound to the delivery's project");
  await page.screenshot({path:path.join(output,"delivery-light.png")});
  await page.evaluate(()=>document.documentElement.dataset.theme="dark");
  await page.screenshot({path:path.join(output,"delivery-dark.png")});
  await page.evaluate(async({project,session})=>{
    const shell=await import("/03-shell.js"), core=await import("/01-core.js"), chat=await import("/05-chat-render.js"), md=await import("/04-markdown.js");
    core.showPane(session); const msg=chat.addMessage("assistant","");
    md.renderMarkdownInto(msg.querySelector(".message-body"),"已交付 `android-r1-current-debug.apk`。也可以打开 [安装包](build/android-r1-current-debug.apk)。");
    window.__deliveryTest.message=msg;
    shell.navigate_view("chat");
  },identity); await settle();
  check(await page.locator(".message-deliveries .file-card").count()===1,"One visible file card is attached to the assistant reply, even with repeated references");
  check(await page.locator(".delivery-inline").count()===2,"Inline filenames and Markdown file links both become clickable");
  await page.locator(".delivery-inline").first().click(); await settle();
  check((await last("open_delivered_path")).path===identity.path,"Reply link opens the same artifact as the delivery page");
  await page.screenshot({path:path.join(output,"reply-dark.png")});
  check(await page.evaluate(async()=>{
    const {matchDeliveredFile}=await import("/06-deliveries.js");const row=window.__deliveryTest.rows[0];
    return matchDeliveredFile(row.name,[row,{...row,path:row.project_dir+"/other/"+row.name}])===null;
  }),"Ambiguous filenames never silently choose a different file");
  await page.evaluate(async()=>{
    const {renderFileCard}=await import("/06-activity.js");
    const row=window.__deliveryTest.rows[0]; const card=renderFileCard(row);
    (await import("/03-shell.js")).setCurrentProject("C:/another-project");
    card.querySelector(".file-card-name").click();
  });await settle();
  check((await last("open_delivered_path")).projectDir===identity.project,"Switching projects cannot redirect an existing card");
  await page.evaluate(async()=>{
    window.__deliveryTest.rows[0].status="unavailable";
    await (await import("/06-deliveries.js")).loadDeliveredFiles(window.__deliveryTest.project.path,{force:true});
  });await settle();
  check(await page.locator(".message-deliveries .file-card-name").isDisabled(),"Moved or deleted files show an unavailable state");
  check((await page.locator(".message-deliveries").innerText()).includes("文件已移动或删除"),"Missing files are not shown as usable deliveries");
  check(errors.length===0,`No browser errors: ${errors.join("; ")}`);
  await writeFile(path.join(output,"browser-acceptance.json"),JSON.stringify({passed:checks.length,checks,errors,boundary:"Real Edge UI, recorded IPC actions; backend checked independently."},null,2));
  console.log(`Delivery browser PASS: ${checks.length} checks`);
} catch(error) { await page.screenshot({path:path.join(output,"failure.png")});throw error; }
finally {await browser.close();await server.close();}
