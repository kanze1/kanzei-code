#!/usr/bin/env node
// R-309 B2:路径裁剪与 full verification evidence 门禁定向测试。
import assert from "node:assert/strict";
import { VERIFY_STEP_KEYS, classifyChangedPaths, validateFullVerification } from "./verify-policy.mjs";

const frontendOnly = classifyChangedPaths(["crates/kanzei-app/ui/01-core.js"]);
assert.equal(frontendOnly.run_frontend, true);
assert.equal(frontendOnly.run_rust, false);
assert.deepEqual(frontendOnly.skipped_steps, ["fmt", "clippy", "test"]);

const rustOnly = classifyChangedPaths(["crates/kanzei-core/src/lib.rs"]);
assert.equal(rustOnly.run_frontend, false);
assert.equal(rustOnly.run_rust, true);
assert.deepEqual(rustOnly.skipped_steps, [
  "parallel_lines_regression",
  "ui_a11y",
  "ui_i18n",
  "ui_markdown",
  "ui_lint",
  "ui_runtime",
  "ui_diagram",
]);

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

const checks = Object.fromEntries(VERIFY_STEP_KEYS.map((key) => [key, "pass 0.1s"]));
const evidence = {
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
console.log("R-309 B2 verify policy 定向测试通过");
