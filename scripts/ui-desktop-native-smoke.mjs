/* global document */
// Actual human window selection -> provider tool calls -> native GUI callbacks.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir,readFile,writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { nativeApp,until,textReply,toolReply,stopOwnedProcess } from "./fixtures/native-app.mjs";
import { decodeExternalSource } from "./fixtures/tool-source.mjs";

assert(process.platform==="win32"&&process.argv[2],"Pass a fresh kzapp.exe");
const output=path.resolve("output/playwright/desktop/native"),fixtureRoot=path.join(output,`form-${Date.now()}`);await mkdir(fixtureRoot,{recursive:true});
let form;
const results=[],checks=[],check=(value,label)=>{assert(value,label);checks.push(label);};
let snapshot,done=false,noWindowDone=false,approvalDone=false,firstWindow;
const actions=["inspect","fill","stale","inspect","key","inspect","submit","inspect","canvas","inspect","open","inspect","dialog_type","inspect","secret_file","file","inspect","password","inspect","password_focus","inspect","surface_password","inspect"];
const app=await nativeApp(process.argv[2],output,"原生窗口软件验收",async(body,res)=>{
  const index=body.messages?.findLastIndex(message=>message.role==="user"&&typeof message.content==="string"&&message.content.startsWith("NATIVE_DESKTOP_"));
  const original=body.messages?.[index];
  if(!original){textReply(res,"No candidates.");return;}
  const tools=body.messages.slice(index+1).filter(message=>message.role==="tool");
  if(!tools.length){toolReply(res,"tool_search",{query:"select:desktop"},0);return;}
  if(original.content.startsWith("NATIVE_DESKTOP_APPROVAL")){
    if(tools.length===1){const element=snapshot.snapshot.elements.find(element=>element.automation_id==="CustomerName");toolReply(res,"desktop",{action:"fill",window_id:snapshot.window_id,snapshot_id:snapshot.snapshot_id,element_id:element.id,text:"UNAPPROVED_REDIRECT"},1);return;}
    results.push({mode:"approval",...decodeExternalSource(tools.at(-1).content)});approvalDone=true;textReply(res,"NATIVE_DESKTOP_APPROVAL_DONE");return;
  }
  if(original.content.startsWith("NATIVE_DESKTOP_NO_WINDOW")){
    if(tools.length===1){toolReply(res,"desktop",{action:"inspect"},1);return;}
    results.push({mode:"unselected",...decodeExternalSource(tools.at(-1).content)});noWindowDone=true;textReply(res,"NATIVE_DESKTOP_NO_WINDOW_DONE");return;
  }
  if(tools.length>1){
    const last=decodeExternalSource(tools.at(-1).content);
    results.push({action:actions[tools.length-2],...last});
    if(!last.content.startsWith("[tool error]")&&!last.content.startsWith("[tool_outcome=")){
      const value=JSON.parse(last.content.split("\n[tool-image] ")[0]);if(value.snapshot)snapshot=value;
    }
  }
  const action=actions[tools.length-1];
  if(!action){done=true;textReply(res,"NATIVE_DESKTOP_FORM_DONE: actual GUI result awaits independent fixture verification.");return;}
  if(action==="inspect"){toolReply(res,"desktop",{action:"inspect"},tools.length);return;}
  assert(snapshot,`Snapshot is required for ${action}`);
  const element=id=>{const found=snapshot.snapshot.elements.find(element=>element.automation_id===id);assert(found,`Missing actual native control ${id}`);return found.id;};
  const target=id=>({window_id:snapshot.window_id,snapshot_id:snapshot.snapshot_id,element_id:element(id)});
  let input;
  if(action==="fill")input={action:"fill",...target("CustomerName"),text:"NATIVE_AGENT_NAME"};
  if(action==="stale")input={action:"fill",...target("CustomerName"),text:"STALE_OVERWRITE"};
  if(action==="key")input={action:"key",...target("CustomerName"),key:"Enter"};
  if(action==="submit")input={action:"invoke",...target("Submit")};
  if(action==="canvas")input={action:"click",...target("DrawingCanvas"),x:100,y:80};
  if(action==="open")input={action:"invoke",...target("ChooseFile")};
  if(action==="secret_file"||action==="file"){
    const name=snapshot.snapshot.elements.find(element=>element.automation_id==="1148"&&element.editable);assert(name,"Observed native file-name control exists");
    input={action:"choose_file",window_id:snapshot.window_id,snapshot_id:snapshot.snapshot_id,element_id:name.id,file:action==="secret_file"?".env":"upload.txt"};
  }
  if(action==="password")input={action:"fill",...target("Secret"),text:"SHOULD_NOT_ENTER_PASSWORD"};
  if(action==="password_focus")input={action:"key",...target("CustomerName"),key:"Tab"};
  if(action==="surface_password"||action==="dialog_type")input={action:"type",window_id:snapshot.window_id,snapshot_id:snapshot.snapshot_id,element_id:snapshot.snapshot.surface.id,text:"SHOULD_NOT_ENTER"};
  assert(input,action);toolReply(res,"desktop",input,tools.length);
});
try{
  form=spawn("C:\\Program Files\\PowerShell\\7\\pwsh.exe",["-NoProfile","-Sta","-File",path.resolve("scripts/fixtures/desktop-form.ps1"),"-StateDirectory",fixtureRoot],{windowsHide:true,stdio:"ignore"});
  await until(()=>existsSync(path.join(fixtureRoot,"window.json")),"Owned native form starts");
  firstWindow=JSON.parse(await readFile(path.join(fixtureRoot,"window.json"),"utf8"));
  await writeFile(path.join(app.project,"upload.txt"),"NATIVE_COPIED_INPUT_BYTES");
  await writeFile(path.join(app.project,".env"),"NATIVE_DESKTOP_SECRET_CANARY");
  const line=await app.createConversation();
  await app.page.locator("#composer-more").click();await app.page.locator("#native-desktop-choose").click();
  await app.page.locator("#native-desktop-dialog .native-window-list button").filter({hasText:firstWindow.title}).click();
  await app.page.locator("#native-desktop-dialog").waitFor({state:"hidden"});
  const selected=await app.invoke("native_desktop_get",{processId:line.id});check(Boolean(selected?.id),"Actual UI window selection publishes a conversation-owned capability");
  const list=await app.invoke("native_desktop_windows");check(!list.windows.some(window=>/^(kzapp|codex|chatgpt)\.exe$/i.test(window.application)),"Human picker excludes agent and approval windows");
  await app.invoke("run_prompt",{projectDir:app.project,processId:line.id,prompt:"NATIVE_DESKTOP_FORM: 填写原生表单、按键、点击画布并选择明确文件，保留失败。",profile:"dev",agent:"dev-pair",model:"stub:test",autonomous:true,autoAllow:true});
  await until(async()=>{assert.equal(app.errors.length,0,app.errors.join("; "));return done&&(await app.invoke("collaboration_snapshot",{projectDir:app.project})).find(row=>row.process_id===line.id)?.status==="completed";},"Actual native GUI workflow completes",180000);
  const state=JSON.parse(await readFile(path.join(fixtureRoot,"state.json"),"utf8"));
  check(state.submitted&&state.name==="NATIVE_AGENT_NAME","Native application callback verifies actual form submission");
  check(state.enter_count===1,"Native application callback verifies the keyboard event");
  check(state.canvas_clicks===1,"Native application callback verifies actual canvas input");
  check(state.file&&await readFile(state.file,"utf8")==="NATIVE_COPIED_INPUT_BYTES","Native file chooser receives the actual retained input copy");
  check(state.file!==path.join(app.project,"upload.txt"),"Native chooser never receives a raceable project path");
  check(await readFile(path.join(app.project,"upload.txt"),"utf8")==="NATIVE_COPIED_INPUT_BYTES","Project input remains unchanged");
  check(results.filter(result=>result.action==="inspect").every(result=>result.source?.kind==="desktop"),"Actual UIA and captured pixels carry Desktop source boundaries");
  check(results.find(result=>result.action==="stale")?.content.includes("NATIVE_WINDOW_TARGET_CHANGED"),"Old snapshots cannot drive another mutation");
  check(results.find(result=>result.action==="password")?.content.includes("Password controls"),"Password input is refused by the actual executor");
  check(results.find(result=>result.action==="surface_password")?.content.includes("Password controls"),"Screenshot-based text input cannot bypass the focused password boundary");
  check(results.find(result=>result.action==="dialog_type")?.content.includes("dedicated file choice"),"Screenshot-based text input cannot bypass the native file dialog boundary");
  check(results.find(result=>result.action==="secret_file")?.content.includes("permission requires user approval"),"AutoAllow cannot choose a sensitive file without exact consent");
  check(!JSON.stringify(app.requests).includes("NATIVE_DESKTOP_SECRET_CANARY")&&!JSON.stringify(app.requests).includes("UIA_PASSWORD_CANARY"),"Credential and password canaries never enter provider requests");
  const next=await app.createConversation();
  check(await app.invoke("native_desktop_get",{processId:next.id})===null,"Another conversation does not inherit the selected window");
  await app.invoke("run_prompt",{projectDir:app.project,processId:next.id,prompt:"NATIVE_DESKTOP_NO_WINDOW: 不得接管上一条对话的窗口。",profile:"dev",agent:"dev-pair",model:"stub:test",autonomous:true,autoAllow:true});
  await until(()=>noWindowDone,"Unselected conversation refuses native inspection");
  check(results.find(result=>result.mode==="unselected")?.content.includes("NATIVE_WINDOW_REQUIRED"),"Provider cannot pick a window by itself");
  await app.page.evaluate(async id=>(await import("./09-sessions.js")).switchProcess(id,true),line.id);
  await app.invoke("run_prompt",{projectDir:app.project,processId:line.id,prompt:"NATIVE_DESKTOP_APPROVAL: 等待一次窗口动作批准。",profile:"dev",agent:"dev-pair",model:"stub:test",autonomous:false,autoAllow:false});
  await until(async()=>(await app.invoke("pending_asks_get",{projectDir:app.project,processId:line.id})).length===1,"Native mutation waits for actual approval");
  const pending=await app.invoke("pending_asks_get",{projectDir:app.project,processId:line.id});
  await app.invoke("native_desktop_select",{processId:line.id,candidateId:null});
  await app.invoke("answer_ask",{id:pending[0].id,reply:"once"});
  await until(()=>approvalDone,"Approved action revalidates its original selected window");
  check(results.find(result=>result.mode==="approval")?.content.includes("NATIVE_WINDOW_TARGET_CHANGED"),"Disconnecting while approval is pending rejects the old capability");
  check(JSON.parse(await readFile(path.join(fixtureRoot,"state.json"),"utf8")).current_name==="NATIVE_AGENT_NAME","Approval cannot redirect the pending mutation after selection changes");
  await app.page.screenshot({path:path.join(output,"native-desktop.png")});
  await writeFile(path.join(output,"acceptance.json"),JSON.stringify({passed:checks.length,checks,results,fixtureRoot,executable:app.exe},null,2));console.log(`Desktop native PASS: ${checks.length} checks`);
}catch(error){await writeFile(path.join(output,"failure.json"),JSON.stringify({checks,results,errors:app.errors,request_count:app.requests.length,fixtureRoot},null,2));await app.page.screenshot({path:path.join(output,"native-failure.png")});throw error;}
finally{try{await app.close();}finally{await stopOwnedProcess(form);}}
