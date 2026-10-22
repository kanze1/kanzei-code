#!/usr/bin/env node
// R-309 B2:路径裁剪与 full verification evidence 门禁定向测试。
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { VERIFY_CHECKS, VERIFY_STEP_KEYS, assertCleanBuildInputs, inspectBuildInputs, classifyChangedPaths, validateCheckRegistry, validateFullVerification, validateVerificationFiles } from "./verify-policy.mjs";

const frontendOnly = classifyChangedPaths(["crates/kanzei-app/ui/01-core.js"]);
assert.equal(frontendOnly.run_frontend, true);
assert.equal(frontendOnly.run_rust, false);
assert(VERIFY_CHECKS.filter(check => check.inputs === "frontend").every(check => !frontendOnly.skipped_steps.includes(check.key)), "shared runtime changes select every frontend suite");
assert(frontendOnly.skipped_steps.includes("metrics_build"));
assert(frontendOnly.skipped_steps.includes("artifact_lifecycle"));

const selectedKeys = file => classifyChangedPaths([file]).checks.map(check => check.key);
const sharedGeometry = selectedKeys("crates/kanzei-app/fixtures/preview-geometry.json");
assert(sharedGeometry.includes("test"), "shared geometry fixtures must run native Rust tests");
assert(sharedGeometry.includes("ui_runtime"), "shared geometry fixtures must run the frontend consumer");
const contextStyle = selectedKeys("crates/kanzei-app/ui/context.css");
assert(contextStyle.includes("context_inspection"));
for (const unrelated of ["ui_backdrop", "ui_surfaces", "ui_math_browser", "artifact_lifecycle", "metrics_build", "ipc_signatures", "verify_policy"]) assert(!contextStyle.includes(unrelated), `context styles do not trigger ${unrelated}`);
assert(selectedKeys("crates/kanzei-app/ui/04-math.js").includes("ui_math_browser"));
assert(selectedKeys("scripts/ui-math-browser-smoke.mjs").includes("ui_math_browser"), "editing a test always selects itself");
for (const file of ["resource", "markdown", "i18n", "tool-summary", "structured-render", "delivery", "side-policy", "experience"]) {
  assert(selectedKeys("scripts/tests/browser-contracts/" + file + ".mjs").includes("ui_browser_contracts"), "each browser domain selects its containing suite");
}
for (const file of ["state.test", "session-identity.test", "surface.test", "permission.test", "run-control.test", "schedules.test", "schedule-notification.test", "schedules-fixture"]) {
  assert(selectedKeys("scripts/tests/ownership/" + file + ".mjs").includes("ui_state_ownership"), "each state domain and shared fixture selects its containing suite");
}
assert(selectedKeys("scripts/tests/verification/package-tags.mjs").includes("verify_policy"));
assert(selectedKeys("scripts/package.ps1").includes("verify_policy"));
assert(selectedKeys("scripts/tests/verification/metrics-gate.mjs").includes("metrics_regression"));
const stateOwnership = VERIFY_CHECKS.find(check => check.key === "ui_state_ownership");
assert(stateOwnership.args.includes("scripts/ui-state-ownership-smoke.mjs"), "state ownership key must execute state ownership tests");
assert(VERIFY_CHECKS.find(check => check.key === "ui_design_audit_repairs").args.includes("scripts/ui-design-audit-repairs-smoke.mjs"), "browser audit remains independently covered");
assert(selectedKeys("scripts/metrics-regression-gate.ps1").includes("metrics_build"), "selected checks bring their producer dependencies");
assert(!selectedKeys("docs/design/memory_system.md").includes("ipc_signatures"));

