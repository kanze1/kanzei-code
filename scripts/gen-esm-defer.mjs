// R-264 批3:批量把顶层 `$("...").xxx(...)` / `on("...")` 调用(单行与跨行)包进 defer。
// 幂等:已 defer 包裹的跳过。跨行用花括号配平识别语句结束。
// defer 是 ESM 下的正确模式(module deferred),classic 下 readyState 非 loading 立即执行。
import fs from "node:fs";

const uiDir = "crates/kanzei-app/ui";

// 0) 特殊修复点(幂等):gen-esm-migrate/fixheaders 从 HEAD 生成会覆盖手动修复,
//    这些修复必须每次跑 defer 时自动重新应用,否则重跑迁移即回退。
// 02-i18n languageSelect:求值期 `$`(01-core)在环内 TDZ,须 let + defer 初始化。
{
  const p = `${uiDir}/02-i18n.js`;
  let src = fs.readFileSync(p, "utf8");
  const need = 'export let languageSelect = null;\ndefer(() => { languageSelect = $("language-select"); });';
  if (src.includes('export const languageSelect = $("language-select");')) {
    src = src.replace('export const languageSelect = $("language-select");', need);
    fs.writeFileSync(p, src);
    console.log("02-i18n: languageSelect const→let+defer 修复(幂等)");
  } else if (!src.includes('export let languageSelect = null;')) {
    // 兜底:若 const 变体已不在且 let 也不在,追加。
    src = src.replace('export const LANGUAGE_PREFERENCES = new Set', `export let languageSelect = null;\ndefer(() => { languageSelect = $("language-select"); });\nexport const LANGUAGE_PREFERENCES = new Set`);
    fs.writeFileSync(p, src);
    console.log("02-i18n: languageSelect 修复追加");
  }
  // languageSelect.value 初始化并入 defer(否则 languageSelect 为 null 时求值报错)。
  const valueInit = 'languageSelect.value = normalizeLanguagePreference(localStorage.getItem("kz-language"));';
  if (src.includes(valueInit + "\ndefer(() => {\n  languageSelect.addEventListener")) {
    src = src.replace(
      valueInit + "\ndefer(() => {\n  languageSelect.addEventListener",
      'defer(() => {\n  ' + valueInit + "\n  languageSelect.addEventListener"
    );
    fs.writeFileSync(p, src);
    console.log("02-i18n: languageSelect.value 并入 defer(幂等)");
  }
}
// 09-sessions:renderProjects 用 setter(setCurrentProject/setActiveProcessId/setActiveSessionId)
// 但 import 头重建会丢——幂等补 import。
{
  const p = `${uiDir}/09-sessions.js`;
  let src = fs.readFileSync(p, "utf8");
  // renderProjects 的直接赋值改 setter(迁移重跑会覆盖)。先改赋值,再补 import——
  // 否则 import 补齐时看不到 setter 调用,漏补。
  let changed2 = false;
  if (src.includes("  currentProject = prefs.current;")) {
    src = src.replace("  currentProject = prefs.current;", "  setCurrentProject(prefs.current);");
    changed2 = true;
  }
  const needSetters = ["setCurrentProject", "setActiveProcessId", "setActiveSessionId"];
  const missing = needSetters.filter((s) => src.includes(`${s}(`) && !src.includes(`${s},\n`) && !src.includes(`${s}\n`));
  // renderProjects 里 activeProcessId/activeSessionId 直接赋 null → setter。
  if (src.includes("    activeProcessId = null;") && src.includes("    activeSessionId = null;")) {
    src = src.replace("    activeProcessId = null;\n    activeSessionId = null;", "    setActiveProcessId(null);\n    setActiveSessionId(null);");
    changed2 = true;
  }
  if (missing.length > 0) {
    // 在 03-shell import 块内追加缺失 setter(在 `running,` 后插入)。
    const insert = missing.map((s) => `  ${s},`).join("\n");
    src = src.replace("  running,\n", `  running,\n${insert}\n`);
    fs.writeFileSync(p, src);
    console.log(`09-sessions: 补 setter import ${missing.join(", ")}`);
  } else if (changed2) {
    fs.writeFileSync(p, src);
    console.log("09-sessions: currentProject 赋值→setter(幂等)");
  }
}
// 14-docs-actions:documentsKind 跨模块写 → setDocumentsKind(12-docs-pages 提供)。
{
  const p = `${uiDir}/14-docs-actions.js`;
  let src = fs.readFileSync(p, "utf8");
  let changed = false;
  for (const [from, to] of [
    ['documentsKind = "req"', 'setDocumentsKind("req")'],
    ['documentsKind = "defect"', 'setDocumentsKind("defect")'],
    ['documentsKind = "both"', 'setDocumentsKind("both")'],
  ]) {
    if (src.includes(from)) {
      src = src.replaceAll(from, to);
      changed = true;
    }
  }
  if (changed) {
    if (!src.includes("setDocumentsKind,") && !src.includes("setDocumentsKind\n")) {
      src = src.replace("  selectWorkspaceProject,\n", "  selectWorkspaceProject,\n  setDocumentsKind,\n");
    }
    fs.writeFileSync(p, src);
    console.log("14-docs-actions: documentsKind → setDocumentsKind(幂等)");
  }
}
// 12-docs-pages:documentsKind/dependencyViewOpen 的 setter 定义(迁移重跑会覆盖)。
{
  const p = `${uiDir}/12-docs-pages.js`;
  let src = fs.readFileSync(p, "utf8");
  let changed = false;
  if (/^export let documentsKind = "req";/m.test(src) && !src.includes("export function setDocumentsKind")) {
    src = src.replace('export let documentsKind = "req";', 'export let documentsKind = "req";\nexport function setDocumentsKind(v) { documentsKind = v; }');
    changed = true;
  }
  if (/^export let dependencyViewOpen = false;/m.test(src) && !src.includes("export function setDependencyViewOpen")) {
    src = src.replace('export let dependencyViewOpen = false;', 'export let dependencyViewOpen = false;\nexport function setDependencyViewOpen(v) { dependencyViewOpen = v; }');
    changed = true;
  }
  if (changed) {
    fs.writeFileSync(p, src);
    console.log("12-docs-pages: 补 setter 定义(幂等)");
  }
}
// 03-shell:currentProject/activeProcessId/activeSessionId 的 setter 定义(迁移重跑会覆盖)。
{
  const p = `${uiDir}/03-shell.js`;
  let src = fs.readFileSync(p, "utf8");
  let changed = false;
  if (/^export let currentProject = null;/m.test(src) && !src.includes("export function setCurrentProject")) {
    src = src.replace('export let currentProject = null;', 'export let currentProject = null;\nexport function setCurrentProject(v) { currentProject = v; }');
    changed = true;
  }
  if (/^export let activeProcessId = null;/m.test(src) && !src.includes("export function setActiveProcessId")) {
    src = src.replace('export let activeProcessId = null;', 'export let activeProcessId = null;\nexport function setActiveProcessId(v) { activeProcessId = v; }');
    changed = true;
  }
  if (/^export let activeSessionId = null;/m.test(src) && !src.includes("export function setActiveSessionId")) {
    src = src.replace('export let activeSessionId = null;', 'export let activeSessionId = null;\nexport function setActiveSessionId(v) { activeSessionId = v; }');
    changed = true;
  }
  if (changed) {
    fs.writeFileSync(p, src);
    console.log("03-shell: 补 setter 定义(幂等)");
  }
}

