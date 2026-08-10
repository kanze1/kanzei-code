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
  // 新一轮 run 开跑:上一轮的「在做」运行证据作废,重新从本轮动作里取。
  if (p.step === 1) clearRuntimeFocus();
  if (p.step > 1) {
    clearEmptyState();
    // 轮次分隔不再进主对话区(用户定调:对话为主);轮次在侧边栏"当前进展"实时可见。
  }
  // 活动面板跨轮保留历史,由用户主动清空/切换项目时清理。
  currentAssistant = null;
  currentReasoning = null;
  currentReasoningHead = null;
  const roundLabel = languageIsEnglish() ? `Round ${p.step}${p.maxSteps > 0 ? `/${p.maxSteps}` : ""}` : p.maxSteps > 0 ? `第 ${p.step}/${p.maxSteps} 轮` : `第 ${p.step} 轮`;
  liveTurn(roundLabel);
  if (running) setStatus(`${roundLabel} · ${t("等待模型")}`, true);
});
on("kz:text", (e) => {
  markFirstSignal();
  // 文本开始后,后续思考属于新的思考段。
  currentReasoning = null;
  currentReasoningHead = null;
  if (running) setStatus("生成中" + ` · ${(outputChars / 1000).toFixed(1)}k`, true);
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
  const title = document.createElement("button");
  title.type = "button";
  title.className = "bg-title";
  title.setAttribute("aria-label", t("展开或收起上下文压缩纪要"));
  title.setAttribute("aria-expanded", "true");
  title.textContent = t("上下文压缩 · 点击查看纪要");
  const detail = document.createElement("div");
  detail.className = "bg-detail";
  detail.textContent = summary;
  el.append(title, detail);
  title.addEventListener("click", () => {
    detail.classList.toggle("hidden");
    title.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
  });
  $("bg-list").appendChild(el);
  while ($("bg-list").childElementCount > BG_MAX) $("bg-list").firstElementChild.remove();
  lastCompactionEntry = el;
}

function addSummaryEntry(summary, path = "") {
  const el = document.createElement("div");
  el.className = "bg-entry ok summary-entry";
  const title = document.createElement("button");
  title.type = "button";
  title.className = "bg-title";
  title.setAttribute("aria-label", t("展开或收起对话总结"));
  title.setAttribute("aria-expanded", "true");
  title.textContent = t("对话小总结 · 点击查看");
  const detail = document.createElement("div");
  detail.className = "bg-detail";
  detail.textContent = path ? `${summary}\n\n${t("已存档")}: ${path}` : summary;
  el.append(title, detail);
  title.addEventListener("click", () => {
    detail.classList.toggle("hidden");
    title.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
  });
  $("bg-list").appendChild(el);
  while ($("bg-list").childElementCount > BG_MAX) $("bg-list").firstElementChild.remove();
  return el;
}
function renderContextDetail() {
  const detail = $("context-detail");
  const t = runTokens;
  const total = t.input + t.cacheRead + t.output;
  detail.innerHTML = `<strong>${localizeDynamic("上下文成分")}</strong><br>${localizeDynamic("输入上下文(系统/历史/工具结果)")}: ${t.input.toLocaleString()} tokens<br>${localizeDynamic("缓存读取(已复用上下文)")}: ${t.cacheRead.toLocaleString()} tokens<br>${localizeDynamic("本轮输出")}: ${t.output.toLocaleString()} tokens${lastCompactionSummary ? `<br>${localizeDynamic("最近一次压缩纪要已收进活动面板")}` : ""}<br>${localizeDynamic("合计")}: ${total.toLocaleString()} tokens`;
  detail.classList.remove("hidden");
  $("status-tokens").setAttribute("aria-expanded", "true");
  if (lastCompactionEntry) {
    activityPanelOpen = true;
    syncActivityPanel();
    lastCompactionEntry.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }
}

function hideContextDetail() {
  $("context-detail").classList.add("hidden");
  $("status-tokens").setAttribute("aria-expanded", "false");
}
function toggleContextDetail() {
  if ($("context-detail").classList.contains("hidden")) renderContextDetail();
  else hideContextDetail();
}

$("status-tokens").title = t("点击查看上下文成分");
$("status-tokens").classList.add("context-clickable");
$("status-tokens").addEventListener("click", toggleContextDetail);
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") hideContextDetail();
});
document.addEventListener("click", (event) => {
  if (event.target.closest("#status-tokens, #context-detail")) return;
  hideContextDetail();
});
on("kz:tool-start", (e) => {
  markFirstSignal();
  // 后端 summarize_input 把整坨入参 JSON 截到 160 字,对所有工具一视同仁——直接拼进
  // 运行日志与「当前动作」行,edit/write 就显示成 `{"new_string":"…","old_strin…`。
  // 与活动栏标题、主对话工具块用同一个 toolCallSummary 挑字段,挑不出来再回落后端摘要。
  // 顺带修掉 summary 缺省时 `.slice()` 直接抛的隐患(事件里 summary 并非必填)。
  const shown = toolCallSummary(e.payload.name, e.payload.input) || String(e.payload.summary ?? "");
  log(`${t("工具")} ${e.payload.name} ${shown}`);
  currentAssistant = null;
  currentReasoning = null;
  chatToolStart(e.payload.id, e.payload.name, e.payload.summary, e.payload.input);
  // R-168:非终端工具先挂起,成功不进活动流，失败在 tool-end 补建。
  // R-173:入参一并传下去——编排派发的勘察/复核子代理靠 input.phase 才认得出来,
  // 只看 name(恒为 "task")会连同它们一起静默,内部进度全丢。
  if (bgQuiet(e.payload.name, e.payload.input)) bgStartQuiet(e.payload.id, e.payload.name, e.payload.summary, e.payload.input);
  else if (isActivityTool(e.payload.name, e.payload.input)) bgAdd(e.payload.id, e.payload.name, e.payload.summary, e.payload.input);
  // R-174:子代理面板独立于活动面板,task 类一律进子代理面板(编排派发带 phase,
  // 模型自派 name=task)。其余工具不进子代理面板。
  if (e.payload.name === "task") agentStart(e.payload.id, e.payload.name, e.payload.summary, e.payload.input);
  liveSet("live-action", `⚙ ${e.payload.name} ${shown.slice(0, 60)}`);
  setStatus(`${t("工具执行中")} · ${e.payload.name}`, true);
});
function isBatchCommit(event) {
  return event?.name === "git"
    && event.ok
    && /^committed verified staged set\b/.test(event.preview || "");
}
// 从工具结果文本里抽条目 ID(R-xxx/D-xxx):tracker 更新与批次提交标题都以它开头,
// 这是「agent 实际在做谁」的运行证据(D-207 三修),喂给 setRuntimeFocus。
function workItemIdFrom(text) {
  return String(text ?? "").match(/\b([RD]-\d{1,4})\b/)?.[1] ?? null;
}

