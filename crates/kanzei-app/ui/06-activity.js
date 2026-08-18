// ---------- 活动面板(R-037):完整工具活动入列,详情点击展开 ----------
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
  renderBgSections();
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
// 活动栏现在保留完整工具轨迹。主对话仍然是主要阅读区,但活动面板需要回答
// 「刚才实际做了什么、现在卡在哪、失败在哪里」,不能把 read/grep/edit 等成功调用
// 静默到只剩终端和错误,否则用户看到的就是空白面板。
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
  return Boolean(name);
}

const BG_TOOL_TYPES = {
  bash: "terminal", process: "terminal",
  read: "file", write: "file", edit: "file", multiedit: "file", glob: "file", grep: "file",
  req: "tracker", defect: "tracker", idea: "tracker", source: "tracker", finding: "tracker", decision: "tracker",
  task: "agent",
  memory_note: "memory", memory_search: "memory", memory_stats: "memory",
};
// 保留这个待收尾表是为了兼容历史回放路径；当前实时工具调用统一进入活动栏，
// 因此 bgQuiet 固定返回 false，成功/失败都保留完整轨迹。
const bgPending = new Map(); // call_id -> {name, summary, input, startedAt}
function bgQuiet(name, input) {
  return false;
}
function bgStartQuiet(id, name, summary, input) {
  if (!id) return;
  bgPending.set(id, { name, summary, input, startedAt: Date.now() });
  // 悬挂上限:异常中断的静默调用不该无限累积。
  if (bgPending.size > BG_MAX) bgPending.delete(bgPending.keys().next().value);
}
// 收尾历史兼容路径中的待定调用。成功返回 true；失败补建真实条目，
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
const bgGroups = new Map(); // `${section}|${phase}` -> {wrap, head, body}
const BG_SECTION_BODY = { running: "bg-running", attention: "bg-attention", done: "bg-done" };
let bgDoneOpen = localStorage.getItem("kz-bg-done-open") === "1";
// 条目该落哪一段。成功判据只认 ToolEnd 的机器可读 outcome(经 activityOutcomeView
// 折算成 cls),不看文案、不反推 DOM:成功与 noop 收进折叠区,其余(失败/需确认/
// 需修正/超时)留在「需要关注」——收错地方比不收更糟。
function bgSectionFor(entry) {
  if (!entry.done) return "running";
  return entry.cls === "ok" || entry.cls === "noop" ? "done" : "attention";
}
function bgPlace(entry) {
  const section = bgSectionFor(entry);
  if (entry.section === section && entry.el.parentNode) return;
  entry.section = section;
  entry.el.dataset.bgSection = section;
  bgGroupBody(entry.phase, section).appendChild(entry.el);
}
function bgGroupBody(phase, section = "running") {
  const host = $(BG_SECTION_BODY[section]) ?? $("bg-list");
  if (!phase) return host;
  // 分组键带上段:同一个 phase 在三段里各有一份组容器,否则条目落位后又被组容器拽回去。
  const key = `${section}|${phase}`;
  let group = bgGroups.get(key);
  if (!group) {
    const wrap = document.createElement("div");
    wrap.className = `bg-group bg-group-${phase}`;
    wrap.dataset.bgPhase = phase;
    wrap.dataset.bgSection = section;
    const head = document.createElement("div");
    head.className = "bg-group-head";
    const body = document.createElement("div");
    body.className = "bg-group-body";
    wrap.append(head, body);
    host.appendChild(wrap);
    group = { wrap, head, body, phase, section };
    bgGroups.set(key, group);
  }
  return group.body;
}
// 组标题给"完成数/总数",这是本轮最直接的推进度读数;整组被筛选清空就收起,
// 不留一个指向空气的标题。
function renderBgGroups() {
  for (const [key, group] of bgGroups) {
    const mine = [...bgEntries.values()].filter((e) => e.phase === group.phase && e.section === group.section);
    if (!mine.length) {
      group.wrap.remove();
      bgGroups.delete(key);
      continue;
    }
    // 分母用**跨段**的全量:段内分母会让「勘察 · 0/2」和「勘察 · 3/3」同屏自相矛盾。
    const all = [...bgEntries.values()].filter((e) => e.phase === group.phase);
    const done = all.filter((entry) => entry.done).length;
    group.head.textContent = `${orchPhaseLabel(group.phase)} · ${done}/${all.length}`;
    group.wrap.dataset.bgGroupDone = `${done}/${all.length}`;
    group.wrap.classList.toggle("hidden", !mine.some((entry) => !entry.el.classList.contains("hidden")));
  }
}
// 三段的计数、空态与折叠。运行中那段额外给出终端条数——用户开这个面板就是要看
// 「现在有几个终端在跑、跑的是什么」。
function renderBgSections() {
  const visible = (section) => [...bgEntries.values()]
    .filter((e) => e.section === section && !e.el.classList.contains("hidden"));
  const running = visible("running");
  const attention = visible("attention");
  const done = visible("done");
  const terminals = running.filter((e) => e.name === "bash" || e.type === "bash").length;
  const runCount = $("bg-running-count");
  if (runCount) {
    runCount.textContent = running.length
      ? (terminals ? `${running.length} · ${t("终端")} ${terminals}` : String(running.length))
      : "";
  }
  const emptyRow = $("bg-running-empty");
  if (emptyRow) emptyRow.classList.toggle("hidden", running.length > 0);
  const attentionSection = $("bg-section-attention");
  if (attentionSection) attentionSection.classList.toggle("hidden", attention.length === 0);
  const attentionCount = $("bg-attention-count");
  if (attentionCount) attentionCount.textContent = attention.length ? String(attention.length) : "";
  const doneSection = $("bg-section-done");
  if (doneSection) doneSection.classList.toggle("hidden", done.length === 0);
  const doneCount = $("bg-done-count");
  if (doneCount) doneCount.textContent = done.length ? String(done.length) : "";
  const doneBody = $("bg-done");
  const toggle = $("bg-done-toggle");
  if (doneBody) doneBody.classList.toggle("hidden", !bgDoneOpen);
  if (toggle) {
    toggle.setAttribute("aria-expanded", String(bgDoneOpen));
    const caret = toggle.querySelector(".bg-section-caret");
    if (caret) caret.textContent = bgDoneOpen ? "▾" : "▸";
  }
  const clear = $("bg-clear-done");
  if (clear) clear.classList.toggle("hidden", done.length === 0);
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
function bgRestart(id, summary, input, sessionId = activeSessionId) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  entry.done = false;
  entry.startedAt = Date.now();
  entry.children.clear();
  entry.input = input;
  entry.sessionId = sessionId;
  entry.live = null;
  entry.summary = toolCallSummary(entry.name, input) || String(summary ?? "");
  entry.target.textContent = entry.summary;
  entry.title.title = entry.summary;
  entry.el.classList.remove("ok", "err", "timeout", "has-detail");
  entry.el.classList.add("running");
  entry.el.dataset.bgStatus = "running";
  // 同名角色第二轮:从折叠区回到运行中段,否则重跑的东西藏在收起来的抽屉里。
  entry.cls = null;
  entry.outcomeState = null;
  bgPlace(entry);
  entry.prog.textContent = `… ${t("子代理启动中")}`;
  entry.detail.innerHTML = "";
  entry.detail.classList.add("hidden");
  bgAppendArgs(entry, input);
  bgSetCurrentTool(entry, null, false);
  bgTick(entry);
  bgRenderActions(id, entry);
  bgSync();
}

