// ---------- 发送 / 停止 ----------
// 鞭挞状态:自动续跑计数(手动发送归零),上限防失控。
const DEFAULT_AUTO_CONTINUE_MAX = 10;
let autoRounds = 0;
let autoPaused = false;
let autoStopAfterRound = false;
let autoContinueTimer = null;
let autoContinueGeneration = 0;
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
  const base = text || `连续推进上限 ${max}`;
  // 自主推进**不再**自带七阶段(闸门已换成进程级「勘察复核」开关)。用户此前的心智
  // 模型是「开鞭挞 = 每轮勘察+复核」,不说出来就会以为勘察静默失效了——所以鞭挞
  // 开着而流水线关着时必须在这里明说,而不是让他从没有勘察块反推。
  const hint = $("auto-continue")?.checked && !phasePipelineOn() ? " · 勘察复核未开(每轮直接实现)" : "";
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
      sendText(continuePrompt(), { auto: true });
    }
  }, 2000);
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
  setRunning(true, attachmentStatus);
  // R-086:本轮运行开始,活动会话状态机同步为运行中——控制事件与状态机同源。
  // converged 复位:新一轮可以覆盖旧终态。
  if (activeSessionId) {
    const state = sessionState(activeSessionId);
    state.running = true;
    state.converged = false;
  }
  startElapsed();
  log(`${auto ? t("鞭挞") : t("发送")}:${prompt.slice(0, 80)}`);
  try {
    const mode = selectedAgent();
    const request = {
      prompt,
      projectDir: currentProject,
      profile: mode.profile,
      agent: mode.agent,
      model: $("model-select").value || null,
      workPriority: selectedWorkPriority(),
      delivery,
      attachments: promptAttachments.map((item) => ({ ...item })),
      processId: activeProcessId,
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
  $("auto-pause").classList.toggle("active", autoPaused);
  $("auto-pause").textContent = autoPaused ? t("继续鞭挞") : t("暂停鞭挞");
  // R-169:暂停状态同步后端状态机。
  void syncAutoRunState();
  if (autoPaused) cancelAutoContinueTimer();
  // BUG 修复:恢复时如果正处于轮间空闲,必须重新调度,否则鞭挞静默死亡。
  if (!autoPaused && !running && $("auto-continue").checked && autoContinueAllowed()) {
    setStatus(`${t("鞭挞恢复")},2 ${t("秒后继续")}…`, false);
    scheduleAutoContinue();
  }
  log(autoPaused ? t("鞭挞已暂停") : t("鞭挞已恢复"));
});
$("auto-stop-round").addEventListener("change", () => {
  autoStopAfterRound = $("auto-stop-round").checked;
  // R-169:本轮后停同步后端状态机(D-111:不持久化,重启即清)。
  void syncAutoRunState();
  log(autoStopAfterRound ? t("本轮结束后将停止鞭挞") : t("已取消本轮后停"));
});
$("auto-max").addEventListener("change", () => {
  const max = autoContinueMax();
  $("auto-max").value = max;
  localStorage.setItem("kz-auto-max", String(max));
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
    void syncAutoRunState();
    toast(t("鞭挞仅适用于自主推进模式，请先切换模式"));
    log(t("鞭挞未开启:结伴开发模式不支持自动续跑"));
    return;
  }
  localStorage.setItem("kz-auto-continue", $("auto-continue").checked ? "1" : "0");
  autoRounds = 0;
  // 开鞭挞的这一刻就要看到「勘察复核未开」提示,而不是等下一轮结束才显示。
  renderAutoStatus();
  if (!$('auto-continue').checked) cancelAutoContinueTimer();
  // R-169:开关同步后端状态机(enabled)。
  void syncAutoRunState();
  log($("auto-continue").checked ? `${t("鞭挞已开启:每轮结束自动推进目标")} (${t("轮")} ${autoContinueMax()})` : t("鞭挞已关闭"));
  // BUG 修复(触发):空闲时勾上鞭挞必须立刻抽第一鞭——原来只挂在"上一轮结束"上,
  // 冷启动勾选后永远没有第一轮,必须手点"继续"才动。
  if ($("auto-continue").checked && !running && !autoPaused) {
    setStatus("鞭挞启动,2 秒后开始…", false);
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
  if (autoContinueAllowed() || !$("auto-continue").checked) return;
  $("auto-continue").checked = false;
  localStorage.setItem("kz-auto-continue", "0");
  autoRounds = 0;
  cancelAutoContinueTimer();
  // R-169:模式不兼容时后端开关同步关闭。
  void syncAutoRunState();
  renderAutoStatus();
  log(t("当前模式不支持鞭挞，已自动关闭"));
  toast(t("鞭挞已关闭：当前进程不是自主推进模式"));
}
function applyProfileValue(backendProfile) {
  const remembered = activeProcessId ? processProfileUi.get(activeProcessId) : null;
  // 回退顺序:本进程的记忆 → 全局上次选择 → dev-pair。少了中间这一档,
  // 新进程与重启后的旧进程都会被静默降级成结伴开发。
  const globalChoice = localStorage.getItem(PROFILE_STORAGE_KEY);
  const fallback = ["dev-pair", "dev-auto"].includes(globalChoice) ? globalChoice : "dev-pair";
  if (backendProfile === "research") $("profile-select").value = "research";
  else $("profile-select").value = remembered && remembered !== "research" ? remembered : fallback;
  syncAutoContinueWithProfile();
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
$("stop").addEventListener("click", () => {
  // 本地立即复位,不依赖后端事件回执(事件通道故障时停止键也必须有效)。
  cancelAutoContinueTimer();
  autoRounds = 0;
  invoke("stop_run", { projectDir: currentProject, processId: activeProcessId }).catch((err) => reportPersistentError(`停止指令失败:${err}`));
  hideAsk();
  stopElapsed();
  setRunning(false, "已停止");
  // R-086:本地复位同样收敛到该会话状态机,不依赖后端事件回执。
  if (activeSessionId) {
    const state = sessionState(activeSessionId);
    state.running = false;
    state.converged = true;
  }
  log(t("已请求停止(本地已复位)"));
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
async function loadModels() {
  const select = $("model-select");
  const saved = localStorage.getItem(prefKey("model")) ?? localStorage.getItem("kz-model") ?? "";
  select.innerHTML = "";
  const def = document.createElement("option");
  def.value = "";
  def.textContent = t("模型:agent 默认");
  select.appendChild(def);
  try {
    const models = await invoke("models_list", { projectDir: currentProject });
    const ids = new Set(models.map((m) => m.id));
    for (const m of models) {
      const opt = document.createElement("option");
      opt.value = m.id;
      opt.textContent = m.label;
      if (m.id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    // D-167:探测不到不等于用不了——端点可能没实现 /models,key 也可能还没配好。
    // 手填过的模型要留在列表里,否则下次重开又得再填一遍。
    for (const id of manualModels()) {
      if (ids.has(id)) continue;
      const opt = document.createElement("option");
      opt.value = id;
      opt.textContent = `${id}(手填)`;
      if (id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    const custom = document.createElement("option");
    custom.value = MANUAL_MODEL_SENTINEL;
    custom.textContent = t("＋ 手填模型…");
    select.appendChild(custom);
    log(`模型列表已刷新(${models.length} 个可选)`);
  } catch (err) {
    reportPersistentError(`模型列表获取失败:${err}`);
  }
}

// 手填模型:provider:model 直指。有些 OpenAI 兼容端点不提供 /models,
// 或者 key 尚未配好导致探测为空,这条通道保证配了 provider 就一定能用。
const MANUAL_MODEL_SENTINEL = "__manual__";
function manualModels() {
  const list = readJson(prefKey("manual-models"), []);
  return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
}
function addManualModel(id) {
  const list = manualModels();
  if (!list.includes(id)) list.push(id);
  writeJson(prefKey("manual-models"), list);
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
  if (select.value === MANUAL_MODEL_SENTINEL) {
    const input = (window.prompt(t("填 provider:model,例如 deepseek:deepseek-chat")) || "").trim();
    // provider 名必须对得上配置里的键,否则后端 resolve_model 会直接失败。
    if (!/^[\w.-]+:.+$/.test(input)) {
      if (input) toast(t("格式应为 provider:model"));
      select.value = localStorage.getItem(prefKey("model")) || "";
      return;
    }
    addManualModel(input);
    localStorage.setItem(prefKey("model"), input);
    loadModels().then(() => {
      $("model-select").value = input;
    });
    if (activeProcessId) {
      updateLocalProcessItem(activeProcessId, { model: input });
      queueProcessUpdate(activeProcessId, { model: input })
        .catch((error) => reportPersistentError(`${t("进程模型保存失败")}:${error}`));
    }
    return;
  }
  localStorage.setItem(prefKey("model"), select.value);
  if (activeProcessId) {
    // 空串=清除本进程的模型覆盖(回落 agent 默认);传 null 会被后端当作"不修改"。
    updateLocalProcessItem(activeProcessId, { model: select.value || null });
    queueProcessUpdate(activeProcessId, { model: select.value })
      .catch((error) => reportPersistentError(`${t("进程模型保存失败")}:${error}`));
  }
});

