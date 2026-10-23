#!/usr/bin/env node
/**
 * R-318:机械校验设计文档身份、tracker 终态声明和默认上下文边界。
 *
 * 只消费结构化索引元数据和文档头部声明；历史快照/被替代正文不参与
 * 当前 tracker 终态冲突判定，避免把历史证据误报成现行漂移。
 */
import path from "node:path";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import { execFileSync } from "node:child_process";
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
  "draft",
  "awaiting_external",
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
    const heading = /^#{1,6}\s+([RD]-\d+)\s+([^\r\n]+)/gm;
    for (const match of text.matchAll(heading)) {
      // 账本格式是 ID + 标题 + [状态]；标题本身也可能带方括号。
      const status = [...match[2].matchAll(/\[([^\]]+)\]/g)]
        .map(hit => hit[1].toLowerCase())
        .findLast(value => TERMINAL.has(value) || NON_TERMINAL.has(value));
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
  const pattern = /\b([RD]-\d+)\b[^\n]{0,100}?(?<![\p{L}\p{N}_])(done|fixed|wontfix|dropped|todo|doing|fixing|open|in progress|进行中|待推进|未实现|待用户拍板|已完成|已交付)(?![\p{L}\p{N}_])/giu;
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

/**
 * D-766:门禁判的是**已提交树**,不是工作副本。
 *
 * 此前这里读 `fs.readFileSync`,于是「设计文档已提交、索引行还在工作副本未提交」
 * 也能过关,发版回执绑定的那棵树其实过不了自己的门禁。改为按 HEAD 取内容后,
 * 工作副本里的未提交文档与未提交索引行同时不可见,判定只反映回执要认证的那棵树。
 */
function readHeadProject(root) {
  const git = (...args) =>
    execFileSync("git", ["-C", root, ...args], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 32 * 1024 * 1024 }).trim();
  const revision = git("rev-parse", "HEAD");
  const listed = (prefix) =>
    git("ls-tree", "-r", "--name-only", revision, "--", prefix)
      .split(/\r?\n/)
      .filter(Boolean);
  const show = (file) => git("show", `${revision}:${file}`);
  const designFiles = listed("docs/design").filter((file) => file.endsWith(".md"));
  const diskDocs = designFiles.map((file) => path.basename(file));
  const documentTexts = new Map(designFiles.map((file) => [path.basename(file), show(file)]));
  const indexText = show(".kanzei/project/architecture/README.md");
  const projectFiles = new Set(listed(".kanzei/project"));
  const trackerTexts = [
    "requirements.md",
    "requirements-archive.md",
    "defects.md",
    "defects-archive.md",
  ].map(file => `.kanzei/project/${file}`)
    .filter(file => projectFiles.has(file))
    .map(show);
  return { indexText, diskDocs, documentTexts, trackerStatuses: parseTrackerStatuses(trackerTexts) };
}

export function run(root) {
  const issues = validateFreshness(readHeadProject(root));
  if (issues.length) {
    console.error(`设计时效门禁失败(${issues.length}):`);
    for (const issue of issues) console.error(`- ${issue}`);
    return false;
  }
  console.log("设计时效门禁通过：索引身份、截至提交、替代关系与现行 tracker 声明一致；历史/被替代快照未参与现行冲突判定");
  return true;
}

function selfTest() {
  assert.deepEqual(
    [...parseTrackerStatuses(["## R-1 用户需求 [done]\n## D-2 带 [内部标签] 的缺陷 [open] (medium)\n"])],
    [["R-1", "done"], ["D-2", "open"]],
    "必须解析真实账本中 ID、标题、状态的顺序",
  );
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
  // D-766:文档已在树里、索引行还没提交,就是这个形态——门禁必须报,不能靠工作副本自证。
  const missingRow = indexText
    .split(/\r?\n/)
    .filter((line) => !line.includes("[`live.md`]"))
    .join("\n");
  issues = validateFreshness({ indexText: missingRow, diskDocs, documentTexts: baseDocs, trackerStatuses: statuses });
  assert(
    issues.some((issue) => issue.includes("design doc missing from index: live.md")),
    "未捕获索引行缺失",
  );
  for (const claim of ["R-1 已完成", "R-1 已交付", "R-1 done"]) {
    assert.equal(statusClaims(`- 关联需求: ${claim}\n`)[0]?.terminal, true, claim);
  }

  // 真实 Git 树验证 IO 边界：工作副本的索引和 tracker 不能修补已提交树的缺陷。
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "kanzei-design-freshness-"));
  try {
    const git = (...args) => execFileSync("git", ["-C", temp, ...args], {
      encoding: "utf8", stdio: ["ignore", "pipe", "pipe"],
    }).trim();
    const write = (file, content) => {
      const target = path.join(temp, file);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, content);
    };
    const commit = () => {
      git("add", ".");
      git("-c", "user.name=Verification Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture");
    };
    const inspect = () => validateFreshness(readHeadProject(temp));
    const index = ".kanzei/project/architecture/README.md";
    const tracker = ".kanzei/project/requirements.md";
    const doc = "docs/design/live.md";
    const row = "- [identity: live_design; last_verified_commit: abcdef1] [`live.md`](../../../docs/design/live.md)\n";
    git("init", "-q");
    write(index, "# 架构索引\n");
    write(tracker, "## R-1 用户需求 [done]\n");
    write(doc, "- identity: live_design\n- 关联需求: R-1 done\n");
    commit();
    write(index, row);
    assert(inspect().some(issue => issue.includes("design doc missing from index: live.md")), "未提交的索引行不得补救 HEAD");
    commit();
    assert.deepEqual(inspect(), []);
    write(tracker, "## R-1 用户需求 [todo]\n");
    write(doc, "- identity: live_design\n- 关联需求: R-1 todo\n");
    git("add", doc);
    git("-c", "user.name=Verification Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "stale design");
    assert(inspect().some(issue => issue.includes("tracker is done")), "未提交的 tracker 状态不得掩盖 HEAD 终态冲突");
    write(doc, "- identity: live_design\n- 关联需求: R-1 done\n");
    assert(inspect().some(issue => issue.includes("tracker is done")), "未提交的正文不得修补 HEAD");
    commit();
    assert(inspect().some(issue => issue.includes("tracker is todo")), "中文/英文正文声明必须与真实账本比较");
    write(doc, "- identity: live_design\n- 关联需求: R-1 待推进\n");
    commit();
    assert.deepEqual(inspect(), []);
    write("docs/design/untracked.md", "not in HEAD\n");
    assert.deepEqual(inspect(), [], "未提交文档不能混入 HEAD 判定");
  } finally {
    assert.equal(path.dirname(temp), path.resolve(os.tmpdir()));
    assert(path.basename(temp).startsWith("kanzei-design-freshness-"));
    fs.rmSync(temp, { recursive: true, force: true });
  }
  console.log("设计时效门禁自测通过：真实账本标题、中英文终态、身份矛盾、历史快照、未提交索引/正文/tracker 的 Git 树回归");
}

const entry = process.argv[1]?.toLowerCase().endsWith("check-design-freshness.mjs");
if (entry) {
  if (process.argv.includes("--self-test")) selfTest();
  if (!run(path.resolve(path.dirname(fileURLToPath(import.meta.url)), ".."))) process.exitCode = 1;
}
