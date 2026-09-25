// R-142:前端最低配 ESLint 冒烟：ui/*.js + scripts/*.mjs 经 no-undef 检查零错误。
// 运行时模块之间通过真实 ESM import/export 连接，不再维护跨文件 globals 清单。
// 与 ui-a11y/ui-i18n/ui-markdown/ui-runtime 冒烟并列,verify.ps1 发布门禁一并执行。
import { ESLint } from "eslint";
import { readFileSync, readdirSync } from "node:fs";
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
// ② ESM 回归守卫(UI-0926):ui/*.js 之间只准走 import/export。读 globalThis.X / window.X 时,
// 若 X 是某个 ui 模块的 ESM 导出、却从没被挂到 globalThis 上,它在真机浏览器里恒为 undefined——
// `typeof globalThis.X === "function"` 的兜底把调用静默吞掉,一个报错都没有。ui-runtime 冒烟的
// vm sandbox 会把**全部**导出复制成全局,于是这类死调用在冒烟里永远是绿的:01-core 的逐事件
// 线路投影、后台线轮末续跑与停止时取消续跑定时器,就这样在真机上从未执行过。
// 真正经 Object.assign(globalThis, {…}) / globalThis.X = … 发布的名字自动放行(兼容桥)。
const UI_DIR = path.resolve(import.meta.dirname, "../crates/kanzei-app/ui");
const stripComments = (src) => src
  .replace(/\/\*[\s\S]*?\*\//g, "")
  .replace(/(^|[^:\\])\/\/[^\n]*/g, "$1");
const HOST = "(?:globalThis|window)";
const ASSIGN_AHEAD = "\\s*(?:\\?\\?|\\|\\||&&)?=(?!=)";
function exportedNames(src) {
  const names = new Set();
  for (const [, name] of src.matchAll(/^export\s+(?:async\s+)?(?:function\*?|class|const|let|var)\s+([A-Za-z_$][\w$]*)/gm)) names.add(name);
  for (const [, body] of src.matchAll(/^export\s+(?:const|let|var)\s*\{([^}]*)\}/gm)) {
    for (const part of body.split(",")) if (part.trim()) names.add(part.split(":").pop().trim());
  }
  for (const [, body] of src.matchAll(/^export\s*\{([^}]*)\}/gm)) {
    for (const part of body.split(",")) if (part.trim()) names.add(part.trim().split(/\s+as\s+/).pop().trim());
  }
  return names;
}
function publishedNames(src) {
  const names = new Set();
  for (const [, body] of src.matchAll(new RegExp(`Object\\.assign\\(\\s*${HOST}\\s*,\\s*\\{([^}]*)\\}`, "g"))) {
    for (const part of body.split(",")) if (part.trim()) names.add(part.split(":")[0].trim());
  }
  for (const [, name] of src.matchAll(new RegExp(`\\b${HOST}\\.([A-Za-z_$][\\w$]*)(?=${ASSIGN_AHEAD})`, "g"))) names.add(name);
  return names;
}
function hostReads(src) {
  const reads = [];
  for (const match of src.matchAll(new RegExp(`\\b${HOST}\\.([A-Za-z_$][\\w$]*)(?!${ASSIGN_AHEAD})`, "g"))) {
    reads.push({ name: match[1], line: src.slice(0, match.index).split("\n").length });
  }
  return reads;
}
// 判据自检:正则一旦与写法脱节,守卫会静默变成恒绿。用回归前的真实形态喂一遍。
{
  const sample = [
    "export function refreshParallelTaskProjection(id) {}",
    "if (typeof globalThis.refreshParallelTaskProjection === \"function\") globalThis.refreshParallelTaskProjection(id);",
    "Object.assign(globalThis, { log, t: translate });",
    "globalThis.__kzBridge = bridge;",
    "const x = globalThis.log; const y = globalThis.__kzBridge?.get(1);",
  ].join("\n");
  const exported = exportedNames(sample);
  const published = publishedNames(sample);
  const dead = hostReads(sample).filter(({ name }) => exported.has(name) && !published.has(name));
  if (dead.length !== 2 || !published.has("log") || !published.has("t") || !published.has("__kzBridge")) {
    console.error(`ESM 回归守卫自检失败:判据与写法脱节(dead=${JSON.stringify(dead)} published=${[...published]})`);
    process.exit(1);
  }
}
const uiSources = readdirSync(UI_DIR)
  .filter((name) => name.endsWith(".js"))
  .sort()
  .map((name) => ({ name, src: stripComments(readFileSync(path.join(UI_DIR, name), "utf8")) }));
const allExported = new Set();
const allPublished = new Set();
for (const { src } of uiSources) {
  for (const name of exportedNames(src)) allExported.add(name);
  for (const name of publishedNames(src)) allPublished.add(name);
}
if (allExported.size < 200 || !allPublished.has("log") || !allPublished.has("t")) {
  console.error(`ESM 回归守卫取样异常:导出 ${allExported.size} 个、已发布 ${[...allPublished].join(",")}——清单可能静默退化`);
  process.exit(1);
}
const deadCalls = [];
for (const { name: file, src } of uiSources) {
  for (const { name, line } of hostReads(src)) {
    if (allExported.has(name) && !allPublished.has(name)) {
      deadCalls.push(`crates/kanzei-app/ui/${file}:${line} 读 globalThis/window.${name}——它是 ESM 导出却从未挂到全局,真机恒为 undefined;改成直接 import`);
    }
  }
}
if (deadCalls.length) {
  console.error(`UI ESM 回归守卫失败(${deadCalls.length} 处死调用):`);
  for (const e of deadCalls) console.error(` - ${e}`);
  process.exit(1);
}

// ③ import 绑定只读:给导入的 let 绑定直接赋值(`sidebarCollapsed = false`)在 ESM 里是 TypeError,
// 走到那一行才炸。只在 ui/*.js 上加开 no-import-assign;提供方要改值就导出 setter。
const importAssignLint = new ESLint({ overrideConfig: { rules: { "no-import-assign": "error" } } });
const importAssignErrors = [];
for (const result of await importAssignLint.lintFiles(["crates/kanzei-app/ui/*.js"])) {
  for (const message of result.messages) {
    if (message.ruleId === "no-import-assign") {
      importAssignErrors.push(`${path.relative(process.cwd(), result.filePath)}:${message.line}:${message.column} ${message.message}`);
    }
  }
}
if (importAssignErrors.length) {
  console.error(`UI ESM 回归守卫失败(${importAssignErrors.length} 处给 import 绑定赋值):`);
  for (const e of importAssignErrors) console.error(` - ${e}`);
  process.exit(1);
}
console.log(`UI ESM 回归守卫通过:${allExported.size} 个导出无 globalThis 死调用(兼容桥 ${allPublished.size} 个),import 绑定无赋值`);

console.log(`UI ESLint 冒烟通过:${results.length} 个文件 no-undef 零错误,模块 import/export 解析正常`);
