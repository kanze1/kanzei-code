import { $, invoke, defer, confirmDialog } from "./01-core.js";
import { openMenu } from "./00-surface.js";
import { t, localizedDocStatus } from "./02-i18n.js";
import { currentProject, onInnerBack, toastError } from "./03-shell.js";
import { layoutPref, setLayoutPref } from "./03-layout.js";
import { renderWorkDocument } from "./11-docs-list.js";
import { renderRequirementDocument } from "./26-requirement-contract.js";
import { needsRequirementResearch } from "./26-requirement-research.js";
import { renderTrackerFields } from "./04-structured.js";
import { deliveryState, loadDeliveredFiles, onDeliveriesChanged } from "./06-deliveries.js";
import { renderFileCard } from "./06-activity.js";
import { batchCells, dependencyLayers, runtimeSummary } from "./30-workspace-model.js";

const projects = new Map();
let mounted = null;
const el = (tag, text, cls) => { const node = document.createElement(tag); if (text != null) node.textContent = text; if (cls) node.className = cls; return node; };
const action = (label, callback, cls = "ghost") => { const button = el("button", label, cls); button.type = "button"; button.addEventListener("click", callback); return button; };
const allEntries = state => [...(state.docs?.requirements || []), ...(state.docs?.defects || [])];
function stateFor(project) {
  if (!projects.has(project)) projects.set(project, { project, tab: layoutPref("management", project) || "req", query: "", filter: "open", detail: null, details: new Map(), docs: null, snapshot: null, error: "", root: null, signature: "", arch: null, stats: null, mapFocus: null, period: "7", session: "" });
  return projects.get(project);
}
function live(state) { return mounted === state && state.root?.isConnected && !state.root.hidden; }

export function mountManagement(project, docs, snapshot, error = "") {
  const state = stateFor(project), host = $("project-overview-content");
  mounted = state;
  if (!state.root) {
    state.root = el("section", null, "management-page"); state.root.id = "management-page";
    state.root.setAttribute("aria-label", t("项目管理"));
    state.nav = el("nav", null, "management-tabs"); state.nav.setAttribute("aria-label", t("管理页面"));
    for (const [key, label] of [["req", "需求"], ["defect", "缺陷"], ["deliveries", "交付"], ["map", "项目地图"]]) {
      const button = action(t(label), () => { state.tab = key; state.detail = null; setLayoutPref("management", project, key); render(state, true); });
      button.dataset.managementTab = key; button.dataset.i18nKey = label; state.nav.append(button);
    }
    state.body = el("div", null, "management-body"); state.root.append(state.nav, state.body);
  }
  for (const root of host.querySelectorAll(".management-page")) if (root !== state.root) root.remove();
  if (state.root.parentElement !== host) host.append(state.root);
  state.root.hidden = false;
  if (docs) state.docs = docs;
  state.snapshot = snapshot; state.error = error;
  render(state);
}

export function hideManagement() { if (mounted?.root) mounted.root.hidden = true; }

async function refresh(state) {
  if (state.loading) return;
  state.loading = true;
  try {
    state.docs = await invoke("docs_snapshot", { projectDir: state.project }); state.error = "";
    if (state.tab === "map") { state.arch = null; state.stats = null; }
    if (state.tab === "deliveries") await loadDeliveredFiles(state.project, { force: true });
  } catch (error) { state.error = String(error); }
  finally { state.loading = false; if (live(state)) render(state, true); }
}

export async function openManagementItem(project, entry, { activate = false } = {}) {
  const { openProjectSpace } = await import("./12-workbench.js");
  document.dispatchEvent(new CustomEvent("kz:work-surface-switch", { detail: { view: "project" } }));
  // Browsing a document does not change the execution recipient. Editing an
  // inactive project's document explicitly activates its existing editor.
  if (!await openProjectSpace(project, "project", { activate })) return;
  const state = stateFor(project); state.detail = entry; state.tab = entry.id.startsWith("D-") ? "defect" : "req";
  if (live(state)) render(state, true);
  try {
    const docs = await invoke("docs_snapshot", { projectDir: project });
    if (state.detail !== entry) return;
    state.docs = docs;
    if (live(state)) render(state, true);
  } catch (error) { if (live(state)) toastError(String(error)); }
}