// 工具执行中的增量输出(bash 等):活动面板对应条目实时追加。
on("kz:tool-progress", (e) => {
  bgStream(e.payload.id, e.payload.chunk);
});
on("kz:task-progress", (e) => {
  const payload = e.payload;
  bgProgress(payload.id, payload.text, payload.trace);
  // R-174:子代理面板同一数据流。trace 里带 input/usage 时是 transcript 与 token 数据源。
  agentProgress(payload.id, payload.text, payload.trace);
  // 子代理不会单独发顶层 tool-end；它每提交一个批次时由 task-progress 带回。
  // 这里马上重新取 Git 推导的进度，不等 parent task 或整轮结束。
  // 「在做」运行证据③:子代理的批次提交同样指认实际在推的条目。
  if (isBatchCommit(payload.trace)) {
    setRuntimeFocus(workItemIdFrom(payload.trace?.preview));
    refreshDocsSoon();
  }
});
on("kz:tool-end", (e) => {
  const p = e.payload;
  log(`${t("工具结果")} ${p.name}: ${p.ok ? t("成功") : t("失败")} — ${p.preview}`, p.ok ? "" : "warn");
  // 工作焦点:req/defect/goal 的增改结果最能代表"它在干哪件事"。
  if (p.ok && ["req", "defect", "goal"].includes(p.name)) {
    liveSet("live-focus", `◉ ${p.preview.replace(/^(updated|added):?\s*/, "").slice(0, 60)}`);
    // 「在做」运行证据①:update 型 tracker 结果(取活时标 doing/fixing、批次进展
    // 都走这里)。add(快记新增)与 close(刚收尾)不指向正在做的条目,不采。
    if (["req", "defect"].includes(p.name) && /^updated:/.test(p.preview)) {
      setRuntimeFocus(workItemIdFrom(p.preview));
    }
    // 文档已经变了,侧栏列表与状态按钮跟着刷新,不等本轮结束。
    refreshDocsSoon();
  }
  // Git 提交标题是批次进度的真源，成功提交后立即重拉文档快照。
  // 「在做」运行证据②:批次提交标题以条目 ID 开头,是最强的"实际在推谁"信号。
  if (isBatchCommit(p)) {
    setRuntimeFocus(workItemIdFrom(p.preview));
    refreshDocsSoon();
  }
  // 测试记录同理:跑完测试后左侧应立即出现结果。
  if (p.ok && ["source", "finding"].includes(p.name)) refreshDocsSoon();
  // 改了文件或跑了命令,工作区状态徽章跟着变(提交后 +N 应当立刻归零)。
  if (p.ok && ["write", "edit", "multiedit", "bash"].includes(p.name)) refreshGitSoon();
  if (p.display?.kind === "todo") {
    renderTodoPanel(p.display.items || [], p.display.done || 0, p.display.total || 0);
  }
  chatToolEnd(p.id, p.ok, p.preview, p.display);
  recordDiffSummary(p.display);
  // R-174:子代理终态进子代理面板 finished 区(task 类顶层 tool-end 只来自父任务收尾,
  // 或被停后补发)。
  if (p.name === "task") agentEnd(p.id, p.ok, p.preview, p.display);
  // 静默工具成功→无声丢弃;失败→bgFinishQuiet 已补建条目,bgEnd 接着画错误详情。
  bgFinishQuiet(p.id, p.ok);
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
  log(`${t("一轮完成")}:in ${p.input} (cache r${p.cacheRead}) · out ${p.output} · ctx ${(ctxTokens / 1000).toFixed(1)}k`);
});
on("kz:error", (e) => {
  cancelAutoContinueTimer();
  const message = e.payload.message;
  reportError(message);
  stopElapsed();
  setRunning(false, "出错");
  bgAbortRunning(`(${localizeDynamic("出错中止")})`);
  liveIdle("出错");
  notifyRunState("failed", message);
  $("log-panel").classList.remove("hidden");
  refreshProcesses();
});
// 流中途断开后重放本轮:后端会把本轮从头重新生成,已渲染的残缺输出必须丢掉,
// 否则重放出的文本会接在半截内容后面变成重复段落。本轮工具尚未执行,无副作用。
on("kz:stream-restart", (e) => {
  const p = e.payload ?? {};
  if (currentAssistant) {
    currentAssistant.remove();
    currentAssistant = null;
  }
  currentReasoning = null;
  currentReasoningHead = null;
  outputChars = 0;
  addMessage("notice", `⟳ ${localizeDynamic("连接中断,正在重新请求本轮")}(${p.attempt}/${p.max})`);
  log(`${localizeDynamic("连接中断,重放本轮")} ${p.attempt}/${p.max},${localizeDynamic("等待")} ${p.delayMs}ms`, "warn");
  setStatus(`${t("连接中断")} · ${t("重放本轮")} ${p.attempt}/${p.max}`, true);
});
on("kz:compacted", (e) => {
  lastCompactionSummary = e.payload?.summary ?? "";
  addMessage("notice", `🗜 ${t("上下文占用过高,已自动压缩为纪要并延续对话")}`);
  if (lastCompactionSummary) addCompactionEntry(lastCompactionSummary);
  log(t("自动压缩完成:多轮历史已替换为纪要"));
  ctxTokens = 0;
  renderTokens();
});
on("kz:stopped", (e) => {
  cancelAutoContinueTimer();
  hideAsk();
  const cancelled = e.payload?.cancelled_queue ?? 0;
  addMessage("notice", cancelled > 0 ? `${t("已停止")}, ${t("已取消")} ${cancelled} ${t("条")} ${t("排队输入")}` : t("已停止"));
  log(cancelled > 0 ? `${t("已手动停止并取消")} ${cancelled} ${t("条")} ${t("排队输入")}` : t("已手动停止"));
  stopElapsed();
  setRunning(false, "已停止");
  bgAbortRunning(`(${t("已停止")})`);
  liveIdle("已停止");
  notifyRunState("stopped", cancelled > 0 ? `${t("已停止")}, ${t("已取消")} ${cancelled} ${t("条")} ${t("排队输入")}` : t("已停止"));
  refreshPendingInputs();
  refreshProcesses();
});
// R-086:会话真正转空闲(后端 run loop 退出,排队输入已跑完或失败中断)。
// 视图的收尾归 kz:done/kz:error/kz:stopped,这里只把标签页按状态机重画一次——
// 订阅本身也是必需的:on() 里的会话状态机收敛逻辑挂在订阅回调上。
on("kz:idle", () => {
  renderProcesses(processItems);
});

