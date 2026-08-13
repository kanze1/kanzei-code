// ---------- 发送 / 停止 ----------
// 鞭挞状态:自动续跑计数(手动发送归零),上限防失控。
const DEFAULT_AUTO_CONTINUE_MAX = 10;
let autoRounds = 0;
let autoPaused = false;
let autoStopAfterRound = false;
const autoContinueTimers = new Map();
let autoStopReason = "";
// 连续无实质动作的轮数:第一次只追加推进指令,第二次才刹车。
// R-169:判定已下沉 harness auto_run 状态机,前端只保留镜像赋值。
let noActionRounds = 0;
// R-170:继续文案降级为用户意图载体(方案 A,评估结论 continue_prompt_dissection.md §5)。
// 引擎规则(取活/批次/阻塞/验收/节奏)全部归 system prompt 与 harness 状态机,
// 文案只保留极简意图句;textarea 承载用户附加意图,删空回落此默认。
const DEFAULT_CONTINUE_PROMPT = "继续推进，规则按系统提示执行。";

// R-169:NUDGE 文案生成已下沉 harness(nudge_prompt),前端不再持有模板。
// 无动作判定与推进指令由引擎给出,前端只负责在收到 Nudge 动作时发送。

function selectedAgent() {
  const mode = $("profile-select").value;
  if (mode === "dev-pair") return { profile: "dev", agent: "dev-pair" };
  if (mode === "dev-auto") return { profile: "dev", agent: "dev" };
  return { profile: "research", agent: "research" };
}
function workPriorityStorageKey() {
  return `kz-work-priority:${currentProject || "default"}`;
}
function selectedWorkPriority() {
  return $("work-priority-select").value === "requirement-first" ? "requirement-first" : "defect-first";
}
function syncWorkPriorityControl() {
  const saved = localStorage.getItem(workPriorityStorageKey());
  $("work-priority-select").value = saved === "requirement-first" ? saved : "defect-first";
  loadWorkFocus();
}

// 开发重心 = preference 记忆条目(真源)。下拉框只是快捷写法,记忆页可手写任意细度
// (「先收完这批缺陷再转需求」这类二元开关表达不了的意图);提示词由记忆生成,
// 所以开关与提示词不可能再互相矛盾——D-128 的根因就是二者写死后对打。
let workFocusMemory = null;
const WORK_FOCUS_PRESETS = {
  "defect-first": {
    title: "开发重心:缺陷优先",
    body: "取活顺序:先从上到下扫描 defects.md,再扫描 requirements.md;前一个队列没有可做项时才看后一个。\npriority 标签只是背景信息,不改变列表顺序。",
  },
  "requirement-first": {
    title: "开发重心:需求优先",
    body: "取活顺序:先从上到下扫描 requirements.md,再扫描 defects.md;前一个队列没有可做项时才看后一个。\npriority 标签只是背景信息,不改变列表顺序。",
  },
};
async function loadWorkFocus() {
  if (!currentProject) return;
  try {
    workFocusMemory = await invoke("memory_focus_get", { projectDir: currentProject });
  } catch {
    workFocusMemory = null;
  }
  // 回显:手写的自定义重心不强行归入两个预设,保持用户当前选择不被覆盖。
  const title = workFocusMemory?.title || "";
  if (title.includes("需求优先")) $("work-priority-select").value = "requirement-first";
  else if (title.includes("缺陷优先")) $("work-priority-select").value = "defect-first";
}

