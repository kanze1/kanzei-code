#!/usr/bin/env node
// R-264 批3:整体迁移脚本(21 文件同时 export + import 头)。
// 步骤:①每个文件的所有顶层声明加 export(与 gen-ui-lint-globals 同口径);
// ②按依赖图在文件头部插入 import 语句(从提供方文件导入消费的符号)。
// 幂等:已 export/已 import 的跳过。`--dry-run` 只打印不写盘。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const uiDir = path.resolve(here, "../crates/kanzei-app/ui");
const dryRun = process.argv.includes("--dry-run");

// 依赖图(由 gen-esm-graph.mjs --write 生成)。
// file -> { imports: { symbol: sourceFile } }
const graph = JSON.parse(fs.readFileSync(path.join(here, "ui-esm-graph.json"), "utf8")).graph;
const uiFiles = fs.readdirSync(uiDir).filter((file) => file.endsWith(".js")).sort();
const graphFiles = Object.keys(graph).sort();
const missingFromGraph = uiFiles.filter((file) => !graphFiles.includes(file));
const staleGraphEntries = graphFiles.filter((file) => !uiFiles.includes(file));
if (missingFromGraph.length > 0 || staleGraphEntries.length > 0) {
  throw new Error(
    [
      "scripts/ui-esm-graph.json 与 crates/kanzei-app/ui/*.js 不一致，已拒绝迁移以避免静默漏项。",
      missingFromGraph.length ? `graph 缺少: ${missingFromGraph.join(", ")}` : "",
      staleGraphEntries.length ? `graph 悬空: ${staleGraphEntries.join(", ")}` : "",
      "请先运行 node scripts/gen-esm-graph.mjs --write。",
    ].filter(Boolean).join(" "),
  );
}

function addExports(src) {
  const lines = src.split(/\r?\n/);
  let changed = 0;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (/^\s/.test(line) || /^export\b/.test(line)) continue;
    if (
      line.match(/^(?:async\s+)?function\s+[A-Za-z_$][\w$]*/) ||
      line.match(/^class\s+[A-Za-z_$][\w$]*/) ||
      line.match(/^(?:const|let|var)\s+[A-Za-z_$][\w$]*/) ||
      line.match(/^(?:const|let|var)\s*\{\s*[^}]+\s*\}\s*=/)
    ) {
      lines[i] = `export ${line}`;
      changed += 1;
    }
  }
  return { text: lines.join("\n"), changed };
}

function addImports(src, file) {
  const imports = graph[file]?.imports ?? {};
  const bySource = new Map();
  for (const [sym, sfile] of Object.entries(imports)) {
    if (!bySource.has(sfile)) bySource.set(sfile, []);
    bySource.get(sfile).push(sym);
  }
  // 生成 import 语句,按源文件排序,每行一条(规范可读)。
  const stmts = [...bySource.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([sfile, syms]) => {
      const sorted = syms.sort();
      // 超过一行的符号列表换行缩进(保持可读)。
      if (sorted.join(", ").length > 80) {
        return `import {\n  ${sorted.join(",\n  ")},\n} from "./${sfile}";`;
      }
      return `import { ${sorted.join(", ")} } from "./${sfile}";`;
    });
  if (stmts.length === 0) return { text: src, added: 0 };
  const header = stmts.join("\n") + "\n\n";
  return { text: header + src, added: stmts.length };
}

let totalExports = 0;
let totalImports = 0;
for (const file of Object.keys(graph).sort()) {
  const p = path.join(uiDir, file);
  const src = fs.readFileSync(p, "utf8");
  const withExports = addExports(src);
  const withImports = addImports(withExports.text, file);
  totalExports += withExports.changed;
  totalImports += withImports.added;
  if (dryRun) {
    console.log(`[dry] ${file}: +${withExports.changed} export, +${withImports.added} import stmts`);
  } else {
    fs.writeFileSync(p, withImports.text);
    console.log(`[ok] ${file}: +${withExports.changed} export, +${withImports.added} import stmts`);
  }
}
console.log(`TOTAL: ${totalExports} exports, ${totalImports} import statements across ${Object.keys(graph).length} files`);