on("kz:done", async (e) => {
  const p = e.payload;
  setAutoStopReason(p.halted ? t("用户拒绝后停止") : t("本轮完成"));

  addMessage(
    "notice",
    `${t("完成")} · steps ${p.steps}${p.history ? ` · 会话 ${p.history} 条` : ""}${p.halted ? ` · ${t("按你的拒绝停止")}` : ""}`
  );
  log(`${t("运行完成")}: ${p.steps} ${t("轮")}, ${t("耗时")} ${((Date.now() - runStart) / 1000).toFixed(1)}s`);
  stopElapsed();
  notifyRunState(p.halted ? "stopped" : "completed", p.halted ? t("按你的拒绝停止") : `${t("完成")} ${p.steps} ${t("轮")}`);
  setRunning(false);
  // 对齐 Claude:当前对话跑完一轮就出现在历史列表里,不用等重启/切项目。
  refreshConversationList();
  // 活动面板保留本轮全部轨迹供回看,下一轮开跑时才翻页(kz:turn step 1)。
  liveIdle(`${t("空闲")} · ${t("上轮")} ${p.steps} ${t("轮")} ${t("完成")}`);
  refreshDocs();
  refreshGit();
  refreshPendingInputs();

  // R-169:鞭挞判定已引擎化(harness auto_run 状态机)——kz:done 携带 autoAction,
  // 前端只执行:Continue→续跑;Nudge→发引擎生成的推进指令;Stop→停+显示原因;
  // NoContinue→不动作(用户拒绝/未开启)。前端不再做任何机械判定
  // (空转画像/连数/全部阻塞/无动作 NUDGE 全部在后端,见 harness auto_run.rs)。
  const action = p.autoAction || { type: "NoContinue" };
  if (action.type === "Continue") {
    autoRounds = action.rounds ?? autoRounds + 1;
    const max = action.max ?? autoContinueMax();
    setStatus(`${t("自主推进")} ${autoRounds}/${max} · 2 ${t("秒后继续")}…`, false);
    renderAutoStatus(`${t("自主推进")} ${autoRounds}/${max} · ${t("等待下一轮")}`);
    scheduleAutoContinue();
  } else if (action.type === "Nudge") {
    autoRounds = action.rounds ?? autoRounds + 1;
    const max = action.max ?? autoContinueMax();
    addMessage("notice", t("上一轮没有实质动作,已追加一次具体推进指令(再无动作才会停)"));
    log(`${t("鞭挞")}:${t("无动作 · 追加推进指令")}`);
    renderAutoStatus(`${t("无动作 · 追加推进指令")} ${autoRounds}/${max}`);
    cancelAutoContinueTimer();
    const generation = autoContinueGeneration;
    autoContinueTimer = setTimeout(() => {
      autoContinueTimer = null;
      if (generation !== autoContinueGeneration || autoPaused || autoStopAfterRound) return;
      if ($("auto-continue").checked && autoContinueAllowed() && !running) {
        sendText(action.prompt, { auto: true });
      }
    }, 2000);
  } else if (action.type === "Stop") {
    autoRounds = 0;
    noActionRounds = 0;
    cancelAutoContinueTimer();
    const reason = action.reason;
    if (reason === "Paused") {
      addMessage("notice", `${t("鞭挞停止")}: ${t("处于暂停中,点顶栏「继续鞭挞」恢复")}`);
      setAutoStopReason("已暂停");
    } else if (reason === "StopAfterRound") {
      $("auto-stop-round").checked = false;
      autoStopAfterRound = false;
      void syncAutoRunState();
      addMessage("notice", `${t("鞭挞停止")}:${t("本轮后停")}(${t("已自动取消勾选,再点鞭挞即可继续")})`);
      log(`${t("鞭挞停止")}:${t("本轮后停")}`);
      setAutoStopReason(`${t("本轮后停")},${t("已停止")}`);
    } else if (reason === "MaxRounds") {
      addMessage("notice", `${t("鞭挞停止")}:${t("已达连上限,点继续或重开鞭挞")} (${action.max ?? autoContinueMax()})`);
      setAutoStopReason(`${t("鞭挞停止")}:${t("已达连上限,点继续或重开鞭挞")}`);
    } else if (reason === "NoAction") {
      addMessage("notice", `${t("鞭挞停止")}:${t("连续两轮没有实质动作(可能目标已达成或确实无可推进项)")}`);
      log(`${t("鞭挞停止")}:${t("连续两轮无动作,鞭挞停止")}`);
      setAutoStopReason(t("连续两轮无动作,鞭挞停止"));
    } else if (reason === "AllBlocked") {
      $("auto-continue").checked = false;
      localStorage.setItem("kz-auto-continue", "0");
      void syncAutoRunState();
      const msg = t("需求与缺陷全部被阻塞，自动推进已停止");
      setAutoStopReason(msg);
      addMessage("notice", `✅ ${msg}`);
      log(t("自动推进停止:需求与缺陷全部被阻塞"));
    } else if (reason === "BacklogEmpty") {
      $("auto-continue").checked = false;
      localStorage.setItem("kz-auto-continue", "0");
      void syncAutoRunState();
      const msg = t("需求与缺陷已清空，自动推进已停止");
      setAutoStopReason(msg);
      addMessage("notice", `✅ ${msg}`);
      log(t("自动推进停止:需求与缺陷已清空"));
    }
  }
  // NoContinue:用户拒绝/未开启——不续跑不重置,等手动输入重新武装。
});

