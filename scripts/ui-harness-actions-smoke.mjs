/* global window */
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { startPreviewServer } from './ui-preview/server.mjs';
const server=await startPreviewServer({port:0,host:'127.0.0.1'});
assert(Number(new URL(server.origin).port)>=10240, 'auto preview port must avoid blocked low ports');
const browser=await chromium.launch({channel:'msedge',headless:true});
try {
 for(const theme of ['dark','light']){
  const page=await browser.newPage({viewport:{width:1280,height:900}});const errors=[];page.on('pageerror',error=>errors.push(String(error)));
  await page.goto(server.origin+`/?scene=chat&theme=${theme}`);await page.waitForFunction(()=>window.__kzPreview?.ready,{timeout:20000});
  await page.evaluate(()=>{let item=null;let revision=0;window.__kzPreview.setCommand('schedule_action',args=>{
   if(args.action==='list')return{tasks:item?[{definition:item,revision:String(revision),next_ms:Date.now()+100000,history:[]}]:[],diagnostics:[]};
   if(args.action==='save'){item=args.definition;revision++;return{saved:true};}
   if(args.action==='toggle'){item.enabled=args.enabled;revision++;return{saved:true};}
   if(args.action==='delete'){item=null;return{deleted:true};}return{queued:true};
  });});
  await page.locator('#workbench-schedules').click();await page.getByRole('button',{name:'新建定时任务',exact:true}).click();
  const dialog=page.locator('#schedules-overlay');await dialog.getByLabel('任务名称',{exact:true}).fill('nightly-review');await dialog.getByLabel('步骤内容',{exact:true}).fill('检查最近的变更');
  await dialog.getByRole('button',{name:'保存',exact:true}).click();await dialog.getByRole('heading',{name:'nightly-review',exact:true}).waitFor();
  await dialog.getByRole('button',{name:'停用',exact:true}).click();await dialog.getByRole('button',{name:'启用',exact:true}).waitFor();
  await dialog.getByRole('button',{name:'编辑',exact:true}).click();await dialog.getByLabel('运行主机',{exact:true}).selectOption('server');await dialog.getByLabel('服务器标识',{exact:true}).fill('ENV-001');
  await dialog.getByLabel('频率',{exact:true}).selectOption('weekly');await dialog.getByLabel('星期',{exact:true}).selectOption('一');
  await page.screenshot({path:`output/playwright/harness-schedule-${theme}.png`});await page.keyboard.press('Escape');assert.equal(await dialog.isVisible(),false);
  await page.evaluate(async()=>{
   const fixture=await window.__kzPreview.fixtures();
   const {switchProcess}=await import('/09-sessions.js');await switchProcess(fixture.ids.idleProcess,true);
   const forked=await window.__TAURI__.core.invoke('process_create',{projectDir:fixture.ids?.project || 'C:/Users/kanzei/Documents/kanzei code',profile:'dev'});
   window.__kzPreview.setCommand('conversation_action',args=>args.action==='preview'?{sourceHash:'preview-version',keptMessages:3,files:[{path:'src/example.rs',restorable:true,pre_exists:true,external_change:false}],unhandled:['Shell / Git effects require separate handling'],worktree:true}:args.action==='fork'?{forked:true,processId:forked.id,prompt:args.text}:{prompt:args.text,skipped:[]});
   window.__kzPreview.setCommand('conversation_compact',{changed:true,before:12000,after:6000,message:'已压缩：12000 → 6000 token'});
   const {addMessage}=await import('/05-chat-render.js');addMessage('user','Harness rewind UI probe');
  });
  const rewind=page.locator('.msg.user').last();await rewind.getByRole('button',{name:'回退或分叉',exact:true}).click();
  const rewindDialog=page.locator('#conversation-action-overlay');await rewindDialog.getByRole('button',{name:'只回退代码',exact:true}).click();
  await page.waitForFunction(()=>window.__kzPreview.calls.some(c=>c.cmd==='conversation_action'&&c.args.action==='code'));
  const codeCall=await page.evaluate(()=>window.__kzPreview.calls.filter(c=>c.cmd==='conversation_action'&&c.args.action==='code').at(-1));assert.equal(codeCall.args.expectedHash,'preview-version');assert.equal(codeCall.args.force,false);
  await rewind.getByRole('button',{name:'回退或分叉',exact:true}).click();await rewindDialog.getByRole('button',{name:'从这里分叉',exact:true}).click();await page.waitForFunction(()=>window.__kzPreview.calls.some(c=>c.cmd==='conversation_action'&&c.args.action==='fork'));
  await page.locator('#prompt').fill('/compact 保留下一步计划');await page.locator('#prompt').press('Enter');await page.waitForFunction(()=>window.__kzPreview.calls.some(c=>c.cmd==='conversation_compact'));
  const compactCall=await page.evaluate(()=>window.__kzPreview.calls.filter(c=>c.cmd==='conversation_compact').at(-1));assert.equal(compactCall.args.focus,'保留下一步计划');
  await page.evaluate(async () => {
   window.__kzPreview.setCommand('run_tool_process_stop', true);
   const {bgAdd}=await import('/06-activity.js');
   bgAdd('single-terminal-probe','process','wait bg123',{action:'wait',id:'bg123'});
  });
  if (!await page.locator('#tasks-panel').isVisible()) await page.locator('#tasks-toggle').click();
  if (await page.locator('#agent-back').isVisible()) await page.locator('#agent-back').click();
  await page.locator('[data-bg-id="single-terminal-probe"] .bg-title').click();
  const stop=page.locator('[data-bg-id="single-terminal-probe"] button').filter({hasText:/^停止$/});
  await stop.click();
  await page.waitForFunction(()=>window.__kzPreview.calls.some(c=>c.cmd==='run_tool_process_stop'));
  const stopCalls=await page.evaluate(()=>window.__kzPreview.calls);
  assert.equal(stopCalls.filter(c=>c.cmd==='run_tool_process_stop').at(-1).args.processId,'bg123');
  assert.equal(stopCalls.filter(c=>c.cmd==='stop_run').length,0,'single terminal stop must not stop the whole run');
  assert.equal(errors.length,0,errors.join('\n'));console.log(`${theme}: 定时任务、回退预览和代码操作、分叉、手动压缩和单个后台终端停止入口通过`);await page.close();
 }
}finally{await browser.close();await server.close();}
