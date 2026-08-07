import { readFileSync } from "node:fs";

const source = readFileSync("crates/kanzei-app/ui/main.js", "utf8");
const required = [
  ["I18N_EN", "英文资源"],
  ["function t(key)", "动态翻译入口"],
  ["function applyLanguage()", "静态节点翻译入口"],
  ["运行中\": \"Running", "运行中翻译键"],
  ["运行完成\": \"Run completed", "运行完成翻译键"],
  ["运行失败\": \"Run failed", "运行失败翻译键"],
  ["运行已停止\": \"Run stopped", "运行已停止翻译键"],
  ["status-mode\").textContent = isRunning ? t(\"运行中\")", "动态状态使用翻译入口"],
];
const missing = required.filter(([needle]) => !source.includes(needle));
if (missing.length) {
  throw new Error(`UI i18n 静态契约缺失: ${missing.map(([, label]) => label).join(", ")}`);
}
console.log(`UI i18n 静态冒烟通过：${required.length} 项资源与动态入口契约已覆盖`);
