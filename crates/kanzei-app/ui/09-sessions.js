let worktreeItems = [];
let worktreeLineCreateInFlight = false;
let worktreeLineCreateSequence = 0;
function renderWorktrees(items) {
  worktreeItems = items ?? [];
  const list = $("worktree-list");
  list.replaceChildren();
  if (!worktreeItems.length) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("暂无隔离工作树");
    list.appendChild(empty);
    return;
  }
  for (const item of worktreeItems) {
    const row = document.createElement("div");
    row.className = "worktree-entry";
    const label = document.createElement("div");
    label.textContent = `${item.branch} · ${item.clean ? t("干净") : `${item.files.length} ${t("项改动")}`}`;
    label.title = item.path;
    const actions = document.createElement("div");
    for (const [text, action] of [[t("差异"), "diff"], [t("收活"), "harvest"], [t("放弃"), "discard"]]) {
      const button = document.createElement("button");
      button.className = `ghost mini ${action === "harvest" ? "worktree-harvest" : action === "discard" ? "worktree-discard" : ""}`;
      button.textContent = text;
      const bound = processItems.find((process) => process.id === item.bound_process);
      if (action !== "diff" && bound && processRunning(bound)) {
        button.disabled = true;
        button.title = t("线路运行中，停止并等待收口后才能操作工作树");
      }
      button.addEventListener("click", () => handleWorktreeAction(item, action));
      actions.appendChild(button);
    }
    row.append(label, actions);
    list.appendChild(row);
  }
}
// R-177 内容③:清单真源是 `git worktree list --porcelain`(后端 worktree_list),
// 前端不再持有任何清单状态。原先存在 localStorage["kz-worktrees:*"] 里,于是三件事
// 都做不到:手工 `git worktree add` 出来的树看不见、换机器/清缓存后清单归零、而树
// 还在磁盘上。一次 IPC 拿全,也不用再逐条 worktree_diff。
//
// D-251 的性质**照旧守住**:projectDir 在 await **之前**认领,回来后再认一次,
// 替旧项目拉回来的清单不画进新项目的面板。
async function refreshWorktrees() {
  if (!currentProject) return renderWorktrees([]);
  const forProject = currentProject;
  let live = [];
  try { live = await invoke("worktree_list", { projectDir: forProject }); }
  catch (error) { log(`${t("工作树清单读取失败")}:${error}`, "warn"); }
  if (currentProject !== forProject) return;
  renderWorktrees(live);
}
// D-251:合并/放弃是一次真实 IPC,用户可能在它落地前切走。projectDir 必须在 await
// **之前**认领——按 currentProject 取就会拿新项目当参数去操作旧项目的工作树。
// 清单本身不再需要维护(真源在 git),末尾重刷一次即可。
async function handleWorktreeAction(item, action) {
  const forProject = currentProject;
  const active = processItems.find((process) => process.id === activeProcessId);
  const discardingActiveLine = action === "discard" && active?.worktree_path === item.path;
  try {
    if (action === "diff") {
      if (item.clean) {
        toast(t("工作树干净,没有未提交差异"));
      } else {
        const file_list = item.files.join("\n");
        const diff = item.diff?.trim() || t("未跟踪文件尚未包含在 git diff 中");
        log(`${item.branch}\n${t("文件列表")}:\n${file_list}\n\n${t("实际差异")}:\n${diff}`, "info");
        $("log-panel").classList.remove("hidden");
        toast(t("工作树差异已写入运行日志"));
      }
      return;
    }
    if (action === "harvest") {
      if (!item.bound_process) {
        toastError(t("该工作树没有绑定线路，不能进入收活流程"));
        return;
      }
      if (item.bound_process !== activeProcessId) await switchProcess(item.bound_process);
      document.querySelector('.activity-item[data-view="lines"]')?.click();
      await refreshLines();
      [...document.querySelectorAll("#lines-list .line-lane")]
        .find((lane) => lane.dataset.processId === item.bound_process)
        ?.querySelector(".line-harvest-toggle")?.click();
      return;
    }
    if (action === "discard" && !window.confirm(`${t("放弃工作树")} ${item.branch}？${t("未提交改动会阻止删除并保留现场")}`)) return;
    const result = await invoke("worktree_discard", { projectDir: forProject, worktreePath: item.path });
    if (String(result).length > 160) {
      log(String(result), "info");
      $("log-panel").classList.remove("hidden");
      toast(t("工作树操作完成，详细结果已写入运行日志"));
    } else {
      toast(result);
    }
    if (action === "discard") {
      // 放弃现在会在后端原子注销绑定进程。三份 UI 投影必须一起刷新；只刷新
      // git 工作树清单会把一个已不存在 cwd 的旧线路页签留在前端。
      await Promise.all([refreshProcesses(), refreshWorktrees(), refreshLines()]);
      if (currentProject !== forProject) return;
      if (discardingActiveLine && !processItems.some((process) => process.id === activeProcessId)) {
        const fallback = processItems.find((process) => process.id.startsWith("d|")) || processItems[0];
        if (fallback) await switchProcess(fallback.id);
      }
    } else {
      await refreshWorktrees();
    }
    refreshGit();
  } catch (error) {
    toastError(String(error), { retry: () => handleWorktreeAction(item, action) });
  }
}
// D-257:这一行曾被 7c5f022 抽 handleWorktreeAction 时的收尾 `}` 吃掉前半段,只剩
// `}("click", refreshWorktrees);` —— 语法合法、node --check 通过,按钮却全仓无监听器。
// 改动这一带时注意别再把它并进上面的函数尾行。
$("worktrees-refresh").addEventListener("click", refreshWorktrees);
async function createWorktreeLine() {
  if (!currentProject || worktreeLineCreateInFlight) return;
  // 同 handleWorktreeAction(D-251):projectDir 在 await 前认领。
  const forProject = currentProject;
  worktreeLineCreateInFlight = true;
  const addButton = $("worktree-add");
  addButton.disabled = true;
  const name = `line-${Date.now()}-${worktreeLineCreateSequence += 1}`;
  try {
    // 建线必须原子完成「建 worktree + 注册进程绑定」；只调用 worktree_create 会留下
    // 一棵没有会话身份的孤树，看得见却不能并行跑任务。
    const item = await invoke("process_create", {
      projectDir: forProject,
      worktreeName: name,
      phasePipeline: false,
      trackerWrites: false,
    });
    if (currentProject !== forProject) return;
    await Promise.all([refreshProcesses(), refreshWorktrees(), refreshLines()]);
    await switchProcess(item.id);
    toast(`${t("并行线路已创建")}:${item.branch || name}`);
  } catch (error) {
    toastError(`${t("创建并行线路失败")}:${error}`);
  } finally {
    worktreeLineCreateInFlight = false;
    addButton.disabled = false;
  }
}
$("worktree-add").addEventListener("click", createWorktreeLine);

