// UI-0926 #8 子代理侧栏(docs/design/subagent_presentation.md §5.6):由原「子代理面板」改造的全高
// 右侧抽屉,与活动面板互斥。两种模式(#agent-panel[data-mode]):
//   list   当前线路的全部子代理,按批次分组、新批次在上;点行进详情,⌖ 定位回对话;底部是运行审计摘要。
//   detail 一次委派的完整版:字形 · 身份 · 描述 · 状态词 / 模型 · 计数 / 停止 · 复制结果 · 定位,
//          正文与卡片内联展开同一个渲染器(指令 / 过程 / 结果),不设高度上限。
// 数据只读 05-subagents.js 的模型;旧面板的「运行中/已完成/已关闭」三段与「关闭/删除/清空」按设计取消
// (它们只改本地视图、不碰后端,用户还得理解「关闭≠停止≠删除」)。
import { $, agentRoleAccent, defer, invoke, motionSync, on } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeSessionId, log, setActivityPanelOpen, syncActivityPanel } from "./03-shell.js";
import {
  SA_ACTIVE,
  buildSubagentRow,
  createSubagentView,
  onSubagentChange,
  onSubagentTick,
  renderSubagentTimeline,
  subagentAgentName,
  subagentByKey,
  subagentCopyResult,
  subagentCountLabel,
  subagentGlyph,
  subagentLocate,
  subagentMetaText,
  subagentRunningCount,
  subagentRunsFor,
  subagentStateWord,
  subagentStop,
  updateSubagentRow,
  updateSubagentRowMeta,
} from "./05-subagents.js";
import { fastStatusText, orchPhaseLabel } from "./06-activity.js";

export let agentPanelOpen = false;
let panelMode = "list";
let detailRun = null;
let detailView = null;
let detailUi = null;
let lastInvoker = null;
const rowEls = new Map(); // run -> 行节点(列表模式)

function setPanelVisible(open) {
  agentPanelOpen = open;
  const host = $("agent-panel");
  host?.classList.toggle("hidden", !open);
  const toggle = $("agent-toggle");
  if (!toggle) return;
  toggle.classList.toggle("active", open);
  toggle.setAttribute("aria-expanded", String(open));
  toggle.title = open ? t("收起子代理面板") : t("打开子代理面板");
}

/// 打开抽屉外壳。与活动面板互斥:一个开着时另一个收起,右侧不叠两栏。
function openPanelShell(invoker) {
  if (invoker) lastInvoker = invoker;
  if (agentPanelOpen) return;
  setActivityPanelOpen(false);
  localStorage.setItem("kz-activity-panel", "0");
  syncActivityPanel();
  setPanelVisible(true);
  refreshAgentPanelStatus(); // D-278:每次打开都刷新就绪状态
}

// rail 开关:开着就收起,收着就打开列表。
export function agentTogglePanel() {
  if (agentPanelOpen) {
    agentClosePanel();
    return;
  }
  openPanelShell($("agent-toggle"));
  showList();
}

// D-350:面板头部 ✕ 关闭。只关子代理面板,活动面板回到 activityPanelOpen 决定的状态
// (打开子代理时已清零,所以不会误弹)。
export function agentClosePanel() {
  setPanelVisible(false);
  syncActivityPanel();
  detailView = null;
}

/// 打开侧栏:给 key 进该次委派的详情,否则进列表。卡片 ↗、组头「查看全部」、内联展开区都走这里。
export function openSubagentPanel(key = null, invoker = null) {
  openPanelShell(invoker);
  const run = key ? subagentByKey(key) : null;
  if (run) showDetail(run);
  else showList();
}

function setMode(mode) {
  panelMode = mode;
  const host = $("agent-panel");
  if (host) host.dataset.mode = mode;
  $("agent-back")?.classList.toggle("hidden", mode !== "detail");
  $("agent-list")?.classList.toggle("hidden", mode !== "list");
  $("agent-detail")?.classList.toggle("hidden", mode !== "detail");
}

