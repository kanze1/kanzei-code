/* global document, window */
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, writeFile, readFile } from "node:fs/promises";
import http from "node:http";
import path from "node:path";
import { WebSocketServer } from "ws";
import { withTask } from "./artifact-lifecycle.mjs";
import { nativeApp, until, textReply, toolReply } from "./fixtures/native-app.mjs";
import clipAudit from "./ui-preview/clip-audit.mjs";

await withTask({suite:"ui-social-native-smoke"},async task=>{
  assert.equal(process.platform,"win32","Social native acceptance requires Windows/WebView2");
  const checks=[],calls=[],posts=[],errors=[],groupDeliveries=[];
  const check=(value,label)=>{assert(value,label);checks.push(label);console.log(`PASS ${label}`);};
  const bv="BV1234567890", video={bvid:bv,aid:22,cid:33,title:"测试视频",desc:"视频简介 <script>外部数据</script>",pic:"",owner:{name:"测试 UP"}};
  const loginPolls=[];
  let loginCode=86101,qrCookie="owner",postMode="success",network={network:{httpServers:[{name:"other"}],websocketServers:[]}};
  const server=task.server(http.createServer(async(req,res)=>{
    try {
      const url=new URL(req.url,"http://127.0.0.1");let raw="";for await(const chunk of req)raw+=chunk;
      const answer=(data,code=0)=>{res.setHeader("Content-Type","application/json");res.end(JSON.stringify({code,data}));};
      calls.push({path:url.pathname,query:Object.fromEntries(url.searchParams)});
      if(url.pathname==="/api/auth/login") {assert.equal(JSON.parse(raw).hash,createHash("sha256").update("local-password.napcat").digest("hex"));answer({Credential:"fixture-credential"});return;}
      if(url.pathname.startsWith("/api/")) {
        assert.equal(req.headers.authorization,"Bearer fixture-credential");
        if(url.pathname.endsWith("CheckLoginStatus"))answer({isLogin:true});
        else if(url.pathname.endsWith("GetQQLoginInfo"))answer({uin:"100",nick:"QQ 测试"});
        else if(url.pathname.endsWith("GetConfig"))answer(network);
        else if(url.pathname.endsWith("SetConfig")){network=JSON.parse(JSON.parse(raw).config);answer({});}
        else throw new Error(`Unknown NapCat route ${url.pathname}`);return;
      }
      switch(url.pathname){
        case "/x/passport-login/web/qrcode/generate":answer({url:"https://passport.bilibili.com/test-login",qrcode_key:String(calls.length)});break;
        case "/x/passport-login/web/qrcode/poll":loginPolls.push(loginCode);res.setHeader("Set-Cookie",[`SESSDATA=${qrCookie}; HttpOnly; Secure`,"bili_jct=csrf; Secure","DedeUserID=1000; Path=/; Domain=bilibili.com","DedeUserID__ckMd5=identity; Path=/"]);answer({code:loginCode});break;
        case "/x/web-interface/nav":{
          if(!req.headers.cookie?.includes("SESSDATA=")||req.headers.cookie.includes("SESSDATA=invalid")){answer(null,-101);break;}
          answer({isLogin:true,mid:req.headers.cookie.includes("SESSDATA=other")?2000:1000,uname:"B站测试",wbi_img:{img_url:"https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png",sub_url:"https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png"}});break;
        }
        case "/x/web-interface/popular":answer({list:[video]});break;
        case "/x/web-interface/wbi/search/type":assert(url.searchParams.has("w_rid"));answer({result:[video]});break;
        case "/x/web-interface/view":answer(video);break;
        case "/x/player/wbi/v2":assert(url.searchParams.has("w_rid"));answer({subtitle:{subtitles:[]}});break;
        case "/x/v2/reply":answer({replies:[{rpid:55,rcount:1,member:{uname:"测试评论者"},content:{message:"测试评论"}}]});break;
        case "/x/v2/reply/reply":answer({root:{rpid:55,member:{uname:"测试评论者"},content:{message:"测试评论"}},replies:[{rpid:56,member:{uname:"楼中楼"},content:{message:"回复内容"}}]});break;
        case "/x/v2/reply/add":{
          const form=Object.fromEntries(new URLSearchParams(raw));assert.equal(form.csrf,"csrf");assert.equal(form.oid,"22");posts.push(form);
          if(postMode==="disconnect"){req.socket.destroy();return;}
          if(postMode==="risk"){answer(null,-412);return;}answer({rpid:100+posts.length});break;
        }
        default:throw new Error(`Unknown Bili route ${url.pathname}`);
      }
    }catch(e){errors.push(String(e));res.statusCode=500;res.end();}
  }));
  const ws=new WebSocketServer({server}),sockets=new Set();
  ws.on("connection",(socket,req)=>{sockets.add(socket);socket.on("close",()=>sockets.delete(socket));assert.equal(req.headers.authorization,`Bearer ${network.network.websocketServers.find(s=>s.name==="kanzei-code").token}`);socket.on("message",raw=>{const call=JSON.parse(String(raw));if(call.action==="send_group_msg")groupDeliveries.push(call);const data=call.action==="get_login_info"?{user_id:100,nickname:"QQ 测试"}:call.action==="send_group_msg"?{message_id:groupDeliveries.length}:[];socket.send(JSON.stringify({status:"ok",retcode:0,echo:call.echo,data}));});});
  await new Promise(resolve=>server.listen(0,"127.0.0.1",resolve));const endpoint=`http://127.0.0.1:${server.address().port}`;
  const app=await nativeApp(path.resolve("target/debug/kzapp.exe"),task.evidence,"Social fixture",(body,res)=>{
    if(!body.tools?.some(t=>t.function.name==="bili_feed")){textReply(res,"MEMORY_REVIEW_PENDING");return;}
    const used=body.messages.flatMap(m=>m.tool_calls||[]).map(c=>c.function.name);
    if(!used.includes("sandbox"))toolReply(res,"sandbox",{command:"mkdir -p notes; printf BILI_WORKSPACE_RESULT > notes/bili.txt; python3 -c 'print(6*7)'"},"sandbox");
    else if(!used.includes("bili_feed"))toolReply(res,"bili_feed",{},"feed");
    else if(!used.includes("bili_video"))toolReply(res,"bili_video",{bvid:bv},"video");
    else if(!used.includes("bili_comments"))toolReply(res,"bili_comments",{bvid:bv},"comments");
    else if(!used.includes("bili_comment_draft"))toolReply(res,"bili_comment_draft",{bvid:bv,body:"助手草稿"},"draft");
    else if(!used.includes("memory_note"))toolReply(res,"memory_note",{summary:"BILI_PENDING_NOTE",detail:"实际读取的讨论线索"},"note");
    else if(body.tools.some(t=>t.function.name==="qq_reply")&&!used.includes("qq_reply"))toolReply(res,"qq_reply",{text:"GROUP_BILI_DONE：群聊读取了 B站视频信息和评论，草稿已保存。"},"group-bili-reply");
    else textReply(res,"BILI_DONE：读取了信息和评论，没有字幕，未分析画面。草稿已保存。");
  },"",{KANZEI_E2E_BILI_ENDPOINT:endpoint});
  const {page,invoke}=app;
  await page.setViewportSize({width:1440,height:900});
  try {
    const nap=path.join(task.environment,"NapCat");await mkdir(path.join(nap,"config"),{recursive:true});await writeFile(path.join(nap,"config/webui.json"),JSON.stringify({port:server.address().port,token:"local-password"}));
    const setup=await invoke("qq_setup_connect",{directory:nap,url:null,password:null,totp:null});
    check(!JSON.stringify(setup).includes("credential"),"NapCat authentication stays in backend");
    check((await invoke("qq_setup_status",{id:setup.id})).account==="100","NapCat account is read in app");
    await invoke("qq_setup_apply",{id:setup.id,port:server.address().port});
    await until(async()=>(await invoke("qq_status")).connected,"Managed OneBot connects");
    check(network.network.httpServers[0].name==="other"&&network.network.websocketServers[0].name==="kanzei-code","QQ setup preserves other network connections");
    check(!(JSON.stringify(await invoke("qq_status"))).includes(network.network.websocketServers[0].token),"Generated bridge token never reaches renderer");
    check(await page.locator("#workspace-sidebar-footer #social-nav").isVisible(),"QQ and Bilibili share one bottom sidebar entry");
    await page.locator("#social-nav").click();await page.locator("#social-toolbar").waitFor();
    await page.locator("#social-bili-status").click();await page.locator('#bili-tabs [data-tab="login"]').click();
    const first=await invoke("bili_login_start"),second=await invoke("bili_login_start");await assert.rejects(()=>invoke("bili_login_poll",{id:first.id}));
    await invoke("bili_login_cancel",{id:second.id});check((await invoke("bili_login_poll",{id:second.id})).status==="cancelled","Replaced and cancelled QR challenges cannot log in");
    await page.getByRole("button",{name:"生成登录二维码",exact:true}).click();await until(()=>loginPolls.includes(86101),"QR waits for scan");
    await page.locator('#bili-tabs [data-tab="browse"]').click();loginCode=86090;
    await until(()=>loginPolls.includes(86090),"QR scan continues outside login tab");
    await page.locator("#social-manager-close").click();loginCode=0;
    await until(async()=>(await invoke("bili_status")).account===1000,"QR login completes while management is closed");
    await until(async()=>await page.locator("#social-bili-status").getAttribute("data-connected")==="true","Header login state updates automatically");
    await page.locator("#social-bili-status").click();
    check((await page.locator("#bili-state").innerText()).includes("B站测试")&&(await page.locator("#bili-content").innerText()).includes("已登录"),"Successful QR login immediately shows account without refresh");
    let status=await invoke("bili_status");check(!JSON.stringify(status).includes("SESSDATA"),"Bilibili status excludes credentials");
    const qrStored=JSON.parse(await readFile(path.join(app.appHome,"bilibili/settings.json"),"utf8"));
    check(qrStored.cookie.includes("DedeUserID=1000")&&qrStored.cookie.includes("DedeUserID__ckMd5=identity")&&!qrStored.cookie.includes("Path="),"QR login saves the complete session cookie set without response attributes");
    const failedLogin=await invoke("bili_login_start");qrCookie="invalid";
    await assert.rejects(()=>invoke("bili_login_poll",{id:failedLogin.id}),/登录未生效或已过期/);
    check((await invoke("bili_login_poll",{id:failedLogin.id})).status==="error","Rejected QR credentials enter a terminal error state");
    check((await invoke("bili_status")).account===1000&&JSON.parse(await readFile(path.join(app.appHome,"bilibili/settings.json"),"utf8")).cookie===qrStored.cookie,"Failed account validation preserves the previous login");
    qrCookie="owner";
    status.settings.comment_interval_seconds=0;await invoke("bili_settings_save",{settings:status.settings});
    await page.locator('#bili-tabs [data-tab="browse"]').click();await page.getByRole("button",{name:"热门视频",exact:true}).click();await page.locator(".bili-video-title").click();
    check((await page.locator("#bili-player").getAttribute("src")).startsWith("https://player.bilibili.com/"),"Video uses official player");
    check(await page.locator(".bili-description script").count()===0,"Video descriptions render as external text");
    await page.getByRole("button",{name:"读取字幕",exact:true}).click();await page.locator(".bili-transcript").waitFor();check((await page.locator(".bili-transcript").innerText()).includes("没有可用字幕"),"Missing subtitles are reported without claiming media analysis");
    await page.getByRole("button",{name:"读取评论",exact:true}).click();await page.getByRole("button",{name:"展开回复",exact:true}).click();await page.getByRole("button",{name:"回复",exact:true}).last().click();
    await page.locator('#bili-comment-form textarea').fill("人工审核评论");await page.getByRole("button",{name:"保存评论草稿",exact:true}).click();await page.getByRole("button",{name:"发布评论",exact:true}).waitFor();
    let draft=(await invoke("bili_status")).records.drafts[0];check(draft.root===55&&draft.parent===56,"Comment draft binds to actual thread and reply");
    await assert.rejects(()=>invoke("bili_publish",{id:draft.id,revision:"stale"}));check(posts.length===0,"Stale draft cannot send");
    await page.getByRole("button",{name:"发布评论",exact:true}).click();await until(async()=>(await invoke("bili_status")).records.drafts[0].state==="sent","Comment receipt saved");
    check(posts.length===1&&posts[0].message==="人工审核评论"&&posts[0].root==="55"&&posts[0].parent==="56","Owner click publishes exactly reviewed text and target");
    await assert.rejects(()=>invoke("bili_publish",{id:draft.id,revision:draft.revision}));check(posts.length===1,"Sent draft is never replayed");
    draft=await invoke("bili_draft_create",{bvid:bv,body:"账号绑定",root:null,parent:null});await invoke("bili_cookie_save",{cookie:"SESSDATA=other; bili_jct=csrf"});await assert.rejects(()=>invoke("bili_publish",{id:draft.id,revision:draft.revision}));check(posts.length===1,"Account switch cannot publish former account drafts");await invoke("bili_cookie_save",{cookie:"SESSDATA=owner; bili_jct=csrf"});
    postMode="disconnect";await assert.rejects(()=>invoke("bili_publish",{id:draft.id,revision:draft.revision}));check((await invoke("bili_status")).records.drafts.find(d=>d.id===draft.id).state==="unknown","Interrupted send is durable unknown");await assert.rejects(()=>invoke("bili_publish",{id:draft.id,revision:draft.revision}));check(posts.length===2,"Unknown send is never replayed");postMode="success";
    await invoke("bili_settings_save",{settings:status.settings});await page.locator('#bili-tabs [data-tab="browse"]').click();await page.locator("#bili-content").getByRole("button",{name:"浏览一轮",exact:true}).click();await page.locator("#social-toolbar").waitFor({state:"visible"});await until(async()=>(await page.locator("#messages").innerText()).includes("BILI_DONE"),"Bili agent round completes in native conversation");
    const request=app.requests.find(r=>r.tools?.some(t=>t.function.name==="bili_feed"));check(request&&!["bash","write","task","bili_publish"].some(name=>request.tools.some(t=>t.function.name===name)),"Bili browsing session has no terminal, delegation, or direct publication tool");
    check(["sandbox","websearch","webfetch"].every(name=>request.tools.some(t=>t.function.name===name)),"Bilibili retains sandbox and public internet tools");
    check(await readFile(path.join(app.appHome,"social-workspaces/personal/notes/bili.txt"),"utf8")==="BILI_WORKSPACE_RESULT","Bilibili agent actually writes a persistent task file inside the isolated workspace");
    check(app.requests.some(r=>r.messages.some(m=>m.role==="tool"&&JSON.stringify(m).includes("助手草稿"))),"Agent actually creates comment draft through tool stream");check(posts.length===2,"Agent browsing cannot publish comments");
    const target=await invoke("bili_conversation_open");
    const pending=await invoke("memory_note_candidates",{projectDir:target.root});check(pending.some(n=>n.summary==="BILI_PENDING_NOTE"&&n.scope==="project"),"Shared personal inbox keeps existing adoption selector");
    await invoke("qq_memory_save",{identity:{account:"100",kind:"friend",peer:"200"},id:null,title:"PERSONAL_UI_MEMORY",description:"个人共享记忆",body:"QQ_PRIVATE_SHARED_WITH_BILI",expectedHash:null});
    const entries=await invoke("memory_entries",{projectDir:target.root,scope:"project",category:null});check(entries.some(e=>e.title==="PERSONAL_UI_MEMORY"&&e.scope==="project"),"QQ personal memory is manageable from Bilibili through existing UI selector");
    const entry=entries.find(e=>e.title==="PERSONAL_UI_MEMORY");
    const graph=await invoke("memory_graph",{projectDir:target.root});check(graph.nodes.some(n=>n.id===entry.id&&n.scope==="project"),"Personal memory appears in default graph filter");
    const global=path.join(app.appHome,"memory");await mkdir(global,{recursive:true});await writeFile(path.join(global,"U-900-private.md"),"---\nid: U-900\nscope: global\ncategory: fact\ntitle: GLOBAL_GRAPH_SENTINEL\ndescription: global private\nstatus: active\ncreated: 2026-11-01\nupdated: 2026-11-01\nsource: user\n---\nPrivate global graph fact.\n");
    const group=path.join(app.appHome,"conversations/qq/100/group/300");await mkdir(group,{recursive:true});const groupGraph=await invoke("memory_graph",{projectDir:group});check(!groupGraph.nodes.some(n=>n.id===entry.id||n.id==="U-900"),"Group memory graph excludes both personal and global memory");
    await invoke("qq_group_save",{group:{id:"300",name:"B站共享测试群",enabled:true,interval_seconds:1,proactive_minutes:0,quiet_start:0,quiet_end:0,persona:""}});
    for(const socket of sockets)socket.send(JSON.stringify({post_type:"message",self_id:100,message_type:"group",group_id:300,user_id:201,message_id:900,sender:{nickname:"测试群友"},message:[{type:"at",data:{qq:"100"}},{type:"text",data:{text:"GROUP_BILI_READ：读取并讨论 B站视频"}}]}));
    await until(()=>groupDeliveries.some(call=>JSON.stringify(call).includes("GROUP_BILI_DONE")),"Group executes Bilibili tools and explicitly replies over QQ");
    const groupRequest=app.requests.find(request=>request.tools?.some(tool=>tool.function.name==="qq_reply")&&JSON.stringify(request.messages).includes("GROUP_BILI_READ"));
    check(groupRequest&&["bili_feed","bili_video","bili_comments","bili_comment_draft"].every(name=>groupRequest.tools.some(tool=>tool.function.name===name)),"Group receives the same Bilibili reading and draft capabilities");
    check(!JSON.stringify(groupRequest.messages).includes("PERSONAL_UI_MEMORY"),"Shared Bilibili tools do not inject personal memory into a group");
    check(groupDeliveries.length===1&&!JSON.stringify(groupDeliveries).includes("tool_calls"),"Group receives only the authored reply, never tool traces");
    check(posts.length===2,"Group Bilibili browsing creates no real publication");
    check(!await page.locator("#social-manager").isVisible()&&await page.locator("#prompt").isVisible(),"Bili browse returns to ordinary conversation and tool stream");
    await page.locator("#prompt").fill("会话草稿保持");await page.locator("#social-manage").click();
    await page.locator('#bili-tabs [data-tab="settings"]').click();await page.locator('#bili-content [name="keyword"]').fill("未保存的浏览偏好");
    await page.locator("#social-platform-qq").click();await page.locator("#social-platform-bili").click();
    check(await page.locator('#bili-content [name="keyword"]').inputValue()==="未保存的浏览偏好","Bili settings survive platform switching");
    await page.locator("#social-manager-close").click();
    check(await page.locator("#prompt").inputValue()==="会话草稿保持","Bili management preserves current conversation draft");
    await page.locator("#social-manage").click();
    for(const theme of ["dark","light"]) {await page.evaluate(theme=>document.documentElement.setAttribute("data-theme",theme),theme);for(const tab of ["browse","drafts","records","login","settings"]){await page.locator(`#bili-tabs [data-tab="${tab}"]`).click();check((await page.evaluate(clipAudit,{viewport:true})).cut===0,`${theme}/${tab} shadow cut=0`);await page.screenshot({path:path.join(task.evidence,`${theme}-${tab}.png`)});}}
    await page.setViewportSize({width:420,height:800});for(const tab of ["browse","drafts","settings"]){await page.locator(`#bili-tabs [data-tab="${tab}"]`).click();check(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth),`Narrow ${tab} no horizontal overflow`);const narrowAudit=await page.evaluate(clipAudit,{viewport:true});await writeFile(path.join(task.evidence,`narrow-${tab}-audit.json`),JSON.stringify(narrowAudit,null,2));check(narrowAudit.cut===0,`Narrow ${tab} shadow cut=0`);await page.screenshot({path:path.join(task.evidence,`narrow-${tab}.png`)});}
    check(errors.length===0&&app.errors.length===0,`No native or fixture errors: ${[...errors,...app.errors].join("; ")}`);
    const saved=JSON.parse(await readFile(path.join(app.appHome,"bilibili/settings.json"),"utf8"));check(saved.account===1000,"Credentials persist in local backend storage");
    await writeFile(path.join(task.evidence,"receipt.json"),JSON.stringify({boundary:"Real WebView2/Rust/SQLite/runtime; local NapCat, Bilibili and model fixtures; no live accounts or real comments",checks,calls,posts,errors},null,2));
  }catch(e){await page.screenshot({path:path.join(task.evidence,"failure.png")});await writeFile(path.join(task.evidence,"failure.json"),JSON.stringify({error:String(e),status:await invoke("bili_status"),active:await page.evaluate(async()=>{const s=await import("./03-shell.js");return {root:s.currentProject,session:s.activeSessionId,process:s.activeProcessId,items:s.processItems};}),calls,posts,errors,requests:app.requests,ui:await page.locator("body").innerText()},null,2));throw e;}
  finally{await app.close();for(const socket of sockets)socket.terminate();await new Promise(resolve=>ws.close(resolve));}
});
