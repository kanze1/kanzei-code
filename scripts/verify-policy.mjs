#!/usr/bin/env node
// R-309 B2:verify 路径裁剪与 release evidence 共同使用的机械策略。
import fs from "node:fs";

export const VERIFY_STEP_KEYS = [
  "parallel_lines_regression",
  "ui_a11y",
  "ui_i18n",
  "ui_markdown",
  "crate_sync",
  "metrics_build",
  "ps1_bom",
  "ui_lint",
  "ipc_event_contract",
  "fmt",
  "clippy",
  "ui_connectivity",
  "ui_runtime",
  "ui_diagram",
  "test",
];

const RUST_STEPS = ["fmt", "clippy", "test"];
const FRONTEND_STEPS = [
  "parallel_lines_regression",
  "ui_a11y",
  "ui_i18n",
  "ui_markdown",
  "ui_lint",
  "ui_runtime",
];

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
    path === "crates/kanzei-app/index.html" ||
    path === "crates/kanzei-app/style.css" ||
    path.startsWith("scripts/ui-") ||
    path === "scripts/parallel-lines-regression.mjs"
  );
}

export function classifyChangedPaths(paths, { full = false } = {}) {
  const changedPaths = [...new Set((paths ?? []).map(normalizePath).filter(Boolean))].sort();
  const hasRust = full || changedPaths.some((path) => isRustPath(path) || isArchitectureDiagramDoc(path));
  const hasFrontend = full || changedPaths.some(isFrontendPath);
  const hasDiagram = hasFrontend || changedPaths.some(isDiagramPath);
  const skippedSteps = [
    ...(hasRust ? [] : RUST_STEPS),
    ...(hasFrontend ? [] : FRONTEND_STEPS),
    ...(hasDiagram ? [] : ["ui_diagram"]),
  ];
  return {
    mode: full ? "full" : "targeted",
    full_verify: full,
    changed_paths: changedPaths,
    run_rust: hasRust,
    run_frontend: hasFrontend,
    run_diagram: hasDiagram,
    skipped_steps: skippedSteps,
  };
}

export function validateFullVerification(evidence, expectedCommit) {
  if (!evidence || typeof evidence !== "object") return "verification.json 不是对象";
  if (evidence.commit !== expectedCommit) {
    return `验证证据绑定 ${evidence.commit ?? "<missing>"},HEAD 是 ${expectedCommit}`;
  }
  if (evidence.all_pass !== true) return "验证证据未全绿";
  if (evidence.full_verify !== true || evidence.mode !== "full") {
    return "验证证据不是 full verify；targeted/cropped evidence 不得打包";
  }
  if (!Array.isArray(evidence.skipped_steps) || evidence.skipped_steps.length !== 0) {
    return "全量验证证据的 skipped_steps 必须为空";
  }
  const checks = evidence.checks;
  if (!checks || typeof checks !== "object") return "验证证据缺少 checks";
  const missing = VERIFY_STEP_KEYS.filter((key) => !(key in checks));
  if (missing.length) return `全量验证证据缺少步骤: ${missing.join(", ")}`;
  return null;
}

function readJson(file) {
  // 剥 BOM:证据可能由不同 PowerShell 宿主写出(5.1 的 Set-Content -Encoding UTF8
  // 带 BOM,pwsh 7 不带)。JSON.parse 遇 BOM 直接语法错,报错完全看不出根因。
  return JSON.parse(fs.readFileSync(file, "utf8").replace(/^\uFEFF/, ""));
}

const [command, arg1, arg2] = process.argv.slice(2);
if (command === "classify") {
  const paths = readJson(arg1);
  const full = process.env.KANZEI_VERIFY_FULL === "1";
  console.log(JSON.stringify(classifyChangedPaths(Array.isArray(paths) ? paths : [paths], { full })));
} else if (command === "validate") {
  const error = validateFullVerification(readJson(arg1), arg2);
  if (error) {
    console.error(error);
    process.exit(1);
  }
  console.log("full verification evidence accepted");
}
