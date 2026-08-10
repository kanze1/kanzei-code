// ---------- 子代理面板(R-174):独立 Running/Finished 分区、单条停止、完整 transcript ----------
// 与活动面板(#bg-panel)平级的独立右侧面板。数据全部来自真实 RunEvent:
// tool-start(建条) → task-progress(trace:当前工具名/累计 token/内部调用序列/transcript 数据)
// → tool-end(终态:成功/失败/被停,移入 Finished 区)。跨轮存活条目等 R-175 提供数据后再接。
const agentEntries = new Map(); // id -> {el, head, meta, detail, calls, tokens, startedAt, done}
let agentPanelOpen = false;

function agentToolType(phase, name) {
  if (phase) return phase; // scouting / review / fixup
  if (name && name !== "task") return name;
  return "task";
}

function agentPhaseLabel(phase) {
  const labels = { scouting: t("勘察"), review: t("复核"), fixup: t("修复") };
  return labels[phase] || phase || t("子代理");
}

function agentTick(entry) {
  const seconds = Math.round((Date.now() - entry.startedAt) / 1000);
  entry.el.dataset.agentElapsed = String(seconds);
  const bits = [`${seconds}s`, `${t("工具调用")} ${entry.calls.size}`, `${t("token")} ${entry.tokens}`];
  if (entry.currentTool) bits.push(`⚙ ${entry.currentTool}`);
  entry.meta.textContent = bits.join(" · ");
}

function agentSetCurrentTool(entry, name, running) {
  entry.currentTool = name ? String(name) : "";
  entry.el.dataset.agentCurrentTool = entry.currentTool;
  agentTick(entry);
}

// 统计 token:task-progress 的 trace 在 phase=="usage" 时携带累计 usage(StepEnd 逐轮累计)。
function agentAddUsage(entry, usage) {
  if (!usage) return;
  const input = Number(usage.input) || 0;
  const output = Number(usage.output) || 0;
  const cacheRead = Number(usage.cache_read) || Number(usage.cacheRead) || 0;
  const cacheWrite = Number(usage.cache_write) || Number(usage.cacheWrite) || 0;
  // StepEnd 是"本轮新增"还是"累计"由后端定;这里按增量累计到面板条目,字段名与
  // 主对话 runTokens 口径一致,且与 usage 结构无关,新旧字段都吸收。
  entry.tokens += input + output + cacheRead + cacheWrite;
}

function agentCountsSync() {
  let running = 0;
  let finished = 0;
  for (const entry of agentEntries.values()) {
    if (entry.done) finished += 1;
    else running += 1;
  }
  $("agent-running-count").textContent = running ? `运行中 ${running}` : "";
  $("agent-finished-count").textContent = finished ? `已完成 ${finished}` : "";
  $("agent-running-count2").textContent = running ? String(running) : "";
  $("agent-finished-count2").textContent = finished ? String(finished) : "";
  $("agent-clear").classList.toggle("hidden", finished === 0);
}

// 打开/收起面板。与活动面板互斥:一个开着时另一个收起,避免右侧两栏叠在一起。
function agentTogglePanel() {
  agentPanelOpen = !agentPanelOpen;
  $("agent-panel").classList.toggle("hidden", !agentPanelOpen);
  $("bg-panel").classList.toggle("hidden", agentPanelOpen);
  $("agent-toggle").classList.toggle("active", agentPanelOpen);
  $("agent-toggle").setAttribute("aria-expanded", String(agentPanelOpen));
  $("agent-toggle").title = agentPanelOpen ? t("收起子代理面板") : t("打开子代理面板");
}

