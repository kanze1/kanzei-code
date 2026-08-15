#!/usr/bin/env node
// R-266:workspace crate 清单与 README 项目结构表机械同步校验。
// 从 Cargo.toml [workspace] members 取 crate 名(规范化 `crates/kanzei-base` →
// `kanzei-base`),与 README `## 项目结构` 表逐行比对:README 缺 crate、或有多余
// crate(不在 members 里),都判失败并点名。README 表用 crate 名首列精确匹配。
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

// 2) 从 README `## 项目结构` 表取 crate 名单列。
const readme = fs.readFileSync(path.join(root, "README.md"), "utf8");
const tableStart = readme.indexOf("## 项目结构");
if (tableStart === -1) {
  console.error("R-266: README 缺少 `## 项目结构` 章节");
  process.exit(1);
}
const tableSection = readme.slice(tableStart, readme.indexOf("\n## ", tableStart + 10));
const readmeCrates = [];
for (const line of tableSection.split(/\r?\n/)) {
  const m = line.match(/^\|\s*`([a-zA-Z0-9_-]+)`\s*\|/);
  if (m) readmeCrates.push(m[1]);
}
if (readmeCrates.length === 0) {
  console.error("R-266: README 项目结构表未解析到任何 crate 行");
  process.exit(1);
}

// 3) 比对:README 缺成员 / README 多出非成员。
const membersSet = new Set(members);
const readmeSet = new Set(readmeCrates);
const missing = members.filter((c) => !readmeSet.has(c)).sort();
const extra = readmeCrates.filter((c) => !membersSet.has(c)).sort();
const failures = [];
if (missing.length > 0) {
  failures.push(`README 缺少 crate: ${missing.join(", ")}`);
}
if (extra.length > 0) {
  failures.push(`README 有多余 crate(不在 Cargo.toml members): ${extra.join(", ")}`);
}
if (failures.length > 0) {
  console.error(`R-266 校验失败(${failures.length} 项):`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log(`R-266 通过:${members.length} 个 crate 与 README 项目结构表一致`);
