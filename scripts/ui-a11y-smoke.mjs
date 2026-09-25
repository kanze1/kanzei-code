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

// 权限/提问弹窗是**非模态**停靠卡片(2026-08-16 起):要判断该不该放行往往得回看
// 刚才的工具轨迹与对话,全屏模态恰恰把判断依据挡在背后。非阻塞形态下 aria-modal
// 必须如实为 false——宣称 true 会让读屏软件把背景内容整块隐藏,与实际可读可交互
// 的事实相反,那才是真的无障碍缺陷。role/aria-labelledby 仍是硬要求。
assert.match(html, /id="ask-overlay"[^>]*role="dialog"[^>]*aria-modal="false"[^>]*aria-labelledby="ask-title"/);
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
assert.match(html, /id="bg-panel"[^>]*role="dialog"[^>]*aria-labelledby="bg-panel-title"/);
assert.match(html, /id="agent-panel"[^>]*role="dialog"[^>]*aria-labelledby="agent-panel-title"/);
assert.match(html, /id="bg-close"[^>]*aria-label="关闭活动面板"/);
assert.match(js, /if \(activityPanelOpen\) agentClosePanel\(\)/);
assert.match(css, /#bg-panel\s*\{[^}]*position: absolute/);
assert.match(css, /#agent-panel\s*\{[^}]*position: absolute/);
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
assert.match(css, /#bg-panel\s*\{[^}]*position: absolute/);
assert.match(css, /#agent-panel\s*\{[^}]*position: absolute/);
assert.match(js, /localStorage\.setItem\("kz-sidebar-collapsed"/);
assert.ok(html.includes('id="send"'), "缺少发送按钮");
assert.ok(html.includes('id="stop"'), "缺少停止按钮");
assert.match(html, /id="composer-more"[\s\S]*id="summarize-btn"/);
assert.match(html, /id="composer-more"[\s\S]*id="worktree-add"/);
assert.match(html, /id="task-options"[\s\S]*id="auto-allow"[\s\S]*id="process-phase-pipeline-wrap"[\s\S]*id="process-tracker-writes-wrap"[\s\S]*<\/details>/);
assert.match(js, /function syncSidebar\(\)/);
assert.match(js, /function syncActivityPanel\(\)/);
assert.match(js, /localStorage\.setItem\("kz-activity-panel"/);
// D-662:renderTodoPanel 随 todowrite 一并摘除。此前这里断言它「计划清空即隐藏」;
// 现在断言它不复存在——留着一条针对已删函数的正向断言,下次有人重新引入
// 同名函数时会误以为受了保护。
assert.doesNotMatch(js, /renderTodoPanel/, "renderTodoPanel 应随 todowrite 一并摘除");
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
{
  const themeBlockEnd = css.indexOf("/* ===== 主题 token 块结束");
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

console.log(`UI 无障碍静态冒烟通过：${static_icon_buttons.length} 个静态 icon-btn，核心键盘语义与焦点规则已覆盖`);
