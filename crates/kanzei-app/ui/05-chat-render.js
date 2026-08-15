// ---------- 消息渲染 ----------
function clearEmptyState() {
  // R-267:空状态改为每个 pane 各有一份(按类名在 pane 内定位)。原来用 id 定位,
  // 多 pane 下会出现重复 id,而且清的可能是别的会话那一份。
  const empty = activePane?.querySelector(".empty-state");
  if (empty) empty.remove();
}

let followLatest = true;
function nearBottom() {
  return messages.scrollHeight - messages.scrollTop - messages.clientHeight < 48;
}
function updateLatestButton() {
  // 容错而非兜底修复:按钮一旦不在 DOM 里(例如又被挪进 #messages 然后被
  // innerHTML 清掉),这里只是不更新,不能把整条滚动/渲染链路一起拖崩——
  // 2026-08-12 就是这么炸的:恢复历史、新建并行线路全报 null.classList。
  const button = $("jump-latest");
  if (button) button.classList.toggle("hidden", followLatest);
}
messages.addEventListener("scroll", () => {
  followLatest = nearBottom();
  updateLatestButton();
});
function scrollBottom(force = false) {
  if (force || followLatest) messages.scrollTop = messages.scrollHeight;
  updateLatestButton();
}
function copyButton() {
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

function addMessage(cls, text) {
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

function addUserMessage(text, promptAttachments = []) {
  const el = addMessage("user", text);
  if (promptAttachments.length === 0) return el;
  const body = el.querySelector(".message-body");
  const attachments = document.createElement("div");
  attachments.className = "message-attachments";
  for (const attachment of promptAttachments) {
    const item = document.createElement("span");
    item.className = "message-attachment";
    const kind = attachment.media_type?.startsWith("image/") ? t("图片") : "PDF";
    item.textContent = `📎 ${attachment.file_name} · ${kind} · ${t("已发送给 agent")}`;
    attachments.appendChild(item);
  }
  body.appendChild(attachments);
  return el;
}

function addErrorMessage(message, { retryable = false } = {}) {
  const el = addMessage("error", "");
  const body = el.querySelector(".message-body");
  const contextOverflow = /context[_ ]length|context overflow|prompt is too long|input is too long|上下文.{0,4}(过长|超限)/i.test(message);
  const level = document.createElement("strong");
  level.className = "error-level";
  const levelKey = contextOverflow ? "可压缩重试" : retryable ? "可重试错误" : "致命错误";
  level.textContent = t(levelKey);
  // R-140 批1:记录级别 key,语言切换时由渲染点重算(同 copy-btn)。
  level.dataset.i18nKey = levelKey;
  const text = document.createElement("div");
  text.textContent = message;
  body.append(level, text);
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

function isRetryableError(message) {
  return /timed out|timeout|connect|connection|dns|网络|连接|超时|context[_ ]length|context overflow|prompt is too long|input is too long|上下文.{0,4}(过长|超限)/i.test(message);
}

function reportError(message, { retryable = isRetryableError(message) } = {}) {
  addErrorMessage(message, { retryable });
  log(`错误:${message}`, "err");
}

let outputChars = 0;
// ---------- D-202:流式渲染合帧 ----------
// 原先每个 delta 都要:整条 renderMarkdown + 整块 innerHTML + 把整条 raw split 一遍
// + 读 scrollHeight(强制同步重排整个消息列表)。前三项在单条消息内是 O(n²),
// 最后一项随轮次增长——流一开就把主线程占满。现在 delta 只累加文本,渲染压到
// 每帧最多一次;上一次渲染实测超过 8ms 就按实测耗时退避(长消息自动降频),
// 无论消息多长都给交互留得出时间片。
let pendingAssistantRender = null;
let pendingReasoningRender = null;
let streamFlushScheduled = false;
let streamRenderCost = 0;
function scheduleStreamRender() {
  if (streamFlushScheduled) return;
  streamFlushScheduled = true;
  const run = () => {
    streamFlushScheduled = false;
    flushStreamRender();
  };
  if (streamRenderCost > 8) setTimeout(run, Math.min(250, Math.round(streamRenderCost)));
  else requestAnimationFrame(run);
}
/// 把累计到的流式文本一次性渲染出去。目标元素可能已被收尾逻辑摘掉引用(甚至已从
/// DOM 摘除,如 stream-restart),照渲染即可——写进游离节点无害,少写一次分支。
function flushStreamRender() {
  const assistant = pendingAssistantRender;
  const reasoning = pendingReasoningRender;
  pendingAssistantRender = null;
  pendingReasoningRender = null;
  if (!assistant && !reasoning) return;
  const started = Date.now();
  if (assistant) {
    assistant.querySelector(".message-body").innerHTML = renderMarkdown(assistant.dataset.raw);
    // 侧边栏"最近在说":assistant 输出的最新一行。只看尾部窗口——扫整条 raw
    // 是纯浪费,而这里只需要最后那一行。
    const line = lastNonEmptyLine(assistant.dataset.raw);
    if (line) liveSet("live-note", `💬 ${line.slice(0, 60)}`);
  }
  if (reasoning) renderReasoningBlock(reasoning);
  streamRenderCost = Date.now() - started;
  scrollBottom();
}
/// 取最后一个非空行。只在尾部窗口里找,并丢掉被窗口截断的首行,避免预览从半个词开始。
function lastNonEmptyLine(raw, window = 2000) {
  let tail = raw.length > window ? raw.slice(-window) : raw;
  if (raw.length > window) {
    const cut = tail.indexOf("\n");
    if (cut >= 0) tail = tail.slice(cut + 1);
  }
  const lines = tail
    .split("\n")
    .map((l) => l.replace(/[#*`]/g, "").trim())
    .filter(Boolean);
  return lines[lines.length - 1] || "";
}
function appendAssistant(text) {
  if (!currentAssistant) {
    currentAssistant = addMessage("assistant md", "");
    currentAssistant.dataset.raw = "";
  }
  currentAssistant.dataset.raw += text;
  outputChars += text.length;
  pendingAssistantRender = currentAssistant;
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
const TOOL_ICON_PATHS = {
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
const TOOL_GROUPS = {
  read: ["read", "file"], files: ["read", "folder"], symbols: ["read", "braces"],
  write: ["write", "pencil"], edit: ["write", "pencil"], insert: ["write", "pencil"],
  multiedit: ["write", "pencil"],
  bash: ["exec", "terminal"], process: ["exec", "terminal"],
  grep: ["search", "search"], glob: ["search", "wildcard"],
  git: ["vcs", "branch"],
  webfetch: ["net", "link"], websearch: ["net", "globe"],
  req: ["tracker", "clipboard"], defect: ["tracker", "bug"], idea: ["tracker", "target"],
  decision: ["tracker", "fork"], source: ["tracker", "book"], finding: ["tracker", "bulb"],
  todowrite: ["plan", "checklist"], work: ["plan", "inbox"],
  task: ["agent", "fanout"],
  architecture: ["asset", "boxlines"], conventions: ["asset", "ruler"], test_record: ["asset", "flask"],
  question: ["ask", "question"],
  collaboration_status: ["collab", "people"],
};
// 前缀兜底:memory_* 与 ui_*/frontend_* 各自同组同字形——「图标标组」这条口径下
// 这是正确行为,顺带让后端新增同族工具时前端零改动、不落 wrench 兜底。
const TOOL_GROUP_PREFIXES = [
  ["memory_", ["memory", "layers"]],
  ["ui_", ["ui", "window"]],
  ["frontend_", ["ui", "window"]],
];
function toolGroupEntry(name) {
  const key = String(name ?? "").trim().toLowerCase();
  if (TOOL_GROUPS[key]) return TOOL_GROUPS[key];
  for (const [prefix, entry] of TOOL_GROUP_PREFIXES) if (key.startsWith(prefix)) return entry;
  return ["other", "wrench"];
}
function toolIconId(name) { return toolGroupEntry(name)[1]; }
/// 图标节点。innerHTML 拼的是常量路径字面量,没有任何外部输入进得来。
/// 组与字形一并写进 data-*:只断言画了图标看不出「画对了但归错组」。
function toolIconNode(name) {
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
function toolCallSummary(name, input) {
  const source = input && typeof input === "object" ? input : {};
  const pick = (...keys) => {
    for (const key of keys) {
      const value = source[key];
      if (typeof value === "string" && value.trim()) return value.trim();
      if (typeof value === "number") return String(value);
    }
    return "";
  };
  let arg;
  switch (name) {
    case "read": case "write": case "edit": arg = pick("path", "file_path", "file"); break;
    case "bash": case "process": arg = pick("command", "action"); break;
    case "grep": arg = pick("pattern"); break;
    case "glob": arg = pick("pattern", "path"); break;
    case "task": arg = pick("prompt"); break;
    case "memory_search": arg = pick("query"); break;
    case "memory_note": arg = pick("summary"); break;
    case "webfetch": arg = pick("url"); break;
    case "question": arg = pick("question"); break;
    case "req": case "defect": case "idea": case "decision": case "memory": case "source": case "finding":
      arg = [pick("action"), pick("id", "title")].filter(Boolean).join(" ");
      break;
    default:
      arg = pick("path", "command", "query", "pattern", "url", "id", "title", "action", "summary");
  }
  arg = String(arg).replace(/\s+/g, " ").trim();
  return arg.length > 76 ? `${arg.slice(0, 75)}…` : arg;
}

/// ⎿ 行的字数预算。摘要在这里切断,剩余原文从同一个位置接着给——
/// 一个字要么在摘要里、要么在详情里,不会两边都有。
const TOOL_PREVIEW_MAX = 110;

/// 把工具结果切成互不重叠的两段:
/// - `text`:⎿ 行的摘要,取第一行有信息量的内容(bash 的 "exit code: 0" 独占首行时顺延到下一行),
///   超过预算就截断;
/// - `rest`:摘要没覆盖到的剩余原文(被顺延跳过的行、被截断的首行尾巴、以及后续所有行)。
/// 从"取摘要"改成"切两段",是因为原先摘要与详情各自独立地从同一份 content 取一遍,
/// 详情那边靠 `full !== preview` 去重——只挡得住单行短结果,首行超长或多行一律双写。
function toolResultSplit(content, isError) {
  const lines = String(content ?? "").split("\n");
  const informative = (line) => line.trim() && !/^exit code:\s*0$/i.test(line.trim());
  let idx = lines.findIndex(informative);
  // 全篇只有 "exit code: 0" 时仍然显示它,别把唯一的结果吞成"完成"。
  if (idx < 0) idx = lines.findIndex((line) => line.trim());
  if (idx < 0) return { text: isError ? t("失败") : t("完成"), rest: "" };
  const head = lines[idx].trim();
  const cut = head.length > TOOL_PREVIEW_MAX;
  const text = cut ? `${head.slice(0, TOOL_PREVIEW_MAX - 1)}…` : head;
  // 被顺延跳过的行没在 ⎿ 露过面,归入剩余部分而不是丢掉;首行被截时把没显示完的尾巴接上,
  // 前置的 … 与 ⎿ 行结尾的 … 呼应,读起来是明确的续接关系。
  const skipped = lines.slice(0, idx).filter((line) => line.trim());
  const tail = cut ? [`…${head.slice(TOOL_PREVIEW_MAX - 1)}`] : [];
  return { text, rest: [...skipped, ...tail, ...lines.slice(idx + 1)].join("\n") };
}

/// 构造一个工具块。done=false 时是运行中占位,后续由 fillToolBlock 收尾。
function buildToolBlock(name, input) {
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
  const summary = toolCallSummary(name, input);
  arg.textContent = summary ? `(${summary})` : "";
  // 类型图标与成败字形**并存**:.tool-msg-status 承载的是「形状 + 颜色双重区分」的
  // 无障碍承诺(D-105),不能被类型图标顶掉。成败在前,类型在后,再是工具名。
  head.append(icon, toolIconNode(name), label, arg);
  // 可访问名带上参数;"展开或收起"只进 aria-label,绝不进可见文本。
  head.setAttribute("aria-label", `${name} ${summary} — ${t("展开或收起工具详情")}`);
  const result = document.createElement("div");
  result.className = "tool-msg-result hidden";
  const detail = document.createElement("div");
  detail.className = "tool-msg-detail hidden";
  head.addEventListener("click", () => {
    if (!detail.children.length) return;
    const open = detail.classList.toggle("hidden");
    head.setAttribute("aria-expanded", String(!open));
  });
  wrap.append(head, result, detail);
  return { wrap, head, icon, result, detail };
}

/// 收尾:状态图标 + 结果摘要行 + 折叠详情(摘要之外的剩余输出 + 完整入参)。
/// ⎿ 行与详情是同一份文本切出来的两段,同一段文字在一个工具块里只出现一次。
function toolOutcomeView(ok, outcome) {
  const state = outcome || (ok ? "success" : "failed");
  if (state === "noop") return { state, cls: "noop", icon: "↪" };
  if (state === "needs_correction" || state === "needs_confirmation") {
    return { state, cls: "warn", icon: "⚠" };
  }
  return state === "success"
    ? { state, cls: "ok", icon: "⏺" }
    : { state, cls: "err", icon: "✗" };
}

function fillToolBlock(block, { ok, outcome, content, display, input }) {
  const view = toolOutcomeView(ok, outcome);
  block.wrap.classList.remove("running");
  block.wrap.classList.add(view.cls);
  block.wrap.dataset.toolOutcome = view.state;
  // 形状与颜色双重区分:只靠颜色对色盲不可辨(D-105 无障碍口径)。
  block.icon.textContent = view.icon;
  const { text: summary, rest } = toolResultSplit(content, !ok);
  block.result.textContent = `⎿ ${summary}`;
  block.result.classList.remove("hidden");
  appendDisplayBlock(block.detail, display);
  // 详情只放摘要没覆盖到的部分:`rest` 非空本身就是"还有没显示完的内容"这个判据,
  // 单行短结果照旧不出框(不给"展开了还是那一行"的假承诺),多行/长首行也不再重复正文。
  if (rest.trim()) {
    const pre = document.createElement("pre");
    pre.className = "tool-msg-raw";
    pre.textContent = rest.length > 8000 ? `${rest.slice(0, 8000)}\n…(${t("已截断")})` : rest;
    block.detail.appendChild(pre);
  }
  if (input && Object.keys(input).length) {
    const pre = document.createElement("pre");
    pre.className = "tool-msg-raw args";
    pre.textContent = JSON.stringify(input, null, 2);
    block.detail.appendChild(pre);
  }
  if (block.detail.children.length) block.wrap.classList.add("has-detail");
}

const chatToolBlocks = new Map();
const CHAT_TOOL_KEEP = 200; // D-090 同款上界:长跑只保留最近块的活引用,DOM 留在历史里。

// R-184 P2:主对话里的 task 工具块按角色折叠成组(R-174 遗留 (a):编排派发的 8 条
// 子代理各自生成一个平铺工具块、偏吵)。组头是唯一新增的 DOM,组内块走既有
// buildToolBlock/fillToolBlock 渲染;同一角色跨轮复用并入同一组,组头显示累计块数。
const chatAgentFolds = new Map(); // role -> {head, body, countEl, count}
function chatAgentFold(role) {
  let group = chatAgentFolds.get(role);
  if (group) return group;
  const wrap = document.createElement("div");
  wrap.className = "agent-fold";
  wrap.dataset.agentRole = role;
  const head = document.createElement("button");
  head.type = "button";
  head.className = "agent-fold-head";
  head.setAttribute("aria-expanded", "false");
  head.setAttribute("aria-label", `${role} — ${t("展开或收起该子代理的工具块")}`);
  const dot = document.createElement("span");
  dot.className = `bg-dot line-accent-${agentRoleAccent(role)}`;
  dot.setAttribute("aria-hidden", "true");
  const label = document.createElement("span");
  label.className = "agent-fold-role";
  label.textContent = role;
  const countEl = document.createElement("span");
  countEl.className = "agent-fold-count dim";
  const caret = document.createElement("span");
  caret.className = "agent-fold-caret";
  caret.textContent = "▸";
  head.append(dot, label, countEl, caret);
  const body = document.createElement("div");
  body.className = "agent-fold-body hidden";
  head.addEventListener("click", () => {
    // classList.toggle 返回的是移除后的状态:展开(类已移除)时返回 false。
    const open = body.classList.toggle("hidden");
    head.setAttribute("aria-expanded", String(!open));
    caret.textContent = open ? "▸" : "▾";
  });
  wrap.append(head, body);
  appendToPane(wrap);
  group = { head, body, countEl, count: 0 };
  chatAgentFolds.set(role, group);
  return group;
}

function chatToolStart(id, name, summary, input) {
  if (!id || chatToolBlocks.has(id)) return;
  clearEmptyState();
  // 实时路径拿不到结构化 input(事件里只有 summary 文本),退化为把 summary 当参数展示。
  const block = buildToolBlock(name, input ?? { command: summary });
  if (name === "task") {
    // task 工具的 id 就是角色名(编排派发)或调用 id(模型自派);折叠组按它归并,
    // 平铺退化为"每个调用一组",不影响非 task 工具的现有渲染路径。
    chatAgentFold(String(id)).body.appendChild(block.wrap);
    const group = chatAgentFolds.get(String(id));
    group.count += 1;
    group.countEl.textContent = `(${group.count})`;
  } else {
    appendToPane(block.wrap);
  }
  chatToolBlocks.set(id, block);
  if (chatToolBlocks.size > CHAT_TOOL_KEEP) {
    chatToolBlocks.delete(chatToolBlocks.keys().next().value);
  }
  scrollBottom();
}
function chatToolEnd(id, ok, preview, display, outcome) {
  const block = chatToolBlocks.get(id);
  if (!block) return;
  // 注意语义:实时事件里的 preview 是后端 runner::preview 的单行摘要(首行 120 字 +
  // " (+N lines)"),不是完整输出。展开区因此只会拿到这一行的尾巴——这是事实,别为了
  // "聊天里也想看全输出"再往 detail 里塞一份 preview,那正是双写的来路。完整输出看
  // 活动面板的 terminal display(走 display.full)或历史回放。
  fillToolBlock(block, { ok, outcome, content: preview, display });
}

let currentReasoningHead = null;
/// 思考块构造器:实时流与历史恢复(15-views-misc renderRecoveredMessages)共用,
/// 两处观感必须一致;body.dataset.raw 始终持有完整思考文本——收起态的复制上下文靠它。
function buildReasoningBlock(raw) {
  const wrap = document.createElement("div");
  wrap.className = "msg reasoning";
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
function appendReasoning(text) {
  if (!currentReasoning) {
    // 思考块:每个思考段独立一块,头部实时显示摘要首行,默认折叠(R-015 修正)。
    clearEmptyState();
    const block = buildReasoningBlock("");
    appendToPane(block.wrap);
    currentReasoning = block.body;
    currentReasoningHead = block.head;
  }
  currentReasoning.dataset.raw += text;
  // D-202:与 assistant 同样合帧,头部摘要跟着渲染一起更新(见 flushStreamRender)。
  currentReasoning._head = currentReasoningHead;
  pendingReasoningRender = currentReasoning;
  scheduleStreamRender();
}
function renderReasoningBlock(body) {
  body.innerHTML = renderMarkdown(body.dataset.raw);
  const head = body._head;
  if (!head) return;
  // 预览取最新的非空行:思考推进时头部跟着走,不再冻结在第一行。
  const lines = body.dataset.raw
    .split("\n")
    .map((l) => l.replace(/[#*`]/g, "").trim())
    .filter(Boolean);
  const preview = (lines[lines.length - 1] || "").slice(0, 60);
  // codex 常常只给一行摘要标题:没有更多内容就不给"展开"的假承诺。
  const expandable = lines.length > 1;
  head.textContent = `· ${preview || t("思考中…")}${expandable ? `(${t("点击展开")})` : ""}`;
  head.classList.toggle("expandable", expandable);
  if (!expandable) head.setAttribute("aria-expanded", "false");
}