// 「勘察复核」= 阶段流水线总闸(2026-08-11 用户定调),勾选框在顶栏「更多」里。
function phasePipelineOn() {
  return $("process-phase-pipeline")?.checked === true;
}
function renderAutoStatus(text = autoStopReason) {
  const el = $("auto-status");
  if (!el) return;
  const max = autoContinueMax();
  const base = text || `${t("连续推进上限")} ${max}`;
  // 自主推进**不再**自带七阶段(闸门已换成进程级「勘察复核」开关)。用户此前的心智
  // 模型是「开鞭挞 = 每轮勘察+复核」,不说出来就会以为勘察静默失效了——所以鞭挞
  // 开着而流水线关着时必须在这里明说,而不是让他从没有勘察块反推。
  const hint = $("auto-continue")?.checked && !phasePipelineOn() ? ` · ${t("勘察复核未开(每轮直接实现)")}` : "";
  el.textContent = localizeDynamic(`${base}${hint}`);
}
// R-170:继续文案 = 用户附加意图 + 极简默认兜底。开发重心/引擎规则已由
// run.rs work_priority_guidance + memory preference 注入 system prompt,不再拼接。
function continuePrompt() {
  return $("continue-prompt").value.trim() || DEFAULT_CONTINUE_PROMPT;
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
function syncAutoRunState() {
  if (!activeSessionId) return;
  return invoke("auto_state_update", {
    sessionId: activeSessionId,
    enabled: $("auto-continue").checked,
    paused: autoPaused,
    stopAfterRound: autoStopAfterRound,
    maxRounds: autoContinueMax(),
  });
}
function resetAutoRunState() {
  if (activeSessionId) void invoke("auto_state_reset", { sessionId: activeSessionId });
}
function cancelAutoContinueTimer(sessionId = activeSessionId) {
  if (!sessionId) return;
  const entry = autoContinueTimers.get(sessionId);
  if (entry?.timer) clearTimeout(entry.timer);
  autoContinueTimers.delete(sessionId);
}
// D-291:续跑闸门的**唯一**判据。原来这几个条件散在两处 setTimeout 里,任一不满足
// 就 `return` ——不发下一轮、不清 auto_pending、不清横幅、不留一个字。界面于是永久
// 钉在「鞭挞 · 等待下一轮」,而那一轮永远不会来(引擎侧还记着 rounds+1,两边状态从此
// 对不上)。闸门必须集中且**开口说话**:不续跑可以,不说为什么不行(D-004 口径)。
function autoContinueBlockedReason(sessionId) {
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  if (!item) return "线路已关闭";
  if (sessionId === activeSessionId) {
    if (!$("auto-continue").checked) return "鞭挞已关闭";
    if (autoPaused) return "已暂停";
    if (autoStopAfterRound) return "本轮后停";
    return null;
  }
  const config = normalizeAutoState(processAutoState.get(item.id), item.id);
  if (!config.enabled) return "鞭挞已关闭";
  if (config.paused) return "已暂停";
  if (config.stopAfterRound) return "本轮后停";
  return null;
}
// `running` 与上面四条不同:它是**瞬态**,不是用户意图。kz:done 有意不收回运行态
// (07-events.js:328「真正收回由 kz:idle/kz:stopped 负责」),所以 2 秒到点时上一轮
// 可能仍标着运行中——旧代码在这里静默放弃,一轮就此永远不来。正确语义是等它落地,
// 但要有头:等满 AUTO_CONTINUE_RUNNING_GRACE 次还在跑,就当卡住了,报出来。
const AUTO_CONTINUE_RUNNING_GRACE = 15;
// 闸门拦下时收口:pending 必须落地,否则横幅与线路徽标一直显示「等待下一轮」。
function abortAutoContinue(reason, sessionId = activeSessionId) {
  if (sessionId) transitionSession(sessionId, "idle");
  if (sessionId === activeSessionId) {
    clearRunPending();
    renderAutoStatus(`${t("鞭挞未续跑")}:${t(reason)}`);
  }
  log(`${t("鞭挞未续跑")}:${t(reason)}`);
  if (sessionId) refreshParallelTaskProjection(sessionId);
}
// 续跑定时器:闸门在**触发时刻**复查(2 秒内用户可能暂停/切模式/新一轮已开跑)。
// generation 不符属于「被更新的一枪取代」,静默是对的——但那条路径的 pending
// 由取消方自己收口,不在这里处理。
function armAutoContinue(prompt, sessionId = activeSessionId, waited = 0) {
  if (!sessionId) return;
  // R-199:档位条件下沉引擎——armAutoContinue 不再检查 autoContinueAllowed(),
  // 引擎在 decide() 已判 Stop(ProfileMismatch) 且计数不 +1;前端不再持有
  // 引擎不知道的续跑否决权(计数与实际轮次不再漂移)。
  cancelAutoContinueTimer(sessionId);
  const generation = (sessionState(sessionId).auto_generation || 0) + 1;
  sessionState(sessionId).auto_generation = generation;
  const timer = setTimeout(async () => {
    const current = autoContinueTimers.get(sessionId);
    if (!current || current.generation !== generation) return;
    autoContinueTimers.delete(sessionId);
    const blocked = autoContinueBlockedReason(sessionId);
    if (blocked) {
      abortAutoContinue(blocked, sessionId);
      return;
    }
    const targetState = sessionState(sessionId);
    const item = processItems.find((candidate) => candidate.session_id === sessionId);
    if (item && processRunning(item)) {
      if (waited < AUTO_CONTINUE_RUNNING_GRACE) {
        armAutoContinue(prompt, sessionId, waited + 1);
      } else {
        abortAutoContinue("上一轮尚未结束", sessionId);
      }
      return;
    }
    transitionSession(sessionId, "starting", { local_start_pending: true });
    if (sessionId === activeSessionId) clearRunPending();
    await sendAutoToSession(prompt, sessionId);
  }, 2000);
  autoContinueTimers.set(sessionId, { timer, generation });
}
function scheduleAutoContinue() {
  armAutoContinue(continuePrompt());
}

async function sendAutoToSession(prompt, sessionId) {
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  if (!item) return abortAutoContinue("线路已关闭", sessionId);
  if (sessionId === activeSessionId) {
    addMessage("notice", `${t("鞭挞已触发")} · ${sessionState(sessionId).auto_rounds || 0}`);
    setRunning(true, t("准备中"));
  }
  try {
    await invoke("run_prompt", {
      prompt,
      projectDir: item.project_dir,
      profile: "dev",
      agent: "dev",
      model: item.model || null,
      workPriority: localStorage.getItem(`kz-work-priority:${item.origin_project}`) === "requirement-first" ? "requirement-first" : "defect-first",
      delivery: "queue",
      attachments: [],
      processId: item.id,
      autonomous: true,
      autoAllow: localStorage.getItem("kz-auto-allow") === "1",
    });
  } catch (error) {
    transitionSession(sessionId, "failed");
    if (sessionId === activeSessionId) {
      reportError(String(error));
      setRunning(false, t("出错"));
    } else {
      reportPersistentError(`${item.label} ${t("鞭挞续跑失败")}:${error}`);
    }
    refreshParallelTaskProjection(sessionId);
  }
}

function handleBackgroundSessionDone(payload) {
  const sessionId = payload?.sessionId;
  if (!sessionId) return;
  const action = payload.autoAction || { type: "NoContinue" };
  const state = sessionState(sessionId);
  state.auto_rounds = action.rounds ?? state.auto_rounds ?? 0;
  if (action.type === "Continue" || action.type === "Nudge" || action.type === "VerifyRound") {
    transitionSession(sessionId, "auto_pending", { auto_rounds: state.auto_rounds });
    refreshParallelTaskProjection(sessionId);
    armAutoContinue(action.type === "Nudge" ? action.prompt : DEFAULT_CONTINUE_PROMPT, sessionId);
  } else if (action.type === "Stop") {
    transitionSession(sessionId, "idle");
    cancelAutoContinueTimer(sessionId);
    refreshParallelTaskProjection(sessionId);
  }
}

// R-169:全部阻塞/清空停止已下沉 harness backlog_status + auto_run 状态机,
// 判定结果随 kz:done 的 autoAction(Stop:AllBlocked/BacklogEmpty)带给前端执行。
function renderAttachments() {
  const box = $("attachments");
  box.innerHTML = "";
  box.classList.toggle("hidden", attachments.length === 0);
  attachments.forEach((item, index) => {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "attachment-chip";
    chip.textContent = `${item.file_name} ×`;
    chip.title = t("移除附件");
    chip.setAttribute("aria-label", `${t("移除附件")} ${item.file_name}`);
    chip.addEventListener("click", () => { attachments.splice(index, 1); renderAttachments(); });
    box.appendChild(chip);
  });
}

function addFiles(files) {
  for (const file of files) {
    if (!(file.type.startsWith("image/") || file.type === "application/pdf" || file.name.toLowerCase().endsWith(".pdf"))) {
      toast(`${t("不支持的附件类型")}: ${file.name}`);
      continue;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const dataUrl = String(reader.result);
      attachments.push({ file_name: file.name, media_type: file.type || "application/pdf", data: dataUrl.split(",", 2)[1] || "" });
      renderAttachments();
    };
    reader.onerror = () => toastError(`${t("读取附件失败")}: ${file.name}`);
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
    toast(t("当前任务还在运行，自动鞭挞将在本轮完成后继续"));
    return;
  }
  if (!currentProject) {
    toast(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  if (!auto) void ensureNotificationPermission();
  if (running) {
    addMessage("user", prompt);
    log(`${t("运行中")}${delivery === "steer" ? t("插入") : t("排队")}:${prompt.slice(0, 80)}`);
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
        processId: activeProcessId,
        autonomous: auto,
        autoAllow: localStorage.getItem("kz-auto-allow") === "1",
      });
      toast(localizeDynamic(delivery === "steer" ? "已插入当前会话，将优先执行" : "已加入队列，将按顺序执行"));
      await refreshPendingInputs();
    } catch (err) {
      reportError(String(err), { retryable: false });
    }
    return;
  }
  if (!auto) {
    autoRounds = 0;
    noActionRounds = 0;
    cancelAutoContinueTimer();
    // R-169:手动发送归零后端状态机计数。
    resetAutoRunState();
  }
  currentAssistant = null;
  currentReasoning = null;
  runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };
  ctxTokens = 0;
  outputChars = 0;
  renderTokens();
  const attachmentStatus = promptAttachments.length > 0
    ? `${auto ? `${t("鞭挞")} ${autoRounds}/${autoContinueMax()} · ` : ""}${t("正在发送")} ${promptAttachments.length} ${t("个附件")} · ${t("准备中")}`
    : auto ? `${t("鞭挞")} ${autoRounds}/${autoContinueMax()} · ${t("准备中")}` : t("准备中");
  if (auto) {
    addMessage("notice", `${t("鞭挞已触发")} · ${autoRounds}/${autoContinueMax()}`);
  } else {
    addUserMessage(prompt, promptAttachments);
  }
  const requestSessionId = activeSessionId;
  const requestProcessId = activeProcessId;
  const requestProject = currentProject;
  if (requestSessionId) {
    const state = transitionSession(requestSessionId, "starting");
    state.auto_pending = false;
    // run_prompt 的 IPC 返回前，旧的 process_list 快照可能仍是 false；
    // 在首个实时事件到达前不能让这份旧快照覆盖本次用户明确的启动意图。
    state.live_running = null;
    state.local_start_pending = true;
    state.terminal_status = "";
  }
  clearRunPending();
  setRunning(true, attachmentStatus);
  // R-086:本轮运行开始,活动会话状态机同步为运行中——控制事件与状态机同源。
  // converged 复位:新一轮可以覆盖旧终态。
  if (requestSessionId) transitionSession(requestSessionId, "starting", { auto_pending: false });
  startElapsed();
  log(`${auto ? t("鞭挞") : t("发送")}:${prompt.slice(0, 80)}`);
  try {
    const mode = selectedAgent();
    const request = {
      prompt,
      projectDir: requestProject,
      profile: mode.profile,
      agent: mode.agent,
      model: $("model-select").value || null,
      workPriority: selectedWorkPriority(),
      delivery,
      attachments: promptAttachments.map((item) => ({ ...item })),
      processId: requestProcessId,
      autonomous: auto,
      autoAllow: localStorage.getItem("kz-auto-allow") === "1",
    };
    if (!auto) lastRequest = request;
    await invoke("run_prompt", request);
  } catch (err) {
    if (requestSessionId) transitionSession(requestSessionId, "failed");
    if (requestSessionId === activeSessionId) {
      reportError(String(err));
      stopElapsed();
      setRunning(false);
    } else {
      const failed = processItems.find((candidate) => candidate.session_id === requestSessionId);
      reportPersistentError(`${failed?.label || requestProcessId} ${t("发送失败")}:${err}`);
    }
    refreshParallelTaskProjection(requestSessionId);
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
    log(`${t("文件补全失败")}:${error}`, "warn");
  }
}

