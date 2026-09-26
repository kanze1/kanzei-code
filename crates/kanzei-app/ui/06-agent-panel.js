// UI2-0926 #14 后台任务侧栏(docs/design/subagent_presentation.md §5.6/§7;用户原话「子代理也弄成侧栏,
// 参考claude的结构呢？弹出也自动化一点」)。Claude 桌面端 Background tasks 的结构,取代原先浮在正文上的
// 「活动」与「子代理」两块浮层(R-334 的浮层定调由用户 2026-09-26 推翻):
//   · 停靠:#main 网格第 2 列,从顶部到状态栏上沿,对话列与运行日志随之变窄、状态栏保持全宽;
//     停靠后对话列不足 600px(主区宽 − 侧栏宽)时改抽屉 + 遮罩。
//   · 三段:运行中 / 需要关注(未确认的失败)/ 已完成(折叠,展开才渲染委派卡)。一批委派一张卡(标题、用时、
//     「N 个子代理 · token」、阶段进度点、「子代理 · 模型 · Token · 用时」小表),与主对话里的 sa-group 一一对应;
//     终端命令复用 06-activity 的条目卡(停止/复制/导出/重跑/实时输出)。
//   · 两种模式(#tasks-panel[data-mode]):list 与 detail(一次委派的完整过程,与卡片内联展开同一个渲染器)。
//   · 自动开合:纯策略模块 06-side-policy.js;本模块把实时事件喂给它,并且是显隐、data-dock、#main[data-side]、
//     --kz-side-col / --kz-dock-right、rail 开关与徽标的**唯一写入者**(reconcileTasksPanel)。
//     自动打开不抢焦点、不滚动对话(重排前记下首个可见消息,重排后按它恢复;跟随最新时贴底)。
// 数据只读 05-subagents.js 与 06-activity.js 的模型,不新增 IPC 或事件。
import { installSplit } from "./00-frame.js";
import { $, activePane, defer, invoke, messages, motionCount, motionSync, on } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeSessionId, log, navigate_view } from "./03-shell.js";
import { onLayoutChange, setSidePanelPref, sidePanelPrefs } from "./03-layout.js";
import { followLatest, scrollBottom } from "./05-chat-render.js";
import {
  SA_ACTIVE,
  createSubagentView,
  formatElapsed,
  formatTokenCount,
  onSubagentChange,
  onSubagentTick,
  renderSubagentTimeline,
  subagentAgentName,
  subagentByKey,
  subagentCopyResult,
  subagentCountLabel,
  subagentElapsed,
  subagentGlyph,
  subagentLocate,
  subagentMetaText,
  subagentRunningCount,
  subagentRunsFor,
  subagentStateWord,
  subagentStop,
  subagentUsageTokens,
} from "./05-subagents.js";
import { bgAck, bgDoneOpen, bgRunningCount, bgSectionCounts, fastStatusText, onBgChange, orchPhaseLabel, setBgDoneOpen } from "./06-activity.js";
import {
  SIDE_WIDTH_MIN,
  createSideModel,
  sideClampWidth,
  sideDecide,
  sideDefaultWidth,
  sideDockMode,
  sideEvent,
  sideMaxWidth,
} from "./06-side-policy.js";

// 值得停留的实时失败:失败/超时/中断/未启动。用户自己停的(cancelled)不算。
const HOLD_STATES = new Set(["failed", "timeout", "interrupted", "rejected"]);
const SECTION_HOST = { running: "tasks-running", attention: "tasks-attention", done: "tasks-done" };

const model = createSideModel();
let clock = () => Date.now();
let panelMode = "list";
let detailRun = null;
let detailView = null;
let detailUi = null;
let lastInvoker = null;
let lingerTimer = null;
let splitApi = null;
let lastDecision = { visible: false, reason: null, dock: null, lingerMs: null, badge: null };
let lastDoneOpen = null;
let reconcileQueued = false;
const interact = { pointer: false, focus: false, on: false };
const cards = new Map(); // `${sid}|b${batch}` -> card
const phaseOpen = new Map(); // 卡片 key -> 用户点过的阶段展开态
const reportedStart = new WeakSet();
const reportedFailure = new WeakSet();
const ackedRuns = new WeakSet();

/// 冒烟注入时钟(推进它验证 6 秒延迟收起);应用里恒为 Date.now。
export function setTasksPanelClock(fn) {
  clock = typeof fn === "function" ? fn : () => Date.now();
}

function isInside(node, container) {
  for (let n = node; n; n = n.parentNode) if (n === container) return true;
  return false;
}
function el(tag, className = "", text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}
function iconButton(className, text, key) {
  const button = el("button", `icon-btn ${className}`, text);
  button.type = "button";
  button.setAttribute("aria-label", t(key));
  button.dataset.i18nAriaLabel = key;
  button.title = t(key);
  button.dataset.i18nTitle = key;
  return button;
}
function currentView() {
  return document.body?.dataset?.view || "chat";
}
function sid() {
  return activeSessionId || "";
}