// 1) 恢复 defer 到 01-core(若缺失)。
const corePath = `${uiDir}/01-core.js`;
let core = fs.readFileSync(corePath, "utf8");
if (!core.includes("export function defer")) {
  const anchor = "export const $ = (id) => document.getElementById(id);";
  const deferCode = `
// R-264 ESM:延迟执行——把「模块求值期跨模块顶层调用」推迟到全部模块求值完成
// (DOMContentLoaded)。classic 下 DOM 已就绪(readyState 非 loading)立即执行,no-op;
// ESM 下循环依赖的求值顺序不保证提供方先就绪,直接顶层调用会 TDZ。与浏览器
// \`<script type="module">\` 的 deferred 语义一致。
export function defer(fn) {
  if (typeof document !== "undefined" && document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", fn);
  } else {
    fn();
  }
}`;
  core = core.replace(anchor, anchor + deferCode);
  fs.writeFileSync(corePath, core);
  console.log("01-core: defer restored");
}

// 2) 批量包裹:识别顶层调用起点(行首无缩进的 `$("...").` 或 `on(`),
//    收集到语句结束(花括号配平 + 分号/`});`),包进 defer(() => {...})。
const files = fs.readdirSync(uiDir).filter((f) => f.endsWith(".js"));
for (const f of files) {
  if (f === "01-core.js") continue;
  const p = `${uiDir}/${f}`;
  const src = fs.readFileSync(p, "utf8");
  const lines = src.split(/\r?\n/);
  const out = [];
  let changed = 0;
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const isTopCall =
      !/^\s/.test(line) &&
      !/^(import|export|const|let|var|function|async function|class|\/\/|\/\*|\*|})/.test(line) &&
      !/^defer\(/.test(line) &&
      // 顶层裸函数调用:setupResize(...)/$("x")?.addEventListener(...)/document.add.../syncActivityPanel()
      // 或顶层 for/if/裸块:块内 $()/跨模块引用是求值期执行。
      (/^[A-Za-z_$][\w$]*(\.[\w$]+)*\(/.test(line) ||
       /^for\s*\(/.test(line) ||
       /^if\s*\(/.test(line) ||
       /^\{/.test(line) ||
       /^\(async\s*\(\)\s*=>/.test(line) ||
       /^(void|await)\s+[A-Za-z_$][\w$]*\s*\(/.test(line));
    if (isTopCall) {
      // 收集本语句(从该行到闭合:括号配平)。
      let depth = 0;
      let started = false;
      const block = [];
      let j = i;
      let done = false;
      while (j < lines.length) {
        const l = lines[j];
        block.push(l);
        for (const ch of l) {
          if (ch === "(" || ch === "{") { depth += 1; started = true; }
          else if (ch === ")" || ch === "}") depth -= 1;
        }
        j += 1;
        if (started && depth <= 0) { done = true; break; }
        if (depth < 0) break; // 括号过度闭合:异常,不包裹
      }
      if (done) {
        const stmt = block.join("\n").replace(/;\s*$/, "");
        out.push(`defer(() => {\n  ${stmt.replace(/\n/g, "\n  ")};\n});`);
        changed += 1;
        i = j;
        continue;
      }
    }
    out.push(line);
    i += 1;
  }
  const joined = out.join("\n");
  const hasDeferImport = /import\s*\{[^}]*\bdefer\b[^}]*\}\s*from\s*["']\.\/01-core\.js["']/.test(joined) || /import\s*\{ defer \} from "\.\/01-core\.js"/.test(joined);
  const needImport = hasDeferImport ? "" : `import { defer } from "./01-core.js";\n`;
  const finalText = joined.startsWith("import") ? needImport + joined : needImport + joined;
  if (joined !== src || (!hasDeferImport && needImport)) {
    fs.writeFileSync(p, finalText);
    console.log(`${f}: defer'd ${changed}`);
  }
}
console.log("done");
