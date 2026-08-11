// ---------- 活动面板(R-037/R-168):终端命令与失败调用入列,详情点击展开 ----------
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
  panel.replaceChildren(buildDiffTree(files));
}

// R-133:diff 汇总按路径层级构成目录树,替代原来的一长串平铺路径——
// 目录可折叠,文件行缩进在所属目录下,+/- 计数与 diff 行同色,视觉清爽不重叠。
function buildDiffTree(files) {
  const root = { name: "", children: new Map(), items: [] };
  for (const item of files) {
    const segs = item.path.split("/").filter(Boolean);
    let node = root;
    for (const seg of segs.slice(0, -1)) {
      if (!node.children.has(seg)) node.children.set(seg, { name: seg, children: new Map(), items: [] });
      node = node.children.get(seg);
    }
    node.items.push(item);
  }
  const wrap = document.createElement("div");
  wrap.className = "diff-tree";
  appendDiffNode(wrap, root, 0);
  return wrap;
}

function appendDiffNode(container, node, depth) {
  // 目录在前、文件在后,目录可折叠(▸/▾),文件行保留增删计数。
  for (const dir of node.children.values()) {
    const box = document.createElement("div");
    box.className = "diff-dir";
    const head = document.createElement("button");
    head.type = "button";
    head.className = "diff-dir-head";
    head.setAttribute("aria-expanded", "true");
    head.textContent = `▾ ${dir.name}`;
    const body = document.createElement("div");
    body.className = "diff-dir-body";
    head.addEventListener("click", () => {
      const open = !body.classList.contains("hidden");
      body.classList.toggle("hidden", open);
      head.textContent = `${open ? "▸" : "▾"} ${dir.name}`;
      head.setAttribute("aria-expanded", String(!open));
    });
    box.append(head, body);
    appendDiffNode(body, dir, depth + 1);
    container.appendChild(box);
  }
  for (const item of node.items) {
    const row = document.createElement("div");
    row.className = "diff-summary-row";
    row.style.paddingLeft = `${8 + depth * 14}px`;
    const name = document.createElement("span");
    name.textContent = item.path;
    name.title = item.path;
    const counts = document.createElement("span");
    counts.className = "diff-summary-counts";
    const add = document.createElement("span");
    add.className = "diff-add";
    add.textContent = `+${item.additions}`;
    const del = document.createElement("span");
    del.className = "diff-del";
    del.textContent = `−${item.deletions}`;
    counts.append(add, del);
    row.append(name, counts);
    container.appendChild(row);
  }
}

function bgSync() {
  // 面板开关只由用户控制;工具事件只能更新内容,不能擅自开关。
  syncActivityPanel();
  syncBgRoleFilterOptions();
  applyBgFilters();
  renderBgGroups();
}

// R-184 P2:角色筛选下拉的选项随条目动态刷新(全部 + 当前有 role 的去重角色),
// 选中值在重建时保留。放在 bgSync 里随条目增减一并维护,不单独监听。
function syncBgRoleFilterOptions() {
  const select = $("bg-role-filter");
  if (!select) return;
  const current = select.value;
  const roles = [...new Set([...bgEntries.values()].map((entry) => entry.role).filter(Boolean))].sort();
  select.replaceChildren();
  const all = document.createElement("option");
  all.value = "all";
  all.textContent = t("全部子代理");
  select.appendChild(all);
  for (const role of roles) {
    const option = document.createElement("option");
    option.value = role;
    option.textContent = role;
    select.appendChild(option);
  }
  select.value = roles.includes(current) || current === "all" ? current : "all";
}
// R-168:活动栏不是完整工具审计(主对话已有内联工具块)，只保留用户需要盯进度的
// 终端命令；任意工具失败仍会在结束时补建，避免把故障信号一起静默掉。
//
// R-173:例外是编排对象按角色表派发的勘察/复核子代理。它们不经模型 tool call,
// 主对话里没有"内联工具块"这个兜底,活动面板是它们唯一的可见处——按 R-168 静默
// 等于把 5 勘察 + 3 复核的全部内部进度直接丢掉。后端(a921b14)用既有三事件上抛,
// input.phase 就是分区依据(scouting/review),据此放行;模型自己派的 task 不带
// phase,仍旧静默,R-168 的口径不动。
const ORCH_PHASES = new Set(["scouting", "review"]);
function orchPhaseOf(input) {
  const phase = input?.phase;
  return typeof phase === "string" && ORCH_PHASES.has(phase) ? phase : null;
}
function orchPhaseLabel(phase) {
  // 写成两个字面量调用而不是查表:i18n 冒烟只扫源码里带字符串常量的翻译调用,
  // 查表写法(t(MAP[phase]))会整条绕过 key 覆盖率检查,英文界面上就地漏译。
  return phase === "scouting" ? t("勘察") : t("复核");
}
function isActivityTool(name, input) {
  return bgIsTerminal(name) || (name === "task" && orchPhaseOf(input) !== null);
}

