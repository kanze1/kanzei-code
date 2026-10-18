/* global window, document, getComputedStyle */
// Real browser rendering of the production control view; no desktop input.
import assert from 'node:assert/strict';
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import {chromium} from 'playwright-core';
const output=path.resolve('output/playwright/native-control/view');
await mkdir(output,{recursive:true});
const root=path.resolve('crates/kanzei-app/ui');
const pageHtml=`<!doctype html><html><head><link rel="stylesheet" href="/native-control.css"></head><body class="native-control-surface"><main id="control" class="native-control native-control-floating"></main><div id="highlight" class="native-control-highlight"></div><script type="module">import {renderNativeControl,renderNativeHighlight} from '/native-control-view.js';window.stops=[];window.dismissed=[];window.render=state=>{renderNativeControl(document.getElementById('control'),state,{compact:true,stop:id=>window.stops.push(id),dismiss:id=>window.dismissed.push(id)});renderNativeHighlight(document.getElementById('highlight'),state);};</script></body></html>`;
const server=http.createServer(async(req,res)=>{
  const file=req.url==='/'?null:req.url?.slice(1);
  if(file && !['native-control.css','native-control-view.js'].includes(file)){res.writeHead(404);res.end();return;}
  res.writeHead(200,{'Content-Type':file?.endsWith('.js')?'text/javascript':file?'text/css':'text/html'});
  res.end(file?await readFile(path.join(root,file)):pageHtml);
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const browser=await chromium.launch({channel:'msedge',headless:true});
const checks=[],check=(value,label)=>{assert(value,label);checks.push(label);};
try{
  const page=await browser.newPage({viewport:{width:510,height:240}}),errors=[];page.on('pageerror',error=>errors.push(String(error)));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);await page.waitForFunction(()=>typeof window.render==='function');
  const state={revision:1,phase:'running',owner:{id:'original-control',title:'Owned test form'},trace:[{id:1,action:'invoke',target:'Submit form',status:'dispatched',bounds:{x:10,y:10,width:200,height:80},point:null}]};
  await page.evaluate(state=>window.render(state),state);
  check(await page.getByText('Kanzei 正在控制电脑').isVisible(),'Control ownership is visible without opening a menu');
  check(await page.getByText('Owned test form').isVisible(),'Actual target title is visible');
  check(await page.getByText('已发送，待核对').isVisible(),'A dispatched action is not labelled verified');
  check(await page.locator('.native-control-click').count()===1,'Actual click dispatch produces a position ripple');
  check(await page.locator('.native-control-dot').evaluate(node=>getComputedStyle(node).animationName)==='native-control-breathe','Running has a live control indicator');
  await page.locator('.native-control-stop').click();
  check(JSON.stringify(await page.evaluate(()=>window.stops))==='["original-control"]','Stop sends the displayed capability identity');
  await page.evaluate(()=>window.oldStop=document.querySelector('.native-control-stop'));
  await page.evaluate(state=>window.render({...state,owner:{id:'new-control',title:'Other form'},revision:2}),state);
  await page.evaluate(()=>{window.oldStop.disabled=false;window.oldStop.click();});
  check((await page.evaluate(()=>window.stops)).at(-1)==='original-control','A detached old button cannot redirect stop to a new owner');
  await page.evaluate(state=>window.render({...state,phase:'stopping'}),state);
  check(await page.locator('.native-control-stop').isDisabled(),'Stopping remains a distinct state until cleanup completes');
  check(await page.locator('#highlight').isHidden(),'Stopping clears the operation overlay');
  await page.evaluate(state=>window.render({...state,phase:'stop_failed',error:'Cleanup not acknowledged'}),state);
  check(await page.getByText('停止尚未确认').isVisible()&&await page.getByRole('alert').isVisible(),'Failed stop stays visible and actionable');
  await page.evaluate(state=>window.render({...state,phase:'stopped'}),state);
  check(await page.getByText('控制已停止').isVisible(),'Confirmed stop is distinguished from a pending request');
  await page.getByRole('button',{name:'收起'}).click();
  check((await page.evaluate(()=>window.dismissed))[0]==='original-control','Only an ended control can be dismissed');
  await page.emulateMedia({reducedMotion:'reduce'});await page.evaluate(state=>window.render(state),state);
  check(await page.locator('.native-control-dot').evaluate(node=>getComputedStyle(node).animationName)==='none','Reduced motion suppresses decorative movement');
  await page.evaluate(state=>window.render({...state,owner:{...state.owner,title:'<img src=x onerror=alert(1)>'}}),state);
  check(await page.locator('img').count()===0,'Window titles remain text, never executable markup');
  check(errors.length===0,'Control view has no browser runtime errors');
  await page.evaluate(state=>window.render(state),state);await page.screenshot({path:path.join(output,'control-view.png')});
  await writeFile(path.join(output,'acceptance.json'),JSON.stringify({checks,errors,scope:'Real Edge rendering of production view module; native execution is a separate final-binary suite'},null,2));
  console.log(`Native control view PASS: ${checks.length} checks`);
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
