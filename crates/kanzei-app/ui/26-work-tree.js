import { localizedStatusWord, t } from "./02-i18n.js";

export const kindLabels = { goal: "目标节点", requirement: "需求节点", implementation: "实现节点", defect: "缺陷节点", research: "调研节点", design: "设计节点", documentation: "文档节点", verification: "验证节点" };
const kindGlyphs = { goal: "◎", requirement: "◇", implementation: "⌘", defect: "!", research: "⌕", design: "◈", documentation: "≡", verification: "✓" };
const stepLabels = { plan: "补齐理解与计划", execute: "推进当前工作", integrate: "验收整体结果", wait: "等待前提满足", finished: "工作已结束" };

function text(tag, className, content) {
  const element = document.createElement(tag);
  element.className = className;
  element.textContent = content;
  return element;
}

function workUnitCard(unit, readiness) {
  const card = document.createElement("article");
  card.className = `work-unit-card ${unit.status}`;
  const head = document.createElement("div");
  head.className = "work-unit-head";
  head.append(text("code", "", unit.unit_id), text("span", "work-unit-status", localizedStatusWord(unit.status, "unit")));
  card.append(head, text("div", "work-unit-objective", unit.objective));
  const meta = (label, content, state = "") => {
    if (content) card.append(text("div", `work-unit-meta ${state}`, `${t(label)}: ${content}`));
  };
  const node = unit.node || {};
  if (node.custom_type) {
    meta("类型定义", node.custom_type.description);
    meta("预期产物", node.custom_type.deliverable);
  }
  if (readiness) meta("下一步", t(stepLabels[readiness.step] || readiness.step));
  meta("当前理解", node.explanation);
  for (const question of node.questions || []) meta("未决问题", question, "blocked");
  meta("支撑父级验收", (node.covers || []).join("；"));
  meta("依赖结果", (unit.dependencies || []).join(" · "));
  for (const blocker of readiness?.blockers || []) meta("阻塞原因", blocker, "blocked");
  for (const criterion of readiness?.uncovered || []) meta("尚未覆盖", criterion, "blocked");
  const checkpoint = unit.last_checkpoint || {};
  meta("实质进展", checkpoint.summary);
  meta("决策记录", (checkpoint.decisions || []).join("；"));
  meta("记忆来源", (checkpoint.retrieval_refs || []).join(", ") || t("未记录"));
  meta("验证方法", (unit.verification || []).join("；") || t("未声明"));
  if (!readiness) meta("阻塞原因", unit.blocked_reason, "blocked");
  meta("下一步", checkpoint.next_action);
  const evidence = unit.evidence || [];
  const acceptance = unit.acceptance || [];
  meta("验收证据", `${evidence.length}/${acceptance.length}`);
  for (const criterion of acceptance) {
    const proof = evidence.find(item => item.criterion === criterion);
    meta(proof ? "已记录证据" : "待验证", criterion);
    if (proof) meta("证据引用", (proof.evidence_refs || []).join(" · "));
  }
  meta("来源", (node.source_refs || []).join(" · "));
  if (unit.terminal_reason) meta("结束原因", unit.terminal_reason);
  return card;
}

export function captureWorkTreeState(root) { return root?.captureState?.() || null; }

/** One domain snapshot drives outline, inspector, references and action gates. */
export function renderWorkTree(units, readiness = {}, prior = null, options = {}) {
  const root = text("section", "work-tree-workspace", "");
  const toolbar = text("div", "work-tree-toolbar", "");
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
      split.disabled = !blockers || blockers.length > 0;
      const reason = blockers?.join("；") || (!blockers ? t("正在读取操作条件") : "");
      split.title = reason;
      split.onclick = () => options.onDecompose(unit, split);
      actions.append(split);
      if (reason) actions.append(text("small", "dim", reason));
    }
    if (options.onStart && ["execute", "integrate"].includes(readiness[id]?.step)) {
      const start = text("button", "ghost work-node-start", t("推进此节点"));
      start.type = "button"; start.onclick = () => options.onStart(unit); actions.append(start);
    }
    inspector.append(actions, workUnitCard(unit, readiness[id]));
    for (const dependency of unit.dependencies || []) {
      if (!nodes.has(dependency)) continue;
      const link = text("button", "ghost work-node-reference", `${t("查看依赖")} · ${nodes.get(dependency).objective}`);
      link.type = "button"; link.onclick = () => reveal(dependency); inspector.append(link);
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
    const status = ["wait", "plan", "integrate"].includes(step) ? t(stepLabels[step]) : localizedStatusWord(unit.status, "unit");
    branch.setAttribute("aria-label", `${unit.objective} · ${status}`);
    const glyph = text("span", "work-tree-glyph", kindGlyphs[unit.node?.kind] || "◇"); glyph.setAttribute("aria-hidden", "true");
    summary.append(caret, glyph, text("span", "work-tree-kind", unit.node?.custom_type?.name || t(kindLabels[unit.node?.kind || "requirement"])), text("span", "work-tree-title", unit.objective), text("span", "work-unit-status", status));
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
  root.captureState = () => ({ open: new Map(open), selected, focused, scope, known: ids, query: search.value,
    scrollTop: tree.scrollTop, restoreFocus: tree.contains(document.activeElement) });
  requestAnimationFrame(() => {
    tree.scrollTop = prior?.scrollTop || 0;
    if (prior?.restoreFocus && root.isConnected) focus(focused);
  });
  return root;
}
