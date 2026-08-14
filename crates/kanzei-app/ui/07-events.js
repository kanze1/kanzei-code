// ---------- 事件订阅 ----------
on("kz:status", (e) => {
  const p = e.payload;
  log(`[${p.stage}] ${p.detail}`);
  // 状态事件按 sessionId 先进入会话状态机,后台线路也要能在侧栏逐条显示阶段。
  if (p.sessionId) renderParallelTaskStatus(processItems);
  if (running) setStatus(`${p.stage} · ${p.detail}`, true);
});
on("kz:meta", (e) => {
  $("status-model").textContent = `${e.payload.model} · ${e.payload.profile}`;
  ctxLimit = e.payload.contextLimit ?? null;
  log(`模型 ${e.payload.model} · agent ${e.payload.agent} · profile ${e.payload.profile}${ctxLimit ? ` · 上下文上限 ${Math.round(ctxLimit / 1000)}k` : ""}`);
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
let todoPanelUserClosed = false;
function renderTodoPanel(items, done, total) {
  todoItems = items || [];
  const panel = $("todo-panel");
  const list = $("todo-list");
  list.innerHTML = "";
  // 计划清空(新会话/历史重放)后复位手动关闭标志,下一轮新计划仍可自动弹出;
  // 用户主动关闭后,后续同轮的工具事件重渲染不得再把面板弹回来。
  if (todoItems.length === 0) todoPanelUserClosed = false;
  panel.classList.toggle("hidden", todoItems.length === 0 || todoPanelUserClosed);
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
// D-350:当前计划面板手动关闭入口。todo-close 点击后隐藏面板并置手动关闭标志,
// 后续 renderTodoPanel(工具事件重渲染)不再自动弹回;计划清空后标志复位。
$("todo-close").addEventListener("click", () => {
  todoPanelUserClosed = true;
  $("todo-panel").classList.add("hidden");
});

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
  // 压缩/总结这类通知也落「运行中」段。原来直接挂 #bg-list 并按 firstElementChild
  // 裁剪——分段之后那会把段骨架当成最老的条目摘掉。裁剪只在本段内按 .bg-entry 走。
  const notices = $("bg-running") ?? $("bg-list");
  notices.appendChild(el);
  while (notices.querySelectorAll(".bg-entry").length > BG_MAX) notices.querySelector(".bg-entry").remove();
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
  // 压缩/总结这类通知也落「运行中」段。原来直接挂 #bg-list 并按 firstElementChild
  // 裁剪——分段之后那会把段骨架当成最老的条目摘掉。裁剪只在本段内按 .bg-entry 走。
  const notices = $("bg-running") ?? $("bg-list");
  notices.appendChild(el);
  while (notices.querySelectorAll(".bg-entry").length > BG_MAX) notices.querySelector(".bg-entry").remove();
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
  // 活动面板保留完整工具轨迹,入参一并传下去以支持编排阶段、角色和完整详情。
  if (bgQuiet(e.payload.name, e.payload.input)) bgStartQuiet(e.payload.id, e.payload.name, e.payload.summary, e.payload.input);
  else if (isActivityTool(e.payload.name, e.payload.input)) bgAdd(e.payload.id, e.payload.name, e.payload.summary, e.payload.input, e.payload.sessionId);
  // R-174:子代理面板独立于活动面板,task 类一律进子代理面板(编排派发带 phase,
  // 模型自派 name=task)。其余工具不进子代理面板。
  if (e.payload.name === "task") agentStart(e.payload.id, e.payload.name, e.payload.summary, e.payload.input, e.payload.sessionId);
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
  const outcome = p.outcome || (p.ok ? "success" : "failed");
  const outcomeLabel = outcome === "noop" ? t("无需修改")
    : outcome === "needs_confirmation" ? t("需要确认")
      : outcome === "needs_correction" ? t("需要修正")
        : p.ok ? t("成功") : t("失败");
  log(`${t("工具结果")} ${p.name}: ${outcomeLabel} — ${p.preview}`, outcome === "success" ? "" : "warn");
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
  chatToolEnd(p.id, p.ok, p.preview, p.display, outcome);
  recordDiffSummary(p.display);
  // R-174:子代理终态进子代理面板 finished 区(task 类顶层 tool-end 只来自父任务收尾,
  // 或被停后补发)。
  if (p.name === "task") agentEnd(p.id, p.ok, p.preview, p.display);
  // 活动栏统一保留工具轨迹；历史兼容待定路径仍由 bgFinishQuiet 收尾，
  // 随后由 bgEnd 更新完成态和错误详情。
  bgFinishQuiet(p.id, p.ok);
  bgEnd(p.id, p.ok, p.preview, p.display, outcome);
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
  const payload = e.payload ?? {};
  const message = payload.message;
  const terminal = payload.terminal !== false;
  if (terminal) reportError(message);
  else reportPersistentError(message);
  // 持久化告警等非终态错误不能把仍在运行的会话投影成空闲；真正运行失败由
  // 后端明确携带 terminal=true，并随后发 kz:idle 收口。
  if (terminal) {
    // D-291:取消续跑定时器只属于终态分支。原来它在函数开头无条件执行,一条
    // terminal=false 的告警(比如持久化警告)就能掐掉已排好的下一轮,而 auto_pending
    // 仍是 true——界面从此停在「等待下一轮」,不报错也不再动。
    cancelAutoContinueTimer(payload.sessionId || activeSessionId);
    stopElapsed();
    if (activeSessionId) {
      // R-206:状态写入唯一入口 transitionSession,不再手工复刻 6 布尔标志。
      // terminal_status 由 transitionSession failed 分支折算为「出错」。
      transitionSession(activeSessionId, "failed");
    }
    setRunning(false, "出错");
    bgAbortRunning(`(${localizeDynamic("出错中止")})`);
    liveIdle("出错");
    notifyRunState("failed", message);
  }
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
  cancelAutoContinueTimer(e.payload?.sessionId || activeSessionId);
  // D-356:停止后会话收敛为 stopped,缓存不再需要(切回走 legacy/typed 完整恢复)。
  dropSessionDomCache(e.payload?.sessionId || activeSessionId);
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
on("kz:idle", (e) => {
  // D-356:会话级终态,运行中缓存失效。
  dropSessionDomCache(e.payload?.sessionId);
  renderProcesses(processItems);
});

on("kz:done", async (e) => {
  const p = e.payload;
  // D-356:kz:done 轮末原子回灌。后端在发 kz:done 前已写入完整 conversation.updated
  // (run.rs 轮末 flush)——**只有切回过(运行中缓存存在)的会话**才需要回灌替换:消息区
  // 是切走时的旧快照,重载完整投影升级(工具调用/结果由 renderRecoveredMessages 配对)。
  // 一直活动的会话消息区已是完整实时渲染,回灌替换反而会清掉轮末「完成/权限被拦」等
  // notice(R-223/R-224 回归);回灌是异步的,调用方不 await 时还会与后续渲染交错吞块。
  // 后台会话清缓存,下次切回时由 loadConversation 拿完整。
  if (p.sessionId) {
    if (p.sessionId === activeSessionId) {
      if (sessionDomCache.has(p.sessionId)) {
        dropSessionDomCache(p.sessionId);
        await loadConversation();
        cacheSessionDom(p.sessionId);
      }
    } else {
      dropSessionDomCache(p.sessionId);
    }
  }
  // 「本轮完成」是**阶段**不是**停机原因**,写进原因槽等于每轮都往里灌一句与
  // 「为什么不再继续」无关的话——而原因槽是无参重绘的回落值,灌进去之后轮次
  // 「3/34」下一帧就被它顶掉。正常完成时清空原因槽,阶段交给 renderAutoRun 算。
  setAutoStopReason(p.halted ? t("按停止/拒绝收尾") : "");

  addMessage(
    "notice",
    `${t("完成")} · steps ${p.steps}${p.history ? ` · 会话 ${p.history} 条` : ""}${p.halted ? ` · ${t("按停止/拒绝收尾")}` : ""}`
  );
  log(`${t("运行完成")}: ${p.steps} ${t("轮")}, ${t("耗时")} ${((Date.now() - runStart) / 1000).toFixed(1)}s`);
  stopElapsed();
  notifyRunState(p.halted ? "stopped" : "completed", p.halted ? t("按停止/拒绝收尾") : `${t("完成")} ${p.steps} ${t("轮")}`);
  // kz:done 只是本轮结束，不是会话级 idle；排队输入或鞭挞续跑仍可能马上开始。
  // 真正收回 stop 和运行态由 kz:idle/kz:stopped 的会话状态机负责。
  if (p.sessionId) refreshParallelTaskProjection(p.sessionId);
  else setRunning(false);
  // 对齐 Claude:当前对话跑完一轮就出现在历史列表里,不用等重启/切项目。
  refreshConversationList();
  // 活动面板保留本轮全部轨迹供回看,下一轮开跑时才翻页(kz:turn step 1)。
  liveIdle(`${t("空闲")} · ${t("上轮")} ${p.steps} ${t("轮")} ${t("完成")}`);
  // R-223:轮末汇总「本轮 N 次被拦(动作/资源清单)」——被拦 ≥1 次时对话流可见。
  summaryBlockedAsks();
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
    if (p.sessionId) transitionSession(p.sessionId, "auto_pending", { auto_rounds: autoRounds });
    if (!p.sessionId || p.sessionId === activeSessionId) setRunPending(`${t("自主推进")} ${autoRounds}/${max} · 2 ${t("秒后继续")}…`);
    renderAutoStatus(`${t("自主推进")} ${autoRounds}/${max} · ${t("等待下一轮")}`);
    if (p.sessionId) refreshParallelTaskProjection(p.sessionId);
    // 收口对象跟着本轮的会话走:并行线结束时 activeSessionId 可能已经是别人了,
    // 拿它清 pending 会把另一条线的横幅清掉,而这条线自己一直挂着(D-291)。
    armAutoContinue(continuePrompt(), p.sessionId);
  } else if (action.type === "Nudge") {
    autoRounds = action.rounds ?? autoRounds + 1;
    const max = action.max ?? autoContinueMax();
    addMessage("notice", t("上一轮没有实质动作,已追加一次具体推进指令(再无动作才会停)"));
    log(`${t("鞭挞")}:${t("无动作 · 追加推进指令")}`);
    renderAutoStatus(`${t("无动作 · 追加推进指令")} ${autoRounds}/${max}`);
    if (p.sessionId) transitionSession(p.sessionId, "auto_pending", { auto_rounds: autoRounds });
    if (!p.sessionId || p.sessionId === activeSessionId) setRunPending(`${t("无动作 · 追加推进指令")} ${autoRounds}/${max} · 2 ${t("秒后继续")}…`);
    if (p.sessionId) refreshParallelTaskProjection(p.sessionId);
    // D-291:与 Continue 分支共用同一个闸门实现(armAutoContinue)。此前这里是一份
    // 复制的 setTimeout,四个条件各自静默 return——两处副本还漏掉了 pending 收口。
    armAutoContinue(action.prompt, p.sessionId);
  } else if (action.type === "VerifyRound") {
    // R-144:已关闭 N 条,插入一轮只读验收核查(SubagentBase read/glob/grep)。
    // 核查不进入主 conversation/queue:核查指令(action.prompt,引擎生成)作为
    // 下一轮输入发回,主代理用只读 task 子代理核对验收证据与真实调用方,
    // 发现问题生成候选缺陷或退回依据;前端只显示状态并继续。
    autoRounds = action.rounds ?? autoRounds + 1;
    const max = action.max ?? autoContinueMax();
    addMessage("notice", t("已关闭 N 条,插入一轮只读验收核查(核对验收证据与真实调用方)"));
    log(`${t("鞭挞")}:${t("验收核查轮")}`);
    renderAutoStatus(`${t("验收核查轮")} ${autoRounds}/${max}`);
    if (p.sessionId) transitionSession(p.sessionId, "auto_pending", { auto_rounds: autoRounds });
    if (!p.sessionId || p.sessionId === activeSessionId) setRunPending(`${t("验收核查轮")} ${autoRounds}/${max} · 2 ${t("秒后继续")}…`);
    if (p.sessionId) refreshParallelTaskProjection(p.sessionId);
    armAutoContinue(action.prompt || continuePrompt(), p.sessionId);
  } else if (action.type === "Stop") {
    if (p.sessionId) transitionSession(p.sessionId, "idle");
    if (!p.sessionId || p.sessionId === activeSessionId) clearRunPending();
    autoRounds = 0;
    noActionRounds = 0;
    cancelAutoContinueTimer(p.sessionId || activeSessionId);
    const reason = action.reason;
    if (reason === "Paused") {
      addMessage("notice", `${t("鞭挞停止")}: ${t("处于暂停中,点顶栏「继续鞭挞」恢复")}`);
      setAutoStopReason("已暂停");
    } else if (reason === "StopAfterRound") {
      applyAutoStopToSession(p.sessionId || activeSessionId, { stopAfterRound: false });
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
      applyAutoStopToSession(p.sessionId || activeSessionId, { enabled: false });
      const msg = t("需求与缺陷全部被阻塞，自动推进已停止");
      setAutoStopReason(msg);
      addMessage("notice", `✅ ${msg}`);
      log(t("自动推进停止:需求与缺陷全部被阻塞"));
    } else if (reason === "BacklogEmpty") {
      applyAutoStopToSession(p.sessionId || activeSessionId, { enabled: false });
      const msg = t("需求与缺陷已清空，自动推进已停止");
      setAutoStopReason(msg);
      addMessage("notice", `✅ ${msg}`);
      log(t("自动推进停止:需求与缺陷已清空"));
    } else if (reason === "ProfileMismatch") {
      // R-199:档位条件由引擎判定,前端只显示(不再持有否决权)。
      applyAutoStopToSession(p.sessionId || activeSessionId, { enabled: false });
      const msg = t("鞭挞已关闭,当前进程不是自主推进模式");
      setAutoStopReason(msg);
      addMessage("notice", `✅ ${msg}`);
      log(t("自动推进停止:当前模式不匹配"));
    }
  }
  // NoContinue:用户拒绝/未开启——不续跑不重置,等手动输入重新武装。
});

// ---------- 权限弹窗 ----------
const askQueues = new Map();
let askActive = null;
// D-337:多选档位下已勾选的选项(question 弹窗);非多选档位不消费。
let askSelectedOptions = [];

function isMultiSelectAsk(ask) {
  if (!ask || ask.kind !== "question") return false;
  if (ask.multiple) return true;
  // 工具没传 multiple 但问题文本声明了多选(如「可多选/补充」)时兜底;
  // 声明有 `multiple` 字段后应显式传值,这里只兜历史行为。
  return /多选|multi[- ]?select/i.test(ask.question || "");
}

function currentQuestionAnswer() {
  if (!askActive || askActive.kind !== "question") return "";
  const parts = [];
  if (askSelectedOptions.length) parts.push(askSelectedOptions.join("\n"));
  const text = $("ask-answer").value.trim();
  if (text) parts.push(text);
  return parts.join("\n");
}

function updateAskSubmitState() {
  if (!askActive || askActive.kind !== "question") return;
  const multi = isMultiSelectAsk(askActive);
  $("ask-submit").disabled =
    multi && askSelectedOptions.length === 0 && !$("ask-answer").value.trim();
}

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

// R-223:自动轮权限被拦的聚合呈现——每次跳过落可见 notice,轮末汇总。
// 数组元素 {action|question, resource, at}。kz:ask 的 parallel/autonomous 跳过
// 分支记录;kz:idle 轮末汇总「本轮 N 次被拦(动作/资源清单)」。
const blockedAsks = [];
let blockedAskSummaryShown = false;

function recordBlockedAsk(payload) {
  const item = {
    what: payload.action || payload.question || "ASK",
    resource: payload.resource || "",
    at: new Date().toISOString(),
  };
  blockedAsks.push(item);
  blockedAskSummaryShown = false;
  const label = payload.action
    ? `${payload.action}${payload.resource ? ` · ${payload.resource}` : ""}`
    : payload.question;
  addMessage("notice", `⚠️ ${t("权限被拦已跳过")}: ${label}`);
  return item;
}

function summaryBlockedAsks() {
  if (!blockedAsks.length || blockedAskSummaryShown) return;
  blockedAskSummaryShown = true;
  const items = blockedAsks
    .map((item) => (item.resource ? `${item.what} · ${item.resource}` : item.what))
    .join("; ");
  addMessage(
    "notice",
    `⚠️ ${t("本轮权限被拦")} ${blockedAsks.length} ${t("次")}${items ? ` — ${items}` : ""}`,
  );
}

on("kz:ask", (e) => {
  const sessionId = askSessionId(e.payload);
  e.payload.sessionId = sessionId;
  // 后端的 NonInteractive 策略不会产生 kz:ask；这里仍做一层防御，避免旧运行
  // 或第三方事件把并行/自举线的询问冒泡到当前用户弹窗。
  if (e.payload.source === "parallel" || e.payload.source === "autonomous") {
    log(`${t("后台询问已跳过")}: ${e.payload.action || e.payload.question || "ASK"}`);
    recordBlockedAsk(e.payload);
    return;
  }
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

// R-223:「自动放行」常驻警示徽标——开启时状态栏可见,重启后仍显示
// (localStorage 持久化,语义是"会记住"而非"仅本次")。
function syncAutoAllowBadge() {
  const on = $("auto-allow").checked;
  $("status-auto-allow").classList.toggle("hidden", !on);
}
$("auto-allow").checked = localStorage.getItem("kz-auto-allow") === "1";
$("auto-allow").addEventListener("change", () => {
  localStorage.setItem("kz-auto-allow", $("auto-allow").checked ? "1" : "0");
  syncAutoAllowBadge();
  log($("auto-allow").checked ? t("已开启自动放行(所有权限询问直接通过;此选择会被记住,跨重启仍生效)") : t("已关闭自动放行"));
});
syncAutoAllowBadge();

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
    const multi = isMultiSelectAsk(askActive);
    askSelectedOptions.length = 0;
    const options = $("ask-options");
    options.innerHTML = "";
    options.classList.toggle("multi", multi);
    for (const option of askActive.options || []) {
      const button = document.createElement("button");
      button.className = "ghost ask-option";
      button.type = "button";
      button.textContent = option;
      button.setAttribute("aria-pressed", "false");
      button.addEventListener("click", () => {
        if (!multi) {
          answerAsk(option);
          return;
        }
        const index = askSelectedOptions.indexOf(option);
        if (index >= 0) {
          askSelectedOptions.splice(index, 1);
        } else {
          askSelectedOptions.push(option);
        }
        const selected = index < 0;
        button.classList.toggle("selected", selected);
        button.setAttribute("aria-pressed", String(selected));
        updateAskSubmitState();
      });
      options.appendChild(button);
    }
    $("ask-answer").value = askActive.default || "";
    $("ask-answer").placeholder = multi ? t("补充说明(可选)") : t("输入你的回答");
    setTimeout(() => $("ask-answer").focus(), 0);
    updateAskSubmitState();
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
$("ask-submit").addEventListener("click", () => answerAsk(currentQuestionAnswer()));
$("ask-answer").addEventListener("keydown", (event) => {
  if (event.key === "Enter") answerAsk(currentQuestionAnswer());
});
$("ask-answer").addEventListener("input", updateAskSubmitState);
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
      // 完整思维链:收起态也全量导出(dataset.raw 一直在),不再截首行 160 字——
      // 摘要贴给别的 AI 没有用,断链的思考等于没复制。
      const raw = el.querySelector(".reasoning-body")?.dataset.raw?.trim();
      if (raw) parts.push(`### ${t("思考")}\n${raw.split("\n").map((line) => `> ${line}`).join("\n")}`);
    } else if (el.classList.contains("tool-chip")) {
      const head = el.querySelector(".head")?.textContent?.trim();
      const result = el.querySelector(".result")?.textContent?.trim();
      if (head) parts.push(`> ${t("工具")}:${head.slice(0, 200)}${result ? `\n> ${result.slice(0, 400)}` : ""}`);
    } else if (el.classList.contains("turn-divider")) {
      parts.push(`---\n${el.textContent}`);
    } else if (el.classList.contains("msg")) {
      // 其余消息形态(error 等)不再被静默跳过:主对话缺失错误上下文,导出就失真。
      const text = el.textContent?.trim();
      if (text) parts.push(`> ${text.slice(0, 500)}`);
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
