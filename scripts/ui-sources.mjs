// B1 共享助手(R-264):UI 源码清单来自 ui/ 目录本身，按文件名排序；不从 index.html
// 解析清单。ESM 单入口时 HTML 只有一个 src，继续从 HTML 取清单会让 joined 静默缩水，
// a11y/i18n/markdown 的「不存在」断言因此可能恒绿。html 仍返回给需要检查真实页面
// 标记的冒烟脚本；scriptSrcs 只是兼容字段，代表源码目录的加载顺序基线。
// vendor/ 子目录不会被纳入：这里只枚举 UI_DIR 的直接 .js 文件。
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const UI_DIR = resolve(root, "crates/kanzei-app/ui");
const MIN_UI_FILES = 24;

export function loadUiSources() {
  const html = readFileSync(resolve(UI_DIR, "index.html"), "utf8");
  const scriptSrcs = readdirSync(UI_DIR, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".js"))
    .map((entry) => entry.name)
    .sort((a, b) => a.localeCompare(b));
  if (scriptSrcs.length < MIN_UI_FILES) {
    throw new Error(
      `ui/ 只有 ${scriptSrcs.length} 个直接 UI script(下限 ${MIN_UI_FILES})——源码清单可能静默退化`,
    );
  }
  const sources = scriptSrcs.map((src) => readFileSync(resolve(UI_DIR, src), "utf8"));
  return { html, scriptSrcs, sources, joined: sources.join("\n") };
}
