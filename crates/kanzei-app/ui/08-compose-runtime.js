// `running` 与上面四条不同:它是**瞬态**,不是用户意图。kz:done 有意不收回运行态
// (07-events.js:328「真正收回由 kz:idle/kz:stopped 负责」),所以 2 秒到点时上一轮
// 可能仍标着运行中——旧代码在这里静默放弃,一轮就此永远不来。正确语义是等它落地,
// 但要有头:等满 AUTO_CONTINUE_RUNNING_GRACE 次还在跑,就当卡住了,报出来。
const AUTO_CONTINUE_RUNNING_GRACE = 15;
// 用户自己按下的刹车(关鞭挞/暂停/本轮后停)不需要额外提示——界面上那个开关就是
// 解释。其余原因是**意外停摆**:线路被关掉、上一轮卡住不结束。后台线出这两种时
// 原实现只 log() 一行,而日志面板默认收起——用户看到的就是「并行跑着跑着没消息了」,
// 没有任何可见解释(用户 2026-08-16 报告)。这类原因必须浮到界面上。
const AUTO_CONTINUE_INTENDED_STOPS = new Set(["鞭挞已关闭", "已暂停", "本轮后停"]);
// 闸门拦下时收口:pending 必须落地,否则横幅与线路徽标一直显示「等待下一轮」。
function abortAutoContinue(reason, sessionId = activeSessionId) {
  releaseAutoContinue(sessionId);
  if (sessionId) transitionSession(sessionId, "idle");
  const item = sessionId ? processItems.find((candidate) => candidate.session_id === sessionId) : null;
  if (sessionId === activeSessionId) {
    clearRunPending();
    renderAutoStatus(`${t("鞭挞未续跑")}:${t(reason)}`);
  } else if (!AUTO_CONTINUE_INTENDED_STOPS.has(reason)) {
    reportPersistentError(`${item?.label ?? sessionId} ${t("鞭挞未续跑")}:${t(reason)}`);
  }
  log(`${t("鞭挞未续跑")}:${t(reason)}`);
  if (sessionId) refreshParallelTaskProjection(sessionId);
}
// 续跑定时器:闸门在**触发时刻**复查(2 秒内用户可能暂停/切模式/新一轮已开跑)。
// generation 不符属于「被更新的一枪取代」,静默是对的——但那条路径的 pending
// 由取消方自己收口,不在这里处理。
// retryLabel:这一枪是 D-403 的失败退避重试(带展示文案),不是正常续跑。标记必须
// 跟着**定时器条目**走,因为终态错误处理器要据此放它一条生路(见 07-events.js kz:error)。
function armAutoContinue(prompt, sessionId = activeSessionId, waited = 0, delayMs = 2000, retryLabel = null) {
  if (!sessionId) return;
  // 在飞 = 这条线的上一枪已经发出但还没收到它的终点事件。此时排下一枪会重复发送,
  // 所以返回是对的;但**不能静默**——标记漏释放时(后台线曾经就是)整条鞭挞永久停摆,
  // 而界面钉在「等待下一轮」,日志里连一行线索都没有。
  if (autoContinueInFlight.has(sessionId)) {
    log(`${t("鞭挞未续跑")}:${t("上一枪仍在飞")} ${sessionId}`, "warn");
    return;
  }
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
    const item = processItems.find((candidate) => candidate.session_id === sessionId);
    if (item && processRunning(item)) {
      if (waited < AUTO_CONTINUE_RUNNING_GRACE) {
        armAutoContinue(prompt, sessionId, waited + 1, 2000, retryLabel);
        return;
      }
      if (item.running) {
        // 后端也说在跑 = 上一轮真没结束,放弃是对的。
        abortAutoContinue("上一轮尚未结束", sessionId);
        return;
      }
      // 宽限耗尽、但**后端权威说没在跑**:那就是本地状态机被某条路径卡在了运行态,
      // 不是上一轮没结束。原实现在这里一律放弃,于是任何一次本地态卡死都升级成
      // 鞭挞永久停摆——auto_pending 不收敛那个 bug 正是这么烧掉 32 秒的。
      // 后端是运行态权威(R-086):按它收敛本地态再继续,让这一类错误自愈而不是致命。
      log(`${t("鞭挞")}:${t("本地运行态与后端不符,按后端空闲继续")}`, "warn");
      transitionSession(sessionId, "idle");
    }
    transitionSession(sessionId, "starting", { local_start_pending: true });
    if (sessionId === activeSessionId) clearRunPending();
    await sendAutoToSession(prompt, sessionId);
  }, delayMs);
  autoContinueTimers.set(sessionId, { timer, generation, retryLabel });
}
function scheduleAutoContinue() {
  armAutoContinue(continuePrompt());
}

