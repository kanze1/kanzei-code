// 记忆图谱纯函数模型(零 import:node 可直接测,见 scripts/ui-memory-graph-smoke.mjs)。
//
// 输入是后端 memory_graph 的载荷(crates/kanzei-tools/src/refgraph/memory_graph.rs):
//   { areas:[{id,kind,label,parent,depth,band,memories}], nodes:[GraphNode], edges:[GraphEdge], stats, warnings }
// 这里只做「看哪些、怎么摆、怎么画」的计算,不碰 DOM、不碰画布。设计见 docs/design/memory_knowledge_graph.md §8。

// 关系 → 中文文案 key(边标签、详情分组、图例都走 t(REL_KEYS[rel]),英文在 02-i18n.js 的 I18N_EN)。
export const REL_KEYS = {
  refs: "关联",
  basis: "依据",
  implements: "实现",
  supersedes: "取代",
  derived_from: "来源",
  mentions: "提及",
  cites: "引用",
  about: "关于",
  has_fingerprint: "指纹",
  has_subject: "同主题",
  contains: "包含",
  depends_on: "依赖",
};

// 区域依据徽标。
export const PROVENANCE_KEYS = { field: "字段", path: "路径", tool: "工具", via: "经由", keyword: "关键词" };

// 节点种类 → 中文文案 key。
export const KIND_KEYS = {
  memory: "记忆",
  requirement: "需求",
  defect: "缺陷",
  decision: "决策",
  doc: "设计文档",
  crate: "代码区域",
  module: "模块",
  fingerprint: "失败指纹",
  subject: "同主题",
};

// 图层:用户可开关的关系组。contains 是骨架,总在;depends_on 由层带位置表达,不画线。
export const LAYERS = ["about", "refs", "supersedes", "concepts", "mentions"];
export const LAYER_KEYS = { about: "关于", refs: "关联", supersedes: "取代", concepts: "指纹", mentions: "提及" };
export const DEFAULT_LAYERS = Object.freeze(["about", "refs", "supersedes", "concepts"]);
const LAYER_OF = {
  about: "about",
  refs: "refs",
  basis: "refs",
  implements: "refs",
  derived_from: "refs",
  supersedes: "supersedes",
  has_fingerprint: "concepts",
  has_subject: "concepts",
  mentions: "mentions",
  cites: "mentions",
  contains: "structure",
  depends_on: "hidden",
};
export function layerOf(rel) {
  return LAYER_OF[rel] ?? "refs";
}

const STALE = new Set(["deprecated", "invalid", "stale"]);
const endId = (end) => (end && typeof end === "object" ? end.id : end);

export function crateOfArea(areaId, areasById) {
  let current = areasById.get(areaId);
  let guard = 0;
  while (current && current.kind !== "crate" && current.parent && guard < 8) {
    current = areasById.get(current.parent);
    guard += 1;
  }
  return current?.kind === "crate" ? current.id : null;
}

/** 节点 → 所在 crate 级区域(外壳分组、聚类锚点用)。 */
export function crateOfNode(node, areasById) {
  if (!node) return null;
  if (node.kind === "crate") return node.id;
  if (node.kind === "module") return crateOfArea(node.id, areasById);
  if (node.kind === "memory") return node.primary_area ? crateOfArea(node.primary_area, areasById) : null;
  return null;
}

function memoryMatches(node, filters) {
  const scope = filters.scope ?? "all";
  if (scope !== "all" && node.scope !== scope) return false;
  const category = filters.category ?? "all";
  if (category !== "all" && node.category !== category) return false;
  // 归档条目只由「含归档」开关决定(归档里几乎都是 deprecated,跟着状态筛选走的话默认「active」下永远看不到);
  // 状态筛选只作用于活动目录里的条目。
  if (node.archived) {
    if (!filters.archived) return false;
  } else {
    const status = filters.status ?? "all";
    if (status === "stale") {
      if (!STALE.has(node.status)) return false;
    } else if (status !== "all" && node.status !== status) return false;
  }
  if (filters.area) {
    const inArea = (node.areas ?? []).some((a) => a === filters.area || a.startsWith(`${filters.area}/`));
    if (!inArea) return false;
  }
  return true;
}

