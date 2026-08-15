// R-264 批3:批量把顶层 `$("...").xxx(...)` / `on("...")` 调用(单行与跨行)包进 defer。
// 幂等:已 defer 包裹的跳过。跨行用花括号配平识别语句结束。
// defer 是 ESM 下的正确模式(module deferred),classic 下 readyState 非 loading 立即执行。
import fs from "node:fs";

const uiDir = "crates/kanzei-app/ui";

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
