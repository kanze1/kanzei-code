// ---------- 活动面板(R-037):全部工具调用按序入列,详情点击展开,跑完保留可回看 ----------
const bgEntries = new Map(); // call_id -> {el, title, prog, meta, detail, startedAt, done}
const diffSummary = new Map();
const BG_MAX = 120;
function renderDiffSummary() {
  const panel = $("diff-summary");
  const label = $("diff-summary-toggle");
  const files = [...diffSummary.values()];
  const additions = files.reduce((sum, item) => sum + item.additions, 0);
  const deletions = files.reduce((sum, item) => sum + item.deletions, 0);
  label.innerHTML = files.length
    ? `· ${files.length} ${escapeHtml(t("文件"))} <span class="diff-add">+${additions}</span>/<span class="diff-del">−${deletions}</span>`
    : "";
  panel.classList.toggle("hidden", files.length === 0);
  panel.innerHTML = files.map((item) => `<div class="diff-summary-row"><span>${escapeHtml(item.path)}</span><span class="diff-summary-counts"><span class="diff-add">+${item.additions}</span><span class="diff-del">−${item.deletions}</span></span></div>`).join("");
}

function bgSync() {
  // 面板开关只由用户控制;工具事件只能更新内容,不能擅自开关。
  syncActivityPanel();
  applyBgFilters();
}
// R-095:活动面板此前只收 task 与 memory_note,绝大多数轮次里它是空的——"打开也没啥用"。
// 现在它是完整的活动流水,靠筛选控制噪音;主对话仍保留内联工具块,两者定位不同:
// 主对话是叙事,活动面板是可检索的执行记录。
function isActivityTool() {
  return true;
}

const BG_TOOL_TYPES = {
  bash: "terminal", process: "terminal",
  read: "file", write: "file", edit: "file", multiedit: "file", glob: "file", grep: "file",
  req: "tracker", defect: "tracker", goal: "tracker", source: "tracker", finding: "tracker", decision: "tracker",
  task: "agent",
  memory_note: "memory", memory_search: "memory", memory_stats: "memory",
};
function bgToolType(name) {
  return BG_TOOL_TYPES[name] ?? "other";
}
// 终端类输出才提供复制/导出:diff 与追踪结果在主对话里已有更好的呈现。
function bgIsTerminal(name) {
  return bgToolType(name) === "terminal";
}

const bgFilters = {
  type: localStorage.getItem("kz-bg-type") || "all",
  status: localStorage.getItem("kz-bg-status") || "all",
};
function bgEntryStatus(entry) {
  if (!entry.done) return "running";
  return entry.el.classList.contains("err") ? "err" : "ok";
}
function applyBgFilters() {
  let shown = 0;
  for (const entry of bgEntries.values()) {
    const typeOk = bgFilters.type === "all" || entry.type === bgFilters.type;
    const statusOk = bgFilters.status === "all" || bgEntryStatus(entry) === bgFilters.status;
    const visible = typeOk && statusOk;
    entry.el.classList.toggle("hidden", !visible);
    if (visible) shown += 1;
  }
  const count = $("bg-count");
  // 有筛选时同时给出"筛出/总数",否则看到 3 条会以为本轮只跑了 3 个工具。
  if (count) {
    count.textContent = bgEntries.size
      ? shown === bgEntries.size
        ? `· ${bgEntries.size}`
        : `· ${shown}/${bgEntries.size}`
      : "";
  }
}

/// 差异汇总必须独立于活动面板的过滤:diff 来自 write/edit,而这两个工具已不进活动面板,
/// 原先把累计写在 bgEnd 里就等于永远拿不到数据,#diff-summary 变成接不到数据源的空壳(D-137)。
function recordDiffSummary(display) {
  if (display?.kind !== "diff") return;
  diffSummary.set(display.path || `#${diffSummary.size + 1}`, {
    path: display.path || "未命名文件",
    additions: display.additions || 0,
    deletions: display.deletions || 0,
  });
  renderDiffSummary();
}

