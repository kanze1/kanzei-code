import { localizedStatusWord, t } from "./02-i18n.js";

export const kindLabels = { goal: "目标节点", requirement: "需求节点", implementation: "实现节点", defect: "缺陷节点", research: "调研节点", design: "设计节点", documentation: "文档节点", verification: "验证节点" };
const kindGlyphs = { goal: "◎", requirement: "◇", implementation: "⌘", defect: "!", research: "⌕", design: "◈", documentation: "≡", verification: "✓" };
const stepLabels = { plan: "补齐理解与计划", execute: "推进当前工作", integrate: "验收整体结果", wait: "等待前提满足", finished: "工作已结束" };
const dispositionLabels = { retain: "继续沿用", revise: "已修订", reverify: "由后继节点重验", replace: "已被替代", retire: "已撤销" };
export const workReviewPending = unit => Boolean(unit.review && !unit.review.resolution);
export const workIsCurrent = unit => unit.status !== "superseded" && !["reverify", "replace", "retire"].includes(unit.review?.resolution?.decision?.disposition);
export const workResultCurrent = unit => unit.status === "done" && workIsCurrent(unit) && !workReviewPending(unit);
const reviewStatus = unit => workReviewPending(unit) ? t("方向待复核") : !workIsCurrent(unit) && unit.review?.resolution ? t(dispositionLabels[unit.review.resolution.decision.disposition]) : null;

function text(tag, className, content) {
  const element = document.createElement(tag);
  element.className = className;
  element.textContent = content;
  return element;
}

/** Human detail is a projection of recorded facts; execution contracts stay in the store. */
function workUnitCard(unit, readiness, nodes, expanded) {
  const card = text("article", `work-unit-card ${unit.status}`, "");
  const node = unit.node || {}, checkpoint = unit.last_checkpoint || {};
  const nameReferences = value => value.replace(/R-\d+\/W\d+/g, id => nodes.get(id)?.objective || id);
  const meta = (parent, label, content, state = "") => {
    if (!content) return;
    const row = text("div", `work-unit-meta ${state}`, "");
    row.append(text("span", "work-meta-label", t(label)), text("span", "work-meta-value", content));
    parent.append(row);
  };
  const section = (key, label) => {
    const details = text("details", "work-inspector-section", "");
    details.dataset.section = key;
    details.open = expanded.get(`${unit.unit_id}:${key}`) || false;
    details.append(text("summary", "", t(label)));
    details.ontoggle = () => expanded.set(`${unit.unit_id}:${key}`, details.open);
    card.append(details); return details;
  };
  const terminal = ["done", "superseded"].includes(unit.status);
  const status = reviewStatus(unit) || (!terminal && ["plan", "wait", "integrate"].includes(readiness?.step)
    ? t(stepLabels[readiness.step]) : localizedStatusWord(unit.status, "unit"));
  meta(card, "当前情况", status);
  if (workReviewPending(unit)) {
    meta(card, "新的方向", unit.review.request.direction);
    meta(card, "调整原因", unit.review.request.reason);
    meta(card, "下一步", t("Agent 评估受影响工作，保留有效结果并修正计划"));
  }
  meta(card, checkpoint.summary ? "实质进展" : "当前理解", checkpoint.summary || node.explanation);
  if (!terminal && !workReviewPending(unit)) {
    const blockers = [...new Set([...(readiness?.blockers || []), unit.blocked_reason].filter(Boolean))];
    meta(card, "阻塞原因", blockers.map(nameReferences).join("\n"), "blocked");
    if (!(unit.acceptance || []).length) meta(card, "下一步", t("先明确完成条件，再开始执行"));
    else if (readiness?.step !== "wait") meta(card, "下一步", checkpoint.next_action);
  }
  meta(card, "结束原因", unit.terminal_reason);
  meta(card, "预期产物", node.custom_type?.deliverable);
  if (unit.review?.resolution) {
    const revision = section("revision", "方向复核记录");
    const decision = unit.review.resolution.decision;
    meta(revision, "处置结果", t(dispositionLabels[decision.disposition]));
    meta(revision, "调整原因", decision.reason);
    meta(revision, "新的方向", unit.review.request.direction);
    meta(revision, "后继工作", (decision.replacements || []).map(id => nodes.get(id)?.objective || id).join("；"));
  }
  if (node.questions?.length) {
    const questions = section("questions", "待澄清的问题");
    // An unresolved research question does not imply a request for human input.
    for (const question of node.questions) questions.append(text("p", "work-inspector-note", question));
  }
  const acceptance = unit.acceptance || [], evidence = unit.evidence || [];
  if (acceptance.length) {
    const outcome = section("outcome", "完成条件与结果");
    for (const criterion of acceptance) {
      const refs = [...new Set(evidence.filter(proof => proof.criterion === criterion).flatMap(proof => proof.evidence_refs || []))];
      meta(outcome, refs.length ? "已记录证据" : "待验证", criterion);
      if (refs.length) meta(outcome, "证据引用", refs.join(" · "));
    }
    for (const criterion of readiness?.uncovered || []) meta(outcome, "尚未覆盖", criterion);
    meta(outcome, "验证方法", (unit.verification || []).join("；"));
  }
  if (node.source_refs?.length || checkpoint.decisions?.length || checkpoint.retrieval_refs?.length) {
    const basis = section("basis", "研究与依据");
    meta(basis, "决策记录", (checkpoint.decisions || []).join("；"));
    meta(basis, "来源", (node.source_refs || []).join(" · "));
    meta(basis, "记忆来源", (checkpoint.retrieval_refs || []).join(" · "));
  }
  const record = section("record", "节点记录");
  meta(record, "节点编号", unit.unit_id);
  if (unit.source_sequence != null) meta(record, "记录版本", String(unit.source_sequence));
  meta(record, "类型定义", node.custom_type?.description);
  meta(record, "支撑父级验收", (node.covers || []).join("；"));
  return card;
}

