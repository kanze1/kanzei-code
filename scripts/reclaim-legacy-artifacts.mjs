// One-time migration of positively identified old test environments into run-owned evidence.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { ArtifactStore, containsPath, directorySize, processSnapshot } from "./artifact-lifecycle.mjs";

const creators = [
  ["kanzei-conventions-test-", /^kanzei-conventions-test-(?<pid>\d+)-\d+$/, "crates/kanzei-tools/src/conventions.rs"],
  ["kz-mem-index-", /^kz-mem-index-(?<pid>\d+)-\d+$/, "crates/kanzei-memory/src/memory/index.rs"],
  ["kz-c6-startup-", /^kz-c6-startup-(?<pid>\d+)-\d+-\d+$/, "crates/kanzei-app/src/run/assembly.rs"],
  ["kz-verification-test-", /^kz-verification-test-(?<pid>\d+)-\d+-\d+$/, "crates/kanzei-tools/src/verification/tests.rs"],
  ["kz-base-", /^kz-base-.+-(?<pid>\d+)-\d+$/, "crates/kanzei-base/src/atomic_file/tests.rs"],
  ["kz-journal-", /^kz-journal-.+-(?<pid>\d+)-\d+$/, "crates/kanzei-base/src/write_log/tests.rs"],
  ["kz-replay-eval-", /^kz-replay-eval-(?:cand-|loo-)?(?<pid>\d+)$/, "crates/kanzei-memory/src/replay_eval.rs"],
];
const digest = file => createHash("sha256").update(fs.readFileSync(file)).digest("hex");
function checkedPath(parent, file) {
  const resolved = fs.realpathSync.native(file), base = fs.realpathSync.native(parent);
  if (!containsPath(base,resolved) || resolved===base) throw new Error("Legacy target escapes its explicitly named root: "+file);
  for(let current=file;current!==parent;current=path.dirname(current)) if(fs.lstatSync(current).isSymbolicLink()) throw new Error("Legacy target traverses a link: "+file);
  return resolved;
}
function referenced(directory,snapshot){return snapshot.some(row=>(row.exe&&containsPath(directory,row.exe))||row.command?.toLowerCase().includes(directory.toLowerCase()));}
export async function planLegacy(store,tempRoot=os.tmpdir()) {
  tempRoot=fs.realpathSync.native(tempRoot);
  const snapshot=await processSnapshot(),pids=new Set(snapshot.map(row=>row.pid)),fixtures=[],caches=[],retained=[],protectedFiles=[];
  for(const entry of fs.readdirSync(tempRoot,{withFileTypes:true})){
    if(!entry.isDirectory()||entry.isSymbolicLink())continue;
    const creator=creators.find(([,expression])=>expression.test(entry.name));if(!creator)continue;
    const [prefix,expression,source]=creator,sourceFile=path.join(store.worktree,source);
    if(!fs.readFileSync(sourceFile,"utf8").includes(prefix))throw new Error("Creator evidence changed: "+source);
    const directory=path.join(tempRoot,entry.name),pid=Number(expression.exec(entry.name).groups.pid);
    if(pids.has(pid)||referenced(directory,snapshot)){retained.push({path:directory,reason:"live-owner-or-reference"});continue;}
    checkedPath(tempRoot,directory);
    fixtures.push({path:directory,pid,source,source_sha256:digest(sourceFile),...directorySize(directory),modified_at:fs.statSync(directory).mtimeMs});
  }
  const maintenance=path.join(store.project,"output","maintenance");
  if(fs.existsSync(maintenance))for(const entry of fs.readdirSync(maintenance,{withFileTypes:true})){
    if(!entry.isDirectory()||entry.isSymbolicLink())continue;
    const archive=path.join(maintenance,entry.name),receipt=path.join(archive,"closeout-report.json"),manifest=path.join(archive,"manifest.json"),preserved=path.join(archive,"preserved-ignored");
    if(!fs.existsSync(receipt)||!fs.existsSync(manifest)||!fs.existsSync(preserved))continue;
    const report=JSON.parse(fs.readFileSync(receipt,"utf8")),owner=JSON.parse(fs.readFileSync(manifest,"utf8"));
    if(report.status!=="closed_out_with_preserved_user_changes"||fs.existsSync(owner.worktree))continue;
    const critical=report.evidence.critical_files;
    if(!Array.isArray(critical))throw new Error("Closeout receipt lacks its critical-file inventory: "+receipt);
    for(const item of critical)if(!fs.existsSync(item.current)||digest(item.current)!==item.sha256)throw new Error("Retained release evidence changed: "+item.current);
    protectedFiles.push(...critical.map(item=>({path:item.current,sha256:item.sha256})));
    // Source closeout receipt is hashed and rechecked on apply; its retained files remain untouched.
    const walk=directory=>{
      const names=new Set(fs.readdirSync(directory));
      if(names.has("profile")&&names.has("kzapp.exe")&&(names.has("project")||names.has("project-A"))){
        for(const name of ["profile","webview","kzapp.exe"]){
          const file=path.join(directory,name);if(!fs.existsSync(file)||critical.some(item=>item.current===file))continue;
          checkedPath(preserved,file);
          if(referenced(file,snapshot)){retained.push({path:file,reason:"live-process"});continue;}
          const stat=fs.statSync(file);
          caches.push({path:file,archive_root:preserved,receipt,receipt_sha256:digest(receipt),bytes:stat.isDirectory()?directorySize(file).bytes:stat.size,sha256:stat.isFile()?digest(file):null});
        }
      }
      for(const name of names){
        const file=path.join(directory,name),stat=fs.lstatSync(file);
        if(stat.isDirectory()&&!stat.isSymbolicLink()&&!["profile","webview","node_modules",".git","target"].includes(name))walk(file);
      }
    };
    walk(preserved);
  }
  return {schema_version:1,project:store.project,worktree:store.worktree,created_at:Date.now(),temp_root:tempRoot,fixtures,caches,retained,protected_files:protectedFiles};
}
function copyFixture(source,destination,omitted){
  fs.mkdirSync(destination,{recursive:true});
  for(const entry of fs.readdirSync(source,{withFileTypes:true})){
    const file=path.join(source,entry.name),out=path.join(destination,entry.name),stat=fs.lstatSync(file);
    if(stat.isSymbolicLink()||["target","node_modules",".git","__pycache__"].includes(entry.name)||/^index\.db(?:-wal|-shm|-journal)?$/.test(entry.name)){
      omitted.push({path:file,reason:"rebuildable-cache-or-link"});continue;
    }
    if(entry.isDirectory())copyFixture(file,out,omitted);else fs.copyFileSync(file,out);
  }
}
export async function applyLegacy(store,plan){
  if(plan.project!==store.project||plan.worktree!==store.worktree)throw new Error("Legacy plan belongs to another project or checkout");
  if(!Array.isArray(plan.protected_files))throw new Error("Legacy plan lacks its protected release evidence inventory");
  for(const item of plan.protected_files)if(digest(item.path)!==item.sha256)throw new Error("Protected release evidence changed: "+item.path);
  for(const item of [...plan.fixtures,...plan.caches])if(plan.protected_files.some(protectedFile=>containsPath(item.path,protectedFile.path)))throw new Error("Legacy candidate contains protected release evidence: "+item.path);
  const results=[];
  for(const [suite,items] of [["legacy-rust-fixtures",plan.fixtures],["legacy-ui-environments",plan.caches]]){
    if(!items.length)continue;
    const run=await store.begin({kind:"legacy-migration",suite}),retired=path.join(run.environment,"retired"),snapshot=await processSnapshot();
    fs.mkdirSync(retired);
    const moved=[],retained=[],omitted=[];
    try{
      for(const item of items){
        if(!fs.existsSync(item.path)){retained.push({path:item.path,reason:"already-absent"});continue;}
        checkedPath(item.archive_root??plan.temp_root,item.path);
        if(referenced(item.path,snapshot)||(item.pid&&snapshot.some(row=>row.pid===item.pid))){retained.push({path:item.path,reason:"live-process"});continue;}
        if(item.source&&digest(path.join(store.worktree,item.source))!==item.source_sha256)throw new Error("Fixture creator changed since plan: "+item.source);
        if(item.receipt&&digest(item.receipt)!==item.receipt_sha256)throw new Error("Closeout receipt changed since plan: "+item.receipt);
        const stat=fs.statSync(item.path);
        if(item.source&&(stat.mtimeMs!==item.modified_at||directorySize(item.path).bytes!==item.bytes))throw new Error("Legacy fixture changed since plan: "+item.path);
        if(item.sha256&&digest(item.path)!==item.sha256)throw new Error("Program copy changed since plan: "+item.path);
        if(item.source)copyFixture(item.path,path.join(run.environment,"historical-fixtures",path.basename(item.path)),omitted);
        const destination=path.join(retired,String(moved.length));
        if(!containsPath(store.environments,destination))throw new Error("Legacy destination escapes the managed environment root");
        fs.renameSync(item.path,destination);
        moved.push(item);
        fs.appendFileSync(path.join(run.environment,"reclaimed.jsonl"),JSON.stringify(item)+"\n");
      }
      fs.writeFileSync(path.join(run.environment,"migration.json"),JSON.stringify({plan_created_at:plan.created_at,moved,retained,omitted,historical_result:suite==="legacy-rust-fixtures"?"unknown; retain as interrupted-run evidence for seven days":"archived test environment; original receipts and project evidence remain"},null,2));
      const complete=await store.finish(run.id,suite==="legacy-rust-fixtures"?"interrupted":"success");
      results.push({run_id:run.id,evidence:complete.evidence,moved:moved.length,source_bytes:moved.reduce((sum,item)=>sum+item.bytes,0),retained});
    }catch(error){
      fs.writeFileSync(path.join(run.environment,"migration-error.json"),JSON.stringify({error:String(error),moved,retained},null,2));
      await store.finish(run.id,"failure");throw error;
    }
  }
  return results;
}
async function main(){
 const [command,file]=process.argv.slice(2),store=new ArtifactStore();
 try{
  if(command==="plan"){
   const plan=await planLegacy(store);
   fs.writeFileSync(file,JSON.stringify(plan,null,2));
   console.log(JSON.stringify({plan:file,fixtures:plan.fixtures.length,fixture_bytes:plan.fixtures.reduce((sum,item)=>sum+item.bytes,0),cache_items:plan.caches.length,cache_bytes:plan.caches.reduce((sum,item)=>sum+item.bytes,0),retained:plan.retained.length}));
  }else if(command==="apply"){
   const result=await applyLegacy(store,JSON.parse(fs.readFileSync(file,"utf8")));
   fs.writeFileSync(file.replace(/\.json$/,"-result.json"),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
  }else throw new Error("Usage: reclaim-legacy-artifacts.mjs plan|apply <plan.json>");
 }finally{store.close();}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))main().catch(error=>{console.error(error);process.exitCode=1;});
