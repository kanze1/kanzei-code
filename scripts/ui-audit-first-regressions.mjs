import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import {parse} from 'acorn';
const dir='crates/kanzei-app/ui/';
function fn(file,name){const text=fs.readFileSync(dir+file,'utf8');const ast=parse(text,{ecmaVersion:'latest',sourceType:'module'});for(let n of ast.body){if(n.type==='ExportNamedDeclaration')n=n.declaration;if(n?.type==='FunctionDeclaration'&&n.id.name===name)return text.slice(n.start,n.end)}throw Error(name)}
// Reuse the existing repository DOM fixture, unchanged; product function bodies come from current sources.
const fixture=fs.readFileSync('scripts/ui-general-chat-owner-smoke.mjs','utf8');
const elAst=parse(fixture,{ecmaVersion:'latest',sourceType:'module'}).body.find(n=>n.type==='ClassDeclaration'&&n.id.name==='Element');
const Element=vm.runInNewContext(fixture.slice(elAst.start,elAst.end)+';Element');
Object.defineProperty(Element.prototype,'innerHTML',{set(value){this.replaceChildren();this.ownText=String(value)},get(){return this.ownText}});
const document={createElement:t=>new Element(t),createTextNode:t=>({textContent:t}),querySelector:()=>null};
const results=[];
{
  const sent=[],msg=new Element(),body=new Element(),actions=new Element();body.className='message-body';actions.className='msg-actions';msg.append(body,actions);
  const ctx=vm.createContext({document,t:x=>x,renderErrorDetail:()=>new Element(),addMessage:()=>msg,activeSessionId:'A',lastRequest:{prompt:'A original',attachments:['A.txt']},sendText:(...x)=>sent.push(x)});
  vm.runInContext(fn('05-chat-render.js','addErrorMessage'),ctx);
  ctx.addErrorMessage('timeout',{retryable:true});
  ctx.lastRequest={prompt:'B unrelated',attachments:['B.txt']};
  await msg.querySelector('.retry-btn').click();
  assert.equal(sent[0][0],'A original');assert.equal(sent[0][1].promptAttachments[0],'A.txt');
  ctx.activeSessionId='B';await msg.querySelector('.retry-btn').click();assert.equal(sent.length,1);
  results.push({id:'AF-R01',observed:'Retry retains the failed A request and refuses a click from B',evidence:sent});
}
{
  const calls=[],body=new Element();
  const ctx=vm.createContext({document,body,t:x=>x,scope:{project:'project-A',process:'conversation-A'},node:(tag,cls,text='')=>{const e=new Element(tag);e.className=cls;e.textContent=text;return e},invoke:async(...x)=>calls.push(x),refresh:async()=>{},status:{}});
  vm.runInContext(fn('28-async-workspace.js','args')+'\n'+fn('28-async-workspace.js','renderLogs'),ctx);
  ctx.renderLogs([{id:'bg-1',running:true,subscribed:false,command:'A command',output:''}]);
  ctx.scope={project:'project-B',process:'conversation-B'};
  await body.querySelector('.mini').onclick();
  assert.equal(calls[0][1].projectDir,'project-A');assert.equal(calls[0][1].id,'bg-1');
  results.push({id:'AF-R02',observed:'A terminal row retains its A owner after scope changes',evidence:calls});
}
{
  const ctx=vm.createContext({document,t:x=>x,highlightLine:(node,text)=>{node.textContent=text},compactDiffLines:x=>x});
  vm.runInContext(fn('06-activity.js','renderDiff'),ctx);
  const result=ctx.renderDiff({path:'x.rs',lines:[{kind:'ctx',text:'unchanged',old_line:10,new_line:12}]});
  await result.querySelector('.mini').click();
  const numbers=result.querySelectorAll('.diff-line-number').map(x=>x.textContent);
  assert.equal(numbers.join(','),'10,12');
  results.push({id:'AF-R03',observed:'Side-by-side diff uses each version line number',evidence:numbers});
}
{
  const calls=[],confirms=[];let gates=0;
  const ctx=vm.createContext({document,window:{},console,t:x=>x,localizedStage:x=>x,Option:function(label,value){const el=new Element('option');el.textContent=label;el.value=value;return el},
    parseUnifiedDiff:()=>[],buildDiffTree:()=>new Element(),renderDiff:()=>new Element(),refreshDocs:()=>{},refreshLines:()=>{},refreshWorktrees:()=>{},refreshGit:()=>{},lineName:()=>'',log:()=>{},toastError:()=>{},
    confirmWorktreeMerge:async()=>true,confirmDialog:async x=>{confirms.push(x);return false},
    invoke:async(command,args)=>{calls.push(command);if(command==='worktree_harvest_candidates')return [];if(command==='worktree_diff')return{files:[],diff:''};if(command==='worktree_gate'){if(++gates===1)return[{name:'test',ok:true}];throw Error('gate execution failed')}if(command==='worktree_merge')return'merged';return[]}});
  vm.runInContext(fn('20-lines.js','harvestClaimId')+'\n'+fn('20-lines.js','buildHarvestPanel'),ctx);
  const panel=ctx.buildHarvestPanel({worktree_path:'worktree-A',label:'A',branch:'branch-A'},'project-A','A');
  await panel.querySelector('.harvest-diff-load').click();await panel.querySelector('.harvest-read-confirm').click();
  await panel.querySelector('.harvest-gate-run').click();await panel.querySelector('.harvest-gate-run').click();
  await panel.querySelector('.harvest-merge-run').click();
  assert(!calls.includes('worktree_merge'));assert.equal(confirms.length,0);assert.equal(panel.querySelector('.harvest-merge-run').disabled,true);
  panel.querySelector('.harvest-merge-run').disabled=false;
  await panel.querySelector('.harvest-merge-run').click();
  assert.equal(confirms.length,1);assert(!calls.includes('worktree_merge'));
  results.push({id:'AF-R04',observed:'A failed gate rerun revokes earlier success and cannot silently merge',evidence:calls});
}
{
  let resolveCapture;
  const original=[],next=[];
  const ctx=vm.createContext({attachments:original,renderAttachments:()=>{},capture:()=>new Promise(r=>{resolveCapture=r}),t:x=>x,toast:()=>{},Date});
  vm.runInContext(fn('08-compose-runtime.js','addPngAttachment')+'\n'+fn('24-preview.js','captureToChat'),ctx);
  const pending=ctx.captureToChat();
  ctx.attachments=next;
  resolveCapture({png:'A-screenshot'});
  await pending;
  assert.equal(original.length,0);assert.equal(next.length,0);
  const currentCapture=ctx.captureToChat();resolveCapture({png:'B-screenshot'});await currentCapture;assert.equal(next[0].data,'B-screenshot');
  results.push({id:'AF-R07',observed:'A delayed screenshot cannot enter B attachments; current owner capture succeeds',evidence:{original,next}});
}
{
  const ctx=vm.createContext({t:x=>x,DOC_LINK:/\[`?([a-z0-9][a-z0-9_-]*\.md)`?\]/});
  vm.runInContext(fn('19-arch.js','parseArchIndex'),ctx);
  const parsed=ctx.parseArchIndex(['## live_design','- [example.md](../../../docs/design/example.md): example']);
  assert(parsed.indexed.has('example.md'));assert.equal(parsed.groups[0].items[0],'example.md');
  const flat=ctx.parseArchIndex(['- [example.md](../../../docs/design/example.md): example']);assert.equal(flat.groups[0].items[0],'example.md');
  results.push({id:'AF-R08',observed:'Ordinary and ungrouped valid Markdown links remain visible',evidence:{indexed:[...parsed.indexed],groups:parsed.groups}});
}
fs.mkdirSync('output/audit-first/root',{recursive:true});
fs.writeFileSync('output/audit-first/root/fix-ui-results.json',JSON.stringify({baseline:'55eaca24',method:'AST-extracted current product functions; owner switching, gate failure, and parser regression assertions',results},null,2)+'\n');
console.log(JSON.stringify(results,null,2));
