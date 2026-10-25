/* global document */
// Real Luna chooses desktop tools; independent native callbacks verify the result.
import assert from "node:assert/strict";

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { nativeApp, until, textReply, stopOwnedProcess } from "./fixtures/native-app.mjs";
import { withTask, spawnOwned as spawn } from "./artifact-lifecycle.mjs";

await withTask({ suite: "ui-desktop-luna-native-smoke" }, async task => {

assert(process.platform === "win32" && process.argv[2], "Pass a fresh kzapp.exe");
const output=task.evidence,fixtureRoot=path.join(task.environment,`form-${Date.now()}`);
await mkdir(fixtureRoot,{recursive:true});
const app=await nativeApp(process.argv[2],output,"Luna 原生窗口验收",async(_body,res)=>textReply(res,"No candidates."),"[limits]\nmax_steps=12\n");
let form;
try {
  form=spawn("C:\\Program Files\\PowerShell\\7\\pwsh.exe",["-NoProfile","-Sta","-File",path.resolve("scripts/fixtures/desktop-form.ps1"),"-StateDirectory",fixtureRoot],{windowsHide:true,stdio:"ignore"});
  await until(()=>existsSync(path.join(fixtureRoot,"window.json")),"Owned form opens");
  const window=JSON.parse(await readFile(path.join(fixtureRoot,"window.json"),"utf8"));
  const line=await app.createConversation();
  await app.page.locator("#composer-more").click();await app.page.locator("#native-desktop-choose").click();
  await app.page.locator("#native-desktop-dialog .native-window-list button").filter({hasText:window.title}).click();
  await app.page.locator("#native-desktop-dialog").waitFor({state:"hidden"});
  await app.invoke("run_prompt",{projectDir:app.project,processId:line.id,profile: "dev-pair",agent:"dev-pair",model:"codex:gpt-6-luna",autonomous:true,autoAllow:true,
    prompt:"请用 desktop 原生窗口工具操作我已经选择的测试窗口：先观察，把 Customer name 填为 LUNA_NATIVE_FORM，然后点击 Submit form。每次修改后重新观察，确认表单回显实际姓名再结束。无需操作密码、文件选择或其他窗口。不要调用终端或浏览器来替代原生工具。"});
  let outcome;
  await until(async()=>{
    assert.equal(app.errors.length,0,app.errors.join("; "));
    outcome=(await app.invoke("collaboration_snapshot",{projectDir:app.project})).find(row=>row.process_id===line.id);
    return outcome && !outcome.running && ["completed","failed","stopped"].includes(outcome.status);
  },"Actual Luna native task reaches its durable outcome",240000);
  const state=JSON.parse(await readFile(path.join(fixtureRoot,"state.json"),"utf8"));
  const history=await app.invoke("conversation_display_get",{projectDir:app.project,processId:line.id});
  const report={model:"codex:gpt-6-luna",outcome,state,history,fixtureRoot,executable:app.exe,scope:"One native form, real model, no retry; deterministic contract and boundary suites are separate"};
  await writeFile(path.join(output,"observation.json"),JSON.stringify(report,null,2));
  assert.equal(outcome.status,"completed");assert(state.submitted&&state.name==="LUNA_NATIVE_FORM","Actual application callback must verify Luna's form submission");
  assert(history.some(message=>JSON.stringify(message).includes("desktop")),"Actual desktop tool trace must be persisted");
  await app.page.screenshot({path:path.join(output,"native-luna.png")});
  console.log("Real Luna native form PASS: submission independently verified");
} catch(error) { await app.page.screenshot({path:path.join(output,"failure.png")});await writeFile(path.join(output,"failure.json"),JSON.stringify({error:String(error),errors:app.errors,fixtureRoot,run:app.run},null,2));throw error; }
finally { try { await app.close(); } finally { await stopOwnedProcess(form); } }
});
