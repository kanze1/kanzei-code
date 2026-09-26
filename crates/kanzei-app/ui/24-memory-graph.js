// 记忆页「列表 | 图谱」:记忆知识图谱接入(docs/design/memory_knowledge_graph.md §9)。
//
// - 数据:invoke("memory_graph") 取后端投影(Markdown 真源的只读投影,后端 stat 缓存);前端再缓存 15 秒,
//   kz:memory-changed 时强制重取。每次 await 之后复查项目与代次:项目 A 的图谱不得画进项目 B。
// - 看什么:记忆页现有的 范围/分类/状态 筛选直接作用在图上(kz:memory-filters),另有区域、图层、含归档、
//   邻域(双击进入 N 跳)与文本视图。检索命中高亮(kz:memory-search-hits),列表里的选中同步(kz:memory-selected)。
// - 点记忆:右侧详情栏显示全文(归档条目只读);点其它节点:详情栏列关系与跳转动作,指纹/主题节点可
//   「用对话合并」(只预填管理对话,不发送)。
// - 画布渲染失败(或环境没有 canvas)就退到文本视图:按 crate → 模块分组的树,给键盘与读屏用。
import { $, defer, invoke, uiPrefsLoad, uiPrefsSave } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, toastError } from "./03-shell.js";
import { jumpToEntry } from "./11-docs-list.js";
import {
  getMemoryEntries,
  memoryFilterSnapshot,
  releaseMemoryDetail,
  setMemoryAreaProvider,
  showMemoryDetail,
  showMemoryTab,
} from "./13-memory.js";
import { openDocViewer } from "./15-views-misc.js";
import { openArchDoc } from "./19-arch.js";
import { canvasSupported, createGraphView } from "./24-graph-view.js";
import {
  DEFAULT_LAYERS,
  KIND_KEYS,
  LAYERS,
  LAYER_KEYS,
  PROVENANCE_KEYS,
  REL_KEYS,
  bandAnchors,
  crateOfArea,
  crateOfNode,
  egoSubgraph,
  groupForTextView,
  nodeRadius,
  nodeStyle,
  visibleGraph,
} from "./24-memory-graph-model.js";

const PAYLOAD_TTL_MS = 15000;
const graphState = {
  view: "list",
  project: null,
  generation: 0,
  payload: null,
  payloadAt: 0,
  payloadProject: null,
  loading: null,
  layers: new Set(DEFAULT_LAYERS),
  archived: false,
  area: "",
  ego: null,
  textView: false,
  graph: null,
  graphPending: null,
  graphFailed: false,
  nodeObjects: new Map(),
  hits: [],
  selectedId: null,
  fitPending: true,
  shown: { nodes: [], links: [] },
  settleMs: null,
};

// 调试与冒烟钩子(scripts/ui-memory-graph-smoke.mjs --browser、ui-preview 的 memory-graph 场景读它)。
const debugHook = {
  ready: false,
  mode: "none",
  nodes: 0,
  links: 0,
  memories: 0,
  settleMs: null,
  hover: (id) => hoverNode(id),
  select: (id) => clickNode(id),
  ego: (id, hops) => enterEgo(id, hops),
};
window.__kzMemoryGraph = debugHook;

const areasById = () => new Map((graphState.payload?.areas ?? []).map((area) => [area.id, area]));
const nodeById = (id) => graphState.payload?.nodes?.find((node) => node.id === id) ?? null;
const areaName = (id) => String(id ?? "").replace(/^area:/, "");

function statusLine(text) {
  const status = $("memory-graph-status");
  if (status) status.textContent = text;
}

function currentFilters() {
  const filters = memoryFilterSnapshot();
  return {
    scope: filters.scope,
    category: filters.category,
    status: filters.status,
    archived: graphState.archived,
    layers: graphState.layers,
    area: graphState.area,
  };
}

// ---------- 视图切换 ----------