function render(state, force = false) {
  if (!live(state)) return;
  for (const button of state.nav.children) button.setAttribute("aria-current", String(button.dataset.managementTab === state.tab ? "page" : "false"));
  const signature = JSON.stringify([state.tab, state.detail, state.docs, state.error]);
  if (!force && signature === state.signature) return;
  // A background snapshot must never replace an editor containing a user's draft.
  if (state.body.querySelector(".editing") && !force) return;
  // Refresh rows around a focused search/filter without replacing its input.
  if (!force && state.fillList && state.body.querySelector(".management-tools")?.contains(document.activeElement)) {
    state.fillList(); return;
  }
  state.signature = signature; state.body.replaceChildren();
  if (state.detail) { renderDetail(state); return; }
  if (state.tab === "map") { renderMapPage(state); return; }
  if (state.tab === "deliveries") { renderDeliveries(state); return; }
  renderList(state);
}

function renderDetail(state) {
  const entry = allEntries(state).find(row => row.id === state.detail.id) || state.detail;
  const back = action("← " + t(state.tab === "defect" ? "缺陷" : "需求"), () => { state.detail = null; render(state, true); });
  const head = el("div", null, "management-detail-nav"); head.append(back, el("span", localizedDocStatus(entry.status), "dim")); state.body.append(head);
  if (!entry.closed && !entry.blocked && entry.status !== "awaiting_external" && !needsRequirementResearch(entry)) {
    head.append(action(t("继续此需求"), async () => {
      const { openProjectSpace } = await import("./12-workbench.js");
      if (await openProjectSpace(state.project, "chat")) await (await import("./26-project-conversations.js")).continueRequirement(entry);
    }));
  }
  if (currentProject === state.project) {
    const cached = state.details.get(entry.id);
    const detail = cached?.classList.contains("editing") ? cached : renderWorkDocument(entry, state.tab);
    state.details.set(entry.id, detail); state.body.append(detail);
  }
  else {
    state.body.append(el("h1", `${entry.id} · ${entry.title}`), state.tab === "req" ? renderRequirementDocument(entry) : renderTrackerFields(entry.fields || []));
    state.body.append(action(t("编辑"), () => void openManagementItem(state.project, entry, { activate: true })));
  }
}

export function progressCells(entry) {
  const cells = batchCells(entry), group = el("span", null, "management-progress");
  group.setAttribute("role", "img");
  group.setAttribute("aria-label", cells.length ? `${t("批次进度")} ${cells.filter(cell => ["done", "superseded"].includes(cell.state)).length}/${cells.length}` : t("尚未拆分批次"));
  group.title = cells.length ? cells.map(cell => `${cell.id}: ${cell.state}`).join(" · ") : t("尚未拆分批次");
  for (const cell of cells) { const square = el("i"); square.dataset.state = cell.state; group.append(square); }
  if (!cells.length) group.append(el("i", null, "unplanned"));
  return group;
}

