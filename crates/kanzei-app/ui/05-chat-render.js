import { defer } from "./01-core.js";
import { messagePanes, motionOnce } from "./01-core.js";
import { setCurrentAssistant, setCurrentReasoning } from "./03-shell.js";
import { appendDisplayBlock, compactDiffLines, quotaNoticeHeadline, quotaTruncation } from "./06-activity.js";
import { openTasksPanel } from "./06-agent-panel.js";
import { $, activePane, promptBox, appendToPane, messages, trimLivePane } from "./01-core.js";
import { t } from "./02-i18n.js";
import { attachments, currentAssistant, currentReasoning, lastRequest, log } from "./03-shell.js";
import { renderMarkdown } from "./04-markdown.js";
import { parseJsonish, stripToolOutcome } from "./04-structured-parse.js";
import { flushLazy, lazyMount, renderErrorDetail, renderToolArgs, renderToolResult } from "./04-structured.js";
import { renderToolSummary, toolArgSummary, toolResultSummary, withToolDuration } from "./05-tool-summary.js";
import { sendText } from "./08-compose-runtime.js";
import { loadEarlierMessages } from "./15-views-misc.js";

// ---------- 消息渲染 ----------
export function clearEmptyState() {
  // R-267:空状态改为每个 pane 各有一份(按类名在 pane 内定位)。原来用 id 定位,
  // 多 pane 下会出现重复 id,而且清的可能是别的会话那一份。
  const empty = activePane?.querySelector(".empty-state");
  if (empty) empty.remove();
}

export let followLatest = true;
export function setFollowLatest(value) { followLatest = value; }
export function nearBottom() {
  return messages.scrollHeight - messages.scrollTop - messages.clientHeight < 48;
}
export function updateLatestButton() {
  // 容错而非兜底修复:按钮一旦不在 DOM 里(例如又被挪进 #messages 然后被
  // innerHTML 清掉),这里只是不更新,不能把整条滚动/渲染链路一起拖崩——
  // 2026-08-12 就是这么炸的:恢复历史、新建并行线路全报 null.classList。
  const button = $("jump-latest");
  if (button) button.classList.toggle("hidden", followLatest);
}
// 我们自己写 scrollTop 时浏览器同样会发 scroll 事件。合帧之后这变成了一个陷阱:
// 一帧里追加几十条,帧末只滚一次;那一次滚动产生的 scroll 事件到达时,后面又追加了
// 更多内容,nearBottom() 已经是 false —— 于是「跟随」被自己的滚动关掉,而关掉之后
// flushScrollBottom 不再滚,再也回不来。实测:纯流式、用户零操作,700 条之后离底
// 20635px 且 followLatest 永久为 false(顺带让 pane 进入「读历史」态、裁剪推迟到硬顶)。
// 判据:落点正好等于我们刚写进去的值 = 这不是人滚的,不参与跟随态重算。
/// 「这次滚动是谁干的」不能靠比对 scrollTop 数值来判断:一帧里可能连着发生两次
/// 程序滚动(裁剪时浏览器自己夹一次 + flushScrollBottom 钉底一次),两个 scroll 事件
/// 的投递顺序与 rAF 回调先后并不保证,记下的值在事件送达前就已经被下一次覆盖——
/// 实测差 49px 就认不出来,于是被当成「用户往上滚了」,跟随态从此关掉再也回不来。
///
/// 改用意图判定:自己滚过之后的一小段时间内不重算跟随态;而**真实手势**(滚轮、
/// 按住滚动条、触摸、按键)立刻作废这个窗口,让用户随时能滚上去停住。
/// 退化方向是安全的:万一漏掉某种手势,最坏也只是晚 120ms 才认出用户在读历史。
export const PROGRAMMATIC_SCROLL_WINDOW_MS = 120;
export let programmaticUntil = 0;
export function noteProgrammaticScroll() {
  programmaticUntil = Date.now() + PROGRAMMATIC_SCROLL_WINDOW_MS;
}
defer(() => {
  for (const gesture of ["wheel", "pointerdown", "touchstart", "keydown"]) {
    messages.addEventListener(gesture, () => { programmaticUntil = 0; }, { passive: true });
  };
});
defer(() => {
  messages.addEventListener("scroll", () => {
    if (Date.now() < programmaticUntil) {
      updateLatestButton();
      return;
    }
    const wasReading = !followLatest;
    followLatest = nearBottom();
    updateLatestButton();
    // 读历史期间 trimLivePane 会让步(见 01-core.js);人滚回底部了就把欠下的那次补上,
    // 否则跑一整轮回来 pane 还挂在硬顶上。
    if (wasReading && followLatest && typeof trimLivePane === "function") trimLivePane(activePane);
    // R-267 批2:触顶自动补齐上一窗。按钮仍在(可点),但滚上去就该出来,
    // 不该让人先找到按钮再点——这是「更丝滑」的一部分。
    if (messages.scrollTop < 80 && typeof loadEarlierMessages === "function") loadEarlierMessages();
  });
});
/// 跟随滚动按帧合并。`messages.scrollTop = messages.scrollHeight` 是一次**强制同步
/// 布局**,而它原先挂在每一条消息、每一个工具块的追加之后:一轮里几十个块 = 几十次
/// 全树布局,DOM 越长每次越贵(实测见 01-core.js trimLivePane 的数字)。
/// 一帧内追加多少条都只在帧末滚一次;force 在合并窗口里是**粘性**的——历史恢复那种
/// 「必须落到底」的请求不能被同帧的普通追加冲掉。
/// requestAnimationFrame 不可用时(冒烟的假 DOM)退回同步,行为与改造前一致。
export let scrollFlushScheduled = false;
export let scrollFlushForce = false;
export function flushScrollBottom() {
  scrollFlushScheduled = false;
  const force = scrollFlushForce;
  scrollFlushForce = false;
  if (force || followLatest) {
    messages.scrollTop = messages.scrollHeight;
    noteProgrammaticScroll();
  }
  updateLatestButton();
}
export function scrollBottom(force = false) {
  if (force) scrollFlushForce = true;
  if (typeof requestAnimationFrame !== "function") {
    flushScrollBottom();
    return;
  }
  if (scrollFlushScheduled) return;
  scrollFlushScheduled = true;
  requestAnimationFrame(flushScrollBottom);
}
export function copyButton() {
  const button = document.createElement("button");
  button.className = "copy-btn";
  button.type = "button";
  button.textContent = t("复制");
  button.title = t("复制消息");
  // R-140 批1/批10:消息容器豁免 observer 后,容器内 t() 渲染点靠 data-i18n-key
  // 在语言切换时由 applyDataI18nKeys(document.body) 重算(渲染点翻译,不再靠事后回译)。
  button.dataset.i18nKey = "复制";
  button.dataset.i18nTitle = "复制消息";
  return button;
}

