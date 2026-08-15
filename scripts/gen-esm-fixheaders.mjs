#!/usr/bin/env node
// R-264 批3:彻底清理所有 import 语句(头部+中部+重复),再重建唯一一套。
// 状态机扫描全文件:遇到 import 开头的行,收集到 `;` 结束(含跨行),整块删除;
// 其余行保留。重建按依赖图生成 import 头。
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
  const lines = src.split(/\r?\n/);
  const kept = [];
  let removed = 0;
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (/^\s*import\s/.test(line)) {
      let block = line;
      let depth = 0;
      for (const ch of line) { if (ch === "{") depth += 1; else if (ch === "}") depth -= 1; }
      let done = line.includes(";") && depth <= 0;
      i += 1;
      while (!done && i < lines.length) {
        const next = lines[i];
        block += "\n" + next;
        for (const ch of next) { if (ch === "{") depth += 1; else if (ch === "}") depth -= 1; }
        if (next.includes(";") && depth <= 0) done = true;
        i += 1;
      }
      removed += 1;
      continue;
    }
    kept.push(line);
    i += 1;
  }
  const body = kept.join("\n").replace(/\n{3,}/g, "\n\n").replace(/^\n+/, "");
  const imports = graph[file].imports ?? {};
  const bySource = new Map();
  for (const [sym, sfile] of Object.entries(imports)) {
    if (!bySource.has(sfile)) bySource.set(sfile, []);
    bySource.get(sfile).push(sym);
  }
  const stmts = [...bySource.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([sfile, syms]) => {
      const sorted = [...new Set(syms)].sort();
      if (sorted.join(", ").length > 80) {
        return `import {\n  ${sorted.join(",\n  ")},\n} from "./${sfile}";`;
      }
      return `import { ${sorted.join(", ")} } from "./${sfile}";`;
    });
  const header = stmts.length > 0 ? stmts.join("\n") + "\n\n" : "";
  if (dryRun) {
    console.log(`[dry] ${file}: 删 ${removed} 条, 重建 ${stmts.length} 条`);
  } else {
    fs.writeFileSync(p, header + body);
    console.log(`[ok] ${file}: 删 ${removed} 条, 重建 ${stmts.length} 条`);
  }
}
