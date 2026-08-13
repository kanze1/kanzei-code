// 探针 v13:全部 msedgewebview2 进程 + 各自监听端口
import { mkdtempSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawn, execSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const exe = join(root, "target", "debug", "kzapp.exe");
const sandbox = mkdtempSync(join(tmpdir(), "kz-e2e-probe-"));
const userData = join(sandbox, "webview");
const home = join(sandbox, "home");
const PORT = 9333;
// R-200:全局根隔离三连(HOME/USERPROFILE/KANZEI_HOME)缺一即退回读开发者真实配置。
const env = { ...process.env, HOME: home, USERPROFILE: home, KANZEI_HOME: join(home, ".kanzei"), WEBVIEW2_USER_DATA_FOLDER: userData, KANZEI_E2E_CDP: String(PORT) };
const child = spawn(exe, [], { env });
await new Promise((r) => setTimeout(r, 8000));
console.log(`kzapp pid=${child.pid} 存活: ${child.exitCode === null}`);
const cmd = execSync(
  'powershell -NoProfile -Command "Get-CimInstance Win32_Process | Where-Object { $_.Name -eq \'msedgewebview2.exe\' } | Select-Object ProcessId,CommandLine | ConvertTo-Json -Depth 1"',
  { encoding: "utf8", timeout: 10000, maxBuffer: 16 * 1024 * 1024 }
);
let procs = [];
try { procs = JSON.parse(cmd); if (!Array.isArray(procs)) procs = [procs]; } catch { console.log("JSON 解析失败"); }
console.log(`msedgewebview2 进程数: ${procs.length}`);
const webviewPids = [];
for (const p of procs) {
  const type = (p.CommandLine.match(/--type=([^\s]+)/) ?? [null, "browser"])[1];
  const cdp = p.CommandLine.includes("remote-debugging-port");
  console.log(`  pid=${p.ProcessId} type=${type} cdp=${cdp}`);
  webviewPids.push(String(p.ProcessId));
}
const net = execSync("netstat -ano", { encoding: "utf8", timeout: 15000 });
const listening = net.split(/\r?\n/).filter((l) => {
  const parts = l.trim().split(/\s+/);
  return parts.length >= 5 && parts[3] === "LISTENING" && webviewPids.includes(parts[4]);
});
console.log(`webview 进程监听端口: ${listening.length}`);
for (const l of listening) console.log(`  ${l.trim()}`);
// 无监听时,再试试直接 fetch localhost 与 127.0.0.1
for (const host of ["127.0.0.1", "localhost", "[::1]"]) {
  try {
    const r = await fetch(`http://${host}:${PORT}/json`, { signal: AbortSignal.timeout(2000) });
    console.log(`fetch ${host}:${PORT} -> ${r.status}`);
  } catch (e) { console.log(`fetch ${host}:${PORT} -> 失败(${e.cause?.code ?? e.message.slice(0, 40)})`); }
}
if (child.exitCode === null) child.kill();
