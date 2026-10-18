// The legacy WinForms UIA provider once exposed password text as a Pane name.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { until } from "./fixtures/native-app.mjs";

const output=path.resolve("output/remediation-20261018/desktop/legacy"),run=path.join(output,String(Date.now()));
await mkdir(run,{recursive:true});
assert(process.env.SystemRoot&&process.env.ProgramFiles);
const fixture=spawn(path.join(process.env.SystemRoot,"System32/WindowsPowerShell/v1.0/powershell.exe"),["-NoProfile","-Sta","-File",path.resolve("scripts/fixtures/desktop-form.ps1"),"-StateDirectory",run],{windowsHide:true,stdio:"ignore"});
try {
  await until(()=>existsSync(path.join(run,"window.json")),"Legacy WinForms fixture opens");
  const window=JSON.parse(await readFile(path.join(run,"window.json"),"utf8"));
  const inspect=spawnSync(path.join(process.env.ProgramFiles,"PowerShell/7/pwsh.exe"),["-NoProfile","-NonInteractive","-Mta","-File",path.resolve("scripts/desktop-helper.ps1"),"-RequestJson",JSON.stringify({action:"inspect",window})],{windowsHide:true,encoding:"utf8",timeout:30000,maxBuffer:8*1024*1024});
  assert(!inspect.error,String(inspect.error));
  const result=JSON.parse(inspect.stdout);
  assert(result.ok,result.error);
  assert(!JSON.stringify(result.snapshot).includes("UIA_PASSWORD_CANARY"),"Legacy UIA provider must not expose password text through names or values");
  assert(result.snapshot.elements.some(element=>element.password),"The native password style remains detectable even when UIA reports a Pane");
  await writeFile(path.join(output,"acceptance.json"),JSON.stringify({passed:2,run,checks:["Legacy password name and value are redacted","Native password identity remains detectable"]},null,2));
  console.log("Legacy UIA password boundary PASS: 2 checks");
} finally { fixture.kill(); }
