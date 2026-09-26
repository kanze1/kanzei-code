import { defer } from "./01-core.js";
import { escapeHtml } from "./04-markdown.js";
import { $, invoke, messages, renderingBackground } from "./01-core.js";
import { localizeDynamic, t } from "./02-i18n.js";
import {
  activeProcessId,
  activeSessionId,
  currentProject,
  processItems,
  running,
  sessionState,
  setStatus,
  statusRunning,
  statusTextSource,
  toast,
  toastError,
} from "./03-shell.js";
import { SIDE_TERMINAL_AUTO_MS } from "./06-side-policy.js";
import { toolCallSummary } from "./05-chat-render.js";
import { classifySubagentEnd, subagentDescription, subagentRelocalize, subagentReplayDuration, subagentReplayTrace } from "./05-subagents.js";
import { cleanInline, cleanPaths, formatDuration, looksLikeNoise, parseJsonish, stripAnsi } from "./04-structured-parse.js";
import { toolArgSummary, toolResultSummary, toolRoots } from "./05-tool-summary.js";
import { highlightLine, renderLocalValidation, renderToolArgs, renderToolResult, structuredNav } from "./04-structured.js";
import { renderContextDetail } from "./07-events.js";
import { autoStopReason, renderAutoStatus } from "./08-auto.js";
import { state } from "./08-compose.js";