export function setMemoryView(view, { persist = true } = {}) {
  const next = view === "graph" ? "graph" : "list";
  graphState.view = next;
  $("memory-view-list")?.setAttribute("aria-pressed", String(next === "list"));
  $("memory-view-graph")?.setAttribute("aria-pressed", String(next === "graph"));
  const scroll = $("memory-scroll");
  if (scroll) scroll.dataset.view = next;
  $("memory-graph-pane")?.classList.toggle("hidden", next !== "graph");
  document.querySelector(".memory-manager-workspace")?.classList.toggle("graph-mode", next === "graph");
  if (persist) {
    void uiPrefsSave({ memory_view: next });
    try {
      localStorage.setItem("kz-memory-view", next);
    } catch {
      /* 存不下只是下次回到列表,不打断当前操作 */
    }
  }
  // 记忆页不在前台时只记下选择,切到记忆页(kz:view-changed)再取数据。
  if (next === "graph" && document.querySelector("#view-memory.active")) void loadAndRender();
  else if (next !== "graph") graphState.graph?.pause();
}

// ---------- 数据 ----------

async function loadPayload(project, { force = false } = {}) {
  const fresh = graphState.payload && graphState.payloadProject === project && Date.now() - graphState.payloadAt < PAYLOAD_TTL_MS;
  if (fresh && !force) return graphState.payload;
  if (graphState.loading?.project === project && !force) return graphState.loading.promise;
  const request = { project };
  request.promise = invoke("memory_graph", { projectDir: project }).finally(() => {
    if (graphState.loading === request) graphState.loading = null;
  });
  graphState.loading = request;
  return request.promise;
}