const rustOnly = classifyChangedPaths(["crates/kanzei-core/src/lib.rs"]);
assert.equal(rustOnly.run_frontend, false);
assert.equal(rustOnly.run_rust, true);
assert(rustOnly.checks.some(check => check.key === "test"));
assert(rustOnly.skipped_steps.includes("ui_runtime"));
for (const input of ["package.json", "package-lock.json", "npm-shrinkwrap.json", ".npmrc"]) {
  assert.equal(classifyChangedPaths([input]).run_frontend, true, `${input} changes frontend execution`);
}
const seen = new Set();
for (const check of VERIFY_CHECKS) {
  assert(!seen.has(check.key), `duplicate ${check.key}`);
  for (const dependency of check.depends_on ?? []) assert(seen.has(dependency), `dependency must precede ${check.key}`);
  assert(["always", "frontend", "rust", "diagram"].includes(check.inputs));
  for (const arg of check.args.filter(arg => arg.startsWith("scripts/"))) assert(fs.existsSync(new URL(`../${arg}`, import.meta.url)));
  seen.add(check.key);
}
assert.throws(() => validateCheckRegistry([VERIFY_CHECKS[0], VERIFY_CHECKS[0]]), /duplicate/);
assert.throws(() => validateCheckRegistry([{ ...VERIFY_CHECKS[0], depends_on: ["future"] }]), /precede/);
const runtimeSource = fs.readFileSync(new URL("./ui-runtime-smoke.mjs", import.meta.url), "utf8");
assert(!/await import\(["']\.\/ui-[^"']*(?:smoke|regressions)\.mjs["']\)/.test(runtimeSource), "runtime gate must not hide independent verification suites");
for (const environment of ["local", "ci"]) {
  const commands = VERIFY_CHECKS.map(check => JSON.stringify([check.program, ...(environment === "ci" ? check.ci_args ?? check.args : check.args)]));
  assert.equal(new Set(commands).size, commands.length, `${environment} must not register the same command twice`);
}

// ── 分区:架构图 ──(UI2-0926 #7:ui_diagram 步的路径触发)
// 只改一般设计文档:只跑图门禁(文档里的图只判能渲染、安全、点击映射),Rust 与其余前端步都跳过。
const docsOnly = classifyChangedPaths(["docs/design/memory_system.md"]);
assert.equal(docsOnly.run_diagram, true);
assert.equal(docsOnly.run_frontend, false);
assert.equal(docsOnly.run_rust, false);
assert.ok(!docsOnly.skipped_steps.includes("ui_diagram"), "改 docs 下的 markdown 必须跑 ui_diagram");
assert.ok(docsOnly.skipped_steps.includes("ui_runtime") && docsOnly.skipped_steps.includes("test"));
// 复核修复:只改 docs/architecture 的图也要跑 Rust 测试(D1–D9 lint 在 Rust 侧读真实文件),
// 否则只改文档时引入的 lint error 要等到后面某个无关的 Rust 提交才暴露。
const archDoc = classifyChangedPaths(["docs/architecture/01_runtime_loop.md"]);
assert.equal(archDoc.run_diagram, true);
assert.equal(archDoc.run_rust, true, "改 docs/architecture 的图必须跑 Rust 测试(lint)");
assert.ok(!archDoc.skipped_steps.includes("test") && !archDoc.skipped_steps.includes("ui_diagram"));
assert.equal(archDoc.run_frontend, false);
// lint 拆到 arch_diagram_lint.rs 之后同样触发图门禁。
assert.equal(classifyChangedPaths(["crates/kanzei-tools/src/arch_diagram_lint.rs"]).run_diagram, true);
// 改 ui/*.js:全部前端步连同图门禁一起跑(渲染器、样式都在 ui/)。
assert.equal(frontendOnly.run_diagram, true);
assert.ok(!frontendOnly.skipped_steps.includes("ui_diagram"));
// 只改一般 Rust 源码:不跑图门禁;改的是 crate 图生成器(或它的 golden)才跑。
assert.equal(rustOnly.run_diagram, false);
const generator = classifyChangedPaths(["crates/kanzei-tools/src/arch_diagram.rs"]);
assert.equal(generator.run_rust, true);
assert.equal(generator.run_diagram, true);
assert.ok(!generator.skipped_steps.includes("ui_diagram"));
const golden = classifyChangedPaths(["crates/kanzei-tools/tests/fixtures/arch_diagram/crates_full.mmd"]);
assert.equal(golden.run_diagram, true);
// 非 markdown 的 docs 资产与仓库根 README 不触发。
assert.equal(classifyChangedPaths(["docs/assets/x.png", "README.md"]).run_diagram, false);
assert.ok(VERIFY_STEP_KEYS.includes("ui_diagram"), "全量证据必须含 ui_diagram");
// ── 分区:架构图 结束 ──