function bgAdd(id, name, summary, input, sessionId = activeSessionId) {
  if (!id) return;
  const phase = name === "task" ? orchPhaseOf(input) : null;
  if (bgEntries.has(id)) {
    if (phase) bgRestart(id, summary, input, sessionId);
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
  const entry = {
    // summary 存显示值:它的两个消费方(重跑填词、导出文件头)都是把它当"人类可读的一行标识"用,
    // 且都在同一段文本里另附了完整入参 JSON,存裸 JSON 只会变成两份 JSON 叠在一起。
    el, title, target, prog, current, meta, detail, actions, type, name, phase, summary: shown, input, sessionId,
    role: phase ? id : null, children: new Map(), startedAt: Date.now(), done: false,
    // 落位判据存在条目上,不从 DOM class 反推(反推在 warn/noop 上一直是错的)。
    cls: null, outcomeState: null, section: null,
  };
  bgEntries.set(id, entry);
  bgPlace(entry);
  bgAppendArgs(entry, input);
  bgTick(entry);
  // 上限裁剪改按登记表走:条目现在可能嵌在分组容器里,再按 #bg-list 的直接子节点裁剪
  // 会把整组连同其中多条一起摘掉、却只注销一个 id,剩下的 id 变成指向游离节点的幽灵条目。
  // 优先摘已完成的:在跑的条目正是用户开着这个面板要看的东西,绝不能被裁掉。
  while (bgEntries.size > BG_MAX) {
    const victim = [...bgEntries].find(([, e]) => e.section === "done")
      ?? [...bgEntries].find(([, e]) => e.done);
    if (!victim) break;
    victim[1].el.remove();
    bgEntries.delete(victim[0]);
  }
  bgRenderActions(id, entry);
  bgSync();
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
        const processId = processItems.find((item) => item.session_id === entry.sessionId)?.id
          || (entry.sessionId === activeSessionId ? activeProcessId : null);
        if (entry.name === "task") {
          await invoke("stop_task", { projectDir: currentProject, processId, taskId: String(id) });
          toast(t("已请求停止该子代理"));
        } else {
          await invoke("stop_run", { projectDir: currentProject, processId });
          toast(t("已请求停止"));
        }
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
    const view = activityOutcomeView(trace.ok, trace.outcome);
    child.row.classList.remove("running");
    child.row.classList.add(view.cls);
    child.row.dataset.toolOutcome = view.state;
    child.meta.textContent = trace.preview || (trace.ok ? t("完成") : t("失败"));
    appendDisplayBlock(child.row, trace.display);
  }
  // 调用数在 children 落定后再刷,先刷会永远少算一次。
  if (!entry.done) bgTick(entry);
}

