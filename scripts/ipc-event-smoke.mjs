#!/usr/bin/env node
// IPC 事件与命令契约求差：扫描 Rust 后端 emit 与新前端 web/src 的 listenEvent/invoke。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..");

function walk(dir, exts, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(p, exts, out);
    else if (exts.some((ext) => entry.name.endsWith(ext))) out.push(p);
  }
  return out;
}

const BACKEND_RE = /(?:\.emit|emit_event|emit_delta)\(\s*"(kz:[a-z-]+)"/g;
const backendEvents = new Set();
for (const file of walk(path.join(root, "crates/kanzei-app/src"), [".rs"])) {
  const src = fs.readFileSync(file, "utf8");
  for (const m of src.matchAll(BACKEND_RE)) backendEvents.add(m[1]);
}

const FRONTEND_RE = /listenEvent(?:<[^()]*>)?\(\s*"(kz:[a-z-]+)"/g;
const frontendEvents = new Set();
const webSrc = path.join(root, "crates/kanzei-app/web/src");
for (const file of walk(webSrc, [".ts", ".tsx"])) {
  if (file.endsWith(".test.ts") || file.endsWith(".test.tsx")) continue;
  const src = fs.readFileSync(file, "utf8");
  for (const m of src.matchAll(FRONTEND_RE)) frontendEvents.add(m[1]);
}

// 命令名求差：前端 invoke("cmd") 必须全部在 scripts/ipc-commands.json 注册。
const registry = JSON.parse(
  fs.readFileSync(path.join(root, "crates/kanzei-app/src/command_registry.json"), "utf8")
);
const contracts = JSON.parse(
  fs.readFileSync(path.join(root, "scripts/ipc-commands.json"), "utf8")
);
const registered = new Set(Object.keys(contracts));
const registeredPaths = new Set(Object.values(contracts).map((c) => c.path));
if (
  registry.length !== registeredPaths.size ||
  registry.some((entry) => !registeredPaths.has(entry.path))
) {
  throw new Error("IPC registry and generated contract differ; run node scripts/sync-ipc-contract.mjs");
}

const invoked = new Map();
for (const file of walk(webSrc, [".ts", ".tsx"])) {
  if (file.endsWith(".test.ts") || file.endsWith(".test.tsx")) continue;
  const src = fs.readFileSync(file, "utf8");
  const rel = path.relative(webSrc, file);
  for (const m of src.matchAll(/\binvoke(?:<[^()]*>)?\(\s*"([a-z][a-z0-9_]*)"/g)) {
    if (!invoked.has(m[1])) invoked.set(m[1], new Set());
    invoked.get(m[1]).add(rel);
  }
}

const missingCommands = [];
for (const [cmd, files] of invoked) {
  if (!registered.has(cmd)) missingCommands.push(`${cmd}(${[...files].join(", ")})`);
}

if (registered.size < 50 || invoked.size < 30) {
  console.error(`IPC 命令求差拒绝异常低计数: 注册 ${registered.size}, 前端调用 ${invoked.size}`);
  process.exit(1);
}
if (missingCommands.length > 0) {
  console.error(`IPC 命令求差失败: 前端调用了未注册命令: ${missingCommands.join(", ")}`);
  process.exit(1);
}
console.log(
  `IPC 命令求差通过: 前端 invoke ${invoked.size} 个命令均已注册（后端共 ${registered.size} 个）`
);

// 前端不得订阅后端永不发射的死事件；后端面向前端的实时事件必须有订阅覆盖。
const frontendOnly = [...frontendEvents].filter((e) => !backendEvents.has(e)).sort();
if (frontendEvents.size < 10) {
  console.error(`IPC 事件订阅过少: ${frontendEvents.size}`);
  process.exit(1);
}
if (frontendOnly.length > 0) {
  console.error(`IPC 事件契约失败: 前端订阅了后端未发射的事件: ${frontendOnly.join(", ")}`);
  process.exit(1);
}
console.log(
  `IPC 事件契约通过: 后端 emit ${backendEvents.size} 个, 前端已订阅 ${frontendEvents.size} 个, 无死订阅`
);
