// 实际 CLI + 专用 worker：取消和崩溃必须结束子进程；中断只阻塞关联单元。
// node scripts/verification-worker-smoke.mjs <reader-lab-directory>
import { execFileSync } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import assert from "node:assert/strict";

const repo = path.resolve(import.meta.dirname, "..");
const lab = path.resolve(process.argv[2]);
const { source } = JSON.parse(await readFile(path.join(lab, "result.json"), "utf8"));
const kz = path.join(repo, "target/debug", process.platform === "win32" ? "kz.exe" : "kz");
const calls = [];
function cli(args, json = true) {
  const output = execFileSync(kz, args, { cwd: source, env: { ...process.env, KANZEI_PROJECT_ROOT: lab }, encoding: "utf8", windowsHide: true, timeout: 15000 });
  calls.push({ args, output }); return json ? JSON.parse(output) : output;
}
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const readJob = (id) => readFile(path.join(lab, ".kanzei/verification", `${id}.json`), "utf8").then(JSON.parse);
async function waitFor(id, predicate, seconds = 12) {
  const deadline = Date.now() + seconds * 1000;
  while (Date.now() < deadline) {
    const job = await readJob(id);
    if (predicate(job)) return job;
    await sleep(200);
  }
  throw new Error(`verification ${id} did not reach expected state`);
}
function alive(pid) { try { process.kill(pid, 0); return true; } catch { return false; } }
async function start(title) {
  const unit = cli(["work", "create-unit", "--requirement", "R-001", "--objective", title, "--acceptance", title]);
  cli(["work", "claim", unit.unit_id, "--reason", "用户授权的验证 worker 真实故障场景"], false);
  const command = process.platform === "win32" ? "Start-Sleep -Seconds 60" : "sleep 60";
  const receipt = cli(["work", "verify-async", unit.unit_id, "--command", command, "--criterion", title, "--environment", "worker lifecycle experiment", "--resource", `worker-smoke-${unit.unit_id}`, "--timeout-ms", "90000"]);
  return waitFor(receipt.verification_job, (j) => j.status === "running" && j.command_pid);
}

const cancelled = await start("取消验证后没有成功证据");
cli(["work", "verification-cancel", cancelled.id], false);
const cancelledResult = await waitFor(cancelled.id, (j) => j.status === "cancelled");
assert(!alive(cancelled.command_pid), "取消后命令进程必须结束");
assert.equal(cli(["work", "get-unit", cancelled.unit_id]).unit.evidence.length, 0);
console.log("cancelled: command stopped, no success evidence");

const crashed = await start("验证进程崩溃后可恢复");
process.kill(crashed.worker_pid, "SIGKILL");
for (let i = 0; i < 30 && alive(crashed.command_pid); i++) await sleep(100);
if (process.platform === "win32") assert(!alive(crashed.command_pid), "worker 退出后 Job Object 必须收口子进程");
console.log("worker killed: child contained; waiting for orphan recovery window");
await sleep(31000);
cli(["work", "verification-jobs"]);
const recovered = await readJob(crashed.id);
assert.equal(recovered.status, "interrupted");
assert.equal(cli(["work", "get-unit", crashed.unit_id]).unit.status, "blocked");
const next = cli(["work", "next", "--detail"]);
assert.equal(next.decision, "start");
await writeFile(path.join(lab, "worker-lifecycle.json"), JSON.stringify({ cancelled: cancelledResult, crashed: recovered, next, calls }, null, 2));
console.log("worker lifecycle passed: cancellation, process containment, interrupted recovery, independent work remains executable");
