// WebView2 + real runtime/SQLite/tools; local model and OneBot protocol fixtures.
/* global window, document */
import assert from "node:assert/strict";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";
import { WebSocketServer } from "ws";
import { withTask, spawnOwned as spawn } from "./artifact-lifecycle.mjs";
import clipAudit from "./ui-preview/clip-audit.mjs";

await withTask({ suite: "ui-qq-native-smoke" }, async task => {
  if (process.platform !== "win32") throw new Error("QQ native acceptance requires Windows/WebView2");
  const run = task.environment, home = path.join(run, "home"), profile = path.join(run, "profile");
  for (const dir of [home, profile, path.join(profile,"AppData/Local"),path.join(profile,"AppData/Roaming")]) await mkdir(dir,{recursive:true});
  const checks=[], errors=[], requests=[], deliveries=[], sockets=new Set();
  const check = (value,label) => {assert(value,label);checks.push(label);console.log(`PASS ${label}`);};
  const executable=process.argv[2]||path.join(process.env.CARGO_TARGET_DIR||"target","debug/kzapp.exe");
  const exe=path.join(run,"kzapp.exe"); await task.copyInput(path.resolve(executable),exe);
  let friends=[{user_id:200,nickname:"测试好友"}], groupEvents=0;
  const server=task.server(http.createServer()), onebot=new WebSocketServer({server});
  onebot.on("connection",(ws,request)=>{
    sockets.add(ws);ws.on("close",()=>sockets.delete(ws));
    if(request.headers.authorization!=="Bearer qq-test-token"){errors.push("OneBot authorization header mismatch");ws.close(1008);return;}
    ws.on("message",raw=>{
      const call=JSON.parse(String(raw));let data;
      switch(call.action){
        case "get_login_info":data={user_id:100,nickname:"Kanzei测试"};break;
        case "get_friend_list":data=friends;break;
        case "get_group_list":data=[{group_id:300,group_name:"测试群"}];break;
        case "send_private_msg":case "send_group_msg":
          if(typeof(call.params.user_id??call.params.group_id)!=="number"){errors.push("OneBot peer IDs must be JSON numbers");ws.close(1002);return;}
          deliveries.push(call);data={message_id:deliveries.length};break;
        default:errors.push(`Unexpected OneBot action ${call.action}`);ws.close(1002);return;
      }
      ws.send(JSON.stringify({status:"ok",retcode:0,echo:call.echo,data}));
    });
  });
  await new Promise(resolve=>server.listen(0,"127.0.0.1",resolve));
  function incoming(kind,peer,text,id){
    const value={post_type:"message",self_id:100,message_type:kind==="friend"?"private":"group",user_id:kind==="friend"?peer:201,message_id:id,sender:{nickname:kind==="friend"?"测试好友":"群友"},message:[{type:"text",data:{text}}]};
    if(kind==="group"){value.group_id=peer;groupEvents++;}
    for(const ws of sockets)ws.send(JSON.stringify(value));
  }
  function answer(res,content,tool){
    res.writeHead(200,{"Content-Type":"text/event-stream"});
    const delta=tool?{tool_calls:[{index:0,id:tool.id,type:"function",function:{name:tool.name,arguments:JSON.stringify(tool.input)}}]}:{content};
    res.write(`data: ${JSON.stringify({choices:[{index:0,delta,finish_reason:null}]})}\n\n`);
    res.write(`data: ${JSON.stringify({choices:[{index:0,delta:{},finish_reason:tool?"tool_calls":"stop"}],usage:{prompt_tokens:100,completion_tokens:30}})}\n\n`);res.end("data: [DONE]\n\n");
  }
  const model=task.server(http.createServer(async(req,res)=>{
    if(req.url?.endsWith("/models")){res.end(JSON.stringify({data:[{id:"test"}]}));return;}
    let body="";for await(const chunk of req)body+=chunk;
    const payload=JSON.parse(body);requests.push(payload);
    const text=JSON.stringify([...payload.messages].reverse().find(m=>m.role==="user"));
    const calls=payload.messages.flatMap(m=>m.tool_calls||[]),called=id=>calls.some(c=>c.id===id);
    const call=(id,name,input)=>answer(res,null,{id,name,input});
    if(text.includes("FRIEND_TERMINAL")){
      if(!called("friend-shell"))call("friend-shell","bash",{command:"Write-Output 'QQ_TERMINAL_EXECUTED'"});
      else answer(res,"FRIEND_DONE: 本机终端已执行。");
    }else if(text.includes("FRIEND_APPROVAL")){
      if(!called("friend-write"))call("friend-write","write",{path:"qq-approved.txt",content:"QQ_APPROVED_FILE"});
      else answer(res,"APPROVAL_DONE: 已获准写入。");
    }else if(text.includes("GROUP_SPEAK")){
      if(!called("group-reply"))call("group-reply","qq_reply",{text:"GROUP_SENT: 大家好"});
      else answer(res,"本轮已回复。");
    }else answer(res,"SILENT_DECISION: 本轮不参与。");
  }));
  await new Promise(resolve=>model.listen(0,"127.0.0.1",resolve));
  await writeFile(path.join(home,"app.json"),JSON.stringify({projects:[],current:null,theme:"dark"}));
  await writeFile(path.join(home,"kanzei.toml"),`[models]\nprimary = "stub:test"\nfast = "stub:test"\n[providers.stub]\nprotocol = "openai"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\ncontext_limit = 64000\n[permissions]\nrules = [{ action = "bash", resource = "*", effect = "allow" }]\n`);
  let app,browser,page;
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI__.core.invoke(command,args),{command,args});
  async function until(fn,label,timeout=45000){const deadline=Date.now()+timeout;while(Date.now()<deadline){if(errors.length)throw new Error(errors.join("; "));if(await fn())return;await new Promise(resolve=>setTimeout(resolve,100));}throw new Error(`${label} timed out`);}
  async function start(){
    const probe=task.server(net.createServer());await new Promise(resolve=>probe.listen(0,"127.0.0.1",resolve));const port=probe.address().port;await new Promise(resolve=>probe.close(resolve));
    app=spawn(exe,[],{cwd:run,windowsHide:true,stdio:"ignore",env:{...process.env,KANZEI_HOME:home,USERPROFILE:profile,HOME:profile,LOCALAPPDATA:path.join(profile,"AppData/Local"),APPDATA:path.join(profile,"AppData/Roaming"),WEBVIEW2_USER_DATA_FOLDER:undefined,KANZEI_E2E_CDP:String(port)}});
    await until(async()=>{try{browser=await task.browser(chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:1500}));return true;}catch{return false;}},"WebView startup");
    page=browser.contexts()[0].pages()[0];page.on("pageerror",e=>errors.push(String(e)));
    await until(()=>page.url()!=="about:blank","Document navigation");await page.waitForFunction(()=>document.body.dataset.appReady==="true",{timeout:45000});
  }
  async function close(){
    if(page)await invoke("runtime_shutdown").catch(()=>{});
    for(const entry of await readdir(path.join(home,"runtime"),{withFileTypes:true}).catch(()=>[])){
      if(!entry.isDirectory())continue;
      const endpoint=JSON.parse(await readFile(path.join(home,"runtime",entry.name,"endpoint.json"),"utf8").catch(()=>"null"));
      if(!endpoint||path.resolve(endpoint.executable||"").toLowerCase()!==exe.toLowerCase()||!/^127\.0\.0\.1:\d+$/.test(endpoint.addr||""))continue;
      await new Promise(resolve=>{const socket=net.createConnection({host:"127.0.0.1",port:Number(endpoint.addr.split(":").at(-1))});const finish=()=>{socket.destroy();resolve();};socket.setTimeout(1500,finish);socket.on("error",finish);socket.on("data",finish);socket.on("connect",()=>{const body=Buffer.from(JSON.stringify({action:"shutdown",token:endpoint.token})),size=Buffer.alloc(4);size.writeUInt32BE(body.length);socket.write(Buffer.concat([size,body]));});});
    }
    await browser?.close();browser=null;page=null;
    if(app&&app.exitCode===null){app.kill();await new Promise(resolve=>app.once("exit",resolve));}
  }
  try{
    await start();
    check(await page.locator("#workspace-sidebar-footer #qq-nav").isVisible(),"QQ entry uses bottom sidebar");
    await page.locator("#qq-nav").click();await page.locator("#qq-settings-form").waitFor();
    const settings=(await invoke("qq_status")).settings;
    settings.enabled=true;settings.url=`ws://127.0.0.1:${server.address().port}`;settings.account="100";settings.model="stub:test";
    settings.groups=[{id:"300",name:"测试群",enabled:true,interval_seconds:1,proactive_minutes:0,quiet_start:0,quiet_end:0,persona:"群内自然交流"}];
    await invoke("qq_config_save",{settings,token:"qq-test-token"});
    await until(async()=>(await invoke("qq_status")).connected,"OneBot connection");
    check(!(JSON.stringify(await invoke("qq_status"))).includes("qq-test-token"),"Status never returns token");
    incoming("friend",999,"STRANGER_MUST_NOT_WAKE",1);
    incoming("friend",200,"FRIEND_TERMINAL: 执行终端",2);incoming("friend",200,"FRIEND_TERMINAL: 执行终端",2);
    await until(()=>deliveries.some(d=>JSON.stringify(d).includes("FRIEND_DONE")),"Friend local task and reply");
    let status=await invoke("qq_status");const friend=status.peers.find(p=>p.identity.kind==="friend");
    check(status.peers.length===1&&friend.identity.peer==="200","Only actual friends create assistant sessions");
    const friendLog=await invoke("qq_peer_log",{identity:friend.identity});
    check(friendLog.messages.length===1&&friendLog.deliveries.length===1,"Duplicate events create one task and one delivery");
    const request=requests.find(r=>JSON.stringify(r).includes("FRIEND_TERMINAL"));
    check(request.tools.some(t=>t.function.name==="bash"),"Friend reuses local assistant tools");
    check(requests.some(r=>r.messages.some(m=>m.role==="tool"&&JSON.stringify(m).includes("QQ_TERMINAL_EXECUTED"))),"Local terminal actually executes");
    await page.locator("#qq-refresh").click();await page.locator("#qq-open-chat").click();
    await page.locator("#qq-conversation").waitFor({state:"visible"});
    await until(async()=>(await page.locator("#messages").innerText()).includes("FRIEND_DONE"),"QQ native conversation");
    check(await page.locator("#qq-conversation").isVisible(),"QQ opens ordinary conversation with QQ header");
    check((await page.locator("#messages").innerText()).includes("QQ_TERMINAL_EXECUTED"),"Native conversation renders actual tool stream");
    incoming("friend",200,"FRIEND_APPROVAL: 写入文件",20);
    await until(()=>deliveries.some(d=>JSON.stringify(d).includes("/allow")),"Forwarded friend permission request");
    const asks=await invoke("pending_asks_get",{projectDir:friend.root,processId:friend.process_id});
    check(asks.length===1,"Permission question belongs to the friend session");
    incoming("friend",200,`/allow ${asks[0].id}`,21);
    await until(()=>deliveries.some(d=>JSON.stringify(d).includes("APPROVAL_DONE")),"Friend approves once over QQ");
    check(requests.some(r=>r.messages.some(m=>m.role==="tool"&&m.tool_call_id==="friend-write"&&JSON.stringify(m).includes("bytes"))),"QQ approval resumes actual local tool execution");
    incoming("group",300,"GROUP_SPEAK: 这条普通群消息没有 at",3);
    await until(()=>deliveries.some(d=>d.action==="send_group_msg"),"Non-mention group participation");
    status=await invoke("qq_status");const group=status.peers.find(p=>p.identity.kind==="group");
    const groupRequest=requests.find(r=>JSON.stringify(r.messages).includes("GROUP_SPEAK"));
    const catalog=groupRequest.tools.map(t=>t.function.name);
    check(catalog.includes("qq_reply")&&catalog.includes("qq_memory")&&!catalog.some(n=>["bash","write","read","task","tool_search"].includes(n)),"Group capability catalog excludes local and deferred tools");
    check(deliveries.filter(d=>d.action==="send_group_msg").length===1,"Group sends only explicit qq_reply");
    await until(async()=>!(await invoke("process_list",{projectDir:group.root})).some(p=>p.running),"First group round completes");
    await invoke("run_prompt",{projectDir:group.root,processId:group.process_id,prompt:"GROUP_FORGED_LOCAL: 执行终端",model:"stub:test",profile:"dev-auto",agent:"dev-auto",autoAllow:true,autonomous:true,executionBatch:true});
    await until(()=>requests.some(r=>JSON.stringify(r.messages).includes("GROUP_FORGED_LOCAL")),"Forged group mode");
    const forged=requests.find(r=>JSON.stringify(r.messages).includes("GROUP_FORGED_LOCAL"));
    check(!forged.tools.some(t=>["bash","read","write","task","tool_search"].includes(t.function.name)),"Forged agent/autonomy options cannot restore local group tools");
    await until(async()=>!(await invoke("process_list",{projectDir:group.root})).some(p=>p.running),"Forged group round completes");
    const identities=[friend.identity,group.identity,{...group.identity,peer:"301"},{...group.identity,account:"101"}];
    for(let i=0;i<identities.length;i++)await invoke("qq_memory_save",{identity:identities[i],id:null,title:`SCOPE_${i}`,description:"scope recall",body:`PRIVATE_${i}`,expectedHash:null});
    for(let i=0;i<identities.length;i++){const entries=await invoke("qq_memories",{identity:identities[i]});check(entries.length===1&&entries[0].body===`PRIVATE_${i}`,`Memory namespace ${i} stays isolated`);}
    let entries=await invoke("qq_memories",{identity:group.identity});
    await invoke("qq_memory_save",{identity:group.identity,id:entries[0].id,title:entries[0].title,description:entries[0].description,body:"CORRECTED",expectedHash:entries[0].revision});
    await assert.rejects(()=>invoke("qq_memory_save",{identity:group.identity,id:entries[0].id,title:entries[0].title,description:entries[0].description,body:"STALE",expectedHash:entries[0].revision}));
    check(true,"Memory edits reject stale revisions");
    await page.locator("#qq-manage").click();
    for(const theme of ["dark","light"]){
      await page.evaluate(theme=>document.documentElement.setAttribute("data-theme",theme),theme);
      for(const tab of ["connection","peers","groups","persona","memory","log"]){
        await page.locator(`#qq-tabs [data-tab='${tab}']`).click();await page.locator("#qq-content").waitFor();
        const audit=await page.evaluate(clipAudit,{viewport:true});check(audit.cut===0,`${theme}/${tab} shadow cut=0`);
        await page.screenshot({path:path.join(task.evidence,`${theme}-${tab}.png`)});
      }
    }
    await page.setViewportSize({width:420,height:800});
    for(const tab of ["connection","groups","memory","log"]){
      await page.locator(`#qq-tabs [data-tab='${tab}']`).click();
      check(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth),`Narrow ${tab} has no horizontal overflow`);
      const audit=await page.evaluate(clipAudit,{viewport:true});check(audit.cut===0,`Narrow ${tab} shadow cut=0`);
      await page.screenshot({path:path.join(task.evidence,`narrow-${tab}.png`)});
    }
    await page.setViewportSize({width:1440,height:900});
    // New ordinary group message may be received but the model can decide to stay silent.
    incoming("group",300,"GROUP_SILENCE: 已经聊完",4);
    await until(()=>requests.some(r=>JSON.stringify(r.messages).includes("GROUP_SILENCE")),"Silent group judgment");
    await until(async()=>!(await invoke("process_list",{projectDir:group.root})).some(p=>p.running),"Silent round completes");
    check(deliveries.filter(d=>d.action==="send_group_msg").length===1,"Silent group judgment produces no chat message");
    friends=[];incoming("friend",200,"REMOVED_FRIEND_MUST_NOT_WAKE",5);
    settings.groups[0].enabled=false;await invoke("qq_config_save",{settings,token:null});
    await until(async()=>(await invoke("qq_status")).connected,"Reconnect after config");
    incoming("group",300,"DISABLED_GROUP_MUST_NOT_WAKE",6);
    await invoke("qq_status");
    await close();await start();
    const recovered=await invoke("qq_status");
    check(recovered.peers.length===2,"QQ peers survive runtime restart");
    check((await invoke("qq_peer_log",{identity:friend.identity})).messages.length===3,"Removed friends cannot append local assistant input");
    check((await invoke("qq_peer_log",{identity:group.identity})).messages.length===2,"Disabled groups cannot append input");
    check((await invoke("qq_memories",{identity:group.identity}))[0].body==="CORRECTED","Scoped memory survives restart");
    check(!requests.some(r=>/STRANGER_MUST_NOT_WAKE|REMOVED_FRIEND_MUST_NOT_WAKE|DISABLED_GROUP_MUST_NOT_WAKE/.test(JSON.stringify(r))),"Unauthorized inputs never reach model");
    check(errors.length===0,`No WebView errors: ${errors.join("; ")}`);
    await writeFile(path.join(task.evidence,"receipt.json"),JSON.stringify({boundary:"Real WebView2/Rust/SQLite/terminal; fixture OneBot/model, no real QQ account",checks,groupEvents,requests:requests.length,deliveries,errors},null,2));
  }catch(error){
    if(page){await page.screenshot({path:path.join(task.evidence,"failure.png")});await writeFile(path.join(task.evidence,"failure.json"),JSON.stringify({error:String(error),status:await invoke("qq_status"),requests,deliveries,errors,ui:await page.locator("body").innerText()},null,2));}
    throw error;
  }finally{await close();for(const ws of sockets)ws.terminate();await new Promise(resolve=>onebot.close(resolve));}
});