let fileSuggestionTimer = null;
promptBox.addEventListener("input", () => {
  promptHistoryIndex = -1;
  clearTimeout(fileSuggestionTimer);
  fileSuggestionTimer = setTimeout(refreshFileSuggestions, 80);
});
function stopAutoForManualInput() {
  if (!$('auto-continue').checked) return false;
  $('auto-continue').checked = false;
  localStorage.setItem("kz-auto-continue", "0");
  autoRounds = 0;
  noActionRounds = 0;
  cancelAutoContinueTimer();
  // R-169:手动输入接管 = 关闭后端自主推进并归零计数。
  void syncAutoRunState();
  resetAutoRunState();
  const message = t("收到手动输入，鞭挞已停止");
  setAutoStopReason(message);
  addMessage("notice", message);
  toast(message);
  log(message);
  return true;
}

function send() {
  const prompt = promptBox.value.trim();
  if (!prompt && attachments.length === 0) return;
  stopAutoForManualInput();
  // 只有附件没有文字时,sendText 的空 prompt 早退会静默吞掉附件(附件在此已被清空)。
  // 给一句默认描述,让图片/文件真的发得出去。
  if (!prompt && attachments.length > 0) {
    sendText(t("看一下这些附件"), { promptAttachments: attachments });
    promptBox.value = "";
    attachments = [];
    renderAttachments();
    return;
  }
  rememberPrompt(prompt);
  hideFileSuggestions();
  const promptAttachments = attachments;
  promptBox.value = "";
  attachments = [];
  renderAttachments();
  sendText(prompt, { promptAttachments });
}