// ---------- 后台任务侧栏里的终端条目(R-037 → UI2-0926 #14):完整工具活动入列,详情点击展开 ----------
// 侧栏的显隐、停靠与徽标归 06-agent-panel.js(reconcileTasksPanel);这里只维护条目,变化经 onBgChange 通知。
// 只收活动线路的条目(缺陷 B:后台线路的 bash 曾经混进当前线路的列表)。
export const bgEntries = new Map(); // call_id -> {el, title, prog, meta, detail, startedAt, done, cls, acked, replay}
const bgListeners = new Set();
/// 订阅条目变化:{ type: "entries" | "sections" } 重算段头与徽标;{ type: "work-start", entry } 终端命令跑满 3 秒;
/// { type: "failure", entry } 跑满 3 秒后失败的终端命令(值得停留的失败)。
export function onBgChange(fn) {
  bgListeners.add(fn);
  return () => bgListeners.delete(fn);
}
function notifyBg(event) {
  for (const fn of bgListeners) {
    try { fn(event); } catch (error) { console.warn(error); }
  }
}
export const diffSummary = new Map();
export const BG_MAX = 120;
export function renderDiffSummary() {
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
export function buildDiffTree(files) {
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

export function appendDiffNode(container, node, depth) {
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
    // UI-0926 #10:目录已经在上层行里,文件行只显示文件名(全路径在 title 与 dataset.path),
    // 点击在文件导览里打开。
    row.dataset.path = item.path;
    const name = document.createElement("button");
    name.type = "button";
    name.className = "sv-linklike diff-summary-name";
    name.textContent = item.path.split("/").filter(Boolean).pop() || item.path;
    name.title = item.path;
    name.addEventListener("click", (event) => {
      event?.stopPropagation?.();
      structuredNav.openPath(item.path, null);
    });
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

export function bgSync() {
  // 显隐只归侧栏的 reconcileTasksPanel(自动开合策略 + 用户开关);工具事件只更新内容。
  applyBgFilters();
  renderBgSections();
}

// R-168(用户原话「不要在活动栏记录所有工具,edit啥的,只记录报错的和非工具的 bash」):
// 活动栏只收**终端类调用**与**失败调用**。成功的 read/grep/edit/tracker 类走 bgStartQuiet 静默,
// 失败时由 bgFinishQuiet 补建条目——「只记录报错的」靠的是收尾补建,不是入列时就全收。
// R-173 曾开过一个例外(编排派发的勘察/复核子代理),UI2-0926 #14 起所有 task(模型自派与编排派发)
// 都是主对话里的子代理卡片与后台任务侧栏里的委派卡,这里不再重复建条目——task 永远不进终端条目,
// 失败也不补建(委派卡自己标失败)。
// —— c611f909(2026-08-12)曾把判据改成恒真/恒假,未带条目编号也未动 tracker,
// R-168 却一直显示 [done];D-729 按用户重申的原意恢复。要改口径请改 BG_TOOL_TYPES 或
// 在此加显式集合,别再回到恒真——恒真等于没有判据。
export const ORCH_PHASES = new Set(["scouting", "review"]);
export function orchPhaseOf(input) {
  const phase = input?.phase;
  return typeof phase === "string" && ORCH_PHASES.has(phase) ? phase : null;
}
export function orchPhaseLabel(phase) {
  // 写成两个字面量调用而不是查表:i18n 冒烟只扫源码里带字符串常量的翻译调用,
  // 查表写法(t(MAP[phase]))会整条绕过 key 覆盖率检查,英文界面上就地漏译。
  return phase === "scouting" ? t("勘察") : t("复核");
}
export function isActivityTool(name) {
  return bgIsTerminal(name);
}

export const BG_TOOL_TYPES = {
  bash: "terminal", process: "terminal",
  read: "file", write: "file", edit: "file", multiedit: "file", glob: "file", grep: "file",
  req: "tracker", defect: "tracker", idea: "tracker", source: "tracker", finding: "tracker", decision: "tracker",
  task: "agent",
  memory_note: "memory", memory_search: "memory", memory_stats: "memory",
};
// 静默调用先挂这里等收尾:成功就无声丢弃,失败由 bgFinishQuiet 补建真实条目。
// R-168 的「只记录报错的」那一半就是靠它兑现的,不是靠入列判据。
export const bgPending = new Map(); // call_id -> {name, summary, input, startedAt}
export function bgQuiet(name) {
  return !isActivityTool(name);
}
export function bgStartQuiet(id, name, summary, input) {
  // 后台线路的调用不进活动线路的列表(缺陷 B);task 的成败归委派卡,不补建终端条目。
  if (!id || renderingBackground || name === "task") return;
  bgPending.set(id, { name, summary, input, startedAt: Date.now() });
  // 悬挂上限:异常中断的静默调用不该无限累积。
  if (bgPending.size > BG_MAX) bgPending.delete(bgPending.keys().next().value);
}
// 收尾历史兼容路径中的待定调用。成功返回 true；失败补建真实条目，
// 让调用方继续走 bgEnd 把错误详情画出来。
export function bgFinishQuiet(id, ok) {
  // 后台线路的收尾不认领活动线路的待定调用(调用 id 可能在两条线上重名)。
  if (renderingBackground) return false;
  const pending = bgPending.get(id);
  if (!pending) return false;
  bgPending.delete(id);
  if (ok) return true;
  bgAdd(id, pending.name, pending.summary, pending.input);
  const entry = bgEntries.get(id);
  if (entry) entry.startedAt = pending.startedAt;
  return false;
}
export function bgToolType(name) {
  return BG_TOOL_TYPES[name] ?? "other";
}
// 终端类输出才提供复制/导出:diff 与追踪结果在主对话里已有更好的呈现。
export function bgIsTerminal(name) {
  return bgToolType(name) === "terminal";
}

export function setBgDoneOpen(value) { bgDoneOpen = Boolean(value); }
export const bgFilters = {
  type: localStorage.getItem("kz-bg-type") || "all",
  status: localStorage.getItem("kz-bg-status") || "all",
};
export function bgEntryStatus(entry) {
  if (!entry.done) return "running";
  return entry.el.classList.contains("err") ? "err" : "ok";
}
export function applyBgFilters() {
  let shown = 0;
  for (const entry of bgEntries.values()) {
    const typeOk = bgFilters.type === "all" || entry.type === bgFilters.type;
    const statusOk = bgFilters.status === "all" || bgEntryStatus(entry) === bgFilters.status;
    const visible = typeOk && statusOk;
    entry.el.classList.toggle("hidden", !visible);
    if (visible) shown += 1;
  }
  // 有筛选时同时给出"筛出/总数",否则看到 3 条会以为本轮只跑了 3 个工具(写在筛选按钮的提示里)。
  const filter = $("tasks-filter");
  if (filter) filter.dataset.filtered = String(shown !== bgEntries.size);
  return { shown, total: bgEntries.size };
}

export const BG_SECTION_BODY = { running: "bg-running", attention: "bg-attention", done: "bg-done" };
export let bgDoneOpen = localStorage.getItem("kz-bg-done-open") === "1";
// 条目该落哪一段。成功判据只认 ToolEnd 的机器可读 outcome(经 activityOutcomeView
// 折算成 cls),不看文案、不反推 DOM:成功与 noop 收进已完成;其余(失败/需确认/需修正/超时)
// 在用户确认之前留在「需要关注」——关掉侧栏、点「知道了」或发下一条消息都算确认(entry.acked),
// 确认后挪进已完成(仍带 ✗ 标记)。历史回放的条目一律已确认。
export function bgSectionFor(entry) {
  if (!entry.done) return "running";
  if (entry.cls === "ok" || entry.cls === "noop") return "done";
  return entry.acked ? "done" : "attention";
}
export function bgPlace(entry) {
  const section = bgSectionFor(entry);
  if (entry.section === section && entry.el.parentNode) return;
  entry.section = section;
  entry.el.dataset.bgSection = section;
  ($(BG_SECTION_BODY[section]) ?? $("bg-list"))?.appendChild(entry.el);
}
/// 确认失败:该线路「需要关注」里的条目挪进已完成(关侧栏、「知道了」、下一条用户消息)。
export function bgAck(sessionId = activeSessionId) {
  let moved = 0;
  for (const entry of bgEntries.values()) {
    if (entry.acked || (sessionId && entry.sessionId && entry.sessionId !== sessionId)) continue;
    if (!entry.done) continue;
    entry.acked = true;
    if (bgSectionFor(entry) !== entry.section) {
      bgPlace(entry);
      moved += 1;
    }
  }
  if (moved) renderBgSections();
  return moved;
}
/// 活动线路还在跑的终端条目数(侧栏徽标与自动收起的判据之一)。
export function bgRunningCount(sessionId = activeSessionId) {
  let count = 0;
  for (const entry of bgEntries.values()) {
    if (!entry.done && (!sessionId || !entry.sessionId || entry.sessionId === sessionId)) count += 1;
  }
  return count;
}
/// 三段里可见(未被筛掉)的终端条目数;侧栏段头把它与委派卡合并计数。
export function bgSectionCounts() {
  const counts = { running: 0, attention: 0, done: 0, total: bgEntries.size };
  for (const entry of bgEntries.values()) {
    if (entry.el.classList.contains("hidden") || !(entry.section in counts)) continue;
    counts[entry.section] += 1;
  }
  return counts;
}
// 段头(计数、空态、折叠)由侧栏合并委派卡后统一渲染(06-agent-panel.js 的 onBgChange 监听)。
export function renderBgSections() {
  notifyBg({ type: "sections" });
}

/// 差异汇总必须独立于活动面板的过滤:diff 来自 write/edit,而这两个工具已不进活动面板,
/// 原先把累计写在 bgEnd 里就等于永远拿不到数据,#diff-summary 变成接不到数据源的空壳(D-137)。
export function recordDiffSummary(display) {
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
export function bgAppendArgs(entry, input) {
  // UI-0926 #10:键值表(路径 chip、命令代码块、多行说明折叠),不再 dump 转义后的 JSON。
  const args = renderToolArgs(entry.name, input, { className: "tool-display bg-args" });
  if (!args) return;
  entry.detail.appendChild(args);
  entry.el.classList.add("has-detail");
}

// 运行中的元信息一行:已运行秒数。1 秒心跳与建条时共用同一段,建条即可读。
// 实时的终端命令跑满 3 秒(SIDE_TERMINAL_AUTO_MS)上报一次 work-start:后台任务侧栏据此自动打开
// (短命令一闪而过不打扰);历史回放的条目不上报。
export function bgTick(entry) {
  const ms = Date.now() - entry.startedAt;
  const seconds = Math.round(ms / 1000);
  entry.el.dataset.bgElapsed = String(seconds);
  entry.meta.textContent = `${seconds}s`;
  if (!entry.done && !entry.replay && !entry.longReported && entry.type === "terminal" && ms >= SIDE_TERMINAL_AUTO_MS) {
    entry.longReported = true;
    notifyBg({ type: "work-start", entry });
  }
}

export function bgAdd(id, name, summary, input, sessionId = activeSessionId) {
  if (!id || bgEntries.has(id)) return;
  // 后台线路的调用不进活动线路的列表(缺陷 B):路由层把后台会话的 kz:tool-start 交给同一个处理函数时
  // renderingBackground 为真;显式传入的 sessionId 与活动线路不同也不收。
  if (renderingBackground || (sessionId && activeSessionId && sessionId !== activeSessionId)) return;
  const type = bgToolType(name);
  const el = document.createElement("div");
  el.className = `bg-entry running bg-type-${type}`;
  el.dataset.bgId = id;
  el.dataset.bgTool = name;
  el.dataset.bgStatus = "running";
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
  // 后端 summarize_input(kanzei-core/src/runner/compaction.rs)把整坨入参 JSON 截到 160 字,
  // 对所有工具一视同仁——edit 于是显示成 `{"new_string":"…","old_strin…`,完全看不出改的是哪个文件。
  // 标题优先走前端按工具名挑字段的 toolCallSummary(05-chat-render.js,主对话工具块同款),
  // 挑不出来再回落后端 summary(回放事件不带 input,靠的就是这一级),最后回落空串。
  const shown = toolCallSummary(name, input) || String(summary ?? "");
  target.textContent = shown;
  title.append(toolName, target);
  title.title = shown;
  const prog = document.createElement("div");
  prog.className = "bg-prog";
  prog.textContent = "…";
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
    // summary 存显示值:它的两个消费方(重跑填词、导出文件头)都是把它当"人类可读的一行标识"用,
    // 且都在同一段文本里另附了完整入参 JSON,存裸 JSON 只会变成两份 JSON 叠在一起。
    el, title, target, prog, current: null, meta, detail, actions, type, name, summary: shown, input, sessionId,
    children: new Map(), startedAt: Date.now(), done: false,
    // 落位判据存在条目上,不从 DOM class 反推(反推在 warn/noop 上一直是错的)。
    // acked:用户确认过的失败挪进已完成;replay:历史回放的条目(不触发自动打开)。
    cls: null, outcomeState: null, section: null, acked: false, replay: false, longReported: false,
  };
  bgEntries.set(id, entry);
  bgPlace(entry);
  bgAppendArgs(entry, input);
  bgTick(entry);
  // 上限裁剪按登记表走,优先摘已完成的:在跑的条目正是用户开着侧栏要看的东西,绝不能被裁掉。
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
export function bgRenderActions(id, entry) {
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

export function bgPlainText(entry) {
  return [
    `# ${entry.name} ${entry.summary}`,
    entry.input ? `\n## 入参\n${JSON.stringify(entry.input, null, 2)}` : "",
    `\n## 输出\n${entry.detail.textContent || entry.prog.textContent || ""}`,
  ].join("\n");
}
// 语法着色搬进 04-structured.js(结构化渲染的唯一真源),这里原名转出,旧调用方不必改。
export { highlightLine };

export const DIFF_CONTEXT_LINES = 3;

// 主对话只看变更附近的上下文；活动面板调用 renderDiff 的默认全量模式。
// 这样 write/edit 的 display 事实不变，主区不会因一行修改展开整份源码。
export function compactDiffLines(lines, context = DIFF_CONTEXT_LINES) {
  if (!Array.isArray(lines) || lines.length === 0) return lines || [];
  const changed = lines
    .map((line, index) => (line?.kind === "add" || line?.kind === "del" ? index : -1))
    .filter((index) => index >= 0);
  if (changed.length === 0) return lines;
  const keep = new Set();
  for (const index of changed) {
    for (let cursor = Math.max(0, index - context); cursor <= Math.min(lines.length - 1, index + context); cursor += 1) {
      keep.add(cursor);
    }
  }
  const compacted = [];
  let cursor = 0;
  while (cursor < lines.length) {
    if (keep.has(cursor)) {
      compacted.push(lines[cursor]);
      cursor += 1;
      continue;
    }
    const start = cursor;
    while (cursor < lines.length && !keep.has(cursor)) cursor += 1;
    compacted.push({ kind: "omitted", count: cursor - start });
  }
  return compacted;
}

export function renderDiff(display, { compact = false } = {}) {
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
  const sourceLines = display.lines?.length ? display.lines : (display.diff || "").split("\n").filter(Boolean).map((text) => ({ kind: text[0] === "+" ? "add" : text[0] === "-" ? "del" : "ctx", text: text.slice(1) }));
  const lines = compact ? compactDiffLines(sourceLines) : sourceLines;
  function omittedLabel(count) {
    return `… ${count} ${t("行上下文已省略")}`;
  }
  function render() {
    body.innerHTML = "";
    body.className = `diff-body ${mode}`;
    if (mode === "unified") {
      let oldLine = 1;
      let newLine = 1;
      for (const line of lines) {
        const row = document.createElement("div");
        row.className = `diff-row ${line.kind || "ctx"}`;
        if (line.kind === "omitted") {
          row.classList.add("diff-omitted");
          row.textContent = omittedLabel(line.count);
          body.appendChild(row);
          continue;
        }
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
        if (line.kind === "omitted") rows.push([line, null]);
        else if (line.kind === "del" && lines[i + 1]?.kind === "add") rows.push([line, lines[++i]]);
        else if (line.kind === "del") rows.push([line, null]);
        else if (line.kind === "add") rows.push([null, line]);
        else rows.push([line, line]);
      }
      for (const [left, right] of rows) {
        const row = document.createElement("div");
        row.className = "diff-split-row";
        if (left?.kind === "omitted") {
          row.classList.add("diff-omitted");
          row.textContent = omittedLabel(left.count);
          body.appendChild(row);
          continue;
        }
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
export function appendDisplayBlock(parent, display, { compact = false, name = "" } = {}) {
  if (!display) return;
  if (display.kind === "json" && display.value && typeof display.value === "object") {
    parent.appendChild(renderToolResult(name, display.value));
  } else if (display.kind === "local_validation") {
    const checks = renderLocalValidation(display);
    if (checks) parent.appendChild(checks);
  } else if (display.kind === "diff") {
    parent.appendChild(renderDiff(display, { compact }));
  } else if (display.kind === "terminal") {
    const block = document.createElement("div");
    block.className = "tool-display term";
    // D-237:活动面板展开区优先展示完整输出(full),而不是 4000 截断的 output。
    // 彩色输出(cargo 等)的 ANSI 转义原样进 DOM 会显示成 `\u001b[32m` 乱码。
    const out = stripAnsi(display.full ?? display.output ?? "");
    block.textContent = `$ ${display.command}\n${out}`;
    parent.appendChild(block);
  } else if (display.kind === "create") {
    const block = document.createElement("div");
    block.className = "tool-display term";
    block.textContent = `${t("新建")} ${display.path}(${display.bytes} bytes)\n${stripAnsi(display.preview)}`;
    parent.appendChild(block);
  } else if (display.kind === "file") {
    parent.appendChild(renderFileCard(display));
  } else if (display.kind === "truncated" && display.preview) {
    // 原工具没有 display 时后端才发 kind=truncated:没有终端块可保,预览单独成块。
    const block = document.createElement("div");
    block.className = "tool-display term";
    block.textContent = stripAnsi(String(display.preview));
    parent.appendChild(block);
  }
  // UI-0926 #10:edit/write 的 display 挂着局部校验结果——逐项 chip,失败项带首个错误与修复上下文。
  if (display.local_validation && display.kind !== "local_validation") {
    const checks = renderLocalValidation(display.local_validation);
    if (checks) parent.appendChild(checks);
  }
  // 配额截断提示追加在原 display 之后:终端块照常保留,提示只补"为什么被截、去哪腾空间"。
  const quota = quotaTruncation(display);
  if (quota) parent.appendChild(renderQuotaNotice(quota));
}

// 工具结果无法外置时的 Inline 截断(R-245 路径)。后端保留工具原 display 并附 quota_truncated;
// 原来没有 display 时发 kind=truncated。对话工具块与活动面板都经 appendDisplayBlock 消费。
export function quotaTruncation(display) {
  if (!display || typeof display !== "object") return null;
  const nested = display.quota_truncated;
  if (nested && typeof nested === "object") return nested;
  return display.kind === "truncated" ? display : null;
}

// 截断原因决定说什么:只有真超配额(artifact_quota_exceeded)才说"已满"、给删除整理建议。
// 锁繁忙(quota_lock_unavailable)与无法计量(quota_unmeasurable)是暂态或环境问题,
// 删历史对话修不好,只给重试方向。⎿ 行、活动面板进度行、提示块标题共用这一句。
export function quotaNoticeHeadline(info) {
  switch (String(info?.reason ?? "")) {
    case "artifact_quota_exceeded":
      return t("工具结果存储已满,本次输出已截断");
    case "quota_lock_unavailable":
      return t("工具结果暂时无法外置(存储锁繁忙),本次输出已截断");
    case "quota_unmeasurable":
      return t("无法计量工具结果存储,本次输出已截断");
    default:
      return t("工具结果无法外置,本次输出已截断");
  }
}

function quotaReasonLabel(code) {
  if (code === "artifact_quota_exceeded") return t("工具结果存储达到配额");
  if (code === "quota_lock_unavailable") return t("存储锁繁忙");
  if (code === "quota_unmeasurable") return t("存储占用无法计量");
  return "";
}

function quotaNoticeHint(code) {
  if (code === "artifact_quota_exceeded") {
    return t("释放空间:在侧栏当前线路的「历史对话」里勾选不再需要的对话,点「删除」并选「删除并安全整理」(只清理无引用 artifact),然后重试");
  }
  if (code === "quota_lock_unavailable") {
    return t("存储锁通常只是被并行任务短暂占用,稍后重试即可;不需要清理历史对话");
  }
  if (code === "quota_unmeasurable") {
    return t("读取工具结果存储目录(.kanzei/artifacts/tool-results)的占用失败,确认该目录可访问、且其中没有符号链接后重试;不需要清理历史对话");
  }
  return "";
}

// 计量失败时后端给 null:显示「未知」,不能让 formatBytes 把它画成 "0 B"。
function quotaBytesText(bytes) {
  return typeof bytes === "number" && Number.isFinite(bytes) ? formatBytes(bytes) : t("未知");
}

export function renderQuotaNotice(info) {
  const code = String(info.reason ?? "");
  const box = document.createElement("div");
  box.className = "tool-display quota-notice";
  box.setAttribute("role", "note");
  box.dataset.reason = code;
  const head = document.createElement("div");
  head.className = "quota-notice-head";
  head.textContent = `⚠ ${quotaNoticeHeadline(info)}`;
  const usage = document.createElement("div");
  usage.className = "quota-notice-usage";
  const label = quotaReasonLabel(code);
  const reason = label ? `${label} (${code})` : code;
  const bits = [`${t("已用")} ${quotaBytesText(info.storage_used_bytes)} / ${t("配额")} ${quotaBytesText(info.quota_bytes)}`];
  if (reason) bits.push(`${t("原因")}: ${reason}`);
  usage.textContent = bits.join(" · ");
  box.append(head, usage);
  const hintText = quotaNoticeHint(code);
  if (hintText) {
    const hint = document.createElement("div");
    hint.className = "quota-notice-hint";
    hint.textContent = hintText;
    box.appendChild(hint);
  }
  return box;
}

// R-329:deliver 的交付卡片。与 create 块的区别是「给谁看」——create 是模型刚写了
// 什么的事实回执,这张卡是给**用户**的:文件名、大小、一句话说明,外加两个真能点的
// 动作(打开 / 在资源管理器中定位)。路径已由后端校验在工作树内。
export function renderFileCard(display) {
  const card = document.createElement("div");
  card.className = "tool-display file-card";
  const head = document.createElement("div");
  head.className = "file-card-head";
  const name = document.createElement("span");
  name.className = "file-card-name";
  name.textContent = display.name || display.path || "";
  const size = document.createElement("span");
  size.className = "file-card-size";
  size.textContent = formatBytes(display.bytes);
  head.append(name, size);
  card.appendChild(head);
  if (display.caption) {
    const caption = document.createElement("div");
    caption.className = "file-card-caption";
    caption.textContent = display.caption;
    card.appendChild(caption);
  }
  const actions = document.createElement("div");
  actions.className = "file-card-actions";
  for (const [label, mode] of [[t("打开"), "open"], [t("在资源管理器中显示"), "reveal"]]) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ghost mini";
    button.textContent = label;
    button.addEventListener("click", () => {
      // 失败要说话:静默失效会让用户以为文件不见了,而多半只是没有默认打开方式。
      void invoke("open_delivered_path", { projectDir: currentProject, path: display.path, mode }).catch((error) =>
        toast(`${t("打开失败")}:${error}`),
      );
    });
    actions.appendChild(button);
  }
  card.appendChild(actions);
  return card;
}

export function formatBytes(bytes) {
  const n = Number(bytes) || 0;
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  // 工具结果存储配额是 2 GiB,按 MB 显示成 2048.0 MB 不直观。
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}
// 工具执行中的增量输出(kz:tool-progress,bash 等长任务):展开区里逐段追加,
// 收起状态下进度行显示最后一行——装依赖/发版这类长命令"跑到哪了"一眼可见。
// 只保留末尾 16k 字符:进度要的是尾部,完整输出等 ToolEnd 的终态块。
export const BG_STREAM_MAX = 16000;
export function bgStream(id, chunk) {
  const entry = bgEntries.get(id);
  if (!entry || entry.done || !chunk) return;
  if (!entry.live) {
    entry.live = document.createElement("pre");
    entry.live.className = "tool-display term bg-live";
    entry.detail.appendChild(entry.live);
    entry.el.classList.add("has-detail");
  }
  const text = (entry.live.textContent + stripAnsi(chunk)).slice(-BG_STREAM_MAX);
  entry.live.textContent = text;
  const lastLine = text.trimEnd().split("\n").pop() || "";
  // 进度行只是「跑到哪了」的提示:绝对路径相对化,展开区的实时流保留原文。
  if (lastLine) entry.prog.textContent = cleanInline(lastLine, toolRoots()).slice(0, 160);
  if (!entry.detail.classList.contains("hidden")) entry.live.scrollTop = entry.live.scrollHeight;
}

/// 子代理内部调用行的参数摘要。有结构化入参就走 toolArgSummary;回放/旧事件只有后端
/// summarize_input(整坨入参 JSON 截到 160 字)时,能解析就按工具挑字段,解析不了就只抽
/// 路径/命令类的首个字段——绝不把 `{"command":…` 这种 JSON 片段贴进行里。
export function traceArgText(name, input, summary) {
  if (input && typeof input === "object") return toolArgSummary(name, input).text;
  const raw = String(summary ?? "").trim();
  if (!raw) return "";
  const parsed = parseJsonish(raw);
  if (parsed && typeof parsed === "object") return toolArgSummary(name, parsed).text;
  const field = raw.match(/"(path|file_path|command|pattern|query|url|id|title|action)"\s*:\s*"((?:\\.|[^"\\])*)/);
  if (field) return toolArgSummary(name, { [field[1]]: field[2].replace(/\\(.)/g, "$1") }).text;
  const clean = cleanInline(raw, toolRoots());
  return looksLikeNoise(clean) ? "" : clean;
}

export function activityOutcomeView(ok, outcome) {
  const state = outcome || (ok ? "success" : "failed");
  if (state === "noop") return { state, cls: "noop" };
  if (state === "needs_correction" || state === "needs_confirmation") return { state, cls: "warn" };
  return state === "success" ? { state, cls: "ok" } : { state, cls: "err" };
}

/// extra = {content, contentTruncated, contentBytes, code, durationMs}(kz:tool-end 携带,见 chatToolEnd)。
export function bgEnd(id, ok, preview, display, outcome, extra = {}) {
  // 后台线路的收尾不碰活动线路的条目(调用 id 可能在两条线上重名,缺陷 B)。
  if (renderingBackground) return;
  const entry = bgEntries.get(id);
  if (!entry) return;
  const view = activityOutcomeView(ok, outcome);
  // 实时流是执行期的临时视图,终态由 display 的完整输出接管,避免同一份输出双份并存。
  if (entry.live) {
    entry.live.remove();
    entry.live = null;
  }
  entry.done = true;
  // 落位判据存条目上:成功/noop 收进已完成,其余在确认前留在「需要关注」。
  entry.cls = view.cls;
  entry.outcomeState = view.state;
  entry.el.classList.remove("running");
  entry.el.classList.add(view.cls);
  entry.el.dataset.toolOutcome = view.state;
  // 超时与「跑了但失败」是两回事(什么都没产出),视觉上必须能分开。
  const timedOut = !ok && /超时/.test(String(preview ?? ""));
  if (timedOut) entry.el.classList.add("timeout");
  entry.el.dataset.bgStatus = timedOut ? "timeout" : view.cls;
  bgPlace(entry);
  // 截断时 preview 首行是 [tool_result_truncated …] 机器标记,换成按原因区分的人话;
  // 详情区另有提示块。
  const quota = quotaTruncation(display);
  // 进度行与主对话 ⎿ 行同一个摘要器(耗时在元信息行里,这里不重复)。
  entry.prog.textContent = quota
    ? `⚠ ${quotaNoticeHeadline(quota)}`
    : toolResultSummary(entry.name, {
      content: extra.content,
      contentTruncated: extra.contentTruncated,
      contentBytes: extra.contentBytes,
      code: extra.code,
      preview,
      display,
      input: entry.input ?? undefined,
      ok,
      outcome,
    }).text;
  // 元信息一行说清:成败与耗时(R-095 验收 ⑤)。耗时优先用后端量的 durationMs(不含前端事件排队延迟)。
  const measured = Number(extra.durationMs);
  const ms = extra.durationMs !== undefined && extra.durationMs !== null && Number.isFinite(measured)
    ? measured
    : Date.now() - entry.startedAt;
  const elapsed = formatDuration(ms);
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
  entry.meta.textContent = [statusText, elapsed].join(" · ");
  // 结构化详情进侧栏内展开区(diff/终端/新建/todo)。
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
    err.textContent = cleanPaths(preview, toolRoots());
    entry.detail.appendChild(err);
  }
  if (entry.detail.children.length) entry.el.classList.add("has-detail");
  bgRenderActions(id, entry);
  // 跑满 3 秒后失败的实时终端命令是「值得停留的失败」:侧栏不自动收起、徽标转红,直到用户确认。
  // 普通工具偶发报错(读了个不存在的文件)只列进「需要关注」,不拦收起。
  if (!ok && view.cls === "err" && entry.type === "terminal" && !entry.replay && ms >= SIDE_TERMINAL_AUTO_MS) {
    notifyBg({ type: "failure", entry });
  }
  bgSync();
}
// 历史轨迹回放(D-208)。run.trace 事件本来就带 name/summary/ok/durationMs,
// 旧实现读的却是不存在的 event.text/event.trace,name 硬编码 "task"、标题硬编码
// "历史子代理轨迹"——上百条同名条目,类型筛选也跟着失真;而且标终态后没重渲染
// 动作区,停止按钮残留在早已结束的历史轨迹上。回放条目一律终态、无停止按钮。
// UI2-0926 #14:回放条目写入落位判据(cls/outcomeState)并重新落位——此前只改 class,
// 4 条回放全挂在「运行中」段、计数「4 · 终端 3」(缺陷 E);回放出来的失败一律算已确认,
// 进已完成(保留 ✗ 标记),也不触发侧栏自动打开。
export function renderRecoveredTraces(payloads) {
  const markReplay = (id) => {
    const entry = bgEntries.get(id);
    if (entry) entry.replay = true;
    return entry;
  };
  for (const payload of payloads || []) {
    for (const event of payload.events || []) {
      if (!event.id) continue; // turn.started / context.compacted 等无 id 事件不进列表
      // UI-0926 #8:落库的 task-progress(无 kind 字段)回放进子代理卡片——计数、工具列表、
      // 自述与实时同形;task 的耗时取 tool.completed.durationMs。
      if (!event.kind && event.trace !== undefined) {
        subagentReplayTrace(activeSessionId, event);
        continue;
      }
      if (event.kind === "tool.completed" && (event.name === "task" || !event.name)) {
        subagentReplayDuration(activeSessionId, event.id, event.durationMs, { isTask: event.name === "task" });
      }
      if (event.kind === "tool.started") {
        if (!event.name) continue;
        // 回放与实时路径一致:终端进条目,其余静默待定(失败才补建)。
        if (bgQuiet(event.name)) {
          bgStartQuiet(event.id, event.name, event.summary || "", null);
        } else if (!bgEntries.has(event.id)) {
          bgAdd(event.id, event.name, event.summary || t("历史轨迹"), null, activeSessionId);
          markReplay(event.id);
        }
      } else if (event.kind === "tool.completed") {
        if (bgFinishQuiet(event.id, event.ok !== false)) continue;
        const entry = markReplay(event.id);
        if (!entry) continue;
        const view = activityOutcomeView(event.ok !== false, event.outcome);
        entry.done = true;
        entry.cls = view.cls;
        entry.outcomeState = view.state;
        entry.acked = true;
        entry.el.classList.remove("running");
        const failed = event.ok === false;
        entry.el.classList.add(view.cls);
        entry.el.dataset.toolOutcome = view.state;
        entry.el.dataset.bgStatus = view.cls;
        // 轨迹只存了 preview(不存正文):成败都按同一个摘要器的降级口径(与主对话同源,
        // 「退出码 101 · …」「路径不存在 · …」而不是 `exit code: 101 (+42 lines)`);失败摘要
        // 为空时才退回清洗过的错误首段。
        const traceSummary = event.preview || event.code
          ? toolResultSummary(entry.name, { ok: !failed, outcome: event.outcome, code: event.code, preview: event.preview ?? "", input: entry.input ?? undefined }).text
          : "";
        entry.prog.textContent = traceSummary
          || (failed && event.error ? cleanInline(event.error, toolRoots()) : t("历史轨迹"));
        const seconds = Number(event.durationMs);
        entry.meta.textContent =
          Number.isFinite(seconds) && seconds >= 1000
            ? `${t("回放")} · ${Math.round(seconds / 1000)}s`
            : t("回放");
        bgPlace(entry);
        bgRenderActions(event.id, entry);
      }
    }
  }
  // 只 started 没 completed 的:线路**还在跑**时它们就是正在跑的调用,保留运行态(缺陷 F:切到在跑的
  // 线路时它们曾一律被收成「中断」);线路已停才是轮次中断,收敛终态,不留假 running 与停止按钮。
  const lineRunning = Boolean(activeSessionId) && sessionState(activeSessionId).running === true;
  for (const [id, entry] of bgEntries) {
    if (entry.done) continue;
    if (lineRunning) {
      // 真实的收尾事件还会来(bgEnd);只是不再因为「跑满 3 秒」触发自动打开——用户是自己切过来看的。
      entry.replay = false;
      entry.longReported = true;
      continue;
    }
    entry.done = true;
    entry.cls = "err";
    entry.outcomeState = "interrupted";
    entry.acked = true;
    entry.el.classList.remove("running");
    entry.el.classList.add("err");
    entry.el.dataset.bgStatus = "err";
    entry.prog.textContent = t("历史轨迹");
    entry.meta.textContent = `${t("回放")} · ${t("无结果(轮次中断)")}`;
    bgPlace(entry);
    bgRenderActions(id, entry);
  }
  // 回放里没等到 completed 的兼容待定调用直接丢弃，不让残留 id 污染后续实时判定。
  bgPending.clear();
  bgSync();
}

export function bgClear() {
  for (const entry of bgEntries.values()) entry.el.remove();
  bgEntries.clear();
  bgPending.clear();
  diffSummary.clear();
  // 只清三段的**内容**,不能 innerHTML="" 整个 #bg-list——那会把三段骨架
  // (段头/空态行/折叠按钮)与委派卡容器一起冲掉,之后所有条目都无处可落,侧栏永久变空白。
  for (const id of Object.values(BG_SECTION_BODY)) {
    const host = $(id);
    if (host) host.replaceChildren();
  }
  renderDiffSummary();
  bgSync();
}
// 中止/出错时把仍在跑的条目标记为中止,不再空转;整轮停止/出错是用户看得见的事,
// 这些条目算已确认,直接进已完成(带 ✗),停止按钮随之撤掉。
export function bgAbortRunning(label) {
  for (const [id, entry] of bgEntries) {
    if (entry.done) continue;
    entry.done = true;
    entry.cls = "err";
    entry.outcomeState = "interrupted";
    entry.acked = true;
    entry.el.classList.remove("running");
    entry.el.classList.add("err");
    entry.el.dataset.bgStatus = "err";
    entry.prog.textContent = label;
    bgPlace(entry);
    bgRenderActions(id, entry);
  }
  renderBgSections();
}
defer(() => {
  setInterval(() => {
    for (const entry of bgEntries.values()) {
      if (!entry.done) bgTick(entry);
    }
  }, 1000);
});

// ---------- 当前进展:侧边栏实时状态卡(把握 agent 进度,不用等它汇报) ----------
export const liveTextSources = new Map();
export function syncDynamicUiLanguage() {
  if (statusTextSource) setStatus(statusTextSource, statusRunning);
  for (const [id, source] of liveTextSources) {
    const el = $(id);
    if (!el) continue;
    el.textContent = localizeDynamic(source);
    el.title = localizeDynamic(source);
  }
  if (!$("context-detail")?.classList.contains("hidden")) renderContextDetail();
  renderAutoStatus(autoStopReason);
  // UI-0926 #8:子代理卡片与侧栏的计数/状态词在渲染点经 t() 产出,切语言时重画。
  subagentRelocalize();
}
export function liveSet(id, text) {
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
export function liveIdle(label) {
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
export function liveTurn(text) {
  const turn = $("live-turn");
  const source = String(text ?? "");
  liveTextSources.set("live-turn", source);
  if (turn) {
    turn.textContent = localizeDynamic(source);
    turn.classList.remove("hidden");
    turn.classList.remove("dim");
  }
}

// ---------- 运行审计摘要(侧栏 #agent-audit)----------
// UI-0926 #8:子代理的数据模型与卡片归 05-subagents.js,侧栏归 06-agent-panel.js;这里只留
// 按会话累计的运行审计(主代理调用/子代理派发/权限询问与拒绝/失败与超时),16-settings 与
// 03-shell 共用的 fastStatusText 也留在这里。
export const agentAudits = new Map(); // session_id -> latest run audit projection
export let agentAuditSession = null;

export function newAgentAudit(sessionId) {
  return {
    sessionId,
    primaryModel: "",
    primaryCalls: 0,
    primaryTokens: 0,
    tasks: new Map(),
    permissionPrompts: 0,
    permissionDenials: 0,
    state: "running",
    finished: false,
  };
}

export function agentAuditFor(sessionId = activeSessionId) {
  if (!sessionId) return null;
  let audit = agentAudits.get(sessionId);
  if (!audit) {
    audit = newAgentAudit(sessionId);
    agentAudits.set(sessionId, audit);
  }
  agentAuditSession = sessionId;
  return audit;
}

export function agentAuditBegin(sessionId) {
  if (!sessionId) return;
  const previous = agentAudits.get(sessionId);
  const audit = newAgentAudit(sessionId);
  audit.primaryModel = previous?.primaryModel || "";
  agentAudits.set(sessionId, audit);
  agentAuditSession = sessionId;
  const card = $("agent-audit");
  if (card && sessionId === activeSessionId) card.classList.add("hidden");
}

export function auditUsageTokens(usage) {
  if (!usage) return 0;
  return ["input", "output", "cache_read", "cacheRead", "cache_write", "cacheWrite"]
    .reduce((sum, key) => sum + (Number(usage[key]) || 0), 0);
}

export function formatAuditTokens(value) {
  const tokens = Number(value) || 0;
  return tokens < 1000 ? String(tokens) : `${(tokens / 1000).toFixed(tokens < 10000 ? 1 : 0)}k`;
}

export function agentAuditMeta(sessionId, model) {
  const audit = agentAuditFor(sessionId);
  if (audit) audit.primaryModel = String(model || "");
}

export function agentAuditStep(sessionId, payload) {
  const audit = agentAuditFor(sessionId);
  if (!audit) return;
  audit.primaryCalls += 1;
  audit.primaryTokens += auditUsageTokens(payload);
}

export function agentAuditTaskStart(sessionId, payload) {
  const audit = agentAuditFor(sessionId);
  if (!audit || payload.name !== "task") return;
  const input = payload.input || {};
  const model = typeof input.model === "string" && input.model.trim() ? input.model : "fast";
  const existing = audit.tasks.get(String(payload.id));
  audit.tasks.set(String(payload.id), {
    id: String(payload.id),
    // 失败清单显示身份与描述,不显示 call_… 调用 id(UI-0926 #8)。
    label: [typeof input.role === "string" ? input.role : "", subagentDescription(input, payload.summary)].filter(Boolean).join(" · "),
    model,
    usage: existing?.usage || null,
    status: "running",
    preview: "",
  });
}

export function agentAuditTaskProgress(sessionId, payload) {
  const audit = agentAuditFor(sessionId);
  const trace = payload.trace;
  if (!audit || !trace) return;
  const task = audit.tasks.get(String(payload.id));
  if (!task) return;
  if (trace.phase === "usage") task.usage = trace.usage || null;
  // UI-0926 #8:meta trace 上报实际模型 id,取代 input.model 的 fast/primary 档位。
  if (trace.phase === "meta" && trace.model) task.model = String(trace.model);
}

export function agentAuditTaskEnd(sessionId, payload) {
  const audit = agentAuditFor(sessionId);
  if (!audit || payload.name !== "task") return;
  const id = String(payload.id);
  const task = audit.tasks.get(id) || { id, model: "fast", usage: null };
  const preview = String(payload.preview || "");
  // UI-0926 #8:用户主动停止的(code=subagent_cancelled,旧数据按文案兜底)记为 stopped,不进「失败与超时」
  // 清单——与卡片的 classifySubagentEnd 同一口径(卡片显示「已停止」,审计不该写「失败」)。
  const stopped = !payload.ok && classifySubagentEnd({ ok: false, code: payload.code, preview }) === "cancelled";
  task.status = payload.ok
    ? "succeeded"
    : stopped ? "stopped"
      : payload.code === "subagent_timeout" || /超时|timeout|wall-clock|timed out/i.test(preview) ? "timeout" : "failed";
  task.preview = preview;
  audit.tasks.set(id, task);
}

export function agentAuditPermission(sessionId, payload) {
  const audit = agentAuditFor(sessionId);
  if (!audit) return;
  if (["deny", "declined"].includes(String(payload.decision || ""))) audit.permissionDenials += 1;
}

export function agentAuditPrompt(sessionId, payload) {
  if (payload.kind === "permission") {
    const audit = agentAuditFor(sessionId);
    if (audit) audit.permissionPrompts += 1;
  }
}

export function auditFact(label, value) {
  const row = document.createElement("div");
  row.className = "agent-audit-fact";
  const key = document.createElement("span");
  key.className = "agent-audit-label";
  key.textContent = label;
  const content = document.createElement("strong");
  content.textContent = value;
  row.append(key, content);
  return row;
}

export function renderAgentAudit(sessionId = agentAuditSession || activeSessionId) {
  const card = $("agent-audit");
  if (!card) return;
  const audit = agentAudits.get(sessionId);
  if (!audit || !audit.finished) {
    card.classList.add("hidden");
    return;
  }
  card.classList.remove("hidden");
  const taskList = [...audit.tasks.values()];
  const failures = taskList.filter((task) => task.status === "failed" || task.status === "timeout");
  const stateLabel = audit.state === "failed" ? t("运行失败") : audit.state === "stopped" ? t("运行已停止") : t("运行完成");
  $("agent-audit-state").textContent = stateLabel;
  const facts = $("agent-audit-facts");
  facts.replaceChildren(
    auditFact(t("主代理调用"), `${audit.primaryCalls} · ${formatAuditTokens(audit.primaryTokens)} ${t("token")}`),
    auditFact(t("子代理派发"), String(taskList.length)),
    auditFact(t("权限询问"), String(audit.permissionPrompts)),
    auditFact(t("权限拒绝"), String(audit.permissionDenials)),
  );
  const modelRows = $("agent-audit-models");
  modelRows.replaceChildren();
  const modelTitle = document.createElement("div");
  modelTitle.className = "agent-audit-subtitle";
  modelTitle.textContent = t("模型调用与 token");
  modelRows.appendChild(modelTitle);
  const models = new Map();
  const primaryKey = audit.primaryModel || t("未知模型");
  models.set(primaryKey, { calls: audit.primaryCalls, tokens: audit.primaryTokens });
  for (const task of taskList) {
    const row = models.get(task.model) || { calls: 0, tokens: 0 };
    row.calls += 1;
    row.tokens += auditUsageTokens(task.usage);
    models.set(task.model, row);
  }
  for (const [model, stats] of models) {
    const row = document.createElement("div");
    row.className = "agent-audit-model";
    row.textContent = `${model} · ${stats.calls} ${t("调用次数")} · ${formatAuditTokens(stats.tokens)} ${t("token")}`;
    modelRows.appendChild(row);
  }
  const failureBox = $("agent-audit-failures");
  failureBox.replaceChildren();
  failureBox.classList.toggle("hidden", failures.length === 0);
  if (failures.length) {
    const title = document.createElement("div");
    title.className = "agent-audit-subtitle";
    title.textContent = t("失败与超时");
    failureBox.appendChild(title);
    for (const task of failures) {
      const row = document.createElement("div");
      row.className = `agent-audit-failure ${task.status}`;
      row.textContent = `${task.label || task.id} · ${task.status === "timeout" ? t("超时") : t("失败")}${task.preview ? ` · ${task.preview}` : ""}`;
      failureBox.appendChild(row);
    }
  } else {
    const empty = document.createElement("div");
    empty.className = "agent-audit-empty";
    empty.textContent = t("无失败或超时");
    failureBox.appendChild(empty);
    failureBox.classList.remove("hidden");
  }
}

export function agentAuditFinish(sessionId, state = "completed") {
  const audit = agentAuditFor(sessionId);
  if (!audit) return;
  audit.state = state;
  audit.finished = true;
  renderAgentAudit(sessionId);
}

// D-278:子代理就绪文案——设置页 fast 行与侧边栏子代理面板共用同一计算,避免两处漂移。
// s 来自 fast_model_status 的载荷:managed/ready/model/installed/serviceUp。
export function fastStatusText(s) {
  if (!s.managed) return { text: t("fast 指向外部 provider,不由本机托管"), warn: false };
  if (s.ready) return { text: `✓ ${t("子代理就绪")}(${s.model})`, warn: false };
  const missing = !s.installed
    ? t("Ollama 未安装")
    : !s.serviceUp
      ? t("Ollama 服务未运行")
      : `${t("模型未拉取")}(${s.model})`;
  return { text: `⚠ ${missing} — ${t("子代理杂活(记忆整理/快速记录)暂不可用")}`, warn: true };
}
