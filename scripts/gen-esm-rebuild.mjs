#!/usr/bin/env node
// R-264 批3:重建 import 头。删除文件头部所有 import 语句,再按依赖图重新生成
// (依赖图已修正 BUILTIN,invoke/listen/t 等按提供方归位)。export 保持不变。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const uiDir = path.resolve(here, "../crates/kanzei-app/ui");
const graph = JSON.parse(fs.readFileSync(path.join(here, "ui-esm-graph.json"), "utf8")).graph;

for (const file of Object.keys(graph).sort()) {
  const p = path.join(uiDir, file);
  let src = fs.readFileSync(p, "utf8");
  // 删除头部连续 import 语句(含跨行块,直到第一个非 import 行)。
  const lines = src.split(/\r?\n/);
  let start = 0;
  let inImport = false;
  let depth = 0;
  while (start < lines.length) {
    const line = lines[start];
    if (!inImport) {
      if (/^\s*import\s/.test(line)) { inImport = true; depth = 0; }
      else break;
    }
    // 统计花括号深度(跨行 import 块)。
    for (const ch of line) {
      if (ch === "{") depth += 1;
      else if (ch === "}") depth -= 1;
    }
    start += 1;
    if (inImport && depth <= 0 && line.includes(";")) break; // 单行 import 结束
    if (inImport && depth <= 0 && !/^\s*import\s/.test(lines[start] ?? "")) break;
  }
  const body = lines.slice(start).join("\n").replace(/^\n+/, "");
  const imports = graph[file].imports ?? {};
  const bySource = new Map();
  for (const [sym, sfile] of Object.entries(imports)) {
    if (!bySource.has(sfile)) bySource.set(sfile, []);
    bySource.get(sfile).push(sym);
  }
  const stmts = [...bySource.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([sfile, syms]) => {
      const sorted = syms.sort();
      if (sorted.join(", ").length > 80) {
        return `import {\n  ${sorted.join(",\n  ")},\n} from "./${sfile}";`;
      }
      return `import { ${sorted.join(", ")} } from "./${sfile}";`;
    });
  const header = stmts.length > 0 ? stmts.join("\n") + "\n\n" : "";
  fs.writeFileSync(p, header + body);
  console.log(`[ok] ${file}: 重建 ${stmts.length} import 语句`);
}