function bgAdd(id, name, summary, input) {
  if (!id || bgEntries.has(id)) return;
  const type = bgToolType(name);
  const el = document.createElement("div");
  el.className = `bg-entry running bg-type-${type}`;
  el.dataset.bgId = id;
  el.dataset.bgTool = name;
  const title = document.createElement("button");
  title.type = "button";
  title.className = "bg-title";
  title.setAttribute("aria-label", t("展开或收起后台任务详情"));
  title.setAttribute("aria-expanded", "false");
  // 工具名与目标分开呈现:此前拼成一行长文本被 ellipsis 截断,看不出改的是哪个文件、
  // 跑的是哪条命令——"打开也没啥用"的直接原因(R-095 验收 ⑤)。
  const toolName = document.createElement("span");
  toolName.className = "bg-tool";
  toolName.textContent = name;
  const target = document.createElement("span");
  target.className = "bg-target";
  target.textContent = summary;
  title.append(toolName, target);
  title.title = summary;
  const prog = document.createElement("div");
  prog.className = "bg-prog";
  prog.textContent = name === "task" ? `… ${t("子代理启动中")}` : "…";
  const meta = document.createElement("div");
  meta.className = "bg-meta";
  const actions = document.createElement("div");
  actions.className = "bg-actions";
  const detail = document.createElement("div");
  detail.className = "bg-detail hidden";
  // 完整入参永远可展开:summary 是一行摘要,复核"到底拿什么参数调的"要看原文。
  if (input && Object.keys(input).length) {
    const args = document.createElement("pre");
    args.className = "tool-display term bg-args";
    args.textContent = JSON.stringify(input, null, 2);
    detail.appendChild(args);
    el.classList.add("has-detail");
  }
  title.addEventListener("click", () => {
    if (detail.children.length) {
      detail.classList.toggle("hidden");
      title.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    }
  });
  el.append(title, prog, meta, actions, detail);
  const list = $("bg-list");
  list.appendChild(el);
  while (list.children.length > BG_MAX) {
    const first = list.firstElementChild;
    if (!first) break;
    bgEntries.delete(first.dataset.bgId);
    first.remove();
  }
  const entry = {
    el, title, prog, meta, detail, actions, type, name, summary, input,
    children: new Map(), startedAt: Date.now(), done: false,
  };
  bgEntries.set(id, entry);
  bgRenderActions(id, entry);
  bgSync();
  list.scrollTop = list.scrollHeight;
}