// ---------- 几何:主区宽、侧栏宽、停靠判据 ----------
// 主区内容盒宽度由 ResizeObserver 缓存:reconcile 每条子代理进度都会跑,逐次 getBoundingClientRect 会强制排版。
let mainWidthCache = 0;
function mainWidth() {
  const width = mainWidthCache || $("main")?.getBoundingClientRect?.().width || 0;
  // 没有布局的环境(假 DOM、首帧之前)按窗口宽减去 rail 估算。
  return width > 0 ? width : Math.max(0, (window.innerWidth || 0) - 48);
}
function panelWidth(mainW) {
  const stored = splitApi?.value?.() ?? null;
  return sideClampWidth(stored ?? sideDefaultWidth(window.innerWidth || 0), mainW);
}
function activeCount(sessionId = sid()) {
  return subagentRunningCount(sessionId) + bgRunningCount(sessionId);
}

// ---------- 对话滚动锚点:停靠/取消停靠会让对话列变宽变窄、长消息重新换行 ----------
function captureScrollAnchor() {
  if (!messages || currentView() !== "chat") return null;
  if (followLatest) return { follow: true };
  const box = messages.getBoundingClientRect?.();
  const kids = activePane?.children ?? [];
  if (!box) return null;
  for (let index = 0; index < kids.length; index += 1) {
    const rect = kids[index].getBoundingClientRect?.();
    if (rect && rect.bottom > box.top) return { el: kids[index], offset: rect.top - box.top };
  }
  return null;
}
function restoreScrollAnchor(anchor) {
  if (!anchor) return;
  if (anchor.follow) {
    scrollBottom(true);
    return;
  }
  if (anchor.el?.isConnected === false) return;
  const box = messages.getBoundingClientRect?.();
  const rect = anchor.el.getBoundingClientRect?.();
  if (!box || !rect) return;
  const delta = rect.top - box.top - anchor.offset;
  if (delta) messages.scrollTop += delta;
}

// ---------- 唯一写入者 ----------
/// 按策略重算侧栏:显隐、停靠/抽屉、#main 列宽、--kz-dock-right、遮罩、rail 开关与徽标、延迟收起计时。
/// 触发点:子代理变更、终端条目变更、用户发消息、切线、切视图、主区尺寸变化、偏好变化、计时到点。
export function reconcileTasksPanel() {
  const panel = $("tasks-panel");
  if (!panel) return lastDecision;
  const main = $("main");
  const sessionId = sid();
  const mainW = mainWidth();
  const width = panelWidth(mainW);
  const dock = sideDockMode({ mainWidth: mainW, panelWidth: width });
  const active = activeCount(sessionId);
  const decision = sideDecide(model, { sid: sessionId, view: currentView(), dock, active, now: clock(), prefs: sidePanelPrefs() });
  const wasVisible = !panel.classList.contains("hidden");
  const docked = decision.visible && decision.dock === "side";
  const layoutChanges = decision.visible !== wasVisible || (decision.visible && panel.dataset.dock !== decision.dock);
  const anchor = layoutChanges ? captureScrollAnchor() : null;
  panel.classList.toggle("hidden", !decision.visible);
  panel.dataset.dock = decision.visible ? decision.dock : dock;
  panel.style.setProperty("--kz-tasks-w", `${width}px`);
  if (main) {
    const side = !decision.visible ? "closed" : docked ? "docked" : "drawer";
    if (main.dataset.side !== side) main.dataset.side = side;
    main.style.setProperty("--kz-side-col", docked ? `${width}px` : "0px");
  }
  // 权限卡的「非对话视图退回右下」与重新打开芯片按停靠宽度让开侧栏(surface.css 读 <html> 上的这个变量)。
  document.documentElement.style.setProperty("--kz-dock-right", docked ? `${width}px` : "0px");
  $("tasks-scrim")?.classList.toggle("hidden", !(decision.visible && decision.dock === "drawer"));
  syncToggle(decision.visible);
  syncBadge(decision.badge, active);
  syncWide(width, mainW);
  if (anchor) restoreScrollAnchor(anchor);
  lastDecision = decision;
  if (decision.visible && !wasVisible) {
    void refreshAgentPanelStatus(); // D-278:每次打开都刷新就绪状态
    renderTasksList();
    splitApi?.sync();
  }
  clearTimeout(lingerTimer);
  lingerTimer = null;
  if (decision.lingerMs !== null) {
    lingerTimer = setTimeout(() => {
      lingerTimer = null;
      reconcileTasksPanel();
    }, decision.lingerMs + 30);
  }
  return decision;
}
function queueReconcile() {
  if (reconcileQueued) return;
  reconcileQueued = true;
  const run = () => {
    reconcileQueued = false;
    reconcileTasksPanel();
  };
  if (typeof requestAnimationFrame === "function") requestAnimationFrame(run);
  else setTimeout(run, 0);
}
/// 调试与冒烟:侧栏此刻的决策。
export function tasksPanelState() {
  return { ...lastDecision, mode: panelMode, pinned: model.pinned };
}

