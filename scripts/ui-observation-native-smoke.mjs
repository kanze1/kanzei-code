/* global document */
// Real UI send -> repeated real read executions -> projected provider request -> durable replay.
import assert from "node:assert/strict";
import { readFile,writeFile,mkdir } from "node:fs/promises";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { nativeApp,until,textReply,toolReply } from "./fixtures/native-app.mjs";

assert(process.platform==="win32" && process.argv[2],"Pass a fresh kzapp.exe");
const output=path.resolve("output/playwright/observations/native");await mkdir(output,{recursive:true});
const source="NATIVE_OBSERVATION_ORIGINAL_SOURCE\n"+"pub fn repeated_evidence() -> bool { true }\n".repeat(200);
let finalRequest;
const app=await nativeApp(process.argv[2],output,"上下文投影原生验收",async(body,res)=>{
  const user=body.messages?.find(message=>message.role==="user" && typeof message.content==="string" && message.content.startsWith("NATIVE_OBSERVATIONS:"));
  if(!user){textReply(res,"No candidates.");return;}
  const results=body.messages.filter(message=>message.role==="tool");
  if(results.length<4){toolReply(res,"read",{path:"source.rs"},results.length);return;}
  finalRequest=body;textReply(res,"NATIVE_OBSERVATIONS_DONE: repeated_evidence returns true, from source.rs.");
});
const checks=[],check=(value,label)=>{assert(value,label);checks.push(label);};let database;
try{
  await writeFile(path.join(app.project,"source.rs"),source);
  const line=await app.createConversation();
  await app.page.locator("#composer-more").click();
  await app.page.locator("#profile-select").selectOption("isolated");
  await app.page.locator("#prompt").fill("NATIVE_OBSERVATIONS: 读取固定源码后给出结论与来源。");await app.page.locator("#send").click();
  await until(async()=>{assert.equal(app.errors.length,0,app.errors.join("; "));return finalRequest && (await app.invoke("collaboration_snapshot",{projectDir:app.project})).find(row=>row.process_id===line.id)?.status==="completed";},"Four actual reads complete");
  const results=finalRequest.messages.filter(message=>message.role==="tool");
  check(results.length===4,"All four actual tool results retain their call identities");
  check(results.filter(result=>result.content.startsWith("[observation_reference]\n")).length===3,"Provider receives references for exactly the three repeated old observations");
  check(results.filter(result=>result.content.includes("NATIVE_OBSERVATION_ORIGINAL_SOURCE")).length===1,"Newest full source evidence remains in the actual provider request");
  check(results.at(-1).content.includes("repeated_evidence"),"Actual provider can inspect the complete retained source");
  check(await readFile(path.join(app.project,"source.rs"),"utf8")===source,"Observation projection never changes the project file");
  database=new DatabaseSync(path.join(app.project,".kanzei/state.db"),{readOnly:true});
  const facts=database.prepare("SELECT payload_json FROM session_events WHERE event_type='session.tool_result_committed'").all().map(row=>JSON.parse(row.payload_json));
  const saved=facts.filter(fact=>JSON.stringify(fact).includes("NATIVE_OBSERVATION_ORIGINAL_SOURCE"));
  check(saved.length===4,"Durable facts retain all four full original observations");
  check(!saved.some(fact=>JSON.stringify(fact).includes("[observation_reference]")),"Request-only references do not overwrite persistent evidence");
  await app.page.reload();await app.page.waitForFunction(()=>document.body.dataset.appReady==="true");
  await app.page.evaluate(async id=>{const sessions=await import("./09-sessions.js");await sessions.refreshProcesses({force:true});await sessions.switchProcess(id,true);},line.id);
  check((await app.page.locator("#messages").innerText()).includes("NATIVE_OBSERVATIONS_DONE"),"Answer and source survive actual history replay");
  await app.page.screenshot({path:path.join(output,"native-observations.png")});
  await writeFile(path.join(output,"acceptance.json"),JSON.stringify({passed:checks.length,checks,request_count:app.requests.length,executable:app.exe},null,2));
  console.log(`Observation native PASS: ${checks.length} checks`);
}catch(error){await writeFile(path.join(output,"failure.json"),JSON.stringify({checks,errors:app.errors,requests:app.requests},null,2));throw error;}
finally{database?.close();await app.close();}