// ---------- R-030:项目内独立进程 ----------
let syncedRunningProcessId = null;
let syncedRunningState = null;
// 已向后端补拉过待答队列的会话,防止每次进程列表刷新都打一次 pending_asks_get。
let askSyncedSession = null;
let processRefreshInFlight = null;
let processRefreshQueued = false;
let processSwitchGeneration = 0;
function processRunning(item) {
  const state = sessionState(item.session_id);
  // 终态事件已经收敛时,旧轮询里的 running=true 不能把线路重新点亮；
  // 未收敛时则合并事件缓存与后端快照,覆盖事件丢失/乱序的窗口。
  return state.converged ? state.running : state.running || Boolean(item.running);
}
async function closeParallelProcess(processId) {
  const item = processItems.find((candidate) => candidate.id === processId);
  if (!item || item.id.startsWith("d|")) return;
  const forProject = currentProject;
  const wasActive = processId === activeProcessId;
  const runningNow = processRunning(item);
  const warning = runningNow
    ? t("线路仍在运行，关闭会先停止并等待收口。")
    : t("关闭会注销线路身份。已合并且干净的工作树会自动回收；有独有内容的工作树会保留。");
  if (!window.confirm(`${t("关闭线路")} ${item.label} (${item.id})？\n${warning}`)) return;
  cancelAutoContinueTimer(item.session_id);
  if (runningNow) transitionSession(item.session_id, "stopping");
  try {
    const result = await invoke("process_close", { processId });
    if (currentProject !== forProject) return;
    if (wasActive) {
      activeProcessId = null;
      activeSessionId = null;
    }
    await Promise.all([refreshProcesses(), refreshWorktrees(), refreshLines()]);
    if (currentProject !== forProject) return;
    if (wasActive && activeProcessId) await switchProcess(activeProcessId, true);
    refreshGit();
    toast(result || `${t("已关闭线路")} ${item.id}`);
  } catch (error) {
    transitionSession(item.session_id, item.running ? "running" : "idle");
    toastError(`${t("关闭线路失败")}:${error}`);
  }
}
function renderParallelTaskStatus(items) {
  const target = $("parallel-task-status");
  const count = $("parallel-task-count");
  if (!target || !count) return;
  const processes = items ?? [];
  count.textContent = processes.length ? `${processes.length} ${t("条任务")}` : "";
  target.replaceChildren();
  if (!processes.length) return;
  for (const item of processes) {
    const state = sessionState(item.session_id);
    if (item.running && item.stage && (!state.stage || state.stage === "空闲")) state.stage = item.stage;
    const runningNow = processRunning(item);
    const pendingNow = state.phase === "auto_pending" || (state.auto_pending === true && !runningNow);
    const stoppingNow = state.phase === "stopping";
    const line = document.createElement("div");
    line.className = "parallel-line";
    line.dataset.processId = item.id;
    const row = document.createElement("button");
    row.type = "button";
    row.className = `parallel-task-row${item.id === activeProcessId ? " active" : ""}${runningNow ? " running" : ""}${pendingNow ? " auto-pending" : ""}`;
    row.dataset.processId = item.id;
    row.setAttribute("role", "listitem");
    const authority = item.authority === "primary" || item.id.startsWith("d|") ? t("主代理") : t("并行线");
    const head = document.createElement("span");
    head.className = "parallel-task-head";
    head.textContent = `${authority} · ${item.label}${item.branch ? ` · ${item.branch}` : ""}`;
    const status = document.createElement("span");
    status.className = "parallel-task-state";
    const stage = runningNow
      ? [state.stage, item.stage].find((value) => value && value !== "空闲") || t("运行中")
      : pendingNow ? t("等待下一轮") : t("空闲");
    const detail = runningNow && state.detail ? ` · ${state.detail}` : "";
    status.textContent = `${runningNow ? "●" : pendingNow ? "◐" : "○"} ${stoppingNow ? t("停止中…") : runningNow ? t("运行中") : pendingNow ? t("鞭挞等待") : t("空闲")} · ${stage}${detail}`;
    row.append(head, status);
    row.title = runningNow
      ? `${authority}: ${state.detail || stage}`
      : pendingNow
        ? `${authority}: ${t("等待下一轮")}`
      : `${authority}: ${t("点击切换到此线路")}`;
    row.addEventListener("click", () => void switchProcess(item.id));
    line.appendChild(row);
    if (!item.id.startsWith("d|")) {
      const close = document.createElement("button");
      close.type = "button";
      close.className = "ghost mini danger parallel-line-close";
      close.textContent = t("关闭");
      close.title = t("关闭线路");
      close.addEventListener("click", (event) => {
        event.stopPropagation();
        void closeParallelProcess(item.id);
      });
      line.appendChild(close);
    }
    const history = document.createElement("div");
    history.className = "parallel-line-history";
    history.dataset.processId = item.id;
    line.appendChild(history);
    target.appendChild(line);
    if (typeof renderLineConversationHistory === "function") renderLineConversationHistory(item.id);
    if (item.id === activeProcessId && pendingNow) {
      setRunPending(`${t("鞭挞")} · ${t("等待下一轮")}`);
    }
  }
}
function refreshParallelTaskProjection(sessionId) {
  if (!sessionId) return;
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  if (!item) return;
  const row = [...document.querySelectorAll(".parallel-task-row")]
    .find((candidate) => candidate.dataset.processId === item.id);
  if (!row) return;
  const state = sessionState(item.session_id);
  const runningNow = processRunning(item);
  const pendingNow = state.phase === "auto_pending" || (state.auto_pending === true && !runningNow);
  const stoppingNow = state.phase === "stopping";
  const authority = item.authority === "primary" || item.id.startsWith("d|") ? t("主代理") : t("并行线");
  const stage = runningNow
    ? [state.stage, item.stage].find((value) => value && value !== "空闲") || t("运行中")
    : pendingNow ? t("等待下一轮") : t("空闲");
  const detail = runningNow && state.detail ? ` · ${state.detail}` : "";
  row.classList.toggle("running", runningNow);
  row.classList.toggle("auto-pending", pendingNow);
  const status = row.querySelector(".parallel-task-state");
  if (status) status.textContent = `${runningNow ? "●" : pendingNow ? "◐" : "○"} ${stoppingNow ? t("停止中…") : runningNow ? t("运行中") : pendingNow ? t("鞭挞等待") : t("空闲")} · ${stage}${detail}`;
  row.title = runningNow
    ? `${authority}: ${state.detail || stage}`
    : pendingNow
      ? `${authority}: ${t("等待下一轮")}`
    : `${authority}: ${t("点击切换到此线路")}`;
  if (item.id === activeProcessId && pendingNow) {
    setRunPending(`${t("鞭挞")} · ${t("等待下一轮")}`);
  } else if (item.id === activeProcessId && stoppingNow) {
    setStopping(t("停止中…"));
  } else if (item.id === activeProcessId && running !== runningNow) {
    syncedRunningProcessId = item.id;
    syncedRunningState = runningNow;
    setRunning(runningNow, runningNow ? t("运行中") : t("空闲"));
  }
}
function renderProcesses(items) {
  const previousItems = processItems;
  const previousProcessKey = processItems.map((item) => item.id).join("\u0000");
  const previousProcessId = activeProcessId;
  processItems = items ?? [];
  const liveIds = new Set(processItems.map((item) => item.id));
  // 只清当前项目中确认已注销的线路；切项目时旧项目配置继续保留。身份虽已由后端
  // 保证永不复用，这里仍回收 timer/session/profile，避免长期运行积累死缓存。
  for (const removed of previousItems.filter((item) =>
    item.origin_project === currentProject && !liveIds.has(item.id))) {
    cancelAutoContinueTimer(removed.session_id);
    sessionStates.delete(removed.session_id);
    processAutoState.delete(removed.id);
    processProfileUi.delete(removed.id);
  }
  persistProcessAutoState();
  persistProcessProfiles();
  const nextProcessKey = processItems.map((item) => item.id).join("\u0000");
  const previousSessionId = activeSessionId;
  // R-086:后端是运行态权威,先把返回的 running 校正进各会话状态机(事件可能
  // 丢失),视图只投影活动会话的状态机,而不是直接信某一次轮询的瞬时值。
  // 已收敛终态(converged)的会话不被旧轮询值翻回——事件在轮询采样之后才发
  // 出是正常时序,此刻 process_list 里仍是 running=true,但会话实际已结束。
  for (const item of processItems) {
    const state = sessionState(item.session_id);
    if (state.converged) continue;
    // 实时事件是运行态的高优先级来源。一次在飞的 process_list 请求可能在
    // 后端刚启动 session 前采样到 false，不能覆盖 kz:turn/status/tool 形成的
    // live_running=true；同理，用户刚点击发送时的本地启动意图也要跨过这个窗口。
    if (state.live_running === true || (state.local_start_pending && !item.running)) {
      state.running = true;
      if (state.phase !== "stopping") state.phase = "running";
    } else if (state.live_running === false) {
      state.running = false;
    } else {
      state.running = Boolean(item.running);
      if (item.running) {
        state.phase = "running";
        state.live_running = true;
        state.local_start_pending = false;
      } else if (!["auto_pending", "stopping", "stopped", "failed"].includes(state.phase)) {
        state.phase = "idle";
      }
    }
  }
  if (!activeProcessId || !processItems.some((item) => item.id === activeProcessId)) {
    const preferred = processItems.find((item) => item.id.startsWith("d|")) || processItems[0];
    activeProcessId = preferred?.id ?? null;
  }
  const active = processItems.find((item) => item.id === activeProcessId);
  activeSessionId = active?.session_id ?? null;
  const activeProcessChanged = previousProcessId !== activeProcessId;
  if (activeProcessChanged && activeProcessId) {
    // 首次加载、切项目重建或活动线被回收后选 fallback 时，必须恢复目标线的
    // 完整设置；普通轮询不重复应用，避免覆盖用户刚修改的控件和鞭挞计数。
    applyAutoUiState(activeProcessId);
    applyProfileValue(active?.profile);
  }
  if (activeSessionId && activeSessionId !== previousSessionId) void syncAutoRunState();
  // R-086:活动会话换人(含首次拿到进程列表——界面重载后就是这条路)时向后端
  // 补拉一次待答队列。后端 asks 表活得比 webview 久,不补拉的话重载前挂起的
  // 权限询问再也不会出现,而后端还在 await 它的答复。按会话去重,只拉一次。
  if (activeSessionId && activeSessionId !== askSyncedSession) {
    askSyncedSession = activeSessionId;
    refreshPendingAsks();
  }
  pumpAsk();
  // 按「线路身份 + 线路运行态」同步运行栏。旧实现只在活动进程换人时同步，
  // 导致同一条线路从空闲变运行后 stop 仍隐藏，底部状态也一直停在空闲。
  // 状态机的本地停止复位仍然优先；下一次真实 process_list/事件投影会校正它。
  const activeRunning = active ? processRunning(active) : false;
  const activeState = active ? sessionState(active.session_id) : null;
  const activePending = active ? (activeState.phase === "auto_pending" || (activeState.auto_pending === true && !activeRunning)) : false;
  const activeStopping = activeState?.phase === "stopping";
  const activeTerminalStatus = active ? sessionState(active.session_id).terminal_status : "";
  if (
    !activePending && !activeStopping && (activeProcessId !== syncedRunningProcessId ||
    activeRunning !== syncedRunningState ||
    running !== activeRunning)
  ) {
    syncedRunningProcessId = activeProcessId;
    syncedRunningState = activeRunning;
    setRunning(activeRunning, activeRunning ? t("运行中") : activeTerminalStatus || t("空闲"));
  }
  if (activeStopping) setStopping(t("停止中…"));
  if (activePending) setRunPending(`${t("鞭挞")} · ${t("等待下一轮")}`);
  renderParallelTaskStatus(processItems);
  // 「勘察复核」= 阶段流水线总闸,默认关(后端 ProcessInfo.phase_pipeline 同默认)。
  $("process-phase-pipeline").checked = active?.phase_pipeline ?? false;
  // 分支线写主根 tracker 必须由用户显式打开；默认线直接写主根，不展示无意义开关。
  const trackerWrap = $("process-tracker-writes-wrap");
  const trackerToggle = $("process-tracker-writes");
  const isWorktreeLine = Boolean(active?.worktree_path);
  trackerWrap.classList.toggle("hidden", !isWorktreeLine);
  trackerToggle.disabled = !isWorktreeLine;
  trackerToggle.checked = isWorktreeLine && (active?.tracker_writes ?? false);
  // 鞭挞面板要跟着重算:自主推进开着而流水线关着时那里有一行提示。
  renderAutoStatus();
  if (previousProcessKey !== nextProcessKey && typeof refreshConversationLists === "function") {
    void refreshConversationLists();
  }
}

