#!/usr/bin/env node
// R-264 批3:增量补 import——按依赖图为每个文件补齐缺失的 import 符号。
// 已有 import 语句的源文件:往对应语句追加缺失符号;缺整个源文件的:新建 import 语句。
// 幂等:已有符号跳过。`--dry-run` 只打印。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const uiDir = path.resolve(here, "../crates/kanzei-app/ui");
const dryRun = process.argv.includes("--dry-run");
const graph = JSON.parse(fs.readFileSync(path.join(here, "ui-esm-graph.json"), "utf8")).graph;

for (const file of Object.keys(graph).sort()) {
  const p = path.join(uiDir, file);
  const src = fs.readFileSync(p, "utf8");
  const imports = graph[file].imports ?? {};
  // 按源文件聚合缺失符号。
  const needBySource = new Map();
  for (const [sym, sfile] of Object.entries(imports)) {
    // 检查是否已 import(粗略:匹配 `import { ... sym ... } from "./sfile"` 跨行)。
    const existing = new RegExp(`from\\s+["']\\./${sfile}["']`).test(src);
    if (existing) {
      // 已在 import 语句里?精确匹配符号名出现在该 import 块。
      const block = src.match(new RegExp(`import\\s*\\{[^}]*\\}\\s*from\\s*["']\\./${sfile}["']`, "s"));
      if (block && new RegExp(`\\b${sym}\\b`).test(block[0])) continue;
      needBySource.set(sfile, [...(needBySource.get(sfile) ?? []), sym]);
    } else {
      needBySource.set(sfile, [...(needBySource.get(sfile) ?? []), sym]);
    }
  }
  if (needBySource.size === 0) continue;
  if (dryRun) {
    console.log(`[dry] ${file}: 需补 import:`);
    for (const [sfile, syms] of needBySource) console.log(`  from ${sfile}: ${syms.join(", ")}`);
    continue;
  }
  // 构造补充语句:已存在该源的 import → 无法原位插入(简化:整文件 import 块重建复杂),
  // 采用「在文件头追加新 import 语句」——ESM 允许多条同源 import 合并?不,会重复声明。
  // 更稳:把缺失符号追加到已有 import 块,或新建块。这里用文本替换:对已有源文件,
  // 在 `import { ... } from "./sfile";` 的闭合 `}` 前插入缺失符号。
  let out = src;
  for (const [sfile, syms] of needBySource) {
    const existingPattern = new RegExp(`(import\\s*\\{[^}]*)\\}\\s*from\\s*["']\\./${sfile}["']`, "s");
    if (existingPattern.test(out)) {
      // 追加到已有块(去重)。
      out = out.replace(existingPattern, (m, head) => {
        const have = new Set(head.match(/\b[A-Za-z_$][\w$]*\b/g) ?? []);
        const add = syms.filter((s) => !have.has(s));
        if (add.length === 0) return m;
        return `${head}  ${add.join(",\n  ")},\n} from "./${sfile}";`;
      });
    } else {
      // 新建 import 语句,插在文件头(首个非 import 行前)。
      const sorted = syms.sort();
      const stmt = `import { ${sorted.join(", ")} } from "./${sfile}";\n`;
      // 插到现有 import 块之后(文件开头连续 import 行后)。
      const lines = out.split(/\r?\n/);
      let idx = 0;
      while (idx < lines.length && /^\s*import\s/.test(lines[idx])) idx += 1;
      lines.splice(idx, 0, stmt.trimEnd());
      out = lines.join("\n");
    }
  }
  fs.writeFileSync(p, out);
  const added = [...needBySource.values()].flat().length;
  console.log(`[ok] ${file}: 补 ${added} 符号`);
}