function syncToggle(visible) {
  const toggle = $("tasks-toggle");
  if (!toggle) return;
  toggle.classList.toggle("active", visible);
  toggle.setAttribute("aria-expanded", String(visible));
}
function syncBadge(badge, active) {
  const toggle = $("tasks-toggle");
  if (toggle) {
    toggle.dataset.running = String(active > 0);
    // 可访问名只在状态变化时重写(读屏不反复播报):「打开或收起后台任务侧栏 · 2 运行中」。
    const bits = [t("打开或收起后台任务侧栏")];
    if (badge?.tone === "run") bits.push(`${badge.count} ${t("运行中")}`);
    else if (badge?.tone === "err") bits.push(`${badge.count} ${t("个失败待查看")}`);
    const label = bits.join(" · ");
    if (toggle.getAttribute("aria-label") !== label) toggle.setAttribute("aria-label", label);
  }
  const node = $("tasks-badge");
  if (node) {
    const text = badge ? String(badge.count) : "";
    if (node.textContent !== text) node.textContent = text;
    node.dataset.tone = badge?.tone ?? "";
    node.classList.toggle("hidden", !badge);
  }
  const head = $("tasks-count");
  if (head) {
    const text = active ? `${active} ${t("运行中")}` : "";
    if (head.textContent !== text) head.textContent = text;
  }
}
function syncWide(width, mainW) {
  const button = $("tasks-wide");
  if (!button) return;
  const max = sideMaxWidth(mainW);
  const wide = max > sideDefaultWidth(window.innerWidth || 0) && width >= max - 1;
  button.setAttribute("aria-pressed", String(wide));
  const key = wide ? "恢复宽度" : "加宽侧栏";
  if (button.dataset.i18nTitle !== key) {
    button.dataset.i18nTitle = key;
    button.dataset.i18nAriaLabel = key;
    button.title = t(key);
    button.setAttribute("aria-label", t(key));
  }
}

// ---------- 开关 API ----------
/// 用户打开(rail、卡片 ↗、组头「查看全部」、「去后台任务侧栏看全」、压缩纪要入口、命令面板):只开不切换。
/// key → 该次委派的详情;batch → 列表里定位到那一批;reveal → 定位到某个条目节点。
export function openTasksPanel({ invoker = null, key = null, batch = null, reveal = null } = {}) {
  if (invoker) lastInvoker = invoker;
  if (currentView() !== "chat") navigate_view("chat");
  sideEvent(model, { type: "user-open", sid: sid() });
  const decision = reconcileTasksPanel();
  const run = key ? subagentByKey(key) : null;
  if (run) showDetail(run);
  else if (panelMode !== "list") showList();
  if (batch !== null && batch !== undefined) revealBatch(batch);
  if (reveal) revealNode(reveal);
  // 抽屉态是盖在对话上的:焦点进侧栏头部,关闭时还回去。停靠态不抢焦点(用户可能正在打字)。
  if (!run && decision.visible && decision.dock === "drawer") $("tasks-close")?.focus?.();
  return decision;
}
/// 用户关闭(✕、rail、Esc、遮罩):确认本线路的失败;本次运行里还有活时压制自动打开,直到下一条用户消息。
export function closeTasksPanel({ returnFocus = false } = {}) {
  const sessionId = sid();
  sideEvent(model, { type: "user-close", sid: sessionId, active: activeCount(sessionId) });
  ackLine(sessionId);
  resetDetail();
  reconcileTasksPanel();
  if (returnFocus) {
    const target = lastInvoker && lastInvoker.isConnected !== false ? lastInvoker : $("tasks-toggle");
    target?.focus?.();
  }
}
export function toggleTasksPanel(invoker = null) {
  if (lastDecision.visible && currentView() === "chat") closeTasksPanel();
  else openTasksPanel({ invoker });
}
/// 05-subagents 的入口:卡片 ↗ / 内联展开区「在侧栏查看完整过程」带 key 进详情;组头「查看全部」不带 key,
/// 定位到它所在的那一批。
export function openSubagentPanel(key = null, invoker = null) {
  const batch = key ? null : Number(invoker?.closest?.(".sa-group")?.dataset?.batch);
  return openTasksPanel({ invoker, key, batch: Number.isFinite(batch) && batch > 0 ? batch : null });
}
/// 用户手动发消息(08-compose-runtime sendText 的非鞭挞分支):新的一次运行——解除压制,上次的失败算看过了。
export function tasksPanelUserRun(sessionId = sid()) {
  sideEvent(model, { type: "user-run", sid: sessionId || "" });
  ackLine(sessionId || "");
  reconcileTasksPanel();
}
function ackLine(sessionId) {
  bgAck(sessionId);
  for (const run of subagentRunsFor(sessionId)) if (HOLD_STATES.has(run.state)) ackedRuns.add(run);
  sideEvent(model, { type: "ack", sid: sessionId });
  renderTasksList();
}

