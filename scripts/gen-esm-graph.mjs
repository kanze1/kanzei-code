#!/usr/bin/env node
// R-264 批3:整体迁移依赖图生成器(只读分析,不改文件)。
// 对每个 ui/*.js 提取:①提供的顶层符号(export 候选);②消费的、定义在其他文件的
// 符号(import 候选)。输出 JSON 到 stdout,供迁移脚本消费。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const uiDir = path.resolve(here, "../crates/kanzei-app/ui");

function topLevelSymbols(src) {
  const names = new Set();
  for (const line of src.split(/\r?\n/)) {
    if (/^\s/.test(line)) continue;
    if (line.startsWith("//") || line.startsWith("/*") || line.startsWith("*")) continue; // 注释行
    let m;
    if ((m = line.match(/^(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_$][\w$]*)/))) {
      names.add(m[1]);
    } else if ((m = line.match(/^(?:export\s+)?class\s+([A-Za-z_$][\w$]*)/))) {
      names.add(m[1]);
    } else if ((m = line.match(/^(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)/))) {
      names.add(m[1]);
    } else if ((m = line.match(/^(?:export\s+)?(?:const|let|var)\s*\{\s*([^}]+)\s*\}\s*=/))) {
      for (const binding of m[1].split(",")) {
        const name = binding.trim().split(":")[0].trim().replace(/["']/g, "");
        if (/^[A-Za-z_$][\w$]*$/.test(name)) names.add(name);
      }
    } else if ((m = line.match(/^window\.([A-Za-z_$][\w$]*)\s*=/))) {
      names.add(m[1]);
    } else if ((m = line.match(/^globalThis\.([A-Za-z_$][\w$]*)\s*=/))) {
      names.add(m[1]);
    }
  }
  return names;
}

function referencedIdentifiers(src) {
  const found = new Set();
  const stripped = src
    .replace(/\/\/[^\n]*/g, " ")
    .replace(/"(?:[^"\\]|\\.)*"/g, " ")
    .replace(/'(?:[^'\\]|\\.)*'/g, " ")
    .replace(/`(?:[^`\\]|\\.)*`/g, " ");
  // 普通标识符:单词边界。`$` 是特殊符号(`$(...)` 简写)——`\b` 对非单词字符
  // 不成立,单独处理:匹配 `$` 后跟 `(`(作为标识符使用)。
  for (const m of stripped.matchAll(/\b([A-Za-z_][\w$]*)\b/g)) {
    found.add(m[1]);
  }
  for (const m of stripped.matchAll(/\$\(/g)) {
    found.add("$");
  }
  return found;
}

const files = fs.readdirSync(uiDir).filter((f) => f.endsWith(".js") && f !== "06-agent-panel.js").sort();
const provides = new Map();
const sourceOf = new Map();
const conflicts = new Map();
for (const file of files) {
  const src = fs.readFileSync(path.join(uiDir, file), "utf8");
  const syms = topLevelSymbols(src);
  provides.set(file, syms);
  for (const s of syms) {
    if (sourceOf.has(s)) {
      if (!conflicts.has(s)) conflicts.set(s, [sourceOf.get(s)]);
      conflicts.get(s).push(file);
    } else {
      sourceOf.set(s, file);
    }
  }
}

// 真内建/浏览器/环境全局——**只排除无提供方的**。有提供方(01-core 的 invoke/listen
// 等)走提供方,不在此列,否则跨文件引用漏识别(R-264 批3 实测:17 文件缺 invoke import)。
const BUILTIN = new Set([
  "Array","Object","String","Number","Boolean","Map","Set","WeakMap","WeakSet","Promise","Symbol",
  "JSON","Math","Date","RegExp","Error","TypeError","ReferenceError","RangeError","SyntaxError",
  "parseInt","parseFloat","isNaN","isFinite","encodeURIComponent","decodeURIComponent","setTimeout",
  "clearTimeout","setInterval","clearInterval","requestAnimationFrame","cancelAnimationFrame",
  "structuredClone","console","window","document","localStorage","navigator","Node","NodeFilter",
  "Option","FileReader","MutationObserver","ResizeObserver","FormData","TextEncoder","TextDecoder",
  "URL","URLSearchParams","Blob","File","Event","CustomEvent","KeyboardEvent","MouseEvent",
  "HTMLElement","HTMLInputElement","HTMLTextAreaElement","HTMLSelectElement","HTMLButtonElement",
  "Element","globalThis","undefined","Infinity","NaN","process","module","exports","require",
  "fetch","WebSocket","Audio","Image","location","history","screen","alert","confirm","prompt",
  "btoa","atob","queueMicrotask","MessageChannel","MessagePort","AbortController","AbortSignal",
  "CryptoKey","crypto","performance","setImmediate","clearImmediate","Proxy","Reflect","WeakRef",
  "FinalizationRegistry","BigInt","Intl","EventSource","__kzTest","__reportInitError",
  "__reportPersistentError","sessionStorage",
]);

const graph = {};
for (const file of files) {
  const src = fs.readFileSync(path.join(uiDir, file), "utf8");
  const refs = referencedIdentifiers(src);
  const provided = provides.get(file);
  const imports = new Map();
  for (const r of refs) {
    if (provided.has(r) || BUILTIN.has(r)) continue;
    const srcFile = sourceOf.get(r);
    if (srcFile && srcFile !== file) imports.set(r, srcFile);
  }
  graph[file] = {
    provides: [...provides.get(file)].sort(),
    imports: Object.fromEntries([...imports.entries()].sort((a, b) => a[0].localeCompare(b[0]))),
  };
}
const output = JSON.stringify({ graph, conflicts: Object.fromEntries(conflicts) }, null, 2) + "\n";
if (process.argv.includes("--write")) {
  fs.writeFileSync(path.join(here, "ui-esm-graph.json"), output);
  console.log(`wrote ${Object.keys(graph).length} UI files to scripts/ui-esm-graph.json`);
} else {
  process.stdout.write(output);
}
