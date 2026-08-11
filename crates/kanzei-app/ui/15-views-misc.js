// ---------- R-053 快速记录:独立子代理结构化落库(需求/缺陷通用),不打断主对话 ----------
function quickCaptureForm(kind, sectionId, noun) {
  const section = $(sectionId);
  const title = section.querySelector(".section-title");
  // 需求与缺陷两种快记现在共用「当前在做」这一个分区标题。旧守卫只看"有没有表单",
  // 于是正在写需求时点「记缺陷」会静默无反应——能力被挡住却不说破,与 D-166/D-210
  // 同一种病。同类再点是幂等(保留已写内容),换一类就换掉表单。
  const opened = title.querySelector(".quickreq-form");
  if (opened) {
    if (opened.dataset.kind === kind) return;
    opened.remove();
  }
  const form = document.createElement("div");
  form.className = "goal-add-form quickreq-form";
  form.dataset.kind = kind;
  const input = document.createElement("textarea");
  input.rows = 3;
  input.placeholder = `${t("自然语言描述")}${t(noun)};Ctrl+Enter 或点${t("提交")},Esc ${t("取消")}。${t("独立子代理后台进行")},不打断当前对话。`;
  const submit = async () => {
    const text = input.value.trim();
    if (!text) {
      toast(t("先写点描述"));
      return;
    }
    // 失败时表单必须还在:提交前销毁会让用户写的描述无处可寻。
    submitBtn.disabled = true;
    cancelBtn.disabled = true;
    input.disabled = true;
    toast(`${t("记录中")}${t(noun)}…(${t("独立子代理后台进行")})`);
    try {
      const msg = await invoke("quick_req", { projectDir: currentProject, description: text, kind });
      form.remove();
      toast(`${t("已记录")}:${msg}`);
      refreshDocs();
    } catch (err) {
      submitBtn.disabled = false;
      cancelBtn.disabled = false;
      input.disabled = false;
      toastError(`${t("记录失败(内容已保留,可重试)")}:${err}`);
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
  cancelBtn.textContent = t("取消");
  cancelBtn.addEventListener("click", () => form.remove());
  const submitBtn = document.createElement("button");
  submitBtn.className = "primary mini";
  submitBtn.textContent = t("提交");
  submitBtn.addEventListener("click", submit);
  bar.append(cancelBtn, submitBtn);
  form.append(input, bar);
  title.append(form);
  input.focus();
}
$("req-quick").addEventListener("click", () => quickCaptureForm("req", "focus-section", "需求"));
$("defect-quick").addEventListener("click", () => quickCaptureForm("defect", "focus-section", "缺陷"));

function renderConventions(conv) {
  const el = $("conv-list");
  el.innerHTML = "";
  if (!conv || !conv.exists) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("未创建,点 ＋ 生成模板;agent 会自动遵守此文件");
    el.appendChild(empty);
    return;
  }
  // 规范不再铺开章节列表占满侧边栏:一行入口,点开进应用内 MD 查看器。
  const item = document.createElement("button");
  item.type = "button";
  item.className = "doc-item conv-entry";
  item.setAttribute("aria-label", `${t("打开开发规范")}，${conv.headings.length}${t("个章节")}`);
  item.textContent = `${conv.headings.length}${t("个章节")} · ${t("点击查看")}`;
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
  input.placeholder = t("目标描述,回车创建(Esc 取消)");
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
      toastError(String(err));
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
    toastError(String(err), { retry: () => $("conv-init").click() });
  }
});
$("conv-open").addEventListener("click", () => openDocViewer("conventions"));

// ---------- 应用内文档查看器:markdown/代码直接渲染,外部打开是兜底 ----------
let viewerKind = null;
function openRuntimeMarkdown(title, content) {
  viewerKind = null;
  $("viewer-title").textContent = title;
  const body = $("viewer-body");
  body.className = "md";
  body.innerHTML = renderMarkdown(content ?? "");
  body.scrollTop = 0;
  $("viewer-external").classList.add("hidden");
  $("viewer-overlay").classList.remove("hidden");
  $("viewer-close").focus();
}
async function openDocViewer(kind) {
  try {
    const doc = await invoke("docs_read", { projectDir: currentProject, kind });
    viewerKind = kind;
    $("viewer-external").classList.remove("hidden");
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
    $("viewer-close").focus();
  } catch (err) {
    toastError(String(err), { retry: () => openDocViewer(kind) });
  }
}
$("viewer-close").addEventListener("click", () => $("viewer-overlay").classList.add("hidden"));
$("viewer-overlay").addEventListener("click", (e) => {
  if (e.target === $("viewer-overlay")) $("viewer-overlay").classList.add("hidden");
});
$("viewer-external").addEventListener("click", () => {
  if (viewerKind) invoke("docs_open", { projectDir: currentProject, kind: viewerKind }).catch((e) => toastError(String(e), { retry: () => $("viewer-external").click() }));
});

