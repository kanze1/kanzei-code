// R-264 批3:恢复 defer 工具到 01-core(export),并批量把顶层单行 `$()` 调用包进 defer。
// defer 是 ESM 下的正确模式(module 本就 deferred,DOMContentLoaded 后执行),
// classic 下 readyState 非 loading → 立即执行,no-op。留在生产代码是正解。
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

// 2) 批量包裹:每个文件顶层单行 `$("...").xxx(...);` 调用 → defer(() => ...)。
// 只处理单行(以 ; 或 }); 结尾且不含未闭合花括号的),跨行需手动。
const files = fs.readdirSync(uiDir).filter((f) => f.endsWith(".js"));
for (const f of files) {
  if (f === "01-core.js") continue;
  const p = `${uiDir}/${f}`;
  const src = fs.readFileSync(p, "utf8");
  const lines = src.split(/\r?\n/);
  let changed = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (/^\s/.test(line)) continue;
    if (/^(import|export|const|let|var|function|async function|class|\/\/|\/\*|\*|})/.test(line)) continue;
    if (/^defer\(/.test(line)) continue;
    // 单行顶层 `$("...").xxx(...)` 以 ; 结尾,花括号闭合。
    if (/^\$\("[^"]+"\)\.[\w$]+\(.*\);$/.test(line) && !/=>\s*\{[^}]*$/.test(line)) {
      lines[i] = `defer(() => ${line.slice(0, -1)});`;
      changed++;
    }
  }
  if (changed > 0) {
    // 补 import defer(脚本生成的 defer 引用,依赖图扫描不到)。
    const needImport = `import { defer } from "./01-core.js";\n`;
    const joined = lines.join("\n");
    const updated = joined.startsWith("import") ? needImport + joined : needImport + joined;
    fs.writeFileSync(p, updated);
    console.log(`${f}: defer'd ${changed} + import`);
  }
}
console.log("done");
