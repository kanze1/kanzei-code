// ---------- 消息渲染 ----------
function clearEmptyState() {
  const empty = $("empty-state");
  if (empty) empty.remove();
}

let followLatest = true;
function nearBottom() {
  return messages.scrollHeight - messages.scrollTop - messages.clientHeight < 48;
}
function updateLatestButton() {
  $("jump-latest").classList.toggle("hidden", followLatest);
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
  // R-140 批1:消息容器豁免 observer 后,容器内 t() 渲染点靠 data-i18n-key
  // 在语言切换时由 syncMessagesLanguage() 重算(渲染点翻译,不再靠事后回译)。
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
  messages.appendChild(el);
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
    case "req": case "defect": case "goal": case "decision": case "memory": case "source": case "finding":
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
  head.append(icon, label, arg);
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
function fillToolBlock(block, { ok, content, display, input }) {
  block.wrap.classList.remove("running");
  block.wrap.classList.add(ok ? "ok" : "err");
  // 形状与颜色双重区分:只靠颜色对色盲不可辨(D-105 无障碍口径)。
  block.icon.textContent = ok ? "⏺" : "✗";
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
function chatToolStart(id, name, summary, input) {
  if (!id || chatToolBlocks.has(id)) return;
  clearEmptyState();
  // 实时路径拿不到结构化 input(事件里只有 summary 文本),退化为把 summary 当参数展示。
  const block = buildToolBlock(name, input ?? { command: summary });
  messages.appendChild(block.wrap);
  chatToolBlocks.set(id, block);
  if (chatToolBlocks.size > CHAT_TOOL_KEEP) {
    chatToolBlocks.delete(chatToolBlocks.keys().next().value);
  }
  scrollBottom();
}
function chatToolEnd(id, ok, preview, display) {
  const block = chatToolBlocks.get(id);
  if (!block) return;
  // 注意语义:实时事件里的 preview 是后端 runner::preview 的单行摘要(首行 120 字 +
  // " (+N lines)"),不是完整输出。展开区因此只会拿到这一行的尾巴——这是事实,别为了
  // "聊天里也想看全输出"再往 detail 里塞一份 preview,那正是双写的来路。完整输出看
  // 活动面板的 terminal display(走 display.full)或历史回放。
  fillToolBlock(block, { ok, content: preview, display });
}

let currentReasoningHead = null;
function appendReasoning(text) {
  if (!currentReasoning) {
    // 思考块:每个思考段独立一块,头部实时显示摘要首行,默认折叠(R-015 修正)。
    clearEmptyState();
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
    body.dataset.raw = "";
    head.addEventListener("click", () => {
      // 单行摘要没有可展开的正文,点了别装作有反应。
      if (head.classList.contains("expandable")) {
        body.classList.toggle("hidden");
        head.setAttribute("aria-expanded", String(!body.classList.contains("hidden")));
      }
    });
    wrap.append(head, body);
    messages.appendChild(wrap);
    currentReasoning = body;
    currentReasoningHead = head;
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

