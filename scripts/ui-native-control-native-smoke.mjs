/* global window, document */
// Published-binary acceptance: actual native input, visible surfaces and OS Escape.
import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import path from 'node:path';
import {nativeApp,until,toolReply,textReply,stopOwnedProcess} from './fixtures/native-app.mjs';
import {decodeExternalSource} from './fixtures/tool-source.mjs';
const exec=promisify(execFile),output=path.resolve('output/playwright/native-control/native'),stamp=String(Date.now());
await mkdir(output,{recursive:true});
const fixtureRoot=path.join(output,`form-${stamp}`),windowFile=path.join(fixtureRoot,'window.json');await mkdir(fixtureRoot);
const cases=new Map(),checks=[],observations=[],check=(value,label)=>{assert(value,label);checks.push(label);};
const deferred=()=>{let complete;const entry={ready:false},promise=new Promise(done=>{complete=done;});return Object.assign(entry,{promise,resolve:()=>{entry.ready=true;complete();}});};
const sequences={trajectory:['inspect','fill','inspect','invoke','inspect'],escape:['inspect','key','inspect'],button:['inspect','fill','inspect'],normal:['inspect','fill','inspect'],inflight:['inspect','key','inspect','fill']};
let form;
const app=await nativeApp(process.argv[2],output,'原生控制体验验收',async(body,res)=>{
  const index=body.messages?.findLastIndex(message=>message.role==='user'&&typeof message.content==='string'&&message.content.startsWith('NATIVE_CONTROL_CASE:'));
  if(index<0 || index===undefined){textReply(res,'No candidates.');return;}
  const mode=body.messages[index].content.split(':')[1],entry=cases.get(mode),tools=body.messages.slice(index+1).filter(message=>message.role==='tool'),sequence=sequences[mode];
  if(!tools.length){toolReply(res,'tool_search',{query:'select:desktop'},0);return;}
  if(tools.length>1){
    const result=decodeExternalSource(tools.at(-1).content);entry.results.push({action:sequence[tools.length-2],...result});
    if(!result.content.startsWith('[tool')){const value=JSON.parse(result.content.split('\n[tool-image] ')[0]);if(value.snapshot)entry.snapshot=value;}
  }
  const gate=mode==='trajectory'?5:4;
  if(mode!=='inflight' && tools.length===gate){entry.arrived.resolve();await entry.continue.promise;}
  let action=sequence[tools.length-1];
  if(!action){textReply(res,'Native workflow ended.');return;}
  if(action==='inspect'){toolReply(res,'desktop',{action},tools.length);return;}
  const snapshot=entry.snapshot,element=snapshot.snapshot.elements.find(element=>element.automation_id===(action==='invoke'?'Submit':'CustomerName'));
  assert(element,'Observed control must exist');
  const input={action,window_id:snapshot.window_id,snapshot_id:snapshot.snapshot_id,element_id:element.id};
  if(action==='fill')input.text=mode==='inflight'?'CONTROL_BLOCK':`CONTROL_${mode.toUpperCase()}`;
  if(action==='key')input.key=mode==='escape'?'Escape':'Enter';
  toolReply(res,'desktop',input,tools.length);
});
const fixture=async()=>JSON.parse(await readFile(path.join(fixtureRoot,'state.json'),'utf8'));
const status=()=>app.invoke('native_desktop_status');
const probe=(request={action:'escape'})=>exec('C:\\Program Files\\PowerShell\\7\\pwsh.exe',['-NoProfile','-File',path.resolve('scripts/fixtures/desktop-keyboard-probe.ps1'),'-WindowFile',windowFile,'-RequestJson',JSON.stringify(request)],{windowsHide:true});
let bar,highlight,previousControl;
try{
  form=spawn('C:\\Program Files\\PowerShell\\7\\pwsh.exe',['-NoProfile','-Sta','-File',path.resolve('scripts/fixtures/desktop-form.ps1'),'-StateDirectory',fixtureRoot],{windowsHide:true,stdio:'ignore'});
  await until(()=>existsSync(windowFile),'Owned form exists');
  const window=JSON.parse(await readFile(windowFile,'utf8'));
  const pages=()=>app.page.context().browser().contexts().flatMap(context=>context.pages());
  await until(()=>{bar=pages().find(page=>page.url().includes('surface=bar'));highlight=pages().find(page=>page.url().includes('surface=highlight'));return bar&&highlight;},'Both native control surfaces exist');
  const surfaceErrors=[];
  for(const page of [bar,highlight]){page.on('pageerror',error=>surfaceErrors.push(String(error)));await page.waitForFunction(()=>document.body.dataset.controlReady==='true');}
  check((await status()).available===true,'Native control requires working surfaces and Escape hook');
  for(const mode of Object.keys(sequences)){
    const line=await app.createConversation(),entry={line,arrived:deferred(),continue:deferred(),results:[],snapshot:null};cases.set(mode,entry);
    await app.page.locator('#composer-more').click();await app.page.locator('#native-desktop-choose').click();
    await app.page.locator('#native-desktop-dialog .native-window-list button').filter({hasText:window.title}).click();
    await app.page.locator('#native-desktop-dialog').waitFor({state:'hidden'});
    await app.invoke('run_prompt',{projectDir:app.project,processId:line.id,prompt:`NATIVE_CONTROL_CASE:${mode}`,profile:'dev',agent:'dev-pair',model:'stub:test',autonomous:true,autoAllow:true});
    if(mode==='inflight'){
      await until(async()=>(await fixture()).blocked===true&&(await status()).helper_pid,'A real native input helper is inside the blocked control callback',60000);
      const active=await status(),pid=active.helper_pid;process.kill(pid,0);
      check(active.phase==='running','In-flight helper belongs to active control');
      await probe();
      await until(async()=>{const value=await status();return value.phase==='stopped'&&value.helper_pid===null;},'Escape acknowledges only after the OS input helper exits');
      assert.throws(()=>process.kill(pid,0),{code:'ESRCH'});checks.push('Independent OS observation confirms helper process exited before stopped');
      check((await status()).trace.some(action=>action.action==='fill'&&action.status==='cancelled'),'The blocked native action is cancelled rather than failing before emergency stop');
      await probe({action:'release-block'});
      await until(async()=>(await fixture()).blocked===false,'Owned form callback released');
      check((await fixture()).current_name==='CONTROL_BLOCK','Interrupted dispatch is retained as an observed partial result, never rolled back or replayed');
    }else{
      await until(()=>entry.arrived.ready,`${mode}: provider reached the expected native checkpoint`,90000);
      const active=await status();
      check(active.phase==='running'&&active.owner.process_id===line.id,`${mode}: native control owns the actual conversation`);
      check(await bar.getByText('Kanzei 正在控制电脑').isVisible(),`${mode}: control bar remains visible over the target app`);
      if(mode==='trajectory'){
        check((await fixture()).submitted&&(await fixture()).name==='CONTROL_TRAJECTORY','Actual form callback confirms submission');
        check(active.trace.some(action=>action.action==='fill'&&action.status==='dispatched'),'Real fill dispatch is recorded without claiming task verification');
        check(active.trace.at(-1).action==='invoke'&&active.trace.at(-1).status==='dispatched','Actual native click drives the action ledger');
        check(await highlight.locator('.native-control-outline').count()===1,'Actual observed target has an independent visual overlay');
        await bar.screenshot({path:path.join(output,'control-bar.png')});await highlight.screenshot({path:path.join(output,'control-target.png')});
        entry.continue.resolve();
        await until(async()=>(await app.invoke('collaboration_snapshot',{projectDir:app.project})).find(row=>row.process_id===line.id)?.status==='completed','Trajectory workflow completed');
        await until(async()=>(await status()).phase==='ended','Normal terminal event ends control without claiming stopped');
      }else{
        if(mode==='escape'){
          check((await fixture()).escape_count===1,'Executor-tagged Escape reaches the form and does not trigger emergency stop');
          await probe();
        }else if(mode==='button'){
          const other=await app.createConversation();
          const rejection=await app.page.evaluate(async controlId=>{try{await window.__TAURI__.core.invoke('native_desktop_stop',{controlId});return null;}catch(error){return String(error);}},previousControl);
          check(Boolean(rejection)&&((await status()).owner.id===active.owner.id),'Stale stop cannot cancel a newer control');
          const point=await bar.getByRole('button',{name:'停止控制'}).evaluate(button=>{const bounds=button.getBoundingClientRect();return{x:(bounds.x+bounds.width/2)/window.innerWidth,y:(bounds.y+bounds.height/2)/window.innerHeight};});
          await probe({action:'click-control',agent_pid:app.child.pid,title:await bar.title(),...point});
          const otherState=(await app.invoke('collaboration_snapshot',{projectDir:app.project})).find(row=>row.process_id===other.id);
          check(!otherState?.running,'Control-bar stop does not target the conversation currently displayed in the main window');
        }else await app.page.locator('#stop').click();
        await until(async()=>{const value=await status();return value.phase==='stopped'&&value.helper_pid===null;},`${mode}: native stop and cancellation converged`);
        if(mode==='button')check(JSON.parse((await probe({action:'foreground'})).stdout).owned_form_foreground,'An actual OS mouse click stops control without stealing the target foreground');
        check(await app.invoke('native_desktop_get',{processId:line.id})===null,`${mode}: stop revokes the selected window capability`);
        previousControl=active.owner.id;
        if(mode==='escape'){
          check((await fixture()).escape_count===1,'Emergency Escape is consumed before reaching the target form');
          await probe();await until(async()=>(await fixture()).escape_count===2,'Escape is released to the foreground app after control stops');
        }
        entry.continue.resolve();
      }
    }
    await until(async()=>!(await app.invoke('collaboration_snapshot',{projectDir:app.project})).find(row=>row.process_id===line.id)?.running,`${mode}: runtime is no longer running`);
    const final=await status();observations.push({mode,control:final,fixture:await fixture(),results:entry.results});
    if(mode!=='trajectory')check(final.phase==='stopped',`${mode}: late provider completion does not revive native control`);
  }
  check(surfaceErrors.length===0,'Both overlay surfaces have no JavaScript errors');
  check(app.errors.length===0,'Main native application has no JavaScript errors');
  const executableSha=createHash('sha256').update(await readFile(app.exe)).digest('hex');
  await app.invoke('runtime_shutdown');
  const mainClosed=app.page.waitForEvent('close');
  const [destruction,closure]=await Promise.allSettled([app.page.evaluate(()=>window.__TAURI__.window.getCurrentWindow().destroy()),mainClosed]);
  assert.equal(closure.status,'fulfilled','Destroy must close the main window');
  if(destruction.status==='rejected')assert.equal(destruction.reason.message,'page.evaluate: Target page, context or browser has been closed','Only the destroyed execution context can interrupt the destroy receipt');
  await until(()=>app.page.isClosed()&&bar.isClosed()&&highlight.isClosed(),'Closing main destroys both auxiliary surfaces');
  await until(()=>app.child.exitCode!==null,'Closing main exits its OS process');
  checks.push('Closing the main window also exits the auxiliary control UI');
  await writeFile(path.join(output,'acceptance.json'),JSON.stringify({checks,observations,surfaceErrors,fixtureRoot,appRun:app.run,executable:app.exe,executable_sha256:executableSha,scope:'Actual WinForms callbacks, tagged executor input, independent OS Escape, in-flight helper exit, visible WebView surfaces and detached runtime cancellation; one execution of each distinct scenario'},null,2));
  console.log(`Native control experience PASS: ${checks.length} checks`);
}catch(error){await writeFile(path.join(output,'failure.json'),JSON.stringify({error:String(error),checks,observations,cases:[...cases].map(([mode,entry])=>({mode,line:entry.line,results:entry.results})),fixtureRoot,appRun:app.run,diagnostics:app.diagnostics},null,2));throw error;}
finally{
  for(const entry of cases.values())entry.continue.resolve();
  try{if(form?.exitCode===null&&existsSync(windowFile))await probe({action:'release-block'});}finally{try{await app.close();}finally{await stopOwnedProcess(form);}}
}
