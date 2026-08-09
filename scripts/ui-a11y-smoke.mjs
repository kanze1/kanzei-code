import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { loadUiSources } from "./ui-sources.mjs";

const root = resolve(import.meta.dirname, "..");
const { html, joined: js } = loadUiSources();
const css = await readFile(resolve(root, "crates/kanzei-app/ui/style.css"), "utf8");

const static_icon_buttons = [...html.matchAll(/<button[^>]*class="icon-btn"[^>]*>/g)];
assert.ok(static_icon_buttons.length > 0, "未发现静态 icon-btn");
assert.equal(
  static_icon_buttons.filter(([tag]) => !tag.includes("aria-label=")).length,
  0,
  "静态 icon-btn 必须有 aria-label"
);

assert.match(html, /id="ask-overlay"[^>]*role="dialog"[^>]*aria-modal="true"[^>]*aria-labelledby="ask-title"/);
assert.match(html, /id="viewer-overlay"[^>]*role="dialog"[^>]*aria-modal="true"[^>]*aria-labelledby="viewer-title"/);
assert.match(js, /if \(event\.key !== "Escape"\) return/);
assert.match(js, /answerAsk\(askActive\.kind === "question" \? "cancel" : "deny"\)/);
assert.match(js, /\$\("viewer-close"\)\.focus\(\)/);
assert.match(js, /if \(event\.key !== "Enter" && event\.key !== " "\) return/);
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
// D-210:缺陷拖拽守卫覆盖全部四项筛选(旧断言只钉 status/priority 的字面量,
// tag/blocked 筛选下列表不完整,提交的顺序会被引擎拒绝)。
assert.match(js, /\["status", "priority", "tag", "blocked"\]\.every/);
assert.match(js, /renderDocList\(defectList,[\s\S]*documentFilters\.defect/);
// D-212:视图容器的显隐只归 .view/.view.active 管。裸 `#view-xxx { display:… }`
// 的 ID 特异性会无条件压过 .view 的 display:none,该视图永远渲染、叠进对话页
// (文件导览页首发就这么翻的车)。带 .active 的规则合法。
const cssNoComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
for (const match of cssNoComments.matchAll(/#view-[\w-]+\s*\{([^}]*)\}/g)) {
  assert.ok(
    !/display\s*:\s*(?!none)/.test(match[1]),
    `裸 #view-* 规则不得设置 display(用 .active 变体): ${match[0].slice(0, 60)}`
  );
}
assert.match(js, /function setRunning\(value, statusText\)[\s\S]*send\.disabled = false/);
assert.match(js, /运行中可插入或排队，按交付方式发送/);
assert.ok(html.includes('id="req-tag-filter"'), "缺少需求标签筛选");
assert.ok(html.includes('id="defect-tag-filter"'), "缺少缺陷标签筛选");
assert.ok(html.includes('id="documents-tag-filter"'), "缺少独立页标签筛选");
assert.match(html, /id="defect-review"[^>]*>自动审查缺陷<\/button>/);
assert.match(html, /id="defect-review-status"[^>]*role="status"[^>]*aria-live="polite"/);
assert.match(js, /invoke\("defect_review", \{ projectDir: currentProject \}\)/);
assert.match(js, /function entryTags\(entry\)/);
assert.match(js, /function syncTagFilter\(select, entries, selected = "all"\)/);
assert.match(js, /function workPriorityStorageKey\(\)/);
assert.match(js, /workPriority: selectedWorkPriority\(\)/);
assert.match(js, /function addUserMessage\(text, promptAttachments = \[\]\)/);
assert.match(html, /id="continue-panel"[\s\S]*id="continue-prompt"/);
assert.match(html, /id="continue-toggle"[\s\S]*id="continue-btn"/);
assert.match(css, /#continue-panel[\s\S]*grid-template-columns: auto minmax\(0, 1fr\)/);
assert.match(css, /#topbar[\s\S]*flex-wrap: nowrap/);
assert.match(css, /@media \(max-width: 1024px\)[\s\S]*#topbar \.crumb, #auto-status \{ display: none; \}/);
assert.match(css, /@media \(max-width: 1024px\)[\s\S]*#process-tabs \{[^}]*min-width: 40px;[^}]*max-width: 120px/);
assert.match(css, /@media \(max-width: 900px\)[\s\S]*#sidebar:not\(\.collapsed\)[\s\S]*position: absolute/);
assert.match(css, /#sidebar:not\(\.collapsed\)[^}]*max-width: min\(320px, calc\(100vw - 360px\)\)/);
assert.match(css, /#sidebar\.collapsed[\s\S]*width: 0/);
assert.match(css, /@media \(max-width: 1400px\)[\s\S]*#todo-panel, #bg-panel[\s\S]*position: absolute/);
assert.match(css, /#todo-panel:not\(\.hidden\):has\(~ #bg-panel:not\(\.hidden\)\)[^}]*bottom: 50%/);
assert.match(css, /#todo-panel:not\(\.hidden\) ~ #bg-panel:not\(\.hidden\)[^}]*top: 50%; right: 0/);
assert.match(js, /localStorage\.setItem\("kz-sidebar-collapsed"/);
assert.ok(html.includes('id="send"'), "缺少发送按钮");
assert.ok(html.includes('id="stop"'), "缺少停止按钮");
assert.match(html, /id="topbar-more"[\s\S]*id="summarize-btn"/);
assert.match(html, /id="topbar-more"[\s\S]*id="worktree-add"[\s\S]*id="process-subagent-wrap"/);
assert.match(js, /function syncSidebar\(\)/);
assert.match(js, /function syncActivityPanel\(\)/);
assert.match(js, /localStorage\.setItem\("kz-activity-panel"/);
assert.match(js, /function renderTodoPanel\([\s\S]*todoItems\.length === 0/);
assert.match(js, /function bgAdd\(/);
assert.match(js, /function syncActivityPanel\(\)/);
assert.match(js, /const setWidth = \(width\)[\s\S]*localStorage\.setItem/);
assert.match(js, /function setupResize\(/);
assert.match(js, /function setRunning\(value, statusText\)[\s\S]*send\.disabled = false/);
assert.match(js, /已发送给 agent/);
assert.match(js, /bgProgress\([\s\S]*appendDisplayBlock\(child\.row, trace\.display\)/);
assert.match(js, /function renderRecoveredTraces\(payloads\)/);
// 批次进度格(R-160):格子是纯装饰(aria-hidden),真正给读屏的是 meter 上的 role=img
// 与带准确数字的 aria-label——盯住这条契约,别再锁实现字符串(旧断言锁死了
// `complexity-level-${level}`,把静态复杂度换成批次进度时它是第一个红的,而无障碍性质
// 其实一点没变)。
assert.match(js, /meter\.className = "complexity-meter batch-meter"/);
assert.match(js, /meter\.setAttribute\("role", "img"\)/);
assert.match(js, /const label = `\$\{t\("批次"\)\} \$\{done\}\/\$\{total\}/);
assert.match(js, /meter\.setAttribute\("aria-label", label\)/);
assert.match(js, /cell\.setAttribute\("aria-hidden", "true"\)/);
// 轨道总长固定、列数随批次数走。写死列数会把 11 个格子折成多行糊成一坨(实测),
// 不固定总长则列表会因条目批次多寡而参差——两条都盯住。
assert.match(js, /meter\.style\.setProperty\("--cells"/);
assert.match(css, /grid-template-columns: repeat\(var\(--cells/);
assert.match(css, /\.doc-row \.complexity-meter \{ flex: 0 0 \d+px; width: \d+px; \}/);
assert.match(css, /#req-list \.doc-item::before \{ display: none; \}/);
assert.match(js, /window\.addEventListener\("focus", resetTitleOnFocus\)/);
assert.match(js, /if \(running\) \{[\s\S]*运行中请先完成或停止当前任务，再打开历史对话/);
assert.match(js, /document\.querySelectorAll\("\[data-doc-id\]"\)[\s\S]*item\.dataset\.docId === ref[\s\S]*offsetParent/);
assert.match(js, /item\.diff\?\.trim\(\)/);
assert.match(js, /t\("实际差异"\)/);

console.log(`UI 无障碍静态冒烟通过：${static_icon_buttons.length} 个静态 icon-btn，核心键盘语义与焦点规则已覆盖`);