async function refreshProcesses() {
  if (!currentProject) return;
  if (processRefreshInFlight) {
    processRefreshQueued = true;
    return processRefreshInFlight;
  }
  const forProject = currentProject;
  processRefreshInFlight = (async () => {
    try {
      const items = await invoke("process_list", { projectDir: forProject });
      if (currentProject === forProject) renderProcesses(items);
    } catch (err) {
      if (currentProject === forProject) log(`${t("进程列表刷新失败")}:${err}`, "warn");
    } finally {
      processRefreshInFlight = null;
      if (processRefreshQueued) {
        processRefreshQueued = false;
        void refreshProcesses();
      }
    }
  })();
  return processRefreshInFlight;
}

async function refreshPendingAsks() {
  if (!currentProject || !activeSessionId) return;
  try {
    const pending = await invoke("pending_asks_get", {
      projectDir: currentProject,
      processId: activeProcessId,
    });
    const queue = askQueueFor(activeSessionId);
    const known = new Set(queue.map((item) => item.id));
    if (askActive?.sessionId === activeSessionId) known.add(askActive.id);
    for (const payload of pending || []) {
      if (!known.has(payload.id)) {
        queue.push(payload);
        known.add(payload.id);
      }
    }
    pumpAsk();
  } catch (err) {
    log(`${t("待处理权限询问恢复失败")}:${err}`, "warn");
  }
}


