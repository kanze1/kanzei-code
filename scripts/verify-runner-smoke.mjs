import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { runChecks } from "./verify-runner.mjs";

const root = fs.mkdtempSync(path.join(os.tmpdir(), "kanzei-verify-scheduler-"));
try {
  const fixture = path.join(root, "fixture.mjs");
  fs.writeFileSync(fixture, `
import fs from 'node:fs';
import assert from 'node:assert/strict';
const action = process.argv[2];
const waitFor = file => new Promise((resolve, reject) => {
  const watcher = fs.watch('.', () => { if(fs.existsSync(file)) { watcher.close(); clearTimeout(deadline); resolve(); } });
  const deadline = setTimeout(() => { watcher.close(); reject(new Error('Fixture barrier did not open: '+file)); }, 15000);
  if(fs.existsSync(file)) { watcher.close(); clearTimeout(deadline); resolve(); }
});
if(action === 'cargo_a') {
  fs.mkdirSync('cargo-lock'); fs.writeFileSync('ready', '');
  await waitFor('release'); fs.rmdirSync('cargo-lock'); fs.writeFileSync('done-a', '');
} else if(action === 'guard') {
  await waitFor('ready'); assert(!fs.existsSync('done-b')); fs.writeFileSync('release', '');
} else if(action === 'cargo_b') {
  assert(fs.existsSync('done-a')); fs.mkdirSync('cargo-lock'); fs.rmdirSync('cargo-lock'); fs.writeFileSync('done-b', '');
} else if(action === 'dependent') {
  assert(fs.existsSync('done-a')); console.error('stderr is evidence'); console.log('dependent ran');
} else if(action === 'failure') {
  console.error('intentional failure'); process.exitCode = 7;
} else if(action === 'blocked') {
  fs.writeFileSync('must-not-run', '');
} else if(action === 'host') {
  assert(!fs.existsSync('cargo-lock')); fs.writeFileSync('host-ran', '');
} else if(action === 'browser_a') {
  fs.mkdirSync('browser-lock'); fs.writeFileSync('browser-ready', '');
  await waitFor('browser-release'); fs.rmdirSync('browser-lock'); fs.writeFileSync('browser-done', '');
} else if(action === 'browser_b') {
  assert(fs.existsSync('browser-done')); fs.mkdirSync('browser-lock'); fs.rmdirSync('browser-lock');
} else if(action === 'browser_guard') {
  await waitFor('browser-ready'); assert(!fs.existsSync('browser-done')); fs.writeFileSync('browser-release', '');
} else { fs.writeFileSync('independent-ran', ''); }
`);
  const make = (key, resources = [], depends_on = []) => ({ key, program: process.execPath, args: [fixture, key], resources, depends_on });
  const checks = [make("cargo_a", ["cargo"]), make("cargo_b", ["cargo"]), make("guard", ["browser"]), make("dependent", [], ["cargo_a"]), make("failure"), make("blocked", [], ["failure"]), make("host", ["host"]), make("independent")];
  const directory = path.join(root, "dist/verification-runs/fixture");
  const active = new Set();
  const report = line => {
    if (line.startsWith("==> ")) {
      const key = line.slice(4);
      assert(active.size < 3);
      if (key === "host") assert.equal(active.size, 0, "host measurement starts alone");
      else assert(!active.has("host"), "nothing starts during a host measurement");
      active.add(key);
    } else active.delete(line.split(" ")[1]);
  };
  const result = await runChecks({ checks, root, directory, runId: "fixture", temporaryRoot: root, workers: 3, browserWorkers: 1, report });
  assert.equal(result.all_pass, false);
  for (const key of ["cargo_a", "cargo_b", "guard", "dependent", "host", "independent"]) assert.equal(result.checks[key].status, "pass", key);
  assert.equal(result.checks.failure.exit_code, 7);
  assert.match(result.checks.blocked.error, /Dependency did not pass/);
  assert(!fs.existsSync(path.join(root, "must-not-run")));
  assert(fs.existsSync(path.join(root, "host-ran")));
  assert(fs.existsSync(path.join(root, "independent-ran")));
  for (const [key, check] of Object.entries(result.checks)) {
    const log = fs.readFileSync(path.join(directory, `${key}.log`));
    assert.equal(createHash("sha256").update(log).digest("hex"), check.log_sha256);
  }
  assert.match(fs.readFileSync(path.join(directory, "dependent.log"), "utf8"), /stderr is evidence/);
  const browser = await runChecks({ checks: [make("browser_a", ["browser"]), make("browser_b", ["browser"]), make("browser_guard")], root, directory, runId: "fixture", temporaryRoot: root, workers: 3, browserWorkers: 1, report: () => {} });
  assert.equal(browser.all_pass, true, "browser capacity is independent of the total worker budget");
  const ci = await runChecks({ checks: [{ ...make("ci"), ci_args: [fixture, "dependent"] }], root, directory, runId: "fixture", temporaryRoot: root, ci: true, report: () => {} });
  assert.equal(ci.all_pass, true);
  assert.deepEqual(ci.checks.ci.command, [process.execPath, fixture, "dependent"]);
  console.log("Verification scheduler passed: real process overlap, Cargo exclusion, dependencies, host isolation, failure propagation, CI commands and log hashes");
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
