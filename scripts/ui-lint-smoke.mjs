// R-142:前端最低配 ESLint 冒烟：ui/*.js + scripts/*.mjs 经 no-undef 检查零错误。
// 运行时模块之间通过真实 ESM import/export 连接，不再维护跨文件 globals 清单。
// 与 ui-a11y/ui-i18n/ui-markdown/ui-runtime 冒烟并列,verify.ps1 发布门禁一并执行。
import { ESLint } from "eslint";
import path from "node:path";

// ①no-undef 检查
const eslint = new ESLint();
const results = await eslint.lintFiles([
  "crates/kanzei-app/ui/*.js",
  "crates/kanzei-app/mobile-pwa/*.js", // R-292:mobile-pwa 入 ESLint 门禁
  "scripts/*.mjs",
]);
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

console.log(`UI ESLint 冒烟通过:${results.length} 个文件 no-undef 零错误,模块 import/export 解析正常`);