function activityOutcomeView(ok, outcome) {
  const state = outcome || (ok ? "success" : "failed");
  if (state === "noop") return { state, cls: "noop" };
  if (state === "needs_correction" || state === "needs_confirmation") return { state, cls: "warn" };
  return state === "success" ? { state, cls: "ok" } : { state, cls: "err" };
}

function bgEnd(id, ok, preview, display, outcome) {
  const entry = bgEntries.get(id);
  if (!entry) return;
  const view = activityOutcomeView(ok, outcome);
  // 实时流是执行期的临时视图,终态由 display 的完整输出接管,避免同一份输出双份并存。
  if (entry.live) {
    entry.live.remove();
    entry.live = null;
  }
  entry.done = true;
  // 落位判据存条目上:成功/noop 收进折叠区,其余留在「需要关注」。
  entry.cls = view.cls;
  entry.outcomeState = view.state;
  entry.el.classList.remove("running");
  entry.el.classList.add(view.cls);
  entry.el.dataset.toolOutcome = view.state;
  // 超时角色后端固定发 ok=false + preview「(超时,未产出结果)」。它与"跑了但失败"
  // 是两回事:超时说明该角色被屏障砍掉、什么都没产出,视觉上必须能与失败分开。
  const timedOut = !ok && /超时/.test(String(preview ?? ""));
  if (timedOut) entry.el.classList.add("timeout");
  entry.el.dataset.bgStatus = timedOut ? "timeout" : view.cls;
  // 超时归「需要关注」:cls 已是 err,bgSectionFor 自然把它留在可见处。
  bgPlace(entry);
  bgSetCurrentTool(entry, null, false);
  entry.prog.textContent = preview || (ok ? t("完成") : t("失败"));
  // 元信息一行说清:成败、耗时、子代理内部调用数。此前只有一个秒数,
  // 看不出成没成,也看不出子代理到底干了多少活(R-095 验收 ⑤)。
  const ms = Date.now() - entry.startedAt;
  const elapsed = ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
  const statusText = view.state === "noop"
    ? `↪ ${t("无需修改")}`
    : view.state === "needs_confirmation"
      ? `⚠ ${t("需要确认")}`
      : view.state === "needs_correction"
        ? `⚠ ${t("需要修正")}`
        : ok
          ? `✓ ${t("成功")}`
          : timedOut
            ? `⏱ ${t("超时")}`
            : `✕ ${t("失败")}`;
  const bits = [statusText, elapsed];
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
  if (view.state === "failed" && preview) {
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
        if (!event.name) continue;
        // 回放与实时路径一致:完整工具调用都进入活动面板。
        if (bgQuiet(event.name)) {
          bgStartQuiet(event.id, event.name, event.summary || "", null);
        } else if (!bgEntries.has(event.id)) {
          bgAdd(event.id, event.name || "task", event.summary || t("历史子代理轨迹"), null, activeSessionId);
        }
      } else if (event.kind === "tool.completed") {
        if (bgFinishQuiet(event.id, event.ok !== false)) continue;
        const entry = bgEntries.get(event.id);
        if (!entry) continue;
        const view = activityOutcomeView(event.ok !== false, event.outcome);
        entry.done = true;
        entry.el.classList.remove("running");
        const failed = event.ok === false;
        entry.el.classList.add(view.cls);
        entry.el.dataset.toolOutcome = view.state;
        entry.el.dataset.bgStatus = view.cls;
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
  // 回放里没等到 completed 的兼容待定调用直接丢弃，不让残留 id 污染后续实时判定。
  bgPending.clear();
  bgSync();
}

function bgClear() {
  for (const entry of bgEntries.values()) entry.el.remove();
  bgEntries.clear();
  bgPending.clear();
  bgGroups.clear();
  diffSummary.clear();
  // 只清三段的**内容**,不能 innerHTML="" 整个 #bg-list——那会把三段骨架
  // (段头/空态行/折叠按钮)一起冲掉,之后所有条目都无处可落,活动面板永久变空白。
  for (const id of Object.values(BG_SECTION_BODY)) {
    const host = $(id);
    if (host) host.replaceChildren();
  }
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
    el?.classList.add("hidden");
    return;
  }
  liveTextSources.set(id, source);
  if (!el) return;
  el.classList.remove("hidden");
  el.textContent = localizeDynamic(source);
  el.title = localizeDynamic(source);
}
function liveIdle(label) {
  const turn = $("live-turn");
  const source = String(label ?? "");
  liveTextSources.set("live-turn", source);
  if (turn) {
    turn.textContent = localizeDynamic(source);
    turn.classList.remove("hidden");
    turn.classList.add("dim");
  }
  liveSet("live-action", "");
}
function liveTurn(text) {
  const turn = $("live-turn");
  const source = String(text ?? "");
  liveTextSources.set("live-turn", source);
  if (turn) {
    turn.textContent = localizeDynamic(source);
    turn.classList.remove("hidden");
    turn.classList.remove("dim");
  }
}

