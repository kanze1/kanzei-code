// Native ESM correctness: ESLint recommended rules and unused bindings for the UI.
// UI-0926 #9:ui/*.js 另加 no-restricted-syntax「弹层唯一写法」8 条(见 eslint.config.js),同样按 error 计;
// 浏览器检查由 verification-checks.json 单独登记、计时和报告。
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
  console.error(`UI ESLint 冒烟失败(${errors.length} 处 error):`);
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
if (allExported.size < 200 || !allPublished.has("log") || !allExported.has("t")) {
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

// ④ 恒真 typeof 守卫。`export function` 的绑定在 link 期就完成初始化,循环 import 也不例外,
// 所以 `typeof 导入名 === "function"` 恒为真;`export let/const` 落在 TDZ 时 typeof 直接抛
// ReferenceError,不会返回 "undefined"。两种写法都拦不住任何东西,只会让读代码的人以为这条
// 依赖边允许缺席——真缺了会当场炸,不会走兜底。2026-10-22 一次清掉 21 处,这条闸防止回流。
// 合法例外:提供方用 `export let` 导出且有 setter 能改绑成非函数(neuralFlowEmit)。
const importedBindings = (src) => {
  const map = new Map();
  for (const [, body, spec] of src.matchAll(/import\s*\{([\s\S]*?)\}\s*from\s*"([^"]+)"/g)) {
    const file = spec.startsWith("./") ? spec.slice(2) : "";
    if (!file || file.includes("/")) continue;
    for (const part of body.split(",")) {
      const text = part.trim();
      if (!text) continue;
      const [original, alias] = text.split(/\s+as\s+/).map((value) => value.trim());
      map.set(alias || original, file);
    }
  }
  return map;
};
// 提供方这一侧:函数声明是 link 期初始化的,其它都是求值期才落定;再导出跟一跳到源头。
function exportForms(src, providerName, depth = 0) {
  const forms = new Map();
  for (const [, async, kw, name] of src.matchAll(/^export\s+(async\s+)?(function\*?|class|const|let|var)\s+([A-Za-z_$][\w$]*)/gm))
    forms.set(name, kw.startsWith("function") ? "function" : "value");
  if (depth > 2) return forms;
  for (const [, body, spec] of src.matchAll(/^export\s*\{([^}]*)\}\s*from\s*"([^"]+)"/gm)) {
    const target = spec.startsWith("./") ? spec.slice(2) : "";
    if (!target || target.includes("/")) continue;
    for (const part of body.split(",")) {
      const text = part.trim();
      if (!text) continue;
      const [original, alias] = text.split(/\s+as\s+/).map((value) => value.trim());
      const upstream = sources.get(target);
      if (!upstream) continue;
      const form = exportForms(upstream, target, depth + 1).get(original);
      if (form) forms.set(alias || original, form);
    }
  }
  return forms;
}
const sources = new Map(uiSources.map(({ name, src }) => [name, src]));
const functionForms = new Map();
for (const [name, src] of sources) {
  for (const [exported, form] of exportForms(src, name)) if (form === "function") functionForms.set(`${name}:${exported}`, true);
}
{
  // 判据自检:与 ② 同理由——正则一脱节,闸就静默恒绿。
  const sample = "import { noteScroll, mutable } from \"./provider.js\";\nif (typeof noteScroll === \"function\") noteScroll();\nif (typeof mutable === \"function\") mutable();\n";
  const bindings = importedBindings(sample);
  const hits = [...bindings].filter(([name]) => name !== "mutable").map(([name]) => name);
  if (bindings.size !== 2 || !bindings.has("noteScroll") || hits.length !== 1) {
    console.error(`恒真守卫自检失败:判据与写法脱节(bindings=${[...bindings.keys()]})`);
    process.exit(1);
  }
}
const vacuous = [];
let inspected = 0;
for (const { name: file, src } of uiSources) {
  const bindings = importedBindings(src);
  for (const [name, provider] of bindings) {
    if (!functionForms.has(`${provider}:${name}`)) continue;
    inspected += 1;
    for (const match of src.matchAll(new RegExp(`typeof\\s+${name.replace(/\$/g, "\\$")}\\s*===?\\s*"(?:function|object)"|typeof\\s+${name.replace(/\$/g, "\\$")}\\s*!==?\\s*"undefined"`, "g"))) {
      vacuous.push(`crates/kanzei-app/ui/${file}:${src.slice(0, match.index).split("\n").length} ${match[0]}——${provider} 用 export function 导出 ${name},链接期就已初始化,该判断恒为真;直接调用`);
    }
  }
}
if (inspected < 150) {
  console.error(`恒真守卫取样异常:只解析到 ${inspected} 个指向函数导出的 import 绑定——清单可能静默退化`);
  process.exit(1);
}
if (vacuous.length) {
  console.error(`UI 恒真 typeof 守卫失败(${vacuous.length} 处):`);
  for (const e of vacuous) console.error(` - ${e}`);
  process.exit(1);
}
console.log(`UI 恒真 typeof 守卫通过:${inspected} 个指向 export function 的 import 绑定上无 typeof 兜底`);