// 每条的可操作项。运行中的后台进程/子代理能单独停;结束后能重跑;
// 终端类输出能复制与导出——这三样是"面板能干活"与"面板只是日志"的分界。
function bgRenderActions(id, entry) {
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
  if (!entry.done && (entry.type === "agent" || entry.type === "terminal")) {
    add(t("停止"), t("只停这一条,不影响本轮其它工具"), async () => {
      try {
        // 后台进程有独立句柄可单独停;子代理没有单条停止通道,只能停整轮。
        if (entry.name === "bash" || entry.name === "process") {
          const pid = entry.input?.process_id ?? entry.input?.processId;
          if (pid) {
            await invoke("run_tool_process_stop", { projectDir: currentProject, processId: String(pid) });
            toast(t("已停止该后台进程"));
            return;
          }
        }
        await invoke("stop_run", { sessionId: activeSessionId });
        toast(t("已请求停止"));
      } catch (error) {
        toastError(`${t("停止失败")}:${error}`);
      }
    });
  }
  if (entry.done) {
    add(t("重跑"), t("把这次调用的参数填回输入框,确认后再执行"), () => {
      // 不直接重放:工具调用有副作用,必须经用户确认。填回输入框是最轻的确认方式。
      const text = `重跑这次调用:${entry.name} ${entry.summary}\n参数:\n${JSON.stringify(entry.input ?? {}, null, 2)}`;
      $("prompt").value = text;
      $("prompt").focus();
      toast(t("已填入输入框,确认后发送"));
    });
  }
  if (bgIsTerminal(entry.name)) {
    add(t("复制"), t("复制完整输出"), async () => {
      await navigator.clipboard.writeText(bgPlainText(entry));
      toast(t("已复制"));
    });
    add(t("导出"), t("把完整输出存成文件"), () => {
      const blob = new Blob([bgPlainText(entry)], { type: "text/plain;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${entry.name}-${id}.txt`.replace(/[^\w.-]/g, "_");
      a.click();
      URL.revokeObjectURL(url);
      toast(t("已导出"));
    });
  }
}

function bgPlainText(entry) {
  return [
    `# ${entry.name} ${entry.summary}`,
    entry.input ? `\n## 入参\n${JSON.stringify(entry.input, null, 2)}` : "",
    `\n## 输出\n${entry.detail.textContent || entry.prog.textContent || ""}`,
  ].join("\n");
}
function highlightLine(container, text, language) {
  const pattern = /("(?:\\.|[^"])*"|'(?:\\.|[^'])*'|\/\/.*|#.*|\b\d+(?:\.\d+)?\b|\b(?:fn|let|const|function|class|return|if|else|for|while|pub|struct|use|import|from|true|false|null|None|async|await)\b)/g;
  let cursor = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > cursor) container.appendChild(document.createTextNode(text.slice(cursor, match.index)));
    const token = document.createElement("span");
    token.className = match[0].startsWith("//") || match[0].startsWith("#") ? "syntax-comment" : /^['"]/.test(match[0]) ? "syntax-string" : /^\d/.test(match[0]) ? "syntax-number" : "syntax-keyword";
    token.textContent = match[0];
    container.appendChild(token);
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) container.appendChild(document.createTextNode(text.slice(cursor)));
}

