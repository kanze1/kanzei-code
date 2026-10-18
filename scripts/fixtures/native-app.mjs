/* global window, document */
// Own every app, profile and server used by a native engineering fixture.
import { spawn } from "node:child_process";
import { mkdir, copyFile, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";

export async function until(predicate, label, timeout = 60000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await predicate()) return; await new Promise(resolve=>setTimeout(resolve,100)); }
  throw new Error(label);
}
export function textReply(res, content) {
  res.writeHead(200,{"Content-Type":"text/event-stream"});
  res.end(`data: ${JSON.stringify({choices:[{index:0,delta:{content},finish_reason:"stop"}]})}\n\ndata: [DONE]\n\n`);
}
export function toolReply(res, name, args, step) {
  res.writeHead(200,{"Content-Type":"text/event-stream"});
  res.end(`data: ${JSON.stringify({choices:[{index:0,delta:{tool_calls:[{index:0,id:`fixture-${step}`,type:"function",function:{name,arguments:JSON.stringify(args)}}]},finish_reason:null}]})}\n\ndata: ${JSON.stringify({choices:[{index:0,delta:{},finish_reason:"tool_calls"}]})}\n\ndata: [DONE]\n\n`);
}
export async function nativeApp(executable, output, label, handle, extraConfig = "") {
  const run=path.join(output,String(Date.now())), project=path.join(run,"project"), profile=path.join(run,"profile"), appHome=path.join(profile,".kanzei");
  for(const dir of [project,appHome,path.join(run,"webview"),path.join(profile,"AppData/Local"),path.join(profile,"AppData/Roaming")]) await mkdir(dir,{recursive:true});
  await writeFile(path.join(appHome,"app.json"),JSON.stringify({projects:[],current:null,theme:"dark"}));
  const errors=[],requests=[];
  const model=http.createServer(async(req,res)=>{
    try {
      if(req.url==="/v1/models"){res.writeHead(200,{"Content-Type":"application/json"});res.end(JSON.stringify({data:[{id:"test"}]}));return;}
      let raw="";for await(const chunk of req) raw+=chunk;
      const body=JSON.parse(raw);requests.push(body);await handle(body,res);
    }catch(error){errors.push(String(error));textReply(res,"FIXTURE_FAILED");}
  });
  await new Promise(resolve=>model.listen(0,"127.0.0.1",resolve));
  await writeFile(path.join(appHome,"kanzei.toml"),`[models]\nprimary="stub:test"\nfast="stub:test"\n[providers.stub]\nprotocol="openai"\nbase_url="http://127.0.0.1:${model.address().port}/v1"\ncontext_limit=128000\n${extraConfig}`);
  const probe=net.createServer();await new Promise(resolve=>probe.listen(0,"127.0.0.1",resolve));const port=probe.address().port;await new Promise(resolve=>probe.close(resolve));
  const exe=path.join(run,"kzapp.exe");await copyFile(path.resolve(executable),exe);
  const app=spawn(exe,[],{cwd:project,windowsHide:true,stdio:"ignore",env:{...process.env,KANZEI_HOME:appHome,USERPROFILE:profile,HOME:profile,LOCALAPPDATA:path.join(profile,"AppData/Local"),APPDATA:path.join(profile,"AppData/Roaming"),WEBVIEW2_USER_DATA_FOLDER:path.join(run,"webview"),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`}});
  let browser,page;
  const invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI__.core.invoke(command,args),{command,args});
  const close=async()=>{
    if(page&&!page.isClosed()) await invoke("runtime_shutdown").catch(error=>console.warn(String(error)));
    await browser?.close();app.kill();model.closeAllConnections();await new Promise(resolve=>model.close(resolve));
  };
  try {
    await until(async()=>{try{browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);return true;}catch{return false;}},"Owned native app starts",30000);
    await until(()=>{page=browser.contexts().flatMap(context=>context.pages())[0];return Boolean(page);},"Owned native page exists");
    page.on("pageerror",error=>errors.push(error.message));
    await page.waitForFunction(()=>document.body.dataset.appReady==="true");
    await invoke("projects_init",{path:project,name:label});await page.reload();await page.waitForFunction(()=>document.body.dataset.appReady==="true");
    await page.locator(".workbench-project-name").filter({hasText:label}).click();
    const createConversation=async()=>{
      const line=await invoke("process_create",{projectDir:project,profile:"dev",model:"stub:test",subagentMode:"off"});
      await page.evaluate(async id=>{const sessions=await import("./09-sessions.js");await sessions.refreshProcesses({force:true});await sessions.switchProcess(id,true);},line.id);
      return line;
    };
    return {run,project,appHome,exe,page,invoke,requests,errors,close,createConversation};
  }catch(error){await close();throw error;}
}
