import { t, languageIsEnglish, localizedDocStatus, localizedStatusWord } from "./02-i18n.js";
import { buildWorkMap } from "./26-work-map-model.js";
import { reviewStatus, workReviewPending, workIsCurrent, workResultCurrent } from "./26-work-tree.js";

const words = (zh, en) => languageIsEnglish() ? en : t(zh);
const kinds = { goal: "目标节点", requirement: "需求节点", implementation: "实现", defect: "缺陷节点", research: "调研节点", design: "设计节点", documentation: "文档节点", verification: "验证节点" };
const steps = { plan: "补齐理解与计划", execute: "推进当前工作", integrate: "验收整体结果", wait: "等待前提满足", finished: "工作已结束" };
const el = (tag, content, cls) => {
  const node = document.createElement(tag);
  if (content != null) node.textContent = content;
  if (cls) node.className = cls;
  return node;
};
const button = (content, callback, cls = "ghost") => {
  const node = el("button", content, cls); node.type = "button";
  node.addEventListener("click", callback); return node;
};
const stateText = node => node.unavailable ? words("未在当前快照中", "Not in the current snapshot") : node.unit ? reviewStatus(node.unit) || localizedStatusWord(node.status, "unit") : localizedDocStatus(node.status);

// Load with the feature rather than touching the unrelated application stylesheet.
function stylesheet() {
  if (document.querySelector('link[data-work-map-style]')) return;
  const link = document.createElement("link"); link.rel = "stylesheet";
  link.href = new URL("./work-map.css", import.meta.url).href; link.dataset.workMapStyle = "";
  document.head.append(link);
}

export function renderWorkMap(entries, snapshot, ui = {}, onOpen = () => {}) {
  stylesheet();
  const map = buildWorkMap(entries, snapshot);
  const root = el("section", null, "goal-map"); root.setAttribute("aria-label", words("目标地图", "Goal map"));
  const legend = el("p", null, "goal-map-legend");
  legend.append(el("span", words("实线：目标分解", "Solid: goal decomposition"), "goal-map-parent-key"), el("span", words("虚线箭头：前置结果 → 使用它的工作", "Dashed arrow: prerequisite → consuming work"), "goal-map-dependency-key"));
  root.append(legend);
  if (!map.nodes.size) { root.append(el("p", words("没有符合条件的目标", "No matching goals"), "management-empty")); return root; }
  if (map.invalid.size) root.append(el("p", words("节点归属异常，请核对工作图", "Invalid ownership; check the work graph"), "management-error"));
  const layout = el("div", null, "goal-map-layout");
  const scroll = el("div", null, "goal-map-scroll"); scroll.tabIndex = 0;
  scroll.setAttribute("aria-label", words("目标关系图，可横向和纵向滚动", "Goal relationships; scroll horizontally and vertically"));
  const canvas = el("div", null, "goal-map-canvas");
  canvas.style.width = `${map.width}px`; canvas.style.height = `${map.height}px`;
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("width", map.width); svg.setAttribute("height", map.height); svg.setAttribute("aria-hidden", "true");
  canvas.append(svg);
  const paths = [];
  for (const edge of map.edges) {
    const from = map.positions.get(edge.from), to = map.positions.get(edge.to);
    const path = document.createElementNS(svg.namespaceURI, "path");
    path.dataset.relation = edge.kind; path.dataset.from = edge.from; path.dataset.to = edge.to;
    if (edge.kind === "parent") {
      const middle = (from.x + 236 + to.x) / 2;
      path.setAttribute("d", `M ${from.x + 236} ${from.y + 52} C ${middle} ${from.y + 52}, ${middle} ${to.y + 52}, ${to.x} ${to.y + 52}`);
    } else {
      // Separate bottom ports keep cross-branch links distinct from ownership.
      const sx = from.x + 164, sy = from.y + 104, tx = to.x + 72, ty = to.y + 104;
      const bend = Math.max(sy, ty) + 20;
      path.setAttribute("d", `M ${sx} ${sy} C ${sx} ${bend}, ${tx} ${bend}, ${tx} ${ty + 6}`);
      const arrow = document.createElementNS(svg.namespaceURI, "path");
      arrow.setAttribute("d", `M ${tx - 4} ${ty + 8} L ${tx} ${ty + 2} L ${tx + 4} ${ty + 8}`);
      arrow.dataset.relation = "arrow"; svg.append(arrow);
      paths.push({ element: arrow, edge });
    }
    paths.push({ element: path, edge }); svg.append(path);
  }
  const inspector = el("section", null, "goal-map-inspector");
  inspector.setAttribute("aria-label", words("节点详情", "Node details")); inspector.setAttribute("aria-live", "polite");
  const cards = new Map();
  const select = (id, reveal = false) => {
    ui.selected = id;
    for (const [key, card] of cards) card.setAttribute("aria-pressed", String(key === id));
    for (const { element, edge } of paths) element.dataset.focused = String(edge.from === id || edge.to === id);
    renderInspector(inspector, map.nodes.get(id), map, select, onOpen);
    if (reveal) cards.get(id)?.scrollIntoView({ block: "nearest", inline: "nearest" });
  };
  for (const node of map.nodes.values()) {
    const card = button("", () => select(node.id), "goal-map-node");
    const position = map.positions.get(node.id);
    card.style.left = `${position.x}px`; card.style.top = `${position.y}px`;
    card.dataset.workMapNode = node.id; card.dataset.state = node.status; card.dataset.context = String(node.context);
    card.dataset.resultCurrent = String(node.unit ? workResultCurrent(node.unit) : node.status === "done");
    card.dataset.reviewState = node.unit && workReviewPending(node.unit) ? "pending" : node.unit && !workIsCurrent(node.unit) ? "historical" : "current";
    card.setAttribute("aria-label", `${node.id} · ${node.title} · ${stateText(node)}`);
    card.title = `${node.id} · ${node.title}`;
    const step = !["done", "superseded"].includes(node.status) && !reviewStatus(node.unit || {}) ? node.readiness?.step : null;
    const summary = ["plan", "wait", "integrate"].includes(step) ? `${stateText(node)} · ${t(steps[step])}` : stateText(node);
    card.append(el("small", `${node.id} · ${node.unavailable ? words("关联引用", "Referenced result") : t(kinds[node.kind] || node.kind)}`, "goal-map-identity"), el("strong", node.title), el("span", summary, "work-unit-status"));
    cards.set(node.id, card); canvas.append(card);
  }
  scroll.append(canvas); layout.append(scroll, inspector); root.append(layout);
  scroll.addEventListener("scroll", () => { ui.left = scroll.scrollLeft; ui.top = scroll.scrollTop; });
  requestAnimationFrame(() => { scroll.scrollLeft = ui.left || 0; scroll.scrollTop = ui.top || 0; });
  const selected = map.nodes.has(ui.selected) ? ui.selected : entries[0]?.id;
  select(selected);
  return root;
}