async function loadAndRender({ force = false } = {}) {
  const project = currentProject;
  const generation = ++graphState.generation;
  if (!project) {
    statusLine(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  if (graphState.payloadProject !== project) statusLine(t("正在构建记忆图谱…"));
  try {
    const payload = await loadPayload(project, { force });
    if (project !== currentProject || generation !== graphState.generation) return; // 竞态守卫:项目 A 的图谱不得画进项目 B
    if (graphState.payloadProject !== project) {
      graphState.nodeObjects.clear();
      graphState.fitPending = true;
    }
    graphState.payload = payload;
    graphState.payloadProject = project;
    graphState.payloadAt = Date.now();
    fillAreaSelect();
    await render();
  } catch (err) {
    if (project !== currentProject || generation !== graphState.generation) return;
    statusLine(`${t("记忆图谱加载失败")}:${err}`);
    toastError(`${t("记忆图谱加载失败")}:${err}`, { retry: () => loadAndRender({ force: true }) });
  }
}

// ---------- 画布 ----------

function nodeLabelFor(node, scale, state) {
  if (state.hovered || state.selected || state.hit || state.lit) return labelText(node, Math.max(scale, 3));
  if (node.kind === "crate") return node.label;
  if (node.kind === "module") return scale >= 0.55 ? node.label : "";
  if (node.kind === "memory") return scale >= 0.85 ? labelText(node, scale) : "";
  return scale >= 1.8 ? labelText(node, scale) : "";
}

function labelText(node, scale) {
  if (node.kind === "memory") return scale >= 2.4 ? `${node.id} ${String(node.title ?? "").slice(0, 14)}` : node.id;
  if (node.kind === "requirement" || node.kind === "defect" || node.kind === "decision") return node.id;
  return node.label || node.id;
}

function labelPriority(node, state) {
  if (state.selected) return 100;
  if (state.hovered) return 90;
  if (state.lit) return 80;
  if (state.hit) return 70;
  if (node.kind === "crate") return 60;
  if (node.kind === "module") return 40;
  if (node.kind === "memory") return 20 + Math.min(10, node.degree ?? 0);
  return 10;
}

function linkStyle(link) {
  const relation = ["refs", "basis", "implements", "supersedes", "derived_from"].includes(link.rel);
  if (link.rel === "about") return { dashed: !(link.provenance === "field" || link.provenance === "path"), arrow: false };
  if (link.rel === "mentions" || link.rel === "cites") return { dashed: true, arrow: false };
  return { dashed: link.strength === "weak", arrow: relation && link.strength === "strong" };
}

async function ensureGraphView() {
  if (graphState.graph) return graphState.graph;
  if (graphState.graphFailed) return null;
  if (!canvasSupported()) {
    graphState.graphFailed = true;
    return null;
  }
  if (!graphState.graphPending) {
    const host = $("memory-graph-canvas");
    const map = () => areasById();
    graphState.graphPending = createGraphView(host, {
      nodeStyle,
      nodeRadius,
      nodeLabel: nodeLabelFor,
      labelPriority,
      linkStyle,
      relLabel: (rel) => t(REL_KEYS[rel] ?? "关联"),
      clusterOf: (node) => (node.kind === "memory" || node.kind === "module" ? crateOfNode(node, map()) : undefined),
      hullGroup: (node) => (node.kind === "memory" || node.kind === "module" || node.kind === "crate" ? crateOfNode(node, map()) : null),
      onHover: (node) => describeHover(node),
      onClick: (node) => void clickNode(node.id),
      onDblClick: (node) => enterEgo(node.id, 2),
      onBackgroundClick: () => {
        graphState.selectedId = null;
        graphState.graph?.select(null);
      },
      onSettled: ({ settleMs }) => {
        graphState.settleMs = settleMs;
        debugHook.settleMs = settleMs;
        debugHook.ready = true;
        // 只在换了一批「看的东西」(新数据、进出邻域、换区域)后适配视图;开关图层/筛选保持用户的缩放。
        if (graphState.fitPending) graphState.graph?.fit(0);
        graphState.fitPending = false;
        statusLine(summaryText());
      },
    })
      .then((view) => {
        graphState.graph = view;
        return view;
      })
      .catch((err) => {
        graphState.graphFailed = true;
        console.warn("[memory-graph] 图形渲染不可用,已退到文本视图", err);
        return null;
      })
      .finally(() => {
        graphState.graphPending = null;
      });
  }
  return graphState.graphPending;
}

function summaryText() {
  const { nodes, links } = graphState.shown;
  const memories = nodes.filter((n) => n.kind === "memory").length;
  const base = `${nodes.length} ${t("个节点")} · ${links.length} ${t("条关系")} · ${memories} ${t("条记忆")}`;
  return graphState.settleMs == null ? base : `${base} · ${t("布局")} ${graphState.settleMs} ms`;
}

function cloneForRender(subgraph) {
  const objects = graphState.nodeObjects;
  const nodes = subgraph.nodes.map((node) => {
    const existing = objects.get(node.id);
    if (existing) {
      Object.assign(existing, node);
      return existing;
    }
    const created = { ...node };
    objects.set(node.id, created);
    return created;
  });
  const links = subgraph.links.map((edge) => ({
    ...edge,
    source: typeof edge.source === "object" ? edge.source.id : edge.source,
    target: typeof edge.target === "object" ? edge.target.id : edge.target,
  }));
  return { nodes, links };
}

async function render() {
  const payload = graphState.payload;
  if (!payload) return;
  const ego = graphState.ego && nodeById(graphState.ego.center) ? graphState.ego : null;
  graphState.ego = ego;
  const subgraph = ego ? egoSubgraph(payload, ego.center, ego.hops, graphState.layers) : visibleGraph(payload, currentFilters());
  graphState.shown = subgraph;
  syncEgoBar();
  syncCanvasLabel(subgraph);
  const project = currentProject;
  const generation = graphState.generation;
  const view = graphState.textView ? null : await ensureGraphView();
  if (project !== currentProject || generation !== graphState.generation) return;
  const useCanvas = Boolean(view) && !graphState.textView;
  $("memory-graph-canvas")?.classList.toggle("hidden", !useCanvas);
  $("memory-graph-list")?.classList.toggle("hidden", useCanvas);
  $("memory-graph-textview")?.setAttribute("aria-pressed", String(!useCanvas));
  renderLegend();
  debugHook.nodes = subgraph.nodes.length;
  debugHook.links = subgraph.links.length;
  debugHook.memories = subgraph.nodes.filter((n) => n.kind === "memory").length;
  if (!useCanvas) {
    debugHook.mode = "text";
    renderTextView(subgraph);
    debugHook.ready = true;
    statusLine(graphState.graphFailed && !graphState.textView ? `${t("图形渲染不可用,已显示文本视图")} · ${summaryText()}` : summaryText());
    return;
  }
  debugHook.mode = "canvas";
  debugHook.ready = false;
  graphState.settleMs = null;
  view.resume();
  const shownCrates = new Set(subgraph.nodes.filter((n) => n.kind === "crate").map((n) => n.id));
  const { anchors, unassigned } = bandAnchors((payload.areas ?? []).filter((area) => shownCrates.has(area.id)));
  view.setData(cloneForRender(subgraph), {
    layout: ego ? "ego" : "clusters",
    anchors,
    unassigned,
    hopOf: subgraph.hopOf,
    center: ego?.center,
  });
  view.select(graphState.selectedId);
  view.highlight(graphState.hits.filter((id) => subgraph.nodes.some((n) => n.id === id)));
  statusLine(`${summaryText()} · ${t("布局中…")}`);
}

function syncCanvasLabel(subgraph) {
  const host = $("memory-graph-canvas");
  if (!host) return;
  host.setAttribute("aria-label", `${t("记忆图谱")}:${subgraph.nodes.length} ${t("个节点")}、${subgraph.links.length} ${t("条关系")}`);
}

// ---------- 悬停 / 点击 / 邻域 ----------

function aboutLine(node) {
  const edges = (graphState.payload?.edges ?? []).filter((edge) => edge.source === node.id && edge.rel === "about");
  if (!edges.length) return t("未归类");
  return edges
    .slice(0, 2)
    .map((edge) => `${areaName(edge.target)}(${t(PROVENANCE_KEYS[edge.provenance] ?? "关键词")}${edge.via ? ` ${edge.via}` : ""})`)
    .join("、");
}

function describeHover(node) {
  if (!node) {
    statusLine(summaryText());
    return;
  }
  const parts = [node.kind === "crate" || node.kind === "module" ? areaName(node.id) : node.id];
  if (node.title && node.title !== node.id && node.kind !== "crate" && node.kind !== "module") parts.push(String(node.title).slice(0, 48));
  if (node.kind === "memory") {
    parts.push(`${node.category ?? ""}/${node.status ?? ""}${node.archived ? ` · ${t("已归档")}` : ""}`);
    parts.push(`${t("关于")} ${aboutLine(node)}`);
  } else {
    parts.push(t(KIND_KEYS[node.kind] ?? "记忆"));
  }
  statusLine(parts.join(" · "));
}

function hoverNode(id) {
  const node = id ? nodeById(id) : null;
  graphState.graph?.hover(node ? node.id : null);
  describeHover(node);
  return Boolean(node);
}

async function clickNode(id) {
  const node = nodeById(id);
  if (!node) return false;
  graphState.selectedId = node.id;
  graphState.graph?.select(node.id);
  if (node.kind === "memory") {
    await openMemoryNode(node);
  } else {
    renderGraphNodeDetail(node);
  }
  return true;
}

async function openMemoryNode(node) {
  const project = currentProject;
  const generation = graphState.generation;
  try {
    let entry = null;
    if (!node.archived) {
      const list = await getMemoryEntries(project, node.scope);
      entry = (list ?? []).find((item) => item.id === node.id) ?? null;
    }
    if (!entry) entry = await invoke("memory_entry_get", { projectDir: project, scope: node.scope, id: node.id });
    if (project !== currentProject || generation !== graphState.generation) return;
    showMemoryDetail(node.scope, { ...entry, scope: node.scope }, { readOnly: Boolean(entry.archived) });
  } catch (err) {
    if (project !== currentProject) return;
    toastError(`${t("记忆条目加载失败")}:${err}`);
  }
}

function enterEgo(id, hops = 2) {
  if (!nodeById(id)) return false;
  graphState.ego = { center: id, hops: Math.max(1, Math.min(3, Number(hops) || 2)) };
  graphState.fitPending = true;
  void render();
  return true;
}

function exitEgo() {
  if (!graphState.ego) return;
  graphState.ego = null;
  graphState.fitPending = true;
  void render();
}

function syncEgoBar() {
  const bar = $("memory-graph-ego");
  if (!bar) return;
  bar.classList.toggle("hidden", !graphState.ego);
  const label = $("memory-graph-ego-center");
  if (label) label.textContent = graphState.ego ? areaName(graphState.ego.center) : "";
  for (const button of bar.querySelectorAll("button[data-hops]")) {
    button.setAttribute("aria-pressed", String(Number(button.dataset.hops) === graphState.ego?.hops));
  }
}

// ---------- 非记忆节点详情 ----------

function relationGroups(node) {
  const groups = new Map();
  for (const edge of graphState.payload?.edges ?? []) {
    if (edge.source !== node.id && edge.target !== node.id) continue;
    if (edge.rel === "depends_on") continue;
    const outgoing = edge.source === node.id;
    const other = nodeById(outgoing ? edge.target : edge.source);
    if (!other) continue;
    const key = edge.rel;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push({ edge, other, outgoing });
  }
  return groups;
}

function button(text, onClick, className = "ghost mini") {
  const el = document.createElement("button");
  el.type = "button";
  el.className = className;
  el.textContent = text;
  el.addEventListener("click", onClick);
  return el;
}

function prefillMergeChat(node, members) {
  const ids = members.map((m) => m.id).join(" ");
  const input = $("memory-chat-input");
  const common = node.kind === "fingerprint" ? `${t("共同指纹")} ${node.title}` : `${t("共同主题")} ${node.title}`;
  if (input) input.value = `${t("合并这些重复记忆")}:${ids}(${common})`;
  showMemoryTab("chat");
  input?.focus();
}

export function renderGraphNodeDetail(node) {
  releaseMemoryDetail();
  const box = $("memory-detail");
  if (!box) return;
  showMemoryTab("read");
  $("memory-reader-empty")?.classList.add("hidden");
  $("memory-scroll")?.classList.add("has-memory-selection");
  box.classList.remove("hidden");
  box.replaceChildren();
  box.dataset.graphNode = node.id;
  const head = document.createElement("div");
  head.className = "memory-detail-head";
  const title = document.createElement("div");
  title.className = "memory-detail-title";
  title.textContent = node.kind === "crate" || node.kind === "module" ? areaName(node.id) : node.title || node.id;
  head.append(title, button(t("关闭详情"), () => {
    releaseMemoryDetail();
    graphState.selectedId = null;
    graphState.graph?.select(null);
  }));
  const meta = document.createElement("div");
  meta.className = "memory-detail-meta";
  const metaParts = [t(KIND_KEYS[node.kind] ?? "记忆")];
  if (node.kind !== "crate" && node.kind !== "module") metaParts.unshift(node.id);
  if (node.status) metaParts.push(node.status);
  if (node.archived) metaParts.push(t("已归档"));
  meta.textContent = metaParts.join(" · ");
  const actions = document.createElement("div");
  actions.className = "memory-graph-node-actions";
  const groups = relationGroups(node);
  const memberMemories = [...groups.values()].flat().filter((r) => r.other.kind === "memory").map((r) => r.other);
  if (node.kind === "requirement" || node.kind === "defect") {
    actions.append(button(t("打开条目"), () => void jumpToEntry(node.id, { expand: true })));
  } else if (node.kind === "decision") {
    actions.append(button(t("打开决策文档"), () => void openDocViewer("decision")));
  } else if (node.kind === "doc") {
    actions.append(button(t("打开设计文档"), () => void openArchDoc(node.id.replace(/^doc:/, ""))));
  } else if (node.kind === "crate" || node.kind === "module") {
    actions.append(button(t("只看该区域"), () => setAreaFilter(node.id)));
  } else if (node.kind === "fingerprint" || node.kind === "subject") {
    actions.append(button(t("用对话合并"), () => prefillMergeChat(node, memberMemories), "ghost"));
  }
  if (!graphState.ego || graphState.ego.center !== node.id) actions.append(button(t("看邻域"), () => enterEgo(node.id, 2)));
  const relations = document.createElement("div");
  relations.className = "memory-graph-relations";
  for (const [rel, items] of groups) {
    const section = document.createElement("section");
    const heading = document.createElement("h3");
    heading.textContent = `${t(REL_KEYS[rel] ?? "关联")} (${items.length})`;
    section.append(heading);
    const list = document.createElement("ul");
    for (const { other, outgoing, edge } of items.slice(0, 40)) {
      const item = document.createElement("li");
      const link = document.createElement("button");
      link.type = "button";
      link.className = "memory-graph-relation";
      link.dataset.nodeId = other.id;
      const name = other.kind === "crate" || other.kind === "module" ? areaName(other.id) : other.id;
      const provenance = edge.provenance ? `(${t(PROVENANCE_KEYS[edge.provenance] ?? "关键词")})` : "";
      link.textContent = `${outgoing ? "→" : "←"} ${name}${provenance} ${other.kind === "memory" || other.kind === "requirement" || other.kind === "defect" || other.kind === "decision" ? String(other.title ?? "").slice(0, 40) : ""}`;
      link.addEventListener("click", () => void clickNode(other.id));
      item.append(link);
      list.append(item);
    }
    if (items.length > 40) {
      const more = document.createElement("li");
      more.className = "dim";
      more.textContent = `… ${items.length - 40}`;
      list.append(more);
    }
    section.append(list);
    relations.append(section);
  }
  if (!groups.size) {
    const empty = document.createElement("p");
    empty.className = "dim";
    empty.textContent = t("没有关系");
    relations.append(empty);
  }
  box.append(head, meta, actions, relations);
}

// ---------- 工具栏 ----------

function fillAreaSelect() {
  const select = $("memory-graph-area");
  if (!select) return;
  const map = areasById();
  const crates = (graphState.payload?.areas ?? []).filter((area) => area.kind === "crate" && area.memories > 0);
  const modules = (graphState.payload?.areas ?? []).filter((area) => area.kind === "module" && area.memories > 0);
  const all = document.createElement("option");
  all.value = "";
  all.textContent = t("全部区域");
  const children = [all];
  for (const crate of crates) {
    const group = document.createElement("optgroup");
    group.label = areaName(crate.id);
    const whole = document.createElement("option");
    whole.value = crate.id;
    whole.textContent = `${areaName(crate.id)} (${crate.memories})`;
    group.append(whole);
    for (const module of modules.filter((m) => crateOfArea(m.id, map) === crate.id)) {
      const option = document.createElement("option");
      option.value = module.id;
      option.textContent = `${areaName(module.id)} (${module.memories})`;
      group.append(option);
    }
    children.push(group);
  }
  select.replaceChildren(...children);
  select.value = graphState.area;
  if (select.value !== graphState.area) graphState.area = "";
}

function setAreaFilter(areaId) {
  graphState.area = areaId || "";
  const select = $("memory-graph-area");
  if (select) select.value = graphState.area;
  graphState.ego = null;
  graphState.fitPending = true;
  void render();
}

function renderLegend() {
  const legend = $("memory-graph-legend");
  if (!legend || legend.childElementCount) return;
  const items = [
    ["fact", "fact"],
    ["sop", "sop"],
    ["habit", "habit"],
    ["preference", "preference"],
    ["context", t("需求/缺陷/决策/文档")],
    ["crate", t("代码区域")],
    ["module", t("模块")],
    ["concept", t("共享指纹/主题")],
  ];
  for (const [kind, text] of items) {
    const item = document.createElement("span");
    item.className = "memory-graph-legend-item";
    const swatch = document.createElement("i");
    swatch.className = "kz-graph-swatch";
    swatch.dataset.kind = kind;
    swatch.setAttribute("aria-hidden", "true");
    item.append(swatch, document.createTextNode(text));
    legend.append(item);
  }
  const note = document.createElement("span");
  note.className = "memory-graph-legend-note";
  note.textContent = t("实心=active · 空心=候选 · 半透明=归档 · 虚线=推断/提及 · 双击进入邻域");
  legend.append(note);
}

// ---------- 文本视图 ----------

function renderTextView(subgraph) {
  const tree = $("memory-graph-list");
  if (!tree) return;
  const groups = groupForTextView(subgraph.nodes, graphState.payload);
  const memoryItem = (node) => {
    const item = document.createElement("li");
    item.setAttribute("role", "treeitem");
    item.tabIndex = -1;
    item.dataset.memoryId = node.id;
    item.dataset.nodeId = node.id;
    item.className = `memory-graph-tree-memory${node.id === graphState.selectedId ? " selected" : ""}${graphState.hits.includes(node.id) ? " hit" : ""}`;
    item.textContent = `${node.id} · ${node.title ?? ""} · ${node.category ?? ""}/${node.status ?? ""}${node.archived ? ` · ${t("已归档")}` : ""}`;
    item.addEventListener("click", () => void clickNode(node.id));
    return item;
  };
  const branch = (label, count) => {
    const item = document.createElement("li");
    item.setAttribute("role", "treeitem");
    item.setAttribute("aria-expanded", "true");
    item.tabIndex = -1;
    const caption = document.createElement("span");
    caption.className = "memory-graph-tree-caption";
    caption.textContent = `${label} (${count})`;
    const group = document.createElement("ul");
    group.setAttribute("role", "group");
    item.append(caption, group);
    return [item, group];
  };
  const children = [];
  for (const group of groups) {
    const total = group.memories.length + group.modules.reduce((sum, m) => sum + m.memories.length, 0);
    const [item, list] = branch(group.id ? group.label : t("未归类"), total);
    for (const module of group.modules) {
      const [moduleItem, moduleList] = branch(module.label, module.memories.length);
      moduleList.append(...module.memories.map(memoryItem));
      list.append(moduleItem);
    }
    list.append(...group.memories.map(memoryItem));
    children.push(item);
  }
  if (!children.length) {
    const empty = document.createElement("li");
    empty.className = "dim";
    empty.textContent = t("该筛选暂无记忆");
    children.push(empty);
  }
  tree.replaceChildren(...children);
  const first = tree.querySelector('[role="treeitem"]');
  if (first) first.tabIndex = 0;
}

function onTreeKeydown(event) {
  const tree = $("memory-graph-list");
  const items = [...(tree?.querySelectorAll('[role="treeitem"]') ?? [])];
  const index = items.indexOf(event.target);
  if (index < 0) return;
  let next = null;
  if (event.key === "ArrowDown") next = items[index + 1];
  else if (event.key === "ArrowUp") next = items[index - 1];
  else if (event.key === "Home") next = items[0];
  else if (event.key === "End") next = items[items.length - 1];
  else if ((event.key === "Enter" || event.key === " ") && event.target.dataset.memoryId) {
    event.preventDefault();
    void clickNode(event.target.dataset.memoryId);
    return;
  }
  if (!next) return;
  event.preventDefault();
  items[index].tabIndex = -1;
  next.tabIndex = 0;
  next.focus();
}

// ---------- 区域提供者(详情页「区域」行) ----------

setMemoryAreaProvider({
  areasFor(scope, id) {
    if (graphState.payloadProject !== currentProject) return null;
    const node = nodeById(id);
    if (!node || node.scope !== scope) return null;
    return (graphState.payload?.edges ?? [])
      .filter((edge) => edge.source === id && edge.rel === "about")
      .map((edge) => ({ area: edge.target, provenance: edge.provenance, via: edge.via }));
  },
  async ensureAreaOptions() {
    const project = currentProject;
    if (!project) return [];
    let payload = graphState.payloadProject === project && graphState.payload ? graphState.payload : null;
    if (!payload) {
      payload = await loadPayload(project);
      if (project !== currentProject) return [];
      // 列表模式下取到的载荷也存下来:详情栏的推断区域(areasFor)与之后切到图谱都用它。
      if (graphState.payloadProject !== project) graphState.nodeObjects.clear();
      graphState.payload = payload;
      graphState.payloadProject = project;
      graphState.payloadAt = Date.now();
    }
    if (project !== currentProject) return [];
    const map = new Map((payload?.areas ?? []).map((area) => [area.id, area]));
    return (payload?.areas ?? []).map((area) => ({
      id: areaName(area.id),
      label: areaName(area.id),
      group: areaName(crateOfArea(area.id, map) ?? area.id),
    }));
  },
});

// ---------- 事件接线 ----------

defer(() => {
  $("memory-view-list")?.addEventListener("click", () => setMemoryView("list"));
  $("memory-view-graph")?.addEventListener("click", () => setMemoryView("graph"));
  $("memory-graph-area")?.addEventListener("change", (event) => setAreaFilter(event.target.value));
  for (const layer of LAYERS) {
    const el = document.querySelector(`#memory-graph-pane button[data-layer="${layer}"]`);
    if (!el) continue;
    el.setAttribute("aria-pressed", String(graphState.layers.has(layer)));
    el.textContent = t(LAYER_KEYS[layer]);
    el.addEventListener("click", () => {
      if (graphState.layers.has(layer)) graphState.layers.delete(layer);
      else graphState.layers.add(layer);
      el.setAttribute("aria-pressed", String(graphState.layers.has(layer)));
      void render();
    });
  }
  $("memory-graph-archived")?.addEventListener("click", () => {
    graphState.archived = !graphState.archived;
    $("memory-graph-archived").setAttribute("aria-pressed", String(graphState.archived));
    void render();
  });
  for (const hop of document.querySelectorAll("#memory-graph-ego button[data-hops]")) {
    hop.addEventListener("click", () => graphState.ego && enterEgo(graphState.ego.center, Number(hop.dataset.hops)));
  }
  $("memory-graph-ego-exit")?.addEventListener("click", exitEgo);
  $("memory-graph-fit")?.addEventListener("click", () => graphState.graph?.fit(400));
  $("memory-graph-textview")?.addEventListener("click", () => {
    graphState.textView = !graphState.textView;
    void render();
  });
  // 元素级 Esc:只在焦点落在画布或文本树上时退出邻域(全局 Esc 归 00-surface.js 的弹层栈)。
  for (const id of ["memory-graph-canvas", "memory-graph-list"]) {
    $(id)?.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && graphState.ego) {
        event.preventDefault();
        exitEgo();
      }
    });
  }
  $("memory-graph-list")?.addEventListener("keydown", onTreeKeydown);

  document.addEventListener("kz:memory-filters", (event) => {
    if (event.detail?.project !== currentProject || graphState.view !== "graph") return;
    graphState.ego = null;
    void render();
  });
  document.addEventListener("kz:memory-search-hits", (event) => {
    if (event.detail?.project !== currentProject) return;
    graphState.hits = [...(event.detail?.ids ?? [])];
    if (graphState.view !== "graph") return;
    if (graphState.graph && !graphState.textView) {
      graphState.graph.highlight(graphState.hits.filter((id) => graphState.shown.nodes.some((n) => n.id === id)));
      const first = graphState.hits.find((id) => graphState.shown.nodes.some((n) => n.id === id));
      if (first) graphState.graph.focus(first, 2);
    } else {
      renderTextView(graphState.shown);
    }
  });
  document.addEventListener("kz:memory-selected", (event) => {
    if (event.detail?.project !== currentProject) return;
    graphState.selectedId = event.detail?.id ?? null;
    graphState.graph?.select(graphState.selectedId);
  });
  document.addEventListener("kz:memory-selection-cleared", () => {
    graphState.selectedId = null;
    graphState.graph?.select(null);
  });
  document.addEventListener("kz:memory-project", () => {
    graphState.generation += 1;
    graphState.payload = null;
    graphState.payloadProject = null;
    graphState.loading = null;
    graphState.nodeObjects.clear();
    graphState.ego = null;
    graphState.area = "";
    graphState.selectedId = null;
    graphState.hits = [];
    graphState.shown = { nodes: [], links: [] };
    graphState.graph?.setData({ nodes: [], links: [] });
    $("memory-graph-list")?.replaceChildren();
    debugHook.ready = false;
    if (graphState.view === "graph" && document.querySelector("#view-memory.active")) void loadAndRender();
  });
  document.addEventListener("kz:memory-changed", (event) => {
    if (event.detail?.project !== currentProject) return;
    graphState.payloadAt = 0;
    if (graphState.view === "graph") void loadAndRender({ force: true });
  });
  document.addEventListener("kz:view-changed", (event) => {
    if (event.detail?.view !== "memory") {
      graphState.graph?.pause();
      return;
    }
    if (graphState.view === "graph") void loadAndRender();
  });
  let restored = null;
  try {
    restored = localStorage.getItem("kz-memory-view");
  } catch {
    restored = null;
  }
  if (restored === "graph") setMemoryView("graph", { persist: false });
  void uiPrefsLoad().then((prefs) => {
    if (prefs?.memory_view && prefs.memory_view !== graphState.view) setMemoryView(prefs.memory_view, { persist: false });
  });
});