const BG_TOOL_TYPES = {
  bash: "terminal", process: "terminal",
  read: "file", write: "file", edit: "file", multiedit: "file", glob: "file", grep: "file",
  req: "tracker", defect: "tracker", goal: "tracker", source: "tracker", finding: "tracker", decision: "tracker",
  task: "agent",
  memory_note: "memory", memory_search: "memory", memory_stats: "memory",
};
// 除终端命令外，所有成功工具调用均静默；未知新工具也默认静默，避免功能扩展后
// 活动栏重新被灌满。失败路径由 bgFinishQuiet 补建真实错误条目。
const bgPending = new Map(); // call_id -> {name, summary, input, startedAt}
function bgQuiet(name, input) {
  return !isActivityTool(name, input);
}
function bgStartQuiet(id, name, summary, input) {
  if (!id) return;
  bgPending.set(id, { name, summary, input, startedAt: Date.now() });
  // 悬挂上限:异常中断的静默调用不该无限累积。
  if (bgPending.size > BG_MAX) bgPending.delete(bgPending.keys().next().value);
}
// 收尾一条静默调用。成功→无声丢弃返回 true;失败→补建条目返回 false,
// 让调用方继续走 bgEnd 把错误详情画出来。
function bgFinishQuiet(id, ok) {
  const pending = bgPending.get(id);
  if (!pending) return false;
  bgPending.delete(id);
  if (ok) return true;
  bgAdd(id, pending.name, pending.summary, pending.input);
  const entry = bgEntries.get(id);
  if (entry) entry.startedAt = pending.startedAt;
  return false;
}
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
  // R-184 P2:按子代理角色筛活动轨迹。只对编排派发(带 role)的条目生效,
  // 其它工具一律通过——角色筛是活动面板里的一个维度,不是把活动栏变成子代理专用。
  role: localStorage.getItem("kz-bg-role") || "all",
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
    const roleOk = bgFilters.role === "all" || entry.role === bgFilters.role;
    const visible = typeOk && statusOk && roleOk;
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

// R-173:编排派发的子代理按 input.phase 分区(勘察 / 复核)。这是用户要的 Running/
// Finished 分区的雏形,复用 #bg-list 而不是另起一个平行面板——独立子代理面板归 R-174,
// 本轮只把丢掉的信息接回来。
const bgGroups = new Map(); // phase -> {wrap, head, body}
function bgGroupBody(phase) {
  const list = $("bg-list");
  if (!phase) return list;
  let group = bgGroups.get(phase);
  if (!group) {
    const wrap = document.createElement("div");
    wrap.className = `bg-group bg-group-${phase}`;
    wrap.dataset.bgPhase = phase;
    const head = document.createElement("div");
    head.className = "bg-group-head";
    const body = document.createElement("div");
    body.className = "bg-group-body";
    wrap.append(head, body);
    list.appendChild(wrap);
    group = { wrap, head, body };
    bgGroups.set(phase, group);
  }
  return group.body;
}
// 组标题给"完成数/总数",这是本轮最直接的推进度读数;整组被筛选清空就收起,
// 不留一个指向空气的标题。
function renderBgGroups() {
  for (const [phase, group] of bgGroups) {
    const rows = [...bgEntries.values()].filter((entry) => entry.phase === phase);
    if (!rows.length) {
      group.wrap.remove();
      bgGroups.delete(phase);
      continue;
    }
    const done = rows.filter((entry) => entry.done).length;
    group.head.textContent = `${orchPhaseLabel(phase)} · ${done}/${rows.length}`;
    group.wrap.dataset.bgGroupDone = `${done}/${rows.length}`;
    group.wrap.classList.toggle("hidden", !rows.some((entry) => !entry.el.classList.contains("hidden")));
  }
}