// ---------- 权限弹窗 ----------
const askQueues = new Map();
let askActive = null;

function askSessionId(payload) {
  return payload?.sessionId || activeSessionId || "__default__";
}

function askQueueFor(sessionId) {
  let queue = askQueues.get(sessionId);
  if (!queue) {
    queue = [];
    askQueues.set(sessionId, queue);
  }
  return queue;
}

on("kz:ask", (e) => {
  const sessionId = askSessionId(e.payload);
  e.payload.sessionId = sessionId;
  // 自动放行(yolo):后台会话也必须直接得到答复,不能因不在当前页签而挂起。
  if (e.payload.kind !== "question" && $("auto-allow").checked) {
    log(`${t("自动放行")}:${e.payload.action} ${e.payload.resource}`);
    invoke("answer_ask", { id: e.payload.id, reply: "once" }).catch((err) =>
      reportPersistentError(`${t("自动放行失败")}:${err}`)
    );
    return;
  }
  askQueueFor(sessionId).push(e.payload);
  if (sessionId === activeSessionId) pumpAsk();
});

$("auto-allow").checked = localStorage.getItem("kz-auto-allow") === "1";
$("auto-allow").addEventListener("change", () => {
  localStorage.setItem("kz-auto-allow", $("auto-allow").checked ? "1" : "0");
  log($("auto-allow").checked ? t("已开启自动放行(本会话所有权限询问直接通过)") : t("已关闭自动放行"));
});

