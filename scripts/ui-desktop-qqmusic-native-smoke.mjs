/* global window */
// A real local vision model chooses every action in the user's selected QQ Music window.
// Playback must be checked independently afterwards; model completion alone is not acceptance.
import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import path from 'node:path';
import {nativeApp,until,textReply} from './fixtures/native-app.mjs';
import {withTask} from './artifact-lifecycle.mjs';

await withTask({suite:'ui-desktop-qqmusic-native-smoke'},async task=>{
assert(process.platform==='win32'&&process.argv[2]&&process.argv[3],'Pass a fresh kzapp.exe and an ollama:model route');
const model=process.argv[3];assert(model.startsWith('ollama:'));
const output=task.evidence;
const app=await nativeApp(process.argv[2],output,'QQ 音乐真实模型验收',async(_body,res)=>textReply(res,'No candidates.'),
  '[providers.ollama]\nprotocol="openai"\nbase_url="http://127.0.0.1:11434/v1"\ncontext_limit=262144\n[limits]\nmax_steps=24\n');
let line,outcome;
try {
  line=await app.invoke('process_create',{projectDir:app.project,profile:'dev-auto',model,reasoning:'low',subagentMode:'off'});
  await app.page.evaluate(async id=>{const sessions=await import('./09-sessions.js');await sessions.refreshProcesses({force:true});await sessions.switchProcess(id,true);},line.id);
  await app.page.locator('#composer-more').click();await app.page.locator('#native-desktop-choose').click();
  const candidate=app.page.locator('#native-desktop-dialog .native-window-list button').filter({hasText:'QQMusic.exe'});
  await candidate.waitFor({state:'visible'});
  assert.equal(await candidate.count(),1,'Select exactly one returned QQ Music window');await candidate.click();
  await app.page.locator('#native-desktop-dialog').waitFor({state:'hidden'});
  await app.invoke('run_prompt',{projectDir:app.project,processId:line.id,profile: "dev-pair",agent:'dev-pair',model,autonomous:true,autoAllow:true,
    prompt:'请用 desktop 原生窗口工具，在我已经选择的 QQ 音乐里搜索并播放 ROTH BART BARON 的 Happy。先观察当前窗口。若 UIA 树没有搜索框，用返回截图的 surface 点击搜索框，重新 inspect 确认焦点，再用 type 输入和 key 发送 Control+A、Enter 等组合键。每次修改后重新观察，使用最新快照的原始 id。只操作这个音乐窗口，不用终端、浏览器或其他窗口替代。结束前确认歌曲和歌手正确，并且已开始播放；如果看不出播放状态，明确说明没有验证。'});
  let previous='';
  await until(async()=>{
    assert.equal(app.errors.length,0,app.errors.join('; '));
    outcome=(await app.invoke('collaboration_snapshot',{projectDir:app.project})).find(row=>row.process_id===line.id);
    const control=await app.invoke('native_desktop_status');
    const progress=JSON.stringify({status:outcome?.status,running:outcome?.running,phase:control.phase,trace:control.trace});
    if(progress!==previous){previous=progress;await writeFile(path.join(app.run,'progress.json'),progress);console.log(progress);}
    return outcome&&!outcome.running&&['completed','failed','stopped'].includes(outcome.status);
  },'Actual model reaches a durable outcome',900000);
  const history=await app.invoke('conversation_display_get',{projectDir:app.project,processId:line.id});
  await writeFile(path.join(app.run,'observation.json'),JSON.stringify({model,line,outcome,history,control:await app.invoke('native_desktop_status'),executable:app.exe,scope:'Real local model; every tool action chosen by the model; independent playback observation still required'},null,2));
  console.log(`Real QQ Music model run recorded: ${app.run}; status=${outcome.status}`);
  assert.equal(outcome.status,'completed');
} catch(error) {
  const history=line?await app.invoke('conversation_display_get',{projectDir:app.project,processId:line.id}):[];
  await writeFile(path.join(app.run,'failure.json'),JSON.stringify({error:String(error),model,line,outcome,history,diagnostics:app.diagnostics},null,2));throw error;
} finally {await app.close();}
});