function showList() {
  detailRun = null;
  detailView = null;
  detailUi = null;
  $("agent-detail")?.replaceChildren();
  setMode("list");
  renderAgentList();
  syncAgentBadge();
}

function showDetail(run) {
  detailRun = run;
  setMode("detail");
  buildDetail(run);
  syncAgentBadge();
  // 进入详情时焦点落在「‹ 返回」(§8)。
  $("agent-back")?.focus();
}

export function renderAgentList() {
  const list = $("agent-list");
  if (!list) return;
  rowEls.clear();
  const runs = subagentRunsFor(activeSessionId);
  if (!runs.length) {
    const empty = document.createElement("div");
    empty.className = "sa-panel-empty";
    empty.textContent = t("当前线路还没有子代理");
    list.replaceChildren(empty);
    return;
  }
  const batches = new Map();
  for (const run of runs) {
    if (!batches.has(run.batch)) batches.set(run.batch, []);
    batches.get(run.batch).push(run);
  }
  const sections = [];
  for (const [batch, items] of [...batches.entries()].sort((a, b) => b[0] - a[0])) {
    const section = document.createElement("section");
    section.className = "sa-panel-batch";
    section.dataset.batch = String(batch);
    const title = document.createElement("div");
    title.className = "sa-panel-batch-title";
    const phase = items[0].phase;
    title.textContent = `${phase ? `${orchPhaseLabel(phase)} · ` : ""}${subagentCountLabel(items.length)}`;
    section.appendChild(title);
    for (const run of items) {
      const row = buildSubagentRow(run, {
        onOpen: (target, anchor) => openSubagentPanel(target.key, anchor),
        onLocate: (target) => subagentLocate(target),
      });
      rowEls.set(run, row);
      section.appendChild(row);
    }
    sections.push(section);
  }
  list.replaceChildren(...sections);
}

function detailButton(key, handler) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "ghost mini";
  button.textContent = t(key);
  button.addEventListener("click", handler);
  return button;
}

function buildDetail(run) {
  const host = $("agent-detail");
  if (!host) return;
  host.replaceChildren();
  const head = document.createElement("div");
  head.className = "sa-detail-head";
  const glyph = document.createElement("span");
  glyph.className = "kz-glyph sa-glyph";
  glyph.setAttribute("aria-hidden", "true");
  const agent = document.createElement("span");
  const desc = document.createElement("span");
  desc.className = "sa-desc";
  const word = document.createElement("span");
  word.className = "sa-detail-state";
  head.append(glyph, agent, desc, word);
  const meta = document.createElement("div");
  meta.className = "sa-detail-meta";
  const actions = document.createElement("div");
  actions.className = "sa-detail-actions";
  const stop = detailButton("停止", () => subagentStop(run));
  stop.classList.add("sa-detail-stop");
  const copy = detailButton("复制结果", () => subagentCopyResult(run));
  const locate = detailButton("定位到对话", () => subagentLocate(run));
  locate.classList.add("sa-detail-locate");
  actions.append(stop, copy, locate);
  const body = document.createElement("div");
  body.className = "sa-detail-body";
  host.append(head, meta, actions, body);
  detailUi = { glyph, agent, desc, word, meta, stop };
  detailView = createSubagentView(body, run, { inline: false });
  renderDetail(run);
}

function renderDetailMeta(run) {
  if (!detailUi) return;
  const model = run.model || run.tier;
  const text = [model, subagentMetaText(run)].filter(Boolean).join(" · ");
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
  detailUi.agent.className = `sa-agent line-accent-${agentRoleAccent(label)}${label ? "" : " hidden"}`;
  detailUi.desc.textContent = run.description || t("子代理");
  detailUi.desc.title = run.description;
  detailUi.word.textContent = subagentStateWord(run.state) || t("运行中");
  detailUi.stop.classList.toggle("hidden", !SA_ACTIVE.has(run.state) || run.state === "stopping");
  const host = $("agent-detail");
  if (host) host.dataset.saState = run.state;
  renderDetailMeta(run);
  renderSubagentTimeline(detailView);
}

