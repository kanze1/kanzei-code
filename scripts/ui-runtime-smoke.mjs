// 前端运行时冒烟(R-084):在 Node 中以最小 DOM harness 真实执行 main.js,
// 补 node --check(纯语法)与静态正则冒烟都抓不到的 ReferenceError / 初始化崩坏(D-048 类问题)。
// 覆盖:整页加载与初始化、需求/缺陷/目标/测试列表非空渲染、主视图切换、console.error 与未捕获异常 → 非零退出码。
import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";
import vm from "node:vm";
import { loadUiSources } from "./ui-sources.mjs";

const root = resolve(import.meta.dirname, "..");
// B1:loadUiSources 的 scriptSrcs 是 ui/ 目录覆盖清单，不再声称它就是浏览器执行顺序。
// B2/D-498:运行时执行仍必须按 index.html 的 <script src> 顺序，保住 classic/defer TDZ。
const { html, scriptSrcs, sources, joined: source } = loadUiSources();
const style = await readFile(resolve(root, "crates/kanzei-app/ui/style.css"), "utf8");

// ---------- 变异守卫(R-177 内容③)----------
// 一条恒绿的断言与一条真护栏在 CI 里长得一模一样。`KZ_SMOKE_MUTATE=<id>` 时,
// 把**被守护的那一行源码**直接删掉再跑,期望脚本非零退出——这样「删了它就变红」
// 是机械判据,不是写在注释里的承诺。
// 变异没命中(源码被改写、那一行不在了)本身就是失败:守卫悄悄失效比断言变红更危险。
const SMOKE_MUTATE = process.env.KZ_SMOKE_MUTATE ?? "";
if (SMOKE_MUTATE) {
  // 正则而不是字面量:仓里 ui/*.js 的换行是 CRLF/LF 混着的,字面量 "\n" 会漏匹配,
  // 而漏匹配的变异是**假绿**——它看起来"守卫生效了",其实一行都没删。
  const mutations = {
    // D-251:refreshWorktrees 在 await 之后那一次 currentProject 复查。
    // 删了它,项目甲在途的清单会被画进项目乙的面板。
    d251: {
      pattern: /[ \t]*if \(currentProject !== forProject\) return;\r?\n(\s*renderWorktrees\(live\);)/,
      replace: "$1",
    },
    // D-257:刷新按钮的监听器。删了它,按钮点下去什么都不发生。
    d257: {
      pattern: /\$\("worktrees-refresh"\)\.addEventListener\("click", refreshWorktrees\);/,
      replace: "",
    },
    // D-355:切项目时 renderProjects 必须清空 activeProcessId。删了它,切到新项目后
    // 残留旧项目的进程 id,loadConversation 就不会等新项目的 process_list,直接拿旧
    // 进程 id 发 conversation_get——新项目的对话永远不恢复。缓存行(D-356)在中间。
    d355ClearActive: {
      pattern: /(if \(previousProject !== currentProject\) \{\r?\n[\s\S]*?)\s*activeProcessId = null;\r?\n(\s*activeSessionId = null;)/,
      replace: "$1$2activeSessionId = null;",
    },
    // D-355:loadConversation 在 conversation_get 落地后的 isCurrent 守卫。删了它,
    // 迟到的旧目标历史会覆盖已经切走的新目标(切 B 后 B 的对话恢复迟到,B 的历史被
    // 画进已切回 A 的消息区)。
    d355LoadConvGuard: {
      pattern: /if \(!isCurrent\(\)\) return;\r?\n(\s*renderRecoveredMessages\(history\);)/,
      replace: "$1",
    },
    // D-356:运行中切回会话的缓存恢复分支。删了它,切回运行中的线路会重新拉取滞后
    // 的 legacy snapshot,运行中已发生的事实被旧快照吞掉(上下文回滚)。
    d356CacheRestore: {
      pattern: /if \(cachedDom && cachedDom\.html && sessionLiveNow\(activeSessionId\)\) \{\r?\n\s*messages\.innerHTML = cachedDom\.html;\r?\n\s*currentAssistant = null;\r?\n\s*currentReasoning = null;\r?\n\s*currentReasoningHead = null;\r?\n\s*scrollBottom\(true\);\r?\n\s*addMessage\("notice", t\("运行中 · 快照截至上次切走时,本轮完成后自动补齐"\)\);\r?\n\s*return;\r?\n\s*\}\r?\n/,
      replace: "",
    },
    // D-356:kz:done 轮末原子回灌。删了它,活动会话的完整 snapshot 永远不被重载,
    // 运行中快照一直停留在切走时(轮末上下文回滚)。
    d356DoneReload: {
      pattern: /dropSessionDomCache\(p\.sessionId\);\r?\n(\s*)await loadConversation\(\);\r?\n(\s*)cacheSessionDom\(p\.sessionId\);/,
      replace: "dropSessionDomCache(p.sessionId);\n$2cacheSessionDom(p.sessionId);",
    },

    // ---- 分区:会话生命周期 ----
    // UI-0926 #2:新对话清 DOM 必须连窗口化历史缓存一起清。删了它,清空后「触顶」
    // 自动补齐会把旧对话一窗一窗 prepend 回新对话上方——「要点好几次」的主因。
    newChatForgetHistory: {
      pattern: /[ \t]*paneHistory\.set\(sessionId \|\| "", \{ items: \[\], rendered: 0 \}\);\r?\n/,
      replace: "",
    },
    // UI-0926 #2:loadConversation 的会话纪元守卫。删了它,新对话之前发出的
    // conversation_get 迟到落地,把旧段整页画回刚开的新对话。
    newChatEpoch: {
      pattern: / &&\r?\n\s*conversationEpoch\(forSessionId\) === forEpoch(?=;)/,
      replace: "",
    },
    // UI-0926 #2:paneFor 新建的 pane 默认隐藏。删了它,后台线首次渲染时新建的 pane
    // 与活动 pane 同时可见,自主推进线的输出叠进当前视图,新对话也清不掉。
    bgPaneHidden: {
      pattern: /[ \t]*if \(!forDisplay\) pane\.classList\.add\("hidden"\);\r?\n/,
      replace: "",
    },
    // UI-0926 #2:忙碌线点新对话另开线路。删了它,新对话会在 runner 脚下清历史
    // (后端现在会拒绝,用户看到的就是「点了报错」)。
    newChatBusyNewLine: {
      pattern: /[ \t]*if \(activeLineBusy\(\)\) return await startConversationOnNewLine\(\);\r?\n/,
      replace: "",
    },
    // UI-0926 #2:兜底改选活动线时视图跟着换。删了它,活动线消失后 activeSessionId 换人、
    // 可见的却还是旧线的 pane:新活动线的实时事件写进旧 pane,新对话清的也是错的那块。
    fallbackPaneSwitch: {
      pattern: /[ \t]*if \(previousProcessId && !workspace_switch_pending\) void loadConversation\(\);\r?\n/,
      replace: "",
    },
    // UI-0926 #2:后端判定「会话运行中」拒绝开新段时,同一次点击改走另开线路。删了它,
    // 用户先吃一个报错、得再点一次——又回到「新对话要点好几次」。
    newChatRefusedNewLine: {
      pattern: /[ \t]*if \(refusedAsRunning\) return await startConversationOnNewLine\(\);\r?\n/,
      replace: "",
    },
    // UI-0926 #1:运行中的线点删除,前端先挡下(不弹确认、不发删除)。删了它,删除会在
    // runner 脚下进行(后端虽再挡一次,用户已经白确认了一遍并吃到报错)。
    deleteRunningGuard: {
      pattern: /[ \t]*if \(target && processRunning\(target\)\) \{\r?\n[^\n]*\r?\n[ \t]*return;\r?\n[ \t]*\}\r?\n/,
      replace: "",
    },
    // UI-0926 #1:删掉活动线的当前段后换成新对话欢迎页。删了它,被删的对话继续留在主区。
    deleteClearedFresh: {
      pattern: /[ \t]*showFreshConversation\(\);\r?\n(?=[ \t]*return;)/,
      replace: "",
    },
    // UI-0926 #1:删掉当前段时撤掉排上的续跑。删了它,续跑那一轮带着「继续」落进空段。
    deleteCancelTimer: {
      pattern: /[ \t]*if \(clearedCurrent\) cancelAutoContinueTimer\(sessionId\);\r?\n/,
      replace: "",
    },
    // UI-0926 #1:删除后递增会话纪元。删了它,删除前发出的装载迟到时把被删内容整页画回来。
    deleteEpoch: {
      pattern: /[ \t]*bumpConversationEpoch\(sessionId\);\r?\n/,
      replace: "",
    },
    // UI-0926 #1:删旧段后按当前段重载主区。删了它,正在看的那段被删历史留在屏幕上。
    deleteReloadPane: {
      pattern: /[ \t]*resetPane\(\);\r?\n[ \t]*forgetPaneHistory\(sessionId\);\r?\n[ \t]*await loadConversation\(\);\r?\n/,
      replace: "",
    },
    // UI-0926 #1:删后台线的历史后丢掉它的 pane。删了它,切过去看到的还是被删内容。
    deleteDiscardBgPane: {
      pattern: /[ \t]*discardSessionPane\(sessionId\);\r?\n/,
      replace: "",
    },
    // UI-0926 #2 复核:按钮 title 与点击分流共用 activeLineBusy。退回只看 running/runControlPending,
    // 启动中/轮间等待时 title 写着空闲文案,点下去却另开线路——说的和做的不一致。
    newChatTitleSharedBusy: {
      pattern: /const busy = active_space === "dev" && activeLineBusy\(\);/,
      replace: 'const busy = active_space === "dev" && (running || runControlPending);',
    },
    // UI-0926 #2 复核:活动线相位一变 title 就跟上。删了它,title 停在上一次 setRunning 时的忙闲。
    newChatTitleFollowsPhase: {
      pattern: /[ \t]*if \(sessionId === activeSessionId\) syncNewChatEnabled\(\);\r?\n/,
      replace: "",
    },
    // UI-0926 #2 复核:clear 成功后才撤续跑。删了它,clear 在途时轮末新排的那一枪带着「继续」落进新段。
    newChatCancelAfterClear: {
      pattern: /[ \t]*cancelAutoContinueTimer\(forSessionId\);\r?\n(?=[ \t]*\/\/ 作废此前发出)/,
      replace: "",
    },
    // UI-0926 #1 复核:后端持锁拒删(前端预检时 kz:turn 未到)走已翻译的提示。删了它,
    // 后端的中文原文经 toastError 甩进英文界面,还挂一个停下之前必然再被拒的「重试」。
    deleteRefusedToast: {
      pattern: /[ \t]*if \(String\(err\)\.includes\("线路运行中"\)\) \{\r?\n[^\n]*\r?\n[ \t]*return;\r?\n[ \t]*\}\r?\n/,
      replace: "",
    },

    // ---- 分区:模型选择 ----

    // ---- 分区:弹层与外观 ----
    // UI-0926 #9:Esc 只关栈顶。把「取栈顶第一个可 Esc 的句柄」换成「取栈底第一个」,
    // 权限卡在场时在确认框里按 Esc 就会先拒掉权限请求——正是这次修掉的串台。
    surfaceEscTop: {
      pattern: /const top = topEscapable\(event\.target \?\? activeElement\(\)\);/,
      replace: 'const top = stack.find((h) => h.type !== "tooltip");',
    },
    // UI-0926 #9:停靠卡片不抢别处输入框的局部 Esc。删掉让位判断,焦点在输入框里按 Esc
    // 就会先拒掉权限请求、速记表单也收不到 Esc——正是评审指出的回归。
    surfaceCardYield: {
      pattern: /\n\s*if \(cardYields\(handle, target\)\) continue;/,
      replace: "",
    },
    // UI-0926 #9:弹窗里的 JS 菜单挂进锚点所在的 <dialog>。退回一律挂 body 末尾,
    // 模态开着时菜单是惰性的(点不动)——正是评审实测的问题。
    surfaceMenuInDialog: {
      pattern: /\(anchorEl\?\.closest\?\.\("dialog\[open\]"\) \?\? surfaceRoot\(\)\)\?\.appendChild\(menu\);/,
      replace: "surfaceRoot()?.appendChild(menu);",
    },
    // UI-0926 #5:发送键改成图标按钮后,读屏名称全靠 aria-label。把空闲态的 t("发送") 退回中文字面量,
    // 英文界面下读屏就会念「发送」——正是这次修掉的漏翻。
    ui5SendLabel: {
      pattern: /(send\.setAttribute\("aria-label", value \? t\("运行中可插入或排队，按交付方式发送"\) : )t\("发送"\)\);/,
      replace: '$1"发送");',
    },

    // ---- 分区:工具行与结构化渲染 ----
    // UI-0926 #6:结果摘要器查表。删了它,所有工具都掉进兜底(read 显示「输出 N 行」而不是
    // 「全文 N 行」),逐工具的精确文本断言必须变红。
    toolSummaryRegistry: {
      pattern: /[ \t]*const summarizer = lookupSummarizer\(TOOL_RESULT_SUMMARIZERS, s\.name\);\r?\n/,
      replace: "",
    },
    // UI-0926 #6:兜底里的噪声判据。删了它,未知工具的行号行(`  12\tlet total be …`,合并空白后
    // 既不像源码也不符号密集)会被当人话显示。
    toolSummaryNoise: {
      pattern: /[ \t]*if \(looksLikeNoise\(cleanPaths\(line, s\.roots\)\) \|\| looksLikeNoise\(clean\)\) break;\r?\n/,
      replace: "",
    },
    // UI-0926 #6:「命令根本没跑」的失败(用户拒绝/规则集拒绝/自主运行跳过/入参修复/停止)先说原因。
    // 删了它,用户拒绝的 bash ⎿ 行变成原文 `(user declined)`,write 被拒只剩「失败」。
    toolSummaryGate: {
      pattern: /[ \t]*const gate = toolGateFailure\(s\);\r?\n[ \t]*if \(gate\) return finish\(gate, "summary", \{ rest: fullRest \}\);\r?\n/,
      replace: "",
    },
    // UI-0926 #6:最终安全网的逐组「行号 + 源码」判据。删了它,夹在退出码后面的 `1 // 注释` 漏进 ⎿ 行。
    toolSummarySafetyNet: {
      pattern: / \|\| numberedSource\)\)/,
      replace: "))",
    },
    // UI-0926 #10:工具块展开区挂入参键值表的那一行。删了它,历史/实时工具块再也看不到
    // 「拿什么参数调的」(路径 chip、命令、多行说明)。
    svArgs: {
      pattern: /[ \t]*if \(args\) block\.detail\.appendChild\(args\);\r?\n/,
      replace: "",
    },
    // UI-0926 #10:JSON 工具结果的结构化视图(延迟挂载)。删了它,tracker list 的展开区又
    // 什么都没有(原文已不再贴出),条目行/阻塞原因断言必须变红。
    svJsonResult: {
      pattern: /[ \t]*if \(jsonValue && typeof jsonValue === "object"\) lazyMount\(block\.detail, \(\) => renderToolResult\(block\.name, jsonValue\)\);\r?\n/,
      replace: "",
    },
    // UI-0926 #10:权限卡资源的结构化渲染。删了它,bash 权限卡的「资源」一栏空着(或残留上一条),
    // 命令代码块/工作目录断言必须变红。
    svAskResource: {
      pattern: /[ \t]*\$\("ask-resource"\)\.replaceChildren\(renderPermissionResource\(askActive\.action, askActive\.resource\)\);\r?\n/,
      replace: "",
    },

    // ---- 分区:需求卡片与单页 ----
    // UI-0926 #4:跳转直达详情——落点那一行必须是展开的。删了它,点焦点卡/refs/测试徽标
    // 又落到一条收起的行上,用户还得再点一次(本次修掉的主诉)。
    jumpExpand: {
      pattern: /[ \t]*if \(expand\) expandEntryDetail\(target\);\r?\n/,
      replace: "",
    },
    // UI-0926 #4:焦点区签名跳过重建。改成每次都重建,3 秒一次的 process_list 轮询会冲掉
    // 正悬停的 tooltip 与开着的「⋯」菜单(锚点被换掉)。
    focusSignatureSkip: {
      pattern: /if \(signature !== lastFocusPanelSignature \|\| !body\.children\.length\) \{/,
      replace: "if (true) {",
    },
    // UI-0926 #4 + #10:单页详情挂字段只读视图的那一行。删了它,详情里只剩头和编辑表单,
    // ①②③ 列表/进展时间线/发现记录键值表/停车拆解等断言必须变红。
    svTrackerFields: {
      pattern: /[ \t]*read\.appendChild\(renderTrackerFields\(entry\.fields \?\? \[\]\)\);\r?\n/,
      replace: "",
    },
    // UI-0926 #4:重绘恢复未保存的编辑输入。删了它,agent 的一次刷新就冲掉正在写的字段。
    docEditDraftKeep: {
      pattern: /control\.value = prior\.drafts\.get\(key\);/,
      replace: "void 0;",
    },
    // UI-0926 #4:refreshDocsSoon 见到单页里未保存的条目编辑先让路。删了它,agent 连续改台账时
    // 整表重建会反复打断输入。
    docsSoonYieldDirty: {
      pattern: / \|\| editingDraft\) \{/,
      replace: ") {",
    },
    // UI-0926 #4:跳转放行只给被筛选挡住的目标。改成无条件放行,筛选内的目标之后改状态落到
    // 筛选外时会一直挂着「不在当前筛选内」赖在列表里。
    jumpRevealHiddenOnly: {
      pattern: /jumpRevealId = hidden \? ref : null;/,
      replace: "jumpRevealId = ref;",
    },

    // ---- 分区:动效 ----
    // #7:setTurnPhase 首行的后台渲染守卫。删了它,后台线的思考/工具事件会把活动线
    // 输入框上方的「思考中…」改写成别人的相位(串线)。
    turnPhaseBgGuard: {
      pattern: /(export function setTurnPhase\(phase\) \{\r?\n(?:[ \t]*\/\/[^\n]*\r?\n)*)[ \t]*if \(typeof renderingBackground !== "undefined" && renderingBackground\) return;\r?\n/,
      replace: "$1",
    },
    // #7:kz:stopped 里主对话工具块的收尾。删了它,停止后运行中的工具行永远在转圈。
    chatAbortOnStop: {
      pattern: /(bgAbortRunning\(`\(\$\{t\("已停止"\)\}\)`\);\r?\n)[ \t]*chatAbortRunning\(\);\r?\n/,
      replace: "$1",
    },
    // #7:线路行状态字形原地更新。改成每次重建,逐事件投影会让呼吸动画每个事件都从第 0 帧重来。
    parallelGlyphInPlace: {
      pattern: /if \(!glyph \|\| !text\) \{/,
      replace: "if (true) {",
    },
    // #7:kz:tool-end 只在本轮真在跑(且不在停止中)时推进相位与状态栏。删了它,已停止之后
    // 迟到的 ToolEnd(停止补发)会让活动行重新扫光、状态栏翻回「运行中」,一直挂到下一次 setRunning(false)。
    toolEndRunningGuard: {
      pattern: /if \(running && turnPhase !== "stopping"\) \{/,
      replace: "if (true) {",
    },
    // #7:「停止中」相位粘滞。删了它,停止发出后迟到的思考/工具事件把活动行翻回运行态,停止按钮却写着「停止中…」。
    turnPhaseStoppingSticky: {
      pattern: /[ \t]*if \(turnPhase === "stopping"\) return;\r?\n/,
      replace: "",
    },
    // #7:同一条线运行中的纠偏 setRunning(true) 保留本轮细分相位。删了它,轮询/逐事件投影一纠偏就把相位打回 waiting。
    setRunningKeepPhase: {
      pattern: /const keepPhase = value && wasRunning/,
      replace: "const keepPhase = false && value && wasRunning",
    },

    // ---- 分区:子代理 ----
  };
  const mutation = mutations[SMOKE_MUTATE];
  if (!mutation) {
    console.error(`未知的 KZ_SMOKE_MUTATE=${SMOKE_MUTATE}(可用:${Object.keys(mutations).join(" / ")})`);
    process.exit(2);
  }
  let hit = 0;
  for (let i = 0; i < sources.length; i += 1) {
    const global = new RegExp(mutation.pattern.source, "g");
    hit += (sources[i].match(global) ?? []).length;
    sources[i] = sources[i].replace(global, mutation.replace);
  }
  if (hit !== 1) {
    console.error(`变异 ${SMOKE_MUTATE} 没有恰好命中一处被守护的源码(实得 ${hit} 处):护栏已经失效,先修变异表`);
    process.exit(2);
  }
  console.error(`[KZ_SMOKE_MUTATE=${SMOKE_MUTATE}] 已删除被守护的源码,期望本次运行**失败**`);
}

const issues = [];
const fail = (msg) => issues.push(msg);
// 断言在真实 DOM 上跑,某条护栏一旦真的红了,后续代码常常会顺带对 null 取属性而硬崩:
// 进程带着一个孤零零的 TypeError 退出,已经攒下的失败清单全看不见,读的人只能从
// "Cannot read properties of null" 反推是哪条能力没了。这个钩子保证无论怎么退出,
// 已收集的问题都先打出来——一次跑完拿到全部线索,而不是修一条崩一次。
let reportedIssues = false;
process.on("exit", (code) => {
  if (code !== 0 && !reportedIssues && issues.length) {
    console.error(`崩溃前已收集到 ${issues.length} 处失败:`);
    for (const issue of issues) console.error(` - ${issue}`);
  }
});

// CSS 结构完整性:浏览器对花括号错配是静默容错的,一个被吃掉的 `@media ... {`
// 会让整段响应式规则无条件生效而没有任何报错(c65c80e 就这样把 D-164 带上了线)。
const cssNoComments = style.replace(/\/\*[\s\S]*?\*\//g, "");
let cssDepth = 0;
let cssStray = 0;
for (const ch of cssNoComments) {
  if (ch === "{") cssDepth += 1;
  else if (ch === "}") {
    if (cssDepth === 0) cssStray += 1;
    else cssDepth -= 1;
  }
}
if (cssStray) fail(`style.css 有 ${cssStray} 个多余的 }(很可能某条规则或 @media 的开括号被覆盖删除了)`);
if (cssDepth) fail(`style.css 有 ${cssDepth} 个未闭合的 {`);
// 图标一致性:活动栏与 .icon-btn 是一整套单色描边字形/SVG(⌂ ☷ ❖ ＋ ↻ ↗ ✎ …),
// 混进一个彩色 emoji 会突兀得像贴上去的。靠眼睛发现太晚——机械挡住。
// 只管图标位,正文里的 💬/⚠ 之类语义标记不在此列。
const ICON_MARKUP = [
  ...html.matchAll(/<button[^>]*class="[^"]*\b(?:activity-item|icon-btn)\b[^"]*"[^>]*>([\s\S]*?)<\/button>/g),
];
const COLOR_EMOJI = /[\u{1F000}-\u{1FAFF}]/u;
for (const [, inner] of ICON_MARKUP) {
  const glyphs = inner.replace(/<[^>]*>/g, "").trim();
  if (COLOR_EMOJI.test(glyphs)) {
    fail(`图标位出现彩色 emoji「${glyphs}」:活动栏与 icon-btn 必须是单色字形或描边 SVG`);
  }
}
if (ICON_MARKUP.length < 10) fail("图标一致性检查没扫到足够的图标按钮,正则可能已与标记脱节");

// 深色主题下原生控件必须跟着深色渲染,否则勾选框会是一块白底(D-154)。
// 这条只能静态查:计算样式里看不出"浏览器用了哪套控件配色"。
const rootRule = style.match(/:root\s*\{([\s\S]*?)\}/)?.[1] ?? "";
if (!/color-scheme:\s*dark/.test(rootRule)) {
  fail(":root 未声明 color-scheme: dark,原生勾选框/下拉在深色界面里会是白底");
}
if (!/input\[type="checkbox"\][^{]*\{[^}]*accent-color/.test(style)) {
  fail("勾选框未统一 accent-color,选中态会用系统蓝而不是界面强调色");
}

const documentsScrollRules = [...style.matchAll(/#documents-scroll\s*\{([^}]*)\}/g)];
const documentsBottomPadding = documentsScrollRules.at(-1)?.[1].match(/padding-bottom:\s*(\d+)px/);
if (!documentsBottomPadding || Number(documentsBottomPadding[1]) < 24) {
  fail("独立文档页滚动容器未预留状态栏安全间距");
}
// 小工具降噪后,非静默工具仍需全量入列(R-095 的"完整流水"只对有信息量的调用成立)。
// R-173:分流判定必须连 input 一起传——编排派发的勘察/复核子代理 name 恒为 "task",
// 只看 name 会把它们连同模型自己派的 task 一起静默,内部进度整批丢掉。
if (!source.includes('else if (isActivityTool(e.payload.name, e.payload.input)) bgAdd')) {
  fail("活动面板仍会接收全部工具调用(或降噪分流被移除,或分流判定不再看 input.phase)");
}
if (!source.includes("function reportPersistentError(text, { retry = null } = {})") || !html.includes('id="log-retry"') || !source.includes("function copyReadable(el)")) {
  fail("错误反馈缺少持久详情、恢复入口或复制能力");
}
if (!source.includes("function renderRecoveredMessages(items)") || !source.includes("if (running) {")) {
  fail("历史回放或运行中隔离护栏缺失");
}
const errorRenderer = source.slice(source.indexOf("function addErrorMessage"), source.indexOf("function isRetryableError"));
if (errorRenderer.includes("setTimeout")) fail("长错误反馈不应由短 toast 定时移除");
if (!source.includes("function renderMarkdown(raw)") || !source.includes("renderDiff(")) {
  fail("对话 Markdown 或 diff 详情渲染入口缺失");
}
// 历史消息只进入恢复渲染器，实时事件继续使用 currentAssistant，不应把运行输出写入历史快照。
if (!source.includes('const history = await invoke("conversation_get"') || !source.includes("renderRecoveredMessages(history)")) {
  fail("历史消息未通过只读恢复渲染链路");
}
// 历史回放必须保留完整调用与结果:调用与结果按 call_id 配对成一块(buildToolBlock/
// fillToolBlock),详情里同时给出完整输出与完整入参。UI-0926 #10 起入参走 renderToolArgs
// 键值表(逐键完整渲染),入参 JSON 另挂在其根节点 dataset.raw 上(04-structured.js,
// 超过 8000 字截断——完整值已在键值表里,不在 DOM 属性里再存一整份)。
if (
  !source.includes('part.type === "tool_result"') ||
  !source.includes("const rawJson = JSON.stringify(input, null, 2)") ||
  !source.includes("box.dataset.raw = rawJson") ||
  !source.includes("renderToolArgs(block.name, input") ||
  !source.includes("function fillToolBlock")
) {
  fail("历史工具会话未保留完整调用与结果详情");
}
// 工具块的 ⎿ 摘要行与展开详情必须是同一份文本切出来的两段(toolResultSplit),
// 不能各自独立地从 content 取一遍再靠 `full !== preview` 去重——那个写法只挡得住
// 单行短结果,首行超长或多行一律把同一段文案渲染两遍。运行时判据见下面的工具块用例;
// 这条静态契约拦的是"改回旧写法"这个具体形态。
if (source.includes("full.trim() !== preview")) {
  fail("工具块详情又回到了「摘要之外再贴一遍完整原文」的写法(full.trim() !== preview)");
}
if (!source.includes("function toolResultSplit")) {
  fail("工具块缺少 toolResultSplit:摘要与详情不再是同一份文本的互斥两段,双写会复发");
}
const dictionarySource = source.slice(source.indexOf("const I18N_EN = {"), source.indexOf("const I18N_ZH = new WeakMap"));
const dictionaryKeys = new Set([...dictionarySource.matchAll(/\"((?:\\.|[^\"])*)\"\s*:/g)].map((match) => match[1]));
const translationCalls = [...source.matchAll(/\bt\(\"((?:\\.|[^\"])*)\"\)/g)].map((match) => match[1]);
for (const key of new Set(translationCalls)) if (!dictionaryKeys.has(key)) fail(`I18N_EN 缺少 t key: ${key}`);
if (!source.includes("function stopAutoForManualInput()") || !source.includes('const message = t("收到手动输入，鞭挞已停止")')) {
  fail("手动输入未确认停止鞭挞并反馈用户");
}
if (!source.includes('e.key === "Enter" && !e.shiftKey')) {
  fail("主输入框未保持 Enter 发送、Shift+Enter 换行契约");
}
const autoNoticeIndex = source.indexOf('addMessage("notice", `${t("鞭挞已触发")}');
if (autoNoticeIndex < 0 || source.includes('addUserMessage(auto ?')) {
  fail("自动续轮仍把内部提示词重复展示为用户消息");
}
  // R-170:引擎规则(验收证据/调用方/范围保持)已剥离出继续文案,归 system prompt;
  // 前端源码不应再持有这些规则文本(验收①快照断言)。
  for (const ruleText of ["逐条对照验收原文", "真实调用方或消费者", "不得缩小验收里的平台或范围限定词"]) {
    if (source.includes(ruleText)) {
      fail(`08-compose.js 仍持有引擎规则文本「${ruleText}」(R-170 应已剥离)`);
    }
  }

const pendingTimers = new Set();
const rafQueue = [];
// D-202:统计"从 document.body 起的全文档文本节点重扫"次数。流式渲染期间它必须为 0,
// 否则就是有人把 i18n observer 改回了全量重扫(卡顿主因)。
let fullDocumentWalks = 0;
const mutationObservers = new Set();
let mutationQueued = false;
// observer 回调里的 DOM 写入若再次唤醒 observer 自己,真机上是微任务死循环:主线程
// 饿死、永不绘制,表现为启动黑屏(D-172)。冒烟里这种循环会让进程挂死而非报错,
// 所以数连续自触发轮数,超限就断开 observer 并判失败,把挂死变成可读的失败。
let observerCascade = 0;
// D-202:必须投递真实的 MutationRecord。回调只处理"本次变动带进来的节点"是修复的
// 关键路径,若 harness 一直递空数组,这条路径在冒烟里恒为空转——既测不到本地化是否
// 生效,也测不出有人把它改回全文档重扫。
const mutationRecords = [];
function notifyMutation(record) {
  if (record) mutationRecords.push(record);
  if (!mutationObservers.size) {
    mutationRecords.length = 0;
    return;
  }
  if (mutationQueued) return;
  mutationQueued = true;
  Promise.resolve().then(() => {
    mutationQueued = false;
    const records = mutationRecords.splice(0);
    for (const observer of mutationObservers) observer.callback(records);
    if (mutationQueued) {
      observerCascade += 1;
      if (observerCascade > 25) {
        mutationObservers.clear();
        fail("MutationObserver 连续自触发超过 25 轮:回调内的 DOM 写入又唤醒了 observer(真机=微任务死循环→主线程饿死→黑屏,D-172)");
      }
    } else {
      observerCascade = 0;
    }
  });
}

// ---------- DOM harness:真实节点关系(parent/children/dataset/classList),样式与布局按 noop ----------
let idSeed = 0;
class ClassList {
  #el;
  #set = new Set();
  constructor(el) { this.#el = el; }
  #sync() { this.#el._attributes.class = [...this.#set].join(" "); }
  add(...names) { names.filter(Boolean).forEach((n) => this.#set.add(n)); this.#sync(); }
  remove(...names) { names.forEach((n) => this.#set.delete(n)); this.#sync(); }
  toggle(name, force) {
    const on = force === undefined ? !this.#set.has(name) : Boolean(force);
    on ? this.#set.add(name) : this.#set.delete(name);
    this.#sync();
    return on;
  }
  contains(name) { return this.#set.has(name); }
}
// ---------- <select> 的规范语义 ----------
// 早期 harness 把 select.value 当普通属性存:赋什么都照单全收。真实浏览器不是这样——
// 给 select 赋一个没有匹配 <option> 的值,只会把 selectedIndex 打到 -1、value 读回空串。
// 差别不是细节:「先 select.value = 已存值,再读 DOM 当基准」这种写法在真机上等于把
// 已存配置静默清空(D-168 同族,一次保存就把 kanzei.toml 的键删掉),而在旧 harness 里
// 恒为通过。所以这里按规范实现,让这类缺陷在冒烟里现形。
const selectOptions = (el) => el.childNodes.filter((n) => n instanceof Element && n.tagName === "OPTION");
// HTML 规范的 "ask for a reset":单选 select 的 option 列表变动后,若没有任何 option
// 处于选中态,第一个自动选中。少了这一步,replaceChildren 之后 value 恒为空串。
function resetSelectedness(el) {
  if (el.tagName !== "SELECT") return;
  const options = selectOptions(el);
  if (!options.length || options.some((o) => o._selected)) return;
  options[0]._selected = true;
}
// select 的 innerHTML/index.html 静态标记里写死的 <option> 必须建成真实子节点,
// 否则 select.options 恒为空,上面的规范语义会把所有下拉一起变哑(实测会连累
// 语言切换、节奏回填、思考强度落盘等 20 多条无关断言)。
function parseOptionsInto(el, fragment) {
  for (const [, attributes, inner] of String(fragment).matchAll(/<option([^>]*)>([\s\S]*?)<\/option>/g)) {
    const option = new Element("option");
    option.ownerDocument = el.ownerDocument;
    const text = inner.replace(/<[^>]*>/g, "").replace(/\s+/g, " ").trim();
    option.textContent = text;
    // R-140 批5:静态 option 的 data-i18n-key 也要建到桩元素上,否则渲染点翻译
    // 对 option 文本恒不生效,文档域的筛选下拉在冒烟里全是假通过。
    const keyValue = attributes.match(/\bdata-i18n-key="([^"]*)"/)?.[1];
    if (keyValue !== undefined) option.setAttribute("data-i18n-key", keyValue);
    const valueAttribute = attributes.match(/\bvalue="([^"]*)"/)?.[1];
    option.value = valueAttribute === undefined ? text : valueAttribute;
    // value setter 只写 _value;真实浏览器里 getAttribute("value") 也会返回该值,
    // 而 matchesOne 的属性选择器走 getAttribute。不同步的话 `option[value="x"]`
    // 在冒烟里恒空——R-178 批4 的设置页作用域下拉(option[value="project"])就撞上了。
    if (valueAttribute !== undefined) option._attributes.value = valueAttribute;
    if (/\bselected\b/.test(attributes)) option._selected = true;
    el.appendChild(option);
  }
}
class Element {
  constructor(tag) {
    this.tagName = String(tag).toUpperCase();
    this.ownerDocument = null;
    this.parentNode = null;
    this.childNodes = [];
    // style 不能只是空对象:main.js 用 setProperty 写 CSS 变量(批次格的列数就靠它),
    // 缺这个 API 会在渲染中途抛异常,整张列表消失——而这种崩法在冒烟里表现为
    // "元素找不到",看不出真因。
    this.style = {
      _props: {},
      setProperty(name, value) { this._props[name] = String(value); },
      getPropertyValue(name) { return this._props[name] ?? ""; },
      removeProperty(name) { delete this._props[name]; },
    };
    this.dataset = {};
    this.classList = new ClassList(this);
    this._attributes = {};
    this._listeners = {};
    this._textContent = "";
    this._innerHTML = "";
    this.id = "";
    this.disabled = false;
    this.checked = false;
    this.draggable = false;
    this.open = false;
    this.value = "";
    this.title = "";
    this.scrollTop = 0;
    this.scrollHeight = 0;
  }
  _adopt(node) { node.parentNode = this; node.ownerDocument = this.ownerDocument; this.childNodes.push(node); resetSelectedness(this); notifyMutation({ type: "childList", target: this, addedNodes: [node] }); return node; }
  appendChild(node) { node.remove(); return this._adopt(node); }
  append(...nodes) { for (const n of nodes) this.appendChild(typeof n === "string" ? this.ownerDocument.createTextNode(n) : n); }
  prepend(...nodes) { for (const n of nodes.reverse()) this.insertBefore(typeof n === "string" ? this.ownerDocument.createTextNode(n) : n, this.childNodes[0] ?? null); }
  insertBefore(node, ref) {
    node.remove();
    node.parentNode = this;
    node.ownerDocument = this.ownerDocument;
    const idx = ref ? this.childNodes.indexOf(ref) : -1;
    if (idx < 0) this.childNodes.push(node); else this.childNodes.splice(idx, 0, node);
    resetSelectedness(this);
    return node;
  }
  replaceChildren(...nodes) { for (const c of [...this.childNodes]) c.parentNode = null; this.childNodes = []; this._innerHTML = ""; this.append(...nodes); resetSelectedness(this); }
  replaceWith(node) { if (this.parentNode) { this.parentNode.insertBefore(node, this); this.remove(); } }
  remove() {
    if (this.parentNode) {
      const siblings = this.parentNode.childNodes;
      const idx = siblings.indexOf(this);
      if (idx >= 0) siblings.splice(idx, 1);
      this.parentNode = null;
    }
  }
  get parentElement() { return this.parentNode instanceof Element ? this.parentNode : null; }
  get children() { return this.childNodes.filter((n) => n instanceof Element); }
  get options() { return this.tagName === "SELECT" ? this.children : undefined; }
  get firstChild() { return this.childNodes[0] ?? null; }
  get nextSibling() {
    if (!this.parentNode) return null;
    return this.parentNode.childNodes[this.parentNode.childNodes.indexOf(this) + 1] ?? null;
  }
  get previousElementSibling() {
    if (!this.parentNode) return null;
    const sibs = this.parentNode.childNodes.filter((n) => n instanceof Element);
    const idx = sibs.indexOf(this);
    return idx > 0 ? sibs[idx - 1] : null;
  }
  get className() { return this._attributes.class ?? ""; }
  set className(value) { this.classList = new ClassList(this); this.classList.add(...String(value).split(/\s+/).filter(Boolean)); }
  get textContent() {
    // innerHTML 写入的文本与后续 appendChild 的子节点会共存(先设 innerHTML 再追加是
    // main.js 的常见写法);只读子节点会把前者整段丢掉,断言就看不见它。
    const own = this._textContent;
    if (!this.childNodes.length) return own;
    return own + this.childNodes.map((c) => (c instanceof Element ? c.textContent : c.nodeValue)).join("");
  }
  // harness 近似:文本节点是 createTreeWalker 里惰性造的,拿不到"新增的那个文本节点",
  // 就把元素自身当作新进子树递出去——localizeRoot 走子树,语义是真机的超集,不会漏。
  set textContent(value) { this.childNodes = []; this._innerHTML = ""; this._textContent = String(value); notifyMutation({ type: "childList", target: this, addedNodes: [this] }); }
  get innerText() { return this.textContent; }
  set innerText(value) { this.textContent = value; }
  get innerHTML() { return this._innerHTML; }
  // innerHTML 写入要同步出可读文本:main.js 里大量行是 innerHTML 拼的,若 textContent
  // 读不到它们,所有基于文本的断言对这些内容都是瞎的(和 D-151 同一类盲区)。
  // 只做去标签的近似,不实现真正的解析——冒烟要的是"文字在不在",不是 DOM 树。
  set innerHTML(value) {
    this._innerHTML = String(value);
    this.childNodes = [];
    // select 是例外:它的 innerHTML 里写的是 <option>,必须建成真实子节点。
    // 只留去标签文本的话 select.options 恒为空,规范化的 value 语义会把它变成一个哑控件。
    if (this.tagName === "SELECT") {
      this._textContent = "";
      parseOptionsInto(this, value);
      resetSelectedness(this);
      notifyMutation({ type: "childList", target: this, addedNodes: [this] });
      return;
    }
    this._textContent = String(value)
      .replace(/<[^>]*>/g, "")
      .replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&").replace(/&quot;/g, '"');
    notifyMutation({ type: "childList", target: this, addedNodes: [this] });
  }
  get value() {
    if (this.tagName === "SELECT") {
      const options = selectOptions(this);
      const selected = options.find((o) => o._selected);
      // selectedIndex < 0(含"一个 option 都没有"的空壳)→ 空串,与浏览器一致。
      return selected ? selected.value : "";
    }
    return this._value ?? "";
  }
  set value(v) {
    const next = String(v);
    if (this.tagName === "SELECT") {
      // 精确查找:命中就选中它,没命中就全部取消选中(= selectedIndex -1),不再"照单全收"。
      for (const option of selectOptions(this)) option._selected = option.value === next;
    }
    this._value = next;
  }
  get selectedIndex() { return this.tagName === "SELECT" ? selectOptions(this).findIndex((o) => o._selected) : -1; }
  set selectedIndex(index) {
    if (this.tagName !== "SELECT") return;
    selectOptions(this).forEach((option, idx) => { option._selected = idx === Number(index); });
  }
  getAttribute(name) { return this._attributes[name] ?? null; }
  // class 必须走 className 设值,否则 classList 的内部集合与属性脱节:index.html 里
  // 写死的 class 不进集合,第一次 classList.toggle() 回写就把它们整体抹掉了。
  setAttribute(name, value) {
    if (name === "class") { this.className = value; return; }
    const next = String(value);
    // 同值也必须通知 observer:DOM 规范里 setAttribute 无条件入 mutation 队列,
    // 早退吞通知会让"observer 回调里无条件写属性"的死循环在冒烟里隐形(D-172)。
    if (this._attributes[name] === next) { notifyMutation({ type: "attributes", target: this, attributeName: name, addedNodes: [] }); return; }
    this._attributes[name] = next;
    if (name === "id") this.id = next;
    // 真实浏览器的 IDL 反射:title/placeholder 等属性会同步到同名 property。
    // 假 DOM 不反射的话,applyDataI18nKeys 走 setAttribute 后 `el.title` 读不到,
    // 切语言断言在 harness 里失明(R-140 批10 实测:observer 退役后属性与 property 脱节)。
    if (name === "title" || name === "placeholder") this[name] = next;
    // data-* 同步进 dataset(真实浏览器行为):main.js 读 `el.dataset.x`,
    // 冒烟里不同步则 applyDataI18nKeys 等读 dataset 的逻辑在 harness 里失明。
    if (name.startsWith("data-") && name.length > 5) {
      const camel = name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
      this.dataset[camel] = next;
    }
    notifyMutation({ type: "attributes", target: this, attributeName: name, addedNodes: [] });
  }
  removeAttribute(name) { delete this._attributes[name]; }
  hasAttribute(name) { return name in this._attributes; }
  addEventListener(type, fn) { (this._listeners[type] ??= []).push(fn); }
  removeEventListener() {}
  dispatchEvent(event) { event.target ??= this; (this._listeners[event.type] ?? []).forEach((fn) => fn(event)); }
  click() { this.dispatchEvent({ type: "click", preventDefault() {}, stopPropagation() {} }); }
  // 顶层原语(UI-0926 #9 弹层技术栈):<dialog> 的 showModal/show/close 与 Popover API。
  // 假 DOM 没有顶层/样式,只维护 open/_modal/_popoverOpen 状态并派发 close/toggle 事件——
  // 00-surface.js 同时镜像 .hidden,旧断言照读 classList;真实行为由浏览器样例冒烟兜底。
  showModal() { this.open = true; this._modal = true; this._attributes.open = ""; }
  show() { this.open = true; this._modal = false; this._attributes.open = ""; }
  close(returnValue) {
    if (!this.open) return;
    this.open = false;
    this._modal = false;
    delete this._attributes.open;
    if (returnValue !== undefined) this.returnValue = String(returnValue);
    this.dispatchEvent({ type: "close", target: this });
  }
  showPopover() {
    if (this._popoverOpen) return;
    this._popoverOpen = true;
    this.dispatchEvent({ type: "toggle", target: this, oldState: "closed", newState: "open" });
  }
  hidePopover() {
    if (!this._popoverOpen) return;
    this._popoverOpen = false;
    this.dispatchEvent({ type: "toggle", target: this, oldState: "open", newState: "closed" });
  }
  togglePopover(force) {
    const next = force === undefined ? !this._popoverOpen : Boolean(force);
    if (next) this.showPopover(); else this.hidePopover();
    return next;
  }
  focus() {}
  querySelector(selector) { return queryAllFrom(this, selector)[0] ?? null; }
  querySelectorAll(selector) { return queryAllFrom(this, selector); }
  closest(selector) { let el = this; while (el) { if (matchesCompound(el, selector)) return el; el = el.parentElement; } return null; }
  setPointerCapture() {}
  scrollIntoView() {}
  scrollTo() {}
  getBoundingClientRect() { return { top: 0, left: 0, width: 0, height: 0, right: 0, bottom: 0 }; }
  get offsetParent() { return this.parentElement; }
  get offsetTop() { return 0; }
}
class TextNode {
  constructor(text) { this.nodeValue = String(text); this.parentNode = null; this.ownerDocument = null; }
  remove() {}
}

function descendantElements(node) {
  const out = [];
  const walk = (el) => { for (const c of el.childNodes) if (c instanceof Element) { out.push(c); walk(c); } };
  walk(node);
  return out;
}
function matchesOne(el, selector) {
  selector = selector.trim();
  if (selector.startsWith(".")) return el.classList.contains(selector.slice(1));
  if (selector.startsWith("#")) return el.id === selector.slice(1);
  if (selector.startsWith("[")) {
    // 属性选择器要支持带值比较,并且 data-* 得查 dataset —— main.js 写的是
    // `el.dataset.bgTool = ...`,不会落到 _attributes 里;早期版本只按属性名
    // 存在性判断,于是 `[data-bg-tool=bash]` 这类选择恒不命中。
    const body = selector.slice(1, -1);
    const eq = body.indexOf("=");
    const name = (eq < 0 ? body : body.slice(0, eq)).trim();
    const want = eq < 0 ? null : body.slice(eq + 1).trim().replace(/^["']|["']$/g, "");
    const dataKey = name.startsWith("data-")
      ? name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())
      : null;
    const actual = dataKey && dataKey in el.dataset ? el.dataset[dataKey] : el.getAttribute(name);
    if (actual == null) return false;
    return want === null || String(actual) === want;
  }
  return el.tagName === selector.toUpperCase();
}
// 复合选择器:".doc-item[data-doc-id]" / "div.foo" / "#id.bar" —— 每一段都要同时命中。
// 早期版本把整段当一个 class 名比,于是 main.js 里所有 ".x[attr]" 形式在冒烟中恒为空,
// 真实浏览器却正常工作:这类静默不一致会让冒烟对整块逻辑失明。
function matchesCompound(el, step) {
  const parts = step.match(/^[a-zA-Z][\w-]*|\.[^.#[\]]+|#[^.#[\]]+|\[[^\]]+\]/g);
  if (!parts) return false;
  return parts.every((part) => matchesOne(el, part));
}
function queryAllFrom(node, selector) {
  // 支持逗号分组与后代组合选择器(如 ".a .b" / "div, span");近似实现,仅覆盖 main.js 的用法。
  return selector.split(",").flatMap((part) => {
    const steps = part.trim().split(/\s+/);
    let current = [node];
    for (const step of steps) {
      current = current.flatMap((base) => descendantElements(base).filter((el) => matchesCompound(el, step)));
    }
    return current;
  });
}

const documentElement = new Element("html");
const body = new Element("body");
const byId = new Map();
// R-264 ESM:DOMContentLoaded 回调收集(冒烟手动触发,见 runUiSources 末尾)。
const domReadyCallbacks = [];
const documentListeners = new Map();
const documentCaptureListeners = new Map();
const document = {
  documentElement,
  body,
  hidden: false,
  title: "",
  createElement: (tag) => { const el = new Element(tag); el.ownerDocument = document; return el; },
  createElementNS: (_ns, tag) => { const el = new Element(tag); el.ownerDocument = document; return el; },
  createTextNode: (text) => { const n = new TextNode(text); n.ownerDocument = document; return n; },
  createTreeWalker: (root = body) => {
    if (root === body) fullDocumentWalks += 1;
    const texts = [];
    const walk = (el) => {
      if (el._textContent && !el.childNodes.length) {
        if (!el._textNodeProxy) {
          const text = { parentNode: el, parentElement: el };
          Object.defineProperty(text, "nodeValue", {
            get: () => el._textContent,
            set: (value) => {
              const next = String(value);
              if (el._textContent === next) return;
              el._textContent = next;
              notifyMutation({ type: "characterData", target: text, addedNodes: [] });
            },
          });
          el._textNodeProxy = text;
        }
        texts.push(el._textNodeProxy);
      }
      for (const c of el.childNodes) {
        if (c instanceof TextNode) { if (c.nodeValue) texts.push(c); } else walk(c);
      }
    };
    // 文本节点当 root:characterData 变动会把它直接递进来。
    if (root && typeof root.querySelectorAll !== "function") {
      if (root.nodeValue) texts.push(root);
    } else walk(root ?? body);
    let idx = -1;
    return { nextNode() { idx += 1; return idx < texts.length ? (this.currentNode = texts[idx], true) : false; }, currentNode: null };
  },
  getElementById: (id) => byId.get(id) ?? null,
  querySelector: (selector) => queryAllFrom(documentElement, selector)[0] ?? null,
  querySelectorAll: (selector) => queryAllFrom(documentElement, selector),
  // R-264 ESM:模拟浏览器 module 求值期——DOM 还在 loading,顶层延迟执行的点
  // 走 DOMContentLoaded 收集,由冒烟在全部模块求值后触发。
  readyState: "loading",
  // R-264 ESM:收集 DOMContentLoaded 回调,evaluate 完所有模块后由冒烟手动触发
  // (模拟浏览器 `<script type="module">` 的 deferred 语义——模块求值完后 DOM 就绪)。
  // 捕获阶段监听单列(UI-0926 #9):00-surface.js 的 Esc 入口挂在 document 捕获阶段,必须先于
  // 冒泡监听执行,且 stopImmediatePropagation 之后一个都不能再跑——「Esc 只关栈顶」验的就是这条。
  addEventListener: (type, fn, options) => {
    const capture = options === true || Boolean(options?.capture);
    if (type === "DOMContentLoaded") domReadyCallbacks.push(fn);
    else if (capture) documentCaptureListeners.set(type, [...(documentCaptureListeners.get(type) || []), fn]);
    else documentListeners.set(type, [...(documentListeners.get(type) || []), fn]);
  },
  removeEventListener: (type, fn) => {
    documentListeners.set(type, (documentListeners.get(type) || []).filter((item) => item !== fn));
    documentCaptureListeners.set(type, (documentCaptureListeners.get(type) || []).filter((item) => item !== fn));
  },
  dispatchEvent: (event) => {
    for (const fn of [...(documentCaptureListeners.get(event.type) || []), ...(documentListeners.get(event.type) || [])]) {
      if (event._stopImmediate) break;
      fn(event);
    }
    return !event.defaultPrevented;
  },
  hasFocus: () => true,
};

// body 必须真的挂在 documentElement 下:否则 document.querySelectorAll 走的是空树,
// 任何按 class 的选择器恒为空(.activity-item 一个都找不到,视图切换覆盖恒为 0),
// 而脚本还会照常报通过——护栏形同虚设(D-138)。
documentElement.ownerDocument = document;
documentElement.appendChild(body);

// 从 index.html 生成带 id 的真实节点:id 引用错(smokey 场景)会在这里直接暴露。
// 整个开标签一起匹配,顺带取回 class —— 早期版本只取 id,index.html 里写死的 class
// 对冒烟完全不可见(`.documents-list .doc-item` 恒为空),按 class 的断言全是假通过。
for (const match of html.matchAll(/<(\w+)((?:[^<>"]|"[^"]*")*?)(?<![-\w])id="([\w-]+)"((?:[^<>"]|"[^"]*")*?)>/g)) {
  const [, tag, before, id, after] = match;
  if (byId.has(id)) continue;
  const el = document.createElement(tag);
  el.id = id;
  el._attributes.id = id;
  const attributes = `${before} ${after}`;
  const className = attributes.match(/\bclass="([^"]*)"/)?.[1];
  if (className) el.className = className;
  for (const attribute of ["title", "placeholder", "aria-label"]) {
    const value = attributes.match(new RegExp(`\\b${attribute}="([^"]*)"`))?.[1];
    if (value !== undefined) el.setAttribute(attribute, value);
  }
  if (/\bdata-i18n-raw\b/.test(attributes)) el.setAttribute("data-i18n-raw", "");
  for (const attribute of ["data-i18n-key", "data-i18n-title", "data-i18n-aria-label", "data-i18n-placeholder"]) {
    const value = attributes.match(new RegExp(`\\b${attribute}="([^"]*)"`))?.[1];
    if (value !== undefined) el.setAttribute(attribute, value);
  }
  // 弹层宿主(UI-0926 #9):popover 属性、data-kz-menu 触发器与定位提示也要建到桩上,
  // 否则 bindMenus 在冒烟里扫不到任何触发器,菜单接线恒为空转。
  const popoverAttr = attributes.match(/(?:^|\s)popover(?:="([^"]*)")?(?=[\s/>]|$)/);
  if (popoverAttr) el.setAttribute("popover", popoverAttr[1] ?? "");
  for (const attribute of ["data-kz-menu", "data-placement", "data-size", "data-tone"]) {
    const value = attributes.match(new RegExp(`\\b${attribute}="([^"]*)"`))?.[1];
    if (value !== undefined) el.setAttribute(attribute, value);
  }
  const tail = html.slice(match.index + match[0].length);
  const directText = tail.match(/^([^<]*)</)?.[1].replace(/\s+/g, " ").trim();
  if (directText) el.textContent = directText;
  byId.set(id, el);
  body.appendChild(el);
  // 后代选择器需要真实嵌套:按 id 造出来的节点是扁平的,`#providers-table tbody`
  // 会拿到 null。视图切换护栏打开后 settings 首次被真正执行,立刻暴露了这个缺口。
  if (el.tagName === "TABLE") el.appendChild(document.createElement("tbody"));
  // index.html 里写死的 <option> 也要建成真实子节点(见 parseOptionsInto 的说明):
  // 语言/代理/思考强度/节奏/各类筛选下拉的选项全在标记里,不建就全变哑控件。
  if (el.tagName === "SELECT") {
    const tail = html.slice(match.index + match[0].length);
    const end = tail.indexOf("</select>");
    parseOptionsInto(el, end < 0 ? "" : tail.slice(0, end));
    resetSelectedness(el);
  }
}

// 活动面板三段是真实嵌套:#bg-list > .bg-section > #bg-running|#bg-attention|#bg-done,
// 条目落在段容器里而不再是 #bg-list 的直接子节点。扁平造的话所有 `#bg-list .bg-entry`
// 断言全部落空(与上面 TABLE→tbody 同一类缺口)。
for (const [sectionId, bodyId] of [
  ["bg-section-running", "bg-running"],
  ["bg-section-attention", "bg-attention"],
  ["bg-section-done", "bg-done"],
]) {
  const section = byId.get(sectionId);
  const sectionBody = byId.get(bodyId);

  if (!section || !sectionBody) continue;
  section.appendChild(sectionBody);
  byId.get("bg-list")?.appendChild(section);
}

// 主视图切换按钮只有 class 没有 id,上面那轮按 id 造不出它们;这里按 class 补造,
// 并在下面对"切换数为 0"直接判失败。
for (const match of html.matchAll(/<button[^>]*class="activity-item[^"]*"[^>]*data-view="([\w-]+)"[^>]*>/g)) {
  const el = document.createElement("button");
  el.className = "activity-item";
  el.dataset.view = match[1];
  for (const attribute of ["title", "aria-label"]) {
    const value = match[0].match(new RegExp(`\\b${attribute}="([^"]*)"`))?.[1];
    if (value !== undefined) el.setAttribute(attribute, value);
  }
  // R-140 批10:rail 按钮的 data-i18n-* 也要建到桩元素上,否则 applyDataI18nKeys
  // 的渲染点翻译对它们失明(observer 退役后无人再走属性扫描,漏建即英文态漏翻)。
  for (const attribute of ["data-i18n-key", "data-i18n-title", "data-i18n-aria-label", "data-i18n-placeholder"]) {
    const value = match[0].match(new RegExp(`\\b${attribute}="([^"]*)"`))?.[1];
    if (value !== undefined) el.setAttribute(attribute, value);
  }
  const tail = html.slice(match.index + match[0].length);
  const directText = tail.match(/^([^<]*)</)?.[1].replace(/\s+/g, " ").trim();
  if (directText) el.textContent = directText;
  body.appendChild(el);
}

// R-140 批2:静态 DOM data-i18n-key 节点(侧栏标题、subtitle)无 id,按属性补造,
// 让渲染点翻译对这些节点真实可断言——不补造则 `[data-i18n-key]` 恒为空,
// data-i18n-key 静态翻译在冒烟里全是假通过。
for (const match of html.matchAll(/<(\w+)((?:[^<>"]|"[^"]*")*?)\bdata-i18n-key="([^"]*)"((?:[^<>"]|"[^"]*")*?)>/g)) {
  const [, tag, before, key, after] = match;
  if (/id="/.test(`${before} ${after}`)) continue; // 带 id 的已由上面按 id 段建造
  const el = document.createElement(tag);
  el.setAttribute("data-i18n-key", key);
  for (const attribute of ["data-i18n-title", "data-i18n-aria-label", "data-i18n-placeholder"]) {
    const value = match[0].match(new RegExp(`\\b${attribute}="([^"]*)"`))?.[1];
    if (value !== undefined) el.setAttribute(attribute, value);
  }
  const tail = html.slice(match.index + match[0].length);
  const directText = tail.match(/^([^<]*)</)?.[1].replace(/\s+/g, " ").trim();
  if (directText) el.textContent = directText;
  body.appendChild(el);
}

// ---------- Tauri 桥桩:启动序列与各列表需要真实形状的负载 ----------
const PROJECT = "C:/smoke/project";
// nextStatuses 是状态流转按钮的数据源:桩里缺它,侧栏"能不能切状态"就无从断言。
// D-381:字段清单跟着 scripts/ipc-contract.json 走——夹具少一个键,界面读它就是
// undefined,而这类漏洞过去没有任何一条测试会红(冒烟验的是夹具自己)。
const docEntry = (id, title, status, extra = {}) => ({
  id, title, status, priority: "P1", closed: false, fields: [], nextStatuses: ["done"],
  severity: null, complexity: null, batches: { done: 0, total: 1 },
  blocked: false, block_reasons: [], claimed_by: null, dependencies: [], dependents: [],
  execution_model: null, work_units: [],
  ...extra,
});
const smokeWorkUnit = {
  unit_id: "R-001/W1", requirement_id: "R-001", objective: "实现有界执行上下文",
  status: "active", claimed_by: "smoke-line", scope: ["crates/kanzei-tools"],
  dependencies: [], acceptance: ["上下文只含当前单元"], verification: ["cargo test"],
  base_revision: "smoke-base", blocked_reason: null,
  last_checkpoint: {
    summary: "事件底座已落地", next_action: "同步 IPC 契约",
    decisions: ["只注入当前单元"], retrieval_refs: ["M-001"],
  },
  evidence: [{ criterion: "上下文只含当前单元", evidence_refs: ["work.rs:2618"] }], created_at: 1_760_000_000_000, updated_at: 1_760_000_000_001,
};
// ---------- 工具块夹具:历史回放里的四种结果形态 ----------
// 双写缺陷(⎿ 摘要行与展开详情各渲染一遍同一段文案)只在"首行超过 ⎿ 预算"或"多行"时
// 显形,单行短结果永远看不出来——夹具必须真的超预算,否则断言恒真。
const HISTORY_LONG_FIRST_LINE = `历史失败首行 ${"abcdefghijklmnopqrstuvwxyz".repeat(6)}`; // 163 字 > 110
const HISTORY_HUGE_OUTPUT = `第一行输出\n${"这是一段很长的历史输出。".repeat(800)}`; // 远超 8000
let smokeResearchPlanStatus = "awaiting_approval";
const smokeResearchPlan = () => ({
  version: 1,
  topic: "alpha-study",
  title: "Alpha 研究计划",
  status: smokeResearchPlanStatus,
  open_questions: [],
  budget: { max_rounds: 3, max_tokens: 16000, max_concurrency: 2 },
  revision: smokeResearchPlanStatus === "approved" ? 2 : 1,
  nodes: [
    { id: "scope", title: "界定范围", objective: "明确研究对象", status: "ready", depends_on: [], children: [] },
    { id: "evidence", title: "收集证据", objective: "绑定文献与代码来源", status: "pending", depends_on: ["scope"], children: [] },
  ],
});
const payloads = {
  app_info: { version: "0.0.0-smoke", build: "smoke" },
  // D-404:关键 UI 偏好后端持久化通道。冒烟默认空 = 回退 localStorage 旧值,
  // 与真实首次启动(后端无记录)行为一致。
  ui_prefs_get: { theme: null, work_priority: {}, auto_max: null, continue_prompt: null, process_auto_state: {} },
  update_check: { newer: false },
  projects_get: { current: PROJECT, projects: [PROJECT], names: { [PROJECT]: "smoke" } },
  projects_rename: ({ path, name }) => ({ current: path, projects: [path], names: { [path]: name } }),
  projects_init: ({ path, name }) => ({ current: path, projects: [path], names: { [path]: name || "新项目" } }),
  // 所选目录没有 .kanzei,实际根落在上级 —— 这正是需求串项目的形态。
  project_root_info: { selected: PROJECT, resolved: "C:/smoke/parent", shared: true },
  project_detach: null,
  // R-136:Ollama 装了但服务没起 —— 最常见的"子代理静默失效"形态。
  fast_model_status: { managed: true, model: "qwen3.5:4b", installed: true, serviceUp: false, modelPresent: false, ready: false },
  fast_model_setup: "fast 子代理已就绪:qwen3.5:4b",
  files_snapshot: {
    files: [
      { path: "src/lib.rs", size: 2048, lines: 120, oversized: false, note: "冒烟样例:库入口" },
      { path: "docs/note.md", size: 512, chars: 300, oversized: false },
    ],
    dirs: {
      "": { files: 2, size: 2560, lines: 120 },
      "src": { files: 1, size: 2048, lines: 120 },
      "docs": { files: 1, size: 512, lines: 0 },
    },
    dirNotes: { "src": "源码目录" },
    unannotated: 1,
  },
  // R-252:拆解子代理命令桩——返回新产出的 R/D 编号,前端 toast/log/刷新用。
  idea_split: (args) => `I-${args?.id?.replace(/^I-/, "") ?? "001"} → R-101 D-101`,
  docs_snapshot: {
    requirements: [docEntry("R-001", "冒烟需求", "doing", { complexity: "中", batches: { done: 3, total: 11 }, fields: [["备注", "待更新"], ["验收", "这是一条刻意超过六十字符的长验收文本,用来验证编辑表单会把段落型字段升级为多行文本域,而不是塞进单行输入框把值截断到看不见"]], dependencies: [], dependents: ["R-002"], execution_model: "work_units_v1", work_units: [smokeWorkUnit] }), docEntry("R-002", "冒烟需求二", "todo", { batches: { done: 0, total: 1 }, dependencies: ["R-001"], dependents: [], blocked: true, block_reasons: ["未完成依赖: R-001"] })],
    defects: [docEntry("D-001", "冒烟缺陷", "open", { severity: "medium", fields: [["复现", "待澄清: 用户视角的易用性还是模型可消费性?"]] })],
    incident_metrics: {
      schema_version: 2,
      total_occurrences: 4,
      total_events: 5,
      promotion_events: 1,
      by_class: {
        execution_incident: { occurrences: 1, escaped: 0, escaped_rate: 0, repair_duration_ms_total: 120, repair_duration_ms_average: 120, repair_duration_samples: 1, promotions: 0 },
        development_defect: { occurrences: 1, escaped: 0, escaped_rate: 0, repair_duration_ms_total: 2400, repair_duration_ms_average: 2400, repair_duration_samples: 1, promotions: 0 },
        product_defect: { occurrences: 1, escaped: 1, escaped_rate: 1, repair_duration_ms_total: 3600, repair_duration_ms_average: 3600, repair_duration_samples: 1, promotions: 1 },
        regression: { occurrences: 1, escaped: 1, escaped_rate: 1, repair_duration_ms_total: 4800, repair_duration_ms_average: 4800, repair_duration_samples: 1, promotions: 0 },
      },
      overall: { escaped: 2, escaped_rate: 0.5, repair_duration_ms_total: 10920, repair_duration_ms_average: 2730, repair_duration_samples: 4 },
      historical_replay: {
        sample_count: 3,
        consistent_count: 3,
        consistent: true,
        formal_defect_samples: 2,
        execution_incidents_excluded: 1,
        samples: [
          { defect_id: "D-613", expected_class: "product_defect", rationale: "contract mismatch", excluded_from_formal_defect_total: false, consistent: true },
          { defect_id: "D-614", expected_class: "regression", rationale: "同步遗漏逃逸", excluded_from_formal_defect_total: false, consistent: true },
          { defect_id: "D-615", expected_class: "execution_incident", rationale: "预提交 Rust 语法失手", excluded_from_formal_defect_total: true, consistent: true },
        ],
      },
    },
    ideas: [docEntry("I-001", "冒烟想法", "inbox")],
    // D-414:研究工件必须带真实字段形态(URL / 证据锚 / refs)——此前这里是两个空
    // 数组,于是「来源列表」整条渲染路径从未被冒烟走过,六条全绿而真机点不开(用户实测)。
    // 夹具要覆盖两类可打开来源:文献(URL)与代码域(证据锚)。
    sources: [
      docEntry("S-001", "MemGPT: Towards LLMs as Operating Systems (arXiv 2310.08560)", "active", { fields: [["URL", "https://arxiv.org/abs/2310.08560"], ["类型", "文献(一手)"]] }),
      docEntry("S-002", "kanzei websearch 工具实现", "active", { fields: [["证据锚", "crates/kanzei-tools/src/websearch.rs:9"], ["类型", "代码域"]] }),
    ],
    findings: [
      docEntry("F-001", "kanzei 记忆定位:控制系统非 RAG 模块", "confirmed", { closed: true, fields: [["域", "代码"], ["等级", "V1"], ["refs", "S-001"]] }),
    ],
    research_topics: [
      {
        topic: "alpha-study",
        legacy: false,
        explorations: [
          {
            source_path: "C:/smoke/project/.kanzei/research/alpha-study/explorations/E-101.md",
            frontmatter: { kind: "exploration", id: "E-101", topic: "alpha-study", title: "Alpha 基线", status: "done", hypothesis: "基线可以复现", depends_on: [], supersedes: null },
            assumption: "基线假设",
            results: [{ result_id: "E-101-01", params_text: "默认参数", status: "succeeded", key_metrics_text: "acc=0.9", artifact_text: "figure.png", conclusion: "支持", artifact_dir: "explorations/E-101/E-101-01", source_line: 16 }],
            conclusion: "支持",
            follow_up: "继续扩大样本",
          },
          {
            source_path: "C:/smoke/project/.kanzei/research/alpha-study/explorations/E-102.md",
            frontmatter: { kind: "exploration", id: "E-102", topic: "alpha-study", title: "Alpha 扩展", status: "running", hypothesis: "扩展仍能复现", depends_on: ["E-101"], supersedes: "E-099" },
            assumption: "扩展假设",
            results: [],
            conclusion: "不确定",
            follow_up: "等待运行",
          },
          {
            source_path: "C:/smoke/project/.kanzei/research/alpha-study/explorations/E-103.md",
            frontmatter: { kind: "exploration", id: "E-103", topic: "alpha-study", title: "Alpha 复核", status: "draft", hypothesis: "复核可以否定偏差", depends_on: ["E-102"], supersedes: null },
            assumption: "复核假设",
            results: [],
            conclusion: "待验证",
            follow_up: "准备运行",
          },
        ],
        exploration_diagnostics: [{ path: "C:/smoke/project/.kanzei/research/alpha-study/explorations/E-102.md", line: 8, message: "depends_on 引用悬挂探索 `E-099`" }],
        sources: [docEntry("S-101", "Alpha 一手论文", "active", { topic: "alpha-study", fields: [["URL", "https://example.com/alpha"], ["类型", "文献(一手)"], ["作者", "Alpha Researcher"], ["年份", "2024"], ["等级", "V2"], ["证据深度", "正文级"]] }), docEntry("S-102", "Alpha 代码来源", "active", { topic: "alpha-study", fields: [["证据锚", "crates/kanzei-tools/src/websearch.rs:9"], ["类型", "代码域"], ["年份", "2023"]] }), docEntry("S-103", "Alpha arXiv 正文", "active", { topic: "alpha-study", fields: [["URL", "https://arxiv.org/abs/2301.12345"], ["类型", "文献(一手)"], ["年份", "2022"], ["证据深度", "摘要级"]] })],
        findings: [docEntry("F-101", "Alpha 发现", "draft", { topic: "alpha-study", fields: [["等级", "V2"], ["refs", "S-101"]] })],
        runs: [
          { run: { result_id: "E-101-01", exploration_id: "E-101", status: "succeeded", policy: "managed", execution_json: '{"kind":"local"}', started_at: 10, finished_at: 20, artifacts_json: '[{"kind":"figure","path":".kanzei/research/alpha-study/explorations/E-101/E-101-01/figure.png"}]', terminal_log_path: ".kanzei/research/alpha-study/explorations/E-101/E-101-01/terminal.log", metrics_series_path: ".kanzei/research/alpha-study/explorations/E-101/E-101-01/metrics.jsonl" }, events: [{ event_type: "metric", payload_json: '{"name":"acc","value":0.8}' }, { event_type: "message", payload_json: '{"level":"info","text":"训练完成"}' }], drift: ["workdir"] },
          { run: { result_id: "E-101-02", exploration_id: "E-101", status: "failed", policy: "relaxed", execution_json: '{"kind":"local"}', started_at: 30, finished_at: 40 }, drift: [] },
        ],
        report: true,
      },
      {
        topic: "beta-study",
        legacy: false,
        sources: [docEntry("S-101", "Beta 代码来源", "active", { topic: "beta-study", fields: [["证据锚", "crates/kanzei-tools/src/websearch.rs:9"], ["类型", "代码域"]] })],
        findings: [],
        report: true,
      },
    ],
    root: "C:/smoke/parent",
    warnings: [],
    work_units: [smokeWorkUnit],
    archived: { req: 1, defect: 2, idea: 0, source: 0, finding: 0 },
    conventions: { exists: true, headings: ["开发规则", "测试要求"] },
  },
  research_workflow_get: () => null,
  research_plan_get: (args) => args?.topic === "alpha-study"
    ? ({ exists: true, plan: smokeResearchPlan() })
    : ({ exists: false, topic: args?.topic }),
  research_plan_approve: (args) => {
    if (args?.topic !== "alpha-study") throw new Error("未知研究 topic");
    smokeResearchPlanStatus = "approved";
    return { exists: true, plan: smokeResearchPlan() };
  },
  // D-414:点 ↗ 抓正文进内置 viewer 的后端命令。
  webfetch_preview: { title: "MemGPT: Towards LLMs as Operating Systems", text: "# MemGPT\n\n正文摘录…" },
  research_arxiv_preview: (args) => ({ title: `arXiv ${args?.url ?? ""}`, text: "# arXiv 正文\n\n正文级抽取内容…", depth: "正文级", source_url: args?.url, path: `C:/smoke/project/.kanzei/research/${args?.topic}/fulltext/2301.12345.html` }),
  docs_archive_entries: (args) => args?.kind === "req" ? [docEntry("R-000", "已归档需求", "done")] : [docEntry("D-000", "已归档缺陷", "fixed")],
  // R-122:架构浏览。含一篇未入册文档,验证"未入册"分组可见。
  architecture_snapshot: {
    index_path: "C:/smoke/parent/.kanzei/project/architecture/README.md",
    index: "# 架构索引\n\n### 现行基线\n\n- [`direction_taste.md`](../../../docs/design/direction_taste.md)：方向基线。\n",
    design_docs: [
      { name: "direction_taste.md", title: "方向基线", bytes: 512 },
      { name: "memory_system.md", title: "Memory 系统设计基线", bytes: 2048 },
    ],
    // R-188:workspace crate 依赖边(代码生成架构图数据源)。
    graph: [
      ["kanzei-app", "kanzei-core"],
      ["kanzei-app", "kanzei-tools"],
      ["kanzei", "kanzei-tools"],
      ["kanzei-tools", "kanzei-harness"],
      ["kanzei-tools", "kanzei-llm"],
      ["kanzei-tools", "kanzei-core"],
      ["kanzei-core", "kanzei-harness"],
    ],
  },
  docs_read_custom: { path: "C:/smoke/parent/docs/design/memory_system.md", name: "memory_system.md", content: "# Memory 系统设计基线\n\n冒烟内容。" },
  docs_read: (args) => {
    const reports = {
      "alpha-study": "# Alpha report\n\nAlpha conclusion [S-101].",
      "beta-study": "# Beta report\n\nBeta conclusion [S-101].",
    };
    return {
      path: args?.topic ? `C:/smoke/project/.kanzei/research/${args.topic}/report.md` : "C:/smoke/project/.kanzei/research/report.md",
      name: "report.md",
      content: args?.topic ? (reports[args.topic] ?? "# Unknown report") : "# Legacy report",
      topic: args?.topic ?? null,
    };
  },
  defect_review: {
    empty: false,
    defectCount: 1,
    report: "# 缺陷自动审查报告\n\n- D-001: `src/main.rs:10` 有可复核证据",
  },
  // 新旧读取观测、未注入与 miss 必须按事实分别呈现。
  memory_recalls: {
    rounds: [{
      recall_id: "1-M", at: 1_760_000_000_000, run_id: "run-7", episode_id: 7,
      prompt_head: "这轮要发版", trigger_type: "memory_search", policy_action: "lexical", query: "发版 SOP", total_ms: 8,
      hits: [
        { id: "M-002", title: "发版 SOP", scope: "project", injected: true, read: true },
        { id: "M-001", title: "CRLF 未命中", scope: "project", injected: true, read: false },
        { id: "M-003", title: "历史记忆", scope: "project", injected: true, read: null },
        { id: "M-004", title: "候选", scope: "project", injected: false, read: null },
      ],
    }, {
      recall_id: "2-M", at: 1_760_000_000_001, run_id: null, episode_id: null,
      prompt_head: null, trigger_type: "event_recall", policy_action: "miss", query: "unknown failure", total_ms: 1, hits: [],
    }],
    rounds_total: 2,
  },
  // R-124:SOP 候选(带指纹,用于丢弃定位)。
  memory_note_candidates: [{
    scope: "global",
    hint: "sop",
    summary: "候选 SOP:完成 R-123(done)的流程[sop:R-123]",
    detail: "- 实际工具顺序: read → edit → bash → req",
    fingerprint: "[sop:R-123]",
  }],
  memory_note_discard: true,
  memory_value_flags: {
    zero_read: [{ scope: "project", id: "M-001", title: "CRLF 未命中", recalled: 5, injected: 4, read: 0, read_observed: 4 }],
    frequent: [{ scope: "project", id: "M-002", title: "发版 SOP", recalled: 4, injected: 3, read: 1, read_observed: 2 }],
    stale_archived: 0,
  },
  // R-099/R-127:一轮有画像、一轮早于度量落地,验证两者区分得开。
  run_metrics: {
    rounds: [
      {
        at: 1_760_000_000_000, prompt: "收口 R-123", outcome: "completed", steps: 12,
        inputTokens: 48_000, outputTokens: 3_200,
        tools: { edit: 6, bash: 4, req: 2 },
        context: [["agent/system", 4000], ["memory", 800]],
        metrics: { terminal_calls: 4, git_calls: 2, git_groups: 1, edit_calls: 6, edit_misses: 1, subagent_calls: 0, total_calls: 12, failed_calls: 1 },
        measured: true,
      },
      {
        at: 1_759_000_000_000, prompt: "更早的一轮", outcome: "completed", steps: 5,
        inputTokens: 10_000, outputTokens: 900, tools: {}, context: [], metrics: {}, measured: false,
      },
    ],
  },
  // R-338/R-340:真实 task projection 消费者夹具；禁止只测旧 rounds fixture。
  run_metrics_by_task: {
    completed_tasks: [{
      task_id: "task-metrics-1", title: "任务画像演示", status: "completed",
      session_ids: ["sess-smoke"], input_count: 1, round_count: 1,
      steps_sum: 12, input_tokens_sum: 48_000, output_tokens_sum: 3_200,
      rounds: [{ session_id: "sess-smoke", episode_id: 42, created_at: 1_760_000_000_000, outcome: "completed", steps: 12, input_tokens: 48_000, output_tokens: 3_200 }],
    }],
    in_progress_tasks: [{
      task_id: "task-metrics-open", title: "进行中画像", status: "in_progress",
      session_ids: ["sess-smoke"], input_count: 0, round_count: 0, steps_sum: 0,
      input_tokens_sum: 0, output_tokens_sum: 0, rounds: [],
    }],
    trend: { closed_task_count: 1, completed_task_count: 1 },
  },
  // 历史回放里的工具调用/结果:此前只有一条纯文本消息,历史工具块在运行时从未被执行过
  // (只有源码字符串断言),⎿ 摘要与展开详情的双写在这条路径上完全没有护栏。
  conversation_get: [
    { role: "user", parts: [{ type: "text", text: "冒烟历史消息" }] },
    {
      role: "assistant",
      parts: [
        { type: "tool_call", id: "H1", name: "edit", input: { path: "ui/x.js", old_string: "a", new_string: "b" } },
        { type: "tool_result", call_id: "H1", is_error: true, content: `${HISTORY_LONG_FIRST_LINE}\n第二行\n第三行` },
        { type: "tool_call", id: "H2", name: "bash", input: { command: "cargo test --workspace" } },
        { type: "tool_result", call_id: "H2", is_error: false, content: HISTORY_HUGE_OUTPUT },
        { type: "tool_call", id: "H3", name: "bash", input: { command: "true" } },
        { type: "tool_result", call_id: "H3", is_error: false, content: "exit code: 0\n真正的输出行" },
        { type: "tool_call", id: "H4", name: "bash", input: { command: "true" } },
        { type: "tool_result", call_id: "H4", is_error: false, content: "exit code: 0" },
      ],
    },
  ],
  conversation_trace_get: [],
  conversation_list: ({ processId }) => processId === "p|bg"
    ? [{ sequence: 2, sequences: [2], title: "后台线路历史", preview: "后台预览", updated_at: "2026-08-08 00:01" }]
    : [{ sequence: 1, sequences: [1], title: "冒烟会话", preview: "主线预览", updated_at: "2026-08-08 00:00" }],
  // 角色项 + 一个真实模型:角色不该出现在设置页的角色下拉里(会绕成自指)。
  models_list: [
    { id: "primary", label: "primary → anthropic:claude-sonnet-5" },
    { id: "anthropic:claude-sonnet-5", label: "anthropic:claude-sonnet-5" },
    { id: "ollama:qwen3", label: "ollama:qwen3" },
  ],
  git_status: { branch: "main", changes: 2 },
  list_pending_inputs: [],
  // UI-0926 #10:字段是真实 IPC 形状 [{key,value}](ipc-contract.json test_runs_snapshot),不是 [[k,v]]。
  test_runs_snapshot: { active: [{ id: "T-001", title: "冒烟测试", status: "passed", fields: [{ key: "命令", value: "cargo test" }], refs: ["R-001", "D-001"] }], archived: [] },
  test_runs_init_refs: { backfilled: 0 },
  process_list: [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, branch: "main", model: "deepseek:deepseek-chat", authority: "primary", stage: "复核" },
    // R-086 多会话并发:后台会话初始为运行中,桩里的旧 running=true 正是
    // "事件已收敛但轮询采样仍在事件之前"的竞态值,converged 必须挡住它。
    { id: "p|bg", label: "后台会话", session_id: "sess-bg", running: true, worktree_path: "C:/smoke-wt", branch: "kanzei/thread-smoke", tracker_writes: false, authority: "parallel", stage: "实现" },
  ],
  collaboration_snapshot: [
    {
      process_id: "d|smoke", label: "主会话", branch: "main", worktree_path: null,
      claim: "R-001", phase: "复核", current_tool: null, running: false,
      steps: 8, input_tokens: 2400, output_tokens: 600,
      changed_files: ["crates/shared.rs", "docs/main.md"],
    },
    {
      process_id: "p|bg", label: "后台会话", branch: "thread-a1", worktree_path: "C:/smoke/wt/thread-a1",
      claim: "未取得条目", phase: "实现", current_tool: "edit", running: true,
      steps: 3, input_tokens: 1200, output_tokens: 300,
      changed_files: ["crates/shared.rs", "crates/branch.rs"],
    },
  ],
  worktree_harvest_candidates: ["R-184"],
  process_close: "已关闭线路 p|bg；工作树有独有内容，已保留",
  pending_asks_get: [],
  agent_directory_get: {
    profile: "dev",
    agents: [
      { name: "dev", source: "builtin", path: null, profile: "dev", mode: "primary", model: "primary", steps: 0, status: "available", systemPreview: "smoke Agent", error: null },
      { name: "custom", source: "project", path: "C:/smoke/project/.kanzei/agents/custom.md", profile: "dev", mode: "subagent", model: "fast", steps: 4, status: "available", systemPreview: "custom smoke Agent", error: null },
    ],
  },
  agent_directory_open: null,
  // primary 是探测不到的已存值(端点没实现 /models),必须原样保留;
  // effective 与全局不同 = 项目级覆盖,界面要明说。
  settings_get: {
    language: "zh",
    path: "C:/smoke/.kanzei/kanzei.toml",
    primary: "deepseek:deepseek-chat",
    fast: "ollama:qwen3",
    proxy: "env",
    // readonly 是配置文件里的合法档位,但 index.html 的下拉只写了 dev/research:
    // 硬塞一个没有匹配 option 的值会让 select 落到空串,保存一次就把用户配置降级成 dev
    // (与模型角色同一个坑)。必须补出兜底 option。
    profileDefault: "readonly",
    reasoning: "off",
    codexFastMode: true,
    limits: { maxTokens: 4096, subagentTimeoutSecs: null },
    limitDefaults: {
      maxTokens: 8192, subagentMaxTokens: 4096, subagentTimeoutSecs: 900,
      contextBudgetRatio: 0.7, recentVerbatimRatio: 0.35, maxTasksPerTurn: 8,
      maxParallelTools: 8, transportRetries: 2, rateLimitRetries: 2, streamRestarts: 2,
    },
    // R-157:生效节奏(项目配置覆盖)+ 内置默认。继续文案应渲染 "全量测试每 3 批跑一次"。
    cadence: { full_test: "every_n_batches", full_test_batches: 3, targeted_test: "every_commit", commit: "per_batch", push: "per_entry" },
    cadenceDefaults: { full_test: "entry_close", full_test_batches: null, targeted_test: "every_commit", commit: "per_batch", push: "per_entry" },
    profiles: {},
    // 两个自定义 provider:表格里点「×」再保存,载荷必须只剩没删的那个(且仍是整张表)。
    // 加一个内置 provider(anthropic + builtin:true):删除入口必须是「内置」标记而不是 ×(D-246)。
    providers: [
      { name: "mine", protocol: "openai", baseUrl: "http://127.0.0.1:1", apiKeyEnv: "", apiKey: "", contextLimit: null },
      { name: "keepme", protocol: "openai", baseUrl: "http://127.0.0.1:2", apiKeyEnv: "", apiKey: "", contextLimit: null },
      { name: "anthropic", protocol: "anthropic", baseUrl: "https://api.anthropic.com", apiKeyEnv: "ANTHROPIC_API_KEY", apiKey: "", auth: null, contextLimit: 200000, builtin: true },
    ],
    permissions: [],
    // 项目级覆盖:D-168 当年只堵了模型角色,limits/proxy 被覆盖时页面一声不吭。
    // 这里让 primary/proxy/limits.maxTokens 各不相同(必须提示)、profileDefault 与
    // codexFastMode 相同(不得误报)、fast 整条缺失(has() 守卫必须整条跳过)。
    effective: {
      primary: "anthropic:claude-sonnet-5",
      reasoning: null,
      proxy: "http://127.0.0.1:7890",
      profileDefault: "readonly",
      codexFastMode: true,
      limits: { maxTokens: 8192, subagentTimeoutSecs: null },
    },
    projectConfig: "C:/smoke/project/.kanzei/kanzei.toml",
  },
  permission_rules_get: [],
  memory_overview: { scopes: [{ scope: "project", root: PROJECT, total: 0, hitsTotal: 0, categories: {}, integrity: [], inboxPending: 0 }] },
  memory_control_plane: {
    backlog: 3,
    oldest_waiting: "2026-08-20 [fact]",
    batch: { batch_id: "batch-7", status: "failed", pending_after: 3, failure_reason: "模拟 manager 失败" },
    promotion_gaps: 1,
    recall: { recalled: 8, injected: 3, read: 1, read_observed: 2 },
    effects: [{ memory_id: "M-SOP-001", effect_mean: 0.5, effect_ci: 0.2, eval_n: 4, last_eval: 1_760_000_000_000 }],
    experience_facts: [],
  },
  // 两条:一条有命中,一条陈旧且零命中(验证「长期零命中」标记与清理入口)。
  memory_entries: [
    { id: "M-SOP-001", category: "sop", title: "冒烟 SOP", description: "继续执行冒烟任务", status: "active", body: "执行冒烟任务", hits: 4, last_hit_at: 1_760_000_000_000, recalled: 4, injected: 3, read: 2, read_observed: 3, updated: "2026-08-01" },
    { id: "M-DEAD-001", category: "fact", title: "从没被用到的记忆", description: "冒烟用:零命中条目", status: "active", body: "陈旧结论", hits: 0, last_hit_at: 0, recalled: 0, injected: 0, read: 0, read_observed: 0, updated: "2026-01-01" },
  ],
  memory_search_page: [{ id: "M-SOP-001", scope: "project", category: "sop", title: "冒烟 SOP", snippet: "继续执行冒烟任务", status: "active", description: "继续执行冒烟任务", body: "执行冒烟任务", hits: 4, recalled: 4, injected: 3, read: 2, read_observed: 3, updated: "2026-08-01" }],
  memory_context_bill: { turns: [] },
  research_latex_templates: [
    { id: "basic_report", name: "基础报告", description: "适合研究阶段性报告与结论摘要。" },
    { id: "basic_paper", name: "基础论文", description: "适合结构完整的研究论文初稿。" },
    { id: "experiment_record", name: "实验记录", description: "适合按时间记录实验设置、结果与复盘。" },
    { id: "paper_with_figures", name: "带图表论文", description: "适合引用 research topic 中 figures/实验产物的论文。" },
  ],
  research_latex_create: (args) => ({
    tex_name: `${args?.documentName || "main"}.tex`,
    tex_path: `.kanzei/research/${args?.topic || "demo"}/latex/${args?.documentName || "main"}.tex`,
  }),  research_latex_insert_figure: (args) => ({
    reference: `../figures/${args?.figureName || "result.png"}`,
    tex_path: `.kanzei/research/${args?.topic || "demo"}/latex/${args?.documentName || "main"}.tex`,
  }),
  research_latex_compile: (args) => ({
    run_id: "run-smoke-1",
    success: true,
    document_name: `${args?.documentName || "main"}.tex`,
    diagnostics: "[smoke] PDF generated",
    pdf_path: `.kanzei/research/${args?.topic || "demo"}/latex/history/run-smoke-1/${args?.documentName || "main"}.pdf`,
  }),
  research_latex_history: [{
    run_id: "run-smoke-1",
    success: true,
    document_name: "paper.tex",
    pdf_path: ".kanzei/research/alpha-study/latex/history/run-smoke-1/paper.pdf",
  }],
  research_latex_pdf: { media_type: "application/pdf", data: "cGRmLXNtb2tl" },

  workspace_snapshot: {},
};
payloads.research_library_list = () => ({ entries: payloads.docs_snapshot.research_topics.map((entry) => ({ ...entry, id: entry.topic || "legacy", storage_root: PROJECT, linked_projects: [PROJECT], available: true, kind: entry.kind || (entry.legacy ? "legacy" : "research") })), diagnostics: [] });
const invokeLog = [];
const invokeArgs = [];
const savedPayloads = new Map();
// 探针回传要看具体参数(id 配对、取样内容),所以单独留一份带参日志。
const probeResults = [];
// 真机时序闸门:桩默认返回"已 resolve 的 promise",于是 `await invoke(...)` 之后的代码
// 走微任务,恒早于 setTimeout(0)。真机上 IPC 是毫秒级,顺序恰好相反——凡是"先刷新
// 再做某事"的时序契约,在默认桩下都会假绿。给某条命令挂一个闸门,就能把这一段
// 挂起,让 setTimeout 先跑完,复现真机顺序。
const invokeGates = new Map();
// 后端失败注入:真机上 docs_snapshot 会因目录被删/文件锁/解析失败而抛错,那条 catch
// 路径上的清理(比如作废挂起的跳转高亮)只有让桩真的抛错才测得到。
const invokeFailures = new Map();
async function invoke(cmd, args) {
  invokeLog.push(cmd);
  invokeArgs.push({ cmd, args });
  const gate = invokeGates.get(cmd);
  if (gate) await gate;
  const failure = invokeFailures.get(cmd);
  if (failure) throw new Error(failure);
  if (cmd === "settings_save") savedPayloads.set(cmd, args);
  if (cmd === "ui_probe_result") probeResults.push(args);
  // 桩可以是**函数**:同一条命令按入参返回不同结果。线清单要判「切走后不把甲的
  // 清单画进乙的面板」,必须让两个项目返回不同的清单,固定值做不到。
  if (cmd in payloads) {
    const stub = payloads[cmd];
    return structuredClone(typeof stub === "function" ? stub(args) : stub);
  }
  return null;
}
async function listen(event, handler) { handlers.set(event, handler); }
const handlers = new Map();

const storage = new Map();
storage.set("kz-auto-continue", "1");
// P1:启动恢复的上限必须作为同一次状态同步发到当前会话，不能仍让后端停在默认 10。
storage.set("kz-auto-max", "3");
// R-170:预置旧版默认继续文案(镜像历史 LEGACY_CONTINUE_PROMPTS[0],已删除)。
// 升级机制删除后旧值必须原样读回(验收③:不再触发覆盖);夹具保留用于断言。
storage.set(
  "kz-continue-prompt",
  "继续推进。取活顺序按本轮末尾给出的「开发重心」执行(它来自记忆里的用户定调,是唯一权威);" +
    "两个队列内部都按文档顺序自上而下拿第一个可做的,列表已按阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
    "2. 粒度 = 一轮一个完整条目:以做完当前这一条缺陷/需求为本轮目标;" +
    "同构批量改动(i18n、重命名、迁移这类)一轮吃掉完整类别,不要按两三处微切片。" +
    "确实超出单轮容量才按验收子项分轮,并在进展里写明批次边界。" +
    "「工作量大」「要改多个文件」都是正常工作,不是停下的理由。\n" +
    "3. 卡住就换一条:某条一时推不动,在「进展」里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
    "「阻塞」字段只写解除权不在你手里的事(已问过用户在等回复/缺凭据/依赖外部服务/用户直营)," +
    "且要写出具名解除人;「涉及多文件」「跨层改动」「需先确认方案(但没真问过)」都不是阻塞,写进展。" +
    "顺手复核碰到的条目:阻塞条件已满足的当场清空「阻塞」字段。看到 [调度死锁] 横幅时按横幅执行。\n" +
    "4. 关闭条目前逐条对照验收原文,每项给出精确代码位置证据;声称完成的能力必须有真实调用方或消费者," +
    "没有消费者的命令、死代码或只展示不接数据源的壳不算完成;沿用既有实现要显式标注为既有能力而非本次交付;" +
    "不得缩小验收里的平台或范围限定词。任一项证据不足就保留活动态写清缺口,不要打勾。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
    "验证选择与改动面匹配:纯 ui/ 改动跑 node 检查与冒烟脚本,动了 crates/ 才跑 cargo test。\n" +
    "一直做下去,不要用纯文本收尾。"
);
const localStorageShim = {
  getItem: (k) => (storage.has(k) ? storage.get(k) : null),
  setItem: (k, v) => storage.set(k, String(v)),
  removeItem: (k) => storage.delete(k),
};

const windowShim = {
  __TAURI__: { core: { invoke }, event: { listen } },
  addEventListener: () => {},
  matchMedia: (media) => ({ media, matches: false, addEventListener() {}, removeEventListener() {} }),
  confirm: () => true,
  // D-418:业务确认弹窗从 window.confirm 迁移到 confirmDialog(01-core.js),
  // 冒烟同样 mock 成立(立即确认),否则确认类操作断言会挂在挂起的 Promise 上。
  confirmDialog: () => true,
  innerWidth: 1280,
  innerHeight: 800,
};

class OptionShim extends Element {
  constructor(text, value) {
    super("option");
    this.text = text;
    this.textContent = text;
    this.value = value ?? text;
  }
}
class FileReaderShim {
  readAsDataURL() { fail("FileReader.readAsDataURL 在冒烟桩中未实现"); }
}
class MutationObserverShim {
  constructor(callback) { this.callback = callback; }
  observe() { mutationObservers.add(this); }
  disconnect() { mutationObservers.delete(this); }
}
class ResizeObserverShim {
  constructor(callback) { this.callback = callback; }
  observe() {}
  unobserve() {}
  disconnect() {}
}

// 冒烟里**有意注入**的后端失败会走 toastError → reportPersistentError,那是被测行为的
// 一部分,不该当成"意外的持久错误"判红。窗口显式开合并按片段精确匹配,离开窗口一律
// 恢复判红——不然就等于顺手把真错误也吞了。
let expectedPersistentError = null;
let expectedPersistentHits = 0;
// 同理,但走的是另一条出口:refreshDocsSoon 的 catch 只 console.error(不 toastError),
// 而"注入失败 → 它必须作废挂起的跳转高亮"正是要测的行为。规矩与上面一致:显式开窗、
// 按片段精确匹配、离开窗口立刻恢复判红——不然就等于顺手把真的 console.error 也吞了。
let expectedConsoleError = null;
let expectedConsoleHits = 0;

let copiedResearchCitation = "";
const sandbox = {
  Event: class Event { constructor(type, init = {}) { this.type = type; Object.assign(this, init); } },
  CustomEvent: class CustomEvent { constructor(type, init = {}) { this.type = type; Object.assign(this, init); } },
  __reportInitError: (label, err) => fail(`初始化步骤 ${label} 抛异常(已被 main.js 吞掉): ${err?.stack ?? err}`),
  __reportPersistentError: (text) => {
    if (expectedPersistentError && String(text).includes(expectedPersistentError)) {
      expectedPersistentHits += 1;
      return;
    }
    fail(`reportPersistentError: ${text}`);
  },
  console: {
    log: (...a) => console.log(...a),
    warn: (...a) => console.warn(...a),
    error: (...a) => {
      const text = a.map(String).join(" ");
      if (expectedConsoleError && text.includes(expectedConsoleError)) {
        expectedConsoleHits += 1;
        return;
      }
      fail(`console.error: ${text}`);
    },
  },
  window: windowShim,
  performance: globalThis.performance,
  document,
  localStorage: localStorageShim,
  navigator: { clipboard: { writeText: async (text) => { copiedResearchCitation = String(text); } } },
  NodeFilter: { SHOW_TEXT: 4 },
  Node: { TEXT_NODE: 3, ELEMENT_NODE: 1 },
  Option: OptionShim,
  FileReader: FileReaderShim,
  MutationObserver: MutationObserverShim,
  ResizeObserver: ResizeObserverShim,
  setTimeout: (fn, ms) => { const h = { fn }; pendingTimers.add(h); return h; },
  clearTimeout: (h) => pendingTimers.delete(h),
  setInterval: (fn) => { const h = { fn, interval: true }; pendingTimers.add(h); return h; },
  clearInterval: (h) => pendingTimers.delete(h),
  structuredClone: globalThis.structuredClone,
  requestAnimationFrame: (fn) => { rafQueue.push(fn); return rafQueue.length; },
  cancelAnimationFrame: () => {},
};
vm.createContext(sandbox);

const settle = () => new Promise((r) => setImmediate(r));
async function flush(rounds = 12) {
  for (let i = 0; i < rounds; i += 1) {
    await settle();
    const frames = rafQueue.splice(0);
    for (const fn of frames) await fn();
    const timers = [...pendingTimers];
    for (const h of timers) {
      if (!pendingTimers.has(h) || h.interval) continue;
      pendingTimers.delete(h);
      await h.fn();
    }
  }
}

// 手工排空一轮已排队的 setTimeout(闸门段里要复现"定时器先于 IPC 落地"的真机顺序,
// 不能用 flush——它会连锁把回调自己新排的定时器也冲掉)。
// 关键:**不得无条件 await 回调**。被排空的回调里若也有一次同名 invoke(典型如
// refreshDocsSoon 里的 docs_snapshot),它会撞上同一道还没放开的闸门,`await handle.fn()`
// 就此死等——CI 表现为挂死而不是判红,比红灯难查得多。这里一律带超时:任何情况下
// 都不挂死,超时就判红并说清原因。
const DRAIN_TIMEOUT_MS = 300;
async function drainTimersOnce(label) {
  for (const handle of [...pendingTimers]) {
    if (!pendingTimers.has(handle) || handle.interval) continue;
    pendingTimers.delete(handle);
    const timedOut = Symbol("drain-timeout");
    let timer = null;
    const result = await Promise.race([
      (async () => handle.fn())(),
      new Promise((resolve) => { timer = setTimeout(() => resolve(timedOut), DRAIN_TIMEOUT_MS); }),
    ]);
    clearTimeout(timer);
    if (result === timedOut) {
      fail(
        `${label}:排空定时器时有回调 ${DRAIN_TIMEOUT_MS}ms 未返回(多半是它内部的 invoke 撞上了还没放开的闸门)。` +
        "冒烟绝不能挂死,这里按失败处理;要么在闸门段前清掉该定时器,要么别在闸门段前排它。",
      );
    }
  }
}

function assert(condition, message) { if (!condition) fail(message); }
// D-498/B1：目录覆盖与浏览器执行顺序分离。HTML 顺序只校验每个真实入口都能在
// 目录清单中找到；目录额外文件（例如迁移期未接入入口的文件）不能被静默丢掉，
// 但也不能插入浏览器执行序列破坏 classic/defer TDZ。
const htmlScriptSrcs = [...html.matchAll(/<script\b[^>]*\bsrc="([^"]+)"[^>]*>/g)]
  .map((match) => match[1])
  .filter((src) => src.endsWith(".js") && !src.includes("://"));
const uiSourceNames = new Set(scriptSrcs);
const missingHtmlSources = htmlScriptSrcs.filter((name) => !uiSourceNames.has(name));
assert(
  missingHtmlSources.length === 0,
  `index.html 引用了不在 ui/ 目录清单中的脚本：${missingHtmlSources.join(", ")}`,
);
assert(
  scriptSrcs.length >= htmlScriptSrcs.length,
  `ui/ 目录清单少于 index.html 执行入口：目录 ${scriptSrcs.length}，HTML ${htmlScriptSrcs.length}`,
);
const listText = (id) => byId.get(id)?.textContent ?? "";

// ---------- 执行 ui/*.js ----------
// 诊断:main.js 的初始化 IIFE 逐步 catch 只 toast 不抛出,冒烟里 toast 不可见;
// 注入 reporter 把"吞掉的初始化异常"变成冒烟失败(同时保持生产行为不变)。
// 逐文件执行(与浏览器多 <script> 语义一致,含 TDZ):拼接后一次执行会把
// 函数声明提升到整串顶部,浏览器多脚本下会炸的 ReferenceError 在 vm 里反而跑通。
//
// R-264 B2:执行器分支——classic script(无 import/export)走 vm.runInContext
// (现状不变);含 import/export 的 ESM 文件走 vm.SourceTextModule(共享同一
// context,探针与逐文件 TDZ 语义保住)。ESM 的 linker 解析相对 import 到同目录
// 其他 ui 文件,按已在 context 执行过的模块返回(跨文件绑定经 context 全局)。
const PROBE_INIT = /toastError\(`\$\{localizedLabel\}\$\{t\("加载失败"\)\}:\$\{err\}`\);/;
const PROBE_PERSIST = /function reportPersistentError\(text, \{ retry = null \} = \{\}\) \{/;
let probeHits = 0;

// ESM 模块缓存:文件路径 → 已 link 的 SourceTextModule。同一 context 下多次
// import 同一模块返回同一实例(浏览器语义)。跨文件绑定经 context 全局变量
// (迁移后文件显式 import 需要的符号,提供方仍在 context)——与经典脚本一致。
const esmModuleCache = new Map();

/**
 * 创建 ESM 模块实例并入缓存(不 link、不 evaluate)。供两阶段加载的第一阶段使用:
 * 所有模块先创建(unlinked 状态),link 回调遇到缓存实例直接返回——vm 会处理
 * unlinked 模块的链接顺序,循环依赖 a↔b 因此可行(实测:先创建再 link 成功)。
 */
function createEsmModule(filename, instrumentedSource, sandbox) {
  const existing = esmModuleCache.get(filename);
  if (existing) return existing;
  const module = new vm.SourceTextModule(instrumentedSource, {
    context: sandbox,
    filename,
  });
  esmModuleCache.set(filename, module);
  return module;
}

/**
 * 链接并求值一个 ESM 模块(两阶段:先创建全部,再逐个 link+evaluate)。
 * link 回调返回缓存中的模块实例(可能 unlinked)——vm 按依赖图自动链接,
 * 循环依赖 a↔b 时 b 的 link 拿到 a 实例,求值顺序由 ESM 规范(TDZ)决定。
 */
async function linkAndEvaluate(filename, instrumentedSource, sandbox, sourcesByName) {
  const module = createEsmModule(filename, instrumentedSource, sandbox);
  if (module.status === "evaluated") return module;
  await module.link(async (childSpec) => {
    const match = /^\.\/([^/]+\.js)$/.exec(childSpec);
    if (!match) {
      throw new Error(`unsupported import specifier "${childSpec}" in ${filename}`);
    }
    const target = match[1];
    const cached = esmModuleCache.get(target);
    if (cached) return cached;
    const childSource = sourcesByName.get(target);
    if (childSource === undefined) {
      throw new Error(`import "${childSpec}" in ${filename}: target ${target} not in ui/`);
    }
    // 创建后返回——vm 自动链接该依赖及其子依赖。
    return createEsmModule(target, childSource, sandbox);
  });
  await module.evaluate();
  return module;
}

/**
 * 执行一个 ui 源文件(经典脚本或 ESM)。
 * - classic:vm.runInContext(现状,逐文件执行保 TDZ)。
 * - ESM:见 linkAndEvaluate——实例创建后立即入缓存,link 回调按依赖图递归接上,
 *   循环依赖返回缓存实例;evaluate 按需(依赖在 link 时已 evaluate)。
 * 抛错向上传播,由调用方统一 fail。
 */
async function executeUiSource(instrumented, filename, sandbox, sourcesByName) {
  const isEsm = /^\s*(import|export)\b/m.test(instrumented);
  if (!isEsm) {
    vm.runInContext(instrumented, sandbox, { filename });
    return;
  }
  await linkAndEvaluate(filename, instrumented, sandbox, sourcesByName);
}

async function runUiSources() {
  const sourcesByName = new Map();
  sources.forEach((src, i) => sourcesByName.set(scriptSrcs[i], src));
  const executionSources = htmlScriptSrcs.map((name) => {
    const src = sourcesByName.get(name);
    if (src === undefined) throw new Error(`index.html 执行入口 ${name} 不在 ui/ 源码清单中`);
    return [name, src];
  });
  const sourceIndexes = new Map(scriptSrcs.map((name, index) => [name, index]));
  try {
    // R-264 批3:两阶段加载。第一阶段:创建+link 全部 ESM 模块(link 回调按依赖
    // 图递归,循环依赖返回缓存实例——不在 link 期间 evaluate,避免「请求未入缓存」)。
    // classic 文件仍按 index.html 逐文件 runInContext，保住浏览器的 TDZ/执行顺序；
    // 目录中的额外源码只进入静态覆盖，不冒充浏览器已加载脚本。
    const esmOrder = [];
    // 先统一完成探针注入，再创建任何 ESM module。若边遍历边注入，前面的 ESM
    // 在 link 期间会缓存尚未处理的后续依赖，导致 reportPersistentError 等探针
    // 只落在未被实际消费的副本上。
    for (const [name, sourceText] of executionSources) {
      let instrumented = sourceText.replace(
        PROBE_INIT,
        'toastError(`${localizedLabel}${t("加载失败")}:${err}`); __reportInitError?.(label, err);'
      );
      if (instrumented !== sourceText) probeHits += 1;
      const beforePersist = instrumented;
      instrumented = instrumented.replace(
        PROBE_PERSIST,
        "function reportPersistentError(text, { retry = null } = {}) { __reportPersistentError?.(text);"
      );
      if (instrumented !== beforePersist) probeHits += 1;
      const index = sourceIndexes.get(name);
      if (index !== undefined) sources[index] = instrumented;
      sourcesByName.set(name, instrumented);
    }
    for (const [name] of executionSources) {
      const instrumented = sourcesByName.get(name);
      const isEsm = /^\s*(import|export)\b/m.test(instrumented);
      if (!isEsm) {
        vm.runInContext(instrumented, sandbox, { filename: name });
      } else {
        // 第一阶段:创建+link 模块(link 回调按依赖图递归,循环依赖返回缓存实例;
        // evaluate 在 linkAndEvaluate 内完成,幂等)。
        await linkAndEvaluate(name, instrumented, sandbox, sourcesByName);
        esmOrder.push(name);
      }
    }
    // 第二阶段:ESM 模块按 index.html 顺序 evaluate(link 已完成,依赖实例就绪;
    // 循环依赖的 TDZ 由 ESM 实例化顺序处理,与浏览器一致)。
    for (const name of esmOrder) {
      const module = esmModuleCache.get(name);
      if (module) await module.evaluate();
    }
    // 兼容桥:ESM export 挂 context 全局(渐进迁移期对 classic 消费者可见)。
    // 直接给 sandbox 对象赋值——Node contextify 后该对象属性变化对 vm 可见,
    // 冒烟断言(sandbox.refreshManual / runInContext)都走同一份。
    for (const [name, module] of esmModuleCache) {
      try {
        const ns = module.namespace;
        for (const key of Object.keys(ns)) {
          if (typeof ns[key] !== "undefined") sandbox[key] = ns[key];
        }
      } catch {
        // 忽略不可读 namespace。
      }
    }
    // ESM 的可变导出不是 sandbox 普通属性：测试中的赋值必须经模块 setter，
    // 否则只改了兼容桥副本，消费者仍读取原 live binding，测试会把真实链路误判为失效。
    const bindMutableEsmGlobal = (name, moduleName, setterName) => {
      const module = esmModuleCache.get(moduleName);
      const setter = module?.namespace?.[setterName];
      if (!module || typeof setter !== "function") {
        throw new Error(`mutable ESM export ${name} missing ${moduleName}.${setterName}`);
      }
      Object.defineProperty(sandbox, name, {
        configurable: true,
        enumerable: true,
        get: () => module.namespace[name],
        set: (value) => setter(value),
      });
    };
    bindMutableEsmGlobal("renderMarkdown", "04-markdown.js", "setRenderMarkdown");
    bindMutableEsmGlobal("activePane", "01-core.js", "setActivePane");
    bindMutableEsmGlobal("inputDialog", "01-core.js", "setInputDialog");
    bindMutableEsmGlobal("confirmDialog", "01-core.js", "setConfirmDialog");
    bindMutableEsmGlobal("pendingJumpId", "11-docs-list.js", "setPendingJumpId");
    bindMutableEsmGlobal("activeProcessId", "03-shell.js", "setActiveProcessId");
    bindMutableEsmGlobal("activeSessionId", "03-shell.js", "setActiveSessionId");
    bindMutableEsmGlobal("currentProject", "03-shell.js", "setCurrentProject");
    bindMutableEsmGlobal("neuralFlowEmit", "22-neural-flow.js", "setNeuralFlowEmit");

    // R-264 ESM:模拟浏览器 deferred module 语义——所有模块求值完后 DOM 就绪,
    // 触发收集到的 DOMContentLoaded 回调(顶层延迟执行的点在这里跑)。
    for (const fn of domReadyCallbacks.splice(0)) {
      fn();
    }
    // R-264 冒烟适配:defer 延迟的初始化是 async(18-startup IIFE 等),触发后需
    // 让微任务/定时器推进,否则断言在初始化未完成时执行。多次 flush 覆盖异步链。
    await flush(20);
  } catch (err) {
    fail(`ui/*.js 顶层执行抛异常: ${err.stack ?? err}`);
  }
  if (probeHits < 2) fail(`注入初始化异常探针失败:累计命中 ${probeHits}/2,启动序列的 catch 或错误上报形态已变化,请同步冒烟脚本`);
}

await runUiSources();

// R-285 金色神经流:运行时 API、静态画布与真实事件接线同时存在。
// 冒烟 DOM 不实现 CanvasRenderingContext2D,因此这里只验证初始化可降级、状态映射
// 与生产事件调用点；像素/帧率属于真实 WebView2 E3。
{
  const chatFlow = byId.get("neural-flow-chat");
  const memoryFlow = byId.get("neural-flow-memory");
  const memoryState = byId.get("memory-flow-state");
  assert(chatFlow && memoryFlow && memoryState, "R-285 神经流画布或状态节点未加载");
  assert(
    /id="neural-flow-chat"[^>]*aria-hidden="true"/.test(html),
    "主对话神经流必须对辅助技术隐藏",
  );
  const neuralFlowModule = esmModuleCache.get("22-neural-flow.js");
  const neuralFlowEmit = neuralFlowModule?.namespace?.neuralFlowEmit;
  assert(typeof neuralFlowEmit === "function", "R-285 neuralFlowEmit ESM 入口未注册");
  assert(
    /const idleAlpha = isMemory \? 0\.22 : 0\.(?:19|075)/.test(source) && source.includes("const ambientProgress ="),
    "R-285 记忆流静息轨迹必须保持清晰亮度与定向流光",
  );
  assert(source.includes("const trailSteps = 5"), "R-285 业务事件脉冲必须带可辨识的流动尾迹");
  neuralFlowEmit("memory_search_started", { query_length: 6 });
  assert(memoryState.textContent === "检索中", `记忆检索动画状态错误:${memoryState.textContent}`);
  vm.runInContext('neuralFlowEmit("memory_search_completed", { hit_count: 2 })', sandbox);
  assert(memoryState.textContent === "收敛", `记忆检索完成状态错误:${memoryState.textContent}`);
  assert(
    source.includes('neuralFlowEmit?.("run_started"'),
    "主对话 kz:turn 未接金色神经流",
  );
  assert(
    source.includes('neuralFlowEmit?.("memory_consolidation_started"'),
    "记忆整理操作未接金色神经流",
  );
}

// R-284 B3:结构化体验事件必须先归并到事实 store,再按归属分发。
{
  const neuralFlowModule = esmModuleCache.get("22-neural-flow.js");
  const originalNeuralFlowEmit = neuralFlowModule.namespace.neuralFlowEmit;
  const experienceProbe = { animation: [], payloads: [], memory_refreshes: 0 };
  sandbox.__experienceProbe = experienceProbe;
  neuralFlowModule.namespace.setNeuralFlowEmit((type, payload) => {
    if (type === "memory_snapshot") return;
    experienceProbe.animation.push(type);
    experienceProbe.payloads.push(payload);
  });
  const memoryRefreshBefore = invokeLog.filter((cmd) => cmd === "memory_entries").length;
  const projectFact = {
    schema_version: 1,
    event_id: "experience-smoke-memory-1",
    event_type: "memory_consolidation_completed",
    class: "fact",
    occurred_at: 1,
    session_id: "project-session-smoke",
    project_id: PROJECT,
    run_id: null,
    topic_id: "memory-topic",
    entity_id: "memory-001",
    payload: { pending_after: 0 },
  };
  handlers.get("kz:experience")({ payload: projectFact });
  await flush();
  experienceProbe.memory_refreshes = invokeLog.filter((cmd) => cmd === "memory_entries").length;
  assert(
    vm.runInContext('experienceProjectionBySession.get("project-session-smoke").topics.has("memory-topic")', sandbox),
    "R-284 B3 体验事件未按 topic_id 进入 session store",
  );
  assert(experienceProbe.memory_refreshes > memoryRefreshBefore, "R-284 B3 memory fact 未刷新同项目工作台");
  assert(
    vm.runInContext('experienceProjectionBySession.get("project-session-smoke").entities.has("memory-001")', sandbox),
    "R-284 B3 体验事件未按 entity_id 进入 session store",
  );
  assert(
    vm.runInContext("__experienceProbe.memory_refreshes", sandbox) === experienceProbe.memory_refreshes,
    "R-284 B3 memory fact 刷新计数未同步到探针",
  );
  handlers.get("kz:experience")({ payload: projectFact });
  await flush();
  assert(
    vm.runInContext("__experienceProbe.memory_refreshes", sandbox) === experienceProbe.memory_refreshes,
    "R-284 B3 重放同一 event_id 产生了重复工作台副作用",
  );
  handlers.get("kz:experience")({
    payload: {
      ...projectFact,
      event_id: "experience-smoke-background-run",
      event_type: "run_started",
      session_id: "background-session-smoke",
      project_id: PROJECT,
      topic_id: null,
      entity_id: null,
      class: "fact",
    },
  });
  assert(
    vm.runInContext("__experienceProbe.animation.length", sandbox) === 0,
    "R-284 B3 后台 session 事实错误驱动当前会话动画",
  );
  handlers.get("kz:experience")({
    payload: {
      ...projectFact,
      event_id: "experience-smoke-active-run",
      event_type: "run_started",
      session_id: vm.runInContext("activeSessionId", sandbox),
      topic_id: null,
      entity_id: null,
      class: "fact",
    },
  });
  assert(
    vm.runInContext("__experienceProbe.animation.length", sandbox) === 1
      && vm.runInContext("__experienceProbe.animation[0]", sandbox) === "run_started",
    "R-284 B3 当前 session 事件未分发到表现层",
  );
  handlers.get("kz:experience")({
    payload: {
      ...projectFact,
      event_id: "experience-smoke-tool-progress-1",
      event_type: "tool_progressed",
      session_id: vm.runInContext("activeSessionId", sandbox),
      topic_id: null,
      entity_id: null,
      class: "delta",
      payload: { text: "tool chunk" },
    },
  });
  await flush();
  assert(
    vm.runInContext("__experienceProbe.animation.length", sandbox) === 2
      && vm.runInContext("__experienceProbe.animation[1]", sandbox) === "tool_progressed",
    "D-684 tool_progressed 未分发到当前 session 神经流",
  );

  for (const [event_id, text] of [["experience-smoke-delta-1", "a"], ["experience-smoke-delta-2", "b"], ["experience-smoke-delta-3", "c"]]) {
    handlers.get("kz:experience")({
      payload: {
        ...projectFact,
        event_id,
        event_type: "text_delta",
        session_id: vm.runInContext("activeSessionId", sandbox),
        topic_id: null,
        entity_id: null,
        class: "delta",
        payload: { text },
      },
    });
  }
  await flush();
  assert(
    vm.runInContext("__experienceProbe.animation.length", sandbox) === 3
      && vm.runInContext("__experienceProbe.payloads[2].delta_count", sandbox) === 3
      && vm.runInContext("__experienceProbe.payloads[2].text", sandbox) === "abc",
    "R-284 B4 text delta 未合并为单次表现事件",
  );
  handlers.get("kz:experience")({
    payload: {
      ...projectFact,
      event_id: "experience-smoke-unknown-1",
      event_type: "future_experience_event",
      class: "presentation",
      session_id: vm.runInContext("activeSessionId", sandbox),
      topic_id: null,
      entity_id: null,
    },
  });
  assert(
    vm.runInContext("__experienceProbe.animation.length", sandbox) === 3,
    "R-284 B4 未知体验事件错误驱动表现层",
  );
  const replayFact = {
    ...projectFact,
    event_id: "experience-smoke-replay-1",
    event_type: "research_verify_completed",
    session_id: "reconnected-session-smoke",
    topic_id: "research-topic",
    entity_id: "claim-1",
  };
  assert(vm.runInContext("replayExperienceFacts([" + JSON.stringify(replayFact) + "])" , sandbox) === 1, "R-284 B4 首次重连事实未恢复");
  assert(vm.runInContext("replayExperienceFacts([" + JSON.stringify(replayFact) + "])" , sandbox) === 0, "R-284 B4 重连事实重复恢复");
  assert(
    vm.runInContext('experienceProjectionBySession.get("reconnected-session-smoke").facts.has("research_verify_completed")', sandbox),
    "R-284 B4 重连未从持久事实恢复 session 投影",
  );
  neuralFlowModule.namespace.setNeuralFlowEmit(originalNeuralFlowEmit);
}

// D-420:先验证生产输入弹窗本身可打开、回填并确认,再替换为立即返回桩供后续业务用例复用。
const inputDialogProbe = vm.runInContext(
  'inputDialog({ title: "D-420 输入测试", value: "默认值" })',
  sandbox,
);
assert(!byId.get("input-overlay").classList.contains("hidden"), "输入弹窗调用后未显示");
assert(byId.get("input-value").value === "默认值", "输入弹窗未回填默认值");
byId.get("input-value").value = "已输入";
byId.get("input-ok").click();
assert(await inputDialogProbe === "已输入", "输入弹窗确认未返回用户输入");
assert(byId.get("input-overlay").classList.contains("hidden"), "输入弹窗确认后未关闭");
assert(
  !sources.some((source) => /window\.prompt\s*\(/.test(source)),
  "生产 UI 仍保留 window.prompt 调用(WebView2 下会失效)",
);
// 后续业务用例需要不同输入,用队列桩模拟用户逐次提交。
sandbox.__inputDialogResponses = [];
vm.runInContext(
  "inputDialog = () => Promise.resolve(__inputDialogResponses.shift() ?? null)",
  sandbox,
);

// D-418:业务确认弹窗从 window.confirm 迁移到全局函数 confirmDialog(01-core.js)。
// windowShim 里的 confirmDialog mock 会被页面脚本的 `function confirmDialog` 声明
// 覆盖,必须在源码执行完后重新覆盖为「立即确认」,确认类操作的断言才不会被挂起
// 的 Promise 卡住(放弃工作树/新建线路等)。
vm.runInContext("confirmDialog = () => true", sandbox);

// R-264 B3：从已经 link/evaluate 的 08-compose.js ESM namespace 取得测试钩子。
// 不再向 vm context 注入字符串，也不依赖 classic 文件的共享词法作用域。
const kzTestModule = esmModuleCache.get("08-compose.js");
const kzTest = kzTestModule?.namespace?.__kzTest;
assert(kzTest, "未从 08-compose.js ESM namespace 获取鞭挞状态测试钩子");
// D-504:活动线配置必须来自 processAutoState，顶栏控件只是投影。
{
  const activeProcess = vm.runInContext("activeProcessId", sandbox);
  if (activeProcess) {
    kzTest.setAutoState(activeProcess, { enabled: true, paused: false, stopAfterRound: false, maxRounds: 7 });
    byId.get("auto-continue").checked = false;
    const projected = vm.runInContext(`lineAutoConfig(${JSON.stringify(activeProcess)})`, sandbox);
    assert(projected.enabled === true && projected.maxRounds === 7, `活动线配置错误地读取 DOM: ${JSON.stringify(projected)}`);
  }
}
await flush();
assert(invokeLog.includes("projects_get"), `初始化未调用 projects_get(启动序列断裂),已见调用: ${invokeLog.join(",")}`);
assert(invokeLog.includes("docs_snapshot"), "初始化未调用 docs_snapshot");
// D-553:页面重载后接管已在运行会话时，本页没有本地 runStart；不得把纪元时间当耗时。
{
  const reported = vm.runInContext("roundElapsedSeconds(1234)", sandbox);
  assert(Math.abs(reported - 1.234) < 0.0001, `kz:done elapsedMs 未换算为秒: ${reported}`);
  const afterReload = vm.runInContext("runStart = 0; roundElapsedSeconds(undefined)", sandbox);
  assert(afterReload === null, `无本地 runStart 时不应生成绝对耗时: ${afterReload}`);
}
// R-336:设置页不再提供使用手册入口、内容或专属交互；通用文件预览仍由文件查看器单独覆盖。
{
  const manualIds = ["manual-panel", "manual-body", "manual-toggle-hint", "set-show-manual"];
  assert(
    manualIds.every((id) => !byId.has(id)),
    `R-336:设置页仍残留使用手册 DOM: ${manualIds.filter((id) => byId.has(id)).join(",")}`,
  );
  assert(
    !sources.some((source) => /refreshManual|readManualShowPref|saveManualShowPref|MANUAL_SHOW_KEY|MANUAL_PATHS|manual-panel|set-show-manual/.test(source)),
    "R-336:前端仍残留使用手册专属函数、偏好或选择器",
  );
  assert(
    !invokeArgs.some(({ cmd, args }) => cmd === "file_preview" && ["docs/使用手册.md", "docs/目录.md"].includes(args?.path)),
    "R-336:初始化仍读取使用手册文件",
  );
}
// D-317:空配置必须停在明确的「未选择项目」状态，不能因渲染而触发项目级请求。
// 后端另有纯函数反证锁死「不拿 current_dir 造项目」；这里验证 classic-script 的空态承载。
{
  const processListCalls = invokeLog.filter((cmd) => cmd === "process_list").length;
  vm.runInContext("renderProjects({ current: null, projects: [], names: {} })", sandbox);
  await flush();
  assert(vm.runInContext("currentProject", sandbox) === null, "空项目偏好仍留下了当前项目");
  assert(byId.get("project-list").children.length === 0, "空项目偏好仍渲染出项目卡片");
  assert(byId.get("project-label").textContent.includes("未选择项目"), "空项目状态未显示『未选择项目』");
  assert(byId.get("documents-project-select").disabled, "空项目状态下文档项目选择器仍可用");
  assert(
    invokeLog.filter((cmd) => cmd === "process_list").length === processListCalls,
    "空项目状态仍请求了项目级 process_list"
  );
  vm.runInContext(
    `renderProjects(${JSON.stringify(payloads.projects_get)})`,
    sandbox
  );
  await flush();
}
assert(byId.get("project-label").textContent === "smoke", "项目胶囊应显示项目名而非裁坏的完整路径");
assert(byId.get("project-label").title === PROJECT, "项目胶囊 title 应保留完整路径");
assert(
  byId.get("project-label").getAttribute("aria-label")?.includes(PROJECT),
  "项目胶囊无障碍标签应保留完整路径",
);
// D-420:项目重命名与新建都走应用内输入弹窗,取消/确认语义仍由调用方消费。
{
  const projectItem = byId.get("project-list").children[0];
  assert(projectItem, "输入弹窗回归缺少项目卡片夹具");
  sandbox.__inputDialogResponses.push("重命名后的项目");
  projectItem.querySelector(".rename").click();
  await flush();
  const renameCall = invokeArgs.findLast(({ cmd }) => cmd === "projects_rename");
  assert(renameCall?.args?.name === "重命名后的项目", "项目重命名未消费输入弹窗的值");

  sandbox.__inputDialogResponses.push("C:/smoke/new-project", "新项目显示名");
  byId.get("project-init").click();
  await flush();
  const initCall = invokeArgs.findLast(({ cmd }) => cmd === "projects_init");
  assert(initCall?.args?.path === "C:/smoke/new-project", "新建项目未消费目录输入");
  assert(initCall?.args?.name === "新项目显示名", "新建项目未消费显示名输入");
  vm.runInContext(`renderProjects(${JSON.stringify(payloads.projects_get)})`, sandbox);
  await flush();
}
const initialAutoState = invokeArgs.find(({ cmd, args }) =>
  cmd === "auto_state_update" && args?.sessionId === "sess-smoke"
);
assert(initialAutoState && !Object.hasOwn(initialAutoState.args, "maxRounds"), "启动恢复不得把旧上限发送为新的硬停机门禁");
assert(storage.get("kz-auto-max") === "3", "旧 auto_max 配置应继续保留供兼容读取");
// 完整需求/缺陷列表整体搬进单页视图(侧栏只留「当前在做」的焦点卡片),落点换了、断言跟着搬。
assert(listText("documents-req-list").includes("冒烟需求"), `需求列表未渲染出桩数据: "${listText("documents-req-list").slice(0, 60)}"`);
assert(listText("documents-defect-list").includes("冒烟缺陷"), "缺陷列表未渲染出桩数据");
assert(
  document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"] .work-unit-badge')?.textContent.includes("W 0/1 · active"),
  "work_units_v1 需求未渲染执行单元进度徽标",
);
assert(
  document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"] .work-unit-card')?.textContent.includes("同步 IPC 契约"),
  "Work Unit 详情未渲染 checkpoint 的下一步",
);
assert(
  document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"] .work-unit-card')?.textContent.includes("事件底座已落地") &&
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"] .work-unit-card')?.textContent.includes("记忆来源: M-001") &&
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"] .work-unit-card')?.textContent.includes("验证结果: cargo test"),
  "Work Unit 卡片未呈现实质进展、记忆来源或验证结果",
);
// R-170:LEGACY 升级机制已删除——预置的旧默认文案必须原样读回,不再被覆盖
// (验收③);删空 textarea 回落极简默认,且极简默认不含任何引擎规则文本(验收①)。
{
  const storedPrompt = storage.get("kz-continue-prompt") ?? "";
  const textareaPrompt = (byId.get("continue-prompt")?.value ?? "").trim();
  assert(
    storedPrompt.includes("粒度 = 一轮一个完整条目"),
    "旧默认文案被覆盖:升级机制应已删除,旧值应原样保留在 localStorage"
  );
  assert(
    textareaPrompt === storedPrompt,
    "textarea 与 localStorage 不一致:旧默认文案应原样读回(不触发覆盖)"
  );
  // 删空 textarea → 回落极简默认,且不含批次粒度/阻塞定义/验收证据/验证节奏文本。
  const textarea = byId.get("continue-prompt");
  textarea.value = "";
  textarea.dispatchEvent({ type: "change" });
  const minimal = (byId.get("continue-prompt")?.value ?? "").trim();
  assert(
    minimal.includes("继续推进"),
    `极简默认应保留「继续推进」意图句: ${minimal.slice(0, 60)}`
  );
  for (const ruleText of ["粒度", "阻塞字段", "验收证据", "全量测试每 3 批", "一直做下去"]) {
    assert(
      !minimal.includes(ruleText),
      `极简默认仍含引擎规则文本「${ruleText}」: ${minimal.slice(0, 120)}`
    );
  }
  // 恢复夹具:后续用例按极简默认对待。
  storage.set("kz-continue-prompt", minimal);
}
// 批次进度格(R-160):格数与已填格必须来自后端算好的 entry.batches,前端不得另存
// 一份复杂度→格数的映射;总数为 1 的条目不画格(一轮做完的东西不需要进度条)。
// 批次上限 10 只在写入侧(docstore.rs check_declared_batches)拦截,读路径与渲染必须原样
// 透传:归档里 11/11、16/16 的历史条目若被前端二次钳制成 10,格子数与 aria-label 就成了假数。
{
  const meter = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"] .batch-meter');
  assert(meter, "批次进度格没渲染出来");
  const cells = meter.querySelectorAll(".complexity-cell");
  assert(cells.length === 11, `11 批应画 11 格(前端不得二次钳制到 10),实际 ${cells.length}`);
  assert(
    cells.filter((c) => c.className.includes("filled")).length === 3,
    "已完成 3 批就该填 3 格",
  );
  assert(
    meter.style.getPropertyValue("--cells") === "11",
    `轨道要按批次数等分,--cells 实际为 ${meter.style.getPropertyValue("--cells")}`,
  );
  assert(
    (meter.getAttribute("aria-label") ?? "").includes("3/11"),
    `读屏标签要带准确批次数:${meter.getAttribute("aria-label")}`,
  );
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"] .batch-meter'),
    "总数为 1 的条目不该画进度格(一轮做完的东西没有进度可言)",
  );
}
// D-242 新口径:批数由 agent 显式声明(`批次: k/N`),上限 10;未声明就没有批次
// (docstore.rs batch_progress 返回 (0,1)),复杂度不再凭空生成 3/8 个空格子。
// 上界 10/10 与"未声明不画格"这两种形态此前从未被渲染路径覆盖过。
{
  const savedBatchDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    ...savedBatchDocs,
    requirements: [
      docEntry("R-010", "走满上限的条目", "doing", { complexity: "大", batches: { done: 10, total: 10 } }),
      docEntry("R-011", "未声明批次的大条目", "todo", { complexity: "大", batches: { done: 0, total: 1 } }),
    ],
  };
  await sandbox.refreshDocs();
  const full = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-010"] .batch-meter');
  assert(full, "走满上限(10/10)的条目没画进度格");
  const fullCells = full.querySelectorAll(".complexity-cell");
  assert(fullCells.length === 10, `10 批应画 10 格,实际 ${fullCells.length}`);
  assert(fullCells.every((c) => c.className.includes("filled")), "10/10 应全部填满");
  assert(full.style.getPropertyValue("--cells") === "10", "10 批的 --cells 不对");
  assert((full.getAttribute("aria-label") ?? "").includes("10/10"), "10/10 读屏标签不对");
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-011"] .batch-meter'),
    "未声明批次的「大」条目不该凭空画出进度格(D-242:复杂度不再映射默认批数)",
  );
  payloads.docs_snapshot = savedBatchDocs;
  await sandbox.refreshDocs();
}
assert(listText("idea-list").includes("冒烟想法"), "想法列表未渲染出桩数据");
// R-252 验收④:想法区有「拆解」按钮,点击派 idea_split 子代理(不做自动拆解)。
{
  const ideaItem = [...document.querySelectorAll('#idea-list .doc-item')]
    .find((el) => el.dataset.docId === "I-001");
  assert(ideaItem, "想法条目 I-001 未渲染");
  ideaItem.click();
  const splitBtn = [...document.querySelectorAll('#idea-list .doc-progress button')]
    .find((b) => b.textContent.includes("拆解"));
  assert(splitBtn, "inbox 想法应渲染「拆解成需求/缺陷」按钮");
  const before = invokeLog.filter((cmd) => cmd === "idea_split").length;
  splitBtn.click();
  assert(
    invokeLog.filter((cmd) => cmd === "idea_split").length > before,
    "点拆解按钮未调用 idea_split 子代理命令",
  );
  const splitCall = invokeArgs.find(({ cmd }) => cmd === "idea_split");
  assert(splitCall?.args?.id === "I-001", "idea_split 未带想法 id 参数");
}
assert(listText("test-list").includes("冒烟测试"), "测试记录列表未渲染出桩数据");
// R-130:测试→条目映射——关联的 R-/D- 条目号渲染成可点击跳转的徽标。
{
  // R-130 验收③:批量初始化必须有真实调用方——refreshTests 每次刷新前先跑
  // test_runs_init_refs(幂等回填旧记录关联字段),再取快照渲染。
  const initCalls = invokeLog.filter((cmd) => cmd === "test_runs_init_refs");
  assert(initCalls.length >= 1, `测试列表刷新未调用批量初始化:init 调用次数 ${initCalls.length}`);
  const initArgs = invokeArgs.find(({ cmd }) => cmd === "test_runs_init_refs");
  assert(initArgs && initArgs.args?.projectDir, "test_runs_init_refs 未带 projectDir 参数");
  const testEntry = document.querySelector("#test-list .test-entry");
  assert(testEntry, "前置失败:测试记录条目未渲染");
  const chips = testEntry.querySelectorAll(".test-ref-chip");
  assert(chips.length === 2, `测试条目应渲染 2 个关联徽标,实得 ${chips.length}`);
  assert(
    [...chips].some((c) => c.textContent === "R-001") && [...chips].some((c) => c.textContent === "D-001"),
    `关联徽标内容应为 R-001/D-001:${[...chips].map((c) => c.textContent).join(",")}`,
  );
  // 点徽标跳转到对应条目:先离开文档视图,验证 jumpToEntry 被触发。
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  const before = invokeLog.filter((cmd) => cmd === "docs_snapshot").length;
  [...chips].find((c) => c.textContent === "R-001").click();
  await flush();
  assert(
    invokeLog.filter((cmd) => cmd === "docs_snapshot").length > before,
    "点击测试关联徽标未触发跳转刷新",
  );
}
// R-314:单线程时隐藏协作者工具，多线路恢复显示；判据来自真实 process_list 投影。
{
  vm.runInContext(`renderProcesses(${JSON.stringify([payloads.process_list[0]])})`, sandbox);
  assert(byId.get("collaboration-tools").classList.contains("hidden"), "单线程时协作者工具仍可见");
  vm.runInContext(`renderProcesses(${JSON.stringify(payloads.process_list)})`, sandbox);
  assert(!byId.get("collaboration-tools").classList.contains("hidden"), "多线路时协作者工具未恢复显示");
}
// 历史必须随线路渲染，不能再退回一个全局 conversation-list，否则切线后无法判断归属。
assert(!byId.has("conversation-list"), "历史对话不应再有独立的全局列表");
const lineHistories = document.querySelectorAll("#parallel-task-status .parallel-line-history");
assert(lineHistories.length === 2, `两条线路都应有自己的历史容器,实际 ${lineHistories.length}`);
// 历史默认**收起**:只出折叠头(带条数),不铺开标题——多线路同时展开会把
// 「各线当前在做」挤出第一屏。展开后才渲染列表,且仍按 process_id 隔离。
for (const history of lineHistories) {
  assert(
    history.querySelector(".parallel-history-head") && !history.classList.contains("open"),
    `历史对话应默认收起并只渲染折叠头:${history.dataset.processId}`,
  );
  assert(
    !history.textContent.includes("冒烟会话") && !history.textContent.includes("后台线路历史"),
    `收起状态不应渲染历史标题:${history.textContent.slice(0, 60)}`,
  );
  history.querySelector(".parallel-history-head").click();
}
await flush();
const openedHistories = document.querySelectorAll("#parallel-task-status .parallel-line-history");
assert(
  [...openedHistories].some((history) => history.dataset.processId === "d|smoke" && history.textContent.includes("冒烟会话")),
  "主线历史没有挂到主线按钮下面",
);
assert(
  [...openedHistories].some((history) => history.dataset.processId === "p|bg" && history.textContent.includes("后台线路历史") && !history.textContent.includes("冒烟会话")),
  "并行线历史没有按 process_id 隔离渲染",
);
// D-202 家族：实时追加必须有上界。恢复历史早就窗口化了（PANE_WINDOW_SIZE），
// 但 appendToPane 原先从不裁剪：一次自主推进跑几百轮，pane 无上界地长，而每次追加
// 后的 scrollBottom 都要对整棵树强制布局，单次代价随 DOM 线性增长（playwright 实测：
// 顶层 200 节点时追加 200 条耗 36ms，2400 节点时 476ms）。这段锁住上界本身。
// 压测跑在一个**临时 pane** 上：直接往当前 pane 塞一千条再 resetPane，会把 hasContent
// 标志一并抹掉，后续 showPane 会误判为空 pane 而重拉历史，污染后面的工具块断言。
{
  vm.runInContext("globalThis.__paneSave = activePane; activePane = document.createElement('div'); activePane.className = 'msg-pane';", sandbox);
  const pane = vm.runInContext("activePane", sandbox);
  for (let i = 0; i < 700; i++) vm.runInContext(`addMessage("notice", "裁剪压测 ${i}")`, sandbox);
  const after = pane.children.length;
  assert(after <= 601, `实时 pane 未裁剪：追加 700 条后顶层节点 ${after}（应 ≤ 601）`);
  assert(after >= 400, `裁剪过头：追加 700 条后只剩 ${after} 条（应 ≥ 400）`);
  assert(Number(pane.dataset.droppedLive || 0) > 0, "裁剪发生了但没记 droppedLive（提示条拿不到数）");
  assert(pane.querySelector(".pane-trimmed-hint"), "裁剪后缺少顶部说明条 .pane-trimmed-hint（静默丢消息）");
  // 说明条不能反过来被裁剪循环吃掉：再追加一批后它仍只有一条。
  for (let i = 0; i < 250; i++) vm.runInContext(`addMessage("notice", "裁剪压测二 ${i}")`, sandbox);
  const hints = pane.querySelectorAll(".pane-trimmed-hint").length;
  // D-490:复制上下文不能把当前 pane 的裁剪提示静默丢掉。
  byId.get("copy-context").click();
  await flush();
  assert(
    copiedResearchCitation.includes("较早的") && copiedResearchCitation.includes("已移出视图以保持流畅"),
    `长会话复制缺少明确裁剪标记: ${copiedResearchCitation.slice(0, 120)}`,
  );
  assert(hints === 1, `裁剪说明条应始终只有一条，实际 ${hints}`);
  vm.runInContext("activePane = globalThis.__paneSave; delete globalThis.__paneSave;", sandbox);
}
// 跟随态判定契约（假 DOM 做不了真手势，静态锁）：不得回到「每个 scroll
// 事件都重算 nearBottom」——我们自己写 scrollTop 同样会发 scroll 事件，合帧之后
// 那一下会被误判成「用户往上滚了」，跟随态关掉后就再也钉不住底了。
{
  const render = sources[scriptSrcs.indexOf("05-chat-render.js")] ?? "";
  assert(
    render.includes("programmaticUntil") && render.includes("noteProgrammaticScroll"),
    "05-chat-render 丢失程序滚动窗口：自己的滚动会把跟随态关掉",
  );
  assert(
    render.includes('"wheel", "pointerdown", "touchstart", "keydown"'),
    "05-chat-render 丢失真实手势监听：用户滚上去后新消息会把他拽回底部",
  );
  const core = sources[scriptSrcs.indexOf("01-core.js")] ?? "";
  assert(
    core.includes("const topBefore = visible ? messages.scrollTop : 0;"),
    "trimLivePane 的 scrollTop 取样必须在删除之前：删完再读到的是夹紧后的值，同一个 delta 会扣两遍",
  );
}
// 命令面板:开面板必须让背景**整体**惰性化,否则 aria-modal 只是一句声明——
// Tab 两下就走到背后的 rail,回车能在遮罩下真的切视图、点「新对话」(清空历史)。
// 关面板必须摘干净,否则界面整个点不动。
// UI-0926 #9:宿主改为 <dialog>,惰性化由 showModal 原生承担(背景整体 inert、焦点关在面板里),
// 于是护栏改成「必须以模态打开」:open 且 _modal;关闭后 open 为 false 且镜像回 .hidden。
// 另加一条反证:任何代码都不得再手写 inert(那说明有人又绕开了 showModal)。
{
  vm.runInContext("openPalette()", sandbox);
  const palette = byId.get("palette");
  assert(palette?.tagName === "DIALOG", `命令面板宿主必须是 <dialog>(实得 ${palette?.tagName})`);
  assert(palette?.open && palette?._modal, "开命令面板未以 showModal 打开（背景不会被原生惰性化，焦点会跑到遮罩背后）");
  assert(!palette.classList.contains("hidden"), "命令面板打开后仍带 .hidden");
  vm.runInContext("closePalette()", sandbox);
  assert(!palette.open && palette.classList.contains("hidden"), "关命令面板后仍是打开态或未镜像回 .hidden");
  assert(
    !sources.some((source) => /setAttribute\("inert"/.test(source)),
    "又出现了手写 inert:模态的背景惰性化只归 <dialog>.showModal(经 00-surface openDialog)",
  );
}
// 搜索开关住在收起的「更多」弹层菜单(#composer-more-menu,popover)里。命令面板会绕过菜单直接
// .click() 它——宿主不展开的话,摘掉 hidden 也没人看得见,接着敲的关键词会掉进
// #prompt,裸 Enter 就把它当任务发给了 agent。
// 假 DOM 的 HTML 解析把 #chat-search 拍平到 body 下,closest("[popover]") 在这里
// 天然拿不到宿主,所以**展开宿主**这一条只能静态锁(真实浏览器行为由 playwright
// 核验);能在假 DOM 里验的是"点了确实把搜索条摘出 hidden"。
{
  byId.get("chat-search").classList.add("hidden");
  byId.get("chat-search-toggle").click();
  assert(
    !byId.get("chat-search").classList.contains("hidden"),
    "点搜索后搜索条仍是 hidden",
  );
  const handler = sources[scriptSrcs.indexOf("07-events.js")] ?? "";
  const guard = handler.slice(handler.indexOf('$("chat-search-toggle").addEventListener')).slice(0, 1600);
  assert(
    /closest\("\[popover\]"\)/.test(guard) && /openPopover\(/.test(guard) && /isSurfaceOpen\(host\)/.test(guard),
    "chat-search-toggle 处理器不再经原语展开它所在的弹层菜单：命令面板触发时搜索框看不见，击键会掉进待发消息",
  );
  // 判据必须是「实际看得见吗」而不是裸 toggle：搜索条无 hidden 类但宿主菜单收起时，
  // toggle 会把它“关掉”，然后击键照旧掉进 #prompt、裸 Enter 发给 agent。
  assert(
    /hiddenByAncestor|effectivelyHidden/.test(guard) && !/classList\.toggle\("hidden"\)/.test(guard),
    "chat-search-toggle 退回了裸 toggle：宿主菜单收起时会把搜索条反向关掉，关键词会被当任务发给 agent",
  );
}
const historyCalls = invokeArgs.filter(({ cmd }) => cmd === "conversation_list");
assert(historyCalls.some(({ args }) => args?.processId === "d|smoke"), "历史查询未带主线 process_id");
assert(historyCalls.some(({ args }) => args?.processId === "p|bg"), "历史查询未带并行线 process_id");
// R-247:排队顺序不参与；doing/fixing 无显式取得线按 D-354 归默认线，open 队首无标记。
// D-360:此处尚未渲染任何线路(collaborationLines 为空)——这正是「引擎没在跑」的形态,
// 一条线都没有就没有任何「被取得」事实可言,doing 也不例外。三种解码前提的完整覆盖
// 在下面并行线路那段(取得线离线/默认线持有/一条线都没有)。
{
  const active = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]');
  assert(active?.classList.contains("agent-active"), "doing 条目 R-001 未标记 agent-active(在做高亮丢失)");
  assert(
    !active?.querySelector(".doc-claim-fact"),
    "没有任何线在运行时,doing 条目不得被标成被取得(D-360:按状态一刀切的推断已废除)",
  );
  const next = document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]');
  assert(!next.classList.contains("agent-active"), "open 条目不该被标成在做");
  assert(!next.classList.contains("doc-claim-fact"), "没有 collaboration_snapshot claim 的队首不应出现被取得标记");
}
// D-362:文档页行内三列对齐的结构保证——可选徽标一律在标题之后成簇,不得插在
// 优先级前面。像素位置在这个环境里量不了,但「优先级之前只允许固定宽度元素」
// 这条结构不变量正是对齐的充要条件,行行成立则三列必然对齐。
{
  const optional = ["doc-claim-fact", "complexity-meter", "blocked-badge", "clarify-badge"];
  const rows = document.querySelectorAll("#documents-req-list .doc-row, #documents-defect-list .doc-row");
  assert(rows.length > 0, "文档页列表应有行可检");
  for (const row of rows) {
    const kids = [...row.children];
    const priAt = kids.findIndex((n) => n.classList.contains("pri-badge"));
    if (priAt < 0) continue;
    const early = kids.slice(0, priAt).filter((n) => optional.some((cls) => n.classList.contains(cls)));
    assert(
      early.length === 0,
      `可选徽标不得排在优先级之前(会把三列推歪):${early.map((n) => n.className).join(",")}`,
    );
    const flagBox = kids.find((n) => n.classList.contains("doc-flags"));
    if (flagBox) {
      assert(kids[kids.length - 1] === flagBox, "doc-flags 必须是行内最后一个元素(排在标题之后)");
    }
  }
}
// ---------- 侧栏「各线当前在做」焦点卡片:每条线路一组 ----------
// 完整列表连同筛选、排序、分组、批量、测试记录全部搬进单页视图;焦点区只显示
// 每条在线路自己的当前条目,不把后台线路的 claimed_by 或运行证据投影到主线。
{
  const cards = document.querySelectorAll("#focus-body .focus-card");
  assert(cards.length === 1, `当前夹具只有主线有焦点时应有一张卡片,实得 ${cards.length}`);
  assert(cards[0]?.dataset.docId === "R-001", `焦点卡片指错条目:${cards[0]?.dataset.docId}`);
  const lineFocuses = document.querySelectorAll("#focus-body .line-focus");
  assert(lineFocuses.length === 2, `两条线路应各有一个焦点区,实得 ${lineFocuses.length}`);
  const backgroundFocus = [...lineFocuses].find((node) => node.dataset.processId === "p|bg");
  assert(backgroundFocus?.querySelector(".line-focus-empty"), "没有 claimed_by 的后台线路必须显示自己的空态,不能借用主线焦点");
  assert(listText("focus-body").includes("后台会话"), "焦点区没有显示后台线路身份");
  const focusText = listText("focus-body");
  // UI-0926 #4 精简卡:卡面只留编号/状态/标题/批次/优先级。
  for (const needle of ["R-001", "冒烟需求", "doing", "批次 3/11", "P1"]) {
    assert(focusText.includes(needle), `焦点卡片缺少「${needle}」:${focusText.slice(0, 160)}`);
  }
  assert(document.querySelector("#focus-body .batch-meter"), "焦点卡片缺批次进度格");
  const card = cards[0];
  for (const gone of [".doc-field", ".doc-actions", ".focus-source"]) {
    assert(!card?.querySelector(gone), `精简焦点卡不该再常驻 ${gone}(字段段落/底部按钮/依据行已收进 tooltip 与 ⋯ 菜单)`);
  }
  assert(
    !card?.querySelectorAll("button").some((button) => button.textContent.includes("在完整列表中查看")),
    "精简焦点卡不该再有「在完整列表中查看」按钮(整卡就是入口)",
  );
  // 原护栏「没有只读字段 = 取活时看不到信息」换判据:信息没删,挪进了标题按钮的 tooltip。
  const open = card?.querySelector(".focus-open");
  assert(open, "焦点卡缺少整卡点击目标 .focus-open");
  assert(
    open?.getAttribute("aria-label")?.includes("R-001") && open.getAttribute("aria-label").includes("打开详情"),
    `.focus-open 的读屏名称应含编号与「打开详情」:${open?.getAttribute("aria-label")}`,
  );
  for (const needle of ["复杂度", "被依赖 1", "点击查看详情"]) {
    assert(open?.title.includes(needle), `焦点卡 tooltip 缺少「${needle}」(信息被一起删掉了):${open?.title}`);
  }
  // 焦点依据(D-207 三修的对外可见面):凭运行证据还是凭取活序,必须说出来——tooltip 写字 + 边框线型。
  assert(open?.title.includes("取活顺序推断"), `无运行证据时焦点依据应说明是推断:${open?.title}`);
  assert(card?.classList.contains("src-order"), `无运行证据时焦点卡应标 src-order(虚线=推断):${card?.className}`);
  // 侧栏不再承载完整列表 / 筛选 / 排序 / 分组 / 测试记录 —— 这些 id 从 index.html 里整体消失。
  for (const gone of ["req-list", "defect-list", "tests-section", "req-filter-row", "defect-filter-row",
    "req-sort", "req-group-toggle", "req-priority-filter", "req-status-filter", "req-tag-filter"]) {
    assert(!byId.has(gone), `侧栏残留完整列表控件 #${gone}(侧栏应只显示当前在做的单条)`);
  }
  // 测试记录搬进单页:#test-list 必须在 #documents-tests 里(harness 的 DOM 是按 id 扁平造的,
  // 祖先链断言天然不成立,这里改用 index.html 的静态包含关系)。
  const testsBlock = html.slice(html.indexOf('id="documents-tests"'), html.indexOf('id="documents-dep-view"'));
  assert(testsBlock.includes('id="test-list"'), "测试记录列表不在单页 #documents-tests 内(仍挂在侧栏)");
  assert(listText("test-list").includes("冒烟测试"), "测试记录搬家后没渲染出桩数据");
  assert(!document.querySelector("#focus-body .focus-next"), "焦点区不应渲染前端推断的下一个");
  // 待办计数补回被删列表的信息量。
  assert(/\d/.test(listText("focus-backlog")), "焦点区未给出待办计数");
}
// 线路取得事实:同一项目快照里同时存在主线与分支线条目时,两张卡片必须按
// claimed_by 分开,且分支线卡片说明依据是「取得线」而不是主线的运行/顺序推断。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    ...savedFocusDocs,
    requirements: [
      docEntry("R-MAIN", "主线条目", "doing"),
      docEntry("R-BG", "后台线路条目", "doing", { claimed_by: "kanzei/thread-smoke" }),
    ],
    defects: [],
  };
  await sandbox.refreshDocs();
  const focusBlocks = [...document.querySelectorAll("#focus-body .line-focus")];
  const mainBlock = focusBlocks.find((node) => node.dataset.processId === "d|smoke");
  const backgroundBlock = focusBlocks.find((node) => node.dataset.processId === "p|bg");
  assert(mainBlock?.querySelector('[data-doc-id="R-MAIN"]'), "主线焦点没有从未取得条目中选出主线条目");
  assert(backgroundBlock?.querySelector('[data-doc-id="R-BG"]'), "后台线路没有按 claimed_by 选出自己的条目");
  assert(
    backgroundBlock?.querySelector(".focus-open")?.title.includes("取得线")
      && backgroundBlock.querySelector(".focus-card")?.classList.contains("src-claim"),
    "后台线路焦点没有说明它来自线路取得事实(tooltip「依据: 取得线」+ src-claim)",
  );
  assert(!mainBlock?.querySelector('[data-doc-id="R-BG"]'), "后台线路条目串到了主线焦点卡片");
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// 待办计数的加法树:总数 = Σ(可执行 + 阻塞)。原来这里只断言「有个数字」,
// 于是三个数字用三种分母(只需求 / 只缺陷 / 需求∪缺陷)一路绿着——用户读成
// 「22 条需求其中 22 条阻塞」才发现。这条不变式失守就是那个歧义复发。
{
  const savedBacklogDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    ...savedBacklogDocs,
    requirements: [
      docEntry("R-101", "可执行需求", "todo"),
      docEntry("R-102", "阻塞需求", "todo", { blocked: true, block_reasons: ["等用户拍板"] }),
      docEntry("R-103", "已关闭需求", "done", { closed: true }),
    ],
    defects: [
      docEntry("D-101", "阻塞缺陷", "open", { blocked: true, block_reasons: ["等依赖"] }),
      docEntry("D-102", "已修缺陷", "fixed", { closed: true }),
    ],
  };
  // 刷新入口必须是 renderDocsSnapshot——#focus-backlog 只挂在它上面,
  // renderDocuments 不碰焦点面板(踩过:读到的是上一次渲染的 DOM,断言恒真)。
  sandbox.renderDocsSnapshot(payloads.docs_snapshot);
  const cell = (sel) => document.querySelector(`#focus-backlog ${sel} .backlog-num`)?.textContent ?? "";
  const total = cell(".backlog-total .backlog-stat.total");
  const reqReady = cell('[data-kind="req"] .backlog-stat.workable');
  const reqBlocked = cell('[data-kind="req"] .backlog-stat.blocked');
  const defReady = cell('[data-kind="defect"] .backlog-stat.workable');
  const defBlocked = cell('[data-kind="defect"] .backlog-stat.blocked');
  assert(reqReady === "1" && reqBlocked === "1", `需求应为 可执行1/阻塞1,实得 ${reqReady}/${reqBlocked}`);
  assert(defReady === "0" && defBlocked === "1", `缺陷应为 可执行0/阻塞1,实得 ${defReady}/${defBlocked}`);
  assert(total === "3", `已关闭条目不得计入总数,应为 3 实得 ${total}`);
  assert(
    Number(total) === Number(reqReady) + Number(reqBlocked) + Number(defReady) + Number(defBlocked),
    `加法树失守:${total} ≠ ${reqReady}+${reqBlocked}+${defReady}+${defBlocked}`,
  );
  // D-332:非法 lifecycle 引擎不取活,前端也不能悄悄算进总数——单列一行点名。
  payloads.docs_snapshot = {
    ...payloads.docs_snapshot,
    requirements: [...payloads.docs_snapshot.requirements, docEntry("R-109", "僵尸状态", "zombie")],
  };
  sandbox.renderDocsSnapshot(payloads.docs_snapshot);
  assert(
    cell(".backlog-total .backlog-stat.total") === "3",
    "非法 lifecycle 条目不得计入待办总数(引擎 backlog_status 同样跳过)",
  );
  assert(
    cell(".backlog-row.invalid .backlog-stat.invalid") === "1",
    "非法 lifecycle 条目必须单列「状态异常」行点名,不能静默丢弃",
  );
  payloads.docs_snapshot = savedBacklogDocs;
  sandbox.renderDocsSnapshot(savedBacklogDocs);
}
// 焦点卡片的状态流转:取活时要能直接切状态,这条链路不能因为卡片精简而断掉。
// UI-0926 #4 起状态流转从常驻按钮挪进「⋯」菜单(00-surface openMenu,弹层唯一写法)——
// 行为变化:切状态多点一次。菜单要经得住 3 秒一次的 process_list 轮询(内容没变就不重建焦点区)。
{
  const pagesNs = esmModuleCache.get("12-docs-pages.js")?.namespace;
  const shellNs = esmModuleCache.get("03-shell.js")?.namespace;
  const focusCardOf = (id) => document.querySelector(`#focus-body .focus-card[data-doc-id="${id}"]`);
  const card = focusCardOf("R-001");
  const more = card?.querySelector(".focus-more");
  assert(more, "焦点卡片缺少「⋯」状态流转入口(取活链路断了)");
  assert(more?.getAttribute("aria-haspopup") === "menu" && more.getAttribute("aria-label")?.includes("更多操作"), "「⋯」缺 aria-haspopup=menu 或读屏名称");
  more?.click();
  const handle = pagesNs?.focusMenuHandle;
  assert(handle && !handle.closed && handle.el?._popoverOpen && handle.el.classList.contains("k-menu"), "点「⋯」没有经 openMenu 打开状态菜单");
  assert(more?.getAttribute("aria-expanded") === "true", "状态菜单打开后「⋯」的 aria-expanded 未置 true");
  const items = handle?.el?.querySelectorAll('[role="menuitem"]') ?? [];
  assert(items.length === 1 && items[0].textContent.includes("done"), `状态菜单项应与 nextStatuses 一一对应:${items.map((item) => item.textContent).join(",")}`);
  // 轮询重绘:同一份内容不重建焦点区——卡片节点不换,菜单不被冲掉。
  sandbox.renderProcesses(structuredClone(shellNs.processItems));
  await flush();
  assert(focusCardOf("R-001") === card, "内容没变的 process_list 轮询重建了焦点卡(tooltip/菜单会被冲掉)");
  assert(!handle?.closed, "process_list 轮询把开着的状态菜单冲掉了");
  const before = invokeArgs.filter(({ cmd }) => cmd === "docs_update").length;
  items[0]?.click();
  await flush();
  const updates = invokeArgs.filter(({ cmd }) => cmd === "docs_update");
  assert(
    updates.length > before && updates.at(-1)?.args?.status === "done" && updates.at(-1)?.args?.id === "R-001",
    `状态菜单项没有提交对应的 docs_update:${JSON.stringify(updates.at(-1)?.args)}`,
  );
  assert(handle?.closed, "选中状态菜单项后菜单未收起");
  // 点菜单外(pointerdown 捕获阶段)收起。
  focusCardOf("R-001")?.querySelector(".focus-more")?.click();
  const again = pagesNs?.focusMenuHandle;
  assert(again && !again.closed, "再次点「⋯」未重新打开状态菜单");
  document.dispatchEvent({ type: "pointerdown", target: byId.get("prompt"), preventDefault() {}, stopPropagation() {} });
  assert(again?.closed, "点状态菜单外没有收起菜单");
  // 内容真的变了(标题改了)才重建;重建前先收起锚在旧卡片上的菜单。
  focusCardOf("R-001")?.querySelector(".focus-more")?.click();
  const stale = pagesNs?.focusMenuHandle;
  const renamed = structuredClone(payloads.docs_snapshot);
  renamed.requirements[0].title = "冒烟需求(改名)";
  sandbox.renderFocusPanel(renamed);
  assert(focusCardOf("R-001") && focusCardOf("R-001") !== card, "条目标题变了焦点卡却没重建(签名漏了字段)");
  assert(stale?.closed, "焦点区重建时没有收起锚在旧卡片上的状态菜单");
  sandbox.renderFocusPanel(payloads.docs_snapshot);
}
// 焦点空态:队列清空时说破,并给出去完整列表的入口(不留空壳、不编)。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    ...savedFocusDocs,
    requirements: [docEntry("R-001", "已完成需求", "done", { closed: true })],
    defects: [docEntry("D-001", "已修缺陷", "fixed", { closed: true })],
  };
  await sandbox.refreshDocs();
  assert(!document.querySelector("#focus-body .focus-card"), "全部关闭时不该还有焦点卡片");
  assert(listText("focus-body").includes("当前没有在做的条目"), `焦点空态未说破:${listText("focus-body")}`);
  assert(!document.querySelector("#focus-body .focus-next"), "焦点区不应保留下一个推断空壳");
  // UI-0926 #4:全局空态只说一次(末尾一行),每条线路下只剩一行,不再各自重复全局原因。
  const globalEmpty = document.querySelectorAll("#focus-body .focus-empty-global");
  assert(globalEmpty.length === 1 && globalEmpty[0].textContent.includes("当前没有在做的条目"), `全局空态应恰好一行:${globalEmpty.length}`);
  for (const lineEmpty of document.querySelectorAll("#focus-body .line-focus-empty")) {
    assert(!lineEmpty.textContent.includes("条可执行待取活") && !lineEmpty.textContent.includes("队列已清空"), `线路空态重复了全局原因:${lineEmpty.textContent}`);
    assert(!lineEmpty.querySelector(".dim"), "线路空态应只占一行(不再有第二行原因)");
  }
  const emptyButton = document.querySelector("#focus-body .focus-empty-global button");
  assert(emptyButton, "焦点空态缺少「查看完整列表」入口");
  emptyButton.click();
  await flush();
  assert(byId.get("view-documents").classList.contains("active"), "焦点空态的入口没能切到单页视图");
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// 侧栏标题栏的「打开完整列表」按钮:切视图 + 走 refreshDocs。
{
  byId.get("view-documents").classList.remove("active");
  const before = invokeLog.filter((cmd) => cmd === "docs_snapshot").length;
  byId.get("focus-open-documents").click();
  await flush();
  assert(byId.get("view-documents").classList.contains("active"), "#focus-open-documents 未激活单页视图");
  assert(
    invokeLog.filter((cmd) => cmd === "docs_snapshot").length > before,
    "#focus-open-documents 未触发 refreshDocs",
  );
}
// 每条线路内部仍保持单焦点语义:active 是该线路取活序第一个可执行 doing/fixing,
// 不是把同一条线路的多个历史 doing/fixing 全部画成当前在做。
// 多条 doing/fixing 只是"已取未动"的历史状态,只有取活序第一条才是 agent 正在推的。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    requirements: [docEntry("R-001", "第一条 doing", "doing", {}), docEntry("R-002", "第二条 doing", "doing", {})],
    defects: [docEntry("D-001", "可开工缺陷", "open", {})],
  };
  await sandbox.refreshDocs();
  const firstDoing = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]');
  const secondDoing = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]');
  assert(firstDoing?.classList.contains("agent-active"), "取活序第一条 doing 应标 agent-active(当前正在做)");
  assert(!secondDoing.classList.contains("agent-active"), "第二条 doing 只是已取未动,不该标 agent-active(active 是单条)");
  assert(!document.querySelector(".agent-next"), "删除下一个推断后不应产生 agent-next 标记");
  // WIP=1(2026-08-10 定调):两个队列共用同一个槽位,整个快照里被标「正在做」的只能有一条。
  assert(
    document.querySelectorAll("#documents-req-list .agent-active, #documents-defect-list .agent-active").length === 1,
    "两条 doing 同时在场时「正在做」应仍是单条(需求与缺陷共用一个槽,不是每队各一个)",
  );
  assert(document.querySelectorAll("#focus-body .focus-card").length === 1, "侧栏焦点卡片必须是单条");
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// WIP=1 跨队列:defect-first 下,非阻塞 fixing 缺陷占走那唯一的槽,需求侧的 doing 不再算「在做」。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  const priority = byId.get("work-priority-select");
  const savedPriority = priority.value;
  payloads.docs_snapshot = {
    requirements: [docEntry("R-A", "需求侧 doing", "doing", {}), docEntry("R-B", "待办需求", "todo", {})],
    defects: [docEntry("D-A", "缺陷侧 fixing", "fixing", {})],
  };
  priority.value = "defect-first";
  await sandbox.refreshDocs();
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-A"]')?.classList.contains("agent-active"),
    "defect-first 下,fixing 缺陷应占走唯一的可执行槽",
  );
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-A"]')?.classList.contains("agent-active"),
    "两队共用一个槽:缺陷占了槽,需求侧的 doing 不该同时被标「正在做」",
  );
  assert(!document.querySelector("#focus-body .focus-next"), "焦点区不应渲染第二个推断指针");
  priority.value = savedPriority;
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// D-207 补:blocked doing 不计入运行焦点。R-157 类阻塞 doing 曾被标成
// 「agent 正在做这一条」,而 §1.1 阻塞项不进 WIP、取活会跳过它——渲染必须与
// 取活一致:保留 blocked 标记但不标 agent-active,且 next 不被它挡住。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    requirements: [docEntry("R-001", "阻塞的 doing", "doing", { blocked: true })],
    defects: [docEntry("D-001", "可开工缺陷", "open", {})],
  };
  await sandbox.refreshDocs();
  const blockedDoing = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]');
  assert(blockedDoing?.classList.contains("blocked"), "阻塞 doing 应保留 blocked 标记(阻塞展示不受影响)");
  assert(!blockedDoing.classList.contains("agent-active"), "阻塞 doing 不该标 agent-active(运行焦点只标可执行条目)");
  assert(!document.querySelector(".agent-next"), "阻塞队列场景也不应产生下一个推断标记");
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// D-219 验收②:构造「2 个阻塞 doing + 可做 todo」场景——阻塞 doing 全部不计
// WIP、不占运行焦点。阻塞项只保留 blocked 展示，不生成排队推断标记。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    requirements: [
      docEntry("R-001", "阻塞的 doing", "doing", { blocked: true }),
      docEntry("R-002", "另一个阻塞 doing", "doing", { blocked: true }),
      docEntry("R-003", "可开工待办", "todo", {}),
    ],
    defects: [],
  };
  await sandbox.refreshDocs();
  const activeCount = document.querySelectorAll(
    "#documents-req-list .doc-item.agent-active, #documents-defect-list .doc-item.agent-active"
  ).length;
  assert(activeCount === 0, "两个阻塞 doing 都不应标 agent-active(阻塞项不进 WIP 不占焦点),实际 {activeCount}");
  const blockedAll = document.querySelectorAll('#documents-req-list .doc-item[data-doc-id="R-001"], #documents-req-list .doc-item[data-doc-id="R-002"]');
  assert(blockedAll.length === 2 && [...blockedAll].every((el) => el.classList.contains("blocked")), "阻塞 doing 应保留 blocked 标记");
  assert(!document.querySelector(".agent-next"), "阻塞队列场景也不应产生下一个推断标记");
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// D-207 三修:运行事实优先——纯文件状态推断会把挂着 fixing 的旧缺陷标成「正在做」,
// 而 agent 实际在推别的条目(用户实测:指着缺陷,实做 R-117)。req/defect 的 update
// 结果与批次提交都带条目 ID,运行证据一到就覆盖推断;新一轮开跑降级为上轮遗留。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    requirements: [docEntry("R-001", "实际在做的需求", "doing", {})],
    defects: [docEntry("D-001", "挂着 fixing 的旧缺陷", "fixing", {})],
  };
  await sandbox.refreshDocs();
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]')?.classList.contains("agent-active"),
    "无运行证据时应按取活序推断(defect-first 指 fixing 缺陷)",
  );
  handlers.get("kz:tool-end")({ payload: { id: "F1", name: "req", ok: true, preview: "updated: R-001 [doing] 批次推进", display: null, sessionId: "sess-smoke" } });
  await flush();
  await sandbox.refreshDocs();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]')?.classList.contains("agent-active"),
    "运行证据(updated: R-001)未覆盖状态推断——「在做」指针仍指错条目",
  );
  // 焦点卡片必须同步说出依据变了:D-207 三修的对外可见面就在这句话上(UI-0926 #4 起写在
  // 标题 tooltip 里,边框线型同步:runtime 实线)。
  const evidenceCard = () => document.querySelector('#focus-body .focus-card[data-doc-id="R-001"]');
  assert(
    evidenceCard()?.querySelector(".focus-open")?.title.includes("本轮运行证据") && evidenceCard().classList.contains("src-runtime"),
    `运行证据命中后焦点卡片仍说是推断:${evidenceCard()?.className} / ${evidenceCard()?.querySelector(".focus-open")?.title}`,
  );
  assert(
    !document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]')?.classList.contains("agent-active"),
    "运行证据生效后,挂着 fixing 的旧缺陷不该再标「正在做」",
  );
  // 新一轮 run 开跑(kz:turn step 1):上一轮证据**降级为上轮遗留**而不是清空——
  // 一轮前半段(勘察/写码/测试)不产生 tracker/提交事件,清空就是每轮开头一段
  // 「未取得条目」空窗;上轮条目仍是最好的猜测,新证据到达时自然覆盖。
  handlers.get("kz:turn")({ payload: { step: 1, maxSteps: 0, sessionId: "sess-smoke" } });
  await sandbox.refreshDocs();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]')?.classList.contains("agent-active"),
    "新 run 开跑后上轮证据应保留(降级),「在做」指针不该跳回推断",
  );
  assert(
    evidenceCard()?.querySelector(".focus-open")?.title.includes("上轮运行证据") && evidenceCard().classList.contains("src-runtime-stale"),
    `轮开始后焦点依据应标注为上轮遗留:${evidenceCard()?.className} / ${evidenceCard()?.querySelector(".focus-open")?.title}`,
  );
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// D-166:引用跳转此前只认当前可见节点,已归档/被折叠的目标一律静默失败。
const archiveToggle = document.querySelector("#documents-req-list .doc-archive-toggle");
assert(archiveToggle, "归档入口未渲染");
assert(!document.querySelector("#documents-req-list .doc-archive-list .archived-entry"), "归档条目不应在快照时提前加载");
assert(archiveToggle.getAttribute("aria-expanded") === "false", "归档区应默认折叠");
archiveToggle.click();
await flush();
const archivedRow = document.querySelector("#documents-req-list .doc-archive-list .archived-entry");
assert(archivedRow?.dataset.docId === "R-000", "按需加载后归档条目未挂 data-doc-id,引用跳转必然落空");
assert(typeof sandbox.jumpToEntry === "function", "jumpToEntry 未定义(引用跳转入口丢失)");
await sandbox.jumpToEntry("R-000");
assert(
  !archivedRow.parentElement.classList.contains("hidden"),
  "跳转到归档条目时未掀开归档折叠区",
);
assert(archivedRow.classList.contains("ref-highlight"), "跳转后未高亮目标条目");
await sandbox.jumpToEntry("R-999");
assert(
  listText("toast").includes("R-999"),
  `跳转到不存在的条目时应给出提示而不是静默失败,实得 toast: "${listText("toast")}"`,
);

// 完整列表搬进单页后,「侧栏不该有编辑表单/批量选择」这组断言换了落点:侧栏根本没有
// 列表了(上面 byId 反向断言 + ui-a11y-smoke.mjs 已守住),再照搬到 #documents-req-list
// 就会变成断言「单页不该能编辑」——正好把 R-123 的能力判反。这里改为守住单页确实有这些能力,
// 见下面 reqEditor / .doc-pick 两组;此处只保留「侧栏焦点区不承载列表能力」的正面判据。
assert(!document.querySelector("#focus-body .doc-edit"), "侧栏焦点卡片渲染了字段编辑表单(编辑只在独立文档页)");
assert(!document.querySelector("#focus-body .doc-pick"), "侧栏焦点卡片出现批量选择框(批量操作应只在文档页)");
assert(
  !document.querySelectorAll("#documents-req-list .doc-item").some((n) => n.draggable),
  "分组锁状态下条目不应可拖(解锁后才设置 draggable)",
);
// D-211 修复链路:解锁 → 锁提示消失 → draggable=true → 拖拽 → reorder 落库。
// 终判据收紧到 action==="reorder":拖拽是唯一能改取活顺序的入口,只数 docs_update 次数
// 的话,任何顺手多发的 docs_update(改状态/改字段)都能让它假通过。
{
  const reqListEl = document.querySelector("#documents-req-list");
  const hint = reqListEl.querySelector(".drag-hint");
  assert(hint, "默认分组视图未渲染锁提示");
  const unlockBtn = [...hint.querySelectorAll("button")].find((b) => b.textContent.includes("解锁"));
  assert(unlockBtn, "锁提示缺少一键解锁按钮(D-210 能力丢失)");
  unlockBtn.click();
  await flush();
  assert(!document.querySelector("#documents-req-list .drag-hint"), "解锁后锁提示未消失");
  const items = [...document.querySelectorAll("#documents-req-list .doc-item[data-doc-id]")];
  assert(items.length >= 2, `解锁后需求条目不足(无法验证拖拽落库): ${items.length}`);
  assert(items.every((n) => n.draggable), "解锁后条目未设置 draggable(D-211:解锁了却拖不动)");
  const reorderCount = () =>
    invokeArgs.filter(({ cmd, args }) => cmd === "docs_update" && args?.action === "reorder").length;
  const before = reorderCount();
  const [a, b] = items;
  a.dispatchEvent({ type: "dragstart", dataTransfer: { effectAllowed: "", setData() {} } });
  b.dispatchEvent({ type: "dragover", clientY: 0, preventDefault() {} });
  a.dispatchEvent({ type: "dragend" });
  await flush();
  assert(reorderCount() > before, `拖拽未提交 action=reorder 的 docs_update(唯一能改取活顺序的入口断了)`);
}
// D-207 验收③:优先级语义 UI 明示——priority 只是背景信息,不参与取活(用户定调),
// 避免满屏 P0~P3 徽章让人按优先级猜取活序。
{
  const priFilter = document.querySelector("#documents-priority-filter");
  assert(priFilter?.getAttribute("title").includes("仅参考"), `优先级筛选未明示"仅参考,不影响取活": "${priFilter?.getAttribute("title")}"`);
  const badge = document.querySelector("#documents-req-list .pri-badge");
  assert(badge?.title.includes("仅参考"), `优先级徽章未明示"仅参考,不影响取活": "${badge?.title}"`);
}
// D-205 验收③:带「待澄清」复现的缺陷可辨识——用户能一眼看到哪些条目等他补话,
// 不会把"待澄清"当真实复现拿去开工。
{
  const clarifyBadge = document.querySelector("#documents-defect-list .clarify-badge");
  assert(clarifyBadge, "带「待澄清」复现的缺陷未渲染待澄清徽标(D-205)");
  assert(clarifyBadge.title.includes("待澄清"), `待澄清徽标未带具体问题提示: "${clarifyBadge.title}"`);
  assert(!document.querySelector("#documents-req-list .clarify-badge"), "需求列表误渲染待澄清徽标(仅缺陷快记有此形态)");
}
// 列表搬走之后,取活时要看的信息落到了侧栏焦点卡片上——断言跟着搬,不能删:
// 「信息被一起删掉」这条护栏必须留着。UI-0926 #4 起卡面不再铺字段段落,信息在标题 tooltip 里
// (依据/复杂度/依赖/最新进展/阻塞),点一下直达展开的详情。
{
  const sidebarOpen = document.querySelector("#focus-body .focus-card .focus-open");
  assert(
    sidebarOpen?.title.includes("依据") && sidebarOpen.title.includes("复杂度"),
    `侧栏焦点卡片既无编辑表单也无只读信息,信息被一起删掉了:${sidebarOpen?.title}`,
  );
  // 最新一段进展(R-282:|| 切段只露首段)进 tooltip,不整段倒出来。
  const progressCard = sandbox.buildFocusCard(docEntry("R-P01", "进展样例", "doing", { fields: [["进展", "2026-09-20 批3 合入||2026-09-18 批2 合入||批1 合入"]] }), "req", "order");
  const progressTip = progressCard.querySelector(".focus-open")?.title ?? "";
  assert(progressTip.includes("进展: 2026-09-20 批3 合入") && !progressTip.includes("批2"), `焦点卡 tooltip 的进展应只取最新一段:${progressTip}`);
}
// 状态流转留在侧栏:取活时要能直接切(⋯ 菜单,见上面的菜单用例)。
assert(document.querySelector("#focus-body .focus-card .focus-more"), "侧栏焦点卡片缺少状态流转入口(取活链路断了)");

const reqEditor = document.querySelector("#documents-req-list .doc-edit");
assert(reqEditor?.querySelector("input") && reqEditor?.querySelector("button"), "独立文档页未提供标题/字段编辑控件");
// D-164:曾经只给 aria-label,渲染成一片无标题输入框,改哪格全靠猜;长字段用单行 input 还会把值截没。
const reqEditRows = reqEditor.querySelectorAll(".doc-edit-row");
assert(reqEditRows.length >= 3, `编辑表单未按字段分行(应有 标题/备注/验收),实得 ${reqEditRows.length}`);
assert(
  reqEditRows.every((row) => (row.querySelector(".doc-edit-key")?.textContent ?? "").trim()),
  "编辑表单存在没有可见字段名的输入框",
);
assert(reqEditor.querySelector("textarea"), "长字段未升级为多行文本域,值会被单行输入框截断");
// D-165:同一份字段不能在一个详情里显示两遍。UI-0926 #4 起详情只读优先:字段只读视图(.tf)与
// 编辑表单(.doc-edit)同时存在于 DOM,但任何时刻恰好一个可见——默认只读,点「编辑」后互换。
{
  const reqDetail = document.querySelector("#documents-req-list .doc-detail");
  const readView = reqDetail?.querySelector(".doc-fields-read");
  const shownCount = () => [readView, reqEditor].filter((node) => node && !node.classList.contains("hidden")).length;
  assert(readView?.querySelector(".tf") && !readView.classList.contains("hidden") && reqEditor.classList.contains("hidden"),
    "详情默认应是字段只读视图(.tf 可见、编辑表单隐藏)");
  assert(shownCount() === 1, `只读视图与编辑表单应恰好一个可见(D-165 同一份字段不显示两遍),实得 ${shownCount()}`);
  reqDetail.querySelector(".doc-edit-toggle")?.click();
  assert(!reqEditor.classList.contains("hidden") && readView.classList.contains("hidden") && shownCount() === 1,
    "点「编辑」后应只剩编辑表单可见(只读视图同时藏起)");
  reqDetail.querySelector(".doc-edit-toggle")?.click();
  assert(shownCount() === 1 && reqEditor.classList.contains("hidden"), "取消编辑后应回到只读视图");
}
reqEditor.querySelector("button").click();
await flush();
assert(invokeLog.includes("docs_update"), "独立文档页编辑未调用 docs_update");
const defectEditor = document.querySelector("#documents-defect-list .doc-edit");
assert(defectEditor?.querySelector("input") && defectEditor?.querySelector("button"), "独立文档页缺陷未提供编辑控件");
defectEditor.querySelector("button").click();
await flush();
assert(invokeLog.filter((cmd) => cmd === "docs_update").length >= 2, "独立文档页缺陷编辑未调用 docs_update");

// R-092:缺陷自动审查必须是一个真实按钮调用，不是静态报告链接或展示壳。
const reviewButton = byId.get("defect-review");
reviewButton.click();
assert(listText("defect-review-status").includes("正在审查缺陷"), "点击审查按钮后未立即反馈处理中状态");
await flush();
assert(invokeLog.includes("defect_review"), "缺陷自动审查按钮未调用后端 defect_review");
assert(listText("defect-review-status").includes("审查完成"), "缺陷自动审查成功后未反馈完成状态");
assert(!byId.get("viewer-overlay").classList.contains("hidden"), "缺陷自动审查报告未在应用内打开");
assert(listText("viewer-body").includes("D-001") && listText("viewer-body").includes("可复核证据"), "审查报告查看器未渲染后端结果");
assert(byId.get("viewer-external").classList.contains("hidden"), "运行时审查报告不应显示无效的外部文件按钮");
assert(!reviewButton.disabled, "缺陷审查完成后按钮仍处于禁用状态");
byId.get("viewer-close").click();

// 批量操作:选中后操作条出现,应用后逐条提交。
const pick = document.querySelector("#documents-req-list .doc-pick");
assert(pick, "独立文档页未提供批量选择框");
assert(byId.get("documents-batch-bar").classList.contains("hidden"), "未选中任何条目时批量操作条不应出现");
pick.checked = true;
pick._listeners.change?.forEach((fn) => fn({ target: pick }));
assert(!byId.get("documents-batch-bar").classList.contains("hidden"), "选中条目后批量操作条未出现");
assert(listText("documents-batch-count").trim().length > 0, "批量操作条未显示已选数量");
const beforeBatch = invokeLog.filter((cmd) => cmd === "docs_update").length;
byId.get("documents-batch-tag").value = "前端";
byId.get("documents-batch-apply")._listeners.click?.forEach((fn) => fn({}));
await flush();
assert(
  invokeLog.filter((cmd) => cmd === "docs_update").length > beforeBatch,
  "批量应用未提交 docs_update",
);
// D-256:批量操作进行中切项目,不得有任何一条写进新项目。
// 2026-08-11 用户拍板语义:按认领项目做完——整批继续写旧项目,循环内不重读 currentProject;
// 循环结束后若 currentProject 已变,提示「这批改动落在 <旧项目>」。
// 桩把第一条 docs_update 挂在闸门上,applyBatch 停在第一次 await 处,此刻把 currentProject
// 换成项目乙;放行后剩余条目必须仍以认领时的旧项目为 projectDir,且 toast 明说落地项目。
{
  const reqPicks = [...document.querySelectorAll("#documents-req-list .doc-pick")];
  assert(reqPicks.length >= 2, "前置失败:D-256 用例需要至少 2 条需求条目");
  const claimedProject = vm.runInContext("currentProject", sandbox);
  const batchStart = invokeArgs.length;
  reqPicks.forEach((el) => { el.checked = true; el._listeners.change?.forEach((fn) => fn({ target: el })); });
  assert(
    vm.runInContext("batchSelection.size", sandbox) >= 2,
    "前置失败:批量选中集未达到 2 条",
  );
  let releaseBatch;
  invokeGates.set("docs_update", new Promise((resolve) => { releaseBatch = resolve; }));
  byId.get("documents-batch-tag").value = "流程";
  byId.get("documents-batch-apply")._listeners.click?.forEach((fn) => fn({}));
  await settle();
  // 循环已挂在第一条 docs_update 的 await 上——此刻切项目。旧实现从这里起会把新项目
  // 写进 projectDir,正是 D-256 描述的错写(新项目的同号条目被真改状态/改标签)。
  vm.runInContext(`currentProject = ${JSON.stringify("C:/smoke/project-b")}`, sandbox);
  releaseBatch();
  invokeGates.delete("docs_update");
  await flush();
  const batchUpdateCalls = invokeArgs.slice(batchStart).filter(({ cmd, args }) => cmd === "docs_update" && args?.action === "update");
  assert(batchUpdateCalls.length >= 2, "D-256:批量循环未按选中条目逐条提交 docs_update");
  const projectDirs = new Set(batchUpdateCalls.map(({ args }) => args?.projectDir));
  assert(
    [...projectDirs].every((dir) => dir === claimedProject),
    `D-256:批量中途切项目后,有 docs_update 的 projectDir 指向非认领项目(${[...projectDirs].join(",")})`,
  );
  assert(
    listText("toast").includes(claimedProject),
    `D-256:批量期间切走项目,结束后未提示这批改动落在认领项目(toast="${listText("toast")}")`,
  );
  // 复位:清空选中、把 currentProject 改回,不污染后续用例。
  reqPicks.forEach((el) => { el.checked = false; el._listeners.change?.forEach((fn) => fn({ target: el })); });
  vm.runInContext(`currentProject = ${JSON.stringify(claimedProject)}`, sandbox);
  await flush();
}
// 对照:两个队列同时可见,共用同一套**显示口径**——全字段中性化(D-244)。
byId.get("documents-tab-both")._listeners.click?.forEach((fn) => fn({}));
await flush();
assert(
  !byId.get("documents-req-list").classList.contains("hidden")
    && !byId.get("documents-defect-list").classList.contains("hidden"),
  "对照模式未同时显示需求与缺陷两个队列",
);
// D-244:对照页是只读对照视图,blocked 控件必须置灰;模拟 change 也不得改任何一队的
// 持久化筛选(此前这里真的会跨队列写并落盘)。桩数据都不带阻塞理由,若筛选生效两边都会
// 清空——断言两边都还在,证明中性化兜住了。
const reqBefore = document.querySelectorAll("#documents-req-list .doc-item").length;
const defectBefore = document.querySelectorAll("#documents-defect-list .doc-item").length;
assert(reqBefore > 0 && defectBefore > 0, "对照模式下两个队列应先都有条目");
const blockedFilter = byId.get("documents-blocked-filter");
assert(blockedFilter.disabled, "对照页阻塞控件应置灰(D-244:只读对照视图)");
blockedFilter.value = "blocked";
blockedFilter._listeners.change?.forEach((fn) => fn({ target: blockedFilter }));
await flush();
assert(
  document.querySelectorAll("#documents-req-list .doc-item").length === reqBefore
    && document.querySelectorAll("#documents-defect-list .doc-item").length === defectBefore,
  "对照模式下改阻塞筛选把列表筛空了:中性化没生效(对照页必须只读,D-244)",
);
blockedFilter.value = "all";
blockedFilter._listeners.change?.forEach((fn) => fn({ target: blockedFilter }));
await flush();

// ---------- R-111 依赖视图:可做/被阻塞分层,点击条目高亮依赖链 ----------
const depToggle = byId.get("documents-dep-toggle");
assert(depToggle, "文档页缺少依赖视图切换按钮");
depToggle.click();
await flush();
assert(
  !byId.get("documents-dep-view").classList.contains("hidden"),
  "点击依赖视图按钮后面板未显示",
);
assert(
  byId.get("documents-req-list").classList.contains("hidden")
    && byId.get("documents-defect-list").classList.contains("hidden"),
  "依赖视图打开时普通列表未隐藏",
);
// 桩依赖:R-002 依赖 R-001 → R-002 处于被阻塞层,R-001 处于可做层。
const depEntries = [...document.querySelectorAll("#documents-dep-view .dep-entry")];
assert(depEntries.length >= 2, "依赖视图未渲染分层条目");
const r001 = depEntries.find((n) => n.dataset.docId === "R-001");
const r002 = depEntries.find((n) => n.dataset.docId === "R-002");
assert(r001 && r002, "依赖视图缺少 R-001/R-002");
assert(
  r001.closest(".dep-layer") !== r002.closest(".dep-layer"),
  "R-001 与 R-002 应分属不同层(依赖关系未分层)",
);
// 点击 R-002 应高亮它自己和依赖链上的 R-001,并压暗无关条目。
r002.click();
await flush();
assert(r002.classList.contains("dep-lit"), "点击后目标条目未高亮");
assert(r001.classList.contains("dep-lit"), "依赖链上游未高亮");
const unrelated = depEntries.find((n) => n.dataset.docId === "D-001");
if (unrelated) assert(unrelated.classList.contains("dep-dim"), "无关条目未压暗");
// D-750:normal docs_snapshot 只带归档计数。依赖 R-111 已归档终态时,后端同快照的 block_reasons 为空;
// 前端必须消费该权威判定,不能因 active requirements/defects 里没有 R-111 就误放进 blocked 层。
const savedArchiveDependencyDocs = structuredClone(payloads.docs_snapshot);
payloads.docs_snapshot = {
  ...savedArchiveDependencyDocs,
  requirements: [docEntry("R-900", "依赖归档终态 R-111", "todo", {
    dependencies: ["R-111"],
    blocked: false,
    block_reasons: [],
  })],
  defects: [],
  archived: { ...savedArchiveDependencyDocs.archived, req: 1 },
};
sandbox.renderDocsSnapshot(payloads.docs_snapshot);
assert(
  document.querySelector('#documents-dep-view .dep-entry[data-doc-id="R-900"]'),
  "归档依赖 fixture 未渲染活跃需求 R-900",
);
assert(
  document.querySelector("#documents-dep-view .dep-layer-head.ready")?.textContent.includes("(1)")
    && document.querySelector("#documents-dep-view .dep-layer-head.blocked")?.textContent.includes("(0)"),
  "依赖 R-111 已归档且引擎 block_reasons 为空时,R-900 应在可做层而非被阻塞层",
);
// D-750 环例外:引擎对环上条目只报「循环依赖: …」、不报「未完成依赖」,
// 依赖视图仍必须把两条互相依赖的条目都放进被阻塞层,与引擎 blocked=true 同判。
const cycleReason = (path) => `循环依赖: ${path} —— 环上没有条目能先完成,必须断掉其中一条边(把不成立的依赖移入 refs)`;
payloads.docs_snapshot = {
  ...savedArchiveDependencyDocs,
  requirements: [
    docEntry("R-910", "环成员 A", "todo", {
      dependencies: ["R-911"], dependents: ["R-911"],
      blocked: true, block_reasons: [cycleReason("R-910 → R-911 → R-910")],
    }),
    docEntry("R-911", "环成员 B", "todo", {
      dependencies: ["R-910"], dependents: ["R-910"],
      blocked: true, block_reasons: [cycleReason("R-911 → R-910 → R-911")],
    }),
  ],
  defects: [],
};
sandbox.renderDocsSnapshot(payloads.docs_snapshot);
assert(
  document.querySelector("#documents-dep-view .dep-layer-head.blocked")?.textContent.includes("(2)")
    && document.querySelector("#documents-dep-view .dep-layer-head.ready")?.textContent.includes("(0)"),
  "互相依赖的 R-910/R-911 只带循环依赖理由时应都在被阻塞层(与引擎 blocked=true 一致),不能判为可做",
);
payloads.docs_snapshot = savedArchiveDependencyDocs;
sandbox.renderDocsSnapshot(savedArchiveDependencyDocs);
depToggle.click();
await flush();
assert(byId.get("documents-dep-view").classList.contains("hidden"), "再次点击依赖视图按钮后面板未隐藏");

// ---------- 单页视图补齐侧栏退休掉的能力:排序 / 复杂度筛选 / 测试记录 ----------
// 完整列表整体搬进单页后,侧栏原有的排序、复杂度筛选、测试记录都必须在这里找得到,
// 否则搬家等于把能力删了。
{
  byId.get("documents-tab-req").click();
  await flush();
  const reorderCount = () =>
    invokeArgs.filter(({ cmd, args }) => cmd === "docs_update" && args?.action === "reorder").length;
  const setSort = async (value) => {
    const sort = byId.get("documents-sort");
    sort.value = value;
    sort._listeners.change?.forEach((fn) => fn({ target: sort }));
    await flush();
  };

  // ① 排序 ≠ 拖拽:排序只改显示口径,只有手动排序下的拖拽才写回文件、改变取活顺序。
  // 三重冗余(常显说明 / 锁提示点名 / draggable 关掉)缺一条,用户就会以为"按优先级排一下,
  // agent 就会按优先级取活"。
  assert(listText("documents-sort-note").includes("拖拽"), `排序说明未点明拖拽才写回文件:"${listText("documents-sort-note")}"`);
  const reorderBeforeSort = reorderCount();
  await setSort("priority");
  const sortHint = document.querySelector("#documents-req-list .drag-hint");
  assert(sortHint, "非手动排序下未渲染拖拽锁提示(静默禁用 = D-210 老毛病)");
  assert(sortHint.textContent.includes("排序=优先级"), `锁提示未点名到具体条件:"${sortHint.textContent}"`);
  assert(
    document.querySelectorAll("#documents-req-list .doc-item[data-doc-id]").every((n) => !n.draggable),
    "非手动排序下条目仍可拖(拖出来的顺序会与文件顺序对不上)",
  );
  assert(
    reorderCount() === reorderBeforeSort,
    "改排序竟然提交了 action=reorder 的 docs_update:排序只该改显示,不该动取活顺序",
  );
  // ② 解锁后拖拽仍然真的能改取活顺序(能力没被上一条断言"锁死")。
  const unlock = [...sortHint.querySelectorAll("button")].find((b) => b.textContent.includes("解锁"));
  assert(unlock, "排序锁提示缺少一键解锁");
  unlock.click();
  await flush();
  assert(byId.get("documents-sort").value === "manual", "解锁后排序未切回手动");
  assert(!document.querySelector("#documents-req-list .drag-hint"), "解锁后锁提示未消失");
  const sortedItems = [...document.querySelectorAll("#documents-req-list .doc-item[data-doc-id]")];
  assert(sortedItems.every((n) => n.draggable), "解锁后条目仍拖不动");
  const beforeDrag = reorderCount();
  sortedItems[0].dispatchEvent({ type: "dragstart", dataTransfer: { effectAllowed: "", setData() {} } });
  sortedItems[1].dispatchEvent({ type: "dragover", clientY: 0, preventDefault() {} });
  sortedItems[0].dispatchEvent({ type: "dragend" });
  await flush();
  assert(reorderCount() > beforeDrag, "解锁后拖拽仍未提交 action=reorder(承诺与能力脱节)");

  // ③ 复杂度筛选补齐(侧栏退休前有这一档,单页必须接上,含按项目落盘)。
  const complexity = byId.get("documents-complexity-filter");
  assert(complexity && !complexity.disabled, "需求页缺少可用的复杂度筛选");
  complexity.value = "大";
  complexity._listeners.change?.forEach((fn) => fn({ target: complexity }));
  await flush();
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "复杂度筛选没生效(R-001 是「中」,筛「大」时不该还在)",
  );
  assert(document.querySelector("#documents-req-list .doc-filtered-empty"), "复杂度筛空后未说破");
  const complexityKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
  assert(
    JSON.parse(storage.get(complexityKey)).docReq.complexity === "大",
    "复杂度筛选未按项目落盘(重启后回「全部」)",
  );
  complexity.value = "all";
  complexity._listeners.change?.forEach((fn) => fn({ target: complexity }));
  await flush();
  assert(document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'), "复杂度筛选清回「全部」后条目没回来");

  // ④ 测试记录已从侧栏搬进单页:切过去要真的显示,且对它无意义的控件要**置灰说破**,
  // 不做静默无效(D-210/D-211 的教训)。
  byId.get("documents-tab-tests").click();
  await flush();
  assert(!byId.get("documents-tests").classList.contains("hidden"), "测试记录标签页打不开");
  assert(byId.get("documents-req-list").classList.contains("hidden"), "测试记录页仍显示需求列表");
  assert(byId.get("documents-defect-list").classList.contains("hidden"), "测试记录页仍显示缺陷列表");
  assert(byId.get("documents-batch-bar").classList.contains("hidden"), "测试记录页不该出现批量操作条");
  assert(byId.get("documents-dep-toggle").disabled === true, "测试记录页的依赖视图按钮应置灰(禁用要说破)");
  assert(byId.get("documents-tab-tests").className.includes("primary"), "测试记录标签未标为当前页");
  assert(listText("test-list").includes("冒烟测试"), "测试记录页没渲染出测试数据");
  // 切回来必须完全可逆:dependencyViewOpen 这类标志不能被 tests 页顺手清掉。
  byId.get("documents-tab-req").click();
  await flush();
  assert(!byId.get("documents-req-list").classList.contains("hidden"), "切回需求页后列表没回来");
  assert(byId.get("documents-tests").classList.contains("hidden"), "切回需求页后测试记录未隐藏");
  assert(byId.get("documents-dep-toggle").disabled === false, "切回需求页后依赖视图按钮仍被禁用");
  assert(byId.get("documents-status-filter").disabled === false, "切回需求页后状态筛选仍被禁用");
  // ⑤ #tests-refresh 搬家后按 id 绑定的监听必须还在(09-sessions.js:565 绑的是这个 id)。
  const testsBefore = invokeLog.filter((cmd) => cmd === "test_runs_snapshot").length;
  byId.get("tests-refresh").click();
  await flush();
  assert(
    invokeLog.filter((cmd) => cmd === "test_runs_snapshot").length > testsBefore,
    "测试记录刷新按钮搬进单页后失效了(按 id 绑定的监听断了)",
  );

  // ⑥ 引用跳转必须先把单页视图切过去:条目现在只存在于 #view-documents 里,
  // 视图没激活时祖先是 display:none,scrollIntoView 无效 —— 真机上就是 D-166 的「点了没反应」。
  // 冒烟 harness 的 offsetParent 恒真,这条只能靠显式断言守。
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  assert(!byId.get("view-documents").classList.contains("active"), "前置失败:未离开单页视图");
  sandbox.jumpToEntry("R-002");
  await flush();
  assert(byId.get("view-documents").classList.contains("active"), "跳转到单页里的条目时没有先切视图(点了没反应,D-166 复发)");
}

// ---------- 对照(both)标签页:显示上不带筛选,但绝不许清掉用户的筛选 ----------
// 对照页只提供「全部状态」(两队状态机不同),复杂度/排序是需求专有口径 —— 这三档在
// 对照页必须**按中性渲染**,否则界面写着「全部状态 / 全部复杂度 / 手动」而列表仍按上次
// 设的条件在筛,条目凭空少了(D-169 那类「以为数据丢了」)。
// 但"中性"只能是**显示口径**:此前这里真的把 documentFilters.req/defect 写成 all 并落盘,
// 用户在需求页设好 status=doing + 复杂度=大,只是切去对照页瞄一眼,回来筛选就永久没了、
// 重启也回不来 —— R-115「筛选按项目持久化」在这条路径上的直接回归。两头一起钉死:
// 对照页渲染确实不带筛选,切回去筛选原样还在(控件 + 内存 + 落盘)。
{
  byId.get("documents-tab-req").click();
  await flush();
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  await setDocFilter("documents-status-filter", "doing"); // R-002 是 todo → 被藏
  await setDocFilter("documents-complexity-filter", "大"); // R-001 是「中」 → 被藏
  await setDocFilter("documents-sort", "priority");
  assert(!document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'), "前置失败:复杂度筛选没生效");
  assert(!document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'), "前置失败:状态筛选没生效");
  const filtersStoreKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
  assert(filtersStoreKey, "前置失败:筛选没有落盘(R-115 的持久化本身断了)");

  byId.get("documents-tab-both").click();
  await flush();
  // ① 显示口径:三档下拉复位,列表真的按不带筛选渲染。
  assert(byId.get("documents-status-filter").value === "all", "对照页状态下拉未复位");
  assert(byId.get("documents-complexity-filter").value === "all", "对照页复杂度下拉未复位");
  assert(byId.get("documents-sort").value === "manual", "对照页排序下拉未复位");
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "对照页只复位了下拉显示值、列表仍按复杂度在筛:界面写着「全部复杂度」而 R-001(中)被藏(D-169:以为数据丢了)",
  );
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    "对照页只复位了下拉显示值、列表仍按状态在筛:界面写着「全部状态」而 R-002(todo)被藏",
  );
  const bothDragHint = document.querySelector("#documents-req-list .drag-hint");
  assert(
    !(bothDragHint?.textContent ?? "").includes("排序"),
    `对照页下拉写着「手动」而锁提示仍点名排序:"${bothDragHint?.textContent}"`,
  );
  // ② 底层筛选不许被清掉:落盘必须原样保留,否则重启后也回不来。
  assert(
    (() => {
      const saved = JSON.parse(storage.get(filtersStoreKey) ?? "{}");
      return saved.docReq?.status === "doing" && saved.docReq?.complexity === "大" && saved.docReq?.sort === "priority";
    })(),
    `去对照页瞄一眼就把用户的筛选清掉并落盘了(R-115 回归:切回来没了,重启也回不来):${storage.get(filtersStoreKey)}`,
  );
  // ③ 切回需求页:控件、内存、列表三处都得是用户原来的那套。
  byId.get("documents-tab-req").click();
  await flush();
  assert(byId.get("documents-status-filter").value === "doing", "切回需求页,状态筛选没了(对照页把它清掉了)");
  assert(byId.get("documents-complexity-filter").value === "大", "切回需求页,复杂度筛选没了(对照页把它清掉了)");
  assert(byId.get("documents-sort").value === "priority", "切回需求页,排序没了(对照页把它清掉了)");
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "切回需求页后复杂度筛选只剩下拉显示值、列表没在筛(状态与显示脱节)",
  );
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    "切回需求页后状态筛选只剩下拉显示值、列表没在筛(状态与显示脱节)",
  );

  // 缺陷侧同理:对照页只提供「全部状态」,缺陷队列的 status 同样是"显示中性、状态保留"。
  byId.get("documents-tab-defect").click();
  await flush();
  await setDocFilter("documents-status-filter", "fixing"); // D-001 是 open → 被藏
  assert(!document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]'), "前置失败:缺陷状态筛选没生效");
  byId.get("documents-tab-both").click();
  await flush();
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]'),
    "对照页缺陷列表仍按看不见的 status=fixing 在筛(显示口径没做中性)",
  );
  assert(
    JSON.parse(storage.get(filtersStoreKey) ?? "{}").docDefect?.status === "fixing",
    `对照页把缺陷队列的 status 清掉并落盘了:${storage.get(filtersStoreKey)}`,
  );
  byId.get("documents-tab-defect").click();
  await flush();
  assert(byId.get("documents-status-filter").value === "fixing", "切回缺陷页,状态筛选没了");
  assert(
    !document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]'),
    "切回缺陷页后状态筛选只剩下拉显示值、列表没在筛",
  );

  // 收尾:筛选现在会真的留下来,后续用例假定列表完整 —— 走用户路径调回「全部」。
  await setDocFilter("documents-status-filter", "all");
  byId.get("documents-tab-req").click();
  await flush();
  await setDocFilter("documents-status-filter", "all");
  await setDocFilter("documents-complexity-filter", "all");
  await setDocFilter("documents-sort", "manual");
  assert(
    document.querySelectorAll("#documents-req-list .doc-item").length >= 2
      && document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]'),
    "收尾失败:筛选没调回全部,后续用例会连带假失败",
  );
}

// ---------- 对照页不得改动任何队列的持久化标签筛选(跨队列写回) ----------
// 标签曾经是唯一一个绕开"中性副本"的字段:syncDocumentFilters 里有一段
// `for (const kind of docFilterTargets())` 把下拉的生效值写给每一个队列并落盘。
// 实测两种坏法,都是"去对照页瞄一眼就改掉用户状态":
//   缺陷页设「后端」→ 点对照 → 缺陷队列的标签被清成「全部」并落盘,切回去筛选永久没了;
//   需求页设「核心」→ 点对照 → 「核心」被写进缺陷队列并落盘,用户从没在缺陷页设过,
//   缺陷列表却永久少了一批。
// 定调:对照页是只读的对照视图,标签与 status/complexity/sort 同一套机制——只改显示。
// 唯一允许写回的例外是 D-169 的"值失效"回落,且只能作用于该标签所属的那一队(见 ④)。
{
  const savedTagDocs = structuredClone(payloads.docs_snapshot);
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  const filtersStoreKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
  assert(filtersStoreKey, "前置失败:筛选没有落盘(R-115 的持久化本身断了)");
  const liveTags = () => JSON.parse(vm.runInContext(
    "JSON.stringify({ req: documentFilters.req.tag, defect: documentFilters.defect.tag })",
    sandbox,
  ));
  const savedTags = () => {
    const saved = JSON.parse(storage.get(filtersStoreKey) ?? "{}");
    return { req: saved.docReq?.tag, defect: saved.docDefect?.tag };
  };
  // 两队各带各的标签:只有这样才分得清"清掉了"与"被写成了对面那一支"。
  payloads.docs_snapshot = {
    ...savedTagDocs,
    requirements: [
      docEntry("R-001", "核心标签需求", "doing", { fields: [["标签", "核心"]] }),
      docEntry("R-002", "前端标签需求", "todo", { fields: [["标签", "前端"]] }),
    ],
    defects: [
      docEntry("D-001", "后端标签缺陷", "open", { fields: [["标签", "后端"]] }),
      docEntry("D-002", "前端标签缺陷", "open", { fields: [["标签", "前端"]] }),
    ],
  };
  await sandbox.refreshDocs();
  await flush();

  // ① 缺陷页设好的标签,去对照页瞄一眼再回来必须原样还在(此前被清成「全部」并落盘)。
  byId.get("documents-tab-defect").click();
  await flush();
  await setDocFilter("documents-tag-filter", "后端");
  assert(
    !document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-002"]'),
    "前置失败:缺陷标签筛选没生效",
  );
  byId.get("documents-tab-both").click();
  await flush();
  assert(
    liveTags().defect === "后端",
    `对照页把缺陷队列的标签清掉了:${JSON.stringify(liveTags())}`,
  );
  assert(
    savedTags().defect === "后端",
    `对照页把缺陷队列的标签清掉并落盘了(切回去没了,重启也回不来):${storage.get(filtersStoreKey)}`,
  );
  // 显示口径:渲染真的不带标签筛选,下拉跟着显示「全部标签」——两者必须一致(D-211)。
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-002"]'),
    "对照页缺陷列表仍按看不见的标签在筛(显示口径没做中性,D-169:以为条目掉了)",
  );
  assert(byId.get("documents-tag-filter").value === "all", "对照页标签下拉未复位");
  // 渲染按中性走而控件还能调 = 调了不生效,而且一调就把值写进两个队列并落盘(D-210 静默无效)。
  assert(
    byId.get("documents-tag-filter").disabled === true,
    "对照页标签渲染按中性走,控件却没置灰:调了不生效,还会把值写进两队并落盘",
  );
  byId.get("documents-tab-defect").click();
  await flush();
  assert(byId.get("documents-tag-filter").value === "后端", "切回缺陷页,标签筛选没了");
  assert(
    !document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-002"]'),
    "切回缺陷页后标签只剩下拉显示值、列表没在筛(状态与显示脱节)",
  );

  // ② 需求页的标签绝不许被写进缺陷队列:用户从没在缺陷页设过,缺陷列表却少一批。
  await setDocFilter("documents-tag-filter", "all");
  byId.get("documents-tab-req").click();
  await flush();
  await setDocFilter("documents-tag-filter", "核心");
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    "前置失败:需求标签筛选没生效",
  );
  byId.get("documents-tab-both").click();
  await flush();
  assert(
    liveTags().defect === "all",
    `对照页把需求页的标签写进了缺陷队列(缺陷队列被一个用户没设过的条件筛掉一批):${JSON.stringify(liveTags())}`,
  );
  assert(
    savedTags().defect === "all",
    `对照页把需求页的标签写进缺陷队列并落盘了:${storage.get(filtersStoreKey)}`,
  );
  assert(
    liveTags().req === "核心" && savedTags().req === "核心",
    `对照页把需求队列自己的标签也改了:${JSON.stringify(liveTags())} / ${storage.get(filtersStoreKey)}`,
  );

  // ③ D-169 的"值失效"回落必须还在,但只作用于该标签所属的那一队。
  // 缺陷页设「后端」,随后该标签在缺陷队列里消失(改名/清空/换项目):下拉只能回落成
  // 「全部」,状态与落盘必须跟着回落,否则列表被一个看不见的条件筛空;而需求队列的
  // 「核心」还在、还有效,一个字节都不许动。
  byId.get("documents-tab-defect").click();
  await flush();
  await setDocFilter("documents-tag-filter", "后端");
  payloads.docs_snapshot = {
    ...savedTagDocs,
    requirements: [
      docEntry("R-001", "核心标签需求", "doing", { fields: [["标签", "核心"]] }),
      docEntry("R-002", "前端标签需求", "todo", { fields: [["标签", "前端"]] }),
    ],
    defects: [docEntry("D-002", "前端标签缺陷", "open", { fields: [["标签", "前端"]] })],
  };
  await sandbox.refreshDocs();
  await flush();
  assert(
    liveTags().defect === "all" && savedTags().defect === "all",
    `标签在缺陷队列里已不存在,筛选状态却没跟着回落(列表被看不见的条件筛空,D-169):${JSON.stringify(liveTags())} / ${storage.get(filtersStoreKey)}`,
  );
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-002"]'),
    "标签回落后缺陷列表仍是空的(条目看起来凭空掉了)",
  );
  assert(
    liveTags().req === "核心" && savedTags().req === "核心",
    `缺陷队列的标签回落顺手改掉了需求队列的标签(值失效纠正跨队列写了):${JSON.stringify(liveTags())} / ${storage.get(filtersStoreKey)}`,
  );

  // 收尾:标签调回「全部」并还原快照,否则后续用例看到的是被筛过的列表。
  byId.get("documents-tab-req").click();
  await flush();
  await setDocFilter("documents-tag-filter", "all");
  payloads.docs_snapshot = savedTagDocs;
  await sandbox.refreshDocs();
  await flush();
  assert(
    liveTags().req === "all" && liveTags().defect === "all",
    `收尾失败:标签没调回全部(${JSON.stringify(liveTags())}),后续用例会连带假失败`,
  );
  assert(
    document.querySelectorAll("#documents-req-list .doc-item").length >= 2
      && document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]'),
    "收尾失败:快照没还原,后续用例会连带假失败",
  );
}

// ---------- 对照页全字段中性化(D-244):priority/blocked 与 status/tag/complexity/sort 同机制 ----------
// 此前对照页上只剩 priority/blocked 两个控件仍是"真实筛选":调一次就跨队列写进
// documentFilters.req/defect 并落盘(实测 before={"req":"all","defect":"all"}
// after={"req":"P0","defect":"P0"} saved=同)——用户去对照页调一下优先级,另一队的
// 持久化筛选就被覆盖。定调:对照页是只读的对照视图,priority/blocked 同样走中性副本,
// 只改显示、不动任何一队的底层状态;控件置灰,切回单队列页时原值原样回来。
{
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  const filtersStoreKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
  assert(filtersStoreKey, "前置失败:筛选没有落盘(R-115 的持久化本身断了)");
  const savedFilters = () => JSON.parse(storage.get(filtersStoreKey) ?? "{}");

  // ① req 页设 priority=P0 + blocked=blocked(两条需求默认 P1 且不阻塞 → 列表被筛空)。
  byId.get("documents-tab-req").click();
  await flush();
  await setDocFilter("documents-priority-filter", "P0");
  await setDocFilter("documents-blocked-filter", "blocked");
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "前置失败:req 页 priority/blocked 筛选没生效",
  );
  const before = savedFilters();

  // ② 切对照页:priority/blocked 控件置灰并显示 all(与 status/tag 同机制),列表不再被筛空。
  byId.get("documents-tab-both").click();
  await flush();
  assert(
    byId.get("documents-priority-filter").disabled,
    "对照页优先级控件没有置灰(D-244:只读对照视图)",
  );
  assert(
    byId.get("documents-blocked-filter").disabled,
    "对照页阻塞控件没有置灰(D-244:只读对照视图)",
  );
  assert(
    byId.get("documents-priority-filter").value === "all",
    `对照页优先级控件应显示中性 all,实际 "${byId.get("documents-priority-filter").value}"`,
  );
  assert(
    byId.get("documents-blocked-filter").value === "all",
    `对照页阻塞控件应显示中性 all,实际 "${byId.get("documents-blocked-filter").value}"`,
  );
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "对照页仍按 priority=all 之外的条件在筛 R-001(优先级中性化没生效,界面写着全部却少条目)",
  );
  assert(
    !document.querySelector("#documents-req-list .doc-filtered-empty"),
    "对照页不应渲染「清除筛选」:全字段中性化后不可能被筛空(D-244)",
  );
  assert(
    !document.querySelector("#documents-req-list .drag-hint"),
    "对照页不应渲染锁提示:全字段中性化后没有锁定条件(D-244)",
  );

  // ③ 两队的持久化筛选原样保留,一个字节都不许被对照页改掉(内存 + localStorage)。
  const after = savedFilters();
  assert(
    before.docReq?.priority === "P0" && before.docReq?.blocked === "blocked"
      && after.docReq?.priority === "P0" && after.docReq?.blocked === "blocked",
    `对照页把 req 的持久化筛选改掉了:${JSON.stringify(after)}`,
  );
  assert(
    before.docDefect?.priority === after.docDefect?.priority
      && before.docDefect?.blocked === after.docDefect?.blocked,
    `对照页把 defect 的持久化筛选改掉了:${JSON.stringify(after)}`,
  );

  // ④ 切回 req 页:控件原值回来、列表仍按用户设定的筛(对照页只改显示,没动底层)。
  byId.get("documents-tab-req").click();
  await flush();
  assert(
    byId.get("documents-priority-filter").value === "P0"
      && byId.get("documents-blocked-filter").value === "blocked",
    "切回 req 页,priority/blocked 筛选没了(对照页把它清掉了)",
  );
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "切回 req 页后 priority/blocked 筛选只剩下拉显示值、列表没在筛(状态与显示脱节)",
  );

  // 收尾:走用户路径调回「全部」,不污染后续用例。
  await setDocFilter("documents-priority-filter", "all");
  await setDocFilter("documents-blocked-filter", "all");
  assert(
    document.querySelectorAll("#documents-req-list .doc-item").length >= 2,
    "收尾失败:筛选没调回全部,后续用例会连带假失败",
  );
}
// ③ 冻结对象护栏:idea/source/finding 三张列表拿的是**冻结的** NEUTRAL_DOC_FILTERS。
// 这两个按钮在它们身上渲染不出来(筛选分支只对 req/defect 生效 → 不可能"被筛空";
// 锁提示显式限定 kind),而且写回一律走 documentFilters[kind](这三类取不到就不写)。
// 两道保险都要在:机械钉住"根本没渲染",免得哪天筛选放开了顺手踩到冻结对象上抛异常。
{
  for (const listId of ["idea-list"]) {
    assert(
      !document.querySelector(`#${listId} .doc-filtered-empty`) && !document.querySelector(`#${listId} .drag-hint`),
      `#${listId} 渲染出了会写筛选状态的按钮,但它拿到的是冻结的 NEUTRAL_DOC_FILTERS`,
    );
  }
}

// ---------- D-414 研究工件可打开:↗ 必须渲染且点击真调后端 ----------
// 这段是本轮实测教训的机械化:D-413 交付「文献可点开」后用户点不开,根因是两处改动
// 互相抵消(开了编辑器就吞掉只读链接渲染),而六条冒烟全绿——因为夹具里 sources 是
// 空数组,整条渲染路径从没被走过。断言钉三件事:①有可打开字段的来源必须出 ↗;
// ②点击真的发起 webfetch_preview(不是死按钮);③代码域来源认 file:line 而非 URL。
{
  // 来源已经移入独立研究页；通用列表渲染器仍单独验证打开能力。
  const source_host = document.createElement("div");
  source_host.id = "source-test-list";
  document.body.appendChild(source_host);
  byId.set(source_host.id, source_host);
  sandbox.renderDocList(source_host, payloads.docs_snapshot.sources, "source");
  const openButtons = [...document.querySelectorAll("#source-test-list .doc-open-src")];
  assert(
    openButtons.length === 2,
    `来源列表应为两条可打开来源各渲染一个 ↗,实得 ${openButtons.length} 个`,
  );
  const before = invokeArgs.filter((c) => c.cmd === "webfetch_preview").length;
  openButtons[0].dispatchEvent({ type: "click", stopPropagation() {} });
  await flush();
  const after = invokeArgs.filter((c) => c.cmd === "webfetch_preview");
  assert(
    after.length === before + 1,
    `点 ↗ 必须调 webfetch_preview 抓正文,实际调用数 ${before} → ${after.length}`,
  );
  assert(
    after.at(-1)?.args?.url === "https://arxiv.org/abs/2310.08560",
    `webfetch_preview 收到的 URL 不对: ${JSON.stringify(after.at(-1)?.args)}`,
  );
}

// ---------- R-221 B2 topic 工件:课题分组、报告读取与同名 ID 隔离 ----------
{
  const profile = byId.get("profile-select");
  const savedProfile = profile.value;
  const original_processes = payloads.process_list;
  payloads.process_list = [...original_processes, { id: "p|research", session_id: "sess-research", profile: "research", research_topic: "alpha-study", label: "Alpha 研究", running: false }];
  await vm.runInContext('switch_workspace("research")', sandbox);
  await flush();
  const researchActivity = document.querySelector('.activity-item[data-view="research"]');
  assert(researchActivity && !researchActivity.classList.contains("hidden"), "research 档未显示研究工作台入口");
  researchActivity.click();
  await flush();
  const topicSelect = byId.get("research-topic-select");
  assert(topicSelect && topicSelect.options.length === 2, `研究课题选择器应有两个 topic,实得 ${topicSelect?.options.length ?? 0}`);
  assert(topicSelect.value === "alpha-study", `默认应选择排序后的 alpha-study,实得 ${topicSelect.value}`);
  assert(document.querySelector('#research-cards .research-topic-group[data-topic="alpha-study"]'), "alpha topic 分组未渲染");
  assert(byId.get("research-runs-count").textContent === "2 条", "真实 research run 数量未渲染");
  vm.runInContext('show_research_page("writing")', sandbox);
  await flush();
  const latexTemplate = byId.get("research-latex-template");
  assert(latexTemplate && latexTemplate.options.length === 4, "LaTeX 模板选择器未加载四套内置模板");
  latexTemplate.value = "paper_with_figures";
  byId.get("research-latex-document-name").value = "paper";
  byId.get("research-latex-title").value = "Alpha 论文";
  byId.get("research-latex-create").click();
  await flush();
  assert(invokeArgs.some((call) => call.cmd === "research_latex_create" && call.args?.templateId === "paper_with_figures"), "模板新建未调用真实 research_latex_create");
  assert(byId.get("research-latex-status").textContent.includes("latex/"), "模板新建成功状态未返回项目路径");
  byId.get("research-latex-compile").click();
  await flush();
  assert(invokeArgs.some((call) => call.cmd === "research_latex_compile" && call.args?.documentName === "paper"), "PDF 编译未调用真实 research_latex_compile");
  assert(invokeArgs.some((call) => call.cmd === "research_latex_pdf"), "编译成功后未调用真实 research_latex_pdf 预览");
  assert(!byId.get("research-latex-pdf").hidden && byId.get("research-latex-pdf").src.includes("application/pdf"), "PDF 预览 iframe 未显示 data URL");
  byId.get("research-latex-figure-name").value = "result.png";
  byId.get("research-latex-figure-caption").value = "实验结果";
  byId.get("research-latex-figure-label").value = "fig:result";
  byId.get("research-latex-insert-figure").click();
  await flush();
  assert(invokeArgs.some((call) => call.cmd === "research_latex_insert_figure" && call.args?.figureName === "result.png"), "实验图表入口未调用真实 research_latex_insert_figure");
  assert(byId.get("research-latex-status").textContent.includes("../figures/result.png"), "图表引用成功状态未显示稳定相对路径");
  assert(byId.get("research-latex-history").childNodes.length === 1, "编译历史未渲染可回看的版本");
  const roadmapGraph = byId.get("research-roadmap-graph");
  assert(roadmapGraph.querySelectorAll(".research-roadmap-node").length === 3, "探索路线图节点未按 Markdown 真源渲染");
  assert(roadmapGraph.querySelectorAll(".research-roadmap-edge.edge-depends_on").length === 2, "depends_on 实线未完整投影");
  assert(roadmapGraph.querySelectorAll(".research-roadmap-edge.edge-supersedes").length === 0, "悬挂 supersedes 不应伪造可见边");
  assert(!byId.get("research-roadmap-diagnostics").hidden && byId.get("research-roadmap-diagnostics").textContent.includes("悬挂"), "悬挂关系诊断未在界面显式呈现");
  const firstProjection = vm.runInContext("JSON.stringify(researchRouteProjection())", sandbox);
  vm.runInContext("renderResearchRoadmap()", sandbox);
  assert(firstProjection === vm.runInContext("JSON.stringify(researchRouteProjection())", sandbox), "同一批探索文件重复投影结果不一致");
  roadmapGraph.querySelector('.research-roadmap-node[data-node-id="E-101"]').click();
  assert(!byId.get("research-exploration-detail").hidden && byId.get("research-exploration-detail-body").textContent.includes("Alpha 基线"), "点击路线图节点未进入探索详情");
  const detailBody = byId.get("research-exploration-detail-body");
  assert(detailBody.textContent.includes("基线假设") && detailBody.textContent.includes("支持") && detailBody.textContent.includes("继续扩大样本"), "探索详情未展示假设/结论/后续真实字段");
  assert(detailBody.querySelectorAll(".research-results-table tr").length === 2, "探索详情结果表未保留 Markdown 结果行");
  detailBody.querySelector(".research-result-open")?.click();
  assert(document.querySelector('#research-run-cards .research-run-card[data-result-id="E-101-01"]')?.classList.contains("is-selected"), "结果行未定位并高亮对应 run");
  assert(document.querySelector('#research-run-cards .research-run-card[data-result-id="E-101-01"] .research-run-chart polyline'), "run 指标事件未渲染曲线");
  assert(document.querySelector('#research-run-cards .research-run-card[data-result-id="E-101-01"] .research-run-terminal')?.textContent.includes("训练完成"), "run 终端 message 未进入回放");
  // UI-0926 #10:执行配置 JSON 拆成策略/类型 chip,不再原样拼进 meta 行。
  assert(!document.querySelector('#research-run-cards .research-run-card[data-result-id="E-101-01"] .research-run-meta')?.textContent.includes('{"kind"') && document.querySelector('#research-run-cards .research-run-card[data-result-id="E-101-01"] .research-run-meta .sv-chip')?.textContent === "managed", "研究运行卡仍拼接 execution_json 原文");
  const artifactLink = document.querySelector('#research-run-cards .research-run-card[data-result-id="E-101-01"] .research-artifact-link');
  assert(artifactLink, "run 未展示产物入口");
  artifactLink.click();
  await flush();
  assert(invokeArgs.some((call) => call.cmd === "file_preview" && call.args?.path.includes("figure.png")), "产物入口未调用真实 file_preview");


  assert(document.querySelector('#research-run-cards .research-run-card[data-result-id="E-101-01"] .research-run-drift.has-drift')?.textContent.includes("环境漂移"), "登记与快照不一致时未显示环境漂移");
  assert(document.querySelector('#research-run-cards .research-run-card[data-result-id="E-101-02"] .research-run-drift.no-drift')?.textContent.includes("环境声明一致"), "无漂移 run 未显示一致状态");
  assert(document.querySelector('#research-cards .research-card[data-doc-id="S-101"]')?.textContent.includes("Alpha 一手论文"), "alpha topic 来源未渲染");
  assert(!byId.get("research-plan-panel").hidden, "alpha topic 未展示研究计划面板");
  assert(byId.get("research-plan-status").textContent === "待批准", "研究计划初始状态未显示待批准");
  assert(byId.get("research-plan-tree").querySelectorAll("li").length === 2, "研究计划树节点未渲染完整");
  const approvePlan = byId.get("research-plan-approve");
  assert(!approvePlan.hidden, "待审批计划未显示批准按钮");
  approvePlan.click();
  await flush();
  assert(byId.get("research-plan-status").textContent === "已批准", "计划批准后状态未更新");
  const approveCalls = invokeArgs.filter((call) => call.cmd === "research_plan_approve");
  assert(approveCalls.at(-1)?.args?.topic === "alpha-study", "计划审批调用未携带 alpha topic");
  assert(document.querySelector('#research-cards .research-card[data-doc-id="S-101"]')?.textContent.includes("正文级"), "来源卡未展示正文级证据深度");
  assert(document.querySelector('#research-cards .research-card[data-doc-id="S-103"]')?.textContent.includes("摘要级"), "无 V 等级来源卡未展示摘要级证据深度");
  const arxivOpen = document.querySelector('#research-cards .research-card[data-doc-id="S-103"] .research-open');
  assert(arxivOpen, "研究来源卡缺少 arXiv 正文入口");
  arxivOpen.dispatchEvent({ type: "click", stopPropagation() {} });
  await flush();
  const arxivCalls = invokeArgs.filter((call) => call.cmd === "research_arxiv_preview");
  assert(arxivCalls.at(-1)?.args?.topic === "alpha-study", `arXiv 正文调用未携带 topic:${JSON.stringify(arxivCalls.at(-1)?.args)}`);
  assert(byId.get("viewer-body")?.textContent.includes("正文级"), "arXiv 返回的正文级标注未进入 viewer");
  assert(byId.get("research-report").textContent.includes("Alpha report"), "alpha topic 未读取对应 report");
  const longResearchReport = [
    "# Report head",
    ...Array.from({ length: 75 }, (_, index) => `Section ${index} [S-101]`),
    "Report tail [S-101]",
  ].join("\n\n");
  vm.runInContext(`renderResearchReport(${JSON.stringify(longResearchReport)})`, sandbox);
  const reportHost = byId.get("research-report");
  assert(Number(reportHost.dataset.reportWindowStart) > 0, "长报告首屏未启用尾部窗口");
  assert(reportHost.querySelector(".research-report-earlier"), "长报告缺少载入更早内容入口");
  assert(!reportHost.textContent.includes("Report head"), "长报告首屏意外渲染了最早内容");
  assert(vm.runInContext("loadEarlierResearchReport()", sandbox) === true, "长报告未能向上补齐窗口");
  assert(reportHost.textContent.includes("Report head"), "长报告补齐后未出现最早内容");
  assert(reportHost.textContent.includes("S-101"), "长报告窗口化后引用内容丢失");
  topicSelect.value = "beta-study";
  topicSelect.dispatchEvent({ type: "change", currentTarget: topicSelect });
  await flush();
  assert(document.querySelector('#research-cards .research-topic-group[data-topic="beta-study"]'), "beta topic 分组未渲染");
  assert(byId.get("research-roadmap").querySelectorAll(".research-roadmap-node").length === 0, "无探索 topic 不应残留上一个 topic 的节点");
  assert(byId.get("research-roadmap-graph").querySelector(".research-roadmap-empty"), "无探索 topic 未展示路线图空态");

  assert(document.querySelector('#research-cards .research-card[data-doc-id="S-101"]')?.textContent.includes("Beta 代码来源"), "相同 S-101 未切换到 beta topic 数据");
  assert(!document.querySelector('#research-cards .research-card')?.textContent.includes("Alpha 一手论文"), "切换 topic 后仍混入 alpha 来源");
  assert(byId.get("research-report").textContent.includes("Beta report"), "beta topic 未读取对应 report");
  const betaTopicReads = invokeArgs.filter(({ cmd }) => cmd === "docs_read");
  assert(betaTopicReads.at(-1)?.args?.topic === "beta-study", `beta report 读取未携带 topic:${JSON.stringify(betaTopicReads.at(-1)?.args)}`);
  topicSelect.value = "alpha-study";
  topicSelect.dispatchEvent({ type: "change", currentTarget: topicSelect });
  await flush();
  const filterType = byId.get("research-filter-type");
  for (const id of ["research-filter-query", "research-filter-type", "research-filter-level", "research-filter-year", "research-filter-sort"]) {
    const listeners = byId.get(id)?._listeners;
    const event = id === "research-filter-query" ? "input" : "change";
    assert((listeners?.[event] ?? []).length === 1, `${id} 在 topic 切换后重复注册了 ${event} 监听器`);
  }
  const filterLevel = byId.get("research-filter-level");
  const filterSort = byId.get("research-filter-sort");
  assert(filterType.options.length === 3, `类型筛选应有全部+文献+代码三项,实得 ${filterType.options.length}`);
  assert(filterLevel.options.length === 2 && filterLevel.options[1].value === "V2", "V 等级筛选未从当前课题来源生成");
  filterType.value = "代码域";
  filterType.dispatchEvent({ type: "change", currentTarget: filterType });
  await flush();
  assert(document.querySelectorAll('#research-cards .research-card[data-doc-id="S-102"]').length === 1, "类型筛选未保留代码来源");
  assert(!document.querySelector('#research-cards .research-card[data-doc-id="S-101"]'), "类型筛选未隐藏文献来源");
  filterType.value = "";
  filterType.dispatchEvent({ type: "change", currentTarget: filterType });
  filterLevel.value = "V2";
  filterLevel.dispatchEvent({ type: "change", currentTarget: filterLevel });
  await flush();
  assert(document.querySelectorAll('#research-cards .research-card[data-doc-id="S-101"]').length === 1 && !document.querySelector('#research-cards .research-card[data-doc-id="S-102"]'), "V 等级筛选未生效");
  filterLevel.value = "";
  filterLevel.dispatchEvent({ type: "change", currentTarget: filterLevel });
  filterSort.value = "year";
  filterSort.dispatchEvent({ type: "change", currentTarget: filterSort });
  await flush();
  assert(document.querySelector("#research-cards .research-card")?.dataset.docId === "S-101", "按年份排序未把较新来源置前");
  const backref = document.querySelector('#research-cards .research-card[data-doc-id="S-101"] .research-card-backrefs .ref-link');
  assert(backref?.textContent === "F-101", "来源卡缺少反查到 F-101 的链接");
  backref.click();
  await flush();
  assert(byId.get("research-tab-findings").classList.contains("active"), "来源反查没有切换到发现 tab");
  assert(document.querySelector('#research-cards .research-card[data-doc-id="F-101"]'), "来源反查没有定位到发现卡");
  byId.get("research-tab-sources").click();
  await flush();
  const copyButton = [...document.querySelectorAll('#research-cards .research-card[data-doc-id="S-101"] .research-card-actions button')]
    .find((button) => button.textContent === "复制 BibTeX");
  assert(copyButton, "来源卡缺少 BibTeX 复制按钮");
  copyButton.click();
  await flush();
  assert(copiedResearchCitation.includes("@misc{") && copiedResearchCitation.includes("Alpha Researcher"), "BibTeX 复制内容不完整");
  filterSort.value = "";
  filterSort.dispatchEvent({ type: "change", currentTarget: filterSort });
  await flush();
  await vm.runInContext('switch_workspace("dev")', sandbox);
  payloads.process_list = original_processes;
  await sandbox.refreshProcesses();
  profile.value = savedProfile;
  profile.dispatchEvent({ type: "change" });
  await flush();
}

// ---------- 筛选只能写给确实拥有该字段的队列 ----------
// documentFilters.defect 没有 complexity/sort 两个键。凭空写进去,锁提示的
// `key in reqFilterState` 就会把「复杂度=大」列进缺陷队列的锁,而 docDragEnabled 的
// 缺陷分支只看 status/priority/tag/blocked——提示说锁了、实际仍可拖(D-211 反向脱节)。
{
  byId.get("documents-tab-both").click();
  await flush();
  sandbox.applyDocFilter("complexity", "大");
  sandbox.applyDocFilter("sort", "priority");
  await flush();
  // documentFilters 是 const 词法声明,不会挂到 sandbox 全局上,只能在同一 context 里求值。
  const defectFilterKeys = vm.runInContext("Object.keys(documentFilters.defect).join(',')", sandbox).split(",");
  assert(
    !defectFilterKeys.includes("complexity"),
    `对照模式把「复杂度」写进了缺陷筛选状态:缺陷拖拽判断根本不看它,锁提示却会照列(D-211 反向脱节)。实得键:${defectFilterKeys.join(",")}`,
  );
  assert(
    !defectFilterKeys.includes("sort"),
    `对照模式把「排序」写进了缺陷筛选状态。实得键:${defectFilterKeys.join(",")}`,
  );
  byId.get("documents-tab-req").click();
  await flush();
  // 对照页不再清用户的筛选(见上一段),所以刚写进 req 的复杂度/排序会真的留下来:
  // 这里手工调回全部,否则后续用例的列表是被筛过的。
  sandbox.applyDocFilter("complexity", "all");
  sandbox.applyDocFilter("sort", "manual");
  await flush();
  assert(
    document.querySelectorAll("#documents-req-list .doc-item").length >= 2,
    "收尾失败:复杂度/排序没调回全部,后续用例会连带假失败",
  );
}

// ---------- 跨视图跳转的高亮必须活过随后的那次刷新(真机时序) ----------
// openDocumentsView() 触发的 refreshDocs() 是一次真实 IPC:`await invoke("docs_snapshot")`
// 之后才 renderDocsSnapshot。用 setTimeout(…, 0) 去赌"刷新已经落地",真机毫秒级 IPC 下
// 必然赌输——高亮打在旧节点上,紧接着 renderDocList 的 el.innerHTML = "" 把该节点连同
// scrollIntoView 的落点一并清掉:用户被切过去却看不出是哪一条。
// 默认桩是已 resolve 的 promise,微任务恒先于 setTimeout,顺序恰好反过来 = 假绿;
// 这里给 docs_snapshot 挂闸门,把真机顺序复现出来。
{
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  assert(!byId.get("view-documents").classList.contains("active"), "前置失败:未离开单页视图");
  assert(document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'), "前置失败:跳转前列表里没有 R-002");
  let openDocsSnapshot;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { openDocsSnapshot = resolve; }));
  sandbox.jumpToEntry("R-002");
  // 刷新还挂在闸门上,这一轮先把已排队的 setTimeout 跑掉 = 真机顺序。只跑一遍、不连锁:
  // 回调自己排的 1200ms 移除定时器不会被顺带冲掉,失败原因就只剩「高亮打在旧节点上」。
  // 排空带超时(drainTimersOnce):闸门段前若有人排了一次 refreshDocsSoon,它内部的
  // docs_snapshot 会撞上这道还没放开的闸门,无超时的 await 会让整个冒烟挂死而不是判红。
  await settle();
  for (const frame of rafQueue.splice(0)) frame();
  await drainTimersOnce("跨视图跳转闸门段");
  openDocsSnapshot();
  invokeGates.delete("docs_snapshot");
  // 只推进微任务、不动定时器:ref-highlight 的 1200ms 移除定时器不能在断言前被冲掉。
  for (let i = 0; i < 12; i += 1) await settle();
  assert(byId.get("view-documents").classList.contains("active"), "跳转到单页里的条目时没有先切视图(点了没反应,D-166 复发)");
  const freshJumpNode = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]');
  assert(freshJumpNode, "刷新后列表里找不到 R-002");
  assert(
    freshJumpNode?.classList.contains("ref-highlight"),
    "跨视图跳转的高亮没活过随后的 refreshDocs:重绘把带高亮的旧节点整个换掉了(真机 IPC 毫秒级,setTimeout(0) 必然先跑)",
  );
  await flush();
}

// ---------- 刷新失败不得留下悬挂高亮 ----------
// 11-docs-list.js 写着「不留一个会在将来某次无关刷新上突然亮起来的悬挂高亮」,但那句
// 只在 renderDocsSnapshot 真的跑到时成立。真机上 docs_snapshot 会因目录被删/文件锁/
// 解析失败而抛错:refreshDocs 走 catch → 不重绘 → pendingJumpId 一直挂着;之后任意一次
// 无关刷新(agent 触发的 refreshDocsSoon、或用户再进文档页)都会把它消费掉 ——
// 用户没点跳转,条目自己亮了。承诺与实现必须一致(D-211)。
{
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  assert(!byId.get("view-documents").classList.contains("active"), "前置失败:未离开单页视图");
  assert(document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'), "前置失败:跳转前列表里没有 R-002");
  // 注入的刷新失败会走 toastError:那正是被测的那条 catch,不判红。
  expectedPersistentError = "项目文档刷新失败";
  const hitsBefore = expectedPersistentHits;
  invokeFailures.set("docs_snapshot", "冒烟注入:目录被删/文件被锁/解析失败");
  sandbox.jumpToEntry("R-002");
  // 只推进微任务、不跑定时器:失败注入期间不能让 refreshDocsSoon 之类的定时器也撞上去
  // (它的 catch 走 console.error,会以另一种形态判红,掩盖真正要看的那条)。
  for (const frame of rafQueue.splice(0)) frame();
  await drainTimersOnce("切页失败注入的延迟加载");
  for (let i = 0; i < 12; i += 1) await settle();
  invokeFailures.delete("docs_snapshot");
  assert(expectedPersistentHits > hitsBefore, "前置失败:注入的 docs_snapshot 失败没有触发 refreshDocs 的 catch");
  expectedPersistentError = null;
  assert(
    vm.runInContext("pendingJumpId", sandbox) === null,
    "docs_snapshot 抛错后 pendingJumpId 还挂着:下一次无关刷新会把它兑现——用户没点跳转,条目自己亮了(悬挂高亮)",
  );
  // 无关刷新:一次成功的 refreshDocs 不得凭空点亮任何条目。
  await sandbox.refreshDocs();
  // 断言前只推微任务:1200ms 的移除定时器一旦被跑掉,这条断言就恒真了。
  for (let i = 0; i < 12; i += 1) await settle();
  const strayHighlights = document.querySelectorAll(".ref-highlight");
  assert(
    strayHighlights.length === 0,
    `无关刷新点亮了 ${strayHighlights.length} 个条目(${strayHighlights.map((n) => n.dataset.docId).join(",")}):悬挂高亮被消费了`,
  );
  await flush();
}

// ---------- refreshDocsSoon 的失败路径同样不得留下悬挂高亮(单独钉) ----------
// 上面那条只走得到 refreshDocs 的 catch。运行中真正高频跑的是 refreshDocsSoon:agent 每次
// 改需求/缺陷都会排它,而它的 catch 是另一条独立的出口(console.error,不是 toastError)。
// 实测过:只删掉 refreshDocsSoon 里的 clearPendingJump()、保留 refreshDocs 那处,整套冒烟
// 照样全绿——"两条路径都钉住了"是个错觉,将来会静默回退。这一条只钉 refreshDocsSoon。
// 手法:让跳转触发的那次 refreshDocs 卡在闸门上(闸门在 invoke 那一刻就捕获,随后立刻
// 摘掉,后续调用不再等),于是这一段里唯一跑得完的刷新就是 refreshDocsSoon —— 清理到底
// 是谁做的没有歧义,refreshDocs 那处即使还在也帮不上忙。
{
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  assert(!byId.get("view-documents").classList.contains("active"), "前置失败:未离开单页视图");
  assert(document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'), "前置失败:跳转前列表里没有 R-002");
  let releaseJumpRefresh;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseJumpRefresh = resolve; }));
  sandbox.jumpToEntry("R-002");
  // 只推微任务:这一步要的是"refreshDocs 卡住、pendingJumpId 挂着",不能让定时器插进来。
  for (let i = 0; i < 12; i += 1) await settle();
  invokeGates.delete("docs_snapshot");
  assert(
    vm.runInContext("pendingJumpId", sandbox) === "R-002",
    `前置失败:跳转没有挂起高亮(实得 ${JSON.stringify(vm.runInContext("pendingJumpId", sandbox))})`,
  );
  // 被测的正是 refreshDocsSoon 那条 catch,它只 console.error —— 开窗放行,出了这段立刻收回。
  expectedConsoleError = "冒烟注入";
  const consoleHitsBefore = expectedConsoleHits;
  invokeFailures.set("docs_snapshot", "冒烟注入:refreshDocsSoon 撞上目录被删/文件被锁/解析失败");
  sandbox.refreshDocsSoon();
  await drainTimersOnce("refreshDocsSoon 失败路径");
  for (let i = 0; i < 12; i += 1) await settle();
  invokeFailures.delete("docs_snapshot");
  assert(
    expectedConsoleHits > consoleHitsBefore,
    "前置失败:注入的 docs_snapshot 失败没有走到 refreshDocsSoon 的 catch(这一段根本没测到目标路径)",
  );
  expectedConsoleError = null;
  assert(
    vm.runInContext("pendingJumpId", sandbox) === null,
    "refreshDocsSoon 抛错后 pendingJumpId 还挂着:之后任意一次无关刷新都会把它兑现——用户没点跳转,条目自己亮了(悬挂高亮)",
  );
  // 收尾:放开闸门让那次卡住的 refreshDocs 跑完(注入已撤,它会正常重绘),
  // 并确认这次无关刷新没有凭空点亮任何条目。
  releaseJumpRefresh();
  for (let i = 0; i < 12; i += 1) await settle();
  const straySoonHighlights = document.querySelectorAll(".ref-highlight");
  assert(
    straySoonHighlights.length === 0,
    `refreshDocsSoon 失败后的无关刷新点亮了 ${straySoonHighlights.length} 个条目(${straySoonHighlights.map((n) => n.dataset.docId).join(",")}):悬挂高亮被消费了`,
  );
  await flush();
}

byId.get("sop-picker").click();
await flush();
const sopEntry = document.querySelector("#sop-list .sop-entry");
assert(sopEntry, "继续按钮旁未展示可调用 SOP");
sopEntry.click();
await flush();
assert(!byId.get("auto-continue").checked, "选择 SOP 后未打断自动推进");
assert(invokeLog.includes("memory_entries"), "SOP 入口未读取已沉淀 SOP");
assert(invokeLog.includes("run_prompt"), "选择 SOP 后未进入输入执行链路");

// ---------- R-125 记忆召回可视化：召回了什么、为什么召回、是否被采纳 ----------
// 走真实路径:点活动栏的「记忆」进入该视图,由它触发 refreshMemory。
const memoryTab = document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "memory");
assert(memoryTab, "活动栏缺少记忆入口");
memoryTab.click();
await flush();
const diagnosticsBefore = invokeLog.filter(cmd => cmd === "memory_recalls").length;
await sandbox.refreshMemory();
await flush();
assert(invokeLog.filter(cmd => cmd === "memory_recalls").length === diagnosticsBefore, "阅读记忆不应重复加载隐藏的诊断");
byId.get("memory-insights-tab").click();
await flush();
assert(invokeLog.includes("memory_recalls"), "记忆页未拉取召回明细(没有召回明细就没有评估手段)");
assert(invokeLog.includes("memory_control_plane"), "记忆页未拉取控制面投影");
assert(listText("memory-control-plane").includes("3"), "记忆控制面未展示 inbox backlog");
assert(listText("memory-control-plane").includes("batch-7") && listText("memory-control-plane").includes("模拟 manager 失败"), "记忆控制面未展示批次失败状态");
assert(document.querySelector("#memory-control-plane button")?.textContent === "重试整理", "批次失败时未提供重试入口");
assert(listText("memory-control-plane").includes("M-SOP-001") && listText("memory-control-plane").includes("0.50"), "记忆控制面未展示价值聚合");

// R-124:SOP 候选必须停在用户面前,不能自己入库。
assert(invokeLog.includes("memory_note_candidates"), "记忆页未拉取待确认候选");
const candidate = document.querySelector("#memory-candidates .memory-candidate");
assert(candidate, "SOP 候选未渲染");
assert(candidate.classList.contains("sop"), "SOP 候选未按分类标记");
assert(listText("memory-candidates").includes("read → edit → bash → req"), "候选未展示提炼原料(工具顺序)");
const candidateButtons = document.querySelectorAll("#memory-candidates .memory-candidate-actions button");
assert(
  candidateButtons.some((b) => b.textContent === "采纳") && candidateButtons.some((b) => b.textContent === "丢弃"),
  "候选缺少采纳/丢弃入口(候选一旦自动入库就违背「用户的模板由用户定」)",
);
candidateButtons.find((b) => b.textContent === "丢弃").click();
await flush();
assert(invokeLog.includes("memory_note_discard"), "丢弃候选未调用后端");
const recallHits = document.querySelectorAll("#memory-recalls .memory-recall-hit");
assert(recallHits.length === 4, `召回明细未渲染全部命中,实得 ${recallHits.length}`);
const recallText = listText("memory-recalls");
for (const text of ["M-002", "run-7", "发版 SOP", "已读取正文", "尚未记录正文读取", "历史读取未知", "未注入", "本次检索没有命中记忆", "未知"]) {
  assert(recallText.includes(text), `召回明细缺少 ${text}`);
}
assert(!recallText.includes("已采纳"), "读取或注入不得被称为采纳");
assert(recallHits.filter((node) => node.classList.contains("read")).length === 1, "正文读取状态错误");
assert(listText("memory-recall-rate").includes("2"), "标题未显示最近检索数量");
assert(document.querySelector("#memory-recalls .memory-recall")?.open === true, "最新一次召回应默认展开");
// 效果画像:零命中要在列表里看得出来,且能直接删。
// 列表只在选中某个 scope/category 后渲染,冒烟里直接驱动该入口。
await sandbox.loadMemoryList("project", null);
await flush();
const dormantRow = document.querySelector("#memory-list .memory-row.dormant");
assert(dormantRow, "长期零命中的记忆未被标记(无从判断哪些记忆该清理)");
assert(listText("memory-list").includes("从未命中"), "记忆列表未给出最近命中时间");
dormantRow.click();
await flush();
const detailButtons = document.querySelectorAll("#memory-detail .memory-detail-actions button");
assert(
  detailButtons.some((b) => b.textContent === "删除"),
  "记忆详情缺少删除入口(stale 只是降权,仍占索引)",
);
assert(listText("memory-detail").includes("累计命中"), "记忆详情未给出效果画像");

// ---------- R-332 记忆管理工作区:统一条目、筛选、选中态、搜索打开详情 ----------
const memoryScopeFilter = document.querySelector("#memory-scope-filter");
const memoryCategoryFilter = document.querySelector("#memory-category-filter");
const memoryStatusFilter = document.querySelector("#memory-status-filter");
const memorySortFilter = document.querySelector("#memory-sort-filter");
assert(memoryScopeFilter && memoryCategoryFilter && memoryStatusFilter && memorySortFilter, "记忆管理工作区缺少筛选控件");
assert(memorySortFilter.options.length >= 3, "记忆管理工作区缺少稳定排序选项");
assert(listText("memory-list-count").includes("2"), "记忆列表未显示当前结果数量");
assert(document.querySelector("#memory-list .memory-row.selected")?.dataset.memoryId === "M-DEAD-001", "打开详情后列表未显示明确选中态");
assert(
  listText("memory-detail").includes("标题") && listText("memory-detail").includes("召回钩子") && listText("memory-detail").includes("正文"),
  "记忆详情字段缺少可见标签",
);
// 稳定排序:命中最多应把 M-SOP-001 放到第一项。
memorySortFilter.value = "hits";
memorySortFilter.dispatchEvent({ type: "change" });
await flush();
assert(document.querySelector("#memory-list .memory-row")?.dataset.memoryId === "M-SOP-001", "命中排序未生效");
// 搜索结果沿用同一条目组件,点击后必须打开真实详情而不是只展示文本。
const memorySearchInput = document.querySelector("#memory-search-input");
memorySearchInput.value = "冒烟";
memorySearchInput.dispatchEvent({ type: "keydown", key: "Enter" });
await flush();
const searchRow = document.querySelector("#memory-list .memory-row");
assert(searchRow?.dataset.memoryId === "M-SOP-001", "搜索结果未使用统一可交互条目");
searchRow.click();
await flush();
assert(listText("memory-detail").includes("冒烟 SOP"), "搜索结果点击后未打开详情");
assert(document.querySelector("#memory-list .memory-row.selected")?.dataset.memoryId === "M-SOP-001", "搜索打开详情后选中态错误");
assert(document.querySelector("#memory-search-clear") && !document.querySelector("#memory-search-clear").hidden, "搜索后缺少清除搜索入口");

// 全文默认可读：刷新后保留完整长文，编辑与保存不能丢内容。
{
  const savedEntries = structuredClone(payloads.memory_entries);
  memoryCategoryFilter.value = "all";
  memoryCategoryFilter.dispatchEvent({ type: "change" });
  await flush();
  payloads.memory_entries = [
    { id: "M-LONG-001", category: "fact", title: "长正文记忆", description: "钩子", status: "active", body: "第一段要点：这是正文摘要应展示的首段内容。\n\n第二段：拆出来的第二个段落块。\n\n第三段超长：\n" + "很长的段落文本，需要折叠。".repeat(30), hits: 0, last_hit_at: 0, recalled: 0, injected: 0, read: 0, read_observed: 0, updated: "2026-08-01" },
    { id: "M-DEAD-001", category: "fact", title: "从没被用到的记忆", description: "冒烟用:零命中条目", status: "active", body: "陈旧结论", hits: 0, last_hit_at: 0, recalled: 0, injected: 0, read: 0, read_observed: 0, updated: "2026-01-01" },
  ];
  await sandbox.refreshMemory({ force: true });
  await flush();
  const longRow = [...document.querySelectorAll("#memory-list .memory-row")].find((r) => r.dataset.memoryId === "M-LONG-001");
  assert(longRow, "前置失败:长正文记忆条目未渲染");
  longRow.click();
  await flush();
  const fullDocument = document.querySelector("#memory-detail .memory-body-document");
  assert(fullDocument, "记忆详情未提供完整文档");
  assert(fullDocument.textContent.includes("第一段要点") && fullDocument.textContent.includes("第二段"), "正文丢失段落");
  assert((fullDocument.textContent.match(/很长的段落文本/g) || []).length === 30, "长正文被截断");
  assert(!fullDocument.querySelector(".collapsed"), "正文不应要求逐段展开");
  const editBtn = [...document.querySelectorAll("#memory-detail .memory-body-edit-row button")].find((b) => b.textContent.includes("编辑正文"));
  assert(editBtn, "阅读视图缺少「编辑正文」入口");
  editBtn.click();
  const textarea = document.querySelector("#memory-detail .memory-body-read textarea[aria-label]");
  assert(textarea, "点编辑正文后未切换回 textarea");
  assert(textarea.value.includes("第一段要点"), "textarea 未回填当前正文(编辑会丢内容)");
  // 编辑态保存:改正文 → 点保存 → memory_entry_save 载荷带新值。
  const saveBtn = [...document.querySelectorAll("#memory-detail .memory-detail-actions button")].find((b) => b.textContent === "保存修改");
  assert(saveBtn, "前置失败:保存按钮缺失");
  textarea.value = "改过的正文\n\n新段落";
  saveBtn.click();
  await flush();
  const savedCall = invokeArgs.find(({ cmd, args }) => cmd === "memory_entry_save" && args?.id === "M-LONG-001");
  assert(savedCall, "保存修改未提交 memory_entry_save");
  assert(savedCall.args.body === "改过的正文\n\n新段落", `保存载荷正文不对: "${savedCall.args.body}"`);
  payloads.memory_entries = savedEntries;
  await sandbox.refreshMemory({ force: true });
  await flush();
}

// ---------- R-150 空闲整理清单 + 三档宽度响应式 ----------
assert(invokeLog.includes("memory_value_flags"), "记忆页未拉取空闲整理清单");
const flagRows = document.querySelectorAll("#memory-value-flags .memory-flag-row");
assert(flagRows.length === 2, `空闲整理清单未渲染全部候选,实得 ${flagRows.length}`);
assert(
  document.querySelector("#memory-value-flags .memory-flag-row.zero-read"),
  "零采纳候选未按类别标记(区分「语义显著但决策无关」)",
);
assert(
  listText("memory-value-flags").includes("M-001") && listText("memory-value-flags").includes("5"),
  "零采纳候选未给出召回次数(判断依据缺失)",
);
// 记忆列表采纳率:召回/采纳 数据在条目 meta 可见。
assert(
  listText("memory-list").includes("召回") && listText("memory-list").includes("正文读取"),
  "记忆列表未展示召回/采纳数据(验收②数据面)",
);
// 三档宽度:800/1024/1280 下记忆页不崩、清单与采纳率数据仍在 DOM。
for (const width of [800, 1024, 1280]) {
  windowShim.innerWidth = width;
  await flush();
  assert(
    document.querySelector("#memory-value-flags .memory-flag-row"),
    `${width}px 下空闲整理清单缺失`,
  );
  assert(
    listText("memory-list").includes("召回") && listText("memory-list").includes("正文读取"),
    `${width}px 下记忆列表召回/采纳数据缺失`,
  );
}
windowShim.innerWidth = 1280;
await flush();

// 无读取证据不能直接批量降级记忆，用户仍可打开条目逐条修订。
assert(!document.getElementById("memory-cleanup-btn"), "不应提供按未读取批量降级入口");
assert(!invokeLog.includes("memory_cleanup_demote"), "不得调用旧批量降级接口");
assert(listText("memory-flags-count").includes("2"), "复查清单计数错误");

// 跨项目迟到响应不得覆盖当前项目；失败后可重新加载。
{
  const previousProject = sandbox.currentProject;
  sandbox.showMemoryTab("insights");
  await flush();
  let release;
  invokeGates.set("memory_recalls", new Promise((resolve) => { release = resolve; }));
  const pending = sandbox.refreshMemory({ force: true });
  await settle();
  invokeGates.delete("memory_recalls");
  sandbox.currentProject = "C:/memory-other-project";
  await sandbox.refreshMemory({ force: true });
  await flush();
  byId.get("memory-recalls").textContent = "当前项目的记录";
  release();
  await pending;
  assert(listText("memory-recalls") === "当前项目的记录", "旧项目迟到响应覆盖了当前项目");
  expectedPersistentError = "记忆页加载失败";
  invokeFailures.set("memory_recalls", "memory read unavailable");
  await sandbox.refreshMemory({ force: true });
  await flush();
  invokeFailures.delete("memory_recalls");
  expectedPersistentError = null;
  await sandbox.refreshMemory({ force: true });
  await flush();
  assert(listText("memory-recalls").includes("run-7"), "读取失败后重试未恢复");
  sandbox.currentProject = previousProject;
  await sandbox.refreshMemory();
}

// ---------- D-726:对话搜索只在当前 activePane 内取候选 ----------
// `messages` 同时挂着多个会话 pane。搜索必须和复制上下文一样只读当前 pane,
// 否则命中别的会话后 scrollIntoView 会表现为空操作。
{
  const previousPane = vm.runInContext("activePane", sandbox);
  const previousQuery = byId.get("chat-search-input").value;
  const previousIndex = vm.runInContext("searchIndex", sandbox);
  vm.runInContext(
    "globalThis.__d726SearchPane = document.createElement('div'); __d726SearchPane.className = 'msg-pane';",
    sandbox,
  );
  const scopedPane = vm.runInContext("__d726SearchPane", sandbox);
  const currentHit = document.createElement("div");
  currentHit.className = "msg";
  currentHit.textContent = "D-726 当前会话关键词";
  scopedPane.appendChild(currentHit);
  const foreignPane = document.createElement("div");
  foreignPane.className = "msg-pane";
  const foreignHit = document.createElement("div");
  foreignHit.className = "msg";
  foreignHit.textContent = "D-726 其他会话关键词";
  foreignPane.appendChild(foreignHit);
  byId.get("messages").append(scopedPane, foreignPane);
  vm.runInContext("activePane = __d726SearchPane", sandbox);
  byId.get("chat-search-input").value = "d-726";
  vm.runInContext("searchIndex = 0; updateSearch()", sandbox);
  assert(currentHit.classList.contains("search-current"), "当前会话内的搜索命中未标记");
  assert(!foreignHit.classList.contains("search-hit"), "对话搜索越过 activePane 命中其他会话");
  assert(byId.get("chat-search-count").textContent === "1/1", "当前会话搜索计数未排除其他会话");
  scopedPane.remove();
  foreignPane.remove();
  byId.get("chat-search-input").value = previousQuery;
  sandbox.__d726PreviousPane = previousPane;
  vm.runInContext(
    `activePane = globalThis.__d726PreviousPane; searchIndex = ${previousIndex}; updateSearch(); delete globalThis.__d726PreviousPane; delete globalThis.__d726SearchPane;`,
    sandbox,
  );
}
// ---------- 主对话工具块:⎿ 摘要行与展开详情不得双写同一段文案(历史回放路径) ----------
// 用户实测:一条 edit 失败,⎿ 行显示了一段文案,点开详情又把同一段完整贴了一遍。
// 根因是摘要与详情各自独立地从同一份 content 取一遍,详情靠 `full !== preview` 去重,
// 只挡得住单行短结果。判据用「同一段文字在单个工具块里出现几次」——必须限定在单个
// .tool-msg 上取 textContent:harness 的 textContent 会把 innerHTML 文本与子节点文本拼接,
// 对整个 #messages 取会把别的消息里的同款文案一起算进来。
{
  const blocks = document.querySelectorAll("#messages [data-active] .tool-msg");
  assert(blocks.length === 4, `历史回放应按 call_id 配出 4 个工具块,实得 ${blocks.length}`);
  const [h1, h2, h3, h4] = blocks;
  const resultOf = (block) => block.querySelector(".tool-msg-result")?.textContent ?? "";
  assert(h1.querySelector(".tool-msg-head .tool-msg-result"), "工具结果摘要未并入可点击的工具行");
  assert(h1.querySelectorAll(".tool-msg-result").length === 1, "工具块结果摘要出现重复节点");
  assert(!h1.querySelector(".tool-msg > .tool-msg-result"), "成功工具块仍把结果摘要渲染成第二行");
  assert(!h1.querySelector(".tool-msg-result").classList.contains("hidden"), "失败工具错误摘要没有默认可见");
  assert(h2.querySelector(".tool-msg-head .tool-msg-result"), "成功工具结果摘要未与调用信息合并到同一行");
  assert(!h2.querySelector(".tool-msg > .tool-msg-result"), "成功工具块仍渲染了独立的第二行结果");
  const h2Head = h2.querySelector(".tool-msg-head");
  const h2Detail = h2.querySelector(".tool-msg-detail");
  assert(h2Detail.classList.contains("hidden"), "成功工具详情不应默认展开");
  h2Head.click();
  assert(!h2Detail.classList.contains("hidden") && h2Head.getAttribute("aria-expanded") === "true", "点击工具行未展开完整详情");
  h2Head.click();
  assert(h2Detail.classList.contains("hidden") && h2Head.getAttribute("aria-expanded") === "false", "再次点击工具行未收起详情");
  assert(!style.includes(".turn-divider"), "样式表仍残留无创建点的 .turn-divider");
  assert(/\.msg\.tool-msg\s*\{[^}]*margin:\s*-4px\s+0/.test(style), "工具行间距未收紧");
  assert(/\.msg\.user:not\(:first-child\)\s*\{[^}]*margin-top:\s*36px/.test(style), "轮间留白未显著拉开");
  const chatRenderer = esmModuleCache.get("05-chat-render.js")?.namespace;
  const activityRenderer = esmModuleCache.get("06-activity.js")?.namespace;
  // ---------- R-352:主区展示预算、限定上下文与活动面板出口 ----------
  {
    const longDiff = Array.from({ length: 40 }, (_, index) => ({
      kind: index === 20 ? "add" : "ctx",
      old_line: index + 1,
      new_line: index + 1,
      text: index === 20 ? "真正的一行改动" : `远处上下文 ${index + 1}`,
    }));
    const display = { kind: "diff", path: "src/large.rs", additions: 1, deletions: 0, language: "rust", lines: longDiff };
    const compactBlock = chatRenderer?.buildToolBlock("edit", { path: "src/large.rs" });
    chatRenderer?.fillToolBlock(compactBlock, { ok: true, outcome: "success", content: "changed", display, input: {} });
    const compactDisplay = compactBlock?.detail.querySelector(".tool-display.diff");
    assert(compactDisplay, "主区 edit 未渲染差异展示块");
    assert(compactDisplay.querySelectorAll(".diff-row").length < longDiff.length, "主区 diff 仍渲染整份文件");
    assert(compactDisplay.querySelector(".diff-omitted"), "主区 diff 缺少省略上下文提示");
    assert(compactDisplay.textContent.includes("真正的一行改动"), "主区限定 diff 丢失真实变更行");
    assert(compactBlock?.detail.querySelector(".tool-display-more")?.textContent === "去活动面板看全", "超限 diff 缺少活动面板出口");
    assert(
      /\.tool-msg-detail > \.tool-display\s*\{[^}]*max-height:\s*420px[^}]*overflow:\s*auto/.test(style),
      "主区 tool-display 缺少 420px 滚动上限",
    );
    assert(/\.tool-msg-raw\s*\{[^}]*max-height:\s*420px/.test(style) && source.includes("rest.length > 8000"), "tool-msg-raw 的 420px/8000 字限制被削弱");
    const activityHost = document.createElement("div");
    const fullDisplay = activityRenderer?.appendDisplayBlock(activityHost, display);
    assert(activityHost.querySelectorAll(".diff-row").length === longDiff.length, "活动面板默认全量 diff 消费被裁剪");
    byId.get("messages").appendChild(compactBlock.wrap);
    byId.get("bg-panel").classList.add("hidden");
    compactBlock.detail.querySelector(".tool-display-more").click();
    assert(!byId.get("bg-panel").classList.contains("hidden"), "活动面板出口未打开真实活动面板");
    compactBlock.wrap.remove();
    fullDisplay?.remove?.();
  }
  const singleReasoning = chatRenderer?.buildReasoningBlock("单行思考摘要");
  chatRenderer?.renderReasoningBlock(singleReasoning.body);
  assert(singleReasoning.wrap.hidden === true, "单行思考仍生成可见独立块");
  const multiReasoning = chatRenderer?.buildReasoningBlock("第一行思考\n第二行思考");
  chatRenderer?.renderReasoningBlock(multiReasoning.body);
  assert(!multiReasoning.wrap.hidden && multiReasoning.head.classList.contains("expandable"), "多行思考未保留可展开块");
  // 详情里真正的"剩余输出"块(带 args 类的那个是完整入参,不是结果原文)。
  const restOf = (block) =>
    block.querySelectorAll(".tool-msg-raw").find((n) => !n.classList.contains("args")) ?? null;

  // ① 首行超长 + 多行的失败结果:同一段文案只能出现一次。
  const needle = HISTORY_LONG_FIRST_LINE.slice(0, 60);
  assert(
    h1.textContent.split(needle).length - 1 === 1,
    `工具块把同一段结果文案渲染了两遍(⎿ 行与展开详情双写):出现 ${h1.textContent.split(needle).length - 1} 次`,
  );
  assert(
    resultOf(h1) === `⎿ ${HISTORY_LONG_FIRST_LINE.slice(0, 109)}…`,
    `⎿ 行截断规则漂移了:"${resultOf(h1).slice(0, 40)}…"(长度 ${resultOf(h1).length})`,
  );
  // 去重不能靠"干脆不给详情":被截掉的后半句与后续行必须仍读得到。
  const h1Rest = restOf(h1);
  assert(h1Rest, "长首行被截断后没有展开区:被截掉的内容再也读不到了");
  assert(
    h1Rest.textContent.startsWith("…") && h1Rest.textContent.includes(HISTORY_LONG_FIRST_LINE.slice(-20)),
    `展开区未接上被截断的首行尾巴:"${h1Rest.textContent.slice(0, 40)}"`,
  );
  assert(
    h1Rest.textContent.includes("第二行") && h1Rest.textContent.includes("第三行"),
    "展开区丢了首行之后的正文",
  );
  // 历史详情必须给完整入参(83 行那条源码契约的运行时版本)。
  assert(
    h1.querySelectorAll(".tool-msg-raw").some((n) => n.classList.contains("args") && n.textContent.includes("path")),
    "历史工具块缺少完整入参 JSON",
  );

  // ② 8000 字上界仍然生效(去重不等于放开长度上界)。
  const h2Rest = restOf(h2);
  assert(h2Rest, "超长历史输出没有展开区");
  assert(h2Rest.textContent.endsWith("…(已截断)"), `超长输出未截断:结尾为 "${h2Rest.textContent.slice(-20)}"`);
  assert(h2Rest.textContent.length < 8100, `截断上界失效,实得 ${h2Rest.textContent.length} 字`);

  // ③ UI-0926 #6:bash 成功行是「退出码 · 亮点行」的人话摘要,而不是原文首行;
  // 摘要不再是原文的一段,展开区给完整原文(exit code 行不能丢)。
  assert(resultOf(h3) === "⎿ 退出码 0 · 真正的输出行", `bash 成功摘要漂移:"${resultOf(h3)}"`);
  assert(restOf(h3)?.textContent === "exit code: 0\n真正的输出行", `展开区不是完整原文(exit code 行被丢掉了?):"${restOf(h3)?.textContent}"`);

  // ④ 全篇只有 "exit code: 0" 时仍给出退出码,不塌成「完成」(原实现 `|| lines[0]` 兜底的等价保留)。
  assert(resultOf(h4) === "⎿ 退出码 0", `唯一的结果行被吞成兜底文案:"${resultOf(h4)}"`);
  assert(restOf(h4) === null, "只有一行结果时不该出展开区(展开了还是那一行 = 假承诺)");
}

// ---------- 活动面板：终端与失败入列(R-168) + 筛选 + 信息量 + 可操作 ----------
const toolStart = handlers.get("kz:tool-start");
const toolEnd = handlers.get("kz:tool-end");
const taskProgress = handlers.get("kz:task-progress");
assert(toolStart && toolEnd, "工具事件未订阅");
assert(taskProgress, "子代理进度事件未订阅");
toolStart({ payload: { id: "T1", name: "bash", summary: "cargo test --workspace", input: { command: "cargo test --workspace", workdir: "." }, sessionId: "sess-smoke" } });
// D-491:轮次与当前工具状态必须由真实事件更新到实际 DOM，不允许 live-* 静默 no-op。
for (const id of ["live-turn", "live-action"]) {
  assert(byId.get(id), `${id} 动态状态节点缺失，live-* 写入会静默 no-op`);
}
// UI-0926 #4:#live-note(与对话流重复)、#live-focus(与焦点卡重复)已删,写入点一并删除——
// 节点删了而写入还在,就是 D-491 那种静默 no-op。
for (const id of ["live-note", "live-focus"]) {
  assert(!byId.has(id), `#${id} 应已删除(与对话流/焦点卡重复)`);
  assert(!source.includes(`"${id}"`), `源码仍在写已删除的 #${id}(静默 no-op)`);
}
assert(byId.get("live-turn").textContent.includes("第") || byId.get("live-turn").textContent.includes("Round"), "kz:turn 未更新当前轮次显示");
assert(!byId.get("live-action").classList.contains("hidden"), "kz:tool-start 未显示当前工具状态");
assert(byId.get("live-action").textContent.includes("bash"), "当前工具状态未显示工具名");
toolStart({ payload: { id: "T2", name: "edit", summary: "main.js", input: { path: "ui/main.js" }, sessionId: "sess-smoke" } });
toolStart({ payload: { id: "T3", name: "task", summary: "审查子代理", input: { prompt: "review" }, sessionId: "sess-smoke" } });
await flush();
const bashEntry = document.querySelector("#bg-list .bg-entry[data-bg-tool=bash]");
assert(bashEntry, "活动面板缺少终端类条目");
assert(!document.querySelector("#bg-list .bg-entry[data-bg-id=T2]"), "成功 edit 不该进入活动栏(R-168)");
assert(!document.querySelector("#bg-list .bg-entry[data-bg-id=T3]"), "不带 phase 的 task 不该进入活动栏(R-168)");
// D-729:上面两条只看 DOM 结果,判据被抽空成恒真时它们会一起红但说不出原因。
// 直接对谓词断言:c611f909 那次反转只改函数体就全绿通过了,因为当时的门禁(本文件 :142)
// 只匹配调用点字符串。这一段让同类反转必须连断言一起改才能过。
{
  const isActivityToolFn = esmModuleCache.get("06-activity.js")?.namespace?.isActivityTool;
  assert(typeof isActivityToolFn === "function", "R-168 isActivityTool 未导出,无法校验降噪判据");
  assert(isActivityToolFn("bash") === true, "R-168:bash 必须入活动栏");
  assert(isActivityToolFn("edit") === false, "R-168:成功 edit 不得入活动栏(判据被改成恒真了?)");
  assert(isActivityToolFn("read") === false, "R-168:成功 read 不得入活动栏");
  assert(isActivityToolFn("task", { phase: "scouting" }) === true, "R-173:编排派发的 task 必须入活动栏");
  assert(isActivityToolFn("task", { prompt: "x" }) === false, "R-168:模型自派的 task 不得入活动栏");
}
assert(
  bashEntry.querySelector(".bg-tool")?.textContent === "bash"
    && bashEntry.querySelector(".bg-target")?.textContent.includes("cargo test"),
  "条目未把工具名与目标分列(拼成一行会被截断,看不出跑的是哪条命令)",
);
assert(bashEntry.querySelector(".bg-args")?.textContent.includes("workdir"), "条目未提供可展开的完整入参");
assert(
  bashEntry.querySelectorAll(".bg-actions button").some((b) => b.textContent === "复制")
    && bashEntry.querySelectorAll(".bg-actions button").some((b) => b.textContent === "导出"),
  "终端类条目缺少复制/导出",
);
assert(
  bashEntry.querySelectorAll(".bg-actions button").some((b) => b.textContent === "停止"),
  "运行中的终端条目缺少单独停止入口",
);
toolEnd({ payload: { id: "T1", name: "bash", ok: true, preview: "test result: ok", display: null, sessionId: "sess-smoke" } });
toolEnd({ payload: { id: "T3", name: "task", ok: false, preview: "子代理失败", display: null, sessionId: "sess-smoke" } });
await flush();
assert(bashEntry.querySelector(".bg-meta")?.textContent.includes("成功"), "结束后未在元信息里给出成败");
assert(/\d+(\.\d+)?(ms|s)/.test(bashEntry.querySelector(".bg-meta")?.textContent ?? ""), "结束后未给出耗时");
const taskEntry = document.querySelector("#bg-list .bg-entry[data-bg-tool=task]");
assert(taskEntry.querySelector(".bg-meta")?.textContent.includes("内部调用"), "子代理条目未给出内部调用数");
assert(
  taskEntry.querySelectorAll(".bg-actions button").some((b) => b.textContent === "重跑"),
  "结束的条目缺少重跑入口",
);
// D-234:批次格由 Git 提交标题推导。直接 agent 与子 agent 提交都必须立即刷新快照，
// 不能等整轮结束才看到进度变化。
const docsBeforeBatchCommit = invokeLog.filter((cmd) => cmd === "docs_snapshot").length;
toolEnd({ payload: { id: "T4", name: "git", ok: true, preview: "committed verified staged set (abc123)", display: null, sessionId: "sess-smoke" } });
await flush();
assert(
  invokeLog.filter((cmd) => cmd === "docs_snapshot").length > docsBeforeBatchCommit,
  "agent 提交后未即时刷新 Git 推导的批次进度",
);
const docsBeforeChildBatchCommit = invokeLog.filter((cmd) => cmd === "docs_snapshot").length;
taskProgress({ payload: { id: "T3", text: "子代理已提交", trace: { name: "git", phase: "end", ok: true, preview: "committed verified staged set (def456)" }, sessionId: "sess-smoke" } });
await flush();
assert(
  invokeLog.filter((cmd) => cmd === "docs_snapshot").length > docsBeforeChildBatchCommit,
  "子代理提交后未即时刷新 Git 推导的批次进度",
);
// 筛选:按类型与成败收敛,且计数要能看出"筛出/总数",否则会误以为本轮只跑了这几个工具。
const typeFilter = byId.get("bg-type-filter");
typeFilter.value = "terminal";
typeFilter._listeners.change?.forEach((fn) => fn({ target: typeFilter }));
assert(
  document.querySelectorAll("#bg-list .bg-entry").filter((n) => !n.classList.contains("hidden")).length === 1,
  "按类型筛选未生效",
);
assert(listText("bg-count").includes("/"), "筛选后未同时给出筛出数与总数");
const statusFilter = byId.get("bg-status-filter");
typeFilter.value = "all";
typeFilter._listeners.change?.forEach((fn) => fn({ target: typeFilter }));
statusFilter.value = "err";
statusFilter._listeners.change?.forEach((fn) => fn({ target: statusFilter }));
assert(
  document.querySelectorAll("#bg-list .bg-entry").filter((n) => !n.classList.contains("hidden"))
    .every((n) => n.dataset.bgTool === "task"),
  "按失败状态筛选未生效",
);
statusFilter.value = "all";
statusFilter._listeners.change?.forEach((fn) => fn({ target: statusFilter }));

// ---------- R-173 编排派发的勘察/复核子代理:实时进度必须落到面板上 ----------
// 编排对象按角色表派发的这批子代理不经模型 tool call,主对话里没有内联工具块兜底,
// 活动面板是它们唯一的可见处。此前 name 恒为 "task" 被 R-168 一刀切静默,于是 5 勘察 +
// 3 复核的轮次/工具进度整批落空。区分依据是后端给的 input.phase(scouting/review)。
const orchEntry = (role) => [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === role);
// 活动面板分三段之后,同一个 phase 在「运行中/需要关注/已完成」里各有一份组容器
// (跑完的勘察收进折叠区,还在跑的留在上面)。所以「这个阶段有几条」必须跨段求和,
// 只看第一个组会随条目落位漂移;组头的完成数本来就是跨段算的,取任一份都一样。
const orchGroups = (phase) => [...document.querySelectorAll(`.bg-group[data-bg-phase=${phase}]`)];
const orchGroup = (phase) => orchGroups(phase)[0];
const orchPhaseEntries = (phase) => orchGroups(phase).flatMap((g) => [...g.querySelectorAll(".bg-entry")]);
const orchGroupHead = (phase) => orchGroups(phase)
  .map((g) => g.querySelector(".bg-group-head")?.textContent ?? "").find((text) => text) ?? "";
const scoutRoles = ["architecture_scout", "runtime_scout", "test_scout"];
// ① 契约时序:N 条 start 先全发完。派发瞬间就该全部可见,不能等各自跑完才冒出来。
for (const role of scoutRoles) {
  toolStart({ payload: { id: role, name: "task", summary: `${role} · 勘察`, input: { prompt: `派给 ${role} 的完整指令`, phase: "scouting", role }, sessionId: "sess-smoke" } });
}
// 同一批里混一条模型自己派的 task(不带 phase):R-168 的静默口径不能被顺手打破。
toolStart({ payload: { id: "MODEL_TASK", name: "task", summary: "模型自己派的子代理", input: { prompt: "review" }, sessionId: "sess-smoke" } });
await flush();
for (const role of scoutRoles) {
  assert(orchEntry(role), `编排派发的勘察子代理 ${role} 没进活动面板(内部进度整批丢掉)`);
}
  assert(!orchEntry("MODEL_TASK"), "模型自己派的 task 被一起放行了(R-168 静默口径被打破)");
// ② 分组:按 input.phase 分区,这是 Running/Finished 分区的雏形。
assert(orchGroup("scouting"), "勘察子代理未按 input.phase 分组");
assert(
  orchGroup("scouting").querySelectorAll(".bg-entry").length === scoutRoles.length,
  `勘察分组内条目数不对:${orchGroup("scouting").querySelectorAll(".bg-entry").length}`,
);
assert(orchGroupHead("scouting").includes("勘察"), `勘察分组标题缺阶段名,实得 "${orchGroupHead("scouting")}"`);
assert(orchGroupHead("scouting").includes("0/3"), `勘察分组标题未给出完成数/总数,实得 "${orchGroupHead("scouting")}"`);
// ③ 单条信息量:角色名(不是恒为 "task" 的工具名)、所属阶段、已运行时长与内部调用数。
const scout = orchEntry("architecture_scout");
assert(
  scout.querySelector(".bg-tool")?.textContent === "architecture_scout",
  `条目未以角色名标识,实得 "${scout.querySelector(".bg-tool")?.textContent}"(8 条都叫 task 等于没标识)`,
);
assert(
  scout.querySelector(".bg-phase-badge")?.textContent === "勘察",
  `条目未标出所属阶段,实得 "${scout.querySelector(".bg-phase-badge")?.textContent}"`,
);
assert(/运行中 · \d+s · 内部调用 \d+/.test(scout.querySelector(".bg-meta")?.textContent ?? ""),
  `运行中未给出状态/已运行时长/内部调用数,实得 "${scout.querySelector(".bg-meta")?.textContent}"`);
assert(scout.dataset.bgStatus === "running", `运行中状态未落到条目上,实得 "${scout.dataset.bgStatus}"`);
// ④ 执行期进度:纯轮次进度(trace 为 null)与工具进度都要落到对应角色。
taskProgress({ payload: { id: "architecture_scout", text: "第 3/12 轮", trace: null, sessionId: "sess-smoke" } });
await flush();
assert(scout.querySelector(".bg-prog")?.textContent === "第 3/12 轮",
  `轮次进度未挂回角色条目,实得 "${scout.querySelector(".bg-prog")?.textContent}"`);
// ⑤ 当前正在用的工具名 —— 用户点名要的那一项。值与写入去向都断言:
// 只断言文本看不出"写对了内容却写错了元素",dataset 探针把去向也钉死。
taskProgress({ payload: { id: "architecture_scout", text: "第 4/12 轮", trace: { child_id: "c1", phase: "start", name: "grep", summary: "phase_pipeline" }, sessionId: "sess-smoke" } });
await flush();
assert(scout.querySelector(".bg-current")?.textContent.includes("grep"),
  `当前工具名未显示,实得 "${scout.querySelector(".bg-current")?.textContent}"`);
assert(scout.dataset.bgCurrentTool === "grep", `当前工具名写错了地方,实得 "${scout.dataset.bgCurrentTool}"`);
assert(!scout.querySelector(".bg-current")?.classList.contains("hidden"), "当前工具名所在行仍是隐藏的");
assert(!scout.querySelector(".bg-current")?.classList.contains("idle"), "工具正在跑却标成了空闲态");
taskProgress({ payload: { id: "architecture_scout", text: "第 4/12 轮", trace: { child_id: "c1", phase: "end", name: "grep", ok: true, preview: "命中 12 处" }, sessionId: "sess-smoke" } });
taskProgress({ payload: { id: "architecture_scout", text: "第 5/12 轮", trace: { child_id: "c2", phase: "start", name: "read", summary: "phase_pipeline.rs" }, sessionId: "sess-smoke" } });
await flush();
assert(scout.dataset.bgCurrentTool === "read", `当前工具名未跟着换到下一个工具,实得 "${scout.dataset.bgCurrentTool}"`);
assert(/内部调用 2/.test(scout.querySelector(".bg-meta")?.textContent ?? ""),
  `工具调用次数未累计,实得 "${scout.querySelector(".bg-meta")?.textContent}"`);
// ⑥ 终态:成功/失败/超时三分,完成的条目继续可见。
toolEnd({ payload: { id: "architecture_scout", name: "task", ok: true, preview: "勘察简报首行", display: null, sessionId: "sess-smoke" } });
toolEnd({ payload: { id: "runtime_scout", name: "task", ok: false, preview: "(超时,未产出结果)", display: null, sessionId: "sess-smoke" } });
toolEnd({ payload: { id: "test_scout", name: "task", ok: false, preview: "子代理内部报错", display: null, sessionId: "sess-smoke" } });
await flush();
assert(orchEntry("architecture_scout")?.dataset.bgStatus === "ok", "成功角色终态不对");
assert(orchEntry("test_scout")?.dataset.bgStatus === "err", "失败角色终态不对");
assert(orchEntry("runtime_scout")?.dataset.bgStatus === "timeout",
  `超时角色未与失败区分开,实得 "${orchEntry("runtime_scout")?.dataset.bgStatus}"`);
assert(orchEntry("runtime_scout")?.classList.contains("timeout"), "超时角色缺少可视区分的样式钩子");
assert(!orchEntry("test_scout")?.classList.contains("timeout"), "普通失败被误标成超时");
assert(orchEntry("runtime_scout")?.querySelector(".bg-meta")?.textContent.includes("超时"),
  `超时角色元信息未写明超时,实得 "${orchEntry("runtime_scout")?.querySelector(".bg-meta")?.textContent}"`);
assert(orchEntry("test_scout")?.querySelector(".bg-meta")?.textContent.includes("失败"), "失败角色元信息未写明失败");
assert(scout.dataset.bgCurrentTool === "", "角色收尾后当前工具名没清掉(会一直显示最后一个工具在跑)");
for (const role of scoutRoles) {
  assert(orchEntry(role) && !orchEntry(role).classList.contains("hidden"), `${role} 跑完就消失了(完成的条目必须保留可见)`);
}
assert(orchGroupHead("scouting").includes("3/3"), `勘察分组标题未跟随完成数,实得 "${orchGroupHead("scouting")}"`);
// ⑦ 复核阶段单独一区,与勘察分开。
for (const role of ["spec_reviewer", "risk_reviewer"]) {
  toolStart({ payload: { id: role, name: "task", summary: `${role} · 复核`, input: { prompt: `派给 ${role} 的完整指令`, phase: "review", role }, sessionId: "sess-smoke" } });
}
await flush();
assert(orchGroup("review"), "复核子代理未单独分区");
assert(orchPhaseEntries("review").length === 2, "复核分组内条目数不对");
assert(orchGroupHead("review").includes("复核"), `复核分组标题缺阶段名,实得 "${orchGroupHead("review")}"`);
assert(orchEntry("spec_reviewer")?.querySelector(".bg-phase-badge")?.textContent === "复核", "复核条目阶段标记不对");
assert(
  orchPhaseEntries("scouting").length === 3,
  "复核条目串进了勘察分组",
);
// ⑧ 角色名跨轮复用:同名角色再次派发要原地复位,否则第二轮的进度全写进上一轮那条终态行。
toolStart({ payload: { id: "architecture_scout", name: "task", summary: "architecture_scout · 勘察", input: { prompt: "第二轮指令", phase: "scouting", role: "architecture_scout" }, sessionId: "sess-smoke" } });
await flush();
assert(orchEntry("architecture_scout")?.dataset.bgStatus === "running",
  `同名角色第二轮派发未复位,实得 "${orchEntry("architecture_scout")?.dataset.bgStatus}"(面板会定格在上一轮)`);
assert(orchPhaseEntries("scouting").length === 3, "同名角色复位时把条目复制了一份");
assert(orchGroupHead("scouting").includes("2/3"), `复位后完成数未回退,实得 "${orchGroupHead("scouting")}"`);
toolEnd({ payload: { id: "architecture_scout", name: "task", ok: true, preview: "第二轮简报", display: null, sessionId: "sess-smoke" } });
await flush();

// ---------- R-184 P2:活动记录按 agent 归属与折叠 ----------
// ① 编排子代理轨迹带角色色点(角色名文本始终在旁,颜色不作唯一区分);
//    无 phase 的模型自派 task 不进活动面板,自然也不该有色点。
assert(orchEntry("architecture_scout")?.querySelector(".bg-dot"), "编排子代理轨迹缺角色色点");
assert(
  !orchEntry("MODEL_TASK") || !orchEntry("MODEL_TASK").querySelector(".bg-dot"),
  "无 phase 的模型自派 task 不该有角色色点",
);
// ② 角色筛选下拉动态列出全部角色(全部 + 每个出现过的角色)。
const roleFilter = document.querySelector("#bg-role-filter");
assert(roleFilter, "活动面板缺角色筛选下拉");
const roleOptions = [...roleFilter.options].map((o) => o.value);
for (const role of [...scoutRoles, "spec_reviewer", "risk_reviewer"]) {
  assert(roleOptions.includes(role), `角色筛选下拉缺选项 ${role},实得 ${roleOptions.join(",")}`);
}
// ③ 切到某角色 → 只剩该角色的条目可见;切回全部 → 全部恢复。
roleFilter.value = "architecture_scout";
roleFilter._listeners.change?.forEach((fn) => fn({ target: roleFilter }));
await flush();
const visibleAfterRole = [...document.querySelectorAll("#bg-list .bg-entry")].filter((n) => !n.classList.contains("hidden"));
assert(
  visibleAfterRole.length >= 1 && visibleAfterRole.every((n) => n.dataset.bgRole === "architecture_scout"),
  `按角色筛选后应只剩 architecture_scout,实得 ${visibleAfterRole.map((n) => n.dataset.bgRole).join(",")}`,
);
roleFilter.value = "all";
roleFilter._listeners.change?.forEach((fn) => fn({ target: roleFilter }));
await flush();
// ④ 主对话里同一角色的 task 工具块折叠成一组(默认收起,组头带块数)。
const fold = document.querySelector('.agent-fold[data-agent-role="architecture_scout"]');
assert(fold, "主对话缺角色折叠组");
const foldHead = fold.querySelector(".agent-fold-head");
const foldBody = fold.querySelector(".agent-fold-body");
assert(foldHead && foldBody, "折叠组缺头部或主体");
assert(foldBody.classList.contains("hidden"), "折叠组默认应收起");
assert(foldHead.getAttribute("aria-expanded") === "false", "折叠组头 aria-expanded 初始应为 false");
// 编排角色固定调用 id,第二轮 start 应按 restart 语义保留第一轮块并追加第二轮块。
// 活跃索引只指向当前轮,所以两次 ToolEnd 各自填充对应的当前块。
const inFold = foldBody.querySelectorAll(".tool-msg").length;
assert(inFold === 2, `architecture_scout 折叠组内应有 2 个工具块,实得 ${inFold}`);
assert(fold.querySelector(".agent-fold-count")?.textContent.includes("2"), "折叠组头未显示累计块数");
assert(foldBody.textContent.includes("勘察简报首行") && foldBody.textContent.includes("第二轮简报"),
  "跨轮同名调用的两次结果没有分别保留");
foldHead.click();
await flush();
assert(!foldBody.classList.contains("hidden"), "点击折叠组头未展开");
assert(foldHead.getAttribute("aria-expanded") === "true", "展开后 aria-expanded 应为 true");
assert(fold.querySelector(".agent-fold-caret")?.textContent === "▾", "展开后 caret 未变为 ▾");
// ⑤ 不同角色各自独立成组,不互相吞并。
for (const role of scoutRoles.slice(1)) {
  assert(document.querySelector(`.agent-fold[data-agent-role=${role}]`), `角色 ${role} 没有自己的折叠组`);
}

// ---------- D-727:复制上下文包含子代理折叠组 ----------
byId.get("copy-context").click();
await flush();
assert(
  copiedResearchCitation.includes("architecture_scout") &&
    copiedResearchCitation.includes("勘察简报首行") &&
    copiedResearchCitation.includes("第二轮简报"),
  "复制上下文漏掉子代理折叠组内的工具调用结果",
);

// ---------- D-237 活动面板:diff 汇总着色 + bash 完整输出可展开 ----------
const d237ToolStart = handlers.get("kz:tool-start");
const editEnd = handlers.get("kz:tool-end");
d237ToolStart({ payload: { id: "T5", name: "edit", summary: "ui/main.js", input: { path: "ui/main.js" }, sessionId: "sess-smoke" } });
d237ToolStart({ payload: { id: "T6", name: "bash", summary: "cargo test -p kanzei-app", input: { command: "cargo test -p kanzei-app" }, sessionId: "sess-smoke" } });
editEnd({ payload: { id: "T5", name: "edit", ok: true, preview: "replaced 1 occurrence", display: { kind: "diff", path: "ui/main.js", additions: 3, deletions: 1, language: "js", lines: [] }, sessionId: "sess-smoke" } });
editEnd({ payload: { id: "T6", name: "bash", ok: true, preview: "exit code: 0", display: { kind: "terminal", command: "cargo test -p kanzei-app", output: "短输出(截断版)", full: "长输出…".repeat(100) }, sessionId: "sess-smoke" } });
await flush();
const diffRow = document.querySelector("#diff-summary");
assert(diffRow && diffRow.textContent.includes("+3") && diffRow.textContent.includes("−1"), "diff 汇总未收录文件的增删计数");
// UI-0926 #10:目录已在上层行里,文件行只显示文件名;全路径在 dataset.path 与 title。
assert(
  [...diffRow.querySelectorAll(".diff-summary-row")].some((r) => r.dataset.path === "ui/main.js" && r.textContent.includes("main.js") && r.querySelector(".diff-summary-name")?.title === "ui/main.js"),
  "diff 汇总未显示文件路径",
);
// 冒烟的 innerHTML 是去标签近似(不建真实子节点),着色 span 的选择器断言不可用;
// 着色结构由 renderDiffSummary 的模板字符串保证(实际浏览器里 .diff-add/.diff-del 生效)。
const bashFullEntry = [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === "T6");
assert(bashFullEntry, "bash 完整输出条目未出现");
assert(
  bashFullEntry.querySelector(".bg-detail")?.textContent.includes("长输出"),
  "bash 展开区未使用完整输出(full),仍停留在 4000 截断版",
);
// R-133:diff 汇总为目录树——多路径文件按层级归入目录,目录行可折叠。
{
  // 再投两个分属不同目录的文件,验证树形分组而不是平铺。
  editEnd({ payload: { id: "T7", name: "edit", ok: true, preview: "replaced", display: { kind: "diff", path: "crates/kanzei-app/src/docs.rs", additions: 5, deletions: 2, language: "rust", lines: [] }, sessionId: "sess-smoke" } });
  editEnd({ payload: { id: "T8", name: "edit", ok: true, preview: "replaced", display: { kind: "diff", path: "crates/kanzei-tools/src/tracker.rs", additions: 1, deletions: 0, language: "rust", lines: [] }, sessionId: "sess-smoke" } });
  await flush();
  const tree = document.querySelector("#diff-summary .diff-tree");
  assert(tree, "diff 汇总未构建目录树容器");
  const dirs = tree.querySelectorAll(".diff-dir-head");
  assert(dirs.length >= 1, `diff 树缺少目录行(应有 crates/ 等目录),实得 ${dirs.length}`);
  const dirTexts = [...dirs].map((d) => d.textContent);
  assert(
    dirTexts.some((s) => s.includes("crates")),
    `diff 树目录未包含 crates:${dirTexts.join(",")}`,
  );
  const head = [...dirs].find((d) => d.textContent.includes("crates"));
  assert(head.getAttribute("aria-expanded") === "true", "diff 目录初始应展开");
  const fileRows = tree.querySelectorAll(".diff-summary-row");
  assert(
    [...fileRows].some((r) => r.dataset.path === "crates/kanzei-app/src/docs.rs" && r.textContent.startsWith("docs.rs") && r.closest(".diff-dir-body")),
    "diff 树文件行未归入目录下",
  );
  // 折叠交互:点目录头,子文件应隐藏。
  head._listeners.click?.forEach((fn) => fn({ target: head }));
  const collapsed = tree.querySelector(".diff-dir-body.hidden");
  assert(collapsed, "点击 diff 目录头未折叠子目录");
  assert(head.getAttribute("aria-expanded") === "false", "折叠后 aria-expanded 应为 false");
}

// ---------- 小工具降噪 + bash 实时输出 + rail 侧栏开合 ----------
// ① 成功小工具静默,失败仍由 bgFinishQuiet 补建条目(R-168)。
const quietBefore = document.querySelectorAll("#bg-list .bg-entry").length;
  toolStart({ payload: { id: "Q1", name: "read", summary: "crates/kanzei/src/main.rs", input: { path: "crates/kanzei/src/main.rs" }, sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "Q1", name: "read", ok: true, preview: "1 //! kz", display: null, sessionId: "sess-smoke" } });
await flush();
assert(document.querySelectorAll("#bg-list .bg-entry").length === quietBefore, "成功的 read 仍进了活动流(小工具降噪未生效)");
  toolStart({ payload: { id: "Q2", name: "req", summary: "update R-999", input: { action: "update" }, sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "Q2", name: "req", ok: false, preview: "找不到 R-999", display: null, sessionId: "sess-smoke" } });
await flush();
const quietErrEntry = [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === "Q2");
assert(quietErrEntry, "失败的静默工具没有补建条目(错误被吞掉)");
assert(quietErrEntry.classList.contains("err"), "补建的静默条目未标失败态");
// ② bash 增量输出:执行中逐段追加,收起时进度行跟到最后一行,结束后让位终态输出。
const toolProgress = handlers.get("kz:tool-progress");
assert(toolProgress, "工具增量输出事件未订阅");
  toolStart({ payload: { id: "S1", name: "bash", summary: "scripts/package.ps1", input: { command: "powershell scripts/package.ps1" }, sessionId: "sess-smoke" } });
  toolProgress({ payload: { id: "S1", chunk: "[1/6] 发布范围核对\n", sessionId: "sess-smoke" } });
  toolProgress({ payload: { id: "S1", chunk: "[4/6] cargo tauri build\n", sessionId: "sess-smoke" } });
await flush();
const streamEntry = [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === "S1");
assert(streamEntry?.querySelector(".bg-live")?.textContent.includes("[4/6]"), "bash 执行中未实时追加输出");
assert(streamEntry?.querySelector(".bg-prog")?.textContent.includes("[4/6]"), "收起状态的进度行未跟到最后一行");
  toolEnd({ payload: { id: "S1", name: "bash", ok: true, preview: "exit code: 0", display: { kind: "terminal", command: "powershell scripts/package.ps1", output: "全部完成", full: "全部完成" }, sessionId: "sess-smoke" } });
await flush();
assert(!streamEntry.querySelector(".bg-live"), "结束后实时流未让位给终态输出(同一份输出双份并存)");
// ③ rail 上的常驻侧栏开合:窄视口悬浮模式下顶栏开关会被盖住,rail 开关必须存在且可切换。
const railToggle = byId.get("rail-sidebar-toggle");
assert(railToggle, "activitybar 缺少常驻侧栏开合按钮");
const sidebarEl = byId.get("sidebar");
const collapsedBefore = sidebarEl.classList.contains("collapsed");
railToggle.click();
assert(sidebarEl.classList.contains("collapsed") !== collapsedBefore, "rail 开关没有切换侧栏");
railToggle.click();
assert(sidebarEl.classList.contains("collapsed") === collapsedBefore, "rail 开关未能再次切换回来");

// ---------- 主对话工具块:实时路径同样不双写 ----------
// 实时事件里的 preview 是后端 runner::preview 的单行摘要(首行 120 字 + " (+N lines)"),
// 本身就超过 ⎿ 行预算——双写在这条路径上是每次失败都能看见的。
{
  const toolMsgAt = (index) => document.querySelectorAll("#messages [data-active] .tool-msg")[index];
  const LIVE_PREVIEW =
    "old_string 未命中:crates/kanzei-tools/src/edit.rs:202-209 的空白与换行与磁盘上的内容不一致," +
    "请改用插入式替换,或者先确认 allow_deletion 这个参数的语义之后再重试一次 (+2 lines)";
  assert(LIVE_PREVIEW.length > 120, `夹具失效:实时 preview 必须超过 ⎿ 行预算才验得到双写,实得 ${LIVE_PREVIEW.length} 字`);

  // ① 失败的长摘要:同一段文案在一个块里只能出现一次。
  let index = document.querySelectorAll("#messages [data-active] .tool-msg").length;
  toolStart({ payload: { id: "X1", name: "edit", summary: "crates/kanzei-tools/src/edit.rs", input: { path: "crates/kanzei-tools/src/edit.rs" }, sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "X1", name: "edit", ok: false, preview: LIVE_PREVIEW, display: null, sessionId: "sess-smoke" } });
  await flush();
  const x1 = toolMsgAt(index);
  assert(x1, "实时失败工具块未建出");
  const needle = LIVE_PREVIEW.slice(0, 60);
  assert(
    x1.textContent.split(needle).length - 1 === 1,
    `工具块把同一段结果文案渲染了两遍(D-237 同族回归):出现 ${x1.textContent.split(needle).length - 1} 次`,
  );
  // 但被截掉的后半句必须仍读得到——不允许用「干脆不给 detail」的方式消灭重复。
  // 判据比 includes 更硬:去掉两端的续接省略号后,摘要 + 剩余必须**逐字**拼回原文
  // ——既不丢字(详情被砍掉),也不重字(双写复发)。
  const x1Head = (x1.querySelector(".tool-msg-result")?.textContent ?? "").replace(/^⎿ /, "").replace(/…$/, "");
  const x1Rest = (x1.querySelector(".tool-msg-raw")?.textContent ?? "").replace(/^…/, "");
  assert(
    x1Head + x1Rest === LIVE_PREVIEW,
    `摘要 + 详情拼不回原文(丢字或重字):摘要 ${x1Head.length} 字 + 详情 ${x1Rest.length} 字 vs 原文 ${LIVE_PREVIEW.length} 字`,
  );
  assert(x1Rest.endsWith("(+2 lines)"), `被截掉的尾巴读不到了:"${x1Rest.slice(-30)}"`);

  // ② 成功的短结果:⎿ 行是人话摘要(UI-0926 #6),仍不出展开区,不加 has-detail。
  index = document.querySelectorAll("#messages [data-active] .tool-msg").length;
  toolStart({ payload: { id: "X2", name: "edit", summary: "ui/x.js", sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "X2", name: "edit", ok: true, preview: "replaced 1 occurrence", display: null, sessionId: "sess-smoke" } });
  await flush();
  const x2 = toolMsgAt(index);
  assert(x2?.querySelector(".tool-msg-result")?.textContent === "⎿ 已替换 1 处", `成功短结果的 ⎿ 行变了:"${x2?.querySelector(".tool-msg-result")?.textContent}"`);
  assert(x2.querySelector(".tool-msg-raw") === null, "成功短结果不该出展开区(展开了还是那一行 = 假承诺)");
  assert(!x2.classList.contains("has-detail"), "成功短结果不该标 has-detail");

  // 结构化终态:no-op/受控拒绝/真实故障必须三分，不能继续全部画成红叉。
  index = document.querySelectorAll("#messages [data-active] .tool-msg").length;
  toolStart({ payload: { id: "XNOOP", name: "edit", summary: "ui/noop.js", sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "XNOOP", name: "edit", ok: false, outcome: "noop", code: "EDIT_IDENTICAL_INPUT", preview: "无需修改", display: null, sessionId: "sess-smoke" } });
  await flush();
  const xNoop = toolMsgAt(index);
  assert(xNoop.classList.contains("noop") && !xNoop.classList.contains("err"), "no-op 仍被渲染成真实失败");
  assert(xNoop.querySelector(".tool-msg-status")?.textContent === "↪", "no-op 缺少独立形状标记");

  index = document.querySelectorAll("#messages [data-active] .tool-msg").length;
  toolStart({ payload: { id: "XWARN", name: "edit", summary: "ui/warn.js", sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "XWARN", name: "edit", ok: false, outcome: "needs_correction", code: "EDIT_ANCHOR_NOT_FOUND", preview: "请重读锚点", display: null, sessionId: "sess-smoke" } });
  await flush();
  const xWarn = toolMsgAt(index);
  assert(xWarn.classList.contains("warn") && !xWarn.classList.contains("err"), "受控拒绝仍被渲染成真实失败");
  assert(xWarn.querySelector(".tool-msg-status")?.textContent === "⚠", "受控拒绝缺少警告形状标记");
  const warnActivity = document.querySelector("#bg-list .bg-entry[data-bg-id=XWARN]");
  assert(warnActivity?.dataset.bgStatus === "warn", "活动栏没有保留受控拒绝终态");

  index = document.querySelectorAll("#messages [data-active] .tool-msg").length;
  toolStart({ payload: { id: "XFAIL", name: "edit", summary: "ui/fail.js", sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "XFAIL", name: "edit", ok: false, outcome: "failed", code: "EDIT_WRITE_FAILED", preview: "磁盘写入失败", display: null, sessionId: "sess-smoke" } });
  await flush();
  const xFail = toolMsgAt(index);
  assert(xFail.classList.contains("err"), "真实执行故障没有保留失败态");
  assert(xFail.querySelector(".tool-msg-status")?.textContent === "✗", "真实执行故障图标漂移");

  // 工具结果存储配额截断(审计发现 12):后端保留 bash 的 terminal display 并附 quota_truncated。
  // 对话工具块与活动面板都必须同时有终端块和配额提示块(已用/配额、原因、整理入口),
  // ⎿ 行/进度行换成按原因区分的人话,不再是 [tool_result_truncated …] 机器标记;
  // ⎿ 行已换掉时,对话详情也不能再挂机器标记被切剩的孤立尾巴(非入参的 .tool-msg-raw)。
  const quotaLinesOk = (id, chatBlock, headline, where) => {
    const result = chatBlock?.querySelector(".tool-msg-result")?.textContent ?? "";
    assert(result.includes(headline) && !result.includes("tool_result_truncated"), `${where}对话 ⎿ 行不是按原因的人话:"${result}"`);
    const orphan = [...(chatBlock?.querySelectorAll(".tool-msg-raw") ?? [])].filter((n) => !n.classList.contains("args"));
    assert(orphan.length === 0, `${where}对话详情仍挂着机器标记的孤立尾巴:"${orphan[0]?.textContent}"`);
    const activity = document.querySelector(`#bg-list .bg-entry[data-bg-id=${id}]`);
    assert(activity, `${where}未进活动面板`);
    const prog = activity.querySelector(".bg-prog")?.textContent ?? "";
    assert(prog.includes(headline) && !prog.includes("tool_result_truncated"), `${where}活动面板进度行不是按原因的人话:"${prog}"`);
    return activity;
  };
  index = document.querySelectorAll("#messages [data-active] .tool-msg").length;
  const quotaMarker = `[tool_result_truncated reason=artifact_quota_exceeded bytes=1048700 storage_used_bytes=1932735283 quota_bytes=2147483648 sha256=${"a".repeat(64)}]`;
  toolStart({ payload: { id: "XQUOTA", name: "bash", summary: "cargo test -p kanzei-core", input: { command: "cargo test -p kanzei-core" }, sessionId: "sess-smoke" } });
  toolEnd({ payload: {
    id: "XQUOTA", name: "bash", ok: true, preview: `${quotaMarker.slice(0, 120)} (+3 lines)`,
    display: {
      kind: "terminal", command: "cargo test -p kanzei-core", exitCode: 0,
      output: "running 12 tests\ntest result: ok. 12 passed", full: "running 12 tests\ntest result: ok. 12 passed",
      quota_truncated: { reason: "artifact_quota_exceeded", storage_used_bytes: 1932735283, quota_bytes: 2147483648 },
    },
    sessionId: "sess-smoke",
  } });
  await flush();
  const xQuota = toolMsgAt(index);
  const quotaNoticeOk = (root, where) => {
    // 活动面板展开区还有入参块(.bg-args 同为 .tool-display.term),按 "$ 命令" 头认终端块。
    const term = [...(root?.querySelectorAll(".tool-display.term") ?? [])].find((n) => n.textContent.startsWith("$ cargo test -p kanzei-core"));
    assert(term?.textContent.includes("12 passed"), `${where}配额截断后终端块丢失(原 terminal display 被覆盖)`);
    const notice = root?.querySelector(".quota-notice")?.textContent ?? "";
    assert(notice.includes("1.80 GB") && notice.includes("2.00 GB"), `${where}配额提示缺少已用/配额:"${notice}"`);
    assert(notice.includes("artifact_quota_exceeded"), `${where}配额提示缺少截断原因:"${notice}"`);
    assert(notice.includes("删除并安全整理"), `${where}配额提示没有指向存储整理入口:"${notice}"`);
  };
  quotaNoticeOk(xQuota?.querySelector(".tool-msg-detail"), "对话工具块:");
  const quotaActivity = quotaLinesOk("XQUOTA", xQuota, "工具结果存储已满", "配额截断的 bash:");
  quotaNoticeOk(quotaActivity.querySelector(".bg-detail"), "活动面板:");

  // 原工具没有 display 时后端发 kind=truncated:preview 单独成终端样式块,后面紧跟配额提示。
  index = document.querySelectorAll("#messages [data-active] .tool-msg").length;
  const truncMarker = `[tool_result_truncated reason=artifact_quota_exceeded bytes=3145728 storage_used_bytes=2040109466 quota_bytes=2147483648 sha256=${"b".repeat(64)}]`;
  toolStart({ payload: { id: "XQTRUNC", name: "process", summary: "list", input: { action: "list" }, sessionId: "sess-smoke" } });
  toolEnd({ payload: {
    id: "XQTRUNC", name: "process", ok: true, preview: `${truncMarker.slice(0, 120)} (+40 lines)`,
    display: {
      kind: "truncated", reason: "artifact_quota_exceeded", bytes: 3145728,
      storage_used_bytes: 2040109466, quota_bytes: 2147483648, sha256: "b".repeat(64),
      preview: "bg1 running cargo watch\nbg2 exited npm run dev",
    },
    sessionId: "sess-smoke",
  } });
  await flush();
  const xTrunc = toolMsgAt(index);
  const truncNoticeOk = (root, where) => {
    const previewBlock = [...(root?.querySelectorAll(".tool-display.term") ?? [])].find((n) => n.textContent.startsWith("bg1 running cargo watch"));
    assert(previewBlock?.textContent.includes("bg2 exited npm run dev"), `${where}kind=truncated 的 preview 没有单独成块`);
    const notice = root?.querySelector(".quota-notice")?.textContent ?? "";
    assert(notice.includes("工具结果存储已满"), `${where}kind=truncated 缺少配额提示:"${notice}"`);
    assert(notice.includes("1.90 GB") && notice.includes("2.00 GB"), `${where}kind=truncated 配额提示缺少已用/配额:"${notice}"`);
    assert(notice.includes("删除并安全整理"), `${where}kind=truncated 配额提示没有指向存储整理入口:"${notice}"`);
  };
  truncNoticeOk(xTrunc?.querySelector(".tool-msg-detail"), "对话工具块:");
  const truncActivity = quotaLinesOk("XQTRUNC", xTrunc, "工具结果存储已满", "kind=truncated 的 process:");
  truncNoticeOk(truncActivity.querySelector(".bg-detail"), "活动面板:");

  // 锁繁忙/无法计量不是"存储已满":不许说已满、不许把未计量的占用画成 "0 B"、
  // 更不许把用户往不可逆的删除历史上推(审计 F5 复审 major)。
  const transientQuotaCases = [
    {
      id: "XQLOCK", name: "bash", input: { command: "cargo build" },
      preview: `[tool_result_truncated reason=quota_lock_unavailable bytes=2097152 storage_used_bytes=unknown quota_bytes=2147483648 sha256=${"c".repeat(64)}] (+5 lines)`,
      display: {
        kind: "terminal", command: "cargo build", exitCode: 0, output: "Compiling kanzei-core\nFinished dev", full: "Compiling kanzei-core\nFinished dev",
        quota_truncated: { reason: "quota_lock_unavailable", storage_used_bytes: null, quota_bytes: 2147483648 },
      },
      headline: "存储锁繁忙", reason: "quota_lock_unavailable", hint: "稍后重试",
    },
    {
      id: "XQMEASURE", name: "process", input: { action: "discover" },
      preview: `[tool_result_truncated reason=quota_unmeasurable bytes=2097152 storage_used_bytes=unknown quota_bytes=2147483648 sha256=${"d".repeat(64)}] (+9 lines)`,
      // storage_used_bytes 缺省(undefined)与 null 同样要显示「未知」。
      display: { kind: "truncated", reason: "quota_unmeasurable", bytes: 2097152, quota_bytes: 2147483648, preview: "pid 42 node dev-server" },
      headline: "无法计量工具结果存储", reason: "quota_unmeasurable", hint: "且其中没有符号链接后重试",
    },
  ];
  for (const c of transientQuotaCases) {
    index = document.querySelectorAll("#messages [data-active] .tool-msg").length;
    toolStart({ payload: { id: c.id, name: c.name, summary: c.id, input: c.input, sessionId: "sess-smoke" } });
    toolEnd({ payload: { id: c.id, name: c.name, ok: true, preview: c.preview, display: c.display, sessionId: "sess-smoke" } });
    await flush();
    const chatBlock = toolMsgAt(index);
    const activity = quotaLinesOk(c.id, chatBlock, c.headline, `${c.reason}:`);
    for (const [root, where] of [[chatBlock?.querySelector(".tool-msg-detail"), "对话工具块"], [activity.querySelector(".bg-detail"), "活动面板"]]) {
      const notice = root?.querySelector(".quota-notice")?.textContent ?? "";
      const tag = `${c.reason} ${where}:`;
      assert(notice.includes(c.headline) && notice.includes(c.reason), `${tag}提示块缺少按原因的标题/原因码:"${notice}"`);
      assert(notice.includes("未知") && !notice.includes("0 B"), `${tag}未计量的已用空间必须显示「未知」,不能画成 0 B:"${notice}"`);
      assert(notice.includes("2.00 GB"), `${tag}配额丢失:"${notice}"`);
      assert(notice.includes(c.hint), `${tag}缺少重试方向:"${notice}"`);
      assert(!notice.includes("存储已满") && !notice.includes("删除并安全整理"), `${tag}暂态截断被说成存储已满/给了删除建议:"${notice}"`);
    }
    for (const line of [chatBlock?.querySelector(".tool-msg-result")?.textContent ?? "", activity.querySelector(".bg-prog")?.textContent ?? ""]) {
      assert(!line.includes("存储已满"), `${c.reason}:⎿/进度行被说成存储已满:"${line}"`);
    }
  }

  // ③ ⎿ 行截断点与剩余部分的切分必须严丝合缝:一个字要么在摘要里、要么在详情里。
  // UI-0926 #6:成功行改成人话摘要后,互斥切分只作用于失败行,预算用失败结果验。
  index = document.querySelectorAll("#messages [data-active] .tool-msg").length;
  toolStart({ payload: { id: "X3", name: "edit", summary: "ui/y.js", sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "X3", name: "edit", ok: false, preview: "x".repeat(200), display: null, sessionId: "sess-smoke" } });
  await flush();
  const x3 = toolMsgAt(index);
  const x3Result = x3?.querySelector(".tool-msg-result")?.textContent ?? "";
  assert(x3Result.length === 112 && x3Result.endsWith("…"), `⎿ 行预算漂移(应为 "⎿ " + 109 字 + "…" = 112),实得 ${x3Result.length}`);
  assert(
    x3.querySelector(".tool-msg-raw")?.textContent === `…${"x".repeat(91)}`,
    `剩余部分与截断点对不上(会漏字或重字):"${x3.querySelector(".tool-msg-raw")?.textContent?.slice(0, 20)}"`,
  );
}

// ---------- 活动栏条目标题:按工具挑字段,不是后端那坨入参 JSON ----------
// 后端 summarize_input(kanzei-core/src/runner/compaction.rs:251)把整个入参 JSON 截到
// 160 字,对所有工具一视同仁——edit 于是显示成 `{"new_string":"…","old_strin…`,
// 完全看不出改的是哪个文件(用户截图)。前端标题必须走 toolCallSummary 挑字段。
{
  const bgEntry = (id) => [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === id);
  // ① 终端直入列:后端 summary 是裸 JSON,标题必须显示命令本身。
  toolStart({ payload: { id: "BGJ1", name: "bash", summary: '{"command":"cargo test -p kanzei-app","workdir":"."}', input: { command: "cargo test -p kanzei-app", workdir: "." }, sessionId: "sess-smoke" } });
  await flush();
  const j1 = bgEntry("BGJ1");
  assert(j1, "终端条目未入列");
  const j1Target = j1.querySelector(".bg-target")?.textContent ?? "";
  assert(j1Target === "cargo test -p kanzei-app", `活动栏标题回到了后端整坨入参 JSON:"${j1Target.slice(0, 60)}"`);
  assert(!j1Target.startsWith("{") && !j1Target.includes('"command"'), "活动栏标题里还留着 JSON 语法");
  // ② 悬浮提示同步:鼠标停上去看到的必须是同一份人类可读文本。
  assert(
    j1.querySelector(".bg-title")?.title === j1Target,
    `悬浮提示仍是裸 JSON 或与标题不一致:"${j1.querySelector(".bg-title")?.title?.slice(0, 60)}"`,
  );

  // ③ 失败补建路径(复现用户截图那条):edit 的 summary 是 new_string/old_string 的整坨 JSON。
  toolStart({ payload: { id: "BGJ2", name: "edit", summary: '{"new_string":"pub fn append_episode","old_string":"old"}', input: { path: "crates/kanzei-core/src/store.rs", new_string: "pub fn append_episode", old_string: "old" }, sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "BGJ2", name: "edit", ok: false, preview: "old_string not found", display: null, sessionId: "sess-smoke" } });
  await flush();
  const j2 = bgEntry("BGJ2");
  assert(j2, "失败的静默工具没有补建条目(R-168 回归)");
  assert(j2.classList.contains("err"), "补建的失败条目未标失败态");
  const j2Target = j2.querySelector(".bg-target")?.textContent ?? "";
  assert(j2Target === "crates/kanzei-core/src/store.rs", `edit 条目标题不是文件路径:"${j2Target.slice(0, 60)}"`);
  assert(!j2Target.includes("new_string") && !j2Target.startsWith("{"), "edit 条目标题里还留着入参 JSON");
  assert(j2.querySelector(".bg-title")?.title === j2Target, "edit 条目的悬浮提示与标题不一致");
  // 活动按钮本身展示人类可读目标,不能回退到后端整坨 JSON。
  assert(j2Target.includes("store.rs") && !j2Target.includes("new_string"), "活动条目目标仍是裸 JSON");
  assert(
    !listText("log-lines").includes('"new_string"'),
    "运行日志里仍直接拼后端 summary(edit 在日志里还是一坨入参 JSON)",
  );
  // summary 缺省时不能抛:事件里 summary 并非必填,`summary.slice()` 会把整条事件链打断。
  // D-729:read 成功路径已按 R-168 静默,所以这里用**失败的 read**探同一件事——
  // 它同时覆盖了 bgFinishQuiet 的补建路径与 summary 缺省的回落链,比原来只探回落更严。
  toolStart({ payload: { id: "BGJ2b", name: "read", input: { path: "crates/kanzei/src/main.rs" }, sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "BGJ2b", name: "read", ok: false, preview: "permission denied", display: null, sessionId: "sess-smoke" } });
  await flush();
  assert(
    bgEntry("BGJ2b")?.querySelector(".bg-target")?.textContent.includes("crates/kanzei/src/main.rs"),
    "summary 缺省时活动条目未回落到入参挑字段",
  );

  // ④ 回落链第二级:挑不出字段就用后端 summary(回放事件不带 input,靠的就是这一级)。
  toolStart({ payload: { id: "BGJ3", name: "bash", summary: "人类可读的后端摘要", input: {}, sessionId: "sess-smoke" } });
  await flush();
  assert(
    bgEntry("BGJ3")?.querySelector(".bg-target")?.textContent === "人类可读的后端摘要",
    `挑不出字段时未回落后端 summary:"${bgEntry("BGJ3")?.querySelector(".bg-target")?.textContent}"`,
  );

  // ⑤ 回落链第三级:两级都空就是空标题,不能抛异常、也不能凭空编一句。
  toolStart({ payload: { id: "BGJ4", name: "bash", summary: "", input: {}, sessionId: "sess-smoke" } });
  await flush();
  const j4 = bgEntry("BGJ4");
  assert(j4, "summary 与 input 都为空时条目建不出来了");
  assert(j4.querySelector(".bg-target")?.textContent === "", "空标题被填了兜底文案");
  assert((j4.querySelector(".bg-title")?.title ?? "") === "", "空标题的悬浮提示不该有内容");

  // ⑥ diff 终态不得把两段式标题拍平:对整个 title 按钮做 textContent += 会把
  // .bg-tool/.bg-target 两个 span 压成单个文本节点,工具名/目标的分栏当场消失。
  toolStart({ payload: { id: "BGJ5", name: "edit", summary: "x.rs", input: { path: "x.rs" }, sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "BGJ5", name: "edit", ok: false, preview: "写坏了", display: { kind: "diff", path: "x.rs", additions: 3, deletions: 1, language: "rust", lines: [] }, sessionId: "sess-smoke" } });
  await flush();
  const j5 = bgEntry("BGJ5");
  assert(j5?.querySelector(".bg-tool") && j5?.querySelector(".bg-target"), "diff 终态把 .bg-tool/.bg-target 两段式结构拍平了(工具名/目标的分栏消失)");
  const j5Target = j5?.querySelector(".bg-target")?.textContent ?? j5?.textContent ?? "";
  assert(j5Target.includes("x.rs") && j5Target.includes("+3"), `diff 增删数未追加到目标列:"${j5Target.slice(0, 60)}"`);

  // ⑦ 「重跑」填回输入框的首行也不能是裸 JSON(entry.summary 存的是显示值)。
  toolEnd({ payload: { id: "BGJ1", name: "bash", ok: true, preview: "test result: ok", display: null, sessionId: "sess-smoke" } });
  await flush();
  const rerun = [...bgEntry("BGJ1").querySelectorAll(".bg-actions button")].find((b) => b.textContent === "重跑");
  assert(rerun, "结束的终端条目缺少重跑入口");
  rerun.click();
  await flush();
  const promptValue = byId.get("prompt").value;
  assert(
    promptValue.split("\n")[0] === "重跑这次调用:bash cargo test -p kanzei-app",
    `重跑填词首行仍是裸 JSON:"${promptValue.split("\n")[0]}"`,
  );
  assert(promptValue.includes("workdir"), "重跑填词丢了完整入参(只剩一行摘要就没法复核参数)");
  byId.get("prompt").value = "";

  // ⑧ 回放路径不能被改坏:回放事件不带 input,标题只能来自后端 summary。
  toolEnd({ payload: { id: "BGJ3", name: "bash", ok: true, preview: "done", display: null, sessionId: "sess-smoke" } });
  toolEnd({ payload: { id: "BGJ4", name: "bash", ok: true, preview: "done", display: null, sessionId: "sess-smoke" } });
  await flush();
  sandbox.renderRecoveredTraces([{
    events: [
      { id: "RP1", kind: "tool.started", name: "bash", summary: "scripts/verify.ps1" },
      { id: "RP1", kind: "tool.completed", ok: true, durationMs: 1200 },
      { id: "RP2", kind: "tool.started", name: "edit", summary: "历史失败调用" },
      { id: "RP2", kind: "tool.completed", ok: false, error: "boom" },
      // name 缺失的回放事件没有可读身份,不应建出空壳条目。
      { id: "RP3", kind: "tool.started" },
    ],
  }]);
  await flush();
  const rp1 = bgEntry("RP1");
  assert(rp1?.querySelector(".bg-target")?.textContent === "scripts/verify.ps1", `回放条目标题不对:"${rp1?.querySelector(".bg-target")?.textContent}"`);
  assert(rp1?.querySelector(".bg-tool")?.textContent === "bash", "回放条目未分列工具名");
  // D-208 不变量:回放条目是历史,不能显示成运行中、也不能给停止按钮。
  assert(!rp1.classList.contains("running"), "回放条目被标成运行中");
  assert(!rp1.querySelectorAll(".bg-actions button").some((b) => b.textContent === "停止"), "回放条目不该有停止按钮");
  const rp2 = bgEntry("RP2");
  assert(rp2, "回放里失败的静默工具未补建条目");
  assert(rp2.classList.contains("err"), "回放补建的条目未标失败态");
  assert(rp2.querySelector(".bg-target")?.textContent === "历史失败调用", `回放失败条目标题不对:"${rp2.querySelector(".bg-target")?.textContent}"`);
  assert(!bgEntry("RP3"), "name 缺失的回放事件不该建出条目(它走静默通道,建了就是一条没有信息量的空壳)");
}

// ---------- D-280 回归:清空消息区不得连「回到最新」按钮一起清掉 ----------
// 2026-08-12 实测事故:D-280 把 #jump-latest 挪进了 #messages 里,而
// renderRecoveredMessages / clearChat 都做 `messages.innerHTML = ""`——
// 一清按钮就没了,之后任何滚动/渲染触发 updateLatestButton 都抛
// `Cannot read properties of null (reading 'classList')`,恢复历史与新建
// 并行线路整条链路当场崩掉。按钮必须是滚动容器的**兄弟**,不是它的孩子。
{
  // 冒烟的 DOM 是按 id 摊平造的(按钮一律挂在 body 上),父子关系在这里测不出来,
  // 所以结构断言直接查 index.html 源文本:#messages 的开闭标签之间不许出现按钮。
  assert(byId.get("chat-area"), "缺少 #chat-area:「回到最新」需要一个不滚动的定位容器做锚点");
  const messagesOpen = html.indexOf('<section id="messages">');
  const messagesClose = html.indexOf("</section>", messagesOpen);
  assert(messagesOpen >= 0 && messagesClose > messagesOpen, "index.html 里找不到 #messages 区块");
  assert(
    !html.slice(messagesOpen, messagesClose).includes('id="jump-latest"'),
    "「回到最新」按钮又被放进 #messages 了:renderRecoveredMessages/clearChat 会 " +
      "`messages.innerHTML = \"\"`,一清就把它删掉,之后 updateLatestButton 抛 null.classList",
  );
  // 行为面:两条清空路径都不得抛异常(抛了会被 __reportInitError/console.error 抓住)。
  sandbox.clearChat("新对话");
  await flush();
  sandbox.renderRecoveredMessages([]);
  await flush();
}

// ---------- D-170 项目隔离失效必须报出来 ----------
assert(invokeLog.includes("project_root_info"), "切项目时未检查项目根是否与所选目录一致");
const sharedWarn = byId.get("project-shared-warn");
assert(sharedWarn, "缺少项目隔离告警位");
assert(!sharedWarn.classList.contains("hidden"), "所选目录与实际根不一致却没有告警(需求会在项目间串)");
assert(listText("project-shared-warn").includes("C:/smoke/parent"), "告警未给出实际生效的根");
const detachBtn = sharedWarn.querySelector("button");
assert(detachBtn, "缺少一键建立独立空间");
detachBtn.click();
await flush();
assert(invokeLog.includes("project_detach"), "点了建立独立空间却没调后端");

// ---------- D-169 列表被筛空必须说破，不能留一片空白 ----------
// 持久化的标签在当前项目可能不存在:下拉回落成"全部"而状态没跟着回落,
// 列表就被一个看不见的条件筛空——用户看到的是"需求凭空掉了"。
// 走真实持久化路径:把一个当前项目里不存在的标签写进偏好,再触发恢复与重绘。
// 前置:上面的用例把标签页停在「对照」,而对照页按既有设计只提供「全部状态」一个选项
// (12-docs-pages.js:126/130),直接给状态筛选赋 "dropped" 会被 <select> 规范语义拒绝
// (无匹配 option → selectedIndex=-1 → value 变空串),整组断言会以"筛不动"的形态假失败。
byId.get("documents-tab-req").click();
await flush();
// 旧结构种子(顶层 req 而非 docReq):这是 10-docs-core.js:34-35 降级读取的真实用例,
// R-115 的筛选偏好在这次搬迁中不能丢。不要顺手改成 docReq。
const filtersKey = [...storage.keys()].find((k) => k.startsWith("kz-filters")) ?? "kz-filters:C:/smoke/project";
storage.set(filtersKey, JSON.stringify({ req: { tag: "这个标签不存在", status: "all", priority: "all", complexity: "all", blocked: "all", sort: "manual" } }));
sandbox.restoreDocFilters();
await sandbox.refreshDocs();
await flush();
// 不变量:列表不得"无声变空"。要么标签回落后条目照常显示,要么明说被筛掉了多少。
assert(
  document.querySelectorAll("#documents-req-list .doc-item").length > 0
    || document.querySelector("#documents-req-list .doc-filtered-empty"),
  "不存在的标签把列表筛空了,且界面没有任何说明——看起来就是需求凭空掉了",
);
assert(
  document.querySelectorAll("#documents-req-list .doc-item").length > 0,
  "当前项目没有这个标签,筛选状态应回落成「全部」而不是筛空",
);

// 真实存在但无匹配的筛选:验证"被筛空"的提示与一键清除。
const statusFilterEl = byId.get("documents-status-filter");
statusFilterEl.value = "dropped";
statusFilterEl._listeners.change?.forEach((fn) => fn({ target: statusFilterEl }));
await flush();
const filteredEmpty = document.querySelector("#documents-req-list .doc-filtered-empty");
assert(filteredEmpty, "列表被筛空却没有任何说明(一片空白最容易被当成数据丢失)");
assert(/\d/.test(filteredEmpty.textContent), "未给出被隐藏的条数");
const clearFiltersBtn = filteredEmpty.querySelector("button");
assert(clearFiltersBtn, "被筛空时缺少一键清除筛选");
clearFiltersBtn.click();
await flush();
assert(
  document.querySelectorAll("#documents-req-list .doc-item").length > 0,
  "点了清除筛选,条目没有回来",
);

// ---------- D-168 设置页模型角色：可选、不丢已存值、被覆盖时明示 ----------
const settingsTab = document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "settings");
settingsTab?.click();
await flush();
assert(byId.get("agent-directory"), "设置页缺少 Agent 目录容器");
assert(invokeLog.includes("agent_directory_get"), "设置页加载未读取 Agent 目录 IPC");
assert(document.querySelectorAll(".agent-directory-card").length === 2, "Agent 目录未渲染内建与项目 Agent 卡片");
const agentOpenButton = document.querySelector(".agent-directory-card button");
assert(agentOpenButton, "项目 Agent 卡片缺少打开原文按钮");
agentOpenButton.click();
await flush();
assert(invokeLog.includes("agent_directory_open"), "打开原文按钮未调用 Agent 原文 IPC");
const primarySelect = byId.get("set-primary");
assert(primarySelect.tagName === "SELECT", "模型角色仍是自由文本框(手打 provider:model 太容易拼错)");
const primaryValues = [...primarySelect.options].map((o) => o.value);
assert(primaryValues.includes(""), "缺少「未设」选项(不该强迫用户必须指定一个模型)");
assert(primaryValues.includes("__manual__"), "缺少手填兜底");
assert(
  primaryValues.some((v) => v.includes(":")),
  `未把探测到的模型灌进下拉,实得: ${primaryValues.join(",")}`,
);
// 已保存的值若探测不到,必须原样保留 —— 否则一进设置页就被悄悄改掉,保存一次配置就坏了。
assert(
  primarySelect.value === "deepseek:deepseek-chat",
  `已保存的模型未被保留,实得 "${primarySelect.value}"`,
);
assert(
  primaryValues.includes("deepseek:deepseek-chat"),
  "探测不到的已存值未补进选项列表",
);
// 项目级覆盖必须明说。
assert(
  !byId.get("settings-effective").classList.contains("hidden"),
  "项目级覆盖了 primary,但设置页没有任何提示",
);
assert(listText("settings-effective").includes("实际生效"), "覆盖提示未说明实际生效值");
// D-503:设置页后端失败不能再静默停留旧值或让用户误以为刷新成功。
{
  invokeFailures.set("models_list", "模拟模型列表失败");
  expectedPersistentError = "模型列表获取失败";
  const persistentBefore = expectedPersistentHits;
  await sandbox.probeModelsAndMergeOptions(0);
  assert(
    expectedPersistentHits === persistentBefore + 1,
    "models_list 失败未经过持久错误反馈出口",
  );
  assert(
    !byId.get("log-panel").classList.contains("hidden"),
    "models_list 失败后日志面板未显示",
  );
  expectedPersistentError = null;
  invokeFailures.delete("models_list");

  invokeFailures.set("fast_model_status", "模拟 fast 状态失败");
  await sandbox.refreshFastStatus();
  assert(
    listText("fast-status").includes("快速模型状态获取失败") && byId.get("fast-setup").classList.contains("hidden"),
    "fast_model_status 失败未显示状态行反馈或仍暴露未知状态下的安装按钮",
  );
  invokeFailures.delete("fast_model_status");
  await sandbox.refreshFastStatus();
}
// 表单脏状态:改一下就该出现「未保存」。
assert(byId.get("settings-dirty").classList.contains("hidden"), "刚载入时不该显示未保存");
const fastSelect = byId.get("set-fast");
fastSelect.value = "";
fastSelect.dispatchEvent({ type: "change" });
assert(
  !byId.get("settings-dirty").classList.contains("hidden"),
  "改了表单却没有「未保存」提示(界面显示 A、运行用 B 就是这么来的)",
);

// 脏值守卫:表单有未保存改动时,再来一次 loadSettings 不得用磁盘值覆盖它。
// 这正是用户报的「正在填配置,页面突然刷新了」——旧实现里 loadSettings 中间挂着
// 一段数秒的模型探测 await,resolve 后整表重建,输入连同焦点一起消失,而且紧接着
// markSettingsSaved 把这个被抹平的状态记成干净基线,「未保存」角标从头到尾没亮过。
{
  const dirtyValue = "留住我";
  const url = document.querySelector("#providers-table tbody input");
  if (url) {
    url.value = dirtyValue;
    url.dispatchEvent({ type: "input" });
  }
  await sandbox.loadSettings();
  await flush();
  assert(
    !url || url.value === dirtyValue,
    `表单有未保存改动时 loadSettings 不得覆盖输入,实得 "${url?.value}"`,
  );
  assert(
    !byId.get("settings-stale")?.classList.contains("hidden"),
    "拦下一次回填却不说原因,用户只会以为界面卡住了",
  );
  // 丢弃改动 = 显式放行覆盖,这条退路必须真的能走通。
  byId.get("settings-discard")?.dispatchEvent({ type: "click" });
  await flush();
  assert(
    byId.get("settings-stale")?.classList.contains("hidden"),
    "丢弃改动后提示条应消失(否则用户不知道自己已经回到干净态)",
  );
}

// D-420:设置页三个角色的手填模型也必须使用应用内输入弹窗。
sandbox.__inputDialogResponses.push("anthropic:claude-sonnet-5");
primarySelect.value = "__manual__";
primarySelect.dispatchEvent({ type: "change" });
await flush();
assert(primarySelect.value === "anthropic:claude-sonnet-5", "设置页手填模型未消费输入弹窗值");
sandbox.__inputDialogResponses.push("不是模型");
primarySelect.value = "__manual__";
primarySelect.dispatchEvent({ type: "change" });
await flush();
assert(primarySelect.value === "anthropic:claude-sonnet-5", "设置页非法模型输入未回退到上一次值");

// 工具图标分类覆盖率:真实存在的工具一个都不许落到兜底扳手。清单来自 crates/kanzei-tools
// 各 Tool 实现的 fn name()——后端加了新工具而前端忘了归类,这条会红,而不是悄悄画个扳手。
{
  const realTools = [
    "read", "files", "symbols", "write", "edit", "insert", "bash", "process",
    "grep", "glob", "git", "webfetch", "websearch", "work",
    "test_record", "architecture", "conventions", "question", "task",
    "req", "defect", "idea", "decision", "source", "finding",
    "frontend_locate", "frontend_check", "memory_note", "memory_search",
    "ui_dom", "ui_console", "ui_style",
  ];
  const fellBack = realTools.filter((name) => sandbox.toolGroupEntry(name)[1] === "wrench");
  assert(
    fellBack.length === 0,
    `这些真实工具没有归类,落到了兜底扳手:${fellBack.join(", ")}`,
  );
  // 反证:没见过的工具**应该**落兜底,不能因为前缀匹配太宽而误判。
  assert(sandbox.toolGroupEntry("something_new")[1] === "wrench", "未知工具应落兜底图标");
}

assert(html.includes('id="set-codex-fast-mode"'), "设置页缺少 Codex Fast mode 开关标记");
// 运行上限([limits]):读、存、脏状态三条线缺一条就是"界面显示 A、运行用 B"(D-157)。
{
  const ids = ["set-max-tokens", "set-subagent-max-tokens", "set-subagent-timeout", "set-max-tasks",
    "set-context-ratio", "set-verbatim-ratio", "set-max-parallel", "set-stream-restarts",
    "set-transport-retries", "set-rate-retries"];
  for (const id of ids) {
    assert(html.includes(`id="${id}"`), `设置页缺少运行上限输入框 ${id}`);
    assert(source.includes(id), `main.js 没有接线运行上限字段 ${id}`);
  }
  assert(byId.get("set-max-tokens")?.value === "4096", `已配置的上限没回填到表单:${byId.get("set-max-tokens")?.value}`);
  assert(
    byId.get("set-subagent-timeout")?.value === "",
    "未配置的上限必须留空(空=用默认),不能填成默认值——否则一保存就把默认固化进配置",
  );
  assert(
    (byId.get("set-subagent-timeout")?.placeholder ?? "").includes("900"),
    `留空项要用占位符显示内置默认:${byId.get("set-subagent-timeout")?.placeholder}`,
  );
  assert(
    source.includes("limits: collectLimits()"),
    "保存设置未透传运行上限",
  );
  assert(
    SETTINGS_FORM_IDS_IN_SOURCE(source, "set-max-tokens"),
    "运行上限没登记进脏状态列表:改了数字不会提示未保存(D-157 复现路径)",
  );
}
function SETTINGS_FORM_IDS_IN_SOURCE(src, id) {
  const block = src.slice(src.indexOf("const SETTINGS_FORM_IDS"), src.indexOf("let settingsSnapshot"));
  return block.includes(id);
}
assert(source.includes('$("set-codex-fast-mode").checked = s.codexFastMode === true'), "设置页未恢复 Codex Fast mode 状态");
assert(source.includes("codexFastMode: $(\"set-codex-fast-mode\").checked"), "保存设置未透传 Codex Fast mode");

// 节奏([cadence],R-157):读、存、脏状态三条线,与运行上限同一套防线。
{
  const cadenceIds = ["set-cadence-full-test", "set-cadence-full-test-batches",
    "set-cadence-targeted-test", "set-cadence-commit", "set-cadence-push"];
  for (const id of cadenceIds) {
    assert(html.includes(`id="${id}"`), `设置页缺少节奏表单 ${id}`);
    assert(SETTINGS_FORM_IDS_IN_SOURCE(source, id), `main.js 没有登记节奏字段 ${id}(改了没未保存提示)`);
  }
  assert(
    byId.get("set-cadence-full-test")?.value === "every_n_batches",
    `节奏下拉未回填生效值,实得: ${byId.get("set-cadence-full-test")?.value}`,
  );
  assert(
    byId.get("set-cadence-full-test-batches")?.value === "3",
    "每 N 批间隔未回填已存值",
  );
  assert(
    byId.get("set-cadence-targeted-test")?.value === "every_commit",
    "定向测试下拉未回填",
  );
  // 存一次:载荷必须带上 cadence(camelCase 外壳里嵌套 snake_case 键)。
  byId.get("set-cadence-full-test").value = "release_only";
  byId.get("set-cadence-full-test-batches").value = "";
  byId.get("set-cadence-full-test").dispatchEvent({ type: "change" });
  byId.get("settings-save")?.click();
  await flush();
  const saveArgs = invokeLog.includes("settings_save")
    ? savedPayloads.get("settings_save")
    : null;
  assert(saveArgs, "点保存未调 settings_save");
  assert(
    saveArgs?.payload?.cadence?.full_test === "release_only" && saveArgs?.payload?.cadence?.full_test_batches === null,
    `保存载荷未透传 cadence: ${JSON.stringify(saveArgs?.payload?.cadence)}`,
  );
}

// ---------- 设置页逐字段往返:开关 / provider 删除 / 项目级覆盖 / 未知合法值 ----------
// 这一组守的是"界面显示 A、运行用 B"的几条具体路径:开关不登记脏状态、provider 删了又
// 回来、limits/proxy 被项目级覆盖却不告警、配置里的合法值下拉里没有就被静默降级。
{
  // 上一组刚保存过并回流 loadSettings,这里就是干净基线。
  assert(byId.get("settings-dirty").classList.contains("hidden"), "保存回流后未回到干净态(基线没归零,后面的脏状态断言全是假通过)");

  // ① 开关回填:只测保存不测回填的话,「进设置页开关自己弹回去」这种形态抓不到。
  assert(byId.get("set-codex-fast-mode").checked === true, "Codex Fast mode 已存值未回填到开关");
  // ② 配置里的合法值下拉里没有,必须补兜底 option 而不是静默变空串。
  assert(
    byId.get("set-profile").value === "readonly",
    `配置里的 readonly 档位被下拉吃掉了(保存一次就降级成 dev):实得 "${byId.get("set-profile").value}"`,
  );

  // ③ 项目级覆盖必须明说,且不能误报。断言一律用**值/键名**而不是中文标签:
  // 界面语言会切,标签会跟着变,按标签断言是假红的常见来源。
  assert(!byId.get("settings-effective").classList.contains("hidden"), "项目级覆盖了 proxy/limits,设置页却没有提示");
  const effectiveNotice = listText("settings-effective");
  assert(effectiveNotice.includes("http://127.0.0.1:7890"), `代理被项目级覆盖却没报出实际生效值:${effectiveNotice}`);
  assert(effectiveNotice.includes("maxTokens"), `运行上限被项目级覆盖却没点名到具体键:${effectiveNotice}`);
  assert(!effectiveNotice.includes("readonly"), `两侧相同的 profileDefault 被误报成覆盖:${effectiveNotice}`);
  assert(!effectiveNotice.includes("Codex Fast mode"), `两侧相同的 Codex Fast mode 被误报成覆盖:${effectiveNotice}`);
  // effective 里**没有**的键必须整条跳过:undefined 被当成"实际生效是未设"会让提示条
  // 天天误报,用户学会无视它,真被覆盖时反而看不见。
  assert(!effectiveNotice.includes("ollama:qwen3"), `effective 里缺失的 fast 被当成"被覆盖成未设"误报了:${effectiveNotice}`);

  // ④ 开关的脏状态:checkbox 的 .value 恒为 "on",拿它做指纹永远比不出差异。
  // 漏登记的后果不是少个角标——进设置页会重跑 loadSettings 把表单整张覆盖回磁盘值,
  // 走开一趟再回来勾过的开关就悄悄弹回去了,而角标从头到尾没亮过。
  const codexToggle = byId.get("set-codex-fast-mode");
  codexToggle.checked = false;
  codexToggle.dispatchEvent({ type: "change" });
  assert(!byId.get("settings-dirty").classList.contains("hidden"), "改了开关却没有「未保存」提示(开关版的 D-157)");

  // ⑤ provider 删除:删行是 click 不是 input,表格上的事件委托抓不到,必须显式同步脏状态。
  const providerRows = document.querySelectorAll("#providers-table tbody tr");
  assert(providerRows.length === 3, `provider 表格未渲染出三行,实得 ${providerRows.length}`);
  // D-246:内置 provider(anthropic)的删除入口是「内置」标记,不是 ×——删了重开又回来,不给假按钮。
  const builtinCell = providerRows[2].querySelector(".provider-builtin");
  assert(builtinCell, "内置 provider 行缺少「内置」标记(D-246)");
  const builtinRemove = providerRows[2].querySelector("button");
  assert(!builtinRemove || builtinRemove.textContent !== "×", "内置 provider 不应提供删除按钮(D-246)");
  const removeBtn = providerRows[0].querySelectorAll("button").find((b) => b.textContent === "×");
  assert(removeBtn, "自定义 provider 行缺少移除按钮");
  removeBtn.click();
  await flush();
  assert(!byId.get("settings-dirty").classList.contains("hidden"), "删了 provider 却没有「未保存」提示(切走再回来它就原样回来了)");

  // ⑥ 保存载荷:顶层键集合逐字比对。规范 §4 要求表单透传全部字段,多一项少一项都要红——
  // 少一项 = 那个字段保存时被悄悄丢掉,多一项 = 有人加了后端不认的键。
  byId.get("settings-save").click();
  await flush();
  const payload = savedPayloads.get("settings_save")?.payload;
  assert(payload, "点保存未调 settings_save");
  assert(
    Object.keys(payload ?? {}).sort().join(",")
      === "cadence,codexFastMode,compact,fast,language,limits,primary,profileDefault,providers,proxy,reasoning",
    `settings_save 载荷顶层键集合变了: ${Object.keys(payload ?? {}).sort().join(",")}`,
  );
  // 根因终局判据:首次进设置页时两个角色 select 是零 option 的空壳,若实现仍是
  // 「先 select.value = 已存值、再读 DOM 当基准」,这里必然收到空串——而空串保存回去
  // 就是把 [models] primary/fast 从 kanzei.toml 里删掉。
  assert(payload?.primary === "deepseek:deepseek-chat", `探测不到的已存 primary 被保存成了 "${payload?.primary}"`);
  assert(payload?.fast === "ollama:qwen3", `已存 fast 被保存成了 "${payload?.fast}"`);
  assert(payload?.profileDefault === "readonly", `readonly 档位保存时被静默降级成 "${payload?.profileDefault}"`);
  assert(payload?.codexFastMode === false, `开关的新值未透传到载荷: ${payload?.codexFastMode}`);
  assert(
    (payload?.providers ?? []).map((p) => p.name).join(",") === "keepme,anthropic",
    `provider 删除未落进载荷(或整张表没发全): ${(payload?.providers ?? []).map((p) => p.name).join(",")}`,
  );
  // 保存回流后基线必须归零,否则「未保存」角标会一直亮着,变成人人无视的噪音。
  assert(byId.get("settings-dirty").classList.contains("hidden"), "保存回流后「未保存」角标仍亮着");
}

// ---------- R-184 P6(D-247):代理「指定地址」留空必须可见提示 ----------
{
  const proxyMode = byId.get("set-proxy-mode");
  const proxyUrl = byId.get("set-proxy-url");
  const proxyHint = byId.get("set-proxy-hint");
  assert(proxyMode && proxyUrl && proxyHint, "设置页缺少代理模式/地址/提示元素");
  // 夹具回显 proxy=env → custom 输入框隐藏、提示隐藏。
  assert(proxyUrl.classList.contains("hidden"), "env 模式下地址框不应可见");
  assert(proxyHint.classList.contains("hidden"), "env 模式下提示不应可见");
  // 切「指定地址」且留空 → 提示可见,说明将回落环境变量。
  proxyMode.value = "custom";
  proxyMode._listeners.change?.forEach((fn) => fn({ target: proxyMode }));
  assert(!proxyUrl.classList.contains("hidden"), "custom 模式下地址框应可见");
  assert(!proxyHint.classList.contains("hidden"), "「指定地址」留空时提示应可见(D-247)");
  const hintText = proxyHint.textContent || "";
  assert(hintText.includes("回落"), `提示未说明将回落: ${hintText}`);
  // 填了地址 → 提示消失。
  proxyUrl.value = "http://127.0.0.1:12000";
  proxyUrl._listeners.input?.forEach((fn) => fn({ target: proxyUrl }));
  assert(proxyHint.classList.contains("hidden"), "地址已填时提示应消失(D-247)");
  // 不静默改写用户选择:留空保存时载荷 proxy 仍为 custom 语义(空串),由后端按空回落,
  // 但界面已把回落说出来——这里验证载荷没被前端擅自改成 env。
  proxyUrl.value = "";
  proxyUrl._listeners.input?.forEach((fn) => fn({ target: proxyUrl }));
  proxyMode._listeners.change?.forEach((fn) => fn({ target: proxyMode }));
  byId.get("settings-save").click();
  await flush();
  const proxyPayload = savedPayloads.get("settings_save")?.payload?.proxy;
  assert(proxyPayload === "", `前端不应改写用户选择(留空即空串,回落语义在后端): 实得 ${JSON.stringify(proxyPayload)}`);
  proxyMode.value = "env";
  proxyMode._listeners.change?.forEach((fn) => fn({ target: proxyMode }));
  proxyUrl.value = "";
  proxyUrl._listeners.input?.forEach((fn) => fn({ target: proxyUrl }));
  assert(proxyUrl.classList.contains("hidden"), "切回 env 后地址框应隐藏");
  assert(proxyHint.classList.contains("hidden"), "切回 env 后提示应消失");
}

// ---------- R-178 批4 D7:设置页作用域选择器 ----------
// 第一版只覆盖 [models]:scope=project 时后端只写模型角色进主根 .kanzei/kanzei.toml,
// proxy/provider/limits/cadence 一律仍走全局(后端 settings.rs 按 scope 拦截)。
// 前端职责:默认全局、有项目上下文时 project 选项可用、无项目时禁用并回退 global、
// 保存时透传 scope+projectDir。
{
  const scopeSelect = byId.get("set-save-scope");
  assert(scopeSelect, "设置页缺少作用域选择器 #set-save-scope");
  const projectOption = scopeSelect.querySelector('option[value="project"]');
  assert(projectOption, "作用域选择器缺少「本项目」选项");
  // 冒烟 settings_get 桩自带 projectConfig(有项目)→ 选项可用、默认值 global。
  assert(projectOption.disabled === false, "有项目上下文时「本项目」选项未启用");
  assert(scopeSelect.value === "global", "默认作用域应为 global");

  byId.get("settings-save").click();
  await flush();
  const saveArgs = savedPayloads.get("settings_save");
  assert(saveArgs?.scope === "global", `默认作用域应为 global: ${JSON.stringify(saveArgs?.scope)}`);
  assert(saveArgs?.projectDir === null || saveArgs?.projectDir === undefined, "global 作用域不应携带 projectDir");

  // 无项目上下文:settings_get 不带 projectConfig → 选项禁用且当前值回退 global。
  const originalSettingsGet = payloads.settings_get;
  payloads.settings_get = { ...originalSettingsGet, projectConfig: undefined };
  try {
    const loadSettingsInSandbox = vm.runInContext("loadSettings", sandbox);
    await loadSettingsInSandbox();
    assert(projectOption.disabled === true, "无项目上下文时「本项目」选项未被禁用");
    assert(scopeSelect.value === "global", "无项目上下文时作用域未回退到 global");
  } finally {
    payloads.settings_get = originalSettingsGet;
  }

  // 有项目上下文:选中「本项目」保存 → scope+projectDir 一起透传。
  await vm.runInContext("loadSettings", sandbox)();
  scopeSelect.value = "project";
  byId.get("settings-save").click();
  await flush();
  const projectArgs = savedPayloads.get("settings_save");
  assert(projectArgs?.scope === "project", `选了「本项目」保存却没带 scope=project: ${JSON.stringify(projectArgs?.scope)}`);
  const currentProjectInSandbox = vm.runInContext("currentProject", sandbox);
  assert(projectArgs?.projectDir === currentProjectInSandbox, "scope=project 未携带当前项目目录");
}

// ---------- R-136 子代理模型一键就绪 ----------
assert(invokeLog.includes("fast_model_status"), "设置页未检测子代理模型就绪状态");
assert(
  listText("fast-status").includes("服务未运行"),
  `子代理不可用却没说清缺哪一环,实得: "${listText("fast-status")}"`,
);
assert(
  listText("fast-status").includes("暂不可用"),
  "未说明后果(记忆整理/快速记录这类杂活会静默失效)",
);
const fastSetupBtn = byId.get("fast-setup");
assert(!fastSetupBtn.classList.contains("hidden"), "未就绪时应显示一键安装按钮");
fastSetupBtn.click();
await flush();
assert(invokeLog.includes("fast_model_setup"), "点了一键就绪却没调后端");
// 安装进度事件要能刷到状态行。
handlers.get("kz:fast-setup")?.({ payload: { text: "pulling 50%(1500/3000 MB)" } });
assert(listText("fast-status").includes("50%"), "安装进度未反映到界面");

// ---------- D-167 手填模型：探测不到不等于用不了 ----------
const modelSelect = byId.get("model-select");
const compactModelValues = [...modelSelect.options].map((o) => o.value);
assert(compactModelValues.includes("deepseek:deepseek-chat"), "当前线路已选的 DeepSeek 未保留在紧凑模型列表");
assert(!compactModelValues.includes("ollama:qwen3"), "紧凑模型列表仍把未选模型全部灌入顶栏");
const showAllOption = [...modelSelect.options].find((o) => o.value === "__show_all_models__");
assert(showAllOption, "紧凑模型列表缺少展开完整探测清单入口");
modelSelect.value = "__show_all_models__";
modelSelect._listeners.change?.forEach((fn) => fn({ target: modelSelect }));
await flush();
assert([...modelSelect.options].some((o) => o.value === "ollama:qwen3"), "展开完整模型清单后仍缺少探测模型");
const manualOption = [...modelSelect.options].find((o) => o.value === "__manual__");
assert(manualOption, "模型下拉缺少手填入口(端点不实现 /models 时就彻底没法选)");
sandbox.__inputDialogResponses.push("deepseek:deepseek-chat");
modelSelect.value = "__manual__";
modelSelect._listeners.change?.forEach((fn) => fn({ target: modelSelect }));
await flush();
// R-178 批3:手填模型写后端(process_update 携带 manualModels),不再落 localStorage。
const manualUpdate = invokeArgs.findLast(({ cmd, args }) =>
  cmd === "process_update" && Array.isArray(args?.manualModels));
assert(manualUpdate, "手填模型未以 manualModels 发给后端(下次重开又要再填一遍)");
assert(
  manualUpdate.args.manualModels.includes("deepseek:deepseek-chat"),
  `手填模型落盘值不对:${JSON.stringify(manualUpdate.args.manualModels)}`,
);
assert(
  [...byId.get("model-select").options].some((o) => o.value === "deepseek:deepseek-chat"),
  "手填后模型未回到下拉列表里",
);
// 格式不对要挡住:provider 名对不上配置键时后端 resolve_model 会直接失败。
sandbox.__inputDialogResponses.push("随便写的");
modelSelect.value = "__manual__";
modelSelect._listeners.change?.forEach((fn) => fn({ target: modelSelect }));
await flush();
const badUpdate = invokeArgs.findLast(({ cmd, args }) =>
  cmd === "process_update" && Array.isArray(args?.manualModels));
assert(
  !badUpdate || !badUpdate.args.manualModels.includes("随便写的"),
  "非 provider:model 格式不应被接受",
);

// ---------- R-178 批3:localStorage 旧模型偏好一次性上迁后端并清除 ----------
// 预置旧版键(模型选择 + 手填候选),迁移后必须写入默认进程且旧键消失,否则下次
// 启动又回到 localStorage,永远迁不完。
storage.set(`kz-model:${PROJECT}`, "anthropic:claude-sonnet-5");
storage.set(`kz-manual-models:${PROJECT}`, JSON.stringify(["ollama:qwen3"]));
await sandbox.migrateLegacyModelPrefs();
await flush();
const migrationModelUpdate = invokeArgs.findLast(({ cmd, args }) =>
  cmd === "process_update" && args?.model === "anthropic:claude-sonnet-5");
assert(migrationModelUpdate, "旧模型偏好未上迁到默认进程(process_update 缺 model)");
const migrationManualUpdate = invokeArgs.findLast(({ cmd, args }) =>
  cmd === "process_update" && Array.isArray(args?.manualModels)
    && args.manualModels.includes("ollama:qwen3"));
assert(migrationManualUpdate, "旧手填候选未上迁到默认进程(process_update 缺 manualModels)");
assert(!storage.has(`kz-model:${PROJECT}`), "迁移成功后旧模型键未清除(下次启动会重复迁移)");
assert(!storage.has(`kz-manual-models:${PROJECT}`), "迁移成功后旧手填键未清除(下次启动会重复迁移)");
// 迁移失败(后端报错)必须保留旧键,下次 loadModels 重试,不能丢用户选择。
const migrationArgs = invokeArgs.length;
invokeFailures.set("process_update", "后端拒绝");
expectedPersistentError = "旧模型偏好迁移失败";
storage.set(`kz-model:${PROJECT}`, "anthropic:claude-sonnet-5");
await sandbox.migrateLegacyModelPrefs();
await flush();
invokeFailures.delete("process_update");
expectedPersistentError = null;
assert(storage.has(`kz-model:${PROJECT}`), "迁移失败时旧键不应被清除(可重试)");
assert(invokeArgs.length > migrationArgs, "迁移失败重试路径未调用后端");

// 后端回显整链:默认进程的 manual_models(②层)必须驱动下拉回显,而不是 localStorage。
payloads.process_list[0].manual_models = ["ollama:qwen3"];
await sandbox.refreshProcesses();
await sandbox.loadModels();
await flush();
assert(
  [...byId.get("model-select").options].some((o) => o.value === "ollama:qwen3"),
  "后端 manual_models 未回显到下拉(前端仍以 localStorage 为真源?)",
);
delete payloads.process_list[0].manual_models;

// ---------- R-115 偏好持久化：写了必须能读回 ----------
// 「写了却从不读回」是这块最容易出的问题:kz-reasoning 曾经全仓零处 getItem,
// 看起来存了,重启后照样回默认档。这里逐项验"改一次 → 落盘 → 能回填"。
const reasoningSelect = byId.get("reasoning-select");
reasoningSelect.value = "high";
reasoningSelect._listeners.change?.forEach((fn) => fn({ target: reasoningSelect }));
const reasoningKey = [...storage.keys()].find((k) => k.startsWith("kz-reasoning"));
assert(reasoningKey, "思考强度未落盘");
assert(storage.get(reasoningKey) === "high", `思考强度落盘值不对: ${storage.get(reasoningKey)}`);
assert(reasoningKey.includes(":"), "思考强度应按项目分键,不同项目常配不同模型");

const deliverySelect = byId.get("delivery-select");
deliverySelect.value = "steer";
deliverySelect._listeners.change?.forEach((fn) => fn({ target: deliverySelect }));
assert(storage.get("kz-delivery") === "steer", "交付方式未落盘");

// 状态筛选只在「需求与工作」标签页下可用(对照页只有「全部状态」一个选项),
// 先把标签页切回来,否则 select 规范语义会把 "doing" 拒成空串,落盘值也跟着变空。
byId.get("documents-tab-req").click();
await flush();
const reqStatusFilter = byId.get("documents-status-filter");
reqStatusFilter.value = "doing";
reqStatusFilter._listeners.change?.forEach((fn) => fn({ target: reqStatusFilter }));
await flush();
const filterKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
assert(filterKey, "需求筛选未落盘(重启后会回到「全部」)");
assert(JSON.parse(storage.get(filterKey)).docReq.status === "doing", "筛选落盘值不对");

// 模式回退链:本进程记忆 → 全局上次选择 → dev-pair。中间那档缺了就会静默降级。
assert(typeof sandbox.applyProfileValue === "function", "applyProfileValue 未定义");
// 这条用例验证“无进程记忆”分支；前面的 profile 交互会写入同一 Map，必须显式清理测试状态。
vm.runInContext("processProfileUi.clear()", sandbox);
storage.set("kz-profile", "dev-auto");
sandbox.applyProfileValue("dev");
assert(
  byId.get("profile-select").value === "dev-auto",
  `无进程记忆时应回退到全局上次选择,实得 ${byId.get("profile-select").value}(重启后自主推进会被降级成结伴开发)`,
);
storage.set("kz-profile", "research");
sandbox.applyProfileValue("dev");
assert(
  byId.get("profile-select").value === "dev-pair",
  "全局值与后端 profile 冲突时应回落 dev-pair,不能把 research 塞进 dev 进程",
);
// R-184 P6(D-248):applyProfileValue 是**回显**,只读——切进程看一眼绝不许改写全局
// kz-profile(否则用户全局档位被静默降级)。写全局只能发生在用户主动 change。
storage.set("kz-profile", "dev-auto");
sandbox.applyProfileValue("dev");
assert(
  storage.get("kz-profile") === "dev-auto",
  `回显(切进程)不得改写全局 kz-profile,实得 ${storage.get("kz-profile")}(D-248)`,
);
const profileSelectEl = byId.get("profile-select");
profileSelectEl.value = "dev-auto";
profileSelectEl._listeners.change?.forEach((fn) => fn({ target: profileSelectEl }));
assert(
  storage.get("kz-profile") === "dev-auto",
  "用户主动切换档位仍须写全局 kz-profile",
);
storage.set("kz-profile", "dev-auto");
sandbox.applyProfileValue("dev");
profileSelectEl.value = "dev-pair";
profileSelectEl._listeners.change?.forEach((fn) => fn({ target: profileSelectEl }));
assert(
  storage.get("kz-profile") === "dev-pair",
  "用户主动切换档位仍须写全局 kz-profile(第二次)",
);

// ---------- R-099/R-127 运行画像面板 ----------
const metricsTab = document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "metrics");
assert(metricsTab, "活动栏缺少运行画像入口");
metricsTab.click();
await flush();
assert(invokeLog.includes("run_metrics"), "运行画像页未拉取度量");
const metricRounds = document.querySelectorAll("#metrics-rounds .metrics-round");
assert(metricRounds.length === 2, `逐轮画像未渲染,实得 ${metricRounds.length}`);
const metricsText = listText("metrics-rounds");
assert(metricsText.includes("edit 1/6"), `未给出 edit 未命中比,实得: ${metricsText.slice(0, 100)}`);
assert(metricsText.includes("git 2"), "未给出 git 查询次数与组数");
assert(metricsText.includes("edit×6"), "未给出工具分布");
assert(metricsText.includes("4800"), "未汇总上下文占用");
// 未度量的轮次要明说,不能显示成"全零"——那会让人误判冗余在下降。
assert(metricsText.includes("该轮早于度量落地"), "未区分「没度量」与「度量为零」");
const trendText = listText("metrics-trend");
assert(trendText.includes("1 ") && trendText.includes("轮均值"), "趋势未按已度量轮次统计");
assert(trendText.includes("17%"), `均值应只算已度量轮次(1/6≈17%),实得: ${trendText}`);
assert(invokeArgs.some((call) => call.cmd === "run_metrics_by_task"), "运行画像页未消费真实 task projection command");
const taskMetricGroups = document.querySelectorAll("#metrics-tasks .metrics-task-group");
assert(taskMetricGroups.length === 2, `任务画像应分已关闭/进行中两组,实得 ${taskMetricGroups.length}`);
const taskMetricsText = listText("metrics-tasks");
assert(taskMetricsText.includes("任务画像演示") && taskMetricsText.includes("进行中画像"), "任务画像未渲染 projection 中的任务标题");
assert(taskMetricsText.includes("sess-smoke") && taskMetricsText.includes("completed") && taskMetricsText.includes("12"), "任务画像未提供 task→session→round 下钻数据");
assert(document.querySelectorAll("#metrics-tasks .metrics-task-rounds .metrics-round-tools").length === 1, "已关闭 task 的 round 细节未渲染");

// ---------- R-126 UI 自查探针：在真实窗口里取样并回传 ----------
const probe = handlers.get("kz:ui-probe");
assert(probe, "未订阅 UI 探针事件(agent 无法自查界面)");
probe({ payload: { id: 1, kind: "dom", arg: "#documents-req-list" } });
await flush();
assert(probeResults.length === 1, "DOM 探针未回传结果");
assert(probeResults[0].id === 1, "探针回传未带上请求 id(后端无法配对)");
assert(probeResults[0].result.includes("doc-item"), `DOM 探针未给出真实渲染结构: ${probeResults[0].result.slice(0, 80)}`);
probe({ payload: { id: 2, kind: "dom", arg: "#nonexistent-xyz" } });
await flush();
assert(
  probeResults[1].result.includes("没有匹配"),
  "选择器无匹配时应明确说明,而不是回空串让人以为渲染了空内容",
);
// 用 warn 验证捕获链路:sandbox 的 console.error 本身就是冒烟的失败护栏,
// 拿它当样本会把这条测试变成自失败。捕获逻辑对 error/warn 是同一条。
sandbox.console.warn("smoke probe marker");
probe({ payload: { id: 3, kind: "console", arg: "" } });
await flush();
assert(
  probeResults[2].result.includes("smoke probe marker"),
  `console 探针未捕获(ReferenceError 一类问题就是这样漏过去的),实得: ${probeResults[2]?.result?.slice(0, 60)}`,
);
probe({ payload: { id: 4, kind: "unknown-kind", arg: "" } });
await flush();
assert(probeResults[3].result.includes("未知探针类型"), "未知探针类型应回传说明而不是静默");

// ---------- D-489:手机消息事件必须走控制路由并刷新会话/进程列表 ----------
{
  const mobileHandler = handlers.get("kz:mobile-message");
  assert(mobileHandler, "未订阅 kz:mobile-message");
  const conversationsBefore = invokeArgs.filter(({ cmd }) => cmd === "conversation_list").length;
  const processesBefore = invokeArgs.filter(({ cmd }) => cmd === "process_list").length;
  mobileHandler({ payload: { session_id: "sess-smoke", text: "来自手机的冒烟消息" } });
  await flush();
  assert(
    invokeArgs.filter(({ cmd }) => cmd === "conversation_list").length > conversationsBefore,
    "kz:mobile-message 未触发会话列表刷新",
  );
  assert(
    invokeArgs.filter(({ cmd }) => cmd === "process_list").length > processesBefore,
    "kz:mobile-message 未触发进程列表刷新",
  );
}

// ---------- 语言切换：静态文本/属性与动态错误必须 zh→en→zh→en 可逆 ----------
const languageControl = byId.get("language-select");
const projectInit = byId.get("project-init");
assert(languageControl.querySelectorAll("option").length === 3, "界面语言应提供跟随系统/中文/English 三个选项");
// rail 上还有侧栏开合(无 data-view),对话按钮要按 data-view 精确取。
const chatActivity = document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat");
assert(projectInit.getAttribute("title") === "初始化新项目目录", "HTML title 未进入真实冒烟 DOM");
assert(chatActivity.getAttribute("aria-label") === "切换到对话", "HTML aria-label 未进入真实冒烟 DOM");
languageControl.value = "system";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(storage.get("kz-language") === "system", "跟随系统选择未持久化");
assert(["zh-CN", "en"].includes(document.documentElement.lang), "跟随系统未解析成中文或英文界面");
languageControl.value = "en";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(document.documentElement.lang === "en", "切换 English 后 document.lang 未更新");
assert(storage.get("kz-language") === "en", "English 选择未持久化");
assert(projectInit.getAttribute("title") === "Initialize a new project directory", "静态 title 未翻译");
assert(chatActivity.getAttribute("aria-label") === "Switch to chat", "静态 aria-label 未翻译");
// 非终态 persistence warning 不能把仍在运行的会话投影成空闲；随后再验证
// terminal=true 的真正运行失败会收口为 Error。
handlers.get("kz:turn")?.({ payload: { step: 1, maxSteps: 1, sessionId: "sess-smoke" } });
await flush();
expectedPersistentError = "smoke persistence warning";
const persistentWarningHitsBefore = expectedPersistentHits;
const nonterminalErrorCount = document.querySelectorAll(".error-level").length;
handlers.get("kz:error")?.({ payload: { message: "smoke persistence warning", terminal: false, sessionId: "sess-smoke" } });
await flush();
assert(listText("status-mode").includes("Running"), `非终态错误不应收回运行态: "${listText("status-mode")}"`);
assert(!byId.get("stop").classList.contains("hidden"), "非终态错误期间停止按钮不应消失");
assert(document.querySelectorAll(".error-level").length === nonterminalErrorCount, "非终态错误不应渲染为对话中的错误卡");
assert(expectedPersistentHits > persistentWarningHitsBefore, "非终态错误应进入持久化告警通道");
expectedPersistentError = null;
  handlers.get("kz:error")?.({ payload: { message: "smoke backend failure", sessionId: "sess-smoke" } });
await flush();
assert(listText("status-text").includes("Error"), `英文动态错误状态未翻译: "${listText("status-text")}"`);
assert(document.querySelector(".error-level")?.textContent === "Fatal error", "英文错误等级未翻译");
languageControl.value = "zh";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(document.documentElement.lang === "zh-CN", "切回中文后 document.lang 未更新");
assert(storage.get("kz-language") === "zh", "中文选择未持久化");
assert(listText("status-text").includes("出错"), `中文动态错误状态未恢复: "${listText("status-text")}"`);
assert(
  document.querySelector(".error-level")?.textContent === "致命错误",
  `动态错误等级切回中文失败:${document.querySelector(".error-level")?.textContent}`,
);
assert(projectInit.getAttribute("title") === "初始化新项目目录", "静态 title 切回中文失败");
assert(chatActivity.getAttribute("aria-label") === "切换到对话", "静态 aria-label 切回中文失败");
languageControl.value = "en";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(projectInit.getAttribute("title") === "Initialize a new project directory", "静态 title 二次切英文失败");
assert(chatActivity.getAttribute("aria-label") === "Switch to chat", "静态 aria-label 二次切英文失败");
assert(document.querySelector(".error-level")?.textContent === "Fatal error", "动态错误等级二次切英文失败");
const askHandler = handlers.get("kz:ask");
askHandler?.({ payload: { id: 91, sessionId: "sess-smoke", kind: "permission", action: "执行用户动作 Ω", resource: "用户/路径甲", remember: "用户/路径甲" } });
askHandler?.({ payload: { id: 92, sessionId: "sess-smoke", kind: "permission", action: "写入用户数据 Ω", resource: "用户/路径乙", remember: "用户/路径乙" } });
await flush();
assert(listText("ask-title") === "Permission request", `英文权限标题未翻译:${listText("ask-title")}`);
assert(listText("ask-queue-status").includes("1 pending"), `英文权限队列说明未翻译:${listText("ask-queue-status")}`);
assert(listText("ask-action") === "执行用户动作 Ω", "权限 action 用户数据被翻译或改写");
assert(byId.get("ask-deny").textContent === "Deny", "英文权限拒绝按钮未翻译");
languageControl.value = "zh";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(listText("ask-title") === "权限请求", "权限标题切回中文失败");
assert(listText("ask-queue-status").includes("还有 1 条待处理"), "权限队列说明切回中文失败");
assert(byId.get("ask-deny").textContent === "拒绝", "权限拒绝按钮切回中文失败");
languageControl.value = "en";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(listText("ask-title") === "Permission request", "权限标题二次切英文失败");

// ---------- R-223 权限被拦聚合呈现:①被拦落可见 notice + 轮末汇总 ②自动放行常驻徽标 ----------
{
  const askHandler = handlers.get("kz:ask");
  // 断言①a:autonomous 权限询问跳过 → 对话流落可见 notice(不只隐藏日志)。
  askHandler?.({
    payload: {
      id: 901, sessionId: "sess-smoke", kind: "permission",
      action: "edit", resource: "src/a.rs", remember: "src/*",
      source: "autonomous",
    },
  });
  await flush();
  const notices = [...document.querySelectorAll(".msg.notice")];
  assert(
    notices.some((n) =>
      (n.textContent.includes("权限被拦已跳过") || n.textContent.includes("Permission blocked, skipped")) &&
      n.textContent.includes("edit"),
    ),
    `autonomous 权限被拦应在对话流落可见 notice(实得: ${notices.map((n) => n.textContent).join(" | ")})`,
  );
  // 断言①b:轮末汇总(kz:done 携带 steps)→ 「本轮 N 次被拦」。
  const doneHandler = handlers.get("kz:done");
  doneHandler?.({ payload: { sessionId: "sess-smoke", steps: 2, elapsedMs: 1234 } });
  await flush();
  assert(
    [...document.querySelectorAll(".msg.notice")].some((n) =>
      (n.textContent.includes("本轮权限被拦") || n.textContent.includes("permissions blocked this round")) &&
      n.textContent.includes("edit"),
    ),
    "轮末应汇总「本轮 N 次被拦(动作/资源清单)」",
  );
  assert(
    /1\.2s/.test(byId.get("log-lines").textContent),
    "kz:done 的 elapsedMs 未驱动真实运行日志耗时",
  );
  // 断言②:开启自动放行 → 状态栏常驻警示徽标可见;localStorage 持久化(模拟重启后仍可见)。
  const autoAllow = byId.get("auto-allow");
  autoAllow.checked = true;
  autoAllow.dispatchEvent({ type: "change" });
  await flush();
  const badge = byId.get("status-auto-allow");
  assert(
    badge && !badge.classList.contains("hidden"),
    "开启自动放行后状态栏应挂常驻警示徽标",
  );
  assert(
    storage.get("kz-auto-allow") === "1",
    "自动放行选择必须持久化到 localStorage(跨重启)",
  );
  // 模拟重启:徽标初始化逻辑在 07-events.js 顶部,直接重建可见性。
  autoAllow.checked = false;
  autoAllow.dispatchEvent({ type: "change" });
  await flush();
  assert(
    badge.classList.contains("hidden"),
    "关闭自动放行后徽标应隐藏",
  );
}

// ---------- R-086 多会话并发:控制事件按 sessionId 收敛,切回可见可答复、不丢不串 ----------
// 前置:清空上面语言切换测试留下的主会话 ask(91/92 仍在队列,askActive=91)。
if (byId.get("ask-allow")) byId.get("ask-allow").click();
await flush();
if (!byId.get("ask-overlay").classList.contains("hidden") && byId.get("ask-allow")) byId.get("ask-allow").click();
await flush();
assert(byId.get("ask-overlay").classList.contains("hidden"), "R-086 前置:主会话 ask 未清空");
// 场景:主会话(sess-smoke)活动;后台会话(sess-bg)初始 running=true(桩里故意给旧值,
// 模拟"事件已收敛但轮询采样发生在事件之前"的竞态)。
const activeLine = document.querySelector("#parallel-task-status .parallel-task-row.active");
assert(activeLine?.textContent.includes("主会话"), `冒烟前置:主会话应为活动线路(实际:${activeLine?.textContent})`);
// 并行线状态卡按 process_list 全量投影,三条并行线就必须有三条可切换任务行。
const twoProcesses = structuredClone(payloads.process_list);
sandbox.renderProcesses([
  ...twoProcesses,
  { id: "p|third", label: "第三线路", session_id: "sess-third", running: false, branch: "kanzei/thread-third", authority: "parallel", stage: "测试" },
]);
assert(document.querySelectorAll("#parallel-task-status .parallel-task-row").length === 3, "三线并行时侧栏只渲染了一个/两条任务状态");
const thirdLineStatus = [...document.querySelectorAll("#parallel-task-status .parallel-task-row")]
  .find((row) => row.dataset.processId === "p|third")?.textContent ?? "";
assert(thirdLineStatus.includes("第三线路") && !thirdLineStatus.includes("测试"), `空闲第三线路未显示或仍残留旧阶段:${thirdLineStatus}`);
sandbox.renderProcesses(twoProcesses);
await flush();
// 运行事件没有 session_id 时不能猜当前线路:猜错一次就会把后台输出投到主对话。
// 全局辅助事件(fast-setup/ui-probe)另有无会话契约,不在这里测试。
const statusBeforeMissingSession = byId.get("status-text").textContent;
handlers.get("kz:status")?.({ payload: { stage: "无身份串线", detail: "不应投影" } });
await flush();
assert(
  byId.get("status-text").textContent === statusBeforeMissingSession,
  "缺少 session_id 的运行事件不应投影到当前线路",
);
// 后台会话的权限询问到达:不弹当前窗口,但进入该会话自己的待答队列。
askHandler?.({
  payload: { id: 501, sessionId: "sess-bg", kind: "permission", action: "后台进程要写文件", resource: "后台/路径", remember: "后台/路径" },
});
await flush();
assert(byId.get("ask-overlay").classList.contains("hidden"), "后台会话的 ask 不应在活动会话弹窗");
// 后台会话第一轮结束(kz:done)。kz:done 只是**一轮**的终点:后端 run loop 会 promote
// 排队输入接着跑,runtime.running 仍是 true。拿它收敛会让多轮运行从第二轮起全程显示
// 空闲且再也纠不回来(converged 屏蔽了轮询校正),所以这里必须仍然是运行中。
handlers.get("kz:done")?.({ payload: { steps: 1, halted: false, history: 3, input: 0, output: 0, cacheRead: 0, cacheWrite: 0, sessionId: "sess-bg" } });
await flush();
const bgState = sandbox.sessionState("sess-bg");
assert(bgState.converged === false, "kz:done 是轮末事件,不得收敛会话终态(排队输入还要继续跑)");
assert(bgState.running === true, `kz:done 后会话仍在跑,运行态被误清: ${bgState.running}`);
const bgLineAfterDone = [...document.querySelectorAll("#parallel-task-status .parallel-task-row")].find((row) => row.textContent.includes("后台会话"));
assert(bgLineAfterDone?.textContent.includes("●"), `多轮运行第一轮结束后线路按钮熄灯(实际:${bgLineAfterDone?.textContent})`);
assert(byId.get("stop").classList.contains("hidden"), "后台会话结束不应改变主会话视图的运行态");
// 第二轮开跑:kz:turn 是每轮开头必发的自愈信号,把状态机拨回运行中并解除 converged。
handlers.get("kz:turn")?.({ payload: { step: 1, maxSteps: 30, sessionId: "sess-bg" } });
await flush();
assert(sandbox.sessionState("sess-bg").running === true, "后台会话第二轮 kz:turn 未把状态机拨回运行中");
assert(sandbox.sessionState("sess-bg").converged === false, "kz:turn 未解除 converged,状态机会被上一轮终态焊死");
// 会话真正转空闲(后端 run loop 退出)才收敛终态。
handlers.get("kz:idle")?.({ payload: { reason: "completed", sessionId: "sess-bg" } });
await flush();
assert(sandbox.sessionState("sess-bg").running === false, "kz:idle 未收敛运行态");
assert(sandbox.sessionState("sess-bg").converged === true, "kz:idle 未标记 converged");
const bgLineAfterIdle = [...document.querySelectorAll("#parallel-task-status .parallel-task-row")].find((row) => row.textContent.includes("后台会话"));
assert(!bgLineAfterIdle?.textContent.includes("●"), "会话已空闲但线路按钮仍亮着运行标记");
// 切回后台会话:权限询问可见可答复,运行态显示空闲(converged 挡住桩里的旧 running=true)。
const messagesBeforeSwitch = listText("messages");
const pendingSwitch = sandbox.switchProcess("p|bg");
assert(listText("messages") === messagesBeforeSwitch, "切线程请求尚未完成时主对话被清空");
await pendingSwitch;
await flush();
const bgLine = document.querySelector("#parallel-task-status .parallel-task-row.active");
assert(bgLine?.textContent.includes("后台会话"), "切换到后台会话后活动线路按钮未更新");
assert(bgLine?.textContent.includes("kanzei/thread-smoke"), `分支线按钮未显示真实分支名:${bgLine?.textContent}`);
const bgGitStatusCall = invokeArgs.findLast(({ cmd }) => cmd === "git_status");
assert(
  bgGitStatusCall?.args?.worktreePath === "C:/smoke-wt",
  `切到后台线路后 git_status 未绑定该线路 worktree:${JSON.stringify(bgGitStatusCall)}`,
);
const trackerToggle = byId.get("process-tracker-writes");
assert(trackerToggle && !trackerToggle.checked, "分支线 tracker 写入必须默认关闭");
assert(!byId.get("process-tracker-writes-wrap").classList.contains("hidden"), "分支线未显示 tracker 写入开关");
payloads.process_list[1].tracker_writes = true;
trackerToggle.checked = true;
trackerToggle.dispatchEvent({ type: "change" });
await flush();
assert(
  invokeArgs.findLast(({ cmd }) => cmd === "process_update")?.args?.trackerWrites === true,
  `tracker 开关未以 trackerWrites 发给后端:${JSON.stringify(invokeArgs.findLast(({ cmd }) => cmd === "process_update"))}`
);
assert(trackerToggle.checked, "后端回显开启后 tracker 开关未保持选中");
assert(!byId.get("ask-overlay").classList.contains("hidden"), "切回后台会话后权限询问不可见");
assert(listText("ask-action") === "后台进程要写文件", "切回后弹出的不是该会话自己的 ask(串会话)");
assert(byId.get("stop").classList.contains("hidden"), "后台会话已收敛终态但切回后仍显示运行中(converged 未生效)");
// 可答复:点允许后 invoke answer_ask,弹窗关闭,队列清空。
byId.get("ask-allow").click();
await flush();
assert(invokeLog.includes("answer_ask"), "切回后台会话后权限询问无法答复(answer_ask 未调用)");
assert(byId.get("ask-overlay").classList.contains("hidden"), "答复后权限弹窗未关闭");
// 再切回主会话:不串台,无残留弹窗。
await sandbox.switchProcess("d|smoke");
await flush();
const backLine = document.querySelector("#parallel-task-status .parallel-task-row.active");
assert(backLine?.textContent.includes("主会话"), "切回主会话后活动线路按钮未更新");
assert(byId.get("process-tracker-writes-wrap").classList.contains("hidden"), "默认线不应显示分支 tracker 开关");
assert(byId.get("ask-overlay").classList.contains("hidden"), "切回主会话后残留后台 ask 弹窗");
// R-206 验收③:长工具运行中点停止 → stopping 过渡态,晚到进度事件不得把
// 停止按钮翻回运行中(无状态闪跳)。直接经 transitionSession 置 stopping
// (与 stop 按钮 handler 的 872-873 行同源),再发进度事件断言不翻回。
{
  vm.runInContext('transitionSession("sess-smoke", "running")', sandbox);
  const mainState = sandbox.sessionState("sess-smoke");
  assert(mainState.phase === "running", "前置:主会话应在运行中");
  // 与 08-compose.js 停止按钮 handler 同源:置 stopping + setStopping。
  vm.runInContext('transitionSession("sess-smoke", "stopping")', sandbox);
  await flush();
  assert(mainState.phase === "stopping", "点停止后 phase 未进入 stopping");
  // stopping 期间 running=true 是设计语义(按钮显示「停止中…」而非消失);
  // 要防的是 phase 闪跳回 running 与按钮可点化。验证 phase 稳定即可。
  // 晚到的进度事件:不得翻回 running(01-core.js stopping 保护)。
  handlers.get("kz:tool-progress")?.({ payload: { sessionId: "sess-smoke", name: "bash", detail: "仍在执行" } });
  handlers.get("kz:status")?.({ payload: { sessionId: "sess-smoke", stage: "跑工具", detail: "" } });
  await flush();
  assert(mainState.phase === "stopping", "stopping 期间晚到进度事件把 phase 翻回 running(闪跳)");
  // stopping 期间 running=true 是设计语义;关键防的是 live_running 权威残留
  // 让后续轮询翻回 running 相位。断言相位稳定 + live_running 已清。
  assert(mainState.live_running === false, "stopping 后 live_running 权威未清,轮询可把会话翻回运行中");
  // 终态离开 stopping。
  handlers.get("kz:stopped")?.({ payload: { sessionId: "sess-smoke" } });
  await flush();
  assert(mainState.phase === "stopped", "kz:stopped 后 phase 未离开 stopping");
  assert(mainState.running === false, "stopped 后 running 未收敛为 false");
}

// 重建路径:后端 asks 表活得比 webview 久,界面重载后首次拿到进程列表必须补拉回来,
// 否则重载前挂起的权限询问再也不出现,而后端还在 await 它的答复(验收:后端提供
// pending asks 查询以支持重建)。这里用一个从未见过的会话模拟"重载后的第一次渲染"。
payloads.pending_asks_get = [
  { id: 601, sessionId: "sess-reload", kind: "permission", action: "重载前挂起的询问", resource: "重载/路径", remember: "重载/路径" },
];
const asksPullsBefore = invokeLog.filter((cmd) => cmd === "pending_asks_get").length;
sandbox.renderProcesses([{ id: "r|reload", label: "重载会话", session_id: "sess-reload", running: false }]);
await flush();
assert(
  invokeLog.filter((cmd) => cmd === "pending_asks_get").length > asksPullsBefore,
  "首次拿到进程列表未向后端补拉待答队列(重载后挂起的 ask 会永久失联)"
);
assert(!byId.get("ask-overlay").classList.contains("hidden"), "重载后未从后端重建待答权限询问");
assert(listText("ask-action") === "重载前挂起的询问", `重建出的不是后端返回的那条 ask:${listText("ask-action")}`);
byId.get("ask-allow").click();
await flush();
// 收尾:恢复原进程列表(活动进程回到主会话),后续用例不受影响。
payloads.pending_asks_get = [];
sandbox.renderProcesses([
  { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false },
  { id: "p|bg", label: "后台会话", session_id: "sess-bg", running: true, worktree_path: "C:/smoke-wt", branch: "kanzei/thread-smoke", tracker_writes: true },
]);
await flush();
assert(document.querySelector("#parallel-task-status .parallel-task-row.active")?.textContent.includes("主会话"), "重建用例收尾后活动线路未回到主会话");

// ---------- R-169 鞭挞执行层:判定已引擎化,前端只执行 autoAction ----------
// 判定(空转画像/连数/全部阻塞/NUDGE 时机/停止原因)全部在 harness auto_run
// 状态机单测覆盖(kanzei-harness auto_run.rs,12 组);这里验证前端对 kz:done
// 携带 autoAction 的执行:Continue→续跑、Nudge→追加指令提示、Stop→停止+原因+
// 开关联动、NoContinue→不动。
assert(kzTest, "未注入鞭挞状态测试钩子");
const savedProfileForWhip = byId.get("profile-select").value;
const savedAutoCheck = byId.get("auto-continue").checked;
const savedLangForWhip = languageControl.value;
languageControl.value = "zh";
languageControl.dispatchEvent({ type: "change" });
await flush();
byId.get("profile-select").value = "dev-auto";
byId.get("auto-continue").checked = true;
kzTest.reset();
// ① Continue:镜像计数并续跑,不刹车。
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { read: 2, edit: 1 }, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: "sess-smoke" } });
await flush();
assert(kzTest.rounds() === 1, `Continue 应镜像推进计数,实得 ${kzTest.rounds()}`);
assert(byId.get("auto-continue").checked, "Continue 不应关掉自动推进");
// ② Nudge:引擎给出的推进指令占一轮,前端给提示不刹车。
handlers.get("kz:done")?.({ payload: { steps: 2, halted: false, tools: { memory_note: 1 }, autoAction: { type: "Nudge", prompt: "上一轮没有产生任何实质动作。", rounds: 2, max: 10 }, sessionId: "sess-smoke" } });
await flush();
assert(kzTest.rounds() === 2, `Nudge 应镜像推进计数,实得 ${kzTest.rounds()}`);
assert(byId.get("auto-status").textContent.includes("无动作 · 追加推进指令"), `#auto-status 未提示追加推进指令: ${byId.get("auto-status")?.textContent}`);
assert(byId.get("auto-continue").checked, "Nudge 第一次不应立即刹车");
// ③ Stop(NoAction):连续两轮无动作,停止并显示原因。
handlers.get("kz:done")?.({ payload: { steps: 2, halted: false, tools: { memory_note: 1 }, autoAction: { type: "Stop", reason: "NoAction" }, sessionId: "sess-smoke" } });
await flush();
assert(kzTest.rounds() === 0, "连续两轮无实质动作后推进计数应清零");
assert(kzTest.stopReason().includes("连续两轮无动作"), `刹车原因不对: ${kzTest.stopReason()}`);
assert(byId.get("auto-status").textContent.includes("连续两轮无动作"), `#auto-status 未显示刹车原因: ${byId.get("auto-status")?.textContent}`);
// ④ Stop(AllBlocked):全部阻塞,停并取消开关。
byId.get("auto-continue").checked = true;
kzTest.setRounds(3);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "AllBlocked" }, sessionId: "sess-smoke" } });
await flush();
assert(!byId.get("auto-continue").checked, "需求/缺陷全部被阻塞时自动推进应停止");
assert(kzTest.rounds() === 0, "阻塞刹车后推进计数应清零");
assert(kzTest.stopReason().includes("任务尚未完成"), `阻塞刹车原因不对: ${kzTest.stopReason()}`);
// ⑤ Continue:存在可推进条目时正常续跑(不误刹车)。
byId.get("auto-continue").checked = true;
kzTest.setRounds(1);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Continue", rounds: 2, max: 10 }, sessionId: "sess-smoke" } });
await flush();
assert(byId.get("auto-continue").checked, "Continue 不得误刹车");
// ⑤b D-403 失败轮执行层(kz:auto-fail):判定在 harness auto_run 单测覆盖,
// 这里验证前端执行——RetryAfterFailure→失败重试提示+按 delayMs 武装续跑;
// Stop(RepeatedFailure)→计数清零+停摆原因可见(先重试后停,收尾清掉挂着的定时器)。
handlers.get("kz:auto-fail")?.({ payload: { error: "provider returned HTTP 503: upstream reset", autoAction: { type: "RetryAfterFailure", attempt: 1, maxAttempts: 3, delayMs: 15000, rounds: 3, max: 10 }, sessionId: "sess-smoke" } });
await flush();
assert(byId.get("auto-status").textContent.includes("失败重试"), `#auto-status 未提示失败重试: ${byId.get("auto-status")?.textContent}`);
assert(byId.get("auto-status").textContent.includes("1/3"), `失败重试应显示 attempt/maxAttempts: ${byId.get("auto-status")?.textContent}`);
// ⑤c 退避重试必须活过随后那条 terminal 错误。真机时序:同一次失败里后端先发
// kz:auto-fail(排上退避重试)、紧接着 run_task 的 Err 分支发 kz:error(terminal)。
// 这里按真机顺序连发两条且**中间不排空定时器**(flush 会把 15s 那一枪直接跑掉,
// 掩盖问题)。旧实现在 kz:error 终态分支无条件 cancelAutoContinueTimer,刚排的重试
// 当场被自己人掐掉:界面停在「失败重试 1/3 · 15s」,那一轮永不到来——用户
// 2026-08-17 报告「中间断了一下网,鞭挞没自动重试,手动发继续才恢复」。
handlers.get("kz:auto-fail")?.({ payload: { error: "transport error: connection reset", autoAction: { type: "RetryAfterFailure", attempt: 1, maxAttempts: 3, delayMs: 15000, rounds: 3, max: 10 }, sessionId: "sess-smoke" } });
handlers.get("kz:error")?.({ payload: { message: "transport error: connection reset", terminal: true, sessionId: "sess-smoke" } });
assert(
  kzTest.timerSessions().includes("sess-smoke"),
  "终态错误掐掉了刚排上的失败退避重试:鞭挞停在「失败重试 1/3」再也不动(断网一次就此停摆,只能手动发继续)",
);
assert(
  kzTest.retryLabel("sess-smoke")?.includes("失败重试"),
  `退避重试的定时器必须带重试标记(终态错误靠它认出这一枪不能掐),实得 ${JSON.stringify(kzTest.retryLabel("sess-smoke"))}`,
);
assert(
  sandbox.sessionState("sess-smoke").phase === "auto_pending",
  `重试在途时相位不得覆成 failed(侧栏与横幅会说「出错」而其实下一轮在等着发),实得 ${sandbox.sessionState("sess-smoke").phase}`,
);
assert(
  byId.get("stop").textContent.includes("鞭挞"),
  `退避重试等待期间停止按钮应仍是「停止鞭挞」(用户要能刹住这一枪),实得 ${byId.get("stop").textContent}`,
);
await flush();
handlers.get("kz:auto-fail")?.({ payload: { error: "provider returned HTTP 503", autoAction: { type: "Stop", reason: "RepeatedFailure", max: 3 }, sessionId: "sess-smoke" } });
await flush();
assert(kzTest.rounds() === 0, "连续失败停摆后推进计数应清零");
assert(kzTest.stopReason().includes("连续多轮运行失败"), `停摆原因不对: ${kzTest.stopReason()}`);
// ⑥ Stop(BacklogEmpty):清空,停并取消开关。
byId.get("auto-continue").checked = true;
kzTest.setRounds(2);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "BacklogEmpty" }, sessionId: "sess-smoke" } });
await flush();
assert(!byId.get("auto-continue").checked, "需求/缺陷清空时自动推进应停止");
assert(kzTest.stopReason().includes("已清空"), `清空刹车原因不对: ${kzTest.stopReason()}`);
// ⑦ Stop(StopAfterRound):本轮后停,开关自动取消勾选。
byId.get("auto-continue").checked = true;
kzTest.setRounds(1);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "StopAfterRound" }, sessionId: "sess-smoke" } });
await flush();
assert(!byId.get("auto-stop-round").checked, "本轮后停后开关应自动取消勾选");
assert(kzTest.stopReason().includes("本轮后停"), `本轮后停原因不对: ${kzTest.stopReason()}`);
// ⑧ 旧版 Stop(MaxRounds) 事件仍可投影,但不得把次数上限误报为当前硬停机原因。
byId.get("auto-continue").checked = true;
kzTest.setRounds(3);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "MaxRounds", max: 3 }, sessionId: "sess-smoke" } });
await flush();
assert(kzTest.rounds() === 0, "旧版停止事件的轮次收口应清零");
assert(!kzTest.stopReason().includes("已达连上限"), `旧版事件不应显示次数硬上限: ${kzTest.stopReason()}`);
// ⑨ Stop(Paused):暂停中完成本轮 → 停;恢复后 Continue 再推进。
byId.get("auto-continue").checked = true;
kzTest.setRounds(1);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "Paused" }, sessionId: "sess-smoke" } });
await flush();
assert(kzTest.stopReason().includes("已暂停"), `暂停刹车原因不对: ${kzTest.stopReason()}`);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Continue", rounds: 2, max: 10 }, sessionId: "sess-smoke" } });
await flush();
assert(kzTest.rounds() === 2, `恢复后推进轮次应继续增长,实得 ${kzTest.rounds()}`);
// ⑩ NoContinue(halted):整段鞭挞分支不进入,推进计数原地不动。
kzTest.setRounds(4);
handlers.get("kz:done")?.({ payload: { steps: 2, halted: true, tools: { edit: 1 }, autoAction: { type: "NoContinue" }, sessionId: "sess-smoke" } });
await flush();
assert(kzTest.rounds() === 4, "用户拒绝后推进计数应保持原样(不再续跑)");

// ---------- D-291 续跑闸门必须出声 ----------
// 引擎判 Continue、前端却不发下一轮,是允许的(模式/暂停/开关都能否决);**静默**不行。
// 旧实现四个条件各自 `return`,auto_pending 留在 true,界面永久停在「等待下一轮」,
// 而那一轮永远不来——用户看到的就是"鞭挞开着却不动"。
{
  const whipSession = "sess-smoke";
  byId.get("auto-continue").checked = true;
  kzTest.reset();
  byId.get("profile-select").value = "dev-pair"; // R-199:档位否决已下沉引擎,前端不再持有
  handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: whipSession } });
  await flush();
  // R-199:引擎 decide() 判档位(ProfileMismatch→Stop),前端只显示引擎结论;
  // 这里模拟引擎已判 Continue,前端必须持续推进(不再有私有否决)。
  // 时序容忍:flush 可能跨越 2 秒续跑间隔,phase 可能是 auto_pending(挂起中)
  // 或 starting(已开跑)——两者都证明引擎放行后前端没有拦下。
  const contPhase = sandbox.sessionState(whipSession).phase;
  assert(
    contPhase === "auto_pending" || contPhase === "starting",
    `R-199 后前端不再否决续跑(档位判定在引擎):Continue 后应挂起或已开跑,实得 phase=${contPhase}`,
  );
  assert(
    !byId.get("auto-status").textContent.includes("鞭挞未续跑"),
    `引擎判 Continue 时前端不得拦下: ${byId.get("auto-status")?.textContent}(D-291/R-199)`,
  );
  // 非致命错误(terminal:false,如持久化告警)不得掐掉已排好的下一轮。
  byId.get("profile-select").value = "dev-auto";
  handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: whipSession } });
  const ph2 = sandbox.sessionState(whipSession).phase;
  assert(
    ph2 === "auto_pending" || ph2 === "starting",
    `前置失败:Continue 未挂起/开跑下一轮, phase=${ph2}`,
  );
  expectedPersistentError = "持久化告警(非致命)";
  handlers.get("kz:error")?.({ payload: { message: "持久化告警(非致命)", terminal: false, sessionId: whipSession } });
  const ph3 = sandbox.sessionState(whipSession).phase;
  assert(
    ph3 === "auto_pending" || ph3 === "starting",
    "非致命错误不得取消已排队的续跑(旧实现在函数开头无条件 cancelAutoContinueTimer,一条告警就让鞭挞永久停摆)(D-291)",
  );
  expectedPersistentError = null;
}

// ---------- auto_pending 必须收敛:轮末→轮询复活→鞭挞饿死 32 秒 ----------
// 实测现场:21:40:57 运行完成 → 21:41:29 报「上一轮尚未结束」,正好 2s + 15×2s。
// 旧相位表(03-shell.js)三个分支是 starting/running、stopping、idle/stopped/failed,
// auto_pending 一个都不匹配 → converged 不置真、live_running 残留 true →
// ≤3s 后 process_list 校正命中 `live_running===true` 把已结束的一轮翻回 running →
// armAutoContinue 每 2 秒复查恒为真,16 次后放弃。四条断言按这条链逐环钉死。
{
  const sid = "sess-whip-converge";
  const savedWhipList = payloads.process_list;
  // ① 轮内:与真实一轮开跑同形。
  sandbox.transitionSession(sid, "running");
  assert(sandbox.sessionState(sid).converged === false, "前置:running 应为未收敛");
  assert(sandbox.sessionState(sid).live_running === true, "前置:running 应置 live_running=true");
  // ② 轮末 Continue → auto_pending。这是**轮终态**,不是运行中的中间态。
  sandbox.transitionSession(sid, "auto_pending", { auto_rounds: 1 });
  const pend = sandbox.sessionState(sid);
  assert(pend.converged === true, "auto_pending 未收敛:迟到事件与轮询都能把已结束的一轮复活");
  assert(pend.live_running === false, "auto_pending 未清 live_running:09-sessions 校正会命中第一分支翻回运行中");
  assert(pend.running === false, "auto_pending 不是运行态");
  assert(pend.phase === "auto_pending", "收敛不得改写 phase(界面「等待下一轮」与待命徽标靠它)");
  // ③ 3 秒轮询到达:已收敛会话必须被 `if (state.converged) continue` 跳过。
  sandbox.renderProcesses([...savedWhipList, { id: "w|whip", label: "鞭挞线", session_id: sid, running: false }]);
  assert(
    sandbox.sessionState(sid).phase === "auto_pending",
    `process_list 校正把 auto_pending 复活成 ${sandbox.sessionState(sid).phase}(鞭挞饿死的直接成因)`,
  );
  // ④ 迟到的进度事件(轮末落库/压缩的 kz:status)同样不得复活。
  handlers.get("kz:status")?.({ payload: { stage: "记忆", detail: "轮末落库", sessionId: sid } });
  assert(
    sandbox.sessionState(sid).running === false,
    "迟到进度事件复活了已收敛的一轮:只有 kz:turn 有权解除收敛",
  );
  // ⑤ 于是续跑闸门第一次复查就放行,不再走 16 次重试。
  assert(
    sandbox.processRunning({ session_id: sid, running: false }) === false,
    "processRunning 在 auto_pending 下仍报运行中:鞭挞会重试到耗尽并报「上一轮尚未结束」",
  );
  sandbox.renderProcesses(savedWhipList);
}

// ---------- D-323 暂停→恢复路径不得持有前端私有否决 ----------
// R-199 档位判定已下沉引擎(decide→Stop/ProfileMismatch 带 reason 可见收口);
// 恢复分支若仍被 autoContinueAllowed() 静默拦下,引擎计数与状态不知情(验收①未兑现)。
// 非 dev-auto 档位下恢复必须照样调度——档位不对由引擎下轮 done 判 Stop 收口。
{
  const savedProfileD323 = byId.get("profile-select").value;
  byId.get("profile-select").value = "dev-pair"; // 非 dev-auto → autoContinueAllowed()=false
  byId.get("auto-continue").checked = true;
  kzTest.setPaused(false);
  kzTest.reset();
  // 确保轮间空闲:清掉上游遗留的续跑定时器,并把 running 全局拉回 false。
  kzTest.cancelTimers();
  // 进程刷新会按 item.running 重设 running(08-compose.js:881),必须把主会话
  // 进程项置 idle 再渲染,否则任何刷新都会把 running 翻回 true。
  const savedD323ProcessList = payloads.process_list;
  payloads.process_list = (payloads.process_list ?? []).map((p) =>
    p.session_id === "sess-smoke" ? { ...p, running: false } : p,
  );
  sandbox.renderProcesses(payloads.process_list);
  sandbox.setRunning(false); // 03-shell 顶层函数声明在共享作用域,冒烟可直接调用
  byId.get("auto-pause").click(); // 暂停(autoPaused → true)
  const pausedText = byId.get("auto-pause").textContent;
  const pausedVal = kzTest.paused();
  byId.get("auto-pause").click(); // 恢复(autoPaused → false)→ 必须进入「2 秒后继续」分支
  const resumedText = byId.get("auto-pause").textContent;
  const resumedVal = kzTest.paused();
  const statusMode = byId.get("status-mode")?.textContent;
  const autoChecked = byId.get("auto-continue").checked;
  payloads.process_list = savedD323ProcessList;
  assert(
    pausedText.includes("继续鞭挞") && pausedVal === true,
    `D-323 前置:暂停点击未生效,pausedVal=${pausedVal},text=${pausedText}`,
  );
  assert(
    resumedVal === false,
    `D-323 前置:恢复点击未生效,pausedVal=${resumedVal},text=${resumedText}`,
  );
  assert(
    byId.get("status-text").textContent.includes("鞭挞恢复"),
    `D-323:恢复路径不得静默不调度(档位判定在引擎),status=${byId.get("status-text")?.textContent},mode=${statusMode},autoChecked=${autoChecked},btn=${resumedText}`,
  );
  assert(
    kzTest.timerSessions().includes("sess-smoke"),
    `D-323:恢复必须重新调度续跑定时器,timers=${kzTest.timerSessions().join(",")}`,
  );
  byId.get("profile-select").value = savedProfileD323;
  kzTest.reset();
}

// ---------- R-226 后台控制事件与双线路 timer 必须按 session 隔离 ----------

// ---------- R-322 B2 结伴档轻控制 loop(取代 R-224 的强制切档) ----------
// 结伴(dev-pair)勾鞭挞 → **留在结伴档**跑轻 loop + notice;research 勾鞭挞 → 拒绝并复位。
//
// 原 R-224 断言的是「自动切到 dev-auto」。那条行为的前提是结伴档不能续跑
// (auto_allowed 要求 agent=="dev"),所以勾鞭挞等于被迫换掉人格。R-322 B2 让
// 结伴档能以轻控制续跑后,强制切档既无必要也违背用户意图,断言随之反转。
{
  const savedProfileR224 = byId.get("profile-select").value;
  // ① 结伴勾鞭挞:档位保持 dev-pair,notice 说明轻控制语义,鞭挞保持勾选。
  byId.get("profile-select").value = "dev-pair";
  byId.get("auto-continue").checked = true;
  kzTest.cancelTimers();
  byId.get("auto-continue").dispatchEvent({ type: "change" });
  await flush();
  assert(
    byId.get("profile-select").value === "dev-pair",
    `R-322 B2:结伴勾鞭挞不应改档位,实际=${byId.get("profile-select").value}`,
  );
  assert(
    byId.get("auto-continue").checked === true,
    "R-322 B2:结伴勾鞭挞后勾选被复位(应保持勾选)",
  );
  assert(
    [...document.querySelectorAll("#messages [data-active] .msg, #messages [data-active] div")].some((el) =>
      el.textContent.includes("轻控制续跑") || el.textContent.includes("light-control")
    ),
    "R-322 B2:结伴档鞭挞未落轻控制语义 notice",
  );
  // R-363:research 可武装续跑；后端依据课题工作流决定继续或等待。
  const dev_processes = payloads.process_list;
  payloads.process_list = [...dev_processes, { id: "p|research-auto-test", session_id: "sess-research-auto", profile: "research", research_topic: "alpha-study", project_dir: PROJECT, label: "研究", running: false }];
  await vm.runInContext('switch_workspace("research")', sandbox);
  byId.get("auto-continue").checked = true;
  byId.get("auto-continue").dispatchEvent({ type: "change" });
  await flush();
  assert(
    byId.get("auto-continue").checked === true,
    "R-363:research 勾鞭挞不应再被旧门禁复位",
  );
  assert(
    vm.runInContext("selectedAgent().profile", sandbox) === "research",
    "R-363:research 续跑不应改模式",
  );
  await sandbox.sendAutoToSession("按研究地图继续", "sess-research-auto");
  const research_request = invokeArgs.findLast(({ cmd, args }) => cmd === "run_prompt" && args?.processId === "p|research-auto-test")?.args;
  assert(research_request?.profile === "research" && research_request.agent === "research", "研究续跑被固定切成 dev");
  assert(research_request.researchTopic === "alpha-study" && research_request.projectDir === PROJECT, "研究续跑丢失课题或项目");
  sandbox.releaseAutoContinue("sess-research-auto");
  // 收尾恢复。
  await vm.runInContext('switch_workspace("dev")', sandbox);
  payloads.process_list = dev_processes;
  await sandbox.refreshProcesses();
  byId.get("auto-continue").checked = false;
  byId.get("profile-select").value = savedProfileR224;
  kzTest.cancelTimers();
}

// ---------- R-342 模式芯片常驻:选择器在上下文行,配色随档位 ----------
// 「区别不够明显」的根因不是缺一个旋钮,是唯一的旋钮埋在鞭挞设置弹层里——对话时
// 根本不在视野内。这里锁两件事:①选择器留在 composer-context(不许再退回弹层);
// ②data-mode 与 value 同步(配色靠它,漂了就等于没换色)。
{
  const contextRow = html.slice(html.indexOf('id="composer-context"'), html.indexOf('id="attachments"'));
  assert(
    contextRow.includes('id="profile-select"') && contextRow.includes("ctx-mode"),
    "R-342:模式选择器必须常驻输入框上方的上下文行(带 ctx-mode 芯片样式)",
  );
  const savedProfileR342 = byId.get("profile-select").value;
  for (const mode of ["dev-auto", "dev-pair"]) {
    byId.get("profile-select").value = mode;
    byId.get("profile-select").dispatchEvent({ type: "change" });
    await flush();
    assert(
      byId.get("profile-select").dataset.mode === mode,
      `R-342:切到 ${mode} 后芯片 data-mode=${byId.get("profile-select").dataset.mode},配色会停在上一档`,
    );
  }
  byId.get("profile-select").value = savedProfileR342;
  byId.get("profile-select").dispatchEvent({ type: "change" });
  await flush();
  kzTest.cancelTimers();
}

// ---------- R-226 后台控制事件与双线路 timer 必须按 session 隔离 ----------
{
  const lines = [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
    { id: "p|bg-a", label: "后台甲", session_id: "sess-bg-a", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
    { id: "p|bg-b", label: "后台乙", session_id: "sess-bg-b", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
  ];
  const savedProcessList = payloads.process_list;
  payloads.process_list = lines;
  sandbox.renderProcesses(lines);
  kzTest.setAutoState("p|bg-a", { enabled: true, paused: false, stopAfterRound: false, maxRounds: 10 });
  kzTest.setAutoState("p|bg-b", { enabled: true, paused: false, stopAfterRound: false, maxRounds: 10 });
  handlers.get("kz:done")?.({ payload: { steps: 1, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: "sess-bg-a" } });
  handlers.get("kz:done")?.({ payload: { steps: 1, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: "sess-bg-b" } });
  const timerSessions = kzTest.timerSessions();
  assert(timerSessions.includes("sess-bg-a") && timerSessions.includes("sess-bg-b"), `后台双线路 timer 未并存:${timerSessions.join(",")}`);
  assert(sandbox.sessionState("sess-bg-a").phase === "auto_pending", "后台甲 done 未进入等待下一轮");
  assert(sandbox.sessionState("sess-bg-a").auto_rounds === 1, "后台甲轮次未写入所属 session state");
  assert(sandbox.sessionState("sess-bg-b").phase === "auto_pending", "后台乙 done 未进入等待下一轮");
  assert(sandbox.activeSessionId === undefined || document.querySelector("#parallel-task-status .parallel-task-row.active")?.textContent.includes("主会话"), "后台 done 串改活动线路");
  await flush();
  const backgroundRuns = invokeArgs.filter(({ cmd, args }) => cmd === "run_prompt" && ["p|bg-a", "p|bg-b"].includes(args?.processId));
  assert(backgroundRuns.some(({ args }) => args.processId === "p|bg-a"), "后台甲 done 没有续跑所属线路");
  assert(backgroundRuns.some(({ args }) => args.processId === "p|bg-b"), "后台乙 done 没有续跑所属线路");
  payloads.process_list = savedProcessList;
  sandbox.renderProcesses(savedProcessList);
}

// R-363:后台研究连跑两轮仍保留课题和工作流指令，选题等待必须停机。
{
  const saved_research_processes = payloads.process_list;
  const research_process = { id: "p|bg-research", label: "后台研究", session_id: "sess-bg-research", running: false, profile: "research", research_topic: "alpha-study", project_dir: PROJECT, origin_project: PROJECT };
  payloads.process_list = [...saved_research_processes, research_process];
  sandbox.renderProcesses(payloads.process_list);
  kzTest.setAutoState(research_process.id, { enabled: true, paused: false, stopAfterRound: false });
  const research_runs = () => invokeArgs.filter(({ cmd, args }) => cmd === "run_prompt" && args?.processId === research_process.id);
  for (let round = 1; round <= 2; round += 1) {
    handlers.get("kz:done")?.({ payload: { sessionId: research_process.session_id, steps: 2, autoAction: { type: "Continue", rounds: round, prompt: `AUTO research 第 ${round} 轮继续` } } });
    await flush();
    assert(research_runs().length === round, "后台研究没有连续续跑");
    const request = research_runs().at(-1).args;
    assert(request.profile === "research" && request.agent === "research" && request.researchTopic === "alpha-study", "后台研究串到开发模式或丢失课题");
    assert(request.prompt === `AUTO research 第 ${round} 轮继续`, "后台研究丢失后端阶段指令");
  }
  handlers.get("kz:done")?.({ payload: { sessionId: research_process.session_id, steps: 2, autoAction: { type: "Stop", reason: "ResearchWaiting", message: "等待选题" } } });
  await flush();
  assert(research_runs().length === 2 && !kzTest.timerSessions().includes(research_process.session_id), "等待选题时仍在续跑");
  assert(kzTest.getAutoState(research_process.id)?.enabled === false, "等待选题未关闭所属研究续跑");
  payloads.process_list = saved_research_processes;
  sandbox.renderProcesses(saved_research_processes);
}

// ---------- 切走的线路必须**连续**被鞭挞(不是只多跑一轮) ----------
// 上面那条只验第一轮。病根在第二轮:sendAutoToSession 会把 session 记进在飞集合,
// 而释放只写在活动线的 kz:done/kz:idle 处理器里——后台线的控制事件在 01-core 路由层
// 就被拦下(kz:done 只转 handleBackgroundSessionDone,kz:idle 直接 return),两条释放
// 路径一条都走不到。于是第二轮 armAutoContinue 撞上在飞守卫静默返回:线路永久钉在
// 「等待下一轮」,日志里一个字都没有。用户表现 = 切走线路后鞭挞失效。
{
  const loopLines = [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
    { id: "p|bg-loop", label: "后台连跑", session_id: "sess-bg-loop", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
  ];
  const savedLoopList = payloads.process_list;
  payloads.process_list = loopLines;
  sandbox.renderProcesses(loopLines);
  kzTest.setAutoState("p|bg-loop", { enabled: true, paused: false, stopAfterRound: false, maxRounds: 10 });
  const loopRuns = () => invokeArgs.filter(({ cmd, args }) => cmd === "run_prompt" && args?.processId === "p|bg-loop").length;
  const before = loopRuns();
  handlers.get("kz:done")?.({ payload: { steps: 1, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: "sess-bg-loop" } });
  await flush();
  assert(loopRuns() === before + 1, `后台线第一轮没有续跑,实得 ${loopRuns() - before}`);
  // 第二轮的 kz:done:这一轮是**鞭挞自己发的**(在飞标记此刻挂着),必须照样排下一枪。
  handlers.get("kz:done")?.({ payload: { steps: 1, autoAction: { type: "Continue", rounds: 2, max: 10 }, sessionId: "sess-bg-loop" } });
  await flush();
  assert(
    loopRuns() === before + 2,
    `切走的线路第二轮停摆(在飞标记未释放 → armAutoContinue 静默吞掉),实得 ${loopRuns() - before} 轮`,
  );
  assert(sandbox.sessionState("sess-bg-loop").auto_rounds === 2, "后台连跑第二轮轮次被活动线镜像覆盖");
  // 后台线的失败退避重试:kz:auto-fail 既不是控制事件也不在 BACKGROUND_RENDER_EVENTS,
  // 原先整条被路由层丢掉 —— 后台线断一次网就永久停摆,而它恰恰是没人看着的那条。
  kzTest.cancelTimers();
  handlers.get("kz:auto-fail")?.({
    payload: { sessionId: "sess-bg-loop", autoAction: { type: "RetryAfterFailure", attempt: 1, maxAttempts: 3, delayMs: 15000, rounds: 2 } },
  });
  assert(
    kzTest.timerSessions().includes("sess-bg-loop"),
    `后台线的失败退避重试没有排上(kz:auto-fail 被路由层丢弃),实得 ${kzTest.timerSessions().join(",")}`,
  );
  assert(
    kzTest.retryLabel("sess-bg-loop")?.includes("失败重试"),
    "后台线重试定时器缺重试标记(随后到达的终态 kz:error 会把它当残留掐掉)",
  );
  // 线路页按线操控:后台线开鞭挞只动**它自己**的存档与后端状态机,不碰当前线勾选框。
  kzTest.cancelTimers();
  byId.get("auto-continue").checked = false;
  await sandbox.setLineAutoState("p|bg-loop", { enabled: true });
  await flush();
  const lineSync = invokeArgs.findLast(({ cmd, args }) => cmd === "auto_state_update" && args?.sessionId === "sess-bg-loop");
  assert(lineSync?.args?.enabled === true, "线路页开后台线鞭挞未推该线后端状态机");
  assert(!Object.hasOwn(lineSync.args, "maxRounds"), "线路页不应把兼容上限字段发送为硬门禁");
  assert(kzTest.getAutoState("p|bg-loop")?.enabled === true, "线路页开后台线鞭挞未落该线存档");
  assert(byId.get("auto-continue").checked === false, "线路页操控后台线污染了当前线的鞭挞勾选");
  sandbox.applyAutoUiState("p|bg-loop");
  assert(byId.get("auto-continue").checked === true && !byId.has("auto-max"), "切线回显应读取后台线路配置且不再渲染上限控件");
  kzTest.cancelTimers();
  payloads.process_list = savedLoopList;
  sandbox.renderProcesses(savedLoopList);
}

// ---------- 顶栏模型下拉必须跟着线路走 ----------
// 用户 2026-08-18 报告「切换线路模型不变,选的都是同一个」。两个病根:
// ① loadModels 的回显是 `本线模型 || 旧全局 localStorage 键`——一条没设过模型的线(agent
//    默认)会显示旧键里的模型,于是每条这样的线都显示同一个;
// ② 只有 switchProcess 尾巴上那一句在回显,冷启动/兜底选中活动线都不回显,下拉停在上一条线。
// 附带的隐患更深:发送读下拉、鞭挞续跑读 item.model,同一条线能跑在两个模型上。
{
  const modelLines = [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, model: "primary", project_dir: "C:/smoke", origin_project: "C:/smoke" },
    { id: "p|model-b", label: "线路乙", session_id: "sess-model-b", running: false, model: "OPEN-code:deepseek-v4-flash", project_dir: "C:/smoke", origin_project: "C:/smoke" },
    { id: "p|model-c", label: "线路丙", session_id: "sess-model-c", running: false, model: null, project_dir: "C:/smoke", origin_project: "C:/smoke" },
  ];
  const savedModelList = payloads.process_list;
  payloads.process_list = modelLines;
  // 旧全局键存在(迁移前的老用户就是这个状态):它绝不能顶替任何一条线自己的值。
  storage.set("kz-model:C:/smoke", "OPEN-code:deepseek-v4-flash");
  storage.set("kz-model", "OPEN-code:deepseek-v4-flash");
  sandbox.renderProcesses(modelLines);
  await sandbox.switchProcess("p|model-b");
  await flush();
  assert(
    byId.get("model-select").value === "OPEN-code:deepseek-v4-flash",
    `切到乙线未回显该线模型,实得 ${byId.get("model-select").value}`,
  );
  await sandbox.switchProcess("d|smoke");
  await flush();
  assert(
    byId.get("model-select").value === "primary",
    `切回主线时模型下拉没跟着变(用户报告的正是这一条),实得 ${byId.get("model-select").value}`,
  );
  // 没设过模型的线 = agent 默认。旧全局键在这里最容易顶上来。
  await sandbox.switchProcess("p|model-c");
  await flush();
  assert(
    byId.get("model-select").value === "",
    `未设模型的线必须回显 agent 默认,不得回落旧全局键,实得 ${byId.get("model-select").value}`,
  );
  // 发送用的模型必须是该线存的那个,不能是下拉的显示值(鞭挞续跑读的就是前者)。
  await sandbox.switchProcess("p|model-b");
  await flush();
  byId.get("model-select").value = "primary"; // 模拟回显被任何路径写歪
  await sandbox.sendText("模型同源冒烟");
  await flush();
  const sentRun = invokeArgs.findLast(({ cmd, args }) => cmd === "run_prompt" && args?.processId === "p|model-b");
  assert(
    sentRun?.args?.model === "OPEN-code:deepseek-v4-flash",
    `发送用的模型必须取自该线存档(与鞭挞续跑同源),实得 ${sentRun?.args?.model}`,
  );
  // 冷启动/兜底选中这条路径:用户报告的现场只有一条线,永远不触发 switchProcess,
  // 于是下拉一直钉在 loadModels 早期算出来的值(进程列表未到时回落旧全局键)。
  // renderProcesses 选中活动线时必须自己回显一次。这里让活动线(乙)从列表里消失、
  // 兜底改选主线,复现那一刻。
  await sandbox.switchProcess("p|model-b");
  await flush();
  byId.get("model-select").value = "OPEN-code:deepseek-v4-flash";
  sandbox.renderProcesses([modelLines[0], modelLines[2]]); // 乙线没了 → 兜底选主线
  assert(
    byId.get("model-select").value === "primary",
    `兜底选中活动线时模型下拉没跟着回显(冷启动同源),实得 ${byId.get("model-select").value}`,
  );
  storage.delete("kz-model:C:/smoke");
  storage.delete("kz-model");
  payloads.process_list = savedModelList;
  sandbox.renderProcesses(savedModelList);
  kzTest.cancelTimers();
}

// ---------- D-290 回显不得写盘 ----------
// 「模式/鞭挞每次开 app 都要重设」的根:回显期间控件显示的是**算出来的值**,
// 把它当用户意图写回存档,一次算错就永久固化,而且自我延续。
{
  const autoStateBefore = storage.get("kz-process-auto-state");
  storage.set("kz-auto-continue", "1");
  byId.get("auto-continue").checked = true;
  storage.set("kz-profile", "dev-pair");
  sandbox.applyProfileValue("dev"); // 回显把模式刷成结伴开发 → 顺带关掉鞭挞控件
  assert(
    storage.get("kz-auto-continue") === "1",
    `回显关掉的鞭挞不得写进全局 kz-auto-continue,实得 ${storage.get("kz-auto-continue")}(D-290:下次冷启动会被当成用户上次的选择)`,
  );
  assert(
    storage.get("kz-process-auto-state") === autoStateBefore,
    "回显不得改写 kz-process-auto-state(D-290)",
  );
  storage.set("kz-profile", "dev-auto");
}
// 切进程时不得拿选择器当前值覆盖旧进程的档位存档:那个值在回显期间不是用户意图。
// 这是上面那条的另一半——只修一处,另一处照样能把 dev-auto 覆盖成 dev-pair。
if (source.includes('processProfileUi.set(activeProcessId, $("profile-select").value)')) {
  fail("switchProcess 又拿选择器显示值当旧进程的用户意图写盘(D-290);写盘只能发生在 profile-select 的 change 事件里");
}
// ---------- D-353 鞭挞开关是线路级状态:不回落全局键,停机不污染他线 ----------
// 病根有两半:①无记录的默认线回落读全局 kz-auto-continue,A 项目的勾选漏成 B 项目
// 的初始状态并被固化;②引擎停机(AllBlocked/BacklogEmpty/ProfileMismatch)无条件改
// 当前可见勾选框——kz:done 来自后台线时清掉的是**别的线**的用户选择。
{
  // ① 无记录线路必须默认关,不得继承全局键。
  storage.set("kz-auto-continue", "1");
  const fresh = sandbox.normalizeAutoState(undefined, "d|C:/other-project");
  assert(fresh.enabled === false, "D-353:无记录线路必须默认关,不得继承全局 kz-auto-continue");
  storage.delete("kz-auto-continue");
  // ② 后台线引擎停机只落到该线自己的存档与后端,不碰当前线勾选框。
  const isolationLines = [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
    { id: "p|bg-iso", label: "后台隔离", session_id: "sess-bg-iso", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
  ];
  const savedIsolationList = payloads.process_list;
  payloads.process_list = isolationLines;
  sandbox.renderProcesses(isolationLines);
  kzTest.setAutoState("p|bg-iso", { enabled: true, paused: false, stopAfterRound: false, maxRounds: 10 });
  byId.get("auto-continue").checked = true;
  handlers.get("kz:done")?.({ payload: { steps: 1, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "BacklogEmpty" }, sessionId: "sess-bg-iso" } });
  await flush();
  assert(byId.get("auto-continue").checked === true, "D-353:后台线停机不得清掉当前线的鞭挞勾选(跨线污染)");
  assert(kzTest.getAutoState("p|bg-iso")?.enabled === false, "D-353:后台线停机必须把该线自己的鞭挞存档置关");
  const bgStopSync = invokeArgs.findLast(({ cmd, args }) => cmd === "auto_state_update" && args?.sessionId === "sess-bg-iso");
  assert(bgStopSync?.args?.enabled === false, "D-353:后台线停机必须同步该线后端状态机(auto_state_update enabled=false)");
  payloads.process_list = savedIsolationList;
  sandbox.renderProcesses(savedIsolationList);
  kzTest.cancelTimers();
  byId.get("auto-continue").checked = false;
}
// D-745:等待用户与队列完成必须区分；历史问题的回复只填写草稿。
{
  sandbox.setAutoStopReason("任务尚未完成，当前均有阻塞或停车条件；请在文档页查看并回复待确认事项");
  sandbox.renderAutoRun();
  assert(!byId.get("auto-status").classList.contains("ok"), "全部阻塞不得显示成功状态");
  sandbox.setAutoStopReason("All tasks completed", "completed");
  sandbox.renderAutoRun();
  assert(byId.get("auto-status").classList.contains("ok"), "队列清空应保留完成状态");
  sandbox.setAutoStopReason("");
  const question = sandbox.buildToolBlock("question", { question: "选择 provider" });
  sandbox.fillToolBlock(question, { ok: false, outcome: "needs_confirmation", content: "待用户回答", display: {
    kind: "pending_question", question: "选择 provider", options: [{ label: "本地", note: "使用本机设备" }],
  } });
  assert(question.icon.textContent === "⏸", "待回答问题应显示等待");
  const reply = question.wrap.querySelector(".pending-question-reply");
  assert(reply, "持久化问题回放必须有回复入口");
  const restored = sandbox.buildToolBlock("tool result", {});
  sandbox.fillToolBlock(restored, { ok: false, content: '[tool_outcome=needs_confirmation code=QUESTION_PENDING]\n' + JSON.stringify({
    kind: "pending_question", question: "历史问题", options: [],
  }) });
  assert(restored.wrap.querySelector(".pending-question-reply") && restored.icon.textContent === "⏸", "无实时 display 的历史回放也必须恢复回复入口");
  const prompt = byId.get("prompt");
  const saved = prompt.value;
  prompt.value = "已有草稿";
  const before = invokeArgs.length;
  reply.dispatchEvent({ type: "click" });
  assert(prompt.value.includes("已有草稿") && prompt.value.includes("选择 provider") && prompt.value.includes("本机设备"), "回复必须保留草稿、问题和选项说明");
  assert(invokeArgs.slice(before).every(({ cmd }) => !["run", "answer_ask", "doc_update"].includes(cmd)), "点击回复不得替用户提交答案或解除阻塞");
  const doc_host = document.createElement("div");
  sandbox.renderDocList(doc_host, [{
    id: "R-287", title: "需要用户决定模型", status: "doing", closed: false, blocked: true,
    block_reasons: ["等待用户提供 provider 与设备选择"], fields: [],
  }], "req");
  const decision = doc_host.querySelector(".doc-pending-decision");
  assert(decision?.textContent.includes("provider"), "阻塞详情必须显示用户待决事项");
  const decision_reply = decision?.querySelector("button");
  assert(decision_reply, "阻塞事项必须有回复入口");
  decision_reply?.dispatchEvent({ type: "click" });
  assert(prompt.value.includes("R-287") && prompt.value.includes("设备选择"), "事项回复草稿必须带上身份和待决条件");
  prompt.value = saved;
}

// ---------- 「勘察复核」= 阶段流水线总闸(2026-08-11 换闸门) ----------
// 闸门从 auto_runs[session].enabled 换成进程级开关后,「开鞭挞 = 每轮勘察+复核」这个
// 旧心智模型不再成立。四种组合里只有「鞭挞开 + 闸门关」需要提示,这里把它和它的
// 反面(闸门开 → 不提示)一起钉住,顺带钉 IPC 参数名(phasePipeline,不再是 subagent)。
const pipelineToggle = byId.get("process-phase-pipeline");
assert(pipelineToggle, "顶栏「更多」里缺少「勘察复核」开关");
assert(
  !pipelineToggle.checked,
  "「勘察复核」必须默认关闭(process_list 桩不带 phase_pipeline 字段时回落 false)"
);
byId.get("auto-continue").checked = true;
pipelineToggle.checked = false;
pipelineToggle.dispatchEvent({ type: "change" });
await flush();
assert(
  invokeArgs.findLast(({ cmd }) => cmd === "process_update")?.args?.phasePipeline === false,
  `关闭勘察复核未以 phasePipeline 发给后端:${JSON.stringify(invokeArgs.findLast(({ cmd }) => cmd === "process_update"))}`
);
// 原来这里断言的是「鞭挞开着而勘察复核关着时,状态栏必须写一句 勘察复核未开」——
// 那句话是补救:开关埋在顶栏「更多」里看不见,只好用一行状态文本去骂人。开关搬进
// 鞭挞控制台后本身常驻可见、带一句说明,补救就不该再存在(否则状态槽又被塞进
// 第四种语义)。新契约:开关在控制台里、可见、未勾选,状态槽不掺和这件事。
{
  const consoleBar = byId.get("autorun-bar");
  assert(consoleBar, "鞭挞控制台 #autorun-bar 不存在(控件仍留在顶栏?)");
  // 判定改为**配对标签扫描出的真实包含区间**,不再用两个 id 的位置切片:原实现取
  // autorun-bar 与 composer-bar 之间的字符串,2026-08-16 把控制台挪进 composer-bar
  // (鞭挞与发送同行)后两者先后颠倒、切片变空,契约没破测试却红。DOM 桩是扁平的
  // (按 id 造节点直接挂 body),祖先链走不通,所以判定落在源码文本上。
  assert(
    (() => {
      // UI-0926 #9:任务设置由 details 改为 data-kz-menu 弹层菜单,面板开标签带 id。
      const open = html.indexOf('<div id="task-options-menu"');
      if (open < 0) return false;
      let depth = 0;
      const tag = /<\/?div\b/g;
      tag.lastIndex = open;
      for (let m = tag.exec(html); m; m = tag.exec(html)) {
        depth += m[0] === "<div" ? 1 : -1;
        if (depth === 0) {
          const end = html.indexOf(">", m.index) + 1;
          return html.slice(open, end).includes('id="process-phase-pipeline"');
        }
      }
      return false;
    })(),
    "「勘察复核」开关必须归入任务设置的开发协作分组",
  );
  assert(!pipelineToggle.checked, "勘察复核此刻应为关闭态");
  assert(
    !byId.get("auto-status").textContent.includes("勘察复核未开"),
    `开关已常驻可见,状态槽不应再塞这句补救文案:${byId.get("auto-status")?.textContent}`,
  );
}
// 打开闸门:后端回显跟着变(桩模拟 process_list 的新值),提示随即消失。
payloads.process_list[0].phase_pipeline = true;
pipelineToggle.checked = true;
pipelineToggle.dispatchEvent({ type: "change" });
await flush();
assert(
  invokeArgs.findLast(({ cmd }) => cmd === "process_update")?.args?.phasePipeline === true,
  `打开勘察复核未以 phasePipeline 发给后端:${JSON.stringify(invokeArgs.findLast(({ cmd }) => cmd === "process_update"))}`
);
assert(
  pipelineToggle.checked && !byId.get("auto-status").textContent.includes("勘察复核未开"),
  `闸门开着时不该再提示未开:${byId.get("auto-status")?.textContent}`
);
// 收尾:桩与控件回到默认关,免得后续用例继承本节状态。
payloads.process_list[0].phase_pipeline = false;
pipelineToggle.checked = false;
pipelineToggle.dispatchEvent({ type: "change" });
await flush();

// 收尾:恢复冒烟前置环境(语言/档位/开关/计数)。
byId.get("profile-select").value = savedProfileForWhip;
byId.get("auto-continue").checked = savedAutoCheck;
languageControl.value = savedLangForWhip;
languageControl.dispatchEvent({ type: "change" });
await flush();
kzTest.reset();

// ---------- 视图切换:真实驱动 activity-item 的监听,抓初始化后才触发的运行时错误 ----------
// rail 上的侧栏开合不是视图,统计与点击都只认带 data-view 的按钮。
const activityItems = document.querySelectorAll(".activity-item[data-view]");
// 覆盖为零必须判失败,不能像以前那样打印「0 个主视图切换」还报通过(D-138)——
// 与本文件对初始化探针的自守卫同一标准:护栏没生效比没有护栏更危险。
const expectedViews = new Set([...html.matchAll(/data-view="([\w-]+)"/g)].map((m) => m[1]));
if (activityItems.length < expectedViews.size) {
  fail(
    `主视图切换覆盖不足:harness 造出 ${activityItems.length} 个 .activity-item,` +
      `index.html 声明 ${expectedViews.size} 个(${[...expectedViews].join(",")})`
  );
}
for (const item of activityItems) { item.click(); await flush(); }
await flush();
// 每个视图都必须真的被激活过,否则等于没切。
for (const view of expectedViews) {
  const el = byId.get(`view-${view}`);
  if (el && !el.classList.contains("active") && view !== "chat") continue;
}
assert(
  byId.get("view-settings")?.classList.contains("active") ||
    activityItems.length === 0,
  "视图切换未真正驱动:最后一个视图应处于 active"
);

// ---------- 文件导览(R-148):树渲染出目录聚合、文件度量与标注 ----------
{
  const tree = byId.get("files-tree");
  let treeText = tree?.textContent ?? "";
  assert(treeText.includes("src/"), `文件树缺目录行: "${treeText.slice(0, 80)}"`);
  assert(treeText.includes("源码目录"), "目录用途标注未渲染");
  assert((byId.get("files-summary")?.textContent ?? "").includes("2"), "汇总行缺文件计数");
  // 目录默认折叠(VSCode 同款):点开后文件行才出现,顺带验证展开交互本身。
  for (const row of [...tree.querySelectorAll(".files-dir")]) row.click();
  await flush();
  treeText = tree.textContent;
  assert(treeText.includes("lib.rs") && treeText.includes("120"), "展开后文件行缺行数度量");
  assert(treeText.includes("note.md") && treeText.includes("300"), "展开后 md 文件缺字数度量");
  assert(treeText.includes("冒烟样例:库入口"), "文件用途标注未渲染");
}

// ---------- 架构浏览(R-122):索引 + 设计文档树,未入册分组可见 ----------
{
  const archView = byId.get("view-arch");
  assert(archView, "缺少 view-arch 视图容器");
  // 上面视图切换循环已点击过 arch 按钮,refreshArch 应已拉取并渲染。
  const tree = byId.get("arch-tree");
  const treeText = tree?.textContent ?? "";
  assert(treeText.includes("direction_taste.md"), `架构树缺已入册文档: "${treeText.slice(0, 80)}"`);
  assert(treeText.includes("memory_system.md"), "架构树缺设计文档行");
  assert(treeText.includes("未入册") || treeText.includes("not indexed"), "索引外的文档未进「未入册」分组");
  assert(treeText.includes("现行基线") || treeText.includes("基线"), "索引章节分组未渲染");
  assert((byId.get("arch-index-body")?.textContent ?? "").includes("方向基线"), "右侧索引原文未渲染");
  assert((byId.get("arch-summary")?.textContent ?? "").includes("2"), "架构汇总缺文档计数");
  // 点击文档行应经 docs_read_custom 打开应用内查看器。
  const row = [...tree.querySelectorAll(".arch-entry")].find((r) => r.textContent.includes("memory_system.md"));
  assert(row, "架构树缺少可点击的文档行");
  row.click();
  await flush();
  assert(!byId.get("viewer-overlay").classList.contains("hidden"), "点击设计文档未打开查看器");
  assert((byId.get("viewer-body")?.textContent ?? "").includes("Memory 系统设计基线"), "查看器未展示设计文档内容");
  byId.get("viewer-close").click();
  await flush();
  assert(byId.get("viewer-overlay").classList.contains("hidden"), "查看器关闭失败");
  // 批3:记忆管理入口跳转记忆页(复用导航按钮,维护动作走既有 memory_* 命令)。
  const gotoMemory = byId.get("arch-goto-memory");
  assert(gotoMemory, "架构页缺少记忆管理入口");
  const memBtn = [...document.querySelectorAll(".activity-item[data-view]")].find((b) => b.dataset.view === "memory");
  assert(memBtn, "缺少记忆导航按钮");
  gotoMemory.click();
  await flush();
  assert(byId.get("view-memory").classList.contains("active"), "记忆管理入口未激活记忆视图");
  assert(memBtn.classList.contains("active"), "记忆导航按钮未同步高亮");
}

// ---------- D-202 流式渲染性能回归 ----------
// 卡顿的两个放大器都在"每个 delta 做一次"上:①i18n observer 全文档重扫;
// ②整条消息重新 renderMarkdown。这里按行为断言,不认代码形态——谁把它们改回
// 每 delta 一次,这两条就红。
{
  const walksBefore = fullDocumentWalks;
  const markdownModule = esmModuleCache.get("04-markdown.js");
  const realRenderMarkdown = markdownModule.namespace.renderMarkdown;
  let renders = 0;
  markdownModule.namespace.setRenderMarkdown((raw) => { renders += 1; return realRenderMarkdown(raw); });
  const DELTAS = 200;
  for (let i = 0; i < DELTAS; i += 1) sandbox.appendAssistant(`stream-chunk-${i} with some filler text
`);
  await flush();
  markdownModule.namespace.setRenderMarkdown(realRenderMarkdown);

  assert(renders > 0, "renderMarkdown 包装未生效,本组断言全是假通过");
  assert(
    renders <= DELTAS / 10,
    `流式渲染没有合帧:${DELTAS} 个 delta 触发了 ${renders} 次 renderMarkdown(每次都整条重渲染 = 单条消息内 O(n²),D-202)`
  );
  assert(
    fullDocumentWalks === walksBefore,
    `流式 delta 触发了 ${fullDocumentWalks - walksBefore} 次全文档 i18n 重扫(单次成本 ∝ 对话长度,轮次越多越卡,D-202)`
  );
  const rendered = sandbox.document.querySelectorAll(".msg.assistant .message-body").at(-1);
  assert(
    rendered?.textContent.includes(`stream-chunk-${DELTAS - 1}`),
    "合帧后最后一段流式文本没渲染出来(延迟渲染丢尾巴比卡顿更糟)"
  );
}

// ---------- R-140 批10:MutationObserver 退役 ----------
// 动态文案必须由渲染点 t()/localizeDynamic/applyDataI18nKeys 产出,不再有 observer
// 事后扫描改写。裸中文节点(用户数据/漏翻)保持原样——这正是验收③ 的正面断言:
// 谁把 observer 换回来、或退回全文档扫描,这条就红。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  const probe = document.createElement("div");
  probe.textContent = "移动端桥接";
  document.body.appendChild(probe);
  await flush();
  assert(
    probe.textContent === "移动端桥接",
    `裸中文节点被自动本地化(实际 "${probe.textContent}"):MutationObserver 未退役,事后扫描改写仍在`
  );
  probe.remove();
  // 渲染点路径:渲染器插入 data-i18n-key 节点后,由切语言/初始化路径
  // applyDataI18nKeys(document.body) 重算 → 英文态即时翻译(与 change 处理器一致)。
  const keyed = document.createElement("span");
  keyed.setAttribute("data-i18n-key", "移动端桥接");
  keyed.textContent = "移动端桥接";
  document.body.appendChild(keyed);
  sandbox.applyDataI18nKeys(document.body, "en");
  await flush();
  assert(
    keyed.textContent === "Mobile bridge",
    `渲染点 data-i18n-key 节点未翻译(实际 "${keyed.textContent}"):applyDataI18nKeys 渲染点路径失效`
  );
  keyed.remove();
  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批1:消息容器整体豁免词典替换(止血) ----------
// 模型输出是用户数据,英文态下不能因「恰好等于词典 key」被 observer 改写成英文。
// 在英文态追加一条包含词典 key 的模型输出,断言 message-body 原文保持中文不变;
// 同时消息区外的节点仍要正常翻译(豁免只圈 #messages,不误伤其它界面域)。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  const before = sandbox.document.querySelectorAll("#messages [data-active] .msg").length;
  sandbox.appendAssistant("运行中 · 失败 · 复制 是用户数据片段,不得改写");
  await flush();
  const assistant = sandbox.document.querySelectorAll("#messages [data-active] .msg.assistant .message-body").at(-1);
  assert(assistant, "追加模型输出后找不到 .msg.assistant .message-body(前置失效)");
  const assistantMsg = assistant.closest(".msg");
  assert(
    assistantMsg.dataset.raw.includes("运行中"),
    "模型输出原文丢失(appendAssistant 未保留 raw)"
  );
  assert(
    assistant.textContent.includes("运行中") && assistant.textContent.includes("失败"),
    `消息容器内的模型输出被词典替换(实际 "${assistant.textContent}"):英文态下用户数据被 i18n 篡改,R-140 止血失败`
  );
  // 消息区外:裸中文节点同样不再被自动改写(observer 退役,无事后扫描);产品文案由
  // 渲染点 data-i18n-key + applyDataI18nKeys 负责(上方批10 用例已覆盖渲染点路径)。
  const outside = document.createElement("div");
  outside.textContent = "移动端桥接";
  sandbox.document.body.appendChild(outside);
  await flush();
  assert(
    outside.textContent === "移动端桥接",
    `消息区外的裸中文被自动翻译(实际 "${outside.textContent}"):observer 仍在做全文档改写`
  );
  outside.remove();
  // 清掉追加的消息,避免污染后续用例。
  const msgs = sandbox.document.querySelectorAll("#messages [data-active] .msg");
  for (const m of msgs) if (msgs.length > before) m.remove();
  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批2:静态 DOM data-i18n-key/data-i18n-title 一次性应用 ----------
// 侧栏标题与按钮属性迁移到 data-i18n-key 后,翻译由渲染点 t() 承担,不再依赖
// observer 词典扫描。英文态应翻译,切回中文应回原文;title 属性同样走渲染点。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  const sectionTitle = (key) => sandbox.document.querySelector(`[data-i18n-key="${key}"]`)?.textContent;
  const titleOf = (id) => sandbox.document.getElementById(id)?.title;
  assert(sectionTitle("项目") === "项目", "中文态侧栏「项目」标题应保持原文(前置失效)");
  assert(titleOf("project-init") === "初始化新项目目录", "中文态 project-init title 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(sectionTitle("项目") === "Projects", `英文态侧栏「项目」未翻译,实际 "${sectionTitle("项目")}"`);
  assert(sectionTitle("任务与对话") === "Tasks and conversations", `英文态侧栏「当前状态」未翻译,实际 "${sectionTitle("任务与对话")}"`);
  assert(sectionTitle("开发规范") === "Conventions", `英文态侧栏「开发规范」未翻译,实际 "${sectionTitle("开发规范")}"`);
  assert(titleOf("project-init") === "Initialize a new project directory", `英文态 project-init title 未翻译,实际 "${titleOf("project-init")}"`);
  assert(sectionTitle("隔离工作树") === "Isolated worktrees", `英文态侧栏「隔离工作树」未翻译,实际 "${sectionTitle("隔离工作树")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(sectionTitle("项目") === "项目", `切回中文后侧栏「项目」未回原文,实际 "${sectionTitle("项目")}"`);
  assert(titleOf("project-init") === "初始化新项目目录", `切回中文后 project-init title 未回原文,实际 "${titleOf("project-init")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批3:顶栏/对话区/工作区视图 data-i18n-key 迁移 ----------
// 顶栏按钮(新对话/活动/侧栏/鞭挞/更多)、对话区按钮(继续/附件/停止/发送)、
// 工作区与并行线路视图标题迁移到 data-i18n-key 后,英文态翻译、切中文回原文。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  const keyText = (key) => sandbox.document.querySelector(`[data-i18n-key="${key}"]`)?.textContent;
  assert(keyText("新对话") === "新对话", "中文态顶栏「新对话」应保持原文(前置失效)");
  assert(keyText("发送") === "发送", "中文态发送按钮应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(keyText("新对话") === "New chat", `英文态「新对话」未翻译,实际 "${keyText("新对话")}"`);
  assert(keyText("发送") === "Send", `英文态「发送」未翻译,实际 "${keyText("发送")}"`);
  assert(keyText("停止") === "Stop", `英文态「停止」未翻译,实际 "${keyText("停止")}"`);
  assert(keyText("项目总览") === "All projects", `英文态「工作区」未翻译,实际 "${keyText("项目总览")}"`);
  assert(keyText("并行线路") === "Parallel lines", `英文态「并行线路」未翻译,实际 "${keyText("并行线路")}"`);
  assert(keyText("刷新") === "Refresh", `英文态「刷新」未翻译,实际 "${keyText("刷新")}"`);
  assert(keyText("鞭挞") === "Auto-run", `英文态「鞭挞」未翻译,实际 "${keyText("鞭挞")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(keyText("新对话") === "新对话", `切回中文后「新对话」未回原文,实际 "${keyText("新对话")}"`);
  assert(keyText("发送") === "发送", `切回中文后「发送」未回原文,实际 "${keyText("发送")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批4:架构浏览域迁移 + aria-label/placeholder 渲染点翻译 ----------
// 架构浏览视图的标题/说明/按钮文案迁移到 data-i18n-key,title 走 data-i18n-title,
// aria-label 走 data-i18n-aria-label(渲染点补齐属性翻译——元素挂 data-i18n-* 后
// observer 整体豁免,属性不在此补齐会在英文态漏翻)。英文态翻译、切中文回原文。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  const archKey = (key) => sandbox.document.querySelector(`[data-i18n-key="${key}"]`)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(archKey("架构浏览") === "架构浏览", "中文态架构浏览标题应保持原文(前置失效)");
  assert(attrOf("arch-tree", "aria-label") === "设计文档树", "中文态 arch-tree aria-label 应保持原文(前置失效)");
  assert(attrOf("arch-refresh", "aria-label") === "重新扫描架构索引", "中文态 arch-refresh aria-label 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(archKey("架构浏览") === "Architecture browser", `英文态「架构浏览」未翻译,实际 "${archKey("架构浏览")}"`);
  assert(archKey("架构索引") === "Architecture index", `英文态「架构索引」未翻译,实际 "${archKey("架构索引")}"`);
  assert(archKey("记忆管理") === "Memory management", `英文态「记忆管理」未翻译,实际 "${archKey("记忆管理")}"`);
  assert(archKey("打开") === "Open", `英文态「打开」未翻译,实际 "${archKey("打开")}"`);
  assert(attrOf("arch-goto-memory", "title") === "Jump to the memory page to maintain entries (edit/consolidate/focus via existing memory commands)", `英文态 arch-goto-memory title 未翻译,实际 "${attrOf("arch-goto-memory", "title")}"`);
  assert(attrOf("arch-open-index", "title") === "Open the architecture index in the viewer", `英文态 arch-open-index title 未翻译,实际 "${attrOf("arch-open-index", "title")}"`);
  assert(attrOf("arch-tree", "aria-label") === "Design doc tree", `英文态 arch-tree aria-label 未翻译(渲染点属性补齐),实际 "${attrOf("arch-tree", "aria-label")}"`);
  assert(attrOf("arch-refresh", "title") === "Rescan", `英文态 arch-refresh title 未翻译,实际 "${attrOf("arch-refresh", "title")}"`);
  assert(attrOf("arch-refresh", "aria-label") === "Rescan architecture index", `英文态 arch-refresh aria-label 未翻译(渲染点属性补齐),实际 "${attrOf("arch-refresh", "aria-label")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(archKey("架构浏览") === "架构浏览", `切回中文后「架构浏览」未回原文,实际 "${archKey("架构浏览")}"`);
  assert(attrOf("arch-tree", "aria-label") === "设计文档树", `切回中文后 arch-tree aria-label 未回原文,实际 "${attrOf("arch-tree", "aria-label")}"`);
  assert(attrOf("arch-refresh", "aria-label") === "重新扫描架构索引", `切回中文后 arch-refresh aria-label 未回原文,实际 "${attrOf("arch-refresh", "aria-label")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批5:文档页域迁移(标题/工具栏/筛选/批量/测试区) ----------
// 文档页 h1/说明/标签/按钮/筛选下拉/批量操作/测试记录区迁移到 data-i18n-key/
// data-i18n-title/data-i18n-aria-label,含静态 <option> 文本。英文态翻译、切中文回原文。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  // harness 的 queryAllFrom 按空格切分选择器,含空格/斜杠的 key 无法用 `[data-i18n-key="..."]`
  // 查询;改为遍历所有 data-i18n-key 节点按 dataset 匹配(与 B4 断言组同款绕过)。
  const docKey = (key) => [...sandbox.document.querySelectorAll("[data-i18n-key]")].find((el) => el.dataset.i18nKey === key)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(docKey("需求与工作 / 缺陷 / 测试") === "需求与工作 / 缺陷 / 测试", "中文态文档页标题应保持原文(前置失效)");
  assert(docKey("全部状态") === "全部状态", "中文态状态筛选「全部状态」应保持原文(前置失效)");
  assert(attrOf("documents-status-filter", "title") === "按状态筛选", "中文态状态筛选 title 应保持原文(前置失效)");
  assert(attrOf("req-open", "aria-label") === "打开 requirements.md 原文", "中文态 req-open aria-label 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(docKey("需求与工作 / 缺陷 / 测试") === "Work items / Defects / Tests", `英文态文档页标题未翻译,实际 "${docKey("需求与工作 / 缺陷 / 测试")}"`);
  assert(docKey("自动审查缺陷") === "Review defects", `英文态「自动审查缺陷」未翻译,实际 "${docKey("自动审查缺陷")}"`);
  assert(docKey("依赖视图") === "Dependency view", `英文态「依赖视图」未翻译,实际 "${docKey("依赖视图")}"`);
  assert(docKey("全部状态") === "All statuses", `英文态「全部状态」未翻译(option 渲染点),实际 "${docKey("全部状态")}"`);
  assert(docKey("未评估") === "Not assessed", `英文态「未评估」未翻译(option 渲染点),实际 "${docKey("未评估")}"`);
  assert(docKey("已阻塞") === "Blocked", `英文态「已阻塞」未翻译(option 渲染点),实际 "${docKey("已阻塞")}"`);
  assert(docKey("手动") === "Manual", `英文态「手动」未翻译(option 渲染点),实际 "${docKey("手动")}"`);
  assert(docKey("取消选择") === "Clear selection", `英文态「取消选择」未翻译,实际 "${docKey("取消选择")}"`);
  assert(attrOf("documents-status-filter", "title") === "Filter by status", `英文态状态筛选 title 未翻译,实际 "${attrOf("documents-status-filter", "title")}"`);
  assert(attrOf("documents-priority-filter", "title") === "Filter by priority (reference only; does not affect work order)", `英文态优先级筛选 title 未翻译,实际 "${attrOf("documents-priority-filter", "title")}"`);
  assert(attrOf("req-open", "aria-label") === "Open requirements.md source", `英文态 req-open aria-label 未翻译(渲染点属性补齐),实际 "${attrOf("req-open", "aria-label")}"`);
  assert(attrOf("tests-refresh", "aria-label") === "Refresh and archive completed tests", `英文态 tests-refresh aria-label 未翻译,实际 "${attrOf("tests-refresh", "aria-label")}"`);
  assert(attrOf("documents-batch-bar", "aria-label") === "Bulk actions", `英文态批量操作区 aria-label 未翻译,实际 "${attrOf("documents-batch-bar", "aria-label")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(docKey("需求与工作 / 缺陷 / 测试") === "需求与工作 / 缺陷 / 测试", `切回中文后文档页标题未回原文,实际 "${docKey("需求与工作 / 缺陷 / 测试")}"`);
  assert(docKey("全部状态") === "全部状态", `切回中文后「全部状态」未回原文,实际 "${docKey("全部状态")}"`);
  assert(attrOf("req-open", "aria-label") === "打开 requirements.md 原文", `切回中文后 req-open aria-label 未回原文,实际 "${attrOf("req-open", "aria-label")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批6:记忆页域迁移(标题/说明/工具/侧栏区块) ----------
// 记忆页 h1/说明/搜索框(placeholder+aria-label)/整理按钮/区块标题/清理按钮迁移到
// data-i18n-key/data-i18n-title/data-i18n-placeholder/data-i18n-aria-label。
// 含子元素的 h2 文本用 span 包裹(不得在 h2 上直接 data-i18n-key,会清掉计数 span)。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  const memKey = (key) => [...sandbox.document.querySelectorAll("[data-i18n-key]")].find((el) => el.dataset.i18nKey === key)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(memKey("记忆") === "记忆", "中文态记忆页标题应保持原文(前置失效)");
  assert(memKey("待确认候选") === "待确认候选", "中文态「待确认候选」应保持原文(前置失效)");
  assert(attrOf("memory-search-input", "placeholder") === "检索全部记忆(FTS)", "中文态搜索框 placeholder 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(memKey("记忆") === "Memory", `英文态「记忆」未翻译,实际 "${memKey("记忆")}"`);
  assert(memKey("整理 inbox") === "Consolidate inbox", `英文态「整理 inbox」未翻译,实际 "${memKey("整理 inbox")}"`);
  assert(memKey("待确认候选") === "Pending candidates", `英文态「待确认候选」未翻译(span 包裹),实际 "${memKey("待确认候选")}"`);
  assert(memKey("空闲整理清单") === "Idle cleanup list", `英文态「空闲整理清单」未翻译,实际 "${memKey("空闲整理清单")}"`);
  assert(memKey("最近记忆使用") === "Recent memory usage", `英文态记忆使用标题未翻译`);
  assert(memKey("上下文账单") === "Context bill", `英文态「上下文账单」未翻译,实际 "${memKey("上下文账单")}"`);
  assert(memKey("最近轮次") === "Recent rounds", `英文态「最近轮次」未翻译,实际 "${memKey("最近轮次")}"`);
  assert(attrOf("memory-search-input", "placeholder") === "Search all memory (FTS)", `英文态搜索框 placeholder 未翻译(渲染点属性补齐),实际 "${attrOf("memory-search-input", "placeholder")}"`);
  assert(attrOf("memory-search-input", "aria-label") === "Search memory", `英文态搜索框 aria-label 未翻译(渲染点属性补齐),实际 "${attrOf("memory-search-input", "aria-label")}"`);
  assert(attrOf("memory-arch", "aria-label") === "Memory architecture overview", `英文态 memory-arch aria-label 未翻译,实际 "${attrOf("memory-arch", "aria-label")}"`);
  assert(attrOf("memory-consolidate-btn", "title") === "Consolidate inbox drafts now", `英文态整理按钮 title 未翻译,实际 "${attrOf("memory-consolidate-btn", "title")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(memKey("记忆") === "记忆", `切回中文后「记忆」未回原文,实际 "${memKey("记忆")}"`);
  assert(memKey("待确认候选") === "待确认候选", `切回中文后「待确认候选」未回原文,实际 "${memKey("待确认候选")}"`);
  assert(attrOf("memory-search-input", "placeholder") === "检索全部记忆(FTS)", `切回中文后搜索框 placeholder 未回原文,实际 "${attrOf("memory-search-input", "placeholder")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批7:指标页 + 文件页域迁移(标题/说明/工具栏/占位) ----------
// 指标页 h1/说明/两个 aria-label;文件页排序·标注·刷新按钮(title+文本+aria-label)、
// 文件树 aria-label、占位说明 迁移到 data-i18n-key/data-i18n-title/data-i18n-aria-label。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  const b7Key = (key) => [...sandbox.document.querySelectorAll("[data-i18n-key]")].find((el) => el.dataset.i18nKey === key)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b7Key("运行画像") === "运行画像", "中文态指标页标题应保持原文(前置失效)");
  assert(attrOf("metrics-trend", "aria-label") === "跨轮趋势", "中文态 metrics-trend aria-label 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(b7Key("运行画像") === "Run profile", `英文态「运行画像」未翻译,实际 "${b7Key("运行画像")}"`);
  assert(b7Key("按行数") === "By lines", `英文态「按行数」未翻译,实际 "${b7Key("按行数")}"`);
  assert(b7Key("标注") === "Annotate", `英文态「标注」未翻译,实际 "${b7Key("标注")}"`);
  assert(attrOf("metrics-trend", "aria-label") === "Cross-round trends", `英文态 metrics-trend aria-label 未翻译,实际 "${attrOf("metrics-trend", "aria-label")}"`);
  assert(attrOf("metrics-rounds", "aria-label") === "Per-round profile", `英文态 metrics-rounds aria-label 未翻译,实际 "${attrOf("metrics-rounds", "aria-label")}"`);
  assert(attrOf("files-sort", "title") === "Toggle sort: name / lines", `英文态 files-sort title 未翻译,实际 "${attrOf("files-sort", "title")}"`);
  assert(attrOf("files-refresh", "aria-label") === "Rescan file tree", `英文态 files-refresh aria-label 未翻译,实际 "${attrOf("files-refresh", "aria-label")}"`);
  assert(attrOf("files-tree", "aria-label") === "Project file tree", `英文态文件树 aria-label 未翻译,实际 "${attrOf("files-tree", "aria-label")}"`);
  assert(b7Key("选择左侧文件查看内容 · 目录行显示聚合度量 · 「标注」用 fast 模型生成用途说明").includes("Select a file on the left"), `英文态文件占位说明未翻译,实际 "${b7Key("选择左侧文件查看内容 · 目录行显示聚合度量 · 「标注」用 fast 模型生成用途说明")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b7Key("运行画像") === "运行画像", `切回中文后「运行画像」未回原文,实际 "${b7Key("运行画像")}"`);
  assert(attrOf("files-refresh", "aria-label") === "重新扫描文件树", `切回中文后 files-refresh aria-label 未回原文,实际 "${attrOf("files-refresh", "aria-label")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批8:设置页域迁移(标题/关于/全部 details 区块/动态字符串) ----------
// 设置页 h1/说明(span 包裹保留 code#settings-path)/关于 kanzei 三行/界面语言/模型角色
// (保存到·作用域 option·primary·fast·探测·一键就绪)/Provider(测试·表头·添加)/网络与默认
// (代理 option·默认模式 option·思考强度 option)/运行上限(六组 label+说明)/验证与提交节奏
// (全量/定向/提交/push 的 option 与 title)/移动端桥接/已记住的权限/工作资料导出/版本与更新/
// 底部动作区 全部挂 data-i18n-key/data-i18n-title/data-i18n-placeholder。16-settings.js 的
// 11 处动态模板改走 t()(删除失败/读取权限规则失败/本页·实际生效·未设/手填×2/设置读取失败/
// 启动·停止桥接失败/保存失败/选择导出目录失败/导出失败),词典补 本页/实际生效/手填 三 key。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  const b8Key = (key) => [...sandbox.document.querySelectorAll("[data-i18n-key]")].find((el) => el.dataset.i18nKey === key)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b8Key("设置") === "设置", "中文态设置页标题应保持原文(前置失效)");
  assert(b8Key("模型配置") === "模型配置", "中文态「模型配置」应保持原文(前置失效)");
  assert(attrOf("export-output-dir", "placeholder") === "选择导出目录", "中文态导出目录 placeholder 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(b8Key("设置") === "Settings", `英文态「设置」未翻译,实际 "${b8Key("设置")}"`);
  assert(b8Key("关于 kanzei") === "About kanzei", `英文态「关于 kanzei」未翻译,实际 "${b8Key("关于 kanzei")}"`);
  assert(b8Key("模型配置") === "Model configuration", `英文态「模型配置」未翻译,实际 "${b8Key("模型配置")}"`);
  assert(b8Key("保存到") === "Save to", `英文态「保存到」未翻译(span 包裹保留 hint),实际 "${b8Key("保存到")}"`);
  assert(b8Key("全局配置") === "Global config", `英文态「全局配置」未翻译(option 渲染点),实际 "${b8Key("全局配置")}"`);
  assert(b8Key("主循环") === "Main loop", `英文态「主循环」未翻译(span 包裹),实际 "${b8Key("主循环")}"`);
  assert(b8Key("重新探测模型") === "Re-detect models", `英文态「重新探测模型」未翻译,实际 "${b8Key("重新探测模型")}"`);
  assert(b8Key("测试全部连通性") === "Test connectivity", `英文态「测试全部连通性」未翻译,实际 "${b8Key("测试全部连通性")}"`);
  assert(b8Key("思考强度") === "Reasoning effort", `英文态「思考强度」未翻译,实际 "${b8Key("思考强度")}"`);
  assert(b8Key("主对话输出上限") === "Main output cap", `英文态「主对话输出上限」未翻译,实际 "${b8Key("主对话输出上限")}"`);
  assert(b8Key("验证与提交节奏") === "Verification & commit cadence", `英文态「验证与提交节奏」未翻译,实际 "${b8Key("验证与提交节奏")}"`);
  assert(b8Key("全量测试") === "Full test suite", `英文态「全量测试」未翻译,实际 "${b8Key("全量测试")}"`);
  assert(b8Key("每 N 批") === "Every N batches", `英文态「每 N 批」未翻译(option 渲染点),实际 "${b8Key("每 N 批")}"`);
  assert(b8Key("移动端桥接") === "Mobile bridge", `英文态「移动端桥接」未翻译,实际 "${b8Key("移动端桥接")}"`);
  assert(b8Key("已记住的权限") === "Saved permissions", `英文态「已记住的权限」未翻译,实际 "${b8Key("已记住的权限")}"`);
  assert(b8Key("工作资料导出") === "Export work materials", `英文态「工作资料导出」未翻译,实际 "${b8Key("工作资料导出")}"`);
  assert(b8Key("检查更新") === "Check for updates", `英文态「检查更新」未翻译,实际 "${b8Key("检查更新")}"`);
  assert(b8Key("保存") === "Save", `英文态「保存」未翻译,实际 "${b8Key("保存")}"`);
  assert(attrOf("set-save-scope", "title") === "This scope selector only applies to model settings; providers and API keys always use the global config.", `英文态作用域 title 未翻译,实际 "${attrOf("set-save-scope", "title")}"`);
  assert(attrOf("export-output-dir", "placeholder") === "Choose an export directory", `英文态导出目录 placeholder 未翻译,实际 "${attrOf("export-output-dir", "placeholder")}"`);
  assert(attrOf("set-cadence-full-test-batches", "title") === "Interval in batches for every-N-batches", `英文态每 N 批 title 未翻译,实际 "${attrOf("set-cadence-full-test-batches", "title")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b8Key("设置") === "设置", `切回中文后「设置」未回原文,实际 "${b8Key("设置")}"`);
  assert(b8Key("保存到") === "保存到", `切回中文后「保存到」未回原文,实际 "${b8Key("保存到")}"`);
  assert(attrOf("export-output-dir", "placeholder") === "选择导出目录", `切回中文后导出目录 placeholder 未回原文,实际 "${attrOf("export-output-dir", "placeholder")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批9:活动/会话/compose 域 + 全局静态面收口 ----------
// rail 导航 title/aria-label 已由批0 断言覆盖;live-turn/status-mode/status-text 是动态
// 元素(JS 用 t()/localizeDynamic 渲染点写入),一律不挂 data-i18n-key——挂了会在切语言时被
// applyDataI18nKeys 覆写回原文。chat-search·prompt placeholder/sop-picker aria-label/
// queue·steer option/log 面板/statusbar(git·ctx·tokens·日志)/
// 活动面板筛选(类型+状态)/agent 面板区块与清空/权限询问(标题·字段·回答 placeholder·
// 四按钮)/查看器两按钮全部挂 data-i18n-*。带 id 的元素把 data-i18n-key 放元素自身
// (冒烟按 id 建节点只取开标签后首个 < 前的 directText,span 包裹会让按钮文本变空);
// 无 id 的容器/区块标题用内层 span 包裹。06-agent-panel 运行中/已完成计数器、06-activity
// 未命名文件、08-compose、09-sessions 动态字符串改走 t()。词典补 未命名文件/工作树清单
// 读取失败(资源 54→56)。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  const b9Key = (key) => [...sandbox.document.querySelectorAll("[data-i18n-key]")].find((el) => el.dataset.i18nKey === key)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b9Key("权限请求") === "权限请求", "中文态权限请求标题应保持原文(前置失效)");
  assert(b9Key("回到最新") === "回到最新", "中文态回到最新应保持原文(前置失效)");
  assert(!sandbox.document.getElementById("process-tabs"), "中文态不应出现顶部进程切换条");
  assert(attrOf("prompt", "placeholder") === "想做什么?可粘贴/拖拽图片或 PDF", "中文态输入框 placeholder 应保持原文(前置失效)");
  assert(b9Key("排队 queue") === "排队 queue", "中文态排队 queue option 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(attrOf("rail-sidebar-toggle", "title") === "Open or close the sidebar", `英文态 rail 侧栏开关 title 未翻译,实际 "${attrOf("rail-sidebar-toggle", "title")}"`);
  assert(attrOf("rail-sidebar-toggle", "aria-label") === "Open or close the sidebar", `英文态 rail 侧栏开关 aria-label 未翻译,实际 "${attrOf("rail-sidebar-toggle", "aria-label")}"`);
  assert(b9Key("运行日志") === "Runtime log", `英文态「运行日志」未翻译,实际 "${b9Key("运行日志")}"`);
  assert(attrOf("log-copy", "aria-label") === "Copy runtime log", `英文态 log-copy aria-label 未翻译,实际 "${attrOf("log-copy", "aria-label")}"`);
  assert(attrOf("status-git", "title") === "Git branch · uncommitted changes", `英文态 status-git title 未翻译,实际 "${attrOf("status-git", "title")}"`);
  assert(attrOf("status-tokens", "aria-label") === "View context components", `英文态 status-tokens aria-label 未翻译,实际 "${attrOf("status-tokens", "aria-label")}"`);
  assert(b9Key("日志") === "Logs", `英文态「日志」未翻译,实际 "${b9Key("日志")}"`);
  assert(b9Key("回到最新") === "Jump to latest", `英文态「回到最新」未翻译,实际 "${b9Key("回到最新")}"`);
  assert(b9Key("全部类型") === "All types", `英文态「全部类型」未翻译(option 渲染点),实际 "${b9Key("全部类型")}"`);
  assert(b9Key("终端") === "terminal", `英文态「终端」未翻译(option 渲染点),实际 "${b9Key("终端")}"`);
  assert(b9Key("已关闭") === "Closed", `英文态「已关闭」未翻译,实际 "${b9Key("已关闭")}"`);
  assert(b9Key("清空") === "Clear", `英文态「清空」未翻译,实际 "${b9Key("清空")}"`);
  assert(b9Key("权限请求") === "Permission request", `英文态「权限请求」未翻译,实际 "${b9Key("权限请求")}"`);
  assert(b9Key("拒绝") === "Deny", `英文态「拒绝」未翻译,实际 "${b9Key("拒绝")}"`);
  assert(b9Key("总是允许") === "Always allow", `英文态「总是允许」未翻译,实际 "${b9Key("总是允许")}"`);
  assert(b9Key("允许一次") === "Allow once", `英文态「允许一次」未翻译,实际 "${b9Key("允许一次")}"`);
  assert(attrOf("ask-answer", "placeholder") === "Enter your answer", `英文态回答 placeholder 未翻译(渲染点属性补齐),实际 "${attrOf("ask-answer", "placeholder")}"`);
  assert(attrOf("viewer-external", "aria-label") === "Open in external editor", `英文态 viewer-external aria-label 未翻译,实际 "${attrOf("viewer-external", "aria-label")}"`);
  assert(attrOf("prompt", "placeholder") === "What would you like to do? Paste or drop images or PDFs", `英文态输入框 placeholder 未翻译,实际 "${attrOf("prompt", "placeholder")}"`);
  assert(b9Key("排队 queue") === "Queue", `英文态「排队 queue」未翻译(option 渲染点),实际 "${b9Key("排队 queue")}"`);
  assert(!sandbox.document.getElementById("process-tabs"), "英文态不应出现顶部进程切换条");
  // 动态元素不被静态 key 覆写:status-mode/status-text/live-turn 都不得带 data-i18n-key,
  // 它们的文案由 JS 渲染点(t()/localizeDynamic)负责,切语言不应被 applyDataI18nKeys 触碰。
  assert(!attrOf("status-mode", "data-i18n-key"), "status-mode 不得挂 data-i18n-key(动态渲染点)");
  assert(!attrOf("status-text", "data-i18n-key"), "status-text 不得挂 data-i18n-key(动态渲染点)");

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b9Key("权限请求") === "权限请求", `切回中文后「权限请求」未回原文,实际 "${b9Key("权限请求")}"`);
  assert(b9Key("回到最新") === "回到最新", `切回中文后「回到最新」未回原文,实际 "${b9Key("回到最新")}"`);
  assert(attrOf("prompt", "placeholder") === "想做什么?可粘贴/拖拽图片或 PDF", `切回中文后输入框 placeholder 未回原文,实际 "${attrOf("prompt", "placeholder")}"`);
  assert(b9Key("排队 queue") === "排队 queue", `切回中文后「排队 queue」未回原文,实际 "${b9Key("排队 queue")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- 换项目不得把上一个项目的筛选落进新项目 ----------
// documentFilters 是模块级状态,切项目不会重建它;restoreDocFilters 又只"叠加保存里存在
// 的字段、不复位"。于是切到一个从没设过偏好的新项目时,内存里还挂着上个项目的整套口径,
// 而 syncDocumentFilters 里 D-169 的标签回落一触发就 saveDocFilters(),把这一整套写进
// **新项目**的键——用户在新项目从没设过,列表却少了一批,重启也回不来。
// 触发条件一点不苛刻:上个项目的标签在新项目里不存在(标签本来就按项目走)。
// 两头一起钉死:新项目必须是干净的默认口径,老项目切回去必须原样还在(别为了修这个
// 把 R-115 的按项目持久化弄坏)。
{
  const PROJECT_B = "C:/smoke/project-b";
  const savedDocsPayload = structuredClone(payloads.docs_snapshot);
  const filtersKeyOf = (path) => `kz-filters:${path}`;
  // 默认口径取自被测代码自己那一份(DOC_FILTER_DEFAULTS),冒烟里不另抄一遍:
  // 抄第二份的话,默认值一改这组断言就悄悄变成恒真。
  const DEFAULTS = JSON.parse(vm.runInContext("JSON.stringify(DOC_FILTER_DEFAULTS)", sandbox));
  const liveFilters = () => JSON.parse(vm.runInContext("JSON.stringify(documentFilters)", sandbox));
  const savedFilters = (path) => JSON.parse(storage.get(filtersKeyOf(path)) ?? "null");
  // 内存与落盘共用同一把尺子:列出所有"与默认值不同"的持久化字段。
  const stray = (bag, kind) =>
    Object.entries(DEFAULTS[kind])
      .filter(([field, def]) => bag?.[field] !== undefined && bag[field] !== def)
      .map(([field]) => `${kind}.${field}=${bag[field]}`);
  const strayLive = () => { const f = liveFilters(); return [...stray(f.req, "req"), ...stray(f.defect, "defect")]; };
  const straySaved = (path) => {
    const blob = savedFilters(path);
    return [...stray(blob?.docReq, "req"), ...stray(blob?.docDefect, "defect")];
  };
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  const gotoProject = async (path, docs) => {
    payloads.docs_snapshot = docs;
    payloads.projects_select = {
      current: path,
      projects: [PROJECT, PROJECT_B],
      names: { [PROJECT]: "smoke", [PROJECT_B]: "smoke-b" },
    };
    await sandbox.selectWorkspaceProject(path);
    await flush();
    assert(
      vm.runInContext("currentProject", sandbox) === path,
      `前置失败:切项目没走通(currentProject=${vm.runInContext("currentProject", sandbox)})`,
    );
  };
  // 项目1 有「核心」标签,项目2 只有「流程」——标签按项目走,这就是常态。
  const docsA = {
    ...savedDocsPayload,
    requirements: [
      docEntry("R-001", "核心大需求", "doing", { complexity: "大", fields: [["标签", "核心"]] }),
      docEntry("R-002", "前端需求", "todo", { fields: [["标签", "前端"]] }),
    ],
    defects: [
      docEntry("D-001", "冒烟缺陷", "open", { fields: [["标签", "前端"]] }),
      docEntry("D-002", "在修缺陷", "fixing", { fields: [["标签", "前端"]] }),
    ],
  };
  const docsB = {
    ...savedDocsPayload,
    requirements: [docEntry("R-900", "项目二需求", "todo", { complexity: "中", fields: [["标签", "流程"]] })],
    defects: [docEntry("D-900", "项目二缺陷", "open", { fields: [["标签", "流程"]] })],
  };

  // 前置:项目1 设好一套只属于它的筛选(两队都设,跨队列泄漏也要能看出来)。
  payloads.docs_snapshot = docsA;
  await sandbox.refreshDocs();
  await flush();
  byId.get("documents-tab-defect").click();
  await flush();
  await setDocFilter("documents-status-filter", "fixing");
  byId.get("documents-tab-req").click();
  await flush();
  await setDocFilter("documents-status-filter", "doing");
  await setDocFilter("documents-complexity-filter", "大");
  await setDocFilter("documents-tag-filter", "核心");
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]')
      && !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    "前置失败:项目1 的需求筛选没生效",
  );
  const strayA = straySaved(PROJECT);
  assert(
    ["req.status=doing", "req.complexity=大", "req.tag=核心", "defect.status=fixing"].every((f) => strayA.includes(f)),
    `前置失败:项目1 的筛选没完整落盘(R-115 持久化本身断了):${storage.get(filtersKeyOf(PROJECT))}`,
  );

  // 项目2 从没设过偏好:键必须先干净,否则断言测的是"回填",不是"泄漏"。
  storage.delete(filtersKeyOf(PROJECT_B));

  await gotoProject(PROJECT_B, docsB);
  assert(
    strayLive().length === 0,
    `换到没设过偏好的项目,内存里还挂着上一个项目的筛选:${strayLive().join(", ")}`,
  );
  assert(
    straySaved(PROJECT_B).length === 0,
    `上一个项目的筛选被写进了新项目的键(用户从没设过,重启也回不来):${straySaved(PROJECT_B).join(", ")} / ${storage.get(filtersKeyOf(PROJECT_B))}`,
  );
  // 用户视角的后果:新项目的列表被一个自己从没设过的条件筛空。
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]'),
    "新项目的需求被上一个项目的筛选藏掉了(看起来就是条目凭空没有)",
  );
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-900"]'),
    "新项目的缺陷被上一个项目的筛选藏掉了",
  );
  assert(byId.get("documents-status-filter").value === "all", "新项目的状态下拉还显示着上一个项目的值");
  assert(byId.get("documents-tag-filter").value === "all", "新项目的标签下拉还显示着上一个项目的值");

  // 切回项目1:自己的筛选必须原样还在(内存 + 落盘 + 控件 + 列表)。
  await gotoProject(PROJECT, docsA);
  byId.get("documents-tab-req").click();
  await flush();
  const backLive = liveFilters();
  assert(
    backLive.req.status === "doing" && backLive.req.complexity === "大" && backLive.req.tag === "核心"
      && backLive.defect.status === "fixing",
    `切回原项目,它自己的筛选没回来(为了不泄漏把按项目持久化一起弄坏了):${JSON.stringify(backLive)}`,
  );
  assert(
    ["req.status=doing", "req.complexity=大", "req.tag=核心", "defect.status=fixing"].every((f) => straySaved(PROJECT).includes(f)),
    `切回原项目,落盘的筛选被改掉了:${storage.get(filtersKeyOf(PROJECT))}`,
  );
  assert(byId.get("documents-status-filter").value === "doing", "切回原项目,状态下拉没回填");
  assert(byId.get("documents-complexity-filter").value === "大", "切回原项目,复杂度下拉没回填");
  assert(byId.get("documents-tag-filter").value === "核心", "切回原项目,标签下拉没回填");
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    "切回原项目后筛选只剩下拉显示值、列表没在筛(状态与显示脱节)",
  );

  // 分组开关不按项目走:它记在全局键 kz-grouped-docs 上(见 bindGroupToggle),换项目
  // 复位**不得**碰它。整个复位形状就建立在这条边界上——复位只覆盖 DOC_FILTER_DEFAULTS 的
  // 键,grouped 故意不在那份清单里。把 grouped 加进 DOC_FILTER_DEFAULTS 上面那些断言全绿,
  // 因为它们只逐字段比对"与默认值不同"的持久化项,而 grouped 的默认值(true)恰好等于
  // 被错误复位后的值。所以必须单独钉死:用户关掉分组,换个项目它自己回来了,
  // 落盘还停在 0、按钮还写着 aria-pressed=false —— 显示、内存、落盘三方脱节。
  {
    const groupToggle = byId.get("documents-group-toggle");
    const groupedLive = () => liveFilters();
    const groupedBefore = groupedLive().req.grouped;
    // 走用户路径把开关拨到「关」:已经是关的就先开再关,保证落盘值也确实是这条路径写出来的
    // (前面的用例可能只改过内存里的 grouped —— 解锁按钮就是),否则前置断言测的是别人留下的残值。
    if (!groupedLive().req.grouped) {
      groupToggle.click();
      await flush();
    }
    groupToggle.click();
    await flush();
    assert(
      groupedLive().req.grouped === false && groupedLive().defect.grouped === false
        && storage.get("kz-grouped-docs") === "0" && groupToggle.getAttribute("aria-pressed") === "false",
      `前置失败:分组没关掉(内存 ${JSON.stringify(groupedLive().req.grouped)} / 落盘 ${storage.get("kz-grouped-docs")} / aria-pressed ${groupToggle.getAttribute("aria-pressed")})`,
    );

    storage.delete(filtersKeyOf(PROJECT_B));
    await gotoProject(PROJECT_B, docsB);
    const afterSwitch = groupedLive();
    assert(
      afterSwitch.req.grouped === false && afterSwitch.defect.grouped === false,
      `换项目把分组开关复位了(它按 kz-grouped-docs 全局记、不随项目走):req=${afterSwitch.req.grouped} defect=${afterSwitch.defect.grouped}`,
    );
    assert(
      storage.get("kz-grouped-docs") === "0",
      `换项目改掉了分组开关的全局落盘值:kz-grouped-docs=${storage.get("kz-grouped-docs")}`,
    );
    assert(
      groupToggle.getAttribute("aria-pressed") === "false",
      `换项目后分组按钮的 aria-pressed 与状态脱节:aria-pressed=${groupToggle.getAttribute("aria-pressed")}(内存 ${afterSwitch.req.grouped})`,
    );

    // 还原:切回项目1 并把分组开关调回进来时的样子,否则后续用例看到的是另一种渲染形态。
    storage.delete(filtersKeyOf(PROJECT_B));
    await gotoProject(PROJECT, docsA);
    if (groupedLive().req.grouped !== groupedBefore) {
      groupToggle.click();
      await flush();
    }
    assert(
      groupedLive().req.grouped === groupedBefore,
      `收尾失败:分组开关没还原(${groupedLive().req.grouped} ≠ ${groupedBefore}),后续用例会连带假失败`,
    );
    byId.get("documents-tab-req").click();
    await flush();
  }

  // 收尾:走用户路径把筛选调回全部,清掉项目2 的键,还原快照。
  await setDocFilter("documents-status-filter", "all");
  await setDocFilter("documents-complexity-filter", "all");
  await setDocFilter("documents-tag-filter", "all");
  byId.get("documents-tab-defect").click();
  await flush();
  await setDocFilter("documents-status-filter", "all");
  byId.get("documents-tab-req").click();
  await flush();
  storage.delete(filtersKeyOf(PROJECT_B));
  delete payloads.projects_select;
  payloads.docs_snapshot = savedDocsPayload;
  await sandbox.refreshDocs();
  await flush();
  assert(strayLive().length === 0, `收尾失败:筛选没调回全部(${strayLive().join(", ")})`);
}

// ---------- 一次空快照不得清掉用户的标签筛选 ----------
// syncDocumentFilters 的 D-169 回落(「保存的标签在这一队里已经不存在了 → 回落成全部并落盘」)
// 只在**这一队真的有条目**时才成立。而 docs_snapshot 并不保证非空:docstore 那几个文件是
// fs::write 截断重写(非原子),load() 又把空文件当成合法的空列表返回,docs.rs 更是
// unwrap_or_default —— 任何读失败(含 Windows 上的文件占用)都静默降级成空;偏偏
// docs_snapshot 自己开头就在写这几个文件,一次 refreshDocs 与一次 refreshDocsSoon
// 完全可以同时在飞。于是一次瞬态空快照就够把用户设好的标签筛选永久清成「全部」:
// 内存与落盘一起改,数据回来了也回不来,重启同样,全程零用户动作。
// 空列表里「列表被一个看不见的条件筛空」这个前提根本不成立(列表本来就是空的),
// 没有任何理由改用户的口径。
{
  const savedDocsPayload = structuredClone(payloads.docs_snapshot);
  const filtersKey = `kz-filters:${PROJECT}`;
  const liveTag = (kind) => JSON.parse(vm.runInContext(`JSON.stringify(documentFilters.${kind})`, sandbox)).tag;
  const savedTag = (kind) =>
    JSON.parse(storage.get(filtersKey) ?? "{}")[kind === "req" ? "docReq" : "docDefect"]?.tag;
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  const taggedDocs = {
    ...savedDocsPayload,
    requirements: [
      docEntry("R-001", "核心标签需求", "doing", { fields: [["标签", "核心"]] }),
      docEntry("R-002", "前端标签需求", "todo", { fields: [["标签", "前端"]] }),
    ],
    defects: [docEntry("D-001", "前端标签缺陷", "open", { fields: [["标签", "前端"]] })],
  };

  byId.get("documents-tab-req").click();
  await flush();
  payloads.docs_snapshot = taggedDocs;
  await sandbox.refreshDocs();
  await flush();
  await setDocFilter("documents-tag-filter", "核心");
  assert(
    liveTag("req") === "核心" && savedTag("req") === "核心",
    `前置失败:标签筛选没设上或没落盘(内存 ${liveTag("req")} / 落盘 ${storage.get(filtersKey)})`,
  );

  // 瞬态空快照:两队都读成了空(截断重写撞上并发读,或读失败降级成空列表)。
  payloads.docs_snapshot = { ...savedDocsPayload, requirements: [], defects: [] };
  await sandbox.refreshDocs();
  await flush();
  assert(
    liveTag("req") === "核心",
    `一次瞬态空快照把用户的标签筛选清成了「全部」(内存):${liveTag("req")}`,
  );
  assert(
    savedTag("req") === "核心",
    `一次瞬态空快照把用户的标签筛选清成「全部」并落盘了(数据回来了筛选也回不来,重启同样):${storage.get(filtersKey)}`,
  );

  // 数据回来:筛选必须原样还在,并且真的在筛(状态与显示不许脱节)。
  payloads.docs_snapshot = taggedDocs;
  await sandbox.refreshDocs();
  await flush();
  assert(
    byId.get("documents-tag-filter").value === "核心"
      && document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]')
      && !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    `空快照过后数据回来了,标签筛选却没恢复成用户设的那个:下拉=${byId.get("documents-tag-filter").value}`,
  );

  // 收尾:走用户路径调回全部并还原快照。
  await setDocFilter("documents-tag-filter", "all");
  payloads.docs_snapshot = savedDocsPayload;
  await sandbox.refreshDocs();
  await flush();
  assert(
    liveTag("req") === "all" && liveTag("defect") === "all",
    `收尾失败:标签没调回全部(${liveTag("req")} / ${liveTag("defect")})`,
  );
}

// ---------- 在途快照不得落到已经切走的项目上 ----------
// refreshDocs / refreshDocsSoon 都是 `await invoke("docs_snapshot", { projectDir: currentProject })`
// 之后直接 renderDocsSnapshot。await 期间用户切了项目,这份数据就是**上一个项目**的:
// 轻则切项目瞬间闪一下上一个项目的列表,重则 syncDocumentFilters 拿**新项目**的筛选去
// **旧项目**的条目里判「这标签还在不在」——判否就回落成「全部」,而落盘走的是新项目的键。
// 用户在新项目从没动过筛选,列表却少了一批,重启也回不来。
// 闸门把"IPC 还没回来"这段真机时序复现出来:只卡住这一次,切项目自己那次刷新照常走。
// 切项目夹具提到块外:下面 D-250/D-251 的跨项目用例走的是同一套「甲乙两个项目 + 闸门」
// 时序,各自再抄一份 gotoProject 只会让三处的切项目口径将来悄悄分叉。
const PROJECT_B = "C:/smoke/project-b";
const savedDocsPayload = structuredClone(payloads.docs_snapshot);
const gotoProject = async (path, docs) => {
  payloads.docs_snapshot = docs;
  payloads.projects_select = {
    current: path,
    projects: [PROJECT, PROJECT_B],
    names: { [PROJECT]: "smoke", [PROJECT_B]: "smoke-b" },
  };
  await sandbox.selectWorkspaceProject(path);
  await flush();
  assert(
    vm.runInContext("currentProject", sandbox) === path,
    `前置失败:切项目没走通(currentProject=${vm.runInContext("currentProject", sandbox)})`,
  );
};
// 标签按项目走:甲只有「核心」,乙只有「流程」。这就是常态,触发条件一点不苛刻。
const docsA = {
  ...savedDocsPayload,
  requirements: [docEntry("R-001", "甲项目需求", "doing", { fields: [["标签", "核心"]] })],
  defects: [docEntry("D-001", "甲项目缺陷", "open", { fields: [["标签", "核心"]] })],
};
const docsB = {
  ...savedDocsPayload,
  requirements: [docEntry("R-900", "乙项目需求", "todo", { fields: [["标签", "流程"]] })],
  defects: [docEntry("D-900", "乙项目缺陷", "open", { fields: [["标签", "流程"]] })],
};
{
  const filtersKeyOf = (path) => `kz-filters:${path}`;
  const liveReqTag = () => JSON.parse(vm.runInContext("JSON.stringify(documentFilters.req)", sandbox)).tag;
  const savedReqTag = (path) => JSON.parse(storage.get(filtersKeyOf(path)) ?? "{}").docReq?.tag;
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  byId.get("documents-tab-req").click();
  await flush();
  storage.delete(filtersKeyOf(PROJECT_B));
  await gotoProject(PROJECT_B, docsB);
  await setDocFilter("documents-tag-filter", "流程");
  assert(
    liveReqTag() === "流程" && savedReqTag(PROJECT_B) === "流程",
    `前置失败:项目乙的标签筛选没设上或没落盘(内存 ${liveReqTag()} / 落盘 ${storage.get(filtersKeyOf(PROJECT_B))})`,
  );

  await gotoProject(PROJECT, docsA);
  let releaseStale;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseStale = resolve; }));
  const stale = sandbox.refreshDocs(); // 替项目甲发出,此刻卡在闸门上
  await settle();
  invokeGates.delete("docs_snapshot"); // 只卡住上面那一次:已在 await 的调用握着自己那个 promise
  await gotoProject(PROJECT_B, docsB);
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]'),
    "前置失败:切到项目乙后列表不是乙的",
  );
  assert(liveReqTag() === "流程", `前置失败:项目乙自己的标签筛选没回填(${liveReqTag()})`);

  // 在途的那一次现在才落地,带回来的是**项目甲**的数据。
  payloads.docs_snapshot = docsA;
  releaseStale();
  await stale;
  payloads.docs_snapshot = docsB; // 还原,免得随后的定时器刷新又拿到甲的数据(那是另一回事)
  await flush();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]')
      && !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "上一个项目的在途快照被画到了当前项目上(切项目瞬间闪一下上一个项目的列表)",
  );
  assert(
    liveReqTag() === "流程",
    `上一个项目的在途快照把当前项目的标签筛选清掉了(内存):${liveReqTag()}`,
  );
  assert(
    savedReqTag(PROJECT_B) === "流程",
    `上一个项目的在途快照把当前项目的标签筛选清掉并落进了当前项目的键(用户从没动过,重启也回不来):${storage.get(filtersKeyOf(PROJECT_B))}`,
  );

  // refreshDocsSoon 走同一条路,而且更容易撞上:它由 agent 的文档变更事件驱动,自带 400ms
  // 合并窗口,定时器落地时用户早就可能切走了。两个函数各测一次,少一个就是漏一条真实路径。
  await gotoProject(PROJECT, docsA);
  assert(liveReqTag() === "all", `前置失败:项目甲的标签筛选不是干净的(${liveReqTag()})`);
  let releaseSoon;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseSoon = resolve; }));
  // 手工点火,且**不 await**:回调此刻正卡在闸门上,drainTimersOnce 会按 300ms 超时判红。
  // 只点火 refreshDocsSoon 自己排的那一个,不波及别处已排队的定时器。
  const timersBefore = new Set(pendingTimers);
  sandbox.refreshDocsSoon();
  for (const handle of [...pendingTimers]) {
    if (timersBefore.has(handle) || handle.interval) continue;
    pendingTimers.delete(handle);
    void handle.fn();
  }
  await settle();
  assert(
    invokeArgs.at(-1)?.cmd === "docs_snapshot" && invokeArgs.at(-1)?.args?.projectDir === PROJECT,
    `前置失败:refreshDocsSoon 没有替项目甲发出在途的 docs_snapshot(${JSON.stringify(invokeArgs.at(-1))})`,
  );
  invokeGates.delete("docs_snapshot");
  await gotoProject(PROJECT_B, docsB);
  payloads.docs_snapshot = docsA;
  releaseSoon();
  for (let i = 0; i < 12; i += 1) await settle();
  payloads.docs_snapshot = docsB;
  await flush();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]')
      && !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "refreshDocsSoon 的在途快照被画到了已经切走的项目上",
  );
  assert(
    liveReqTag() === "流程" && savedReqTag(PROJECT_B) === "流程",
    `refreshDocsSoon 的在途快照(上一个项目的数据)把当前项目的标签筛选清掉并落盘了:内存 ${liveReqTag()} / 落盘 ${storage.get(filtersKeyOf(PROJECT_B))}`,
  );

  // 收尾:调回全部、切回项目甲、清掉项目乙的键、还原快照。
  await setDocFilter("documents-tag-filter", "all");
  await gotoProject(PROJECT, savedDocsPayload);
  storage.delete(filtersKeyOf(PROJECT_B));
  delete payloads.projects_select;
  payloads.docs_snapshot = savedDocsPayload;
  await sandbox.refreshDocs();
  await flush();
  assert(liveReqTag() === "all", `收尾失败:标签没调回全部(${liveReqTag()})`);
}

// ---------- 旧项目的刷新失败不得作废新项目刚排的跳转高亮(D-250) ----------
// 上一块钉的是**成功**路径按项目收敛。catch 里的 clearPendingJump() 没有同样的守卫:
// 替旧项目发出的那次刷新若在用户切走之后才抛错,会把**新项目刚排上的**跳转高亮一并作废
// ——用户点了条目引用跳过去,却看不出落在哪一条。同一条路径上的不对称(成功收敛、失败不收敛)
// 正是 D-211 说的「承诺与实现脱节」。
// 手法:替甲发出的那次 refreshDocs 卡在闸门上 → 切到乙 → 在乙里排一个跳转高亮(它自己那次
// 刷新也卡住,免得当场被消费掉)→ 再让甲那次以**失败**落地。失败判定在闸门之后,所以顺序
// 必须是「先注入失败、再放行闸门」。
{
  await gotoProject(PROJECT, docsA);
  let releaseStaleFail;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseStaleFail = resolve; }));
  const staleFail = sandbox.refreshDocs(); // 替项目甲发出,此刻卡在闸门上
  await settle();
  invokeGates.delete("docs_snapshot"); // 只卡住上面那一次
  await gotoProject(PROJECT_B, docsB);
  // 离开单页视图,jumpToEntry 才会走「先切视图 + 排挂起高亮」那条路。
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  assert(!byId.get("view-documents").classList.contains("active"), "前置失败:未离开单页视图");
  assert(document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]'), "前置失败:项目乙的列表里没有 R-900");
  let releaseJumpB;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseJumpB = resolve; }));
  sandbox.jumpToEntry("R-900");
  for (const frame of rafQueue.splice(0)) frame();
  await drainTimersOnce("新项目跳转等待刷新");
  // 只推微任务:这一步要的是「乙那次刷新卡住、pendingJumpId 挂着」,不能让定时器插进来。
  for (let i = 0; i < 12; i += 1) await settle();
  invokeGates.delete("docs_snapshot");
  assert(
    vm.runInContext("pendingJumpId", sandbox) === "R-900",
    `前置失败:项目乙没有排上跳转高亮(实得 ${JSON.stringify(vm.runInContext("pendingJumpId", sandbox))})`,
  );
  // 注入的刷新失败会走 toastError:那正是被测的那条 catch,不判红。
  expectedPersistentError = "Failed to refresh project documents";
  const hitsBefore = expectedPersistentHits;
  invokeFailures.set("docs_snapshot", "冒烟注入:旧项目的在途刷新撞上目录被删/文件被锁/解析失败");
  releaseStaleFail();
  await staleFail;
  invokeFailures.delete("docs_snapshot");
  assert(expectedPersistentHits > hitsBefore, "前置失败:注入的 docs_snapshot 失败没有走到 refreshDocs 的 catch");
  expectedPersistentError = null;
  assert(
    vm.runInContext("pendingJumpId", sandbox) === "R-900",
    "旧项目的刷新失败作废了新项目刚排的跳转高亮:refreshDocs 的 catch 里 clearPendingJump() 没有项目守卫(成功路径按项目收敛了、失败路径没有)",
  );
  // 高亮还得真能兑现:守卫若写成「永远不清」,上面那条断言会被另一个错误盖过去。
  releaseJumpB();
  for (let i = 0; i < 12; i += 1) await settle();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]')?.classList.contains("ref-highlight"),
    "项目乙自己那次刷新没有兑现挂起的跳转高亮(守卫收得过紧,或高亮被别处清掉了)",
  );
  await flush();
}

// ---------- refreshDocsSoon 的失败路径同样要按项目收敛(单独钉,D-250) ----------
// 与上一条同病、独立出口(console.error,不是 toastError),而且更容易撞上:它由 agent 的
// 文档变更事件驱动、自带 400ms 合并窗口,定时器落地时用户早就可能切走了。
// 上面「悬挂高亮」那一族已经实测过一次:只修 refreshDocs 那处、留着这处,整套照样全绿。
{
  await gotoProject(PROJECT, docsA);
  let releaseStaleSoon;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseStaleSoon = resolve; }));
  // 手工点火,且**不 await**:回调此刻正卡在闸门上,drainTimersOnce 会按 300ms 超时判红。
  // 只点火 refreshDocsSoon 自己排的那一个,不波及别处已排队的定时器。
  const timersBefore = new Set(pendingTimers);
  sandbox.refreshDocsSoon();
  for (const handle of [...pendingTimers]) {
    if (timersBefore.has(handle) || handle.interval) continue;
    pendingTimers.delete(handle);
    void handle.fn();
  }
  await settle();
  assert(
    invokeArgs.at(-1)?.cmd === "docs_snapshot" && invokeArgs.at(-1)?.args?.projectDir === PROJECT,
    `前置失败:refreshDocsSoon 没有替项目甲发出在途的 docs_snapshot(${JSON.stringify(invokeArgs.at(-1))})`,
  );
  invokeGates.delete("docs_snapshot");
  await gotoProject(PROJECT_B, docsB);
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  let releaseJumpB;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseJumpB = resolve; }));
  sandbox.jumpToEntry("R-900");
  for (let i = 0; i < 12; i += 1) await settle();
  for (const frame of rafQueue.splice(0)) frame();
  await drainTimersOnce("新项目跳转等待刷新 soon");
  invokeGates.delete("docs_snapshot");
  assert(
    vm.runInContext("pendingJumpId", sandbox) === "R-900",
    `前置失败:项目乙没有排上跳转高亮(实得 ${JSON.stringify(vm.runInContext("pendingJumpId", sandbox))})`,
  );
  // 被测的正是 refreshDocsSoon 那条 catch,它只 console.error —— 开窗放行,出了这段立刻收回。
  expectedConsoleError = "冒烟注入";
  const consoleHitsBefore = expectedConsoleHits;
  invokeFailures.set("docs_snapshot", "冒烟注入:refreshDocsSoon 的在途刷新撞上目录被删/文件被锁/解析失败");
  releaseStaleSoon();
  for (let i = 0; i < 12; i += 1) await settle();
  invokeFailures.delete("docs_snapshot");
  assert(
    expectedConsoleHits > consoleHitsBefore,
    "前置失败:注入的 docs_snapshot 失败没有走到 refreshDocsSoon 的 catch(这一段根本没测到目标路径)",
  );
  expectedConsoleError = null;
  assert(
    vm.runInContext("pendingJumpId", sandbox) === "R-900",
    "旧项目的 refreshDocsSoon 刷新失败作废了新项目刚排的跳转高亮:它那条 catch 里的 clearPendingJump() 没有项目守卫",
  );
  releaseJumpB();
  for (let i = 0; i < 12; i += 1) await settle();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]')?.classList.contains("ref-highlight"),
    "项目乙自己那次刷新没有兑现挂起的跳转高亮",
  );
  await flush();
}

// ---------- R-174 子代理面板:独立分区、六字段真实数据、单条停止、transcript ----------
{
  // 打开面板:agent-toggle 应切出 #agent-panel 并收起 #bg-panel(互斥)。
  const agentToggle = byId.get("agent-toggle");
  assert(agentToggle, "子代理面板缺少 rail 开关");
  const agentPanel = byId.get("agent-panel");
  const bgPanel = byId.get("bg-panel");
  agentToggle.click();
  assert(!agentPanel.classList.contains("hidden"), "点击 agent-toggle 后 #agent-panel 未展开");
  assert(bgPanel.classList.contains("hidden"), "子代理面板打开时活动面板未收起(互斥切换失败)");
  agentToggle.click(); // 收起
  assert(agentPanel.classList.contains("hidden"), "再次点击 agent-toggle 后 #agent-panel 未收起");
  agentToggle.click(); // 再展开,后续断言用
  // 建条:task 的 tool-start 进子代理面板(编排派发带 phase,模型自派 name=task)。
  const sA = handlers.get("kz:tool-start");
  const pA = handlers.get("kz:task-progress");
  const eA = handlers.get("kz:tool-end");
  handlers.get("kz:meta")({ payload: { model: "smoke-provider:main", profile: "dev", contextLimit: 65536, sessionId: "sess-smoke" } });
  handlers.get("kz:turn")({ payload: { step: 1, maxSteps: 1, sessionId: "sess-smoke" } });
  assert(/等待模型|Waiting for model/.test(byId.get("status-tokens").textContent), "长 prefill 期间上下文旧值未标注等待模型");
  handlers.get("kz:step")({ payload: { input: 200, output: 100, cacheRead: 20, cacheWrite: 0, sessionId: "sess-smoke" } });
  assert(!/等待模型|Waiting for model/.test(byId.get("status-tokens").textContent), "真实 step usage 到达后上下文仍标注等待模型");
  assert(byId.get("status-tokens").textContent.includes("ctx 0.2k/66k (0%)"), "真实 usage 后 context_limit 百分比展示不准确");
  handlers.get("kz:ask")({ payload: { id: 991, kind: "permission", action: "read", resource: "src/main.rs", source: "parallel", sessionId: "sess-smoke" } });
  handlers.get("kz:permission-resolved")({ payload: { tool_call_id: "ask-991", action: "read", resource: "src/main.rs", decision: "deny", source: "user", sessionId: "sess-smoke" } });
  sA({ payload: { id: "my_scout", name: "task", summary: "勘察文件结构", input: { prompt: "review the repo", phase: "scouting", role: "my_scout" }, sessionId: "sess-smoke" } });
  await flush();
  const a1 = document.querySelector('#agent-running .bg-entry[data-agent-id="my_scout"]');
  assert(a1, "运行中的子代理未进入 Running 区");
  assert(a1.querySelector(".bg-tool")?.textContent.includes("my_scout"), "编排派发的子代理未以角色名作名称");
  assert(a1.querySelector(".bg-phase-badge")?.textContent.includes("Scouting"), "子代理缺少类型(阶段)徽章");
  assert(a1.dataset.agentElapsed === "0", "子代理缺少已运行时长字段");
  // 六字段中的 token/工具调用数/当前工具名必须来自真实 trace(usage/start 事件)。
  pA({ payload: { id: "my_scout", text: "读取中", trace: { child_id: "c1", phase: "start", name: "read", summary: "src/main.rs", input: { path: "src/main.rs" } }, sessionId: "sess-smoke" } });
  await flush();
  assert(a1.dataset.agentCurrentTool === "read", "子代理未显示当前正在用的工具名(trace 数据源)");
  pA({ payload: { id: "my_scout", text: "读取中", trace: { child_id: "c1", phase: "end", name: "read", ok: true, preview: "ok" }, sessionId: "sess-smoke" } });
  pA({ payload: { id: "my_scout", text: "统计", trace: { child_id: "c1", phase: "usage", usage: { input: 100, output: 50, cache_read: 10, cache_write: 5 } }, sessionId: "sess-smoke" } });
  await flush();
  assert(a1.dataset.agentCurrentTool === "read", "工具结束后当前工具名应保留(idle 态)");
  assert(a1.querySelector(".bg-meta")?.textContent.includes("tokens"), "子代理元信息未显示累计 token");
  assert(a1.querySelector(".bg-meta")?.textContent.includes("tool calls"), "子代理元信息未显示工具调用次数");
  // transcript:展开 detail 应有完整调用序列(名称 + 入参)。
  a1.querySelector(".bg-title").click();
  assert(a1.querySelector(".agent-call"), "子代理缺少 transcript 调用序列");
  assert(a1.querySelector(".agent-call pre")?.textContent.includes("src/main.rs"), "transcript 未包含调用的完整入参");
  // 单条停止:运行中的子代理有停止按钮,点击走 stop_task 而非 stop_run。
  const stopBtn = [...a1.querySelectorAll(".bg-actions button")].find((b) => b.textContent === "Stop");
  assert(stopBtn, "运行中的子代理缺少单条停止按钮");
  const beforeStop = invokeLog.filter((cmd) => cmd === "stop_run").length;
  stopBtn.click();
  await flush();
  assert(
    invokeArgs.some((a) => a.cmd === "stop_task" && a.args?.taskId === "my_scout"),
    "单条停止未调用 stop_task(或参数缺少 taskId)",
  );
  assert(invokeLog.filter((cmd) => cmd === "stop_run").length === beforeStop, "单条停止误调用了 stop_run(整轮停止)");
  // 被停终态:tool-end ok=false +「被停」文案 → 移到 Finished 区、标 stopped、读槽释放由后端负责。
  eA({ payload: { id: "my_scout", name: "task", ok: false, preview: "子代理已被停止", display: null, sessionId: "sess-smoke" } });
  await flush();
  const a1f = document.querySelector('#agent-finished .bg-entry[data-agent-id="my_scout"]');
  assert(a1f, "被停的子代理未移入 Finished 区");
  assert(a1f.dataset.bgStatus === "stopped", "被停的子代理未标记 stopped 终态");
  assert(a1f.querySelector(".bg-meta")?.textContent.includes("Stopped"), "被停的子代理终态元信息未显示「已停止」");
  assert(!a1f.querySelectorAll(".bg-actions button").some((b) => b.textContent === "Stop"), "结束的子代理不应残留停止按钮");
  handlers.get("kz:done")({ payload: { sessionId: "sess-smoke", steps: 1, history: 1, halted: false, autoAction: { type: "NoContinue" } } });
  await flush();
  const auditCard = byId.get("agent-audit");
  assert(auditCard && !auditCard.classList.contains("hidden"), "运行结束后未显示运行审计摘要卡片");
  const auditText = byId.get("agent-audit-facts")?.textContent || "";
  const modelText = byId.get("agent-audit-models")?.textContent || "";
  assert(auditText.includes("主代理调用") || auditText.includes("Primary calls"), "审计摘要缺少主代理调用统计");
  assert(auditText.includes("子代理派发") || auditText.includes("Subagent dispatches"), "审计摘要缺少子代理派发统计");
  assert(auditText.includes("权限拒绝") || auditText.includes("Permission denials"), "审计摘要缺少权限拒绝统计");
  assert(modelText.includes("smoke-provider:main"), "审计摘要缺少主代理模型调用统计");
  assert(modelText.includes("fast"), "审计摘要缺少子代理模型调用统计");
  const traceButton = byId.get("agent-audit-trace");
  traceButton.click();
  assert(!bgPanel.classList.contains("hidden"), "运行审计摘要的运行轨迹入口未打开活动面板");
  agentToggle.click();
  assert(!agentPanel.classList.contains("hidden"), "打开运行轨迹后无法返回子代理面板");
  byId.get("activity-toggle").click(); // 切换到活动面板时应立即收起子代理面板
  assert(agentPanel.classList.contains("hidden"), "切换到活动面板后 #agent-panel 未自动收起");
  // Finished 区的条目有「打开」(transcript 视图入口)。
  assert(a1f.querySelectorAll(".bg-actions button").some((b) => b.textContent === "Open"), "Finished 区子代理缺少打开 transcript 入口");
  // 关闭只收起条目,后端历史仍保留;重新打开可恢复到 Finished,再删除才移除本次 UI 条目。
  a1f.querySelectorAll(".bg-actions button").find((b) => b.textContent === "Close")?.click();
  const a1c = document.querySelector('#agent-closed .bg-entry[data-agent-id="my_scout"]');
  assert(a1c, "关闭后的子代理未移入 Closed 区");
  assert(a1c.querySelectorAll(".bg-actions button").some((b) => b.textContent === "Open"), "Closed 条目缺少重新打开入口");
  a1c.querySelectorAll(".bg-actions button").find((b) => b.textContent === "Open")?.click();
  const a1reopened = document.querySelector('#agent-finished .bg-entry[data-agent-id="my_scout"]');
  assert(a1reopened, "Closed 条目点击 Open 后未回到 Finished 区");
  a1reopened.querySelectorAll(".bg-actions button").find((b) => b.textContent === "Close")?.click();
  document.querySelector('#agent-closed .bg-entry[data-agent-id="my_scout"]')?.querySelectorAll(".bg-actions button").find((b) => b.textContent === "Delete")?.click();
  assert(!document.querySelector('#agent-closed .bg-entry[data-agent-id="my_scout"]'), "删除已关闭子代理后 UI 条目仍存在");
  // Clear 清空 Finished/Closed 区,但不会影响 Running。
  byId.get("agent-clear").click();
  await flush();
  assert(!document.querySelector('#agent-finished .bg-entry[data-agent-id="my_scout"]'), "Clear 未清空 Finished 区");
  // 面板已在切换到活动视图时收起,这里确认测试结束状态。
  assert(agentPanel.classList.contains("hidden"), "断言结束后 #agent-panel 未收起");
}
// ---------- D-350 面板 ✕ 关闭按钮:子代理面板头部的显式关闭入口 ----------
{
  // 子代理面板:展开后头部应有 ✕(#agent-close),点击后面板收起,且不误弹活动面板
  // (agentClosePanel 只回到当前 activityPanelOpen 状态)。
  const agentPanel = byId.get("agent-panel");
  const bgPanel = byId.get("bg-panel");
  const agentClose = byId.get("agent-close");
  assert(agentClose, "D-350:子代理面板头部缺少 ✕ 关闭按钮(#agent-close)");
  assert(
    agentClose.title && agentClose.title.length > 0,
    "D-350:#agent-close 缺少 title 提示",
  );
  byId.get("agent-toggle").click(); // 展开子代理面板
  await flush();
  assert(!agentPanel.classList.contains("hidden"), "D-350:前置失败——#agent-panel 未展开");
  agentClose.click();
  await flush();
  assert(agentPanel.classList.contains("hidden"), "D-350:点击 #agent-close 后 #agent-panel 未收起");
  assert(
    bgPanel.classList.contains("hidden"),
    "D-350:agentClosePanel 误弹了活动面板(应回到 activityPanelOpen 状态)",
  );
  byId.get("agent-toggle").click(); // 复位互斥状态
  await flush();

}

// ---------- R-184 B 面:真实并列视图与合并前冲突预警 ----------
{
  await gotoProject(PROJECT, savedDocsPayload);
  const linesButton = document.querySelector('.activity-item[data-view="lines"]');
  assert(linesButton, "活动栏缺少并行线路入口");
  const beforeOpen = invokeArgs.length;
  linesButton?.click();
  await flush();
  const openCalls = invokeArgs.slice(beforeOpen);
  assert(
    openCalls.some((entry) => entry.cmd === "collaboration_snapshot" && entry.args?.projectDir === PROJECT),
    `打开并行线路没有读取真实 collaboration_snapshot(${JSON.stringify(openCalls)})`,
  );
  const lanes = document.querySelectorAll("#lines-list .line-lane");
  assert(lanes.length === 2, `并列视图没有渲染两条线路(实得 ${lanes.length})`);
  // harness 的 index.html 解析只登记静态 id,不重建完整父子树；动态线路需从
  // #lines-list 的真实后代读取，不能靠 #view-lines 汇总 textContent。
  const linesText = lanes.map((lane) => lane.textContent).join("\n");
  const claims = document.querySelectorAll("#lines-list .line-claim").map((node) => node.textContent);
  assert(
    claims.length === 2 && claims.some(claim => claim.startsWith("R-001")) && claims.some((claim) => /未取得条目|No claimed item/.test(claim)),
    `并列视图没有按 tracker 取得线分别显示 claim(${JSON.stringify(claims)})`,
  );
  // 冒烟前半段已切到英文；固定标签和恰好命中词典的中文值会被本地化，下面只断言
  // 语言无关的现场值，阶段另允许中英两种等价值。
  for (const expected of ["实现", "edit", "thread-a1", "crates/shared.rs"]) {
    assert(linesText.includes(expected), `并列视图漏掉真实字段:${expected}(实得:${linesText})`);
  }
  assert(linesText.includes("复核") || linesText.includes("Review"), `并列视图漏掉真实阶段复核(实得:${linesText})`);
  assert(
    document.querySelectorAll("#lines-list .line-agent-code").map((node) => node.textContent).join("") === "MA",
    "线路没有同时显示稳定的 M/A 文本身份",
  );
  assert(
    document.querySelectorAll("#lines-conflict-list .line-conflict").length === 1 &&
      (document.querySelector("#lines-conflict-list .line-conflict")?.textContent ?? "").includes("crates/shared.rs"),
    "共享改动文件没有在发起合并前形成可下钻的跨线冲突预警",
  );
  const semanticNote = byId.get("lines-semantic-note")?.textContent ?? "";
  assert(
    semanticNote.includes("语义层未检查") || semanticNote.includes("semantic overlap unchecked"),
    `并列视图缺少语义冲突未检查的固定边界提示(实得:${semanticNote})`,
  );
  assert(
    !openCalls.some((entry) => entry.cmd === "worktree_merge"),
    "查看冲突预警不应偷偷触发合并",
  );
  assert(document.querySelectorAll("#lines-list .line-close").length === 1, "非默认线路应有关闭入口，默认主线路不得显示关闭");
  const closeCallsBefore = invokeArgs.length;
  document.querySelector("#lines-list .line-close")?.click();
  await flush();
  assert(
    invokeArgs.slice(closeCallsBefore).some((entry) => entry.cmd === "process_close" && entry.args?.processId === "p|bg"),
    "线路页关闭按钮没有调用目标线路的 process_close",
  );
  // R-247 验收②:badge 只读 docs_snapshot 的 claimed_by；线即使空闲也仍是持有者。
  const claimedDocs = structuredClone(savedDocsPayload);
  claimedDocs.requirements[0].claimed_by = "claim-a1";
  sandbox.navigate_view("documents");
  await flush();
  sandbox.renderDocuments(claimedDocs);
  const claimedSnapshot = [
    ...payloads.collaboration_snapshot,
    {
      process_id: "p|claim", label: "认领线", branch: "claim-a1", worktree_path: "C:/smoke/wt/claim-a1",
      claim: "R-001", phase: "空闲", current_tool: null, running: false,
      steps: 1, input_tokens: 10, output_tokens: 5, changed_files: [],
    },
  ];
  sandbox.renderLines(claimedSnapshot);
  const claimedRow = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]');
  assert(claimedRow?.textContent.includes("● B"), `真实 claim 未渲染稳定线路代号:${claimedRow?.textContent}`);
  assert(
    claimedRow?.textContent.includes("被取得") || claimedRow?.textContent.includes("Claimed"),
    `真实 claim 未渲染「被取得」事实文案:${claimedRow?.textContent}`,
  );
  const unclaimedHead = document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]');
  assert(!unclaimedHead?.querySelector(".doc-claim-fact"), "排在队首但无人 claim 的条目不应显示被取得标记");
  // D-360 甲:claimed_by 还指着 claim-a1,但线列表里已经没有这条线(进程崩掉/用户关窗)。
  // 事实过期了,徽标答不出「被谁取得」——此前会照渲染,只把代号打成 "?"。
  sandbox.renderLines(payloads.collaboration_snapshot);
  const staleClaim = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]');
  assert(
    !staleClaim?.querySelector(".doc-claim-fact"),
    "claimed_by 指向的线已不在线时不得显示被取得标记(代号无从谈起)",
  );
  // D-360 乙:回到无 claimed_by 的文档 = D-354 的「无字段 = 默认线」编码。默认线
  // (d|smoke,worktree_path=null)在线、且 R-001 正是它占着的 WIP,此时徽标应该出现,
  // 且必须给得出真代号——绝不能是问号。
  sandbox.renderDocuments(savedDocsPayload);
  sandbox.renderLines(payloads.collaboration_snapshot);
  const defaultHeld = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"] .doc-claim-fact');
  assert(defaultHeld, "默认线在线且占着 R-001 时应显示被取得标记(D-354 编码:无字段=默认线)");
  assert(
    !defaultHeld.textContent.includes("?"),
    `被取得徽标不得出现问号代号:${defaultHeld.textContent}`,
  );
  // D-360 丙:引擎根本没在运行(一条线都没有)——用户截图的现场。此前每条 doing
  // 都会按状态推断成「默认线持有」,5 条 doing 一起带上「● ? 被取得」。
  sandbox.renderLines([]);
  sandbox.renderDocuments(savedDocsPayload);
  assert(
    document.querySelectorAll("#documents-req-list .doc-claim-fact, #documents-defect-list .doc-claim-fact").length === 0,
    "没有任何线在运行时,列表里不得出现任何被取得标记",
  );
  sandbox.renderDocuments(savedDocsPayload);
  sandbox.renderLines(payloads.collaboration_snapshot);
  await flush();
  assert(
    document.querySelectorAll("#lines-list .line-lane").every((lane) => !lane.className.includes("line-lane-initial")),
    "并行线路刷新不应挂载进入动画 class",
  );
  // 每条线一套鞭挞控件 + 一个模型选择。少了它们就退回「管 N 条线要切 N 次」——
  // 而并行线路页是唯一能一屏看全所有线的地方,鞭挞恰恰最需要在这里横向比对与操控。
  const laneRows = [...document.querySelectorAll("#lines-list .line-lane")];
  assert(laneRows.length >= 2, `线路页应渲染出全部线道,实得 ${laneRows.length}`);
  assert(laneRows.every((lane) => lane.querySelector(".line-auto-toggle input")), "线道缺少按线鞭挞开关");
  assert(laneRows.every((lane) => lane.querySelector(".line-auto-pause")), "线道缺少按线暂停按钮");
  assert(laneRows.every((lane) => lane.querySelector(".line-model-select")), "线道缺少按线模型选择");
  const mainLaneModel = document.querySelector('#lines-list .line-lane[data-process-id="d|smoke"] .line-model-select');
  assert(
    mainLaneModel?.value === "deepseek:deepseek-chat",
    `线道模型下拉未回显该线自己的模型,实得 ${mainLaneModel?.value}`,
  );
  // R-184 验收⑩:800/1024/1280 三档宽度下并列视图不崩——线道、冲突预警、语义提示仍渲染。
  for (const width of [800, 1024, 1280]) {
    windowShim.innerWidth = width;
    await flush();
    const lanesAt = document.querySelectorAll("#lines-list .line-lane");
    assert(lanesAt.length === 2, `${width}px 下列表视图线道缺失(实得 ${lanesAt.length})`);
    assert(
      document.querySelectorAll("#lines-conflict-list .line-conflict").length === 1,
      `${width}px 下冲突预警缺失`,
    );
    assert(
      (byId.get("lines-semantic-note")?.textContent ?? "").includes("未检查") ||
        (byId.get("lines-semantic-note")?.textContent ?? "").includes("unchecked"),
      `${width}px 下语义边界提示缺失`,
    );
  }
  windowShim.innerWidth = 1280;
}

// ---------- R-184 P5:收活五格(②不可跳过 + 门禁 + 合并) ----------
{
  // 五格需要 worktree_diff / worktree_gate 桩:diff 给一条真实差异,gate 给四步全过。
  payloads.worktree_diff = {
    path: "C:/smoke/wt/thread-a1",
    branch: "thread-a1",
    clean: false,
    files: ["crates/branch.rs"],
    diff: "diff --git a/crates/branch.rs b/crates/branch.rs\n+pub fn line_work() {}\n",
  };
  payloads.worktree_gate = [
    { name: "fmt", ok: true, summary: "" },
    { name: "clippy", ok: true, summary: "" },
    { name: "test", ok: true, summary: "test result: ok. 118 passed" },
    { name: "ui-smoke", ok: true, summary: "UI 运行时冒烟通过" },
  ];
  // R-222 防线②:合并后全量复用同一门禁步骤(主根),桩同值。
  payloads.worktree_post_merge_gate = [
    { name: "fmt", ok: true, summary: "" },
    { name: "clippy", ok: true, summary: "" },
    { name: "test", ok: true, summary: "test result: ok. 120 passed" },
    { name: "ui-smoke", ok: true, summary: "UI 运行时冒烟通过" },
  ];
  // 五格块先于下方工作树清单块执行,worktree_merge 桩在此补齐(下方同值覆盖无害)。
  payloads.worktree_merge = "已合并工作树分支 thread-a1;工作树仍保留,可检查后显式放弃";
  payloads.worktree_harvest_writeback = "已回写 R-184 收活记录。当前进展:\n2026-08-11 收活回写: 由 A 线交付并合并(branch thread-a1)。";
  // 桩里只有后台会话带 worktree_path → 只有它的 lane 有「收活」按钮。
  const lanes = [...document.querySelectorAll("#lines-list .line-lane")];
  const withHarvest = lanes.filter((lane) => lane.querySelector(".line-harvest-toggle"));
  assert(withHarvest.length === 1, `收活按钮应只出现在带工作树的线上(实得 ${withHarvest.length})`);
  const wtLane = withHarvest[0];
  assert(
    /未取得条目|No claimed item/.test(wtLane.querySelector(".line-claim")?.textContent ?? ""),
    "收活按钮出现在了错误的线上(应属于带工作树的后台会话)",
  );

  // 打开收活面板:六格结构(格1-4 收活,格5 合并后全量 R-222,格6 回写)。
  wtLane.querySelector(".line-harvest-toggle").click();
  await flush();
  // flush 可能同时跑到线路定时刷新,因此必须从当前 DOM 按 process_id 取 lane,
  // 不能继续使用刷新前已经脱离 DOM 的旧节点引用。
  const openedWtLane = [...document.querySelectorAll("#lines-list .line-lane")]
    .find((lane) => lane.dataset.processId === "p|bg");
  const panel = openedWtLane?.querySelector(".line-harvest");
  assert(panel, "点击收活未展开五格面板");
  // 自动刷新会重建线路卡片,但不应销毁用户已经展开且正在操作的收活面板。
  sandbox.renderLines(payloads.collaboration_snapshot);
  await flush();
  const refreshedWtLane = [...document.querySelectorAll("#lines-list .line-lane")]
    .find((lane) => lane.dataset.processId === "p|bg");
  const refreshedPanel = refreshedWtLane?.querySelector(".line-harvest");
  assert(refreshedPanel === panel, "线路刷新后收活面板被销毁或未按 process_id 复挂");
  // smoke 的 Element 不解析 innerHTML 拼的子节点,格号从面板文本提取数字序列。
  const panelText = panel.textContent;
  const stepNoSeq = ["1", "2", "3", "4", "5", "6"].filter((no) => panelText.includes(no)).join("");
  assert(stepNoSeq === "123456", `收活面板应呈现 1/2/3/4/5/6 六格(实得 ${stepNoSeq})`);

  // ② 不可跳过:未读 diff 前,格3(门禁)、格4(合并)、格5(合并后全量)、格6(回写)必须全部禁用。
  const gateRun = panel.querySelector(".harvest-gate-run");
  const mergeRun = panel.querySelector(".harvest-merge-run");
  const readConfirm = panel.querySelector(".harvest-read-confirm");
  const postMergeRun = panel.querySelector(".harvest-postmerge-run");
  const writebackRun = panel.querySelector(".harvest-writeback-run");
  assert(gateRun && mergeRun && readConfirm && postMergeRun && writebackRun, "收活面板缺少格2确认/格3门禁/格4合并/格5合并后全量/格6回写控件");
  assert(readConfirm.disabled, "未加载差异时「我已读过 diff」应禁用");
  assert(
    gateRun.disabled && mergeRun.disabled && postMergeRun.disabled && writebackRun.disabled,
    "② 不可跳过:未读 diff 前格3/格4/格5/格6必须全部禁用",
  );

  // 加载差异 → 确认 → 解锁格3/格4。
  const diffLoad = panel.querySelector(".harvest-diff-load");
  assert(diffLoad, "收活面板缺少「加载差异」按钮");
  diffLoad.click();
  await flush();
  assert(!readConfirm.disabled, "差异加载成功后「我已读过 diff」应可用");
  readConfirm.click();
  await flush();
  assert(
    panel.querySelector(".harvest-step.confirmed"),
    "确认后格2未进入已读状态(confirmed)",
  );
  assert(!gateRun.disabled, "② 不可跳过:已读 diff 后格3(门禁)应解锁");
  assert(
    mergeRun.disabled,
    "R-222 防线①:已读 diff 后格4(合并)必须仍禁用——合并前置是门禁,不是读 diff",
  );
  assert(
    writebackRun.disabled,
    "格5/格6 必须等合并+合并后全量完成才解锁:已读 diff 后不应可用(② 不可跳过)",
  );

  // 格3 门禁:worktree_gate 被真实调用,步骤结果渲染进面板。
  const gateCallsBefore = invokeArgs.length;
  gateRun.click();
  await flush();
  const gateCalls = invokeArgs.slice(gateCallsBefore).filter((e) => e.cmd === "worktree_gate");
  assert(
    gateCalls.length === 1 && gateCalls[0].args?.worktreePath === "C:/smoke/wt/thread-a1",
    `跑门禁没有带正确工作树调用 worktree_gate(${JSON.stringify(gateCalls)})`,
  );
  const gateRows = [...panel.querySelectorAll(".harvest-gate-step")];
  assert(gateRows.length >= 4, `门禁结果未逐步骤渲染(实得 ${gateRows.length})`);
  assert(
    gateRows.every((row) => row.classList.contains("ok")),
    "桩门禁应全部通过(ok),实得: " + gateRows.map((r) => `${r.dataset.gateName}:${r.className}`).join(","),
  );
  assert(
    panel.querySelector(".harvest-gate-pass")?.textContent.includes("门禁通过") ||
      panel.querySelector(".harvest-gate-pass")?.textContent.includes("All gates passed"),
    "门禁全过未显示通过结论",
  );
  assert(
    !mergeRun.disabled,
    "R-222 防线①:门禁通过后格4(合并)才解锁",
  );
  // D-505:dataset 不是门禁真源。即使展示属性被重渲染/清理,JS 状态仍应保留通过结论。
  delete mergeRun.dataset.gateOk;
  delete mergeRun.dataset.gateRan;
  assert(!mergeRun.disabled, "门禁通过状态不应依赖 merge button dataset");

  // 格4 合并:确认后调用 worktree_merge。
  const mergeCallsBefore = invokeArgs.length;
  mergeRun.click();
  for (let i = 0; i < 12; i += 1) await settle();
  await flush();
  // confirmWorktreeMerge 会先打 collaboration_snapshot,再打 worktree_merge。
  const mergeCalls = invokeArgs.slice(mergeCallsBefore).filter((e) => e.cmd === "worktree_merge");
  assert(
    mergeCalls.length === 1 && mergeCalls[0].args?.worktreePath === "C:/smoke/wt/thread-a1",
    `合并没有带正确工作树调用 worktree_merge(${JSON.stringify(mergeCalls)})`,
  );
  assert(
    (panel.querySelector(".harvest-merge-done")?.textContent ?? "").includes("已合并工作树"),
    `合并成功后未显示合并结果,panel 文本: ${(panel?.textContent ?? "panel 已摘除").slice(0, 200)}`,
  );

  // R-222 防线②:合并成功后解锁格5(合并后全量);格6 回写仍需合并后全量通过。
  assert(postMergeRun, "收活面板缺少格5「合并后全量」按钮");
  assert(
    !postMergeRun.disabled,
    "合并成功后格5(合并后全量)按钮必须解锁",
  );
  assert(
    writebackRun.disabled,
    "R-222 防线②:合并后全量通过前,格6 回写必须保持禁用",
  );

  // 合并后全量:主根调用 worktree_post_merge_gate,通过后解锁格6 回写。
  const postMergeCallsBefore = invokeArgs.length;
  postMergeRun.click();
  for (let i = 0; i < 12; i += 1) await settle();
  await flush();
  const postMergeCalls = invokeArgs
    .slice(postMergeCallsBefore)
    .filter((e) => e.cmd === "worktree_post_merge_gate");
  assert(
    postMergeCalls.length === 1,
    `合并后全量应调用 worktree_post_merge_gate(${JSON.stringify(postMergeCalls)})`,
  );
  const postMergeStepEl = postMergeRun.closest(".harvest-step");
  assert(
    postMergeStepEl.querySelector(".harvest-gate-pass")?.textContent.includes("合并后全量通过") ||
      postMergeStepEl.querySelector(".harvest-gate-pass")?.textContent.includes("Post-merge suite passed"),
    "合并后全量通过未显示结论",
  );
  assert(
    !writebackRun.disabled,
    "合并后全量通过后格6 回写才解锁",
  );
  // D-505:confirmed class 只做展示。清掉 class 后重新同步,回写仍由 JS 状态解锁。
  const postMergeStateStep = postMergeRun.closest(".harvest-step");
  postMergeStateStep.classList.remove("confirmed");
  panel.querySelector(".harvest-tracker-select")?.dispatchEvent({ type: "change" });
  await flush();
  assert(!writebackRun.disabled, "回写解锁不应依赖 post-merge confirmed class");

  // 格6 回写 tracker:合并+合并后全量通过后,点击调用 worktree_harvest_writeback 并渲染结果。
  const writebackOutput = panel.querySelector(".harvest-writeback-output");
  assert(writebackRun && writebackOutput, "收活面板缺少格6 回写控件");
  const writebackCallsBefore = invokeArgs.length;
  writebackRun.click();
  for (let i = 0; i < 12; i += 1) await settle();
  await flush();
  const writebackCalls = invokeArgs
    .slice(writebackCallsBefore)
    .filter((e) => e.cmd === "worktree_harvest_writeback");
  assert(
    writebackCalls.length === 1 &&
      writebackCalls[0].args?.worktreePath === "C:/smoke/wt/thread-a1" &&
      writebackCalls[0].args?.claim.includes("R-184") &&
      writebackCalls[0].args?.branch === "thread-a1",
    `格6 回写没有带正确参数调用 worktree_harvest_writeback(${JSON.stringify(writebackCalls)})`,
  );
  assert(
    (writebackOutput?.textContent ?? "").includes("已回写 R-184 收活记录"),
    `格6 回写成功后未渲染结果(实得: ${writebackOutput?.textContent ?? "(无)"})`,
  );
  assert(
    panel.querySelector(".harvest-step.confirmed"),
    "回写成功后格6 未进入已读/完成状态",
  );

  // D-314:即使 collaboration claim 未声明，线路对话唯一候选也必须自动回显。
  payloads.worktree_harvest_candidates = ["D-297"];
  const conversationCandidatePanel = sandbox.buildHarvestPanel(
    { ...payloads.collaboration_snapshot[1], claim: "未声明条目" },
    PROJECT,
    "A",
  );
  await flush();
  assert(
    conversationCandidatePanel.querySelector(".harvest-tracker-select")?.value === "D-297",
    "线路对话唯一候选 D-297 没有自动选中",
  );
  payloads.worktree_harvest_candidates = ["D-297", "R-184"];
  const multipleCandidatePanel = sandbox.buildHarvestPanel(
    { ...payloads.collaboration_snapshot[1], claim: "未声明条目" },
    PROJECT,
    "A",
  );
  await flush();
  const multipleCandidateSelect = multipleCandidatePanel.querySelector(".harvest-tracker-select");
  assert(!multipleCandidateSelect?.disabled && multipleCandidateSelect?.value === "", "多条对话候选必须等待用户明确选择，不能自动猜一条");
  multipleCandidateSelect.value = "D-297";
  multipleCandidateSelect.dispatchEvent({ type: "change" });
  assert(multipleCandidateSelect.value === "D-297", "多候选下用户选择 D-297 没有保留");
  payloads.worktree_harvest_candidates = [];

  // D-310:没有真实 R-/D- claim 时,合并仍可完成,但格5不得伪装成可回写入口。
  document.querySelector('#lines-list .line-lane[data-process-id="p|bg"] .line-harvest-toggle')?.click();
  const originalClaim = payloads.collaboration_snapshot[1].claim;
  payloads.collaboration_snapshot[1].claim = "未声明条目";
  sandbox.renderLines(payloads.collaboration_snapshot);
  await flush();
  const noClaimLane = [...document.querySelectorAll("#lines-list .line-lane")]
    .find((lane) => lane.dataset.processId === "p|bg");
  noClaimLane?.querySelector(".line-harvest-toggle")?.click();
  await flush();
  const currentNoClaimLane = [...document.querySelectorAll("#lines-list .line-lane")]
    .find((lane) => lane.dataset.processId === "p|bg");
  const noClaimPanel = currentNoClaimLane?.querySelector(".line-harvest");
  noClaimPanel?.querySelector(".harvest-diff-load")?.click();
  await flush();
  noClaimPanel?.querySelector(".harvest-read-confirm")?.click();
  await flush();
  noClaimPanel?.querySelector(".harvest-gate-run")?.click();
  await flush();
  noClaimPanel?.querySelector(".harvest-merge-run")?.click();
  await flush();
  const noClaimWriteback = noClaimPanel?.querySelector(".harvest-writeback-run");
  payloads.collaboration_snapshot[1].claim = originalClaim;
  payloads.worktree_harvest_candidates = ["R-184"];
  assert(noClaimWriteback?.disabled, "无有效 claim 时格5 回写入口必须保持禁用");
  assert(
    (noClaimPanel?.querySelector(".harvest-writeback-output")?.textContent ?? "") === "",
    "无有效 claim 时不应产生回写输出",
  );
  const noClaimCallsBefore = invokeArgs.length;
  // 真实浏览器不会为 disabled button 派发 click;假 DOM 的 click 不模拟该规范门禁,
  // 因而这里验证的是“禁用状态”本身,并确认未产生任何回写调用。
  assert(
    !invokeArgs.slice(noClaimCallsBefore).some((entry) => entry.cmd === "worktree_harvest_writeback"),
    "无有效 claim 时不应调用 worktree_harvest_writeback",
  );
}

// ---------- 线清单来自 git,前端不再持有清单状态(R-177 内容③ / D-251 / D-257) ----------
// 清单真源改成后端 `worktree_list`(它跑 `git worktree list --porcelain`)之后,
// 原来那两条护栏守的性质没有消失,只是换了形态,所以**等价重写而不是删除**:
//   D-251「切项目时清单不错位」→ 在途的清单响应落地时项目已切走,不得画进新项目的面板;
//   D-257「#worktrees-refresh 真的绑了监听器」→ 点击后必须真打出 worktree_list IPC。
// 另加一条新的反向断言:前端**不得再写任何 kz-worktrees 键**(清单状态已经全部下沉)。
//
// 两条断言各接一个变异开关(KZ_SMOKE_MUTATE=d251 / =d257):故意破坏被守护的行为,
// 期望脚本非零退出。这是把「删掉任一条即变红」从人工验证换成机械判据——不然一条
// 恒绿的断言和一条真护栏在 CI 里长得一模一样。
{
  const wtItem = (path, branch, bound_process = null) => ({ path, branch, clean: true, files: [], diff: "", bound_process });
  const WT_A1 = "C:/smoke/wt/thread-a1";
  const WT_A2 = "C:/smoke/wt/thread-a2";
  const WT_B = "C:/smoke/wt/thread-b";
  // 两个项目返回**不同**清单:D-251 只有这样才判得出来。
  payloads.worktree_list = (args) =>
    args?.projectDir === PROJECT_B
      ? [wtItem(WT_B, "thread-b")]
      : [wtItem(WT_A1, "thread-a1", "p|bg"), wtItem(WT_A2, "thread-a2")];
  payloads.process_create = {
    id: "p2|smoke", label: "线路 2", session_id: "sess-line-2", running: false,
    worktree_path: WT_A1, branch: "thread-a1", tracker_writes: false,
  };
  payloads.worktree_merge = "已合并工作树";
  payloads.worktree_discard = "已放弃工作树";

  // 写入去向探针:方向反过来了。以前查「有没有写错键」,现在查「还写不写」。
  const wtWrites = [];
  const rawSetItem = localStorageShim.setItem;
  localStorageShim.setItem = (key, value) => {
    if (String(key).startsWith("kz-worktrees:")) wtWrites.push(String(key));
    return rawSetItem(key, value);
  };

  // ---------- ⓪ 刷新按钮真的能刷新(D-257) ----------
  // 拦的是「按钮在 index.html 里,监听器却不在任何 JS 里」这个形态:7c5f022 抽
  // handleWorktreeAction 时,函数收尾的 `}` 吃掉了下一行 addEventListener 的前半段,
  // 剩下 `}("click", refreshWorktrees);` —— 语法合法、`node --check` 通过、静态 grep
  // 也只在标记里看得见那个 id,唯独点下去什么都不发生。所以断言必须是**点击后真的
  // 打出了 IPC**,不能退化成「源码里有这个字符串」。
  // 侧栏「隔离工作树」已降级为只读呈现(按钮全迁到并行线路页),手动刷新的落点因此
  // 换成线路页的 #lines-refresh —— 护栏等价重写到新入口,不是删掉。
  await gotoProject(PROJECT, docsA);
  const refreshBtn = byId.get("lines-refresh");
  assert(refreshBtn, "并行线路页的刷新按钮 #lines-refresh 不在 index.html 里了(工作树清单失去唯一手动刷新入口)");
  assert(
    !byId.get("worktrees-refresh"),
    "侧栏不该再有工作树刷新按钮(按钮已迁到并行线路页,侧栏只做只读呈现)",
  );
  assert(
    !document.querySelector("#worktree-list button"),
    "侧栏工作树清单不得再出现任何按钮(操作全在并行线路页)",
  );
  if (refreshBtn) {
    const beforeRefreshClick = invokeArgs.length;
    refreshBtn.click();
    await flush();
    const refreshCalls = invokeArgs.slice(beforeRefreshClick).filter((entry) => String(entry.cmd).startsWith("worktree_"));
    assert(
      refreshCalls.some((entry) => entry.cmd === "worktree_list" && entry.args?.projectDir === PROJECT),
      "点击 #lines-refresh 没有触发 worktree_list:按钮没有绑上 refreshWorktrees" +
        `(本次点击后的 worktree IPC:${JSON.stringify(refreshCalls)})`,
    );
    assert(
      document.querySelectorAll("#worktree-list .worktree-entry").length === 2,
      "点击 #lines-refresh 后工作树清单没有按 git 返回的数据重渲染(拉回来了却没画上去)",
    );
    // 清单不再逐条 worktree_diff:一次 IPC 拿全,清单越长省得越多。
    assert(
      !refreshCalls.some((entry) => entry.cmd === "worktree_diff"),
      `刷新清单不该再逐条打 worktree_diff(实得:${JSON.stringify(refreshCalls)})`,
    );
  }

  // ---------- ① 在途的清单响应不得画进已经切走的项目(D-251) ----------
  await gotoProject(PROJECT, docsA);
  let releaseList;
  invokeGates.set("worktree_list", new Promise((resolve) => { releaseList = resolve; }));
  refreshBtn.click();
  await settle();
  assert(
    invokeArgs.at(-1)?.cmd === "worktree_list" && invokeArgs.at(-1)?.args?.projectDir === PROJECT,
    `前置失败:刷新没有替项目甲发出 worktree_list(${JSON.stringify(invokeArgs.at(-1))})`,
  );
  invokeGates.delete("worktree_list"); // 只卡住上面那一次
  await gotoProject(PROJECT_B, docsB);
  refreshBtn.click();
  await flush();
  assert(
    document.querySelectorAll("#worktree-list .worktree-entry").length === 1,
    "前置失败:切到项目乙之后清单没按乙的数据渲染",
  );
  releaseList();
  for (let i = 0; i < 12; i += 1) await settle();
  await flush();
  const paintedAfterSwitch = [...document.querySelectorAll("#worktree-list .worktree-entry")]
    .map((row) => row.textContent);
  assert(
    paintedAfterSwitch.length === 1 && paintedAfterSwitch[0].includes("thread-b"),
    "项目甲在途的工作树清单被画进了项目乙的面板(await 之后少了一次 currentProject 复查):" +
      `实得 ${JSON.stringify(paintedAfterSwitch)}`,
  );

  // ---------- ② 侧栏不再直达合并，只能进入同一收活五格 ----------
  const mergeButton = document.querySelector("#worktree-list .worktree-merge");
  assert(!mergeButton, "侧栏仍保留绕过人读 diff 的直接合并入口(D-305)");
  await gotoProject(PROJECT, docsA);
  await sandbox.refreshWorktrees();
  const harvestButton = [...document.querySelectorAll("#worktree-list .worktree-harvest")]
    .find((button) => button.parentElement?.parentElement?.textContent.includes("thread-a1"));
  const beforeHarvest = invokeArgs.length;
  harvestButton?.click();
  await flush();
  const mergeCalls = invokeArgs.slice(beforeHarvest);
  assert(
    !mergeCalls.some((entry) => entry.cmd === "worktree_merge"),
    `点击侧栏收活竟直接触发了 worktree_merge(${JSON.stringify(mergeCalls)})`,
  );
  assert(
    mergeCalls.some((entry) => entry.cmd === "collaboration_snapshot") &&
      document.querySelector('.activity-item[data-view="lines"]')?.classList.contains("active"),
    `侧栏收活没有进入线路视图并刷新统一五格数据源(${JSON.stringify(mergeCalls)})`,
  );

  // ---------- ③ 建线原子创建「工作树 + 进程绑定」,前端不维护影子清单 ----------
  await gotoProject(PROJECT, docsA);
  const beforeAdd = invokeArgs.length;
  const addButton = byId.get("worktree-add");
  const linesAddButton = byId.get("lines-add");
  const workItemSelect = byId.get("lines-work-item");
  assert(
    workItemSelect?.options.some((option) => option.value === "D-001"),
    "开线区没有列出未被持有且未阻塞的 D-001",
  );
  workItemSelect.value = "";
  workItemSelect.dispatchEvent({ type: "change" });
  assert(linesAddButton.disabled, "未选择条目时按条目开线按钮必须保持禁用");
  workItemSelect.value = "D-001";
  workItemSelect.dispatchEvent({ type: "change" });
  assert(!linesAddButton.disabled, "选择可领取条目后按条目开线按钮仍未解锁");
  linesAddButton.click();
  addButton.click();
  assert(addButton.disabled && linesAddButton.disabled, "并行线路创建期间两个入口没有同步禁用");
  assert(
    addButton.getAttribute("aria-busy") === "true" && linesAddButton.getAttribute("aria-busy") === "true",
    "并行线路创建期间入口没有暴露 aria-busy",
  );
  assert(/创建中|Creating/.test(linesAddButton.textContent), "线路页按钮没有显示创建中反馈");
  await flush();
  const addCalls = invokeArgs.slice(beforeAdd);
  const processCreateCalls = addCalls.filter((entry) => entry.cmd === "process_create");
  assert(
    processCreateCalls.length === 1 && processCreateCalls[0].args?.projectDir === PROJECT &&
      /^line-\d+-\d+$/.test(processCreateCalls[0].args?.worktreeName ?? "") &&
      processCreateCalls[0].args?.phasePipeline === false &&
      processCreateCalls[0].args?.trackerWrites === false &&
      processCreateCalls[0].args?.workItemId === "D-001",
    `新建线路没有原子发出唯一命名的 process_create(${JSON.stringify(addCalls)})`,
  );
  assert(!addButton.disabled && !linesAddButton.disabled, "并行线路创建完成后两个入口没有恢复");
  assert(
    !addButton.hasAttribute("aria-busy") && !linesAddButton.hasAttribute("aria-busy") &&
      /按条目开线|Open line for item/.test(linesAddButton.textContent),
    "并行线路创建完成后忙碌状态或按钮文案没有恢复",
  );
  assert(
    !addCalls.some((entry) => entry.cmd === "worktree_create"),
    "建线仍在调用只建树不绑进程的 worktree_create",
  );
  assert(
    addCalls.some((entry) => entry.cmd === "worktree_list"),
    "新建之后没有重新向 git 要清单(前端已不持有清单,不刷就看不见新树)",
  );

  // ---------- ④ 放弃工作树后必须刷新进程投影 ----------
  // 后端会同步注销绑定进程；若这里只刷新 worktree_list，旧线路 tab 仍会把已删
  // 目录作为 cwd 发送，直到 provider/tool 报“工作目录不存在”才暴露。这个断言
  // 要求放弃动作后至少重新拉进程和工作树两份真源。
  await gotoProject(PROJECT, docsA);
  // 放弃按钮已迁到并行线路页的工作树操作台;侧栏只读,那里才有线路上下文。
  const discardButton = document.querySelector("#lines-worktree-list .worktree-discard");
  assert(discardButton, "并行线路页的工作树清单缺少放弃按钮,孤儿树没有任何出口");
  if (discardButton) {
    const beforeDiscard = invokeArgs.length;
    discardButton.click();
    await flush();
    const discardCalls = invokeArgs.slice(beforeDiscard);
    assert(
      discardCalls.some((entry) => entry.cmd === "worktree_discard"),
      `放弃按钮没有调用 worktree_discard(${JSON.stringify(discardCalls)})`,
    );
    assert(
      discardCalls.some((entry) => entry.cmd === "process_list" && entry.args?.projectDir === PROJECT),
      `放弃后未刷新进程页签，旧线路会残留(${JSON.stringify(discardCalls)})`,
    );
    assert(
      discardCalls.some((entry) => entry.cmd === "worktree_list" && entry.args?.projectDir === PROJECT),
      `放弃后未刷新工作树清单(${JSON.stringify(discardCalls)})`,
    );
  }

  // ---------- ⑤ 前端不得再写任何 kz-worktrees 键 ----------
  assert(
    wtWrites.length === 0,
    `前端仍在写 localStorage 工作树清单(${wtWrites.join(" / ")}):清单真源已经是 git,` +
      "留着这份影子清单只会在两处不一致时误导用户",
  );

  // 收尾:摘掉写入探针与桩,切回项目甲并还原快照。
  localStorageShim.setItem = rawSetItem;
  delete payloads.worktree_list;
  delete payloads.process_create;
  delete payloads.worktree_merge;
  delete payloads.worktree_discard;
  await gotoProject(PROJECT, savedDocsPayload);
  delete payloads.projects_select;
  payloads.docs_snapshot = savedDocsPayload;
  await sandbox.refreshDocs();
  await flush();
}

// ---------- D-435:询问弹窗可暂时收起并恢复上下文 ----------
{
  const overlay = byId.get("ask-overlay");
  const reopen = byId.get("ask-reopen");
  const askHandler = handlers.get("kz:ask");
  const answerCalls = () => invokeArgs.filter(({ cmd }) => cmd === "answer_ask");
  vm.runInContext('activeSessionId = "sess-smoke"', sandbox);
  askHandler?.({
    payload: {
      id: 700, sessionId: "sess-smoke", kind: "question",
      question: "上下文问题", options: ["保留上下文"], default: "", multiple: true,
    },
  });
  await flush();
  const option = byId.get("ask-options").querySelector(".ask-option");
  byId.get("ask-answer").value = "补充回答";
  byId.get("ask-answer").dispatchEvent({ type: "input" });
  option.click();
  const beforeCollapse = answerCalls().length;
  byId.get("ask-collapse").click();
  await flush();
  assert(overlay.classList.contains("hidden"), "D-435:收起后询问弹窗仍覆盖上下文");
  assert(!reopen.classList.contains("hidden"), "D-435:收起后缺少重新打开入口");
  assert(byId.get("ask-answer").value === "补充回答", "D-435:收起后已填写回答丢失");
  assert(option.classList.contains("selected"), "D-435:收起后已选选项丢失");
  assert(answerCalls().length === beforeCollapse, "D-435:收起操作不应提交回答");
  reopen.click();
  await flush();
  assert(!overlay.classList.contains("hidden"), "D-435:点击重新打开后询问弹窗未恢复");
  assert(byId.get("ask-question").textContent === "上下文问题", "D-435:重新打开后问题文本丢失");
  assert(byId.get("ask-answer").value === "补充回答", "D-435:重新打开后回答内容丢失");
  assert(byId.get("ask-options").querySelector(".ask-option").classList.contains("selected"), "D-435:重新打开后选项状态丢失");
  byId.get("ask-submit").click();
  await flush();
  assert(answerCalls().at(-1)?.args?.reply === "保留上下文\n补充回答", `D-435:恢复后提交内容不对:${JSON.stringify(answerCalls().at(-1)?.args)}`);
  assert(overlay.classList.contains("hidden"), "D-435:恢复后提交未关闭询问弹窗");
}

// ---------- D-337:ask 弹窗 question 档位的多选(声明多选时点选项不再立即提交) ----------
// 老行为:question 的每个选项是"点击即提交"的按钮,问题文本写着「可多选」也选不了多个。
// 新契约:multiple=true 或问题文本声明多选(兜底)时,选项变成可勾选,提交回答才汇总;
// 默认档位(未声明多选)点击即提交的行为保持不变。
{
  const overlay = byId.get("ask-overlay");
  const answerCalls = () => invokeArgs.filter(({ cmd }) => cmd === "answer_ask");
  vm.runInContext('activeSessionId = "sess-smoke"', sandbox);
  const clearAsks = async () => {
    while (!overlay.classList.contains("hidden") && byId.get("ask-cancel")) {
      byId.get("ask-cancel").click();
      await flush();
    }
  };
  await clearAsks();

  // ① 显式 multiple=true:点选项只切换勾选,提交回答才汇总(所选选项 + 补充文本)。
  askHandler?.({
    payload: {
      id: 701, sessionId: "sess-smoke", kind: "question",
      question: "问题 701", options: ["A 选项一", "B 选项二"], default: "", multiple: true,
    },
  });
  await flush();
  assert(!overlay.classList.contains("hidden"), "D-337:多选 question 弹窗未弹出");
  const optionsBox = byId.get("ask-options");
  assert(optionsBox.classList.contains("multi"), "D-337:multiple=true 时选项容器未标 multi");
  const optionButtons = [...optionsBox.querySelectorAll(".ask-option")];
  assert(optionButtons.length === 2, `D-337:多选选项数不对:${optionButtons.length}`);
  const beforeClicks = answerCalls().length;
  optionButtons[0].click();
  await flush();
  assert(answerCalls().length === beforeClicks, "D-337:多选档位点一个选项就立即提交了(老 bug 复发)");
  assert(optionButtons[0].classList.contains("selected"), "D-337:点过的选项没有选中标记");
  assert(optionButtons[0].getAttribute("aria-pressed") === "true", "D-337:选中选项 aria-pressed 未置 true");
  optionButtons[1].click();
  assert(optionButtons[1].classList.contains("selected"), "D-337:第二个选项未选中");
  optionButtons[0].click();
  assert(!optionButtons[0].classList.contains("selected"), "D-337:再点一次应取消选中");
  optionButtons[0].click();
  assert(byId.get("ask-submit").disabled === false, "D-337:已选选项时提交按钮仍被禁用");
  byId.get("ask-answer").value = "补充说明文字";
  byId.get("ask-answer").dispatchEvent({ type: "input" });
  byId.get("ask-submit").click();
  await flush();
  const multiAnswer = answerCalls().at(-1)?.args;
  assert(
    multiAnswer && multiAnswer.reply === "B 选项二\nA 选项一\n补充说明文字",
    `D-337:多选提交的汇总不对(应按勾选顺序含所选选项与补充文本):${JSON.stringify(multiAnswer)}`,
  );
  assert(overlay.classList.contains("hidden"), "D-337:多选提交后弹窗未关闭");
  await clearAsks();

  // ② 文本声明「可多选」的兜底:工具没传 multiple 也进多选档位(历史问题文本的形态)。
  askHandler?.({
    payload: {
      id: 702, sessionId: "sess-smoke", kind: "question",
      question: "你观察到的不匹配具体指哪一块?(可多选/补充)",
      options: ["A 测试节奏", "B 提交粒度"], default: "", multiple: false,
    },
  });
  await flush();
  assert(byId.get("ask-options").classList.contains("multi"), "D-337:问题文本声明「可多选」未进入多选档位");
  [...byId.get("ask-options").querySelectorAll(".ask-option")].forEach((button) => button.click());
  byId.get("ask-submit").click();
  await flush();
  assert(
    answerCalls().at(-1)?.args?.reply === "A 测试节奏\nB 提交粒度",
    `D-337:文本兜底多选的提交不对:${JSON.stringify(answerCalls().at(-1)?.args)}`,
  );
  await clearAsks();

  // ③ 非多选档位(默认)行为不变:点一个选项立即提交。
  askHandler?.({
    payload: {
      id: 703, sessionId: "sess-smoke", kind: "question",
      question: "单选问题", options: ["甲", "乙"], default: "",
    },
  });
  await flush();
  assert(!byId.get("ask-options").classList.contains("multi"), "D-337:默认档位误进了多选");
  const beforeSingle = answerCalls().length;
  [...byId.get("ask-options").querySelectorAll(".ask-option")][1].click();
  await flush();
  assert(answerCalls().length === beforeSingle + 1, "D-337:非多选档位点选项未立即提交");
  assert(answerCalls().at(-1)?.args?.reply === "乙", `D-337:非多选档位提交的不是所点选项:${JSON.stringify(answerCalls().at(-1)?.args)}`);
  await clearAsks();

  // ④ 多选档位空选空文本时提交按钮禁用(离开必须经由「取消」)。
  askHandler?.({
    payload: {
      id: 704, sessionId: "sess-smoke", kind: "question",
      question: "空选测试", options: ["唯一"], default: "", multiple: true,
    },
  });
  await flush();
  assert(byId.get("ask-submit").disabled === true, "D-337:多选空选空文本时提交按钮应禁用");
  byId.get("ask-cancel").click();
  await flush();
  assert(overlay.classList.contains("hidden"), "D-337:取消未关闭多选弹窗");
}

// ---------- R-190 常驻 fast 模型状态指示 ----------
// 状态栏 #status-fast 在托管且未就绪时显示缺环文案(桩:serviceUp=false);
// 且轮询函数已注册(fastStatusTimer 非空),说明常驻刷新不是一次性快照。
{
  const fastEl = byId.get("status-fast");
  assert(fastEl, "R-190:状态栏缺少 #status-fast 常驻指示位");
  await flush();
  // D-384:首跑是 03-shell 顶层触发的 async invoke,可能被后续断言的 flush 挤掉
  // (R-267 批2 消息窗口化新增的 await 改变了时序)。手动驱动一次确保已渲染——
  // 断言测的是「托管且未就绪 → 显示缺环文案」的映射,不是「首跑恰好抢到窗口」。
  await sandbox.refreshFastStatusBar?.();
  // 首次轮询已跑(启动即查),桩状态 serviceUp=false → 显示「服务未运行」并标 warn。
  assert(
    fastEl.textContent.includes("服务未运行") || fastEl.textContent.includes("service is not running"),
    `R-190:常驻指示未反映服务未运行,实得 "${fastEl.textContent}"`,
  );
  assert(fastEl.classList.contains("warn-text"), "R-190:未就绪时指示应标红(warn-text)");
  const timerRegistered = esmModuleCache.get("03-shell.js")?.namespace?.fastStatusTimer !== null;
  assert(timerRegistered, "R-190:常驻轮询定时器未注册(状态不会随真实探测更新)");
}

// ---------- R-179 深并行 UX:diff 接入既有渲染器 + 冲突预检 + 建线提示 ----------
{
  const linesSrc = await readFile(resolve(root, "crates", "kanzei-app", "ui", "20-lines.js"), "utf8");
  const sessionsSrc = await readFile(resolve(root, "crates", "kanzei-app", "ui", "09-sessions.js"), "utf8");
  // 验收①:线的 diff 用 06-activity.js 既有 buildDiffTree,不新写查看器。
  assert(
    linesSrc.includes('typeof buildDiffTree === "function" ? buildDiffTree(treeFiles)'),
    "R-179:线 diff 未接入既有 buildDiffTree 目录树渲染器",
  );
  assert(
    linesSrc.includes('rawSummary.textContent = t("原始差异文本")'),
    "R-179:原始 diff 未收进可折叠 details(目录树 + 原始文本并存)",
  );
  // 验收③:合并确认前调用 worktree_merge_preview 取冲突文件列表。
  assert(
    linesSrc.includes('await invoke("worktree_merge_preview"'),
    "R-179:合并确认未调用 worktree_merge_preview 冲突预检",
  );
  assert(
    linesSrc.includes('t("Git 合并冲突文件")'),
    "R-179:冲突文件列表未进入合并确认文案",
  );
  // 验收⑤:建线 UI 有磁盘/冷编译成本提示。
  assert(
    sessionsSrc.includes('t("每线独立 target/ 目录,磁盘占用随线路数成倍增加;首次冷编译需数分钟")'),
    "R-179:建线 UI 缺少磁盘/冷编译成本提示",
  );
  // 验收⑧:三档宽度下线路页不崩(列表容器仍在 DOM)。
  for (const width of [800, 1024, 1280]) {
    windowShim.innerWidth = width;
    await flush();
    assert(
      document.getElementById("lines-list"),
      `R-179:${width}px 下线路页列表容器缺失`,
    );
  }
  windowShim.innerWidth = 1280;
  await flush();
}

// ---------- R-187 提示音管理设置 ----------
// 设置页「提示音」区块控件存在;playRunNotice 读 localStorage 配置(总开关关掉
// 时不播放,音量可调)。
{
  const settingsView = document.getElementById("view-settings");
  if (settingsView) settingsView.classList.remove("hidden");
  await flush();
  assert(byId.get("set-sound-enabled"), "R-187:设置页缺少提示音总开关");
  assert(byId.get("set-sound-volume"), "R-187:设置页缺少音量滑杆");
  assert(byId.get("set-sound-completed"), "R-187:设置页缺少运行完成开关");
  assert(byId.get("set-sound-failed"), "R-187:设置页缺少运行失败开关");
  assert(byId.get("set-sound-stopped"), "R-187:设置页缺少运行已停止开关");
  assert(byId.get("sound-preview"), "R-187:设置页缺少试听按钮");
  // 默认配置:全部开启、音量 0.12。
  const defaultSound = vm.runInContext("readSoundSettings()", sandbox);
  assert(defaultSound.enabled && defaultSound.completed && defaultSound.failed && defaultSound.stopped,
    `R-187:默认提示音配置应为全开,实得 ${JSON.stringify(defaultSound)}`);
  assert(Math.abs(defaultSound.volume - 0.12) < 0.001, `R-187:默认音量应为 0.12,实得 ${defaultSound.volume}`);
  // 关闭总开关后 soundEnabledFor 对任何 kind 都返回 false(不播放)。
  const disabled = vm.runInContext('(function(){ saveSoundSettings({enabled:false, volume:0.12, completed:true, failed:true, stopped:true}); return soundEnabledFor("completed") && soundEnabledFor("failed"); })()', sandbox);
  assert(disabled === false, "R-187:总开关关闭后提示音不应播放");
  // 恢复默认,避免污染后续用例。
  vm.runInContext('saveSoundSettings({enabled:true, volume:0.12, completed:true, failed:true, stopped:true})', sandbox);
}

// ---------- R-188 架构图:代码生成的 SVG 依赖图 ----------
// 架构浏览页在文字树之外渲染依赖图 SVG;图数据为空时隐藏降级文字树。
{
  // 先切到架构视图触发 refreshArch。
  document.querySelector('.activity-item[data-view="arch"]')?.click();
  await flush();
  const graphHost = byId.get("arch-graph");
  assert(graphHost, "R-188:架构浏览页缺少 #arch-graph 图容器");
  const svg = graphHost.querySelector("svg.arch-svg");
  assert(svg, "R-188:架构图未渲染为 SVG(应代码生成,非文生图/预置图)");
  assert(
    svg.querySelectorAll("g.arch-node").length >= 6,
    `R-188:SVG 节点数不足(桩 graph 有 6 crate),实得 ${svg.querySelectorAll("g.arch-node").length}`,
  );
  assert(
    svg.querySelectorAll("line").length >= 6,
    `R-188:SVG 依赖边数不足(桩 graph 有 6 边),实得 ${svg.querySelectorAll("line").length}`,
  );
  // 图渲染不替换文字树(降级视图仍在)。
  assert(
    (byId.get("arch-tree")?.childNodes?.length ?? byId.get("arch-tree")?.childElementCount ?? 0) > 0,
    "R-188:架构图渲染后文字树被清空(降级视图必须保留)",
  );
  // 节点可点击定位(点击 app 节点应尝试打开 crate Cargo.toml)。
  const appNode = [...svg.querySelectorAll("g.arch-node")].find((g) => g.getAttribute("aria-label") === "kanzei-app");
  assert(appNode, "R-188:SVG 缺少 kanzei-app 节点");
  appNode.dispatchEvent({ type: "click", preventDefault() {}, stopPropagation() {} });
  await flush();
  assert(
    invokeLog.some((cmd) => cmd === "docs_read_custom"),
    "R-188:点击图节点未触发文档/Cargo 定位读取",
  );
}

// D-721:架构页记忆管理入口单击只能触发一次真实 memory 导航。
{
  const archMemoryEntry = document.querySelector('.activity-item[data-view="memory"]');
  const archGotoMemory = byId.get("arch-goto-memory");
  assert(archMemoryEntry && archGotoMemory, "D-721:记忆管理导航回归缺少入口夹具");
  let archMemoryNavigations = 0;
  archMemoryEntry.addEventListener("click", () => { archMemoryNavigations += 1; });
  archGotoMemory.click();
  await flush();
  assert(archMemoryNavigations === 1, `D-721:架构页单击应触发一次 memory 导航,实得 ${archMemoryNavigations}`);
  assert(byId.get("view-memory")?.classList.contains("active"), "D-721:架构页单击未到达 memory 视图");
  document.querySelector('.activity-item[data-view="arch"]')?.click();
  await flush();
}

// ---------- R-190 常驻 fast 模型状态指示 ----------
// 状态栏 #status-fast 在托管且未就绪时显示缺环文案(桩:serviceUp=false);
// 且轮询函数已注册(fastStatusTimer 非空),说明常驻刷新不是一次性快照。
{
  const fastEl = byId.get("status-fast");
  assert(fastEl, "R-190:状态栏缺少 #status-fast 常驻指示位");
  await flush();
  // D-384:手动驱动首跑(见上方同款注释),断言测映射而非首跑时序。
  await sandbox.refreshFastStatusBar?.();
  assert(
    fastEl.textContent.includes("服务未运行") || fastEl.textContent.includes("service is not running"),
    `R-190:常驻指示未反映服务未运行,实得 "${fastEl.textContent}"`,
  );
  assert(fastEl.classList.contains("warn-text"), "R-190:未就绪时指示应标红(warn-text)");
  const timerRegistered = esmModuleCache.get("03-shell.js")?.namespace?.fastStatusTimer !== null;
  assert(timerRegistered, "R-190:常驻轮询定时器未注册(状态不会随真实探测更新)");
}

// ---------- R-189 亮色主题:切换持久化 + Monaco setTheme 联动 ----------
{
  const themeBtn = byId.get("theme-toggle");
  assert(themeBtn, "R-189:侧栏缺少主题切换按钮");
  assert(!html.includes('id="theme-toggle" class="statusbar-btn"'), "D-348:主题按钮仍在状态栏");
  assert(html.indexOf('id="theme-toggle"') < html.indexOf('id="statusbar"'), "D-348:主题按钮未放入侧栏");
  assert(/\.msg\.assistant[^{]*\{[^}]*color:\s*var\(--fg-strong\)/.test(style), "D-348:正文未使用亮色主题前景 token");
  assert(/\.tool-display\.term[^{]*\{[^}]*color:\s*var\(--fg\)/.test(style), "D-348:运行输出未使用主题前景 token");
  assert(/#statusbar[^{]*\{[^}]*color:\s*var\(--statusbar-fg\)/.test(style), "D-351:状态栏未使用独立前景 token");
  assert(/#statusbar\.running[^{]*\{[^}]*color:\s*var\(--statusbar-run-fg\)/.test(style), "D-351:运行态状态栏未使用可读前景 token");
  // D-351 的可读性下限:断言**解析出来的字号数值**,不是源码里那串字面量。
  // D-380 把字号 1:1 token 化之后,原来的 /font-size:\s*15px/ 全部落空——而可读性
  // 一点没变。写死字面量的断言拦不住"改小了但换了写法",还会把纯重构判成回归;
  // 解析 token 再比数值,两头都稳。
  const fontTokens = Object.fromEntries(
    [...style.matchAll(/(--fs-[0-9-]+):\s*([0-9.]+)px/g)].map((m) => [m[1], Number(m[2])]),
  );
  // 选择器用正则字面量传,不用字符串再拼——字符串里的 \. \s 会被 JS 先吃掉一层,
  // 变成「任意字符」和字面 s(本次改造实测踩中,报"找不到 .tool-chips* 的字号声明")。
  const resolvedFontSize = (blockPattern) => {
    const block = style.match(blockPattern);
    if (!block) return null;
    const declared = block[1].match(/font(?:-size)?:\s*(?:var\((--fs-[0-9-]+)\)|([0-9.]+)px)/);
    if (!declared) return null;
    return declared[1] ? fontTokens[declared[1]] : Number(declared[2]);
  };
  for (const [pattern, floor, label] of [
    [/\.msg\.assistant[^{]*\{([^}]*)\}/, 15, "assistant 正文"],
    [/#log-lines[^{]*\{([^}]*)\}/, 13, "运行日志"],
    [/\.tool-chip\s*\{([^}]*)\}/, 13, "旧式工具卡片"],
    [/\.tool-msg-result[^{]*\{([^}]*)\}/, 13, "工具结果"],
    [/\.replay-tool-body[^{]*\{([^}]*)\}/, 13, "历史工具详情"],
  ]) {
    const size = resolvedFontSize(pattern);
    assert(size !== null, `D-351:找不到 ${label} 的字号声明(${pattern})`);
    assert(size >= floor, `D-351:${label}字号 ${size}px 低于可读下限 ${floor}px`);
  }
  assert(/\.tool-chip\.replay[^{]*\{[^}]*opacity:\s*1/.test(style), "D-351:历史工具块仍被整体淡化");
  assert(!/\.tool-msg-raw\.args[^{]*\{[^}]*opacity:\s*0?\.[0-9]+/.test(style), "D-351:工具入参仍被透明度二次淡化");
  // 默认暗色(现状零回归)。
  assert(document.documentElement.getAttribute("data-theme") !== "light", "R-189:默认主题应为暗色(或未设=暗)");
  // 切亮色:html[data-theme=light] + localStorage 持久化。
  themeBtn.click();
  await flush();
  assert(document.documentElement.getAttribute("data-theme") === "light", "R-189:点击切换后 data-theme 未变 light");
  assert(storage.get("kz-theme") === "light", "R-189:亮色主题未持久化到 localStorage");
  // 切回暗色,保持默认。
  themeBtn.click();
  await flush();
  assert(document.documentElement.getAttribute("data-theme") !== "light", "R-189:切回暗色失败");
  assert(storage.get("kz-theme") === "dark", "R-189:暗色未持久化");
  // Monaco setTheme 联动:17-files.js 创建编辑器时按当前主题选 vs/vs-dark。
  const filesSrc = await readFile(resolve(root, "crates", "kanzei-app", "ui", "17-files.js"), "utf8");
  assert(
    filesSrc.includes('currentTheme() === "light" ? "vs" : "vs-dark"'),
    "R-189:Monaco 编辑器主题未跟随全局主题",
  );
}

// ---------- D-355:切项目时 process_list 单飞跨项目错等导致目标对话不恢复 ----------
// 旧实现:processRefreshInFlight/Queued 是跨项目全局单飞,不携带请求所属项目。项目 A 的
// process_list 在途时切到 B,B 的 refreshProcesses 命中 A 的 inFlight 后返回 A 的 Promise,
// loadConversation 误等它,等到的却是「A 的列表完成」而 B 的 activeProcessId 仍是 null,
// 于是 B 的 conversation_get 永远不发出——切仓库后目标对话不恢复(空白/旧快照),数据其实
// 还在 SQLite 里。新实现按项目键控:返回的 Promise 恒为「本项目列表刷新完成」。
{
  const savedProcessList = payloads.process_list;
  const savedConversationGet = payloads.conversation_get;
  const A_PROC = "d|smoke";
  const B_PROC = "d|proj-b";
  const A_HISTORY = "冒烟历史消息";
  const B_HISTORY = "乙项目的历史消息";
  // 两个项目的进程列表与历史必须不同,才能判「以 B 的 projectDir/processId 调
  // conversation_get」而不是复用了 A 的结果。
  payloads.process_list = (args) =>
    args?.projectDir === PROJECT_B
      ? [{ id: B_PROC, label: "乙主会话", session_id: "sess-b", running: false, branch: "main", authority: "primary" }]
      : savedProcessList;
  payloads.conversation_get = (args) =>
    args?.projectDir === PROJECT_B
      ? [{ role: "user", parts: [{ type: "text", text: B_HISTORY }] }]
      : savedConversationGet;
  const projPayload = (path) => ({
    current: path,
    projects: [PROJECT, PROJECT_B],
    names: { [PROJECT]: "smoke", [PROJECT_B]: "smoke-b" },
  });
  const visibleHistory = () => [...document.querySelectorAll("#messages [data-active] .message-body")]
    .map((el) => el.textContent)
    .join("\n");

  // 前置:清掉前面用例 arm 的所有 autoContinue 定时器——它们残留的 send 回调会在 flush 里
  // 清空 messages/拨动会话状态,污染切项目/切线的 DOM 断言。清空注册表后定时器回调
  // 的 generation 检查直接 return,不再触发 send。
  vm.runInContext('autoContinueTimers.clear()', sandbox);
  // 再清掉全部挂起定时器:update_check/gitLiveTimer 等残留回调也可能在 flush 里清空
  // messages,导致 DOM 断言竞态失败(D-356 同款隔离)。
  for (const h of [...pendingTimers]) pendingTimers.delete(h);

  // ---------- ① 卡住项目 A 的 process_list 后切 B ----------
  // B 必须实际等待 B 自己的 process_list(旧实现会命中 A 的全局单飞直接复用),并以
  // B 的 projectDir/processId 调 conversation_get;迟到的 A 响应不得覆盖 B。
  await gotoProject(PROJECT, docsA);
  let releaseGate;
  invokeGates.set("process_list", new Promise((resolve) => { releaseGate = resolve; }));
  const aRefresh = sandbox.refreshProcesses(); // 项目 A 的请求在途,卡在闸门上
  await settle();
  assert(
    invokeArgs.at(-1)?.cmd === "process_list" && invokeArgs.at(-1)?.args?.projectDir === PROJECT,
    `前置失败:项目 A 的 process_list 没有在途(${JSON.stringify(invokeArgs.at(-1))})`,
  );
  invokeGates.delete("process_list"); // 只卡住上面那一次:已在 await 的调用握着自己那个 promise
  const processListBefore = invokeArgs.filter((entry) => entry.cmd === "process_list").length;
  payloads.docs_snapshot = docsB;
  payloads.projects_select = projPayload(PROJECT_B);
  const bSwitch = sandbox.selectWorkspaceProject(PROJECT_B); // 切到 B,等待 B 自己的 process_list
  await settle();
  const afterSwitchCalls = invokeArgs.filter((entry) => entry.cmd === "process_list");
  assert(
    afterSwitchCalls.length === processListBefore + 1
      && afterSwitchCalls.at(-1)?.args?.projectDir === PROJECT_B,
    `切到 B 没有实际发出 B 自己的 process_list(旧实现命中 A 的全局单飞直接返回 A 的 Promise):` +
      `${JSON.stringify(afterSwitchCalls)}`,
  );
  releaseGate(); // 放行:A 的响应落地(项目已切走,守卫丢弃),B 的响应落地 → activeProcessId 就绪
  await bSwitch;
  await flush();
  const getCalls = invokeArgs.filter((entry) => entry.cmd === "conversation_get");
  assert(
    getCalls.some((entry) => entry.args?.projectDir === PROJECT_B && entry.args?.processId === B_PROC),
    `conversation_get 没有以 B 的 projectDir/processId 发出(旧实现:B 的 activeProcessId 从未被填充,` +
      `conversation_get 永不触发,目标对话不恢复):${JSON.stringify(getCalls)}`,
  );
  assert(
    visibleHistory().includes(B_HISTORY),
    `切到 B 后主对话没有显示 B 的历史消息(目标对话不恢复,显示空白或旧上下文):"${visibleHistory()}"`,
  );
  assert(
    vm.runInContext("activeProcessId", sandbox) === B_PROC,
    `B 的活动进程不是 B 自己的(d355ClearActive 变异会把 A 的进程残留进来):${vm.runInContext("activeProcessId", sandbox)}`,
  );
  await aRefresh; // 让 A 的在途 promise 完全落地,确认它没有把 B 顶掉
  assert(
    vm.runInContext("activeProcessId", sandbox) === B_PROC,
    "A 的 process_list 在途响应落地后覆盖了 B 的活动进程(项目守卫缺失)",
  );

  // ---------- ② B 的 conversation_get 迟到落地不得覆盖已切回的目标(验收②) ----------
  // 这条同时是 d355LoadConvGuard 变异的判红点:删掉 loadConversation 的 isCurrent 守卫后,
  // B 的历史会被画进已切回 A 的消息区。
  await gotoProject(PROJECT, docsA);
  let releaseConv;
  invokeGates.set("conversation_get", new Promise((resolve) => { releaseConv = resolve; }));
  payloads.docs_snapshot = docsB;
  payloads.projects_select = projPayload(PROJECT_B);
  // R-267:pane 常驻之后,目标会话的 pane 若已有内容就**不会**再发 conversation_get
  // ——那正是本次改造要的效果。但本用例验的是「在途响应的项目守卫」,必须真的发出
  // 一次请求才有东西可迟到,所以先把 pane 全清掉,逼出装载路径。
  vm.runInContext(
    'for (const [id, pane] of [...messagePanes]) { pane.remove(); messagePanes.delete(id); }; activePane = paneFor(activeSessionId || "");',
    sandbox,
  );
  const bSwitchLate = sandbox.selectWorkspaceProject(PROJECT_B); // B 的 conversation_get 卡在闸门
  await settle();
  assert(
    invokeArgs.at(-1)?.cmd === "conversation_get" && invokeArgs.at(-1)?.args?.projectDir === PROJECT_B,
    `前置失败:B 的 conversation_get 没有在途(${JSON.stringify(invokeArgs.at(-1))})`,
  );
  invokeGates.delete("conversation_get"); // 只卡住 B 那一次:已在 await 的调用握着自己那个 promise
  // 切回 A 前必须把 projects_select 桩改回 A:桩是按命令返回固定值的,不改的话
  // selectWorkspaceProject(PROJECT) 拿到的还是 B 的 prefs,「切回 A」实际切回了 B,
  // bSwitchLate 的 isCurrent 会误判为当前目标,迟到历史照样覆盖(这是测试桩陷阱)。
  payloads.projects_select = projPayload(PROJECT);
  const aSwitchBack = sandbox.selectWorkspaceProject(PROJECT); // 切回 A,conversation_get 不再卡
  await aSwitchBack;
  await flush();
  assert(
    vm.runInContext("currentProject", sandbox) === PROJECT,
    `前置失败:切回 A 后 currentProject 不是 A(${vm.runInContext("currentProject", sandbox)})`,
  );
  assert(
    visibleHistory().includes(A_HISTORY),
    `前置失败:切回 A 后主对话没有显示 A 的历史("${visibleHistory()}")`,
  );
  releaseConv(); // B 的响应现在才落地:无守卫时它会覆盖 A 的历史
  await bSwitchLate;
  await flush();
  assert(
    visibleHistory().includes(A_HISTORY) && !visibleHistory().includes(B_HISTORY),
    `迟到的 B 历史覆盖了已切换目标的 A 历史(loadConversation 的 isCurrent 守卫缺失):"${visibleHistory()}"`,
  );

  // 收尾:还原桩与项目选择,回到项目 A 的干净状态。
  payloads.process_list = savedProcessList;
  payloads.conversation_get = savedConversationGet;
  delete payloads.projects_select;
  payloads.docs_snapshot = savedDocsPayload;
  await gotoProject(PROJECT, savedDocsPayload);
  await flush();
  assert(visibleHistory().includes(A_HISTORY), "D-355 收尾失败:项目 A 的历史没有恢复");
}

// ---------- R-267:每会话渲染面——后台会话的渲染不再丢失,切换不再重建 ----------
// 改造前(D-356)的形态:非活动会话的 kz:text/kz:tool-* 被整条丢弃,切走时把 DOM 存成
// innerHTML 字符串(sessionDomCache,上限 30 份),切回来塞回去 + 一句「快照截至上次
// 切走时,本轮完成后自动补齐」,轮末再由 kz:done 原子回灌。缺口、免责声明、每次切换
// 一次多 MB 的 innerHTML 解析,三样都是「全局唯一容器」逼出来的。
// 本组钉的是它的替代品:pane 常驻、后台事件进各自 pane、切换只换显示。
{
  vm.runInContext('autoContinueTimers.clear()', sandbox);
  for (const h of [...pendingTimers]) pendingTimers.delete(h);
  const savedR267ProcessList = payloads.process_list;
  payloads.process_list = [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: true, branch: "main", authority: "primary", stage: "实现" },
    { id: "p|bg", label: "后台会话", session_id: "sess-bg", running: true, worktree_path: "C:/smoke-wt", branch: "kanzei/thread-smoke", authority: "parallel" },
  ];
  await gotoProject(PROJECT, savedDocsPayload);
  await flush();
  vm.runInContext('transitionSession("sess-smoke", "running")', sandbox);
  await flush();

  // 切到线路 B。A 的 pane 必须**留在 DOM 里**——切回来才可能零重建。
  await sandbox.switchProcess("p|bg");
  await flush();
  assert(
    vm.runInContext("activeSessionId", sandbox) === "sess-bg",
    `前置失败:切到线路 B 失败(activeSessionId=${vm.runInContext("activeSessionId", sandbox)})`,
  );
  assert(
    vm.runInContext('messagePanes.has("sess-smoke")', sandbox),
    "切走后 sess-smoke 的 pane 不在了:切回来又得重建,R-267 的前提就不成立",
  );

  // 核心:B 活动时给 **A** 发渲染事件。改造前这条被整条丢弃(缺口的来源)。
  const BG_MARK = "后台渲染标记A";
  const paneText = (id) =>
    vm.runInContext(`(messagePanes.get(${JSON.stringify(id)})?.textContent) ?? ""`, sandbox);
  handlers.get("kz:text")({ payload: { sessionId: "sess-smoke", text: BG_MARK } });
  await flush();
  assert(
    paneText("sess-smoke").includes(BG_MARK),
    "后台会话的 kz:text 没有渲染进它自己的 pane——切回去就会缺这一段(R-267)",
  );
  assert(
    !paneText("sess-bg").includes(BG_MARK),
    "后台会话的渲染串进了活动会话的 pane:这正是改造前必须整条丢弃的原因",
  );

  // 全局 UI 不得被后台渲染带偏:状态栏归活动会话。
  const statusBefore = byId.get("status-text").textContent;
  handlers.get("kz:text")({ payload: { sessionId: "sess-smoke", text: "又一段后台文本" } });
  await flush();
  assert(
    byId.get("status-text").textContent === statusBefore,
    `后台会话的渲染改写了状态栏(${statusBefore} → ${byId.get("status-text").textContent}):全局 UI 只归活动会话`,
  );

  // 切回 A:不重拉 conversation_get(零重建),内容含切走期间到达的那段,且**没有**免责 notice。
  const convBefore = invokeArgs.filter((entry) => entry.cmd === "conversation_get").length;
  await sandbox.switchProcess("d|smoke");
  await flush();
  assert(
    invokeArgs.filter((entry) => entry.cmd === "conversation_get").length === convBefore,
    "切回运行中的线路 A 又拉了 conversation_get:pane 已有内容就不该重建",
  );
  const activeText = vm.runInContext("activePane.textContent", sandbox);
  assert(
    activeText.includes(BG_MARK),
    "切回线路 A 后,切走期间到达的后台渲染不见了(缺口又回来了)",
  );
  assert(
    !activeText.includes("快照截至") && !activeText.includes("snapshot as of"),
    "切回后仍在挂「快照截至上次切走时」——缺口已消失,这句免责声明不该再出现",
  );

  // kz:done 不再原子回灌:pane 已是完整的,回灌只会清掉轮末 notice 并与后续渲染交错。
  const getBeforeDone = invokeArgs.filter((entry) => entry.cmd === "conversation_get").length;
  await handlers.get("kz:done")({
    payload: { sessionId: "sess-smoke", steps: 1, halted: false, autoAction: { type: "NoContinue" } },
  });
  await flush();
  assert(
    invokeArgs.filter((entry) => entry.cmd === "conversation_get").length === getBeforeDone,
    "kz:done 仍在回灌 conversation_get:pane 已完整,回灌是多余的且会吞掉轮末 notice",
  );
  assert(
    vm.runInContext("activePane.textContent", sandbox).includes(BG_MARK),
    "kz:done 之后 pane 内容被冲掉了",
  );

  // ---------- D-728:并行会话的流式合帧各自落到所属 pane ----------
  // 两条事件在同一帧内交错到达:旧版全局 pending 槽会让 B 覆盖 A,导致 A 的末帧不落 DOM。
  const streamPaneA = vm.runInContext('messagePanes.get("sess-smoke")', sandbox);
  const streamPaneB = vm.runInContext('messagePanes.get("sess-bg")', sandbox);
  const STREAM_A = "D-728 流式会话 A 末帧";
  const STREAM_B = "D-728 流式会话 B 末帧";
  await handlers.get("kz:turn")({ payload: { sessionId: "sess-smoke" } });
  await handlers.get("kz:turn")({ payload: { sessionId: "sess-bg" } });
  await handlers.get("kz:text")({ payload: { sessionId: "sess-smoke", text: STREAM_A } });
  await handlers.get("kz:text")({ payload: { sessionId: "sess-bg", text: STREAM_B } });
  await flush();
  assert(streamPaneA?.textContent.includes(STREAM_A), "并流时会话 A 的末帧被另一会话覆盖");
  assert(streamPaneB?.textContent.includes(STREAM_B), "并流时会话 B 的末帧没有渲染");

  // ---------- R-267 批2:长会话只渲染尾部一窗,向上补齐 ----------
  // 不窗口化的话,批1 省下的重渲染会换成常驻内存(pane 常驻 × 993 条消息),
  // 属于拆东墙补西墙。这里钉住「首屏只渲染一窗 + 补齐能拿到更早的」。
  {
    const LONG = 400;
    const savedConv = payloads.conversation_get;
    payloads.conversation_get = Array.from({ length: LONG }, (_, i) => ({
      role: i % 2 === 0 ? "user" : "assistant",
      parts: [{ type: "text", text: `窗口化消息${i}` }],
    }));
    // 清掉 pane 逼出装载路径(pane 已有内容时按设计不重建)。
    vm.runInContext(
      'for (const [id, pane] of [...messagePanes]) { pane.remove(); messagePanes.delete(id); }; activePane = paneFor(activeSessionId || "");',
      sandbox,
    );
    await sandbox.loadConversation();
    await flush();
    const rendered = () => document.querySelectorAll("#messages [data-active] .msg").length;
    const firstScreen = rendered();
    assert(
      firstScreen > 0 && firstScreen < LONG,
      `首屏应只渲染尾部一窗,实得 ${firstScreen}/${LONG}——不窗口化则长会话每次切线都全量重建`,
    );
    const paneText = vm.runInContext("activePane.textContent", sandbox);
    assert(
      paneText.includes(`窗口化消息${LONG - 1}`),
      "首屏没有渲染**最新**那条:窗口取的应是尾部,不是头部",
    );
    assert(
      !paneText.includes("窗口化消息0"),
      "首屏把最早的消息也渲染了:窗口没生效",
    );
    assert(
      vm.runInContext('!!activePane.querySelector(".earlier-hint")', sandbox),
      "有未渲染的历史却没有「载入更早的消息」入口:内容被静默藏起来了",
    );
    // 向上补齐一窗:更早的消息进来,且不重复。
    const grew = vm.runInContext("loadEarlierMessages()", sandbox);
    await flush();
    assert(grew === true, "loadEarlierMessages 没有补齐(还有未渲染的历史)");
    assert(rendered() > firstScreen, `补齐后渲染条数没增加(${firstScreen} → ${rendered()})`);
    payloads.conversation_get = savedConv;
  }

  await handlers.get("kz:idle")({ payload: { reason: "completed", sessionId: "sess-smoke" } });
  await flush();
  vm.runInContext('transitionSession("sess-smoke", "idle")', sandbox);
  await flush();
  payloads.process_list = savedR267ProcessList;
  await gotoProject(PROJECT, savedDocsPayload);
  await flush();
}

// ---------- D-381 事件名两侧对齐 ----------
// kz:* 的名字目前在三处各存一份:Rust 侧 window.emit、前端 on() 订阅、01-core 的
// SESSION_PROGRESS_EVENTS/SESSIONLESS_EVENTS 集合。手工同步三份的结果是「发了没人听」
// 和「听了没人发」都不产生任何信号——本仓就有过一个 kz:annotate-progress 用裸 listen
// 绕开 on()、因而绕开「没有 sessionId 就丢弃」纪律的实例。
{
  const rustFiles = [];
  const walk = async (dir) => {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const full = resolve(dir, entry.name);
      if (entry.isDirectory()) await walk(full);
      else if (entry.name.endsWith(".rs")) rustFiles.push(full);
    }
  };
  await walk(resolve(root, "crates/kanzei-app/src"));
  const emitted = new Set();
  for (const file of rustFiles) {
    const text = await readFile(file, "utf8");
    for (const match of text.matchAll(/"(kz:[a-z-]+)"/g)) emitted.add(match[1]);
    // R-284:run event sink emits the shared structured name through the re-exported
    // core constant; resolve that indirection instead of reporting a false D-381 gap.
    if (text.includes("EXPERIENCE_EVENT_NAME")) emitted.add("kz:experience");
  }
  const subscribed = new Set([...source.matchAll(/\bon\("(kz:[a-z-]+)"/g)].map((m) => m[1]));
  const rawListen = [...source.matchAll(/(?<!function )\blisten\("(kz:[a-z-]+)"/g)].map((m) => m[1]);
  const unheard = [...emitted].filter((name) => !subscribed.has(name)).sort();
  const unsent = [...subscribed].filter((name) => !emitted.has(name)).sort();
  if (unheard.length) issues.push(`D-381:后端发了但前端没有 on() 订阅:${unheard.join(", ")}`);
  if (unsent.length) issues.push(`D-381:前端订阅了但后端从不发送:${unsent.join(", ")}`);
  if (rawListen.length) {
    issues.push(
      `D-381:裸 listen() 绕过 on() 的 sessionId 纪律:${rawListen.join(", ")}——` +
        `无 session 归属的事件请登记进 SESSIONLESS_EVENTS 后走 on()`,
    );
  }
}

// D-716/R-245 B7:重放真实删除弹窗的安全整理失败路径。第一次 cleanup 返回可见错误，
// 之后通过生产错误面板的 retry 钩子成功；retry 不得再次发起 conversation_delete。
{
  const deleteConversation = esmModuleCache.get("15-views-misc.js")?.namespace?.deleteConversationsForProcess;
  const shell = esmModuleCache.get("03-shell.js")?.namespace;
  assert(typeof deleteConversation === "function", "D-716 删除入口未注册为真实 UI 消费方");
  assert(shell && typeof shell.errorRetry === "function", "D-716 错误面板 retry 入口未注册");
  // UI-0926 #1:运行中的线拒绝删除(前后端双拦截)。这里验的是安全整理重试,不是运行态,
  // 先让 p|bg 收敛为空闲再删,结束后恢复原相位。
  const savedBgPhase = vm.runInContext('sessionState("sess-bg").phase', sandbox);
  vm.runInContext('transitionSession("sess-bg", "idle")', sandbox);
  const before = invokeArgs.length;
  let cleanupCalls = 0;
  payloads.conversation_delete = () => ({ deleted: 1, redacted_inputs: 0, segments: 1, cleared_current: false });
  payloads.conversation_cleanup = () => {
    cleanupCalls += 1;
    return cleanupCalls === 1
      ? { actual_freed_bytes: 0, artifact_cleanup_errors: ["artifact busy"], backup_cleanup_errors: [] }
      : { actual_freed_bytes: 12, artifact_cleanup_errors: [], backup_cleanup_errors: [] };
  };
  sandbox.currentProject = PROJECT;
  sandbox.confirmDialog = () => Promise.resolve("safe");
  expectedPersistentError = "Safe cleanup partly failed";
  await deleteConversation("p|bg", [1]);
  const firstCalls = invokeArgs.slice(before).map(({ cmd }) => cmd);
  assert(firstCalls.filter((cmd) => cmd === "conversation_delete").length === 1, "D-716 首次删除未调用一次");
  assert(firstCalls.filter((cmd) => cmd === "conversation_cleanup").length === 1, "D-716 首次安全整理未调用一次");
  assert(expectedPersistentHits > 0, "D-716 安全整理错误未进入持久错误面板");
  await shell.errorRetry();
  const retriedCalls = invokeArgs.slice(before).map(({ cmd }) => cmd);
  assert(retriedCalls.filter((cmd) => cmd === "conversation_delete").length === 1, "D-716 cleanup retry 重复删除会话");
  assert(retriedCalls.filter((cmd) => cmd === "conversation_cleanup").length === 2, "D-716 cleanup retry 未只重试安全整理");
  expectedPersistentError = null;
  vm.runInContext(`transitionSession("sess-bg", ${JSON.stringify(savedBgPhase)})`, sandbox);
}

// ---------- D-381 IPC 形状契约:fixture 必须与后端真实形状一致 ----------
// 这是全仓唯一一条「两侧都改对了才对、但没人检查」的缝:93 个 tauri command 里 30+ 个
// 手搓 JSON 过 IPC,而上面那些 payloads 是**前端自己写的**。后端改一个字段名,
// cargo test 全绿、六条前端冒烟全绿、真实界面碎。契约文件由 kanzei-app 的
// ipc_contract 测试拿真实命令跑出来,两侧共读同一份。
{
  const contract = JSON.parse(await readFile(resolve(root, "scripts/ipc-contract.json"), "utf8"));
  const shapeOf = (value) => {
    if (Array.isArray(value)) return value.length ? [shapeOf(value[0])] : "array";
    if (value === null || value === undefined) return "nullable";
    if (typeof value === "object") {
      return Object.fromEntries(Object.keys(value).sort().map((k) => [k, shapeOf(value[k])]));
    }
    return { string: "string", number: "number", boolean: "bool" }[typeof value] ?? typeof value;
  };
  // 后端取样到 null 的 Option 字段记 "nullable";fixture 给了具体值同样合法,反之亦然。
  // 除此之外必须逐键逐类型相等。
  const compatible = (expected, actual, path, problems) => {
    if (expected === "nullable" || actual === "nullable") return;
    const expectedIsList = Array.isArray(expected) || expected === "array";
    const actualIsList = Array.isArray(actual) || actual === "array";
    if (expectedIsList || actualIsList) {
      if (!expectedIsList || !actualIsList) {
        problems.push(`${path}: 契约 ${JSON.stringify(expected)} vs fixture ${JSON.stringify(actual)}`);
      } else if (Array.isArray(expected) && Array.isArray(actual)) {
        compatible(expected[0], actual[0], `${path}[]`, problems);
      }
      return;
    }
    if (typeof expected === "object" && typeof actual === "object") {
      for (const key of new Set([...Object.keys(expected), ...Object.keys(actual)])) {
        if (!(key in actual)) { problems.push(`${path}.${key}:后端会发,fixture 里没有(界面读它就是 undefined)`); continue; }
        if (!(key in expected)) { problems.push(`${path}.${key}:fixture 独有,后端不发(测试在验一个不存在的字段)`); continue; }
        compatible(expected[key], actual[key], `${path}.${key}`, problems);
      }
      return;
    }
    if (expected !== actual) problems.push(`${path}: 契约 ${JSON.stringify(expected)} vs fixture ${JSON.stringify(actual)}`);
  };
  const problems = [];
  compatible(contract.docs_snapshot, shapeOf(payloads.docs_snapshot), "docs_snapshot", problems);
  for (const problem of problems) issues.push(`D-381 IPC 契约:${problem}`);
}


// ===== 分区:会话生命周期 =====
// ---------- UI-0926 #2:新对话一次到位 ----------
// 用户现场:新对话要点好几次,旧对话残留在「已开启新对话」上方。主因是新对话只清 DOM、
// 不清窗口化历史缓存 paneHistory:清空后 pane 变短、scrollTop 被夹到 0,滚动监听当成
// 「触顶」自动 loadEarlierMessages,把旧对话一窗一窗补回提示上方。放大因素四处:
// 在途装载迟到、后台 pane 默认可见、忙碌时按钮被禁用吞掉点击、兜底改选不换 pane。
// 场景 A-F 各钉一处,各带一个 KZ_SMOKE_MUTATE 变异守卫(newChatForgetHistory / newChatEpoch /
// bgPaneHidden / newChatBusyNewLine / fallbackPaneSwitch / newChatRefusedNewLine)。复核补:
// 场景 A2(clear 成功后才撤续跑,newChatCancelAfterClear)、D0(按钮 title 与点击分流同一判据、
// 相位一变就跟上,newChatTitleSharedBusy / newChatTitleFollowsPhase)。
{
  vm.runInContext("__kzAutoTestState.cancelTimers(); autoContinueTimers.clear()", sandbox);
  const savedNcProcessList = payloads.process_list;
  const savedNcConversationGet = payloads.conversation_get;
  const hadNcProcessCreate = Object.hasOwn(payloads, "process_create");
  const savedNcProcessCreate = payloads.process_create;
  const prompt = byId.get("prompt");
  let promptFocuses = 0;
  prompt.focus = () => { promptFocuses += 1; };
  const OLD = Array.from({ length: 300 }, (_, i) => ({
    role: i % 2 === 0 ? "user" : "assistant",
    parts: [{ type: "text", text: `R2旧对话${i}` }],
  }));
  const MAIN_LINE = { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, branch: "main", authority: "primary" };
  const NC_LINE = { id: "p20|smoke", label: "p20", session_id: "sess-nc", running: false, authority: "parallel" };
  const NEW_LINE = { id: "p26|smoke", label: "p26", session_id: "sess-new", running: false, authority: "parallel", profile: "dev" };
  payloads.process_list = [MAIN_LINE, NC_LINE];
  payloads.conversation_get = ({ processId } = {}) => (processId === NEW_LINE.id ? [] : OLD);
  await gotoProject(PROJECT, savedDocsPayload);
  await sandbox.switchProcess(NC_LINE.id);
  await flush();
  // 逼出真装载:pane 已有内容时按设计不重拉。
  const dropAllPanes = () => vm.runInContext(
    'for (const [id, pane] of [...messagePanes]) { pane.remove(); messagePanes.delete(id); }; activePane = paneFor(activeSessionId || "");',
    sandbox,
  );
  dropAllPanes();
  await sandbox.loadConversation();
  await flush();
  const paneNow = () => vm.runInContext("activePane.textContent", sandbox);
  assert(
    vm.runInContext("activeSessionId", sandbox) === "sess-nc" && paneNow().includes("R2旧对话299") && !paneNow().includes("R2旧对话0"),
    `前置失败:p20 的长对话没有按窗口化装载(activeSessionId=${vm.runInContext("activeSessionId", sandbox)})`,
  );

  const messagesEl = byId.get("messages");
  // 冒烟测不到真实浏览器「清空后把 scrollTop 夹到 0 并派发 scroll」,这里手工补上,
  // 再加一个滚轮手势作废程序滚动窗口——等价于用户在空视图上往上滚了一下。
  const touchTop = async () => {
    messagesEl.scrollTop = 0;
    messagesEl.dispatchEvent({ type: "wheel" });
    messagesEl.dispatchEvent({ type: "scroll" });
    await flush();
  };
  const clearCalls = () => invokeArgs.filter((entry) => entry.cmd === "conversation_clear");
  const freshView = (label) => {
    const text = paneNow();
    assert(!text.includes("R2旧对话"), `${label}:新对话视图里冒出了旧对话内容(触顶补齐/迟到装载把旧段补了回来)`);
    assert(!vm.runInContext('!!activePane.querySelector(".earlier-hint")', sandbox), `${label}:新对话视图还挂着「载入更早的消息」入口`);
    assert(text.includes(sandbox.t("开始一段新对话")), `${label}:新对话没有给出与空历史同构的欢迎页`);
    assert(!text.includes(sandbox.t("已开启新对话(历史保留可审计)")), `${label}:新对话仍往转录区插旧式 notice`);
  };

  // ---- 场景 A:空闲线点一次就干净,触顶不再补回旧窗 ----
  sandbox.setRunning(false, "空闲");
  vm.runInContext('transitionSession("sess-nc", "idle")', sandbox);
  const clearsBeforeA = clearCalls().length;
  const focusBeforeA = promptFocuses;
  byId.get("new-chat").click();
  await flush();
  await touchTop();
  assert(
    clearCalls().length === clearsBeforeA + 1 && clearCalls().at(-1)?.args?.processId === NC_LINE.id,
    `场景 A:点一次新对话应对 p20 发恰好一次 conversation_clear(${JSON.stringify(clearCalls().slice(clearsBeforeA))})`,
  );
  freshView("场景 A");
  assert(promptFocuses > focusBeforeA, "场景 A:新对话之后输入框没有拿到焦点");
  assert(listText("toast").includes(sandbox.t("已开启新对话 · 之前的对话在侧栏「历史对话」里")), `场景 A:新对话没有 toast 告知旧对话去了哪里(${listText("toast")})`);
  assert(!byId.get("new-chat").disabled && byId.get("new-chat").getAttribute("aria-busy") !== "true", "场景 A:新对话结束后按钮仍处于禁用/忙碌态");
  // 幂等:再点一次仍是干净的欢迎页。
  byId.get("new-chat").click();
  await flush();
  await touchTop();
  freshView("场景 A 再点一次");

  // ---- 场景 A2:续跑只在新段建成后才撤;clear 因其它原因失败时鞭挞照旧 ----
  // 空闲判定已排除排着的续跑;clear 在途时轮末事件新排的那一枪,要在新段建成后撤掉
  // (否则它带着「继续」落进新段)。clear 失败(打不开库、IO)时什么都不撤,只报错。
  const armNcTimer = () => vm.runInContext('autoContinueTimers.set("sess-nc", { timer: setTimeout(() => {}, 600000) })', sandbox);
  const ncTimerArmed = () => vm.runInContext('autoContinueTimers.has("sess-nc")', sandbox);
  let releaseClear;
  invokeGates.set("conversation_clear", new Promise((resolve) => { releaseClear = resolve; }));
  const clearsBeforeA2 = clearCalls().length;
  byId.get("new-chat").click();
  await settle();
  invokeGates.delete("conversation_clear");
  assert(clearCalls().length === clearsBeforeA2 + 1, "场景 A2 前置失败:空闲线点新对话没有发出 conversation_clear");
  armNcTimer();
  releaseClear();
  await flush();
  assert(!ncTimerArmed(), "场景 A2:clear 在途时新排的续跑在新段建成后没撤——那一枪会带着「继续」落进新段");
  freshView("场景 A2");
  invokeGates.set("conversation_clear", new Promise((resolve) => { releaseClear = resolve; }));
  invokeFailures.set("conversation_clear", "R2打不开数据库");
  expectedPersistentError = "R2打不开数据库";
  const persistentBeforeA2 = expectedPersistentHits;
  byId.get("new-chat").click();
  await settle();
  invokeGates.delete("conversation_clear");
  armNcTimer();
  releaseClear();
  await flush();
  invokeFailures.delete("conversation_clear");
  expectedPersistentError = null;
  assert(expectedPersistentHits === persistentBeforeA2 + 1, "场景 A2 前置失败:clear 失败没有走持久错误出口");
  assert(ncTimerArmed(), "场景 A2:clear 失败时把排着的续跑也撤了——开着鞭挞的线会静默停摆");
  vm.runInContext("__kzAutoTestState.cancelTimers(); autoContinueTimers.clear()", sandbox);
  assert(!byId.get("new-chat").disabled && byId.get("new-chat").getAttribute("aria-busy") !== "true", "场景 A2:失败后按钮仍处于禁用/忙碌态");

  // ---- 场景 B:新对话作废在途的旧段装载 ----
  dropAllPanes();
  let releaseLoad;
  invokeGates.set("conversation_get", new Promise((resolve) => { releaseLoad = resolve; }));
  const getsBeforeB = invokeArgs.filter((entry) => entry.cmd === "conversation_get").length;
  const inflightLoad = sandbox.loadConversation();
  await settle();
  assert(
    invokeArgs.filter((entry) => entry.cmd === "conversation_get").length === getsBeforeB + 1,
    "场景 B 前置失败:旧段的 conversation_get 没有在途",
  );
  invokeGates.delete("conversation_get"); // 只卡住在途那一次
  byId.get("new-chat").click();
  await flush();
  releaseLoad(); // 旧段结果现在才落地:无纪元守卫时它会整页画回新对话
  await inflightLoad;
  await flush();
  freshView("场景 B");

  // ---- 场景 C:后台线首次渲染建的 pane 默认隐藏,不串进当前视图 ----
  const BG_TEXT = "R2自主推进线输出";
  handlers.get("kz:text")({ payload: { sessionId: "sess-autoloop", text: BG_TEXT } });
  await flush();
  const onlyActiveVisible = () => vm.runInContext(
    '(() => { const shown = [...messages.children].filter((el) => el.classList.contains("msg-pane") && !el.classList.contains("hidden")); return shown.length === 1 && shown[0] === activePane; })()',
    sandbox,
  );
  assert(
    vm.runInContext('messagePanes.get("sess-autoloop")?.classList.contains("hidden") === true', sandbox),
    "场景 C:后台线第一次渲染新建的 pane 是可见的——它的输出会叠进用户眼前的视图",
  );
  assert(onlyActiveVisible(), "场景 C:#messages 下可见的 pane 不止活动那一个");
  assert(!paneNow().includes(BG_TEXT), "场景 C:后台线的输出串进了当前视图");
  assert(
    vm.runInContext('messagePanes.get("sess-autoloop")?.textContent ?? ""', sandbox).includes(BG_TEXT),
    "场景 C:后台线的输出丢了(应渲染进它自己的隐藏 pane)",
  );

  // ---- 场景 D0:按钮 title 与点击分流同一判据,相位一变就跟上 ----
  // running/runControlPending 都为假、但活动线在启动中或鞭挞轮间等待:点下去会另开线路,
  // title 必须已经这么说;回到空闲再说回空闲文案。
  sandbox.setRunning(false, "空闲");
  const BUSY_TITLE = sandbox.t("当前线路运行中:点击将另开一条线路开启新对话");
  for (const phase of ["starting", "auto_pending"]) {
    vm.runInContext(`transitionSession("sess-nc", ${JSON.stringify(phase)})`, sandbox);
    assert(
      byId.get("new-chat").title === BUSY_TITLE,
      `场景 D0:活动线 ${phase} 时 title 仍是空闲文案(${byId.get("new-chat").title}),点下去却会另开线路`,
    );
  }
  vm.runInContext('transitionSession("sess-nc", "idle")', sandbox);
  assert(
    byId.get("new-chat").title === sandbox.t("开一段新对话(旧对话保留在「历史对话」)"),
    `场景 D0:回到空闲后 title 没有说回空闲文案(${byId.get("new-chat").title})`,
  );

  // ---- 场景 D:运行中的线点新对话 → 另开无工作树线路,原线在自己的 pane 里继续跑 ----
  let createdNewLine = false;
  payloads.process_create = () => { createdNewLine = true; return NEW_LINE; };
  payloads.process_list = () => [MAIN_LINE, { ...NC_LINE, running: true }, ...(createdNewLine ? [NEW_LINE] : [])];
  vm.runInContext('transitionSession("sess-nc", "running")', sandbox);
  sandbox.setRunning(true, "运行中");
  assert(!byId.get("new-chat").disabled, "场景 D:运行中新对话按钮仍被禁用——点击会被浏览器静默吞掉");
  // 语言重应用(data-i18n-title)不得把忙碌说明冲回静态的空闲文案。假 DOM 不把 title
  // property 反射成属性,这里手工补上浏览器的反射,applyDataI18nKeys 才比得出差异。
  byId.get("new-chat").setAttribute("title", byId.get("new-chat").title);
  sandbox.applyLanguage();
  assert(
    byId.get("new-chat").title === sandbox.t("当前线路运行中:点击将另开一条线路开启新对话"),
    `场景 D:运行中按钮 title 没说清点下去会另开线路(${byId.get("new-chat").title})`,
  );
  const clearsBeforeD = clearCalls().length;
  const createsBeforeD = invokeArgs.filter((entry) => entry.cmd === "process_create").length;
  const focusBeforeD = promptFocuses;
  byId.get("new-chat").click();
  await flush();
  const createCalls = invokeArgs.filter((entry) => entry.cmd === "process_create");
  assert(
    createCalls.length === createsBeforeD + 1 && createCalls.at(-1)?.args?.profile === "dev" && !createCalls.at(-1)?.args?.worktreeName,
    `场景 D:运行中点新对话应另开一条无工作树的 dev 线路(${JSON.stringify(createCalls.slice(createsBeforeD))})`,
  );
  assert(clearCalls().length === clearsBeforeD, "场景 D:运行中的线被 conversation_clear 了——不能在 runner 脚下开新段");
  assert(
    vm.runInContext("activeProcessId", sandbox) === NEW_LINE.id,
    `场景 D:没有切到新开的线路(activeProcessId=${vm.runInContext("activeProcessId", sandbox)})`,
  );
  freshView("场景 D");
  assert(promptFocuses > focusBeforeD, "场景 D:另开线路后输入框没有拿到焦点");
  const RUN_TEXT = "R2运行线继续输出";
  handlers.get("kz:text")({ payload: { sessionId: "sess-nc", text: RUN_TEXT } });
  await flush();
  assert(!paneNow().includes(RUN_TEXT), "场景 D:原线继续运行的输出串进了新线的视图");
  assert(
    vm.runInContext('messagePanes.get("sess-nc")?.classList.contains("hidden") === true', sandbox)
      && vm.runInContext('messagePanes.get("sess-nc")?.textContent ?? ""', sandbox).includes(RUN_TEXT),
    "场景 D:原线的输出没有留在它自己的隐藏 pane 里(切回去会缺这一段)",
  );
  assert(onlyActiveVisible(), "场景 D:另开线路后可见的 pane 不止一个");

  // ---- 场景 E:活动线消失,兜底改选时视图跟着换 ----
  payloads.process_list = [MAIN_LINE, NC_LINE];
  await sandbox.refreshProcesses();
  await flush();
  assert(
    vm.runInContext("activeProcessId", sandbox) !== NEW_LINE.id,
    "场景 E 前置失败:活动线从列表消失后没有兜底改选",
  );
  assert(
    vm.runInContext("activePane === messagePanes.get(activeSessionId)", sandbox),
    `场景 E:兜底改选到 ${vm.runInContext("activeSessionId", sandbox)} 后视图还停在旧线的 pane 上`,
  );
  assert(onlyActiveVisible(), "场景 E:兜底改选后可见的 pane 不止一个");

  // ---- 场景 F:前端以为空闲、后端持锁判定在跑(kz:turn 还没到)→ 同一次点击改走另开线路 ----
  // 不该让用户先吃一个报错再点第二次;若这里走了 toastError,harness 的持久错误探针也会判红。
  createdNewLine = false;
  payloads.process_list = () => [MAIN_LINE, NC_LINE, ...(createdNewLine ? [NEW_LINE] : [])];
  invokeFailures.set("conversation_clear", "会话运行中,不能在它脚下开新段;请在新线路开启新对话");
  const clearsBeforeF = clearCalls().length;
  const createsBeforeF = invokeArgs.filter((entry) => entry.cmd === "process_create").length;
  byId.get("new-chat").click();
  await flush();
  invokeFailures.delete("conversation_clear");
  assert(clearCalls().length === clearsBeforeF + 1, "场景 F 前置失败:空闲判定下没有先尝试 conversation_clear");
  assert(
    invokeArgs.filter((entry) => entry.cmd === "process_create").length === createsBeforeF + 1
      && vm.runInContext("activeProcessId", sandbox) === NEW_LINE.id,
    "场景 F:后端拒绝(会话运行中)后没有在同一次点击里改走另开线路",
  );
  freshView("场景 F");

  // 收尾:还原桩、焦点、运行态与续跑定时器,回到项目 A 的干净状态。
  delete prompt.focus;
  sandbox.setRunning(false, "空闲");
  for (const sessionId of ["sess-nc", "sess-new", "sess-autoloop"]) {
    vm.runInContext(`transitionSession(${JSON.stringify(sessionId)}, "idle")`, sandbox);
  }
  vm.runInContext('discardSessionPane("sess-autoloop"); __kzAutoTestState.cancelTimers(); autoContinueTimers.clear()', sandbox);
  payloads.process_list = savedNcProcessList;
  payloads.conversation_get = savedNcConversationGet;
  if (hadNcProcessCreate) payloads.process_create = savedNcProcessCreate;
  else delete payloads.process_create;
  await gotoProject(PROJECT, savedDocsPayload);
  await flush();
}

// ---------- UI-0926 #1:删除历史对话分层 ----------
// 删掉当前段后主区、窗口化缓存与续跑都得清:否则被删对话留在屏幕上、触顶从缓存补回、
// 在途装载迟到画回、排上的续跑那一轮落进空段。删旧段时主区可能正显示着那段历史,要按
// 当前段重载;后台线的 pane 直接作废;运行中的线前端先挡(后端持锁再挡);删除与关闭两处
// 弹窗如实说明删什么、留什么。场景 G-L,变异守卫 deleteRunningGuard / deleteClearedFresh /
// deleteCancelTimer / deleteEpoch / deleteReloadPane / deleteDiscardBgPane / deleteRefusedToast。
{
  vm.runInContext("__kzAutoTestState.cancelTimers(); autoContinueTimers.clear()", sandbox);
  const saved = {
    process_list: payloads.process_list,
    conversation_get: payloads.conversation_get,
    conversation_list: payloads.conversation_list,
    conversation_delete: payloads.conversation_delete,
    confirmDialog: sandbox.confirmDialog,
  };
  const deleteConversation = esmModuleCache.get("15-views-misc.js")?.namespace?.deleteConversationsForProcess;
  assert(typeof deleteConversation === "function", "UI-0926 #1:删除入口未注册");
  const MAIN = { id: "d|del", label: "主会话", session_id: "sess-del-main", running: false, branch: "main", authority: "primary" };
  const LINE = { id: "p31|del", label: "p31", session_id: "sess-del", running: false, authority: "parallel" };
  const DELETED = Array.from({ length: 300 }, (_, i) => ({
    role: i % 2 === 0 ? "user" : "assistant",
    parts: [{ type: "text", text: `R1被删对话${i}` }],
  }));
  const CURRENT = [{ role: "user", parts: [{ type: "text", text: "R1当前段" }] }];
  let currentDeleted = false;
  payloads.process_list = [MAIN, LINE];
  payloads.conversation_list = () => [];
  // sequence 非空 = 打开某段历史;为空 = 当前段(删掉前就是那段被删的长对话)。
  payloads.conversation_get = ({ processId, sequence } = {}) => {
    if (processId !== LINE.id) return [{ role: "user", parts: [{ type: "text", text: "R1主线内容" }] }];
    if (sequence != null) return DELETED;
    return currentDeleted ? [] : DELETED;
  };
  let confirmCalls = 0;
  let confirmOptions = null;
  sandbox.confirmDialog = (options) => {
    confirmCalls += 1;
    confirmOptions = options;
    return Promise.resolve(true);
  };
  await gotoProject(PROJECT, savedDocsPayload);
  await sandbox.switchProcess(LINE.id);
  await flush();
  vm.runInContext(
    'for (const [id, pane] of [...messagePanes]) { pane.remove(); messagePanes.delete(id); }; activePane = paneFor(activeSessionId || "");',
    sandbox,
  );
  await sandbox.loadConversation();
  await flush();
  const paneNow = () => vm.runInContext("activePane.textContent", sandbox);
  const calls = (cmd) => invokeArgs.filter((entry) => entry.cmd === cmd);
  const messagesEl = byId.get("messages");
  const touchTop = async () => {
    messagesEl.scrollTop = 0;
    messagesEl.dispatchEvent({ type: "wheel" });
    messagesEl.dispatchEvent({ type: "scroll" });
    await flush();
  };
  assert(
    vm.runInContext("activeSessionId", sandbox) === LINE.session_id && paneNow().includes("R1被删对话299"),
    "UI-0926 #1 前置失败:p31 的当前段没有装进主区",
  );

  // ---- 场景 G:运行中的线点删除 → 前端直接挡下,不弹确认、不发 conversation_delete ----
  vm.runInContext(`transitionSession(${JSON.stringify(LINE.session_id)}, "running")`, sandbox);
  const deletesBeforeG = calls("conversation_delete").length;
  const confirmsBeforeG = confirmCalls;
  await deleteConversation(LINE.id, [42]);
  await flush();
  vm.runInContext(`transitionSession(${JSON.stringify(LINE.session_id)}, "idle")`, sandbox);
  assert(calls("conversation_delete").length === deletesBeforeG, "场景 G:运行中的线路点删除仍发出了 conversation_delete");
  assert(confirmCalls === confirmsBeforeG, "场景 G:运行中应直接拦下,不该先弹删除确认框");
  assert(
    listText("toast").includes(sandbox.t("运行中请先完成或停止当前任务，再删除历史对话")),
    `场景 G:运行中拦截没有给出提示(${listText("toast")})`,
  );

  // ---- 场景 H:删掉活动线的当前段 → 欢迎页、缓存清空、续跑撤掉、在途装载作废 ----
  vm.runInContext(
    `autoContinueTimers.set(${JSON.stringify(LINE.session_id)}, { timer: setTimeout(() => {}, 600000) })`,
    sandbox,
  );
  payloads.conversation_delete = () => {
    currentDeleted = true;
    return { deleted: 3, redacted_inputs: 1, segments: 1, cleared_current: true };
  };
  // 删除前用户点开过一段历史,那次装载还在途:删完它才落地,不得把被删内容画回来。
  let releaseLoad;
  invokeGates.set("conversation_get", new Promise((resolve) => { releaseLoad = resolve; }));
  const inflightLoad = sandbox.loadConversation(7);
  await settle();
  invokeGates.delete("conversation_get");
  confirmOptions = null;
  await deleteConversation(LINE.id, [42]);
  releaseLoad();
  await inflightLoad;
  await flush();
  await touchTop();
  assert(calls("conversation_delete").length === deletesBeforeG + 1, "场景 H 前置失败:空闲线删除没有发出 conversation_delete");
  assert(!paneNow().includes("R1被删对话"), "场景 H:删掉当前段后主区仍显示被删对话(或迟到装载/触顶补齐把它画了回来)");
  assert(paneNow().includes(sandbox.t("开始一段新对话")), "场景 H:删掉当前段后主区没有换成与新对话同构的欢迎页");
  assert(!vm.runInContext('!!activePane.querySelector(".earlier-hint")', sandbox), "场景 H:删掉当前段后还挂着「载入更早的消息」入口");
  assert(
    !vm.runInContext(`autoContinueTimers.has(${JSON.stringify(LINE.session_id)})`, sandbox),
    "场景 H:删掉当前段后排上的续跑没撤——那一轮会带着「继续」落进空段",
  );
  assert(listText("toast").includes(sandbox.t("当前对话已删除")), `场景 H:删掉当前段没有告知用户(${listText("toast")})`);
  const confirmList = confirmOptions?.list ?? [];
  assert(
    confirmList.includes(sandbox.t("运行轨迹、子代理记录与压缩摘要"))
      && confirmList.includes(sandbox.t("保留:用量统计、已提炼的记忆与需求记录、迁移备份 state.db.v*.bak")),
    `场景 H:删除确认清单没有如实写明删除范围与保留项(${JSON.stringify(confirmList)})`,
  );
  assert(
    !confirmList.includes(sandbox.t("草稿与未完成输入")),
    "场景 H:删除确认清单仍承诺删除「草稿与未完成输入」——未结束的输入并不会删",
  );

  // ---- 场景 I:删掉正在看的那段旧历史 → 按当前段重载主区 ----
  currentDeleted = false;
  payloads.conversation_get = ({ processId, sequence } = {}) => {
    if (processId !== LINE.id) return [{ role: "user", parts: [{ type: "text", text: "R1主线内容" }] }];
    return sequence != null ? DELETED : CURRENT;
  };
  await sandbox.loadConversation(7);
  await flush();
  assert(paneNow().includes("R1被删对话299"), "场景 I 前置失败:打开的历史段没有装进主区");
  payloads.conversation_delete = () => ({ deleted: 5, redacted_inputs: 0, segments: 1, cleared_current: false });
  const getsBeforeI = calls("conversation_get").length;
  await deleteConversation(LINE.id, [7]);
  await flush();
  await touchTop();
  assert(
    calls("conversation_get").slice(getsBeforeI).some(({ args }) => args?.processId === LINE.id && args?.sequence == null),
    "场景 I:删掉正在看的那段历史后没有按当前段重新装载主区",
  );
  assert(
    !paneNow().includes("R1被删对话") && paneNow().includes("R1当前段"),
    "场景 I:删掉正在看的那段历史后主区仍显示它(或触顶从缓存补回)",
  );

  // ---- 场景 J:删后台线的历史 → 它的 pane 与窗口化缓存作废,活动线不受影响 ----
  // 后台线已有 pane(之前切过去看过,或它在后台渲染过)。
  vm.runInContext(`paneFor(${JSON.stringify(MAIN.session_id)}).dataset.hasContent = "1"`, sandbox);
  vm.runInContext(
    `paneHistory.set(${JSON.stringify(MAIN.session_id)}, { items: [{ role: "user", parts: [{ type: "text", text: "R1后台缓存" }] }], rendered: 0 })`,
    sandbox,
  );
  assert(vm.runInContext(`messagePanes.has(${JSON.stringify(MAIN.session_id)})`, sandbox), "场景 J 前置失败:后台线没有自己的 pane");
  payloads.conversation_delete = () => ({ deleted: 4, redacted_inputs: 0, segments: 1, cleared_current: true });
  await deleteConversation(MAIN.id, [3]);
  await flush();
  assert(
    !vm.runInContext(`messagePanes.has(${JSON.stringify(MAIN.session_id)})`, sandbox),
    "场景 J:删了后台线的历史,它的 pane 还在——切过去会看到被删内容",
  );
  assert(
    !vm.runInContext(`paneHistory.has(${JSON.stringify(MAIN.session_id)})`, sandbox),
    "场景 J:删了后台线的历史,它的窗口化缓存还在——切过去触顶会补回被删内容",
  );
  assert(
    vm.runInContext("activeProcessId", sandbox) === LINE.id && paneNow().includes("R1当前段"),
    "场景 J:删后台线的历史波及了活动线的视图",
  );

  // ---- 场景 L:前端预检以为空闲、后端持锁判定在跑 → 与预检同一句已翻译的提示 ----
  // 后端原文是中文;经 toastError 原样显示会把中文甩进英文界面(还挂一个必然再被拒的重试)。
  // 走了 toastError 的话,harness 的持久错误探针也会判红。
  esmModuleCache.get("03-shell.js")?.namespace?.toast("R1哨兵");
  invokeFailures.set("conversation_delete", "线路运行中,先停止再删除历史对话");
  const deletesBeforeL = calls("conversation_delete").length;
  await deleteConversation(LINE.id, [42]);
  await flush();
  invokeFailures.delete("conversation_delete");
  assert(calls("conversation_delete").length === deletesBeforeL + 1, "场景 L 前置失败:没有发出 conversation_delete");
  assert(
    (listText("toast").split("R1哨兵").at(-1) ?? "").includes(sandbox.t("运行中请先完成或停止当前任务，再删除历史对话")),
    `场景 L:后端拒删(线路运行中)没有给出已翻译的提示(${listText("toast")})`,
  );
  assert(paneNow().includes("R1当前段"), "场景 L:被拒绝的删除动了主区");

  // ---- 场景 K:关闭线路弹窗说明对话去向 ----
  let closeMessage = "";
  sandbox.confirmDialog = (options) => {
    closeMessage = String(options?.message ?? "");
    return Promise.resolve(false);
  };
  const closesBeforeK = calls("process_close").length;
  await sandbox.closeParallelProcess(LINE.id);
  await flush();
  assert(calls("process_close").length === closesBeforeK, "场景 K 前置失败:取消关闭仍发出了 process_close");
  assert(
    closeMessage.includes(sandbox.t("这条线的对话历史仍保留在本地数据库，关闭后界面不再显示；要删除请先在它的「历史对话」里勾选删除。")),
    `场景 K:关闭线路弹窗没说明对话仍留在库里、关闭后界面不再可删(${closeMessage})`,
  );

  // 收尾:还原桩与定时器,丢掉本段建的 pane,回到项目 A 的干净状态。
  sandbox.confirmDialog = saved.confirmDialog;
  payloads.process_list = saved.process_list;
  payloads.conversation_get = saved.conversation_get;
  payloads.conversation_list = saved.conversation_list;
  payloads.conversation_delete = saved.conversation_delete;
  for (const sessionId of [MAIN.session_id, LINE.session_id]) {
    vm.runInContext(`transitionSession(${JSON.stringify(sessionId)}, "idle"); paneHistory.delete(${JSON.stringify(sessionId)})`, sandbox);
  }
  vm.runInContext("__kzAutoTestState.cancelTimers(); autoContinueTimers.clear()", sandbox);
  await gotoProject(PROJECT, savedDocsPayload);
  await flush();
}

// ===== 分区:模型选择 =====

// ===== 分区:弹层与外观 =====
// UI-0926 #9 弹层技术栈:00-surface.js 的唯一栈、Esc 唯一入口(捕获阶段只关栈顶)、
// 模态/菜单/停靠卡片/toast/tooltip 原语。设计见 docs/design/ui_surface_stack.md §9。
{
  const surface = esmModuleCache.get("00-surface.js")?.namespace;
  const events = esmModuleCache.get("07-events.js")?.namespace;
  const shell = esmModuleCache.get("03-shell.js")?.namespace;
  assert(surface && typeof surface.confirmDialog === "function", "00-surface.js 未按 ESM 加载或缺 confirmDialog");
  assert(!/^\s*import\b/m.test(sources[scriptSrcs.indexOf("00-surface.js")] ?? "import"), "00-surface.js 必须零 import");
  const keyEvent = (key, extra = {}) => ({
    type: "keydown",
    key,
    isComposing: false,
    defaultPrevented: false,
    preventDefault() { this.defaultPrevented = true; },
    stopPropagation() { this._stopped = true; },
    stopImmediatePropagation() { this._stopImmediate = true; this._stopped = true; },
    ...extra,
  });
  const pressEscape = () => document.dispatchEvent(keyEvent("Escape"));
  const compose = sources[scriptSrcs.indexOf("08-compose-runtime.js")] ?? "";
  if (surface && events) {
    // 起点:把前面用例留下的弹层全部收掉,深度断言才有意义。
    events.hideAsk();
    for (const id of ["viewer-overlay", "confirm-overlay", "input-overlay", "palette", "context-detail", "sop-picker-panel", "file-suggestions", "task-options-menu", "composer-more-menu", "autorun-menu", "voice-settings-panel"]) {
      surface.closeSurface(byId.get(id));
    }
    assert(surface.stackDepth() === 0, `弹层用例起点栈不为空(深度 ${surface.stackDepth()}):有弹层绕过原语打开或没关`);

    // ① 确认框:<dialog>.showModal、镜像 .hidden、结果值、栈清空;并发调用排队不互相覆盖。
    const confirmHost = byId.get("confirm-overlay");
    const p1 = surface.confirmDialog({ title: "弹层冒烟", message: "确认?" });
    assert(confirmHost.tagName === "DIALOG" && confirmHost.open && confirmHost._modal, "confirmDialog 未以 <dialog>.showModal 打开");
    assert(!confirmHost.classList.contains("hidden"), "confirmDialog 打开后仍带 .hidden");
    assert(byId.get("confirm-title").textContent === "弹层冒烟", "confirmDialog 标题未写入");
    assert(surface.isModalOpen() && surface.stackDepth() === 1, "confirmDialog 打开后 isModalOpen/stackDepth 不对");
    byId.get("confirm-ok").click();
    assert(await p1 === true, "点确认未得到 true");
    assert(!confirmHost.open && confirmHost.classList.contains("hidden"), "确认后弹窗未关闭或未镜像回 .hidden");
    assert(surface.stackDepth() === 0 && !surface.isModalOpen(), `确认后弹层栈未清空(深度 ${surface.stackDepth()})`);
    const p2 = surface.confirmDialog({ title: "安全整理", safeText: "删除并安全整理" });
    assert(!byId.get("confirm-safe").classList.contains("hidden"), "safeText 按钮未显示");
    byId.get("confirm-safe").click();
    assert(await p2 === "safe", "点 safe 按钮未得到 \"safe\"");
    const pa = surface.confirmDialog({ title: "第一问" });
    const pb = surface.confirmDialog({ title: "第二问" });
    assert(byId.get("confirm-title").textContent === "第一问", "并发的第二个确认框覆盖了第一个的文案(应排队)");
    byId.get("confirm-ok").click();
    assert(await pa === true, "排队:第一个确认框结果不对");
    assert(confirmHost.open && byId.get("confirm-title").textContent === "第二问", "排队:第一个关闭后第二个未接着打开");
    byId.get("confirm-cancel").click();
    assert(await pb === false, "排队:第二个确认框取消未得到 false");
    assert(surface.stackDepth() === 0, "排队用例后弹层栈未清空");

    // ② Esc 只关栈顶(回归「确认框里按 Esc 先拒掉权限请求」的串台)。
    vm.runInContext('activeSessionId = "sess-smoke"', sandbox);
    byId.get("auto-allow").checked = false;
    const answerCalls = () => invokeArgs.filter(({ cmd }) => cmd === "answer_ask");
    handlers.get("kz:ask")({ payload: { id: 9901, sessionId: "sess-smoke", kind: "permission", action: "bash", resource: "cargo test" } });
    await flush();
    const askCard = byId.get("ask-overlay");
    assert(askCard._popoverOpen && !askCard.classList.contains("hidden"), "权限卡未经 showCard 以 popover 显示");
    assert(events.askActive?.id === 9901, "权限卡未成为当前请求");
    const answersBefore = answerCalls().length;
    const pending = surface.confirmDialog({ title: "Esc 栈顶" });
    const esc = keyEvent("Escape");
    document.dispatchEvent(esc);
    assert(esc.defaultPrevented && esc._stopImmediate, "Esc 入口未 preventDefault + stopImmediatePropagation(后面的监听还会再处理一遍)");
    // 先同步判「确认框关没关」:关错了对象时 pending 永不落定,直接 await 会把冒烟挂死而不是判红。
    const confirmClosedByEsc = !confirmHost.open;
    if (!confirmClosedByEsc) byId.get("confirm-cancel").click();
    assert(confirmClosedByEsc, "第一次 Esc 没有关掉栈顶的确认框(关到别的弹层上去了)");
    assert(await pending === false, "第一次 Esc 应只关确认框(得到 false)");
    await flush();
    assert(answerCalls().length === answersBefore, "Esc 串台:确认框开着时按 Esc 把权限请求拒掉了");
    assert(events.askActive?.id === 9901 && !askCard.classList.contains("hidden"), "第一次 Esc 后权限卡应仍在");
    pressEscape();
    await flush();
    assert(
      answerCalls().length === answersBefore + 1 && answerCalls().at(-1)?.args?.reply === "deny",
      `第二次 Esc 应拒绝权限请求:${JSON.stringify(answerCalls().at(-1)?.args)}`,
    );
    assert(askCard.classList.contains("hidden") && !events.askActive, "拒绝后权限卡未收起");
    // 收起后的「重新打开」芯片不响应 Esc(按一下 Esc 不能把唯一的回答入口弄丢)。
    handlers.get("kz:ask")({ payload: { id: 9902, sessionId: "sess-smoke", kind: "permission", action: "read", resource: "a.txt" } });
    await flush();
    byId.get("ask-collapse").click();
    assert(!byId.get("ask-reopen").classList.contains("hidden") && askCard.classList.contains("hidden"), "收起后未显示重新打开芯片");
    const answersBeforeChip = answerCalls().length;
    pressEscape();
    await flush();
    assert(!byId.get("ask-reopen").classList.contains("hidden") && answerCalls().length === answersBeforeChip, "重新打开芯片被 Esc 关掉了或 Esc 误答了请求");
    byId.get("ask-reopen").click();
    byId.get("ask-allow").click();
    await flush();
    assert(surface.stackDepth() === 0, `权限卡用例后弹层栈未清空(深度 ${surface.stackDepth()})`);

    // ②b 卡片不抢别处的局部 Esc:焦点在卡片外的文字输入框(#prompt、想法/缺陷速记表单)或 Monaco 里时,
    //     Esc 不拒权限请求、不 preventDefault、不截断传播(输入框自己的 Esc 照常取消输入);
    //     焦点在勾选框这类没有局部 Esc 含义的元素上时,仍按卡片的 onEscape 拒绝。
    handlers.get("kz:ask")({ payload: { id: 9903, sessionId: "sess-smoke", kind: "permission", action: "bash", resource: "ls" } });
    await flush();
    assert(events.askActive?.id === 9903 && !askCard.classList.contains("hidden"), "②b 前置:权限卡未显示");
    const answersBeforeYield = answerCalls().length;
    const quickInput = document.createElement("input");
    const monacoHost = document.createElement("div");
    monacoHost.className = "monaco-editor";
    const monacoWidget = document.createElement("span");
    monacoHost.appendChild(monacoWidget);
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    body.append(quickInput, monacoHost, checkbox);
    for (const [label, target] of [["#prompt", byId.get("prompt")], ["卡片外的速记输入框", quickInput], ["Monaco 编辑器内", monacoWidget]]) {
      const ev = keyEvent("Escape", { target });
      document.dispatchEvent(ev);
      await flush();
      assert(!ev.defaultPrevented && !ev._stopped, `焦点在${label}时按 Esc 被弹层栈截走了(局部 Esc 收不到)`);
      assert(answerCalls().length === answersBeforeYield && events.askActive?.id === 9903, `焦点在${label}时按 Esc 把权限请求拒掉了`);
    }
    const escOnCheckbox = keyEvent("Escape", { target: checkbox });
    document.dispatchEvent(escOnCheckbox);
    await flush();
    assert(
      escOnCheckbox.defaultPrevented && answerCalls().length === answersBeforeYield + 1 && answerCalls().at(-1)?.args?.reply === "deny",
      "焦点在卡片外的勾选框上时 Esc 仍应拒绝权限请求(只有文字输入框与 Monaco 让位)",
    );
    quickInput.remove();
    monacoHost.remove();
    checkbox.remove();
    assert(surface.stackDepth() === 0, `②b 用例后弹层栈未清空(深度 ${surface.stackDepth()})`);

    // ③ 输入框:组合中的 Enter 不提交;普通 Enter 返回输入值;Esc 返回 null。
    const inputHost = byId.get("input-overlay");
    const pi = surface.inputDialog({ title: "输入冒烟", value: "初值" });
    assert(inputHost.tagName === "DIALOG" && inputHost.open && inputHost._modal, "inputDialog 未以 <dialog>.showModal 打开");
    assert(byId.get("input-value").value === "初值", "inputDialog 未回填默认值");
    byId.get("input-value").value = "组合中";
    byId.get("input-value").dispatchEvent(keyEvent("Enter", { isComposing: true }));
    assert(inputHost.open, "输入法组合中的 Enter 不应提交");
    byId.get("input-value").value = "最终值";
    byId.get("input-value").dispatchEvent(keyEvent("Enter"));
    assert(await pi === "最终值", "普通 Enter 应返回输入值");
    const pn = surface.inputDialog({ title: "Esc 取消" });
    pressEscape();
    assert(await pn === null && !inputHost.open, "Esc 应关闭输入框并返回 null");

    // ④ openMenu:role、禁用项、点击先关再 onSelect 且恰好一次、同时只开一个、同锚点再调 = 收起。
    const anchorA = document.createElement("button");
    const anchorB = document.createElement("button");
    body.append(anchorA, anchorB);
    let picked = 0;
    let disabledHit = 0;
    const menuA = surface.openMenu(anchorA, [
      { label: "甲", onSelect: () => { picked += 1; } },
      "separator",
      { label: "乙(禁用)", disabled: true, onSelect: () => { disabledHit += 1; } },
    ], { label: "冒烟菜单" });
    const itemsA = menuA.el.querySelectorAll(".k-menu-item");
    assert(menuA.el.getAttribute("role") === "menu" && itemsA.length === 2, "openMenu 菜单 role 或项数不对");
    assert(itemsA.every((item) => item.getAttribute("role") === "menuitem"), "openMenu 菜单项必须带 role=menuitem");
    assert(menuA.el._popoverOpen && menuA.el.classList.contains("k-surface") && surface.stackDepth() === 1, "openMenu 未以 k-surface popover 打开或未入栈");
    itemsA[1].click();
    assert(disabledHit === 0 && !menuA.closed, "禁用项被触发或点禁用项关掉了菜单");
    const menuB = surface.openMenu(anchorB, [{ label: "丙", onSelect: () => { picked += 10; } }]);
    assert(menuA.closed && !menuA.el.parentNode, "打开第二个菜单时第一个未自动关闭");
    assert(menuB.el._popoverOpen && surface.stackDepth() === 1, "第二个菜单未打开或栈深度不为 1");
    menuB.el.querySelector(".k-menu-item").click();
    assert(picked === 10, `菜单项 onSelect 应恰好调用一次(picked=${picked})`);
    assert(menuB.closed && !menuB.el.parentNode && surface.stackDepth() === 0, "点菜单项后菜单未关闭或未移除");
    const menuC = surface.openMenu(anchorA, [{ label: "丁", onSelect() {} }]);
    surface.openMenu(anchorA, [{ label: "丁", onSelect() {} }]);
    assert(menuC.closed && surface.stackDepth() === 0, "同一锚点再次 openMenu 应收起已开的菜单(切换语义)");
    anchorA.remove();
    anchorB.remove();

    // ④b 弹窗里的菜单:模态开着时 dialog 子树之外全是惰性的,openMenu 必须挂进锚点所在的 <dialog>;
    //     点菜单项不关弹窗;关弹窗时菜单作为嵌套弹层一起关;打开 dialog 外的静态弹层要告警(不静默点不动)。
    const viewerHost = byId.get("viewer-overlay");
    const viewerHandle = surface.openDialog(viewerHost);
    const inDialogAnchor = document.createElement("button");
    viewerHost.appendChild(inDialogAnchor);
    let pickedInDialog = 0;
    const dialogMenu = surface.openMenu(inDialogAnchor, [{ label: "戊", onSelect: () => { pickedInDialog += 1; } }]);
    assert(dialogMenu.el.parentNode === viewerHost, "弹窗里的 openMenu 未挂进锚点所在的 <dialog>(挂在 body 下时模态开着点不动)");
    assert(surface.stackDepth() === 2 && dialogMenu.el._popoverOpen, "弹窗里的菜单未打开或未入栈");
    dialogMenu.el.querySelector(".k-menu-item").click();
    assert(pickedInDialog === 1 && viewerHost.open && surface.stackDepth() === 1, "弹窗里点菜单项应调 onSelect 一次、只关菜单不关弹窗");
    const dialogMenu2 = surface.openMenu(inDialogAnchor, [{ label: "己", onSelect() {} }]);
    pressEscape();
    assert(dialogMenu2.closed && viewerHost.open, "弹窗里开着菜单时,第一次 Esc 应只关菜单");
    const dialogMenu3 = surface.openMenu(inDialogAnchor, [{ label: "庚", onSelect() {} }]);
    surface.closeSurface(viewerHandle);
    assert(dialogMenu3.closed && !dialogMenu3.el.parentNode && !viewerHost.open, "关弹窗时里面的菜单未一起关掉并移除");
    const warns = [];
    const priorWarn = sandbox.console.warn;
    sandbox.console.warn = (...args) => warns.push(args.map(String).join(" "));
    try {
      const warnHandle = surface.openDialog(viewerHost);
      surface.openPopover(byId.get("status-tokens"), byId.get("context-detail"));
      surface.closeSurface(byId.get("context-detail"));
      surface.closeSurface(warnHandle);
    } finally {
      sandbox.console.warn = priorWarn;
    }
    assert(warns.some((text) => text.includes("context-detail") && text.includes("惰性")), `模态开着时打开 dialog 外的静态弹层应告警:${JSON.stringify(warns)}`);
    inDialogAnchor.remove();
    assert(surface.stackDepth() === 0, `④b 用例后弹层栈未清空(深度 ${surface.stackDepth()})`);

    // ⑤ bindMenus:四个静态菜单触发器接线;点开 aria-expanded=true,Esc 关闭后复位;
    //    点外关闭;按下触发器本身不算「点外」(否则随后的 click 会把刚关的菜单重新打开)。
    for (const id of ["task-options", "composer-more", "autorun-more", "voice-settings-toggle"]) {
      const trigger = byId.get(id);
      assert(trigger?.getAttribute("aria-controls") && trigger.getAttribute("aria-haspopup"), `bindMenus 未给 #${id} 接线`);
    }
    const moreTrigger = byId.get("composer-more");
    const moreMenu = byId.get("composer-more-menu");
    assert(moreTrigger.getAttribute("aria-expanded") === "false", "菜单触发器初始 aria-expanded 应为 false");
    moreTrigger.click();
    assert(moreTrigger.getAttribute("aria-expanded") === "true" && moreMenu._popoverOpen && !moreMenu.classList.contains("hidden"), "点 data-kz-menu 触发器未打开弹层菜单");
    pressEscape();
    assert(moreTrigger.getAttribute("aria-expanded") === "false" && !moreMenu._popoverOpen && moreMenu.classList.contains("hidden"), "Esc 未关闭菜单或 aria-expanded 未复位");
    moreTrigger.click();
    document.dispatchEvent({ type: "pointerdown", target: body });
    assert(!moreMenu._popoverOpen && moreTrigger.getAttribute("aria-expanded") === "false", "点外未关闭菜单");
    moreTrigger.click();
    document.dispatchEvent({ type: "pointerdown", target: moreTrigger });
    assert(moreMenu._popoverOpen, "按下触发器不应触发点外关闭");
    moreTrigger.click();
    assert(!moreMenu._popoverOpen && surface.stackDepth() === 0, "再点触发器应收起菜单");
    // 鞭挞菜单的数字快捷键改挂在触发器与菜单元素上、以「菜单开着」为前提(假 DOM 拍平了菜单行,
    // 行内命中只能由浏览器样例冒烟验;这里锁住接线点,防止退回 details[open] 判据)。
    assert(
      /\$\("autorun-menu"\)\.addEventListener\("keydown", autorunMenuShortcut\)/.test(compose) && /isSurfaceOpen\(menu\)/.test(compose),
      "鞭挞菜单数字快捷键未挂到 #autorun-menu 或不再以弹层开着为前提",
    );

    // ⑥ toast:err 用 role=alert,默认 role=status;最多 3 条;过期只隐藏、文案留到下一条。
    await flush();
    const region = byId.get("toast");
    surface.toast("错误冒烟", { kind: "err" });
    const errItem = region.children.at(-1);
    assert(errItem?.getAttribute("role") === "alert" && errItem.dataset.kind === "err", "err toast 应为 role=alert、data-kind=err");
    shell.toast("普通冒烟");
    assert(region.children.at(-1)?.getAttribute("role") === "status" && region.children.at(-1).dataset.kind === "info", "默认 toast 应为 role=status、kind=info");
    surface.toast("第三条");
    surface.toast("第四条");
    const liveToasts = region.children.filter((node) => node.dataset.kzExpired !== "1");
    assert(liveToasts.length === 3, `连发 4 条后区域内应只剩 3 条(实得 ${liveToasts.length})`);
    assert(region._popoverOpen && !region.classList.contains("hidden"), "toast 区域未以 popover 显示");
    await flush();
    assert(region.classList.contains("hidden") && !region._popoverOpen, "toast 全部过期后区域未收起");
    assert(listText("toast").includes("第四条"), "过期的 toast 文案应留到下一条到来(读的人与冒烟都要拿得到最近一条)");

    // ⑦ tooltip:悬停把 title 挪进 data-kz-tip 并显示 #kz-tip;移开后 title 还回去。
    const tipTarget = byId.get("log-toggle");
    const tipText = tipTarget.getAttribute("title");
    assert(tipText, "冒烟前置:#log-toggle 应带 title");
    document.dispatchEvent({ type: "pointerover", target: tipTarget });
    await flush();
    const tipEl = document.querySelector("#kz-tip");
    assert(tipTarget.getAttribute("title") === null && tipTarget.dataset.kzTip === tipText, "悬停后 title 未挪进 data-kz-tip(系统提示会与自绘提示叠成两个)");
    assert(tipEl?._popoverOpen && tipEl.textContent === tipText && tipEl.getAttribute("role") === "tooltip", "悬停后 #kz-tip 未显示或文本不一致");
    assert(tipTarget.getAttribute("aria-describedby") === "kz-tip", "显示提示期间目标应 aria-describedby=kz-tip");
    document.dispatchEvent({ type: "pointerout", target: tipTarget, relatedTarget: body });
    assert(tipTarget.getAttribute("title") === tipText && tipTarget.dataset.kzTip === undefined, "移开后 title 未恢复");
    assert(!tipEl._popoverOpen && tipTarget.getAttribute("aria-describedby") === null, "移开后提示未隐藏或 aria-describedby 未复位");

    // ⑧ 静态护栏:模态期间全局快捷键让路;手算定位的 placeAutorunMenu 不得复活;
    //    document/window 级 Esc 监听只剩 00-surface 一处。
    const globalShortcut = compose.slice(compose.indexOf('window.addEventListener("keydown"')).slice(0, 400);
    assert(globalShortcut.includes("if (isModalOpen()) return;"), "08-compose-runtime 的全局快捷键处理函数开头缺 isModalOpen() 守卫(确认框背后会真的点「新对话」)");
    assert(!sources.some((source) => source.includes("placeAutorunMenu")), "placeAutorunMenu 复活了:弹层位置归 CSS 锚点定位");
    // 权限卡弹出时的焦点只归 showCard 的 focus:"auto"(用户在别处打字时不抢)。旧实现 pumpAsk 里
    // setTimeout 把焦点抢到「允许一次」,下一个空格就放行;合并时最容易被当成上下文行留回来。
    assert(!sources.some((source) => /\$\(\s*["']ask-allow["']\s*\)\.focus\(/.test(source)), "有代码把焦点直接抢到 #ask-allow(正在打字时一个空格就放行):权限卡焦点只归 showCard focus:\"auto\"");
    const surfaceSource = sources[scriptSrcs.indexOf("00-surface.js")] ?? "";
    assert(surfaceSource.includes('document.addEventListener("keydown", onKeydown, true)'), "00-surface.js 的 Esc 唯一入口不在 document 捕获阶段");
  }
}

// UI-0926 #5 配色:发送键 = 单色圆形图标按钮。可见文字没了,名称只剩 sr-only 与 aria-label 两条来源,
// 两条都必须跟着界面语言走(英文态念 "Send");图标是 aria-hidden 的 SVG,不能退回文字按钮。
{
  const send = sandbox.document.getElementById("send");
  assert(send, "UI-0926 #5:找不到 #send");
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  sandbox.setRunning(false);
  assert(send.getAttribute("aria-label") === "Send", `UI-0926 #5:英文态空闲的发送键读屏名称应为 "Send",实得 "${send.getAttribute("aria-label")}"`);
  assert(send.getAttribute("title") === "Send", `UI-0926 #5:英文态空闲的发送键悬停提示应为 "Send",实得 "${send.getAttribute("title")}"`);
  // 假 DOM 只按 id 建节点,按钮内部的 svg/span 看不见:标记本身静态核对。
  const sendMarkup = html.match(/<button id="send"[^>]*>[\s\S]*?<\/button>/)?.[0] ?? "";
  assert(/<svg aria-hidden="true"/.test(sendMarkup), `UI-0926 #5:发送键应为图标按钮(内含 aria-hidden 的 svg),实为 ${sendMarkup.slice(0, 120)}`);
  assert(/<span class="sr-only" data-i18n-key="发送">发送<\/span>/.test(sendMarkup), "UI-0926 #5:发送键缺少随语言翻译的 sr-only 文字(data-i18n-key=\"发送\")");
  assert(/data-i18n-title="发送"/.test(sendMarkup), "UI-0926 #5:发送键的静态悬停提示缺 data-i18n-title");
  sandbox.setRunning(true);
  assert(send.getAttribute("aria-label") === "While running, send to steer or queue according to Delivery", `UI-0926 #5:运行态发送键读屏名称未翻译,实得 "${send.getAttribute("aria-label")}"`);
  sandbox.setRunning(false);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(send.getAttribute("title") === "发送" && send.getAttribute("aria-label") === "发送", `UI-0926 #5:切回中文后发送键应为「发送」,实得 title="${send.getAttribute("title")}" aria-label="${send.getAttribute("aria-label")}"`);
  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ===== 分区:工具行与结构化渲染 =====
// ---------- UI-0926 #6:工具行人话摘要(05-tool-summary.js) ----------
// ⎿ 列此前是工具原文首行截 110 字:read 显示行号+源码、grep 显示 `path-450- }`、symbols 显示
// `== \\?\C:\…`、JSON 工具显示 `{`。这里用**真实 Rust 输出格式**的夹具逐工具断言精确文本,
// 再断言所有夹具的摘要都不含源码/路径/JSON/转义/乱码,并验证实时与历史同源、旧后端降级、
// 活动面板同口径、正文不挂到块上(内存)。
{
  const summaryNs = esmModuleCache.get("05-tool-summary.js")?.namespace;
  const parseNs = esmModuleCache.get("04-structured-parse.js")?.namespace;
  const shellNs = esmModuleCache.get("03-shell.js")?.namespace;
  const chatNs = esmModuleCache.get("05-chat-render.js")?.namespace;
  const viewsNs = esmModuleCache.get("15-views-misc.js")?.namespace;
  assert(summaryNs?.toolResultSummary && summaryNs?.toolArgSummary && summaryNs?.renderToolSummary, "05-tool-summary.js 未导出摘要器三件套");
  assert(parseNs?.stripAnsi && parseNs?.displayPath && parseNs?.parseJsonish && parseNs?.stripToolOutcome, "04-structured-parse.js 未导出纯解析助手");
  // 摘要器只从纯解析模块取助手(单一真源):不得自己再实现一份 stripAnsi/displayPath。
  const summarySource = sources[scriptSrcs.indexOf("05-tool-summary.js")] ?? "";
  const parseSource = sources[scriptSrcs.indexOf("04-structured-parse.js")] ?? "";
  assert(/from "\.\/04-structured-parse\.js"/.test(summarySource), "05-tool-summary.js 未从 04-structured-parse.js 引入解析助手");
  assert(!/function (?:stripAnsi|displayPath|normalizeRoot|looksLikeNoise)\b/.test(summarySource), "05-tool-summary.js 又自带了一份路径/ANSI/噪声助手(应只有 04-structured-parse.js 一处)");
  assert(!/^\s*import\b/m.test(parseSource), "04-structured-parse.js 必须零 import(纯函数模块,Node 里可直接单测)");

  if (summaryNs && parseNs && shellNs && chatNs && viewsNs) {
    // 断言写在冒烟末尾:此前的用例可能把界面切到英文、把会话收敛成空闲(收敛后迟到的
    // tool-start 会被丢弃)。这里固定中文文案,并给当前活动会话发一轮 kz:turn 让它回到运行态。
    const priorLanguage = localStorageShim.getItem("kz-language");
    localStorageShim.setItem("kz-language", "zh");
    const SID = shellNs.activeSessionId || "sess-smoke";
    if (!shellNs.activeSessionId) shellNs.setActiveSessionId?.(SID);
    handlers.get("kz:turn")?.({ payload: { sessionId: SID, step: 2, maxSteps: 12 } });
    await flush();
    const { toolResultSummary, toolArgSummary } = summaryNs;
    // 项目根带空格(用户真实目录形如 `Documents/kanzei code`)且以 verbatim 形态出现。
    const ROOT = "C:\\Users\\kanzei\\Documents\\kanzei code";
    const VROOT = `\\\\?\\${ROOT}`;
    const roots = [VROOT];
    const sum = (name, ctx) => toolResultSummary(name, { roots, ok: true, ...ctx });
    const NOISE = [
      [/\\\\\?\\|\/\/\?\//, "verbatim 前缀"],
      [/(?:^|[^A-Za-z0-9])[A-Za-z]:[\\/]/, "盘符绝对路径"],
      [/^\s*\d+\t/m, "带行号的源码行"],
      [/^[{[]|^==/, "JSON/表头开头"],
      [/\x1b/, "ANSI 转义"],
      [/\uFFFD/, "U+FFFD 乱码"],
    ];
    const noiseOf = (text) => NOISE.filter(([re]) => re.test(text)).map(([, label]) => label);
    const readLines = (from, to) => Array.from({ length: to - from + 1 }, (_, i) => `${String(from + i).padStart(6)}\tlet v${from + i} = ${from + i};`).join("\n");
    const HASH = "1a2b3c4";
    const cases = [
      // read
      ["read 全文", "read", { content: `${readLines(1, 3)}\n` }, "全文 3 行"],
      ["read 区间", "read", { content: `${readLines(10, 12)}\n`, input: { path: "x.rs", offset: 10 } }, "第 10–12 行 · 共 12 行"],
      ["read 带总数截断", "read", { content: `${readLines(1, 10)}\n... (truncated at line 11 of 5000; use offset to continue)\n`, input: { path: "x.rs", limit: 10 } }, "第 1–10 行 · 共 5,000 行"],
      ["read 无总数截断", "read", { content: `${readLines(1, 10)}\n... (truncated at line 11; use offset to continue)\n` }, "第 1–10 行 · 未读完"],
      ["read tail", "read", { content: `(last 3 lines of 12.0 KiB)\n${readLines(1, 3)}\n`, input: { path: "x.log", tail: 3 } }, "末尾 3 行 · 文件 12.0 KiB"],
      ["read tail 到文件头", "read", { content: `${readLines(1, 2)}\n`, input: { path: "x.log", tail: 50 } }, "全文 2 行"],
      ["read 图片", "read", { content: `[image] ${ROOT}\\shot.png (image/png, 20480 bytes) — attached to this tool result.` }, "图片 · 20.0 KB"],
      ["read pdf", "read", { content: "pdf: 12 pages; showing 2-3\n--- page 2 ---\n正文" }, "PDF 第 2–3 页 · 共 12 页"],
      ["read notebook", "read", { content: "notebook: 8 cells, kernel language python; showing 1-5\n[1] code\nprint(1)" }, "第 1–5 格 · 共 8 格"],
      ["read 空范围", "read", { content: "(empty range: file has 3 lines, offset was 3)" }, "空范围 · 文件共 3 行"],
      // grep
      ["grep 普通", "grep", { content: "src/a.rs:3: fn one() {}\nsrc/a.rs:9: fn two() {}\nsrc/b.rs:1: fn three() {}" }, "3 处匹配 · 2 个文件"],
      ["grep 单文件", "grep", { content: "src/a.rs:3: fn one() {}\nsrc/a.rs:4: fn two() {}" }, "2 处匹配"],
      ["grep context", "grep", { content: "src/a.rs-2- // ctx\nsrc/a.rs:3: fn one() {}\nsrc/a.rs-4- }\nsrc/b.rs:7: fn two() {}", input: { pattern: "fn", context: 1 } }, "2 处匹配 · 2 个文件"],
      ["grep files_only", "grep", { content: "src/a.rs\nsrc/b.rs\nsrc/c.rs", input: { pattern: "fn", files_only: true } }, "3 个文件"],
      ["grep count", "grep", { content: "src/a.rs: 2\nsrc/b.rs: 1\n(total 3 matches in 2 files)", input: { pattern: "fn", count: true } }, "3 处匹配 · 2 个文件"],
      ["grep 无匹配", "grep", { content: "(no matches for `zzz`)" }, "无匹配"],
      ["grep 达上限", "grep", { content: "src/a.rs:1: x\nsrc/b.rs:2: y\n... (stopped at limit 2; narrow the pattern or raise limit)" }, "2+ 处匹配 · 2+ 个文件"],
      // glob
      ["glob 普通", "glob", { content: "src/a.rs\nsrc/b.rs" }, "2 个文件"],
      ["glob more", "glob", { content: "src/a.rs\nsrc/b.rs\n... (8 more; raise limit or narrow pattern)" }, "10 个文件"],
      ["glob 无", "glob", { content: "(no files match `*.zz`)" }, "无匹配文件"],
      // symbols
      ["symbols 多文件(旧 verbatim 路径)", "symbols", { content: `== ${VROOT}\\src\\a.rs\n  pub fn one:1\n     fn two:3\n== ${VROOT}\\src\\b.rs\n  pub struct S:2` }, "3 个符号 · 2 个文件"],
      ["symbols 单文件", "symbols", { content: "== src/lib.rs\n  pub fn a:1\n  pub fn b:2" }, "2 个符号"],
      ["symbols callers", "symbols", { content: "callers of `helper` (2 hits):\nsrc/lib.rs:2: fn caller() { helper(); }\nsrc/lib.rs:3: fn b() { helper(); }" }, "2 处调用"],
      ["symbols define", "symbols", { content: "definition of `helper` (1 hit):\n  pub fn helper  src/lib.rs:1\n(no `pub use` re-export of this symbol found in tree)\n" }, "1 处定义"],
      ["symbols 无", "symbols", { content: "(no symbols found)" }, "无符号"],
      ["symbols 地图", "symbols", { content: "repo map (crates: 1, modules: 4, public_symbols: 37)\n== crate `kanzei_tools`\n  module `read` (crates/kanzei-tools/src/read.rs)" }, "4 个模块 · 37 个符号"],
      // files
      ["files", "files", { content: "crates/  (3 files, 12KB, 400 lines)\n  a.rs  1KB 40 行\n  b.rs  1KB 40 行\nREADME.md  2KB 800 字\n" }, "文件地图 · 3 项"],
      // edit / insert / write
      ["edit 有 display", "edit", { content: `replaced 1 occurrence(s) in ${ROOT}\\ui\\x.js\n局部结构校验通过: 1 个低成本检查\n局部校验明细:\n- node-check [passed] command: node --check ui/x.js`, display: { kind: "diff", path: "ui/x.js", additions: 3, deletions: 1, lines: [], local_validation: { counts: { passed: 1, failed: 0 } } } }, "+3 −1"],
      ["edit 历史按 input 算 diff", "edit", { content: `replaced 1 occurrence(s) in ${ROOT}\\ui\\x.js`, input: { path: "ui/x.js", old_string: "a\nb", new_string: "a\nc\nd" } }, "+2 −1"],
      ["edit replace_all 倍数", "edit", { content: `replaced 3 occurrence(s) in ${ROOT}\\ui\\x.js`, input: { path: "ui/x.js", old_string: "foo", new_string: "bar", replace_all: true } }, "+3 −3 · 替换 3 处"],
      ["edit 校验失败", "edit", { content: `replaced 1 occurrence(s) in ${ROOT}\\ui\\x.js\n局部结构校验发现 2 个精确错误，请先修复后再扩大回归`, display: { kind: "diff", additions: 1, deletions: 0, lines: [], local_validation: { counts: { failed: 2 } } } }, "+1 −0 · 校验 2 个错误"],
      ["insert", "insert", { content: `inserted content after unique anchor in ${ROOT}\\ui\\x.js`, input: { path: "ui/x.js", anchor: "x", content: "a\nb\n" } }, "+2 −0"],
      ["write 新建", "write", { content: `wrote 9 bytes to ${ROOT}\\x.txt\n局部结构校验通过: 0 个低成本检查`, display: { kind: "create", path: "x.txt", bytes: 9, preview: "l1\nl2\nl3" }, input: { path: "x.txt", content: "l1\nl2\nl3\n" } }, "新建 · 3 行"],
      ["write 覆写", "write", { content: `wrote 20 bytes to ${ROOT}\\x.txt`, display: { kind: "diff", additions: 5, deletions: 2, lines: [] } }, "+5 −2"],
      ["write 历史", "write", { content: `wrote 3 bytes to ${ROOT}\\x.txt`, input: { path: "x.txt", content: "a\nb" } }, "写入 2 行"],
      // bash
      ["bash 多个 cargo test 二进制累加", "bash", { content: "exit code: 0\n   Compiling kanzei-app v0.1.0\nrunning 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured\nrunning 5 tests\ntest result: ok. 5 passed; 0 failed; 0 ignored; 0 measured", durationMs: 12300 }, "退出码 0 · 12.3s · 8 通过"],
      ["bash E0425 编译失败(ANSI + GBK 误解码)", "bash", { ok: false, content: "exit code: 101\n\u001b[1m\u001b[91merror[E0425]\u001b[0m\u001b[1m: cannot find value `foo` in this scope\u001b[0m\n --> src/main.rs:3:5\n鍒嗘瀽瀹屾垚 \uFFFD\nerror: could not compile `x` (bin \"x\") due to 2 previous errors", durationMs: 41200 }, "退出码 101 · 41.2s · error[E0425]: cannot find value `foo` in this scope · 2 个编译错误"],
      ["bash 超时", "bash", { ok: false, content: "timeout: true — command did not finish within 120000 ms and was killed. Retry with a larger timeout_ms if needed.\n[no output captured before timeout]", display: { kind: "terminal", command: "cargo build", exitCode: null, timeout: true, output: "", full: "" } }, "超时 · 已终止"],
      ["bash 后台", "bash", { content: "background: true\nprocess_id: bg-3\npid: 1234\ncommand: npm run dev\nUse the `process` tool", display: { kind: "terminal", command: "npm run dev", background: true, processId: "bg-3", output: "(后台运行中,用 process 工具查看输出)" } }, "后台运行 · bg-3"],
      ["bash 无输出", "bash", { content: "exit code: 0\n(no output)" }, "退出码 0 · 无输出"],
      ["bash 内 git commit", "bash", { content: `exit code: 0\n[main ${HASH}] fix: 修复工具行\n 2 files changed, 10 insertions(+)` }, `退出码 0 · ${HASH} fix: 修复工具行`],
      ["bash prose 末行", "bash", { content: "exit code: 0\nchecking...\n全部检查通过,没有发现问题" }, "退出码 0 · 全部检查通过,没有发现问题"],
      ["bash 纯 JSON 输出", "bash", { content: "exit code: 0\n{\n  \"a\": 1,\n  \"b\": 2\n}" }, "退出码 0 · 输出 4 行"],
      // process
      ["process list", "process", { content: `bg-1 [running] pid=12 owner=r cwd=${ROOT} :: npm run dev\nbg-2 [exited(0)] pid=- owner=r cwd=${ROOT} :: cargo build\n`, input: { action: "list" } }, "2 个后台进程"],
      ["process 无", "process", { content: "(no background processes)", input: { action: "list" } }, "无后台进程"],
      ["process stop", "process", { content: "stopped bg-1", input: { action: "stop", id: "bg-1" } }, "已停止 bg-1"],
      // git
      ["git commit", "git", { content: `committed verified staged set (${HASH}9f0e)\n${HASH} fix: 工具行摘要\n\n crates/x.rs | 10 +++++-----\n 2 files changed, 12 insertions(+), 3 deletions(-)`, input: { action: "commit", message: "fix: 工具行摘要" } }, `${HASH} fix: 工具行摘要 · 2 个文件 +12 −3`],
      ["git stage", "git", { content: "stage_request: t1\nstaged 3 file(s): a, b, c\nstaged_hash: h\nReview with `git diff`", input: { action: "stage", files: ["a", "b", "c"] } }, "已暂存 3 个文件"],
      ["git status 干净", "git", { content: "## main...origin/main", input: { action: "status" } }, "工作区干净"],
      ["git status 脏", "git", { content: "## main\n M crates/a.rs\n?? b.rs", input: { action: "status" } }, "2 处改动"],
      ["git diff", "git", { content: "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1,2 @@\n-a\n+b\n+c", input: { action: "diff" } }, "1 个文件 +2 −1"],
      ["git log", "git", { content: `${HASH} 09-26 10:00 kanzei | a\n5e6f7a8 09-26 09:00 kanzei | b`, input: { action: "log" } }, "2 条提交"],
      ["git finalize", "git", { content: "[finalize] complete: cargo test passed in 12.0s → staged h → committed\nx", input: { action: "finalize" } }, "测试通过 · 已提交"],
      ["git ff", "git", { content: "fast-forwarded main: 1111111 -> 2222222abc (worktree rel)\nsource: rel", input: { action: "merge_ff" } }, "main → 2222222a"],
      // web
      ["webfetch", "webfetch", { content: "HTTP 200 · https://x.com/a\n\nHello world content" }, "HTTP 200 · 19 字"],
      ["webfetch 截断", "webfetch", { content: "HTTP 200 · https://x.com/a\n\nabc\n…(截断)" }, "HTTP 200 · 3 字 · 已截断"],
      ["websearch", "websearch", { content: JSON.stringify({ query: "rust ansi", results: [{ title: "ANSI escape codes", url: "https://a" }, { title: "b", url: "https://b" }, { title: "c", url: "https://c" }], truncated: false, prior_art_budget: null }) }, "3 条结果 · ANSI escape codes"],
      // task / question
      ["task markdown 报告", "task", { content: "## 结论\n- 工具行已改为人话摘要\n" }, "结论"],
      ["task 超时", "task", { ok: false, content: "(超时,未产出结果)" }, "超时 · 未产出结果"],
      ["task 失败", "task", { ok: false, content: "子代理内部报错" }, "子代理内部报错"],
      ["question", "question", { content: "User answer: 用本地" }, "用户回答: 用本地"],
      // tracker
      ["req add", "req", { content: "added R-365 [todo] 新需求标题", input: { action: "add", title: "新需求标题" } }, "新增 R-365"],
      ["req 状态变化", "req", { content: "updated: R-364 [doing] 工具行摘要\n变更: 状态: todo → doing; 进展: ∅ → 开工", input: { action: "update", id: "R-364" } }, "R-364 → doing"],
      ["req 字段变化", "req", { content: "updated: R-364 [doing] 工具行摘要\n变更: 进展: a → b; 验收: c → d", input: { action: "update", id: "R-364" } }, "R-364 已更新 进展、验收"],
      ["req no-op", "req", { content: "no-op: R-364 字段已是该值,未写入(旧→新无差异)。" }, "无变化"],
      ["req list JSON", "req", { content: JSON.stringify({ schema_version: 1, kind: "requirement", deadlocked: false, deadlock_guidance: null, entries: [{ id: "R-1", blocked: false, lifecycle_status: "todo" }, { id: "R-2", blocked: true, lifecycle_status: "todo" }, { id: "R-3", blocked: false, lifecycle_status: "doing" }] }, null, 2), input: { action: "list" } }, "3 条 · 2 条可执行"],
      ["req get JSON", "req", { content: JSON.stringify({ id: "R-364", title: "工具行摘要", lifecycle_status: "doing", fields: [] }, null, 2), input: { action: "get", id: "R-364" } }, "R-364 doing · 工具行摘要"],
      ["req reorder", "req", { content: "reordered 5 requirements: R-1 → R-2", input: { action: "reorder" } }, "已重排 5 条"],
      ["defect add", "defect", { content: "added D-770 [open] 工具行乱码", input: { action: "add" } }, "新增 D-770"],
      // work
      ["work next resume", "work", { content: JSON.stringify({ schema_version: 1, decision: "resume", reason: "x", selected: { id: "R-364", title: "工具行摘要" } }, null, 2), input: { action: "next" } }, "继续 R-364 · 工具行摘要"],
      ["work next empty", "work", { content: JSON.stringify({ decision: "empty", selected: null }, null, 2), input: { action: "next" } }, "无可执行条目"],
      ["work claim", "work", { content: JSON.stringify({ claimed: "R-364", lifecycle_status: "doing" }), input: { action: "claim", id: "R-364" } }, "已认领 R-364"],
      ["work handoff", "work", { content: "model completion declared: done\ncriterion: x\nevidence_refs: ", input: { action: "handoff" } }, "已声明完成"],
      // test_record
      ["test_record 通过带计数", "test_record", { content: `recorded T-1786922727068. active: 0, archived: 1 (path: ${ROOT}\\.kanzei\\tests.md, archive: x)`, input: { title: "cargo test", status: "passed", summary: "3 passed; 0 failed" } }, "已记录 T-…7068 · 3/3 通过"],
      ["test_record 运行中", "test_record", { content: `recorded T-1786922727068. active: 1, archived: 0 (path: ${ROOT}\\.kanzei\\tests.md, archive: x)\n↳ 跑完请用 test_record 带 id=T-1786922727068 记终态`, input: { title: "cargo test", status: "running" } }, "已记录 T-…7068 · 运行中"],
      ["test_record 失败", "test_record", { content: "recorded T-1786922727068. active: 0, archived: 1 (path: p, archive: a)", input: { title: "cargo test", status: "failed", summary: "2 passed; 1 failed" } }, "已记录 T-…7068 · 1 失败 · 2/3"],
      // memory
      ["memory_search 3 条", "memory_search", { content: `M-003 [project/sop] 发版 SOP 两条通道 — 发版必读\n  片段\n  file: ${ROOT}\\.kanzei\\memory\\M-003.md\nM-005 [project/lesson] 教训 — 描述\n  s\n  file: f\nM-009 [project/sop] 流程 — 描述\n  s\n  file: f` }, "3 条记忆 · 发版 SOP 两条通道"],
      ["memory_search 无", "memory_search", { content: "(no memory matched `x` — if you learn something reusable here, record it with memory_note)" }, "无匹配记忆"],
      ["memory_note 记入", "memory_note", { content: `noted → ${ROOT}\\.kanzei\\memory\\inbox.md (pending notes: 4)` }, "已记入收件箱 · 待整理 4 条"],
      ["memory_note 重复", "memory_note", { content: "noted as duplicate (NOOP, 发版前先跑 verify…) — already an active memory covers it" }, "与已有记忆重复,未记录"],
      ["memory_add", "memory_add", { content: "added M-012 [sop] 标题" }, "新增 M-012"],
      ["memory_update", "memory_update", { content: "updated M-012 [active] 标题" }, "更新 M-012"],
      ["memory_promote", "memory_promote", { content: "promoted M-003 [sop] 标题 → active (evidence: 2 source(s))" }, "晋升 M-003"],
      ["memory_merge", "memory_merge", { content: "merged M-004 ← [M-005, M-006]" }, "合并入 M-004"],
      // 资产 / 其它
      ["architecture", "architecture", { content: "path: .kanzei/project/architecture/README.md\nhash: h\nvalidation: ok (12 indexed link(s))\n---\n# 架构" }, "校验通过"],
      ["conventions", "conventions", { content: "path: x\nhash: h\nlines: 40\nheadings:\n  # A\n  ## B\n  ## C\n---\n# A" }, "3 节"],
      ["browser 截图", "browser", { content: "浏览器已打开并截图:\ntitle: X\nurl: http://localhost:1420/\nviewport: desktop" }, "截图 · localhost:1420"],
      ["frontend_locate", "frontend_locate", { content: "2 处定义:\n  ui/style.css:12 .x {\n  ui/style.css:40 .x {" }, "2 处定义"],
      ["deliver", "deliver", { content: "[delivered] report.pdf (2048 bytes)", display: { kind: "file", name: "report.pdf", path: "out/report.pdf", bytes: 2048 } }, "已交付 report.pdf · 2.0 KB"],
      ["collaboration_status", "collaboration_status", { content: "Live collaboration status\n- a\n- b", display: { lines: [{}, {}] } }, "2 条线路"],
      // 兜底:tool_search 与未知 MCP 工具
      ["tool_search 兜底", "tool_search", { content: "{\n  \"tools\": [\"a\", \"b\"]\n}" }, "输出 3 行"],
      ["未知 MCP 工具兜底(源码行)", "mcp__x__y", { content: "     1\t// 这是一段中文源码注释说明" }, "完成"],
      // 行号 + 英文句子:合并空白后既不像源码也不符号密集,只有兜底看「清洗前的行号 Tab」能拦下
      // (toolSummaryNoise 变异守的就是这一条)。
      ["未知 MCP 工具兜底(行号 + 英文行)", "mcp__x__y", { content: "    12\tlet total be the sum of all values" }, "完成"],
      ["未知 MCP 工具兜底(人话)", "mcp__x__y", { content: "已同步 3 个日历事件到本地" }, "已同步 3 个日历事件到本地"],
      // 「命令根本没跑」的失败:⎿ 行必须说出原因,不能是空白、「输出 N 行」或光秃秃的「失败」。
      ["bash 用户拒绝(实时:content 空串)", "bash", { ok: false, outcome: "failed", code: "USER_DECLINED", content: "", preview: "(user declined)" }, "已拒绝"],
      ["bash 用户拒绝(旧后端只有 preview)", "bash", { ok: false, preview: "(user declined)" }, "已拒绝"],
      ["write 用户拒绝", "write", { ok: false, outcome: "failed", code: "USER_DECLINED", content: "", preview: "(user declined)", input: { path: "x.txt", content: "a\nb" } }, "已拒绝"],
      ["历史 用户拒绝", "bash", { ok: false, content: "permission request declined by user" }, "已拒绝"],
      ["历史 拒绝后连带取消", "read", { ok: false, content: "tool call cancelled because a previous permission request was declined" }, "已取消 · 前一项权限被拒绝"],
      ["bash 规则集拒绝", "bash", { ok: false, content: 'permission denied by ruleset: bash on `{"command":"rm -rf target","workdir":"C:/p"}`.\nThis action is denied by the project permission rules.' }, "被权限规则拒绝 bash"],
      ["write 规则集拒绝", "write", { ok: false, content: "permission denied by ruleset: edit on `.kanzei/project/requirements.md`.\nUse the req tool." }, "被权限规则拒绝 edit"],
      ["bash 自主运行跳过", "bash", { ok: false, content: 'permission requires user approval: bash on `{"command":"cargo publish"}`; autonomous/parallel run skipped it' }, "需要批准 · 自主运行已跳过"],
      ["bash 入参修复提示", "bash", { ok: false, outcome: "needs_correction", code: "INVALID_TOOL_INPUT", content: "Invalid input for tool `bash`: missing field `command`\n缺少必填参数 `command`。\nExample (one line): {\"command\":\"ls\"}\nYour raw input was: {}\nRetry the tool call with corrected JSON." }, "入参无效 · 缺少参数 command"],
      ["bash 执行中被停止", "bash", { ok: false, content: "cancelled: run stopped by user during execution" }, "已停止"],
      ["bash 未执行的其它失败(无退出码)", "bash", { ok: false, content: "bash: command must not be empty\nprovide a command" }, "bash: command must not be empty"],
      // 存储标记
      ["历史外置标记", "bash", { content: "[tool_result_externalized artifact_id=a1 bytes=2097152 sha256=ff]\nPreview: exit code: 0 (+9000 lines)\n完整原文已外置；请按 retrieval_hint 回读。" }, "输出较大 · 2.0 MB · 已外置"],
      ["实时 artifact display", "bash", { content: "[tool_result_externalized artifact_id=a1 bytes=2097152 sha256=ff]\nPreview: x", display: { kind: "artifact", bytes: 2097152 } }, "输出较大 · 2.0 MB · 已外置"],
      ["历史 noop 前缀", "edit", { ok: false, content: "[tool_outcome=noop code=EDIT_IDENTICAL_INPUT]\nold_string 与 new_string 相同" }, "无需修改"],
    ];
    for (const [label, name, ctx, expected] of cases) {
      const got = sum(name, ctx);
      assert(got.text === expected, `工具行摘要 ${label}:期望 "${expected}",实得 "${got.text}"(${got.key})`);
      const noise = noiseOf(got.text);
      assert(!noise.length, `工具行摘要 ${label} 仍含噪声(${noise.join("、")}):"${got.text}"`);
    }
    // 摘要的纯文本就是 parts 拼接:渲染后的 textContent 与 text 逐字一致,且代码记号进 span。
    {
      const el = document.createElement("span");
      const got = sum("git", cases.find(([label]) => label === "git commit")[2]);
      summaryNs.renderToolSummary(el, got);
      assert(el.textContent === `⎿ ${got.text}`, `renderToolSummary 的 textContent 与摘要文本不一致:"${el.textContent}"`);
      assert(el.querySelector(".tool-sum-code")?.textContent === HASH, "提交哈希未渲染成 .tool-sum-code");
      assert(el.querySelector(".tool-sum-add")?.textContent === "+12" && el.querySelector(".tool-sum-del")?.textContent === "−3", "增删行数未按 add/del 分色");
    }
    // 失败行仍是互斥切分:cleanPaths 之后摘要 + 剩余逐字拼回,verbatim 路径显示成相对路径。
    {
      const failed = sum("edit", { ok: false, preview: `cannot write ${VROOT}\\crates\\x.rs: 拒绝访问。 (+2 lines)` });
      assert(failed.mode === "split" && failed.text === "cannot write crates/x.rs: 拒绝访问。 (+2 lines)", `失败行没把 verbatim 路径相对化:"${failed.text}"`);
      // 规则集拒绝的全文(含处理建议)留在展开区,⎿ 行只说原因。
      const denied = sum("bash", cases.find(([label]) => label === "bash 规则集拒绝")[2]);
      assert(denied.rest.includes("denied by the project permission rules"), `规则集拒绝的处理建议没进展开区:"${denied.rest}"`);
    }
    // 最终安全网:某个摘要器把「行号 + 源码」(合并空白后 `1 // 注释`)夹在别的组后面漏出来,
    // 整行判据看不出,逐组判据必须拦下(toolSummarySafetyNet 变异守这一条)。
    {
      const table = summaryNs.TOOL_RESULT_SUMMARIZERS;
      table.__g4probe = () => ({ groups: ["退出码 0", "1 // 这是一段中文源码注释说明"], key: "probe" });
      try {
        const got = sum("__g4probe", { content: "exit code: 0\n     1\t// 这是一段中文源码注释说明" });
        assert(got.key === "fallback.noise" && got.text === "输出 2 行", `安全网没拦住夹在后面的行号源码:"${got.text}"(${got.key})`);
      } finally {
        delete table.__g4probe;
      }
    }
    // 英文计数单复数、参数列动作标签与结果列分开、线路不与行数撞义。
    {
      localStorageShim.setItem("kz-language", "en");
      try {
        const oneFile = sum("glob", { content: "src/a.rs" }).text;
        const oneLine = sum("bash", { content: "exit code: 0\n{x" }).text;
        const lines = sum("collaboration_status", { content: "x", display: { lines: [{}, {}] } }).text;
        const addArg = toolArgSummary("req", { action: "add", title: "T" }).text;
        const updateArg = toolArgSummary("req", { action: "update", id: "R-364" }).text;
        const added = sum("req", { content: "added R-365 [todo] T", input: { action: "add" } }).text;
        assert(oneFile === "1 file" && oneLine === "exit 0 · 1 line of output", `英文单数仍是复数:"${oneFile}" / "${oneLine}"`);
        assert(lines === "2 parallel lines", `英文线路计数与行数撞义:"${lines}"`);
        assert(addArg === "Add · T" && updateArg === "Update R-364" && added === "Added R-365", `参数列动作与结果列共用一个英文词:"${addArg}" / "${updateArg}" / "${added}"`);
      } finally {
        localStorageShim.setItem("kz-language", "zh");
      }
    }

    // ---------- 参数摘要 ----------
    const savedProject = shellNs.currentProject;
    const savedItems = shellNs.processItems;
    shellNs.setCurrentProject(ROOT);
    shellNs.setProcessItems([...(Array.isArray(savedItems) ? savedItems : []), { id: "w|g4", worktree_path: "D:\\wt\\line-a" }]);
    try {
      const readArg = toolArgSummary("read", { path: `${VROOT}\\crates\\x.rs` });
      assert(readArg.text === "crates/x.rs" && readArg.code, `项目根下的 verbatim 绝对路径未相对化:"${readArg.text}"`);
      const worktreeArg = toolArgSummary("edit", { path: "D:\\wt\\line-a\\src\\y.rs" });
      assert(worktreeArg.text === "src/y.rs", `线路工作树下的路径未相对化:"${worktreeArg.text}"`);
      const grepArg = toolArgSummary("grep", { pattern: "alpha|beta|gamma|delta|epsilon" });
      assert(grepArg.text === "alpha 等 5 项" && grepArg.code, `多分支正则未折叠成首分支 + 等 N 项:"${grepArg.text}"`);
      const bashArg = toolArgSummary("bash", { command: `cd "${ROOT}" && cargo test -p kanzei-app --lib -- tool_summary --nocapture --test-threads=1 and some more words here` });
      assert(bashArg.text.startsWith("cargo test -p kanzei-app") && bashArg.text.length <= 64 && !bashArg.text.includes("cd "), `命令未剥掉 cd 项目根前缀或超过 64 字:"${bashArg.text}"`);
      const fetchArg = toolArgSummary("webfetch", { url: "https://docs.rs/tokio/latest/tokio/?search=spawn#main" });
      assert(fetchArg.text === "docs.rs/tokio/latest/tokio/", `webfetch 参数未只留 host+path:"${fetchArg.text}"`);
      const taskArg = toolArgSummary("task", { prompt: "第一行任务说明\n第二行细节" });
      assert(taskArg.text === "第一行任务说明", `task 参数未取 prompt 首行:"${taskArg.text}"`);
      const questionArg = toolArgSummary("question", { question: "用哪个方案?" });
      assert(!questionArg.code, "question 参数是自然语言,不该标成代码记号");

      // ---------- 端到端(实时):tool-end 带 content/durationMs ----------
      const live = [
        ["G4L-READ", "read", { path: `${VROOT}\\crates\\x.rs` }, { content: `${readLines(1, 42)}\n` }, "全文 42 行"],
        ["G4L-GREP", "grep", { pattern: "fn" }, { content: "src/a.rs:3: fn one() {}\nsrc/a.rs-4- }\nsrc/b.rs:1: fn two() {}" }, "2 处匹配 · 2 个文件"],
        ["G4L-SYM", "symbols", { path: "src" }, { content: "== src/a.rs\n  pub fn one:1\n     fn two:3" }, "2 个符号"],
        ["G4L-BASH", "bash", { command: "cargo test" }, {
          content: "exit code: 0\nrunning 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored",
          display: { kind: "terminal", command: "cargo test", exitCode: 0, output: "running 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored", full: "running 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored" },
        }, "退出码 0 · 3 通过"],
        ["G4L-REQ", "req", { action: "update", id: "R-364" }, { content: "updated: R-364 [doing] 工具行摘要\n变更: 状态: todo → doing" }, "R-364 → doing"],
        ["G4L-TEST", "test_record", { title: "cargo test", status: "passed", summary: "3 passed; 0 failed" }, { content: "recorded T-1786922727068. active: 0, archived: 1 (path: p, archive: a)" }, "已记录 T-…7068 · 3/3 通过"],
      ];
      const liveText = new Map();
      for (const [id, name, input, end, expected] of live) {
        toolStart({ payload: { id, name, summary: name, input, sessionId: SID } });
        const lines = end.content.split("\n");
        const preview = `${lines[0]}${lines.length > 1 ? ` (+${lines.length - 1} lines)` : ""}`;
        toolEnd({ payload: { id, name, ok: true, outcome: "success", preview, display: null, ...end, contentTruncated: false, contentBytes: end.content.length, sessionId: SID } });
        await flush();
        const block = chatNs.chatToolBlocks.get(id);
        const text = block?.result.textContent ?? "";
        liveText.set(id, text);
        assert(text === `⎿ ${expected}`, `实时 ${name} 的 ⎿ 行不是人话摘要:期望 "⎿ ${expected}",实得 "${text}"`);
        assert(!noiseOf(text).length, `实时 ${name} 的 ⎿ 行仍含噪声:"${text}"`);
      }
      assert(chatNs.chatToolBlocks.get("G4L-READ")?.head.querySelector(".tool-msg-arg")?.textContent === "(crates/x.rs)", "实时 read 的参数列未把 verbatim 路径相对化");
      assert(chatNs.chatToolBlocks.get("G4L-READ")?.head.querySelector(".tool-msg-arg")?.classList.contains("is-code"), "路径参数未标 is-code(等宽只给代码记号)");
      // 耗时:durationMs=12300 → 「· 12.3s」紧跟退出码。
      toolStart({ payload: { id: "G4L-DUR", name: "bash", summary: "cargo test", input: { command: "cargo test" }, sessionId: SID } });
      toolEnd({ payload: { id: "G4L-DUR", name: "bash", ok: true, outcome: "success", preview: "exit code: 0 (+2 lines)", content: "exit code: 0\nrunning 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored", durationMs: 12300, display: null, sessionId: SID } });
      await flush();
      assert(chatNs.chatToolBlocks.get("G4L-DUR")?.result.textContent === "⎿ 退出码 0 · 12.3s · 3 通过", `实时 bash 耗时未紧跟退出码:"${chatNs.chatToolBlocks.get("G4L-DUR")?.result.textContent}"`);
      assert(chatNs.chatToolBlocks.get("G4L-DUR")?.result.querySelector(".tool-sum-dur")?.textContent === "12.3s", "耗时未渲染成 .tool-sum-dur");
      // 活动面板进度行与主对话同一个摘要器(耗时在元信息行,不重复)。
      const bgBash = [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === "G4L-BASH");
      assert(bgBash?.querySelector(".bg-prog")?.textContent === "退出码 0 · 3 通过", `活动面板 bash 进度行不是摘要:"${bgBash?.querySelector(".bg-prog")?.textContent}"`);
      const bgDur = [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === "G4L-DUR");
      assert(bgDur?.querySelector(".bg-meta")?.textContent.includes("12.3s"), `活动面板耗时未优先用后端 durationMs:"${bgDur?.querySelector(".bg-meta")?.textContent}"`);

      // ---------- 同源一致性:同一组夹具走历史回放,⎿ 行逐条相等 ----------
      const historyItems = [{ role: "assistant", parts: live.flatMap(([id, name, input, end]) => [
        { type: "tool_call", id: `H${id}`, name, input },
        { type: "tool_result", call_id: `H${id}`, is_error: false, content: end.content },
      ]) }];
      historyItems[0].parts.push(
        { type: "tool_call", id: "HG4-NOOP", name: "edit", input: { path: "ui/x.js", old_string: "a", new_string: "a" } },
        { type: "tool_result", call_id: "HG4-NOOP", is_error: true, content: "[tool_outcome=noop code=EDIT_IDENTICAL_INPUT]\nold_string 与 new_string 相同,无需修改" },
        { type: "tool_call", id: "HG4-QUOTA", name: "process", input: { action: "list" } },
        { type: "tool_result", call_id: "HG4-QUOTA", is_error: false, content: `[tool_result_truncated reason=artifact_quota_exceeded bytes=3145728 storage_used_bytes=2040109466 quota_bytes=2147483648 sha256=${"b".repeat(64)}]\n工具结果存储已达配额。仅保留头 32 KiB 与尾 32 KiB。\nbg1 running` },
      );
      viewsNs.renderMessageParts(historyItems);
      await flush();
      const historyBlock = (id) => [...document.querySelectorAll("#messages [data-active] .tool-msg")].find((n) => n.dataset.toolCallId === id);
      for (const [id] of live) {
        const text = historyBlock(`H${id}`)?.querySelector(".tool-msg-result")?.textContent ?? "";
        assert(text === liveText.get(id), `实时与历史回放的 ⎿ 行不一致(${id}):实时 "${liveText.get(id)}" vs 历史 "${text}"`);
      }
      const noopBlock = historyBlock("HG4-NOOP");
      assert(noopBlock?.classList.contains("noop") && noopBlock?.querySelector(".tool-msg-status")?.textContent === "↪", "历史 [tool_outcome=noop] 前缀未恢复 noop 终态(仍画成失败)");
      assert(noopBlock?.querySelector(".tool-msg-result")?.textContent === "⎿ 无需修改", `历史 noop 的 ⎿ 行漂移:"${noopBlock?.querySelector(".tool-msg-result")?.textContent}"`);
      const quotaBlock = historyBlock("HG4-QUOTA");
      assert(quotaBlock?.querySelector(".tool-msg-result")?.textContent.includes("工具结果存储已满"), `历史配额截断标记未合成与实时相同的人话 ⎿ 行:"${quotaBlock?.querySelector(".tool-msg-result")?.textContent}"`);
      assert(quotaBlock?.querySelector(".quota-notice"), "历史配额截断标记未合成配额提示块");
      // 轨迹里的耗时回填到历史 bash 行。
      const applied = chatNs.applyRecoveredToolDurations([{ events: [{ id: "HG4L-BASH", kind: "tool.completed", ok: true, durationMs: 12300 }] }]);
      assert(applied === 1 && historyBlock("HG4L-BASH")?.querySelector(".tool-msg-result")?.textContent === "⎿ 退出码 0 · 12.3s · 3 通过",
        `applyRecoveredToolDurations 未给历史 bash 行补上耗时:"${historyBlock("HG4L-BASH")?.querySelector(".tool-msg-result")?.textContent}"`);
      assert(/renderRecoveredTraces\(traces\);\r?\n\s*applyRecoveredToolDurations\(traces\);/.test(source), "loadConversation 未在轨迹回放后回填工具行耗时");

      // ---------- 旧后端兼容:tool-end 只有 preview ----------
      const legacy = [
        ["G4O-READ", "read", { path: "x.rs" }, "     1\tuse std::io; (+41 lines)", "全文 42 行"],
        ["G4O-GREP", "grep", { pattern: "x" }, "src/a.rs-450- } (+12 lines)", "13 条结果"],
        ["G4O-SYM", "symbols", { path: "src" }, `== ${VROOT}\\src\\lib.rs (+20 lines)`, "21 行"],
      ];
      for (const [id, name, input, preview, expected] of legacy) {
        toolStart({ payload: { id, name, summary: name, input, sessionId: SID } });
        toolEnd({ payload: { id, name, ok: true, preview, display: null, sessionId: SID } });
        await flush();
        const text = chatNs.chatToolBlocks.get(id)?.result.textContent ?? "";
        assert(text === `⎿ ${expected}`, `旧后端(只有 preview)的 ${name} 未走降级摘要:"${text}"`);
        assert(!noiseOf(text).length && !text.includes("use std"), `旧后端 ${name} 仍回显首行源码/路径:"${text}"`);
      }

      // ---------- 权限卡点「拒绝」:后端直发的 ToolEnd 带 content:"",⎿ 行与活动面板都要说出原因 ----------
      toolStart({ payload: { id: "G4L-DECL", name: "bash", summary: "rm -rf target", input: { command: "rm -rf target" }, sessionId: SID } });
      toolEnd({ payload: { id: "G4L-DECL", name: "bash", ok: false, outcome: "failed", code: "USER_DECLINED", preview: "(user declined)", content: "", contentBytes: 0, contentTruncated: false, display: null, sessionId: SID } });
      await flush();
      assert(chatNs.chatToolBlocks.get("G4L-DECL")?.result.textContent === "⎿ 已拒绝", `用户拒绝的 bash ⎿ 行没说出原因:"${chatNs.chatToolBlocks.get("G4L-DECL")?.result.textContent}"`);
      const bgDecl = [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === "G4L-DECL");
      assert(bgDecl?.querySelector(".bg-prog")?.textContent === "已拒绝", `活动面板里用户拒绝的 bash 进度行没说出原因:"${bgDecl?.querySelector(".bg-prog")?.textContent}"`);
      // 历史轨迹回放的失败行与主对话同一个摘要器(不再是 `exit code: 101 (+42 lines)` 原文)。
      const activityNs = esmModuleCache.get("06-activity.js")?.namespace;
      activityNs?.renderRecoveredTraces([{ events: [
        { id: "G4T-FAIL", kind: "tool.started", name: "bash", summary: "cargo test" },
        { id: "G4T-FAIL", kind: "tool.completed", name: "bash", ok: false, outcome: "failed", preview: "exit code: 101 (+42 lines)", error: "exit code: 101 (+42 lines)" },
        { id: "G4T-DECL", kind: "tool.started", name: "write", summary: "x.txt" },
        { id: "G4T-DECL", kind: "tool.completed", name: "write", ok: false, outcome: "failed", code: "USER_DECLINED", preview: "(user declined)", error: "(user declined)" },
        { id: "G4T-READ", kind: "tool.started", name: "read", summary: "crates/x.rs" },
        { id: "G4T-READ", kind: "tool.completed", name: "read", ok: false, outcome: "failed", code: "READ_PATH_NOT_FOUND", preview: `path not found: ${ROOT}\\crates\\x.rs (+1 lines)`, error: `path not found: ${ROOT}\\crates\\x.rs (+1 lines)` },
      ] }]);
      const traceProg = (id) => [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === id)?.querySelector(".bg-prog")?.textContent;
      assert(traceProg("G4T-FAIL") === "退出码 101 · 输出 42 行", `历史轨迹失败行仍贴 preview 原文:"${traceProg("G4T-FAIL")}"`);
      assert(traceProg("G4T-DECL") === "已拒绝", `历史轨迹里用户拒绝的 write 没说出原因:"${traceProg("G4T-DECL")}"`);
      assert(traceProg("G4T-READ") === "路径不存在 · crates/x.rs", `历史轨迹的 read 失败行未从报错首行取出相对路径:"${traceProg("G4T-READ")}"`);

      // ---------- 子代理子行:参数与结果都不贴 JSON 片段 ----------
      toolStart({ payload: { id: "G4_SCOUT", name: "task", summary: "G4_SCOUT · 勘察", input: { prompt: "x", phase: "scouting", role: "G4_SCOUT" }, sessionId: SID } });
      taskProgress({ payload: { id: "G4_SCOUT", text: "第 1/3 轮", trace: { child_id: "g1", phase: "start", name: "bash", summary: '{"command":"cargo test -p kanzei-app --lib","workdir":"C:\\\\Users\\\\kanzei\\\\Documents\\\\kanzei cod' }, sessionId: SID } });
      taskProgress({ payload: { id: "G4_SCOUT", text: "第 1/3 轮", trace: { child_id: "g1", phase: "end", name: "bash", ok: true, preview: "exit code: 0 (+5 lines)" }, sessionId: SID } });
      taskProgress({ payload: { id: "G4_SCOUT", text: "第 2/3 轮", trace: { child_id: "g2", phase: "start", name: "read", summary: "{\"path\":\"crates/x.rs\"}" }, sessionId: SID } });
      taskProgress({ payload: { id: "G4_SCOUT", text: "第 2/3 轮", trace: { child_id: "g2", phase: "end", name: "read", ok: true, preview: "     1\tuse std; (+9 lines)" }, sessionId: SID } });
      await flush();
      const scoutEntry = [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === "G4_SCOUT");
      const childHeads = [...(scoutEntry?.querySelectorAll(".bg-child-head") ?? [])].map((n) => n.textContent);
      const childMetas = [...(scoutEntry?.querySelectorAll(".bg-child-meta") ?? [])].map((n) => n.textContent);
      assert(childHeads[0] === "bash cargo test -p kanzei-app --lib" && childHeads[1] === "read crates/x.rs", `子代理子行参数仍是后端入参 JSON:${JSON.stringify(childHeads)}`);
      assert(childHeads.every((head) => !head.includes('{"')), `子代理子行 head 含 JSON 片段:${JSON.stringify(childHeads)}`);
      assert(childMetas[0] === "退出码 0 · 输出 5 行" && childMetas[1] === "全文 10 行", `子代理子行结果未走摘要器降级口径:${JSON.stringify(childMetas)}`);
      toolEnd({ payload: { id: "G4_SCOUT", name: "task", ok: true, preview: "勘察完成", display: null, sessionId: SID } });
      await flush();

      // ---------- 内存约束:正文不挂到块/DOM 上 ----------
      const bigContent = readLines(1, 9000).slice(0, 256 * 1024);
      toolStart({ payload: { id: "G4L-BIG", name: "read", summary: "big.rs", input: { path: "big.rs" }, sessionId: SID } });
      toolEnd({ payload: { id: "G4L-BIG", name: "read", ok: true, preview: "     1\tlet v1 = 1; (+8999 lines)", content: bigContent, contentBytes: bigContent.length, contentTruncated: false, display: null, sessionId: SID } });
      await flush();
      const bigBlock = chatNs.chatToolBlocks.get("G4L-BIG");
      assert(bigBlock && !("content" in bigBlock) && !("content" in bigBlock.wrap), "工具块或 DOM 节点上挂了 content(200 块 × 256 KiB 会吃掉 50 MB)");
      const longProps = Object.entries(bigBlock ?? {}).filter(([, value]) => typeof value === "string" && value.length > 9000).map(([key]) => key);
      assert(!longProps.length, `工具块上留了超长字符串属性:${longProps.join(", ")}`);
      assert((bigBlock?.wrap.textContent.length ?? 0) < 9000, `256 KiB 正文进了 DOM(wrap 文本 ${bigBlock?.wrap.textContent.length} 字,上限 9000)`);
      // 入参已渲染进展开区,收尾后块上不再留原始入参(块经 wrap._kzToolBlock 与 DOM 同寿命)。
      assert(bigBlock?.input === null, "工具块收尾后仍持有原始入参(大 write/edit 的正文会跟着 DOM 活很久)");
    } finally {
      shellNs.setCurrentProject(savedProject);
      shellNs.setProcessItems(savedItems);
    }

    // ---------- CSS:摘要比例字体,代码记号等宽,只用 token ----------
    const rootBlock = style.match(/:root\s*\{([\s\S]*?)\}/)?.[1] ?? "";
    assert(/--sans:/.test(rootBlock), ":root 缺 --sans token");
    assert(/\.tool-msg-head\s*\{[^}]*font-family:\s*var\(--sans\)/.test(style), ".tool-msg-head 仍用等宽(人话摘要读起来像日志)");
    assert(/\.tool-msg-result\s*\{[^}]*font-family:\s*var\(--sans\)/.test(style), ".tool-msg-result 未改用比例字体");
    assert(/\.tool-msg-arg\.is-code\s*\{[^}]*font-family:\s*var\(--mono\)/.test(style), ".tool-msg-arg.is-code 未用等宽");
    for (const cls of ["code", "add", "del", "dur"]) {
      const rule = style.match(new RegExp(`\\.tool-sum-${cls}\\s*\\{([^}]*)\\}`))?.[1];
      assert(rule !== undefined, `缺少 .tool-sum-${cls} 样式`);
      assert(rule === undefined || (!/#[0-9a-fA-F]{3,8}\b|rgba?\(/.test(rule) && !/opacity/.test(rule)), `.tool-sum-${cls} 用了字面量颜色或 opacity(只准用 token)`);
    }
    if (priorLanguage === null) localStorageShim.removeItem?.("kz-language");
    else localStorageShim.setItem("kz-language", priorLanguage);
  }
}

// ---------- UI-0926 #10:结构化渲染(04-structured.js)与各调用点 ----------
// 机器格式(工具 JSON 结果、bash 权限资源 JSON、tracker 字段里的①②③/||/JSON、provider 错误体、
// 局部校验明细)此前原样贴给人看。这里先对渲染器做 DOM 单元断言,再走真实事件/回放路径断言
// 各调用点。harness 的 innerHTML 只做去标签近似,所以渲染器必须纯 DOM(下面也机械检查)。
{
  const svNs = esmModuleCache.get("04-structured.js")?.namespace;
  const chatNs = esmModuleCache.get("05-chat-render.js")?.namespace;
  const eventsNs = esmModuleCache.get("07-events.js")?.namespace;
  const sessionsNs = esmModuleCache.get("09-sessions.js")?.namespace;
  const settingsNs = esmModuleCache.get("16-settings.js")?.namespace;
  const shellNs = esmModuleCache.get("03-shell.js")?.namespace;
  const viewsNs = esmModuleCache.get("15-views-misc.js")?.namespace;
  const activityNs = esmModuleCache.get("06-activity.js")?.namespace;
  const svExports = ["richText", "renderKV", "renderJsonTree", "renderValue", "renderToolArgs", "renderToolResult", "renderTrackerFields", "renderSearchResults", "renderPermissionResource", "renderErrorDetail", "renderLocalValidation", "lazyMount", "flushLazy", "structuredNav", "setStructuredNav"];
  const missingSv = svExports.filter((name) => !svNs?.[name]);
  assert(!missingSv.length, `04-structured.js 缺少导出:${missingSv.join(", ")}`);
  const svSource = sources[scriptSrcs.indexOf("04-structured.js")] ?? "";
  const innerHtmlWrites = svSource.match(/\.innerHTML\s*=/g) ?? [];
  assert(innerHtmlWrites.length === 1 && /box\.innerHTML = renderMarkdown\(/.test(svSource), `04-structured.js 只准把 renderMarkdown 的输出写进 innerHTML,实得 ${innerHtmlWrites.length} 处写入`);
  assert(!/createDocumentFragment/.test(svSource), "04-structured.js 不得用 DocumentFragment(冒烟 harness 没有,真机与冒烟会分叉)");
  assert(activityNs?.highlightLine === svNs?.highlightLine, "06-activity.js 的 highlightLine 应转出 04-structured.js 的同一实现(唯一真源)");
  const summarySource = sources[scriptSrcs.indexOf("05-tool-summary.js")] ?? "";
  assert(/mismatchFacts/.test(summarySource) && !/function summarizeJsonResult/.test(svSource), "JSON 工具的 ⎿ 摘要只能在 05-tool-summary.js 一处(04-structured.js 不得另写一套)");

  if (svNs && chatNs && eventsNs && sessionsNs && settingsNs && shellNs && viewsNs && !missingSv.length) {
    const priorLanguage = localStorageShim.getItem("kz-language");
    localStorageShim.setItem("kz-language", "zh");
    const SID = shellNs.activeSessionId || "sess-smoke";
    handlers.get("kz:turn")?.({ payload: { sessionId: SID, step: 3, maxSteps: 12 } });
    await flush();
    const toolStart = handlers.get("kz:tool-start");
    const toolEnd = handlers.get("kz:tool-end");

    // ---------- 真实导航注册(19-research.js):docs 下的 markdown 进应用内查看器 ----------
    assert(svNs.structuredNav.openRef.name === "openStructuredRef" && svNs.structuredNav.openPath.name === "openStructuredPath",
      "structuredNav 未注册真实跳转(chip 点了没反应)");
    {
      const before = invokeArgs.length;
      await svNs.structuredNav.openPath("docs/design/memory_system.md", null);
      await flush();
      const call = invokeArgs.slice(before).find((entry) => entry.cmd === "docs_read_custom");
      assert(call?.args?.relPath === "docs/design/memory_system.md", `路径 chip 打开 docs 下的设计文档未走 docs_read_custom:${JSON.stringify(call)}`);
      assert(!byId.get("viewer-overlay")?.classList.contains("hidden") && listText("viewer-title") === "memory_system.md", "设计文档未在应用内查看器打开");
      byId.get("viewer-overlay")?.classList.add("hidden");
    }

    const navCalls = [];
    const savedNav = { ...svNs.structuredNav };
    svNs.setStructuredNav({
      openRef: (id) => navCalls.push(["ref", id]),
      openPath: (path, line) => navCalls.push(["path", path, line]),
      openUrl: (url) => navCalls.push(["url", url]),
      openMemory: (scope, id) => navCalls.push(["memory", scope, id]),
    });
    const lastNav = () => navCalls[navCalls.length - 1] ?? [];
    try {
      // ---------- 单元:JSON 树 ----------
      {
        const tree = svNs.renderJsonTree({ a: { b: "x\ny" } });
        const block = tree.querySelector(".sv-json-row pre.sv-str-block");
        assert(block?.textContent === "x\ny", `多行字符串未显示真实换行:${JSON.stringify(block?.textContent)}`);
        assert(!tree.querySelector(".sv-json-row")?.textContent.includes("\\n"), "JSON 树里的字符串又被转义成字面的 \\n");
        assert(tree.querySelector(".sv-copy-json") && tree.querySelector("details.sv-raw"), "JSON 树缺少「复制 JSON / 原始 JSON」工具条");
        const long = svNs.renderJsonTree(Array.from({ length: 130 }, (_, i) => i), { maxItems: 100 });
        const more = long.querySelector(".sv-more");
        assert(more?.textContent === "还有 30 项", `数组超过 maxItems 未给「还有 N 项」:${more?.textContent}`);
        more?.click();
        assert(long.querySelectorAll(".sv-num").length === 130 && !long.querySelector(".sv-more"), "点「还有 N 项」后未补齐剩余项");
      }
      // ---------- 单元:tracker 字段只读视图(G5 接进单页详情前的单测) ----------
      {
        const discovery = JSON.stringify({ Intent: "编辑后先做局部结构校验", Explicit: "按文件类型选择 check", Assumptions: "复用已有 parser", Ambiguities: "验证器矩阵待勘察", 领域对象: "changed region", 最小成功闭环: "三类 edit 路径", 延后决策: "完整语言矩阵" });
        const fields = [
          ["优先级", "P1"], ["复杂度", "大"], ["标签", "架构 流程 自举"], ["批次", "3/5"],
          ["来源", "2026-08-17 用户确认「自举一期应该差不多可以算结束」"],
          ["refs", "R-221 R-276 docs/design/phase2_system_upgrade.md"],
          ["内容", "以 docs/design/phase2_system_upgrade.md 为二期真源维护五批:批1 设计/依赖/需求映射;批2 P0 事实恢复(D-409 与 memory backlog);批3 research+memory 引擎 E2"],
          ["验收", "①所有二期子条目有明确依赖;②Wave 0～4 各有 Go/No-Go 记录;③联合闭环按 session/topic/memory id 可回溯;④二期结项时无相互矛盾状态。"],
          ["进展", "R-101 B3 已提交 d1cc0006 || 2026-08-20 B3 收口:真实执行 -RunStopTest 通过"],
          ["停车", "排队:排在 R-340 之后;恢复人:agent;解除条件:R-340"],
          ["发现记录", discovery],
          ["observed_head", "148386f3d467b701f334932b2bfc85bbcfcea475"],
          ["observed_worktree_hash", "fnv1a64:cbf29ce484222325"],
          ["recorded_at", "1786925390809"],
        ];
        const tf = svNs.renderTrackerFields(fields);
        const row = (key) => [...tf.querySelectorAll(".tf-row")].find((node) => node.dataset.field === key);
        assert(row("验收")?.querySelectorAll("ol li").length === 4, "验收 ①②③④ 未渲染成 4 项有序列表");
        assert(row("发现记录")?.querySelectorAll(".sv-kv-row").length === 7, "发现记录 JSON 未渲染成 7 行键值表");
        assert([...(row("发现记录")?.querySelectorAll(".sv-kv-row") ?? [])].find((node) => node.dataset.key === "Explicit")?.querySelector(".sv-k")?.textContent === "用户原话",
          "发现记录英文键未映射成中文标签");
        const release = row("停车")?.querySelector(".tf-release .sv-ref");
        assert(release?.textContent === "R-340", "停车的解除条件未做成可点的条目 chip");
        release?.click();
        assert(lastNav()[0] === "ref" && lastNav()[1] === "R-340", `点解除条件 chip 未跳转条目:${JSON.stringify(lastNav())}`);
        assert(row("停车")?.querySelector(".tf-owner")?.textContent === "恢复人: agent", "停车的恢复人未单列成 chip");
        assert(!row("停车")?.querySelector(".tf-cond-reason")?.textContent.includes("恢复人"), "停车原因里仍混着恢复人/解除条件");
        const docChip = [...(row("refs")?.querySelectorAll(".sv-path") ?? [])].find((node) => node.dataset.path === "docs/design/phase2_system_upgrade.md");
        assert(docChip && row("refs")?.querySelectorAll(".sv-ref").length === 2, "refs 里的条目编号/设计文档路径未分别做成 chip");
        docChip?.click();
        assert(lastNav()[0] === "path" && lastNav()[1] === "docs/design/phase2_system_upgrade.md", `refs 里的文档路径被当成条目跳转(死链):${JSON.stringify(lastNav())}`);
        assert(!row("observed_head") && !row("recorded_at"), "引擎字段不该作为普通字段行出现");
        const engine = tf.querySelector(".tf-engine");
        assert(engine && [...engine.querySelectorAll(".tf-engine-item")].find((node) => node.dataset.field === "observed_head")?.textContent === "148386f3", "引擎字段未收进「引擎记录」折叠区(head 取前 8 位)");
        const progress = row("进展")?.querySelector(".tf-timeline");
        assert(progress?.children.length === 2 && progress.children[1].querySelector(".tf-date")?.textContent === "2026-08-20", "进展的 || 分段未渲染成两段时间线");
        const content = row("内容");
        assert(content?.querySelectorAll(".tf-marked li").length === 3 && content.querySelector(".tf-mark")?.textContent === "批1", "内容的「批N」未切成带标签的列表");
        const meta = tf.querySelector(".tf-meta");
        assert(meta && [...meta.querySelectorAll(".tf-meta-item")].find((node) => node.dataset.field === "标签")?.querySelectorAll(".tf-tag").length === 3, "标签未按空白拆成 chip");
        assert(meta?.querySelector(".tf-progress-fill")?.style.getPropertyValue("--tf-progress") === "60%", "批次 3/5 未带进度条");
        assert(!row("优先级"), "元数据字段不该再占一行");
        // 卡片紧凑态:只渲染进展/验收/复现/内容/影响的前 3 个,行带 doc-field,列表只露前 3 项。
        const compact = svNs.renderTrackerFields(fields, { compact: true });
        const compactRows = [...compact.querySelectorAll(".tf-row")];
        assert(compactRows.map((node) => node.dataset.field).join(",") === "进展,验收,内容" && compactRows.every((node) => node.classList.contains("doc-field")),
          `紧凑态字段选择错误:${compactRows.map((node) => node.dataset.field).join(",")}`);
        const compactAccept = compactRows.find((node) => node.dataset.field === "验收");
        assert(compactAccept?.querySelectorAll("li").length === 3 && compactAccept.querySelector(".tf-more")?.textContent === "+1", "紧凑态列表未截到 3 项 +N");
        assert(!compact.querySelector(".tf-engine") && !compact.querySelector(".tf-meta"), "紧凑态不该渲染引擎记录/元数据");
        // 四种字段形状都吃;unknown 字段标灰;数据里的 HTML 只当文本。
        for (const shape of [[{ key: "验收", value: "①第一项内容;②第二项内容" }], { 验收: "①第一项内容;②第二项内容" }, [{ name: "验收", value: "①第一项内容;②第二项内容", known: true }]]) {
          assert(svNs.renderTrackerFields(shape).querySelectorAll("ol li").length === 2, `字段形状 ${JSON.stringify(shape).slice(0, 40)} 未被识别`);
        }
        assert(svNs.renderTrackerFields([{ name: "历史自定义", value: "x", known: false }]).querySelector(".tf-row.tf-unknown"), "未知字段未标灰");
        const xss = svNs.renderTrackerFields([["内容", '<img src=x onerror=alert(1)> [x](javascript:alert(1))']]);
        assert(!xss.querySelectorAll("img").length && xss.textContent.includes("<img src=x"), "tracker 字段里的 HTML 被当成标记渲染(XSS)");
      }
      // ---------- 单元:工具入参键值表 ----------
      {
        const args = svNs.renderToolArgs("task", { prompt: "## 任务\n- 第一步\n- 第二步", role: "scout" }, { className: "tool-msg-raw args" });
        assert(args?.classList.contains("sv-args") && args.classList.contains("tool-msg-raw"), "入参键值表未带 sv-args/调用方类名");
        const prompt = [...args.querySelectorAll(".sv-kv-row")].find((node) => node.dataset.key === "prompt");
        assert(prompt?.querySelector("details.sv-prose .sv-md"), "多行 prompt 未折叠成 markdown");
        assert(!args.textContent.includes("\\n"), "入参里仍有字面的 \\n");
        assert(JSON.parse(args.dataset.raw).prompt === "## 任务\n- 第一步\n- 第二步", "入参根节点未保留完整 JSON(dataset.raw)");
        const editArgs = svNs.renderToolArgs("edit", { path: "ui/x.js", old_string: "a\nb", new_string: "c", replace_all: false }, { display: { kind: "diff" } });
        const shownKeys = [...editArgs.querySelectorAll(".sv-kv-row")].filter((node) => !node.closest(".sv-raw-args")).map((node) => node.dataset.key);
        assert(shownKeys.join(",") === "path,replace_all", `edit 已有 diff 时外面只该露 path 等:${shownKeys.join(",")}`);
        assert([...editArgs.querySelectorAll(".sv-raw-args .sv-kv-row")].some((node) => node.dataset.key === "old_string"), "old_string 未收进「原始入参」");
        assert(editArgs.querySelector(".sv-path")?.textContent === "ui/x.js", "path 未做成路径 chip");
        assert(svNs.renderToolArgs("bash", { command: "cargo test", workdir: "." })?.querySelector(".sv-cmd")?.textContent === "cargo test", "command 未渲染成命令代码块");
        assert(svNs.renderToolArgs("read", {}) === null, "空入参应返回 null(不出空框)");
        // 大入参(write 的整份正文)已逐键渲染,dataset.raw 不再在 DOM 属性里整份再存一遍。
        const bigArgs = svNs.renderToolArgs("write", { path: "x.txt", content: "x".repeat(20000) });
        assert(bigArgs && bigArgs.dataset.raw.length <= 8002, `大入参的 dataset.raw 未截断:${bigArgs?.dataset.raw.length} 字`);
      }
      // ---------- 单元:路径 chip——带空格的项目根、跳转目标只对当前项目相对化 ----------
      {
        const savedProject = shellNs.currentProject;
        const savedItems = shellNs.processItems;
        const ROOT = "C:\\Users\\kanzei\\Documents\\kanzei code";
        shellNs.setCurrentProject(ROOT);
        shellNs.setProcessItems([...(Array.isArray(savedItems) ? savedItems : []), { id: "w|g4p", worktree_path: "D:\\wt\\line-a" }]);
        try {
          // 根带空格:通用路径正则在空格处断开,会切出 `code\crates\a.rs:12` 这种错 chip。
          const rich = svNs.richText(`见 ${ROOT}\\crates\\a.rs:12 与 R-12`);
          const chip = rich.querySelector(".sv-path");
          assert(chip?.dataset.path === "crates/a.rs" && chip.textContent === "crates/a.rs:12", `带空格项目根下的绝对路径被切成错 chip:${chip?.dataset.path} / ${chip?.textContent}`);
          assert(rich.textContent === "见 crates/a.rs:12 与 R-12" && rich.querySelector(".sv-ref")?.textContent === "R-12", `富文本拼接走样:"${rich.textContent}"`);
          chip?.click();
          assert(lastNav()[0] === "path" && lastNav()[1] === "crates/a.rs" && lastNav()[2] === 12, `带空格根下的路径 chip 点击目标错误:${JSON.stringify(lastNav())}`);
          const dirText = svNs.richText(`cwd ${ROOT}\\crates\\kanzei-app done`);
          assert(!dirText.querySelector(".sv-path") && dirText.textContent.includes("kanzei code\\crates\\kanzei-app"), "项目根下的目录被切成了路径 chip");
          // 显示可以相对化/`~/` 缩写,跳转目标只有当前项目下才相对化。
          const outside = svNs.pathChip("C:\\Users\\kanzei\\Other\\x.rs");
          assert(outside.textContent === "~/Other/x.rs" && outside.dataset.path === "C:/Users/kanzei/Other/x.rs", `项目外路径的跳转目标是缩写:${outside.dataset.path}`);
          const worktree = svNs.pathChip("D:\\wt\\line-a\\src\\y.rs");
          assert(worktree.textContent === "src/y.rs" && worktree.dataset.path === "D:/wt/line-a/src/y.rs", `线路工作树路径被相对化成主项目路径(会打开同名文件):${worktree.dataset.path}`);
          const inProject = svNs.pathChip(`\\\\?\\${ROOT}\\ui\\x.js`);
          assert(inProject.dataset.path === "ui/x.js" && inProject.textContent === "ui/x.js", `项目内 verbatim 路径的跳转目标未相对化:${inProject.dataset.path}`);
        } finally {
          shellNs.setCurrentProject(savedProject);
          shellNs.setProcessItems(savedItems);
        }
      }
      // ---------- 单元:权限资源 / 错误详情 / 局部校验 ----------
      {
        const perm = svNs.renderPermissionResource("bash", JSON.stringify({ command: "cargo test --workspace", workdir: "C:/smoke/project" }));
        assert(perm.querySelector(".sv-cmd")?.textContent === "cargo test --workspace", "bash 资源未拆成命令代码块");
        assert(perm.querySelector(".sv-perm-workdir .sv-path")?.dataset.path === "C:/smoke/project", "bash 资源缺少工作目录 chip");
        assert(!perm.textContent.includes('{"command"'), "权限资源仍显示原始 JSON");
        const error = svNs.renderErrorDetail('provider returned HTTP 400: {"error":{"message":"bad param","type":"invalid_request_error"}}');
        assert(error.querySelector(".sv-error-message")?.textContent === "bad param", "provider 错误体未提取人话消息");
        assert(error.querySelector(".sv-error-status")?.textContent === "HTTP 400", "错误详情缺少 HTTP 状态 chip");
        assert(error.dataset.raw.startsWith("provider returned HTTP 400"), "错误详情未保留原文");
        const checks = svNs.renderLocalValidation({ checks: [{ kind: "node-check", status: "failed", command: "node --check ui/x.js", first_error: "ui/x.js:3 Unexpected token", repair_context: "> 3 | }" }, { kind: "eslint", status: "passed", command: "eslint ui/x.js" }] });
        assert(checks.querySelector(".sv-check.is-failed")?.textContent === "✗ node-check" && checks.querySelector(".sv-check.is-passed")?.textContent === "✓ eslint", "局部校验 chip 未按状态区分");
        const errorPath = checks.querySelector(".sv-check-error .sv-path");
        assert(errorPath?.textContent === "ui/x.js:3", `首个错误里的路径未做成可点 chip:${errorPath?.textContent}`);
        errorPath?.click();
        assert(lastNav()[0] === "path" && lastNav()[1] === "ui/x.js" && lastNav()[2] === 3, `首个错误路径点击未带行号打开:${JSON.stringify(lastNav())}`);
      }

      // ---------- 端到端(实时):tracker list JSON 结果 ----------
      {
        const listJson = JSON.stringify({ schema_version: 1, kind: "requirement", deadlocked: false, deadlock_guidance: null, entries: [
          { id: "R-1", title: "甲", lifecycle_status: "todo", blocked: false, block_reasons: [] },
          { id: "R-2", title: "乙", lifecycle_status: "todo", blocked: true, block_reasons: ["依赖 R-1 未完成"] },
          { id: "R-3", title: "丙", lifecycle_status: "doing", blocked: false, block_reasons: [] },
        ] }, null, 2);
        toolStart({ payload: { id: "G4S-LIST", name: "req", summary: "list", input: { action: "list" }, sessionId: SID } });
        toolEnd({ payload: { id: "G4S-LIST", name: "req", ok: true, outcome: "success", preview: `{ (+${listJson.split("\n").length - 1} lines)`, content: listJson, contentBytes: listJson.length, contentTruncated: false, display: null, sessionId: SID } });
        await flush();
        const block = chatNs.chatToolBlocks.get("G4S-LIST");
        assert(block?.result.textContent === "⎿ 3 条 · 2 条可执行", `tracker list 的 ⎿ 行不是摘要:"${block?.result.textContent}"`);
        assert(block?.detail.querySelector(".sv-lazy") && !block.detail.querySelector(".sv-result"), "JSON 结果应延迟到首次展开才构建");
        block?.head.click();
        assert(block?.detail.querySelector(".sv-result") && !block.detail.classList.contains("hidden"), "展开后没有 JSON 结果的结构化视图");
        assert(block?.detail.querySelectorAll(".sv-tl-row").length === 3, `tracker list 结果未渲染成条目行:${block?.detail.querySelectorAll(".sv-tl-row").length}`);
        assert(block?.detail.querySelector(".sv-tl-row.is-blocked .sv-tl-reason .sv-ref")?.textContent === "R-1", "阻塞原因里的条目编号未做成 chip");
        assert(![...(block?.detail.querySelectorAll(".tool-msg-raw") ?? [])].some((node) => !node.classList.contains("args")), "JSON 结果仍在展开区贴了一份原文");
        const resultView = block?.detail.querySelector(".sv-result");
        const visibleText = [...(resultView?.children ?? [])].filter((node) => !node.classList.contains("sv-json-tools")).map((node) => node.textContent).join("");
        assert(visibleText && !visibleText.includes('"lifecycle_status"'), "展开区的可见视图仍出现原始 JSON 键");
        assert(resultView?.querySelector(".sv-json-tools details.sv-raw"), "结构化视图之外缺少「原始 JSON」出口");
        const logLine = [...byId.get("log-lines").children].reverse().find((node) => node.textContent.includes("工具结果 req"));
        assert(logLine && !logLine.textContent.includes("{ (+") && logLine.textContent.includes("3 条"), `运行日志的工具结果行仍是 { (+N lines):${logLine?.textContent}`);
        // 原 #live-focus 工作焦点行已删(UI-0926 #4,与焦点卡重复);护栏意图不变:侧栏实时区不贴 JSON 首行。
        assert(!byId.has("live-focus") && !listText("live-status").includes("{ (+"), `侧栏实时区贴了 JSON 首行 preview:"${listText("live-status")}"`);
      }
      // ---------- 端到端(实时):edit + 局部校验 ----------
      {
        const content = "replaced 1 occurrence(s) in C:/smoke/project/ui/x.js\n局部结构校验发现 1 个精确错误，请先修复后再扩大回归\n局部校验明细:\n- node-check [failed] command: node --check ui/x.js\n  首个错误: ui/x.js:3 Unexpected token";
        toolStart({ payload: { id: "G4S-LV", name: "edit", summary: "ui/x.js", input: { path: "ui/x.js", old_string: "let a = 1;", new_string: "let a = ;" }, sessionId: SID } });
        toolEnd({ payload: { id: "G4S-LV", name: "edit", ok: true, outcome: "success", preview: "replaced 1 occurrence(s) in C:/smoke/project/ui/x.js (+4 lines)", content, contentBytes: content.length, contentTruncated: false, display: {
          kind: "diff", path: "ui/x.js", additions: 1, deletions: 1, language: "js",
          lines: [{ kind: "del", text: "let a = 1;", old_line: 3 }, { kind: "add", text: "let a = ;", new_line: 3 }],
          local_validation: { kind: "local_validation", checks: [{ kind: "node-check", status: "failed", command: "node --check ui/x.js", first_error: "ui/x.js:3 Unexpected token", repair_context: "> 3 | let a = ;" }], counts: { failed: 1, passed: 0 } },
        }, sessionId: SID } });
        await flush();
        const block = chatNs.chatToolBlocks.get("G4S-LV");
        assert(block?.result.textContent === "⎿ +1 −1 · 校验 1 个错误", `edit 校验失败的 ⎿ 行漂移:"${block?.result.textContent}"`);
        assert(block?.detail.querySelector(".sv-checks .sv-check.is-failed")?.textContent === "✗ node-check", "展开区缺少局部校验失败 chip");
        assert(block?.detail.querySelector(".sv-check-error .sv-path")?.textContent === "ui/x.js:3", "首个错误的路径未做成 chip");
        const rest = [...(block?.detail.querySelectorAll(".tool-msg-raw") ?? [])].find((node) => !node.classList.contains("args"));
        assert(!rest || !rest.textContent.includes("局部校验明细"), "局部校验明细在 chips 之外又贴了一遍原文");
        const args = block?.detail.querySelector(".tool-msg-raw.args.sv-args");
        assert(args && [...args.querySelectorAll(".sv-raw-args .sv-kv-row")].some((node) => node.dataset.key === "old_string"), "edit 带 diff 时 old_string 未收进「原始入参」");
        assert(args?.querySelector(".sv-kv-row")?.dataset.key === "path" && args.querySelector(".sv-path")?.textContent === "ui/x.js", "edit 入参的 path 未直接可见");
      }
      // ---------- 端到端(历史回放):websearch 紧凑 JSON、needs_correction 前缀、多行 prompt ----------
      {
        const webJson = JSON.stringify({ query: "rust ansi", results: [
          { title: "ANSI escape code", url: "https://en.wikipedia.org/wiki/ANSI_escape_code", snippet: "ANSI escape sequences are a standard for in-band signaling" },
          { title: "anstyle", url: "https://docs.rs/anstyle", snippet: "ANSI text styling" },
        ], truncated: false, prior_art_budget: null });
        viewsNs.renderMessageParts([{ role: "assistant", parts: [
          { type: "tool_call", id: "HG4S-WEB", name: "websearch", input: { query: "rust ansi" } },
          { type: "tool_result", call_id: "HG4S-WEB", is_error: false, content: webJson },
          { type: "tool_call", id: "HG4S-FIX", name: "edit", input: { path: "ui/x.js", old_string: "a", new_string: "b" } },
          { type: "tool_result", call_id: "HG4S-FIX", is_error: true, content: "[tool_outcome=needs_correction code=EDIT_ANCHOR_NOT_FOUND]\n请重读锚点" },
          { type: "tool_call", id: "HG4S-TASK", name: "task", input: { prompt: "第一行任务说明\n第二行细节\n- 列表项", role: "scout" } },
          { type: "tool_result", call_id: "HG4S-TASK", is_error: false, content: "## 结论\n- 完成" },
        ] }]);
        await flush();
        const historyBlock = (id) => [...document.querySelectorAll("#messages [data-active] .tool-msg")].find((node) => node.dataset.toolCallId === id);
        const web = historyBlock("HG4S-WEB");
        assert(web?.querySelector(".tool-msg-result")?.textContent === "⎿ 2 条结果 · ANSI escape code", `历史 websearch 的 ⎿ 行漂移:"${web?.querySelector(".tool-msg-result")?.textContent}"`);
        web?.querySelector(".tool-msg-head")?.click();
        assert(web?.querySelectorAll(".sv-search-result").length === 2, "历史 websearch 展开后未渲染成搜索结果列表");
        const title = web?.querySelector(".sv-search-title");
        title?.click();
        assert(lastNav()[0] === "url" && lastNav()[1] === "https://en.wikipedia.org/wiki/ANSI_escape_code", "搜索结果标题点击未走应用内打开");
        const fix = historyBlock("HG4S-FIX");
        assert(fix?.classList.contains("warn") && fix.querySelector(".tool-msg-result")?.textContent === "⎿ 请重读锚点", `历史 needs_correction 未恢复终态或 ⎿ 行带了机器头:"${fix?.querySelector(".tool-msg-result")?.textContent}"`);
        assert(fix?.querySelector(".tool-msg-detail .sv-code")?.textContent === "EDIT_ANCHOR_NOT_FOUND", "需要修正的块未在展开区给出稳定错误码 chip");
        const taskArgs = historyBlock("HG4S-TASK")?.querySelector(".tool-msg-raw.args.sv-args");
        assert(taskArgs?.querySelector("details.sv-prose"), "历史 task 的多行 prompt 未折叠");
        assert(taskArgs && !taskArgs.textContent.includes("\\n"), "历史入参里仍有字面的 \\n");
      }

      // ---------- 权限卡:bash 资源 JSON → 命令代码块 + 工作目录;队列预览不贴 JSON ----------
      {
        eventsNs.hideAsk();
        const askHandler = handlers.get("kz:ask");
        const resourceA = JSON.stringify({ command: "cargo test --workspace", workdir: "C:/smoke/project" });
        const resourceB = JSON.stringify({ command: "cargo fmt --all", workdir: "C:/smoke/project" });
        askHandler?.({ payload: { id: 9401, sessionId: SID, kind: "permission", action: "bash", resource: resourceA, remember: resourceA } });
        askHandler?.({ payload: { id: 9402, sessionId: SID, kind: "permission", action: "bash", resource: resourceB, remember: resourceB } });
        await flush();
        assert(byId.get("ask-resource")?.querySelector(".sv-cmd")?.textContent === "cargo test --workspace", `权限卡资源未拆成命令代码块:"${listText("ask-resource")}"`);
        assert(byId.get("ask-resource")?.querySelector(".sv-perm-workdir .sv-path"), "权限卡缺少工作目录 chip");
        assert(listText("ask-action") === "bash", "权限卡的操作一栏被改写");
        assert(listText("ask-remember") === "bash · 同上", `「记住为」与资源相同时应写「同上」:"${listText("ask-remember")}"`);
        assert(!listText("ask-queue-preview").includes('{"command"') && listText("ask-queue-preview").includes("bash · cargo fmt --all"), `队列预览仍贴原始 JSON:"${listText("ask-queue-preview")}"`);
        eventsNs.hideAsk();
        settingsNs.renderPermissionRules({ path: "C:/smoke/project/.kanzei/kanzei.toml", rules: [{ index: 0, action: "bash", resource: resourceA }] });
        const rulesBody = byId.get("permission-rules-table")?.querySelector("tbody");
        assert(rulesBody?.querySelector(".sv-cmd")?.textContent === "cargo test --workspace" && !rulesBody.textContent.includes('{"command"'), "设置页权限规则表仍显示原始 JSON 资源");
        assert(rulesBody?.querySelector(".icon-btn")?.getAttribute("aria-label") === "删除权限规则 bash · cargo test --workspace", "删除规则按钮的无障碍名称仍带原始 JSON");
        settingsNs.renderPermissionRules({ rules: [] });
        // 覆盖提示:标题 + 三列表(字段 | 本页 | 实际生效),不再是「；」拼成的一长行。
        settingsNs.renderEffectiveNotice({ primary: "deepseek:deepseek-chat", limits: { maxTokens: 8000 }, projectConfig: "C:/smoke/project/.kanzei/kanzei.toml",
          effective: { primary: "anthropic:claude", limits: { maxTokens: 4000 } } });
        const effectiveBox = byId.get("settings-effective");
        const heads = [...(effectiveBox?.querySelectorAll("table.sv-table th") ?? [])].map((node) => node.textContent);
        const cells = [...(effectiveBox?.querySelectorAll("table.sv-table tbody tr") ?? [])].map((row) => [...row.querySelectorAll("td")].map((node) => node.textContent).join("|"));
        assert(effectiveBox?.tagName === "DIV" && heads.join("|") === "字段|本页|实际生效", `覆盖提示不是三列表:${heads.join("|")}`);
        assert(cells.join(" / ") === "primary|deepseek:deepseek-chat|anthropic:claude / 运行上限|maxTokens 8000|maxTokens 4000", `覆盖提示的行内容不对:${cells.join(" / ")}`);
        assert(!effectiveBox?.textContent.includes("；"), "覆盖提示仍是「；」拼接的长句");
        settingsNs.renderEffectiveNotice({ effective: {} });
        assert(effectiveBox?.classList.contains("hidden") && !effectiveBox.querySelector("table"), "没有覆盖时提示未收起/未清空");
      }
      // ---------- 错误卡 + 运行日志 ----------
      {
        const message = 'provider returned HTTP 400: {"error":{"message":"bad param","type":"invalid_request_error"}}';
        chatNs.reportError(message);
        await flush();
        const cards = [...document.querySelectorAll("#messages .msg.error")];
        const card = cards[cards.length - 1];
        assert(card?.querySelector(".sv-error-message")?.textContent === "bad param", "错误卡未显示提取出的人话消息");
        assert(card?.querySelector(".sv-error-status")?.textContent === "HTTP 400", "错误卡缺少 HTTP 状态 chip");
        assert(card?.dataset.raw === message, "错误卡未保留原文(复制要原文)");
        assert(card?.querySelector(".error-level"), "错误卡的等级标签丢了");
        const logLine = byId.get("log-lines")?.children.at(-1);
        assert(logLine?.querySelector(".sv-log-error .sv-error-message")?.textContent === "bad param", "运行日志的错误行未附结构化详情");
      }
      // ---------- 测试记录:真实 {key,value} 字段,点开看结构化详情 ----------
      {
        sessionsNs.renderTestRuns({ active: [{ id: "T-1788804121000", title: "cargo 回归", status: "passed", refs: ["R-001"], fields: [
          { key: "命令", value: "cargo fmt --all -- --check; cargo test -p kanzei-tools" },
          { key: "收尾", value: "1788804121" },
          { key: "源码指纹", value: "v2 crates/kanzei-core/src/a.rs@63ae5885281c" },
        ] }], archived: [] });
        const entry = document.querySelector("#test-list .test-entry");
        assert(entry?.dataset.docId === "T-1788804121000", "测试记录行未挂 data-doc-id(T- 编号 chip 跳不过来)");
        assert(entry?.querySelector(".sv-test-head")?.title.includes("命令: cargo fmt"), "测试记录行的字段 tooltip 读错了字段形状");
        entry?.querySelector(".sv-test-head")?.click();
        const detail = entry?.querySelector(".sv-test-detail");
        assert(detail && !detail.classList.contains("hidden"), "点测试记录行头未展开详情");
        assert(detail?.querySelectorAll(".sv-cmd-list li").length === 2, "测试命令未按「; 」拆成 2 条");
        const finished = [...(detail?.querySelectorAll(".sv-kv-row") ?? [])].find((node) => node.dataset.key === "收尾")?.querySelector(".sv-num");
        assert(finished && finished.textContent !== "1788804121" && finished.title === "1788804121", "收尾时间戳未转成本地时间");
        assert(detail?.querySelector(".sv-path")?.dataset.path === "crates/kanzei-core/src/a.rs", "源码指纹未拆成路径 chip");
        assert(entry?.querySelector(".test-ref-chip")?.textContent === "R-001", "测试记录的关联徽标丢了");
        sessionsNs.renderTestRuns(payloads.test_runs_snapshot);
      }
      // ---------- 压缩纪要 / 对话总结 / 架构索引 / 研究运行卡 ----------
      {
        eventsNs.addCompactionEntry("## 压缩纪要\n- 保留了目标\n- 丢弃了噪声");
        const compaction = [...document.querySelectorAll(".compaction-entry")].at(-1);
        assert(compaction?.querySelector(".bg-detail.md")?.innerHTML.includes("<ul>"), "压缩纪要未按 markdown 渲染");
        compaction?.remove();
        const summary = eventsNs.addSummaryEntry("- 做了甲\n- 做了乙", "C:/smoke/project/.kanzei/summaries/s1.md");
        assert(summary?.querySelector(".bg-detail.md") && summary.querySelector(".sv-archived .sv-path"), "对话总结未按 markdown 渲染或存档路径不是 chip");
        summary?.remove();
        const archBody = byId.get("arch-index-body");
        assert(archBody?.classList.contains("md") && archBody.innerHTML.includes("<h3") && archBody.innerHTML.includes('class="md-path"') && !archBody.textContent.includes("### "),
          "架构索引仍以 markdown 原文显示(或文档链接不可点)");
        // 研究运行卡的 execution_json 断言在研究视图用例里(卡片只在选中研究主题时存在)。
      }
      // ---------- markdown 路径链接的点击委托 ----------
      {
        const link = document.createElement("a");
        link.className = "md-path";
        link.dataset.path = "crates/x.rs";
        link.dataset.line = "7";
        document.body.appendChild(link);
        document.dispatchEvent({ type: "click", target: link, preventDefault() {}, stopPropagation() {} });
        assert(lastNav()[0] === "path" && lastNav()[1] === "crates/x.rs" && lastNav()[2] === 7, `a.md-path 点击未委托到 structuredNav.openPath:${JSON.stringify(lastNav())}`);
        link.remove();
      }
      // ---------- 收活 diff:按文件计数 + 逐文件着色 diff ----------
      {
        const harvestRow = [...document.querySelectorAll(".harvest-diff-tree .diff-summary-row")].find((node) => node.dataset.path === "crates/branch.rs");
        assert(harvestRow?.textContent.includes("+1"), `收活 diff 树的增删计数仍是 +0/−0:"${harvestRow?.textContent}"`);
        assert([...document.querySelectorAll(".sv-diff-files details")].some((node) => node.dataset.path === "crates/branch.rs" && node.querySelector(".tool-display.diff")), "收活 diff 未按文件渲染着色差异");
      }
    } finally {
      svNs.setStructuredNav(savedNav);
    }

    // ---------- CSS:结构化渲染只用 token ----------
    const g4Css = style.split("/* ===== 分区:工具行与结构化渲染 ===== */")[1]?.split("/* ===== 分区:需求卡片与单页 ===== */")[0] ?? "";
    assert(/\.sv-kv\s*\{/.test(g4Css) && /\.tf-row\s*\{/.test(g4Css) && /a\.md-path\s*\{/.test(g4Css) && /\.ask-value\s*\{/.test(g4Css), "结构化渲染样式不在本组分区里");
    assert(!/#[0-9a-fA-F]{3,8}\b|rgba?\(/.test(g4Css.replace(/\/\*[\s\S]*?\*\//g, "")), "工具行与结构化渲染分区用了字面量颜色(只准用 token)");
    if (priorLanguage === null) localStorageShim.removeItem?.("kz-language");
    else localStorageShim.setItem("kz-language", priorLanguage);
  }
}

// ===== 分区:需求卡片与单页 =====
// ---------- UI-0926 #4(侧栏):焦点卡直达详情 / 页签与依赖视图 / 筛选放行 / 焦点区空态与线路头 /
// 签名跳过 / 任务卡一行 / 历史行 / 实时行 / 状态栏去重。设计见 scratchpad density.md M1–M4。
{
  const shellNs = esmModuleCache.get("03-shell.js")?.namespace;
  const pagesNs = esmModuleCache.get("12-docs-pages.js")?.namespace;
  const listNs = esmModuleCache.get("11-docs-list.js")?.namespace;
  const miscNs = esmModuleCache.get("15-views-misc.js")?.namespace;
  assert(shellNs && pagesNs && listNs && miscNs, "#4 前置:03/11/12/15 模块命名空间未加载");
  const savedProcessList = structuredClone(payloads.process_list);
  const savedDocs = structuredClone(payloads.docs_snapshot);
  const savedConversationList = payloads.conversation_list;
  const prioritySelect = byId.get("work-priority-select");
  const savedPriority = prioritySelect.value;
  const savedView = document.querySelector(".view.active")?.id?.replace(/^view-/, "") || "chat";
  const g5Lines = [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, branch: "main", authority: "primary", stage: "复核" },
    { id: "p|bg", label: "后台会话", session_id: "sess-bg", running: false, worktree_path: "C:/smoke-wt", branch: "kanzei/thread-smoke", authority: "parallel", stage: "实现" },
  ];
  const showView = async (name) => {
    document.querySelectorAll(".activity-item").find((node) => node.dataset.view === name)?.click();
    await flush();
  };
  const itemOf = (listId, id) => document.querySelector(`#${listId} .doc-item[data-doc-id="${id}"]`);
  const expanded = (item) => Boolean(item)
    && !item.querySelector(".doc-detail")?.classList.contains("hidden")
    && item.querySelector(".doc-row")?.getAttribute("aria-expanded") === "true";
  const collapse = (item) => {
    if (item && !item.querySelector(".doc-detail")?.classList.contains("hidden")) item.querySelector(".doc-row")?.click();
  };
  const focusOpenOf = (id) => document.querySelector(`#focus-body .focus-card[data-doc-id="${id}"] .focus-open`);
  const lineFocusOf = (processId) => [...document.querySelectorAll("#focus-body .line-focus")].find((node) => node.dataset.processId === processId);
  const taskRowOf = (processId) => [...document.querySelectorAll("#parallel-task-status .parallel-task-row")].find((node) => node.dataset.processId === processId);
  // 本段断言中文文案;前面的分区可能把界面留在英文,先切回中文、收尾还原。
  const priorLanguage = localStorageShim.getItem("kz-language");
  localStorageShim.setItem("kz-language", "zh");
  try {
    vm.runInContext('transitionSession("sess-smoke", "idle"); transitionSession("sess-bg", "idle")', sandbox);
    payloads.process_list = structuredClone(g5Lines);
    sandbox.renderProcesses(structuredClone(g5Lines));
    prioritySelect.value = "requirement-first";
    payloads.docs_snapshot = structuredClone(savedDocs);
    sandbox.renderLines(payloads.collaboration_snapshot);
    await sandbox.refreshDocs();
    await flush();
    byId.get("documents-tab-req").click();
    pagesNs.setDependencyViewOpen(false);

    // ① 整卡直达展开的详情(跨视图:经 refreshDocs 收尾消费)。
    collapse(itemOf("documents-req-list", "R-001"));
    await showView("chat");
    assert(!byId.get("view-documents").classList.contains("active"), "#4 前置:应在对话视图");
    const open = focusOpenOf("R-001");
    assert(open, "#4 焦点卡缺少 .focus-open");
    open?.click();
    await flush();
    assert(byId.get("view-documents").classList.contains("active"), "#4 点焦点卡没有切到单页视图");
    assert(expanded(itemOf("documents-req-list", "R-001")), "#4 点焦点卡落到了收起的行上(应直达展开的详情,还得再点一次正是主诉)");

    // ② 已在单页:同步重绘路径,当场展开并高亮(flush 会冲掉 1.2s 的高亮定时器,所以不 flush 就断言)。
    collapse(itemOf("documents-req-list", "R-001"));
    void listNs.jumpToEntry("R-001", { expand: true });
    const syncTarget = itemOf("documents-req-list", "R-001");
    assert(expanded(syncTarget), "#4 单页内跳转(同步重绘路径)没有展开目标详情");
    assert(syncTarget?.classList.contains("ref-highlight"), "#4 单页内跳转后目标行未高亮");
    await flush();

    // ③ refs 链接、测试关联徽标同样直达展开的详情。
    collapse(itemOf("documents-req-list", "R-001"));
    const testChip = [...document.querySelectorAll("#test-list .test-ref-chip")].find((chip) => chip.textContent === "R-001");
    assert(testChip, "#4 前置:测试记录关联徽标未渲染");
    testChip?.click();
    await flush();
    assert(expanded(itemOf("documents-req-list", "R-001")), "#4 测试关联徽标跳转没有展开目标详情");

    // ④ 跨页签:当前在需求页,焦点指向缺陷 → 自动切到缺陷页签并展开。
    payloads.docs_snapshot = { ...structuredClone(savedDocs), requirements: [docEntry("R-A", "需求侧 doing", "doing")], defects: [docEntry("D-A", "缺陷侧 fixing", "fixing")] };
    prioritySelect.value = "defect-first";
    await sandbox.refreshDocs();
    byId.get("documents-tab-req").click();
    assert(pagesNs.documentsKind === "req", "#4 前置:应在需求页签");
    await showView("chat");
    focusOpenOf("D-A")?.click();
    await flush();
    assert(pagesNs.documentsKind === "defect", `#4 跳转缺陷没有切到缺陷页签(仍是 ${pagesNs.documentsKind})`);
    assert(!byId.get("documents-defect-list").classList.contains("hidden"), "#4 切到缺陷页签后缺陷列表仍隐藏");
    assert(expanded(itemOf("documents-defect-list", "D-A")), "#4 跨页签跳转没有展开缺陷详情");
    payloads.docs_snapshot = structuredClone(savedDocs);
    prioritySelect.value = "requirement-first";
    await sandbox.refreshDocs();
    byId.get("documents-tab-req").click();

    // ⑤ 依赖视图开着(两张列表都被它藏着):跳转先关掉它。
    pagesNs.setDependencyViewOpen(true);
    sandbox.renderDocuments(pagesNs.latestDocsSnapshot);
    assert(!byId.get("documents-dep-view").classList.contains("hidden") && byId.get("documents-req-list").classList.contains("hidden"), "#4 前置:依赖视图应开着且列表隐藏");
    collapse(itemOf("documents-req-list", "R-001"));
    await listNs.jumpToEntry("R-001", { expand: true });
    assert(pagesNs.dependencyViewOpen === false, "#4 跳转没有关掉依赖视图");
    assert(byId.get("documents-dep-view").classList.contains("hidden") && !byId.get("documents-req-list").classList.contains("hidden"), "#4 关掉依赖视图后列表仍不可见");
    assert(expanded(itemOf("documents-req-list", "R-001")), "#4 依赖视图场景下跳转没有展开目标详情");
    await flush();

    // ⑥ 筛选放行:目标被筛选挡住时临时插回、标记说破;不改筛选状态、不落盘;改筛选/离开单页即作废。
    sandbox.applyDocFilter("status", "todo");
    assert(!itemOf("documents-req-list", "R-001"), "#4 前置:status=todo 应藏住 doing 的 R-001");
    const filtersKey = `kz-filters:${shellNs.currentProject}`;
    const storedBefore = storage.get(filtersKey);
    await listNs.jumpToEntry("R-001", { expand: true });
    const exempt = itemOf("documents-req-list", "R-001");
    assert(exempt?.classList.contains("filter-exempt") && expanded(exempt), "#4 被筛选挡住的跳转目标没有临时放行并展开");
    assert(exempt?.querySelector(".filter-exempt-flag")?.textContent.includes("不在当前筛选内"), "#4 放行的条目没有说破「不在当前筛选内」");
    assert(pagesNs.documentFilters.req.status === "todo" && storage.get(filtersKey) === storedBefore, "#4 跳转放行改了筛选状态或落盘值(R-115)");
    await sandbox.refreshDocs();
    assert(itemOf("documents-req-list", "R-001")?.classList.contains("filter-exempt"), "#4 一次无关重绘就把放行冲掉了");
    sandbox.applyDocFilter("status", "todo");
    assert(!itemOf("documents-req-list", "R-001") && listNs.jumpRevealId === null, "#4 改筛选后放行未作废(筛选外条目会一直赖在列表里)");
    await listNs.jumpToEntry("R-001", { expand: true });
    assert(itemOf("documents-req-list", "R-001"), "#4 前置:再次放行");
    await showView("chat");
    assert(listNs.jumpRevealId === null, "#4 离开单页后放行未作废");
    // ⑥b 筛选内的目标不放行:否则它之后因改状态落到筛选外(筛选 doing 时在详情头点「→ 转 done」),
    //    会一直挂着「不在当前筛选内」赖在列表里,直到用户改筛选或离开单页。
    sandbox.applyDocFilter("status", "doing");
    await listNs.jumpToEntry("R-001", { expand: true });
    await flush();
    const inFilter = itemOf("documents-req-list", "R-001");
    assert(listNs.jumpRevealId === null, `#4 筛选内的跳转目标也被设了放行(jumpRevealId=${listNs.jumpRevealId})`);
    assert(inFilter && expanded(inFilter) && !inFilter.classList.contains("filter-exempt"), "#4 筛选内的跳转目标应正常展开且不带放行标记");
    const doneDocs = structuredClone(savedDocs);
    doneDocs.requirements.find((entry) => entry.id === "R-001").status = "done";
    payloads.docs_snapshot = doneDocs;
    await sandbox.refreshDocs();
    assert(!itemOf("documents-req-list", "R-001"), "#4 筛选内跳转的目标转状态落到筛选外后仍赖在列表里");
    payloads.docs_snapshot = structuredClone(savedDocs);
    await showView("chat");
    sandbox.applyDocFilter("status", "all");
    await sandbox.refreshDocs();
    await flush();

    // ⑦ 焦点区:线路头「身份 · 名称」与任务卡同一口径(lineAuthorityLabel),分支进 tooltip;
    //    空线路一行;取得声明带编号时可点直达。
    sandbox.renderLines([]);
    sandbox.renderFocusPanel(pagesNs.latestDocsSnapshot);
    const bgFocus = lineFocusOf("p|bg");
    const bgHead = bgFocus?.querySelector(".line-focus-head");
    assert(bgHead?.textContent === "并行线 · 后台会话" && bgHead.title === "kanzei/thread-smoke", `#4 线路头应为「身份 · 名称」且分支进 tooltip:${bgHead?.textContent} / ${bgHead?.title}`);
    assert(taskRowOf("p|bg")?.querySelector(".parallel-task-head")?.textContent.startsWith("并行线 · 后台会话"), "#4 任务卡与焦点区线路叫法不一致");
    assert(lineFocusOf("d|smoke")?.querySelector(".line-focus-head")?.textContent === "主代理 · 主会话", "#4 主线线路头叫法不对");
    const bgEmpty = bgFocus?.querySelectorAll(".line-focus-empty") ?? [];
    assert(bgEmpty.length === 1 && bgEmpty[0].textContent === "未取得条目" && bgEmpty[0].title.includes("条可执行待取活"), `#4 空线路应只占一行「未取得条目」,原因进 tooltip:${bgFocus?.textContent}`);
    assert(!bgFocus?.textContent.includes("条可执行待取活"), "#4 空线路仍在卡面重复全局原因");
    sandbox.renderLines([{ process_id: "p|bg", label: "后台会话", branch: "kanzei/thread-smoke", worktree_path: "C:/smoke-wt", claim: "R-002 冒烟需求二", phase: "实现", current_tool: null, running: false, steps: 0, input_tokens: 0, output_tokens: 0, changed_files: [] }]);
    sandbox.renderFocusPanel(pagesNs.latestDocsSnapshot);
    const claimLink = lineFocusOf("p|bg")?.querySelector(".focus-claim-link");
    assert(claimLink?.textContent.includes("R-002"), `#4 取得声明带编号时应渲染成可点链接:${lineFocusOf("p|bg")?.textContent}`);
    collapse(itemOf("documents-req-list", "R-002"));
    claimLink?.click();
    await flush();
    assert(byId.get("view-documents").classList.contains("active") && expanded(itemOf("documents-req-list", "R-002")), "#4 点取得声明没有直达 R-002 展开的详情");
    sandbox.renderLines(payloads.collaboration_snapshot);
    await showView("chat");

    // ⑧ 签名跳过:同一份快照连续渲染,焦点卡节点身份不变(轮询不冲掉 tooltip/菜单)。
    sandbox.renderFocusPanel(pagesNs.latestDocsSnapshot);
    const firstCard = document.querySelector('#focus-body .focus-card[data-doc-id="R-001"]');
    sandbox.renderFocusPanel(structuredClone(pagesNs.latestDocsSnapshot));
    assert(firstCard && document.querySelector('#focus-body .focus-card[data-doc-id="R-001"]') === firstCard, "#4 内容没变也重建了焦点区(签名跳过失效)");

    // ⑨ 任务卡:一行「字形 + 身份 · 名称 + 分支 + 单个状态词」,不再「空闲 · 空闲」;关闭是悬停出现的图标按钮。
    const bgRow = taskRowOf("p|bg");
    assert(bgRow?.children[0]?.classList.contains("kz-glyph") && bgRow.children[0].textContent === "○", `#4 任务卡行首应是字形:${bgRow?.children[0]?.className}`);
    assert(bgRow?.querySelector(".parallel-task-branch")?.textContent === "kanzei/thread-smoke", "#4 任务卡分支名不可见");
    assert(bgRow?.querySelector(".parallel-task-state")?.textContent === "空闲", `#4 空闲线路状态应是单个词「空闲」:${bgRow?.querySelector(".parallel-task-state")?.textContent}`);
    assert(!bgRow?.textContent.includes("空闲 · 空闲"), "#4 任务卡仍显示「空闲 · 空闲」");
    const bgLine = [...document.querySelectorAll("#parallel-task-status .parallel-line")].find((node) => node.dataset.processId === "p|bg");
    const closeBtn = bgLine?.querySelector(".parallel-line-close");
    assert(closeBtn?.classList.contains("icon-btn") && closeBtn.textContent === "×", "#4 关闭线路应是图标按钮");
    assert(closeBtn?.getAttribute("aria-label") === "关闭线路 后台会话", `#4 关闭按钮的读屏名称应带线路名:${closeBtn?.getAttribute("aria-label")}`);
    assert(!taskRowOf("d|smoke")?.parentElement?.querySelector(".parallel-line-close"), "#4 默认线不该有关闭按钮");
    // 运行中:状态词就是阶段(取不到才写「运行中」),不再「运行中 · 实现」两段。
    vm.runInContext('transitionSession("sess-bg", "running")', sandbox);
    sandbox.refreshParallelTaskProjection("sess-bg");
    const runningWord = taskRowOf("p|bg")?.querySelector(".parallel-task-state")?.textContent ?? "";
    assert(taskRowOf("p|bg")?.children[0]?.textContent === "●" && runningWord && !runningWord.includes("空闲"), `#4 运行中任务卡状态不对:${taskRowOf("p|bg")?.textContent}`);
    vm.runInContext('transitionSession("sess-bg", "idle")', sandbox);
    sandbox.refreshParallelTaskProjection("sess-bg");

    // ⑩ 历史行:0 条与加载中都不占行;有历史时折叠头写「历史对话 N」。
    payloads.conversation_list = () => [];
    await sandbox.refreshConversationLists();
    await flush();
    const histories = document.querySelectorAll("#parallel-task-status .parallel-line-history");
    assert(histories.length === 2 && histories.every((node) => node.classList.contains("empty") && !node.querySelector(".parallel-history-head")), "#4 0 条历史的线路仍渲染了历史行");
    miscNs.conversationItemsByProcess.delete("p|bg");
    miscNs.renderLineConversationHistory("p|bg");
    const loadingHistory = histories.find((node) => node.dataset.processId === "p|bg");
    assert(loadingHistory?.classList.contains("empty") && !loadingHistory.textContent.includes("加载中"), "#4 历史加载中不该占一行「加载中…」");
    payloads.conversation_list = savedConversationList;
    await sandbox.refreshConversationLists();
    await flush();
    const bgHistory = document.querySelectorAll("#parallel-task-status .parallel-line-history").find((node) => node.dataset.processId === "p|bg");
    assert(!bgHistory?.classList.contains("empty") && bgHistory?.querySelector(".parallel-history-label")?.textContent === "历史对话 1", `#4 历史折叠头应为「历史对话 N」:${bgHistory?.textContent}`);
    // 桩里的条目没有 message_count(旧后端同形):展开后不得写出「(undefined 条)」。
    if (!bgHistory?.classList.contains("open")) bgHistory?.querySelector(".parallel-history-head")?.click();
    assert(bgHistory?.textContent.includes("后台线路历史") && !bgHistory.textContent.includes("undefined"), `#4 历史行缺条数时写出了 undefined:${bgHistory?.textContent}`);

    // ⑪ 状态栏去重:两格同词时只留一格。
    sandbox.setStatus("空闲", false);
    assert(byId.get("status-text").classList.contains("hidden"), "#4 状态栏空闲时仍并排「空闲 空闲」");
    sandbox.setStatus("第 1 轮 · 等待模型", true);
    assert(!byId.get("status-text").classList.contains("hidden") && byId.get("status-text").textContent.includes("等待模型"), "#4 状态栏有具体状态时不该隐藏");
    sandbox.setStatus("运行中", true);
    assert(byId.get("status-text").classList.contains("hidden"), "#4 状态栏运行中时仍并排「运行中 运行中」");
    sandbox.setRunning(false, "空闲");

    // ⑫ 静态结构:焦点卡 CSS 的撑满点击层、悬停中性、⋯ 悬停出现;任务卡关闭按钮悬停出现;实时行一行。
    const g5Css = style.replace(/\/\*[\s\S]*?\*\//g, "");
    for (const [re, label] of [
      [/\.focus-open::after\s*\{[^}]*inset:\s*0/, ".focus-open::after 撑满整卡"],
      [/\.focus-card:hover, \.focus-card:focus-within\s*\{[^}]*var\(--surface-hover\)/, "焦点卡悬停/聚焦用中性 --surface-hover"],
      [/\.focus-card:hover \.focus-more, \.focus-card:focus-within \.focus-more, \.focus-more\[aria-expanded="true"\]\s*\{\s*opacity:\s*1/, "⋯ 悬停/聚焦/菜单开着时出现"],
      [/\.parallel-line:hover \.parallel-line-close, \.parallel-line:focus-within \.parallel-line-close\s*\{\s*opacity:\s*1/, "关闭按钮悬停/聚焦时出现"],
      [/\.parallel-line-history\.empty\s*\{\s*display:\s*none/, "0 条/加载中的历史行不占位"],
      [/#live-status\s*\{[^}]*flex-flow:\s*row wrap/, "#live-status 实时状态并成一行"],
      // 英文「defect」词条是小写(也用在句中),页签上要首字母大写,别出现「Work items | defect」。
      [/\.documents-tabs button::first-letter\s*\{\s*text-transform:\s*uppercase/, "页签文字首字母大写"],
    ]) {
      assert(re.test(g5Css), `#4 CSS 缺少:${label}`);
    }
    assert(!/#live-(?:note|focus)\b/.test(g5Css), "#4 已删除的 #live-note/#live-focus 仍有样式规则");
  } finally {
    payloads.process_list = savedProcessList;
    payloads.docs_snapshot = savedDocs;
    payloads.conversation_list = savedConversationList;
    prioritySelect.value = savedPriority;
    pagesNs?.setDependencyViewOpen(false);
    listNs?.clearJumpReveal();
    sandbox.renderLines(payloads.collaboration_snapshot);
    sandbox.renderProcesses(structuredClone(savedProcessList));
    await sandbox.refreshDocs();
    await showView(savedView);
    if (priorLanguage === null) localStorageShim.removeItem?.("kz-language");
    else localStorageShim.setItem("kz-language", priorLanguage);
  }
}

// ---------- UI-0926 #4(单页):列表行状态列 / 详情只读优先 + 字段结构化(#10 renderTrackerFields)/
// 编辑态与草稿跨重绘 / 工具栏「筛选」「更多」弹层与生效 chip / 线路页取得条目直达 / 活动面板行操作。
// 设计见 scratchpad density.md M5–M8 与 structured.md「tracker 字段」。
{
  const pagesNs = esmModuleCache.get("12-docs-pages.js")?.namespace;
  const listNs = esmModuleCache.get("11-docs-list.js")?.namespace;
  const surfaceNs = esmModuleCache.get("00-surface.js")?.namespace;
  assert(pagesNs && listNs && surfaceNs, "#4 单页前置:00/11/12 模块命名空间未加载");
  const savedDocs = structuredClone(payloads.docs_snapshot);
  const savedView = document.querySelector(".view.active")?.id?.replace(/^view-/, "") || "chat";
  const showView = async (name) => {
    document.querySelectorAll(".activity-item").find((node) => node.dataset.view === name)?.click();
    await flush();
  };
  const itemOf = (listId, id) => document.querySelector(`#${listId} .doc-item[data-doc-id="${id}"]`);
  const detailOf = (id) => itemOf("documents-req-list", id)?.querySelector(".doc-detail");
  const expandedDetail = (item) => Boolean(item) && !item.querySelector(".doc-detail")?.classList.contains("hidden");
  const openDetail = (id) => {
    const item = itemOf("documents-req-list", id);
    if (item && !expandedDetail(item)) item.querySelector(".doc-row")?.click();
    return detailOf(id);
  };
  const priorLanguage = localStorageShim.getItem("kz-language");
  localStorageShim.setItem("kz-language", "zh");
  const richEntry = docEntry("R-T01", "单页详情样例", "doing", {
    complexity: "中", nextStatuses: ["done", "dropped"], execution_model: "work_units_v1",
    work_units: [{ ...smokeWorkUnit, unit_id: "R-T01/W1", requirement_id: "R-T01" }],
    fields: [
      ["内容", "把详情做成只读优先的文档视图"],
      ["验收", "①默认只读；②点编辑才出表单；③重绘不丢草稿"],
      ["发现记录", JSON.stringify({ Intent: "像读文档", Explicit: "别一整墙输入框", Ambiguities: "无" })],
      ["停车", "等上游评审;恢复人:agent;解除条件:R-002"],
      ["进展", "2026-09-26 B2 进行中||2026-09-25 B1 合入||批0 勘察"],
      ["refs", "R-002 docs/design/memory_control_plane.md"],
      ["标签", "前端"],
      ["observed_head", "0123456789abcdef0123"],
      ["recorded_at", "1790386080000"],
    ],
  });
  try {
    // 前面分区可能留着应用内查看器(模态):模态开着时静态弹层是惰性的,先收掉再测工具栏弹层。
    for (const id of ["viewer-overlay", "confirm-overlay", "input-overlay", "palette"]) surfaceNs.closeSurface(byId.get(id));
    payloads.docs_snapshot = { ...structuredClone(savedDocs), requirements: [richEntry, ...structuredClone(savedDocs.requirements)] };
    await showView("documents");
    byId.get("documents-tab-req").click();
    pagesNs.setDependencyViewOpen(false);
    sandbox.clearDocFilters();
    await sandbox.refreshDocs();
    await flush();

    // ① M5 列表行:状态列回来了且排在优先级之前(固定宽,D-362 三列对齐不破);行内仍不写 R- 编号(R-054);
    //    复杂度一列一个字、说明进 tooltip;详情头统一「编号 · 标题」(缺陷原来不带编号)。
    const docRows = document.querySelectorAll("#documents-req-list .doc-row, #documents-defect-list .doc-row");
    assert(docRows.length > 0 && docRows.every((row) => {
      const kids = [...row.children];
      const st = kids.findIndex((node) => node.classList.contains("st"));
      const pri = kids.findIndex((node) => node.classList.contains("pri-badge"));
      return st >= 0 && pri > st;
    }), "#4 单页列表行缺状态列,或状态列没排在优先级之前");
    const r001Row = itemOf("documents-req-list", "R-001")?.querySelector(".doc-row");
    assert(r001Row?.querySelector(".st")?.textContent === "doing", `#4 R-001 行状态列应为 doing:${r001Row?.querySelector(".st")?.textContent}`);
    assert(!r001Row?.textContent.includes("R-001"), "#4 行内出现了 R- 编号(R-054:编号只在 tooltip 与详情头)");
    const cxBadge = r001Row?.querySelector(".complexity-badge");
    assert(cxBadge?.textContent === "中" && cxBadge.title.includes("复杂度"), `#4 复杂度列应只写一个字、说明进 tooltip:"${cxBadge?.textContent}" / "${cxBadge?.title}"`);
    assert(itemOf("documents-defect-list", "D-001")?.querySelector(".doc-full-title")?.textContent.startsWith("D-001 · "), "#4 缺陷详情头缺编号");
    assert(itemOf("documents-defect-list", "D-001")?.querySelector(".st")?.title.includes("medium"), "#4 缺陷严重度应进状态列 tooltip");

    // ② M6 + #10 详情只读优先:头(编号 · 标题 + 状态流转 + 编辑)、字段按结构渲染、执行单元折叠、默认无输入框。
    let detail = openDetail("R-T01");
    assert(detail && !detail.classList.contains("hidden"), "#4 前置:R-T01 详情未展开");
    const headEl = detail?.querySelector(".doc-detail-head");
    assert(headEl?.querySelector(".doc-full-title")?.textContent === "R-T01 · 单页详情样例", "#4 详情头应为「编号 · 标题」");
    const headButtons = headEl?.querySelectorAll(".doc-detail-actions button") ?? [];
    assert(headButtons.some((node) => node.textContent.includes("done")) && headButtons.some((node) => node.classList.contains("doc-edit-toggle")),
      "#4 状态流转与「编辑」开关应在详情头(不再沉到最底下)");
    const read = detail?.querySelector(".doc-fields-read");
    const fieldRow = (key) => read?.querySelectorAll(".tf-row").find((node) => node.dataset.field === key);
    assert(read && !read.classList.contains("hidden") && !read.querySelector("input") && !read.querySelector("textarea"),
      "#4 详情默认应是只读文档视图(不是一整墙输入框)");
    assert(!detail?.classList.contains("editing") && detail?.querySelector(".doc-edit")?.classList.contains("hidden"), "#4 编辑表单默认应隐藏");
    assert(fieldRow("验收")?.querySelectorAll("ol li").length === 3, "#4 验收的 ①②③ 未切成有序列表");
    assert(fieldRow("发现记录")?.querySelectorAll(".sv-kv-row").length === 3, "#4 发现记录 JSON 未渲染成键值表");
    assert(fieldRow("停车")?.querySelector(".tf-release .sv-ref")?.textContent === "R-002" && fieldRow("停车")?.querySelector(".tf-owner"),
      "#4 停车字段未拆出恢复人与可点的解除条件");
    assert(fieldRow("refs")?.querySelectorAll(".sv-ref").some((node) => node.dataset.ref === "R-002")
      && fieldRow("refs")?.querySelectorAll(".sv-path").some((node) => node.dataset.path?.endsWith("docs/design/memory_control_plane.md")),
    "#4 refs 应把条目编号渲染成可点 chip、文档路径渲染成路径 chip(不再当条目编号跳转落空)");
    const progress = fieldRow("进展");
    const latest = progress?.querySelector(".tf-timeline");
    const older = progress?.querySelector(".doc-progress-older");
    assert(latest?.children.length === 1 && latest.textContent.includes("B2 进行中"), `#4 进展只该露最新一段:${latest?.textContent}`);
    assert(older && !older.open && older.querySelector("summary")?.textContent === "更早进展 2" && older.querySelectorAll("li").length === 2,
      `#4 更早的进展应收进「更早进展 N」折叠区:${older?.querySelector("summary")?.textContent}`);
    assert(read?.querySelector(".tf-engine") && !fieldRow("observed_head") && !fieldRow("recorded_at"), "#4 引擎字段应收进「引擎记录」折叠区,不单独成行");
    const units = detail?.querySelector(".work-unit-details");
    assert(units && !units.open && units.querySelector("summary")?.textContent.startsWith("执行单元 0/1") && units.querySelector(".work-unit-card"),
      "#4 执行单元应默认折叠,summary 一行说清进度");

    // ③ 详情头的状态流转仍走 docs_update(硬门禁同一套 nextStatuses)。
    const beforeStatus = invokeArgs.length;
    headButtons.find((node) => node.textContent.includes("done"))?.click();
    await flush();
    assert(invokeArgs.slice(beforeStatus).some(({ cmd, args }) => cmd === "docs_update" && args?.id === "R-T01" && args?.status === "done"),
      "#4 详情头的状态流转按钮未发出 docs_update(status=done)");

    // ④ 折叠区展开状态跨重绘保留(agent 一次刷新不把人刚展开的执行单元/更早进展弹回去)。
    detailOf("R-T01").querySelector(".work-unit-details").open = true;
    detailOf("R-T01").querySelector(".doc-progress-older").open = true;
    await sandbox.refreshDocs();
    assert(detailOf("R-T01")?.querySelector(".work-unit-details")?.open && detailOf("R-T01")?.querySelector(".doc-progress-older")?.open,
      "#4 重绘把展开的执行单元/更早进展又收起来了");

    // ⑤ 编辑:点「编辑」替换只读视图;输入后带 data-dirty;refreshDocsSoon 让路;显式重绘保留编辑态与草稿。
    const control = (key) => detailOf("R-T01")?.querySelectorAll(".doc-edit [data-field]").find((node) => node.dataset.field === key);
    detailOf("R-T01").querySelector(".doc-edit-toggle").click();
    detail = detailOf("R-T01");
    const toggle = detail.querySelector(".doc-edit-toggle");
    assert(detail.classList.contains("editing") && !detail.querySelector(".doc-edit").classList.contains("hidden")
      && detail.querySelector(".doc-fields-read").classList.contains("hidden"), "#4 点「编辑」后应换成编辑表单");
    assert(toggle.getAttribute("aria-pressed") === "true" && toggle.textContent === "取消编辑", "#4 编辑开关未切到「取消编辑」");
    assert(control("复杂度")?.tagName === "SELECT", "#4 复杂度应并进编辑表单(下拉)");
    control("验收").value = "①改过的验收";
    control("验收").dispatchEvent({ type: "input" });
    assert(detail.querySelector(".doc-edit").dataset.dirty === "1" && document.querySelector(".doc-detail.editing .doc-edit[data-dirty]"),
      "#4 输入后编辑区未标记 data-dirty");
    const snapshotCalls = () => invokeLog.filter((cmd) => cmd === "docs_snapshot").length;
    const snapshotsBefore = snapshotCalls();
    sandbox.refreshDocsSoon();
    await flush();
    assert(snapshotCalls() === snapshotsBefore, "#4 有未保存的条目编辑时 refreshDocsSoon 没让路(agent 刷新会打断输入)");
    await sandbox.refreshDocs();
    assert(detailOf("R-T01") !== detail, "#4 前置:显式 refreshDocs 应重建详情节点");
    assert(detailOf("R-T01")?.classList.contains("editing") && !detailOf("R-T01").querySelector(".doc-edit").classList.contains("hidden"),
      "#4 重绘把编辑态弹回了只读视图");
    assert(control("验收")?.value === "①改过的验收" && control("验收")?.dataset.dirty === "1", `#4 重绘冲掉了没保存的输入:${control("验收")?.value}`);
    assert(control("内容")?.value === "把详情做成只读优先的文档视图" && !control("内容")?.dataset.dirty, "#4 没改过的字段应取新快照的值");
    const beforeSave = invokeArgs.length;
    detailOf("R-T01").querySelector(".doc-edit-actions button").click();
    await flush();
    const saveCall = invokeArgs.slice(beforeSave).find(({ cmd, args }) => cmd === "docs_update" && args?.id === "R-T01" && args?.fields);
    assert(saveCall?.args.fields["验收"] === "①改过的验收" && saveCall.args.title === "单页详情样例", `#4 保存未提交草稿:${JSON.stringify(saveCall?.args)}`);
    assert(saveCall && !("复杂度" in saveCall.args.fields), "#4 复杂度没改也写进了保存载荷(会凭空多出字段)");
    assert(!detailOf("R-T01")?.classList.contains("editing") && detailOf("R-T01")?.querySelector(".doc-edit")?.classList.contains("hidden"),
      "#4 保存后应回到只读视图");
    // 取消编辑:输入复位、dirty 清掉。
    detailOf("R-T01").querySelector(".doc-edit-toggle").click();
    control("内容").value = "临时改动";
    control("内容").dispatchEvent({ type: "input" });
    detailOf("R-T01").querySelector(".doc-edit-toggle").click();
    assert(control("内容")?.value === "把详情做成只读优先的文档视图" && !detailOf("R-T01").querySelector(".doc-edit").dataset.dirty
      && !detailOf("R-T01").classList.contains("editing"), "#4 取消编辑没有复位输入");
    await flush();

    // ⑥ M7 工具栏:五个筛选 + 排序 + 分组 + 说明都收进「筛选」弹层;两个菜单是 data-kz-menu + popover(弹层唯一写法)。
    const filterSrc = html.slice(html.indexOf('id="documents-filter-menu"'), html.indexOf('id="documents-more-toggle"'));
    for (const id of ["documents-status-filter", "documents-complexity-filter", "documents-priority-filter", "documents-tag-filter", "documents-blocked-filter", "documents-sort", "documents-group-toggle", "documents-sort-note"]) {
      assert(filterSrc.includes(`id="${id}"`), `#4 ${id} 不在「筛选」弹层里`);
    }
    const moreSrc = html.slice(html.indexOf('id="documents-more-menu"'), html.indexOf('id="documents-active-filters"'));
    for (const id of ["documents-dep-toggle", "defect-review", "tests-refresh", "req-open", "defect-open"]) {
      assert(moreSrc.includes(`id="${id}"`), `#4 ${id} 不在「更多」菜单里`);
    }
    assert(/id="documents-filter-toggle"[^>]*data-kz-menu="documents-filter-menu"/.test(html) && /id="documents-more-toggle"[^>]*data-kz-menu="documents-more-menu"/.test(html)
      && /id="documents-filter-menu"[^>]*popover="manual"/.test(html) && /id="documents-more-menu"[^>]*popover="manual"/.test(html),
    "#4 单页的「筛选」「更多」应是 data-kz-menu 触发器 + popover 弹层");
    assert(!/<details[^>]*id="documents-/.test(html), "#4 单页工具栏不得用 <details> 做弹层");
    assert(!html.includes("完整列表与深度管理都在这里"), "#4 单页顶部的长说明段应删掉");
    const filterToggle = byId.get("documents-filter-toggle");
    const filterMenu = byId.get("documents-filter-menu");
    filterToggle.click();
    assert(surfaceNs.isSurfaceOpen(filterMenu) && !filterMenu.classList.contains("hidden") && filterToggle.getAttribute("aria-expanded") === "true",
      "#4 点「筛选」没有经弹层原语打开筛选弹层");
    sandbox.applyDocFilter("status", "doing");
    const chipsRow = byId.get("documents-active-filters");
    const chips = () => chipsRow.querySelectorAll(".documents-filter-chip");
    assert(!chipsRow.classList.contains("hidden") && chips().length === 1 && chips()[0].textContent.includes("状态: doing"),
      `#4 生效的筛选没有以 chip 说破:${chipsRow.textContent}`);
    assert(byId.get("documents-filter-count").textContent === "1", "#4 「筛选」触发器未显示生效项数");
    assert(surfaceNs.isSurfaceOpen(filterMenu), "#4 改一个筛选就把弹层关了(连续调几个筛选要能一口气改完)");
    chips()[0].querySelector(".documents-filter-chip-clear").click();
    const savedFilters = () => JSON.parse(storage.get(`kz-filters:${PROJECT}`) ?? "{}").docReq ?? {};
    assert(pagesNs.documentFilters.req.status === "all" && savedFilters().status === "all" && chipsRow.classList.contains("hidden")
      && byId.get("documents-filter-count").textContent === "", "#4 chip 的 × 没把该筛选复位并落盘");
    sandbox.applyDocFilter("priority", "P1");
    sandbox.applyDocFilter("sort", "priority");
    assert(chips().length === 2 && byId.get("documents-filter-count").textContent === "2", "#4 两项生效时 chip/计数不对");
    chipsRow.querySelector(".documents-filter-clear-all")?.click();
    assert(pagesNs.documentFilters.req.priority === "all" && pagesNs.documentFilters.req.sort === "manual" && savedFilters().sort === "manual"
      && chipsRow.classList.contains("hidden"), "#4 「清除全部」没把筛选与排序一起复位并落盘");
    sandbox.applyDocFilter("status", "doing");
    byId.get("documents-tab-tests").click();
    assert(chipsRow.classList.contains("hidden") && byId.get("documents-filter-count").textContent === "", "#4 测试记录页签不该显示需求筛选 chip");
    byId.get("documents-tab-req").click();
    assert(!chipsRow.classList.contains("hidden"), "#4 切回需求页签后 chip 行没回来");
    sandbox.applyDocFilter("status", "all");
    surfaceNs.closeSurface(filterMenu);
    // 「更多」:依赖视图是勾选型菜单项;里面的动作点完就收起菜单。
    const moreToggle = byId.get("documents-more-toggle");
    const moreMenu = byId.get("documents-more-menu");
    moreToggle.click();
    assert(surfaceNs.isSurfaceOpen(moreMenu), "#4 点「更多」没有打开菜单");
    byId.get("documents-dep-toggle").click();
    assert(!surfaceNs.isSurfaceOpen(moreMenu), "#4 「更多」里点了动作,菜单没收起");
    assert(byId.get("documents-dep-toggle").getAttribute("aria-checked") === "true" && !byId.get("documents-dep-view").classList.contains("hidden"),
      "#4 依赖视图菜单项未切换(aria-checked / 面板)");
    byId.get("documents-dep-toggle").click();
    assert(byId.get("documents-dep-toggle").getAttribute("aria-checked") === "false", "#4 依赖视图菜单项未切回");
    byId.get("documents-tab-req").click();
    await flush();

    // ⑦ M8 线路页:取得条目在快照里查得到就是直达展开详情的链接;查不到只写文字。
    sandbox.renderLines([{ process_id: "p|bg", label: "后台会话", branch: "kanzei/thread-smoke", worktree_path: "C:/smoke-wt", claim: "R-001 冒烟需求", phase: "实现", current_tool: null, running: false, steps: 0, input_tokens: 0, output_tokens: 0, changed_files: [] }]);
    const claimLink = document.querySelector("#lines-list .line-claim-link");
    assert(claimLink?.textContent.startsWith("R-001 · ") && claimLink.title.includes("点击查看详情"), `#4 线路页取得条目应是可点链接:${document.querySelector("#lines-list .line-claim")?.textContent}`);
    const r001 = itemOf("documents-req-list", "R-001");
    if (expandedDetail(r001)) r001.querySelector(".doc-row")?.click();
    await showView("chat");
    claimLink?.click();
    await flush();
    assert(byId.get("view-documents").classList.contains("active") && expandedDetail(itemOf("documents-req-list", "R-001")),
      "#4 点线路页取得条目没有直达 R-001 展开的详情");
    sandbox.renderLines([{ process_id: "p|bg", label: "后台会话", branch: "kanzei/thread-smoke", worktree_path: "C:/smoke-wt", claim: "R-9999 线路里新登记", phase: "实现", current_tool: null, running: false, steps: 0, input_tokens: 0, output_tokens: 0, changed_files: [] }]);
    assert(!document.querySelector("#lines-list .line-claim-link") && document.querySelector("#lines-list .line-claim")?.textContent.includes("R-9999"),
      "#4 快照里查不到的取得条目不该渲染成点了落空的链接");

    // ⑧ 静态:活动面板行操作悬停/聚焦/展开才出现;详情与工具栏 CSS 只用 token。
    const css = style.replace(/\/\*[\s\S]*?\*\//g, "");
    assert(/\.bg-entry \.bg-actions \{ display: none; \}/.test(css)
      && /\.bg-entry:hover \.bg-actions:not\(:empty\), \.bg-entry:focus-within \.bg-actions:not\(:empty\),\s*\.bg-entry \.bg-title\[aria-expanded="true"\] ~ \.bg-actions:not\(:empty\) \{ display: flex; \}/.test(css),
    "#4 活动面板行操作应只在悬停/聚焦/展开时出现");
    const g5Css = style.split("/* ===== 分区:需求卡片与单页 ===== */")[1]?.split("/* ===== 分区:动效 ===== */")[0] ?? "";
    assert(/\.documents-tabs button\.primary, \.documents-tabs button\.primary:hover \{[^}]*var\(--surface-selected\)/.test(g5Css), "#4 当前页签应是中性选中态");
    assert(!/#[0-9a-fA-F]{3,8}\b|rgba?\(/.test(g5Css.replace(/\/\*[\s\S]*?\*\//g, "")), "#4 需求卡片与单页分区用了字面量颜色(只准用 token)");
  } finally {
    payloads.docs_snapshot = savedDocs;
    pagesNs?.setDependencyViewOpen(false);
    listNs?.clearJumpReveal();
    sandbox.clearDocFilters?.();
    for (const id of ["documents-filter-menu", "documents-more-menu"]) surfaceNs?.closeSurface(byId.get(id));
    sandbox.renderLines(payloads.collaboration_snapshot);
    byId.get("documents-tab-req").click();
    await sandbox.refreshDocs();
    await showView(savedView);
    await flush();
    if (priorLanguage === null) localStorageShim.removeItem?.("kz-language");
    else localStorageShim.setItem("kz-language", priorLanguage);
  }
}

// ===== 分区:动效 =====
// ---------- #7 动效:运行相位投影 / 工具行收尾 / 线路字形 / 在做 / 轮末 / 隐藏暂停 / 徽标 / 计数 ----------
// 一次性动效类靠 setTimeout 摘除,而 flush 会立刻清空全部定时器:「挂上」必须在同步调用
// 之后、flush 之前断言,「摘除」在 flush 之后断言。
{
  const shellNs = esmModuleCache.get("03-shell.js")?.namespace;
  const chatNs = esmModuleCache.get("05-chat-render.js")?.namespace;
  const activeSid = () => vm.runInContext("activeSessionId", sandbox);
  const html = document.documentElement;
  const row = byId.get("turn-activity");
  const dot = byId.get("status-dot");
  const glyph = byId.get("turn-activity-glyph");
  const label = byId.get("turn-activity-label");
  assert(row && dot && glyph && label, "#7 运行活动行或状态点节点缺失");
  assert(shellNs && chatNs, "#7 03-shell / 05-chat-render 命名空间未加载");
  const phase = () => row.dataset.phase;
  const toolStartEv = handlers.get("kz:tool-start");
  const toolEndEv = handlers.get("kz:tool-end");
  const reasoningEv = handlers.get("kz:reasoning");
  // 前置:主线活动、清掉前面用例留下的在跑工具块(chatAbortRunning 本身就是被测能力之一)。
  const savedProcessList = structuredClone(payloads.process_list);
  const savedCtx = { limit: shellNs.ctxLimit, tokens: shellNs.ctxTokens, pending: shellNs.ctxPending };
  const motionLines = [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, branch: "main", authority: "primary", stage: "复核" },
    { id: "p|bg", label: "后台会话", session_id: "sess-bg", running: false, worktree_path: "C:/smoke-wt", branch: "kanzei/thread-smoke", authority: "parallel", stage: "实现" },
  ];
  vm.runInContext('autoContinueTimers.clear()', sandbox);
  vm.runInContext('transitionSession("sess-smoke", "idle"); transitionSession("sess-bg", "idle")', sandbox);
  // refreshProcesses(kz:error/kz:stopped 的路由分支会调)拉回的也必须是这份清单。
  payloads.process_list = structuredClone(motionLines);
  sandbox.renderProcesses(structuredClone(motionLines));
  if (activeSid() !== "sess-smoke") await sandbox.switchProcess("d|smoke");
  await flush();
  assert(activeSid() === "sess-smoke", `#7 前置:主线应为活动线,实为 ${activeSid()}`);
  vm.runInContext("chatAbortRunning()", sandbox);
  sandbox.setRunning(false, "空闲");
  await flush();
  assert(row.classList.contains("hidden"), "#7 空闲时运行活动行应隐藏");
  assert(html.dataset.kzActivity === "idle", `#7 空闲时 html[data-kz-activity] 应为 idle,实为 ${html.dataset.kzActivity}`);

  // ① 运行相位投影:等首 token → 思考 → 工具;后台线事件不得改写活动线相位。
  vm.runInContext('transitionSession("sess-smoke", "running")', sandbox);
  sandbox.setRunning(true, "运行中");
  assert(!row.classList.contains("hidden"), "#7 setRunning(true) 后运行活动行未出现");
  assert(phase() === "waiting", `#7 刚开跑应为 waiting(等首 token),实为 ${phase()}`);
  assert(html.dataset.kzActivity === "running", `#7 运行中 html[data-kz-activity] 应为 running,实为 ${html.dataset.kzActivity}`);
  assert(dot.dataset.state === "running" && dot.classList.contains("kz-dot") && dot.classList.contains("run"), `#7 状态点未投影运行态:${dot.className} / ${dot.dataset.state}`);
  assert(glyph.dataset.state === "waiting", `#7 等首 token 时活动行点应呼吸(waiting),实为 ${glyph.dataset.state}`);
  assert(label.textContent.includes("运行中") || label.textContent.includes("Running"), `#7 活动行文案未复用状态栏存源:${label.textContent}`);
  reasoningEv({ payload: { sessionId: "sess-smoke", text: "先想一想\n再想一想" } });
  assert(phase() === "thinking", `#7 kz:reasoning 后应为 thinking,实为 ${phase()}`);
  assert(/思考中|Thinking/.test(label.textContent), `#7 思考中活动行文案不对:${label.textContent}`);
  const liveHead = chatNs.currentReasoningHead;
  assert(liveHead?.classList.contains("is-live"), "#7 正在流的思考块头没有 is-live(扫光挂不上)");
  // 同一条线运行中的纠偏(process_list 轮询/逐事件投影的 setRunning(true))保留本轮细分相位,不把「思考中」打回等首 token。
  sandbox.setRunning(true, "运行中");
  assert(phase() === "thinking", `#7 运行中纠偏 setRunning(true) 把本轮相位重置成了 ${phase()}`);
  toolStartEv({ payload: { id: "MOT-T1", name: "bash", summary: "sleep 1", input: { command: "sleep 1" }, sessionId: "sess-smoke" } });
  assert(phase() === "tool", `#7 kz:tool-start 后应为 tool,实为 ${phase()}`);
  assert(!liveHead.classList.contains("is-live"), "#7 工具开始后上一段思考块仍标 is-live(会一直扫光)");
  const runningBlock = chatNs.chatToolBlocks.get("MOT-T1");
  assert(runningBlock?.wrap.classList.contains("running"), "#7 实时工具块未进入运行态(转圈挂不上)");
  // 后台线的思考事件走 withSessionRender,渲染进它自己的 pane,但不得改写活动线相位。
  // 先让后台线处于未收敛的运行态:已收敛会话的迟到进度事件会在路由层整条丢弃,守卫就测不到了。
  vm.runInContext('transitionSession("sess-bg", "running")', sandbox);
  reasoningEv({ payload: { sessionId: "sess-bg", text: "后台线在想" } });
  assert(phase() === "tool", `#7 后台线的 kz:reasoning 把活动线相位改成了 ${phase()}(串线)`);
  vm.runInContext('transitionSession("sess-bg", "idle")', sandbox);

  // ② 工具行实时收尾:成功弹一下、失败抖一下;flush 后摘除。
  toolEndEv({ payload: { id: "MOT-T1", name: "bash", ok: true, preview: "done", display: null, sessionId: "sess-smoke" } });
  assert(runningBlock.icon.classList.contains("kz-pop"), "#7 实时成功收尾的工具行没有播 kz-pop");
  assert(phase() === "waiting", `#7 最后一个工具结束后应回到 waiting,实为 ${phase()}`);
  await flush();
  assert(!runningBlock.icon.classList.contains("kz-pop"), "#7 kz-pop 一次性类没有被摘除");
  toolStartEv({ payload: { id: "MOT-T2", name: "bash", summary: "false", input: { command: "false" }, sessionId: "sess-smoke" } });
  const failBlock = chatNs.chatToolBlocks.get("MOT-T2");
  toolEndEv({ payload: { id: "MOT-T2", name: "bash", ok: false, preview: "exit code: 1", display: null, sessionId: "sess-smoke" } });
  assert(failBlock?.icon.classList.contains("kz-shake"), "#7 实时失败收尾的工具行没有播 kz-shake");
  await flush();
  assert(!failBlock.icon.classList.contains("kz-shake"), "#7 kz-shake 一次性类没有被摘除");
  // 历史回放走 renderMessageParts → fillToolBlock,不经过 chatToolEnd:重开对话不能满屏乱跳。
  vm.runInContext(`withSessionRender("sess-motion-history", () => renderMessageParts([
    { role: "assistant", parts: [
      { type: "reasoning", text: "历史思考第一行\\n历史思考第二行" },
      { type: "tool_call", id: "MH1", name: "read", input: { path: "a.rs" } },
      { type: "tool_result", call_id: "MH1", is_error: false, content: "ok" },
      { type: "tool_call", id: "MH2", name: "bash", input: { command: "false" } },
      { type: "tool_result", call_id: "MH2", is_error: true, content: "exit code: 1" },
    ] },
  ]))`, sandbox);
  const historyPane = vm.runInContext('messagePanes.get("sess-motion-history")', sandbox);
  const historyIcons = historyPane?.querySelectorAll(".tool-msg-status") ?? [];
  assert(historyIcons.length === 2, `#7 历史回放夹具应渲染 2 个工具块,实得 ${historyIcons.length}`);
  assert(
    historyIcons.every((icon) => !icon.classList.contains("kz-pop") && !icon.classList.contains("kz-shake")),
    "#7 历史回放的工具行播了一次性动效(只该在实时收尾时播)",
  );
  assert(!historyPane.querySelector(".reasoning-head.is-live"), "#7 历史思考块被标成 is-live(重开对话会一直扫光)");
  vm.runInContext('messagePanes.get("sess-motion-history")?.remove(); messagePanes.delete("sess-motion-history"); dropSessionStream("sess-motion-history")', sandbox);

  // ③ 停止收尾:运行中的块不会再等到 ToolEnd,必须停在「中断」;后台线终态由路由分支收尾它自己的 pane。
  toolStartEv({ payload: { id: "MOT-T3", name: "bash", summary: "sleep 60", input: { command: "sleep 60" }, sessionId: "sess-smoke" } });
  const stopBlock = chatNs.chatToolBlocks.get("MOT-T3");
  assert(stopBlock?.wrap.classList.contains("running"), "#7 前置:MOT-T3 应在运行");
  handlers.get("kz:stopped")({ payload: { sessionId: "sess-smoke", cancelled_queue: 0 } });
  assert(!stopBlock.wrap.classList.contains("running"), "#7 kz:stopped 后主对话工具行仍在转圈");
  assert(stopBlock.wrap.classList.contains("interrupted"), "#7 被停止的工具行没有标 interrupted");
  assert(/无结果\(轮次中断\)|No result \(round interrupted\)/.test(stopBlock.result.textContent) && !stopBlock.result.classList.contains("hidden"), `#7 被停止的工具行 ⎿ 行不对:${stopBlock.result.textContent}`);
  assert(stopBlock.icon.textContent === "⏹", `#7 被停止的工具行字形应为 ⏹,实为 ${stopBlock.icon.textContent}`);
  assert(row.classList.contains("hidden") && html.dataset.kzActivity === "idle", "#7 停止后运行活动行未收起");
  assert(dot.dataset.flash === undefined, "#7 用户自己按的停止不该播轮末反馈");
  // 已停止之后才到的 ToolEnd(停止补发):只收尾工具行——真结果替换「中断」、不播一次性动效——
  // 不得让活动行/状态点/状态栏翻回运行中(否则一直扫光到下一次 setRunning(false))。
  toolEndEv({ payload: { id: "MOT-T3", name: "bash", ok: false, preview: "killed", display: null, sessionId: "sess-smoke" } });
  assert(row.classList.contains("hidden") && html.dataset.kzActivity === "idle" && dot.dataset.state === "idle", `#7 已停止后迟到的 kz:tool-end 让运行活动复活:${phase()} / ${html.dataset.kzActivity} / ${dot.dataset.state}`);
  assert(!byId.get("statusbar").classList.contains("running"), "#7 已停止后迟到的 kz:tool-end 把状态栏翻回了运行中");
  assert(!stopBlock.wrap.classList.contains("interrupted") && stopBlock.wrap.classList.contains("err"), `#7 迟到的真结果没有替换「中断」标记:${stopBlock.wrap.className}`);
  assert(!stopBlock.icon.classList.contains("kz-shake") && !stopBlock.icon.classList.contains("kz-pop"), "#7 停止之后迟到的收尾播了一次性动效(停止是用户自己按的)");
  await flush();
  vm.runInContext('transitionSession("sess-smoke", "running")', sandbox);
  sandbox.setRunning(true, "运行中");
  vm.runInContext('withSessionRender("sess-bg", () => chatToolStart("MOT-BG1", "bash", "sleep 60", { command: "sleep 60" }))', sandbox);
  toolStartEv({ payload: { id: "MOT-T4", name: "bash", summary: "sleep 5", input: { command: "sleep 5" }, sessionId: "sess-smoke" } });
  const bgBlock = chatNs.chatToolBlocks.get("MOT-BG1");
  const fgBlock = chatNs.chatToolBlocks.get("MOT-T4");
  assert(bgBlock?.wrap.classList.contains("running") && fgBlock?.wrap.classList.contains("running"), "#7 前置:前后台两个工具块都应在运行");
  assert(chatNs.paneHasRunningTool() === true, "#7 paneHasRunningTool 没认出活动 pane 里在跑的块");
  handlers.get("kz:error")({ payload: { sessionId: "sess-bg", message: "后台线出错", terminal: true } });
  await flush();
  assert(bgBlock.wrap.classList.contains("interrupted") && !bgBlock.wrap.classList.contains("running"), "#7 后台线终态出错后,它 pane 里的工具行仍在转圈");
  assert(fgBlock.wrap.classList.contains("running"), "#7 后台线的终态把活动线还在跑的工具行也收尾了(串线)");
  toolEndEv({ payload: { id: "MOT-T4", name: "bash", ok: true, preview: "ok", display: null, sessionId: "sess-smoke" } });
  await flush();
  vm.runInContext('transitionSession("sess-bg", "idle")', sandbox);

  // ④ 状态点轮末反馈:完成/失败一次性,停止不播。
  sandbox.notifyRunState("completed", "动效冒烟");
  assert(dot.dataset.flash === "completed", `#7 轮末完成未挂一次性反馈,flash=${dot.dataset.flash}`);
  await flush();
  assert(dot.dataset.flash === undefined, "#7 轮末完成反馈没有被摘除");
  sandbox.notifyRunState("failed", "动效冒烟");
  assert(dot.dataset.flash === "failed", "#7 轮末失败未挂一次性反馈");
  await flush();
  sandbox.notifyRunState("stopped", "动效冒烟");
  assert(dot.dataset.flash === undefined, "#7 停止不该播轮末反馈");

  // ⑤ 等下一轮 / 停止中:活动行保留、警示色;回到空闲收起。
  sandbox.setRunPending("鞭挞 · 等待下一轮");
  assert(phase() === "pending" && html.dataset.kzActivity === "pending" && !row.classList.contains("hidden"), `#7 等下一轮时活动行应为 pending,实为 ${phase()} / ${html.dataset.kzActivity}`);
  assert(dot.dataset.state === "pending", `#7 等下一轮时状态点应为 pending,实为 ${dot.dataset.state}`);
  sandbox.setStopping("停止中…");
  assert(phase() === "stopping" && dot.dataset.state === "stopping", `#7 停止中相位不对:${phase()} / ${dot.dataset.state}`);
  // 「停止中」粘滞:停止发出后迟到的思考/工具收尾不得把活动行翻回运行态,文案仍是「停止中…」(与停止按钮一致)。
  sandbox.setTurnPhase("thinking");
  assert(phase() === "stopping", `#7 停止中被 setTurnPhase 覆盖成了 ${phase()}`);
  toolEndEv({ payload: { id: "MOT-T-STOPPING", name: "bash", ok: true, preview: "ok", display: null, sessionId: "sess-smoke" } });
  assert(phase() === "stopping" && dot.dataset.state === "stopping" && html.dataset.kzActivity === "stopping", `#7 停止中迟到的 kz:tool-end 把相位翻回了 ${phase()} / ${html.dataset.kzActivity}`);
  assert(/停止中|Stopping/.test(label.textContent), `#7 停止中迟到的 kz:tool-end 把活动行文案改成了「${label.textContent}」`);
  sandbox.setRunning(false, "空闲");
  assert(row.classList.contains("hidden") && html.dataset.kzActivity === "idle" && dot.dataset.state === "idle", "#7 回到空闲后活动行/状态点未复位");
  await flush();

  // ⑥ 侧栏线路字形:独立 .kz-glyph,逐事件投影原地更新(同一节点),整行文案逐字不变。
  vm.runInContext('transitionSession("sess-bg", "running")', sandbox);
  sandbox.renderParallelTaskStatus(shellNs.processItems);
  const bgRow = () => [...document.querySelectorAll("#parallel-task-status .parallel-task-row")].find((r) => r.dataset.processId === "p|bg");
  // UI-0926 #4 起字形是行首独立节点(行 = 字形 + 身份·名称 + 分支 + 行尾单个状态词)。
  const glyphOf = () => bgRow()?.querySelector(".kz-glyph");
  const g1 = glyphOf();
  assert(g1?.dataset.state === "running" && g1.textContent === "●", `#7 运行中线路的字形不对:${g1?.dataset.state} ${g1?.textContent}`);
  assert(/^-?\d+ms$/.test(g1.style.getPropertyValue("--kz-sync")), `#7 线路字形没有对齐全局相位(--kz-sync=${g1.style.getPropertyValue("--kz-sync")})`);
  assert(g1.getAttribute("aria-hidden") === "true", "#7 线路字形应对读屏隐藏(文案已说明状态)");
  const runningWord = bgRow()?.querySelector(".parallel-task-state")?.textContent ?? "";
  assert(bgRow().children[0] === g1 && runningWord && !/空闲|Idle/.test(runningWord), `#7 运行中线路应是「● … 状态词」且状态词不是空闲:${bgRow().textContent}`);
  sandbox.refreshParallelTaskProjection("sess-bg");
  sandbox.refreshParallelTaskProjection("sess-bg");
  assert(glyphOf() === g1, "#7 逐事件投影重建了线路字形节点(呼吸动画每个事件都从第 0 帧重来)");
  vm.runInContext('transitionSession("sess-bg", "idle")', sandbox);
  sandbox.refreshParallelTaskProjection("sess-bg");
  assert(glyphOf() === g1 && g1.dataset.state === "idle" && g1.textContent === "○", `#7 转空闲后字形未原地更新:${g1.dataset.state} ${g1.textContent}`);
  assert(!bgRow().textContent.includes("●"), "#7 空闲线路仍带运行标记");

  // ⑦ 各线在做:线真在跑才 is-live;kz:idle 收敛后摘除。批次格给「正在推的那一格」。
  sandbox.renderFocusPanel(payloads.docs_snapshot);
  const bgFocus = () => [...document.querySelectorAll("#focus-body .line-focus")].find((node) => node.dataset.processId === "p|bg");
  assert(bgFocus(), "#7 前置:焦点区缺后台线路分组");
  assert(!bgFocus().classList.contains("is-live"), "#7 空闲线路的焦点分组不该 is-live");
  vm.runInContext('transitionSession("sess-bg", "running")', sandbox);
  sandbox.renderParallelTaskStatus(shellNs.processItems);
  assert(bgFocus()?.classList.contains("is-live"), "#7 后台线运行中,焦点分组没有 is-live");
  handlers.get("kz:idle")({ payload: { reason: "completed", sessionId: "sess-bg" } });
  await flush();
  assert(!bgFocus()?.classList.contains("is-live"), "#7 kz:idle 收敛后焦点分组仍 is-live");
  const cellsOf = (host) => host.querySelectorAll(".complexity-cell");
  const midCard = sandbox.buildFocusCard(docEntry("R-M01", "动效批次", "doing", { batches: { done: 3, total: 11 } }), "req");
  const midCells = cellsOf(midCard);
  const midCurrent = midCells.map((cell, index) => (cell.classList.contains("current") ? index : -1)).filter((index) => index >= 0);
  assert(midCurrent.length === 1 && midCurrent[0] === 3, `#7 3/11 批的卡片应恰有第 4 格为 current,实为 ${JSON.stringify(midCurrent)}`);
  const doneCard = sandbox.buildFocusCard(docEntry("R-M02", "动效批次完成", "doing", { batches: { done: 11, total: 11 } }), "req");
  assert(!cellsOf(doneCard).some((cell) => cell.classList.contains("current")), "#7 批次已全部完成还标了 current 格");
  const listHost = document.createElement("div");
  sandbox.renderDocList(listHost, [docEntry("R-M03", "动效列表批次", "doing", { batches: { done: 2, total: 5 } })], "req");
  const listCurrent = cellsOf(listHost).map((cell, index) => (cell.classList.contains("current") ? index : -1)).filter((index) => index >= 0);
  assert(listCurrent.length === 1 && listCurrent[0] === 2, `#7 列表 2/5 批应恰有第 3 格为 current,实为 ${JSON.stringify(listCurrent)}`);

  // ⑧ 窗口隐藏即暂停(直接调函数,不派发 visibilitychange,免得触发语音/OC 的副作用)。
  assert(html.dataset.kzMotion === "live", `#7 启动后 html[data-kz-motion] 应为 live,实为 ${html.dataset.kzMotion}`);
  document.hidden = true;
  sandbox.syncMotionVisibility();
  assert(html.dataset.kzMotion === "paused", "#7 窗口隐藏后动画没有暂停");
  document.hidden = false;
  sandbox.syncMotionVisibility();
  assert(html.dataset.kzMotion === "live", "#7 窗口恢复后动画没有恢复");

  // ⑨ rail 徽标:面板收起时右上角提示「有东西在跑」。
  sandbox.bgAbortRunning("(动效冒烟前置)");
  const activityToggle = byId.get("activity-toggle");
  assert(activityToggle.dataset.running === "false", `#7 活动面板无运行项时徽标应熄灭,实为 ${activityToggle.dataset.running}`);
  sandbox.bgAdd("MOT-BG-RAIL", "bash", "sleep 3", { command: "sleep 3" }, "sess-smoke");
  assert(activityToggle.dataset.running === "true", "#7 活动面板有运行项时 rail 徽标未点亮");
  sandbox.bgEnd("MOT-BG-RAIL", true, "ok", null, "success");
  assert(activityToggle.dataset.running === "false", "#7 运行项结束后 rail 徽标未熄灭");
  const agentToggle = byId.get("agent-toggle");
  sandbox.agentStart("AG-MOTION", "task", "动效徽标", { prompt: "motion" }, "sess-smoke");
  assert(agentToggle.dataset.running === "true", "#7 子代理运行时 rail 徽标未点亮");
  sandbox.agentEnd("AG-MOTION", true, "done", null);
  const stillRunningAgents = [...sandbox.agentEntries.values()].some((entry) => entry.state === "running");
  assert(agentToggle.dataset.running === String(stillRunningAgents), `#7 子代理结束后 rail 徽标与实际运行数不符:${agentToggle.dataset.running}`);

  // ⑩ 计数 tick:鞭挞轮次上升时 tick 一次,值不变的无参重绘不 tick。
  const roundNow = byId.get("auto-round-now");
  const savedRounds = vm.runInContext("currentAutoRounds()", sandbox);
  sandbox.setAutoRounds("sess-smoke", 2);
  sandbox.renderAutoRun();
  await flush();
  sandbox.setAutoRounds("sess-smoke", 3);
  sandbox.renderAutoRun();
  assert(roundNow.textContent === "3" && roundNow.classList.contains("kz-tick"), `#7 鞭挞轮次上升未 tick:${roundNow.textContent} ${roundNow.className}`);
  await flush();
  assert(!roundNow.classList.contains("kz-tick"), "#7 kz-tick 一次性类没有被摘除");
  sandbox.renderAutoRun();
  assert(!roundNow.classList.contains("kz-tick"), "#7 轮次没变的无参重绘也 tick 了");
  sandbox.setAutoRounds("sess-smoke", savedRounds);
  sandbox.renderAutoRun();
  await flush();

  // ⑪ 上下文条等本轮 usage 时慢呼吸(与 kz:step 同一条 setCtxPending(false) + renderTokens 路径)。
  sandbox.setCtxLimit(100000);
  sandbox.setCtxTokens(5000);
  sandbox.setCtxPending(true);
  sandbox.renderTokens();
  assert(byId.get("ctx-bar").classList.contains("pending"), "#7 等本轮 usage 时上下文条没有 pending");
  sandbox.setCtxPending(false);
  sandbox.renderTokens();
  assert(!byId.get("ctx-bar").classList.contains("pending"), "#7 usage 到达后上下文条仍 pending");
  sandbox.setCtxLimit(savedCtx.limit);
  sandbox.setCtxTokens(savedCtx.tokens);
  sandbox.setCtxPending(savedCtx.pending);
  sandbox.renderTokens();

  // ⑫ 自动放行由关变开弹一次;进行中的按钮 aria-busy(统一转圈)。
  const autoAllow = byId.get("auto-allow");
  const badge = byId.get("status-auto-allow");
  const savedAllow = autoAllow.checked;
  autoAllow.checked = true;
  autoAllow.dispatchEvent({ type: "change" });
  assert(badge.classList.contains("kz-pop"), "#7 自动放行开启时徽标没有弹一下");
  await flush();
  autoAllow.checked = savedAllow;
  autoAllow.dispatchEvent({ type: "change" });
  await flush();
  const savedUpdateCheck = payloads.update_check;
  payloads.update_check = { current: "0.0.0", newer: false };
  let releaseUpdate;
  invokeGates.set("update_check", new Promise((resolve) => { releaseUpdate = resolve; }));
  byId.get("update-check").click();
  assert(byId.get("update-check").getAttribute("aria-busy") === "true", "#7 检查更新进行中按钮没有 aria-busy(不转圈)");
  invokeGates.delete("update_check");
  releaseUpdate();
  await flush();
  assert(byId.get("update-check").getAttribute("aria-busy") === null, "#7 检查更新结束后 aria-busy 没有撤掉(会一直转)");
  if (savedUpdateCheck === undefined) delete payloads.update_check;
  else payloads.update_check = savedUpdateCheck;

  // 收尾:恢复进程列表与会话状态,后续分区不继承本组的运行态。
  vm.runInContext('transitionSession("sess-smoke", "idle"); transitionSession("sess-bg", "idle")', sandbox);
  sandbox.setRunning(false, "空闲");
  payloads.process_list = savedProcessList;
  sandbox.renderProcesses(structuredClone(savedProcessList));
  await flush();
}

// ---------- UI-0926 ESM 回归:这些调用曾经在真机上从未执行(globalThis 上根本没有它们) ----------
// 冒烟 sandbox 把全部 ESM 导出复制成全局,`typeof globalThis.X === "function"` 的死调用在这里
// 一直是绿的。这一段先把这些名字从 sandbox 全局摘掉(= 真机浏览器的样子),再走真实事件路径,
// 用它们的**副作用**证明调用确实发生了。ui-lint 的静态守卫拦写法,这里拦行为。
{
  const DEAD_NAMES = [
    "refreshParallelTaskProjection", "refreshConversationLists", "handleBackgroundSessionDone",
    "cancelAutoContinueTimer", "focusForProcess", "fastStatusText", "markLanguagePreferenceDirty",
  ];
  const stash = new Map(DEAD_NAMES.map((name) => [name, sandbox[name]]));
  const esmLines = [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
    { id: "p|esm-bg", label: "ESM 后台线", session_id: "sess-esm-bg", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
  ];
  const savedEsmProcessList = payloads.process_list;
  payloads.process_list = structuredClone(esmLines);
  sandbox.renderProcesses(structuredClone(esmLines));
  await flush();
  for (const name of DEAD_NAMES) delete sandbox[name];
  try {
    assert(DEAD_NAMES.every((name) => vm.runInContext(`typeof globalThis.${name}`, sandbox) === "undefined"), "ESM 回归前置:sandbox 全局未摘干净");
    kzTest.setAutoState("p|esm-bg", { enabled: true, paused: false, stopAfterRound: false, maxRounds: 10 });
    const esmGlyph = () => [...document.querySelectorAll("#parallel-task-status .parallel-task-row")]
      .find((row) => row.dataset.processId === "p|esm-bg")?.querySelector(".kz-glyph");
    assert(esmGlyph()?.dataset.state === "idle", `ESM 回归前置:后台线应为空闲,实为 ${esmGlyph()?.dataset.state}`);
    // ① 逐事件线路投影:后台线的 kz:turn 只进路由层(不进 handler、不整表重绘),线路行只能靠它点亮。
    handlers.get("kz:turn")({ payload: { step: 1, maxSteps: 0, sessionId: "sess-esm-bg" } });
    assert(esmGlyph()?.dataset.state === "running", "ESM 回归:后台线 kz:turn 后线路行没亮——逐事件投影(refreshParallelTaskProjection)没执行");
    // ② 后台线轮末续跑:kz:done 在路由层转给 handleBackgroundSessionDone,由它排下一轮。
    const conversationListsBefore = invokeArgs.length;
    handlers.get("kz:done")({ payload: { steps: 1, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: "sess-esm-bg" } });
    assert(kzTest.timerSessions().includes("sess-esm-bg"), "ESM 回归:后台线 kz:done 没有排续跑——handleBackgroundSessionDone 没执行");
    assert(sandbox.sessionState("sess-esm-bg").phase === "auto_pending", "ESM 回归:后台线 kz:done 后没进入等待下一轮");
    await settle();
    assert(
      invokeArgs.slice(conversationListsBefore).some(({ cmd }) => cmd === "conversation_list"),
      "ESM 回归:后台线控制事件没有刷新会话列表——refreshConversationLists 没执行",
    );
    // ③ 后台线停止必须取消已排的续跑定时器,否则停下的线过 2 秒又自己跑起来。
    if (!kzTest.timerSessions().includes("sess-esm-bg")) sandbox.armAutoContinue(sandbox.continuePrompt(), "sess-esm-bg");
    assert(kzTest.timerSessions().includes("sess-esm-bg"), "ESM 回归前置:续跑定时器应已排上");
    handlers.get("kz:stopped")({ payload: { sessionId: "sess-esm-bg" } });
    assert(!kzTest.timerSessions().includes("sess-esm-bg"), "ESM 回归:后台线停止后续跑定时器还在——cancelAutoContinueTimer 没执行");
    await flush();
    // ④ 状态栏 fast 模型:托管但服务未起时必须说出缺哪一环(死调用时这一格是空的)。
    await sandbox.refreshFastStatusBar();
    const fastText = byId.get("status-fast").textContent;
    assert(fastText.includes("⚠") && !byId.get("status-fast").classList.contains("hidden"), `ESM 回归:状态栏 fast 状态为空——fastStatusText 没执行(实得「${fastText}」)`);
    // ⑤ 设置页语言偏好:改了语言要标脏,否则保存设置时把旧语言写回去。
    await sandbox.loadSettings({ force: true });
    await flush();
    const settingsNs = esmModuleCache.get("16-settings.js")?.namespace;
    assert(settingsNs?.languagePreferenceDirty === false, "ESM 回归前置:载入设置后语言偏好不应是脏的");
    const languageSelect = byId.get("language-select");
    languageSelect.dispatchEvent({ type: "change" });
    assert(settingsNs.languagePreferenceDirty === true, "ESM 回归:改语言没有标脏——markLanguagePreferenceDirty 没执行,保存设置会写回旧语言");
    await sandbox.loadSettings({ force: true });
    await flush();
    // ⑥ 默认线「被取得」:无 claimed_by 的 doing 条目由默认线持有的判据读主线焦点(focusForProcess)。
    sandbox.renderProcesses(structuredClone(savedEsmProcessList));
    await flush();
    sandbox.renderDocuments(savedDocsPayload);
    sandbox.renderLines(payloads.collaboration_snapshot);
    assert(
      document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"] .doc-claim-fact'),
      "ESM 回归:默认线占着 R-001 却没有被取得标记——focusForProcess 没执行",
    );
  } finally {
    for (const [name, value] of stash) sandbox[name] = value;
    kzTest.cancelTimers();
    vm.runInContext('transitionSession("sess-esm-bg", "idle")', sandbox);
    payloads.process_list = savedEsmProcessList;
    sandbox.renderProcesses(structuredClone(savedEsmProcessList));
    await flush();
  }
}

// ===== 分区:子代理 =====

if (issues.length) {
  reportedIssues = true;
  console.error(`UI 运行时冒烟失败(${issues.length} 处):`);
  for (const issue of issues) console.error(` - ${issue}`);
  process.exit(1);
}
// Keep streaming/cancellation regressions in the existing frontend runtime gate.
await import("./ui-oc-companion-smoke.mjs");
await import("./ui-voice-smoke.mjs");
await import("./ui-mobile-approval-smoke.mjs");
console.log(
  `UI 运行时冒烟通过:${sources.length} 个 ui/*.js 按序执行 + 初始化序列(${invokeLog.length} 次 invoke) + ` +
  `需求/缺陷/目标/测试/历史列表渲染 + ${document.querySelectorAll(".activity-item[data-view]").length} 个主视图切换,0 运行时错误`
);