/// 差异汇总必须独立于活动面板的过滤:diff 来自 write/edit,而这两个工具已不进活动面板,
/// 原先把累计写在 bgEnd 里就等于永远拿不到数据,#diff-summary 变成接不到数据源的空壳(D-137)。
function recordDiffSummary(display) {
  if (display?.kind !== "diff") return;
  diffSummary.set(display.path || `#${diffSummary.size + 1}`, {
    path: display.path || t("未命名文件"),
    additions: display.additions || 0,
    deletions: display.deletions || 0,
  });
  renderDiffSummary();
}

// 完整入参永远可展开:summary 是一行摘要,复核"到底拿什么参数调的"要看原文。
// 编排派发的子代理尤其需要——input.prompt 就是派给该角色的完整指令。
function bgAppendArgs(entry, input) {
  if (!input || !Object.keys(input).length) return;
  const args = document.createElement("pre");
  args.className = "tool-display term bg-args";
  args.textContent = JSON.stringify(input, null, 2);
  entry.detail.appendChild(args);
  entry.el.classList.add("has-detail");
}

// 当前正在用的工具名。工具结束后保留名字但转灰(.idle):子代理在两次工具调用之间
// 是在思考,清空会让这一行大部分时间是空的,反而看不出它刚干了什么。
function bgSetCurrentTool(entry, name, running) {
  if (!entry.current) return;
  const label = String(name ?? "");
  entry.current.textContent = label ? `⚙ ${label}` : "";
  entry.current.classList.toggle("hidden", !label);
  entry.current.classList.toggle("idle", Boolean(label) && !running);
  // 写入去向探针:值断言看不出"写对了但写错了地方",dataset 把去向也钉死。
  entry.el.dataset.bgCurrentTool = label;
}

// 运行中的元信息一行。编排派发的子代理要的是"跑了多久 + 内部调用了几次工具",
// 只有秒数看不出它到底在推进还是卡死。1 秒心跳与建条时共用同一段,建条即可读。
function bgTick(entry) {
  const seconds = Math.round((Date.now() - entry.startedAt) / 1000);
  entry.el.dataset.bgElapsed = String(seconds);
  entry.meta.textContent = entry.phase
    ? `${t("运行中")} · ${seconds}s · ${t("内部调用")} ${entry.children.size}`
    : `${seconds}s`;
}

// 角色名就是 id,而角色跨轮复用(每个自主推进轮都有 architecture_scout)。同名角色
// 再次派发时必须原地复位:被 bgEntries.has(id) 直接挡掉的话,第二轮的 progress/end
// 会全写进上一轮那条已终态的行,面板从此定格在上一轮。
function bgRestart(id, summary, input) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  entry.done = false;
  entry.startedAt = Date.now();
  entry.children.clear();
  entry.input = input;
  entry.live = null;
  entry.summary = toolCallSummary(entry.name, input) || String(summary ?? "");
  entry.target.textContent = entry.summary;
  entry.title.title = entry.summary;
  entry.el.classList.remove("ok", "err", "timeout", "has-detail");
  entry.el.classList.add("running");
  entry.el.dataset.bgStatus = "running";
  entry.prog.textContent = `… ${t("子代理启动中")}`;
  entry.detail.innerHTML = "";
  entry.detail.classList.add("hidden");
  bgAppendArgs(entry, input);
  bgSetCurrentTool(entry, null, false);
  bgTick(entry);
  bgRenderActions(id, entry);
  bgSync();
}

