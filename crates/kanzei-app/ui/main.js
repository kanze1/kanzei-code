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
let attachments = [];
let lastRequest = null;
let runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };

// ---------- 视图切换 ----------
document.querySelectorAll(".activity-item").forEach((item) => {
  item.addEventListener("click", () => {
    document.querySelectorAll(".activity-item").forEach((i) => i.classList.remove("active"));
    item.classList.add("active");
    const view = item.dataset.view;
    document.body.classList.toggle("documents-active", view === "documents");
    document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
    $(`view-${view}`).classList.add("active");
    if (view === "settings") loadSettings();
    if (view === "workspace") refreshWorkspace();
    if (view === "documents") refreshDocs();
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

let completionAudioContext = null;
const baseTitle = document.title;

function playRunNotice(kind) {
  try {
    const AudioCtor = window.AudioContext || window.webkitAudioContext;
    if (!AudioCtor) return;
    completionAudioContext ??= new AudioCtor();
    if (completionAudioContext.state === "suspended") completionAudioContext.resume().catch(() => {});
    const now = completionAudioContext.currentTime;
    const frequencies = kind === "failed" ? [220, 165] : kind === "stopped" ? [330] : [523, 659];
    frequencies.forEach((frequency, index) => {
      const oscillator = completionAudioContext.createOscillator();
      const gain = completionAudioContext.createGain();
      oscillator.frequency.value = frequency;
      oscillator.type = "sine";
      gain.gain.setValueAtTime(0.0001, now + index * 0.11);
      gain.gain.exponentialRampToValueAtTime(0.12, now + index * 0.11 + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + index * 0.11 + 0.1);
      oscillator.connect(gain).connect(completionAudioContext.destination);
      oscillator.start(now + index * 0.11);
      oscillator.stop(now + index * 0.11 + 0.11);
    });
  } catch (error) {
    log(`完成提示音不可用:${error}`, "warn");
  }
}

function notifyRunState(kind, text) {
  const labels = { completed: "运行完成", failed: "运行失败", stopped: "运行已停止" };
  const label = labels[kind] || "运行状态";
  toast(`${label}: ${text}`);
  playRunNotice(kind);
  if (!document.hasFocus() || document.hidden) {
    document.title = `🔔 ${label} · ${baseTitle}`;
    if ("Notification" in window && Notification.permission === "granted") {
      try {
        new Notification(label, { body: text, tag: "kanzei-run-state" });
      } catch (error) {
        log(`系统通知不可用:${error}`, "warn");
      }
    }
  }
}

document.addEventListener("visibilitychange", () => {
  if (!document.hidden && document.hasFocus()) document.title = baseTitle;
});
let activityPanelOpen = localStorage.getItem("kz-activity-panel") === "1";

function syncActivityPanel() {
  $("bg-panel").classList.toggle("hidden", !activityPanelOpen);
  const toggle = $("activity-toggle");
  toggle.classList.toggle("active", activityPanelOpen);
  toggle.textContent = activityPanelOpen ? "活动 ✓" : "活动";
  toggle.title = activityPanelOpen ? "隐藏右侧活动面板" : "显示右侧活动面板";
}

$("activity-toggle").addEventListener("click", () => {
  activityPanelOpen = !activityPanelOpen;
  localStorage.setItem("kz-activity-panel", activityPanelOpen ? "1" : "0");
  syncActivityPanel();
});
syncActivityPanel();

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
  $("status-mode").textContent = isRunning ? "运行中" : "空闲";
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
  const bar = $("ctx-bar");
  if (ctxTokens > 0) {
    const k = (ctxTokens / 1000).toFixed(1);
    if (ctxLimit) {
      const pct = Math.round((ctxTokens / ctxLimit) * 100);
      text += `${text ? " · " : ""}ctx ${k}k/${Math.round(ctxLimit / 1000)}k (${pct}%)`;
      $("status-tokens").classList.toggle("ctx-warn", pct >= 70);
      // 进度条:容量占用一眼可见,≥70% 变警示色(自动压缩阈值同源)。
      bar.classList.remove("hidden");
      bar.classList.toggle("warn", pct >= 70);
      $("ctx-bar-fill").style.width = `${Math.min(pct, 100)}%`;
      bar.title = `上下文 ${k}k / ${Math.round(ctxLimit / 1000)}k(${pct}%,≥70% 自动压缩)`;
    } else {
      text += `${text ? " · " : ""}ctx ${k}k`;
      bar.classList.add("hidden");
    }
  } else {
    bar.classList.add("hidden");
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

function addErrorMessage(message, { retryable = false } = {}) {
  const el = addMessage("error", "");
  const body = el.querySelector(".message-body");
  const level = document.createElement("strong");
  level.className = "error-level";
  level.textContent = retryable ? "可重试错误" : "致命错误";
  const text = document.createElement("div");
  text.textContent = message;
  body.append(level, text);
  if (retryable && lastRequest) {
    const actions = el.querySelector(".msg-actions");
    const retry = document.createElement("button");
    retry.className = "retry-btn";
    retry.type = "button";
    retry.textContent = "重试上一次请求";
    retry.addEventListener("click", () => {
      retry.disabled = true;
      retry.textContent = "正在重试…";
      sendText(lastRequest.prompt, { promptAttachments: lastRequest.attachments });
    });
    actions.appendChild(retry);
  }
  return el;
}

function isRetryableError(message) {
  return /timed out|timeout|connect|connection|dns|网络|连接|超时/i.test(message);
}

function reportError(message, { retryable = isRetryableError(message) } = {}) {
  addErrorMessage(message, { retryable });
  log(`错误:${message}`, "err");
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

// ---------- 活动面板(R-037):全部工具调用按序入列,详情点击展开,跑完保留可回看 ----------
const bgEntries = new Map(); // call_id -> {el, title, prog, meta, detail, startedAt, done}
const diffSummary = new Map();
const BG_MAX = 120;
function renderDiffSummary() {
  const panel = $("diff-summary");
  const label = $("diff-summary-toggle");
  const files = [...diffSummary.values()];
  const additions = files.reduce((sum, item) => sum + item.additions, 0);
  const deletions = files.reduce((sum, item) => sum + item.deletions, 0);
  label.textContent = files.length ? `· ${files.length} 文件 +${additions}/−${deletions}` : "";
  panel.classList.toggle("hidden", files.length === 0);
  panel.innerHTML = files.map((item) => `<div class="diff-summary-row"><span>${escapeHtml(item.path)}</span><span>+${item.additions}/−${item.deletions}</span></div>`).join("");
}

function bgSync() {
  // 面板开关只由用户控制;工具事件只能更新内容,不能擅自开关。
  syncActivityPanel();
}
function bgAdd(id, name, summary) {
  if (!id || bgEntries.has(id)) return;
  const el = document.createElement("div");
  el.className = "bg-entry running";
  const title = document.createElement("div");
  title.className = "bg-title";
  title.textContent = `${name} ${summary}`;
  title.title = summary;
  const prog = document.createElement("div");
  prog.className = "bg-prog";
  prog.textContent = name === "task" ? "… 子代理启动中" : "…";
  const meta = document.createElement("div");
  meta.className = "bg-meta";
  const detail = document.createElement("div");
  detail.className = "bg-detail hidden";
  title.addEventListener("click", () => {
    if (detail.children.length) detail.classList.toggle("hidden");
  });
  el.append(title, prog, meta, detail);
  const list = $("bg-list");
  list.appendChild(el);
  while (list.children.length > BG_MAX) list.firstElementChild.remove();
  bgEntries.set(id, { el, title, prog, meta, detail, children: new Map(), startedAt: Date.now(), done: false });
  bgSync();
  list.scrollTop = list.scrollHeight;
}
function highlightLine(container, text, language) {
  const pattern = /("(?:\\.|[^"])*"|'(?:\\.|[^'])*'|\/\/.*|#.*|\b\d+(?:\.\d+)?\b|\b(?:fn|let|const|function|class|return|if|else|for|while|pub|struct|use|import|from|true|false|null|None|async|await)\b)/g;
  let cursor = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > cursor) container.appendChild(document.createTextNode(text.slice(cursor, match.index)));
    const token = document.createElement("span");
    token.className = match[0].startsWith("//") || match[0].startsWith("#") ? "syntax-comment" : /^['"]/.test(match[0]) ? "syntax-string" : /^\d/.test(match[0]) ? "syntax-number" : "syntax-keyword";
    token.textContent = match[0];
    container.appendChild(token);
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) container.appendChild(document.createTextNode(text.slice(cursor)));
}

function renderDiff(display) {
  const block = document.createElement("div");
  block.className = "tool-display diff";
  let mode = "unified";
  const header = document.createElement("div");
  header.className = "diff-file-header";
  const label = document.createElement("span");
  label.textContent = `${display.path || "文件"}  +${display.additions || 0} −${display.deletions || 0} · ${display.language || "text"}`;
  const toggle = document.createElement("button");
  toggle.className = "ghost mini";
  toggle.textContent = "并排";
  header.append(label, toggle);
  const body = document.createElement("div");
  body.className = "diff-body";
  const lines = display.lines?.length ? display.lines : (display.diff || "").split("\n").filter(Boolean).map((text) => ({ kind: text[0] === "+" ? "add" : text[0] === "-" ? "del" : "ctx", text: text.slice(1) }));
  function render() {
    body.innerHTML = "";
    body.className = `diff-body ${mode}`;
    if (mode === "unified") {
      let oldLine = 1;
      let newLine = 1;
      for (const line of lines) {
        const row = document.createElement("div");
        row.className = `diff-row ${line.kind || "ctx"}`;
        const oldNo = document.createElement("span");
        const newNo = document.createElement("span");
        oldNo.className = newNo.className = "diff-line-number";
        oldNo.textContent = line.old_line ?? (line.kind === "add" ? "" : oldLine++);
        newNo.textContent = line.new_line ?? (line.kind === "del" ? "" : newLine++);
        const text = document.createElement("code");
        highlightLine(text, line.text || "", display.language || "text");
        row.append(oldNo, newNo, text);
        body.appendChild(row);
      }
    } else {
      const rows = [];
      for (let i = 0; i < lines.length; i += 1) {
        const line = lines[i];
        if (line.kind === "del" && lines[i + 1]?.kind === "add") rows.push([line, lines[++i]]);
        else if (line.kind === "del") rows.push([line, null]);
        else if (line.kind === "add") rows.push([null, line]);
        else rows.push([line, line]);
      }
      for (const [left, right] of rows) {
        const row = document.createElement("div");
        row.className = "diff-split-row";
        for (const line of [left, right]) {
          const pane = document.createElement("div");
          pane.className = `diff-pane ${line?.kind || "empty"}`;
          if (line) {
            const no = document.createElement("span");
            no.className = "diff-line-number";
            no.textContent = line.old_line ?? line.new_line ?? "";
            const text = document.createElement("code");
            highlightLine(text, line.text || "", display.language || "text");
            pane.append(no, text);
          }
          row.appendChild(pane);
        }
        body.appendChild(row);
      }
    }
  }
  toggle.addEventListener("click", () => {
    mode = mode === "unified" ? "split" : "unified";
    toggle.textContent = mode === "unified" ? "并排" : "统一";
    render();
  });
  block.append(header, body);
  render();
  return block;
}
function bgProgress(id, text, trace) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  if (text) entry.prog.textContent = text;
  if (!trace) return;
  entry.detail.classList.add("trace-detail");
  let child = entry.children.get(trace.child_id);
  if (trace.phase === "start") {
    if (!child) {
      const row = document.createElement("div");
      row.className = "bg-child running";
      const head = document.createElement("div");
      head.className = "bg-child-head";
      head.textContent = `${trace.name} ${trace.summary || ""}`;
      const meta = document.createElement("div");
      meta.className = "bg-child-meta";
      row.append(head, meta);
      entry.detail.appendChild(row);
      child = { row, head, meta };
      entry.children.set(trace.child_id, child);
      entry.el.classList.add("has-detail");
    }
  } else if (child) {
    child.row.classList.remove("running");
    child.row.classList.add(trace.ok ? "ok" : "err");
    child.meta.textContent = trace.preview || (trace.ok ? "完成" : "失败");
    appendDisplayBlock(child.row, trace.display);
  }
}

function bgEnd(id, ok, preview, display) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  entry.done = true;
  entry.el.classList.remove("running");
  entry.el.classList.add(ok ? "ok" : "err");
  entry.prog.textContent = preview || (ok ? "完成" : "失败");
  entry.meta.textContent = `${Math.round((Date.now() - entry.startedAt) / 1000)}s`;
  // 结构化详情进面板内展开区(diff/终端/新建/todo)。
  const d = display;
  if (d?.kind === "diff") {
    entry.title.textContent += `  +${d.additions} −${d.deletions}`;
    diffSummary.set(d.path || entry.title.textContent, {
      path: d.path || "未命名文件",
      additions: d.additions || 0,
      deletions: d.deletions || 0,
    });
    renderDiffSummary();
    entry.detail.appendChild(renderDiff(d));
  } else if (d?.kind === "terminal") {
    const block = document.createElement("div");
    block.className = "tool-display term";
    block.textContent = `$ ${d.command}\n${d.output}`;
    entry.detail.appendChild(block);
  } else if (d?.kind === "create") {
    const block = document.createElement("div");
    block.className = "tool-display term";
    block.textContent = `新建 ${d.path}(${d.bytes} bytes)\n${d.preview}`;
    entry.detail.appendChild(block);
  }
  if (!ok && preview) {
    const err = document.createElement("div");
    err.className = "tool-display term";
    err.textContent = preview;
    entry.detail.appendChild(err);
  }
  if (entry.detail.children.length) entry.el.classList.add("has-detail");
}
function renderRecoveredTraces(payloads) {
  const ids = new Set();
  for (const payload of payloads || []) {
    for (const event of payload.events || []) {
      ids.add(event.id);
      if (!bgEntries.has(event.id)) bgAdd(event.id, "task", "历史子代理轨迹");
      bgProgress(event.id, event.text, event.trace);
    }
  }
  for (const id of ids) {
    const entry = bgEntries.get(id);
    if (!entry) continue;
    entry.done = true;
    entry.el.classList.remove("running");
    entry.el.classList.add("ok");
    entry.prog.textContent = "历史轨迹";
    entry.meta.textContent = "回放";
  }
}

function bgClear() {
  for (const entry of bgEntries.values()) entry.el.remove();
  bgEntries.clear();
  $("bg-list").innerHTML = "";
  bgSync();
}
// 中止/出错时把仍在跑的条目标记为中止,不再空转。
function bgAbortRunning(label) {
  for (const entry of bgEntries.values()) {
    if (!entry.done) {
      entry.done = true;
      entry.el.classList.remove("running");
      entry.el.classList.add("err");
      entry.prog.textContent = label;
    }
  }
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
    // 轮次分隔不再进主对话区(用户定调:对话为主);轮次在侧边栏"当前进展"实时可见。
  }
  // 活动面板跨轮保留历史,由用户主动清空/切换项目时清理。
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
let todoItems = [];
function renderTodoPanel(items, done, total) {
  todoItems = items || [];
  const panel = $("todo-panel");
  const list = $("todo-list");
  list.innerHTML = "";
  panel.classList.toggle("hidden", todoItems.length === 0);
  $("todo-count").textContent = total ? `${done}/${total}` : "";
  for (const item of todoItems) {
    const row = document.createElement("div");
    row.className = `todo-entry ${item.status}`;
    const status = document.createElement("span");
    status.className = "todo-status";
    status.textContent = item.status === "done" ? "✓" : item.status === "doing" ? "●" : item.status === "dropped" ? "×" : "○";
    const content = document.createElement("span");
    content.className = "todo-content";
    content.textContent = item.content;
    row.append(status, content);
    list.appendChild(row);
  }
}

// R-037 对话为主:工具活动一律不进主对话区,收束到右侧活动面板。
let lastCompactionSummary = "";
let lastCompactionEntry = null;

function addCompactionEntry(summary) {
  const el = document.createElement("div");
  el.className = "bg-entry ok compaction-entry";
  const title = document.createElement("div");
  title.className = "bg-title";
  title.textContent = "上下文压缩 · 点击查看纪要";
  const detail = document.createElement("div");
  detail.className = "bg-detail";
  detail.textContent = summary;
  el.append(title, detail);
  title.addEventListener("click", () => detail.classList.toggle("hidden"));
  $("bg-list").appendChild(el);
  while ($("bg-list").childElementCount > BG_MAX) $("bg-list").firstElementChild.remove();
  lastCompactionEntry = el;
}

function addSummaryEntry(summary, path = "") {
  const el = document.createElement("div");
  el.className = "bg-entry ok summary-entry";
  const title = document.createElement("div");
  title.className = "bg-title";
  title.textContent = "对话小总结 · 点击查看";
  const detail = document.createElement("div");
  detail.className = "bg-detail";
  detail.textContent = path ? `${summary}\n\n已存档: ${path}` : summary;
  el.append(title, detail);
  title.addEventListener("click", () => detail.classList.toggle("hidden"));
  $("bg-list").appendChild(el);
  while ($("bg-list").childElementCount > BG_MAX) $("bg-list").firstElementChild.remove();
  return el;
}
function renderContextDetail() {
  const detail = $("context-detail");
  const t = runTokens;
  const total = t.input + t.cacheRead + t.output;
  detail.innerHTML = `<strong>上下文成分</strong><br>输入上下文(系统/历史/工具结果): ${t.input.toLocaleString()} tokens<br>缓存读取(已复用上下文): ${t.cacheRead.toLocaleString()} tokens<br>本轮输出: ${t.output.toLocaleString()} tokens${lastCompactionSummary ? "<br>最近一次压缩纪要已收进活动面板" : ""}<br>合计: ${total.toLocaleString()} tokens`;
  if (lastCompactionEntry) {
    activityPanelOpen = true;
    syncActivityPanel();
    lastCompactionEntry.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }
}

$("status-tokens").title = "点击查看上下文成分";
$("status-tokens").classList.add("context-clickable");
$("status-tokens").addEventListener("click", renderContextDetail);
on("kz:tool-start", (e) => {
  markFirstSignal();
  log(`工具 ${e.payload.name} ${e.payload.summary}`);
  currentAssistant = null;
  currentReasoning = null;
  bgAdd(e.payload.id, e.payload.name, e.payload.summary);
  liveSet("live-action", `⚙ ${e.payload.name} ${e.payload.summary.slice(0, 60)}`);
  setStatus(`工具执行中 · ${e.payload.name}`, true);
});
on("kz:task-progress", (e) => bgProgress(e.payload.id, e.payload.text, e.payload.trace));
on("kz:tool-end", (e) => {
  const p = e.payload;
  log(`工具结果 ${p.name}: ${p.ok ? "成功" : "失败"} — ${p.preview}`, p.ok ? "" : "warn");
  // 工作焦点:req/defect/goal 的增改结果最能代表"它在干哪件事"。
  if (p.ok && ["req", "defect", "goal"].includes(p.name)) {
    liveSet("live-focus", `◉ ${p.preview.replace(/^(updated|added):?\s*/, "").slice(0, 60)}`);
  }
  if (p.display?.kind === "todo") {
    renderTodoPanel(p.display.items || [], p.display.done || 0, p.display.total || 0);
  }
  bgEnd(p.id, p.ok, p.preview, p.display);
  setStatus("运行中", true);
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
  cancelAutoContinueTimer();
  const message = e.payload.message;
  reportError(message);
  stopElapsed();
  setRunning(false, "出错");
  bgAbortRunning("(出错中止)");
  liveIdle("出错");
  notifyRunState("failed", message);
  $("log-panel").classList.remove("hidden");
});
on("kz:compacted", (e) => {
  lastCompactionSummary = e.payload?.summary ?? "";
  addMessage("notice", "🗜 上下文占用过高,已自动压缩为纪要并延续对话");
  if (lastCompactionSummary) addCompactionEntry(lastCompactionSummary);
  log("自动压缩完成:多轮历史已替换为纪要");
  ctxTokens = 0;
  renderTokens();
});
on("kz:stopped", (e) => {
  cancelAutoContinueTimer();
  hideAsk();
  const cancelled = e.payload?.cancelled_queue ?? 0;
  addMessage("notice", cancelled > 0 ? `已停止,已取消 ${cancelled} 条排队输入` : "已停止");
  log(cancelled > 0 ? `已手动停止并取消 ${cancelled} 条排队输入` : "已手动停止");
  stopElapsed();
  setRunning(false, "已停止");
  bgAbortRunning("(已停止)");
  liveIdle("已停止");
  notifyRunState("stopped", cancelled > 0 ? `已停止并取消 ${cancelled} 条排队输入` : "已停止");
  refreshPendingInputs();
});
on("kz:done", async (e) => {
  const p = e.payload;
  setAutoStopReason(p.halted ? "用户拒绝后停止" : "本轮完成");

  addMessage(
    "notice",
    `完成 · steps ${p.steps}${p.history ? ` · 会话 ${p.history} 条` : ""}${p.halted ? " · 已按你的拒绝停止" : ""}`
  );
  log(`运行完成:${p.steps} 轮,耗时 ${((Date.now() - runStart) / 1000).toFixed(1)}s`);
  stopElapsed();
  notifyRunState(p.halted ? "stopped" : "completed", p.halted ? "已按你的拒绝停止" : `完成 ${p.steps} 轮`);
  setRunning(false);
  // 对齐 Claude:当前对话跑完一轮就出现在历史列表里,不用等重启/切项目。
  refreshConversationList();
  // 活动面板保留本轮全部轨迹供回看,下一轮开跑时才翻页(kz:turn step 1)。
  liveIdle(`空闲 · 上轮 ${p.steps} 轮完成`);
  refreshDocs();
  refreshGit();
  refreshPendingInputs();

  // 鞭挞:正常完成且上轮有实质动作(>1 轮 = 有工具调用)才续;拒绝/纯聊天即停。
  if (await stopAutoWhenBacklogEmpty()) return;
  if ($("auto-continue").checked && autoContinueAllowed() && !p.halted) {
    if (autoPaused) {
      addMessage("notice", "鞭挞已暂停,点击“继续鞭挞”恢复");
      return;
    }
    if (autoStopAfterRound) {
      autoStopAfterRound = false;
      $("auto-stop-round").checked = false;
      addMessage("notice", "已完成本轮,鞭挞停止");
      return;
    }
    if (p.steps <= 1 && autoRounds > 0) {
      addMessage("notice", "鞭挞停止:上一轮没有实质动作(可能目标已达成或被阻塞)");
      log("鞭挞停止:steps<=1");
      autoRounds = 0;
      return;
    }
    const max = autoContinueMax();
    if (autoRounds >= max) {
      addMessage("notice", `鞭挞停止:已达 ${max} 连上限,点“继续”或重开鞭挞`);
      autoRounds = 0;
      return;
    }
    autoRounds += 1;
    setStatus(`自主推进 ${autoRounds}/${max} · 2 秒后继续…`, false);
    renderAutoStatus(`自主推进 ${autoRounds}/${max} · 等待下一轮`);
    scheduleAutoContinue();
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

function updateAskQueueStatus() {
  const total = (askActive ? 1 : 0) + askQueue.length;
  const status = $("ask-queue-status");
  const preview = $("ask-queue-preview");
  status.textContent = total > 1 ? `当前请求 1/${total} · 还有 ${total - 1} 条待处理` : "当前无其他待处理请求";
  const lines = askQueue.slice(0, 4).map((item, index) => {
    const text = item.kind === "question" ? item.question : `${item.action} · ${item.resource}`;
    return `${index + 2}. ${text}`;
  });
  preview.textContent = lines.join("\n");
  preview.classList.toggle("hidden", lines.length === 0);
}

function pumpAsk() {
  if (askActive || askQueue.length === 0) {
    updateAskQueueStatus();
    return;
  }
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
  updateAskQueueStatus();
}

function hideAsk() {
  askQueue.length = 0;
  askActive = null;
  $("ask-overlay").classList.add("hidden");
  updateAskQueueStatus();
}

async function answerAsk(reply) {
  if (!askActive) return;
  const id = askActive.id;
  const question = askActive.kind === "question";
  const summary = question ? askActive.question : `${askActive.action}: ${askActive.resource}`;
  askActive = null;
  $("ask-overlay").classList.add("hidden");
  updateAskQueueStatus();
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

// ---------- 复制上下文:整段对话导出为 markdown(贴给其他 AI 用) ----------
$("copy-context").addEventListener("click", async () => {
  const parts = [];
  for (const el of messages.children) {
    if (el.classList.contains("user")) {
      const text = (el.querySelector(".message-body")?.textContent ?? el.textContent).trim();
      if (text) parts.push(`## 用户\n${text}`);
    } else if (el.classList.contains("assistant")) {
      const raw = (el.dataset.raw ?? el.textContent).trim();
      if (raw) parts.push(`## 助手\n${raw}`);
    } else if (el.classList.contains("reasoning")) {
      const raw = el.querySelector(".reasoning-body")?.dataset.raw?.trim();
      if (raw) parts.push(`> 思考:${raw.split("\n").find(Boolean)?.slice(0, 160) ?? ""}`);
    } else if (el.classList.contains("tool-chip")) {
      const head = el.querySelector(".head")?.textContent?.trim();
      if (head) parts.push(`> 工具:${head.slice(0, 200)}`);
    } else if (el.classList.contains("turn-divider")) {
      parts.push(`---\n${el.textContent}`);
    }
  }
  if (!parts.length) {
    toast("当前没有可复制的对话");
    return;
  }
  try {
    await navigator.clipboard.writeText(parts.join("\n\n"));
    toast(`已复制上下文(${parts.length} 段)`);
  } catch (err) {
    log(`复制上下文失败:${err}`, "err");
    toast("复制失败,请检查剪贴板权限");
  }
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
// 鞭挞状态:自动续跑计数(手动发送归零),上限防失控。
const DEFAULT_AUTO_CONTINUE_MAX = 10;
let autoRounds = 0;
let autoPaused = false;
let autoStopAfterRound = false;
let autoContinueTimer = null;
let autoContinueGeneration = 0;
let autoOvernight = localStorage.getItem("kz-overnight-mode") === "1";
let autoStopReason = "";
const CONTINUE_PROMPT =
  "继续:检查活跃目标(goal list)与最新进展,推进下一个具体步骤并落地(改代码/跑测试/更新文档);" +
  "完成后用 goal update 记录进展。收尾优先:已是 doing 的需求先推到 done(req update <id> done)再开新的,doing 同时不超过 2 个。" +
  "取活顺序:按需求列表自上而下拿第一个可做的(列表顺序即用户意志,priority 只是背景信息)。" +
  "若工作区有已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
  "若所有活跃目标已达成或被阻塞,明确说明原因,不要做无意义的空转。";

function selectedAgent() {
  const mode = $("profile-select").value;
  if (mode === "dev-pair") return { profile: "dev", agent: "dev-pair" };
  if (mode === "dev-auto") return { profile: "dev", agent: "dev" };
  return { profile: "research", agent: "research" };
}

function renderAutoStatus(text = autoStopReason) {
  const el = $("auto-status");
  if (!el) return;
  const max = autoContinueMax();
  el.textContent = text || (autoOvernight ? `过夜待机 · 上限 ${max}` : `连续推进上限 ${max}`);
}

function setAutoStopReason(reason) {
  autoStopReason = reason;
  renderAutoStatus(reason);
}
function autoContinueAllowed() {
  return $("profile-select").value === "dev-auto";
}
function autoContinueMax() {
  const value = Number.parseInt($("auto-max").value, 10);
  return Number.isFinite(value) ? Math.min(100, Math.max(1, value)) : DEFAULT_AUTO_CONTINUE_MAX;
}
function cancelAutoContinueTimer() {
  if (autoContinueTimer) clearTimeout(autoContinueTimer);
  autoContinueTimer = null;
  autoContinueGeneration += 1;
}
function scheduleAutoContinue() {
  cancelAutoContinueTimer();
  const generation = autoContinueGeneration;
  autoContinueTimer = setTimeout(() => {
    autoContinueTimer = null;
    if (generation !== autoContinueGeneration || autoPaused || autoStopAfterRound) return;
    if ($("auto-continue").checked && autoContinueAllowed() && !running) {
      sendText(CONTINUE_PROMPT, { auto: true });
    }
  }, 2000);
}

async function stopAutoWhenBacklogEmpty() {
  if (!$("auto-continue").checked || !autoContinueAllowed()) return false;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    const active = [...(snapshot.requirements ?? []), ...(snapshot.defects ?? [])]
      .some((entry) => !entry.closed && !["done", "dropped", "fixed", "wontfix"].includes(entry.status));
    if (active) return false;
    $("auto-continue").checked = false;
    localStorage.setItem("kz-auto-continue", "0");
    autoRounds = 0;
    cancelAutoContinueTimer();
    setAutoStopReason("需求与缺陷已清空，自动推进停止");
    addMessage("notice", "✅ 需求与缺陷已清空，自动推进已停止");
    log("自动推进停止:需求与缺陷已清空");
    return true;
  } catch (error) {
    log(`检查需求/缺陷是否清空失败:${error}`, "warn");
    return false;
  }
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
    toast("当前任务还在运行，自动鞭挞将在本轮完成后继续");
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
      await refreshPendingInputs();
    } catch (err) {
      reportError(String(err), { retryable: false });
    }
    return;
  }
  if (!auto) {
    autoRounds = 0;
    cancelAutoContinueTimer();
  }
  currentAssistant = null;
  currentReasoning = null;
  runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };
  ctxTokens = 0;
  outputChars = 0;
  renderTokens();
  addMessage("user", auto ? `(鞭挞 ${autoRounds}/${autoContinueMax()})${prompt}` : prompt);
  setRunning(true, auto ? `鞭挞 ${autoRounds}/${autoContinueMax()} · 准备中` : "准备中");
  startElapsed();
  log(`${auto ? "鞭挞" : "发送"}:${prompt.slice(0, 80)}`);
  try {
    const mode = selectedAgent();
    const request = {
      prompt,
      projectDir: currentProject,
      profile: mode.profile,
      agent: mode.agent,
      model: $("model-select").value || null,
      delivery,
      attachments: promptAttachments.map((item) => ({ ...item })),
    };
    if (!auto) lastRequest = request;
    await invoke("run_prompt", request);
  } catch (err) {
    reportError(String(err));
    stopElapsed();
    setRunning(false);
  }
}

const PROMPT_HISTORY_KEY = "kz-prompt-history";
const PROMPT_HISTORY_LIMIT = 30;
let promptHistory = (() => {
  try { return JSON.parse(localStorage.getItem(PROMPT_HISTORY_KEY) || "[]").filter((item) => typeof item === "string"); }
  catch (_) { return []; }
})();
let promptHistoryIndex = -1;
let promptHistoryDraft = "";

function rememberPrompt(prompt) {
  const value = prompt.trim();
  if (!value) return;
  promptHistory = [value, ...promptHistory.filter((item) => item !== value)].slice(0, PROMPT_HISTORY_LIMIT);
  localStorage.setItem(PROMPT_HISTORY_KEY, JSON.stringify(promptHistory));
  promptHistoryIndex = -1;
}

function navigatePromptHistory(direction) {
  if (promptHistory.length === 0) return false;
  if (promptHistoryIndex === -1) promptHistoryDraft = promptBox.value;
  const next = promptHistoryIndex + direction;
  if (next < 0 || next > promptHistory.length) return false;
  promptHistoryIndex = next;
  promptBox.value = next === promptHistory.length ? promptHistoryDraft : promptHistory[next];
  promptBox.setSelectionRange(promptBox.value.length, promptBox.value.length);
  return true;
}

let fileSuggestions = [];
let fileSuggestionIndex = -1;
let fileSuggestionToken = null;
let fileSuggestionRequest = 0;

function currentFileToken() {
  const cursor = promptBox.selectionStart;
  const before = promptBox.value.slice(0, cursor);
  const match = before.match(/(?:^|\s)@([^\s]*)$/);
  if (!match) return null;
  return { start: cursor - match[1].length - 1, end: cursor, query: match[1] };
}

function hideFileSuggestions() {
  fileSuggestions = [];
  fileSuggestionIndex = -1;
  fileSuggestionToken = null;
  $("file-suggestions").classList.add("hidden");
  $("file-suggestions").replaceChildren();
}

function renderFileSuggestions() {
  const box = $("file-suggestions");
  box.replaceChildren();
  fileSuggestions.forEach((path, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `file-suggestion${index === fileSuggestionIndex ? " active" : ""}`;
    button.textContent = `@${path}`;
    button.addEventListener("mousedown", (event) => {
      event.preventDefault();
      chooseFileSuggestion(index);
    });
    box.appendChild(button);
  });
  box.classList.toggle("hidden", fileSuggestions.length === 0);
}

function chooseFileSuggestion(index = fileSuggestionIndex) {
  const path = fileSuggestions[index];
  const token = currentFileToken() || fileSuggestionToken;
  if (!path || !token) return;
  promptBox.value = `${promptBox.value.slice(0, token.start)}@${path} ${promptBox.value.slice(token.end)}`;
  const cursor = token.start + path.length + 2;
  promptBox.focus();
  promptBox.setSelectionRange(cursor, cursor);
  hideFileSuggestions();
}

async function refreshFileSuggestions() {
  const token = currentFileToken();
  if (!token || !currentProject) {
    hideFileSuggestions();
    return;
  }
  fileSuggestionToken = token;
  const request = ++fileSuggestionRequest;
  try {
    const paths = await invoke("project_files", { projectDir: currentProject, query: token.query });
    if (request !== fileSuggestionRequest || !currentFileToken()) return;
    fileSuggestions = paths;
    fileSuggestionIndex = paths.length ? 0 : -1;
    renderFileSuggestions();
  } catch (error) {
    hideFileSuggestions();
    log(`文件补全失败:${error}`, "warn");
  }
}

let fileSuggestionTimer = null;
promptBox.addEventListener("input", () => {
  promptHistoryIndex = -1;
  clearTimeout(fileSuggestionTimer);
  fileSuggestionTimer = setTimeout(refreshFileSuggestions, 80);
});
function send() {
  const prompt = promptBox.value.trim();
  if (!prompt && attachments.length === 0) return;
  rememberPrompt(prompt);
  hideFileSuggestions();
  const promptAttachments = attachments;
  promptBox.value = "";
  attachments = [];
  renderAttachments();
  sendText(prompt, { promptAttachments });
}

$("send").addEventListener("click", send);
$("continue-btn").addEventListener("click", () => sendText(CONTINUE_PROMPT));
$("auto-continue").checked = localStorage.getItem("kz-auto-continue") === "1";
$("overnight-mode").checked = autoOvernight;
renderAutoStatus();
$("overnight-mode").addEventListener("change", () => {
  autoOvernight = $("overnight-mode").checked;
  localStorage.setItem("kz-overnight-mode", autoOvernight ? "1" : "0");
  setAutoStopReason(autoOvernight ? "过夜模式已启用，需手动开启鞭挞后运行" : "过夜模式已关闭");
});
$("auto-max").value = Math.min(100, Math.max(1, Number.parseInt(localStorage.getItem("kz-auto-max"), 10) || DEFAULT_AUTO_CONTINUE_MAX));
$("auto-stop-round").checked = localStorage.getItem("kz-auto-stop-round") === "1";
autoStopAfterRound = $("auto-stop-round").checked;
$("auto-pause").addEventListener("click", () => {
  autoPaused = !autoPaused;
  $("auto-pause").classList.toggle("active", autoPaused);
  $("auto-pause").textContent = autoPaused ? "继续鞭挞" : "暂停鞭挞";
  if (autoPaused) cancelAutoContinueTimer();
  // BUG 修复:恢复时如果正处于轮间空闲,必须重新调度,否则鞭挞静默死亡。
  if (!autoPaused && !running && $("auto-continue").checked && autoContinueAllowed()) {
    setStatus("鞭挞恢复,2 秒后继续…", false);
    scheduleAutoContinue();
  }
  log(autoPaused ? "鞭挞已暂停" : "鞭挞已恢复");
});
$("auto-stop-round").addEventListener("change", () => {
  autoStopAfterRound = $("auto-stop-round").checked;
  localStorage.setItem("kz-auto-stop-round", autoStopAfterRound ? "1" : "0");
});
$("auto-max").addEventListener("change", () => {
  const max = autoContinueMax();
  $("auto-max").value = max;
  localStorage.setItem("kz-auto-max", String(max));
  renderAutoStatus();
  autoRounds = 0;
  cancelAutoContinueTimer();
  log(`鞭挞上限已设为 ${max} 连`);
});
$("auto-continue").addEventListener("change", () => {
  if ($("auto-continue").checked && !autoContinueAllowed()) {
    $("auto-continue").checked = false;
    localStorage.setItem("kz-auto-continue", "0");
    autoRounds = 0;
    cancelAutoContinueTimer();
    toast("鞭挞仅适用于自主推进模式，请先切换模式");
    log("鞭挞未开启:结伴开发模式不支持自动续跑");
    return;
  }
  localStorage.setItem("kz-auto-continue", $("auto-continue").checked ? "1" : "0");
  autoRounds = 0;
  if (!$('auto-continue').checked) cancelAutoContinueTimer();
  log($("auto-continue").checked ? `鞭挞已开启:每轮结束自动推进目标(上限 ${autoContinueMax()} 连)` : "鞭挞已关闭");
});
$("profile-select").addEventListener("change", () => {
  if (!autoContinueAllowed() && $("auto-continue").checked) {
    $("auto-continue").checked = false;
    localStorage.setItem("kz-auto-continue", "0");
    autoRounds = 0;
    cancelAutoContinueTimer();
    log("已切换结伴/研究模式，鞭挞自动关闭");
  }
});
$("stop").addEventListener("click", () => {
  // 本地立即复位,不依赖后端事件回执(事件通道故障时停止键也必须有效)。
  cancelAutoContinueTimer();
  autoRounds = 0;
  invoke("stop_run", { projectDir: currentProject }).catch((err) => log(`停止指令失败:${err}`, "err"));
  hideAsk();
  stopElapsed();
  setRunning(false, "已停止");
  log("已请求停止(本地已复位)");
});
promptBox.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("file-suggestions").classList.contains("hidden")) {
    e.preventDefault();
    hideFileSuggestions();
    return;
  }
  if ((e.key === "Tab" || e.key === "Enter") && fileSuggestions.length > 0 && !e.ctrlKey && !e.metaKey) {
    e.preventDefault();
    chooseFileSuggestion();
    return;
  }
  if (e.key === "ArrowDown" && (promptBox.selectionStart === promptBox.value.length || promptBox.value === "")) {
    if (fileSuggestions.length > 0) {
      e.preventDefault();
      fileSuggestionIndex = (fileSuggestionIndex + 1) % fileSuggestions.length;
      renderFileSuggestions();
      return;
    }
    if (navigatePromptHistory(1)) e.preventDefault();
  } else if (e.key === "ArrowUp" && (promptBox.selectionStart === 0 || promptBox.value === "")) {
    if (fileSuggestions.length > 0) {
      e.preventDefault();
      fileSuggestionIndex = (fileSuggestionIndex - 1 + fileSuggestions.length) % fileSuggestions.length;
      renderFileSuggestions();
      return;
    }
    if (navigatePromptHistory(-1)) e.preventDefault();
  } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    send();
  }
});