async function switchProcess(processId, forceReload = false) {
  if (processId === activeProcessId && !forceReload) return;
  const target = processItems.find((item) => item.id === processId);
  if (!target) return;
  const switchGeneration = ++processSwitchGeneration;
  const forProject = currentProject;
  const isCurrentSwitch = () =>
    switchGeneration === processSwitchGeneration && currentProject === forProject;
  // 后端只保存 dev/research;前端的 dev-auto 档位由 profile-select 的 change 事件
  // 在**用户改动时**绑定到当时的进程,这里不再重复写一次。
  //
  // D-290:原来这里拿 `$("profile-select").value` 当「旧进程的用户意图」写盘。选择器
  // 的值在回显期间是算出来的,不是用户选的——启动竞态里它可能还停在 dev-pair,于是
  // 一次切线就把存档里的 dev-auto 覆盖成 dev-pair,下次冷启动读回 dev-pair,再顺手
  // 关掉鞭挞……用户的表现就是「每次打开都要重新设模式和鞭挞」。回显不得写盘。
  if (activeProcessId) {
    rememberAutoUiState(activeProcessId);
  }
  hideAsk(true);
  activeProcessId = processId;
  activeSessionId = target.session_id;
  applyAutoUiState(activeProcessId);
  applyProfileValue(target.profile);
  void syncAutoRunState();
  // 下面有一次显式 await refreshPendingAsks(),先认领这个会话,免得 renderProcesses
  // 里的补拉守卫又打一次 pending_asks_get(结果会被 id 去重,只是白跑一趟)。
  askSyncedSession = target.session_id;
  pumpAsk();
  // R-086:运行态投影自该会话的状态机(后台终态已收敛),不直接信 processItems
  // 的瞬时轮询值——事件先到状态机,切回时看到的才是准的。
  if (sessionState(target.session_id).phase === "stopping") setStopping(t("停止中…"));
  else if (sessionState(target.session_id).phase === "auto_pending") setRunPending(`${t("鞭挞")} · ${t("等待下一轮")}`);
  else setRunning(processRunning(target), processRunning(target) ? t("运行中") : t("空闲"));
  renderProcesses(processItems);
  // 不在请求发出前清空消息:切线期间保留旧内容,等目标线程的历史完整恢复后
  // renderRecoveredMessages 一次性替换,避免慢请求/竞态把主对话显示成空白。
  bgClear();
  renderTodoPanel([], 0, 0);
  await loadConversation(null, switchGeneration);
  if (!isCurrentSwitch()) return;
  await refreshPendingAsks();
  if (!isCurrentSwitch()) return;
  await refreshDocs();
  if (!isCurrentSwitch()) return;
  await loadModels();
  if (!isCurrentSwitch()) return;
  // 模型下拉按进程回显:未设置覆盖时回到 agent 默认(空值),不保留上一个进程的选择。
  $("model-select").value = target.model || "";
  refreshGit();
  refreshPendingInputs();
  void refreshProcesses();
  log(`${t("已切换到进程")} ${target.label}`);
}