function agentStart(id, name, summary, input) {
  if (!id) return;
  const phase = name === "task" ? orchPhaseOf(input) : null;
  const existing = agentEntries.get(id);
  if (existing && !existing.done) return; // 同 id 仍在跑,原地更新
  if (existing) {
    // 角色跨轮复用(architecture_scout 每轮都派):旧条目进 finished 后同名再次派发,
    // 直接原地复位成新 running 条目,避免面板越积越长。
    agentEntries.delete(id);
    existing.el.remove();
  }
  const el = document.createElement("div");
  el.className = "bg-entry running";
  el.dataset.agentId = id;
  el.dataset.bgStatus = "running";
  const title = document.createElement("button");
  title.type = "button";
  title.className = "bg-title";
  title.setAttribute("aria-label", t("展开或收起子代理详情"));
  title.setAttribute("aria-expanded", "false");
  const toolName = document.createElement("span");
  toolName.className = "bg-tool";
  // 名称:编排派发的角色以角色名(id)为身份,模型自派的一律叫 task。
  toolName.textContent = phase ? id : name;
  const target = document.createElement("span");
  target.className = "bg-target";
  const shown = toolCallSummary(name, input) || String(summary ?? "");
  target.textContent = shown;
  title.append(toolName, target);
  if (phase) {
    const badge = document.createElement("span");
    badge.className = "bg-phase-badge";
    badge.textContent = agentPhaseLabel(phase);
    title.append(badge);
  }
  title.title = shown;
  const prog = document.createElement("div");
  prog.className = "bg-prog";
  prog.textContent = `… ${t("子代理启动中")}`;
  const meta = document.createElement("div");
  meta.className = "bg-meta";
  const actions = document.createElement("div");
  actions.className = "bg-actions";
  const detail = document.createElement("div");
  detail.className = "bg-detail hidden";
  title.addEventListener("click", () => {
    if (detail.children.length) {
      detail.classList.toggle("hidden");
      title.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    }
  });
  el.append(title, prog, meta, actions, detail);
  const entry = {
    el, title, target, prog, meta, actions, detail, phase, name,
    calls: new Map(), tokens: 0, currentTool: "", startedAt: Date.now(), done: false,
  };
  agentEntries.set(id, entry);
  $("agent-running").appendChild(el);
  // transcript 数据:tool-start 的 input 就是该子代理的初始指令(toolCallSummary 挑字段
  // 挑不出时 target 回落 summary),完整入参进 detail 可展开。
  if (input && Object.keys(input).length) {
    const args = document.createElement("pre");
    args.className = "tool-display term bg-args";
    args.textContent = JSON.stringify(input, null, 2);
    detail.appendChild(args);
    el.classList.add("has-detail");
  }
  agentTick(entry);
  agentRenderActions(id, entry);
  agentCountsSync();
}

function agentProgress(id, text, trace) {
  const entry = agentEntries.get(id);
  if (!entry || entry.done) return;
  if (text) entry.prog.textContent = text;
  if (!trace) {
    if (!entry.done) agentTick(entry);
    return;
  }
  if (trace.phase === "usage") {
    // usage 的 trace 里 name 是空串(后端构造),不覆盖当前工具名。
    agentAddUsage(entry, trace.usage);
    agentTick(entry);
    return;
  }
  agentSetCurrentTool(entry, trace.name, trace.phase === "start");
  let call = entry.calls.get(trace.child_id);
  if (trace.phase === "start") {
    if (!call) {
      call = { name: trace.name, summary: trace.summary || "", input: trace.input || null, ok: null, preview: null, display: null };
      entry.calls.set(trace.child_id, call);
    }
  } else if (call) {
    call.ok = trace.ok;
    call.preview = trace.preview || "";
    call.display = trace.display || null;
  }
  // 调用的入参与输出都收进 detail,这就是 transcript 的原始数据源。
  renderAgentTranscript(entry);
  if (!entry.done) agentTick(entry);
}

