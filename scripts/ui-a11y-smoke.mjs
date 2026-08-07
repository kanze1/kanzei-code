import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const html = await readFile(resolve(root, "crates/kanzei-app/ui/index.html"), "utf8");
const js = await readFile(resolve(root, "crates/kanzei-app/ui/main.js"), "utf8");
const css = await readFile(resolve(root, "crates/kanzei-app/ui/style.css"), "utf8");

const static_icon_buttons = [...html.matchAll(/<button[^>]*class="icon-btn"[^>]*>/g)];
assert.ok(static_icon_buttons.length > 0, "未发现静态 icon-btn");
assert.equal(
  static_icon_buttons.filter(([tag]) => !tag.includes("aria-label=")).length,
  0,
  "静态 icon-btn 必须有 aria-label"
);

for (const selector of ["activity-item", "sidebar-toggle", "auto-continue", "auto-allow"]) {
  assert.ok(html.includes(`id="${selector}"`) || html.includes(`class="${selector}`), `缺少核心控件 ${selector}`);
}
assert.match(js, /activity-item[\s\S]*aria-current/);
assert.match(js, /project-item[\s\S]*item\.click\(\)/);
assert.match(js, /doc-row[\s\S]*aria-expanded/);
assert.match(js, /workspace-card[\s\S]*card\.click\(\)/);
assert.match(js, /remove\.setAttribute\("aria-label"/);
assert.match(js, /rename\.setAttribute\("aria-label"/);
assert.match(css, /#auto-allow-wrap input, #auto-continue-wrap input\s*\{[\s\S]*opacity: 0/);
assert.doesNotMatch(css, /#auto-allow-wrap input, #auto-continue-wrap input\s*\{\s*display:\s*none/);
assert.match(css, /:focus-visible/);
assert.equal((js.match(/function reportError\(/g) || []).length, 1, "reportError 只能有一个定义");
assert.match(js, /function toastError\(text, options = \{\}\) \{\s*reportPersistentError\(text, options\);/);
assert.match(js, /function reportPersistentError\([\s\S]*?\$\("log-panel"\)\.classList\.remove\("hidden"\)/);
assert.match(css, /@media \(max-width: 1400px\)[\s\S]*#todo-panel, #bg-panel[\s\S]*position: absolute/);
assert.match(css, /#todo-panel:not\(\.hidden\) ~ #bg-panel:not\(\.hidden\)/);
assert.match(css, /#app \{[^}]*position: relative/);
assert.match(css, /\.resize-handle \{ position: fixed/);
assert.match(js, /handle\.setAttribute\("role", "separator"\)/);
assert.match(js, /handle\.addEventListener\("keydown"/);
assert.match(js, /function hideContextDetail\(\)/);
assert.match(js, /function toggleContextDetail\(\)/);
assert.match(js, /event\.key === "Escape"/);
assert.match(js, /function docDragEnabled\(kind, listEl, filterState\)/);
assert.match(js, /filterState\.priority === "all"/);
assert.match(js, /renderDocList\(defectList,[\s\S]*documentFilters\.defect/);
assert.match(js, /function setRunning\(value, statusText\)[\s\S]*send\.disabled = false/);
assert.match(js, /运行中可插入或排队，按交付方式发送/);
assert.match(js, /bgEntries\.delete\(first\.dataset\.bgId\)/);
assert.match(js, /diffSummary\.clear\(\)/);
assert.match(js, /window\.addEventListener\("focus", resetTitleOnFocus\)/);
assert.match(js, /if \(running\) \{[\s\S]*运行中请先完成或停止当前任务，再打开历史对话/);
assert.match(js, /document\.querySelectorAll\("\[data-doc-id\]"\)[\s\S]*item\.dataset\.docId === ref[\s\S]*offsetParent/);

console.log(`UI 无障碍静态冒烟通过：${static_icon_buttons.length} 个静态 icon-btn，核心键盘语义与焦点规则已覆盖`);
