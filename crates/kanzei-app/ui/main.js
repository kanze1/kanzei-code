// kanzei 桌面端前端逻辑(静态,无构建步骤)。
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const messages = $("messages");
const promptBox = $("prompt");
const sendBtn = $("send");
const statusEl = $("status");

let running = false;
let currentAssistant = null; // 正在流式追加的 assistant 消息节点
let currentReasoning = null;
let currentTool = null;

// ---------- 消息渲染 ----------
function scrollBottom() {
  messages.scrollTop = messages.scrollHeight;
}

function addMessage(cls, text) {
  const el = document.createElement("div");
  el.className = `msg ${cls}`;
  el.textContent = text;
  messages.appendChild(el);
  scrollBottom();
  return el;
}

function appendAssistant(text) {
  if (!currentAssistant) {
    currentAssistant = addMessage("assistant", "");
  }
  currentAssistant.textContent += text;
  scrollBottom();
}

function appendReasoning(text) {
  if (!currentReasoning) {
    currentReasoning = addMessage("reasoning", "");
  }
  currentReasoning.textContent += text;
  scrollBottom();
}

function toolStart(name, summary) {
  currentAssistant = null; // 文本块被工具调用切断,下一段文本另起节点
  currentReasoning = null;
  const chip = document.createElement("div");
  chip.className = "tool-chip running";
  const head = document.createElement("div");
  head.className = "head";
  head.textContent = `${name} ${summary}`;
  chip.appendChild(head);
  messages.appendChild(chip);
  currentTool = chip;
  scrollBottom();
}

function toolEnd(ok, preview) {
  if (!currentTool) return;
  currentTool.classList.remove("running");
  currentTool.classList.add(ok ? "ok" : "err");
  const result = document.createElement("div");
  result.className = "result";
  result.textContent = preview;
  currentTool.appendChild(result);
  currentTool = null;
  scrollBottom();
}

function setRunning(value) {
  running = value;
  sendBtn.disabled = value;
  statusEl.textContent = value ? "运行中…" : "";
}

// ---------- 事件订阅 ----------
listen("kz:meta", (e) => {
  $("model-badge").textContent = e.payload.model;
  $("profile-badge").textContent = e.payload.profile;
});
listen("kz:text", (e) => appendAssistant(e.payload.text));
listen("kz:reasoning", (e) => appendReasoning(e.payload.text));
listen("kz:tool-start", (e) => toolStart(e.payload.name, e.payload.summary));
listen("kz:tool-end", (e) => toolEnd(e.payload.ok, e.payload.preview));
listen("kz:error", (e) => {
  addMessage("error", e.payload.message);
  setRunning(false);
});
listen("kz:done", (e) => {
  const p = e.payload;
  const note = `steps ${p.steps} · in ${p.input} (cache r${p.cacheRead} w${p.cacheWrite}) · out ${p.output}${p.halted ? " · 已按你的拒绝停止" : ""}`;
  addMessage("notice", note);
  setRunning(false);
  refreshDocs();
});

// ---------- 权限弹窗 ----------
const askQueue = [];
let askActive = null;

listen("kz:ask", (e) => {
  askQueue.push(e.payload);
  pumpAsk();
});

function pumpAsk() {
  if (askActive || askQueue.length === 0) return;
  askActive = askQueue.shift();
  $("ask-action").textContent = askActive.action;
  $("ask-resource").textContent = askActive.resource;
  $("ask-overlay").classList.remove("hidden");
}

async function answerAsk(allow) {
  if (!askActive) return;
  const id = askActive.id;
  askActive = null;
  $("ask-overlay").classList.add("hidden");
  await invoke("answer_ask", { id, allow });
  pumpAsk();
}

$("ask-allow").addEventListener("click", () => answerAsk(true));
$("ask-deny").addEventListener("click", () => answerAsk(false));

// ---------- 发送 ----------
async function send() {
  const prompt = promptBox.value.trim();
  if (!prompt || running) return;
  promptBox.value = "";
  currentAssistant = null;
  currentReasoning = null;
  addMessage("user", prompt);
  setRunning(true);
  try {
    await invoke("run_prompt", {
      prompt,
      projectDir: $("project-dir").value.trim(),
      profile: null,
    });
  } catch (err) {
    addMessage("error", String(err));
    setRunning(false);
  }
}

sendBtn.addEventListener("click", send);
promptBox.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    send();
  }
});

// ---------- 侧边栏文档 ----------
function statusClass(status) {
  return `st st-${status || "todo"}`;
}

function renderDocList(el, entries) {
  el.innerHTML = "";
  for (const entry of entries) {
    const item = document.createElement("div");
    item.className = `doc-item${entry.closed ? " closed" : ""}`;
    item.title = `${entry.id} ${entry.title}`;
    const id = document.createElement("span");
    id.className = "id";
    id.textContent = entry.id;
    const st = document.createElement("span");
    st.className = statusClass(entry.status);
    st.textContent = entry.status + (entry.severity ? `/${entry.severity}` : "");
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = entry.title;
    item.append(id, st, title);
    el.appendChild(item);
  }
}

async function refreshDocs() {
  const dir = $("project-dir").value.trim();
  if (!dir) return;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: dir });
    renderDocList($("req-list"), snapshot.requirements);
    renderDocList($("defect-list"), snapshot.defects);
    $("req-count").textContent = `(${snapshot.requirements.filter((r) => !r.closed).length})`;
    $("defect-count").textContent = `(${snapshot.defects.filter((d) => !d.closed).length})`;
    const parts = snapshot.root.replaceAll("\\", "/").split("/");
    $("project-name").textContent = parts[parts.length - 1] || snapshot.root;
  } catch (err) {
    console.error(err);
  }
}

$("project-dir").addEventListener("change", refreshDocs);

// ---------- 启动 ----------
(async () => {
  $("project-dir").value = await invoke("default_project_dir");
  await refreshDocs();
  addMessage("notice", "kanzei 就绪 — 输入任务开始(权限请求会弹窗询问)");
})();