$("send").addEventListener("click", send);
$("continue-btn").addEventListener("click", () => sendText(continuePrompt()));

async function openSopPicker() {
  if (!currentProject) {
    toast(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  const panel = $("sop-picker-panel");
  const list = $("sop-list");
  panel.classList.remove("hidden");
  list.replaceChildren();
  const loading = document.createElement("p");
  loading.className = "dim";
  loading.textContent = `${t("选择 SOP")}…`;
  list.appendChild(loading);
  try {
    const scopes = await Promise.all(["project", "global"].map((scope) =>
      invoke("memory_entries", { projectDir: currentProject, scope, category: "sop" })
    ));
    const entries = scopes.flat().filter((entry) => entry.status === "active");
    list.replaceChildren();
    if (!entries.length) {
      const empty = document.createElement("p");
      empty.className = "dim";
      empty.textContent = t("暂无可调用的 SOP");
      list.appendChild(empty);
      return;
    }
    for (const entry of entries) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "sop-entry";
      const title = document.createElement("strong");
      title.textContent = entry.title;
      const description = document.createElement("span");
      description.className = "dim";
      description.textContent = entry.description || entry.body?.slice(0, 120) || "";
      button.append(title, description);
      button.addEventListener("click", () => {
        const content = String(entry.body || "").trim();
        promptBox.value = content;
        panel.classList.add("hidden");
        stopAutoForManualInput();
        promptBox.focus();
        if (!content) {
          toast(t("SOP 内容为空"));
          return;
        }
        rememberPrompt(content);
        const delivery = $("delivery-select");
        const previous = delivery.value;
        delivery.value = "queue";
        void sendText(content).finally(() => { delivery.value = previous; });
        toast(t("SOP 已填入继续输入"));
      });
      list.appendChild(button);
    }
  } catch (error) {
    list.replaceChildren();
    const failed = document.createElement("p");
    failed.className = "dim";
    failed.textContent = `${t("SOP 加载失败")}: ${error}`;
    list.appendChild(failed);
  }
}
$("sop-picker").addEventListener("click", openSopPicker);
$("sop-picker-close").addEventListener("click", () => $("sop-picker-panel").classList.add("hidden"));

$("continue-toggle").addEventListener("click", () => {
  const panel = $("continue-panel");
  const open = panel.classList.toggle("hidden") === false;
  $("continue-toggle").setAttribute("aria-expanded", String(open));
  $("continue-toggle").textContent = t(open ? "收起文案" : "继续文案");
  if (open) $("continue-prompt").focus();
});
$("auto-continue").checked = localStorage.getItem("kz-auto-continue") === "1";
renderAutoStatus();
// R-170:LEGACY 升级机制已删除(规则剥离后无「历史默认需升级」契约错位)。
// 存什么读什么:用户自定义文案原样保留;删空回落极简默认。
{
  const stored = (localStorage.getItem("kz-continue-prompt") || "").trim();
  $("continue-prompt").value = stored || DEFAULT_CONTINUE_PROMPT;
}
$("continue-prompt").addEventListener("change", () => {
  const value = $("continue-prompt").value.trim();
  localStorage.setItem("kz-continue-prompt", value || DEFAULT_CONTINUE_PROMPT);
  $("continue-prompt").value = value || DEFAULT_CONTINUE_PROMPT;
});
$("auto-max").value = Math.min(100, Math.max(1, Number.parseInt(localStorage.getItem("kz-auto-max"), 10) || DEFAULT_AUTO_CONTINUE_MAX));
// 「本轮后停」是一次性意图,不是偏好:绝不持久化。
// 曾经持久化过——勾一次后 localStorage 永远是 "1",每次启动都重新武装,
// 表现为"鞭挞跑一轮就停,怎么都停不掉"(D-111)。这里顺手清掉存量键。
localStorage.removeItem("kz-auto-stop-round");
$("auto-stop-round").checked = false;
autoStopAfterRound = false;
// 启动时等 activeSessionId 就绪后同步所有控件；仅同步 enabled 会让已保存的轮数上限
// 在后端回落为默认 10，造成展示与实际安全上限不一致。
void syncAutoRunState();
$("auto-pause").addEventListener("click", () => {
  autoPaused = !autoPaused;
  rememberAutoUiState();
  $("auto-pause").classList.toggle("active", autoPaused);
  $("auto-pause").textContent = autoPaused ? t("继续鞭挞") : t("暂停鞭挞");
  // R-169:暂停状态同步后端状态机。
  void syncAutoRunState();
  if (autoPaused) cancelAutoContinueTimer();
  // BUG 修复:恢复时如果正处于轮间空闲,必须重新调度,否则鞭挞静默死亡。
  // R-199/D-323:档位条件下沉引擎,恢复路径不再持有前端私有否决——非 dev-auto 时
  // 静默不调度会让引擎计数与状态不知情(验收①未兑现)。恢复一律重新调度,
  // 档位不对由引擎下轮 done 判 Stop(ProfileMismatch) 带 reason 可见收口。
  if (!autoPaused && !running && $("auto-continue").checked) {
    setStatus(`${t("鞭挞恢复")},2 ${t("秒后继续")}…`, false);
    scheduleAutoContinue();
  }
  log(autoPaused ? t("鞭挞已暂停") : t("鞭挞已恢复"));
});
$("auto-stop-round").addEventListener("change", () => {
  autoStopAfterRound = $("auto-stop-round").checked;
  rememberAutoUiState();
  // R-169:本轮后停同步后端状态机(D-111:不持久化,重启即清)。
  void syncAutoRunState();
  log(autoStopAfterRound ? t("本轮结束后将停止鞭挞") : t("已取消本轮后停"));
});
$("auto-max").addEventListener("change", () => {
  const max = autoContinueMax();
  $("auto-max").value = max;
  localStorage.setItem("kz-auto-max", String(max));
  rememberAutoUiState();
  renderAutoStatus();
  autoRounds = 0;
  cancelAutoContinueTimer();
  // R-169:上限同步后端状态机。
  void syncAutoRunState();
  resetAutoRunState();
  log(`${t("鞭挞上限已设为")} ${max} ${t("轮")}`);
});
$("auto-continue").addEventListener("change", () => {
  if ($("auto-continue").checked && !autoContinueAllowed()) {
    $("auto-continue").checked = false;
    localStorage.setItem("kz-auto-continue", "0");
    autoRounds = 0;
    cancelAutoContinueTimer();
    rememberAutoUiState();
    void syncAutoRunState();
    toast(t("鞭挞仅适用于自主推进模式，请先切换模式"));
    log(t("鞭挞未开启:结伴开发模式不支持自动续跑"));
    return;
  }
  localStorage.setItem("kz-auto-continue", $("auto-continue").checked ? "1" : "0");
  autoRounds = 0;
  rememberAutoUiState();
  // 开鞭挞的这一刻就要看到「勘察复核未开」提示,而不是等下一轮结束才显示。
  renderAutoStatus();
  if (!$('auto-continue').checked) cancelAutoContinueTimer();
  // R-169:开关同步后端状态机(enabled)。
  void syncAutoRunState();
  log($("auto-continue").checked ? `${t("鞭挞已开启:每轮结束自动推进目标")} (${t("轮")} ${autoContinueMax()})` : t("鞭挞已关闭"));
  // BUG 修复(触发):空闲时勾上鞭挞必须立刻抽第一鞭——原来只挂在"上一轮结束"上,
  // 冷启动勾选后永远没有第一轮,必须手点"继续"才动。
  if ($("auto-continue").checked && !running && !autoPaused) {
    setStatus(t("鞭挞启动,2 秒后开始…"), false);
    scheduleAutoContinue();
  }
});
const PROFILE_STORAGE_KEY = "kz-profile";
const savedProfile = localStorage.getItem(PROFILE_STORAGE_KEY);
if (["dev-pair", "dev-auto", "research"].includes(savedProfile)) {
  $("profile-select").value = savedProfile;
}
// 后端只认 dev/research(决定 agent 选择),dev-auto 是前端的鞭挞档位,按进程单独记住,
// 否则切换进程回显时自主推进会被静默降级成结伴开发。
// R-115:这份映射必须落盘。早期只放在内存里,重启后它是空的,回退分支就把模式
// 降级成结伴开发——哪怕 kz-profile 里明明存着自主推进(D-155)。
const PROCESS_PROFILE_KEY = "kz-process-profile";
const processProfileUi = new Map(
  Object.entries(readJson(PROCESS_PROFILE_KEY, {})).filter(([, v]) =>
    ["dev-pair", "dev-auto", "research"].includes(v),
  ),
);
function persistProcessProfiles() {
  writeJson(PROCESS_PROFILE_KEY, Object.fromEntries(processProfileUi));
}

// 鞭挞是线路级控制状态。旧实现只把 enabled 放在全局 localStorage，切到一条
// 尚未配置的并行线时会把主线的勾选状态直接写进新 session，甚至让旧线路的定时器
// 在新线路上发送继续指令。没有记录的并行线默认关闭，必须由用户在该线路主动开启。
const PROCESS_AUTO_STATE_KEY = "kz-process-auto-state";
const processAutoState = new Map(
  Object.entries(readJson(PROCESS_AUTO_STATE_KEY, {})).filter(([, value]) => value && typeof value === "object"),
);
function normalizeAutoState(value, processId) {
  const storedMax = Number.parseInt(value?.maxRounds, 10);
  const legacyMax = Number.parseInt(localStorage.getItem("kz-auto-max"), 10);
  return {
    enabled: value?.enabled === undefined
      ? Boolean(processId?.startsWith("d|") && localStorage.getItem("kz-auto-continue") === "1")
      : value.enabled === true,
    paused: value?.paused === true,
    stopAfterRound: value?.stopAfterRound === true,
    maxRounds: Number.isFinite(storedMax)
      ? Math.min(100, Math.max(1, storedMax))
      : Number.isFinite(legacyMax) ? Math.min(100, Math.max(1, legacyMax)) : DEFAULT_AUTO_CONTINUE_MAX,
  };
}
function persistProcessAutoState() {
  writeJson(PROCESS_AUTO_STATE_KEY, Object.fromEntries(processAutoState));
}
// D-290:回显期间(applyProfileValue 把存档值刷回控件)一律不许落盘。控件在这一刻
// 显示的是**算出来的值**,不是用户意图;把它当意图写回去,一次算错就永久固化——
// 用户每次开 app 都得重设模式与鞭挞,正是这条路径自我延续的结果。
let applyingProfileEcho = false;
function rememberAutoUiState(processId = activeProcessId) {
  if (!processId || applyingProfileEcho) return;
  processAutoState.set(processId, {
    enabled: $("auto-continue").checked,
    paused: autoPaused,
    stopAfterRound: autoStopAfterRound,
    maxRounds: autoContinueMax(),
  });
  persistProcessAutoState();
}
function applyAutoUiState(processId) {
  const next = normalizeAutoState(processAutoState.get(processId), processId);
  processAutoState.set(processId, next);
  $("auto-continue").checked = next.enabled;
  autoPaused = next.paused;
  autoStopAfterRound = next.stopAfterRound;
  $("auto-stop-round").checked = autoStopAfterRound;
  $("auto-max").value = String(next.maxRounds);
  $("auto-pause").classList.toggle("active", autoPaused);
  $("auto-pause").textContent = autoPaused ? t("继续鞭挞") : t("暂停鞭挞");
  autoRounds = 0;
  renderAutoStatus();
  persistProcessAutoState();
}

// 进程级设置必须按线路串行落库。模型/profile/reasoning 原先是 fire-and-forget，
// 快速切换或刷新时旧请求可能晚于新请求完成，把刚选的值覆盖回去。
const processUpdateQueues = new Map();
function queueProcessUpdate(processId, fields) {
  const previous = processUpdateQueues.get(processId) || Promise.resolve();
  const next = previous
    .catch(() => {})
    .then(() => invoke("process_update", { processId, ...fields }));
  processUpdateQueues.set(processId, next);
  next.finally(() => {
    if (processUpdateQueues.get(processId) === next) processUpdateQueues.delete(processId);
  }).catch(() => {});
  return next;
}

function updateLocalProcessItem(processId, fields) {
  const item = processItems.find((candidate) => candidate.id === processId);
  if (item) Object.assign(item, fields);
}

function syncAutoContinueWithProfile() {
  // R-199:档位条件由引擎判定(decide→Stop/ProfileMismatch),前端不再持有否决权。
  // 切换 profile 时**不**主动取消勾选——引擎会在下一轮 done 事件里判 Stop 并带
  // reason,07-events.js 的 ProfileMismatch 分支负责取消勾选 + 显示原因。这里再
  // 关一次会让「用户明明勾了、却被静默取消」的旧漂移复发(D-290/R-199)。
  if (autoContinueAllowed() || !$("auto-continue").checked) return;
  // 非 dev-auto 且当前勾选:保留勾选,交给引擎下轮判定;仅同步本地存储供
  // normalizeAutoState 冷启动读回时不被误判。
  rememberAutoUiState();
}
function applyProfileValue(backendProfile) {
  // D-290:没有进程身份就没有「该显示谁的档位」这个问题。此时既读不到本进程记忆,
  // 回退链又会算出 dev-pair,把控件刷成结伴开发 —— 随后 syncAutoContinueWithProfile
  // 顺手关掉鞭挞,下一次 switchProcess 再把这个假值写进存档。启动竞态里 activeProcessId
  // 尚未就绪的那一瞬,就是整条降级链的起点。不知道就别动控件。
  if (!activeProcessId) return;
  const remembered = processProfileUi.get(activeProcessId);
  // 主线兼容旧的全局偏好；并行线没有本线设置时必须从安全默认 dev-pair 起步，
  // 不能因为主线曾选过 dev-auto 就让新线静默开启鞭挞。
  const globalChoice = localStorage.getItem(PROFILE_STORAGE_KEY);
  const fallback = activeProcessId?.startsWith("d|") && ["dev-pair", "dev-auto"].includes(globalChoice)
    ? globalChoice
    : "dev-pair";
  if (backendProfile === "research") $("profile-select").value = "research";
  else $("profile-select").value = remembered && remembered !== "research" ? remembered : fallback;
  // 回显期间关掉的鞭挞只是**跟随显示**,不是用户按下的开关:不落盘、不写全局键。
  applyingProfileEcho = true;
  try {
    syncAutoContinueWithProfile();
  } finally {
    applyingProfileEcho = false;
  }
}
$("profile-select").addEventListener("change", () => {
  const value = $("profile-select").value;
  localStorage.setItem(PROFILE_STORAGE_KEY, value);
  if (activeProcessId) {
    processProfileUi.set(activeProcessId, value);
    persistProcessProfiles();
    const profile = value === "research" ? "research" : "dev";
    updateLocalProcessItem(activeProcessId, { profile });
    queueProcessUpdate(activeProcessId, { profile })
      .catch((error) => reportPersistentError(`${t("进程模式保存失败")}:${error}`));
  }
  syncAutoContinueWithProfile();
  rememberAutoUiState();
});
$("work-priority-select").addEventListener("change", async () => {
  const value = selectedWorkPriority();
  localStorage.setItem(workPriorityStorageKey(), value);
  if (!currentProject) return;
  try {
    // 切换 = 写记忆(真源),不是只改本地开关;记忆页随后可把正文改成任意细度。
    workFocusMemory = await invoke("memory_focus_set", {
      projectDir: currentProject,
      title: WORK_FOCUS_PRESETS[value].title,
      body: WORK_FOCUS_PRESETS[value].body,
    });
    log(localizeDynamic(value === "requirement-first" ? "已切换为需求优先" : "已切换为缺陷优先"));
  } catch (err) {
    toastError(`${t("开发重心保存失败")}:${err}`);
  }
});
$("stop").addEventListener("click", async () => {
  const targetSessionId = activeSessionId;
  const targetProcessId = activeProcessId;
  const targetProject = currentProject;
  cancelAutoContinueTimer(targetSessionId);
  autoRounds = 0;
  if (runControlPending && !running) {
    if (targetSessionId) transitionSession(targetSessionId, "stopped");
    $("auto-continue").checked = false;
    rememberAutoUiState();
    void syncAutoRunState();
    clearRunPending();
    setRunning(false, t("已停止"));
    log(t("已停止鞭挞等待"));
    return;
  }
  if (targetSessionId) transitionSession(targetSessionId, "stopping");
  setStopping(t("停止中…"));
  hideAsk();
  try {
    await invoke("stop_run", { projectDir: targetProject, processId: targetProcessId });
    log(t("停止指令已确认，等待会话终态"));
  } catch (err) {
    const item = processItems.find((candidate) => candidate.session_id === targetSessionId);
    if (targetSessionId) transitionSession(targetSessionId, item?.running ? "running" : "idle");
    if (targetSessionId === activeSessionId) {
      setRunning(Boolean(item?.running), item?.running ? t("运行中") : t("空闲"));
    }
    reportPersistentError(`${t("停止指令失败")}:${err}`);
  }
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
  } else if (e.key === "Enter" && !e.shiftKey) {
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
const SHOW_ALL_MODELS_SENTINEL = "__show_all_models__";
let showAllModels = false;
async function loadModels({ showAll = false } = {}) {
  showAllModels = showAll;
  const select = $("model-select");
  // R-178 批3:首次进入项目时把 localStorage 旧键上迁后端(幂等,成功后不再执行)。
  void migrateLegacyModelPrefs();
  const activeModel = processItems.find((item) => item.id === activeProcessId)?.model || "";
  const saved = activeModel || legacyModelPrefValue();
  const selectedIds = new Set([saved, activeModel, ...manualModels()].filter(Boolean));
  select.innerHTML = "";
  const def = document.createElement("option");
  def.value = "";
  def.textContent = t("模型:agent 默认");
  select.appendChild(def);
  try {
    const models = await invoke("models_list", { projectDir: currentProject });
    const ids = new Set(models.map((m) => m.id));
    // 顶栏默认只展示已选/已记住的模型和两个角色入口；完整探测清单仍可按需展开。
    const visibleModels = showAll
      ? models
      : models.filter((m) => ["primary", "fast"].includes(m.id) || selectedIds.has(m.id));
    for (const m of visibleModels) {
      const opt = document.createElement("option");
      opt.value = m.id;
      opt.textContent = m.label;
      if (m.id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    // 当前线路/项目记住的直指模型即使未被 /models 返回，也必须保留为可见选项。
    // 这是 DeepSeek 等端点探测失败时仍能继续使用的关键兜底。
    for (const id of selectedIds) {
      if (ids.has(id)) continue;
      const opt = document.createElement("option");
      opt.value = id;
      opt.textContent = `${id}(${t("已记住")})`;
      if (id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    if (!showAll && models.some((m) => !visibleModels.includes(m))) {
      const all = document.createElement("option");
      all.value = SHOW_ALL_MODELS_SENTINEL;
      all.textContent = t("显示全部探测模型…");
      select.appendChild(all);
    }
    // D-167:探测不到不等于用不了——端点可能没实现 /models,key 也可能还没配好。
    // 手填过的模型要留在列表里,否则下次重开又得再填一遍。
    for (const id of manualModels()) {
      if (ids.has(id) || selectedIds.has(id)) continue;
      const opt = document.createElement("option");
      opt.value = id;
      opt.textContent = `${id}(${t("手填")})`;
      if (id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    const custom = document.createElement("option");
    custom.value = MANUAL_MODEL_SENTINEL;
    custom.textContent = t("＋ 手填模型…");
    select.appendChild(custom);
    log(`${t("模型列表已刷新")}(${models.length} 个可选)`);
  } catch (err) {
    reportPersistentError(`${t("模型列表获取失败")}:${err}`);
  }
}

// 手填模型:provider:model 直指。有些 OpenAI 兼容端点不提供 /models,
// 或者 key 尚未配好导致探测为空,这条通道保证配了 provider 就一定能用。
const MANUAL_MODEL_SENTINEL = "__manual__";
// R-178 批3:localStorage 旧键一次性上迁后端(②层),前端不再以 localStorage 为真源。
// 旧键形态:`kz-model`(更早的全局键)、`kz-model:<project>`(R-115 项目级)、
// `kz-manual-models:<project>`(手填候选)。保留旧键 fallback 一个版本——迁移执行前
// 旧值仍可读(legacyModelPrefValue/legacyManualModels),迁移成功后旧键即清除。
// 首次进入项目时把 localStorage 旧键上迁到默认进程(②层持久选择)并清除旧键。
// 幂等:旧键不存在时直接返回;迁移失败保留旧键,下次 loadModels 再试。不设一次性
// 标志——「旧键清除后自然不再迁移」就是幂等,也让失败可重试(保留旧键 fallback
// 一个版本:迁移函数保留到下一大版本再删)。
function legacyModelPrefValue() {
  return localStorage.getItem(prefKey("model")) ?? localStorage.getItem("kz-model") ?? "";
}
function legacyManualModels() {
  const list = readJson(prefKey("manual-models"), []);
  return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
}
async function migrateLegacyModelPrefs() {
  if (!currentProject) return;
  const legacyModel = legacyModelPrefValue();
  const legacyManual = legacyManualModels();
  if (!legacyModel && legacyManual.length === 0) return;
  const defaultProcess = processItems.find((item) => item.id.startsWith("d|"));
  if (!defaultProcess) return; // 默认进程尚未就绪,待 process_list 后由 loadModels 再触发
  const patch = {};
  if (legacyModel) patch.model = legacyModel;
  if (legacyManual.length > 0) patch.manualModels = legacyManual;
  try {
    await invoke("process_update", { processId: defaultProcess.id, ...patch });
    localStorage.removeItem(prefKey("model"));
    localStorage.removeItem("kz-model");
    localStorage.removeItem(prefKey("manual-models"));
    log(`已迁移旧模型偏好到后端:${JSON.stringify(patch)}`);
  } catch (error) {
    reportPersistentError(`${t("旧模型偏好迁移失败")}:${error}`);
  }
}
function manualModels() {
  const legacy = legacyManualModels();
  const list = legacy.length > 0 ? legacy : (processItems.find((item) => item.id.startsWith("d|"))?.manual_models ?? []);
  return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
}
function addManualModel(id) {
  const list = manualModels();
  if (!list.includes(id)) list.push(id);
  const defaultProcess = processItems.find((item) => item.id.startsWith("d|"));
  if (defaultProcess) {
    return queueProcessUpdate(defaultProcess.id, { manualModels: list })
      .then(() => refreshProcesses())
      .catch((error) => reportPersistentError(`${t("手填模型保存失败")}:${error}`));
  }
  // 默认进程未就绪(极端时序),退回 localStorage 暂存,由迁移函数下次接手。
  writeJson(prefKey("manual-models"), list);
  return Promise.resolve();
}
// R-115:模型与思考强度按项目记——不同项目常配不同模型,共用一个全局键会互相打架。
// 思考强度此前只写不读(kz-reasoning 全仓零处 getItem),等于每次重启都回默认档。
function prefKey(name) {
  return `kz-${name}:${currentProject || "default"}`;
}
function restoreProjectPrefs() {
  const reasoning = localStorage.getItem(prefKey("reasoning"));
  const select = $("reasoning-select");
  // 选项不存在时不要硬塞:赋一个无效值会让 select 落到空串,反而清掉配置默认档。
  if (reasoning !== null && [...select.options].some((o) => o.value === reasoning)) {
    select.value = reasoning;
  }
  const delivery = localStorage.getItem("kz-delivery");
  const deliverySelect = $("delivery-select");
  if (delivery && [...deliverySelect.options].some((o) => o.value === delivery)) {
    deliverySelect.value = delivery;
  }
  restoreDocFilters();
}

// 思考强度:空值=用配置默认档,其余为本进程覆盖。
$("reasoning-select").addEventListener("change", () => {
  const value = $("reasoning-select").value;
  localStorage.setItem(prefKey("reasoning"), value);
  if (activeProcessId) {
    updateLocalProcessItem(activeProcessId, { reasoning: value });
    queueProcessUpdate(activeProcessId, { reasoning: value })
      .catch((error) => reportPersistentError(`${t("进程思考强度保存失败")}:${error}`));
  }
});

$("model-select").addEventListener("change", () => {
  const select = $("model-select");
  if (select.value === SHOW_ALL_MODELS_SENTINEL) {
    const selected = processItems.find((item) => item.id === activeProcessId)?.model ||
      legacyModelPrefValue();
    loadModels({ showAll: true }).then(() => {
      select.value = selected;
    });
    return;
  }
  if (select.value === MANUAL_MODEL_SENTINEL) {
    const input = (window.prompt(t("填 provider:model,例如 deepseek:deepseek-chat")) || "").trim();
    // provider 名必须对得上配置里的键,否则后端 resolve_model 会直接失败。
    if (!/^[\w.-]+:.+$/.test(input)) {
      if (input) toast(t("格式应为 provider:model"));
      select.value = processItems.find((item) => item.id === activeProcessId)?.model || "";
      return;
    }
    addManualModel(input).then(() => loadModels()).then(() => {
      $("model-select").value = input;
    });
    if (activeProcessId) {
      updateLocalProcessItem(activeProcessId, { model: input });
      queueProcessUpdate(activeProcessId, { model: input })
        .catch((error) => reportPersistentError(`${t("进程模型保存失败")}:${error}`));
    }
    return;
  }
  if (activeProcessId) {
    // 空串=清除本进程的模型覆盖(回落 agent 默认);传 null 会被后端当作"不修改"。
    updateLocalProcessItem(activeProcessId, { model: select.value || null });
    queueProcessUpdate(activeProcessId, { model: select.value })
      .catch((error) => reportPersistentError(`${t("进程模型保存失败")}:${error}`));
  }
});

