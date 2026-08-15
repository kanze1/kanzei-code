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
for (const selector of ["activity-item", "rail-sidebar-toggle", "auto-continue", "auto-allow"]) {
  assert.ok(html.includes(`id="${selector}"`) || html.includes(`class="${selector}`), `缺少核心控件 ${selector}`);
}
assert.match(js, /activity-item[\s\S]*aria-current/);
assert.match(js, /project-item[\s\S]*item\.click\(\)/);
assert.match(js, /doc-row[\s\S]*aria-expanded/);
assert.match(js, /workspace-card[\s\S]*card\.click\(\)/);
assert.match(js, /remove\.setAttribute\("aria-label"/);
assert.match(js, /rename\.setAttribute\("aria-label"/);
// 胶囊开关把原生勾选框视觉隐藏,必须用 opacity:0 保留可聚焦——display:none 会把它
// 从 tab 序里摘掉,键盘用户就切不动鞭挞。#auto-allow-wrap 已随鞭挞控制台改成菜单行,
// 勾选框恢复可见,不再走这套隐藏;仍用胶囊的只剩 #auto-continue-wrap。
assert.match(css, /#auto-continue-wrap input\s*\{[\s\S]*opacity: 0/);
assert.doesNotMatch(css, /#auto-continue-wrap input\s*\{\s*display:\s*none/);
assert.match(css, /\.menu-row\b/, "鞭挞设置面板的行式布局丢失");
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
// 缺陷列表必须拿缺陷队列自己的筛选状态。旧断言是
// /renderDocList\(defectList,[\s\S]*documentFilters\.defect/ —— `[\s\S]*` 贪婪跨全文,
// 只要文件后面任意位置还出现过 documentFilters.defect(它出现了好几次),调用点写成
// 什么都能匹配上,等于没断言。改成:先切出调用点那一行(不跨行),再对这一行本身断言,
// 并显式排除拿错队列的写法——传错 state 时必须红。
const defectListCall = js.match(/^[^\n]*renderDocList\(defectList,[^\n]*$/m);
assert.ok(defectListCall, "找不到 renderDocList(defectList, …) 调用点");
assert.match(
  defectListCall[0],
  /documentFilters\.defect/,
  `缺陷列表没有拿缺陷队列的筛选状态:${defectListCall[0].trim()}`
);
assert.doesNotMatch(
  defectListCall[0],
  /documentFilters\.req/,
  `缺陷列表拿了需求队列的筛选状态:${defectListCall[0].trim()}`
);
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
// 侧栏重构后完整列表整体搬进单页:侧栏不再持有任何筛选控件(标签筛选能力由下一条
// #documents-tag-filter 守着)。这里改成反向断言而不是删掉——删了就没人守「标签筛选
// 被谁顺手删掉」,留着正向断言又会把「已经搬走」误报成「丢了」。
assert.ok(!html.includes('id="req-tag-filter"'), "侧栏不该再持有需求标签筛选(完整列表已搬进单页)");
assert.ok(!html.includes('id="defect-tag-filter"'), "侧栏不该再持有缺陷标签筛选(完整列表已搬进单页)");
assert.ok(html.includes('id="documents-tag-filter"'), "缺少独立页标签筛选");
// 侧栏只剩「当前在做」的焦点卡片:列表、筛选条、分组/排序控件、测试记录都不该还在侧栏。
for (const gone of ["req-list", "defect-list", "tests-section", "req-filter-row", "defect-filter-row", "req-sort", "req-group-toggle", "req-priority-filter", "req-status-filter"]) {
  assert.ok(!html.includes(`id="${gone}"`), `侧栏残留完整列表控件 ${gone}(侧栏应只显示当前在做的单条)`);
}
assert.ok(html.includes('id="focus-section"') && html.includes('id="focus-body"'), "侧栏缺少「当前在做」焦点分区");
assert.match(html, /id="focus-section"[\s\S]{0,400}?class="section-title"/, "焦点分区缺 section-title(快记表单挂在它下面)");
// 测试记录搬进单页后三个 id 必须原样保留在 #documents-tests 内:09-sessions.js 按 id 绑定,
// 改名会让顶层 addEventListener 在 null 上抛错,整条 ui/*.js 执行链断掉。
const testsBlock = html.slice(html.indexOf('id="documents-tests"'), html.indexOf('id="documents-dep-view"'));
for (const id of ["test-list"]) {
  assert.ok(testsBlock.includes(`id="${id}"`), `测试记录列表 ${id} 不在单页 #documents-tests 内`);
}
for (const id of ["test-count", "tests-refresh"]) {
  assert.ok(html.includes(`id="${id}"`), `测试记录控件 ${id} 丢失(09-sessions.js 按 id 绑定)`);
}
assert.match(html, /id="defect-review"[^>]*><span data-i18n-key="自动审查缺陷">自动审查缺陷<\/span><\/button>/);
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
// 窄窗口下顶栏做减法。原来这里连 #auto-status 一起藏——鞭挞的停机原因是窄屏第一个
// 被丢掉的东西。它已随控制台搬进 composer(flex-wrap,不靠隐藏让位),这条只该管 crumb;
// 反证:#auto-status 不得再出现在任何 display:none 的媒体查询里。
assert.match(css, /@media \(max-width: 1024px\)[\s\S]*#topbar \.crumb \{ display: none; \}/);
assert.doesNotMatch(css, /#auto-status[^{}]*\{\s*display: none/, "鞭挞停机原因不得在窄窗口下被整个藏掉");
assert.doesNotMatch(html, /id="process-tabs"/, "顶部进程切换条不应与左侧线路状态按钮重复");
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
assert.match(html, /id="topbar-more"[\s\S]*id="worktree-add"[\s\S]*id="process-phase-pipeline-wrap"/);
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
// 批次格填充色曾只在 `#req-list` 下定义,列表搬进单页后 #documents-req-list 里的已完成格
// 全是透明的。改按条目类名限定后,容器 id 不得再出现在批次格规则里。
assert.doesNotMatch(css, /#req-list \.doc-item/, "批次格/条目样式仍按已删除的 #req-list 容器限定");
assert.match(css, /\.doc-item\.pri-P1 \.complexity-cell\.filled, \.focus-card\.pri-P1 \.complexity-cell\.filled/);
assert.match(css, /\.focus-card \{/, "缺少侧栏焦点卡片样式");
assert.match(js, /window\.addEventListener\("focus", resetTitleOnFocus\)/);
assert.match(js, /if \(running\) \{[\s\S]*运行中请先完成或停止当前任务，再打开历史对话/);
assert.match(js, /document\.querySelectorAll\("\[data-doc-id\]"\)[\s\S]*item\.dataset\.docId === ref[\s\S]*offsetParent/);
assert.match(js, /item\.diff\?\.trim\(\)/);
assert.match(js, /t\("实际差异"\)/);

// ---------- D-380 设计语言:主题块之外不得出现字面量颜色 ----------
// 亮色主题(R-189)是在暗色之上「把 token 覆盖一遍」做出来的,于是任何漏掉 token 的
// 字面量都会在亮色下**照旧渲染暗色**。危险的是这类逃逸集中在 hover/active 这些
// 静态看不出来的态:实测曾有权限对话框的「拒绝」键 hover 变成深灰实心块、并行线路条
// hover 完全没有反馈、消息附件分隔线在浅底上不可见。靠人眼复查抓不住,故立此判据。
//
// 白名单只有 mask-image:遮罩取的是 alpha 通道,写什么颜色都一样,不是主题的一部分。
{
  const themeBlockEnd = css.indexOf("/* 勾选/单选的选中色统一走强调色");
  assert.ok(themeBlockEnd > 0, "找不到主题 token 块的结束标记,判据无法定位");
  const offenders = [];
  css.slice(themeBlockEnd).split("\n").forEach((line, index) => {
    if (/mask-image/.test(line)) return;
    if (/#[0-9a-fA-F]{3,8}\b/.test(line)) offenders.push(`+${index}: ${line.trim()}`);
  });
  assert.deepEqual(
    offenders,
    [],
    ["style.css 主题块之外出现字面量颜色(亮色主题下会照旧渲染暗色)。",
     '改法:在 :root 与 [data-theme="light"] 两组各给一个语义 token,引用点写 var(--x)。',
     ...offenders].join("\n"),
  );
  // var(--x, #fallback) 的回退分支同样绕过主题:token 都存在时它是死代码,
  // 一旦 token 改名它就会静默把暗色值顶上来。
  assert.doesNotMatch(
    css,
    /var\(--[a-z0-9-]+,\s*#[0-9a-fA-F]{3,8}\)/,
    "var() 里还留着字面量回退:token 改名时它会静默供给暗色值",
  );
}

// ---------- D-380 设计语言:活动栏图标必须是同一套描边 SVG ----------
// 曾经 11 个入口里 7 个是 24 viewBox / stroke 1.6 的内联 SVG(CSS 渲染 22px),
// 另外 4 个是 Unicode 字形 ⌂ ☷ ❖ ◉ —— 它们继承 body 的 13px,尺寸差 1.7 倍,
// 笔画粗细还由系统装了什么字体决定。style.css 里那条「整套图标必须是单色描边」的
// 注释拦住了彩色 emoji,却没拦住「SVG 与字形混用」本身。这是产品第一眼看到的一列。
{
  const items = [...html.matchAll(/<button[^>]*class="activity-item[^"]*"[^>]*>([\s\S]*?)<\/button>/g)];
  assert.ok(items.length >= 10, `活动栏入口只解析到 ${items.length} 个,判据定位失效`);
  const glyphs = items
    .map((match) => match[0])
    .filter((markup) => !markup.includes("<svg"))
    .map((markup) => (markup.match(/data-view="([a-z]+)"|id="([a-z-]+)"/) || [markup.slice(0, 60)])[0]);
  assert.deepEqual(
    glyphs,
    [],
    `活动栏还有非 SVG 图标(字形按 13px 渲染,与 22px 的 SVG 并排):${glyphs.join(", ")}`,
  );
  // 同一套 = 同一份规格:viewBox 与描边宽度不得各写各的。
  for (const [markup] of items) {
    assert.ok(
      /viewBox="0 0 24 24"/.test(markup),
      `活动栏图标 viewBox 不统一:${markup.slice(0, 80)}`,
    );
    assert.ok(
      /stroke-width="1\.[68]"/.test(markup),
      `活动栏图标描边宽度不在 1.6/1.8 规格内:${markup.slice(0, 80)}`,
    );
  }
}

// ---------- D-380 设计语言:字号与层级只准走 token ----------
// 色彩早就 token 化了,排版与层级没有:改造前 221 条 font-size 声明散着 17 个不同值
// (含 9.5/10.5/11.5/12.5/13.5 五个半像素档),11 个 z-index 是裸数字,叠加关系只能
// 靠全文搜索复原。token 化本身不改一个像素,但它把「这道阶梯到底有几级」变成看得见、
// 可集中修改的一处;判据保证新代码不会再往回退。
{
  const themeBlockEnd = css.indexOf("/* 勾选/单选的选中色统一走强调色");
  const body = css.slice(themeBlockEnd);
  const rawFontSizes = body.match(/font(?:-size)?:\s*[0-9.]+px/g) || [];
  assert.deepEqual(
    rawFontSizes,
    [],
    `字号必须走 --fs-* token(裸 px 绕开集中管理):${rawFontSizes.join(", ")}`,
  );
  const rawZ = body.match(/z-index:\s*\d+/g) || [];
  assert.deepEqual(rawZ, [], `层级必须走 --z-* token:${rawZ.join(", ")}`);
  // token 本身必须定义齐全,否则引用点会静默取到 initial。
  for (const used of new Set((css.match(/var\((--(?:fs|z)-[a-z0-9-]+)\)/g) || []).map((v) => v.slice(4, -1)))) {
    assert.ok(
      new RegExp(`${used}:\s*[^;]+;`).test(css),
      `引用了未定义的设计 token:${used}`,
    );
  }
}

console.log(`UI 无障碍静态冒烟通过：${static_icon_buttons.length} 个静态 icon-btn，核心键盘语义与焦点规则已覆盖`);