// transcript:该子代理内部每次工具调用的 名称 + 入参 + 输出,按发生顺序渲染。
function renderAgentTranscript(entry) {
  const detail = entry.detail;
  // 重建:调用序列有新增/终结时整体重画(频率低,一次 task-progress 一批)。
  detail.querySelectorAll(".agent-call").forEach((node) => node.remove());
  for (const call of entry.calls.values()) {
    const row = document.createElement("div");
    row.className = "agent-call";
    row.dataset.agentCall = call.name;
    const head = document.createElement("div");
    head.className = "agent-call-head";
    head.textContent = call.ok === false ? `✕ ${call.name} ${call.preview || ""}` : `✓ ${call.name} ${call.summary}`;
    row.appendChild(head);
    if (call.input && Object.keys(call.input).length) {
      const pre = document.createElement("pre");
      pre.className = "tool-display term";
      pre.textContent = JSON.stringify(call.input, null, 2);
      row.appendChild(pre);
    }
    detail.appendChild(row);
  }
  detail.classList.remove("hidden");
  entry.el.classList.add("has-detail");
  if (detail.children.length) entry.title.setAttribute("aria-expanded", "true");
}

function agentEnd(id, ok, preview, display) {
  const entry = agentEntries.get(id);
  if (!entry) return;
  entry.done = true;
  entry.el.classList.remove("running");
  const stopped = !ok && /被停|停止|stopped|cancelled/.test(String(preview ?? ""));
  entry.el.classList.add(ok ? "ok" : stopped ? "timeout" : "err");
  entry.el.dataset.bgStatus = ok ? "ok" : stopped ? "stopped" : "err";
  agentSetCurrentTool(entry, null, false);
  entry.prog.textContent = preview || (ok ? t("完成") : t("失败"));
  const ms = Date.now() - entry.startedAt;
  const elapsed = ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
  const bits = [ok ? `✓ ${t("成功")}` : stopped ? `⏹ ${t("已停止")}` : `✕ ${t("失败")}`, elapsed];
  if (entry.calls.size) bits.push(`${t("工具调用")} ${entry.calls.size}`);
  if (entry.tokens) bits.push(`${t("token")} ${entry.tokens}`);
  entry.meta.textContent = bits.join(" · ");
  if (display) appendDisplayBlock(entry.detail, display);
  renderAgentTranscript(entry);
  // 从 running 区挪到 finished 区:移动节点即可,entry 引用不变。
  $("agent-finished").appendChild(entry.el);
  agentRenderActions(id, entry);
  agentCountsSync();
}

// 每条的操作项:运行中的能单条停止;结束后能重跑、能查看 transcript(点标题展开)。
function agentRenderActions(id, entry) {
  entry.actions.innerHTML = "";
  const add = (label, title, handler) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ghost mini";
    btn.textContent = label;
    btn.title = title;
    btn.addEventListener("click", (event) => {
      event.stopPropagation();
      handler();
    });
    entry.actions.appendChild(btn);
    return btn;
  };
  if (!entry.done) {
    // R-174:子代理单条停止通道——不再只能停整轮。
    add(t("停止"), t("只停这一条子代理,不影响本轮其它工具"), async () => {
      try {
        await invoke("stop_task", { projectDir: currentProject, taskId: String(id) });
        toast(t("已请求停止该子代理"));
      } catch (error) {
        toastError(`${t("停止失败")}:${error}`);
      }
    });
  }
  if (entry.done) {
    add(t("打开"), t("查看完整 transcript(工具调用序列 + 每次入参与输出)"), () => {
      const detail = entry.detail;
      detail.classList.toggle("hidden");
      entry.title.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    });
  }
}

// Clear:清空 Finished 区,保留运行中的。
function agentClearFinished() {
  for (const [id, entry] of agentEntries) {
    if (entry.done) {
      entry.el.remove();
      agentEntries.delete(id);
    }
  }
  agentCountsSync();
}

function agentPanelSetup() {
  const toggle = $("agent-toggle");
  toggle.addEventListener("click", agentTogglePanel);
  $("agent-clear").addEventListener("click", agentClearFinished);
}
// defer 脚本执行时 DOM 已解析完毕,agent-toggle 一定存在;与 activity-toggle 的
// syncActivityPanel 一样在模块加载期挂好监听。
agentPanelSetup();
