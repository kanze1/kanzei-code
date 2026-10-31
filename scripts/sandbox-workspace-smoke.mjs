// Real namespace and persistence acceptance; missing WSL/Python/bubblewrap fails.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import readline from "node:readline";
import { withTask, spawnOwned } from "./artifact-lifecycle.mjs";

await withTask({suite:"sandbox-workspace-smoke"},async task=>{
  const helper=await readFile("scripts/sandbox-helper.py","utf8");task.input("scripts/sandbox-helper.py");
  const personal=path.join(task.environment,"personal"),group=path.join(task.environment,"group"),outside=path.join(task.environment,"host-secret.txt");
  await mkdir(personal);await mkdir(group);await writeFile(outside,"HOST_SECRET_CANARY");
  const linux=file=>process.platform==="win32"?execFileSync("wsl.exe",["--exec","/usr/bin/wslpath","-a",file],{encoding:"utf8",windowsHide:true}).trim():file;
  const checks=[],receipts=[];
  const check=(ok,label)=>{assert(ok,label);checks.push(label);console.log("PASS "+label);};
  async function execute(workspace,command,{timeout=15000,onStarted}={}){
    const args=["-I","-u","-c",helper];
    const child=process.platform==="win32"?spawnOwned("wsl.exe",["--exec","/usr/bin/python3",...args],{windowsHide:true,stdio:["pipe","pipe","pipe"]}):spawnOwned("/usr/bin/python3",args,{stdio:["pipe","pipe","pipe"]});
    const diagnostics=[];child.stderr.on("data",bytes=>diagnostics.push(bytes));
    child.stdin.write(JSON.stringify({workspace:linux(workspace),command,timeout_ms:timeout})+"\n");
    let result;
    for await(const line of readline.createInterface({input:child.stdout})){
      const event=JSON.parse(line);
      if(event.event==="started"&&onStarted)await onStarted(child);
      if(event.event==="result")result=event.result;
    }
    const code=child.exitCode??await new Promise(resolve=>child.once("exit",resolve));child.stdin.end();
    assert.equal(code,0,Buffer.concat(diagnostics).toString());assert(result,"execution receipt");receipts.push(result);return result;
  }
  const quote=value=>"'"+value.replaceAll("'","'\\''")+"'";
  const inspect=`import pathlib,os,socket
assert not pathlib.Path(${JSON.stringify(linux(outside))}).exists()
assert not any(pathlib.Path(p).exists() for p in ['/mnt/c','/run/WSL','/init','/home/kanzei'])
assert not os.environ.get('WSL_INTEROP')
assert set(os.environ) <= {'PATH','HOME','LANG','TMPDIR','PWD','LC_CTYPE','SHLVL','_'}
for host in ['127.0.0.1','1.1.1.1','169.254.169.254']:
    with socket.socket() as s:
        s.settimeout(.2)
        assert s.connect_ex((host,443)) != 0
pathlib.Path('tasks').mkdir(exist_ok=True)
pathlib.Path('tasks/result.txt').write_text('PERSISTED_RESULT')
print('HOST_AND_NETWORK_ISOLATED')`;
  let receipt=await execute(personal,"python3 -c "+quote(inspect));
  check(receipt.isolation_verified&&receipt.exit_code===0&&receipt.output.includes("HOST_AND_NETWORK_ISOLATED"),"Namespace, host files, interop, environment, loopback and internet sockets isolated");
  check(receipt.workspace==="persistent"&&receipt.host_writes==="workspace_only","Receipt names the actual persistent write boundary");
  receipt=await execute(personal,"cat tasks/result.txt; printf SECOND_RUN >> tasks/result.txt");
  check(receipt.exit_code===0&&receipt.output.includes("PERSISTED_RESULT"),"A separate command reads previous task files");
  check(await readFile(path.join(personal,"tasks/result.txt"),"utf8")==="PERSISTED_RESULTSECOND_RUN","Only the owned workspace retains writes");
  receipt=await execute(group,"test ! -e tasks/result.txt; printf GROUP_ONLY > group.txt");
  check(receipt.exit_code===0,"Group workspace cannot see personal task files");
  receipt=await execute(personal,"test ! -e group.txt; printf FORGED > /usr/kanzei-sandbox-canary");
  check(receipt.exit_code!==0,"Runtime filesystem is immutable and group files stay absent");
  let overlapping;
  receipt=await execute(personal,"sleep 60",{onStarted:async child=>{
    overlapping=await execute(personal,"printf OVERLAP > should-not-exist");child.stdin.write("cancel\n");
  }});
  check(overlapping.status==="refused"&&!overlapping.isolation_verified,"Concurrent commands cannot mutate a shared workspace");
  check(receipt.status==="cancelled"&&receipt.isolation_verified,"Cancellation acknowledges namespace cleanup");
  receipt=await execute(personal,"sleep 60",{timeout:200});
  check(receipt.status==="timeout"&&receipt.isolation_verified,"Deadline terminates the sandbox");
  receipt=await execute(personal,"test ! -e should-not-exist; cat tasks/result.txt");
  check(receipt.exit_code===0,"Workspace lock releases after cancellation and timeout");
  check(await readFile(outside,"utf8")==="HOST_SECRET_CANARY","Host canary remains unchanged");
  await writeFile(path.join(task.evidence,"receipt.json"),JSON.stringify({boundary:"Actual WSL/Linux bubblewrap namespaces; persistent isolated workspaces; no mock executor",checks,receipts},null,2));
});
