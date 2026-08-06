// kanzei 桌面端前端逻辑(静态,无构建步骤)。
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// 事件订阅统一入口:注册失败必须可见(D-005 教训——ACL 拒绝时曾静默失联)。
function on(event, handler) {
  listen(event, handler).catch((err) => {
    log(`事件订阅失败 ${event}: ${err} — 界面将收不到运行事件,请反馈`, "err");
    $("log-panel").classList.remove("hidden");
  });
}

const $ = (id) => document.getElementById(id);
const messages = $("messages");
const promptBox = $("prompt");

let running = false;
let currentProject = null;
let currentAssistant = null;
let currentReasoning = null;
let currentTool = null;
let attachments = [];
let runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };

// ---------- 视图切换 ----------
document.querySelectorAll(".activity-item").forEach((item) => {
  item.addEventListener("click", () => {
    document.querySelectorAll(".activity-item").forEach((i) => i.classList.remove("active"));
    item.classList.add("active");
    const view = item.dataset.view;
    document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
    $(`view-${view}`).classList.add("active");
    if (view === "settings") loadSettings();
  });
});

// ---------- toast ----------
let toastTimer = null;
function toast(text) {
  const el = $("toast");
  el.textContent = text;
  el.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.add("hidden"), 2600);
}

// ---------- 运行日志面板 ----------
const LOG_MAX = 300;
function log(text, cls = "") {
  const lines = $("log-lines");
  const line = document.createElement("div");
  line.className = `log-line ${cls}`;
  const time = new Date().toTimeString().slice(0, 8);
  line.textContent = `${time}  ${text}`;
  lines.appendChild(line);
  while (lines.childElementCount > LOG_MAX) lines.firstElementChild.remove();
  lines.scrollTop = lines.scrollHeight;
}
$("log-toggle").addEventListener("click", () => $("log-panel").classList.toggle("hidden"));
$("log-clear").addEventListener("click", () => ($("log-lines").innerHTML = ""));

// ---------- 状态栏 ----------
function setStatus(text, isRunning) {
  $("status-text").textContent = text;
  $("status-dot").className = `dot ${isRunning ? "run" : "idle"}`;
  $("statusbar").classList.toggle("running", !!isRunning);
}

// 运行计时 + 首响应看门狗:等太久时把"卡在哪"讲清楚。
let runStart = 0;
let firstSignal = false;
let elapsedTimer = null;
function startElapsed() {
  runStart = Date.now();
  firstSignal = false;
  clearInterval(elapsedTimer);
  elapsedTimer = setInterval(() => {
    const secs = Math.floor((Date.now() - runStart) / 1000);
    $("status-elapsed").textContent = `· ${secs}s`;
    if (!firstSignal && secs > 0 && secs % 15 === 0) {
      log(`仍在等待模型首个响应(已 ${secs}s)——订阅高峰或网络较慢时属正常;超时上限 15s 连接 / 180s 读`, "warn");
    }
  }, 1000);
}
function stopElapsed() {
  clearInterval(elapsedTimer);
  elapsedTimer = null;
  $("status-elapsed").textContent = "";
}
function markFirstSignal() {
  if (!firstSignal) {
    firstSignal = true;
    log(`模型开始响应(${((Date.now() - runStart) / 1000).toFixed(1)}s)`);
  }
}

let ctxLimit = null;
let ctxTokens = 0;
function renderTokens() {
  const t = runTokens;
  let text = t.input + t.output === 0
    ? ""
    : `in ${t.input} (cache r${t.cacheRead} w${t.cacheWrite}) · out ${t.output}`;
  if (ctxTokens > 0) {
    const k = (ctxTokens / 1000).toFixed(1);
    if (ctxLimit) {
      const pct = Math.round((ctxTokens / ctxLimit) * 100);
      text += `${text ? " · " : ""}ctx ${k}k/${Math.round(ctxLimit / 1000)}k (${pct}%)`;
      $("status-tokens").classList.toggle("ctx-warn", pct >= 70);
    } else {
      text += `${text ? " · " : ""}ctx ${k}k`;
    }
  }
  $("status-tokens").textContent = text;
}

function setRunning(value, statusText) {
  running = value;
  $("send").disabled = value;
  $("stop").classList.toggle("hidden", !value);
  setStatus(statusText ?? (value ? "运行中" : "空闲"), value);
}

// ---------- markdown-lite(无依赖:代码围栏/行内码/加粗/标题;先转义再渲染,安全) ----------
function escapeHtml(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}
function renderMarkdown(raw) {
  const parts = escapeHtml(raw).split(/```/);
  let html = "";
  parts.forEach((seg, i) => {
    if (i % 2 === 1) {
      const nl = seg.indexOf("\n");
      html += `<pre class="code">${nl >= 0 ? seg.slice(nl + 1) : seg}</pre>`;
    } else {
      html += seg
        .replace(/`([^`\n]+)`/g, "<code>$1</code>")
        .replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>")
        .replace(/^#{1,6}\s+(.+)$/gm, '<strong class="md-h">$1</strong>');
    }
  });
  return html;
}

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
  button.textContent = "复制";
  button.title = "复制消息";
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

