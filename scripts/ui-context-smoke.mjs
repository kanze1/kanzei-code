/* global window, document */
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdir,writeFile} from 'node:fs/promises';
import {chromium} from 'playwright-core';
import {startPreviewServer} from './ui-preview/server.mjs';
import {contextGroups,requestSources} from '../crates/kanzei-app/ui/07-context-model.js';

const output='output/playwright/context';await mkdir(output,{recursive:true});
const server=await startPreviewServer({port:0});const browser=await chromium.launch({channel:'msedge',headless:true});
const page=await browser.newPage({viewport:{width:1500,height:1050}}),errors=[],checks=[];
page.on('pageerror',error=>errors.push(String(error)));
const texts=new Map();
function block(kind,role,label,text,source=null){const sha256=createHash('sha256').update(text).digest('hex');texts.set(sha256,text);return{position:0,kind,role,label,sha256,chars:[...text].length,bytes:Buffer.byteLength(text),preview:text.slice(0,180),inspectable:true,source};}
const blocks=[block('system','system','agent/system','规则原文 v2 · <script>must stay inert</script>'),block('system','system','core/preferences','项目约束片段'),block('message','user','messages/1/1','请参考 https://provided.example/repo'),block('tool_call','assistant','webfetch','{"url":"https://source.example/article"}',{call_id:'read-1',tool:'webfetch',input:{url:'https://source.example/article'}}),block('tool_result','user','webfetch','读取片段 v1 · 第 10–12 行',{call_id:'read-1',tool:'webfetch',input:{url:'https://source.example/article'},observation:{kind:'web',location:'https://source.example/article'}}),block('tool_schema','tool','webfetch','{"name":"webfetch"}')];blocks.forEach((value,index)=>value.position=index);
const old=block('system','system','agent/system','规则原文 v1');
const manifest={format_version:1,model:'fixture',protocol:'OpenAiChat',wire_sha256:'a'.repeat(64),wire_bytes:500,estimated_tokens:1234,message_count:2,repeated_results:1,omitted_bytes:200,contributions:[],adjustments:[{kind:'pruned',before_tokens:2000,after_tokens:1234}],source_reads:[{key:'optional/missing',status:'failed',refresh_each_step:false,error:'Optional source unavailable'}],blocks};
manifest.execution={cwd:'C:/project/worktree',project_root:'C:/project',source:{head:'frozen-head',fingerprint:'source-hash'},python:{executable:'C:/project/.venv/Scripts/python.exe'}};manifest.provider_channel='fixture-route';manifest.request_parameters={model:'api-model',reasoning_effort:'high'};
const web=[{acquisition:{id:'acquired',final_url:'https://acquired.example/not-in-request',original_url:'https://acquired.example/not-in-request',fetched_at_utc:'2026-10-22T03:00:00Z',artifact:{sha256:'b'.repeat(64),path:'.kanzei/artifacts/web/original.md'}}}];
assert.equal(contextGroups(manifest).flatMap(group=>group.blocks).length,blocks.length);
assert.equal(requestSources(manifest,web).find(source=>source.acquisition)?.included,false);checks.push('acquisition does not fabricate request inclusion');
assert.equal(requestSources({...manifest,blocks:[blocks[4],{...blocks[4],sha256:'another-version'}]},[]).length,1);checks.push('repeated resource observations retain versions under one source');
try{
 await page.goto(server.origin+'/?scene=chat&theme=dark');await page.waitForFunction(()=>window.__kzPreview?.ready);
 await page.evaluate(async({manifest,web,old,texts})=>{
  const shell=await import('/03-shell.js');window.contextScope={sessionId:shell.activeSessionId,projectDir:shell.currentProject,processId:shell.activeProcessId};
  window.contextReplies={manifest,web,old,texts};
  window.__kzPreview.setCommand('context_snapshot',args=>({sessionId:window.contextScope.sessionId,requests:[{sequence:20,step:2,model:'fixture'},{sequence:10,step:1,model:'fixture'}],selected:{sequence:args.sequence??20,at:1792638000000,turnId:'fixture-run',step:args.sequence===10?1:2,manifest:args.sequence===10?{...manifest,blocks:[old]}:manifest},web}));
  window.__kzPreview.setCommand('context_content',args=>window.contextReplies.texts[args.sha256]);
  shell.setCtxTokens(1234);shell.setCtxLimit(128000);shell.renderTokens();
 },{manifest,web,old,texts:Object.fromEntries(texts)});
 await page.evaluate(async()=>{const shell=await import('/03-shell.js');shell.setCtxTokens(0);shell.renderTokens();});assert.equal(await page.locator('#status-tokens').isVisible(),true);checks.push('context remains inspectable before provider usage is available');
 await page.evaluate(async()=>{const shell=await import('/03-shell.js');shell.setCtxTokens(1234);shell.renderTokens();});
 await page.locator('#status-tokens').click();await page.locator('.context-request-select').waitFor();
 assert.equal(await page.locator('#context-detail').isVisible(),true);assert.equal(await page.locator('.context-assembly-group').count(),9);checks.push('existing context entry renders real assembly groups');
 const identity=page.locator('.context-assembly-group').filter({hasText:'运行快照'}).first();await identity.locator('summary').click();assert.match(await identity.innerText(),/frozen-head/);assert.match(await identity.innerText(),/api-model/);checks.push('execution workspace, source identity and actual request parameters remain inspectable');
 await page.locator('.context-assembly-group').filter({hasText:'Agent 规则与运行策略'}).locator('summary').first().click();
 await page.locator('.context-block').filter({hasText:'系统提示词'}).locator('summary').click();
 await page.locator('.context-original').filter({hasText:'规则原文 v2'}).waitFor();assert.equal(await page.locator('#context-detail script').count(),0);checks.push('source content is exact snapshot text and cannot execute markup');
 await page.screenshot({path:output+'/assembly-dark.png',fullPage:true});
 await page.locator('[data-context-tab="sources"]').click();await page.getByText('消息中的链接 · 尚无对应正文读取记录').waitFor();
 assert.match(await page.locator('.context-tab-body, #context-tab-body').innerText(),/已获取原文 · 纳入状态未记录/);checks.push('provided links and independent acquisition receipts stay distinct');
 await page.locator('[data-context-tab="assembly"]').click();
 await page.locator('.context-request-select').selectOption('10');await page.waitForFunction(()=>document.querySelectorAll('.context-block').length===1);
 await page.locator('.context-assembly-group').filter({has:page.locator('.context-block')}).first().locator('summary').first().click();await page.locator('.context-block summary').click();await page.getByText('规则原文 v1',{exact:true}).waitFor();checks.push('historical request loads its original version');
 await page.evaluate(()=>window.__kzPreview.emit('kz:context-request',{sessionId:window.contextScope.sessionId,sequence:21}));await page.locator('.context-new-request').waitFor();
 assert.equal(await page.locator('.context-request-select').inputValue(),'10');assert.equal(await page.getByText('规则原文 v1',{exact:true}).count(),1);checks.push('new requests do not replace inspected historical evidence');
 await page.locator('.context-new-request').click();await page.waitForFunction(()=>document.querySelector('.context-request-select')?.value==='20');
 await page.locator('.context-tabs button').last().focus();await page.locator('.context-tabs button').last().press('ArrowLeft');assert.equal(await page.locator('[data-context-tab="sources"]').getAttribute('aria-selected'),'true');checks.push('keyboard tab navigation is available');
 for(const theme of ['light','dark'])for(const width of [1500,760,420]){
  await page.setViewportSize({width,height:1050});await page.evaluate(theme=>document.documentElement.dataset.theme=theme,theme);
  const rect=await page.locator('#context-detail').boundingBox();assert(rect&&rect.x>=-1&&rect.x+rect.width<=width+1,JSON.stringify({theme,width,rect}));
 }
 await page.screenshot({path:output+'/sources-narrow.png',fullPage:true});checks.push('context surface stays within the viewport across 6 theme/width combinations');
 await page.locator('#prompt').fill('未发送的草稿');
 await page.locator('#status-tokens').click();
 await page.evaluate(()=>window.__kzPreview.setCommand('context_snapshot',()=>new Promise(resolve=>window.resolveOldContext=resolve)));
 await page.locator('#status-tokens').click();await page.waitForFunction(()=>typeof window.resolveOldContext==='function');
 await page.evaluate(async()=>{const history=await import('/15-views-misc.js');history.bumpConversationEpoch(window.contextScope.sessionId);window.resolveOldContext({sessionId:window.contextScope.sessionId,requests:[],selected:null,web:[]});});
 assert.equal(await page.locator('#context-detail').isVisible(),false);assert.equal(await page.locator('#prompt').inputValue(),'未发送的草稿');checks.push('late reads after a new segment cannot restore old context or overwrite the draft');
 await page.evaluate(()=>window.__kzPreview.setCommand('context_snapshot',()=>({sessionId:window.contextScope.sessionId,requests:[],selected:null,web:[]})));
 await page.locator('#status-tokens').click();await page.getByText('这段对话尚未记录请求装配清单。后续请求会在这里保留来源、顺序和内容版本。').waitFor();checks.push('legacy conversations show missing evidence explicitly');
 await page.evaluate(async()=>{const shell=await import('/03-shell.js');shell.setActiveSessionId('another-conversation');});assert.equal(await page.locator('#context-detail').isVisible(),false);checks.push('switching conversation closes the old context surface');
 assert.deepEqual(errors,[]);checks.push('production ESM has no browser runtime errors');
 await writeFile(output+'/acceptance.json',JSON.stringify({checks,errors},null,2));console.log(JSON.stringify({checks,errors},null,2));
}finally{await browser.close();await server.close();}