function renderDiff(display) {
  const block = document.createElement("div");
  block.className = "tool-display diff";
  let mode = "unified";
  const header = document.createElement("div");
  header.className = "diff-file-header";
  const label = document.createElement("span");
  label.textContent = `${display.path || t("文件")}  +${display.additions || 0} −${display.deletions || 0} · ${display.language || "text"}`;
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "ghost mini";
  toggle.setAttribute("aria-label", t("切换差异并排或统一视图"));
  toggle.setAttribute("aria-pressed", "false");
  toggle.textContent = t("并排");
  header.append(label, toggle);
  const body = document.createElement("div");
  body.className = "diff-body";
  const lines = display.lines?.length ? display.lines : (display.diff || "").split("\n").filter(Boolean).map((text) => ({ kind: text[0] === "+" ? "add" : text[0] === "-" ? "del" : "ctx", text: text.slice(1) }));
  function render() {
    body.innerHTML = "";
    body.className = `diff-body ${mode}`;
    if (mode === "unified") {
      let oldLine = 1;
      let newLine = 1;
      for (const line of lines) {
        const row = document.createElement("div");
        row.className = `diff-row ${line.kind || "ctx"}`;
        const oldNo = document.createElement("span");
        const newNo = document.createElement("span");
        oldNo.className = newNo.className = "diff-line-number";
        oldNo.textContent = line.old_line ?? (line.kind === "add" ? "" : oldLine++);
        newNo.textContent = line.new_line ?? (line.kind === "del" ? "" : newLine++);
        const text = document.createElement("code");
        highlightLine(text, line.text || "", display.language || "text");
        row.append(oldNo, newNo, text);
        body.appendChild(row);
      }
    } else {
      const rows = [];
      for (let i = 0; i < lines.length; i += 1) {
        const line = lines[i];
        if (line.kind === "del" && lines[i + 1]?.kind === "add") rows.push([line, lines[++i]]);
        else if (line.kind === "del") rows.push([line, null]);
        else if (line.kind === "add") rows.push([null, line]);
        else rows.push([line, line]);
      }
      for (const [left, right] of rows) {
        const row = document.createElement("div");
        row.className = "diff-split-row";
        for (const line of [left, right]) {
          const pane = document.createElement("div");
          pane.className = `diff-pane ${line?.kind || "empty"}`;
          if (line) {
            const no = document.createElement("span");
            no.className = "diff-line-number";
            no.textContent = line.old_line ?? line.new_line ?? "";
            const text = document.createElement("code");
            highlightLine(text, line.text || "", display.language || "text");
            pane.append(no, text);
          }
          row.appendChild(pane);
        }
        body.appendChild(row);
      }
    }
  }
  toggle.addEventListener("click", () => {
    mode = mode === "unified" ? "split" : "unified";
    toggle.textContent = mode === "unified" ? t("并排") : t("统一");
    toggle.setAttribute("aria-pressed", String(mode === "split"));
    render();
  });
  block.append(header, body);
  render();
  return block;
}
function appendDisplayBlock(parent, display) {
  if (!display) return;
  if (display.kind === "diff") {
    parent.appendChild(renderDiff(display));
  } else if (display.kind === "terminal") {
    const block = document.createElement("div");
    block.className = "tool-display term";
    // D-237:活动面板展开区优先展示完整输出(full),而不是 4000 截断的 output。
    const out = display.full ?? display.output ?? "";
    block.textContent = `$ ${display.command}\n${out}`;
    parent.appendChild(block);
  } else if (display.kind === "create") {
    const block = document.createElement("div");
    block.className = "tool-display term";
    block.textContent = `${t("新建")} ${display.path}(${display.bytes} bytes)\n${display.preview}`;
    parent.appendChild(block);
  }
}
function bgProgress(id, text, trace) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  if (text) entry.prog.textContent = text;
  if (!trace) return;
  entry.detail.classList.add("trace-detail");
  let child = entry.children.get(trace.child_id);
  if (trace.phase === "start") {
    if (!child) {
      const row = document.createElement("div");
      row.className = "bg-child running";
      const head = document.createElement("div");
      head.className = "bg-child-head";
      head.textContent = `${trace.name} ${trace.summary || ""}`;
      const meta = document.createElement("div");
      meta.className = "bg-child-meta";
      row.append(head, meta);
      entry.detail.appendChild(row);
      child = { row, head, meta };
      entry.children.set(trace.child_id, child);
      entry.el.classList.add("has-detail");
    }
  } else if (child) {
    child.row.classList.remove("running");
    child.row.classList.add(trace.ok ? "ok" : "err");
    child.meta.textContent = trace.preview || (trace.ok ? t("完成") : t("失败"));
    appendDisplayBlock(child.row, trace.display);
  }
}

