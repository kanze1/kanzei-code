// Actual pure production ESM, plus read-only Git output; no UI/parser reimplementation.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const before = process.argv.includes("--before");
const output = path.resolve(process.env.KANZEI_STRUCTURED_OUTPUT || "output/audit-WB4/structured/contracts");
await mkdir(output, { recursive: true });
const source = before ? execFileSync("git", ["show", "26acb7f0:crates/kanzei-app/ui/04-structured-parse.js"], { encoding: "utf8" }) : null;
const parser = await import(before ? `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`
  : pathToFileURL(path.resolve("crates/kanzei-app/ui/04-structured-parse.js")).href);
const checks = [], failures = [];
const check = (name, test) => {
  try { test(); checks.push(name); } catch (error) { failures.push({ name, error: String(error.message) }); }
};
const inline = "字段示例：`handoff_scope: request; handoff_target: input-a` 是本段示例，请原样保留。";
check("Inline code examples preserve their body and following explanation", () => assert.equal(parser.stripInternalHandoff(inline), inline));
const fenced = "````markdown\n```text\n交接范围: work_item\n交接目标: R-123\n```\n````";
check("A shorter nested fence cannot close a longer literal fence", () => assert.equal(parser.stripInternalHandoff(fenced), fenced));
for (const sample of [
  "示例：``a ` handoff_scope: request; handoff_target: input-a`` 后续说明。",
  "```text\n```javascript\n交接范围: work_item\n交接目标: R-123\n```",
  "~~~~text\n~~~\n交接范围: work_item\n交接目标: R-123\n~~~~",
]) check(`Literal handoff example stays intact: ${JSON.stringify(sample)}`, () => assert.equal(parser.stripInternalHandoff(sample), sample));
check("Actual inline handoff remains hidden beside inline code", () => assert.equal(parser.stripInternalHandoff("已完成 `demo`； handoff_scope: request; handoff_target: input-a"), "已完成 `demo`"));
const live = "已完成。\n交接范围: work_item\n交接目标: R-123\n验收标准: passed\n证据: tests";
check("Actual internal handoff fields remain hidden", () => assert.equal(parser.stripInternalHandoff(live), "已完成。"));
const ordinary = "交接清单模板\n交接范围: 各参与团队\n交接目标: 下一班值班人\n证据: 签字单";
check("Ordinary human handoff templates remain intact", () => assert.equal(parser.stripInternalHandoff(ordinary), ordinary));
check("Tool outcome marker matches the Rust serialized contract", () => assert.deepEqual(parser.stripToolOutcome("[tool_outcome=needs_correction code=EDIT_ANCHOR_NOT_FOUND]\nre-read target"),
  { outcome: "needs_correction", code: "EDIT_ANCHOR_NOT_FOUND", body: "re-read target" }));
check("Externalized result marker preserves byte count", () => assert.deepEqual(parser.parseStorageMarker("[tool_result_externalized artifact_id=a bytes=123 sha256=b]\nPreview: value"), { kind: "externalized", bytes: 123 }));
check("Rust preview marker preserves total line count", () => assert.deepEqual(parser.parsePreview("same (+1 lines)"), { first: "same", lineCount: 2 }));
check("Verbatim Windows root retains exact relative navigation target", () => assert.equal(parser.relativeToRoot("\\\\?\\C:\\repo space\\src\\main.rs", "C:/repo space"), "src/main.rs"));
check("Rich tokens concatenate exactly to the input", () => {
  const text = "See https://example.com/page, C:/repo space/src/main.rs:12 and R-123.";
  assert.equal(parser.tokenizeRich(text, { roots: ["C:/repo space"] }).map(token => token.value).join(""), text);
});
check("HTTP provider JSON error preserves status, message and code", () => {
  const result = parser.parseErrorText('provider HTTP 429: {"error":{"message":"quota exceeded","code":"rate_limit"}}');
  assert.equal(result.status, "429"); assert.equal(result.message, "quota exceeded"); assert.equal(result.fields.code, "rate_limit");
});
const oldDir = path.join(output, "old"), newDir = path.join(output, "new");
await mkdir(oldDir, { recursive: true }); await mkdir(newDir, { recursive: true });
for (const name of ["中文一.txt", "中文二.txt"]) {
  await writeFile(path.join(oldDir, name), "before\n"); await writeFile(path.join(newDir, name), "after\n");
}
let diff;
try { diff = execFileSync("git", ["-c", "core.quotePath=true", "diff", "--no-index", "--no-ext-diff", oldDir, newDir], { encoding: "utf8" }); }
catch (error) { if (error.status !== 1) throw error; diff = String(error.stdout); }
await writeFile(path.join(output, "actual-git.diff"), diff);
const files = parser.parseUnifiedDiff(diff);
await writeFile(path.join(output, "observed-diff.json"), JSON.stringify(files, null, 2));
check("Quoted Git file headers preserve separate file identities", () => assert.equal(files.length, 2));
check("Quoted UTF-8 Git paths are decoded into readable file names", () => {
  assert(files.some(file => file.path.endsWith("中文一.txt"))); assert(files.some(file => file.path.endsWith("中文二.txt")));
});
check("Quoted-file hunk counts belong to their own files", () => assert(files.every(file => file.additions === 1 && file.deletions === 1)));
check("Plain Git paths with spaces and deleted files keep their identity", () => {
  const result = parser.parseUnifiedDiff("diff --git a/space name.txt b/space name.txt\n--- a/space name.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-before");
  assert.equal(result[0].path, "space name.txt"); assert.equal(result[0].deletions, 1); assert.equal(result[0].additions, 0);
});
check("Git quoted escapes are decoded only once", () => {
  const encoded = String.raw`"b/quote\"tab\tback\\345.txt"`;
  const result = parser.parseUnifiedDiff(`diff --git "a/old.txt" ${encoded}\n--- "a/old.txt"\n+++ ${encoded}\n@@ -1 +1 @@\n-before\n+after`);
  assert.equal(result[0].path, 'quote"tab\tback\\345.txt'); assert.equal(result[0].language, "text");
});
await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ status: failures.length ? "failed" : "passed", before, checks, failures,
  boundary: "Actual pure ESM and read-only git diff on ignored fixture files. No Cargo/source/user data mutation." }, null, 2));
console.log(`Structured contracts: ${checks.length} PASS, ${failures.length} FAIL`);
for (const failure of failures) console.error(`${failure.name}: ${failure.error}`);
if (failures.length) process.exitCode = 1;
