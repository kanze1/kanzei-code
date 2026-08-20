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
]);

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
