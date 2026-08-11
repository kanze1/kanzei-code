#!/usr/bin/env node
// R-142:前端最低配 ESLint 冒烟。两条断言:
//  ①ui/*.js + scripts/*.mjs 经 no-undef 检查零错误(ESLint Node API,非子进程);
//  ②ui-lint-globals.json 与源码顶层声明同步(新增顶层函数/常量后重跑 gen 脚本)。
// 与 ui-a11y/ui-i18n/ui-markdown/ui-runtime 冒烟并列,verify.ps1 发布门禁一并执行。
import { ESLint } from "eslint";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));

// ①no-undef 检查
const eslint = new ESLint();
const results = await eslint.lintFiles(["crates/kanzei-app/ui/*.js", "scripts/*.mjs"]);
const errors = [];
for (const result of results) {
  for (const message of result.messages) {
    if (message.severity === 2) {
      errors.push(`${path.relative(process.cwd(), result.filePath)}:${message.line}:${message.column} ${message.message} (${message.ruleId})`);
    }
  }
}
if (errors.length) {
  console.error(`UI ESLint 冒烟失败(${errors.length} 处 no-undef):`);
  for (const e of errors) console.error(` - ${e}`);
  process.exit(1);
}

// ②globals 清单同步检查(新增顶层声明而未重跑 gen 时,这里红)
try {
  execFileSync(process.execPath, [path.join(here, "gen-ui-lint-globals.mjs"), "--check"], { encoding: "utf8" });
} catch (err) {
  console.error(err.stdout || err.message);
  process.exit(1);
}

console.log(
  `UI ESLint 冒烟通过:${results.length} 个文件 no-undef 零错误,` +
  `globals 清单与源码同步(${JSON.parse(fs.readFileSync(path.join(here, "ui-lint-globals.json"), "utf8")).length} 个标识符)`
);
