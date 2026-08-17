// B0 共享助手(R-154):从 index.html 解析 <script src> 清单，严格按浏览器声明顺序读入 UI 脚本。
// 四个冒烟脚本共用，清单解析逻辑只有一份。
//
// D-498：不能用 readdir().sort() 模拟浏览器顺序。index.html 的 script 顺序才是
// classic/defer 脚本的实际加载契约；目录前缀只是一种命名习惯，不是执行真源。
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const UI_DIR = resolve(root, "crates/kanzei-app/ui");
const MIN_UI_FILES = 20;
const SCRIPT_SRC_RE = /<script\b[^>]*\bsrc="([^"]+)"[^>]*>/g;

export function loadUiSources() {
  const html = readFileSync(resolve(UI_DIR, "index.html"), "utf8");
  const scriptSrcs = [...html.matchAll(SCRIPT_SRC_RE)]
    .map((match) => match[1])
    .filter((src) => src.endsWith(".js") && !src.includes("://"));
  if (scriptSrcs.length < MIN_UI_FILES) {
    throw new Error(
      `index.html 只有 ${scriptSrcs.length} 个本地 UI script(下限 ${MIN_UI_FILES})——脚本顺序清单可能静默退化`,
    );
  }
  const sources = scriptSrcs.map((src) => {
    const file = resolve(UI_DIR, src);
    const content = readFileSync(file, "utf8");
    return content;
  });
  return { html, scriptSrcs, sources, joined: sources.join("\n") };
}