function bgAdd(id, name, summary, input) {
  if (!id) return;
  const phase = name === "task" ? orchPhaseOf(input) : null;
  if (bgEntries.has(id)) {
    if (phase) bgRestart(id, summary, input);
    return;
  }
  const type = bgToolType(name);
  const el = document.createElement("div");
  el.className = `bg-entry running bg-type-${type}${phase ? " bg-orch" : ""}`;
  el.dataset.bgId = id;
  el.dataset.bgTool = name;
  el.dataset.bgStatus = "running";
  if (phase) {
    el.dataset.bgPhase = phase;
    el.dataset.bgRole = id;
  }
  const title = document.createElement("button");
  title.type = "button";
  title.className = "bg-title";
  title.setAttribute("aria-label", t("展开或收起后台任务详情"));
  title.setAttribute("aria-expanded", "false");
  // 工具名与目标分开呈现:此前拼成一行长文本被 ellipsis 截断,看不出改的是哪个文件、
  // 跑的是哪条命令——"打开也没啥用"的直接原因(R-095 验收 ⑤)。
  const toolName = document.createElement("span");
  toolName.className = "bg-tool";
  // 编排派发的这批里,"task" 对所有 8 条都一样,毫无区分度;角色名才是身份。
  toolName.textContent = phase ? id : name;
  // R-184 P2:编排派发的子代理轨迹带角色色点(●,按角色名确定性取色),与主对话
  // 折叠组同源;色点旁始终有角色名文本,颜色只作辅助不唯一承载区分。
  const dot = phase ? document.createElement("span") : null;
  if (dot) {
    dot.className = `bg-dot line-accent-${agentRoleAccent(id)}`;
    dot.setAttribute("aria-hidden", "true");
  }
  const target = document.createElement("span");
  target.className = "bg-target";
  // 后端 summarize_input(kanzei-core/src/runner/compaction.rs)把整坨入参 JSON 截到 160 字,
  // 对所有工具一视同仁——edit 于是显示成 `{"new_string":"…","old_strin…`,完全看不出改的是哪个文件。
  // 标题优先走前端按工具名挑字段的 toolCallSummary(05-chat-render.js,主对话工具块同款),
  // 挑不出来再回落后端 summary(回放事件不带 input,靠的就是这一级),最后回落空串。
  const shown = toolCallSummary(name, input) || String(summary ?? "");
  target.textContent = shown;
  title.append(...(dot ? [dot] : []), toolName, target);
  // 所属阶段随条目走,不只挂在组标题上:筛选/滚动之后单看一行也要知道它是勘察还是复核。
  if (phase) {
    const badge = document.createElement("span");
    badge.className = "bg-phase-badge";
    badge.textContent = orchPhaseLabel(phase);
    title.append(badge);
  }
  title.title = shown;
  const prog = document.createElement("div");
  prog.className = "bg-prog";
  prog.textContent = name === "task" ? `… ${t("子代理启动中")}` : "…";
  // 当前正在用的工具名单独一行:bg-meta 每秒被心跳整行重写,挂那儿会被冲掉。
  const current = phase ? document.createElement("div") : null;
  if (current) current.className = "bg-current hidden";
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
  el.append(title, prog, ...(current ? [current] : []), meta, actions, detail);
  bgGroupBody(phase).appendChild(el);
  const list = $("bg-list");
  const entry = {
    // summary 存显示值:它的两个消费方(重跑填词、导出文件头)都是把它当"人类可读的一行标识"用,
    // 且都在同一段文本里另附了完整入参 JSON,存裸 JSON 只会变成两份 JSON 叠在一起。
    el, title, target, prog, current, meta, detail, actions, type, name, phase, summary: shown, input,
    role: phase ? id : null, children: new Map(), startedAt: Date.now(), done: false,
  };
  bgEntries.set(id, entry);
  bgAppendArgs(entry, input);
  bgTick(entry);
  // 上限裁剪改按登记表走:条目现在可能嵌在分组容器里,再按 #bg-list 的直接子节点裁剪
  // 会把整组连同其中多条一起摘掉、却只注销一个 id,剩下的 id 变成指向游离节点的幽灵条目。
  while (bgEntries.size > BG_MAX) {
    const oldestId = bgEntries.keys().next().value;
    bgEntries.get(oldestId)?.el.remove();
    bgEntries.delete(oldestId);
  }
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
// 工具执行中的增量输出(kz:tool-progress,bash 等长任务):展开区里逐段追加,
// 收起状态下进度行显示最后一行——装依赖/发版这类长命令"跑到哪了"一眼可见。
// 只保留末尾 16k 字符:进度要的是尾部,完整输出等 ToolEnd 的终态块。
const BG_STREAM_MAX = 16000;
function bgStream(id, chunk) {
  const entry = bgEntries.get(id);
  if (!entry || entry.done || !chunk) return;
  if (!entry.live) {
    entry.live = document.createElement("pre");
    entry.live.className = "tool-display term bg-live";
    entry.detail.appendChild(entry.live);
    entry.el.classList.add("has-detail");
  }
  const text = (entry.live.textContent + chunk).slice(-BG_STREAM_MAX);
  entry.live.textContent = text;
  const lastLine = text.trimEnd().split("\n").pop() || "";
  if (lastLine) entry.prog.textContent = lastLine.slice(0, 160);
  if (!entry.detail.classList.contains("hidden")) entry.live.scrollTop = entry.live.scrollHeight;
}

function bgProgress(id, text, trace) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  if (text) entry.prog.textContent = text;
  if (!trace) {
    // 纯轮次进度(trace 为 null)也让心跳行跟上,不必等下一次 1 秒 tick。
    if (!entry.done) bgTick(entry);
    return;
  }
  entry.detail.classList.add("trace-detail");
  bgSetCurrentTool(entry, trace.name, trace.phase === "start");
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
  // 调用数在 children 落定后再刷,先刷会永远少算一次。
  if (!entry.done) bgTick(entry);
}