export function addMessage(cls, text) {
  clearEmptyState();
  const el = document.createElement("div");
  el.className = `msg ${cls}`;
  const body = document.createElement("div");
  body.className = "message-body";
  body.textContent = text;
  const actions = document.createElement("span");
  actions.className = "msg-actions";
  actions.appendChild(copyButton());
  el.append(body, actions);
  appendToPane(el);
  scrollBottom();
  return el;
}

export function addUserMessage(text, promptAttachments = []) {
  const el = addMessage("user", text);
  if (promptAttachments.length === 0) return el;
  const body = el.querySelector(".message-body");
  const attachments = document.createElement("div");
  attachments.className = "message-attachments";
  for (const attachment of promptAttachments) {
    const item = document.createElement("span");
    item.className = "message-attachment";
    const kind = attachment.media_type?.startsWith("image/") ? t("图片") : "PDF";
    item.textContent = `${attachment.file_name} · ${kind} · ${t("已发送给 agent")}`;
    attachments.appendChild(item);
  }
  body.appendChild(attachments);
  return el;
}

export function addErrorMessage(message, { retryable = false } = {}) {
  const el = addMessage("error", "");
  const body = el.querySelector(".message-body");
  const contextOverflow = /context[_ ]length|context overflow|prompt is too long|input is too long|上下文.{0,4}(过长|超限)/i.test(message);
  const level = document.createElement("strong");
  level.className = "error-level";
  const levelKey = contextOverflow ? "可压缩重试" : retryable ? "可重试错误" : "致命错误";
  level.textContent = t(levelKey);
  // R-140 批1:记录级别 key,语言切换时由渲染点重算(同 copy-btn)。
  level.dataset.i18nKey = levelKey;
  // UI-0926 #10:provider 的 HTTP 错误体/错误链拆成人话消息 + chips + 原因链,原文留在
  // dataset.raw(复制走原文)。
  body.append(level, renderErrorDetail(message));
  el.dataset.raw = String(message ?? "");
  if (retryable && lastRequest) {
    const actions = el.querySelector(".msg-actions");
    const retry = document.createElement("button");
    retry.className = "retry-btn";
    retry.type = "button";
    retry.textContent = t("重试上一次请求");
    retry.addEventListener("click", () => {
      retry.disabled = true;
      retry.textContent = t("正在重试…");
      sendText(lastRequest.prompt, { promptAttachments: lastRequest.attachments });
    });
    actions.appendChild(retry);
  }
  return el;
}

export function isRetryableError(message) {
  return /timed out|timeout|connect|connection|dns|网络|连接|超时|context[_ ]length|context overflow|prompt is too long|input is too long|上下文.{0,4}(过长|超限)/i.test(message);
}

export function reportError(message, { retryable = isRetryableError(message) } = {}) {
  addErrorMessage(message, { retryable });
  log(`${t("错误")}:${message}`, "err");
}

export let outputChars = 0;
export function setOutputChars(value) { outputChars = Number(value) || 0; }
// ---------- D-202:流式渲染合帧 ----------
// 原先每个 delta 都要:整条 renderMarkdown + 整块 innerHTML + 把整条 raw split 一遍
// + 读 scrollHeight(强制同步重排整个消息列表)。前三项在单条消息内是 O(n²),
// 最后一项随轮次增长——流一开就把主线程占满。现在 delta 只累加文本,渲染压到
// 每帧最多一次;上一次渲染实测超过 8ms 就按实测耗时退避(长消息自动降频),
// 无论消息多长都给交互留得出时间片。D-728:合帧槽与调度标记按 pane 隔离,
// 后台会话的待渲染节点不会再覆盖活动会话的末帧。
export const pendingAssistantRender = new WeakMap();
export const pendingReasoningRender = new WeakMap();
export const streamFlushScheduled = new WeakSet();
export const streamRenderCost = new WeakMap();
export function scheduleStreamRender() {
  const pane = activePane;
  if (!pane || streamFlushScheduled.has(pane)) return;
  streamFlushScheduled.add(pane);
  const run = () => {
    streamFlushScheduled.delete(pane);
    flushStreamRender(pane);
  };
  const cost = streamRenderCost.get(pane) || 0;
  if (cost > 8) setTimeout(run, Math.min(250, Math.round(cost)));
  else requestAnimationFrame(run);
}
/// 把累计到的流式文本一次性渲染出去。目标元素可能已被收尾逻辑摘掉引用(甚至已从
/// DOM 摘除,如 stream-restart),照渲染即可——写进游离节点无害,少写一次分支。
/// D-728:传入 pane 固定本次 flush 的归属;切换线路后不向新 pane 滚动。
/// (侧栏「最近在说」#live-note 与对话流本身重复,UI-0926 #4 删掉。)
export function flushStreamRender(pane = activePane) {
  if (!pane) return;
  const assistant = pendingAssistantRender.get(pane);
  const reasoning = pendingReasoningRender.get(pane);
  pendingAssistantRender.delete(pane);
  pendingReasoningRender.delete(pane);
  if (!assistant && !reasoning) return;
  const started = Date.now();
  if (assistant) {
    assistant.querySelector(".message-body").innerHTML = renderMarkdown(assistant.dataset.raw);
  }
  if (reasoning) renderReasoningBlock(reasoning);
  streamRenderCost.set(pane, Date.now() - started);
  if (pane === activePane) scrollBottom();
}
export function appendAssistant(text) {
  if (!currentAssistant) {
    setCurrentAssistant(addMessage("assistant md", ""));
    currentAssistant.dataset.raw = "";
  }
  currentAssistant.dataset.raw += text;
  outputChars += text.length;
  pendingAssistantRender.set(activePane, currentAssistant);
  scheduleStreamRender();
}