function renderList(state) {
  const tools = el("div", null, "management-tools");
  const search = el("input"); search.type = "search"; search.placeholder = t("搜索编号或标题"); search.setAttribute("aria-label", search.placeholder); search.value = state.query;
  const filter = el("select"); filter.setAttribute("aria-label", t("状态"));
  for (const [key, label] of [["open", "未完成"], ["doing", "进行中"], ["blocked", "已阻塞"], ["awaiting_external", "待外部验收"], ["all", "全部"]]) {
    const option = el("option", t(label)); option.value = key; filter.append(option);
  }
  filter.value = state.filter;
  const list = el("div", null, "management-list");
  const fill = () => {
    list.replaceChildren();
    const entries = (state.tab === "req" ? state.docs?.requirements : state.docs?.defects) || [];
    const query = state.query.toLocaleLowerCase();
    const rows = entries.filter(entry => `${entry.id} ${entry.title}`.toLocaleLowerCase().includes(query) && (state.filter === "all" || state.filter === "open" ? state.filter === "all" || !entry.closed : state.filter === "blocked" ? entry.blocked : state.filter === "doing" ? ["doing", "fixing"].includes(entry.status) : entry.status === state.filter));
    for (const entry of rows) {
      const row = action("", () => void openManagementItem(state.project, entry), "management-row"); row.dataset.workId = entry.id;
      const identity = el("div", null, "management-identity"); identity.append(el("small", `${entry.id} · ${localizedDocStatus(entry.status)}`, "dim"), el("strong", entry.title));
      row.append(identity, progressCells(entry), el("span", entry.priority || "—", "management-priority"));
      list.append(row);
    }
    if (!rows.length) list.append(el("p", t(state.docs ? "没有符合条件的条目" : "正在读取…"), "management-empty"));
  };
  state.fillList = fill;
  search.addEventListener("input", () => { state.query = search.value; fill(); });
  filter.addEventListener("change", () => { state.filter = filter.value; fill(); });
  const create = action(t(state.tab === "defect" ? "登记缺陷" : "登记需求"), () => document.dispatchEvent(new CustomEvent("kz:open-work-module", { detail: { project: state.project, module: "work", capture: state.tab } })), "primary");
  const more = action("⋯", () => openMenu(more, [{ label: t("高级筛选与批量管理"), onSelect: async () => {
    const kind = state.tab;
    const { openProjectSpace } = await import("./12-workbench.js");
    if (await openProjectSpace(state.project, "documents")) $(`documents-tab-${kind}`)?.click();
  } }]));
  more.setAttribute("aria-label", t("更多"));
  tools.append(search, filter, action(t("刷新"), () => void refresh(state)), create, more);
  state.body.append(tools);
  if (state.error) state.body.append(el("p", `${t("读取失败")}: ${state.error}`, "management-error"));
  state.body.append(list); fill();
  const archived = Number(state.docs?.archived?.[state.tab] || 0);
  if (archived) state.body.append(action(`${t("已归档")} · ${archived}`, async () => {
    try {
      const rows = await invoke("docs_archive_entries", { projectDir: state.project, kind: state.tab });
      if (!live(state)) return;
      const archive = el("section", null, "management-archive"); archive.append(el("h2", t("已归档")));
      for (const entry of rows) archive.append(action(`${entry.id} · ${entry.title}`, () => void openManagementItem(state.project, { ...entry, closed: true }), "management-archive-row"));
      state.body.querySelector(".management-archive")?.remove(); state.body.append(archive);
    } catch (error) { toastError(String(error)); }
  }));
}