// ---------- 委派卡 ----------
function batchKey(sessionId, batch) {
  return `${sessionId || ""}|b${batch}`;
}
function batchesOf(sessionId) {
  const batches = new Map();
  for (const run of subagentRunsFor(sessionId)) {
    if (!batches.has(run.batch)) batches.set(run.batch, []);
    batches.get(run.batch).push(run);
  }
  return [...batches.entries()].sort((a, b) => b[0] - a[0]);
}
function batchSection(runs) {
  if (runs.some((run) => SA_ACTIVE.has(run.state))) return "running";
  if (runs.some((run) => HOLD_STATES.has(run.state) && !run.replay && !ackedRuns.has(run))) return "attention";
  return "done";
}
function stripInline(text) {
  return String(text ?? "")
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/\*\*([^*]*)\*\*/g, "$1")
    .replace(/^\s{0,3}(?:#{1,6}|[-*+]|\d+[.)])\s+/gm, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .trim();
}
function clip(text, max) {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}
/// 派发这批子代理之前那段助手话(文本模型 dataset.raw,不取 textContent——会把「复制」按钮的字带进来)。
function preambleOf(run) {
  let node = run?.groupEl?.previousElementSibling ?? null;
  for (let step = 0; node && step < 6; step += 1, node = node.previousElementSibling) {
    if (node.classList?.contains("user")) return "";
    if (node.classList?.contains("assistant") && node.dataset?.raw) return stripInline(node.dataset.raw);
  }
  return "";
}
function firstSentence(text) {
  const line = text.split(/\n+/).find((item) => item.trim()) ?? "";
  const sentence = line.split(/(?<=[。！？!?:：])|(?<=\.)(?=\s)/)[0] ?? line;
  return clip(sentence.trim().replace(/[。.！？!?:：]+$/, ""), 60);
}
function batchTitle(runs) {
  if (runs.length === 1) return runs[0].description || t("子代理");
  const preamble = preambleOf(runs[0]);
  if (preamble) return firstSentence(preamble) || t("并行委派");
  return runs[0].phase ? `${orchPhaseLabel(runs[0].phase)} · ${subagentCountLabel(runs.length)}` : t("并行委派");
}
function batchElapsed(runs, now) {
  if (runs.some((run) => run.replay)) {
    const durations = runs.map((run) => run.durationMs).filter(Number.isFinite);
    return durations.length ? Math.max(...durations) : null;
  }
  // 结束时刻优先用后端量的 durationMs(不含前端事件排队延迟,与卡片计数同一口径)。
  const start = Math.min(...runs.map((run) => run.startedAt));
  const endOf = (run) => (SA_ACTIVE.has(run.state) ? now : Number.isFinite(run.durationMs) ? run.startedAt + run.durationMs : run.endedAt ?? now);
  return Math.max(0, Math.max(...runs.map(endOf)) - start);
}
function rowState(run) {
  if (SA_ACTIVE.has(run.state)) return "running";
  if (HOLD_STATES.has(run.state)) return run.state === "failed" ? "failed" : "warn";
  if (run.state === "cancelled") return "stopped";
  return "done";
}

function buildCard(key, sessionId, batch) {
  const card = el("article", "tp-card");
  card.dataset.kind = "delegation";
  card.dataset.tpKey = key;
  card.dataset.batch = String(batch);
  const head = el("div", "tp-card-head");
  const title = el("button", "tp-card-title");
  title.type = "button";
  const stop = iconButton("tp-card-stop", "■", "停止这批子代理");
  head.append(title, stop);
  const sub = el("div", "tp-card-sub");
  const kind = el("span", "tp-card-kind", t("子代理"));
  const elapsed = el("span", "tp-card-elapsed");
  sub.append(kind, el("span", "tp-sep", " · "), elapsed);
  const stats = el("div", "tp-card-sub tp-card-stats");
  const desc = el("p", "tp-card-desc hidden");
  const phase = el("div", "tp-phase");
  const phaseHead = el("button", "tp-phase-head");
  phaseHead.type = "button";
  const phaseLabel = el("span", "tp-phase-label");
  const phaseCount = el("span", "tp-phase-count");
  const caret = el("span", "tp-phase-caret");
  caret.setAttribute("aria-hidden", "true");
  phaseHead.append(phaseLabel, phaseCount, caret);
  const dots = el("div", "tp-dots");
  dots.setAttribute("aria-hidden", "true");
  const table = el("div", "tp-agents");
  const tableHead = el("div", "tp-agents-head");
  tableHead.setAttribute("aria-hidden", "true");
  tableHead.append(el("span", "", t("子代理")), el("span", "", t("模型")), el("span", "", t("Token")), el("span", "", t("用时")));
  const rows = el("div", "tp-agents-rows");
  rows.setAttribute("role", "list");
  table.append(tableHead, rows);
  phase.append(phaseHead, dots, table);
  card.append(head, sub, stats, desc, phase);
  const record = {
    key, sessionId, batch, el: card, runs: [], section: null,
    ui: { title, stop, elapsed, stats, desc, phaseLabel, phaseCount, caret, phaseHead, dots, table, rows, rowEls: new Map(), dotEls: [] },
  };
  title.addEventListener("click", () => {
    // 卡片标题 = 定位到对话里的那一组(卡片已被裁掉时 subagentLocate 自己说清楚)。
    if (record.runs[0]) subagentLocate(record.runs[0]);
  });
  stop.addEventListener("click", () => {
    for (const run of record.runs) if (SA_ACTIVE.has(run.state) && run.state !== "stopping") void subagentStop(run);
  });
  phaseHead.addEventListener("click", () => {
    phaseOpen.set(key, !isPhaseOpen(record));
    updateCard(record, clock());
  });
  return record;
}
function isPhaseOpen(record) {
  return phaseOpen.has(record.key) ? phaseOpen.get(record.key) : record.section !== "done";
}
function buildRow(run) {
  const row = el("button", "tp-agent-row");
  row.type = "button";
  row.setAttribute("role", "listitem");
  row.dataset.saKey = run.key;
  const name = el("span", "tp-agent-name");
  const glyph = el("span", "kz-glyph sa-glyph");
  glyph.setAttribute("aria-hidden", "true");
  const label = el("span", "tp-agent-label");
  name.append(glyph, label);
  const modelCell = el("span", "tp-agent-model");
  const tokens = el("span", "tp-agent-tok");
  const time = el("span", "tp-agent-time");
  row.append(name, modelCell, tokens, time);
  row._tp = { glyph, label, model: modelCell, tokens, time, aria: null };
  row.addEventListener("click", () => {
    lastInvoker = row;
    showDetail(run);
  });
  return row;
}
function updateRowTime(row, run, now) {
  const ms = subagentElapsed(run, now);
  const text = ms === null ? "—" : formatElapsed(ms);
  if (row._tp.time.textContent !== text) row._tp.time.textContent = text;
}
function updateRow(row, run, now) {
  const ui = row._tp;
  const [char, glyphState] = subagentGlyph(run);
  if (ui.glyph.dataset.state !== glyphState) {
    ui.glyph.dataset.state = glyphState;
    if (SA_ACTIVE.has(run.state)) motionSync(ui.glyph);
  }
  if (ui.glyph.textContent !== char) ui.glyph.textContent = char;
  row.dataset.s = rowState(run);
  row.dataset.saState = run.state;
  const who = [subagentAgentName(run), run.description].filter(Boolean).join(" · ") || t("子代理");
  if (ui.label.textContent !== who) {
    ui.label.textContent = who;
    ui.label.title = who;
  }
  const modelText = run.model || run.tier || "—";
  if (ui.model.textContent !== modelText) {
    ui.model.textContent = modelText;
    ui.model.title = modelText;
  }
  const tokens = subagentUsageTokens(run.usage);
  const tokenText = tokens > 0 ? formatTokenCount(tokens) : "—";
  if (ui.tokens.textContent !== tokenText) ui.tokens.textContent = tokenText;
  updateRowTime(row, run, now);
  // 可访问名与卡片同一口径:只随状态/身份变化重写,运行中不带逐秒变化的耗时。
  const aria = [who, subagentStateWord(run.state) || t("运行中"), SA_ACTIVE.has(run.state) ? "" : subagentMetaText(run, now)]
    .filter(Boolean).join(" — ");
  if (ui.aria !== aria) {
    ui.aria = aria;
    row.setAttribute("aria-label", aria);
  }
}
function updateCard(record, now) {
  const { ui, runs } = record;
  const n = runs.length;
  const running = runs.filter((run) => SA_ACTIVE.has(run.state)).length;
  const finished = n - running;
  const failed = runs.filter((run) => HOLD_STATES.has(run.state)).length;
  record.el.dataset.state = record.section ?? batchSection(runs);
  const title = batchTitle(runs);
  if (ui.title.textContent !== title) {
    ui.title.textContent = title;
    ui.title.title = `${title} · ${t("定位到对话")}`;
  }
  ui.stop.classList.toggle("is-off", running === 0);
  ui.stop.disabled = running === 0;
  const elapsed = batchElapsed(runs, now);
  const elapsedText = elapsed === null ? "—" : formatElapsed(elapsed);
  if (ui.elapsed.textContent !== elapsedText) ui.elapsed.textContent = elapsedText;
  const tokens = runs.reduce((sum, run) => sum + subagentUsageTokens(run.usage), 0);
  const stats = [subagentCountLabel(n)];
  if (tokens > 0) stats.push(`${formatTokenCount(tokens)} ${t("token")}`);
  if (failed) stats.push(`${failed} ${t("失败")}`);
  const statsText = stats.join(" · ");
  if (ui.stats.textContent !== statsText) ui.stats.textContent = statsText;
  const preamble = n > 1 ? preambleOf(runs[0]) : "";
  const descText = preamble && firstSentence(preamble) !== clip(preamble, 60) ? clip(preamble, 240) : "";
  if (ui.desc.textContent !== descText) ui.desc.textContent = descText;
  ui.desc.classList.toggle("hidden", !descText);
  const phaseText = runs[0]?.phase ? orchPhaseLabel(runs[0].phase) : t("委派");
  if (ui.phaseLabel.textContent !== phaseText) ui.phaseLabel.textContent = phaseText;
  const countText = `${finished}/${n}`;
  if (ui.phaseCount.textContent !== countText) ui.phaseCount.textContent = countText;
  const open = isPhaseOpen(record);
  ui.phaseHead.setAttribute("aria-expanded", String(open));
  ui.caret.textContent = open ? "⌄" : "›";
  ui.table.classList.toggle("hidden", !open);
  // 进度点:每个子代理一个(完成 = 中性,在跑 = 强调色,失败 = 红;批次进度不用绿,ui_color_semantics §4)。
  while (ui.dotEls.length < n) {
    const dot = el("i");
    ui.dots.appendChild(dot);
    ui.dotEls.push(dot);
  }
  runs.forEach((run, index) => {
    const state = rowState(run);
    if (ui.dotEls[index].dataset.s !== state) ui.dotEls[index].dataset.s = state;
  });
  if (!open) return;
  for (const run of runs) {
    let row = ui.rowEls.get(run);
    if (!row) {
      row = buildRow(run);
      ui.rowEls.set(run, row);
      ui.rows.appendChild(row);
    }
    updateRow(row, run, now);
  }
}

/// 列表重画:委派卡按批次(新的在上)落进三段;已完成段折叠时不建卡,只计数。
export function renderTasksList() {
  const panel = $("tasks-panel");
  if (!panel) return;
  const sessionId = sid();
  const now = clock();
  for (const record of cards.values()) record.el.remove();
  cards.clear();
  if (!panel.classList.contains("hidden")) {
    for (const [batch, runs] of batchesOf(sessionId)) {
      const section = batchSection(runs);
      if (section === "done" && !bgDoneOpen) continue;
      const key = batchKey(sessionId, batch);
      const record = buildCard(key, sessionId, batch);
      record.runs = runs;
      record.section = section;
      updateCard(record, now);
      $(SECTION_HOST[section])?.appendChild(record.el);
      cards.set(key, record);
    }
  }
  lastDoneOpen = bgDoneOpen;
  renderChrome();
}
function delegationCounts(sessionId) {
  const counts = { running: 0, attention: 0, done: 0 };
  for (const [, runs] of batchesOf(sessionId)) counts[batchSection(runs)] += 1;
  return counts;
}
/// 段头:计数、空态、「需要关注」与「已完成」的显隐、已完成的折叠(委派卡与终端条目合并计数)。
function renderChrome() {
  const bg = bgSectionCounts();
  const dg = delegationCounts(sid());
  const running = bg.running + dg.running;
  const attention = bg.attention + dg.attention;
  const done = bg.done + dg.done;
  const runCount = $("bg-running-count");
  if (runCount) motionCount(runCount, running ? String(running) : "");
  $("bg-running-empty")?.classList.toggle("hidden", running > 0);
  $("bg-section-attention")?.classList.toggle("hidden", attention === 0);
  const attentionCount = $("bg-attention-count");
  if (attentionCount) attentionCount.textContent = attention ? String(attention) : "";
  $("bg-section-done")?.classList.toggle("hidden", done === 0);
  const doneCount = $("bg-done-count");
  if (doneCount) doneCount.textContent = done ? String(done) : "";
  $("bg-done-body")?.classList.toggle("hidden", !bgDoneOpen);
  const toggle = $("bg-done-toggle");
  if (toggle) {
    toggle.setAttribute("aria-expanded", String(bgDoneOpen));
    const caret = toggle.querySelector(".bg-section-caret");
    if (caret) caret.textContent = bgDoneOpen ? "▾" : "▸";
  }
  $("bg-clear-done")?.classList.toggle("hidden", bg.done === 0);
  // 筛选按钮的提示里给出「筛出/总数」:筛掉一半条目而看不出来,会以为本轮只跑了这几个工具。
  const filter = $("tasks-filter");
  if (filter) {
    const shown = bg.running + bg.attention + bg.done;
    const filtered = shown !== bg.total;
    filter.classList.toggle("active", filtered);
    const title = filtered ? `${t("筛选与清理")} · ${shown}/${bg.total}` : t("筛选与清理");
    if (filter.title !== title) filter.title = title;
  }
}
function updateCardFor(run) {
  if (!run || run.sessionId !== sid()) return;
  const record = cards.get(batchKey(run.sessionId, run.batch));
  const runs = subagentRunsFor(run.sessionId).filter((item) => item.batch === run.batch);
  const section = batchSection(runs);
  const panelVisible = !$("tasks-panel")?.classList.contains("hidden");
  // 新的一批、换段(运行中 → 已完成/需要关注)或已完成段里本来没建卡:整表重画(批次数量有限)。
  if (!record || record.section !== section) {
    if (panelVisible || record) renderTasksList();
    else renderChrome();
    return;
  }
  record.runs = runs;
  updateCard(record, clock());
  renderChrome();
}
function revealBatch(batch) {
  const key = batchKey(sid(), batch);
  let record = cards.get(key);
  if (!record && !bgDoneOpen) {
    setBgDoneOpen(true);
    renderTasksList();
    record = cards.get(key);
  }
  if (!record) return;
  record.el.scrollIntoView?.({ block: "nearest" });
  flash(record.el);
}
function revealNode(node) {
  if (isInside(node, $("bg-done-body")) && !bgDoneOpen) {
    setBgDoneOpen(true);
    renderTasksList();
  }
  node.scrollIntoView?.({ behavior: "smooth", block: "nearest" });
}
function flash(node) {
  node.classList.add("tp-flash");
  clearTimeout(node._tpFlash);
  node._tpFlash = setTimeout(() => node.classList.remove("tp-flash"), 1200);
}

// ---------- 详情 ----------
function setMode(mode) {
  panelMode = mode;
  const host = $("tasks-panel");
  if (host) host.dataset.mode = mode;
  $("agent-back")?.classList.toggle("hidden", mode !== "detail");
  $("bg-list")?.classList.toggle("hidden", mode !== "list");
  $("agent-detail")?.classList.toggle("hidden", mode !== "detail");
}
function resetDetail() {
  detailRun = null;
  detailView = null;
  detailUi = null;
  $("agent-detail")?.replaceChildren();
  setMode("list");
}
function showList() {
  resetDetail();
  renderTasksList();
}
function showDetail(run) {
  detailRun = run;
  // 打开一次失败的详情 = 看过了这次失败。
  if (HOLD_STATES.has(run.state) && !ackedRuns.has(run)) {
    ackedRuns.add(run);
    sideEvent(model, { type: "ack", sid: run.sessionId, key: run.key });
    reconcileTasksPanel();
  }
  setMode("detail");
  buildDetail(run);
  // 进入详情时焦点落在「‹ 返回」(§8;只在用户点进来时发生,自动打开不会进详情)。
  $("agent-back")?.focus?.();
}
function detailButton(key, handler) {
  const button = el("button", "ghost mini", t(key));
  button.type = "button";
  button.addEventListener("click", handler);
  return button;
}
function buildDetail(run) {
  const host = $("agent-detail");
  if (!host) return;
  host.replaceChildren();
  const head = el("div", "sa-detail-head");
  const glyph = el("span", "kz-glyph sa-glyph");
  glyph.setAttribute("aria-hidden", "true");
  const agent = el("span");
  const desc = el("span", "sa-desc");
  const word = el("span", "sa-detail-state");
  head.append(glyph, agent, desc, word);
  const meta = el("div", "sa-detail-meta");
  const actions = el("div", "sa-detail-actions");
  const stop = detailButton("停止", () => subagentStop(run));
  stop.classList.add("sa-detail-stop");
  const copy = detailButton("复制结果", () => subagentCopyResult(run));
  const locate = detailButton("定位到对话", () => subagentLocate(run));
  locate.classList.add("sa-detail-locate");
  actions.append(stop, copy, locate);
  const body = el("div", "sa-detail-body");
  host.append(head, meta, actions, body);
  detailUi = { glyph, agent, desc, word, meta, stop };
  detailView = createSubagentView(body, run, { inline: false });
  renderDetail(run);
}
function renderDetailMeta(run) {
  if (!detailUi) return;
  const modelText = run.model || run.tier;
  const text = [modelText, subagentMetaText(run)].filter(Boolean).join(" · ");
  if (detailUi.meta.textContent !== text) detailUi.meta.textContent = text;
}
function renderDetail(run) {
  if (!detailUi) return;
  const [char, state] = subagentGlyph(run);
  if (detailUi.glyph.dataset.state !== state) {
    detailUi.glyph.dataset.state = state;
    if (SA_ACTIVE.has(run.state)) motionSync(detailUi.glyph);
  }
  detailUi.glyph.textContent = char;
  const label = subagentAgentName(run);
  detailUi.agent.textContent = label;
  detailUi.agent.className = `sa-agent${label ? "" : " hidden"}`;
  detailUi.desc.textContent = run.description || t("子代理");
  detailUi.desc.title = run.description;
  detailUi.word.textContent = subagentStateWord(run.state) || t("运行中");
  detailUi.stop.classList.toggle("hidden", !SA_ACTIVE.has(run.state) || run.state === "stopping");
  const host = $("agent-detail");
  if (host) host.dataset.saState = run.state;
  renderDetailMeta(run);
  renderSubagentTimeline(detailView);
}

/// 切换线路:列表换成新线路的数据;详情不属于新线路时回到列表;按新线路的状态重算显隐与徽标。
export function agentPanelSync() {
  if (detailRun && detailRun.sessionId !== sid()) resetDetail();
  renderTasksList();
  reconcileTasksPanel();
}


// ---------- 实时事件 → 策略 ----------
// 在 defer 里注册:05-subagents ↔ 06-agent-panel ↔ 06-activity 有循环依赖,顶层调用对方的 onXxx 会撞 TDZ。
function subscribe() {
  onSubagentChange((run) => {
    if (run && !run.replay) {
      // 实时子代理第一次进入活动态 = work-start(自动打开的触发之一);历史回放不报。
      if (SA_ACTIVE.has(run.state) && !reportedStart.has(run)) {
        reportedStart.add(run);
        sideEvent(model, { type: "work-start", sid: run.sessionId }, sidePanelPrefs());
      }
      if (HOLD_STATES.has(run.state) && !reportedFailure.has(run)) {
        reportedFailure.add(run);
        sideEvent(model, { type: "failure", sid: run.sessionId, key: run.key, hold: true });
      }
    }
    if (panelMode === "detail" && detailRun) {
      // 该会话的历史被重载(run 已被丢弃):详情回到列表。
      if (run === null && !subagentRunsFor(detailRun.sessionId).includes(detailRun)) resetDetail();
      else if (run === null || run === detailRun) renderDetail(detailRun);
    }
    if (run === null) renderTasksList();
    else updateCardFor(run);
    reconcileTasksPanel();
  });
  onSubagentTick(() => {
    if ($("tasks-panel")?.classList.contains("hidden")) return;
    const now = clock();
    if (panelMode === "detail") {
      if (detailRun && SA_ACTIVE.has(detailRun.state)) renderDetailMeta(detailRun);
      return;
    }
    // 1 秒刷新只动计时文本;可访问名只随状态变化重写(updateRow),不每秒重写。
    for (const record of cards.values()) {
      if (!record.runs.some((run) => SA_ACTIVE.has(run.state))) continue;
      const elapsed = batchElapsed(record.runs, now);
      const text = elapsed === null ? "—" : formatElapsed(elapsed);
      if (record.ui.elapsed.textContent !== text) record.ui.elapsed.textContent = text;
      for (const [run, row] of record.ui.rowEls) if (SA_ACTIVE.has(run.state)) updateRowTime(row, run, now);
    }
  });
  onBgChange((event) => {
    const entrySid = event.entry?.sessionId || sid();
    if (event.type === "work-start") sideEvent(model, { type: "work-start", sid: entrySid }, sidePanelPrefs());
    else if (event.type === "failure") sideEvent(model, { type: "failure", sid: entrySid, key: `bg:${event.entry?.el?.dataset?.bgId ?? ""}`, hold: true });
    if (event.type === "sections") {
      // 已完成段的展开态变了(用户点了段头):委派卡要按新的折叠态重画,其余只更新段头。
      if (lastDoneOpen !== bgDoneOpen) renderTasksList();
      else renderChrome();
    }
    reconcileTasksPanel();
  });
  onLayoutChange((section) => {
    syncSettingsControls();
    if (section === "*" || section === "side_panel") reconcileTasksPanel();
  });
}

// D-278:面板头部就绪状态行。只在 fast 模型**未就绪**时出现(就绪时不占一行);
// 查询失败(命令不可用/旧引擎)时保持隐藏,不遮挡面板其余内容。
export async function refreshAgentPanelStatus() {
  const line = $("agent-panel-status");
  if (!line) return;
  let status;
  try {
    status = await invoke("fast_model_status");
  } catch {
    line.classList.add("hidden");
    return;
  }
  const st = fastStatusText(status);
  line.textContent = st.text;
  line.classList.toggle("hidden", !st.warn);
  line.classList.toggle("warn-text", st.warn);
}

// ---------- 设置页「后台任务侧栏」两个开关(偏好经 ui_prefs 的 ui_layout.side_panel 持久化) ----------
function syncSettingsControls() {
  const prefs = sidePanelPrefs();
  const autoOpen = $("set-side-auto-open");
  if (autoOpen) autoOpen.checked = prefs.autoOpen;
  const autoClose = $("set-side-auto-close");
  if (autoClose) autoClose.checked = prefs.autoClose;
}

function setInteract(source, value) {
  interact[source] = value;
  const any = interact.pointer || interact.focus;
  if (any === interact.on) return;
  interact.on = any;
  sideEvent(model, { type: "interact", sid: sid(), on: any, now: clock() });
  reconcileTasksPanel();
}

defer(() => {
  subscribe();
  const panel = $("tasks-panel");
  if (panel) panel.dataset.mode = "list";
  $("tasks-toggle")?.addEventListener("click", (event) => toggleTasksPanel(event?.currentTarget ?? $("tasks-toggle")));
  $("tasks-close")?.addEventListener("click", () => closeTasksPanel({ returnFocus: true }));
  $("tasks-scrim")?.addEventListener("click", () => closeTasksPanel({ returnFocus: true }));
  $("tasks-ack")?.addEventListener("click", () => {
    ackLine(sid());
    reconcileTasksPanel();
  });
  $("tasks-wide")?.addEventListener("click", () => {
    if (!splitApi) return;
    const mainW = mainWidth();
    if (panelWidth(mainW) >= sideMaxWidth(mainW) - 1) splitApi.reset();
    else splitApi.set(sideMaxWidth(mainW));
  });
  $("agent-back")?.addEventListener("click", () => {
    showList();
    $("tasks-close")?.focus?.();
  });
  // Esc 挂在侧栏自身(不挂 document/window,ui_surface_stack §5):详情回列表,列表关侧栏(算用户关闭)并还焦点。
  panel?.addEventListener("keydown", (event) => {
    if (event.key !== "Escape" || event.isComposing) return;
    event.preventDefault?.();
    event.stopPropagation?.();
    if (panelMode === "detail") {
      showList();
      $("tasks-close")?.focus?.();
      return;
    }
    closeTasksPanel({ returnFocus: true });
  });
  // 悬停/焦点在侧栏内时暂停自动收起,离开后重新计时。
  panel?.addEventListener("pointerenter", () => setInteract("pointer", true));
  panel?.addEventListener("pointerleave", () => setInteract("pointer", false));
  panel?.addEventListener("focusin", () => setInteract("focus", true));
  panel?.addEventListener("focusout", (event) => {
    if (!isInside(event.relatedTarget, panel)) setInteract("focus", false);
  });
  $("agent-audit-trace")?.addEventListener("click", () => {
    // 运行轨迹 = 同一个侧栏里的终端条目:回到列表、展开已完成段并滚过去。
    showList();
    setBgDoneOpen(true);
    try { localStorage.setItem("kz-bg-done-open", "1"); } catch { /* 忽略 */ }
    renderTasksList();
    $("bg-section-running")?.scrollIntoView?.({ block: "start" });
  });
  for (const [id, key] of [["set-side-auto-open", "auto_open"], ["set-side-auto-close", "auto_close"]]) {
    $(id)?.addEventListener("change", (event) => setSidePanelPref(key, Boolean(event?.target?.checked ?? $(id).checked)));
  }
  syncSettingsControls();
  // 左缘分隔条:宽度夹在 [320, min(760, 主区 − 600)],偏好经 ui_layout.splits.tasks 持久化;⤢ 在默认宽与上限之间切换。
  splitApi = installSplit(panel, {
    id: "tasks",
    side: "left",
    min: SIDE_WIDTH_MIN,
    max: () => sideMaxWidth(mainWidth()),
    title: t("拖动调整面板宽度"),
    ariaLabel: t("调整后台任务侧栏宽度"),
    onChange: () => reconcileTasksPanel(),
  });
  // 主区尺寸变化(窗口缩放、左侧栏开合)重算停靠/抽屉;按帧合并。
  const main = $("main");
  if (main && typeof ResizeObserver === "function") {
    new ResizeObserver((entries) => {
      const width = entries?.[0]?.contentRect?.width;
      if (width > 0) mainWidthCache = width;
      queueReconcile();
    }).observe(main);
  }
  window.addEventListener("resize", () => {
    mainWidthCache = 0;
    queueReconcile();
  });
  // D-278:一键就绪进度事件也同步刷新面板状态行(面板开着时在设置页操作,回到面板即最新)。
  on("kz:fast-setup", (event) => {
    if (!$("tasks-panel")?.classList.contains("hidden")) void refreshAgentPanelStatus();
    const text = event.payload?.text;
    if (text) {
      $("fast-status").textContent = text;
      log(`${t("子代理安装")}:${text}`);
    }
  });
  reconcileTasksPanel();
});