// ---------- 主对话内联工具块(R-090):运行细节进对话流,主对话不再贫乏 ----------
// 形态对齐 Claude Code:一行 `工具名(主要参数)` + 一行 `⎿ 结果摘要`,详情默认折叠。
// 实时与历史回放共用同一个构造器,两处观感必须一致。

// ---------- 工具类型图标(按语义分组的 24×24 单色描边内联 SVG) ----------
// 与活动栏既有 SVG 同一套参数:fill=none / stroke=currentColor / stroke-width=1.7 /
// round 端点。stroke=currentColor 是「跟随主题」的全部机关——颜色只由 CSS 的 color 给,
// 亮暗主题切换零改动。口径:**图标标「组」,文字标「身份」**。同组多工具共用一个字形,
// 区分靠紧邻的 .tool-msg-name(D-105:不得只靠颜色/形状区分,文本始终在场)。
// 这条口径同时让 memory_* / ui_* / frontend_* 的前缀兜底成为正确行为而不是将就。
export const TOOL_ICON_PATHS = {
  file: "M7 3h6l4.2 4.2V20a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Z M13 3v4.5h4.2 M9 13h6 M9 16.5h4",
  folder: "M4 6a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6Z",
  braces: "M9.6 3.8c-2.2 0-2.2 2.6-2.2 4.4S6.6 11.4 5 11.4v1.2c1.6 0 2.4 1.4 2.4 3.2s0 4.4 2.2 4.4 M14.4 3.8c2.2 0 2.2 2.6 2.2 4.4s.8 3.2 2.4 3.2v1.2c-1.6 0-2.4 1.4-2.4 3.2s0 4.4-2.2 4.4",
  pencil: "M4 20.2h3.9L19.4 8.7a1.9 1.9 0 0 0 0-2.7l-1.4-1.4a1.9 1.9 0 0 0-2.7 0L4 16.3v3.9Z M14.6 5.9l3.5 3.5",
  terminal: "M3.5 5.5h17a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1h-17a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1Z M6.5 9.5 9.5 12l-3 2.5 M12.5 14.5h5",
  search: "M10.8 4a6.8 6.8 0 1 0 0 13.6 6.8 6.8 0 0 0 0-13.6Z M15.8 15.8 20.5 20.5",
  wildcard: "M12 4.5v15 M5.5 8.2l13 7.6 M18.5 8.2l-13 7.6",
  branch: "M7 4.6a2.2 2.2 0 1 0 0 4.4 2.2 2.2 0 0 0 0-4.4Z M7 15a2.2 2.2 0 1 0 0 4.4 2.2 2.2 0 0 0 0-4.4Z M17 4.6a2.2 2.2 0 1 0 0 4.4 2.2 2.2 0 0 0 0-4.4Z M7 9v6 M17 9v1.4c0 2.2-1.8 4-4 4h-2.4",
  link: "M10.2 13.8a4.2 4.2 0 0 0 6 0l2.6-2.6a4.2 4.2 0 0 0-6-6l-1.3 1.3 M13.8 10.2a4.2 4.2 0 0 0-6 0l-2.6 2.6a4.2 4.2 0 0 0 6 6l1.3-1.3",
  globe: "M12 3.2a8.8 8.8 0 1 0 0 17.6 8.8 8.8 0 0 0 0-17.6Z M3.4 9.6h17.2 M3.4 14.4h17.2 M12 3.2c2.4 2.6 3.6 5.6 3.6 8.8S14.4 18.2 12 20.8 8.4 15.2 8.4 12 9.6 5.8 12 3.2Z",
  clipboard: "M9.2 4.2h5.6v2.6H9.2z M15.6 5.5h1.9a1 1 0 0 1 1 1V20a1 1 0 0 1-1 1H6.5a1 1 0 0 1-1-1V6.5a1 1 0 0 1 1-1h1.9 M9 13.4l2 2 4-4",
  bug: "M8.4 9.2a3.6 3.6 0 0 1 7.2 0v3.4a3.6 3.6 0 0 1-7.2 0V9.2Z M9.4 7 8 5.4 M14.6 7 16 5.4 M8.4 11H5 M15.6 11H19 M8.7 14.6 5.8 16.6 M15.3 14.6l2.9 2 M12 16.2v4.4",
  target: "M12 3.6a8.4 8.4 0 1 0 0 16.8 8.4 8.4 0 0 0 0-16.8Z M12 8.4a3.6 3.6 0 1 0 0 7.2 3.6 3.6 0 0 0 0-7.2Z",
  fork: "M4 7.5h4.6l3.4 4.5 3.4-4.5H20 M17.2 4.7 20 7.5l-2.8 2.8 M4 16.5h4.6l1.9-2.5 M17.2 13.7 20 16.5l-2.8 2.8 M13.6 16.5H20",
  book: "M4.5 5.4A2.4 2.4 0 0 1 6.9 3h12.6v14.4H6.9a2.4 2.4 0 0 0-2.4 2.4V5.4Z M4.5 19.8A2.4 2.4 0 0 0 6.9 21h12.6v-3.6",
  bulb: "M12 3.2a6 6 0 0 0-3.4 10.9c.6.4 1 1.1 1 1.9h4.8c0-.8.4-1.5 1-1.9A6 6 0 0 0 12 3.2Z M9.8 18.4h4.4 M10.6 21h2.8",
  checklist: "M4 6.4 5.6 8l2.6-3 M4 12.4 5.6 14l2.6-3 M4 18.4 5.6 20l2.6-3 M11.4 6.6H20 M11.4 12.6H20 M11.4 18.6H20",
  inbox: "M4 14.6v3.9a1.5 1.5 0 0 0 1.5 1.5h13a1.5 1.5 0 0 0 1.5-1.5v-3.9 M8.2 10.4 12 14.2l3.8-3.8 M12 3.6v10.6",
  layers: "M12 3.2 20 7.6 12 12 4 7.6l8-4.4Z M4 12l8 4.4 8-4.4 M4 16.4l8 4.4 8-4.4",
  fanout: "M12 3.4a2.4 2.4 0 1 0 0 4.8 2.4 2.4 0 0 0 0-4.8Z M6 15.6a2.4 2.4 0 1 0 0 4.8 2.4 2.4 0 0 0 0-4.8Z M18 15.6a2.4 2.4 0 1 0 0 4.8 2.4 2.4 0 0 0 0-4.8Z M12 8.2v3.6 M6 15.6v-3.8h12v3.8",
  boxlines: "M4 6a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6Z M7 9h10 M7 12h6 M7 15h4",
  ruler: "M3.6 14.2 14.2 3.6a1.2 1.2 0 0 1 1.7 0l4.5 4.5a1.2 1.2 0 0 1 0 1.7L9.8 20.4a1.2 1.2 0 0 1-1.7 0l-4.5-4.5a1.2 1.2 0 0 1 0-1.7Z M8 9.8l2 2 M11 6.8l2 2 M14.2 13.2l2 2 M11.2 16.2l2 2",
  flask: "M9.8 3h4.4 M10.6 3v6.4L5.4 18a2 2 0 0 0 1.7 3h9.8a2 2 0 0 0 1.7-3l-5.2-8.6V3 M8 14.6h8",
  question: "M12 3.2a8.8 8.8 0 1 0 0 17.6 8.8 8.8 0 0 0 0-17.6Z M9.6 9.4a2.5 2.5 0 1 1 3.3 2.4c-.6.2-.9.8-.9 1.4v.7 M12 16.9h.01",
  window: "M3.6 5.4h16.8a1 1 0 0 1 1 1v11.2a1 1 0 0 1-1 1H3.6a1 1 0 0 1-1-1V6.4a1 1 0 0 1 1-1Z M2.6 9.6h18.8 M5.6 7.5h.01 M8 7.5h.01",
  people: "M9.4 4.8a3.2 3.2 0 1 0 0 6.4 3.2 3.2 0 0 0 0-6.4Z M3.4 20.4c0-3.3 2.7-5.2 6-5.2s6 1.9 6 5.2 M16.4 6.2a2.8 2.8 0 1 1 0 5.6 M17.4 14.4c2.4.4 4.2 2.1 4.2 5",
  wrench: "M14.6 4.4a4.6 4.6 0 0 0-5.7 5.7l-4.7 4.7a1.5 1.5 0 0 0 0 2.1l2.9 2.9a1.5 1.5 0 0 0 2.1 0l4.7-4.7a4.6 4.6 0 0 0 5.7-5.7l-3 3-2.8-.7-.7-2.8 3-3Z",
};
// 工具名 → [组, 字形]。组决定配色与语义归属,字形决定画什么。
export const TOOL_GROUPS = {
  read: ["read", "file"], files: ["read", "folder"], symbols: ["read", "braces"],
  write: ["write", "pencil"], edit: ["write", "pencil"], insert: ["write", "pencil"],
  multiedit: ["write", "pencil"],
  bash: ["exec", "terminal"], process: ["exec", "terminal"],
  grep: ["search", "search"], glob: ["search", "wildcard"],
  git: ["vcs", "branch"],
  webfetch: ["net", "link"], websearch: ["net", "globe"],
  req: ["tracker", "clipboard"], defect: ["tracker", "bug"], idea: ["tracker", "target"],
  decision: ["tracker", "fork"], source: ["tracker", "book"], finding: ["tracker", "bulb"],
  work: ["plan", "inbox"],
  task: ["agent", "fanout"],
  architecture: ["asset", "boxlines"], conventions: ["asset", "ruler"], test_record: ["asset", "flask"],
  question: ["ask", "question"],
  collaboration_status: ["collab", "people"],
};
// 前缀兜底:memory_* 与 ui_*/frontend_* 各自同组同字形——「图标标组」这条口径下
// 这是正确行为,顺带让后端新增同族工具时前端零改动、不落 wrench 兜底。
export const TOOL_GROUP_PREFIXES = [
  ["memory_", ["memory", "layers"]],
  ["ui_", ["ui", "window"]],
  ["frontend_", ["ui", "window"]],
];
export function toolGroupEntry(name) {
  const key = String(name ?? "").trim().toLowerCase();
  if (TOOL_GROUPS[key]) return TOOL_GROUPS[key];
  for (const [prefix, entry] of TOOL_GROUP_PREFIXES) if (key.startsWith(prefix)) return entry;
  return ["other", "wrench"];
}
/// 图标节点。innerHTML 拼的是常量路径字面量,没有任何外部输入进得来。
/// 组与字形一并写进 data-*:只断言画了图标看不出「画对了但归错组」。
export function toolIconNode(name) {
  const [group, icon] = toolGroupEntry(name);
  const span = document.createElement("span");
  span.className = `tool-icon tool-icon-${group}`;
  span.setAttribute("aria-hidden", "true"); // 纯装饰:身份由紧邻的工具名文本承载
  span.dataset.toolIcon = icon;
  span.dataset.toolGroup = group;
  span.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="${TOOL_ICON_PATHS[icon]}"/></svg>`;
  return span;
}

/// 工具调用的人类摘要:取该工具最有信息量的那个参数,而不是整坨 JSON。
/// 真源是 05-tool-summary.js 的 toolArgSummary(路径相对化、长正则/长命令智能截断);
/// 这里保留旧名供活动面板/事件日志调用。
export function toolCallSummary(name, input) {
  return toolArgSummary(name, input).text;
}

// 失败行的互斥切分(toolResultSplit)与 ⎿ 行预算搬进了 05-tool-summary.js(摘要器唯一真源),
// 这里原名转出,旧调用方与冒烟不必改。
export { TOOL_PREVIEW_MAX, toolResultSplit } from "./05-tool-summary.js";

export function displayNeedsActivityNotice(display) {
  if (!display) return false;
  if (display.kind === "diff") {
    const lines = Array.isArray(display.lines) ? display.lines : [];
    return Boolean(display.truncated) || (lines.length > 0 && compactDiffLines(lines).length < lines.length);
  }
  if (display.kind === "terminal") {
    const text = String(display.full ?? display.output ?? "");
    return text.length > 4000 || text.split("\n").length > 20;
  }
  if (display.kind === "create") {
    return String(display.preview ?? "").split("\n").length > 20;
  }
  return false;
}

export function appendActivityNotice(parent) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "ghost mini tool-display-more";
  button.textContent = t("去后台任务侧栏看全");
  button.title = t("去后台任务侧栏看全");
  button.addEventListener("click", (event) => {
    event.stopPropagation();
    // UI2-0926 #14:只打开、不切换——此前复用 rail 开关的 click,侧栏已开着时反而把它关掉(缺陷 C)。
    openTasksPanel({ invoker: button });
  });
  parent.appendChild(button);
}

/// 构造一个工具块。done=false 时是运行中占位,后续由 fillToolBlock 收尾。
export function buildToolBlock(name, input) {
  const wrap = document.createElement("div");
  wrap.className = "msg tool-msg running";
  const head = document.createElement("button");
  head.type = "button";
  head.className = "tool-msg-head";
  head.setAttribute("aria-expanded", "false");
  const icon = document.createElement("span");
  icon.className = "tool-msg-status";
  icon.textContent = "⏺";
  const label = document.createElement("span");
  label.className = "tool-msg-name";
  label.textContent = name;
  const arg = document.createElement("span");
  arg.className = "tool-msg-arg";
  // 参数列:路径/命令/正则这类代码记号标 is-code 用等宽,自然语言(问题、标题)用比例字体;
  // 悬浮看清洗后的完整值。
  const argSummary = toolArgSummary(name, input);
  const summary = argSummary.text;
  arg.textContent = summary ? `(${summary})` : "";
  if (argSummary.code) arg.classList.add("is-code");
  if (argSummary.title) arg.title = argSummary.title;
  // 类型图标与成败字形**并存**:.tool-msg-status 承载的是「形状 + 颜色双重区分」的
  // 无障碍承诺(D-105),不能被类型图标顶掉。成败在前,类型在后,再是工具名。
  const result = document.createElement("span");
  result.className = "tool-msg-result hidden";
  head.append(icon, toolIconNode(name), label, arg, result);
  // 可访问名带上参数;“展开或收起”只进 aria-label,绝不进可见文本。
  head.setAttribute("aria-label", `${name} ${summary} — ${t("展开或收起工具详情")}`);
  const detail = document.createElement("div");
  detail.className = "tool-msg-detail hidden";
  head.addEventListener("click", () => {
    if (!detail.children.length) return;
    // 结构化结果(JSON 树等)延迟到首次展开才构建。
    flushLazy(detail);
    const open = detail.classList.toggle("hidden");
    head.setAttribute("aria-expanded", String(!open));
  });
  wrap.append(head, detail);
  // name/input 随块走:收尾时按工具摘要(实时 tool-end 不再带入参)。
  const block = { wrap, head, icon, result, detail, name, input };
  // 历史回放补耗时要从 DOM 找回块(applyRecoveredToolDurations)。
  wrap._kzToolBlock = block;
  return block;
}

/// 收尾:状态图标 + 结果摘要行 + 折叠详情(摘要之外的剩余输出 + 完整入参)。
/// ⎿ 行与详情是同一份文本切出来的两段,同一段文字在一个工具块里只出现一次。
export function toolOutcomeView(ok, outcome) {
  const state = outcome || (ok ? "success" : "failed");
  if (state === "noop") return { state, cls: "noop", icon: "↪" };
  if (state === "needs_correction" || state === "needs_confirmation") {
    return { state, cls: "warn", icon: "⚠" };
  }
  return state === "success"
    ? { state, cls: "ok", icon: "⏺" }
    : { state, cls: "err", icon: "✗" };
}

/// ctx = {ok, outcome, code, content, preview, contentTruncated, contentBytes, display, input, durationMs}。
/// 实时路径给 content(与历史同源的正文)+ preview,历史回放只给 content(带 outcome 机器头);
/// 旧后端只有 preview 时摘要器走降级口径。正文只在这一次调用里用,**不挂到块或 DOM 上**
/// (200 个块 × 256 KiB 会把内存吃到 50 MB)。
export function fillToolBlock(block, { ok, outcome, code, content, preview, contentTruncated, contentBytes, display, input, durationMs } = {}) {
  input ??= block.input ?? undefined;
  // 历史 ToolResult 保存模型内容；从稳定结果码恢复和实时事件相同的等待视图。
  if (!display && String(content).startsWith("[tool_outcome=needs_confirmation code=QUESTION_PENDING]\n")) {
    try {
      const pending = JSON.parse(String(content).slice(String(content).indexOf("\n") + 1));
      if (pending.kind === "pending_question") { display = pending; outcome = "needs_confirmation"; }
    } catch { /* 损坏历史仍以原始输出展示。 */ }
  }
  // ⎿ 行 = 按工具的人话摘要(05-tool-summary.js,实时与历史同一个摘要器);失败行仍是
  // 摘要 + 剩余互斥切分。历史正文的 [tool_outcome=…] 头在摘要器里剥掉并恢复终态。
  const summary = toolResultSummary(block.name, {
    ok, outcome, code, content, preview, contentTruncated, contentBytes, display, input, durationMs,
  });
  // 历史没有 display:从配额截断标记合成与实时相同的 display,提示块与 ⎿ 人话一致。
  if (!display && summary.storage?.kind === "truncated") {
    const { reason, bytes, storage_used_bytes, quota_bytes } = summary.storage;
    display = { kind: "truncated", reason, bytes, storage_used_bytes, quota_bytes };
  }
  const view = toolOutcomeView(ok, outcome || summary.outcome);
  block.wrap.classList.remove("running");
  block.wrap.classList.add(view.cls);
  block.wrap.dataset.toolOutcome = view.state;
  // 形状与颜色双重区分:只靠颜色对色盲不可辨(D-105 无障碍口径)。
  block.icon.textContent = view.icon;
  block.wrap.dataset.toolSummary = summary.key;
  renderToolSummary(block.result, summary);
  block.result.classList.toggle("tool-sum-warn", summary.tone === "warn");
  // 只留补耗时要用的摘要骨架(短字符串),不留正文。
  block.summaryBase = Number(durationMs) >= 1000 ? null : { groups: summary.groups, durAt: summary.durAt, text: summary.text, title: summary.title };
  let rest = summary.rest;
  // 截断时 ⎿ 行原本是 [tool_result_truncated …] 机器标记;换成按原因区分的人话,
  // 已用/配额、原因与处理建议见展开区提示块。
  const quota = quotaTruncation(display);
  // UI-0926 #10:JSON 结果(tracker/work/websearch…)的展开区是结构化视图,不再贴整坨 JSON
  // 原文。值直接从正文解析(后端不另发 json display);超过 64 KiB、被截断或非成功的正文
  // 不解析,照旧给原文。
  const body = typeof content === "string" ? stripToolOutcome(content).body : "";
  const jsonValue = !quota && !contentTruncated && body.length <= 65536 && summary.outcome === "success" ? parseJsonish(body) : null;
  if (jsonValue && typeof jsonValue === "object") rest = "";
  // 局部校验:display 带结构化结果时由 chips 渲染(appendDisplayBlock),正文里同一份
  // 「局部校验明细」文本不再重复。
  if (display?.local_validation && rest) rest = rest.replace(/\n?局部校验明细:[\s\S]*$/, "");
  if (quota) {
    block.result.textContent = `⎿ ⚠ ${quotaNoticeHeadline(quota)}`;
    block.summaryBase = null;
    block.result.classList.add("quota-truncated");
  }
  block.result.classList.remove("hidden");
  // 稳定错误码(EDIT_ANCHOR_NOT_FOUND 等)是定位问题的抓手:需要修正/确认/失败时放在展开区首部。
  if (summary.code && !["success", "noop"].includes(view.state) && display?.kind !== "pending_question") {
    const chip = document.createElement("span");
    chip.className = "sv-chip sv-code";
    chip.textContent = summary.code;
    chip.title = t("错误码");
    block.detail.appendChild(chip);
  }
  appendDisplayBlock(block.detail, display, { compact: true });
  if (jsonValue && typeof jsonValue === "object") lazyMount(block.detail, () => renderToolResult(block.name, jsonValue));
  if (display?.kind === "pending_question" && typeof display.question === "string") {
    block.icon.textContent = "⏸";
    block.summaryBase = null;
    block.result.textContent = `${t("待用户回答")}: ${display.question}`;
    const reply = document.createElement("button");
    reply.type = "button";
    reply.className = "pending-question-reply";
    reply.textContent = t("回复此问题");
    reply.addEventListener("click", () => {
      const options = (Array.isArray(display.options) ? display.options : []).map((option) =>
        typeof option === "string" ? option : [option.label, option.note].filter(Boolean).join(": "));
      const context = [display.question, ...options, `${t("我的补充")}: `].join("\n");
      promptBox.value = [promptBox.value.trim(), context].filter(Boolean).join("\n\n");
      promptBox.dispatchEvent(new Event("input", { bubbles: true }));
      promptBox.focus();
    });
    block.wrap.appendChild(reply);
  }

  if (displayNeedsActivityNotice(display)) appendActivityNotice(block.detail);
  // 详情里的原文:失败行放摘要没覆盖到的剩余(一个字只出现一次);成功多行放完整原文
  // (⎿ 行已是人话,不再是原文的一段);单行短结果照旧不出框(不给"展开了还是那一行"
  // 的假承诺)。截断时 ⎿ 行已换成人话,`rest` 只剩机器标记的孤立尾巴,内容由终端/预览块
  // 与提示块承载;终端块本身就是完整输出,不再贴第二份。
  const terminalShown = display?.kind === "terminal" && Boolean(display.full ?? display.output);
  if (rest.trim() && !quota && !terminalShown) {
    const pre = document.createElement("pre");
    pre.className = "tool-msg-raw";
    pre.textContent = rest.length > 8000 ? `${rest.slice(0, 8000)}\n…(${t("已截断")})` : rest;
    block.detail.appendChild(pre);
  }
  // 完整入参:键值表(路径成 chip、命令成代码块、多行说明折叠),原始 JSON 在 dataset.raw。
  const args = renderToolArgs(block.name, input, { display, className: "tool-msg-raw args" });
  if (args) block.detail.appendChild(args);
  if (block.detail.children.length) block.wrap.classList.add("has-detail");
  // 入参已渲染进展开区;块经 wrap._kzToolBlock 与 DOM 同寿命,收尾后不再留一份原始入参
  // (补耗时只用 summaryBase)。
  block.input = null;
}

export const chatToolBlocks = new Map();
export const CHAT_TOOL_KEEP = 200; // D-090 同款上界:长跑只保留最近块的活引用,DOM 留在历史里。

export function chatToolStart(id, name, summary, input) {
  const existing = id ? chatToolBlocks.get(id) : null;
  // 同一调用仍在执行时保持去重;后端编排角色会跨轮复用 id,上一轮已结束则按
  // 活动面板的 restart 语义创建新块。旧块留在 DOM 历史里,Map 只指向当前块,
  // 因而后续同 id 的 ToolEnd 不会覆写上一轮已结束的块。
  if (!id || (existing && !existing.finished)) return;
  clearEmptyState();
  // 事件不带结构化 input 时(旧后端/编排补发)退化为把 summary 当参数展示;但这份
  // 替身不能当入参存下——否则收尾时会被当成「完整入参」贴进展开区。
  const block = buildToolBlock(name, input ?? { command: summary });
  block.input = input ?? null;
  block.finished = false;
  appendToPane(block.wrap);
  chatToolBlocks.set(id, block);
  if (chatToolBlocks.size > CHAT_TOOL_KEEP) {
    chatToolBlocks.delete(chatToolBlocks.keys().next().value);
  }
  scrollBottom();
}
/// extra = {content, contentTruncated, contentBytes, code, durationMs}(UI-0926 起 kz:tool-end 携带)。
/// content 是与历史同源的结果正文(≤256 KiB),摘要器据此按工具解析,实时与历史显示一致;
/// 旧后端没有 content 时摘要器只拿到 preview(首行 120 字 + " (+N lines)"),走降级口径。
export function chatToolEnd(id, ok, preview, display, outcome, extra = {}) {
  const block = chatToolBlocks.get(id);
  if (!block) return;
  block.finished = true;
  fillToolBlock(block, { ok, outcome, preview, display, input: block.input ?? undefined, ...extra });
}

/// 历史回放:工具块先按正文渲染,轨迹(conversation_trace_get)里的耗时后到。按
/// tool.completed 的 durationMs(≥1s)给对应块补上「· 12.3s」——只用块上留的摘要骨架
/// 重排 ⎿ 行,不重新解析正文(正文不在块上)。
export function applyRecoveredToolDurations(traces, pane = activePane) {
  if (!pane) return 0;
  const blocks = new Map();
  for (const el of pane.querySelectorAll(".tool-msg")) {
    const id = el.dataset?.toolCallId;
    if (id && el._kzToolBlock) blocks.set(id, el._kzToolBlock);
  }
  if (!blocks.size) return 0;
  let applied = 0;
  for (const payload of traces || []) {
    for (const event of payload?.events || []) {
      if (event?.kind !== "tool.completed" || !(Number(event.durationMs) >= 1000)) continue;
      const block = blocks.get(event.id);
      if (!block?.summaryBase || block.result.classList.contains("quota-truncated")) continue;
      const timed = withToolDuration(block.summaryBase, event.durationMs);
      renderToolSummary(block.result, timed);
      block.summaryBase = null;
      applied += 1;
    }
  }
  return applied;
}

export let currentReasoningHead = null;
export function setCurrentReasoningHead(value) { currentReasoningHead = value; }
/// 思考块构造器:实时流与历史恢复(15-views-misc renderRecoveredMessages)共用,
/// 两处观感必须一致;body.dataset.raw 始终持有完整思考文本——收起态的复制上下文靠它。
export function buildReasoningBlock(raw) {
  const wrap = document.createElement("div");
  wrap.className = "msg reasoning";
  wrap.hidden = true;
  const head = document.createElement("button");
  head.type = "button";
  head.className = "reasoning-head";
  head.setAttribute("aria-label", t("展开或收起思考过程"));
  head.setAttribute("aria-expanded", "false");
  head.textContent = `· ${t("思考中…")}`;
  const body = document.createElement("div");
  body.className = "reasoning-body md hidden";
  body.dataset.raw = raw;
  body._head = head;
  head.addEventListener("click", () => {
    // 单行摘要没有可展开的正文,点了别装作有反应。
    if (head.classList.contains("expandable")) {
      body.classList.toggle("hidden");
      head.setAttribute("aria-expanded", String(!body.classList.contains("hidden")));
    }
  });
  wrap.append(head, body);
  return { wrap, head, body };
}
export function appendReasoning(text) {
  if (!currentReasoning) {
    // 思考块:每个思考段独立一块,头部实时显示摘要首行,默认折叠(R-015 修正)。
    clearEmptyState();
    const block = buildReasoningBlock("");
    appendToPane(block.wrap);
    setCurrentReasoning(block.body);
    setCurrentReasoningHead(block.head);
    // #7:正在流的思考块(全局运行中时扫光);下一段文本/工具/新一轮开始时 endReasoningLive 摘掉。
    block.head.classList.add("is-live");
  }
  currentReasoning.dataset.raw += text;
  // D-202:与 assistant 同样合帧,头部摘要跟着渲染一起更新(见 flushStreamRender)。
  currentReasoning._head = currentReasoningHead;
  pendingReasoningRender.set(activePane, currentReasoning);
  scheduleStreamRender();
}
export function renderReasoningBlock(body) {
  body.innerHTML = renderMarkdown(body.dataset.raw);
  const head = body._head;
  if (!head) return;
  // 预览取最新的非空行:思考推进时头部跟着走,不再冻结在第一行。
  const lines = body.dataset.raw
    .split("\n")
    .map((l) => l.replace(/[#*`]/g, "").trim())
    .filter(Boolean);
  const preview = (lines[lines.length - 1] || "").slice(0, 60);
  // codex 常常只给一行摘要标题:没有更多内容就不把它作为主区独立块展示。
  const expandable = lines.length > 1;
  const wrap = body.parentElement;
  if (wrap) wrap.hidden = !expandable;
  head.textContent = `· ${preview || t("思考中…")}${expandable ? `(${t("点击展开")})` : ""}`;
  head.classList.toggle("expandable", expandable);
  if (!expandable) head.setAttribute("aria-expanded", "false");
}