// ⑤ 间距刻度(SP1)。style.css 的 :root 定义了 --sp-1..6 六档,但 D-380 只写「新代码用它」,
// 没有任何东西拦住老写法,于是 13 张样式表里同时漂着 20 多个间距值——刻度形同虚设,改一档密度
// 要全表搜。判据只认「已有同值 token 的 px 字面量」,刻度外的值(10px、14px…)先放过:
// 换成 token 会改变视觉,那是设计决定,不是机械迁移。2026-10-22 把同值字面量清到 0。
const SPACING_PROP = "(?:padding|margin|gap|row-gap|column-gap)(?:-(?:top|right|bottom|left|inline|block)(?:-(?:start|end))?|-(?:start|end))?";
const cssFiles = readdirSync(UI_DIR).filter((name) => name.endsWith(".css")).sort();
const spacingTokens = new Map(
  [...readFileSync(path.join(UI_DIR, "style.css"), "utf8").matchAll(/(--sp-\d)\s*:\s*(\d+(?:\.\d+)?px)/g)].map(([, token, px]) => [px, token]),
);
if (spacingTokens.size < 6) {
  console.error(`间距刻度取样异常:style.css 只解析到 ${spacingTokens.size} 档 --sp-*——判据可能静默退化`);
  process.exit(1);
}
const stripCssComments = (text) => text.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "));
function spacingLiterals(text, tokens) {
  const hits = [];
  for (const match of text.matchAll(new RegExp(`-?${SPACING_PROP}\\s*:\\s*([^;}\\n]*)`, "g"))) {
    const value = match[1];
    if (!value.trim() || value.includes("var(") || value.includes("calc(")) continue;
    for (const px of value.match(/(?<![-\d.])\d+(?:\.\d+)?px/g) ?? []) {
      if (tokens.has(px)) hits.push({ line: text.slice(0, match.index).split("\n").length, decl: match[0].trim(), px, token: tokens.get(px) });
    }
  }
  return hits;
}
const PROP_RE = SPACING_PROP;
{
  // 判据自检:与 ② 同理由,正则与写法一脱节闸门就静默恒绿。
  const sample = ":root { --sp-4: 8px; }\n.a { padding: 8px; margin-top: 3px; gap: var(--sp-4); }\n/* padding: 8px */";
  const found = spacingLiterals(stripCssComments(sample), new Map([["8px", "--sp-4"]]));
  if (found.length !== 1 || found[0].line !== 2) {
    console.error(`间距刻度自检失败:判据与写法脱节(${JSON.stringify(found)})`);
    process.exit(1);
  }
}
const spacingOffenders = [];
for (const name of cssFiles) {
  const text = stripCssComments(readFileSync(path.join(UI_DIR, name), "utf8"));
  for (const hit of spacingLiterals(text, spacingTokens)) {
    spacingOffenders.push(`crates/kanzei-app/ui/${name}:${hit.line} ${hit.decl}——${hit.px} 已有同值 token,改用 var(${hit.token})`);
  }
}
if (spacingOffenders.length) {
  console.error(`UI 间距刻度(SP1)失败(${spacingOffenders.length} 处已有同值 token 却写死 px):`);
  for (const e of spacingOffenders) console.error(` - ${e}`);
  process.exit(1);
}
console.log(`UI 间距刻度(SP1)通过:${cssFiles.length} 张样式表里 0 处写死已有同值 token 的间距`);

// ⑥ HD1:hidden 的唯一真源在 app.css 的 base 层。UA 的 [hidden]{display:none} 是 normal,
// 视图表里任何 display 规则都能盖掉,于是过去 26 处 `[hidden]{display:none}` 各自打补丁;
// 补丁之间还互相卷 important。base 层的 important 赢过 app/surface 的一切 important,一条顶全部。
const hiddenDupes = [];
for (const name of cssFiles) {
  if (name === "app.css") continue; // 唯一真源本身
  const text = stripCssComments(readFileSync(path.join(UI_DIR, name), "utf8"));
  for (const match of text.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = match[1].trim();
    if (!/\[hidden\]/.test(selector) || /:not\(\s*\[hidden\]/.test(selector)) continue;
    if (![...selector.split(/,(?![^(]*\))/).map((part) => part.trim()).filter(Boolean)].every((part) => /\[hidden\]/.test(part))) continue;
    const decls = match[2].split(";").map((value) => value.trim()).filter(Boolean);
    if (decls.length && decls.every((decl) => /^display:\s*none(\s*!important)?$/.test(decl))) {
      hiddenDupes.push(`crates/kanzei-app/ui/${name}:${text.slice(0, match.index).split("\n").length} ${selector.replace(/\s+/g, " ")} { ${decls.join("; ")} }——app.css 的 base 层已有 [hidden] 的 display:none!important,整条删掉`);
    }
  }
}
if (hiddenDupes.length) {
  console.error(`UI hidden 唯一真源(HD1)失败(${hiddenDupes.length} 处重复声明):`);
  for (const e of hiddenDupes) console.error(` - ${e}`);
  process.exit(1);
}
console.log(`UI hidden 唯一真源(HD1)通过:[hidden] 的隐藏语义只在 app.css base 层声明一次`);

console.log(`UI ESLint 冒烟通过:${results.length} 个文件通过已配置的 ESLint 正确性与弹层规则,模块 import/export 解析正常`);
