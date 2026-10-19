#!/usr/bin/env node
// R-309 B2:verify 路径裁剪与 release evidence 共同使用的机械策略。
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { createHash } from "node:crypto";
import { fileURLToPath, pathToFileURL } from "node:url";

export const VERIFY_CHECKS = JSON.parse(fs.readFileSync(new URL("./verification-checks.json", import.meta.url), "utf8"));
export const VERIFY_STEP_KEYS = VERIFY_CHECKS.map(({ key }) => key);
export function validateCheckRegistry(checks) {
  if (!Array.isArray(checks)) throw new Error("Verification registry must be an array");
  const seen = new Set();
  for (const check of checks) {
    if (!/^[a-z][a-z0-9_]*$/.test(check.key) || seen.has(check.key)) throw new Error(`Invalid or duplicate verification key: ${check.key}`);
    if (!["always", "rust", "frontend", "diagram"].includes(check.inputs)) throw new Error(`Unknown inputs for ${check.key}`);
    if (typeof check.program !== "string" || !check.program.trim() || !Array.isArray(check.args) || !check.args.every(arg => typeof arg === "string") || (check.ci_args !== undefined && (!Array.isArray(check.ci_args) || !check.ci_args.every(arg => typeof arg === "string")))) throw new Error(`Invalid command for ${check.key}`);
    for (const dependency of check.depends_on ?? []) if (!seen.has(dependency)) throw new Error(`Dependency must precede ${check.key}: ${dependency}`);
    seen.add(check.key);
  }
}
validateCheckRegistry(VERIFY_CHECKS);
// Shared by verification and packaging. Build configuration and test fixtures are
// inputs too; project runtime data and generated output are deliberately outside.
export const BUILD_INPUT_PATHS = [
  "crates", "scripts", "ui", ".github", ".cargo", "docs", "README.md",
  "Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain", "rust-toolchain.toml",
  "package.json", "package-lock.json", "npm-shrinkwrap.json", ".npmrc",
  "eslint.config.js", ".gitattributes",
];

export function assertCleanBuildInputs(root, expectedCommit) {
  const git = (...args) => execFileSync("git", ["-C", root, ...args], { encoding: "utf8" }).trim();
  const commit = git("rev-parse", "HEAD");
  if (expectedCommit && commit !== expectedCommit) throw new Error(`HEAD changed: ${expectedCommit} -> ${commit}`);
  const dirty = [
    git("diff", "--name-only", "HEAD", "--", ...BUILD_INPUT_PATHS),
    git("ls-files", "--others", "--exclude-standard", "--", ...BUILD_INPUT_PATHS),
  ].filter(Boolean).join("\n");
  if (dirty) throw new Error(`构建输入未提交，产物无法绑定 HEAD:\n${dirty}`);
  return { commit, tree: git("rev-parse", "HEAD^{tree}") };
}