/// rail 开关右上角的运行数徽标 + 头部「N 运行中」。data-running 仍是布尔(#7 的 rail 点与冒烟共用)。
export function syncAgentBadge() {
  const count = subagentRunningCount(activeSessionId);
  const toggle = $("agent-toggle");
  if (toggle) toggle.dataset.running = String(count > 0);
  const badge = $("agent-badge");
  if (badge) {
    const text = count > 0 ? String(count) : "";
    if (badge.textContent !== text) badge.textContent = text;
    badge.classList.toggle("hidden", count === 0);
  }
  const head = $("agent-running-count");
  if (head) head.textContent = count ? `${count} ${t("运行中")}` : "";
}

/// 切换线路:列表换成新线路的数据;详情不属于新线路时回到列表;徽标重算。
export function agentPanelSync() {
  if (detailRun && detailRun.sessionId !== (activeSessionId || "")) {
    if (agentPanelOpen) showList();
    else {
      detailRun = null;
      detailView = null;
      detailUi = null;
      setMode("list");
    }
  } else if (agentPanelOpen && panelMode === "list") {
    renderAgentList();
  }
  syncAgentBadge();
}

onSubagentChange((run) => {
  syncAgentBadge();
  if (!agentPanelOpen) return;
  if (panelMode === "detail") {
    if (!detailRun) return;
    // 该会话的历史被重载(run 已被丢弃):详情回到列表。
    if (run === null && !subagentRunsFor(detailRun.sessionId).includes(detailRun)) showList();
    else if (run === null || run === detailRun) renderDetail(detailRun);
    return;
  }
  if (run && run.sessionId !== (activeSessionId || "")) return;
  const row = run ? rowEls.get(run) : null;
  if (row) updateSubagentRow(row, run);
  else renderAgentList();
});

onSubagentTick(() => {
  if (!agentPanelOpen) return;
  if (panelMode === "detail") {
    if (detailRun && SA_ACTIVE.has(detailRun.state)) renderDetailMeta(detailRun);
    return;
  }
  // 只刷计数文本;可访问名只随状态变化重写(updateSubagentRow),不每秒重写。
  const now = Date.now();
  for (const [run, row] of rowEls) if (SA_ACTIVE.has(run.state)) updateSubagentRowMeta(row, run, now);
});

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

defer(() => {
  const panel = $("agent-panel");
  if (panel) panel.dataset.mode = "list";
  $("agent-toggle")?.addEventListener("click", agentTogglePanel);
  $("agent-close")?.addEventListener("click", agentClosePanel);
  $("agent-back")?.addEventListener("click", () => {
    showList();
    $("agent-close")?.focus();
  });
  // Esc 挂在面板自身(不挂 document/window,ui_surface_stack §5):详情回列表,列表关面板并还焦点。
  panel?.addEventListener("keydown", (event) => {
    if (event.key !== "Escape" || event.isComposing) return;
    event.preventDefault?.();
    event.stopPropagation?.();
    if (panelMode === "detail") {
      showList();
      $("agent-close")?.focus();
      return;
    }
    agentClosePanel();
    (lastInvoker ?? $("agent-toggle"))?.focus?.();
  });
  $("agent-audit-trace")?.addEventListener("click", () => {
    agentClosePanel();
    setActivityPanelOpen(true);
    localStorage.setItem("kz-activity-panel", "1");
    syncActivityPanel();
  });
  // D-278:一键就绪进度事件也同步刷新面板状态行(面板开着时在设置页操作,回到面板即最新)。
  on("kz:fast-setup", (event) => {
    if (agentPanelOpen) refreshAgentPanelStatus();
    const text = event.payload?.text;
    if (text) {
      $("fast-status").textContent = text;
      log(`${t("子代理安装")}:${text}`);
    }
  });
  syncAgentBadge();
});