function bgEnd(id, ok, preview, display) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  entry.done = true;
  entry.el.classList.remove("running");
  entry.el.classList.add(ok ? "ok" : "err");
  entry.prog.textContent = preview || (ok ? t("完成") : t("失败"));
  // 元信息一行说清:成败、耗时、子代理内部调用数。此前只有一个秒数,
  // 看不出成没成,也看不出子代理到底干了多少活(R-095 验收 ⑤)。
  const ms = Date.now() - entry.startedAt;
  const elapsed = ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
  const bits = [ok ? `✓ ${t("成功")}` : `✕ ${t("失败")}`, elapsed];
  if (entry.type === "agent") bits.push(`${t("内部调用")} ${entry.children.size}`);
  entry.meta.textContent = bits.join(" · ");
  // 结构化详情进面板内展开区(diff/终端/新建/todo)。
  const d = display;
  if (d?.kind === "diff") {
    entry.title.textContent += `  +${d.additions} −${d.deletions}`;
  }
  appendDisplayBlock(entry.detail, d);
  if (!ok && preview) {
    const err = document.createElement("div");
    err.className = "tool-display term";
    err.textContent = preview;
    entry.detail.appendChild(err);
  }
  if (entry.detail.children.length) entry.el.classList.add("has-detail");
  bgRenderActions(id, entry);
  bgSync();
}
// 历史轨迹回放(D-208)。run.trace 事件本来就带 name/summary/ok/durationMs,
// 旧实现读的却是不存在的 event.text/event.trace,name 硬编码 "task"、标题硬编码
// "历史子代理轨迹"——上百条同名条目,类型筛选也跟着失真;而且标终态后没重渲染
// 动作区,停止按钮残留在早已结束的历史轨迹上。回放条目一律终态、无停止按钮。
function renderRecoveredTraces(payloads) {
  for (const payload of payloads || []) {
    for (const event of payload.events || []) {
      if (!event.id) continue; // turn.started / context.compacted 等无 id 事件不进列表
      if (event.kind === "tool.started") {
        if (!bgEntries.has(event.id)) {
          bgAdd(event.id, event.name || "task", event.summary || t("历史子代理轨迹"));
        }
      } else if (event.kind === "tool.completed") {
        const entry = bgEntries.get(event.id);
        if (!entry) continue;
        entry.done = true;
        entry.el.classList.remove("running");
        const failed = event.ok === false;
        entry.el.classList.add(failed ? "err" : "ok");
        entry.prog.textContent = failed && event.error ? String(event.error) : t("历史轨迹");
        const seconds = Number(event.durationMs);
        entry.meta.textContent =
          Number.isFinite(seconds) && seconds >= 1000
            ? `${t("回放")} · ${Math.round(seconds / 1000)}s`
            : t("回放");
        bgRenderActions(event.id, entry);
      }
    }
  }
  // 只 started 没 completed 的(轮次中断):同样收敛终态,不留假 running 与停止按钮。
  for (const [id, entry] of bgEntries) {
    if (entry.done) continue;
    entry.done = true;
    entry.el.classList.remove("running");
    entry.el.classList.add("err");
    entry.prog.textContent = t("历史轨迹");
    entry.meta.textContent = `${t("回放")} · ${t("无结果(轮次中断)")}`;
    bgRenderActions(id, entry);
  }
  bgSync();
}

function bgClear() {
  for (const entry of bgEntries.values()) entry.el.remove();
  bgEntries.clear();
  diffSummary.clear();
  $("bg-list").innerHTML = "";
  renderDiffSummary();
  bgSync();
}
// 中止/出错时把仍在跑的条目标记为中止,不再空转。
function bgAbortRunning(label) {
  for (const entry of bgEntries.values()) {
    if (!entry.done) {
      entry.done = true;
      entry.el.classList.remove("running");
      entry.el.classList.add("err");
      entry.prog.textContent = label;
    }
  }
}
setInterval(() => {
  for (const entry of bgEntries.values()) {
    if (!entry.done) entry.meta.textContent = `${Math.round((Date.now() - entry.startedAt) / 1000)}s`;
  }
}, 1000);

// ---------- 当前进展:侧边栏实时状态卡(把握 agent 进度,不用等它汇报) ----------
const liveTextSources = new Map();
function syncDynamicUiLanguage() {
  if (statusTextSource) setStatus(statusTextSource, statusRunning);
  for (const [id, source] of liveTextSources) {
    const el = $(id);
    if (!el) continue;
    el.textContent = localizeDynamic(source);
    el.title = localizeDynamic(source);
  }
  if (!$("context-detail")?.classList.contains("hidden")) renderContextDetail();
  renderAutoStatus(autoStopReason);
}
function liveSet(id, text) {
  const el = $(id);
  const source = String(text ?? "");
  if (!source) {
    liveTextSources.delete(id);
    el.classList.add("hidden");
    return;
  }
  liveTextSources.set(id, source);
  el.classList.remove("hidden");
  el.textContent = localizeDynamic(source);
  el.title = localizeDynamic(source);
}
function liveIdle(label) {
  const turn = $("live-turn");
  const source = String(label ?? "");
  liveTextSources.set("live-turn", source);
  turn.textContent = localizeDynamic(source);
  turn.classList.add("dim");
  liveSet("live-action", "");
}
function liveTurn(text) {
  const turn = $("live-turn");
  const source = String(text ?? "");
  liveTextSources.set("live-turn", source);
  turn.textContent = localizeDynamic(source);
  turn.classList.remove("dim");
}