async function sendAutoToSession(prompt, sessionId) {
  if (autoContinueInFlight.has(sessionId)) return;
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  if (!item) return abortAutoContinue("线路已关闭", sessionId);
  autoContinueInFlight.add(sessionId);
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
    releaseAutoContinue(sessionId);
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
  // 在飞标记必须在这里释放。活动线走 07-events 的 kz:done/kz:idle 处理器释放,而后台线
  // 的控制事件在 01-core 路由层就被拦下(kz:done 只转到本函数,kz:idle 直接 return)——
  // 两条释放路径后台线一条都走不到。于是切走一条正在鞭挞的线之后:那一轮的 kz:done 到达,
  // 本函数转 auto_pending 再 armAutoContinue,而 armAutoContinue 第一行的在飞守卫直接静默
  // 返回,下一轮永远不排。用户看到的就是「切走的线卡在等待下一轮再也不动」,且一个字都没有。
  releaseAutoContinue(sessionId);
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
    // 引擎判定该线不能再续跑(全阻塞/清空/档位不符)时,后台线自己的鞭挞存档
    // 也要置关——否则切回该线时勾选框回显"开着",与引擎的实际停机对不上;
    // 本轮后停是一次性意图,同样要在所属线上落地取消,不能等用户切回来。
    if (["AllBlocked", "BacklogEmpty", "ProfileMismatch"].includes(action.reason)) {
      applyAutoStopToSession(sessionId, { enabled: false });
    } else if (action.reason === "StopAfterRound") {
      applyAutoStopToSession(sessionId, { stopAfterRound: false });
    }
    refreshParallelTaskProjection(sessionId);
  }
}

// 失败停摆的原因文案:活动线(07-events kz:auto-fail)与后台线(下面那个)必须同一份,
// 否则同一件事在两条线上说法不同。
function autoFailStopReasonText(reason) {
  if (reason === "RateLimited") return t("provider 限流(429)，自动推进已暂停，请等待后手动恢复");
  if (reason === "RepeatedFailure") return t("连续多轮运行失败,自动推进已停止(已发手机通知)");
  return t("运行失败:致命错误,自动推进已停止");
}

