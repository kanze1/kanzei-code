import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { loadUiSources } from "./ui-sources.mjs";
import { checkSurfaceRules, formatViolations, selfTestSurfaceRules } from "./ui-surface-rules.mjs";

const root = resolve(import.meta.dirname, "..");
const { html, joined: js, scriptSrcs, sources } = loadUiSources();
const uiSources = scriptSrcs.map((name, index) => ({ name, text: sources[index] }));
const css = await readFile(resolve(root, "crates/kanzei-app/ui/style.css"), "utf8");

const static_icon_buttons = [...html.matchAll(/<button[^>]*class="icon-btn"[^>]*>/g)];
assert.ok(static_icon_buttons.length > 0, "未发现静态 icon-btn");
assert.equal(
  static_icon_buttons.filter(([tag]) => !tag.includes("aria-label=")).length,
  0,
  "静态 icon-btn 必须有 aria-label"
);

// 权限/提问弹窗是**非模态**停靠卡片(2026-08-16 起):要判断该不该放行往往得回看
// 刚才的工具轨迹与对话,全屏模态恰恰把判断依据挡在背后。非阻塞形态下 aria-modal
// 必须如实为 false——宣称 true 会让读屏软件把背景内容整块隐藏,与实际可读可交互
// 的事实相反,那才是真的无障碍缺陷。role/aria-labelledby 仍是硬要求。
assert.match(html, /id="ask-overlay"[^>]*role="dialog"[^>]*aria-modal="false"[^>]*aria-labelledby="ask-title"/);
// UI-0926 #9 弹层技术栈:权限卡是 popover="manual" 的停靠卡片(showCard/hideCard),
// 查看器/确认/输入/命令面板是 <dialog>(openDialog → showModal,原生模态语义,不再手写 aria-modal)。
assert.match(html, /id="ask-overlay"[^>]*popover="manual"/, "权限卡必须是 popover=manual 的停靠卡片");
for (const id of ["viewer-overlay", "confirm-overlay", "input-overlay", "palette"]) {
  assert.match(html, new RegExp(`<dialog id="${id}"[^>]*class="[^"]*\\bk-surface k-dialog\\b`), `${id} 必须是 <dialog class="k-surface k-dialog">`);
}
assert.match(html, /<dialog id="viewer-overlay"[^>]*aria-labelledby="viewer-title"/);
// UI-0926 #3:项目模型配置同为 <dialog class="k-surface k-dialog">(openDialog),标题可读;输入框上方的模型/思考芯片是菜单按钮。
assert.match(html, /<dialog id="project-models-overlay"[^>]*class="[^"]*\bk-surface k-dialog\b[^"]*"[^>]*aria-labelledby="project-models-title"/, "项目模型配置必须是带 aria-labelledby 的 <dialog class=\"k-surface k-dialog\">");
for (const id of ["model-picker", "reasoning-picker"]) {
  assert.match(html, new RegExp(`<button type="button" id="${id}"[^>]*aria-haspopup="menu"[^>]*aria-expanded="false"`), `${id} 必须是带 aria-haspopup/aria-expanded 的菜单按钮(不再是原生 select)`);
}
// Esc 只有一个入口:00-surface.js 在 document 捕获阶段只关栈顶;权限卡的 Esc 经 onEscape 拒绝/取消。
assert.match(js, /if \(event\.key !== "Escape" \|\| event\.isComposing\) return;/);
assert.match(js, /document\.addEventListener\("keydown", onKeydown, true\)/);
assert.match(js, /onEscape: \(\) => \{\s*if \(askActive\) answerAsk\(askActive\.kind === "question" \? "cancel" : "deny"\)/);
assert.match(js, /answerAsk\(askActive\.kind === "question" \? "cancel" : "deny"\)/);
assert.match(js, /openDialog\(\$\("viewer-overlay"\), \{ initialFocus: "#viewer-close" \}\)/);
assert.match(js, /if \(event\.key !== "Enter" && event\.key !== " "\) return/);
for (const selector of ["activity-item", "rail-sidebar-toggle", "auto-continue", "auto-allow"]) {
  assert.ok(html.includes(`id="${selector}"`) || html.includes(`class="${selector}`), `缺少核心控件 ${selector}`);
}
assert.match(js, /activity-item[\s\S]*aria-current/);
// UI2-0926 #1:项目切换只剩侧栏头部的项目卡菜单(菜单按钮语义 + click/方向键都打开 openProjectMenu);
// 项目总览卡片是「标题真按钮 + ⋯ 菜单按钮」,重命名/移除在 ⋯ 里(读屏名称带项目名)。
assert.match(html, /<button type="button" id="project-switch"[^>]*aria-haspopup="menu"[^>]*aria-expanded="false"/, "项目卡必须是带 aria-haspopup/aria-expanded 的菜单按钮");
assert.doesNotMatch(html, /id="project-switch"[^>]*aria-controls=/, "项目卡不再是某个分区的开合把手(不得再带 aria-controls)");
assert.match(js, /button\.addEventListener\("click", \(\) => openProjectMenu\(\)\)/, "项目卡点击未接 openProjectMenu");
assert.match(js, /event\.key !== "ArrowDown" && event\.key !== "ArrowUp"[\s\S]{0,120}openProjectMenu\(\)/, "项目卡方向键未接 openProjectMenu");
assert.match(js, /doc-row[\s\S]*aria-expanded/);
assert.match(js, /open\.className = "workspace-card-open"/, "项目总览卡片缺标题真按钮 .workspace-card-open");
assert.match(js, /open\.setAttribute\("aria-label", `\$\{t\("选择工作区项目"\)\} \$\{project\.name\}`\)/, "项目总览卡片标题按钮缺读屏名称");
assert.match(js, /more\.className = "icon-btn workspace-card-more"[\s\S]{0,400}more\.setAttribute\("aria-haspopup", "menu"\)[\s\S]{0,200}more\.setAttribute\("aria-label", `\$\{t\("更多操作"\)\} \$\{project\.name\}`\)/, "项目总览卡片 ⋯ 缺菜单按钮语义或带项目名的读屏名称");
assert.match(css, /\.workspace-card-open::after \{[^}]*inset: 0/, "项目总览卡片缺撑满整卡的点击覆盖层");
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
// UI2-0926 #14:活动/子代理两块浮层合成一个停靠的后台任务侧栏(用户推翻 R-334 的浮层定调)。
// 侧栏是 complementary 地标(<aside> + aria-labelledby),不是 dialog;rail 上只剩一个开关,带 aria-controls/expanded。
assert.match(html, /<aside id="tasks-panel"[^>]*aria-labelledby="tasks-panel-title"/, "#tasks-panel 必须是带 aria-labelledby 的 <aside> 地标");
assert.doesNotMatch(html, /id="tasks-panel"[^>]*role="dialog"/, "停靠侧栏不是对话框,不写 role=dialog");
assert.match(html, /id="tasks-toggle"[^>]*aria-controls="tasks-panel"[^>]*aria-expanded="false"/, "#tasks-toggle 必须带 aria-controls=tasks-panel 与 aria-expanded");
assert.match(html, /id="tasks-close"[^>]*aria-label="关闭后台任务侧栏"/);
for (const gone of ["bg-panel", "agent-panel", "agent-toggle", "activity-toggle", "bg-close", "agent-close"]) {
  assert.ok(!html.includes(`id="${gone}"`), `旧浮层/旧开关 #${gone} 应已合进后台任务侧栏`);
}
assert.match(js, /export function reconcileTasksPanel\(/, "侧栏显隐的唯一写入者 reconcileTasksPanel 丢失");
assert.match(css, /#main \{\s*--kz-side-col: 0px;\s*display: grid;/, "#main 必须是网格(侧栏停靠在第 2 列)");
assert.match(css, /#main > #tasks-panel\[data-dock="drawer"\] \{[^}]*position: absolute/, "抽屉态侧栏必须 position:absolute(盖在对话上,不推挤主区)");
// D-662:todowrite 摘除后 #todo-panel 的规则(含抽屉上下分区)应当一条不剩。
// 子代理面板与活动面板本就互斥切换,不会同屏,没有第二个面板能与之并存。
assert.doesNotMatch(css, /#todo-panel/, "#todo-panel 规则应随 todowrite 一并摘除");
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
// 顶栏 2026-08-16 整条删除:项目名/模型/思考强度/更多菜单下沉到输入区上方的
// 上下文行。反证——#topbar 不得以任何形式回到 HTML 或样式表里。
assert.doesNotMatch(html, /id="topbar"/, "顶栏已删除,不得重新出现");
assert.doesNotMatch(css, /#topbar[\s{,]/, "顶栏样式已删除,不得重新出现");
// 上下文行必须 flex-wrap:它在窄窗口下靠换行让位,而不是像旧顶栏那样 nowrap + 隐藏内容。
assert.match(css, /\.composer-context \{[\s\S]*?flex-wrap: wrap/, "上下文行必须可换行");
assert.match(css, /#composer > #composer-context \{ display: flex; \}/, "上下文行须显式恢复 flex(composer 限宽规则会压成 block)");
// 窄窗口下上下文行做减法的方式是**收窄**而不是隐藏:项目名/模型仍然看得见。
assert.match(css, /@media \(max-width: 1024px\)[\s\S]*?\.ctx-chip \{ max-width: 20ch; \}/);
assert.doesNotMatch(css, /\.ctx-chip[^{}]*\{\s*display: none/, "项目上下文胶囊不得在窄窗口下被整个藏掉");
assert.doesNotMatch(css, /#auto-status[^{}]*\{\s*display: none/, "鞭挞停机原因不得在窄窗口下被整个藏掉");
assert.doesNotMatch(html, /id="process-tabs"/, "顶部进程切换条不应与左侧线路状态按钮重复");
assert.match(css, /@media \(max-width: 900px\)[\s\S]*#sidebar:not\(\.collapsed\)[\s\S]*position: absolute/);
assert.match(css, /#sidebar:not\(\.collapsed\)[^}]*max-width: min\(320px, calc\(100vw - 360px\)\)/);
assert.match(css, /#sidebar\.collapsed[\s\S]*width: 0/);
// 侧栏停靠在网格第 2 列(从顶部到状态栏上沿);停靠判据与抽屉在 ui-narrow-layout-smoke 里真布局验证。
assert.match(css, /#main > #tasks-panel \{ grid-column: 2; grid-row: 1 \/ span 2; \}/, "#tasks-panel 必须在 #main 网格第 2 列跨视图与日志两行");
assert.match(js, /localStorage\.setItem\("kz-sidebar-collapsed"/);
assert.ok(html.includes('id="send"'), "缺少发送按钮");
assert.ok(html.includes('id="stop"'), "缺少停止按钮");
assert.match(html, /id="composer-more"[\s\S]*id="summarize-btn"/);
assert.match(html, /id="composer-more"[\s\S]*id="worktree-add"/);
// 任务设置是 data-kz-menu 触发器 + popover 弹层菜单(UI-0926 #9),开关都在弹层里。
assert.match(html, /id="task-options"[^>]*data-kz-menu="task-options-menu"/);
assert.match(html, /id="task-options-menu"[^>]*popover="manual"[\s\S]*id="auto-allow"[\s\S]*id="process-phase-pipeline-wrap"[\s\S]*id="process-tracker-writes-wrap"[\s\S]*id="composer-more"/);
assert.doesNotMatch(html, /<details[^>]*id="(?:composer-more|task-options|autorun-more)"/, "输入区菜单不得再用 <details> 做弹层");
assert.match(js, /function syncSidebar\(\)/);
// 侧栏开合不再存 localStorage(本机重启即丢,D-404);自动开合偏好经 ui_prefs 的 ui_layout.side_panel。
assert.doesNotMatch(js, /kz-activity-panel/, "活动面板开合状态已并入后台任务侧栏的策略,不得复活 kz-activity-panel");
assert.match(js, /export function sidePanelPrefs\(/, "后台任务侧栏的两个偏好(自动弹出/自动收起)读取入口丢失");
// D-662:renderTodoPanel 随 todowrite 一并摘除。此前这里断言它「计划清空即隐藏」;
// 现在断言它不复存在——留着一条针对已删函数的正向断言,下次有人重新引入
// 同名函数时会误以为受了保护。
assert.doesNotMatch(js, /renderTodoPanel/, "renderTodoPanel 应随 todowrite 一并摘除");
assert.match(js, /function bgAdd\(/);
assert.doesNotMatch(js, /function syncActivityPanel\(\)/, "syncActivityPanel 已并入 reconcileTasksPanel(显隐唯一写入者)");
// UI2-0926 #4:布局分隔条改走 00-frame.js 的 installSplit——尺寸写 <html> 上的 --kz-split-<id>、偏好经存储
// (应用里是 ui_prefs 的 ui_layout),不再写元素内联 width(内联宽度压过 #sidebar.collapsed,收起后留空栏)。
assert.match(js, /export function installSplit\(/, "布局分隔条的唯一入口 installSplit 丢失");
assert.match(js, /rootStyle\.setProperty\(cssVar, `\$\{next\}px`\);\s*storeSet\("splits"/, "分隔条必须把尺寸写成 CSS 变量并经存储持久化");
assert.doesNotMatch(js, /function setupResize\(/, "旧 setupResize(写内联 width)不得复活");
assert.match(css, /#sidebar \{ width: var\(--kz-split-sidebar\); \}/, "#sidebar 宽度必须引用 --kz-split-sidebar");
assert.match(css, /#sidebar\.collapsed \{ width: 0;/, "#sidebar.collapsed { width: 0 } 丢失(收起后会留空栏)");
assert.match(js, /function setRunning\(value, statusText\)[\s\S]*send\.disabled = false/);
assert.match(js, /已发送给 agent/);
// UI2-0926 #14:子代理(含编排派发)的过程只归子代理卡片与委派卡,终端条目不再接 task-progress(不重复两份)。
assert.doesNotMatch(js, /export function bgProgress\(/, "终端条目不再接子代理进度:task 的过程归委派卡");
assert.match(js, /export function bgRunningCount\(/, "侧栏徽标与自动收起要用的终端在跑计数丢失");
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
// 配色语义(docs/design/ui_color_semantics.md):批次格是进度不是状态——已完成格一律中性,
// 不随优先级换色(旧版 P1 琥珀格与「阻塞」同色、P2 蓝格);唯一的彩色是线真在跑时的当前格。
assert.match(
  css,
  /\.doc-item \.complexity-cell\.filled, \.focus-card \.complexity-cell\.filled \{ background: var\(--dim\); border-color: var\(--dim\); \}/,
  "批次格已完成格应一律中性:.doc-item/.focus-card 的 .complexity-cell.filled 用 var(--dim)",
);
assert.doesNotMatch(css, /\.pri-P[0-3][^{]*\.complexity-cell/, "批次格不得再按优先级(.pri-P*)着色");
assert.match(css, /\.focus-card \{/, "缺少侧栏焦点卡片样式");
// UI-0926 #4 精简焦点卡:整卡的点击目标是真按钮(键盘可达)并带读屏名称;「⋯」声明自己弹菜单;
// 撑满覆盖层让整卡可点;任务卡关闭按钮是带线路名的图标按钮。
assert.match(js, /open\.setAttribute\("aria-label", `\$\{entry\.id\} \$\{entry\.title\} · \$\{t\("打开详情"\)\}`\)/, "焦点卡 .focus-open 缺读屏名称");
assert.match(js, /more\.setAttribute\("aria-haspopup", "menu"\)/, "焦点卡「⋯」缺 aria-haspopup=menu");
assert.match(css, /\.focus-open::after \{[^}]*inset: 0/, "焦点卡缺撑满整卡的点击覆盖层");
assert.match(js, /close\.setAttribute\("aria-label", `\$\{t\("关闭线路"\)\} \$\{item\.label\}`\)/, "任务卡关闭图标按钮缺带线路名的读屏名称");
assert.match(js, /window\.addEventListener\("focus", resetTitleOnFocus\)/);
assert.match(js, /if \(running\) \{[\s\S]*运行中请先完成或停止当前任务，再打开历史对话/);
assert.match(js, /document\.querySelectorAll\("\[data-doc-id\]"\)[\s\S]*item\.dataset\.docId === ref[\s\S]*offsetParent/);
assert.match(js, /item\.diff\?\.trim\(\)/);
assert.match(js, /t\("实际差异"\)/);

// ---------- #7 动效纪律:状态图标只在状态真的在进行时才动 ----------
// 纪律写在 style.css「分区:动效」头注释里;这里把它变成机械判据。去注释后再查,
// 注释里提到的选择器/属性名不算数。
{
  const motionCss = css.replace(/\/\*[\s\S]*?\*\//g, "");
  // ① token 齐全:循环时长全是 2400ms 的约数(motionSync 靠它对齐相位),缺一个引用点就静默取 initial。
  for (const token of [
    "--motion-fast", "--motion-base", "--motion-slow",
    "--motion-loop-fast", "--motion-spin", "--motion-loop", "--motion-loop-slow",
    "--ease-out", "--ease-in-out", "--ease-spring", "--shimmer-base", "--shimmer-hot",
  ]) {
    assert.match(motionCss, new RegExp(`${token}:\\s*[^;]+;`), `#7 动效 token 未定义:${token}`);
  }
  for (const [token, ms] of [["--motion-loop-fast", 600], ["--motion-spin", 800], ["--motion-loop", 1200], ["--motion-loop-slow", 2400]]) {
    const value = Number(motionCss.match(new RegExp(`${token}:\\s*(\\d+)ms`))?.[1]);
    assert.ok(value === ms && 2400 % value === 0, `#7 循环档 ${token} 必须是 2400ms 的约数(实为 ${value}ms)`);
  }
  // ② 关键帧只动 opacity/transform(合成层);唯一例外 kz-shimmer 只动 background-position。
  const keyframes = [...motionCss.matchAll(/@keyframes\s+([\w-]+)\s*\{((?:[^{}]*\{[^{}]*\})*)\s*\}/g)];
  assert.ok(keyframes.some(([, name]) => name === "kz-breathe") && keyframes.some(([, name]) => name === "kz-spin"), "#7 动效关键帧缺失(kz-breathe/kz-spin)");
  for (const [, name, body] of keyframes) {
    const props = new Set([...body.matchAll(/([\w-]+)\s*:/g)].map(([, prop]) => prop));
    const allowed = name === "kz-shimmer" ? ["background-position"] : ["opacity", "transform"];
    const extra = [...props].filter((prop) => !allowed.includes(prop));
    assert.deepEqual(extra, [], `#7 @keyframes ${name} 动了 ${extra.join(", ")}(只准 ${allowed.join("/")},其余每帧重排/重绘)`);
  }
  // ③ 无限循环只挂在状态选择器上,且时长走 token。选择器按顶层逗号切(:is(...) 里的逗号不算),
  // 每一段都必须带状态门,否则基类一匹配就永远在跑——空闲时也在耗电,就是 agent-pulse 的老毛病。
  const STATE_GATE = /\.running|\.pending|\.is-live|\.suspected-stuck|\[data-state=|\[data-phase=|\[data-kz-activity=|\[data-running=|\[data-voice-state=|\[data-live=|\[data-waiting=|\[aria-busy=|:not\(\.hidden\)/;
  const splitSelectors = (selector) => {
    const parts = [];
    let depth = 0;
    let current = "";
    for (const ch of selector) {
      if (ch === "(" || ch === "[") depth += 1;
      else if (ch === ")" || ch === "]") depth -= 1;
      if (ch === "," && depth === 0) {
        parts.push(current.trim());
        current = "";
      } else current += ch;
    }
    if (current.trim()) parts.push(current.trim());
    return parts;
  };
  const rulesCss = motionCss.replace(/@keyframes\s+[\w-]+\s*\{(?:[^{}]*\{[^{}]*\})*\s*\}/g, "");
  let infiniteRules = 0;
  for (const [, selector, body] of rulesCss.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (!/\binfinite\b/.test(body)) continue;
    infiniteRules += 1;
    for (const part of splitSelectors(selector.trim())) {
      assert.match(part, STATE_GATE, `#7 无限动效挂在了非状态选择器上(基类永远在跑):${part}`);
    }
    for (const [, value] of body.matchAll(/(?:^|;)\s*animation(?:-duration)?\s*:\s*([^;]+)/g)) {
      assert.doesNotMatch(value, /(?:^|[\s,])\.?\d+(?:\.\d+)?m?s\b/, `#7 无限动效用了裸时长(必须走 --motion-loop* token):${selector.trim()} → ${value.trim()}`);
    }
  }
  assert.ok(infiniteRules >= 10, `#7 只解析到 ${infiniteRules} 条无限动效规则,判据可能已与样式表脱节`);
  // ④ 窗口隐藏即暂停。
  assert.match(motionCss, /html\[data-kz-motion="paused"\] \*[^{]*\{[^}]*animation-play-state:\s*paused/, "#7 缺少窗口隐藏时统一暂停动画的规则");
  // ⑤ 减少动效:运行态退化为慢呼吸(运行中是信息不是装饰),工具行转圈也在其中。
  const reduceBlocks = [];
  for (const match of motionCss.matchAll(/@media \(prefers-reduced-motion: reduce\) \{/g)) {
    let depth = 1;
    let index = match.index + match[0].length;
    const start = index;
    while (depth > 0 && index < motionCss.length) {
      if (motionCss[index] === "{") depth += 1;
      else if (motionCss[index] === "}") depth -= 1;
      index += 1;
    }
    reduceBlocks.push(motionCss.slice(start, index - 1));
  }
  assert.ok(
    reduceBlocks.some((block) => /\.tool-msg\.running \.tool-msg-status::before[^{]*\{[^}]*kz-breathe/.test(block)),
    "#7 减少动效时工具行转圈没有退化为慢呼吸(运行态信息被全局 .01ms 规则抹掉)",
  );
  assert.ok(
    reduceBlocks.some((block) => /\.kz-dot:is\([^)]*\[data-state="running"\][^{]*\{[^}]*kz-breathe[^}]*!important/.test(block)),
    "#7 减少动效时运行点没有用 !important 退化为慢呼吸",
  );
  // ⑥ 旧的无纪律动画已删:常驻 border 呼吸、裸 1.7s/1s 脉冲。
  for (const gone of ["agent-pulse", "line-running-pulse", "pulse 1s"]) {
    assert.ok(!motionCss.includes(gone), `#7 旧动画 ${gone} 仍在样式表里`);
  }
  // 工具行运行中必须真的在转(不是只换颜色):转圈挂在 .running 上、时长走 --motion-spin。
  assert.match(motionCss, /\.tool-msg\.running \.tool-msg-status::before\s*\{[^}]*animation:\s*kz-spin var\(--motion-spin\)/, "#7 主对话工具行运行中没有转圈");
  // ⑦ 标记:运行活动行(读屏可达但不每秒播报)与状态点原语。
  assert.match(html, /<div id="turn-activity" class="[^"]*\bhidden\b[^"]*"[^>]*role="status"[^>]*aria-live="off"[^>]*data-i18n-aria-label="运行状态"/, "#7 运行活动行缺失或无障碍属性不全(role=status / aria-live=off / 默认隐藏)");
  for (const id of ["turn-activity-glyph", "turn-activity-label", "turn-activity-elapsed"]) {
    assert.ok(html.includes(`id="${id}"`), `#7 运行活动行缺子节点 ${id}`);
  }
  assert.match(html, /id="turn-activity-label"[^>]*data-i18n-raw/, "#7 活动行文案由 JS 本地化后写入,必须挡住 i18n 观察者二次翻译");
  assert.match(html, /<span id="status-dot" class="[^"]*\bkz-dot\b[^"]*"[^>]*aria-hidden="true"/, "#7 状态栏点未改用 .kz-dot 原语(或未对读屏隐藏)");
  // 状态点的扩散环画在 ::after 上。基线 `.status-left > span { overflow: hidden }`(防长文案撑破)同样命中
  // 这个点,会把环整个裁掉,运行中看起来仍是静止的点。冒烟的假 DOM 算不出 computed overflow,只能静态查覆盖规则
  // (特异性 1,1,0 高于 0,1,1)。
  assert.match(motionCss, /(?:^|\})\s*\.status-left > #status-dot\s*\{[^}]*overflow:\s*visible/, "#7 状态点没有放开 overflow(基线 .status-left > span 会裁掉扩散环)");
  // ⑧ 钩子接线:隐藏暂停监听、相位投影入口、停止收尾与后台守卫。
  assert.match(js, /document\.addEventListener\("visibilitychange", syncMotionVisibility\)/, "#7 窗口可见性变化未接 syncMotionVisibility");
  assert.match(js, /function setTurnPhase\(phase\) \{[\s\S]{0,200}?if \(typeof renderingBackground !== "undefined" && renderingBackground\) return;/, "#7 setTurnPhase 缺后台渲染守卫(后台线会改写活动线相位)");
  assert.match(js, /setDataIfChanged\(document\.documentElement, "kzActivity", activity\)/, "#7 全局相位未投影到 html[data-kz-activity]");
  assert.match(js, /\$\("update-check"\)\.setAttribute\("aria-busy", "true"\)/, "#7 检查更新进行中未标 aria-busy");
}

// ---------- D-380 设计语言:主题块之外不得出现字面量颜色 ----------
// 亮色主题(R-189)是在暗色之上「把 token 覆盖一遍」做出来的,于是任何漏掉 token 的
// 字面量都会在亮色下**照旧渲染暗色**。危险的是这类逃逸集中在 hover/active 这些
// 静态看不出来的态:实测曾有权限对话框的「拒绝」键 hover 变成深灰实心块、并行线路条
// hover 完全没有反馈、消息附件分隔线在浅底上不可见。靠人眼复查抓不住,故立此判据。
//
// 白名单只有 mask-image:遮罩取的是 alpha 通道,写什么颜色都一样,不是主题的一部分。
//
// UI-0926 #9:旧判据只认 #hex,放过了 25 处 rgba()(其中 10 处在弹层上)。现在交给
// ui-surface-rules.mjs:C1 连颜色函数与颜色名一起查,另有 T1 token 分层、S1 弹层外观归属、
// H 页面结构、J 脚本四组判据;模块自带反例自测,任何一条判据恒绿都会先在这里红。
{
  const themeBlockEnd = css.indexOf("/* ===== 主题 token 块结束");
  assert.ok(themeBlockEnd > 0, "找不到主题 token 块的结束标记,判据无法定位");
  const silentRules = selfTestSurfaceRules();
  assert.deepEqual(silentRules, [], `ui-surface-rules 判据没能命中自己的反例(恒绿):${silentRules.join(", ")}`);
  const surfaceCss = await readFile(resolve(root, "crates/kanzei-app/ui/surface.css"), "utf8");
  const pwaCss = await readFile(resolve(root, "crates/kanzei-app/mobile-pwa/style.css"), "utf8");
  const violations = checkSurfaceRules({ css, surfaceCss, pwaCss, html, sources: uiSources });
  assert.deepEqual(
    violations.map((v) => `${v.rule} ${v.file}:${v.line}`),
    [],
    ["弹层与外观静态门禁(ui-surface-rules)未通过:", formatViolations(violations)].join("\n"),
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
  const themeBlockEnd = css.indexOf("/* ===== 主题 token 块结束");
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

// ---------- UI-0926 #5 配色:token 齐全、两套主题对齐、对比度下限、选中/焦点中性 ----------
// 配色改成「中性灰阶 + 单一陶土橙」后,光换 token 值守不住:引用一个没定义的 token 浏览器会静默
// 取 initial(--fg-dim/--sans/--bg-deep 曾各吃了一年回退);暗色块加了颜色、亮色块忘了跟,亮色下
// 照旧渲染暗色值;对比度靠目测;强调色又被顺手用回选中/焦点——满屏橙色就是这样回来的。
// 以下每条都是机械判据,报错写全判据与改法。
{
  const surfaceCss = await readFile(resolve(root, "crates/kanzei-app/ui/surface.css"), "utf8");
  const strip = (text) => text.replace(/\/\*[\s\S]*?\*\//g, "");
  const cssClean = strip(css);
  const allClean = `${cssClean}\n${strip(surfaceCss)}`;

  // ① 未定义 token:style.css/surface.css 里每个 var(--x) 都必须有 "--x:" 定义(带回退值的也算——
  //    回退值只在 token 缺失时生效,token 被删/改名时它会静默顶替主题值)。白名单只放运行时由脚本写入的。
  const RUNTIME_TOKENS = new Set([
    "--cells", // 11-docs-list.js / 12-docs-pages.js 按批次数写入
    "--voice-level", // 23-voice.js 写音量
    "--kz-sync", // 01-core.js motionSync 写动画相位(动效分区)
    "--tf-progress", // 04-structured.js renderTrackerFields 写批次进度条宽度
    // ── 分区:后台任务侧栏与可调框 ── 00-frame.js 按用户拖出的几何写可调框的摆放变量(surface.css §10)。
    "--kz-frame-l", "--kz-frame-r", "--kz-frame-t", "--kz-frame-b", "--kz-frame-w", "--kz-frame-h",
    // ── 分区:架构图 ── 04-diagram.js 按「适应」后的图高写画布高度(clamp 在脚本里算)。
    "--kz-diagram-h",
  ]);
  const definedTokens = new Set([...allClean.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]));
  const undefinedTokens = [...new Set([...allClean.matchAll(/var\(\s*(--[a-z0-9-]+)/g)].map((m) => m[1]))]
    .filter((name) => !definedTokens.has(name) && !RUNTIME_TOKENS.has(name));
  assert.deepEqual(
    undefinedTokens,
    [],
    `引用了未定义的 token(浏览器静默取 initial):${undefinedTokens.join(", ")}。改法:换成已定义的语义 token,或在 :root 与 [data-theme="light"] 里补定义;运行时由脚本写入的才进白名单。`,
  );

  // ② 两套主题对齐::root 里值含十六进制颜色的 token,亮色块必须重新给值(否则亮色下照旧渲染暗色)。
  //    豁免:排版/尺寸类(--fs/--z/--sp/--r-/--radius/--mono/--sans)与 var() 别名(随被引用者自动换色)。
  const tokenBlock = (pattern) => Object.fromEntries(
    [...(strip(css).match(pattern)?.[1] ?? "").matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]),
  );
  const darkTokens = tokenBlock(/:root\s*\{([^}]*)\}/);
  const lightOverrides = tokenBlock(/\[data-theme="light"\]\s*\{([^}]*)\}/);
  assert.ok(Object.keys(darkTokens).length > 50 && Object.keys(lightOverrides).length > 50, "主题 token 块解析失败(:root 或 [data-theme=\"light\"] 找不到),判据无法定位");
  const THEME_EXEMPT = /^--(?:fs|z|sp|r)-|^--(?:radius|mono|sans)$/;
  const unaligned = Object.entries(darkTokens)
    .filter(([name, value]) => /#[0-9a-fA-F]{3,8}\b/.test(value) && !THEME_EXEMPT.test(name) && !(name in lightOverrides))
    .map(([name]) => name);
  assert.deepEqual(unaligned, [], `暗色块定义了颜色、亮色块没有跟上:${unaligned.join(", ")}。改法:在 [data-theme="light"] 里给同名 token 一个亮色值。`);

  // ③ WCAG 2.x 对比度下限:文字 ≥ 4.5,焦点环与强调色填充等非文本 ≥ 3。亮色 = :root 与亮色块合并后的结果;
  //    半透明前景先按下层表面合成再算。底色含 --surface-raised(输入区,暗色下是最亮的一层,dim 在它上面最紧)。
  const lightTokens = { ...darkTokens, ...lightOverrides };
  const resolveColor = (tokens, name, seen = new Set()) => {
    const value = tokens[name];
    assert.ok(value, `对比度判据:token ${name} 未定义`);
    const alias = value.match(/^var\((--[a-z0-9-]+)\)$/);
    if (alias && !seen.has(alias[1])) return resolveColor(tokens, alias[1], seen.add(name));
    const hex = value.match(/^#([0-9a-fA-F]{6})([0-9a-fA-F]{2})?$/);
    assert.ok(hex, `对比度判据:${name} 的值 "${value}" 不是 6/8 位十六进制,无法计算`);
    const channel = (i) => parseInt(hex[1].slice(i * 2, i * 2 + 2), 16);
    return { rgb: [channel(0), channel(1), channel(2)], alpha: hex[2] ? parseInt(hex[2], 16) / 255 : 1 };
  };
  const luminance = ([r, g, b]) => {
    const lin = (c) => {
      const v = c / 255;
      return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  };
  const contrast = (tokens, fgName, bgName) => {
    const bg = resolveColor(tokens, bgName);
    assert.equal(bg.alpha, 1, `对比度判据:底色 ${bgName} 必须不透明`);
    const fg = resolveColor(tokens, fgName);
    const rgb = fg.rgb.map((c, i) => c * fg.alpha + bg.rgb[i] * (1 - fg.alpha));
    const [hi, lo] = [luminance(rgb), luminance(bg.rgb)].sort((a, b) => b - a);
    return (hi + 0.05) / (lo + 0.05);
  };
  const pairs = [];
  for (const fg of ["--fg", "--fg-strong", "--dim", "--accent-text", "--ok", "--err", "--warn", "--info"]) {
    for (const bg of ["--bg", "--sidebar-bg", "--panel", "--panel2", "--surface-overlay", "--surface-raised"]) pairs.push([fg, bg, 4.5]);
  }
  pairs.push(
    ["--statusbar-fg", "--statusbar", 4.5],
    ["--statusbar-run-fg", "--statusbar-run", 4.5],
    ["--primary-fg", "--primary-bg", 4.5],
    ["--on-danger", "--danger-btn", 4.5],
    // rail 运行数徽标:主区色数字放在 accent-text 实心底上(徽标是文字)。
    ["--bg", "--accent-text", 4.5],
    // ── 分区:后台任务侧栏与可调框 ── 后台任务徽标的失败态:主区色数字放在 --err 实心底上(#tasks-badge[data-tone=err])。
    ["--bg", "--err", 4.5],
  );
  for (const bg of ["--bg", "--panel", "--sidebar-bg"]) pairs.push(["--focus-ring", bg, 3]);
  // 非文本:强调色填充(发送键、运行点、代号框)在主区与侧栏上可辨;发送键白色箭头在强调色填充上可辨。
  // 强调色填充上不放正文字(白字在 #d25e28 上只有 3.9,只够图标)。
  pairs.push(["--accent", "--bg", 3], ["--accent", "--sidebar-bg", 3], ["--on-accent", "--accent", 3]);
  const lowContrast = [];
  for (const [theme, tokens] of [["暗色", darkTokens], ["亮色", lightTokens]]) {
    for (const [fg, bg, floor] of pairs) {
      const ratio = contrast(tokens, fg, bg);
      if (ratio < floor) lowContrast.push(`${theme} ${fg} 在 ${bg} 上 ${ratio.toFixed(2)} < ${floor}`);
    }
  }
  assert.deepEqual(lowContrast, [], `对比度低于 WCAG 下限(文字 4.5、焦点环/强调色填充/填充上的图标 3):\n${lowContrast.join("\n")}`);

  // ④ 选中/焦点一律中性:强调色只承载「运行中 / 链接 / 品牌 / 看这里」。
  const rulesOf = (text) => [...text.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .map((m) => ({ branches: m[1].split(/,(?![^(]*\))/).map((b) => b.trim().replace(/\s+/g, " ")), body: m[2] }))
    .filter((rule) => rule.branches[0] && !rule.branches[0].startsWith("@"));
  const rules = rulesOf(cssClean);
  const bodiesFor = (selector) => rules.filter((rule) => rule.branches.includes(selector)).map((rule) => rule.body);
  for (const selector of [
    ".parallel-task-row.active", ".workspace-switcher button.active",
    ".research-page-nav button.active", ".palette-row.active", ".files-row.active", ".arch-entry.active",
    ".activity-item.active", ".memory-row.selected", '.documents-list .doc-row[aria-expanded="true"]',
  ]) {
    const bodies = bodiesFor(selector);
    assert.ok(bodies.length, `找不到选中态规则 ${selector}(判据定位失效)`);
    assert.ok(!bodies.some((body) => /var\(--accent/.test(body)), `选中态 ${selector} 不得使用强调色(--accent*):选中/激活一律中性,用 var(--surface-selected) + var(--fg-strong)`);
    assert.ok(bodies.some((body) => body.includes("var(--surface-selected)")), `选中态 ${selector} 必须用 var(--surface-selected) 表达(柔灰圆角块)`);
  }
  const accentFocus = rulesOf(allClean)
    .filter((rule) => rule.branches.some((branch) => /:focus(?:-visible|-within)?\b/.test(branch)))
    .filter((rule) => /var\(--accent(?:-soft|-hover)?\)/.test(rule.body))
    .map((rule) => rule.branches.join(", "));
  assert.deepEqual(accentFocus, [], `焦点态不得使用强调色(轮廓/边框/底色一律 var(--focus-ring) 或中性表面;链接文字色 --accent-text 不在此列):\n${accentFocus.join("\n")}`);

  // ⑤ 主按钮单色、输入区大圆角 + 柔阴影、用户气泡无竖条。
  const primary = bodiesFor("button.primary").join(";");
  assert.ok(primary.includes("var(--primary-bg)") && primary.includes("var(--primary-fg)"), "button.primary 必须用单色主按钮 token --primary-bg/--primary-fg(不用强调色填充)");
  const composer = bodiesFor("#composer").join(";");
  assert.ok(composer.includes("var(--r-xl)") && composer.includes("var(--elev-composer)"), "#composer 必须用 var(--r-xl) 圆角与 var(--elev-composer) 阴影");
  const userBubble = bodiesFor(".msg.user");
  assert.ok(userBubble.length, "找不到 .msg.user 规则(判据定位失效)");
  assert.ok(!userBubble.some((body) => /border-left/.test(body)), ".msg.user 不得再有左竖条(border-left):用户消息是圆角灰气泡");
}

// ---------- UI2-0926 配色 ③b 叠色对比度:胶囊与卡片文字按合成后的底色算 ----------
// ③ 只算 token 对 token(不透明底)。胶囊底多是半透明(--badge-soft / --accent-soft / --alert-soft),
// 放在焦点卡(--panel2)上、悬停时卡片再叠一层 --surface-hover,字的真实底色是三层合成的结果:
// 实测 --dim 叠 --badge-soft 叠焦点卡只有 3.98,悬停 3.42,而 ③ 全绿(复核 2026-09-26)。
// 这里按宿主逐层合成:胶囊选择器与字色/底色从样式表按原文取(同一选择器多条规则按源码顺序合并),
// 新增一种胶囊只要落在这些类族里就自动被算进来;底色不是 token 或 transparent 的直接报「无法计算」。
const CHIP_HOSTS = {
  // 文档类胶囊(状态/优先级/阻塞/待澄清):焦点卡、侧栏列表行、文档页列表行,各自带悬停。
  doc: [
    ["焦点卡", ["--panel2"]], ["焦点卡悬停", ["--panel2", "--surface-hover"]],
    ["侧栏行", ["--sidebar-bg"]], ["侧栏行悬停", ["--sidebar-bg", "--surface-hover"]],
    ["文档页行", ["--bg"]], ["文档页行悬停", ["--bg", "--surface-hover"]],
  ],
  // 项目卡运行状态胶囊:项目卡是 --surface-raised(悬停只加阴影与边框,不叠底色)。
  workspace: [["项目卡", ["--surface-raised"]]],
  // 焦点卡里的纯文字(编号、chip、阻塞原因、标题):没有自己的底,直接落在卡上。
  focusText: [["焦点卡", ["--panel2"]], ["焦点卡悬停", ["--panel2", "--surface-hover"]]],
};
const CHIP_FAMILIES = [
  [/^\.st-[a-z-]+$/, "doc"],
  [/^\.pri-badge\.(?:P[0-3]|unset)$/, "doc"],
  [/^\.(?:blocked|clarify)-badge$/, "doc"],
  [/^\.workspace-status\.[a-z-]+$/, "workspace"],
  [/^\.focus-[a-z-]+$/, "focusText"],
];
function chipContrastViolations(styleText) {
  const clean = styleText.replace(/\/\*[\s\S]*?\*\//g, "");
  const tokenBlock = (pattern) => Object.fromEntries(
    [...(clean.match(pattern)?.[1] ?? "").matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]),
  );
  const dark = tokenBlock(/:root\s*\{([^}]*)\}/);
  const themes = [["暗色", dark], ["亮色", { ...dark, ...tokenBlock(/\[data-theme="light"\]\s*\{([^}]*)\}/) }]];
  const rules = [...clean.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .map((m) => ({ branches: m[1].split(/,(?![^(]*\))/).map((b) => b.trim().replace(/\s+/g, " ")), body: m[2] }))
    .filter((rule) => rule.branches[0] && !rule.branches[0].startsWith("@"));
  const out = [];
  // 值 → {rgb, alpha};transparent/none → 全透明;别名逐级解开;算不出返回 null。
  const colorOf = (tokens, value, seen = new Set()) => {
    if (value === undefined || /^(?:transparent|none)$/.test(value)) return { rgb: [0, 0, 0], alpha: 0 };
    const alias = value.match(/^var\((--[a-z0-9-]+)\)$/);
    if (alias) return seen.has(alias[1]) ? null : colorOf(tokens, tokens[alias[1]] ?? "", seen.add(alias[1]));
    const hex = value.match(/^#([0-9a-fA-F]{6})([0-9a-fA-F]{2})?$/);
    if (!hex) return null;
    return { rgb: [0, 1, 2].map((i) => parseInt(hex[1].slice(i * 2, i * 2 + 2), 16)), alpha: hex[2] ? parseInt(hex[2], 16) / 255 : 1 };
  };
  const over = (top, base) => top.rgb.map((c, i) => c * top.alpha + base[i] * (1 - top.alpha));
  const lum = (rgb) => rgb.reduce((sum, c, i) => {
    const v = c / 255;
    return sum + [0.2126, 0.7152, 0.0722][i] * (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  }, 0);
  const ratio = (a, b) => {
    const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
    return (hi + 0.05) / (lo + 0.05);
  };
  const chips = new Map();
  for (const { branches } of rules) {
    for (const branch of branches) {
      const family = CHIP_FAMILIES.find(([pattern]) => pattern.test(branch));
      if (family && !chips.has(branch)) chips.set(branch, family[1]);
    }
  }
  for (const [chip, host] of chips) {
    // 同一选择器的多条规则按源码顺序合并(后写的覆盖先写的)。
    const decl = {};
    for (const { branches, body } of rules) {
      if (!branches.includes(chip)) continue;
      for (const m of body.matchAll(/(?:^|;)\s*(background(?:-color)?|color)\s*:\s*([^;]+)/g)) {
        decl[m[1] === "color" ? "color" : "background"] = m[2].trim();
      }
    }
    if (!decl.color) continue; // 没有自己的字色 = 继承宿主正文,已由 ③ 覆盖
    for (const [theme, tokens] of themes) {
      const fg = colorOf(tokens, decl.color);
      const chipBg = colorOf(tokens, decl.background);
      if (!fg || !chipBg) {
        out.push(`③b ${theme} ${chip}:字色 "${decl.color}" / 底色 "${decl.background ?? "无"}" 不是 token 或 transparent,无法按合成底色计算对比度。改法:胶囊的字与底都用语义 token`);
        continue;
      }
      for (const [label, layers] of CHIP_HOSTS[host]) {
        let base = null;
        for (const layer of layers) {
          const c = colorOf(tokens, `var(${layer})`);
          base = base ? over(c, base) : c.rgb;
        }
        const bg = over(chipBg, base);
        const r = ratio(over(fg, bg), bg);
        if (r < 4.5) {
          out.push(`③b ${theme} ${chip}(${decl.color} 叠在 ${decl.background ?? "透明底"} 上,放在${label} ${layers.join(" + ")})合成后 ${r.toFixed(2)} < 4.5。改法:带半透明底的中性胶囊字色用 --fg 以上,或换不透明底;纯文字换更亮/更深的语义色`);
        }
      }
    }
  }
  return out;
}
{
  const violations = chipContrastViolations(css);
  assert.deepEqual(violations, [], `叠色对比度(③b)未通过:\n${violations.join("\n")}`);
  // 自测:复核实测的三种回退都必须红;类族找不到胶囊也红(判据定位失效)。
  const counted = new Set();
  for (const [pattern] of CHIP_FAMILIES) {
    const hit = [...css.matchAll(/([^{}]+)\{/g)].some((m) => m[1].split(",").some((b) => pattern.test(b.trim())));
    if (hit) counted.add(String(pattern));
  }
  assert.equal(counted.size, CHIP_FAMILIES.length, `③b 自测:有胶囊类族在 style.css 里一条规则都匹配不到(判据定位失效):${CHIP_FAMILIES.map(([p]) => String(p)).filter((p) => !counted.has(p)).join(", ")}`);
  const rootStart = css.indexOf(":root {");
  const rootEnd = css.indexOf("}", rootStart);
  const setRoot = (name, value) => {
    const block = css.slice(rootStart, rootEnd);
    const pattern = new RegExp(`(${name}:\\s*)[^;]+;`);
    assert.ok(pattern.test(block), `③b 自测::root 里找不到 ${name}`);
    return css.slice(0, rootStart) + block.replace(pattern, `$1${value};`) + css.slice(rootEnd);
  };
  const counterexamples = [
    ["P2 胶囊回到 --dim 叠 --badge-soft", `${css}\n.pri-badge.P2 { background: var(--badge-soft); color: var(--dim); }`],
    ["待办胶囊字色单独改回 --dim(底色来自另一条规则)", `${css}\n.st-todo { color: var(--dim); }`],
    ["项目卡空闲胶囊 --muted 叠 --badge-soft", `${css}\n.workspace-status.idle { color: var(--muted); }`],
    ["暗色 --dim 回到 #9a9a9a(焦点卡悬停 4.37)", setRoot("--dim", "#9a9a9a")],
  ];
  const silent = counterexamples.filter(([, mutated]) => chipContrastViolations(mutated).length === 0).map(([label]) => label);
  assert.deepEqual(silent, [], `叠色对比度判据没能命中自己的反例(恒绿):${silent.join(";")}`);
}

// ---------- UI-0926 配色 ⑥ 颜色语义:一种含义一种颜色(docs/design/ui_color_semantics.md) ----------
// ①-⑤ 只管颜色有没有 token 化、够不够对比、选中/焦点是否中性,不管「这个颜色表达什么」:同一个琥珀
// 同时是 P1、阻塞和运行中,绿色同时是空闲和完成,蓝色一处扛了十种含义,优先级在一行里画三遍,
// 全都绿着通过。这里把语义表变成机械判据,并照 selfTestSurfaceRules 的做法给每条判据喂反例:
// 判据恒绿就先在这里红。
//   橙 = 进行中(+品牌/发送键/真链接/一次性「看这里」);琥珀 = 需要注意(含等你批准/回答);
//   绿 = 一件工作成功收尾;红 = 失败与 P0;灰 = 其余一切;蓝(--info)只给代码/JSON 着色。
function colorSemanticsViolations(styleText, surfaceText = "") {
  const strip = (text) => text.replace(/\/\*[\s\S]*?\*\//g, "");
  const clean = strip(styleText);
  const rules = [...`${clean}\n${strip(surfaceText)}`.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .map((m) => ({ branches: m[1].split(/,(?![^(]*\))/).map((b) => b.trim().replace(/\s+/g, " ")), body: m[2] }))
    .filter((rule) => rule.branches[0] && !rule.branches[0].startsWith("@"));
  const out = [];
  // 中性语义的选择器:待办/空闲/可执行/被取得/被读取/身份/来源/工具单步成功/进度格与进度条/P1-P3 胶囊/
  // 记忆生效/排队投递/研究 V1。按前缀匹配(选择器本身及其后代、伪类、伪元素都算)。
  const NEUTRAL = [
    ".st-todo", ".st-open", ".st-draft", ".st-active", ".backlog-stat.workable", ".doc-claim-fact", ".dep-layer-head.ready",
    '.kz-dot[data-state="idle"]', '.kz-dot[data-state="stopping"]', '.kz-glyph[data-state="idle"]', '.kz-glyph[data-state="stopping"]',
    ".doc-item .complexity-cell.filled", ".focus-card .complexity-cell.filled", ".tf-progress-fill",
    ".pri-badge.P1", ".pri-badge.P2", ".pri-badge.P3", ".pri-badge.unset", ".workspace-status.idle",
    ".line-agent-code", ".sa-agent", ".picker-source",
    ".tool-msg.ok .tool-msg-status", ".tool-chip.ok .head::before", ".bg-entry.ok .bg-title::before",
    ".doc-archive-toggle", ".archived-entry",
    ".memory-status-badge.active", ".memory-recall-hit.read .memory-recall-flag", ".queue-entry .queue-delivery", ".v-badge.v-v1",
    // 鞭挞开关圆点与阶段字的「静息态」(选择器不带 [data-phase] 前缀 = 开着但待命、或尚未开):中性。
    // 推进中 / 等待下一轮以 .autorun-bar[data-phase=…] 开头,不在此列,由下面的 ⑥w 要求强调色。
    "#auto-continue-wrap", ".auto-phase",
  ];
  // 编号/路径引用:平时中性,悬停/聚焦才变橙(语义表「引用中性 + 虚下划线,悬停才变橙」)。同样按前缀匹配,
  // 但 :hover / :focus* 分支放行——.ref-link:hover 用 --accent-text 是合法的,只按前缀匹配会误判。
  const NEUTRAL_AT_REST = [".ref-link", ".sv-chip.sv-ref", ".sv-chip.sv-path", "a.md-path"];
  const STATUS_VAR = /var\(--(?:ok|warn|alert|info|err|danger|accent|dot-run|line-[1-4]|badge-(?:ok|warn|info|alert|err))/;
  // ⑥b 用:状态色 token,含别名与它们的 -soft/-text 变体。
  const EDGE_STATUS_VAR = String.raw`var\(--(?:ok|warn|alert|info|err|danger|accent|dot-run|surface-attention|log-gold|arch-unindexed|diff-add|diff-del|badge-(?:ok|warn|info|alert|err))(?:-[a-z]+)?\)`;
  // 列表行与卡片本身(选择器的主体 = 最后一个复合选择器):任何边框、box-shadow、::before/::after 底色都不准用状态色。
  const ROW_CARD = /\.(?:doc-item|focus-card|memory-row|memory-candidate|work-unit-card|metrics-round)(?![\w-])/;
  const ROW_CARD_EDGE = new RegExp(String.raw`(?:^|;)\s*(?:border(?:-left|-inline-start)?(?:-color)?|box-shadow)\s*:[^;]*` + EDGE_STATUS_VAR);
  const PSEUDO_FILL = new RegExp(String.raw`(?:^|;)\s*background(?:-color)?\s*:[^;]*` + EDGE_STATUS_VAR);
  // 全局:任何元素都不准用状态色画左侧竖条(border-left / border-inline-start / 横向偏移的 inset 阴影)。
  // 显式例外(见 ui_color_semantics.md §7):活动面板子行 .bg-child 的左边框就是它的状态位;
  // 自检失败项 .sv-check-fail 是引文式的缩进块。新增例外必须登记在这里并在设计文档写明理由。
  const STRIPE = new RegExp(String.raw`(?:^|;)\s*(?:border-(?:left|inline-start)(?:-color)?\s*:[^;]*|box-shadow\s*:[^;]*\binset\s+-?(?:\d*\.)?\d*[1-9]\d*px\s+0(?:px)?\s+0(?:px)?\s[^;]*)` + EDGE_STATUS_VAR);
  const STRIPE_EXCEPTIONS = new Set([".bg-child.warn", ".bg-child.err", ".bg-child.running", ".sv-check-fail"]);
  const subjectOf = (branch) => branch.split(/\s*[>+~]\s*|\s+(?![^(]*\))/).pop();
  for (const { branches, body } of rules) {
    const selector = branches.join(", ");
    // (a) 优先级只编码一次(行内胶囊):不得再画竖条(::before)、给编号(.id)染色、给批次格换色。
    for (const branch of branches) {
      if (/\.pri-P[0-3](?![\w-])/.test(branch) && /\.complexity-cell|::?before|\s\.id(?![\w-])/.test(branch)) {
        out.push(`⑥a 优先级在胶囊之外又编码了一遍:${branch}。改法:优先级只写在 .pri-badge 上(P0 红,P1/P2 中性,P3 描边)`);
      }
    }
    // (b) 列表行/卡片不画彩色竖条或描边(Codex 没有;五种颜色各说各话)。只查 border-left 拦不住换个写法:
    //     box-shadow: inset 3px 0 0、整圈 border-color、border-inline-start、::before 底色条都算。
    const rowCards = branches.filter((branch) => ROW_CARD.test(subjectOf(branch)));
    const rowCardHits = rowCards.filter((branch) => ROW_CARD_EDGE.test(body) || (/::?(?:before|after)\b/.test(subjectOf(branch)) && PSEUDO_FILL.test(body)));
    if (rowCardHits.length) {
      out.push(`⑥b 列表行/卡片本身用状态色画了竖条/描边/阴影:${rowCardHits.join(", ")}。改法:状态用文字胶囊或字形表达,边框与阴影保持中性`);
    }
    const stripes = branches.filter((branch) => !STRIPE_EXCEPTIONS.has(branch) && !rowCards.includes(branch));
    if (stripes.length && STRIPE.test(body)) {
      out.push(`⑥b 彩色左竖条:${stripes.join(", ")}。改法:状态用文字胶囊或字形表达;确属「边框即状态位」的,登记进 STRIPE_EXCEPTIONS 并在 ui_color_semantics.md 写明`);
    }
    // (c) 蓝色不表达任何状态:var(--info)/--badge-info 只准出现在语法/JSON 着色选择器里。
    if (/var\(--(?:info|badge-info)\)/.test(body) && !branches.every((branch) => /\.sv-json|\.syntax|\.hl-|\.tok-/.test(branch))) {
      out.push(`⑥c --info 被当成状态色:${selector}。改法:状态走 ok/warn/err/accent 语义表,配置/分类签一律中性`);
    }
    // (d) 中性语义不得着状态色。
    for (const branch of branches) {
      const hit = NEUTRAL.find((n) => branch === n || (branch.startsWith(n) && /^[\s:.[]/.test(branch.slice(n.length))))
        ?? NEUTRAL_AT_REST.find((n) => branch === n || (branch.startsWith(n) && /^(?:[\s.[]|:(?!hover|focus))/.test(branch.slice(n.length))));
      if (hit && STATUS_VAR.test(body)) out.push(`⑥d 中性语义 ${hit} 用了状态色:${branch} { ${body.trim()} }。改法:用 --dim/--fg/--fg-strong`);
    }
  }
  // (p) 语义表里必须着色的几处:计数零灰/阻塞琥珀、需要你 = attention 琥珀、完成绿、P0 红。
  const bodiesOf = (selector) => rules.filter((rule) => rule.branches.includes(selector)).map((rule) => rule.body).join(";");
  for (const [selector, token] of [
    [".backlog-stat.is-zero .backlog-num", "--dim"],
    [".backlog-stat.blocked .backlog-num", "--warn"],
    ['.kz-dot[data-state="attention"]', "--warn"],
    ['.kz-glyph[data-state="attention"]', "--warn"],
    ['.kz-dot[data-state="done"]', "--ok"],
    [".pri-badge.P0", "--err"],
  ]) {
    if (!bodiesOf(selector).includes(`var(${token})`)) out.push(`⑥p ${selector} 必须用 var(${token})(语义表),实际:${bodiesOf(selector) || "找不到规则"}`);
  }
  // (w) 输入区鞭挞组(UI2-0926 #11 复核):开关圆点 / 轮次 / 阶段字一律不得用绿——开关打开不是「一件工作成功收尾」;
  //     推进中与等待下一轮(pending,待续跑)属进行中家族:圆点背景必须是 --accent,pending 阶段字的颜色必须是强调色家族。
  //     复核实测圆点曾是 --ok、pending 阶段字曾是 --warn,⑥a-⑥t 都没覆盖这两个选择器。
  {
    const WHIP_SUBJECT = /^(?:#auto-continue-wrap|\.auto-phase|\.auto-progress)(?![\w-])/;
    const OK_VAR = /var\(--(?:ok|badge-ok|diff-add)(?:-[a-z]+)?\)/;
    // 主体取法同 subjectOf,但先把括号里的内容抹掉::has(> input:checked) 里的 > 不是组合符(subjectOf 会在那里切开)。
    const whipSubject = (branch) => subjectOf(branch.replace(/\((?:[^()]|\([^()]*\))*\)/g, "()"));
    const declValues = (body, prop) => [...body.matchAll(new RegExp(String.raw`(?:^|;)\s*${prop}\s*:\s*([^;]+)`, "g"))].map((m) => m[1].trim());
    const phaseRules = (phase, subject) => rules.filter((rule) => rule.branches.some((branch) => branch.includes(`[data-phase="${phase}"]`) && whipSubject(branch) === subject));
    for (const { branches, body } of rules) {
      const whip = branches.filter((branch) => WHIP_SUBJECT.test(whipSubject(branch)));
      if (whip.length && OK_VAR.test(body)) out.push(`⑥w 鞭挞组用了成功绿:${whip.join(", ")} { ${body.trim()} }。改法:开着待命 = 中性(currentColor),推进中/等待下一轮 = --accent`);
    }
    for (const phase of ["running", "pending"]) {
      const fills = phaseRules(phase, "#auto-continue-wrap::before").flatMap((rule) => declValues(rule.body, "background(?:-color)?"));
      if (!fills.some((value) => value === "var(--accent)") || fills.some((value) => value !== "var(--accent)")) {
        out.push(`⑥w .autorun-bar[data-phase="${phase}"] 的开关圆点背景必须是 var(--accent)(进行中家族),实际:${fills.join(" / ") || "找不到规则"}`);
      }
    }
    const pendingText = phaseRules("pending", ".auto-phase").flatMap((rule) => declValues(rule.body, "color"));
    if (!pendingText.length || pendingText.some((value) => !/^var\(--accent(?:-text)?\)$/.test(value))) {
      out.push(`⑥w 等待下一轮(pending)的阶段字颜色必须是 --accent-text(进行中家族,与活动行、kz-dot pending 一致),实际:${pendingText.join(" / ") || "找不到规则"}`);
    }
  }
  // (e) 别名 token 在 :root 只定义一次、指向语义表里的那一个颜色,亮色块不得重给字面值——
  //     重给就把「一种含义一种颜色」又拆回两套(旧版 --alert/--log-gold/--arch-unindexed 与 --warn 同值四个名)。
  const tokenBlock = (pattern) => Object.fromEntries(
    [...(clean.match(pattern)?.[1] ?? "").matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]),
  );
  const dark = tokenBlock(/:root\s*\{([^}]*)\}/);
  const light = tokenBlock(/\[data-theme="light"\]\s*\{([^}]*)\}/);
  // (t) 主题 token 块里只能有声明。注释里写了星号紧跟斜杠会提前结束注释,剩下的文字成了一条非法声明,
  //     一直吞到下一个分号——实测曾把亮色块的 --bg: #ffffff 吞掉,文档页/线路页在亮色下整块是 #181818,
  //     而上面按声明解析的判据照样读到 #ffffff、全绿。所以看剩余物:剥掉注释与全部声明后一个字都不能剩。
  for (const [label, pattern] of [[":root", /:root\s*\{([^}]*)\}/], ['[data-theme="light"]', /\[data-theme="light"\]\s*\{([^}]*)\}/]]) {
    const residue = (clean.match(pattern)?.[1] ?? "").replace(/(?:--[a-z0-9-]+|color-scheme)\s*:\s*[^;]+;/g, "").trim();
    if (residue) out.push(`⑥t ${label} 主题块里有不是声明的残留(多半是注释被星号加斜杠提前结束,吞掉了后面的声明):${residue.slice(0, 80)}`);
  }
  const ALIASES = {
    "--alert": "--warn", "--log-gold": "--warn", "--arch-unindexed": "--warn", "--surface-attention": "--warn",
    "--badge-alert": "--badge-warn", "--dot-idle": "--dim", "--dot-run": "--accent",
    "--memory-flow": "--accent", "--memory-flow-hot": "--accent-text",
    "--statusbar-fg": "--dim", "--statusbar-run-fg": "--accent-text", "--diff-add": "--ok", "--diff-del": "--err",
  };
  for (const [name, target] of Object.entries(ALIASES)) {
    if (dark[name] !== `var(${target})`) out.push(`⑥e ${name} 在 :root 必须是 var(${target}) 的别名,实际:${dark[name] ?? "未定义"}`);
    if (name in light) out.push(`⑥e ${name} 是别名,不得在 [data-theme="light"] 里重给值(${light[name]})`);
  }
  for (const name of ["--line-1", "--line-2", "--line-3", "--line-4"]) {
    if (name in dark && !/^var\(--/.test(dark[name])) out.push(`⑥e ${name} 已弃用(身份不用色),只准保留为中性别名,实际:${dark[name]}`);
    if (name in light) out.push(`⑥e ${name} 不得在亮色块里重给值`);
  }
  // (g) 暗色表面只靠明度分层、对齐 Codex:主区最深 < 侧栏 < 输入区;代码块比主区亮(否则看不见边);全部 R=G=B。
  const rgbOf = (name, seen = new Set()) => {
    const value = dark[name];
    const alias = value?.match(/^var\((--[a-z0-9-]+)\)$/);
    if (alias && !seen.has(alias[1])) return rgbOf(alias[1], seen.add(name));
    const hex = value?.match(/^#([0-9a-fA-F]{6})$/);
    return hex ? [0, 1, 2].map((i) => parseInt(hex[1].slice(i * 2, i * 2 + 2), 16)) : null;
  };
  const lum = (name) => {
    const rgb = rgbOf(name);
    if (!rgb) return NaN;
    const lin = (c) => ((c / 255) <= 0.03928 ? c / 255 / 12.92 : ((c / 255 + 0.055) / 1.055) ** 2.4);
    return 0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2]);
  };
  const surfaces = ["--bg", "--sidebar-bg", "--surface-raised", "--code-bg", "--panel", "--panel2", "--surface-overlay"];
  for (const name of surfaces) {
    const rgb = rgbOf(name);
    if (!rgb) out.push(`⑥g 暗色表面 ${name} 不是 6 位十六进制,无法校验层级:${dark[name] ?? "未定义"}`);
    else if (!(rgb[0] === rgb[1] && rgb[1] === rgb[2])) out.push(`⑥g 暗色表面 ${name} 不是纯中性(R=G=B):${dark[name]}`);
  }
  if (!(lum("--bg") < lum("--sidebar-bg") && lum("--sidebar-bg") < lum("--surface-raised"))) {
    out.push(`⑥g 暗色表面层级倒置:须 L(--bg ${dark["--bg"]}) < L(--sidebar-bg ${dark["--sidebar-bg"]}) < L(--surface-raised ${dark["--surface-raised"]})(主区最深、侧栏亮一档、输入区浮起——倒过来就是「发灰」)`);
  }
  if (!(lum("--code-bg") > lum("--bg"))) {
    out.push(`⑥g 代码块 --code-bg ${dark["--code-bg"]} 必须比主区 --bg ${dark["--bg"]} 亮,否则代码块在主区上看不见边`);
  }
  // 只比大小拦不住「退回旧值」:旧主区 #1e1e1e 仍比侧栏 #1f1f1f 暗一级、旧输入区 #262626 仍比主区亮,
  // 而这两个值正是「发灰」的根因(复核 2026-09-26 实测变异全绿)。所以再要求最小明度级差
  // (WCAG 对比度同一公式):侧栏/代码块比主区 ≥ 1.05(Codex #1f1f1f:#181818 = 1.08),
  // 输入区比主区 ≥ 1.3(Codex #303030:#181818 = 1.35,旧 #262626 只有 1.17)。
  const step = (hi, lo) => (lum(hi) + 0.05) / (lum(lo) + 0.05);
  for (const [hi, floor, why] of [
    ["--sidebar-bg", 1.05, "侧栏要比主区亮一档,左右分得开"],
    ["--code-bg", 1.05, "代码块要在主区上看得出边"],
    ["--surface-raised", 1.3, "输入区要明显浮起"],
  ]) {
    const got = step(hi, "--bg");
    if (!(got >= floor)) out.push(`⑥g 暗色表面级差不够:(L(${hi} ${dark[hi]})+.05)/(L(--bg ${dark["--bg"]})+.05) = ${got.toFixed(3)} < ${floor}(${why})`);
  }
  return out;
}
{
  const surfaceCss = await readFile(resolve(root, "crates/kanzei-app/ui/surface.css"), "utf8");
  const violations = colorSemanticsViolations(css, surfaceCss);
  assert.deepEqual(violations, [], `颜色语义门禁(⑥)未通过:\n${violations.join("\n")}`);
  // 自测:每条判据喂一个反例,必须报出对应编号;锚点找不到也要红(判据定位失效)。
  const rootStart = css.indexOf(":root {");
  const rootEnd = css.indexOf("}", rootStart);
  assert.ok(rootStart >= 0 && rootEnd > rootStart, "⑥ 自测:找不到 :root 块");
  const mutateRoot = (name, value) => {
    const block = css.slice(rootStart, rootEnd);
    const pattern = new RegExp(`(${name}:\\s*)[^;]+;`);
    assert.ok(pattern.test(block), `⑥ 自测::root 里找不到 ${name}`);
    return css.slice(0, rootStart) + block.replace(pattern, `$1${value};`) + css.slice(rootEnd);
  };
  const lightOpen = '[data-theme="light"] {';
  const lightAt = css.indexOf(lightOpen);
  assert.ok(lightAt > 0, "⑥ 自测:找不到亮色块");
  const withLight = (decl) => `${css.slice(0, lightAt + lightOpen.length)}\n  ${decl}${css.slice(lightAt + lightOpen.length)}`;
  const dropRule = (pattern) => {
    assert.ok(pattern.test(css), `⑥ 自测:找不到要删的规则 ${pattern}`);
    return css.replace(pattern, "");
  };
  const counterexamples = [
    ["⑥a", `${css}\n.doc-item.pri-P1 .complexity-cell.filled { background: var(--warn); }`],
    ["⑥a", `${css}\n.doc-item.pri-P0::before { background: var(--err); }`],
    ["⑥b", `${css}\n.focus-card.blocked { border-left-color: var(--warn); }`],
    // 复核实测的绕过写法:inset 阴影竖条、整圈描边、::before 底色条、度量页轮次行、行卡以外的 inline-start 竖条。
    ["⑥b", `${css}\n.doc-item.agent-active { box-shadow: inset 3px 0 0 var(--accent); }`],
    ["⑥b", `${css}\n.focus-card.blocked { border-color: var(--warn); }`],
    ["⑥b", `${css}\n.doc-item.agent-active::before { content: ""; background: var(--accent); }`],
    ["⑥b", `${css}\n.metrics-round.halted { border-left: 2px solid var(--warn); }`],
    ["⑥b", `${css}\n.activity-row.failed { border-inline-start: 2px solid var(--err); }`],
    ["⑥b", `${css}\n.activity-row.failed { box-shadow: inset 2px 0 0 var(--danger); }`],
    ["⑥c", `${css}\n.queue-entry .queue-delivery { color: var(--info); }`],
    ["⑥d", `${css}\n.kz-dot[data-state="idle"] { background: var(--ok); }`],
    ["⑥d", `${css}\n.backlog-stat.workable .backlog-num { color: var(--ok); }`],
    // 引用超载强调橙(根因之一)、批次进度条部分完成也是绿:
    ["⑥d", `${css}\n.ref-link { color: var(--accent-text); }`],
    ["⑥d", `${css}\n.sv-chip.sv-ref { color: var(--accent-text); }`],
    ["⑥d", `${css}\n.tf-progress-fill { background: var(--ok); }`],
    ["⑥e", withLight("--dot-idle: #1d7a3c;")],
    ["⑥t", withLight("/* 别名 --dot-*/--memory-flow 只在 :root 定义 */")],
    ["⑥e", mutateRoot("--alert", "#dcb45e")],
    ["⑥g", mutateRoot("--sidebar-bg", "#000000")],
    ["⑥g", mutateRoot("--code-bg", "#000000")],
    ["⑥g", mutateRoot("--surface-raised", "#2a2530")],
    // 退回旧值(方案 tests 点名):主区 #1e1e1e、输入区 #262626——只比大小时两条都是绿的。
    ["⑥g", mutateRoot("--bg", "#1e1e1e")],
    ["⑥g", mutateRoot("--surface-raised", "#262626")],
    ["⑥p", dropRule(/\.backlog-stat\.is-zero \.backlog-num \{[^}]*\}/)],
    ["⑥p", dropRule(/\.kz-glyph\[data-state="attention"\] \{[^}]*\}/)],
    // 输入区鞭挞组(复核):开关一开就是绿点、推进中圆点换成别的色、等待下一轮的阶段字用琥珀、待命圆点染强调色。
    ["⑥w", `${css}\n#auto-continue-wrap:has(> input:checked)::before { border: 0; background: var(--ok); }`],
    ["⑥w", dropRule(/\.autorun-bar:is\(\[data-phase="running"\], \[data-phase="pending"\]\) #auto-continue-wrap::before \{[^}]*\}/)],
    ["⑥w", `${css}\n.autorun-bar:is([data-phase="pending"], [data-phase="paused"]) .auto-phase { color: var(--warn); }`],
    ["⑥d", `${css}\n#auto-continue-wrap:has(> input:checked)::before { background: var(--accent); }`],
  ];
  const silent = counterexamples
    .map(([id, mutated], index) => [`${id}#${index}`, colorSemanticsViolations(mutated, surfaceCss).some((v) => v.startsWith(id))])
    .filter(([, caught]) => !caught)
    .map(([label]) => label);
  assert.deepEqual(silent, [], `颜色语义判据没能命中自己的反例(恒绿):${silent.join(", ")}`);
}
// ── 分区:侧栏与需求页 ──
// UI2-0926 #5「需求页面缺少字体之间的间隔和明暗关系」:明暗三档写成机械判据——标题是全行最亮最大的字
// (--fg-strong 14px),状态/复杂度/组头是暗而小的元数据(--dim 12px);勾选框用 opacity 隐藏(键盘仍可 Tab 到,
// 不得用 display:none / visibility:hidden);列表不再套外框、组头不再画实线。颜色语义归 ⑥,这里只管字阶。
{
  const clean = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const bodyOf = (selector) => [...clean.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter((m) => m[1].split(/,(?![^(]*\))/).map((b) => b.trim().replace(/\s+/g, " ")).includes(selector))
    .map((m) => m[2]).join(";");
  const expectDecl = (selector, pattern, why) => assert.match(bodyOf(selector), pattern, `#5 字阶:${selector} ${why}(实际:${bodyOf(selector).trim() || "找不到规则"})`);
  expectDecl(".documents-list .doc-row .title", /color:\s*var\(--fg-strong\)/, "标题必须是最亮的 --fg-strong");
  expectDecl(".documents-list .doc-row .title", /font-size:\s*var\(--fs-14\)/, "标题必须比元数据大一档(14px)");
  expectDecl(".documents-list .doc-row .st", /color:\s*var\(--dim\)/, "状态列是元数据,默认暗色(在做/完成的语义色由 :is() 分支覆盖)");
  expectDecl(".documents-list .doc-row .st", /font-size:\s*var\(--fs-12\)/, "状态列 12px");
  expectDecl(".documents-list .doc-row .complexity-badge", /color:\s*var\(--dim\)/, "复杂度列暗色");
  expectDecl(".documents-list .doc-group-head", /color:\s*var\(--dim\)/, "组头暗色");
  expectDecl(".documents-list .doc-group-head", /border-bottom:\s*0/, "组头不再画实线");
  expectDecl(".documents-list .doc-row", /min-height:\s*36px/, "行高统一 36px");
  expectDecl(".documents-list .doc-pick", /opacity:\s*0/, "勾选框平时用 opacity 隐藏");
  assert.doesNotMatch(bodyOf(".documents-list .doc-pick"), /display:\s*none|visibility:\s*hidden/, "#5 勾选框不得用 display:none/visibility:hidden 隐藏(键盘 Tab 不到)");
  expectDecl(".documents-list.has-selection .doc-pick", /opacity:\s*1/, "已有选中时勾选框整列常显");
  assert.doesNotMatch(bodyOf(".documents-list"), /border:\s*1px/, "#5 需求列表不再套外框");
}

// 彩色 emoji 绕过调色板:⚡ 必须带 U+FE0E 变成文字字形(HTML 里写 ⚡&#xFE0E;),才继承 CSS color、随主题取色。
// 状态栏「自动放行」是静态 HTML;输入区芯片与状态栏 kz:meta 的 ⚡ 由 ui-runtime-smoke 实渲染断言。
{
  assert.ok(html.includes('id="status-auto-allow"') && /id="status-auto-allow"[^>]*>⚡&#xFE0E; /.test(html), "状态栏「自动放行」的 ⚡ 必须写成 ⚡&#xFE0E;(文字字形,随主题取色)");
  const bareBolts = [...html.matchAll(/⚡(?!&#xFE0E;|︎)/g)].length;
  assert.equal(bareBolts, 0, `index.html 里有 ${bareBolts} 处不带 U+FE0E 的 ⚡:彩色 emoji 不受 CSS color 控制,写成 ⚡&#xFE0E;`);
}

// ── 分区:对话单列与输入区 ──
// UI2-0926 #12(docs/design/chat_presentation_contract.md §4.4):对话区只有一条列宽真源。消息 pane、运行活动行、
// 输入区三处宽度必须是同一个表达式;#messages 左右对称(不得再给 OC 立绘留右沟,旧版 222px 让整列左移 100px);
// 工具组折叠态失败行常驻、单行组不显示组头;组头转圈守 #7 动效纪律。判据自带反例自测(每条喂一条必须命中的样本)。
{
  const COLUMN_EXPR = "min(var(--chat-col), 100cqi - 2 * var(--chat-gutter))";
  const cssText = css.replace(/\r\n/g, "\n");
  const strip = (text) => text.replace(/\/\*[\s\S]*?\*\//g, "");
  const bodiesOf = (text, selector) => {
    const out = [];
    for (const match of strip(text).matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
      const selectors = match[1].split(/,(?![^(]*\))/).map((part) => part.trim().replace(/\s+/g, " "));
      if (selectors.includes(selector)) out.push(match[2].replace(/\s+/g, " "));
    }
    return out;
  };
  const columnViolations = (text) => {
    const out = [];
    const clean = strip(text);
    if (/padding-right:\s*222px/.test(clean)) out.push("#messages 仍给 OC 立绘预留 222px 右沟(OC 关着也整列左移)");
    const view = bodiesOf(text, "#view-chat").join(";");
    if (!/--chat-col:\s*768px/.test(view) || !/--chat-gutter:\s*\d+px/.test(view) || !/container-type:\s*inline-size/.test(view)) {
      out.push("#view-chat 必须定义 --chat-col / --chat-gutter 并是 inline-size 容器(列宽唯一真源)");
    }
    for (const selector of ["#messages > .msg-pane", "#turn-activity", "#composer"]) {
      const bodies = bodiesOf(text, selector);
      if (!bodies.some((body) => body.includes(`width: ${COLUMN_EXPR}`))) out.push(`${selector} 的宽度不是列宽表达式 ${COLUMN_EXPR}`);
      if (bodies.some((body) => /max-width:\s*1080px/.test(body))) out.push(`${selector} 仍自带 1080px 上限(第二个列宽真源)`);
    }
    const messages = bodiesOf(text, "#messages").join(";");
    if (!/scrollbar-gutter:\s*stable both-edges/.test(messages)) out.push("#messages 缺 scrollbar-gutter: stable both-edges(滚动条让 pane 比输入区偏 4px)");
    if (!/padding:\s*\d+px 0 \d+px/.test(messages)) out.push("#messages 左右内边距必须为 0(列宽由 pane 自己决定)");
    // 复核:错误卡的「重试」常驻在流内(复制行悬停才显形,重试若跟着藏起来,出错时还得先找按钮)。
    if (!bodiesOf(text, ".msg.error > .msg-actions").some((body) => /position:\s*static/.test(body) && /opacity:\s*1\b/.test(body))) out.push("错误卡的「重试」不再常驻:.msg.error > .msg-actions 须 position: static + opacity: 1");
    if (!bodiesOf(text, '.tool-group:not([data-expanded="1"], [data-count="1"]) > .tool-group-body:has(> .tool-msg.err)').some((body) => /display:\s*flex/.test(body))) {
      out.push("工具组折叠态失败行不再常驻可见(契约 §4.1「错了不该藏起来」)");
    }
    if (!bodiesOf(text, '.tool-group[data-count="1"] > .tool-group-head').some((body) => /display:\s*none/.test(body))) out.push("单行工具组仍显示组头(1 次调用就是那一行)");
    const motion = clean.slice(clean.indexOf("#turn-activity {"));
    if (!/\.tool-group\[data-running="1"\] \.tool-group-spin\s*\{[^}]*animation:\s*kz-spin var\(--motion-spin\)/.test(motion)) out.push("工具组运行中转圈缺失、没走 --motion-spin 或不在动效分区");
    if (!/prefers-reduced-motion[\s\S]*\.tool-group\[data-running="1"\] \.tool-group-spin[^{]*\{[^}]*kz-breathe/.test(clean)) out.push("减少动效时工具组转圈没有退化为慢呼吸");
    return out;
  };
  const found = columnViolations(cssText);
  assert.deepEqual(found, [], `对话单列静态门禁未通过:\n - ${found.join("\n - ")}`);
  const swap = (from, to) => {
    assert.ok(cssText.includes(from), `对话单列自测:样式表里找不到 ${from}`);
    return cssText.replace(from, to);
  };
  const counterexamples = [
    ["222px", `${cssText}\n#messages:not(:has(.empty-state)) { padding-right: 222px; }`],
    ["列宽真源", swap("--chat-col: 768px;", "--chat-width: 768px;")],
    ["pane 宽度", swap(`width: ${COLUMN_EXPR}; margin: 0 auto;\n}`, "width: 100%; max-width: 1080px; margin: 0 auto;\n}")],
    ["活动行宽度", swap(`gap: 8px; width: ${COLUMN_EXPR};`, "gap: 8px; width: calc(100% - 48px); max-width: 1080px;")],
    ["滚动条对称", swap("scrollbar-gutter: stable both-edges;", "")],
    ["失败常驻", swap('.tool-group:not([data-expanded="1"], [data-count="1"]) > .tool-group-body:has(> .tool-msg.err) { display: flex; }', "")],
    ["单行组头", swap('.tool-group[data-count="1"] > .tool-group-head { display: none; }', "")],
    ["重试常驻", swap(".msg.error > .msg-actions { position: static; opacity: 1; margin-top: 8px; }", ".msg.error > .msg-actions { margin-top: 8px; }")],
    ["组转圈", swap('.tool-group[data-running="1"] .tool-group-spin { display: inline-block; animation: kz-spin var(--motion-spin) linear infinite; }', '.tool-group[data-running="1"] .tool-group-spin { display: inline-block; }')],
  ];
  const silent = counterexamples.filter(([, text]) => columnViolations(text).length === 0).map(([label]) => label);
  assert.deepEqual(silent, [], `对话单列判据没能命中自己的反例(恒绿):${silent.join(", ")}`);

  // UI2-0926 #11 输入区控件几何(docs/design/ui_surface_stack.md「输入区控件几何」):一套 28px 静默盒子 .kz-ctl;
  // 全部 select 垂直居中(base-select 的 UA 盒子 align-items 是 normal,固定高度时文字贴顶);鞭挞组容器不画框不加底
  // (旧版容器边框与内部胶囊描边叠成双线);下拉箭头统一细 V 形遮罩;「继续」只在空闲、「排队」只在运行时出现。
  // 像素级判据(等高、居中、无叠压、墨迹)在浏览器里量,见 scripts/ui-composer-geometry.mjs。
  const surfaceText = (await readFile(resolve(root, "crates/kanzei-app/ui/surface.css"), "utf8")).replace(/\r\n/g, "\n");
  const htmlText = html.replace(/\r\n/g, "\n");
  const composerViolations = (text, surface, markup) => {
    const out = [];
    if (!bodiesOf(text, "select").some((body) => /align-items:\s*center/.test(body))) out.push("基础 select 规则缺 align-items: center(模式芯片文字贴在上半截)");
    if (!/--ctl-h:\s*28px/.test(strip(text))) out.push("缺控件高度 token --ctl-h: 28px");
    if (!bodiesOf(text, ".kz-ctl").some((body) => /height:\s*var\(--ctl-h\)/.test(body))) out.push(".kz-ctl 的高度不是 var(--ctl-h)");
    for (const gone of ["ctx-select", "seg-btn", "seg-select", "composer-secondary", "composer-actions"]) {
      if (new RegExp(`\\.${gone}\\b`).test(strip(text)) || new RegExp(`class="[^"]*\\b${gone}\\b`).test(markup)) out.push(`旧输入区类 .${gone} 仍在(第二套控件尺寸)`);
    }
    const autorun = bodiesOf(text, ".autorun-bar").join(";");
    if (/background(?:-color)?:(?!\s*(?:none|transparent)\s*(?:;|$))/.test(autorun) || /border(?:-width)?:(?!\s*(?:0|none)\s*(?:;|$))/.test(autorun)) out.push(".autorun-bar 容器不得画框或加底(与内部胶囊描边叠成双线)");
    for (const [label, body] of [["running", bodiesOf(text, '.autorun-bar[data-phase="running"]').join(";")], ["paused", bodiesOf(text, '.autorun-bar[data-phase="paused"]').join(";")]]) {
      if (/border-color|background/.test(body)) out.push(`.autorun-bar[data-phase="${label}"] 不得给容器描边或加底(运行态只体现在开关圆点与活动行)`);
    }
    if (!/select::picker-icon\s*\{[^}]*var\(--icon-chevron\)/.test(strip(surface))) out.push("surface.css 的 select::picker-icon 没用 --icon-chevron 细 V 形(UA 实心 ▼ 字形基线不齐)");
    if (!/html:not\(\[data-kz-activity="running"\], \[data-kz-activity="stopping"\]\) #delivery-select \{ display: none; \}/.test(text)) out.push("#delivery-select 没有按 html[data-kz-activity] 门控(空闲时不该出现「排队」)");
    if (!/html:is\(\[data-kz-activity="running"\], \[data-kz-activity="stopping"\]\) #continue-btn \{ display: none; \}/.test(text)) out.push("#continue-btn 没有按 html[data-kz-activity] 门控(运行中不该出现「继续」)");
    return out;
  };
  const composerFound = composerViolations(cssText, surfaceText, htmlText);
  assert.deepEqual(composerFound, [], `输入区控件几何静态门禁未通过:\n - ${composerFound.join("\n - ")}`);
  const composerCounterexamples = [
    ["select 居中", swap("text-overflow: ellipsis; align-items: center;\n}", "text-overflow: ellipsis;\n}"), surfaceText, htmlText],
    ["控件高度", swap("height: var(--ctl-h); min-height: 0;", "height: 30px; min-height: 0;"), surfaceText, htmlText],
    ["旧类", `${cssText}\n.seg-btn { padding: 6px 10px; }`, surfaceText, htmlText],
    ["鞭挞外框", `${cssText}\n.autorun-bar[data-phase="running"] { border-color: var(--accent); background: var(--statusbar-run); }`, surfaceText, htmlText],
    ["鞭挞容器", swap("padding: 0; border: 0; border-radius: 0; background: none;", "padding: 3px 0; border: 1px solid transparent; border-radius: var(--radius); background: transparent;"), surfaceText, htmlText],
    ["picker-icon", cssText, surfaceText.replace(/select::picker-icon \{[^}]*\}/, "select::picker-icon { color: var(--surface-muted); }"), htmlText],
    ["继续门控", swap('html:is([data-kz-activity="running"], [data-kz-activity="stopping"]) #continue-btn { display: none; }', ""), surfaceText, htmlText],
  ];
  const composerSilent = composerCounterexamples.filter(([, text, surface, markup]) => composerViolations(text, surface, markup).length === 0).map(([label]) => label);
  assert.deepEqual(composerSilent, [], `输入区控件几何判据没能命中自己的反例(恒绿):${composerSilent.join(", ")}`);
}

// ── 分区:记忆图谱 ──
// docs/design/memory_knowledge_graph.md §10。画布颜色只从 --graph-* token 读(24-graph-view.js readGraphPalette),
// CSS 级联管不到画布,所以对比度与语义在这里静态把关:
//   ① 对比度:节点与实线(类别色、上下文灰、crate、模块、概念、强边)对 --bg / --panel ≥ 3(非文本);
//      标签对 --bg ≥ 4.5;选中环、检索命中环对 --bg ≥ 3。暗色与亮色都算。--graph-edge-weak 是装饰性的虚线,豁免。
//   ② 语义:四个类别色(fact/sop/habit/preference)是图谱专用的数据色,只准出现在图谱选择器里(.kz-graph-*、
//      #memory-graph-*),且必须是自己的 hex、不得借状态色;其余 --graph-* 别名不得指向状态色——唯一例外是检索命中
//      --graph-hit → --accent-text(语义表「一次性的看这里」)。
//   ③ 标记:画布 role=img + aria-describedby 状态栏;视图切换、图层、含归档、文本视图按钮带 aria-pressed;文本视图 role=tree。
// 自测:三个反例必须各自报出(列表行借类别色、概念节点借琥珀、类别色写成状态色别名)。
function memoryGraphTokenViolations(styleText) {
  const strip = (text) => text.replace(/\/\*[\s\S]*?\*\//g, "");
  const clean = strip(styleText);
  const block = (pattern) => Object.fromEntries(
    [...(clean.match(pattern)?.[1] ?? "").matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]),
  );
  const dark = block(/:root\s*\{([^}]*)\}/);
  const light = { ...dark, ...block(/\[data-theme="light"\]\s*\{([^}]*)\}/) };
  const out = [];
  const resolve = (tokens, name, seen = new Set()) => {
    const value = tokens[name];
    if (!value) return null;
    const alias = value.match(/^var\((--[a-z0-9-]+)\)$/);
    if (alias && !seen.has(alias[1])) return resolve(tokens, alias[1], seen.add(name));
    const hex = value.match(/^#([0-9a-fA-F]{6})$/);
    return hex ? [0, 1, 2].map((i) => parseInt(hex[1].slice(i * 2, i * 2 + 2), 16)) : null;
  };
  const lum = (rgb) => rgb.map((c) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  }).reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
  const ratio = (a, b) => {
    const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
    return (hi + 0.05) / (lo + 0.05);
  };
  const CATEGORY = ["--graph-fact", "--graph-sop", "--graph-habit", "--graph-preference"];
  const MARKS = [...CATEGORY, "--graph-context", "--graph-crate", "--graph-module", "--graph-concept", "--graph-edge"];
  const pairs = [
    ...MARKS.flatMap((fg) => ["--bg", "--panel"].map((bg) => [fg, bg, 3])),
    ["--graph-label", "--bg", 4.5], ["--graph-label-dim", "--bg", 4.5],
    ["--graph-focus", "--bg", 3], ["--graph-hit", "--bg", 3],
  ];
  for (const [theme, tokens] of [["暗色", dark], ["亮色", light]]) {
    for (const [fg, bg, floor] of pairs) {
      const a = resolve(tokens, fg);
      const b = resolve(tokens, bg);
      if (!a || !b) {
        out.push(`①${theme} ${fg} / ${bg} 解析不到 6 位 hex(未定义或别名链断了)`);
        continue;
      }
      const r = ratio(a, b);
      if (r < floor) out.push(`①${theme} ${fg} 在 ${bg} 上 ${r.toFixed(2)} < ${floor}。改法:调 ${fg} 的明度(亮色块与暗色块各一份)`);
    }
  }
  const STATUS = /^var\(--(?:ok|warn|alert|err|danger|info|accent|accent-text|dot-run|diff-add|diff-del|badge-[a-z]+)\)$/;
  for (const name of CATEGORY) {
    for (const [theme, tokens] of [["暗色", dark], ["亮色", block(/\[data-theme="light"\]\s*\{([^}]*)\}/)]]) {
      if (!/^#[0-9a-fA-F]{6}$/.test(tokens[name] ?? "")) out.push(`②${theme} ${name} 必须是图谱专用的 6 位 hex(不借状态色、不做别名),实际:${tokens[name] ?? "未定义"}`);
    }
  }
  for (const [name, value] of Object.entries(dark)) {
    if (!name.startsWith("--graph-") || name === "--graph-hit") continue;
    if (STATUS.test(value)) out.push(`② ${name} 指向了状态色 ${value}:图谱里颜色只表达节点种类,状态色(橙/琥珀/绿/红/蓝)在别处有专属含义`);
  }
  if (dark["--graph-hit"] !== "var(--accent-text)") out.push(`② --graph-hit 必须是 var(--accent-text)(检索命中 = 一次性的「看这里」),实际:${dark["--graph-hit"] ?? "未定义"}`);
  const rules = [...clean.replace(/:root\s*\{[^}]*\}/, "").replace(/\[data-theme="light"\]\s*\{[^}]*\}/, "").matchAll(/([^{}]+)\{([^{}]*)\}/g)];
  for (const [, selector, body] of rules) {
    const used = CATEGORY.filter((name) => body.includes(`var(${name})`));
    if (!used.length) continue;
    const branches = selector.split(",").map((b) => b.trim()).filter(Boolean);
    const outside = branches.filter((b) => !/\.kz-graph-|#memory-graph-|\.memory-graph-/.test(b));
    if (outside.length) out.push(`② 图谱类别色 ${used.join("/")} 用到了图谱之外:${outside.join(", ")}。改法:列表/徽章等界面元素的分类一律中性(ui_color_semantics.md「记忆 SOP 分类」为灰)`);
  }
  return out;
}
{
  const violations = memoryGraphTokenViolations(css);
  assert.deepEqual(violations, [], `记忆图谱配色判据未通过:\n${violations.join("\n")}`);
  const rootAt = css.indexOf(":root {");
  const counterexamples = [
    ["②", `${css}\n.memory-row.fact { color: var(--graph-fact); }`],
    ["②", css.slice(0, rootAt + 7) + "\n  --graph-concept: var(--warn);" + css.slice(rootAt + 7).replace(/--graph-concept:\s*[^;]+;/, "")],
    ["②", css.replace(/(\[data-theme="light"\]\s*\{[\s\S]*?)--graph-sop:\s*#[0-9a-fA-F]{6};/, "$1--graph-sop: var(--ok);")],
    ["①", css.replace(/(:root\s*\{[\s\S]*?)--graph-habit:\s*#[0-9a-fA-F]{6};/, "$1--graph-habit: #1c1c1c;")],
  ];
  const silent = counterexamples
    .map(([id, mutated], index) => [`${id}#${index}`, memoryGraphTokenViolations(mutated).some((v) => v.startsWith(id))])
    .filter(([, caught]) => !caught)
    .map(([label]) => label);
  assert.deepEqual(silent, [], `记忆图谱配色判据没能命中自己的反例(恒绿):${silent.join(", ")}`);
  // ③ 标记
  const tag = (id) => html.match(new RegExp(`<[a-z]+[^>]*\\bid="${id}"[^>]*>`))?.[0] ?? "";
  assert.ok(/role="img"/.test(tag("memory-graph-canvas")) && /aria-describedby="memory-graph-status"/.test(tag("memory-graph-canvas")), "#memory-graph-canvas 要有 role=img 与 aria-describedby=memory-graph-status(画布本身读不出内容)");
  assert.ok(/role="status"/.test(tag("memory-graph-status")), "#memory-graph-status 要是 role=status(悬停与布局信息写在这里)");
  assert.ok(/role="tree"/.test(tag("memory-graph-list")), "#memory-graph-list(文本视图)要是 role=tree");
  for (const id of ["memory-view-list", "memory-view-graph", "memory-graph-archived", "memory-graph-textview"]) {
    assert.ok(/aria-pressed="(?:true|false)"/.test(tag(id)), `#${id} 是开关按钮,要有 aria-pressed`);
  }
  const layerButtons = [...html.matchAll(/<button[^>]*data-layer="[a-z]+"[^>]*>/g)].map((m) => m[0]);
  assert.ok(layerButtons.length === 5 && layerButtons.every((b) => /aria-pressed="(?:true|false)"/.test(b)), "图层按钮(5 个)都要有 aria-pressed");
}

// ── 分区:文件编辑 ──
// UI2-0926 #6(docs/design/files_editor.md):文件页编辑的无障碍与颜色语义静态判据。
// ① 状态与告警:冲突横幅 role=alert(保存被拒要被读出来)、未保存 / 同步提示 role=status、只读原因 role=note;
//    保存键 aria-keyshortcuts=Control+S;比较是开关(aria-pressed);新建是带读屏名的图标键。
// ② 分隔条复用 00-frame installSplit(不另写拖拽):文件树那条的读屏名是「调整文件树宽度」,分隔条 aria-controls 指向窗格;
//    树行 aria-selected 标当前文件。
// ③ 颜色:未保存 = 琥珀(--warn,与设置页 .settings-dirty 同色,ui_color_semantics §3「配置未保存」),冲突横幅 = --alert-soft 浅底。
// 判据自带反例自测(每条喂一条必须命中的样本)。
function filesEditorA11yViolations(htmlText, cssText, sourceText) {
  const out = [];
  const tag = (id) => htmlText.match(new RegExp(`<[a-z]+[^>]*\\bid="${id}"[^>]*>`))?.[0] ?? "";
  const clean = cssText.replace(/\/\*[\s\S]*?\*\//g, "");
  const bodyOf = (selector) => [...clean.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter((m) => m[1].split(/,(?![^(]*\))/).map((b) => b.trim().replace(/\s+/g, " ")).includes(selector))
    .map((m) => m[2]).join(";");
  if (!/role="alert"/.test(tag("files-conflict"))) out.push("①a #files-conflict 必须 role=alert(保存被拒/磁盘版本变了要被读屏读出来)");
  if (!/role="status"/.test(tag("files-dirty"))) out.push("①b #files-dirty(未保存)必须 role=status");
  if (!/role="status"/.test(tag("files-sync"))) out.push("①c #files-sync(已从磁盘更新)必须 role=status");
  if (!/role="note"/.test(tag("files-readonly"))) out.push("①d #files-readonly(只读原因)必须 role=note");
  if (!/aria-keyshortcuts="Control\+S"/.test(tag("files-save"))) out.push("①e #files-save 必须 aria-keyshortcuts=\"Control+S\"");
  if (!/aria-pressed="(?:true|false)"/.test(tag("files-compare"))) out.push("①f #files-compare 是开关,必须带 aria-pressed");
  if (!/aria-label="新建文件"/.test(tag("files-new")) || !/data-i18n-aria-label="新建文件"/.test(tag("files-new"))) out.push("①g #files-new 图标键必须有可译的读屏名「新建文件」");
  if (!/installSplit\(\$\("files-side"\), \{[\s\S]{0,600}?ariaKey: "调整文件树宽度"/.test(sourceText)) out.push("②a 文件树分隔条必须经 installSplit 安装,读屏名「调整文件树宽度」(词条键 ariaKey,切语言重译)");
  if (!/if \(pane\.id\) handle\.setAttribute\("aria-controls", pane\.id\);/.test(sourceText)) out.push("②b installSplit 的分隔条必须 aria-controls 指向被调尺寸的窗格");
  if (!/row\.setAttribute\("aria-selected", active \? "true" : "false"\)/.test(sourceText)) out.push("②c 文件树行必须用 aria-selected 标出当前文件");
  if (!/background:\s*var\(--warn\)/.test(bodyOf(".files-dirty-dot"))) out.push("③a .files-dirty-dot(未保存点)必须是 var(--warn)(琥珀 = 需要注意/配置未保存)");
  if (!/color:\s*var\(--warn\)/.test(bodyOf(".files-dirty"))) out.push("③b .files-dirty(未保存)字色必须是 var(--warn)");
  if (!/background:\s*var\(--alert-soft\)/.test(bodyOf(".files-conflict"))) out.push("③c .files-conflict 必须是 --alert-soft 浅底(与 .settings-effective 同款,不画彩色左竖条)");
  if (/border-left:/.test(bodyOf(".files-conflict"))) out.push("③d .files-conflict 不得画彩色左竖条(ui_color_semantics §4)");
  return out;
}
{
  const violations = filesEditorA11yViolations(html, css, js);
  assert.deepEqual(violations, [], `文件编辑无障碍/配色判据未通过:\n${violations.join("\n")}`);
  const counterexamples = [
    ["①a", html.replace(/(id="files-conflict"[^>]*) role="alert"/, "$1"), css, js],
    ["①e", html.replace(/ aria-keyshortcuts="Control\+S"/, ""), css, js],
    ["②b", html, css, js.replace(/if \(pane\.id\) handle\.setAttribute\("aria-controls", pane\.id\);/, "")],
    ["②c", html, css, js.replace(/row\.setAttribute\("aria-selected", active \? "true" : "false"\)/, "")],
    ["③a", html, css.replace(/(\.files-dirty-dot \{[^}]*background:\s*)var\(--warn\)/, "$1var(--ok)"), js],
    ["③d", html, css.replace(/(\.files-conflict \{)/, "$1 border-left: 3px solid var(--warn);"), js],
  ];
  const silent = counterexamples
    .map(([id, h, c, j], index) => [`${id}#${index}`, filesEditorA11yViolations(h, c, j).some((v) => v.startsWith(id))])
    .filter(([, caught]) => !caught)
    .map(([label]) => label);
  assert.deepEqual(silent, [], `文件编辑判据没能命中自己的反例(恒绿):${silent.join(", ")}`);
}
// ── 分区:文件编辑(完) ──

console.log(`UI 无障碍静态冒烟通过：${static_icon_buttons.length} 个静态 icon-btn，核心键盘语义与焦点规则已覆盖`);
