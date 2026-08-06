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

let outputChars = 0;
function appendAssistant(text) {
  if (!currentAssistant) {
    currentAssistant = addMessage("assistant md", "");
    currentAssistant.dataset.raw = "";
  }
  currentAssistant.dataset.raw += text;
  currentAssistant.innerHTML = renderMarkdown(currentAssistant.dataset.raw);
  outputChars += text.length;
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
    head.addEventListener("click", () => body.classList.toggle("hidden"));
    wrap.append(head, body);
    messages.appendChild(wrap);
    currentReasoning = body;
    currentReasoningHead = head;
  }
  currentReasoning.dataset.raw += text;
  currentReasoning.innerHTML = renderMarkdown(currentReasoning.dataset.raw);
  if (currentReasoningHead) {
    const preview = currentReasoning.dataset.raw
      .split("\n")[0]
      .replace(/[#*`]/g, "")
      .trim()
      .slice(0, 60);
    currentReasoningHead.textContent = `· ${preview || "思考中…"}(点击展开)`;
  }
  scrollBottom();
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
    divider.textContent = `第 ${p.step}/${p.maxSteps} 轮`;
    messages.appendChild(divider);
    scrollBottom();
  }
  currentAssistant = null;
  currentReasoning = null;
  currentReasoningHead = null;
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
  messages.appendChild(chip);
  currentTool = chip;
  setStatus(`工具执行中 · ${e.payload.name}`, true);
  scrollBottom();
});
on("kz:tool-end", (e) => {
  const p = e.payload;
  log(`工具结果 ${p.name}: ${p.ok ? "成功" : "失败"} — ${p.preview}`, p.ok ? "" : "warn");
  if (currentTool) {
    const chip = currentTool;
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
    if (!p.ok) result.classList.remove("hidden");
    currentTool = null;
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
  refreshDocs();
  refreshGit();

  // 连跑:正常完成且上轮有实质动作(>1 轮 = 有工具调用)才续;拒绝/纯聊天即停。
  if ($("auto-continue").checked && !p.halted) {
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
      if ($("auto-continue").checked && !running) sendText(CONTINUE_PROMPT, { auto: true });
    }, 2000);
  }
});

// ---------- 权限弹窗 ----------
const askQueue = [];
let askActive = null;

on("kz:ask", (e) => {
  // 自动放行(yolo):不弹窗,直接允许并留日志。
  if ($("auto-allow").checked) {
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
  $("ask-action").textContent = askActive.action;
  $("ask-resource").textContent = askActive.resource;
  $("ask-remember").textContent = `${askActive.action} ${askActive.remember ?? askActive.resource}`;
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
  const summary = `${askActive.action}: ${askActive.resource}`;
  askActive = null;
  $("ask-overlay").classList.add("hidden");
  log(`权限 ${reply === "deny" ? "拒绝" : reply === "always" ? "总是允许" : "允许一次"} — ${summary}`);
  try {
    await invoke("answer_ask", { id, reply });
  } catch (err) {
    log(`权限应答失败:${err}`, "err");
  }
  pumpAsk();
}

$("ask-allow").addEventListener("click", () => answerAsk("once"));
$("ask-always").addEventListener("click", () => answerAsk("always"));
$("ask-deny").addEventListener("click", () => answerAsk("deny"));

// ---------- 发送 / 停止 ----------
// 连跑状态:自动续跑计数(手动发送归零),上限防失控。
const AUTO_CONTINUE_MAX = 10;
let autoRounds = 0;
const CONTINUE_PROMPT =
  "继续:检查活跃目标(goal list)与最新进展,推进下一个具体步骤并落地(改代码/跑测试/更新文档);" +
  "完成后用 goal update 记录进展。若工作区有已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
  "若所有活跃目标已达成或被阻塞,明确说明原因,不要做无意义的空转。";

async function sendText(prompt, { auto = false } = {}) {
  // 任何拒绝发送的理由都要说出来,绝不静默(D-004)。
  if (!prompt) return;
  if (running) {
    if (!auto) toast("上一个任务还在运行——点「停止」或等它结束");
    return;
  }
  if (!currentProject) {
    toast("先在左侧「项目」里添加并选择一个目录");
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
    await invoke("run_prompt", {
      prompt,
      projectDir: currentProject,
      profile: $("profile-select").value,
      model: $("model-select").value || null,
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
  if (!prompt) return;
  promptBox.value = "";
  sendText(prompt);
}

$("send").addEventListener("click", send);
$("continue-btn").addEventListener("click", () => sendText(CONTINUE_PROMPT));
$("auto-continue").checked = localStorage.getItem("kz-auto-continue") === "1";
$("auto-continue").addEventListener("change", () => {
  localStorage.setItem("kz-auto-continue", $("auto-continue").checked ? "1" : "0");
  autoRounds = 0;
  log($("auto-continue").checked ? "连跑已开启:每轮结束自动推进目标(上限 10 连)" : "连跑已关闭");
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
        clearChat("已切换项目,对话历史重新开始");
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
function renderDocList(el, entries, kind) {
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

    const row = document.createElement("div");
    row.className = "doc-row";
    row.title = `${entry.id} ${entry.title}(点击展开)`;
    const id = document.createElement("span");
    id.className = "id";
    id.textContent = entry.id;
    const st = document.createElement("span");
    st.className = `st st-${entry.status || "todo"}`;
    st.textContent = entry.status + (entry.severity ? `/${entry.severity}` : "");
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
}

async function refreshDocs() {
  if (!currentProject) return;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    renderDocList($("req-list"), snapshot.requirements, "req");
    renderDocList($("defect-list"), snapshot.defects, "defect");
    renderDocList($("goal-list"), snapshot.goals ?? [], "goal");
    $("req-count").textContent = `${snapshot.requirements.filter((r) => !r.closed).length}`;
    $("defect-count").textContent = `${snapshot.defects.filter((d) => !d.closed).length}`;
    $("goal-count").textContent = `${(snapshot.goals ?? []).filter((g) => g.status === "active").length}`;
    renderConventions(snapshot.conventions);
  } catch (err) {
    console.error(err);
  }
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
$("conv-open").addEventListener("click", () =>
  invoke("docs_open", { projectDir: currentProject, kind: "conventions" }).catch((e) => toast(String(e)))
);

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
    await invoke("conversation_clear");
    clearChat("已开启新对话(历史已清空)");
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
  $(btn).addEventListener("click", () =>
    invoke("docs_open", { projectDir: currentProject, kind }).catch((e) => toast(String(e)))
  );
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
  await loadModels();
  refreshGit();
  setStatus("空闲", false);
})();