function bgEnd(id, ok, preview, display) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  // 实时流是执行期的临时视图,终态由 display 的完整输出接管,避免同一份输出双份并存。
  if (entry.live) {
    entry.live.remove();
    entry.live = null;
  }
  entry.done = true;
  entry.el.classList.remove("running");
  entry.el.classList.add(ok ? "ok" : "err");
  // 超时角色后端固定发 ok=false + preview「(超时,未产出结果)」。它与"跑了但失败"
  // 是两回事:超时说明该角色被屏障砍掉、什么都没产出,视觉上必须能与失败分开。
  const timedOut = !ok && /超时/.test(String(preview ?? ""));
  if (timedOut) entry.el.classList.add("timeout");
  entry.el.dataset.bgStatus = ok ? "ok" : timedOut ? "timeout" : "err";
  bgSetCurrentTool(entry, null, false);
  entry.prog.textContent = preview || (ok ? t("完成") : t("失败"));
  // 元信息一行说清:成败、耗时、子代理内部调用数。此前只有一个秒数,
  // 看不出成没成,也看不出子代理到底干了多少活(R-095 验收 ⑤)。
  const ms = Date.now() - entry.startedAt;
  const elapsed = ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
  const bits = [ok ? `✓ ${t("成功")}` : timedOut ? `⏱ ${t("超时")}` : `✕ ${t("失败")}`, elapsed];
  if (entry.type === "agent") bits.push(`${t("内部调用")} ${entry.children.size}`);
  entry.meta.textContent = bits.join(" · ");
  // 结构化详情进面板内展开区(diff/终端/新建/todo)。
  const d = display;
  // 增减行数只能追加到 .bg-target 里:对整个 title 按钮做 textContent += 等于把
  // .bg-tool/.bg-target 两个子 span 拍平成单个文本节点,工具名/目标的分栏结构当场消失。
  if (d?.kind === "diff" && entry.target) {
    entry.target.textContent += `  +${d.additions} −${d.deletions}`;
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
        // 回放同样遵守小工具降噪:成功的静默调用不进列表,失败的照常补建。
        if (bgQuiet(event.name)) {
          bgStartQuiet(event.id, event.name, event.summary || "", null);
        } else if (!bgEntries.has(event.id)) {
          bgAdd(event.id, event.name || "task", event.summary || t("历史子代理轨迹"));
        }
      } else if (event.kind === "tool.completed") {
        if (bgFinishQuiet(event.id, event.ok !== false)) continue;
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
  // 回放里没等到 completed 的静默调用直接丢弃,不让残留 id 污染后续实时判定。
  bgPending.clear();
  bgSync();
}

function bgClear() {
  for (const entry of bgEntries.values()) entry.el.remove();
  bgEntries.clear();
  bgPending.clear();
  bgGroups.clear();
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
      entry.el.dataset.bgStatus = "err";
      bgSetCurrentTool(entry, null, false);
      entry.prog.textContent = label;
    }
  }
  renderBgGroups();
}
setInterval(() => {
  for (const entry of bgEntries.values()) {
    if (!entry.done) bgTick(entry);
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