export function captureWorkTreeState(root) { return root?.captureState?.() || null; }

/** One domain snapshot drives outline, inspector, references and action gates. */
export function renderWorkTree(units, readiness = {}, prior = null, options = {}) {
  const root = text("section", "work-tree-workspace", "");
  const toolbar = text("div", "work-tree-toolbar", "");
  let view = prior?.view || (window.innerWidth < 850 ? "outline" : "map");
  root.dataset.view = view;
  const views = text("div", "work-tree-views", "");
  for (const [value, label] of [["map", "目标地图"], ["outline", "大纲"]]) {
    const button = text("button", "ghost", t(label)); button.type = "button";
    button.setAttribute("aria-pressed", String(view === value));
    button.onclick = () => {
      view = value; root.dataset.view = value;
      for (const control of views.children) control.setAttribute("aria-pressed", String(control === button));
    };
    views.append(button);
  }
  toolbar.append(views);
  const search = document.createElement("input");
  search.type = "search"; search.placeholder = t("搜索节点，保留目标路径");
  search.setAttribute("aria-label", t("搜索节点")); search.value = prior?.query || "";
  toolbar.append(search);
  const layout = text("div", "work-tree-layout", "");
  const tree = document.createElement("ul");
  tree.className = "work-unit-list work-tree";
  tree.setAttribute("role", "tree");
  tree.setAttribute("aria-label", t("目标树"));
  const inspector = text("aside", "work-tree-inspector", "");
  inspector.setAttribute("aria-label", t("验收与依据"));
  layout.append(tree, inspector); root.append(toolbar, layout);
  const nodes = new Map(units.map(unit => [unit.unit_id, unit]));
  const scope = nodes.has(prior?.scope) ? prior.scope : null;
  const elements = new Map(), groups = new Map();
  const open = new Map(prior?.open || []);
  const detailOpen = new Map(prior?.detailOpen || []);
  let selected = nodes.has(prior?.selected) ? prior.selected : (units.find(unit => unit.status === "active") || units[0])?.unit_id;
  let focused = nodes.has(prior?.focused) ? prior.focused : selected;
  const ids = new Set(units.map(unit => unit.unit_id));
  const children = new Map();
  for (const unit of units) {
    const parent = unit.node?.parent_id || null;
    if (!children.has(parent)) children.set(parent, []);
    children.get(parent).push(unit);
  }
  const rendered = new Set();
  const changeScope = (id, selection = id) => {
    const state = root.captureState();
    state.scope = id; state.selected = selection; state.focused = selection; state.query = ""; state.restoreFocus = true;
    let parent = nodes.get(selection)?.node?.parent_id;
    const visited = new Set();
    while (parent && !visited.has(parent)) { visited.add(parent); state.open.set(parent, true); parent = nodes.get(parent)?.node?.parent_id; }
    root.replaceWith(renderWorkTree(units, readiness, state, options));
  };
  if (scope) {
    const back = text("button", "ghost work-tree-breadcrumb", t("全部目标")); back.type = "button";
    back.onclick = () => changeScope(null, scope);
    toolbar.prepend(back, text("span", "dim", nodes.get(scope).objective));
  }
  const select = id => {
    selected = id;
    for (const [key, el] of elements) el.setAttribute("aria-selected", String(key === id));
    const unit = nodes.get(id);
    inspector.dataset.kind = unit.node?.kind || "requirement";
    inspector.replaceChildren(text("h4", "work-tree-inspector-title", unit.objective));
    const actions = text("div", "work-tree-actions", "");
    if (children.get(id)?.length && id !== scope) {
      const zoom = text("button", "ghost work-node-focus", t("聚焦此分支")); zoom.type = "button";
      zoom.onclick = () => changeScope(id); actions.append(zoom);
    }
    if (options.onDecompose) {
      const split = text("button", "ghost work-node-split", t("裂变需求"));
      split.type = "button";
      const blockers = options.planningBlockers?.[id];
      const missingCriteria = !(unit.acceptance || []).length;
      split.disabled = !blockers || blockers.length > 0 || missingCriteria;
      const reason = blockers?.join("；") || (!blockers ? t("正在读取操作条件") : missingCriteria ? t("先明确完成条件，再拆分子工作") : "");
      split.title = reason;
      split.onclick = () => options.onDecompose(unit, split);
      actions.append(split);
    }
    if (options.onRevise && workIsCurrent(unit) && !workReviewPending(unit)) {
      const revise = text("button", "ghost work-node-revise", t("调整方向")); revise.type = "button";
      revise.onclick = () => options.onRevise(unit); actions.append(revise);
    }
    if (options.onStart && ["execute", "integrate"].includes(readiness[id]?.step)) {
      const start = text("button", "ghost work-node-start", t("推进此节点"));
      start.type = "button"; start.onclick = () => options.onStart(unit); actions.append(start);
    }
    inspector.append(actions, workUnitCard(unit, readiness[id], nodes, detailOpen));
    for (const dependency of unit.dependencies || []) {
      if (!nodes.has(dependency)) continue;
      const link = text("button", "ghost work-node-reference", `${t("查看依赖")} · ${nodes.get(dependency).objective}`);
      link.type = "button"; link.onclick = () => reveal(dependency); inspector.append(link);
    }
    for (const successor of unit.review?.resolution?.decision?.replacements || []) {
      if (!nodes.has(successor)) continue;
      const link = text("button", "ghost work-node-reference", `${t("后继工作")} · ${nodes.get(successor).objective}`);
      link.type = "button"; link.onclick = () => reveal(successor); inspector.append(link);
    }
  };
  const visible = () => [...elements.values()].filter(el => !el.hidden && !el.closest("[hidden]"));
  const focus = id => {
    focused = id;
    for (const [key, el] of elements) el.tabIndex = key === id ? 0 : -1;
    elements.get(id)?.focus({ preventScroll: true });
  };
  const display = () => {
    const query = search.value.trim().toLowerCase(), matches = new Set();
    if (query) for (const unit of units) {
      if (!`${unit.unit_id} ${unit.objective} ${unit.node?.custom_type?.name || t(kindLabels[unit.node?.kind] || "需求节点")}`.toLowerCase().includes(query)) continue;
      let id = unit.unit_id;
      while (id && !matches.has(id)) { matches.add(id); id = nodes.get(id)?.node?.parent_id; }
    }
    for (const [id, el] of elements) {
      el.hidden = Boolean(query && !matches.has(id));
      if (groups.has(id)) {
        const expanded = Boolean(query || open.get(id));
        el.setAttribute("aria-expanded", String(expanded)); groups.get(id).hidden = !expanded;
      }
    }
    const rows = visible();
    if (!rows.some(el => el.dataset.workNodeId === focused)) focused = rows[0]?.dataset.workNodeId;
    for (const [id, el] of elements) el.tabIndex = id === focused ? 0 : -1;
  };
  const reveal = id => {
    if (!elements.has(id)) { changeScope(null, id); return; }
    search.value = "";
    let parent = nodes.get(id)?.node?.parent_id;
    const seen = new Set();
    while (parent && !seen.has(parent)) { seen.add(parent); open.set(parent, true); parent = nodes.get(parent)?.node?.parent_id; }
    display(); select(id); focus(id); elements.get(id)?.scrollIntoView({ block: "nearest" });
  };
  const toggle = id => { open.set(id, !open.get(id)); display(); };
  const append = (list, unit, depth = 1, invalid = false) => {
    if (rendered.has(unit.unit_id)) return;
    rendered.add(unit.unit_id);
    const item = document.createElement("li");
    item.className = "work-tree-item work-tree-node";
    const branch = item;
    branch.dataset.workNodeId = unit.unit_id;
    branch.setAttribute("role", "treeitem"); branch.setAttribute("aria-level", String(depth));
    branch.dataset.kind = unit.node?.kind || "requirement";
    branch.dataset.status = unit.status;
    branch.dataset.review = workReviewPending(unit) ? "pending" : workIsCurrent(unit) ? "current" : "historical";
    elements.set(unit.unit_id, branch);
    if (!open.has(unit.unit_id)) open.set(unit.unit_id, !["done", "superseded"].includes(unit.status));
    if (prior?.known && !prior.known.has(unit.unit_id)) branch.classList.add("work-node-new");
    const summary = text("div", "work-tree-row", "");
    const descendants = children.get(unit.unit_id) || [];
    const caret = text("button", "work-tree-toggle", descendants.length ? "›" : "·");
    caret.type = "button"; caret.tabIndex = -1;
    caret.setAttribute("aria-label", t("展开或收起")); caret.disabled = !descendants.length;
    caret.onclick = event => { event.stopPropagation(); toggle(unit.unit_id); focus(unit.unit_id); };
    const step = readiness[unit.unit_id]?.step;
    const status = reviewStatus(unit) || (["wait", "plan", "integrate"].includes(step) ? t(stepLabels[step]) : localizedStatusWord(unit.status, "unit"));
    branch.setAttribute("aria-label", `${unit.objective} · ${status}`);
    const glyph = text("span", "work-tree-glyph", kindGlyphs[unit.node?.kind] || "◇"); glyph.setAttribute("aria-hidden", "true");
    const kind = text("span", "work-tree-kind", unit.node?.custom_type?.name || t(kindLabels[unit.node?.kind || "requirement"]));
    kind.title = t(kindLabels[unit.node?.kind || "requirement"]);
    summary.append(caret, glyph, kind, text("span", "work-tree-title", unit.objective),
      text("span", "work-unit-status", status));
    if (descendants.length) summary.append(text("small", "work-tree-count", String(descendants.length)));
    summary.onclick = () => { select(unit.unit_id); focus(unit.unit_id); };
    branch.addEventListener("focus", () => { focused = unit.unit_id; });
    branch.addEventListener("keydown", event => {
      if (event.target !== branch) return;
      const rows = visible(), index = rows.indexOf(branch);
      let target;
      if (event.key === "ArrowDown") target = rows[Math.min(index + 1, rows.length - 1)];
      else if (event.key === "ArrowUp") target = rows[Math.max(index - 1, 0)];
      else if (event.key === "Home") target = rows[0];
      else if (event.key === "End") target = rows.at(-1);
      else if (event.key === "ArrowRight") { if (descendants.length && !open.get(unit.unit_id)) toggle(unit.unit_id); else target = descendants.map(child => elements.get(child.unit_id)).find(el => rows.includes(el)); }
      else if (event.key === "ArrowLeft") { if (descendants.length && open.get(unit.unit_id)) toggle(unit.unit_id); else target = elements.get(unit.node?.parent_id); }
      else if (event.key === "Enter" || event.key === " ") select(unit.unit_id);
      else return;
      event.preventDefault(); event.stopPropagation();
      if (target) { focus(target.dataset.workNodeId); target.scrollIntoView({ block: "nearest" }); }
    });
    branch.append(summary);
    if (invalid) branch.append(text("div", "work-unit-meta blocked", t("节点归属异常，请核对工作图")));
    if (descendants.length) {
      const nested = document.createElement("ul");
      nested.className = "work-tree-children"; nested.setAttribute("role", "group"); groups.set(unit.unit_id, nested);
      for (const child of descendants) append(nested, child, depth + 1);
      branch.append(nested);
    }
    list.append(item);
  };
  if (scope) append(tree, nodes.get(scope));
  else for (const unit of units.filter(unit => !unit.node?.parent_id || !ids.has(unit.node.parent_id))) append(tree, unit, 1, Boolean(unit.node?.parent_id && !ids.has(unit.node.parent_id)));
  // Invalid cycles must remain visible instead of silently dropping work from the tree.
  if (!scope) for (const unit of units) if (!rendered.has(unit.unit_id)) append(tree, unit, 1, true);
  if (!elements.has(selected)) selected = scope;
  if (!elements.has(focused)) focused = selected;
  search.oninput = display;
  display(); if (selected) select(selected);
  root.captureState = () => ({ detailOpen: new Map(detailOpen), open: new Map(open), selected, focused, scope, known: ids, query: search.value, view,
    scrollTop: tree.scrollTop, scrollLeft: tree.scrollLeft, restoreFocus: tree.contains(document.activeElement) });
  requestAnimationFrame(() => {
    tree.scrollTop = prior?.scrollTop || 0;
    tree.scrollLeft = prior?.scrollLeft || 0;
    if (prior?.restoreFocus && root.isConnected) focus(focused);
  });
  return root;
}
