// kanzei 桌面端前端逻辑(静态,无构建步骤)。
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const messages = $("messages");
const promptBox = $("prompt");

let running = false;
let currentProject = null;
let currentAssistant = null;
let currentReasoning = null;
let currentTool = null;
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

function renderTokens() {
  const t = runTokens;
  if (t.input + t.output === 0) {
    $("status-tokens").textContent = "";
    return;
  }
  $("status-tokens").textContent =
    `in ${t.input} (cache r${t.cacheRead} w${t.cacheWrite}) · out ${t.output}`;
}

function setRunning(value, statusText) {
  running = value;
  $("send").disabled = value;
  $("stop").classList.toggle("hidden", !value);
  setStatus(statusText ?? (value ? "运行中" : "空闲"), value);
}

// ---------- 消息渲染 ----------
function clearEmptyState() {
  const empty = $("empty-state");
  if (empty) empty.remove();
}

function scrollBottom() {
  messages.scrollTop = messages.scrollHeight;
}

function addMessage(cls, text) {
  clearEmptyState();
  const el = document.createElement("div");
  el.className = `msg ${cls}`;
  el.textContent = text;
  messages.appendChild(el);
  scrollBottom();
  return el;
}

function appendAssistant(text) {
  if (!currentAssistant) currentAssistant = addMessage("assistant", "");
  currentAssistant.textContent += text;
  scrollBottom();
}

function appendReasoning(text) {
  if (!currentReasoning) currentReasoning = addMessage("reasoning", "");
  currentReasoning.textContent += text;
  scrollBottom();
}

// ---------- 事件订阅 ----------
listen("kz:status", (e) => {
  const p = e.payload;
  log(`[${p.stage}] ${p.detail}`);
  if (running) setStatus(`${p.stage} · ${p.detail}`, true);
});
listen("kz:meta", (e) => {
  $("status-model").textContent = `${e.payload.model} · ${e.payload.profile}`;
  log(`模型 ${e.payload.model} · agent ${e.payload.agent} · profile ${e.payload.profile}`);
  if (running) setStatus("等待模型响应", true);
});
listen("kz:text", (e) => {
  markFirstSignal();
  if (running) setStatus("生成中", true);
  appendAssistant(e.payload.text);
});
listen("kz:reasoning", (e) => {
  markFirstSignal();
  if (running) setStatus("思考中", true);
  appendReasoning(e.payload.text);
});
listen("kz:tool-start", (e) => {
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
  messages.appendChild(chip);
  currentTool = chip;
  setStatus(`工具执行中 · ${e.payload.name}`, true);
  scrollBottom();
});
listen("kz:tool-end", (e) => {
  log(`工具结果 ${e.payload.name}: ${e.payload.ok ? "成功" : "失败"} — ${e.payload.preview}`, e.payload.ok ? "" : "warn");
  if (currentTool) {
    currentTool.classList.remove("running");
    currentTool.classList.add(e.payload.ok ? "ok" : "err");
    const result = document.createElement("div");
    result.className = "result";
    result.textContent = e.payload.preview;
    currentTool.appendChild(result);
    currentTool = null;
  }
  setStatus("运行中", true);
  scrollBottom();
});
listen("kz:step", (e) => {
  const p = e.payload;
  runTokens.input += p.input;
  runTokens.output += p.output;
  runTokens.cacheRead += p.cacheRead;
  runTokens.cacheWrite += p.cacheWrite;
  renderTokens();
  log(`一轮完成:in ${p.input} (cache r${p.cacheRead}) · out ${p.output}`);
});
listen("kz:error", (e) => {
  addMessage("error", e.payload.message);
  log(`错误:${e.payload.message}`, "err");
  stopElapsed();
  setRunning(false, "出错");
  $("log-panel").classList.remove("hidden");
});
listen("kz:stopped", () => {
  hideAsk();
  addMessage("notice", "已停止");
  log("已手动停止");
  stopElapsed();
  setRunning(false, "已停止");
});
listen("kz:done", (e) => {
  const p = e.payload;
  addMessage("notice", `完成 · steps ${p.steps}${p.halted ? " · 已按你的拒绝停止" : ""}`);
  log(`运行完成:${p.steps} 轮,耗时 ${((Date.now() - runStart) / 1000).toFixed(1)}s`);
  stopElapsed();
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

function hideAsk() {
  askQueue.length = 0;
  askActive = null;
  $("ask-overlay").classList.add("hidden");
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

// ---------- 发送 / 停止 ----------
async function send() {
  const prompt = promptBox.value.trim();
  // 任何拒绝发送的理由都要说出来,绝不静默(D-004)。
  if (!prompt) return;
  if (running) {
    toast("上一个任务还在运行——点「停止」或等它结束");
    return;
  }
  if (!currentProject) {
    toast("先在左侧「项目」里添加并选择一个目录");
    return;
  }
  promptBox.value = "";
  currentAssistant = null;
  currentReasoning = null;
  runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };
  renderTokens();
  addMessage("user", prompt);
  setRunning(true, "准备中");
  startElapsed();
  log(`发送:${prompt.slice(0, 80)}`);
  try {
    await invoke("run_prompt", {
      prompt,
      projectDir: currentProject,
      profile: $("profile-select").value,
    });
  } catch (err) {
    addMessage("error", String(err));
    log(`发送被拒:${err}`, "err");
    stopElapsed();
    setRunning(false);
  }
}

$("send").addEventListener("click", send);
$("stop").addEventListener("click", () => invoke("stop_run"));
promptBox.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    send();
  }
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
      renderProjects(await invoke("projects_select", { path }));
      refreshDocs();
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

// ---------- 侧边栏文档 ----------
function renderDocList(el, entries) {
  el.innerHTML = "";
  if (entries.length === 0) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = "(空)";
    el.appendChild(empty);
    return;
  }
  for (const entry of entries) {
    const item = document.createElement("div");
    item.className = `doc-item${entry.closed ? " closed" : ""}`;
    item.title = `${entry.id} ${entry.title}`;
    const id = document.createElement("span");
    id.className = "id";
    id.textContent = entry.id;
    const st = document.createElement("span");
    st.className = `st st-${entry.status || "todo"}`;
    st.textContent = entry.status + (entry.severity ? `/${entry.severity}` : "");
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = entry.title;
    item.append(id, st, title);
    el.appendChild(item);
  }
}

async function refreshDocs() {
  if (!currentProject) return;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    renderDocList($("req-list"), snapshot.requirements);
    renderDocList($("defect-list"), snapshot.defects);
    $("req-count").textContent = `${snapshot.requirements.filter((r) => !r.closed).length}`;
    $("defect-count").textContent = `${snapshot.defects.filter((d) => !d.closed).length}`;
  } catch (err) {
    console.error(err);
  }
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

    const tdRemove = document.createElement("td");
    const removeBtn = document.createElement("button");
    removeBtn.className = "icon-btn";
    removeBtn.textContent = "×";
    removeBtn.addEventListener("click", () => {
      settingsProviders.splice(index, 1);
      renderProviders();
    });
    tdRemove.appendChild(removeBtn);

    tr.append(tdName, tdProtocol, tdUrl, tdKey, tdRemove);
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
  await refreshDocs();
  setStatus("空闲", false);
})();