// ---------- 子代理面板(R-174):与活动面板共用本文件的事件/渲染基础 ----------
// 子代理面板仍是独立 DOM 视图,但其状态、transcript、usage 与活动轨迹由同一脚本维护,
// 避免 06-agent-panel.js 与 06-activity.js 之间复制调用器和渲染辅助函数。
const agentEntries = new Map(); // id -> {el, head, meta, detail, calls, tokens, startedAt, state}
let agentPanelOpen = false;

// D-278:子代理就绪文案——设置页 fast 行与侧边栏子代理面板共用同一计算,避免两处漂移。
// s 来自 fast_model_status 的载荷:managed/ready/model/installed/serviceUp。
function fastStatusText(s) {
  if (!s.managed) return { text: t("fast 指向外部 provider,不由本机托管"), warn: false };
  if (s.ready) return { text: `✓ ${t("子代理就绪")}(${s.model})`, warn: false };
  const missing = !s.installed
    ? t("Ollama 未安装")
    : !s.serviceUp
      ? t("Ollama 服务未运行")
      : `${t("模型未拉取")}(${s.model})`;
  return { text: `⚠ ${missing} — ${t("子代理杂活(记忆整理/快速记录)暂不可用")}`, warn: true };
}

// D-278:面板头部就绪状态行。打开面板时查询 fast_model_status,与设置页同源文案;
// 查询失败(命令不可用/旧引擎)时保持隐藏,不遮挡面板其余内容。
async function refreshAgentPanelStatus() {
  const line = $("agent-panel-status");
  if (!line) return;
  let s;
  try {
    s = await invoke("fast_model_status");
  } catch {
    line.classList.add("hidden");
    return;
  }
  const st = fastStatusText(s);
  line.textContent = st.text;
  line.classList.remove("hidden");
  line.classList.toggle("warn-text", st.warn);
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
  let closed = 0;
  for (const entry of agentEntries.values()) {
    if (entry.state === "running") running += 1;
    else if (entry.state === "finished") finished += 1;
    else if (entry.state === "closed") closed += 1;
  }
  $("agent-running-count").textContent = running ? `${t("运行中")} ${running}` : "";
  $("agent-finished-count").textContent = finished ? `${t("已完成")} ${finished}` : "";
  $("agent-running-count2").textContent = running ? String(running) : "";
  $("agent-finished-count2").textContent = finished ? String(finished) : "";
  $("agent-closed-count2").textContent = closed ? String(closed) : "";
  $("agent-clear").classList.toggle("hidden", finished === 0 && closed === 0);
}

