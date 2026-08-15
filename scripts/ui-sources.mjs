// B0 共享助手(R-154):从 index.html 解析 <script src> 清单按序读入全部 ui 脚本。
// 四个冒烟脚本共用,清单解析逻辑只有一份。
//
// R-264 B1(ESM 迁移前置):不再从 HTML 取清单——ESM 单入口下 index.html 只剩一个
// src,继续解析 HTML 会让 loadUiSources().joined 只剩入口内容,而 a11y/i18n/markdown
// 三个冒烟对 joined 做正则/includes 断言:断言"存在"的红一片,断言"不存在"的
// (doesNotMatch/!includes/missingKeys 为空)全部恒绿——静默失效陷阱(设计文档 §二 B1)。
// 正确改法:遍历 ui/*.js 全目录并按文件名排序;**并加文件数下限断言(≥20)**,
// 防止清单再次静默退化(文件被误删/改名时冒烟不再"看着绿")。
import { readdirSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const UI_DIR = resolve(root, "crates/kanzei-app/ui");

// 文件数下限:当前 21 个 ui/*.js(R-154 拆分后稳定)。低于此数说明清单解析
// 静默退化(目录被误清/迁移时漏文件),直接抛错让冒烟红,而不是假装绿。
const MIN_UI_FILES = 20;

export function loadUiSources() {
  const html = readFileSync(resolve(UI_DIR, "index.html"), "utf8");
  const files = readdirSync(UI_DIR)
    .filter((name) => name.endsWith(".js") && name !== "main.js")
    .sort();
  if (files.length < MIN_UI_FILES) {
    throw new Error(
      `ui/*.js 目录只有 ${files.length} 个文件(下限 ${MIN_UI_FILES})——清单解析静默退化,冒烟机制失效`,
    );
  }
  const sources = files.map((name) => readFileSync(resolve(UI_DIR, name), "utf8"));
  // scriptSrcs 保留为相对文件名清单(与旧签名兼容,部分冒烟按文件名索引)。
  return { html, scriptSrcs: files, sources, joined: sources.join("\n") };
}