/**
 * 按筛选取可见子图。
 * filters: { scope:"project"|"global"|"all", category, status:"active"|"candidate"|"shadow"|"stale"|"all",
 *            archived:boolean, layers:Iterable<string>, area:""|"area:…" }
 * 状态筛选只管活动目录里的记忆;归档记忆只看 archived 开关。
 * 规则:记忆按筛选;crate 级区域只在它(或它的模块、它的子区域)有可见记忆时出现——空 crate 占掉整条层带,
 * 把真正有内容的地方挤成一团;模块只在有可见记忆关于它时出现;需求/缺陷/决策/文档只在与可见记忆
 * 相连(且那条边的图层开着)时出现;概念节点要 ≥2 条可见记忆共享才出现。
 * 返回 { nodes, links }(元素是载荷里的原对象,调用方自己克隆)。
 */
export function visibleGraph(payload, filters = {}) {
  const nodes = payload?.nodes ?? [];
  const edges = payload?.edges ?? [];
  const layers = new Set(filters.layers ?? DEFAULT_LAYERS);
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const visible = new Set();
  for (const node of nodes) {
    if (node.kind === "memory" && memoryMatches(node, filters)) visible.add(node.id);
  }
  // 有可见记忆的区域 → 它的 crate 级祖先链都出现(kanzei-app/ui 有记忆时 kanzei-app 也在)。
  const areasById = new Map((payload?.areas ?? []).map((a) => [a.id, a]));
  for (const node of nodes) {
    if (!visible.has(node.id) || node.kind !== "memory") continue;
    for (const area of node.areas ?? []) {
      let current = areasById.get(area);
      let guard = 0;
      while (current && guard < 8) {
        if (current.kind === "crate" && byId.has(current.id)) visible.add(current.id);
        current = current.parent ? areasById.get(current.parent) : null;
        guard += 1;
      }
    }
  }
  const layerOn = (rel) => {
    const layer = layerOf(rel);
    return layer === "structure" || layers.has(layer);
  };
  const conceptUse = new Map();
  for (const edge of edges) {
    const source = endId(edge.source);
    const target = endId(edge.target);
    if (!visible.has(source) || byId.get(source)?.kind !== "memory") continue;
    const other = byId.get(target);
    if (!other || other.kind === "memory" || other.kind === "crate") continue;
    if (edge.rel === "about" && !layers.has("about")) continue;
    if (!layerOn(edge.rel)) continue;
    if (other.kind === "fingerprint" || other.kind === "subject") {
      conceptUse.set(target, (conceptUse.get(target) ?? 0) + 1);
      continue;
    }
    visible.add(target);
  }
  for (const [id, count] of conceptUse) if (count >= 2) visible.add(id);
  const links = edges.filter((edge) => {
    const source = endId(edge.source);
    const target = endId(edge.target);
    return visible.has(source) && visible.has(target) && layerOf(edge.rel) !== "hidden" && layerOn(edge.rel);
  });
  return { nodes: nodes.filter((n) => visible.has(n.id)), links };
}

/**
 * N 跳邻域:从中心沿可见图层的边双向走 hops 跳。contains/depends_on 不参与遍历(否则一跳就把整个 crate
 * 拉进来);例外:中心本身是 crate 时允许沿 contains 走一跳(看这个 crate 下有哪些模块、再到记忆)。
 * 返回 { nodes, links, hopOf: Map(id → 跳数) }。
 */