// 打开/收起面板。与活动面板互斥:一个开着时另一个收起,避免右侧两栏叠在一起。
function agentTogglePanel() {
  agentPanelOpen = !agentPanelOpen;
  $("agent-panel").classList.toggle("hidden", !agentPanelOpen);
  $("bg-panel").classList.toggle("hidden", agentPanelOpen);
  $("agent-toggle").classList.toggle("active", agentPanelOpen);
  $("agent-toggle").setAttribute("aria-expanded", String(agentPanelOpen));
  $("agent-toggle").title = agentPanelOpen ? t("收起子代理面板") : t("打开子代理面板");
  if (agentPanelOpen) refreshAgentPanelStatus(); // D-278:每次打开都刷新就绪状态
}

// D-350:面板头部 ✕ 关闭。与 agentTogglePanel 的互斥切换不同,这里只关子代理面板,
// 并把活动面板恢复到用户既有的 activityPanelOpen 状态,不强行弹开。
function agentClosePanel() {
  agentPanelOpen = false;
  $("agent-panel").classList.add("hidden");
  syncActivityPanel(); // bg-panel 回到 activityPanelOpen 决定的状态
  $("agent-toggle").classList.remove("active");
  $("agent-toggle").setAttribute("aria-expanded", "false");
  $("agent-toggle").title = t("打开子代理面板");
}