// ---------- git 状态 ----------
async function refreshGit() {
  if (!currentProject) return;
  try {
    const g = await invoke("git_status", { projectDir: currentProject });
    $("status-git").textContent = g.branch
      ? `⎇ ${g.branch}${g.changes ? ` +${g.changes}` : ""}`
      : "";
    $("status-git").title = g.last ? `${t("最近提交")}:${g.last}` : "";
  } catch {
    $("status-git").textContent = "";
  }
}

// 运行中改文件/跑命令后刷新工作区徽章,合并 600ms 内的连续变更。
let gitLiveTimer = null;
function refreshGitSoon() {
  clearTimeout(gitLiveTimer);
  gitLiveTimer = setTimeout(() => {
    gitLiveTimer = null;
    refreshGit();
  }, 600);
}

function renderRecoveredMessages(items) {
  followLatest = true;
  messages.innerHTML = "";
  currentAssistant = null;
  currentReasoning = null;
  currentReasoningHead = null;
  // 调用与结果按 call_id 配对成一块渲染:原先每个 part 各占一行,
  // 结果行只显示原始 call id,对人毫无信息量(用户 2026-08-08 反馈"太丑")。
  const pending = new Map();
  for (const message of items ?? []) {
    for (const part of message.parts ?? []) {
      if (part.type === "tool_call") {
        const block = buildToolBlock(part.name || "tool", part.input);
        messages.appendChild(block.wrap);
        if (part.id) pending.set(part.id, { block, input: part.input });
        continue;
      }
      if (part.type === "tool_result") {
        const entry = pending.get(part.call_id);
        if (entry) {
          pending.delete(part.call_id);
          fillToolBlock(entry.block, {
            ok: !part.is_error,
            content: part.content,
            input: entry.input,
          });
        } else {
          // 配对不上(历史被压缩过):独立成块,总比丢掉强。
          const orphan = buildToolBlock("tool result", {});
          messages.appendChild(orphan.wrap);
          fillToolBlock(orphan, { ok: !part.is_error, content: part.content });
        }
        continue;
      }
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
  // 没等到结果的调用(轮次被中断):标出来,不要停在"运行中"的假象上。
  for (const { block } of pending.values()) {
    block.wrap.classList.remove("running");
    block.result.textContent = `⎿ ${t("无结果(轮次中断)")}`;
    block.result.classList.remove("hidden");
  }
  if (!items?.length) {
    messages.innerHTML = `<div id="empty-state"><div class="logo-mark">K</div><div class="hint">${t("输入任务开始 · 权限请求会弹窗询问 · Ctrl+Enter 发送")}</div></div>`;
  }
  scrollBottom(true);
}

async function loadConversation(sequence = null, switchGeneration = null) {
  if (!currentProject) return;
  // 启动时项目列表与历史恢复并行触发,先确保进程列表已选出主会话,再锁定
  // processId。否则首次 conversation_get 可能带着 null,历史会被竞态丢掉。
  if (!activeProcessId && typeof refreshProcesses === "function") await refreshProcesses();
  if (!currentProject || !activeProcessId) return;
  // 线程切换是异步的:conversation_get 与 trace_get 之间用户可能再次切线。
  // 两个 IPC 必须锁定同一项目/同一进程,且晚返回的旧请求不能覆盖当前线程。
  const forProject = currentProject;
  const forProcessId = activeProcessId;
  const isCurrent = () =>
    (switchGeneration === null || switchGeneration === processSwitchGeneration) &&
    currentProject === forProject &&
    activeProcessId === forProcessId;
  try {
    bgClear();
    renderTodoPanel([], 0, 0);
    const history = await invoke("conversation_get", {
      projectDir: forProject,
      processId: forProcessId,
      sequence,
    });
    if (!isCurrent()) return;
    renderRecoveredMessages(history);
    const traces = await invoke("conversation_trace_get", {
      projectDir: forProject,
      processId: forProcessId,
      sequence,
    });
    if (!isCurrent()) return;
    renderRecoveredTraces(traces);
    log(`${t("已恢复")} ${history.length} ${t("条")} ${t("历史消息")} ${traces.length} ${t("组工具轨迹")}`);
  } catch (err) {
    addMessage("error", `${t("历史消息恢复失败")}:${err}`);
    toastError(`${t("历史消息恢复失败")}:${err}`, { retry: () => loadConversation(sequence) });
  }
}

let conversationItems = [];
function renderConversationList(items) {
  conversationItems = items ?? [];
  const el = $("conversation-list");
  el.innerHTML = "";
  $("chat-select-all").checked = false;
  $("conversation-count").textContent = items.length;
  if (!items.length) {
    el.textContent = t("暂无历史对话");
    return;
  }
  for (const item of [...items].reverse()) {
    const row = document.createElement("div");
    row.className = "doc-item conv-row";
    row.title = t("点击打开 · 勾选后点标题栏的删除图标批量删除");
    const check = document.createElement("input");
    check.type = "checkbox";
    check.className = "chat-check";
    check.dataset.seqs = JSON.stringify(item.sequences ?? [item.sequence]);
    check.addEventListener("click", (e) => e.stopPropagation());
    check.addEventListener("change", () => {
      const checks = [...document.querySelectorAll(".chat-check")];
      $("chat-select-all").checked = checks.length > 0 && checks.every((item) => item.checked);
    });
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = `${item.title || t("新对话")} (${item.message_count} ${t("条")})`;
    row.append(check, title);
    row.addEventListener("click", async () => {
      if (running) {
        toast(t("运行中请先完成或停止当前任务，再打开历史对话"));
        return;
      }
      try {
        await loadConversation(item.sequence);
        addMessage("notice", `${t("已打开历史对话")} #${item.sequence}`);
      } catch (err) {
        toastError(String(err));
      }
    });
    el.appendChild(row);
  }
}

$("chat-select-all").addEventListener("change", (event) => {
  document.querySelectorAll(".chat-check").forEach((check) => { check.checked = event.target.checked; });
});

$("chat-del").addEventListener("click", async () => {
  const sequences = [...document.querySelectorAll(".chat-check:checked")]
    .flatMap((c) => JSON.parse(c.dataset.seqs));
  if (!sequences.length) {
    toast(t("先勾选要删除的历史对话"));
    return;
  }
  try {
    const n = await invoke("conversation_delete", { projectDir: currentProject, processId: activeProcessId, sequences });
    toast(`${t("已删除")} ${n}${t("份对话快照")}`);
    await refreshConversationList();
  } catch (err) {
    toastError(String(err), { retry: () => $("chat-del").click() });
  }
});

async function refreshConversationList() {
  if (!currentProject) return;
  try {
    renderConversationList(await invoke("conversation_list", { projectDir: currentProject, processId: activeProcessId }));
  } catch (err) {
    $("conversation-list").textContent = `${t("历史对话加载失败")}:${err}`;
    toastError(`${t("历史对话加载失败")}:${err}`, { retry: refreshConversationList });
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
    toast(t("任务运行中,先停止再开新对话"));
    return;
  }
  try {
    await invoke("conversation_clear", { projectDir: currentProject, processId: activeProcessId });
    clearChat(t("已开启新对话(历史已清空)"));
    await refreshConversationList();
    log(t("新对话:多轮历史已清空"));
  } catch (err) {
    toastError(String(err), { retry: () => $("new-chat").click() });
  }
});

// ---------- 对话总结 ----------
$("summarize-btn").addEventListener("click", async () => {
  if (!currentProject) {
    toast(t("先选择一个项目"));
    return;
  }
  const transcript = [...messages.querySelectorAll(".msg, .tool-chip")]
    .map((el) => el.textContent.trim())
    .filter(Boolean)
    .join("\n\n")
    .slice(0, 60000);
  if (!transcript) {
    toast(t("当前没有可总结的对话"));
    return;
  }
  $("summarize-btn").disabled = true;
  setStatus(`${t("总结中")}(fast model)`, true);
  log(t("开始总结当前对话…"));
  try {
    const r = await invoke("summarize_chat", { projectDir: currentProject, transcript });
    addSummaryEntry(r.summary, r.path);
    toast(t("小总结已收纳到活动面板"));
    log(`总结完成,已收纳并存档:${r.path}`);
  } catch (err) {
    toastError(`总结失败:${err}`, { retry: () => $("summarize-btn").click() });
  } finally {
    $("summarize-btn").disabled = false;
    setStatus(running ? t("运行中") : t("空闲"), running);
  }
});

for (const [btn, kind] of [["req-open", "req"], ["defect-open", "defect"], ["goal-open", "goal"], ["report-open", "report"]]) {
  $(btn).addEventListener("click", () => openDocViewer(kind));
}