function normalizePath(value) {
  return String(value).replaceAll("\\", "/").replace(/^\.\//, "");
}

function isRustPath(path) {
  return (
    path.endsWith(".rs") ||
    path === "Cargo.toml" ||
    path === "Cargo.lock" ||
    path.endsWith("/Cargo.toml") ||
    path.endsWith("/Cargo.lock")
  );
}

// UI2-0926 #7:架构图浏览器门禁(scripts/ui-diagram-smoke.mjs)真渲染 docs 下全部 mermaid 与 crate 图 golden。
// 改文档里的图、改 crate 图生成器或它的 golden、改门禁本身,都要跑它;前端改动一律带上(渲染器与样式都在 ui/)。
function isDiagramPath(path) {
  return (
    (path.startsWith("docs/") && path.endsWith(".md")) ||
    path.startsWith("crates/kanzei-tools/src/arch_diagram") ||
    path.startsWith("crates/kanzei-tools/tests/fixtures/arch_diagram/") ||
    path === "scripts/ui-diagram-smoke.mjs"
  );
}

// docs/architecture 的图由 Rust 侧 lint(arch_diagram_lint.rs,repo_default_diagrams_have_no_lint_errors 读真实文件)
// 把关 D1–D9:只改图也要跑 Rust 测试,否则只改文档时引入的 lint error 要等到后面某个无关的 Rust 提交才暴露。
function isArchitectureDiagramDoc(path) {
  return path.startsWith("docs/architecture/");
}

function isFrontendPath(path) {
  return (
    path.startsWith("crates/kanzei-app/ui/") ||
    path.startsWith("crates/kanzei-app/mobile-pwa/") ||
    path === "eslint.config.js" ||
    path === "package.json" ||
    path === "package-lock.json" ||
    path === "npm-shrinkwrap.json" ||
    path === ".npmrc" ||
    path === "crates/kanzei-app/index.html" ||
    path === "crates/kanzei-app/style.css" ||
    path.startsWith("scripts/ui-") ||
    path === "scripts/parallel-lines-regression.mjs"
  );
}

export function classifyChangedPaths(paths, { full = false } = {}) {
  const changedPaths = [...new Set((paths ?? []).map(normalizePath).filter(Boolean))].sort();
  const policyChanged = changedPaths.some((path) => path.startsWith("scripts/verify") || path === "scripts/verification-checks.json" || path === ".github/workflows/ci.yml");
  const hasRust = full || policyChanged || changedPaths.some((path) => isRustPath(path) || isArchitectureDiagramDoc(path) || path.startsWith(".cargo/") || path.startsWith("rust-toolchain"));
  const hasFrontend = full || policyChanged || changedPaths.some(isFrontendPath);
  const hasDiagram = hasFrontend || changedPaths.some(isDiagramPath);
  const selectedInputs = { always: true, rust: hasRust, frontend: hasFrontend, diagram: hasDiagram };
  const skippedSteps = VERIFY_CHECKS.filter(({ inputs }) => !selectedInputs[inputs]).map(({ key }) => key);
  return {
    mode: full ? "full" : "targeted",
    full_verify: full,
    changed_paths: changedPaths,
    run_rust: hasRust,
    run_frontend: hasFrontend,
    run_diagram: hasDiagram,
    skipped_steps: skippedSteps,
    checks: VERIFY_CHECKS.filter(({ key }) => !skippedSteps.includes(key)),
  };
}

export function validateFullVerification(evidence, expectedCommit) {
  if (!evidence || typeof evidence !== "object") return "verification.json 不是对象";
  if (evidence.commit !== expectedCommit) {
    return `验证证据绑定 ${evidence.commit ?? "<missing>"},HEAD 是 ${expectedCommit}`;
  }
  if (evidence.schema_version !== 3) return "验证证据 schema_version 必须为 3，请重新验证";
  if (!["local", "ci"].includes(evidence.execution_environment)) return "验证证据缺少 execution_environment";
  if (evidence.all_pass !== true) return "验证证据未全绿";
  if (evidence.full_verify !== true || evidence.mode !== "full") {
    return "验证证据不是 full verify；targeted/cropped evidence 不得打包";
  }
  if (!Array.isArray(evidence.skipped_steps) || evidence.skipped_steps.length !== 0) {
    return "全量验证证据的 skipped_steps 必须为空";
  }
  const checks = evidence.checks;
  if (!checks || typeof checks !== "object" || Array.isArray(checks)) return "验证证据缺少 checks";
  const missing = VERIFY_STEP_KEYS.filter((key) => !(key in checks));
  if (missing.length) return `全量验证证据缺少步骤: ${missing.join(", ")}`;
  const extra = Object.keys(checks).filter(key => !VERIFY_STEP_KEYS.includes(key));
  if (extra.length) return `验证证据包含未注册步骤: ${extra.join(", ")}`;
  for (const [key, result] of Object.entries(checks)) {
    if (!result || typeof result !== "object" || result.status !== "pass" || result.exit_code !== 0 || !Number.isFinite(result.elapsed_seconds) || result.elapsed_seconds < 0) {
      return `验证步骤 ${key} 没有有效的通过结果`;
    }
    const check = VERIFY_CHECKS.find(check => check.key === key);
    const args = evidence.execution_environment === "ci" ? (check.ci_args ?? check.args) : check.args;
    if (JSON.stringify(result.command) !== JSON.stringify([check.program, ...args])) return `验证步骤 ${key} 的执行命令与清单不一致`;
    if (!new RegExp(`^dist/verification-runs/[A-Za-z0-9][A-Za-z0-9_-]*/${key}\\.log$`).test(result.log_path) || !/^[0-9a-f]{64}$/.test(result.log_sha256)) return `验证步骤 ${key} 缺少独立日志证据`;
  }
  return null;
}

export function validateVerificationFiles(evidence, root) {
  if (evidence.lifecycle_run_id) {
    const common = execFileSync("git", ["-C", root, "rev-parse", "--path-format=absolute", "--git-common-dir"], { encoding: "utf8" }).trim();
    const project = fs.realpathSync.native(path.dirname(common));
    if (evidence.artifact_project_root !== project) return "验证证据的项目归属不一致";
    const markerPath = path.join(project, "dist", "verification-runs", evidence.lifecycle_run_id, ".kanzei-artifact-owner.json");
    if (!fs.existsSync(markerPath)) return "验证证据缺少任务所有权登记";
    const marker = JSON.parse(fs.readFileSync(markerPath, "utf8"));
    if (marker.project !== project || marker.run_id !== evidence.lifecycle_run_id || marker.type !== "evidence") return "验证证据任务所有权不一致";
    if (Object.values(evidence.checks).some(check => !check.log_path.startsWith(`dist/verification-runs/${evidence.lifecycle_run_id}/`))) return "验证日志没有绑定同一次任务";
    root = project;
  }
  for (const [key, result] of Object.entries(evidence.checks)) {
    const log = path.resolve(root, result.log_path);
    if (!fs.existsSync(log)) return `验证步骤 ${key} 的日志不存在`;
    const digest = createHash("sha256").update(fs.readFileSync(log)).digest("hex");
    if (digest !== result.log_sha256) return `验证步骤 ${key} 的日志已改变`;
  }
  return null;
}

function readJson(file) {
  // 剥 BOM:证据可能由不同 PowerShell 宿主写出(5.1 的 Set-Content -Encoding UTF8
  // 带 BOM,pwsh 7 不带)。JSON.parse 遇 BOM 直接语法错,报错完全看不出根因。
  return JSON.parse(fs.readFileSync(file, "utf8").replace(/^\uFEFF/, ""));
}

const isEntry = process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;
const [command, arg1, arg2] = isEntry ? process.argv.slice(2) : [];
if (command === "classify") {
  const paths = readJson(arg1);
  const full = process.env.KANZEI_VERIFY_FULL === "1";
  console.log(JSON.stringify(classifyChangedPaths(Array.isArray(paths) ? paths : [paths], { full })));
} else if (command === "validate") {
  const evidence = readJson(arg1);
  const error = validateFullVerification(evidence, arg2) ?? validateVerificationFiles(evidence, fileURLToPath(new URL("../", import.meta.url)));
  if (error) {
    console.error(error);
    process.exit(1);
  }
  console.log("full verification evidence accepted");
} else if (command === "assert-clean") {
  console.log(JSON.stringify(assertCleanBuildInputs(arg1, arg2)));
}