function renderDeliveries(state) {
  const deliveries = deliveryState(state.project);
  state.body.append(el("h1", t("交付")), el("p", t("当前项目的交付文件"), "dim"));
  const tools = el("div", null, "management-tools");
  const search = el("input"); search.type = "search"; search.placeholder = t("搜索交付文件"); search.setAttribute("aria-label", search.placeholder); search.value = state.deliveryQuery || "";
  const filter = el("select"); filter.setAttribute("aria-label", t("交付整理"));
  for (const [key, label] of [["active", "当前交付"], ["archived", "已归档"], ["removed", "已移除记录"], ["all", "全部"]]) { const option = el("option", t(label)); option.value = key; filter.append(option); }
  filter.value = state.deliveryFilter || "active";
  const session = el("select"); session.setAttribute("aria-label", t("所属对话")); const all = el("option", t("全部对话")); all.value = ""; session.append(all);
  for (const id of new Set(deliveries.rows.map(row => row.session_id))) {
    const label = state.snapshot?.lines?.find(line => line.session_id === id)?.label || id;
    const option = el("option", label); option.value = id; session.append(option);
  }
  session.value = state.deliverySession || "";
  tools.append(search, filter, session, action(t("刷新"), () => void refresh(state))); state.body.append(tools);
  const list = el("div", null, "management-deliveries");
  const fill = () => {
    list.replaceChildren();
    const query = search.value.toLocaleLowerCase();
    const rows = deliveries.rows.filter(row => `${row.name || ""} ${row.path || ""} ${row.caption || ""}`.toLocaleLowerCase().includes(query)
      && (!session.value || row.session_id === session.value)
      && (filter.value === "all" || filter.value === "removed" ? filter.value === "all" || row.removed : filter.value === "archived" ? row.archived && !row.removed : !row.archived && !row.removed));
    for (const row of rows) {
      const item = el("article", null, "management-delivery"); item.dataset.deliveryId = row.id;
      item.append(renderFileCard(row, { projectDir: state.project }));
      const more = action(t("整理"), () => openMenu(more, [
        { label: t(row.archived || row.removed ? "恢复到当前交付" : "归档"), onSelect: () => void manageDelivery(state, row, row.archived || row.removed ? "restore" : "archive") },
        !row.removed && { label: t("移除记录"), onSelect: () => void manageDelivery(state, row, "remove") },
        row.status === "trashed" ? { label: t("恢复文件"), onSelect: () => void manageDelivery(state, row, "restore_file") }
          : row.status === "available" && { label: t("删除文件"), danger: true, onSelect: () => void manageDelivery(state, row, "trash") },
      ].filter(Boolean)), "ghost mini");
      item.append(more); list.append(item);
    }
    if (!rows.length) list.append(el("p", deliveries.error ? `${t("交付读取失败")}: ${deliveries.error}` : t(deliveries.loaded ? "没有符合条件的交付文件" : "正在读取…"), "management-empty"));
  };
  search.addEventListener("input", () => { state.deliveryQuery = search.value; fill(); });
  filter.addEventListener("change", () => { state.deliveryFilter = filter.value; fill(); });
  session.addEventListener("change", () => { state.deliverySession = session.value; fill(); });
  state.fillList = fill; fill();
  state.body.append(list);
  if (!deliveries.loaded && !deliveries.request) void loadDeliveredFiles(state.project);
}

async function manageDelivery(state, row, operation) {
  if (state.deliveryBusy) return;
  if (operation === "trash" && !await confirmDialog({ title: t("删除文件"), message: row.path,
    list: [t("文件移到项目的交付回收区，可以在已归档中恢复。")], okText: t("删除文件"), danger: true })) return;
  state.deliveryBusy = true;
  try { await invoke("delivery_manage", { projectDir: state.project, id: row.id, action: operation }); await loadDeliveredFiles(state.project, { force: true }); }
  catch (error) { toastError(String(error)); }
  finally { state.deliveryBusy = false; if (live(state)) render(state, true); }
}