const full = classifyChangedPaths(["README.md"], { full: true });
assert.equal(full.mode, "full");
assert.equal(full.full_verify, true);
assert.equal(full.skipped_steps.length, 0);

const checks = Object.fromEntries(VERIFY_CHECKS.map(({ key, program, args }) => [key, { status: "pass", exit_code: 0, elapsed_seconds: 0.1, command: [program, ...args], log_path: `dist/verification-runs/fixture/${key}.log`, log_sha256: createHash("sha256").update("").digest("hex") }]));
const evidence = {
  schema_version: 3,
  execution_environment: "local",
  commit: "abc123",
  all_pass: true,
  mode: "full",
  full_verify: true,
  skipped_steps: [],
  checks,
};
assert.equal(validateFullVerification(evidence, "abc123"), null);
assert.match(
  validateFullVerification({ ...evidence, full_verify: false, mode: "targeted" }, "abc123"),
  /targeted\/cropped/,
);
assert.match(
  validateFullVerification({ ...evidence, skipped_steps: ["test"] }, "abc123"),
  /skipped_steps/,
);
assert.match(validateFullVerification(evidence, "different"), /HEAD 是 different/);
for (const invalid of [null, "", "pass 0.1s", "fail 0.1s", {}, { status: "pass", exit_code: 1, elapsed_seconds: 0 }, { status: "pass", exit_code: 0, elapsed_seconds: -1 }, { status: "fail", exit_code: 0, elapsed_seconds: 0 }]) {
  assert.match(validateFullVerification({ ...evidence, checks: { ...checks, test: invalid } }, "abc123"), /test/);
}
assert.match(validateFullVerification({ ...evidence, schema_version: 1 }, "abc123"), /schema_version/);
const missingChild = structuredClone(evidence);
delete missingChild.checks.ui_browser_contracts;
assert.match(validateFullVerification(missingChild, "abc123"), /缺少步骤/);
assert.match(validateFullVerification({ ...evidence, checks: { ...checks, test: { ...checks.test, command: ["node", "irrelevant.mjs"] } } }, "abc123"), /执行命令/);
assert.match(validateFullVerification({ ...evidence, checks: { ...checks, test: { ...checks.test, log_path: "../outside.log" } } }, "abc123"), /日志证据/);
// Actual git repositories exercise tracked dependency edits, untracked build
// configuration, and HEAD movement, rather than comparing path literals.
const temp = fs.mkdtempSync(path.join(os.tmpdir(), "kanzei-verify-policy-"));
try {
  const git = (...args) => execFileSync("git", ["-C", temp, ...args], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  git("init", "-q");
  fs.writeFileSync(path.join(temp, "Cargo.lock"), "baseline\n");
  git("add", ".");
  git("-c", "user.name=Verification Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture");
  const baseline = assertCleanBuildInputs(temp);
  fs.appendFileSync(path.join(temp, "Cargo.lock"), "dependency edit\n");
  assert.throws(() => assertCleanBuildInputs(temp, baseline.commit), /Cargo.lock/);
  const dirty = inspectBuildInputs(temp);
  assert.deepEqual(dirty.dirty_paths, ["Cargo.lock"]);
  inspectBuildInputs(temp, dirty);
  fs.appendFileSync(path.join(temp, "Cargo.lock"), "another edit\n");
  assert.throws(() => inspectBuildInputs(temp, dirty), /changed during verification/);
  fs.writeFileSync(path.join(temp, "Cargo.lock"), "baseline\n");
  // D-766:需求/缺陷/架构索引是构建输入。未提交的项目资产必须挡住发版门禁,
  // 否则回执绑定的那棵树过不了它自己声明的时效判据。
  fs.mkdirSync(path.join(temp, ".kanzei", "project", "architecture"), { recursive: true });
  fs.writeFileSync(path.join(temp, ".kanzei", "project", "architecture", "README.md"), "index row\n");
  assert.throws(() => assertCleanBuildInputs(temp), /\.kanzei\/project\/architecture/);
  assert.deepEqual(inspectBuildInputs(temp).dirty_paths, [".kanzei/project/architecture/README.md"]);
  fs.rmSync(path.join(temp, ".kanzei"), { recursive: true, force: true });
  fs.mkdirSync(path.join(temp, ".cargo"));
  fs.writeFileSync(path.join(temp, ".cargo/config.toml"), "[build]\njobs=1\n");
  assert.throws(() => assertCleanBuildInputs(temp), /config.toml/);
  git("add", ".");
  git("-c", "user.name=Verification Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "config");
  assert.throws(() => assertCleanBuildInputs(temp, baseline.commit), /HEAD changed/);
  assertCleanBuildInputs(temp);
  for (const result of Object.values(checks)) {
    const log = path.join(temp, result.log_path);
    fs.mkdirSync(path.dirname(log), { recursive: true });
    fs.writeFileSync(log, "");
  }
  assert.equal(validateVerificationFiles(evidence, temp), null);
  fs.writeFileSync(path.join(temp, checks.test.log_path), "tampered");
  assert.match(validateVerificationFiles(evidence, temp), /日志已改变/);
  fs.unlinkSync(path.join(temp, checks.test.log_path));
  assert.match(validateVerificationFiles(evidence, temp), /日志不存在/);
  // Execute the actual PowerShell runner in an isolated repository. Native
  // stderr is evidence, while exit codes determine results. A failed check
  // must retain its evidence and must not prevent independent checks running.
  if (process.platform === "win32") {
    await import("./tests/verification/package-tags.mjs");
    const scripts = path.join(temp, "scripts");
    fs.mkdirSync(scripts);
    for (const name of ["verify.ps1", "verify-policy.mjs", "verify-runner.mjs", "artifact-lifecycle.mjs", "artifact-lifecycle.ps1"]) {
      fs.copyFileSync(new URL(`./${name}`, import.meta.url), path.join(scripts, name));
    }
    const fixtureChecks = [
      { key: "native_stderr", inputs: "always", program: "node", args: ["scripts/native-stderr.mjs"], ci_args: ["scripts/native-stderr.mjs", "--ci-fixture"] },
      { key: "native_failure", inputs: "always", program: "node", args: ["scripts/native-failure.mjs"] },
      { key: "after_failure", inputs: "always", program: "node", args: ["scripts/after-failure.mjs"] },
    ];
    fs.writeFileSync(path.join(scripts, "verification-checks.json"), JSON.stringify(fixtureChecks));
    fs.writeFileSync(path.join(scripts, "native-stderr.mjs"), 'console.log("stdout evidence"); console.error("stderr evidence");');
    fs.writeFileSync(path.join(scripts, "native-failure.mjs"), 'console.error("intentional fixture failure"); process.exitCode = 7;');
    fs.writeFileSync(path.join(scripts, "after-failure.mjs"), 'console.log("independent check still ran");');
    git("add", "scripts");
    git("-c", "user.name=Verification Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "runner fixture");
    const run = (environment = {}, mode = "-Full") => execFileSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(scripts, "verify.ps1"), mode], {
      cwd: temp, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, KANZEI_VERIFY_BASE: "HEAD", KANZEI_VERIFY_FULL: "", KANZEI_FIXTURE_EXIT: "0", ...environment },
    });
    let failedOutput = "";
    assert.throws(run, error => {
      failedOutput = `${error.stdout}\n${error.stderr}`;
      return error.status !== 0;
    });
    assert.equal(fs.existsSync(path.join(temp, "dist/verification.json")), false);
    const runs = path.join(temp, "dist/verification-runs");
    const reports = () => fs.readdirSync(runs)
      .map(name => path.join(runs, name, "results.json"))
      .filter(file => fs.existsSync(file))
      .map(file => JSON.parse(fs.readFileSync(file, "utf8")));
    const failed = reports().find(report => report.all_pass === false);
    assert(failed, `failed execution retains its complete result report:\n${failedOutput}`);
    assert.equal(failed.checks.native_stderr.status, "pass");
    assert.equal(failed.checks.native_failure.exit_code, 7);
    assert.equal(failed.checks.after_failure.status, "pass");
    assert.match(fs.readFileSync(path.join(temp, failed.checks.native_stderr.log_path), "utf8"), /stdout evidence[\s\S]*stderr evidence|stderr evidence[\s\S]*stdout evidence/);
    for (const result of Object.values(failed.checks)) {
      assert(result.elapsed_seconds >= 0);
      assert.equal(createHash("sha256").update(fs.readFileSync(path.join(temp, result.log_path))).digest("hex"), result.log_sha256);
    }
    fs.writeFileSync(path.join(scripts, "native-failure.mjs"), 'console.log("fixture repaired"); process.exitCode = Number(process.env.KANZEI_FIXTURE_EXIT);');
    git("add", "scripts/native-failure.mjs");
    git("-c", "user.name=Verification Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "repair fixture");
    run();
    const runnerEvidence = JSON.parse(fs.readFileSync(path.join(temp, "dist/verification.json"), "utf8"));
    assert.equal(runnerEvidence.schema_version, 3);
    assert.equal(runnerEvidence.all_pass, true);
    assert.equal(reports().length, 2);
    execFileSync(process.execPath, [path.join(scripts, "verify-policy.mjs"), "validate", path.join(temp, "dist/verification.json"), git("rev-parse", "HEAD")], { cwd: temp, stdio: "pipe" });
    run({}, "-Ci");
    const ciEvidence = JSON.parse(fs.readFileSync(path.join(temp, "dist/verification.json"), "utf8"));
    assert.equal(ciEvidence.execution_environment, "ci");
    assert.deepEqual(ciEvidence.checks.native_stderr.command, ["node", ...fixtureChecks[0].ci_args]);
    execFileSync(process.execPath, [path.join(scripts, "verify-policy.mjs"), "validate", path.join(temp, "dist/verification.json"), git("rev-parse", "HEAD")], { cwd: temp, stdio: "pipe" });
    assert.throws(() => run({ KANZEI_FIXTURE_EXIT: "7" }));
    assert.equal(fs.existsSync(path.join(temp, "dist/verification.json")), false, "failure at the same HEAD invalidates a previous successful receipt");
    assert.equal(reports().length, 4);
    fs.appendFileSync(path.join(scripts, "after-failure.mjs"), '\nconsole.log("uncommitted fix");');
    run({}, "");
    const targeted = JSON.parse(fs.readFileSync(path.join(temp, "dist/verification-targeted.json"), "utf8"));
    assert.equal(targeted.mode, "targeted");
    assert.equal(targeted.full_verify, false);
    assert(targeted.changed_paths.includes("scripts/after-failure.mjs"));
    assert.equal(targeted.worktree_sha256, inspectBuildInputs(temp).worktree_sha256);
    assert.match(validateFullVerification(targeted, git("rev-parse", "HEAD")), /targeted\/cropped/);
  }
} finally {
  fs.rmSync(temp, { recursive: true, force: true });
}
console.log("R-309 B2 verify policy 定向测试通过");
