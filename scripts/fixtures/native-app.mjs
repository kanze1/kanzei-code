/* global window, document */
// Own every app, profile and server used by a native engineering fixture.
import { spawnOwned as spawn, currentTask } from "../artifact-lifecycle.mjs";
import { mkdir, copyFile, writeFile } from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright-core";
import { once } from "node:events";

export async function stopOwnedProcess(child) {
  if (!child?.pid || child.exitCode !== null || child.signalCode !== null) return;
  const exited = once(child, "exit");
  child.kill();
  await exited;
}

export async function closeNativeResources({ browser, app, model, probe }, shutdown) {
  const failures = [];
  if (shutdown) {
    try { await shutdown(); } catch (error) { failures.push(error); }
  }
  const closeServer = async server => {
    if (!server?.listening) return;
    server.closeAllConnections?.();
    await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  };
  const results = await Promise.allSettled([
    Promise.resolve().then(()=>browser?.close()), stopOwnedProcess(app), closeServer(model), closeServer(probe),
  ]);
  failures.push(...results.filter(result => result.status === "rejected").map(result => result.reason));
  if (failures.length) throw new AggregateError(failures, "Native fixture cleanup failed; every owned resource release was attempted");
}

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
  const task=currentTask();
  if(!task) throw new Error("Native app fixtures require withTask to own the environment and retained evidence");
  task.input(executable);
  const run=task.environment, project=path.join(run,"project"), profile=path.join(run,"profile"), appHome=path.join(profile,".kanzei");
  for(const dir of [project,appHome,path.join(run,"webview"),path.join(profile,"AppData/Local"),path.join(profile,"AppData/Roaming")]) await mkdir(dir,{recursive:true});
  await writeFile(path.join(appHome,"app.json"),JSON.stringify({projects:[],current:null,theme:"dark"}));
  const errors=[],requests=[];
  const model=task.server(http.createServer(async(req,res)=>{
    try {
      if(req.url==="/v1/models"){res.writeHead(200,{"Content-Type":"application/json"});res.end(JSON.stringify({data:[{id:"test"}]}));return;}
      let raw="";for await(const chunk of req) raw+=chunk;
      const body=JSON.parse(raw);requests.push(body);await handle(body,res);
    }catch(error){errors.push(String(error));textReply(res,"FIXTURE_FAILED");}
  }));
  const probe=task.server(net.createServer()), exe=path.join(run,"kzapp.exe");
  const diagnostics={ page_errors:[], failed_invocations:[], resource_errors:[] };
  let app,browser,page,closing;
  const invoke=async(command,args={})=>{
    try { return await page.evaluate(({command,args})=>window.__TAURI__.core.invoke(command,args),{command,args}); }
    catch(cause) {
      diagnostics.failed_invocations.push({command,message:String(cause)});
      throw new Error(`Fixture IPC ${command} failed: ${cause instanceof Error ? cause.message : String(cause)}`,{cause});
    }
  };
  const close=()=>closing ??= (async()=>{
    if(page&&!page.isClosed()) {
      const [captured]=await Promise.allSettled([page.evaluate(async()=>(await import("./00-console-log.js")).uiConsoleLog)]);
      diagnostics.ui_console=captured.status==="fulfilled"?{status:"captured",entries:captured.value}:{status:"failed",error:String(captured.reason)};
    }
    try {
      await closeNativeResources({browser,app,model,probe},page&&!page.isClosed()?()=>invoke("runtime_shutdown"):undefined);
    } catch(error) {
      diagnostics.resource_errors.push(String(error),...(error.errors??[]).map(String));
      throw error;
    } finally {
      await writeFile(path.join(run,"native-diagnostics.json"),JSON.stringify(diagnostics,null,2));
    }
  })();
  try {
    await new Promise((resolve,reject)=>model.once("error",reject).listen(0,"127.0.0.1",resolve));
    await writeFile(path.join(appHome,"kanzei.toml"),`[models]\nprimary="stub:test"\nfast="stub:test"\n[providers.stub]\nprotocol="openai"\nbase_url="http://127.0.0.1:${model.address().port}/v1"\ncontext_limit=128000\n${extraConfig}`);
    await new Promise((resolve,reject)=>probe.once("error",reject).listen(0,"127.0.0.1",resolve));const port=probe.address().port;await new Promise((resolve,reject)=>probe.close(error=>error?reject(error):resolve()));
    await copyFile(path.resolve(executable),exe);
    app=spawn(exe,[],{cwd:project,windowsHide:true,stdio:"ignore",env:{...process.env,KANZEI_HOME:appHome,USERPROFILE:profile,HOME:profile,LOCALAPPDATA:path.join(profile,"AppData/Local"),APPDATA:path.join(profile,"AppData/Roaming"),WEBVIEW2_USER_DATA_FOLDER:path.join(run,"webview"),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`}});
    app.on("error",error=>{errors.push(error.message);diagnostics.resource_errors.push(String(error));});
    await until(async()=>{try{browser=await task.browser(chromium.connectOverCDP(`http://127.0.0.1:${port}`));return true;}catch{return false;}},"Owned native app starts",30000);
    await until(()=>{page=browser.contexts().flatMap(context=>context.pages())[0];return Boolean(page);},"Owned native page exists");
    page.on("pageerror",error=>{errors.push(error.message);diagnostics.page_errors.push({message:error.message,stack:error.stack});});
    await page.waitForFunction(()=>document.body.dataset.appReady==="true");
    await invoke("projects_init",{path:project,name:label});await page.reload();await page.waitForFunction(()=>document.body.dataset.appReady==="true");
    await page.locator(".workbench-project-name").filter({hasText:label}).click();
    const createConversation=async()=>{
      const line=await invoke("process_create",{projectDir:project,profile:"dev",model:"stub:test",subagentMode:"off"});
      await page.evaluate(async id=>{const sessions=await import("./09-sessions.js");await sessions.refreshProcesses({force:true});await sessions.switchProcess(id,true);},line.id);
      return line;
    };
    return {run,project,appHome,exe,child:app,page,invoke,requests,errors,diagnostics,close,createConversation};
  }catch(error){
    try { await close(); } catch(cleanupError) { throw new AggregateError([error,cleanupError],"Native fixture initialization and cleanup failed"); }
    throw error;
  }
}