$("process-add").addEventListener("click", async () => {
  if (!currentProject) return;
  try {
    // 新进程与默认进程同一默认:勘察复核关(要显式打开才强制走七阶段)。
    const item = await invoke("process_create", { projectDir: currentProject, phasePipeline: false });
    await refreshProcesses();
    await switchProcess(item.id);
  } catch (err) {
    toastError(`${t("创建进程失败")}:${err}`);
  }
});

$("process-phase-pipeline").addEventListener("change", async (event) => {
  if (!activeProcessId) return;
  try {
    await queueProcessUpdate(activeProcessId, { phasePipeline: event.target.checked });
    updateLocalProcessItem(activeProcessId, { phase_pipeline: event.target.checked });
    await refreshProcesses();
    log(event.target.checked ? t("勘察复核已开启:每个任务强制走勘察→实现→复核") : t("勘察复核已关闭:恢复一问一答,模型仍可自己派子代理"));
  } catch (err) {
    event.target.checked = !event.target.checked;
    toastError(`${t("更新进程能力失败")}:${err}`);
  }
  renderAutoStatus();
});

$("process-tracker-writes").addEventListener("change", async (event) => {
  const active = processItems.find((item) => item.id === activeProcessId);
  if (!activeProcessId || !active?.worktree_path) return;
  try {
    await queueProcessUpdate(activeProcessId, { trackerWrites: event.target.checked });
    updateLocalProcessItem(activeProcessId, { tracker_writes: event.target.checked });
    await refreshProcesses();
    log(event.target.checked ? t("当前分支线已允许写主根追踪器") : t("当前分支线已恢复为只读主根追踪器"));
  } catch (err) {
    event.target.checked = !event.target.checked;
    toastError(`${t("更新追踪器写入权限失败")}:${err}`);
  }
});

