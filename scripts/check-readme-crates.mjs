#!/usr/bin/env node
// R-266:workspace crate 清单与系统合同的组件表机械同步校验。
// 从 Cargo.toml [workspace] members 取 crate 名(规范化 `crates/kanzei-base` →
// `kanzei-base`),与系统合同 `## 组件与依赖` 的链接首列精确比对。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..");

// 1) 从 Cargo.toml members 取 crate 名。
const cargoToml = fs.readFileSync(path.join(root, "Cargo.toml"), "utf8");
const members = [];
let inWorkspace = false;
let inMembers = false;
for (const line of cargoToml.split(/\r?\n/)) {
  const t = line.trim();
  if (t.startsWith("[workspace]")) { inWorkspace = true; continue; }
  if (!inWorkspace) continue;
  if (t.startsWith("members")) { inMembers = true; continue; }
  if (inMembers && t === "]") { inMembers = false; continue; }
  if (inMembers && t.startsWith('"')) {
    const dir = t.trimEnd().replace(/,$/, "").trim().replace(/^"|"$/g, "");
    const crate = path.basename(dir); // crates/kanzei-base → kanzei-base
    members.push(crate);
  }
}
if (members.length === 0) {
  console.error("R-266: Cargo.toml 未解析到任何 workspace members,校验机制退化");
  process.exit(1);
}

// 2) 系统合同是组件所有权清单的唯一文档来源。
const documentPath = "docs/design/system_contracts.md";
const document = fs.readFileSync(path.join(root, documentPath), "utf8");
const heading = "## 组件与依赖";
const tableStart = document.indexOf(heading);
if (tableStart === -1) {
  console.error(`R-266: ${documentPath} 缺少 ${heading} 章节`);
  process.exit(1);
}
const sectionEnd = document.indexOf("\n## ", tableStart + heading.length);
const tableSection = document.slice(tableStart, sectionEnd === -1 ? document.length : sectionEnd);
const documentedCrates = [];
for (const line of tableSection.split(/\r?\n/)) {
  const m = line.match(/^\|\s*\[([a-zA-Z0-9_-]+)\]\([^)]+\)\s*\|/);
  if (m) documentedCrates.push(m[1]);
}
if (documentedCrates.length === 0) {
  console.error(`R-266: ${documentPath} 组件表未解析到任何 crate 行`);
  process.exit(1);
}

// 3) 缺失、多余和重复行均不能代表一个完整的所有权清单。
const membersSet = new Set(members);
const documentedSet = new Set(documentedCrates);
const missing = members.filter((c) => !documentedSet.has(c)).sort();
const extra = documentedCrates.filter((c) => !membersSet.has(c)).sort();
const duplicate = [...documentedSet].filter(c => documentedCrates.filter(value => value === c).length > 1).sort();
const failures = [];
if (missing.length > 0) {
  failures.push(`系统合同缺少 crate: ${missing.join(", ")}`);
}
if (extra.length > 0) {
  failures.push(`系统合同有多余 crate(不在 Cargo.toml members): ${extra.join(", ")}`);
}
if (duplicate.length > 0) failures.push(`系统合同有重复 crate: ${duplicate.join(", ")}`);
if (failures.length > 0) {
  console.error(`R-266 校验失败(${failures.length} 项):`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log(`R-266 通过:${members.length} 个 crate 与系统合同组件表一致`);