window.addEventListener("keydown", (e) => {
  const modifier = e.ctrlKey || e.metaKey;
  if (!modifier || e.altKey) return;
  if (e.key.toLowerCase() === "k") {
    e.preventDefault();
    promptBox.focus();
    return;
  }
  if (!e.shiftKey) return;
  if (e.key.toLowerCase() === "c") {
    e.preventDefault();
    $("stop").click();
  } else if (e.key.toLowerCase() === "n") {
    e.preventDefault();
    $("new-chat").click();
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

// ---------- 队列输入 ----------
function renderPendingInputs(items) {
  const list = $("queue-list");
  const count = $("queue-count");
  list.innerHTML = "";
  count.textContent = items.length ? `(${items.length})` : "";
  if (!items.length) {
    const empty = document.createElement("div");
    empty.className = "queue-empty";
    empty.textContent = "暂无排队输入";
    list.appendChild(empty);
    return;
  }
  for (const item of items) {
    const entry = document.createElement("div");
    entry.className = "queue-entry";
    entry.title = item.prompt;
    const prompt = document.createElement("div");
    prompt.className = "queue-prompt";
    prompt.textContent = item.prompt;
    const delivery = document.createElement("span");
    delivery.className = "queue-delivery";
    delivery.textContent = item.delivery === "steer" ? "steer" : "queue";
    const cancel = document.createElement("button");
    cancel.className = "queue-cancel";
    cancel.textContent = "撤销";
    cancel.title = "撤销这条排队输入";
    cancel.addEventListener("click", async () => {
      cancel.disabled = true;
      try {
        const changed = await invoke("cancel_input", {
          projectDir: currentProject,
          inputId: item.input_id,
        });
        if (changed) {
          toast("已撤销排队输入");
          await refreshPendingInputs();
        }
      } catch (err) {
        cancel.disabled = false;
        toast(`撤销失败:${err}`);
      }
    });
    entry.append(prompt, delivery, cancel);
    list.appendChild(entry);
  }
}

async function refreshPendingInputs() {
  if (!currentProject) {
    renderPendingInputs([]);
    return;
  }
  try {
    renderPendingInputs(await invoke("list_pending_inputs", { projectDir: currentProject }));
  } catch (err) {
    log(`队列刷新失败:${err}`, "warn");
  }
}

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
    name.textContent = prefs.names?.[path] || baseName(path);
    const pathEl = document.createElement("span");
    pathEl.className = "path";
    pathEl.textContent = path;
    const remove = document.createElement("button");
    remove.className = "icon-btn remove";
    remove.textContent = "×";
    remove.title = "移除(不删除文件)";
    remove.addEventListener("click", async (e) => {
      e.stopPropagation();
      if (!window.confirm(`移除项目“${name.textContent}”吗？只解除登记,不会删除磁盘文件。`)) return;
      try {
        const wasCurrent = currentProject === path;
        const next = await invoke("projects_remove", { path });
        renderProjects(next);
        if (wasCurrent && currentProject !== path) {
          clearChat();
          bgClear();
          renderTodoPanel([], 0, 0);
          await loadConversation();
          await refreshDocs();
          await loadModels();
          refreshGit();
          await refreshPendingInputs();
        }
      } catch (err) {
        toast(String(err));
      }
    });
    const rename = document.createElement("button");
    rename.className = "icon-btn rename";
    rename.textContent = "✎";
    rename.title = "重命名项目(只修改显示名)";
    rename.addEventListener("click", async (e) => {
      e.stopPropagation();
      const nextName = window.prompt("项目显示名", prefs.names?.[path] || baseName(path));
      if (nextName === null || !nextName.trim()) return;
      try {
        renderProjects(await invoke("projects_rename", { path, name: nextName.trim() }));
      } catch (err) {
        toast(String(err));
      }
    });
    item.append(name, pathEl, rename, remove);
    item.addEventListener("click", async () => {
      const previous = currentProject;
      renderProjects(await invoke("projects_select", { path }));
      if (previous && previous !== path) {
        clearChat();
        bgClear();
        renderTodoPanel([], 0, 0);
        await loadConversation();
      }
      refreshDocs();
      loadModels();
      refreshGit();
      refreshPendingInputs();
    });
    list.appendChild(item);
  }
  $("project-label").textContent = prefs.current ?? "(未选择项目)";
}

$("project-init").addEventListener("click", async () => {
  const path = window.prompt("新项目目录路径(不存在时会创建)");
  if (path === null || !path.trim()) return;
  const name = window.prompt("项目显示名(可留空)", baseName(path.trim()));
  if (name === null) return;
  try {
    const prefs = await invoke("projects_init", {
      path: path.trim(),
      name: name.trim() || null,
    });
    renderProjects(prefs);
    clearChat("已初始化并切换到新项目");
    await loadConversation();
    await refreshDocs();
    await loadModels();
    refreshGit();
    await refreshPendingInputs();
    toast("项目初始化完成");
  } catch (err) {
    toast(String(err));
  }
});

$("project-add").addEventListener("click", async () => {
  try {
    const prefs = await invoke("projects_pick");
    if (prefs) {
      const previous = currentProject;
      renderProjects(prefs);
      if (previous !== currentProject) {
        clearChat();
        bgClear();
        renderTodoPanel([], 0, 0);
        await loadConversation();
        await refreshDocs();
        await loadModels();
        refreshGit();
        await refreshPendingInputs();
      } else {
        await refreshDocs();
      }
    }
  } catch (err) {
    toast(String(err));
  }
});

// ---------- 侧边栏文档(可展开 + 状态流转) ----------
const reqFilters = { status: "all", priority: "all", complexity: "all", sort: localStorage.getItem("kz-req-sort") || "manual" };
const priorityRank = { P0: 0, P1: 1, P2: 2, P3: 3 };
const statusRank = { doing: 0, todo: 1, done: 2, dropped: 3 };
const complexityRank = { "小": 0, "中": 1, "大": 2 };

function filterRequirements(entries) {
  const filtered = entries
    .filter((entry) => reqFilters.status === "all" || entry.status === reqFilters.status)
    .filter((entry) => reqFilters.priority === "all" || entry.priority === reqFilters.priority);
  const complexityValue = (entry) => entry.complexity || "unassessed";
  const complexityFiltered = filtered.filter((entry) => reqFilters.complexity === "all" || complexityValue(entry) === reqFilters.complexity);
  // 手动模式(R-054 默认):文件顺序即开发顺序,不做任何排序。
  if (reqFilters.sort === "manual") return complexityFiltered;
  return complexityFiltered.sort((a, b) => {
    if (reqFilters.sort === "id") return a.id.localeCompare(b.id, undefined, { numeric: true });
    if (reqFilters.sort === "complexity") return (complexityRank[complexityValue(a)] ?? 99) - (complexityRank[complexityValue(b)] ?? 99) || a.id.localeCompare(b.id, undefined, { numeric: true });
    if (reqFilters.sort === "status") {
      return (statusRank[a.status] ?? 99) - (statusRank[b.status] ?? 99) || a.id.localeCompare(b.id, undefined, { numeric: true });
    }
    return (priorityRank[a.priority] ?? 99) - (priorityRank[b.priority] ?? 99)
      || (statusRank[a.status] ?? 99) - (statusRank[b.status] ?? 99)
      || a.id.localeCompare(b.id, undefined, { numeric: true });
  });
}

// R-054:拖拽重排(手动模式限定)。拖完提交完整 ID 序——注意 order 必须覆盖
// 全部条目,所以在有筛选时禁止拖拽(顺序不完整会被引擎拒绝)。
let dragReqId = null;
function reqDragEnabled() {
  return reqFilters.sort === "manual" && reqFilters.status === "all" && reqFilters.priority === "all" && reqFilters.complexity === "all";
}
async function commitReqOrder(listEl) {
  const order = [...listEl.querySelectorAll(".doc-item[data-req-id]")].map((el) => el.dataset.reqId);
  try {
    const msg = await invoke("docs_update", {
      projectDir: currentProject,
      kind: "req",
      action: "reorder",
      id: "",
      order,
    });
    log(msg);
    refreshDocs();
  } catch (err) {
    toast(`排序保存失败:${err}`);
    refreshDocs();
  }
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
  let position = 0;
  for (const entry of entries) {
    position += 1;
    const item = document.createElement("div");
    // 优先级着色(pri-P0 红 / P1 黄 / P2 蓝 / P3 灰):扫一眼就知道轻重。
    const pri = (entry.priority || "").toUpperCase();
    item.className = `doc-item${entry.closed ? " closed" : ""}${/^P[0-3]$/.test(pri) ? ` pri-${pri}` : ""}`;
    item.dataset.docId = entry.id;

    const row = document.createElement("div");
    row.className = "doc-row";
    row.title = `${entry.id} ${entry.title}(点击展开)`;
    const id = document.createElement("span");
    id.className = "id";
    // R-054(用户拍板):需求行内不显示 R-xxx(乱序观感),只显示位置序号;身份收进展开详情。
    if (kind === "req") {
      id.className = "pos";
      id.textContent = `#${position}`;
    } else {
      id.textContent = entry.id;
    }
    const st = document.createElement("span");
    st.className = `st st-${entry.status || "todo"}`;
    st.textContent = entry.status + (entry.severity ? `/${entry.severity}` : "");
    row.append(id, st);
    // 拖拽重排(仅需求 + 手动模式 + 无筛选)。提交放在 dragend:
    // 松手落在行间隙时 drop 不触发,只靠 drop 会静默丢单。
    if (kind === "req" && reqDragEnabled()) {
      item.dataset.reqId = entry.id;
      item.draggable = true;
      item.addEventListener("dragstart", (e) => {
        dragReqId = entry.id;
        item.classList.add("dragging");
        el.dataset.orderBefore = [...el.querySelectorAll(".doc-item[data-req-id]")]
          .map((n) => n.dataset.reqId)
          .join(",");
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", entry.id);
      });
      item.addEventListener("dragend", () => {
        item.classList.remove("dragging");
        dragReqId = null;
        const now = [...el.querySelectorAll(".doc-item[data-req-id]")]
          .map((n) => n.dataset.reqId)
          .join(",");
        if (now !== el.dataset.orderBefore) commitReqOrder(el);
      });
      item.addEventListener("dragover", (e) => {
        e.preventDefault();
        const dragging = el.querySelector(".doc-item.dragging");
        if (!dragging || dragging === item) return;
        const rect = item.getBoundingClientRect();
        const before = e.clientY < rect.top + rect.height / 2;
        el.insertBefore(dragging, before ? item : item.nextSibling);
      });
    }
    if (kind === "req") {
      const badge = document.createElement("button");
      badge.className = `pri-badge ${/^P[0-3]$/.test(pri) ? pri : "unset"}`;
      badge.textContent = /^P[0-3]$/.test(pri) ? pri : "P?";
      badge.title = "点击循环调整优先级";
      badge.addEventListener("click", async (event) => {
        event.stopPropagation();
        const order = ["P0", "P1", "P2", "P3"];
        const next = order[(order.indexOf(pri) + 1) % order.length];
        try {
          await invoke("docs_update", { projectDir: currentProject, kind: "req", action: "update", id: entry.id, priority: next });
          toast(`${entry.id} 优先级已调整为 ${next}`);
          refreshDocs();
        } catch (error) {
          toast(`优先级保存失败:${error}`);
        }
      });
      row.appendChild(badge);
    }    // 复杂度(R-051):不用文字,行底部宽度条表达体量(小短/中中/大长);未评估无条。
    const cx = (entry.complexity || "").trim();
    if (["小", "中", "大"].includes(cx)) {
      item.classList.add(`cx-${cx === "小" ? "s" : cx === "中" ? "m" : "l"}`);
      item.title = `复杂度:${cx}`;
    }
    const complexityBadge = document.createElement("span");
    complexityBadge.className = "complexity-badge";
    complexityBadge.textContent = cx === "小" || cx === "中" || cx === "大" ? `复杂度:${cx}` : "未评估";
    row.appendChild(complexityBadge);
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = entry.title;
    row.appendChild(title);
    item.appendChild(row);

    // 展开面板:完整标题、字段、合法的状态流转按钮(与硬门禁同一套规则)。
    const detail = document.createElement("div");
    detail.className = "doc-detail hidden";
    const full = document.createElement("div");
    full.className = "doc-full-title";
    // 行内不显示需求 ID(R-054),身份收进展开详情——这里必须给全。
    full.textContent = kind === "req" ? `${entry.id} · ${entry.title}` : entry.title;
    detail.appendChild(full);
    for (const [key, value] of entry.fields ?? []) {
      const f = document.createElement("div");
      f.className = "doc-field";
      if (key.toLowerCase() === "refs") {
        f.append(`${key}: `);
        for (const ref of String(value).split(/[\s,]+/).filter(Boolean)) {
          const link = document.createElement("button");
          link.className = "ref-link";
          link.textContent = ref;
          link.addEventListener("click", (event) => {
            event.stopPropagation();
            const target = document.querySelector(`[data-doc-id="${ref}"]`);
            target?.scrollIntoView({ behavior: "smooth", block: "center" });
            target?.classList.add("ref-highlight");
            setTimeout(() => target?.classList.remove("ref-highlight"), 1200);
          });
          f.appendChild(link);
          f.append(" ");
        }
      } else {
        f.textContent = `${key}: ${value}`;
      }
      detail.appendChild(f);
    }
    if (kind === "req" && !entry.closed) {
      const complexityRow = document.createElement("div");
      complexityRow.className = "doc-progress";
      const complexitySelect = document.createElement("select");
      complexitySelect.innerHTML = "<option value=\"\">未评估</option><option>小</option><option>中</option><option>大</option>";
      complexitySelect.value = cx;
      complexitySelect.title = "设置需求复杂度";
      complexitySelect.addEventListener("click", (event) => event.stopPropagation());
      complexitySelect.addEventListener("change", async () => {
        try {
          await invoke("docs_update", { projectDir: currentProject, kind: "req", action: "update", id: entry.id, fields: { "复杂度": complexitySelect.value } });
          toast("复杂度已保存");
          refreshDocs();
        } catch (error) {
          toast(`复杂度保存失败:${error}`);
        }
      });
      complexityRow.append("复杂度: ", complexitySelect);
      detail.appendChild(complexityRow);
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

function formatWorkspaceTime(value) {
  if (!value) return "暂无时间";
  return new Date(Number(value)).toLocaleString();
}

async function selectWorkspaceProject(path) {
  try {
    const previous = currentProject;
    renderProjects(await invoke("projects_select", { path }));
    if (previous !== path) {
      clearChat();
      bgClear();
      renderTodoPanel([], 0, 0);
      await loadConversation();
      await refreshDocs();
      await loadModels();
      refreshGit();
      await refreshPendingInputs();
    }
    refreshWorkspace();
  } catch (error) {
    toast(`切换项目失败:${error}`);
  }
}

function renderWorkspace(snapshot) {
  const root = $("workspace-projects");
  root.replaceChildren();
  for (const project of snapshot.projects ?? []) {
    const card = document.createElement("section");
    card.className = `workspace-card${project.current ? " current" : ""}`;
    const head = document.createElement("div");
    head.className = "workspace-card-head";
    const title = document.createElement("strong");
    title.textContent = project.name;
    const status = document.createElement("span");
    status.className = `workspace-status ${project.status}`;
    status.textContent = project.status === "running" ? "运行中" : project.status === "failed" ? "失败" : "空闲";
    head.append(title, status);
    const path = document.createElement("div");
    path.className = "dim workspace-path";
    path.textContent = project.path;
    const conversation = project.conversation;
    const summary = document.createElement("div");
    summary.className = "workspace-summary";
    summary.textContent = conversation
      ? `当前对话: ${conversation.title} · ${conversation.message_count} 条`
      : "当前对话: 暂无";
    const activity = document.createElement("div");
    activity.className = "workspace-activity dim";
    const trace = (project.recent_activity ?? []).flatMap((item) => item.events ?? []);
    activity.textContent = trace.length
      ? `最近活动: ${trace.slice(0, 3).map((item) => item.text || item.name || "运行事件").join(" · ")}`
      : "最近活动: 暂无";
    const queue = document.createElement("div");
    queue.className = "workspace-meta dim";
    queue.textContent = `排队 ${project.pending_count ?? 0} 条 · 更新于 ${formatWorkspaceTime(project.updated_at)}`;
    card.append(head, path, summary, activity, queue);
    card.addEventListener("click", () => selectWorkspaceProject(project.path));
    root.appendChild(card);
  }
}

async function refreshWorkspace() {
  try {
    renderWorkspace(await invoke("workspace_snapshot"));
  } catch (error) {
    toast(`工作区刷新失败:${error}`);
  }
}
let documentsKind = "req";
let latestDocsSnapshot = null;
function renderDocuments(snapshot) {
  latestDocsSnapshot = snapshot;
  const reqList = $("documents-req-list");
  const defectList = $("documents-defect-list");
  if (!reqList || !defectList) return;
  const saved = { ...reqFilters };
  reqFilters.status = "all";
  reqFilters.priority = "all";
  reqFilters.complexity = "all";
  reqFilters.sort = "manual";
  renderDocList(reqList, snapshot.requirements ?? [], "req", snapshot.archived?.req ?? 0);
  Object.assign(reqFilters, saved);
  const status = $("documents-status-filter").value;
  const priority = $("documents-priority-filter").value;
  const defects = (snapshot.defects ?? []).filter((entry) =>
    (status === "all" || entry.status === status) && (priority === "all" || entry.priority === priority));
  renderDocList(defectList, defects, "defect", snapshot.archived?.defect ?? 0);
  reqList.classList.toggle("hidden", documentsKind !== "req");
  defectList.classList.toggle("hidden", documentsKind !== "defect");
  $("documents-tab-req").className = documentsKind === "req" ? "primary" : "ghost";
  $("documents-tab-defect").className = documentsKind === "defect" ? "primary" : "ghost";
}
async function refreshDocs() {
  if (!currentProject) return;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    renderDocList($("req-list"), snapshot.requirements, "req", snapshot.archived?.req ?? 0);
    renderDocList($("defect-list"), snapshot.defects, "defect", snapshot.archived?.defect ?? 0);
    renderDocList($("goal-list"), snapshot.goals ?? [], "goal", snapshot.archived?.goal ?? 0);
    renderDocuments(snapshot);
    renderDocList($("source-list"), snapshot.sources ?? [], "source", snapshot.archived?.source ?? 0);
    renderDocList($("finding-list"), snapshot.findings ?? [], "finding", snapshot.archived?.finding ?? 0);
    $("research-count").textContent = `${(snapshot.sources ?? []).length + (snapshot.findings ?? []).length}`;
    $("req-count").textContent = `${snapshot.requirements.filter((r) => !r.closed).length}`;
    $("defect-count").textContent = `${snapshot.defects.filter((d) => !d.closed).length}`;
    $("goal-count").textContent = `${(snapshot.goals ?? []).filter((g) => g.status === "active").length}`;
    renderConventions(snapshot.conventions);
    await refreshConversationList();
  } catch (err) {
    console.error(err);
  }
}

$("documents-tab-req").addEventListener("click", () => { documentsKind = "req"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
$("documents-tab-defect").addEventListener("click", () => { documentsKind = "defect"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
for (const id of ["documents-status-filter", "documents-priority-filter"]) $(id).addEventListener("change", () => { if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
for (const [id, key] of [["req-status-filter", "status"], ["req-priority-filter", "priority"], ["req-complexity-filter", "complexity"], ["req-sort", "sort"]]) {
  $(id).addEventListener("change", (event) => {
    reqFilters[key] = event.target.value;
    if (key === "sort") localStorage.setItem("kz-req-sort", event.target.value);
    refreshDocs();
  });
}
$("req-complexity-filter").value = reqFilters.complexity;
$("req-sort").value = reqFilters.sort;

// ---------- R-053 快速记录:独立子代理结构化落库(需求/缺陷通用),不打断主对话 ----------
function quickCaptureForm(kind, listId, noun) {
  const list = $(listId);
  if (list.querySelector(".quickreq-form")) return;
  const form = document.createElement("div");
  form.className = "goal-add-form quickreq-form";
  const input = document.createElement("textarea");
  input.rows = 3;
  input.placeholder = `自然语言描述${noun};Ctrl+Enter 或点提交,Esc 取消。子代理后台结构化落库,不打断当前对话。`;
  const submit = async () => {
    const text = input.value.trim();
    if (!text) {
      toast("先写点描述");
      return;
    }
    form.remove();
    toast(`记${noun}中…(独立子代理后台进行)`);
    try {
      const msg = await invoke("quick_req", { projectDir: currentProject, description: text, kind });
      toast(`已记录:${msg}`);
      refreshDocs();
    } catch (err) {
      toast(`记${noun}失败:${err}`);
    }
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") form.remove();
    else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) submit();
  });
  const bar = document.createElement("div");
  bar.className = "quickreq-bar";
  const cancelBtn = document.createElement("button");
  cancelBtn.className = "ghost mini";
  cancelBtn.textContent = "取消";
  cancelBtn.addEventListener("click", () => form.remove());
  const submitBtn = document.createElement("button");
  submitBtn.className = "primary mini";
  submitBtn.textContent = "提交";
  submitBtn.addEventListener("click", submit);
  bar.append(cancelBtn, submitBtn);
  form.append(input, bar);
  list.prepend(form);
  input.focus();
}
$("req-quick").addEventListener("click", () => quickCaptureForm("req", "req-list", "需求"));
$("defect-quick").addEventListener("click", () => quickCaptureForm("defect", "defect-list", "缺陷"));

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
  // 规范不再铺开章节列表占满侧边栏:一行入口,点开进应用内 MD 查看器。
  const item = document.createElement("div");
  item.className = "doc-item";
  item.style.cursor = "pointer";
  item.textContent = `${conv.headings.length} 个章节 · 点击查看`;
  item.title = conv.headings.slice(0, 12).join("\n");
  item.addEventListener("click", () => openDocViewer("conventions"));
  el.appendChild(item);
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
    bgClear();
    renderTodoPanel([], 0, 0);
    const history = await invoke("conversation_get", { projectDir: currentProject, sequence });
    renderRecoveredMessages(history);
    const traces = await invoke("conversation_trace_get", { projectDir: currentProject, sequence });
    renderRecoveredTraces(traces);
    log(`已恢复 ${history.length} 条历史消息和 ${traces.length} 组工具轨迹`);
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
    addSummaryEntry(r.summary, r.path);
    toast("小总结已收纳到活动面板");
    log(`总结完成,已收纳并存档:${r.path}`);
  } catch (err) {
    toast(`总结失败:${err}`);
    log(`总结失败:${err}`, "err");
  } finally {
    $("summarize-btn").disabled = false;
    setStatus(running ? "运行中" : "空闲", running);
  }
});

for (const [btn, kind] of [["req-open", "req"], ["defect-open", "defect"], ["goal-open", "goal"], ["report-open", "report"]]) {
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
      const envInput = document.createElement("input");
      envInput.value = p.apiKeyEnv ?? "";
      envInput.placeholder = "环境变量名(可选)";
      envInput.title = "读取该环境变量作为 key";
      envInput.addEventListener("input", () => (p.apiKeyEnv = envInput.value));
      const keyInput = document.createElement("input");
      keyInput.type = "password";
      keyInput.value = p.apiKey ?? "";
      keyInput.placeholder = "或直接粘贴 key";
      keyInput.title = "直填优先于环境变量;明文存 kanzei.toml";
      keyInput.addEventListener("input", () => (p.apiKey = keyInput.value));
      tdKey.append(envInput, keyInput);
      if (p.keyPresent !== null && p.keyPresent !== undefined) {
        const state = document.createElement("span");
        state.className = `key-state ${p.keyPresent ? "key-ok" : "key-missing"}`;
        state.textContent = p.keyPresent ? "已设" : "缺失";
        tdKey.appendChild(state);
      }
    }
    // 当场探测:401/超时都给可操作提示,不用跑一轮对话才发现 key 坏了。
    {
      const testBtn = document.createElement("button");
      testBtn.className = "ghost mini";
      testBtn.textContent = "测试";
      const result = document.createElement("div");
      result.className = "key-test-result";
      testBtn.addEventListener("click", async () => {
        result.textContent = "测试中…";
        try {
          result.textContent = await invoke("provider_test", {
            protocol: p.protocol,
            baseUrl: p.baseUrl,
            apiKeyEnv: p.apiKeyEnv || null,
            apiKey: p.apiKey || null,
            auth: p.auth || null,
          });
        } catch (err) {
          result.textContent = `测试失败:${err}`;
        }
      });
      tdKey.append(testBtn, result);
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

function renderPermissionRules(data) {
  const tbody = $("permission-rules-table").querySelector("tbody");
  tbody.replaceChildren();
  const rules = data?.rules ?? [];
  $("permission-rules-empty").classList.toggle("hidden", rules.length > 0);
  $("permission-rules-path").textContent = data?.path ? `配置: ${data.path}` : "";
  for (const rule of rules) {
    const row = document.createElement("tr");
    const action = document.createElement("td");
    action.textContent = rule.action;
    const resource = document.createElement("td");
    resource.textContent = rule.resource;
    const controls = document.createElement("td");
    const remove = document.createElement("button");
    remove.className = "icon-btn";
    remove.title = "删除规则";
    remove.textContent = "×";
    remove.addEventListener("click", async () => {
      if (!confirm(`删除 ${rule.action} / ${rule.resource}？`)) return;
      try {
        await invoke("permission_rule_delete", { projectDir: currentProject, index: rule.index });
        toast("已删除权限规则");
        loadPermissionRules();
      } catch (err) {
        toast(`删除失败: ${err}`);
      }
    });
    controls.appendChild(remove);
    row.append(action, resource, controls);
    tbody.appendChild(row);
  }
}

async function loadPermissionRules() {
  if (!currentProject) {
    renderPermissionRules({ rules: [] });
    return;
  }
  try {
    renderPermissionRules(await invoke("permission_rules_get", { projectDir: currentProject }));
  } catch (err) {
    renderPermissionRules({ rules: [] });
    toast(`读取权限规则失败: ${err}`);
  }
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
  loadPermissionRules();
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
          apiKey: p.apiKey || null,
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

// ---------- 版本与更新(GitHub Releases 为源) ----------
let updateUrl = null;
$("update-check").addEventListener("click", async () => {
  $("update-result").textContent = "检查中…";
  $("update-install").classList.add("hidden");
  updateUrl = null;
  try {
    const r = await invoke("update_check");
    if (r.current) $("update-current").textContent = r.current;
    if (r.status === "none") {
      $("update-result").textContent = r.message;
    } else if (r.newer && r.url) {
      updateUrl = r.url;
      $("update-result").textContent = `发现新版本:${r.latest}`;
      $("update-install").classList.remove("hidden");
    } else {
      $("update-result").textContent = `已是最新(${r.latest || r.current})`;
    }
  } catch (err) {
    $("update-result").textContent = `检查失败:${err}`;
  }
});
$("update-install").addEventListener("click", async () => {
  if (!updateUrl) return;
  $("update-result").textContent = "下载中…(安装器就绪后会自动弹出)";
  $("update-install").disabled = true;
  try {
    $("update-result").textContent = await invoke("update_install", { url: updateUrl });
  } catch (err) {
    $("update-result").textContent = String(err);
  } finally {
    $("update-install").disabled = false;
  }
});

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
    $("update-current").textContent = String(info.build).split(" ")[0];
    log(`kanzei 桌面端启动 · v${info.version} (${info.build})`);
  } catch (err) {
    log(`获取版本失败:${err}`, "warn");
  }
  // 启动静默检查更新(安装版通道):有新包只弹一条 toast,不打断;失败不打扰。
  setTimeout(async () => {
    try {
      const r = await invoke("update_check");
      if (r.newer && r.url) toast(`发现新版本 ${r.latest} — 设置页「检查更新」可一键安装`);
    } catch {}
  }, 3000);
  renderProjects(await invoke("projects_get"));
  await loadConversation();
  await refreshDocs();
  await loadModels();
  refreshGit();
  await refreshPendingInputs();
  setStatus("空闲", false);
})();
