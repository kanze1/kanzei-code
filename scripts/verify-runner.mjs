// Dependency-aware execution. Cargo shares one queue; browser suites have a
// bounded queue; host measurements run alone. Every check keeps its own log.
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { performance } from "node:perf_hooks";
import { pathToFileURL } from "node:url";

export async function runChecks({ checks, root, directory, runId, ci = false, workers = Math.min(4, os.availableParallelism()), browserWorkers = 2, report = console.log }) {
  if (!Number.isInteger(workers) || workers < 1 || !Number.isInteger(browserWorkers) || browserWorkers < 1) throw new Error("Invalid verification concurrency");
  const pending = [...checks], running = new Map(), results = new Map();
  const started = performance.now();
  fs.mkdirSync(directory, { recursive: true });
  const resources = check => new Set([...(check.resources ?? []), ...(check.program === "cargo" ? ["cargo"] : [])]);
  const canStart = check => {
    if (running.size >= workers) return false;
    const wanted = resources(check), held = [...running.values()].map(entry => resources(entry.check));
    if (wanted.has("host")) return running.size === 0;
    if (held.some(set => set.has("host"))) return false;
    return (!wanted.has("cargo") || !held.some(set => set.has("cargo"))) &&
      (!wanted.has("browser") || held.filter(set => set.has("browser")).length < browserWorkers);
  };
  const execute = async (check, dependencyError) => {
    const at = performance.now(), args = ci ? (check.ci_args ?? check.args) : check.args;
    const log = path.join(directory, `${check.key}.log`);
    const output = fs.createWriteStream(log);
    const hash = createHash("sha256");
    const write = data => { output.write(data); hash.update(data); };
    const result = { status: "pass", exit_code: 0, elapsed_seconds: 0, command: [check.program, ...args], log_path: `dist/verification-runs/${runId}/${check.key}.log`, log_sha256: "" };
    report(`==> ${check.key}`);
    if (dependencyError) {
      result.status = "fail"; result.exit_code = 1; result.error = dependencyError;
      write(dependencyError + "\n");
    } else {
      const temp = fs.mkdtempSync(path.join(process.env.TEMP ?? os.tmpdir(), "check-"));
      result.temporary_directory = temp;
      await new Promise(resolve => {
        const child = spawn(check.program, args, { cwd: root, windowsHide: true, env: { ...process.env, TEMP: temp, TMP: temp, TMPDIR: temp }, stdio: ["ignore", "pipe", "pipe"] });
        child.stdout.on("data", write); child.stderr.on("data", write);
        child.once("error", error => { result.error = error.message; write(error.message + "\n"); });
        child.once("close", (code, signal) => {
          result.exit_code = code ?? 1;
          if (code !== 0) { result.status = "fail"; result.error ??= signal ? `Terminated by ${signal}` : `Check exited with ${result.exit_code}`; }
          resolve();
        });
      });
    }
    await new Promise((resolve, reject) => { output.once("error", reject); output.end(resolve); });
    result.elapsed_seconds = Math.round((performance.now() - at) * 1000) / 1e6;
    result.log_sha256 = hash.digest("hex");
    report(`${result.status === "pass" ? "OK" : "FAIL"} ${check.key} (${result.elapsed_seconds.toFixed(1)}s)${result.error ? `: ${result.error}` : ""}`);
    results.set(check.key, result);
  };
  while (pending.length || running.size) {
    for (let index = 0; index < pending.length;) {
      const check = pending[index], dependencies = check.depends_on ?? [];
      if (!dependencies.every(key => results.has(key)) || !canStart(check)) { index++; continue; }
      pending.splice(index, 1);
      const failed = dependencies.find(key => results.get(key).status !== "pass");
      const promise = execute(check, failed && `Dependency did not pass: ${failed}`).finally(() => running.delete(check.key));
      running.set(check.key, { check, promise });
    }
    if (running.size) await Promise.race([...running.values()].map(entry => entry.promise));
    else if (pending.length) throw new Error(`Unresolved verification dependencies: ${pending.map(check => check.key).join(", ")}`);
  }
  return { checks: Object.fromEntries(checks.map(check => [check.key, results.get(check.key)])), all_pass: [...results.values()].every(result => result.status === "pass"), elapsed_seconds: (performance.now() - started) / 1000, workers, browser_workers: browserWorkers };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const [planFile, directory, runId, environment] = process.argv.slice(2);
  const plan = JSON.parse(fs.readFileSync(planFile, "utf8").replace(/^\uFEFF/, ""));
  const result = await runChecks({ checks: plan.checks, root: process.cwd(), directory, runId, ci: environment === "ci" });
  fs.writeFileSync(path.join(directory, "results.json"), JSON.stringify({ ...result, execution_environment: environment }, null, 2) + "\n");
  process.exitCode = result.all_pass ? 0 : 1;
}
