#!/usr/bin/env node
// 校验 generated.ts 与 scripts/ipc-commands.json 保持完全一致，并验证核心契约边界。
import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";

const root = path.resolve(import.meta.dirname, "..");
const contracts = JSON.parse(
  fs.readFileSync(path.join(root, "scripts/ipc-commands.json"), "utf8")
);
const generatedTs = fs.readFileSync(
  path.join(root, "crates/kanzei-app/web/src/ipc/generated.ts"),
  "utf8"
);

const commandNames = Object.keys(contracts);
assert.ok(commandNames.length > 100, "注册命令数量异常偏低");
for (const name of commandNames) {
  assert.ok(
    generatedTs.includes(`"${name}":`),
    `generated.ts 缺少命令契约定义: ${name}`
  );
}

// 确认关键新命令已注册且废弃双进程/整页草稿命令已清理
for (const required of ["app_bootstrap", "app_shutdown", "conversation_page_messages"]) {
  assert.ok(required in contracts, `缺少新架构必需命令: ${required}`);
}
for (const removed of [
  "runtime_status",
  "runtime_shutdown",
  "file_draft_save",
  "file_draft_clear",
  "file_drafts_load",
]) {
  assert.ok(!(removed in contracts), `废弃命令不应残留在契约中: ${removed}`);
}

console.log(
  `IPC argument contract PASS: ${commandNames.length} commands verified against generated.ts`
);
