// Read-only projection: ownership determines layout; dependencies never invent a parent.
export function buildWorkMap(entries, snapshot = {}) {
  const allEntries = [...(snapshot.requirements || []), ...(snapshot.defects || []), ...entries];
  const catalog = new Map();
  const readiness = Object.assign({}, ...allEntries.map(entry => entry.work_readiness || {}));
  for (const entry of allEntries) catalog.set(entry.id, {
    id: entry.id, title: entry.title, status: entry.status, kind: entry.id.startsWith("D-") ? "defect" : "requirement",
    parent: null, dependencies: entry.dependencies || [], entry, unit: null,
  });
  const units = new Map((snapshot.work_units || []).map(unit => [unit.unit_id, unit]));
  for (const entry of allEntries) for (const unit of entry.work_units || []) units.set(unit.unit_id, unit);
  for (const unit of units.values()) catalog.set(unit.unit_id, {
    id: unit.unit_id, title: unit.objective, status: unit.status, kind: unit.node?.kind || "requirement",
    parent: unit.node?.parent_id || unit.requirement_id, dependencies: unit.dependencies || [],
    entry: catalog.get(unit.requirement_id)?.entry, unit, readiness: readiness[unit.unit_id],
  });
  const initial = new Set(entries.map(entry => entry.id));
  for (const unit of units.values()) if (initial.has(unit.requirement_id)) initial.add(unit.unit_id);
  const nodes = new Map();
  const include = id => {
    if (nodes.has(id)) return;
    const source = catalog.get(id);
    const node = source ? { ...source, context: !initial.has(id) } : {
      id, title: id, status: "unknown", kind: "unknown", parent: null, dependencies: [], context: true, unavailable: true,
    };
    nodes.set(id, node);
    if (node.parent) include(node.parent);
    for (const dependency of node.dependencies) include(dependency);
  };
  for (const id of initial) include(id);
  const edges = [], seen = new Set();
  const edge = (from, to, kind) => {
    const key = JSON.stringify([from, to, kind]);
    if (!seen.has(key)) { seen.add(key); edges.push({ from, to, kind }); }
  };
  for (const node of nodes.values()) {
    if (node.parent) edge(node.parent, node.id, "parent");
    // Arrow direction: a prerequisite result points to the work consuming it.
    for (const dependency of node.dependencies) edge(dependency, node.id, "dependency");
  }
  const parents = new Map([...nodes.values()].map(node => [node.id, node.parent]));
  const checked = new Set(), invalid = new Set();
  for (const id of nodes.keys()) {
    const path = new Set();
    let cursor = id;
    while (cursor && !checked.has(cursor)) {
      if (path.has(cursor)) {
        // Break only the presentation cycle. Preserve declared edges and flag it.
        parents.set(cursor, null);
        for (const member of path) invalid.add(member);
        break;
      }
      path.add(cursor); cursor = parents.get(cursor);
    }
    for (const member of path) checked.add(member);
  }
  const children = new Map([[null, []]]);
  for (const node of nodes.values()) {
    const parent = parents.get(node.id) || null;
    if (!children.has(parent)) children.set(parent, []);
    children.get(parent).push(node.id);
  }
  const positions = new Map();
  let row = 0, maxDepth = 0;
  const place = (id, depth) => {
    maxDepth = Math.max(maxDepth, depth);
    const descendants = children.get(id) || [];
    const ys = descendants.map(child => place(child, depth + 1));
    const y = ys.length ? (ys[0] + ys.at(-1)) / 2 : 24 + row++ * 138;
    positions.set(id, { x: 24 + depth * 292, y });
    return y;
  };
  for (const root of children.get(null)) { place(root, 0); row += .35; }
  return { nodes, edges, children, positions, invalid, width: 48 + maxDepth * 292 + 236, height: Math.max(190, row * 138 + 24) };
}
