/* global document */
// One live batch through the actual application and configured DDG path, without retries.
import assert from "node:assert/strict";
import { mkdir,writeFile } from "node:fs/promises";
import path from "node:path";
import { dataset,retrievalScore,canonicalSourceKey } from "./search-quality.mjs";
import { nativeApp,until,textReply,toolReply } from "./fixtures/native-app.mjs";
import { decodeExternalSource } from "./fixtures/tool-source.mjs";

assert(process.platform==="win32" && process.argv[2],"Pass a fresh kzapp.exe");
const output=path.resolve("output/playwright/search-quality/native");await mkdir(output,{recursive:true});
let receipt,finished=false;
const bodies=[],fetchUrls=[];
const app=await nativeApp(process.argv[2],output,"实时搜索质量观察",async(body,res)=>{
  const original=body.messages?.find(message=>message.role==="user" && typeof message.content==="string" && message.content.startsWith("NATIVE_SEARCH_QUALITY:"));
  if(!original){textReply(res,"No candidates.");return;}
  const results=body.messages.filter(message=>message.role==="tool");
  if(!results.length){toolReply(res,"tool_search",{query:"select:websearch"},0);return;}
  if(results.length===1){toolReply(res,"websearch",{queries:dataset.queries.map(({q,domains})=>({q,domains})),max_results:5},1);return;}
  if(results.length===2){
    const decoded=decodeExternalSource(results[1].content);
    receipt=JSON.parse(decoded.content.replace(/^(?:\[tool error\]\n|\[tool_outcome=[^\n]+\]\n)+/,""));
    toolReply(res,"tool_search",{query:"select:webfetch"},2);return;
  }
  if(results.length>3){
    const index=results.length-4,decoded=decodeExternalSource(results.at(-1).content);
    bodies[index]={id:dataset.queries[index].id,url:fetchUrls[index],source:decoded.source,content:decoded.content,is_error:decoded.content.startsWith("[tool error]")};
  }
  const index=results.length-3;
  if(index<4){
    const query=dataset.queries[index];
    const source=receipt.queries[index].results.find(result=>query.expected_urls.some(expected=>canonicalSourceKey(expected)===canonicalSourceKey(result.url)));
    assert(source,"Each successful candidate query must supply a fixed relevant source before body verification");
    fetchUrls[index]=source.url;
    const card=dataset.documents.find(document=>document.target===query.id&&document.level==="primary");
    toolReply(res,"webfetch",{ref:source.ref,find:card.find,max_chars:12000},results.length);return;
  }
  finished=true;
  textReply(res,`NATIVE_SEARCH_QUALITY_DONE: covered candidates ${receipt.queries.filter(query=>query.results.length).length}/4; retrieval failures and uncovered vendors remain explicit.`);
},"[web]\nsearch_backend=\"duckduckgo\"\n");
try{
  const line=await app.createConversation();
  await app.invoke("run_prompt",{projectDir:app.project,processId:line.id,prompt:"NATIVE_SEARCH_QUALITY: 对四个明确对象各搜索一次，保留缺失和失败。",profile:"dev",agent:"dev-pair",model:"stub:test",autonomous:true,autoAllow:true});
  await until(async()=>{assert.equal(app.errors.length,0,app.errors.join("; "));return finished && (await app.invoke("collaboration_snapshot",{projectDir:app.project})).find(row=>row.process_id===line.id)?.status==="completed";},"Live search and body observations finish",180000);
  assert.equal(receipt.queries.length,4);
  const groups=receipt.queries.map((query,index)=>{
    assert.equal(query.query,dataset.queries[index].q);
    assert.equal(query.attempts.length,1,"Exactly one backend attempt per frozen query");
    assert.equal(query.attempts[0].backend,"duckduckgo");
    if(query.status==="failed") assert(query.error && query.results.length===0,"Failure cannot become a healthy empty search");
    return {id:dataset.queries[index].id,...query};
  });
  const score=retrievalScore(groups);
  const result={observed_at:new Date().toISOString(),dataset_revision:dataset.schema,scope:dataset.scope,backend:"duckduckgo",attempts:4,groups,score,bodies,health_observed:true,candidate_retrieval_pass:score.coverage===1,answer_quality_pass:null,successful_body_fetches:bodies.filter(body=>!body.is_error).length,recency_assessment:"not assessed: these queries request stable documentation, not recent news",model:"deterministic fixture controls requests; no claim of Luna answer quality",executable:app.exe};
  await writeFile(path.join(app.run,"live-results.json"),JSON.stringify(result,null,2));
  await writeFile(path.join(output,"live-results.json"),JSON.stringify(result,null,2));
  await app.page.screenshot({path:path.join(output,"native-live-search.png")});
  console.log(`Live search observation complete: candidate coverage ${score.covered}/${score.total}; statuses ${groups.map(group=>group.status).join(",")}; answer quality awaits separate evaluation`);
}catch(error){await writeFile(path.join(app.run,"failure.json"),JSON.stringify({error:String(error),receipt,bodies,errors:app.errors},null,2));throw error;}
finally{await app.close();}