function renderMapPage(state) {
  const head = el("div", null, "management-map-head"); head.append(el("h1", t("项目地图")), action(t("让模型生成项目地图"), async () => {
    const { openProjectSpace } = await import("./12-workbench.js");
    if (!await openProjectSpace(state.project, "chat")) return;
    const prompt = $("prompt");
    const text = t("请调查当前项目并生成项目地图：说明主要模块、入口、依赖关系、数据流与运行方式，结合实际代码给出 Mermaid 图和文件链接，并保存为项目文档。已有地图时请更新。先阅读项目内的 AGENTS.md 和相关 Skills。");
    prompt.value = [prompt.value.trimEnd(), text].filter(Boolean).join("\n\n"); prompt.dispatchEvent(new Event("input", { bubbles: true })); prompt.focus();
  }), action(t("刷新"), () => void refresh(state))); state.body.append(head);
  const graph = el("section", null, "management-graph"); graph.setAttribute("aria-label", t("项目依赖图")); state.body.append(graph);
  if (state.arch) drawGraph(graph, state); else graph.append(el("p", t("正在读取依赖…"), "dim"));
  const runtime = el("section", null, "management-runtime"); state.body.append(runtime); renderRuntime(runtime, state);
  if (!state.mapRequest && (!state.arch || !state.stats)) {
    state.mapRequest = Promise.allSettled([
      invoke("architecture_snapshot", { projectDir: state.project }),
      invoke("run_metrics", { projectDir: state.project, limit: 200 }),
      invoke("memory_recalls", { projectDir: state.project, limit: 200 }),
    ]).then(([arch, stats, memory]) => {
      state.arch = arch.status === "fulfilled" ? arch.value : { error: String(arch.reason) };
      state.stats = stats.status === "fulfilled" ? stats.value : { error: String(stats.reason), rounds: [] };
      state.memory = memory.status === "fulfilled" ? memory.value : { error: String(memory.reason), rounds: [] };
      if (live(state) && state.tab === "map") { graph.replaceChildren(); drawGraph(graph, state); renderRuntime(runtime, state); }
    }).finally(() => { state.mapRequest = null; });
  }
}

function drawGraph(host, state) {
  const members = state.arch.crates?.members || [], edges = state.arch.crates?.edges || [];
  if (!members.length) { host.append(el("p", state.arch.error ? `${t("依赖读取失败")}: ${state.arch.error}` : t("此项目尚无可读取的模块依赖清单"), "management-empty")); return; }
  const tools = el("div", null, "management-map-tools");
  tools.append(el("span", t(state.mapFocus ? "直接关系 · 左侧调用者，右侧依赖" : "依赖主干 · 点击模块查看全部直接关系"), "dim"));
  if (state.mapFocus) tools.append(action(t("返回主干"), () => { state.mapFocus = null; host.replaceChildren(); drawGraph(host, state); }));
  host.append(tools);
  const { layers, edges: reduced, cyclic } = dependencyLayers(members, edges);
  const byName = new Map(members.map(member => [member.name, member]));
  const focus = state.mapFocus && byName.has(state.mapFocus) ? state.mapFocus : null;
  const shown = focus ? [edges.filter(edge => edge.to === focus).map(edge => edge.from), [focus], edges.filter(edge => edge.from === focus).map(edge => edge.to)].map(layer => [...new Set(layer)].filter(name => byName.has(name))) : layers;
  const canvas = el("div", null, "management-map-canvas"), positions = new Map();
  const width = Math.max(760, shown.length * 248), height = Math.max(190, Math.max(...shown.map(layer => layer.length)) * 98 + 24);
  canvas.style.width = `${width}px`; canvas.style.height = `${height}px`;
  shown.forEach((layer, column) => layer.forEach((name, index) => {
    const x = column * 248 + 16, y = (height - layer.length * 98) / 2 + index * 98;
    positions.set(focus ? `${column}:${name}` : name, { x, y });
    const member = byName.get(name), incoming = edges.filter(edge => edge.to === name).length;
    const card = action("", () => { state.mapFocus = name; host.replaceChildren(); drawGraph(host, state); }, "management-map-node");
    card.style.left = `${x}px`; card.style.top = `${y}px`; card.dataset.focused = String(name === focus);
    card.append(el("strong", name), el("small", member.description || member.group || member.dir || "", "dim"));
    if (incoming > 1) card.append(el("span", `${t("被依赖")} ${incoming}`, "management-shared"));
    card.title = member.entry || name; canvas.append(card);
  }));
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg"); svg.setAttribute("width", width); svg.setAttribute("height", height); svg.setAttribute("aria-hidden", "true");
  const seen = new Set();
  const links = focus ? edges.filter(edge => edge.from === focus || edge.to === focus) : reduced.filter(edge => { if (seen.has(edge.to)) return false; seen.add(edge.to); return true; });
  for (const edge of links) {
    const from = positions.get(focus ? `${edge.from === focus ? 1 : 0}:${edge.from}` : edge.from);
    const to = positions.get(focus ? `${edge.to === focus ? 1 : 2}:${edge.to}` : edge.to);
    if (!from || !to || from.x >= to.x) continue;
    const start = from.x + 212, end = to.x, middle = (start + end) / 2, path = document.createElementNS(svg.namespaceURI, "path");
    path.setAttribute("d", `M ${start} ${from.y + 36} H ${middle} V ${to.y + 36} H ${end}`); svg.append(path);
  }
  canvas.prepend(svg); const scroll = el("div", null, "management-map-scroll"); scroll.tabIndex = 0; scroll.append(canvas); host.append(scroll);
  if (cyclic.length && !focus) host.append(el("p", t("存在循环依赖；同层显示，点击模块查看直接关系。"), "dim"));
  if (!focus) host.append(el("small", t("共享依赖在主干中只连一次；点击模块可查看普通、构建和开发依赖。"), "dim"));
}