// ---------- 项目管理 ----------
function baseName(path) {
  const parts = path.replaceAll("\\", "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

function syncDocumentsProjectSelect(prefs) {
  const select = $("documents-project-select");
  if (!select) return;
  select.replaceChildren();
  for (const path of prefs.projects ?? []) {
    select.appendChild(new Option(prefs.names?.[path] || baseName(path), path));
  }
  select.value = prefs.current ?? "";
  select.disabled = !(prefs.projects ?? []).length;
}

// D-170:所选目录若没有 .kanzei,后端会一路向上找,落到祖先目录上——
// 于是共用同一祖先的几个项目读的是同一份 requirements.md,需求在项目之间串。
// 存量项目改根会让会话 id 变化(历史看起来消失),所以不静默迁移:如实报出来,
// 给一键分离,由用户决定。
// 隔离问题往往一次影响多个项目(它们共用同一个祖先)。只在当前项目上提示会让
// 用户切一个发现一个,修到一半以为修完了。这里一次报全,只报一次。
let isolationReported = false;
async function reportIsolationAcrossProjects() {
  if (isolationReported) return;
  isolationReported = true;
  try {
    const report = await invoke("projects_isolation_report");
    for (const path of report.autoRepaired ?? []) {
      log(`${t("已为本项目建立独立空间")}:${path}`);
    }
    const shared = report.shared ?? [];
    if (shared.length) {
      log(
        `${t("以下项目仍与上级目录共用数据,切过去可一键分离")}:` +
          shared.map((s) => `${s.project} → ${s.resolved}`).join("；"),
        "warn",
      );
    }
  } catch {
    /* 体检失败不影响主流程 */
  }
}

async function checkProjectIsolation() {
  const box = $("project-shared-warn");
  if (!box || !currentProject) return;
  let info;
  try {
    info = await invoke("project_root_info", { projectDir: currentProject });
  } catch {
    return;
  }
  // 无损修复过就只留一行日志,不打扰——用户看到的内容没有任何变化。
  if (info.autoRepaired) log(`${t("已为本项目建立独立空间")}:${info.selected}`);
  box.classList.toggle("hidden", !info.shared);
  if (!info.shared) {
    // 顺带体检一次全部项目:受影响的往往不止当前这个,切一个发现一个太慢。
    reportIsolationAcrossProjects();
    return;
  }
  box.innerHTML = "";
  const text = document.createElement("div");
  text.textContent = `${t("本项目没有独立空间,正在使用上级目录的数据(与共用该上级的其它项目混在一起)")}:${info.resolved}`;
  const act = document.createElement("button");
  act.type = "button";
  act.className = "ghost mini";
  act.textContent = t("在此建立独立空间");
  act.title = t("只在本目录创建 .kanzei,不搬动上级目录的既有条目");
  act.addEventListener("click", async () => {
    try {
      await invoke("project_detach", { projectDir: currentProject });
      toast(t("已建立独立空间"));
      // 分离改变了项目根:文档、会话、记忆都要按新根重取,否则界面还停在旧根的数据上。
      await refreshDocs();
      await loadConversation();
      isolationReported = false; // 允许再体检一次,看还有没有别的项目共用
      checkProjectIsolation();
    } catch (err) {
      toastError(`${t("建立独立空间失败")}:${err}`);
    }
  });
  box.append(text, act);
}

function renderProjects(prefs) {
  const previousProject = currentProject;
  currentProject = prefs.current;
  syncWorkPriorityControl();
  // R-115:按项目记的偏好(模型/思考强度/筛选)要跟着项目切换回填,
  // 也覆盖了启动这一次——currentProject 在这里才第一次确定。
  restoreProjectPrefs();
  checkProjectIsolation();
  if (previousProject !== currentProject) {
    activeProcessId = null;
    activeSessionId = null;
  }
  const list = $("project-list");
  list.innerHTML = "";
  for (const path of prefs.projects) {
    const item = document.createElement("div");
    item.className = `project-item${path === prefs.current ? " active" : ""}`;
    item.setAttribute("role", "button");
    item.tabIndex = 0;
    item.setAttribute("aria-label", `${t("选择项目")} ${prefs.names?.[path] || baseName(path)}`);
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = prefs.names?.[path] || baseName(path);
    const pathEl = document.createElement("span");
    pathEl.className = "path";
    pathEl.textContent = path;
    const remove = document.createElement("button");
    remove.className = "icon-btn remove";
    remove.textContent = "×";
    remove.title = t("移除(不删除文件)");
    remove.setAttribute("aria-label", `${t("移除项目")} ${name.textContent}`);
    remove.addEventListener("click", async (e) => {
      e.stopPropagation();
      if (!window.confirm(`${t("移除项目")}“${name.textContent}”吗？${t("只解除登记,不会删除磁盘文件。")}`)) return;
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
        toastError(String(err));
      }
    });
    const rename = document.createElement("button");
    rename.className = "icon-btn rename";
    rename.textContent = "✎";
    rename.title = t("重命名项目(只修改显示名)");
    rename.setAttribute("aria-label", `${t("重命名项目")} ${name.textContent}`);
    rename.addEventListener("click", async (e) => {
      e.stopPropagation();
      const nextName = window.prompt(t("项目显示名"), prefs.names?.[path] || baseName(path));
      if (nextName === null || !nextName.trim()) return;
      try {
        renderProjects(await invoke("projects_rename", { path, name: nextName.trim() }));
      } catch (err) {
        toastError(String(err));
      }
    });
    item.append(name, pathEl, rename, remove);
    item.addEventListener("keydown", (e) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      item.click();
    });
    item.addEventListener("click", async () => {
      const previous = currentProject;
      renderProjects(await invoke("projects_select", { path }));
      if (previous && previous !== path) {
        // 运行状态属于会话:切项目后必须按目标项目重算,否则旧项目的 kz:done 被会话过滤器
        // 丢弃,新项目会永久卡在"运行中"(发送键禁用)。refreshProcesses 会带回真实状态。
        setRunning(false, t("空闲"));
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
  $("project-label").textContent = prefs.current ?? `(${localizeDynamic("未选择项目")})`;
  syncDocumentsProjectSelect(prefs);
  refreshProcesses();
}

$("project-init").addEventListener("click", async () => {
  const path = window.prompt(t("新项目目录路径(不存在时会创建)"));
  if (path === null || !path.trim()) return;
  const name = window.prompt(t("项目显示名(可留空)"), baseName(path.trim()));
  if (name === null) return;
  try {
    const prefs = await invoke("projects_init", {
      path: path.trim(),
      name: name.trim() || null,
    });
    renderProjects(prefs);
    clearChat(t("已初始化并切换到新项目"));
    await loadConversation();
    await refreshDocs();
    await loadModels();
    refreshGit();
    await refreshPendingInputs();
    toast(t("项目初始化完成"));
  } catch (err) {
    toastError(String(err));
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
    toastError(String(err));
  }
});

// ---------- 队列输入 ----------
function renderPendingInputs(items) {
  const list = $("queue-list");
  const count = $("queue-count");
  // 排队条挂在 composer(用户定调:排队输入放到排队按钮那里),空队列整条隐藏。
  $("composer-queue")?.classList.toggle("hidden", !items.length);
  list.innerHTML = "";
  count.textContent = items.length ? `(${items.length})` : "";
  if (!items.length) {
    const empty = document.createElement("div");
    empty.className = "queue-empty";
    empty.textContent = t("暂无排队输入");
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
    cancel.textContent = t("撤销");
    cancel.title = t("撤销这条排队输入");
    cancel.addEventListener("click", async () => {
      cancel.disabled = true;
      try {
        const changed = await invoke("cancel_input", {
          projectDir: currentProject,
          inputId: item.input_id,
          processId: activeProcessId,
        });
        if (changed) {
          toast(t("已撤销排队输入"));
          await refreshPendingInputs();
        }
      } catch (err) {
        cancel.disabled = false;
        toastError(`${t("撤销失败")}:${err}`);
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
    renderPendingInputs(await invoke("list_pending_inputs", {
      projectDir: currentProject,
      processId: activeProcessId,
    }));
  } catch (err) {
    log(`${t("队列刷新失败")}:${err}`, "warn");
  }
}

function renderTestRuns(snapshot) {
  const list = $("test-list");
  const records = [...(snapshot?.active ?? []), ...(snapshot?.archived ?? [])];
  list.replaceChildren();
  $("test-count").textContent = `${records.length}`;
  if (!records.length) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("暂无测试记录");
    list.appendChild(empty);
    return;
  }
  for (const record of records.slice().reverse()) {
    const row = document.createElement("div");
    row.className = `test-entry test-${record.status}`;
    row.textContent = `${record.status === "passed" ? "✓" : record.status === "failed" ? "×" : record.status === "running" ? "●" : "○"} ${record.id} ${record.title}`;
    row.title = (record.fields ?? []).map((field) => `${field.key}: ${field.value}`).join("\n");
    // R-130:测试→条目映射可见——关联的 R-/D- 条目号渲染成可点跳转的徽标,
    // 让「这条测试为哪个条目背书」一眼可见,点一下直接跳到该条目。
    const refs = record.refs ?? [];
    if (refs.length) {
      const refRow = document.createElement("div");
      refRow.className = "test-entry-refs";
      for (const refId of refs) {
        const chip = document.createElement("button");
        chip.type = "button";
        chip.className = "test-ref-chip";
        chip.textContent = refId;
        chip.title = `${t("跳转到")} ${refId}`;
        chip.addEventListener("click", () => jumpToEntry(refId));
        refRow.appendChild(chip);
      }
      row.appendChild(refRow);
    }
    list.appendChild(row);
  }
}

async function refreshTests() {
  if (!currentProject) {
    renderTestRuns({ active: [], archived: [] });
    return;
  }
  try {
    // R-130 验收③:批量导入/初始化有真实消费者——每次刷新前把旧记录里标题含
    // R-/D- 条目号的补写「关联」字段(幂等:已结构化的不动,无变化不写盘),
    // 再取快照渲染。旧记录因此也能在列表里带出可跳转的关联徽标。
    await invoke("test_runs_init_refs", { projectDir: currentProject });
    renderTestRuns(await invoke("test_runs_snapshot", { projectDir: currentProject }));
  } catch (error) {
    log(`${t("测试记录刷新失败")}:${error}`, "warn");
  }
}

$("tests-refresh").addEventListener("click", refreshTests);