// ---------- #7 动效:工具行实时收尾反馈 / 停止收尾 / 思考块在流 ----------
/// 实时收尾的一次性反馈:成功/待确认弹一下,失败抖一下,noop 不播。只由 kz:tool-end 的实时
/// 处理在 chatToolEnd 之后调用——历史回放直接走 fillToolBlock,不经过这里,重开对话不会满屏乱跳。
/// 停止收尾(chatAbortRunning)之后才到的 ToolEnd(停止补发):chatToolEnd 已写上真结果,
/// 这里只撤掉「中断」标记、不播——停止是用户自己按的。
export function playToolOutcomeMotion(id) {
  const block = chatToolBlocks.get(id);
  const wrap = block?.wrap;
  if (!wrap?.classList || !block.icon) return;
  if (wrap.classList.contains("interrupted")) {
    wrap.classList.remove("interrupted");
    return;
  }
  if (wrap.classList.contains("err")) motionOnce(block.icon, "kz-shake", 520);
  else if (wrap.classList.contains("ok") || wrap.classList.contains("warn")) motionOnce(block.icon, "kz-pop", 360);
}
/// 某个 pane 里仍在「运行中」的工具块。chatToolBlocks 跨会话共用一张表,按块所在 pane 筛;
/// 已被裁掉/清空的块 closest 取不到 pane,自然不算。
function runningToolBlocksIn(pane) {
  const found = [];
  if (!pane) return found;
  for (const block of chatToolBlocks.values()) {
    if (block.finished || !block.wrap) continue;
    if (block.wrap.closest?.(".msg-pane") !== pane) continue;
    found.push(block);
  }
  return found;
}
export function paneHasRunningTool(pane = activePane) {
  return runningToolBlocksIn(pane).length > 0;
}
/// 停止/终态出错时收尾:运行中的块不会再等到 ToolEnd(停止就是不等它),转圈要停在「中断」,
/// 否则它会在对话里转到天荒地老。活动面板那边由 bgAbortRunning 收尾,这里只管主对话。
export function chatAbortRunning(pane = activePane) {
  const blocks = runningToolBlocksIn(pane);
  for (const block of blocks) {
    block.finished = true;
    block.wrap.classList.remove("running");
    block.wrap.classList.add("interrupted");
    block.wrap.dataset.toolOutcome = "interrupted";
    block.icon.textContent = "⏹";
    block.result.textContent = `⎿ ${t("无结果(轮次中断)")}`;
    block.result.classList.remove("hidden");
  }
  return blocks.length;
}
/// 后台线的终态由 01-core 的路由分支处理(不进 handler),按会话找它自己的 pane。
export function chatAbortRunningFor(sessionId) {
  const pane = messagePanes.get(sessionId || "");
  return pane ? chatAbortRunning(pane) : 0;
}
/// 思考段结束(文本/工具/新一轮/停止):摘掉 is-live。不放进 setCurrentReasoningHead——
/// withSessionRender 借它切换渲染上下文,放进去会把后台线正在流的思考块也一起熄掉。
export function endReasoningLive() {
  currentReasoningHead?.classList?.remove("is-live");
}