function renderRuntime(host, state) {
  host.replaceChildren(); const head = el("div", null, "management-runtime-head"); head.append(el("h2", t("运行数据")));
  const period = el("select"); period.setAttribute("aria-label", t("统计时间"));
  for (const [value, label] of [["1", "最近 24 小时"], ["7", "最近 7 天"], ["0", "最近样本"]]) { const option = el("option", t(label)); option.value = value; period.append(option); }
  period.value = state.period;
  const session = el("select"); session.setAttribute("aria-label", t("统计对话")); session.append(el("option", t("全部对话"))); session.firstChild.value = "";
  const sessions = new Map((state.snapshot?.lines || []).map(line => [line.session_id, line.label || line.name || t("对话")]));
  for (const row of state.stats?.rounds || []) if (!sessions.has(row.sessionId)) sessions.set(row.sessionId, `${t("历史对话")} ${sessions.size + 1}`);
  for (const [value, label] of sessions) { if (!value) continue; const option = el("option", label); option.value = value; session.append(option); }
  session.value = state.session;
  period.addEventListener("change", () => { state.period = period.value; renderRuntime(host, state); });
  session.addEventListener("change", () => { state.session = session.value; renderRuntime(host, state); }); head.append(period, session); host.append(head);
  if (!state.stats) { host.append(el("p", t("正在读取运行数据…"), "dim")); return; }
  const summary = runtimeSummary(state.stats.rounds || [], state.memory?.rounds || [], { since: Number(state.period) ? Date.now() - Number(state.period) * 86400000 : 0, session: state.session });
  const duration = summary.meanDuration == null ? "—" : summary.meanDuration < 60000 ? `${(summary.meanDuration / 1000).toFixed(1)} s` : `${(summary.meanDuration / 60000).toFixed(1)} min`;
  const values = [["平均轮次时长", duration, `${summary.durationSamples} ${t("轮已观测")}`], ["工具调用", summary.tools, `${summary.measured} ${t("轮已观测")}`], ["失败调用", summary.failures, t("不含预期拒绝和用户停止")], ["记忆召回", summary.recalls, t("项目记忆条目次数")], ["记忆注入", summary.injected, `${summary.memorySamples} ${t("轮已观测")}`], ["记忆读取", summary.read, `${summary.readSamples} ${t("轮有读取观测")}`]];
  const elapsed = value => value == null ? "—" : value < 60000 ? `${(value / 1000).toFixed(1)} s` : `${(value / 60000).toFixed(1)} min`;
  values.unshift(["完成轮次", summary.rounds, t("当前筛选范围")], ["输入 Token", summary.inputTokens, `${summary.tokenSamples} ${t("轮已观测")}`], ["输出 Token", summary.outputTokens, `${summary.tokenSamples} ${t("轮已观测")}`], ["执行步数", summary.steps, t("已记录的执行步数")], ["累计执行时长", elapsed(summary.totalDuration), `${summary.durationSamples} ${t("轮已观测")}`], ["P95 轮次时长", elapsed(summary.p95Duration), `${summary.durationSamples} ${t("轮已观测")}`], ["预期拒绝", summary.rejected, t("工具主动拒绝的调用")]);
  const metrics = el("dl", null, "management-metrics");
  for (const [label, value, note] of values) { const metric = el("div"); metric.append(el("dt", t(label)), el("dd", value ?? "—"), el("small", note, "dim")); metrics.append(metric); }
  host.append(metrics, el("p", `${summary.rounds} ${t("轮符合筛选；从最近最多 200 轮已完成记录和 200 次记忆观测中统计。未记录显示 —。")}`, "dim"));
  const outcomes = el("div", null, "runtime-outcomes");
  for (const [name, count] of summary.outcomes) outcomes.append(el("span", `${name} · ${count}`));
  host.append(outcomes);
  if (summary.toolNames.length) {
    const tools = el("details", null, "runtime-tools"); tools.append(el("summary", t("工具使用分布")));
    const distribution = el("dl", null, "runtime-tool-distribution");
    const maximum = Math.max(...summary.toolNames.map(([, count]) => count), 1);
    for (const [name, count] of summary.toolNames) {
      const row = el("div"), meter = el("meter"); meter.min = 0; meter.max = maximum; meter.value = count; meter.setAttribute("aria-label", name);
      row.append(el("dt", name), meter, el("dd", count)); distribution.append(row);
    }
    tools.append(distribution); host.append(tools);
  }
  const details = el("details", null, "runtime-rounds"); details.append(el("summary", t("逐轮运行记录")));
  const scroll = el("div", null, "runtime-table-scroll"), table = el("table"), headRow = el("tr");
  for (const label of ["时间", "所属对话", "结果", "时长", "输入 Token", "输出 Token", "工具调用", "执行步数"]) headRow.append(el("th", t(label)));
  const thead = el("thead"); thead.append(headRow); table.append(thead); const body = el("tbody");
  for (const row of summary.rows) {
    const tr = el("tr"); tr.title = row.prompt || "";
    for (const value of [new Date(row.at).toLocaleString(), sessions.get(row.sessionId) || row.sessionId || "—", row.outcome || "—", elapsed(row.durationMs), row.inputTokens ?? "—", row.outputTokens ?? "—", row.measured ? row.metrics?.total_calls ?? "—" : "—", row.steps ?? "—"]) tr.append(el("td", value));
    body.append(tr);
    const context = el("tr", null, "runtime-round-context"); const cell = el("td"); cell.colSpan = 8;
    const more = el("details"); more.append(el("summary", row.prompt || t("轮次详情")), el("pre", JSON.stringify({ tools: row.tools, metrics: row.metrics, context: row.context }, null, 2))); cell.append(more); context.append(cell); body.append(context);
  }
  table.append(body); scroll.append(table); details.append(scroll); host.append(details);
  if (state.stats.error || state.memory?.error) host.append(el("p", `${t("部分数据读取失败")}: ${state.stats.error || state.memory.error}`, "management-error"));
}

defer(() => {
  onInnerBack(() => {
    if (document.body.dataset.view !== "project" || !mounted?.detail || !live(mounted)) return false;
    mounted.detail = null; render(mounted, true); return true;
  });
  document.addEventListener("kz:management-snapshot", event => {
    const state = projects.get(event.detail.project);
    if (state) { state.docs = event.detail.snapshot; render(state); }
  });
  onDeliveriesChanged(project => { const state = projects.get(project); if (state && live(state) && state.tab === "deliveries") render(state, true); });
  document.addEventListener("kz:management-refresh", event => { const state = projects.get(event.detail?.project || currentProject); if (state) void refresh(state); });
});
