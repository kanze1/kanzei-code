#!/usr/bin/env node
// R-299:IPC 事件契约机械求差(emit/listen 两侧集合)。
// 后端 crates/kanzei-app 的 window.emit/emit_event 事件名集合,与前端 ui/*.js 的
// on()/listen() 订阅名集合,必须严格一致:
//   - 后端 emit 了但前端没人订阅(backend-only)→ error:事件发了没人听,
//     通常是后端改名没同步前端,或新事件忘了接线;
//   - 前端订阅了但后端从不 emit(frontend-only)→ error:死订阅,
//     可能是事件被移除/改名,前端在等一个永远不会来的事件。
// 任一侧独有即红——机械判据,不靠人自觉;后端改事件名立刻被抓。
// 当前基线(2026-08-18):两侧各 22 个事件,差集为空。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..");

function walk(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(p, out);
    else if (entry.name.endsWith(".rs")) out.push(p);
  }
  return out;
}

// 后端:window.emit("kz:xxx" / ui.emit("kz:xxx" / emit_event("kz:xxx", 跨行 \s* 容忍
// 多行形式(如 window.emit(\n "kz:stopped",\n payload))。只抓 kz: 前缀事件名。
const BACKEND_RE = /(?:\.emit|emit_event)\(\s*"(kz:[a-z-]+)"/g;
const backendEvents = new Set();
for (const file of walk(path.join(root, "crates/kanzei-app/src"))) {
  const src = fs.readFileSync(file, "utf8");
  for (const m of src.matchAll(BACKEND_RE)) backendEvents.add(m[1]);
}

// 前端:on("kz:xxx" / listen("kz:xxx"(Tauri 事件订阅)。
const FRONTEND_RE = /(?:on|listen)\(\s*"(kz:[a-z-]+)"/g;
const frontendEvents = new Set();
for (const file of fs.readdirSync(path.join(root, "crates/kanzei-app/ui")).filter((f) => f.endsWith(".js"))) {
  const src = fs.readFileSync(path.join(root, "crates/kanzei-app/ui", file), "utf8");
  for (const m of src.matchAll(FRONTEND_RE)) frontendEvents.add(m[1]);
}

const backendOnly = [...backendEvents].filter((e) => !frontendEvents.has(e)).sort();
const frontendOnly = [...frontendEvents].filter((e) => !backendEvents.has(e)).sort();
const MIN_IPC_EVENTS = 10;

if (backendEvents.size < MIN_IPC_EVENTS || frontendEvents.size < MIN_IPC_EVENTS) {
  console.error(
    `IPC 事件契约求差拒绝空集/异常低计数:后端 ${backendEvents.size},前端 ${frontendEvents.size},下限 ${MIN_IPC_EVENTS}`,
  );
  process.exit(1);
}

if (backendOnly.length || frontendOnly.length) {
  console.error(`IPC 事件契约求差失败:后端 emit ${backendEvents.size} 个,前端订阅 ${frontendEvents.size} 个`);
  if (backendOnly.length) {
    console.error(`  后端 emit 但前端未订阅(${backendOnly.length}): ${backendOnly.join(", ")}`);
    console.error("  事件发出没人听——通常是后端改名没同步前端,或新事件漏接线。请同步 ui/*.js 的 on() 订阅或补接线。");
  }
  if (frontendOnly.length) {
    console.error(`  前端订阅但后端无 emit(${frontendOnly.length}): ${frontendOnly.join(", ")}`);
    console.error("  死订阅——事件被移除/改名,前端在等一个永远不会来的事件。请同步后端 emit 或删掉前端订阅。");
  }
  process.exit(1);
}

console.log(
  `IPC 事件契约求差通过:后端 emit ${backendEvents.size} 个 = 前端订阅 ${frontendEvents.size} 个,差集为空。` +
    `事件名(${[...backendEvents].sort().join(", ")})`
);
