// WebView2 + real runtime/SQLite/tools; local model and OneBot protocol fixtures.
/* global window, document */
import assert from "node:assert/strict";
import { mkdir, readFile, readdir, writeFile, access } from "node:fs/promises";
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
  const workspace=path.join(home,"social-workspaces/personal");
  const liveScript='from pathlib import Path\nimport time\nPath("live-started").write_text("started")\nwhile not Path("live-release").exists():\n time.sleep(0.05)\nprint("QQ_LIVE_FINISHED")';
  const server=task.server(http.createServer((req,res)=>{res.statusCode=404;res.end();})), onebot=new WebSocketServer({server});
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
  function incoming(kind,peer,text,id,segments=null){
    const value={post_type:"message",self_id:100,message_type:kind==="friend"?"private":"group",user_id:kind==="friend"?peer:201,message_id:id,sender:{nickname:kind==="friend"?"测试好友":"群友"},message:segments||[{type:"text",data:{text}}]};
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
    if(text.includes("FRIEND_LIVE")){
      if(!called("live-shell"))call("live-shell","sandbox",{command:"python3 -c '"+liveScript+"'",timeout_ms:120000});
      else answer(res,"LIVE_DONE: 实时工具结束。");
    }else if(text.includes("FRIEND_TERMINAL")){
      if(!called("friend-shell"))call("friend-shell","sandbox",{command:"printf QQ_TERMINAL_EXECUTED"});
      else answer(res,"FRIEND_DONE: 沙箱命令已执行。");
    }else if(text.includes("FRIEND_APPROVAL")){
      if(!called("friend-write"))call("friend-write","sandbox",{command:"printf QQ_WORKSPACE_FILE > qq-workspace.txt; cat qq-workspace.txt"});
      else answer(res,"APPROVAL_DONE: 已写入独立工作区。");
    }else if(text.includes("GROUP_MENTION")){
      if(!called("mention-reply"))call("mention-reply","qq_reply",{text:"MENTION_SENT: 点名回复"});
      else answer(res,"点名回复完成。");
    }else if(text.includes("GROUP_SILENCE")){
      answer(res,"SILENT_DECISION: 本轮不参与。");
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
    app=spawn(exe,[],{cwd:run,windowsHide:true,stdio:"ignore",env:{...process.env,KANZEI_HOME:home,USERPROFILE:profile,HOME:profile,LOCALAPPDATA:path.join(profile,"AppData/Local"),APPDATA:path.join(profile,"AppData/Roaming"),WEBVIEW2_USER_DATA_FOLDER:path.join(run,"webview"),KANZEI_E2E_CDP:String(port)}});
    await until(async()=>{try{browser=await task.browser(chromium.connectOverCDP(`http://127.0.0.1:${port}`,{timeout:1500}));return true;}catch{return false;}},"WebView startup");
    page=browser.contexts()[0].pages()[0];page.on("pageerror",e=>errors.push(String(e)));
    await until(()=>page.url()!=="about:blank","Document navigation");await page.waitForFunction(()=>document.body.dataset.appReady==="true",{timeout:45000});
    await page.setViewportSize({width:1440,height:900});
    await page.evaluate(async()=>{window.__qqEvents=[];for(const name of ["kz:input-received","kz:turn","kz:tool-start","kz:tool-end","kz:lifecycle"]){await window.__TAURI__.event.listen(name,event=>{window.__qqEvents.push({name,payload:event.payload});});}});
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
    check(await page.locator("#workspace-sidebar-footer #social-nav").isVisible(),"Unified social entry uses bottom sidebar");
    check(await page.locator("#qq-nav, #bili-nav, #view-qq, #view-bili").count()===0,"Standalone management routes and duplicated entries are removed");
    await page.locator("#social-nav").click();await page.locator("#social-toolbar").waitFor();
    check(!await page.locator("#social-manager").isVisible()&&await page.locator("#prompt").isVisible(),"Social entry opens projectless chat with ordinary composer before connection");
    await page.locator("#social-manage").click();await page.locator("#qq-settings-form").waitFor();
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
    check(request.tools.some(t=>t.function.name==="sandbox")&&!request.tools.some(t=>["bash","read","write","browser","desktop","task","process"].includes(t.function.name)),"Friend gets isolated workspace tools without host control");
    check(requests.some(r=>r.messages.some(m=>m.role==="tool"&&JSON.stringify(m).includes("QQ_TERMINAL_EXECUTED"))),"Isolated POSIX command actually executes");
    await page.locator("#qq-refresh").click();await page.locator("#social-manager-close").click();
    await page.locator("#social-target").click();await page.getByRole("menuitemcheckbox",{name:"测试好友"}).click();
    await until(async()=>(await page.locator("#messages").innerText()).includes("FRIEND_DONE"),"QQ native conversation");
    check((await page.locator("#social-target-name").innerText())==="测试好友"&&await page.locator("#prompt").isVisible(),"QQ target uses the ordinary conversation and composer");
    check((await page.locator("#messages").innerText()).includes("QQ_TERMINAL_EXECUTED"),"Native conversation renders actual tool stream");
    await page.locator("#social-target").click();await page.getByRole("menuitemcheckbox",{name:"大肥鱼",exact:false}).click();
    incoming("friend",200,"FRIEND_LIVE: 持续执行本机工具",22);
    await until(async()=>{try{await access(path.join(workspace,"live-started"));return true;}catch{return false;}},"Live sandbox reaches file barrier");
    await until(async()=>await page.locator("#social-nav").getAttribute("data-running")==="true","Background QQ run marks social entry");
    await page.locator("#social-target").click();await page.getByRole("menuitemcheckbox",{name:/测试好友/}).click();
    await until(async()=>(await page.locator('#messages .msg-pane[data-active="1"]').innerText()).includes("FRIEND_LIVE"),"Externally committed QQ input appears in local conversation");
    check((await page.locator('#messages .msg-pane[data-active="1"]').innerText()).includes("FRIEND_TERMINAL"),"Opening background stream hydrates older committed history");
    check((await page.locator("#social-live-state").innerText()).includes("sandbox")&&await page.locator('#messages .msg-pane[data-active="1"] .tool-msg.running').count()>0,"In-flight QQ sandbox shows live execution state after switching in");
    await writeFile(path.join(workspace,"live-release"),"release");
    await until(async()=>(await page.locator('#messages .msg-pane[data-active="1"]').innerText()).includes("LIVE_DONE"),"Local conversation receives live tool completion without refresh");
    check(await page.locator('#messages .msg-pane[data-active="1"] .tool-msg.running').count()===0,"Recovered running tool settles from the real completion event");
    incoming("friend",200,"FRIEND_APPROVAL: 写入文件",20);
    await until(()=>deliveries.some(d=>JSON.stringify(d).includes("APPROVAL_DONE")),"Friend completes isolated file work");
    check((await readFile(path.join(workspace,"qq-workspace.txt"),"utf8"))==="QQ_WORKSPACE_FILE","QQ file work persists only inside social workspace");
    const asks=await invoke("pending_asks_get",{projectDir:friend.root,processId:friend.process_id});
    check(asks.length===0,"Sandbox work does not request host permission");
    incoming("group",300,"GROUP_SPEAK: 这条普通群消息没有 at",3);
    await until(()=>deliveries.some(d=>d.action==="send_group_msg"),"Non-mention group participation");
    status=await invoke("qq_status");const group=status.peers.find(p=>p.identity.kind==="group");
    const groupRequest=requests.find(r=>JSON.stringify(r.messages).includes("GROUP_SPEAK"));
    const catalog=groupRequest.tools.map(t=>t.function.name);
    check(["qq_reply","qq_memory","sandbox","websearch","webfetch","bili_feed","bili_video","bili_comments","bili_comment_draft"].every(name=>catalog.includes(name))&&!catalog.some(n=>["bash","write","read","task","tool_search","bili_publish"].includes(n)),"Group shares Bilibili reading and draft tools alongside its sandbox, without host execution or publication tools");
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
    await page.locator("#prompt").fill("未发送的助手草稿");
    await page.locator("#social-manage").click();
    await page.locator('#qq-tabs [data-tab="groups"]').click();
    await page.getByRole("button",{name:"添加群",exact:true}).click();
    await page.locator('#qq-group-editor [name="id"]').fill("301");
    await page.locator('#qq-group-editor [name="name"]').fill("可删除的群");
    await page.locator("#social-platform-bili").click();await page.locator("#social-platform-qq").click();
    check(await page.locator('#qq-group-editor [name="id"]').inputValue()==="301","Group draft survives platform switching");
    await page.getByRole("button",{name:"保存群聊规则",exact:true}).click();
    await until(async()=>(await invoke("qq_status")).settings.groups.length===2,"Group created from native editor");
    await page.locator('.qq-rule-row[data-group-id="301"]').getByRole("button",{name:"删除规则",exact:true}).click();
    await until(async()=>(await invoke("qq_status")).settings.groups.length===1,"Group deletion persists");
    check((await invoke("qq_status")).settings.groups[0].id==="300","Deleting a rule preserves other saved rules");
    await page.getByRole("button",{name:"添加群",exact:true}).click();await page.getByRole("button",{name:"取消",exact:true}).click();
    check(await page.locator("#qq-group-editor").count()===0,"Blank group draft cancels without validation or persisting");
    await page.locator("#social-manager-close").click();
    check(await page.locator("#prompt").inputValue()==="未发送的助手草稿"&&await page.locator("#prompt").isVisible(),"Management leaves active conversation draft intact");
    await page.locator("#social-manage").click();
    for(const theme of ["dark","light"]){
      await page.evaluate(theme=>document.documentElement.setAttribute("data-theme",theme),theme);
      for(const tab of ["connection","groups","persona","memory","log"]){
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
    await page.locator("#social-manager-close").click();
    // New ordinary group message may be received but the model can decide to stay silent.
    incoming("group",300,"GROUP_SILENCE: 已经聊完",4);
    await until(()=>requests.some(r=>JSON.stringify(r.messages).includes("GROUP_SILENCE")),"Silent group judgment");
    await until(async()=>!(await invoke("process_list",{projectDir:group.root})).some(p=>p.running),"Silent round completes");
    check(deliveries.filter(d=>d.action==="send_group_msg").length===1,"Silent group judgment produces no chat message");
    // A direct @ wakes during quiet hours and within the ambient interval.
    const hour=new Date().getHours();settings.groups[0].interval_seconds=600;settings.groups[0].quiet_start=hour;settings.groups[0].quiet_end=(hour+1)%24;
    await invoke("qq_config_save",{settings,token:null});await until(async()=>(await invoke("qq_status")).connected,"Quiet rule reconnect");
    const before=requests.length, priorDeliveries=deliveries.length;
    incoming("group",300,"OTHER_AT_MUST_STAY_QUIET",30,[{type:"at",data:{qq:"all"}},{type:"at",data:{qq:101}},{type:"text",data:{text:"OTHER_AT_MUST_STAY_QUIET"}}]);
    incoming("group",300,"GROUP_MENTION: 在干嘛",31,[{type:"at",data:{qq:"100"}},{type:"text",data:{text:"GROUP_MENTION: 在干嘛"}}]);
    incoming("group",300,"GROUP_MENTION: 在干嘛",31,[{type:"at",data:{qq:"100"}},{type:"text",data:{text:"GROUP_MENTION: 在干嘛"}}]);
    await until(()=>deliveries.slice(priorDeliveries).some(d=>JSON.stringify(d).includes("MENTION_SENT")),"Explicit mention bypasses quiet hours and 600-second ambient interval",10000);
    await until(async()=>!(await invoke("process_list",{projectDir:group.root})).some(p=>p.running),"Mention round completes");
    check(deliveries.slice(priorDeliveries).filter(d=>d.action==="send_group_msg").length===1,"Duplicate mentions yield exactly one reply");
    const mention=requests.slice(before).find(r=>JSON.stringify(r.messages).includes("群聊点名回复"));
    check(mention&&!mention.tools.some(t=>["bash","read","write","task","tool_search"].includes(t.function.name)),"Direct group mention retains restricted group capabilities");
    check(requests.slice(before).every(r=>{
      const prompt=String([...r.messages].reverse().find(m=>m.role==="user")?.content||"");
      return prompt.startsWith("群聊点名回复")&&prompt.split("当前消息 JSON：").at(-1).includes("GROUP_MENTION")&&!prompt.split("当前消息 JSON：").at(-1).includes("OTHER_AT_MUST_STAY_QUIET");
    }),"Other-account and at-all mentions do not wake rounds during quiet hours");
    friends=[];incoming("friend",200,"REMOVED_FRIEND_MUST_NOT_WAKE",5);
    await page.locator("#social-nav").click();await page.locator("#social-manage").click();await page.locator('#qq-tabs [data-tab="groups"]').click();
    await page.locator('.qq-rule-row[data-group-id="300"]').getByRole("button",{name:"删除规则",exact:true}).click();
    await until(async()=>(await invoke("qq_status")).settings.groups.length===0,"Last saved rule can be deleted");
    await page.locator('#qq-tabs [data-tab="persona"]').click();await page.locator('#qq-settings-form [name="persona"]').fill("保存人设不会复活删除的规则");await page.locator('#qq-settings-form button[type="submit"]').click();
    await until(async()=>(await invoke("qq_status")).settings.persona==="保存人设不会复活删除的规则","Independent settings persist");
    check((await invoke("qq_status")).settings.groups.length===0,"Saving another form cannot resurrect deleted rules");
    await until(async()=>(await invoke("qq_status")).connected,"Reconnect after config");
    incoming("group",300,"DISABLED_GROUP_MUST_NOT_WAKE",6,[{type:"at",data:{qq:100}},{type:"text",data:{text:"DISABLED_GROUP_MUST_NOT_WAKE"}}]);
    await invoke("qq_status");
    await close();await start();
    const recovered=await invoke("qq_status");
    check(recovered.peers.length===2,"QQ peers survive runtime restart");
    check(recovered.settings.groups.length===0,"Deleted rules stay deleted after runtime restart");
    check(!requests.some(r=>r.messages.some(m=>m.role==="user"&&String(m.content).startsWith("继续推进当前任务。"))),"Opening social conversations cannot start project auto-continuation");
    check((await invoke("qq_peer_log",{identity:friend.identity})).messages.length===3,"Removed friends cannot append local assistant input");
    check((await invoke("qq_peer_log",{identity:group.identity})).messages.length===4,"Deleted groups cannot append mention input");
    check((await invoke("qq_memories",{identity:group.identity}))[0].body==="CORRECTED","Scoped memory survives restart");
    check(!requests.some(r=>/STRANGER_MUST_NOT_WAKE|REMOVED_FRIEND_MUST_NOT_WAKE|DISABLED_GROUP_MUST_NOT_WAKE/.test(JSON.stringify(r))),"Unauthorized inputs never reach model");
    check(errors.length===0,`No WebView errors: ${errors.join("; ")}`);
    await writeFile(path.join(task.evidence,"receipt.json"),JSON.stringify({boundary:"Real WebView2/Rust/SQLite/WSL sandbox; fixture OneBot/model, no real QQ account",checks,groupEvents,requests:requests.length,deliveries,errors},null,2));
  }catch(error){
    if(page){await page.screenshot({path:path.join(task.evidence,"failure.png")});await writeFile(path.join(task.evidence,"failure.json"),JSON.stringify({error:String(error),status:await invoke("qq_status"),requests,deliveries,errors,ui:await page.locator("body").innerText(),events:await page.evaluate(async()=>{const shell=await import("./03-shell.js"), panes=await import("./01-chat-panes.js");return {active:shell.activeSessionId,stage:shell.sessionState(shell.activeSessionId),panes:[...panes.messagePanes].map(([id,pane])=>({id,loaded:pane.dataset.historyLoaded,text:pane.innerText})),events:window.__qqEvents};})},null,2));}
    throw error;
  }finally{await close();for(const ws of sockets)ws.terminate();await new Promise(resolve=>onebot.close(resolve));}
});