export function egoSubgraph(payload, center, hops = 2, layers = DEFAULT_LAYERS) {
  const nodes = payload?.nodes ?? [];
  const edges = payload?.edges ?? [];
  const layerSet = new Set(layers);
  const byId = new Map(nodes.map((n) => [n.id, n]));
  if (!byId.has(center)) return { nodes: [], links: [], hopOf: new Map() };
  const walkable = (edge, from) => {
    if (edge.rel === "depends_on") return false;
    if (edge.rel === "contains") return from === center && byId.get(center)?.kind === "crate";
    return layerSet.has(layerOf(edge.rel));
  };
  const adjacency = new Map();
  const push = (a, b, edge) => {
    if (!adjacency.has(a)) adjacency.set(a, []);
    adjacency.get(a).push([b, edge]);
  };
  for (const edge of edges) {
    push(endId(edge.source), endId(edge.target), edge);
    push(endId(edge.target), endId(edge.source), edge);
  }
  const hopOf = new Map([[center, 0]]);
  let frontier = [center];
  for (let depth = 1; depth <= hops; depth += 1) {
    const next = [];
    for (const id of frontier) {
      for (const [neighbor, edge] of adjacency.get(id) ?? []) {
        if (hopOf.has(neighbor) || !walkable(edge, id)) continue;
        hopOf.set(neighbor, depth);
        next.push(neighbor);
      }
    }
    frontier = next;
  }
  const links = edges.filter((edge) => {
    const source = endId(edge.source);
    const target = endId(edge.target);
    if (!hopOf.has(source) || !hopOf.has(target)) return false;
    if (edge.rel === "depends_on") return false;
    if (edge.rel === "contains") return byId.get(center)?.kind === "crate" && (source === center || target === center);
    return layerSet.has(layerOf(edge.rel));
  });
  return { nodes: nodes.filter((n) => hopOf.has(n.id)), links, hopOf };
}

// 层带布局常量:入口层在上,基础层在下(依赖向下)。
export const BANDS = 4;
export const BAND_H = 240;
export const COL_W = 360;
const finite = (value) => (Number.isFinite(value) ? value : 0);

/**
 * crate 级区域 → 层带锚点。band 3(入口)在最上;同带按 id 排序后居中排开;没有 crate 的层带不占行
 * (上下顺序不变,只是压紧)。调用方传当前可见的 crate 级区域。
 * 带号非数字按 0 带处理;空输入的未归类锚点回退为 0——d3 四叉树遇到 Infinity/NaN 会死循环
 * (原型实测:空数组 Math.max → -Infinity)。
 * 返回 { anchors: Map(id → {x,y}), unassigned: {x,y} }(未归类记忆的锚点在右上角)。
 */
export function bandAnchors(areas) {
  const crates = (areas ?? []).filter((a) => a && a.kind === "crate").map((a) => ({ id: a.id, band: Math.max(0, Math.min(BANDS - 1, Math.round(finite(Number(a.band))))) }));
  crates.sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  const rows = new Map();
  for (const crate of crates) {
    if (!rows.has(crate.band)) rows.set(crate.band, []);
    rows.get(crate.band).push(crate.id);
  }
  const anchors = new Map();
  const order = [...rows.keys()].sort((a, b) => b - a);
  order.forEach((band, row) => {
    const ids = rows.get(band);
    ids.forEach((id, index) => {
      const x = (index - (ids.length - 1) / 2) * COL_W;
      const y = row * BAND_H - ((order.length - 1) * BAND_H) / 2;
      anchors.set(id, { x, y });
    });
  });
  const xs = [...anchors.values()].map((a) => a.x);
  // 没有可见 crate 时 Math.max() 是 -Infinity:喂给 d3 四叉树会死循环(原型实测卡死),这里必须回退为有限值。
  const right = xs.length ? Math.max(...xs) + COL_W * 0.8 : 0;
  const unassigned = { x: right, y: -((Math.max(order.length, 1) - 1) * BAND_H) / 2 };
  return { anchors, unassigned };
}

export function nodeRadius(node) {
  if (node.kind === "crate") return 15;
  if (node.kind === "module") return 8;
  if (node.kind === "memory") return 6 + Math.min(6, 1.5 * Math.log2(1 + Math.max(0, Number(node.hits) || 0)));
  return 5.5;
}

const GLYPH = { requirement: "R", defect: "D", decision: "A", doc: "§" };

