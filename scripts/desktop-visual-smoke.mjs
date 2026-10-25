// Actual provider calls -> conversation-owned snapshot -> custom-drawn native UI.
import assert from 'node:assert/strict';
import { spawnOwned as spawn, withTask } from './artifact-lifecycle.mjs';
import { readFile,writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { nativeApp,until,stopOwnedProcess,toolReply,textReply } from './fixtures/native-app.mjs';
import { decodeExternalSource } from './fixtures/tool-source.mjs';

await withTask({suite:'desktop-visual-smoke'},async task=>{
assert(process.platform==='win32'&&process.argv[2],'Pass a fresh kzapp.exe');
const output=task.evidence,run=task.environment;
const program='C:\\Program Files\\PowerShell\\7\\pwsh.exe';
const checks=[],results=[],check=(value,label)=>{assert(value,label);checks.push(label);};
const actions=['inspect','root','inspect','click','inspect','type','inspect','chord','inspect','invalid','inspect','final','inspect','key','inspect','submit','stale','inspect','bounds','inspect','geometry'];
let fixture,observed,done=false;
const app=await nativeApp(process.argv[2],output,'截图操作软件回归',async(body,res)=>{
  const index=body.messages?.findLastIndex(message=>message.role==='user'&&typeof message.content==='string'&&message.content.startsWith('DESKTOP_VISUAL:'));
  if(index<0||index===undefined){textReply(res,'No candidates.');return;}
  const tools=body.messages.slice(index+1).filter(message=>message.role==='tool');
  if(!tools.length){toolReply(res,'tool_search',{query:'select:desktop'},0);return;}
  if(tools.length>1){
    const result=decodeExternalSource(tools.at(-1).content);results.push({action:actions[tools.length-2],...result});
    if(!result.content.startsWith('[tool')){const value=JSON.parse(result.content.split('\n[tool-image] ')[0]);if(value.snapshot)observed=value;}
  }
  const action=actions[tools.length-1];
  if(!action){done=true;textReply(res,'DESKTOP_VISUAL_DONE');return;}
  if(action==='inspect'){toolReply(res,'desktop',{action},tools.length);return;}
  assert(observed?.snapshot.surface,'Actual tool observation must expose a screenshot surface');
  const surface=observed.snapshot.surface;
  const input={action,window_id:observed.window_id,snapshot_id:observed.snapshot_id,element_id:surface.id};
  const canvas=observed.snapshot.elements.find(element=>element.automation_id==='VisualCanvas');assert(canvas);
  const point=(x,y)=>({x:(canvas.bounds.x-surface.bounds.x+x)*surface.width/surface.bounds.width,y:(canvas.bounds.y-surface.bounds.y+y)*surface.height/surface.bounds.height});
  if(action==='root'){input.action='invoke';input.element_id=observed.snapshot.elements[0].id;}
  if(action==='click'){input.action='click';Object.assign(input,point(200,140));}
  if(action==='type'||action==='final'){input.action='type';input.text=action==='type'?'Visual input \u4e2d\u6587 \ud83c\udfb5':'FINAL_VISUAL_VALUE';}
  if(action==='chord'||action==='invalid'||action==='key'){input.action='key';input.key=action==='chord'?'Control+A':action==='invalid'?'Control+Unsupported':'Enter';}
  if(action==='submit'||action==='stale'){input.action='click';Object.assign(input,point(1150,840));}
  if(action==='bounds'){input.action='click';input.x=surface.width;input.y=0;}
  if(action==='geometry'){
    await writeFile(path.join(run,'move.request'),'move');
    await until(async()=>JSON.parse(await readFile(path.join(run,'state.json'),'utf8')).moved,'Fixture independently moves after the observation');
    input.action='click';Object.assign(input,point(1150,840));
  }
  toolReply(res,'desktop',input,tools.length);
});
try {
  fixture=spawn(program,['-NoProfile','-Sta','-File',path.resolve('scripts/fixtures/desktop-canvas.ps1'),'-StateDirectory',run],{windowsHide:true,stdio:'ignore'});
  await until(()=>existsSync(path.join(run,'window.json')),'Visual fixture opens');
  const window=JSON.parse(await readFile(path.join(run,'window.json'),'utf8'));
  const line=await app.createConversation();
  await app.page.locator('#composer-more').click();await app.page.locator('#native-desktop-choose').click();
  await app.page.locator('#native-desktop-dialog .native-window-list button').filter({hasText:window.title}).click();
  await app.page.locator('#native-desktop-dialog').waitFor({state:'hidden'});
  await app.invoke('run_prompt',{projectDir:app.project,processId:line.id,prompt:'DESKTOP_VISUAL: 按截图点击自绘输入区，输入文字、组合键并提交。',profile: "dev-pair",agent:'dev-pair',model:'stub:test',autonomous:true,autoAllow:true});
  await until(async()=>{assert.equal(app.errors.length,0,app.errors.join('; '));return done&&!(await app.invoke('collaboration_snapshot',{projectDir:app.project})).find(row=>row.process_id===line.id)?.running;},'Visual provider workflow completes',180000);
  const state=JSON.parse(await readFile(path.join(run,'state.json'),'utf8'));
  check(!observed.snapshot.elements.some(element=>element.editable),'Custom-drawn input has no UIA editable control');
  check(results.find(result=>result.action==='root')?.content.includes('content or identity changed'),'Dynamic title reproduces the obsolete root-control rejection');
  check(observed.snapshot.surface.width<observed.snapshot.surface.bounds.width,'Actual screenshot exercises scaling');
  const accepted=['click','type','chord','final','key','submit'];
  for(const action of accepted)check(results.find(result=>result.action===action)?.content.includes('"status":"dispatched"'),`${action}: actual desktop bridge dispatches the requested action`);
  check(state.clicks.length===2&&Math.abs(state.clicks[0].x-200)<=1&&Math.abs(state.clicks[0].y-140)<=1,'Native callback verifies scaled screenshot coordinates');
  check(state.cleared_text==='Visual input \u4e2d\u6587 \ud83c\udfb5'&&state.text==='FINAL_VISUAL_VALUE'&&state.submitted&&state.enter_count===1,'Actual native callbacks verify Unicode input, Control+A, current-focus Enter and visual submission');
  check(results.find(result=>result.action==='invalid')?.content.includes('Unsupported named key'),'Unsupported chord is rejected before input');
  check(results.find(result=>result.action==='stale')?.content.includes('NATIVE_WINDOW_TARGET_CHANGED'),'Mutation consumes the screenshot reference');
  check(results.find(result=>result.action==='bounds')?.content.includes('screenshot bounds'),'Screenshot coordinate bounds are enforced');
  check(results.find(result=>result.action==='geometry')?.content.includes('surface moved, resized or changed'),'Moving the window rejects the previously observed screenshot coordinates');
  const control=await app.invoke('native_desktop_status');
  const firstClick=control.trace.find(action=>action.action==='click'&&action.status==='dispatched');
  const surface=observed.snapshot.surface,canvas=observed.snapshot.elements.find(element=>element.automation_id==='VisualCanvas');
  check(Math.abs(firstClick.point.x-(canvas.bounds.x-surface.bounds.x+200))<=1&&Math.abs(firstClick.point.y-(canvas.bounds.y-surface.bounds.y+140))<=1,'Visible action trace uses native coordinates matching the actual screenshot click');
  check(!JSON.stringify(app.requests).includes('"runtime_id"')&&!JSON.stringify(app.requests).includes('"signature"'),'OS target identities stay owned by the executor');
  await writeFile(path.join(run,'acceptance.json'),JSON.stringify({passed:checks.length,checks,state,results,control,executable:app.exe},null,2));
  console.log(`Desktop visual PASS: ${checks.length} checks; ${run}`);
} catch(error) {
  await writeFile(path.join(run,'failure.json'),JSON.stringify({error:String(error),checks,results,errors:app.errors},null,2));throw error;
} finally {try{await app.close();}finally{await stopOwnedProcess(fixture);}}
});