function renderInspector(host, node, map, select, onOpen) {
  host.replaceChildren();
  if (!node) return;
  host.append(el("code", node.id), el("h2", node.title), el("p", stateText(node), "dim"));
  const field = (label, values, cls = "") => {
    if (!values?.length) return;
    const section = el("div", null, `goal-map-field ${cls}`);
    section.append(el("h3", t(label)));
    for (const value of values) section.append(el("p", value));
    host.append(section);
  };
  const relations = (label, ids) => {
    if (!ids.length) return;
    const section = el("div", null, "goal-map-field"); section.append(el("h3", t(label)));
    for (const id of ids) section.append(button(`${id} · ${map.nodes.get(id)?.title || id}`, () => select(id, true), "ghost goal-map-reference"));
    host.append(section);
  };
  if (node.context) field(words("关联上下文", "Related context"), [words("此节点是当前筛选之外的前提或归属，不代表新的待办。", "This node provides context outside the current filter; it is not a new task.")]);
  if (node.unavailable) {
    field(words("引用说明", "Reference"), [words("当前快照没有这个节点，可能已归档；不据此推断它已完成或被阻塞。", "This node may be archived. Its absence does not imply completion or blockage.")]);
    return;
  }
  if (node.entry) host.append(button(words("打开需求详情", "Open work document"), () => onOpen(node.entry)));
  relations(words("所属目标", "Parent goal"), node.parent ? [node.parent] : []);
  relations("依赖结果", node.dependencies);
  const unit = node.unit;
  const actionable = !unit || (!["done", "superseded"].includes(unit.status) && !workReviewPending(unit) && workIsCurrent(unit));
  if (actionable) {
    field("下一步", node.readiness ? [t(steps[node.readiness.step] || node.readiness.step)] : []);
    field("阻塞原因", node.readiness?.blockers || (unit?.blocked_reason ? [unit.blocked_reason] : node.entry?.block_reasons), "blocked");
    field("尚未覆盖", node.readiness?.uncovered, "blocked");
  }
  if (!unit) return;
  if (workReviewPending(unit)) {
    field("新的方向", [unit.review.request.direction]);
    field("调整原因", [unit.review.request.reason]);
    field("下一步", [t("Agent 评估受影响工作，保留有效结果并修正计划")]);
  }
  if (unit.review?.resolution) {
    const decision = unit.review.resolution.decision;
    field("方向复核记录", [reviewStatus(unit) || stateText(node), decision.reason]);
    field("后继工作", (decision.replacements || []).map(id => `${id} · ${map.nodes.get(id)?.title || id}`));
    relations(words("查看后继", "View successor"), (decision.replacements || []).filter(id => map.nodes.has(id)));
  }
  field("当前理解", unit.node?.explanation ? [unit.node.explanation] : []);
  field("未决问题", unit.node?.questions, "blocked");
  field("支撑父级验收", unit.node?.covers);
  field("实质进展", unit.last_checkpoint?.summary ? [unit.last_checkpoint.summary] : []);
  field("下一步", actionable && unit.last_checkpoint?.next_action ? [unit.last_checkpoint.next_action] : []);
  for (const criterion of unit.acceptance || []) {
    const refs = [...new Set((unit.evidence || []).filter(item => item.criterion === criterion).flatMap(item => item.evidence_refs || []))];
    field(refs.length ? "已记录证据" : "待验证", [criterion]);
    field("证据引用", refs);
  }
  field("验证方法", unit.verification);
  field("来源", unit.node?.source_refs);
  field("结束原因", unit.terminal_reason ? [unit.terminal_reason] : []);
}
