import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import http from "node:http";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { ArtifactStore, sameProcess, containsPath, withTask, spawnOwned, withCacheLocks } from "./artifact-lifecycle.mjs";
import { planLegacy, applyLegacy } from "./reclaim-legacy-artifacts.mjs";

const root = fs.mkdtempSync(path.join(os.tmpdir(), "kanzei-artifact-lifecycle-"));
let store;
const checks = [];
const check = (condition, label) => { assert(condition, label); checks.push(label); };
const orphan = run => { run.owner = { ...run.owner, started: "retired-owner-identity" }; store.put(run); };
try {
  execFileSync("git", ["init", "-q", root]);
  execFileSync("git", ["-C", root, "-c", "user.name=Fixture", "-c", "user.email=fixture@local", "commit", "--allow-empty", "-qm", "fixture"]);
  store = new ArtifactStore(root);
  const cache = path.join(root, "target"); fs.mkdirSync(cache);
  const run = await store.begin({ suite: "export", kind: "test", caches: [cache] });
  check(spawnSync("git",["-C",run.environment,"rev-parse","--show-toplevel"],{stdio:"ignore"}).status!==0,"Owned temporary environments cannot inherit the source repository through Git discovery");
  const stamp=BigInt(Date.now())*1000000n,line=`lease-${process.pid}-${stamp}`;
  const nested=path.join(run.environment,"tmp",`kz-k2-lease-${process.pid}-${stamp}`);
  const worktree=path.join(path.dirname(nested),`.kanzei-worktree-${path.basename(nested)}.${line}`);
  fs.mkdirSync(nested);
  execFileSync("git",["init","-q",nested]);
  execFileSync("git",["-C",nested,"-c","user.name=Fixture","-c","user.email=fixture@local","commit","--allow-empty","-qm","fixture"]);
  execFileSync("git",["-C",nested,"worktree","add","-qb",`kanzei/thread-${line}`,worktree]);
  check(execFileSync("git",["-C",worktree,"rev-parse","--is-inside-work-tree"],{encoding:"utf8"}).trim()==="true","Task-owned temporary paths support real nested Git worktree metadata on Windows");
  execFileSync("git",["-C",nested,"worktree","remove",worktree]);
  const previousTemp=process.env.TEMP;
  try{
    process.env.TEMP=path.join(run.environment,"tmp");
    const reopened=new ArtifactStore(root);
    try{check(reopened.environments===store.environments,"Reopening a run uses its registered namespace even when TEMP now points inside that run");}finally{reopened.close();}
  }finally{if(previousTemp===undefined)delete process.env.TEMP;else process.env.TEMP=previousTemp;}
  fs.mkdirSync(path.join(run.environment, "project", ".kanzei"), { recursive: true });
  fs.writeFileSync(path.join(run.environment, "project", ".kanzei", "events.json"), "actual tool receipt");
  fs.mkdirSync(path.join(run.environment, "webview"));
  fs.writeFileSync(path.join(run.environment, "webview", "cache"), "runtime only");
  fs.writeFileSync(path.join(run.environment, "kzapp.exe"), "private binary copy");
  await store.finish(run.id, "success");
  check(!fs.existsSync(run.environment), "Completed environment is removed after evidence export");
  check(fs.readFileSync(path.join(run.evidence, "raw", "project", ".kanzei", "events.json"), "utf8") === "actual tool receipt", "Original project receipts survive environment deletion");
  const manifest=JSON.parse(fs.readFileSync(path.join(run.evidence,"environment-manifest.json"),"utf8"));
  check(manifest.files[0].sha256.length===64 && ["webview","kzapp.exe"].every(name=>manifest.omitted.some(item=>item.path===name)),"Export records hashes and explicitly omits browser cache and program copies");
  check(sameProcess(run.owner,{...run.owner}) && !sameProcess(run.owner,{...run.owner,started:"reused-pid"}),"PID reuse never identifies the old owner as a live lease");
  check(containsPath(root,run.evidence)&&!containsPath(root,path.join(root,"..","outside")),"Sibling paths cannot pass the owned-root boundary");

  const active=await store.begin({suite:"leases",kind:"build",caches:[cache]});
  await assert.rejects(store.resetDevelopmentProfile(cache),/active leases/);
  check(fs.existsSync(active.environment),"A real active run blocks shared-cache deletion");
  await store.finish(active.id,"success");

  const held=await store.begin({suite:"held-process",kind:"test"});
  const child=spawn(process.execPath,["-e","setInterval(()=>{},1000)",held.environment],{windowsHide:true,stdio:"ignore"});
  await once(child,"spawn");
  try {
    await assert.rejects(store.finish(held.id,"success"),/still live/);
    check(fs.existsSync(held.environment)&&store.get(held.id).state==="failure","A live process keeps its environment and prevents a false green result");
  }finally{const exited=once(child,"exit");child.kill();await exited;}
  orphan(store.get(held.id));
  await store.reap();
  check(!fs.existsSync(held.environment),"A later entry reclaims the ended environment after its last process exits");

  const interrupted=await store.begin({suite:"interrupted",kind:"test"});
  fs.writeFileSync(path.join(interrupted.environment,"failure.txt"),"interrupted evidence");
  orphan(interrupted);
  await store.reap();
  check(store.get(interrupted.id).state==="interrupted"&&!fs.existsSync(interrupted.environment),"An interrupted owner's environment is recovered without a time-based deletion guess");
  check(fs.readFileSync(path.join(interrupted.evidence,"raw","failure.txt"),"utf8")==="interrupted evidence","Interrupted-run evidence is exported before recovery");

  const successes=[];
  for(let index=0;index<5;index++){
    const item=await store.begin({suite:"retention",kind:"test"});await store.finish(item.id,"success");
    item.ended_at=Date.now()+index;item.state="success";item.cleanup_state="removed";item.exported=true;store.put(item);
    if(index===0)store.pin("release:current",[item.id]);
    orphan(item);successes.push(item);
  }
  await store.reap();
  check(fs.existsSync(successes[0].evidence)&&!fs.existsSync(successes[1].evidence)&&successes.slice(2).every(item=>fs.existsSync(item.evidence)),"Latest three successes are retained per suite and current-release pins override the count");
  const failed=await store.begin({suite:"failed-expiry",kind:"test"});await store.finish(failed.id,"failure");
  const old=store.get(failed.id);old.ended_at=Date.now()-8*86400000;orphan(old);await store.reap();
  check(!fs.existsSync(failed.evidence),"Unreferenced failure evidence expires after the configured retention period");

  const guarded=await store.begin({suite:"marker-guard",kind:"test"});
  const marker=path.join(guarded.environment,".kanzei-artifact-owner.json"),original=fs.readFileSync(marker);
  fs.writeFileSync(marker,JSON.stringify({project:root,run_id:"another-task",type:"environment"}));orphan(guarded);
  await assert.rejects(store.reap(),/ownership marker mismatch/);
  check(fs.existsSync(guarded.environment),"A mismatched ownership marker prevents deletion");
  fs.writeFileSync(marker,original);await store.reap();

  fs.mkdirSync(path.join(cache,"debug"),{recursive:true});fs.mkdirSync(path.join(cache,"release"),{recursive:true});
  fs.writeFileSync(path.join(cache,"debug","old.obj"),"obsolete full-debug cache");fs.writeFileSync(path.join(cache,"release","kzapp.exe"),"retained release cache");
  const reset=await store.resetDevelopmentProfile(cache);
  check(reset.before.bytes>0&&reset.after.bytes===0&&fs.existsSync(path.join(cache,"release","kzapp.exe")),"A development-profile reset preserves Cargo's lock identities and the release cache");
  await withCacheLocks(cache,async()=>{
    await assert.rejects(withCacheLocks(cache,async()=>{throw new Error("must not acquire");}),/cache lock/);
    check(true,"A real competing profile lock blocks cleanup without treating unrelated builds as cache users");
  });

  const installers=[];
  const clock=Date.now,releaseTime=clock();
  try{
    Date.now=()=>releaseTime;
    for(let index=0;index<3;index++){
      const installer=path.join(root,"installer-"+index+".exe");fs.writeFileSync(installer,"installer "+index);installers.push(installer);
      store.recordRelease("commit-"+index,installer,"packaged");store.recordRelease("commit-"+index,installer,"published");
    }
  }finally{Date.now=clock;}
  check(installers.every(file=>fs.existsSync(file)),"Publishing alone cannot discard the installed rollback version");
  store.recordRelease("commit-0",installers[0],"installed");
  check(installers.every(file=>fs.existsSync(file)),"Accepting an older installed release preserves newer packages still awaiting installation");
  assert.throws(()=>store.recordRelease("invalid-reference",installers[2],"packaged","missing-evidence"),/Cannot pin/);
  check(!store.get("release:invalid-reference"),"Release registration and evidence references commit atomically");
  store.recordRelease("commit-2",installers[2],"installed");
  check(!fs.existsSync(installers[0])&&installers.slice(1).every(file=>fs.existsSync(file)),"Publication plus installation acceptance retains the current and previous installers even when timestamps tie");

  let failureEnvironment,ownedChild,ownedServer;
  await assert.rejects(withTask({root,suite:"failure-finally",kind:"test"},async task=>{
    failureEnvironment=task.environment;fs.writeFileSync(path.join(task.environment,"diagnostics.json"),'{"failure":"injected"}');
    ownedServer=task.server(http.createServer());await new Promise(resolve=>ownedServer.listen(0,"127.0.0.1",resolve));
    ownedChild=spawnOwned(process.execPath,["-e","setInterval(()=>{},1000)"],{windowsHide:true,stdio:"ignore"});await once(ownedChild,"spawn");throw new Error("injected task failure");
  }),/injected task failure/);
  check(!fs.existsSync(failureEnvironment)&&!ownedServer.listening&&(ownedChild.exitCode!==null||ownedChild.signalCode!==null),"Thrown tasks close real servers, wait for owned child exit and reclaim the environment in finally");
  if(process.platform==="win32"){
    const nativeFixture=path.join(root,"native-output.mjs"),nativeRunner=path.join(root,"native-command.ps1");
    fs.writeFileSync(nativeFixture,'console.log("stdout evidence"); console.error("stderr evidence"); process.exitCode = Number(process.argv[2]);');
    fs.writeFileSync(nativeRunner,`param([string]$Helper, [string]$Node, [string]$Fixture, [string]$Evidence, [int]$Code)
$ErrorActionPreference = "Stop"
. $Helper
$run = [pscustomobject]@{ evidence = $Evidence }
$actual = Invoke-ArtifactCommand $Node @($Fixture, "$Code") $run "native-$Code.log"
if ($actual -ne $Code) { throw "Expected exit $Code, received $actual" }
`);
    for(const code of [0,7]){
      const result=spawnSync("powershell.exe",["-NoProfile","-File",nativeRunner,"-Helper",fileURLToPath(new URL("./artifact-lifecycle.ps1",import.meta.url)),"-Node",process.execPath,"-Fixture",nativeFixture,"-Evidence",root,"-Code",String(code)],{encoding:"utf8",windowsHide:true});
      assert.equal(result.status,0,`${result.stdout}\n${result.stderr}`);
      const log=fs.readFileSync(path.join(root,`native-${code}.log`)),receipt=JSON.parse(fs.readFileSync(path.join(root,`native-${code}.log.json`),"utf8"));
      check(log.includes("stdout evidence")&&log.includes("stderr evidence"),`PowerShell 5.1 preserves both output streams for native exit ${code}`);
      check(receipt.exit_code===code&&receipt.log_sha256===createHash("sha256").update(log).digest("hex"),`Native exit ${code} and original log hash determine the command receipt`);
    }
    let privateWorker,privateEnvironment;
    await withTask({root,suite:"private-process",kind:"test"},async task=>{
      privateEnvironment=task.environment;const executable=path.join(task.environment,"fixture-worker.exe");
      await task.copyInput(process.execPath,executable);
      privateWorker=spawn(executable,["-e","setInterval(()=>{},1000)"],{windowsHide:true,stdio:"ignore"});await once(privateWorker,"spawn");
    });
    check(!fs.existsSync(privateEnvironment)&&privateWorker.exitCode!==null,"A private executable left alive by a fixture is stopped through verified Windows process identity before cleanup");
  }

  const outside=path.join(root,"retained-user-data");fs.mkdirSync(outside);fs.writeFileSync(path.join(outside,"keep.txt"),"keep user bytes");
  const linkedCache=path.join(root,"linked-cache");fs.mkdirSync(linkedCache);fs.symlinkSync(outside,path.join(linkedCache,"debug"),process.platform==="win32"?"junction":"dir");
  await assert.rejects(withCacheLocks(linkedCache,async()=>{throw new Error("must not traverse");}),/filesystem link/);
  check(fs.readdirSync(outside).length===1,"A linked cache profile is rejected before any Cargo lock file is written into user data");
  const linked=await store.begin({suite:"nested-link",kind:"test"});fs.symlinkSync(outside,path.join(linked.environment,"linked"),process.platform==="win32"?"junction":"dir");
  await store.finish(linked.id,"success");check(fs.readFileSync(path.join(outside,"keep.txt"),"utf8")==="keep user bytes","Cleanup removes an owned directory's link without following it into user data");

  const legacyRoot=path.join(root,"legacy-temp");fs.mkdirSync(legacyRoot);
  const creator=path.join(root,"crates/kanzei-tools/src/conventions.rs");fs.mkdirSync(path.dirname(creator),{recursive:true});fs.writeFileSync(creator,'// kanzei-conventions-test- creator evidence');
  const deadFixture=path.join(legacyRoot,"kanzei-conventions-test-99999999-123"),liveFixture=path.join(legacyRoot,"kanzei-conventions-test-"+process.pid+"-123");
  for(const folder of [deadFixture,liveFixture]){fs.mkdirSync(folder);fs.writeFileSync(path.join(folder,"result.json"),'{"actual":"historical"}');}
  const plan=await planLegacy(store,legacyRoot);check(plan.fixtures.length===1&&plan.retained.length===1,"Legacy inventory requires a known creator and a dead owner; an active owner stays untouched");
  const imported=await applyLegacy(store,plan);
  check(!fs.existsSync(deadFixture)&&fs.existsSync(liveFixture)&&fs.existsSync(path.join(imported[0].evidence,"raw/historical-fixtures",path.basename(deadFixture),"result.json")),"Legacy reclamation exports original fixture data before removing only the ended environment");
  console.log("Artifact lifecycle PASS: "+checks.length+" ownership, process, evidence, retention and cache invariants");
} finally {
  const environmentRoot=store?.environments;
  store?.close();
  if(environmentRoot){assert(containsPath(os.tmpdir(),environmentRoot));fs.rmSync(environmentRoot,{recursive:true});}
  fs.rmSync(root,{recursive:true});
}