// D-403 的失败退避重试对后台线同样必须生效。kz:auto-fail 既不是控制事件、也不在
// BACKGROUND_RENDER_EVENTS 里,路由层原本把后台线的这条整条丢掉:在飞标记不释放、
// 重试不排、停摆原因不落——后台线断一次网就永久停摆,而它恰恰是没人看着的那条。
// 与 handleBackgroundSessionDone 同构:只动**所属线**的状态,绝不写当前线的控制台文本槽。
function handleBackgroundAutoFail(payload) {
  const sessionId = payload?.sessionId;
  if (!sessionId) return;
  releaseAutoContinue(sessionId);
  const action = payload.autoAction || { type: "NoContinue" };
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  const label = item?.label ?? sessionId;
  if (action.type === "RetryAfterFailure") {
    const delayMs = action.delayMs ?? 15000;
    const retryLabel = `${t("失败重试")} ${action.attempt}/${action.maxAttempts ?? 3} · ${Math.round(delayMs / 1000)}s`;
    transitionSession(sessionId, "auto_pending", {
      auto_rounds: action.rounds ?? sessionState(sessionId).auto_rounds ?? 0,
    });
    armAutoContinue(DEFAULT_CONTINUE_PROMPT, sessionId, 0, delayMs, retryLabel);
    log(`${label} ${t("鞭挞")}:${retryLabel}`, "warn");
  } else if (action.type === "Stop") {
    transitionSession(sessionId, "idle");
    cancelAutoContinueTimer(sessionId);
    // 后台线停摆没人看着:必须浮到界面上(abortAutoContinue 同一口径),不能只 log 一行。
    reportPersistentError(`${label} ${t("鞭挞停止")}:${autoFailStopReasonText(action.reason)}`);
  }
  refreshParallelTaskProjection(sessionId);
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

// 发送用的模型 = **该线存的模型**,不是下拉的显示值。下拉是回显,而回显曾经会回落到旧全局
// 键(见 loadModels 的注释);鞭挞续跑读的一直是 item.model。两条路不同源的后果是:同一条线
// 手动发一句和自动轮跑在两个不同的模型上,而界面上只有一个下拉,看不出来。用户改下拉时
// change 处理器已经先 updateLocalProcessItem,所以这里读到的就是刚选的那个值。
function lineModelFor(processId) {
  const item = processItems.find((candidate) => candidate.id === processId);
  return item ? item.model || null : $("model-select").value || null;
}

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
        model: lineModelFor(activeProcessId),
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
    setAutoRounds(activeSessionId, 0);
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
    // R-206:全部经 transitionSession 折算;补充 detail 一次传入,不再手工直写。
    transitionSession(requestSessionId, "starting", {
      auto_pending: false,
      // run_prompt 的 IPC 返回前，旧的 process_list 快照可能仍是 false；
      // 在首个实时事件到达前不能让这份旧快照覆盖本次用户明确的启动意图。
      live_running: null,
      local_start_pending: true,
      terminal_status: "",
    });
  }
  clearRunPending();
  setRunning(true, attachmentStatus);
  // R-086/R-206:活动会话状态机已在上面 transitionSession("starting") 统一收敛,
  // 这里不再重复调用——重复写块是 R-197 叠在旧块上的残渣(见 R-206 验收④)。
  startElapsed();
  log(`${auto ? t("鞭挞") : t("发送")}:${prompt.slice(0, 80)}`);
  try {
    const mode = selectedAgent();
    const request = {
      prompt,
      projectDir: requestProject,
      profile: mode.profile,
      agent: mode.agent,
      model: lineModelFor(requestProcessId),
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
  rememberAutoUiState();
  setAutoRounds(activeSessionId, 0);
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
// 鞭挞开关是线路级状态,唯一真源是 kz-process-auto-state(按 processId 分键)。
// 旧全局键 kz-auto-continue 让 A 项目的勾选漏进 B 项目的默认线并被固化——
// 启动回显、停机收口、无记录回落全部不得再碰全局键;存量键就地清除。
localStorage.removeItem("kz-auto-continue");
renderAutoStatus();
// R-170:LEGACY 升级机制已删除(规则剥离后无「历史默认需升级」契约错位)。
// 存什么读什么:用户自定义文案原样保留;删空回落极简默认。
{
  const stored = (localStorage.getItem("kz-continue-prompt") || "").trim();
  $("continue-prompt").value = stored || DEFAULT_CONTINUE_PROMPT;
  // D-404:localStorage 可能重启即丢;后端 app.json 权威值覆盖。
  void uiPrefsLoad().then((p) => {
    if (p.continue_prompt) $("continue-prompt").value = p.continue_prompt;
  });
}
$("continue-prompt").addEventListener("change", () => {
  const value = $("continue-prompt").value.trim();
  localStorage.setItem("kz-continue-prompt", value || DEFAULT_CONTINUE_PROMPT);
  $("continue-prompt").value = value || DEFAULT_CONTINUE_PROMPT;
  // D-404:后端持久化。
  void uiPrefsSave({ continue_prompt: value || DEFAULT_CONTINUE_PROMPT });
});
$("auto-max").value = Math.min(100, Math.max(1, Number.parseInt(localStorage.getItem("kz-auto-max"), 10) || DEFAULT_AUTO_CONTINUE_MAX));
// D-404:localStorage 可能重启即丢;后端 app.json 权威值覆盖(缺省回落默认)。
// 同步回写 localStorage,normalizeAutoState 的 legacyMax 回退路径也能拿到后端值。
void uiPrefsLoad().then((p) => {
  if (Number.isFinite(p.auto_max)) {
    const max = Math.min(100, Math.max(1, Number(p.auto_max)));
    localStorage.setItem("kz-auto-max", String(max));
    $("auto-max").value = String(max);
  }
});
// 「本轮后停」是一次性意图,不是偏好:绝不持久化。
// 曾经持久化过——勾一次后 localStorage 永远是 "1",每次启动都重新武装,
// 表现为"鞭挞跑一轮就停,怎么都停不掉"(D-111)。这里顺手清掉存量键。
localStorage.removeItem("kz-auto-stop-round");
$("auto-stop-round").checked = false;
autoStopAfterRound = false;
// 启动时等 activeSessionId 就绪后同步所有控件；仅同步 enabled 会让已保存的轮数上限
// 在后端回落为默认 10，造成展示与实际安全上限不一致。
void syncAutoRunState();
// 停机原因徽标旁的一键复跑:达上限时引擎不关 enabled,清零轮次重排即可;
// AllBlocked/BacklogEmpty/ProfileMismatch 会把 checked 置 false,那时先把开关打开,
// 复用 change 分支既有的档位提示,不在这里再判一次。
$("auto-resume").addEventListener("click", () => {
  const toggle = $("auto-continue");
  if (!toggle.checked) {
    toggle.checked = true;
    toggle.dispatchEvent(new Event("change"));
    return;
  }
  setAutoRounds(activeSessionId, 0);
  setAutoStopReason("");
  resetAutoRunState();
  setStatus(`${t("鞭挞恢复")},2 ${t("秒后继续")}…`, false);
  scheduleAutoContinue();
});
// 面板开着时数字键直接命中对应行(参照 Claude 的 Mode 菜单)。只在 details[open]
// 且焦点不在输入框里时生效——否则会把用户在上限框里敲的数字吞掉。
/// 把鞭挞设置面板夹在视口里。它原先靠 CSS 锚一条固定边(right:0 / left:0),
/// 而触发器的横坐标随 #composer-bar 换行与侧栏宽度(可拖 220~460)大幅漂移——
/// 锚哪边都会在某一段窗口宽度下把 420px 宽的面板整个顶出视口,那一片里
/// 「自动放行」这类开关看得见也点不到。改成以触发器为原点算 left,再夹进
/// [8, innerWidth-width-8];position:fixed 让它不受祖先裁剪影响。
function placeAutorunMenu() {
  const host = $("autorun-more");
  const menu = host?.querySelector(".autorun-menu");
  if (!host || !menu || !host.open) return;
  if (typeof host.getBoundingClientRect !== "function") return;
  const anchor = host.getBoundingClientRect();
  const width = menu.offsetWidth || Math.min(420, window.innerWidth - 16);
  const left = Math.min(Math.max(8, anchor.left), Math.max(8, window.innerWidth - width - 8));
  menu.style.left = `${Math.round(left)}px`;
  menu.style.bottom = `${Math.round(Math.max(8, window.innerHeight - anchor.top + 6))}px`;
}
$("autorun-more").addEventListener("toggle", placeAutorunMenu);
window.addEventListener("resize", placeAutorunMenu);

$("autorun-more").addEventListener("keydown", (event) => {
  if (!$("autorun-more").open) return;
  const tag = String(event.target?.tagName || "").toLowerCase();
  if (tag === "input" || tag === "select" || tag === "textarea") return;
  const row = $("autorun-more").querySelector(`.menu-row[data-shortcut="${event.key}"]`);
  if (!row) return;
  event.preventDefault();
  const control = row.querySelector('input[type="checkbox"]') || row.querySelector("button");
  if (!control) return;
  if (control.tagName.toLowerCase() === "button") control.click();
  else {
    control.checked = !control.checked;
    control.dispatchEvent(new Event("change"));
  }
});
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
// R-322 B3:目标条件。change(失焦/回车)才同步,不逐字符打后端。
// 不落 localStorage:目标是**一次性意图**,跟「本轮后停」同类(D-111)——
// 持久化会让它在下次开应用时静默复活,驱动一段跟它无关的对话。
$("auto-goal")?.addEventListener("change", () => {
  renderGoalState();
  void syncAutoRunState();
  const goal = currentGoalText().trim();
  log(goal ? `${t("目标条件已设置")}:${goal}` : t("目标条件已清除"));
});
$("auto-max").addEventListener("change", () => {
  const max = autoContinueMax();
  $("auto-max").value = max;
  localStorage.setItem("kz-auto-max", String(max));
  void uiPrefsSave({ auto_max: max }); // D-404:后端持久化
  rememberAutoUiState();
  renderAutoStatus();
  setAutoRounds(activeSessionId, 0);
  cancelAutoContinueTimer();
  // R-169:上限同步后端状态机。
  void syncAutoRunState();
  resetAutoRunState();
  log(`${t("鞭挞上限已设为")} ${max} ${t("轮")}`);
});
$("auto-continue").addEventListener("change", () => {
  if ($("auto-continue").checked && $("profile-select").value === "research") {
    // R-224:research 档位仍拒绝——研究模式无自主推进语义,自动切会掩盖误操作。
    $("auto-continue").checked = false;
    setAutoRounds(activeSessionId, 0);
    cancelAutoContinueTimer();
    rememberAutoUiState();
    void syncAutoRunState();
    toast(t("鞭挞不适用于研究模式"));
    log(t("鞭挞未开启:研究模式不支持自动续跑"));
    return;
  }
  if ($("auto-continue").checked && $("profile-select").value === "dev-pair") {
    // R-322 B2 取代 R-224 的强制切档。
    //
    // R-224 让结伴勾鞭挞自动切成 dev-auto,理由是「省去先切模式再勾两步」。但它的
    // 真实前提是**结伴档当时根本不能续跑**(auto_allowed 要求 agent=="dev"),
    // 所以那不是省两步,是「你要 loop 就得换掉人格」。现在结伴档能以轻控制续跑,
    // 前提消失:勾鞭挞就在结伴档里跑,人格不动,引擎不 Nudge、不插核查轮、不标冗余,
    // 模型说完成即停。想要重门禁的用户自己切 dev-auto——那是**显式**选择,不再被替选。
    addMessage("notice", t("结伴档鞭挞:轻控制续跑,引擎不追加推进指令,模型说完成即停"));
  }
  setAutoRounds(activeSessionId, 0);
  rememberAutoUiState();
  // 开鞭挞的这一刻就要看到「勘察复核未开」提示,而不是等下一轮结束才显示。
  renderAutoStatus();
  if (!$('auto-continue').checked) cancelAutoContinueTimer();
  // R-169:开关同步后端状态机(enabled)。
  void syncAutoRunState();
  log($("auto-continue").checked ? `${t("鞭挞已开启:每轮结束自动推进队列")} (${t("轮")} ${autoContinueMax()})` : t("鞭挞已关闭"));
  // BUG 修复(触发):空闲时勾上鞭挞必须立刻抽第一鞭——原来只挂在"上一轮结束"上,
  // 冷启动勾选后永远没有第一轮,必须手点"继续"才动。
  if ($("auto-continue").checked && !running && !autoPaused) {
    setStatus(t("鞭挞启动,2 秒后开始…"), false);
    scheduleAutoContinue();
  }
});
// 研究区(来源/发现/report)只在 research 档出现。dev 档下这两条文档线零写入方
// (提示词里根本没有 source/finding 工具)、零消费者,常驻侧栏就是两个永远的「(空)」:
// 占着位置,还让人以为功能坏了。语义处置留给 R-221 research 模式重定位,这里先按档位收起。
// 不挂进 syncAutoContinueWithProfile:那个函数中间有早退分支,挂进去会漏调。
function syncResearchSectionVisibility() {
  // R-322:门禁强度回显搭同一趟车。三处调用点(冷启动 / 进程回显 / 用户切换)
  // 一次覆盖全,不必再挂第四个监听——挂多了才是漏调的来源。
  // 放在早退之前:research-section 不存在时强度徽标仍要刷新。
  if (typeof renderHarnessIntensity === "function") renderHarnessIntensity();
  if (typeof renderGoalState === "function") renderGoalState();
  const section = $("research-section");
  if (!section) return;
  section.classList.toggle("hidden", $("profile-select")?.value !== "research");
  // R-276 批3:侧栏研究区与主视图工作台同一个档位判据,一处切换两处同步。
  if (typeof syncResearchWorkspaceVisibility === "function") syncResearchWorkspaceVisibility();
}
const PROFILE_STORAGE_KEY = "kz-profile";
const savedProfile = localStorage.getItem(PROFILE_STORAGE_KEY);
if (["dev-pair", "dev-auto", "research"].includes(savedProfile)) {
  $("profile-select").value = savedProfile;
}
// 冷启动也要对齐一次:HTML 里默认 hidden,存的档位若是 research 得把它放出来。
syncResearchSectionVisibility();
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
// R-264 B3：为 08-compose.js 的 ESM 测试 facade 提供线路级状态访问。
globalThis.__kzProcessAutoState = processAutoState;
// D-404:localStorage 可能重启即丢;后端 app.json 是权威。合并完成前 persist 只写
// localStorage(禁止用本地旧值先覆盖后端权威值),合并后双写并刷新当前线控件。
let uiPrefsAutoStateMerged = false;
void uiPrefsLoad().then((p) => {
  const saved = p.process_auto_state || {};
  for (const [k, v] of Object.entries(saved)) {
    if (v && typeof v === "object") processAutoState.set(k, v);
  }
  uiPrefsAutoStateMerged = true;
  if (activeProcessId && $("auto-continue")) applyAutoUiState(activeProcessId);
  else persistProcessAutoState();
});
function normalizeAutoState(value, _processId) {
  const storedMax = Number.parseInt(value?.maxRounds, 10);
  const legacyMax = Number.parseInt(localStorage.getItem("kz-auto-max"), 10);
  return {
    // 无记录 = 关。旧实现让默认线回落读全局 kz-auto-continue,于是 A 项目开鞭挞、
    // B 项目首次打开就继承为开,还随 applyAutoUiState 落盘固化成 B 的"用户选择"。
    // 鞭挞是否开启只能来自用户在**该线路**上的显式勾选。
    enabled: value?.enabled === true,
    paused: value?.paused === true,
    stopAfterRound: value?.stopAfterRound === true,
    maxRounds: Number.isFinite(storedMax)
      ? Math.min(100, Math.max(1, storedMax))
      : Number.isFinite(legacyMax) ? Math.min(100, Math.max(1, legacyMax)) : DEFAULT_AUTO_CONTINUE_MAX,
  };
}
function persistProcessAutoState() {
  writeJson(PROCESS_AUTO_STATE_KEY, Object.fromEntries(processAutoState));
  // D-404:后端 app.json 双写;权威值合并完成前禁止先写(避免本地旧值覆盖权威)。
  if (!uiPrefsAutoStateMerged) return;
  void uiPrefsSave({ process_auto_state: Object.fromEntries(processAutoState) });
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
// 引擎停机收口(AllBlocked/BacklogEmpty/ProfileMismatch/本轮后停)必须落在**停机会话
// 所属的线**上:kz:done 可能来自后台线甚至另一个项目的线,直接改当前可见勾选框
// 会把别的线的用户选择清掉——全局键时代「跨项目唯一状态」的另一半病根。
function applyAutoStopToSession(sessionId, patch) {
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  if (item) {
    const next = normalizeAutoState(processAutoState.get(item.id), item.id);
    Object.assign(next, patch);
    processAutoState.set(item.id, next);
    persistProcessAutoState();
  }
  if (!sessionId || sessionId === activeSessionId) {
    if (patch.enabled !== undefined) $("auto-continue").checked = patch.enabled;
    if (patch.stopAfterRound !== undefined) {
      autoStopAfterRound = patch.stopAfterRound;
      $("auto-stop-round").checked = patch.stopAfterRound;
    }
    void syncAutoRunState();
  } else {
    // 非当前线:后端状态机也要知道,否则该线下轮 done 仍按旧开关判定。
    void invoke("auto_state_update", {
      sessionId,
      enabled: patch.enabled,
      stopAfterRound: patch.stopAfterRound,
    });
  }
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
  // 轮次不能一律清零:切回一条**正在鞭挞**的线路时,它已经跑到第 7 轮了,显示
  // 0/10 会让人以为还能再跑十轮,实际下一轮就撞上限停机;进度条也一起回到 0%。
  // 真源是会话状态里的 auto_rounds(07-events.js 每轮都写),这里读回来即可。
  const target = processItems.find((item) => item.id === processId);
  setAutoRounds(target?.session_id, currentAutoRounds(target?.session_id));
  // 停机原因与一次性提示是**上一条线/上一个项目**留下的文本,跨线路跨项目串台会让
  // 用户按别人的停机理由去判断当前线。切换即清,由新线自己的事件重新写。
  autoHint = "";
  autoStopReason = "";
  renderAutoStatus();
  persistProcessAutoState();
}

// 线路页要能直接操控任意一条线的鞭挞；配置始终从 processAutoState 读取，顶栏 DOM 仅是投影。

function lineAutoConfig(processId) {
  return normalizeAutoState(processAutoState.get(processId), processId);
}
async function setLineAutoState(processId, patch) {
  const item = processItems.find((candidate) => candidate.id === processId);
  if (!item) return null;
  const next = { ...lineAutoConfig(processId), ...patch };
  // R-224 同价:研究线没有自主推进语义,从线路页开也一样拒绝。
  if (next.enabled && (processProfileUi.get(processId) === "research" || item.profile === "research")) {
    toast(t("鞭挞不适用于研究模式"));
    return null;
  }
  // R-322 B2:「结伴 + 勾着鞭挞」不再自相矛盾——它就是轻控制 loop 的正常形态,
  // 所以线路页开鞭挞也不再改写该线档位(原 R-224 同价逻辑一并去掉)。
  if (processId === activeProcessId) {
    $("auto-continue").checked = next.enabled;
    autoPaused = next.paused;
    autoStopAfterRound = next.stopAfterRound;
    $("auto-stop-round").checked = next.stopAfterRound;
    $("auto-max").value = String(next.maxRounds);
    $("auto-pause").classList.toggle("active", autoPaused);
    $("auto-pause").textContent = autoPaused ? t("继续鞭挞") : t("暂停鞭挞");
    rememberAutoUiState(processId);
    renderAutoStatus();
    await syncAutoRunState();
  } else {
    processAutoState.set(processId, next);
    persistProcessAutoState();
    await invoke("auto_state_update", {
      sessionId: item.session_id,
      enabled: next.enabled,
      paused: next.paused,
      stopAfterRound: next.stopAfterRound,
      maxRounds: next.maxRounds,
    });
  }
  // 关/暂停立刻撤掉在途的那一枪;开且该线空闲就当场抽第一鞭——不然「开了没反应」要等到
  // 下一个轮末才可见(顶栏勾选走的正是这条语义,线路页不能比它弱)。
  if (!next.enabled || next.paused) {
    cancelAutoContinueTimer(item.session_id);
    if (sessionState(item.session_id).phase === "auto_pending") transitionSession(item.session_id, "idle");
  } else if (!processRunning(item) && sessionState(item.session_id).phase !== "auto_pending") {
    armAutoContinue(processId === activeProcessId ? continuePrompt() : DEFAULT_CONTINUE_PROMPT, item.session_id);
  }
  refreshParallelTaskProjection(item.session_id);
  if (typeof renderLines === "function" && typeof collaborationLines !== "undefined" && collaborationLines.length) {
    renderLines(collaborationLines);
  }
  return next;
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
  syncResearchSectionVisibility();
}
$("profile-select").addEventListener("change", () => {
  const value = $("profile-select").value;
  syncResearchSectionVisibility();
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
$("work-priority-select").addEventListener("change", () => {
  const value = selectedWorkPriority();
  // 只写这一处。引擎读的就是它(run.rs normalize_work_priority → WorkPriority
  // → resolve_work_decision);不再镜像成 preference 记忆,理由见文件上方说明。
  localStorage.setItem(workPriorityStorageKey(), value);
  // D-404:同时写后端 app.json(每项目一份,全量 map)。
  void uiPrefsLoad().then((p) => {
    const wp = { ...(p.work_priority || {}), [currentProject || "default"]: value };
    void uiPrefsSave({ work_priority: wp });
  });
  log(localizeDynamic(value === "requirement-first" ? "已切换为需求优先" : "已切换为缺陷优先"));
});
$("stop").addEventListener("click", async () => {
  const targetSessionId = activeSessionId;
  const targetProcessId = activeProcessId;
  const targetProject = currentProject;
  cancelAutoContinueTimer(targetSessionId);
  setAutoRounds(activeSessionId, 0);
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
  armStoppingWatchdog(targetSessionId);
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