let outputChars = 0;
function appendAssistant(text) {
  if (!currentAssistant) {
    currentAssistant = addMessage("assistant md", "");
    currentAssistant.dataset.raw = "";
  }
  currentAssistant.dataset.raw += text;
  currentAssistant.querySelector(".message-body").innerHTML = renderMarkdown(currentAssistant.dataset.raw);
  outputChars += text.length;
  // 侧边栏"最近在说":assistant 输出的最新一行。
  const lines = currentAssistant.dataset.raw
    .split("\n")
    .map((l) => l.replace(/[#*`]/g, "").trim())
    .filter(Boolean);
  if (lines.length) liveSet("live-note", `💬 ${lines[lines.length - 1].slice(0, 60)}`);
  scrollBottom();
}

let currentReasoningHead = null;
function appendReasoning(text) {
  if (!currentReasoning) {
    // 思考块:每个思考段独立一块,头部实时显示摘要首行,默认折叠(R-015 修正)。
    clearEmptyState();
    const wrap = document.createElement("div");
    wrap.className = "msg reasoning";
    const head = document.createElement("div");
    head.className = "reasoning-head";
    head.textContent = "· 思考中…";
    const body = document.createElement("div");
    body.className = "reasoning-body md hidden";
    body.dataset.raw = "";
    head.addEventListener("click", () => {
      // 单行摘要没有可展开的正文,点了别装作有反应。
      if (head.classList.contains("expandable")) body.classList.toggle("hidden");
    });
    wrap.append(head, body);
    messages.appendChild(wrap);
    currentReasoning = body;
    currentReasoningHead = head;
  }
  currentReasoning.dataset.raw += text;
  currentReasoning.innerHTML = renderMarkdown(currentReasoning.dataset.raw);
  if (currentReasoningHead) {
    // 预览取最新的非空行:思考推进时头部跟着走,不再冻结在第一行。
    const lines = currentReasoning.dataset.raw
      .split("\n")
      .map((l) => l.replace(/[#*`]/g, "").trim())
      .filter(Boolean);
    const preview = (lines[lines.length - 1] || "").slice(0, 60);
    // codex 常常只给一行摘要标题:没有更多内容就不给"展开"的假承诺。
    const expandable = lines.length > 1;
    currentReasoningHead.textContent = `· ${preview || "思考中…"}${expandable ? "(点击展开)" : ""}`;
    currentReasoningHead.classList.toggle("expandable", expandable);
  }
  scrollBottom();
}

// ---------- 后台任务面板:子代理/长时间运行的工具实时监控 ----------
const bgEntries = new Map(); // call_id -> {el, prog, meta, startedAt, done}
function bgSync() {
  $("bg-panel").classList.toggle("hidden", $("bg-list").children.length === 0);
}
function bgAdd(id, name, summary) {
  if (!id || bgEntries.has(id)) return;
  const make = () => {
    if (bgEntries.has(id)) return;
    const el = document.createElement("div");
    el.className = "bg-entry running";
    const title = document.createElement("div");
    title.className = "bg-title";
    title.textContent = `${name} ${summary}`;
    title.title = summary;
    const prog = document.createElement("div");
    prog.className = "bg-prog";
    prog.textContent = "…";
    const meta = document.createElement("div");
    meta.className = "bg-meta";
    el.append(title, prog, meta);
    $("bg-list").appendChild(el);
    bgEntries.set(id, { el, prog, meta, startedAt: Date.now(), done: false });
    bgSync();
  };
  // task 立即入面板;其他工具超过 2.5s 还没结束才算"长任务"。
  if (name === "task") make();
  else setTimeout(() => { if (toolChips.has(id)) make(); }, 2500);
}
function bgProgress(id, text) {
  const entry = bgEntries.get(id);
  if (entry && !entry.done) entry.prog.textContent = text;
}
function bgEnd(id, ok, preview) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  entry.done = true;
  entry.el.classList.remove("running");
  entry.el.classList.add(ok ? "ok" : "err");
  entry.prog.textContent = preview || (ok ? "完成" : "失败");
  // 留 6 秒看得见结果,然后让位。
  setTimeout(() => { entry.el.remove(); bgEntries.delete(id); bgSync(); }, 6000);
}
function bgClear() {
  for (const entry of bgEntries.values()) entry.el.remove();
  bgEntries.clear();
  bgSync();
}
setInterval(() => {
  for (const entry of bgEntries.values()) {
    if (!entry.done) entry.meta.textContent = `${Math.round((Date.now() - entry.startedAt) / 1000)}s`;
  }
}, 1000);

// ---------- 当前进展:侧边栏实时状态卡(把握 agent 进度,不用等它汇报) ----------
function liveSet(id, text) {
  const el = $(id);
  if (!text) {
    el.classList.add("hidden");
    return;
  }
  el.classList.remove("hidden");
  el.textContent = text;
  el.title = text;
}
function liveIdle(label) {
  const turn = $("live-turn");
  turn.textContent = label;
  turn.classList.add("dim");
  liveSet("live-action", "");
}
function liveTurn(text) {
  const turn = $("live-turn");
  turn.textContent = text;
  turn.classList.remove("dim");
}

// ---------- 事件订阅 ----------
on("kz:status", (e) => {
  const p = e.payload;
  log(`[${p.stage}] ${p.detail}`);
  if (running) setStatus(`${p.stage} · ${p.detail}`, true);
});
on("kz:meta", (e) => {
  $("status-model").textContent = `${e.payload.model} · ${e.payload.profile}`;
  ctxLimit = e.payload.contextLimit ?? null;
  log(`模型 ${e.payload.model} · agent ${e.payload.agent} · profile ${e.payload.profile}${ctxLimit ? ` · 上下文上限 ${Math.round(ctxLimit / 1000)}k` : ""}`);
  if (running) setStatus("等待模型响应", true);
});
on("kz:turn", (e) => {
  const p = e.payload;
  if (p.step > 1) {
    clearEmptyState();
    const divider = document.createElement("div");
    divider.className = "turn-divider";
    divider.textContent = p.maxSteps > 0 ? `第 ${p.step}/${p.maxSteps} 轮` : `第 ${p.step} 轮`;
    messages.appendChild(divider);
    scrollBottom();
  }
  currentAssistant = null;
  currentReasoning = null;
  currentReasoningHead = null;
  liveTurn(p.maxSteps > 0 ? `第 ${p.step}/${p.maxSteps} 轮` : `第 ${p.step} 轮`);
  if (running) setStatus(`第 ${p.step} 轮 · 等待模型`, true);
});
on("kz:text", (e) => {
  markFirstSignal();
  // 文本开始后,后续思考属于新的思考段。
  currentReasoning = null;
  currentReasoningHead = null;
  if (running) setStatus(`生成中 · ${(outputChars / 1000).toFixed(1)}k 字`, true);
  appendAssistant(e.payload.text);
});
on("kz:reasoning", (e) => {
  markFirstSignal();
  if (running) setStatus("思考中", true);
  appendReasoning(e.payload.text);
});
// 工具块按调用 id 配对:并行 task 结束顺序不定,靠全局 currentTool 会张冠李戴(D-017 根因)。
const toolChips = new Map();
on("kz:tool-start", (e) => {
  markFirstSignal();
  log(`工具 ${e.payload.name} ${e.payload.summary}`);
  currentAssistant = null;
  currentReasoning = null;
  clearEmptyState();
  const chip = document.createElement("div");
  chip.className = "tool-chip running";
  const head = document.createElement("div");
  head.className = "head";
  head.textContent = `${e.payload.name} ${e.payload.summary}`;
  chip.appendChild(head);
  // task 子代理:块内实时进度行,kz:task-progress 持续刷新。
  if (e.payload.name === "task") {
    const prog = document.createElement("div");
    prog.className = "task-progress";
    prog.textContent = "… 子代理启动中";
    chip.appendChild(prog);
  }
  messages.appendChild(chip);
  currentTool = chip;
  if (e.payload.id) toolChips.set(e.payload.id, chip);
  bgAdd(e.payload.id, e.payload.name, e.payload.summary);
  liveSet("live-action", `⚙ ${e.payload.name} ${e.payload.summary.slice(0, 60)}`);
  setStatus(`工具执行中 · ${e.payload.name}`, true);
  scrollBottom();
});
on("kz:task-progress", (e) => {
  const p = e.payload;
  const chip = toolChips.get(p.id);
  if (chip) {
    const prog = chip.querySelector(".task-progress");
    if (prog) prog.textContent = `… ${p.text}`;
  }
  bgProgress(p.id, p.text);
});
on("kz:tool-end", (e) => {
  const p = e.payload;
  log(`工具结果 ${p.name}: ${p.ok ? "成功" : "失败"} — ${p.preview}`, p.ok ? "" : "warn");
  // 工作焦点:req/defect/goal 的增改结果最能代表"它在干哪件事"。
  if (p.ok && ["req", "defect", "goal"].includes(p.name)) {
    liveSet("live-focus", `◉ ${p.preview.replace(/^(updated|added):?\s*/, "").slice(0, 60)}`);
  }
  const chip = (p.id && toolChips.get(p.id)) || currentTool;
  if (p.id) toolChips.delete(p.id);
  if (chip === currentTool) currentTool = null;
  bgEnd(p.id, p.ok, p.preview);
  if (chip) {
    chip.querySelector(".task-progress")?.remove();
    chip.classList.remove("running");
    chip.classList.add(p.ok ? "ok" : "err");
    const collapsibles = [];

    const result = document.createElement("div");
    result.className = "result hidden";
    result.textContent = p.preview;
    chip.appendChild(result);
    collapsibles.push(result);

    // 结构化展示:diff 默认收纳,头部保留文件路径和增删统计;终端块也默认折叠。
    const d = p.display;
    if (d && d.kind === "diff") {
      const stat = document.createElement("span");
      stat.className = "diff-stat";
      stat.textContent = ` +${d.additions} −${d.deletions} ${d.path}`;
      chip.querySelector(".head").appendChild(stat);
      const block = document.createElement("div");
      block.className = "tool-display diff hidden";
      for (const line of (d.diff || "").split("\n")) {
        const ln = document.createElement("div");
        ln.className =
          line.startsWith("+") ? "dl add" : line.startsWith("-") ? "dl del" : "dl ctx";
        ln.textContent = line || " ";
        block.appendChild(ln);
      }
      chip.appendChild(block);
      collapsibles.push(block);
    } else if (d && d.kind === "terminal") {
      const block = document.createElement("div");
      block.className = "tool-display term hidden";
      block.textContent = `$ ${d.command}\n${d.output}`;
      chip.appendChild(block);
      collapsibles.push(block);
    } else if (d && d.kind === "create") {
      const block = document.createElement("div");
      block.className = "tool-display term";
      block.textContent = `新建 ${d.path}(${d.bytes} bytes)\n${d.preview}`;
      chip.appendChild(block);
      collapsibles.push(block);
    }

    chip.querySelector(".head").addEventListener("click", () => {
      for (const el of collapsibles) el.classList.toggle("hidden");
    });
    // 失败结果直接展开可见,且不参与折叠切换(避免与 diff 展开状态错位)。
    if (!p.ok) {
      result.classList.remove("hidden");
      collapsibles.splice(collapsibles.indexOf(result), 1);
    }
    const actions = document.createElement("span");
    actions.className = "msg-actions";
    actions.appendChild(copyButton());
    chip.appendChild(actions);
  }
  setStatus("运行中", true);
  scrollBottom();
});
on("kz:step", (e) => {
  const p = e.payload;
  runTokens.input += p.input;
  runTokens.output += p.output;
  runTokens.cacheRead += p.cacheRead;
  runTokens.cacheWrite += p.cacheWrite;
  // 本轮 prompt 体积 ≈ 当前上下文占用。
  ctxTokens = p.input + p.cacheRead;
  renderTokens();
  log(`一轮完成:in ${p.input} (cache r${p.cacheRead}) · out ${p.output} · ctx ${(ctxTokens / 1000).toFixed(1)}k`);
});
on("kz:error", (e) => {
  addMessage("error", e.payload.message);
  log(`错误:${e.payload.message}`, "err");
  stopElapsed();
  setRunning(false, "出错");
  toolChips.clear();
  bgClear();
  liveIdle("出错");
  $("log-panel").classList.remove("hidden");
});
on("kz:compacted", () => {
  addMessage("notice", "🗜 上下文占用过高,已自动压缩为纪要并延续对话");
  log("自动压缩完成:多轮历史已替换为纪要");
  ctxTokens = 0;
  renderTokens();
});
on("kz:stopped", (e) => {
  hideAsk();
  const cancelled = e.payload?.cancelled_queue ?? 0;
  addMessage("notice", cancelled > 0 ? `已停止,已取消 ${cancelled} 条排队输入` : "已停止");
  log(cancelled > 0 ? `已手动停止并取消 ${cancelled} 条排队输入` : "已手动停止");
  stopElapsed();
  setRunning(false, "已停止");
  toolChips.clear();
  bgClear();
  liveIdle("已停止");
});
on("kz:done", (e) => {
  const p = e.payload;
  addMessage(
    "notice",
    `完成 · steps ${p.steps}${p.history ? ` · 会话 ${p.history} 条` : ""}${p.halted ? " · 已按你的拒绝停止" : ""}`
  );
  log(`运行完成:${p.steps} 轮,耗时 ${((Date.now() - runStart) / 1000).toFixed(1)}s`);
  stopElapsed();
  setRunning(false);
  // 对齐 Claude:当前对话跑完一轮就出现在历史列表里,不用等重启/切项目。
  refreshConversationList();
  toolChips.clear();
  bgClear();
  liveIdle(`空闲 · 上轮 ${p.steps} 轮完成`);
  refreshDocs();
  refreshGit();

  // 连跑:正常完成且上轮有实质动作(>1 轮 = 有工具调用)才续;拒绝/纯聊天即停。
  if ($("auto-continue").checked && autoContinueAllowed() && !p.halted) {
    if (p.steps <= 1 && autoRounds > 0) {
      addMessage("notice", "连跑停止:上一轮没有实质动作(可能目标已达成或被阻塞)");
      log("连跑停止:steps<=1");
      autoRounds = 0;
      return;
    }
    if (autoRounds >= AUTO_CONTINUE_MAX) {
      addMessage("notice", `连跑停止:已达 ${AUTO_CONTINUE_MAX} 连上限,点「继续」或重开连跑`);
      autoRounds = 0;
      return;
    }
    autoRounds += 1;
    setStatus(`连跑:${autoRounds}/${AUTO_CONTINUE_MAX},2 秒后继续…`, false);
    setTimeout(() => {
      if ($("auto-continue").checked && autoContinueAllowed() && !running) sendText(CONTINUE_PROMPT, { auto: true });
    }, 2000);
  }
});

// ---------- 权限弹窗 ----------
const askQueue = [];
let askActive = null;

on("kz:ask", (e) => {
  // 自动放行(yolo):不弹窗,直接允许并留日志。
  if (e.payload.kind !== "question" && $("auto-allow").checked) {
    log(`自动放行:${e.payload.action} ${e.payload.resource}`);
    invoke("answer_ask", { id: e.payload.id, reply: "once" }).catch((err) =>
      log(`自动放行失败:${err}`, "err")
    );
    return;
  }
  askQueue.push(e.payload);
  pumpAsk();
});

$("auto-allow").checked = localStorage.getItem("kz-auto-allow") === "1";
$("auto-allow").addEventListener("change", () => {
  localStorage.setItem("kz-auto-allow", $("auto-allow").checked ? "1" : "0");
  log($("auto-allow").checked ? "已开启自动放行(本会话所有权限询问直接通过)" : "已关闭自动放行");
});

function pumpAsk() {
  if (askActive || askQueue.length === 0) return;
  askActive = askQueue.shift();
  const question = askActive.kind === "question";
  $("ask-title").textContent = question ? "需要你的回答" : "权限请求";
  $("permission-fields").classList.toggle("hidden", question);
  $("permission-buttons").classList.toggle("hidden", question);
  $("question-fields").classList.toggle("hidden", !question);
  $("question-buttons").classList.toggle("hidden", !question);
  if (question) {
    $("ask-question").textContent = askActive.question;
    const options = $("ask-options");
    options.innerHTML = "";
    for (const option of askActive.options || []) {
      const button = document.createElement("button");
      button.className = "ghost ask-option";
      button.textContent = option;
      button.addEventListener("click", () => answerAsk(option));
      options.appendChild(button);
    }
    $("ask-answer").value = askActive.default || "";
    setTimeout(() => $("ask-answer").focus(), 0);
  } else {
    $("ask-action").textContent = askActive.action;
    $("ask-resource").textContent = askActive.resource;
    $("ask-remember").textContent = `${askActive.action} ${askActive.remember ?? askActive.resource}`;
  }
  $("ask-overlay").classList.remove("hidden");
}

function hideAsk() {
  askQueue.length = 0;
  askActive = null;
  $("ask-overlay").classList.add("hidden");
}

async function answerAsk(reply) {
  if (!askActive) return;
  const id = askActive.id;
  const question = askActive.kind === "question";
  const summary = question ? askActive.question : `${askActive.action}: ${askActive.resource}`;
  askActive = null;
  $("ask-overlay").classList.add("hidden");
  log(`${question ? "回答" : "权限"} ${reply === "deny" ? "拒绝" : reply === "always" ? "总是允许" : reply} — ${summary}`);
  try {
    await invoke("answer_ask", { id, reply });
  } catch (err) {
    log(`权限应答失败:${err}`, "err");
  }
  pumpAsk();
}

$("ask-deny").addEventListener("click", () => answerAsk("deny"));
$("ask-always").addEventListener("click", () => answerAsk("always"));
$("ask-allow").addEventListener("click", () => answerAsk("once"));
$("ask-cancel").addEventListener("click", () => answerAsk("cancel"));
$("ask-submit").addEventListener("click", () => answerAsk($("ask-answer").value.trim()));
$("ask-answer").addEventListener("keydown", (event) => {
  if (event.key === "Enter") answerAsk($("ask-answer").value.trim());
});

// ---------- 阅读辅助 ----------
async function copyReadable(el) {
  const text = el.dataset.raw || [...el.childNodes]
    .filter((node) => !(node.nodeType === Node.ELEMENT_NODE && node.classList.contains("msg-actions")))
    .map((node) => node.textContent || "")
    .join("")
    .trim();
  if (!text) return toast("没有可复制的内容");
  try {
    await navigator.clipboard.writeText(text);
    toast("已复制");
  } catch (err) {
    log(`复制失败:${err}`, "err");
    toast("复制失败，请检查剪贴板权限");
  }
}
messages.addEventListener("click", (event) => {
  const button = event.target.closest(".copy-btn");
  if (button) copyReadable(button.closest(".msg, .tool-chip"));
});

let searchMatches = [];
let searchIndex = 0;
function updateSearch() {
  const query = $("chat-search-input").value.trim().toLowerCase();
  document.querySelectorAll(".search-hit, .search-current").forEach((el) => el.classList.remove("search-hit", "search-current"));
  searchMatches = query ? [...messages.querySelectorAll(".msg, .tool-chip")].filter((el) => el.textContent.toLowerCase().includes(query)) : [];
  searchIndex = Math.min(searchIndex, Math.max(0, searchMatches.length - 1));
  searchMatches.forEach((el) => el.classList.add("search-hit"));
  if (searchMatches.length) {
    const current = searchMatches[searchIndex];
    current.classList.add("search-current");
    current.scrollIntoView({ block: "center" });
  }
  $("chat-search-count").textContent = query ? `${searchMatches.length ? searchIndex + 1 : 0}/${searchMatches.length}` : "";
}
function moveSearch(delta) {
  if (!searchMatches.length) return;
  searchIndex = (searchIndex + delta + searchMatches.length) % searchMatches.length;
  updateSearch();
}
$("chat-search-toggle").addEventListener("click", () => {
  const bar = $("chat-search");
  bar.classList.toggle("hidden");
  if (!bar.classList.contains("hidden")) $("chat-search-input").focus();
});
$("chat-search-input").addEventListener("input", () => { searchIndex = 0; updateSearch(); });
$("chat-search-input").addEventListener("keydown", (event) => {
  if (event.key === "Enter") moveSearch(event.shiftKey ? -1 : 1);
  if (event.key === "Escape") $("chat-search").classList.add("hidden");
});
$("chat-search-prev").addEventListener("click", () => moveSearch(-1));
$("chat-search-next").addEventListener("click", () => moveSearch(1));
$("jump-latest").addEventListener("click", () => {
  followLatest = true;
  scrollBottom(true);
  messages.scrollTo({ top: messages.scrollHeight, behavior: "smooth" });
});

// ---------- 发送 / 停止 ----------
// 连跑状态:自动续跑计数(手动发送归零),上限防失控。
const AUTO_CONTINUE_MAX = 10;
let autoRounds = 0;
const CONTINUE_PROMPT =
  "继续:检查活跃目标(goal list)与最新进展,推进下一个具体步骤并落地(改代码/跑测试/更新文档);" +
  "完成后用 goal update 记录进展。收尾优先:已是 doing 的需求先推到 done(req update <id> done)再开新的,doing 同时不超过 2 个。" +
  "若工作区有已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
  "若所有活跃目标已达成或被阻塞,明确说明原因,不要做无意义的空转。";

function selectedAgent() {
  const mode = $("profile-select").value;
  if (mode === "dev-pair") return { profile: "dev", agent: "dev-pair" };
  if (mode === "dev-auto") return { profile: "dev", agent: "dev" };
  return { profile: "research", agent: "research" };
}

function autoContinueAllowed() {
  return $("profile-select").value === "dev-auto";
}

function renderAttachments() {
  const box = $("attachments");
  box.innerHTML = "";
  box.classList.toggle("hidden", attachments.length === 0);
  attachments.forEach((item, index) => {
    const chip = document.createElement("span");
    chip.className = "attachment-chip";
    chip.textContent = `${item.file_name} ×`;
    chip.title = "移除附件";
    chip.addEventListener("click", () => { attachments.splice(index, 1); renderAttachments(); });
    box.appendChild(chip);
  });
}

function addFiles(files) {
  for (const file of files) {
    if (!(file.type.startsWith("image/") || file.type === "application/pdf" || file.name.toLowerCase().endsWith(".pdf"))) {
      toast(`不支持的附件类型: ${file.name}`);
      continue;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const dataUrl = String(reader.result);
      attachments.push({ file_name: file.name, media_type: file.type || "application/pdf", data: dataUrl.split(",", 2)[1] || "" });
      renderAttachments();
    };
    reader.onerror = () => toast(`读取附件失败: ${file.name}`);
    reader.readAsDataURL(file);
  }
}

$("attach").addEventListener("click", () => $("attachment-input").click());
$("attachment-input").addEventListener("change", (e) => { addFiles(e.target.files); e.target.value = ""; });
promptBox.addEventListener("dragover", (e) => { e.preventDefault(); });
promptBox.addEventListener("drop", (e) => { e.preventDefault(); addFiles(e.dataTransfer.files); });
promptBox.addEventListener("paste", (e) => {
  const files = [...(e.clipboardData?.files || [])];
  if (files.length) { e.preventDefault(); addFiles(files); }
});

async function sendText(prompt, { auto = false, promptAttachments = [] } = {}) {
  // 任何拒绝发送的理由都要说出来,绝不静默(D-004)。
  if (!prompt) return;
  const delivery = $("delivery-select").value;
  if (running && auto) {
    toast("当前任务还在运行，自动连跑将在本轮完成后继续");
    return;
  }
  if (!currentProject) {
    toast("先在左侧「项目」里添加并选择一个目录");
    return;
  }
  if (running) {
    addMessage("user", prompt);
    log(`运行中${delivery === "steer" ? "插入" : "排队"}:${prompt.slice(0, 80)}`);
    try {
      const mode = selectedAgent();
      await invoke("run_prompt", {
        prompt,
        projectDir: currentProject,
        profile: mode.profile,
        agent: mode.agent,
        model: $("model-select").value || null,
        delivery,
        attachments: promptAttachments,
      });
      toast(delivery === "steer" ? "已插入当前会话，将优先执行" : "已加入队列，将按顺序执行");
    } catch (err) {
      addMessage("error", String(err));
      log(`提交被拒:${err}`, "err");
    }
    return;
  }
  if (!auto) autoRounds = 0;
  currentAssistant = null;
  currentReasoning = null;
  runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };
  ctxTokens = 0;
  outputChars = 0;
  renderTokens();
  addMessage("user", auto ? `(连跑 ${autoRounds}/${AUTO_CONTINUE_MAX})${prompt}` : prompt);
  setRunning(true, auto ? `连跑 ${autoRounds}/${AUTO_CONTINUE_MAX} · 准备中` : "准备中");
  startElapsed();
  log(`${auto ? "连跑" : "发送"}:${prompt.slice(0, 80)}`);
  try {
    const mode = selectedAgent();
    await invoke("run_prompt", {
      prompt,
      projectDir: currentProject,
      profile: mode.profile,
      agent: mode.agent,
      model: $("model-select").value || null,
      delivery,
      attachments: promptAttachments,
    });
  } catch (err) {
    addMessage("error", String(err));
    log(`发送被拒:${err}`, "err");
    stopElapsed();
    setRunning(false);
  }
}

function send() {
  const prompt = promptBox.value.trim();
  if (!prompt && attachments.length === 0) return;
  const promptAttachments = attachments;
  promptBox.value = "";
  attachments = [];
  renderAttachments();
  sendText(prompt, { promptAttachments });
}

$("send").addEventListener("click", send);
$("continue-btn").addEventListener("click", () => sendText(CONTINUE_PROMPT));
$("auto-continue").checked = localStorage.getItem("kz-auto-continue") === "1";
$("auto-continue").addEventListener("change", () => {
  if ($("auto-continue").checked && !autoContinueAllowed()) {
    $("auto-continue").checked = false;
    localStorage.setItem("kz-auto-continue", "0");
    autoRounds = 0;
    toast("连跑仅适用于自主推进模式，请先切换模式");
    log("连跑未开启:结伴开发模式不支持自动续跑");
    return;
  }
  localStorage.setItem("kz-auto-continue", $("auto-continue").checked ? "1" : "0");
  autoRounds = 0;
  log($("auto-continue").checked ? "连跑已开启:每轮结束自动推进目标(上限 10 连)" : "连跑已关闭");
});
$("profile-select").addEventListener("change", () => {
  if (!autoContinueAllowed() && $("auto-continue").checked) {
    $("auto-continue").checked = false;
    localStorage.setItem("kz-auto-continue", "0");
    autoRounds = 0;
    log("已切换结伴/研究模式，连跑自动关闭");
  }
});
$("stop").addEventListener("click", () => {
  // 本地立即复位,不依赖后端事件回执(事件通道故障时停止键也必须有效)。
  invoke("stop_run", { projectDir: currentProject }).catch((err) => log(`停止指令失败:${err}`, "err"));
  hideAsk();
  stopElapsed();
  setRunning(false, "已停止");
  log("已请求停止(本地已复位)");
});
promptBox.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    send();
  }
});

// ---------- 模型直选 ----------
async function loadModels() {
  const select = $("model-select");
  const saved = localStorage.getItem("kz-model") ?? "";
  select.innerHTML = "";
  const def = document.createElement("option");
  def.value = "";
  def.textContent = "模型:agent 默认";
  select.appendChild(def);
  try {
    const models = await invoke("models_list", { projectDir: currentProject });
    for (const m of models) {
      const opt = document.createElement("option");
      opt.value = m.id;
      opt.textContent = m.label;
      if (m.id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    log(`模型列表已刷新(${models.length} 个可选)`);
  } catch (err) {
    log(`模型列表获取失败:${err}`, "warn");
  }
}
$("model-select").addEventListener("change", () => {
  localStorage.setItem("kz-model", $("model-select").value);
});

// ---------- 项目管理 ----------
function baseName(path) {
  const parts = path.replaceAll("\\", "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

function renderProjects(prefs) {
  currentProject = prefs.current;
  const list = $("project-list");
  list.innerHTML = "";
  for (const path of prefs.projects) {
    const item = document.createElement("div");
    item.className = `project-item${path === prefs.current ? " active" : ""}`;
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = baseName(path);
    const pathEl = document.createElement("span");
    pathEl.className = "path";
    pathEl.textContent = path;
    const remove = document.createElement("button");
    remove.className = "icon-btn remove";
    remove.textContent = "×";
    remove.title = "移除(不删除文件)";
    remove.addEventListener("click", async (e) => {
      e.stopPropagation();
      renderProjects(await invoke("projects_remove", { path }));
      refreshDocs();
    });
    item.append(name, pathEl, remove);
    item.addEventListener("click", async () => {
      const previous = currentProject;
      renderProjects(await invoke("projects_select", { path }));
      if (previous && previous !== path) {
        clearChat();
        await loadConversation();
      }
      refreshDocs();
      loadModels();
      refreshGit();
    });
    list.appendChild(item);
  }
  $("project-label").textContent = prefs.current ?? "(未选择项目)";
}

$("project-add").addEventListener("click", async () => {
  try {
    const prefs = await invoke("projects_pick");
    if (prefs) {
      renderProjects(prefs);
      refreshDocs();
    }
  } catch (err) {
    toast(String(err));
  }
});

// ---------- 侧边栏文档(可展开 + 状态流转) ----------
const reqFilters = { status: "all", priority: "all", sort: "priority" };
const priorityRank = { P0: 0, P1: 1, P2: 2, P3: 3 };
const statusRank = { doing: 0, todo: 1, done: 2, dropped: 3 };

function filterRequirements(entries) {
  return entries
    .filter((entry) => reqFilters.status === "all" || entry.status === reqFilters.status)
    .filter((entry) => reqFilters.priority === "all" || entry.priority === reqFilters.priority)
    .sort((a, b) => {
      if (reqFilters.sort === "id") return a.id.localeCompare(b.id, undefined, { numeric: true });
      if (reqFilters.sort === "status") {
        return (statusRank[a.status] ?? 99) - (statusRank[b.status] ?? 99) || a.id.localeCompare(b.id, undefined, { numeric: true });
      }
      return (priorityRank[a.priority] ?? 99) - (priorityRank[b.priority] ?? 99)
        || (statusRank[a.status] ?? 99) - (statusRank[b.status] ?? 99)
        || a.id.localeCompare(b.id, undefined, { numeric: true });
    });
}

function renderDocList(el, entries, kind, archivedCount = 0) {
  if (kind === "req") entries = filterRequirements(entries);
  el.innerHTML = "";
  if (entries.length === 0 && archivedCount === 0) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = "(空)";
    el.appendChild(empty);
    return;
  }
  for (const entry of entries) {
    const item = document.createElement("div");
    item.className = `doc-item${entry.closed ? " closed" : ""}`;

    const row = document.createElement("div");
    row.className = "doc-row";
    row.title = `${entry.id} ${entry.title}(点击展开)`;
    const id = document.createElement("span");
    id.className = "id";
    id.textContent = entry.id;
    const st = document.createElement("span");
    st.className = `st st-${entry.status || "todo"}`;
    st.textContent = entry.status + (entry.priority ? `/${entry.priority}` : "") + (entry.severity ? `/${entry.severity}` : "");
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = entry.title;
    row.append(id, st, title);
    item.appendChild(row);

    // 展开面板:完整标题、字段、合法的状态流转按钮(与硬门禁同一套规则)。
    const detail = document.createElement("div");
    detail.className = "doc-detail hidden";
    const full = document.createElement("div");
    full.className = "doc-full-title";
    full.textContent = entry.title;
    detail.appendChild(full);
    for (const [key, value] of entry.fields ?? []) {
      const f = document.createElement("div");
      f.className = "doc-field";
      f.textContent = `${key}: ${value}`;
      detail.appendChild(f);
    }
    // 目标专属:进展速记(写入 fields.进展,注入上下文时 agent 可见)。
    if (kind === "goal" && !entry.closed) {
      const progressRow = document.createElement("div");
      progressRow.className = "doc-progress";
      const input = document.createElement("input");
      input.placeholder = "记录进展/调整方向,回车保存";
      input.addEventListener("click", (e) => e.stopPropagation());
      input.addEventListener("keydown", async (e) => {
        if (e.key !== "Enter" || !input.value.trim()) return;
        try {
          const msg = await invoke("docs_update", {
            projectDir: currentProject,
            kind,
            action: "update",
            id: entry.id,
            fields: { "进展": input.value.trim() },
          });
          log(msg);
          refreshDocs();
        } catch (err) {
          toast(String(err));
        }
      });
      progressRow.appendChild(input);
      detail.appendChild(progressRow);
    }
    if ((entry.nextStatuses ?? []).length > 0) {
      const actions = document.createElement("div");
      actions.className = "doc-actions";
      for (const next of entry.nextStatuses) {
        const btn = document.createElement("button");
        btn.className = "ghost mini";
        btn.textContent = `→ ${next}`;
        btn.addEventListener("click", async (e) => {
          e.stopPropagation();
          try {
            const msg = await invoke("docs_update", {
              projectDir: currentProject,
              kind,
              action: "update",
              id: entry.id,
              status: next,
            });
            log(msg);
            refreshDocs();
          } catch (err) {
            toast(String(err));
            log(`状态流转失败:${err}`, "warn");
          }
        });
        actions.appendChild(btn);
      }
      detail.appendChild(actions);
    }
    item.appendChild(detail);
    row.addEventListener("click", () => detail.classList.toggle("hidden"));
    el.appendChild(item);
  }
  // 已完成项归档在 *-archive.md,不占侧边栏;一行入口可翻历史。
  if (archivedCount > 0) {
    const foot = document.createElement("div");
    foot.className = "doc-empty";
    foot.style.cursor = "pointer";
    foot.title = "打开归档文件";
    foot.textContent = `${archivedCount} 条已归档 ↗`;
    foot.addEventListener("click", () => {
      openDocViewer(`${kind}-archive`);
    });
    el.appendChild(foot);
  }
}

async function refreshDocs() {
  if (!currentProject) return;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    renderDocList($("req-list"), snapshot.requirements, "req", snapshot.archived?.req ?? 0);
    renderDocList($("defect-list"), snapshot.defects, "defect", snapshot.archived?.defect ?? 0);
    renderDocList($("goal-list"), snapshot.goals ?? [], "goal", snapshot.archived?.goal ?? 0);
    $("req-count").textContent = `${snapshot.requirements.filter((r) => !r.closed).length}`;
    $("defect-count").textContent = `${snapshot.defects.filter((d) => !d.closed).length}`;
    $("goal-count").textContent = `${(snapshot.goals ?? []).filter((g) => g.status === "active").length}`;
    renderConventions(snapshot.conventions);
    await refreshConversationList();
  } catch (err) {
    console.error(err);
  }
}

for (const [id, key] of [["req-status-filter", "status"], ["req-priority-filter", "priority"], ["req-sort", "sort"]]) {
  $(id).addEventListener("change", (event) => {
    reqFilters[key] = event.target.value;
    refreshDocs();
  });
}

function renderConventions(conv) {
  const el = $("conv-list");
  el.innerHTML = "";
  if (!conv || !conv.exists) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = "(未创建,点 ＋ 生成模板;agent 会自动遵守此文件)";
    el.appendChild(empty);
    return;
  }
  for (const heading of conv.headings) {
    const item = document.createElement("div");
    item.className = "doc-item";
    item.textContent = `§ ${heading}`;
    el.appendChild(item);
  }
}

// 新建目标:内联输入(webview 无 window.prompt)。
$("goal-add").addEventListener("click", () => {
  const list = $("goal-list");
  if (list.querySelector(".goal-add-form")) return;
  const form = document.createElement("div");
  form.className = "goal-add-form";
  const input = document.createElement("input");
  input.placeholder = "目标描述,回车创建(Esc 取消)";
  input.addEventListener("keydown", async (e) => {
    if (e.key === "Escape") {
      form.remove();
      return;
    }
    if (e.key !== "Enter" || !input.value.trim()) return;
    try {
      const msg = await invoke("docs_update", {
        projectDir: currentProject,
        kind: "goal",
        action: "add",
        id: "",
        title: input.value.trim(),
      });
      log(msg);
      form.remove();
      refreshDocs();
    } catch (err) {
      toast(String(err));
    }
  });
  form.appendChild(input);
  list.prepend(form);
  input.focus();
});

$("conv-init").addEventListener("click", async () => {
  try {
    const path = await invoke("conventions_init", { projectDir: currentProject });
    toast(`规范文件已就绪:${path}`);
    refreshDocs();
  } catch (err) {
    toast(String(err));
  }
});
$("conv-open").addEventListener("click", () => openDocViewer("conventions"));

// ---------- 应用内文档查看器:markdown/代码直接渲染,外部打开是兜底 ----------
let viewerKind = null;
async function openDocViewer(kind) {
  try {
    const doc = await invoke("docs_read", { projectDir: currentProject, kind });
    viewerKind = kind;
    $("viewer-title").textContent = doc.name;
    const body = $("viewer-body");
    if (doc.name.endsWith(".md")) {
      body.className = "md";
      body.innerHTML = renderMarkdown(doc.content);
    } else {
      body.className = "";
      body.innerHTML = `<pre class="code">${escapeHtml(doc.content)}</pre>`;
    }
    body.scrollTop = 0;
    $("viewer-overlay").classList.remove("hidden");
  } catch (err) {
    toast(String(err));
  }
}
$("viewer-close").addEventListener("click", () => $("viewer-overlay").classList.add("hidden"));
$("viewer-overlay").addEventListener("click", (e) => {
  if (e.target === $("viewer-overlay")) $("viewer-overlay").classList.add("hidden");
});
$("viewer-external").addEventListener("click", () => {
  if (viewerKind) invoke("docs_open", { projectDir: currentProject, kind: viewerKind }).catch((e) => toast(String(e)));
});

// ---------- git 状态 ----------
async function refreshGit() {
  if (!currentProject) return;
  try {
    const g = await invoke("git_status", { projectDir: currentProject });
    $("status-git").textContent = g.branch
      ? `⎇ ${g.branch}${g.changes ? ` +${g.changes}` : ""}`
      : "";
    $("status-git").title = g.last ? `最近提交:${g.last}` : "";
  } catch {
    $("status-git").textContent = "";
  }
}

function renderRecoveredMessages(items) {
  followLatest = true;
  messages.innerHTML = "";
  currentAssistant = null;
  currentReasoning = null;
  currentReasoningHead = null;
  for (const message of items ?? []) {
    // 回放只呈现对话正文:思考/工具结果不回放,工具调用折叠成一行痕迹。
    const toolNames = [];
    for (const part of message.parts ?? []) {
      if (part.type === "tool_call" && part.name) toolNames.push(part.name);
    }
    if (toolNames.length) {
      const chip = document.createElement("div");
      chip.className = "tool-chip ok replay";
      const head = document.createElement("div");
      head.className = "head";
      head.textContent = toolNames.join(" · ");
      chip.appendChild(head);
      messages.appendChild(chip);
    }
    for (const part of message.parts ?? []) {
      if (part.type !== "text" || !part.text?.trim()) continue;
      const el = addMessage(message.role === "assistant" ? "assistant md" : "user", "");
      if (message.role === "assistant") {
        el.dataset.raw = part.text;
        el.querySelector(".message-body").innerHTML = renderMarkdown(part.text);
      } else {
        el.querySelector(".message-body").textContent = part.text;
      }
    }
  }
  if (!items?.length) {
    messages.innerHTML = '<div id="empty-state"><div class="logo-mark">K</div><div class="hint">输入任务开始 · 权限请求会弹窗询问 · Ctrl+Enter 发送</div></div>';
  }
  scrollBottom(true);
}

async function loadConversation(sequence = null) {
  if (!currentProject) return;
  try {
    const history = await invoke("conversation_get", { projectDir: currentProject, sequence });
    renderRecoveredMessages(history);
    log(`已恢复 ${history.length} 条历史消息`);
  } catch (err) {
    addMessage("error", `历史消息恢复失败:${err}`);
    log(`历史消息恢复失败:${err}`, "warn");
  }
}

function renderConversationList(items) {
  const el = $("conversation-list");
  el.innerHTML = "";
  $("conversation-count").textContent = items.length;
  if (!items.length) {
    el.textContent = "(暂无历史对话)";
    return;
  }
  for (const item of [...items].reverse()) {
    const row = document.createElement("div");
    row.className = "doc-item conv-row";
    row.title = "点击打开 · 勾选后点 🗑 批量删除";
    const check = document.createElement("input");
    check.type = "checkbox";
    check.className = "chat-check";
    check.dataset.seqs = JSON.stringify(item.sequences ?? [item.sequence]);
    check.addEventListener("click", (e) => e.stopPropagation());
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = `${item.title || "新对话"} (${item.message_count} 条)`;
    row.append(check, title);
    row.addEventListener("click", async () => {
      try {
        await loadConversation(item.sequence);
        addMessage("notice", `已打开历史对话 #${item.sequence}`);
      } catch (err) {
        toast(String(err));
      }
    });
    el.appendChild(row);
  }
}

$("chat-del").addEventListener("click", async () => {
  const sequences = [...document.querySelectorAll(".chat-check:checked")]
    .flatMap((c) => JSON.parse(c.dataset.seqs));
  if (!sequences.length) {
    toast("先勾选要删除的历史对话");
    return;
  }
  try {
    const n = await invoke("conversation_delete", { projectDir: currentProject, sequences });
    toast(`已删除 ${n} 份对话快照`);
    await refreshConversationList();
  } catch (err) {
    toast(String(err));
  }
});

async function refreshConversationList() {
  if (!currentProject) return;
  try {
    renderConversationList(await invoke("conversation_list", { projectDir: currentProject }));
  } catch (err) {
    $("conversation-list").textContent = `历史对话加载失败:${err}`;
  }
}

// ---------- 新对话 ----------
function clearChat(noticeText) {
  messages.innerHTML = "";
  currentAssistant = null;
  currentReasoning = null;
  currentReasoningHead = null;
  ctxTokens = 0;
  renderTokens();
  if (noticeText) addMessage("notice", noticeText);
}

$("new-chat").addEventListener("click", async () => {
  if (running) {
    toast("任务运行中,先停止再开新对话");
    return;
  }
  try {
    await invoke("conversation_clear", { projectDir: currentProject });
    clearChat("已开启新对话(历史已清空)");
    await refreshConversationList();
    log("新对话:多轮历史已清空");
  } catch (err) {
    toast(String(err));
  }
});

// ---------- 对话总结 ----------
$("summarize-btn").addEventListener("click", async () => {
  if (!currentProject) {
    toast("先选择一个项目");
    return;
  }
  const transcript = [...messages.querySelectorAll(".msg, .tool-chip")]
    .map((el) => el.textContent.trim())
    .filter(Boolean)
    .join("\n\n")
    .slice(0, 60000);
  if (!transcript) {
    toast("当前没有可总结的对话");
    return;
  }
  $("summarize-btn").disabled = true;
  setStatus("总结中(fast 模型)", true);
  log("开始总结当前对话…");
  try {
    const r = await invoke("summarize_chat", { projectDir: currentProject, transcript });
    addMessage("notice", `📋 对话总结\n${r.summary}\n(已存档 ${r.path})`);
    log(`总结完成,已存档:${r.path}`);
  } catch (err) {
    toast(`总结失败:${err}`);
    log(`总结失败:${err}`, "err");
  } finally {
    $("summarize-btn").disabled = false;
    setStatus(running ? "运行中" : "空闲", running);
  }
});

for (const [btn, kind] of [["req-open", "req"], ["defect-open", "defect"], ["goal-open", "goal"]]) {
  $(btn).addEventListener("click", () => openDocViewer(kind));
}

// ---------- 设置 ----------
let settingsProviders = [];

function renderProviders() {
  const tbody = document.querySelector("#providers-table tbody");
  tbody.innerHTML = "";
  settingsProviders.forEach((p, index) => {
    const tr = document.createElement("tr");

    const tdName = document.createElement("td");
    const nameInput = document.createElement("input");
    nameInput.value = p.name;
    nameInput.addEventListener("input", () => (p.name = nameInput.value));
    tdName.appendChild(nameInput);

    const tdProtocol = document.createElement("td");
    const protocolSelect = document.createElement("select");
    for (const proto of ["anthropic", "openai", "openai-responses"]) {
      const opt = document.createElement("option");
      opt.value = proto;
      opt.textContent = proto;
      if (p.protocol === proto) opt.selected = true;
      protocolSelect.appendChild(opt);
    }
    protocolSelect.addEventListener("change", () => (p.protocol = protocolSelect.value));
    tdProtocol.appendChild(protocolSelect);

    const tdUrl = document.createElement("td");
    const urlInput = document.createElement("input");
    urlInput.value = p.baseUrl;
    urlInput.addEventListener("input", () => (p.baseUrl = urlInput.value));
    tdUrl.appendChild(urlInput);

    const tdKey = document.createElement("td");
    if (p.auth) {
      // 特殊认证(codex 订阅登录态):只展示,不可编辑成 key。
      const badge = document.createElement("span");
      badge.className = "key-state key-ok";
      badge.textContent = `订阅登录态(${p.auth})`;
      tdKey.appendChild(badge);
    } else {
      const keyInput = document.createElement("input");
      keyInput.value = p.apiKeyEnv ?? "";
      keyInput.placeholder = "(本地服务留空)";
      keyInput.addEventListener("input", () => (p.apiKeyEnv = keyInput.value));
      tdKey.appendChild(keyInput);
      if (p.keyPresent !== null && p.keyPresent !== undefined) {
        const state = document.createElement("span");
        state.className = `key-state ${p.keyPresent ? "key-ok" : "key-missing"}`;
        state.textContent = p.keyPresent ? "已设" : "缺失";
        tdKey.appendChild(state);
      }
    }

    // D-015:context_limit 必须在表单可见可编辑,保存不许丢字段。
    const tdCtx = document.createElement("td");
    const ctxInput = document.createElement("input");
    ctxInput.type = "number";
    ctxInput.value = p.contextLimit ?? "";
    ctxInput.placeholder = "(不限)";
    ctxInput.addEventListener("input", () => {
      const n = parseInt(ctxInput.value, 10);
      p.contextLimit = Number.isFinite(n) && n > 0 ? n : null;
    });
    tdCtx.appendChild(ctxInput);

    const tdRemove = document.createElement("td");
    const removeBtn = document.createElement("button");
    removeBtn.className = "icon-btn";
    removeBtn.textContent = "×";
    removeBtn.addEventListener("click", () => {
      settingsProviders.splice(index, 1);
      renderProviders();
    });
    tdRemove.appendChild(removeBtn);

    tr.append(tdName, tdProtocol, tdUrl, tdKey, tdCtx, tdRemove);
    tbody.appendChild(tr);
  });
}

async function loadSettings() {
  const s = await invoke("settings_get");
  $("settings-path").textContent = s.path;
  $("set-primary").value = s.primary ?? "";
  $("set-fast").value = s.fast ?? "";
  $("set-profile").value = s.profileDefault;
  const proxy = s.proxy;
  if (proxy === "env" || proxy === "off") {
    $("set-proxy-mode").value = proxy;
    $("set-proxy-url").classList.add("hidden");
  } else {
    $("set-proxy-mode").value = "custom";
    $("set-proxy-url").value = proxy;
    $("set-proxy-url").classList.remove("hidden");
  }
  settingsProviders = s.providers;
  renderProviders();
}

$("set-proxy-mode").addEventListener("change", () => {
  $("set-proxy-url").classList.toggle("hidden", $("set-proxy-mode").value !== "custom");
});

$("provider-add").addEventListener("click", () => {
  settingsProviders.push({ name: "", protocol: "openai", baseUrl: "http://", apiKeyEnv: "" });
  renderProviders();
});

$("settings-save").addEventListener("click", async () => {
  const mode = $("set-proxy-mode").value;
  const proxy = mode === "custom" ? $("set-proxy-url").value.trim() : mode;
  try {
    await invoke("settings_save", {
      payload: {
        primary: $("set-primary").value,
        fast: $("set-fast").value,
        proxy,
        profileDefault: $("set-profile").value,
        providers: settingsProviders.map((p) => ({
          name: p.name,
          protocol: p.protocol,
          baseUrl: p.baseUrl,
          apiKeyEnv: p.apiKeyEnv || null,
          auth: p.auth || null,
          contextLimit: p.contextLimit ?? null,
        })),
      },
    });
    toast("已保存");
    loadSettings();
  } catch (err) {
    toast(`保存失败: ${err}`);
  }
});

$("settings-open").addEventListener("click", () => invoke("settings_open").catch((e) => toast(String(e))));

// ---------- 侧边栏分区折叠:点标题文字收/展,记忆到 localStorage ----------
document.querySelectorAll(".sidebar-section").forEach((section) => {
  const title = section.querySelector(".section-title > span:first-child");
  if (!title) return;
  // key 剔除数字:标题里的计数(如"目标 3")会变,不能进 key。
  const key = `kz-collapse-${title.textContent.replace(/[\d\s]/g, "").slice(0, 8)}`;
  if (localStorage.getItem(key) === "1") section.classList.add("collapsed");
  title.addEventListener("click", () => {
    const collapsed = section.classList.toggle("collapsed");
    localStorage.setItem(key, collapsed ? "1" : "0");
  });
});

// ---------- 启动 ----------
(async () => {
  try {
    const info = await invoke("app_info");
    $("status-version").textContent = `v${info.version} (${info.build})`;
    log(`kanzei 桌面端启动 · v${info.version} (${info.build})`);
  } catch (err) {
    log(`获取版本失败:${err}`, "warn");
  }
  renderProjects(await invoke("projects_get"));
  await loadConversation();
  await refreshDocs();
  await loadModels();
  refreshGit();
  setStatus("空闲", false);
})();