function agentStart(id, name, summary, input, sessionId = activeSessionId) {
  if (!id) return;
  const phase = name === "task" ? orchPhaseOf(input) : null;
  const existing = agentEntries.get(id);
  if (existing && existing.state === "running") return; // 同 id 仍在跑,原地更新
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
  el.dataset.agentState = "running";
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
    calls: new Map(), messages: [], tokens: 0, currentTool: "", startedAt: Date.now(), state: "running", sessionId,
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
  if (!entry || entry.state !== "running") return;
  if (text) entry.prog.textContent = text;
  if (!trace) {
    if (entry.state === "running") agentTick(entry);
    return;
  }
  if (trace.phase === "usage") {
    // usage 的 trace 里 name 是空串(后端构造),不覆盖当前工具名。
    agentAddUsage(entry, trace.usage);
    agentTick(entry);
    return;
  }
  if (trace.phase === "text") {
    const text = String(trace.text ?? "");
    if (text.trim()) {
      entry.messages.push(text);
      renderAgentTranscript(entry);
    }
    if (entry.state === "running") agentTick(entry);
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
  if (entry.state === "running") agentTick(entry);
}

// transcript:子代理自己的文字消息 + 每次工具调用,按各自数据源可回看。
function renderAgentTranscript(entry) {
  const detail = entry.detail;
  // 重建:调用序列或正文有新增时整体重画(频率低,一次 task-progress 一批)。
  detail.querySelectorAll(".agent-message, .agent-call").forEach((node) => node.remove());
  for (const text of entry.messages) {
    const message = document.createElement("div");
    message.className = "agent-message md";
    message.innerHTML = renderMarkdown(text);
    detail.appendChild(message);
  }
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
  // 工具和正文都默认折叠；用户点标题或「打开」才展开。
  if (detail.children.length) entry.el.classList.add("has-detail");
  if (detail.children.length) entry.title.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
}

function agentEnd(id, ok, preview, display) {
  const entry = agentEntries.get(id);
  if (!entry) return;
  entry.state = "finished";
  entry.el.dataset.agentState = "finished";
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

function agentClose(id) {
  const entry = agentEntries.get(id);
  if (!entry || entry.state !== "finished") return;
  entry.state = "closed";
  entry.el.dataset.agentState = "closed";
  $("agent-closed").appendChild(entry.el);
  agentRenderActions(id, entry);
  agentCountsSync();
}

function agentDelete(id) {
  const entry = agentEntries.get(id);
  if (!entry || entry.state !== "closed") return;
  entry.el.remove();
  agentEntries.delete(id);
  agentCountsSync();
}

// 每条的操作项:运行中的能单条停止;结束后能查看/关闭;关闭后才能删除本地条目。
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
  if (entry.state === "running") {
    // R-174:子代理单条停止通道——不再只能停整轮。
    add(t("停止"), t("只停这一条子代理,不影响本轮其它工具"), async () => {
      try {
        const processId = processItems.find((item) => item.session_id === entry.sessionId)?.id
          || (entry.sessionId === activeSessionId ? activeProcessId : null);
        await invoke("stop_task", { projectDir: currentProject, processId, taskId: String(id) });
        toast(t("已请求停止该子代理"));
      } catch (error) {
        toastError(`${t("停止失败")}:${error}`);
      }
    });
  }
  if (entry.state === "finished") {
    add(t("打开"), t("查看完整 transcript(工具调用序列 + 每次入参与输出)"), () => {
      const detail = entry.detail;
      detail.classList.toggle("hidden");
      entry.title.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    });
    add(t("关闭"), t("关闭该条目但保留后端历史与审计记录"), () => agentClose(id));
  }
  if (entry.state === "closed") {
    add(t("打开"), t("重新打开该条目"), () => {
      entry.state = "finished";
      entry.el.dataset.agentState = "finished";
      $("agent-finished").appendChild(entry.el);
      agentRenderActions(id, entry);
      agentCountsSync();
    });
    add(t("删除"), t("从当前面板删除该条目,不删除后端历史"), () => agentDelete(id));
  }
}

// Clear:清空 Finished/Closed 区,保留运行中的;真正删除前必须先关闭。
function agentClearFinished() {
  for (const [id, entry] of agentEntries) {
    if (entry.state === "finished" || entry.state === "closed") {
      entry.el.remove();
      agentEntries.delete(id);
    }
  }
  agentCountsSync();
}

function agentPanelSetup() {
  const toggle = $("agent-toggle");
  toggle.addEventListener("click", agentTogglePanel);
  $("agent-close").addEventListener("click", agentClosePanel);
  $("agent-clear").addEventListener("click", agentClearFinished);
  // D-278:一键就绪进度事件也同步刷新面板状态行(面板开着时在设置页操作,回到面板即最新)。
  on("kz:fast-setup", (event) => {
    if (agentPanelOpen) refreshAgentPanelStatus();
    const text = event.payload?.text;
    if (text) {
      $("fast-status").textContent = text;
      log(`${t("子代理安装")}:${text}`);
    }
  });
}
// 06-activity.js 是唯一的活动/子代理脚本,加载时 DOM 已解析完毕。
agentPanelSetup();

