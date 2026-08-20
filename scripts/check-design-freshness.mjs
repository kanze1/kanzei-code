#!/usr/bin/env node
/**
 * R-318:机械校验设计文档身份、tracker 终态声明和默认上下文边界。
 *
 * 只消费结构化索引元数据和文档头部声明；历史快照/被替代正文不参与
 * 当前 tracker 终态冲突判定，避免把历史证据误报成现行漂移。
 */
import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";

const IDENTITIES = new Set([
  "live_design",
  "validated_design",
  "historical_snapshot",
  "superseded",
]);
const TERMINAL = new Set(["done", "fixed", "dropped", "wontfix"]);
const NON_TERMINAL = new Set([
  "todo",
  "doing",
  "fixing",
  "open",
  "in progress",
  "进行中",
  "待推进",
  "未实现",
  "待用户拍板",
]);

export function parseIndex(indexText) {
  const entries = [];
  for (const [lineNumber, line] of indexText.split(/\r?\n/).entries()) {
    const link = line.match(/\[`([^`]+\.md)`\]\(\.\.\/\.\.\/\.\.\/docs\/design\/([^)]*)\)/);
    const identity = line.match(/\[identity:\s*(\w+)\b/);
    if (!link || !identity) continue;
    const kind = identity[1];
    entries.push({
      lineNumber: lineNumber + 1,
      label: link[1],
      file: link[2],
      kind,
      lastVerifiedCommit: line.match(/last_verified_commit:\s*([0-9a-f]{7,40})/)?.[1] ?? null,
      asOfCommit: line.match(/as_of_commit:\s*([0-9a-f]{7,40})/)?.[1] ?? null,
      supersededBy: line.match(/superseded_by:\s*([a-z0-9_]+\.md)/)?.[1] ?? null,
    });
  }
  return entries;
}

function parseTrackerStatuses(texts) {
  const statuses = new Map();
  for (const text of texts) {
    const heading = /^#{1,6}\s+([RD]-\d+)\s+\[[^\]]+\]\s*([^\n]*)/gm;
    for (const match of text.matchAll(heading)) {
      const status = match[0].match(/\[([^\]]+)\]/)?.[1]?.toLowerCase();
      if (!status) continue;
      const id = match[1];
      if (!statuses.has(id) || TERMINAL.has(status)) statuses.set(id, status);
    }
  }
  return statuses;
}

function statusClaims(text) {
  const header = text.split(/\n(?=##\s)/, 1)[0];
  const claims = [];
  const pattern = /\b([RD]-\d+)\b[^\n]{0,100}?\b(done|fixed|wontfix|dropped|todo|doing|fixing|open|in progress|进行中|待推进|未实现|待用户拍板|已完成|已交付)\b/gi;
  for (const match of header.matchAll(pattern)) {
    const raw = match[2].toLowerCase();
    claims.push({ id: match[1], terminal: TERMINAL.has(raw) || raw === "已完成" || raw === "已交付", raw });
  }
  return claims;
}

function documentIdentityDeclarations(text) {
  return [...text.matchAll(/^\s*-\s*(?:身份|identity)\s*:\s*([a-z_]+)/gim)].map((match) => match[1]);
}

export function validateFreshness({ indexText, diskDocs, documentTexts, trackerStatuses }) {
  const issues = [];
  const entries = parseIndex(indexText);
  const disk = [...diskDocs].sort();
  const indexed = entries.map((entry) => entry.file).sort();

  if (entries.length !== disk.length) {
    issues.push(`index has ${entries.length} structured links but disk has ${disk.length} design docs`);
  }
  if (new Set(indexed).size !== indexed.length) issues.push("duplicate design document identity entries");
  for (const file of disk) if (!indexed.includes(file)) issues.push(`design doc missing from index: ${file}`);
  for (const entry of entries) {
    if (!IDENTITIES.has(entry.kind)) issues.push(`line ${entry.lineNumber}: unknown identity ${entry.kind}`);
    if (["live_design", "validated_design"].includes(entry.kind) && !entry.lastVerifiedCommit) {
      issues.push(`line ${entry.lineNumber}: ${entry.kind} missing last_verified_commit`);
    }
    if (entry.kind === "historical_snapshot" && !entry.asOfCommit) {
      issues.push(`line ${entry.lineNumber}: historical_snapshot missing as_of_commit`);
    }
    if (entry.kind === "superseded") {
      if (!entry.supersededBy) issues.push(`line ${entry.lineNumber}: superseded missing superseded_by`);
      else if (!disk.includes(entry.supersededBy)) issues.push(`line ${entry.lineNumber}: replacement missing: ${entry.supersededBy}`);
    }

    const text = documentTexts.get(entry.file) ?? "";
    const declarations = documentIdentityDeclarations(text);
    if (new Set(declarations).size > 1) {
      issues.push(`${entry.file}: conflicting identity declarations: ${[...new Set(declarations)].join(", ")}`);
    }
    if (declarations.length && declarations[0] !== entry.kind) {
      issues.push(`${entry.file}: document identity ${declarations[0]} disagrees with index ${entry.kind}`);
    }

    // Historical evidence is intentionally not compared with today's tracker state.
    if (["historical_snapshot", "superseded"].includes(entry.kind)) continue;
    for (const claim of statusClaims(text)) {
      const actual = trackerStatuses.get(claim.id);
      if (!actual || !TERMINAL.has(actual) && !NON_TERMINAL.has(actual)) continue;
      const actualTerminal = TERMINAL.has(actual);
      if (claim.terminal !== actualTerminal) {
        issues.push(`${entry.file}: ${claim.id} claims ${claim.raw}, tracker is ${actual}`);
      }
    }
  }
  return issues;
}

function readProject(root) {
  const designDir = path.join(root, "docs", "design");
  const diskDocs = fs.readdirSync(designDir).filter((file) => file.endsWith(".md"));
  const documentTexts = new Map(diskDocs.map((file) => [file, fs.readFileSync(path.join(designDir, file), "utf8")]));
  const indexText = fs.readFileSync(path.join(root, ".kanzei", "project", "architecture", "README.md"), "utf8");
  const trackerTexts = [
    "requirements.md",
    "requirements-archive.md",
    "defects.md",
    "defects-archive.md",
  ].map((file) => {
    const full = path.join(root, ".kanzei", "project", file);
    return fs.existsSync(full) ? fs.readFileSync(full, "utf8") : "";
  });
  return { indexText, diskDocs, documentTexts, trackerStatuses: parseTrackerStatuses(trackerTexts) };
}

export function run(root) {
  const issues = validateFreshness(readProject(root));
  if (issues.length) {
    console.error(`设计时效门禁失败(${issues.length}):`);
    for (const issue of issues) console.error(`- ${issue}`);
    return false;
  }
  console.log("设计时效门禁通过：索引身份、截至提交、替代关系与现行 tracker 声明一致；历史/被替代快照未参与现行冲突判定");
  return true;
}

function selfTest() {
  const indexText = [
    "## live_design",
    "- [identity: live_design; last_verified_commit: abcdef1] [`live.md`](../../../docs/design/live.md)",
    "## historical_snapshot",
    "- [identity: historical_snapshot; as_of_commit: abcdef1] [`history.md`](../../../docs/design/history.md)",
    "## superseded",
    "- [identity: superseded; as_of_commit: abcdef1; superseded_by: live.md] [`old.md`](../../../docs/design/old.md)",
  ].join("\n");
  const diskDocs = ["history.md", "live.md", "old.md"];
  const baseDocs = new Map([
    ["live.md", "- identity: live_design\n- 关联需求: R-1 done\n"],
    ["history.md", "- 关联需求: R-2 todo\n"],
    ["old.md", "- 关联需求: R-1 todo\n正文历史快照\n"],
  ]);
  const statuses = new Map([["R-1", "done"], ["R-2", "done"]]);
  let issues = validateFreshness({ indexText, diskDocs, documentTexts: baseDocs, trackerStatuses: statuses });
  assert.deepEqual(issues, [], `历史/被替代快照误报: ${issues.join("; ")}`);

  const conflict = new Map(baseDocs);
  conflict.set("live.md", "- identity: validated_design\n- 关联需求: R-1 todo\n");
  issues = validateFreshness({ indexText, diskDocs, documentTexts: conflict, trackerStatuses: statuses });
  assert(issues.some((issue) => issue.includes("identity") && issue.includes("disagrees")), "未捕获同文档身份矛盾");
  assert(issues.some((issue) => issue.includes("R-1") && issue.includes("tracker is done")), "未捕获终态冲突");
  console.log("设计时效门禁自测通过：终态冲突、同文档身份矛盾、历史快照不误报");
}

const entry = process.argv[1]?.toLowerCase().endsWith("check-design-freshness.mjs");
if (entry) {
  if (process.argv.includes("--self-test")) selfTest();
  else if (!run(path.resolve(path.dirname(fileURLToPath(import.meta.url)), ".."))) process.exitCode = 1;
}
