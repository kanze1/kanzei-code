#!/usr/bin/env node
// R-142:从 ui/*.js 提取顶层声明的跨文件全局白名单(no-undef 需要)。
// ui/*.js 是经典 script 按序加载,顶层 const/let/function/class 共享全局作用域,
// 但 ESLint 按单文件分析——本文件声明、别文件使用的名字必须进 globals,否则误报。
// 输出到 scripts/ui-lint-globals.json 供 eslint.config.js 引用;
// ui-lint-smoke 会校验该清单与源码同步(新增顶层标识符后重跑本脚本)。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const uiDir = path.resolve(here, "../crates/kanzei-app/ui");
const outFile = path.join(here, "ui-lint-globals.json");

const names = new Set();
for (const file of fs.readdirSync(uiDir).filter((f) => f.endsWith(".js")).sort()) {
  const src = fs.readFileSync(path.join(uiDir, file), "utf8");
  for (const line of src.split(/\r?\n/)) {
    const t = line.trim();
    let m;
    if ((m = t.match(/^(?:export\s+)?(?:async\s+)?function\s+([A-Za-z_$][\w$]*)/))) {
      names.add(m[1]);
    } else if ((m = t.match(/^(?:export\s+)?class\s+([A-Za-z_$][\w$]*)/))) {
      names.add(m[1]);
    } else if ((m = t.match(/^(?:const|let|var)\s+([A-Za-z_$][\w$]*)/))) {
      names.add(m[1]);
    } else if ((m = t.match(/^(?:const|let|var)\s*\{\s*([^}]+)\s*\}\s*=/))) {
      // 解构声明:const { invoke, listen } = window.__TAURI__.* / const { a: x } = ...
      for (const binding of m[1].split(",")) {
        const name = binding.trim().split(":")[0].trim().replace(/["']/g, "");
        if (/^[A-Za-z_$][\w$]*$/.test(name)) names.add(name);
      }
    } else if ((m = t.match(/^window\.([A-Za-z_$][\w$]*)\s*=/))) {
      names.add(m[1]);
    } else if ((m = t.match(/^globalThis\.([A-Za-z_$][\w$]*)\s*=/))) {
      names.add(m[1]);
    }
    // window["x"] = 与 Object.assign(window, {...}) 形式
    else if ((m = t.match(/^window\[["']([A-Za-z_$][\w$]*)["']\]\s*=/))) {
      names.add(m[1]);
    } else if ((m = t.match(/^Object\.assign\(window, \{([^}]*)\}\)/))) {
      for (const kv of m[1].split(",")) {
        const k = kv.trim().split(":")[0].trim().replace(/["']/g, "");
        if (/^[A-Za-z_$][\w$]*$/.test(k)) names.add(k);
      }
    }
  }
}

const globals = [...names].sort();
if (process.argv.includes("--check")) {
  const existing = JSON.parse(fs.readFileSync(outFile, "utf8"));
  const missing = globals.filter((n) => !existing.includes(n));
  const stale = existing.filter((n) => !globals.includes(n));
  if (missing.length || stale.length) {
    console.error(`ui-lint-globals.json 与源码不同步:`);
    if (missing.length) console.error(`  缺(${missing.length}): ${missing.slice(0, 10).join(", ")}...`);
    if (stale.length) console.error(`  多余(${stale.length}): ${stale.slice(0, 10).join(", ")}...`);
    console.error(`请重跑: node scripts/gen-ui-lint-globals.mjs`);
    process.exit(1);
  }
  console.log(`ui-lint-globals.json 与源码同步(${globals.length} 个标识符)`);
} else {
  fs.writeFileSync(outFile, JSON.stringify(globals, null, 2) + "\n");
  console.log(`生成 ${outFile}:${globals.length} 个顶层标识符`);
}