function updateAskQueueStatus() {
  const queue = activeSessionId ? askQueueFor(activeSessionId) : [];
  const total = (askActive ? 1 : 0) + queue.length;
  const status = $("ask-queue-status");
  const preview = $("ask-queue-preview");
  status.textContent = total > 1
    ? `${t("当前请求")} 1/${total} · ${languageIsEnglish() ? `${total - 1} ${t("条待处理")}` : `${t("还有")} ${total - 1} ${t("条待处理")}`}`
    : t("当前无其他待处理请求");
  const lines = queue.slice(0, 4).map((item, index) => {
    const text = item.kind === "question" ? item.question : `${item.action} · ${item.resource}`;
    return `${index + 2}. ${text}`;
  });
  preview.textContent = lines.join("\n");
  preview.classList.toggle("hidden", lines.length === 0);
}

function pumpAsk() {
  if (askActive || !activeSessionId) {
    updateAskQueueStatus();
    return;
  }
  const queue = askQueueFor(activeSessionId);
  if (queue.length === 0) {
    updateAskQueueStatus();
    return;
  }
  askActive = queue.shift();
  const question = askActive.kind === "question";
  $("ask-title").textContent = question ? t("需要你的回答") : t("权限请求");
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
    setTimeout(() => $("ask-allow").focus(), 0);
  }
  $("ask-overlay").classList.remove("hidden");
  updateAskQueueStatus();
}

