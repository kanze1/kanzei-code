// Structural tree rendering shared by work nodes and existing tracker roots.
import { t } from "./02-i18n.js";

function text(tag, className, content) {
  const element = document.createElement(tag);
  element.className = className;
  element.textContent = content;
  return element;
}

export function renderTreeView(items, prior = null, controls = {}) {
  const { searchControl, onFilter } = controls;
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
  const search = searchControl || document.createElement("input");
  search.type = "search"; search.placeholder = t("搜索节点，保留目标路径");
  search.setAttribute("aria-label", t("搜索节点")); search.value = prior?.query || "";
  if (!searchControl) toolbar.append(search);
  const layout = text("div", "work-tree-layout", "");
  const tree = document.createElement("ul");
  tree.className = "work-unit-list work-tree";
  tree.setAttribute("role", "tree");
  tree.setAttribute("aria-label", t("目标树"));
  const inspector = text("aside", "work-tree-inspector", "");
  inspector.setAttribute("aria-label", t("验收与依据"));
  layout.append(tree, inspector); root.append(toolbar, layout);
  const nodes = new Map(items.map(item => [item.id, item]));
  const scope = nodes.has(prior?.scope) ? prior.scope : null;
  const elements = new Map(), groups = new Map();
  const open = new Map(prior?.open || []);
  const detailOpen = new Map(prior?.detailOpen || []);
  let selected = nodes.has(prior?.selected) ? prior.selected : (items.find(item => item.primary) || items[0])?.id;
  let focused = nodes.has(prior?.focused) ? prior.focused : selected;
  const ids = new Set(items.map(item => item.id));
  const children = new Map();
  for (const item of items) {
    const parent = item.parentId || null;
    if (!children.has(parent)) children.set(parent, []);
    children.get(parent).push(item);
  }
  const rendered = new Set();
  const changeScope = (id, selection = id) => {
    const state = root.captureState();
    state.scope = id; state.selected = selection; state.focused = selection; state.query = ""; state.restoreFocus = true;
    let parent = nodes.get(selection)?.parentId;
    const visited = new Set();
    while (parent && !visited.has(parent)) { visited.add(parent); state.open.set(parent, true); parent = nodes.get(parent)?.parentId; }
    const next = renderTreeView(items, state, controls);
    next.className = root.className;
    for (const [key, value] of Object.entries(root.dataset)) if (key !== "view") next.dataset[key] = value;
    root.replaceWith(next);
  };
  if (scope) {
    const back = text("button", "ghost work-tree-breadcrumb", t("全部目标")); back.type = "button";
    back.onclick = () => changeScope(null, scope);
    toolbar.prepend(back, text("span", "dim", nodes.get(scope).title));
  }
  const select = id => {
    selected = id;
    for (const [key, el] of elements) el.setAttribute("aria-selected", String(key === id));
    const item = nodes.get(id);
    inspector.dataset.kind = item.kind;
    inspector.replaceChildren(text("h4", "work-tree-inspector-title", item.title));
    const actions = text("div", "work-tree-actions", "");
    if (children.get(id)?.length && id !== scope) {
      const zoom = text("button", "ghost work-node-focus", t("聚焦此分支")); zoom.type = "button";
      zoom.onclick = () => changeScope(id); actions.append(zoom);
    }
    inspector.append(actions);
    item.renderDetails({ inspector, actions, detailOpen, reveal });
  };
  const visible = () => [...elements.values()].filter(el => !el.hidden && !el.closest("[hidden]"));
  const focus = id => {
    focused = id;
    for (const [key, el] of elements) el.tabIndex = key === id ? 0 : -1;
    elements.get(id)?.focus({ preventScroll: true });
  };
  const display = () => {
    const query = search.value.trim().toLowerCase(), matches = new Set();
    if (query) for (const item of items) {
      if (!`${item.id} ${item.title} ${item.kindLabel}`.toLowerCase().includes(query)) continue;
      let id = item.id;
      while (id && !matches.has(id)) { matches.add(id); id = nodes.get(id)?.parentId; }
    }
    for (const [id, el] of elements) {
      el.hidden = Boolean(query && !matches.has(id));
      if (groups.has(id)) {
        const expanded = Boolean(query || open.get(id));
        el.setAttribute("aria-expanded", String(expanded)); groups.get(id).hidden = !expanded;
      }
    }
    const rows = visible();
    inspector.hidden = rows.length === 0; empty.hidden = rows.length > 0;
    if (rows.length && !rows.some(row => row.dataset.workNodeId === selected)) select(rows[0].dataset.workNodeId);
    onFilter?.({ query: search.value, visibleIds: rows.map(row => row.dataset.workNodeId) });
    if (!rows.some(el => el.dataset.workNodeId === focused)) focused = rows[0]?.dataset.workNodeId;
    for (const [id, el] of elements) el.tabIndex = id === focused ? 0 : -1;
  };
  const reveal = id => {
    if (!elements.has(id)) { changeScope(null, id); return; }
    search.value = "";
    let parent = nodes.get(id)?.parentId;
    const seen = new Set();
    while (parent && !seen.has(parent)) { seen.add(parent); open.set(parent, true); parent = nodes.get(parent)?.parentId; }
    display(); select(id); focus(id); elements.get(id)?.scrollIntoView({ block: "nearest" });
  };
  const toggle = id => { open.set(id, !open.get(id)); display(); };
  const append = (list, item, depth = 1, invalid = false) => {
    if (rendered.has(item.id)) return;
    rendered.add(item.id);
    const branch = document.createElement("li");
    branch.className = "work-tree-item work-tree-node";
    branch.dataset.workNodeId = item.id;
    branch.setAttribute("role", "treeitem"); branch.setAttribute("aria-level", String(depth));
    branch.dataset.kind = item.kind;
    branch.dataset.status = item.status;
    branch.dataset.review = item.reviewState;
    elements.set(item.id, branch);
    if (!open.has(item.id)) open.set(item.id, item.expanded);
    if (prior?.known && !prior.known.has(item.id)) branch.classList.add("work-node-new");
    const summary = text("div", "work-tree-row", "");
    const descendants = children.get(item.id) || [];
    const caret = text("button", "work-tree-toggle", descendants.length ? "›" : "·");
    caret.type = "button"; caret.tabIndex = -1;
    caret.setAttribute("aria-label", t("展开或收起")); caret.disabled = !descendants.length;
    caret.onclick = event => { event.stopPropagation(); toggle(item.id); focus(item.id); };
    const status = item.statusLabel;
    branch.setAttribute("aria-label", `${item.title} · ${status}`);
    const glyph = text("span", "work-tree-glyph", item.glyph); glyph.setAttribute("aria-hidden", "true");
    const kind = text("span", "work-tree-kind", item.kindLabel);
    kind.title = item.kindLabel;
    const title = text("span", "work-tree-title", item.title); title.title = item.title;
    summary.append(caret, glyph, kind, title,
      text("span", "work-unit-status", status));
    if (descendants.length) summary.append(text("small", "work-tree-count", String(descendants.length)));
    summary.onclick = () => { select(item.id); focus(item.id); };
    branch.addEventListener("focus", () => { focused = item.id; });
    branch.addEventListener("keydown", event => {
      if (event.target !== branch) return;
      const rows = visible(), index = rows.indexOf(branch);
      let target;
      if (event.key === "ArrowDown") target = rows[Math.min(index + 1, rows.length - 1)];
      else if (event.key === "ArrowUp") target = rows[Math.max(index - 1, 0)];
      else if (event.key === "Home") target = rows[0];
      else if (event.key === "End") target = rows.at(-1);
      else if (event.key === "ArrowRight") { if (descendants.length && !open.get(item.id)) toggle(item.id); else target = descendants.map(child => elements.get(child.id)).find(el => rows.includes(el)); }
      else if (event.key === "ArrowLeft") { if (descendants.length && open.get(item.id)) toggle(item.id); else target = elements.get(item.parentId); }
      else if (event.key === "Enter" || event.key === " ") select(item.id);
      else return;
      event.preventDefault(); event.stopPropagation();
      if (target) { focus(target.dataset.workNodeId); target.scrollIntoView({ block: "nearest" }); }
    });
    branch.append(summary);
    if (invalid) branch.append(text("div", "work-unit-meta blocked", t("节点归属异常，请核对工作图")));
    if (descendants.length) {
      const nested = document.createElement("ul");
      nested.className = "work-tree-children"; nested.setAttribute("role", "group"); groups.set(item.id, nested);
      for (const child of descendants) append(nested, child, depth + 1);
      branch.append(nested);
    }
    list.append(branch);
  };
  if (scope) append(tree, nodes.get(scope));
  else for (const item of items.filter(item => !item.parentId || !ids.has(item.parentId))) append(tree, item, 1, Boolean(item.parentId && !ids.has(item.parentId)));
  // Invalid cycles must remain visible instead of silently dropping work from the tree.
  if (!scope) for (const item of items) if (!rendered.has(item.id)) append(tree, item, 1, true);
  if (!elements.has(selected)) selected = scope;
  if (!elements.has(focused)) focused = selected;
  const empty = text("li", "work-tree-empty dim", t("没有符合条件的条目"));
  empty.setAttribute("role", "none"); tree.append(empty);
  search.oninput = display;
  root.setQuery = value => { search.value = value; display(); };
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