/**
 * 节点外观(调色板 key 而不是颜色值,颜色由渲染器从 --graph-* token 取):
 * { shape: "circle"|"diamond", color: key, fill: bool, dashed: bool, alpha, glyph }
 * 记忆:按分类取色;active 实心,candidate/shadow 空心环,deprecated/invalid/归档 半透明 + 虚线环。
 */
export function nodeStyle(node) {
  if (node.kind === "memory") {
    const color = ["fact", "sop", "habit", "preference"].includes(node.category) ? node.category : "context";
    const faded = node.archived || STALE.has(node.status);
    return {
      shape: "circle",
      color,
      fill: !faded && node.status === "active",
      dashed: faded,
      alpha: faded ? 0.4 : 1,
      glyph: null,
    };
  }
  if (node.kind === "fingerprint" || node.kind === "subject") {
    return { shape: "diamond", color: "concept", fill: false, dashed: false, alpha: 1, glyph: null };
  }
  if (node.kind === "crate") return { shape: "circle", color: "crate", fill: true, dashed: false, alpha: 1, glyph: null };
  if (node.kind === "module") return { shape: "circle", color: "module", fill: true, dashed: false, alpha: 1, glyph: null };
  return { shape: "circle", color: "context", fill: true, dashed: false, alpha: node.archived ? 0.55 : 1, glyph: GLYPH[node.kind] ?? null };
}

/** 单调链凸包:[[x,y]…] → 逆时针顶点序列(共线点剔除)。 */
export function convexHull(points) {
  const pts = [...(points ?? [])].filter((p) => Number.isFinite(p[0]) && Number.isFinite(p[1])).sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  if (pts.length < 3) return pts;
  const cross = (o, a, b) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const lower = [];
  for (const p of pts) {
    while (lower.length >= 2 && cross(lower[lower.length - 2], lower[lower.length - 1], p) <= 0) lower.pop();
    lower.push(p);
  }
  const upper = [];
  for (const p of [...pts].reverse()) {
    while (upper.length >= 2 && cross(upper[upper.length - 2], upper[upper.length - 1], p) <= 0) upper.pop();
    upper.push(p);
  }
  return lower.slice(0, -1).concat(upper.slice(0, -1));
}

/**
 * 屏幕空间标签去重叠:按优先级贪心放置,与已放置的框重叠就剔除。网格分桶加速(cellPx 一格)。
 * cands: [{ id, x, y, w, h, priority }](x,y 为标签框中心),返回放下的 id 集合。
 */
export function placeLabels(cands, cellPx = 64) {
  const cell = Math.max(8, Number(cellPx) || 64);
  const sorted = [...(cands ?? [])].filter((c) => Number.isFinite(c.x) && Number.isFinite(c.y)).sort((a, b) => (b.priority ?? 0) - (a.priority ?? 0));
  const grid = new Map();
  const placed = new Set();
  const key = (i, j) => `${i},${j}`;
  for (const cand of sorted) {
    const x0 = cand.x - cand.w / 2;
    const x1 = cand.x + cand.w / 2;
    const y0 = cand.y - cand.h / 2;
    const y1 = cand.y + cand.h / 2;
    const i0 = Math.floor(x0 / cell);
    const i1 = Math.floor(x1 / cell);
    const j0 = Math.floor(y0 / cell);
    const j1 = Math.floor(y1 / cell);
    let clash = false;
    for (let i = i0; i <= i1 && !clash; i += 1) {
      for (let j = j0; j <= j1 && !clash; j += 1) {
        for (const box of grid.get(key(i, j)) ?? []) {
          if (x0 < box[2] && x1 > box[0] && y0 < box[3] && y1 > box[1]) {
            clash = true;
            break;
          }
        }
      }
    }
    if (clash) continue;
    placed.add(cand.id);
    const box = [x0, y0, x1, y1];
    for (let i = i0; i <= i1; i += 1) {
      for (let j = j0; j <= j1; j += 1) {
        const k = key(i, j);
        if (!grid.has(k)) grid.set(k, []);
        grid.get(k).push(box);
      }
    }
  }
  return placed;
}