function hideAsk(preserveActive = false) {
  if (preserveActive && askActive) {
    askQueueFor(askActive.sessionId).unshift(askActive);
  } else if (activeSessionId) {
    askQueueFor(activeSessionId).length = 0;
  }
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
  const replyLabel = reply === "deny" ? t("拒绝") : reply === "always" ? t("总是允许") : reply;
  log(`${question ? t("回答") : t("权限")} ${replyLabel} — ${summary}`);
  try {
    await invoke("answer_ask", { id, reply });
  } catch (err) {
    reportPersistentError(`${t("权限应答失败")}:${err}`);
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
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (!$('ask-overlay').classList.contains("hidden") && askActive) {
    answerAsk(askActive.kind === "question" ? "cancel" : "deny");
    return;
  }
  if (!$('viewer-overlay').classList.contains("hidden")) $("viewer-close").click();
});

// ---------- 阅读辅助 ----------
async function copyReadable(el) {
  const text = el.dataset.raw || [...el.childNodes]
    .filter((node) => !(node.nodeType === Node.ELEMENT_NODE && node.classList.contains("msg-actions")))
    .map((node) => node.textContent || "")
    .join("")
    .trim();
  if (!text) return toast(t("没有可复制的内容"));
  try {
    await navigator.clipboard.writeText(text);
    toast(t("已复制"));
  } catch (err) {
    toastError(`${t("复制失败")}:${err}`);
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
      if (text) parts.push(`## ${t("用户")}\n${text}`);
    } else if (el.classList.contains("assistant")) {
      const raw = (el.dataset.raw ?? el.textContent).trim();
      if (raw) parts.push(`## ${t("助手")}\n${raw}`);
    } else if (el.classList.contains("reasoning")) {
      const raw = el.querySelector(".reasoning-body")?.dataset.raw?.trim();
      if (raw) parts.push(`> ${t("思考")}:${raw.split("\n").find(Boolean)?.slice(0, 160) ?? ""}`);
    } else if (el.classList.contains("tool-chip")) {
      const head = el.querySelector(".head")?.textContent?.trim();
      if (head) parts.push(`> ${t("工具")}:${head.slice(0, 200)}`);
    } else if (el.classList.contains("turn-divider")) {
      parts.push(`---\n${el.textContent}`);
    }
  }
  if (!parts.length) {
    toast(t("当前没有可复制的对话"));
    return;
  }
  try {
    await navigator.clipboard.writeText(parts.join("\n\n"));
    toast(`${t("已复制上下文")}(${parts.length} ${t("段")})`);
  } catch (err) {
    toastError(`${t("复制上下文失败")}:${err}`);
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

