// B0 共享助手(R-154):从 index.html 解析 <script src> 清单按序读入全部 ui 脚本。
// 四个冒烟脚本共用,清单解析逻辑只有一份。单文件形态(= 现 main.js)是清单的退化情形;
// 后续 R-154 批次把 main.js 拆成 18 个文件时,冒烟对文件形态透明,无需再改。
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");

export function loadUiSources() {
  const html = readFileSync(resolve(root, "crates/kanzei-app/ui/index.html"), "utf8");
  const scriptSrcs = [...html.matchAll(/<script\s+src="([^"]+)"[^>]*>/g)].map((m) => m[1]);
  if (scriptSrcs.length === 0) throw new Error("index.html 未解析到任何 <script src>,冒烟机制退化");
  const sources = scriptSrcs.map((src) => readFileSync(resolve(root, "crates/kanzei-app/ui", src), "utf8"));
  return { html, scriptSrcs, sources, joined: sources.join("\n") };
}