/** 检索:按 id、标题、标签、区域(不含 area: 前缀)不分大小写匹配;记忆排前。 */
export function searchNodes(payload, query) {
  const q = String(query ?? "").trim().toLowerCase();
  if (!q) return [];
  const hits = [];
  for (const node of payload?.nodes ?? []) {
    const hay = [node.id, node.title, node.label, ...(node.areas ?? []).map((a) => a.replace(/^area:/, ""))]
      .filter(Boolean)
      .join("\n")
      .toLowerCase();
    if (hay.includes(q)) hits.push(node);
  }
  hits.sort((a, b) => (a.kind === "memory" ? 0 : 1) - (b.kind === "memory" ? 0 : 1) || String(a.id).localeCompare(String(b.id), undefined, { numeric: true }));
  return hits.map((n) => n.id);
}

/**
 * 文本视图(键盘/读屏兜底):可见记忆按 crate → 模块分组。
 * 返回 [{ id, label, memories:[node], modules:[{ id, label, memories:[node] }] }],未归类的记忆在 id 为 "" 的组。
 */
export function groupForTextView(visibleNodes, payload) {
  const areasById = new Map((payload?.areas ?? []).map((a) => [a.id, a]));
  const groups = new Map();
  const groupFor = (crateId) => {
    const id = crateId ?? "";
    if (!groups.has(id)) {
      const area = areasById.get(id);
      groups.set(id, { id, label: area ? area.id.replace(/^area:/, "") : "", memories: [], modules: new Map() });
    }
    return groups.get(id);
  };
  for (const node of visibleNodes ?? []) {
    if (node.kind !== "memory") continue;
    const primary = node.primary_area;
    const crateId = primary ? crateOfArea(primary, areasById) : null;
    const group = groupFor(crateId);
    const area = primary ? areasById.get(primary) : null;
    if (area && area.kind === "module") {
      if (!group.modules.has(primary)) group.modules.set(primary, { id: primary, label: area.label, memories: [] });
      group.modules.get(primary).memories.push(node);
    } else {
      group.memories.push(node);
    }
  }
  const byId = (a, b) => String(a.id).localeCompare(String(b.id), undefined, { numeric: true });
  return [...groups.values()]
    .map((group) => ({
      id: group.id,
      label: group.label,
      memories: group.memories.sort(byId),
      modules: [...group.modules.values()].map((m) => ({ ...m, memories: m.memories.sort(byId) })).sort(byId),
    }))
    .sort((a, b) => (a.id === "" ? 1 : b.id === "" ? -1 : a.id.localeCompare(b.id)));
}

/**
 * 确定性抖动:节点 id → [dx, dy],各在 [-0.5, 0.5)。渲染器给新节点定初始位置用它代替 Math.random,
 * 同一份数据每次打开都排出同一张图(d3-force 自己的随机源是固定种子的 LCG,初始位置定了结果就定了)。
 * FNV-1a 32 位哈希后过 murmur3 的 fmix32 雪崩(裸 FNV-1a 对 M-001/M-002 这类只差末位的 id 高位几乎不变,
 * 连号记忆会排成一条斜线),x、y 用两个不同的种子各混一次。
 */
export function seededJitter(id) {
  const text = String(id ?? "");
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i += 1) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  const fmix = (value) => {
    let k = value >>> 0;
    k ^= k >>> 16;
    k = Math.imul(k, 0x85ebca6b) >>> 0;
    k ^= k >>> 13;
    k = Math.imul(k, 0xc2b2ae35) >>> 0;
    k ^= k >>> 16;
    return k >>> 0;
  };
  return [fmix(h) / 4294967296 - 0.5, fmix(h ^ 0x9e3779b9) / 4294967296 - 0.5];
}

/** 邻居表:id → Set(相邻 id)(悬停高亮用)。 */
export function neighborsOf(links) {
  const map = new Map();
  for (const link of links ?? []) {
    const a = endId(link.source);
    const b = endId(link.target);
    if (!map.has(a)) map.set(a, new Set());
    if (!map.has(b)) map.set(b, new Set());
    map.get(a).add(b);
    map.get(b).add(a);
  }
  return map;
}
