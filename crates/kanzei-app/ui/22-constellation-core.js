// UI2-0926 #10 对话背景(星座背景)的纯函数:零 import、零 DOM,node 冒烟可直接 import
// (scripts/ui-constellation-smoke.mjs 另以 data: URL 导入改过的源码做变异)。设计见 docs/design/ui_chat_backdrop.md。
//
// 坐标约定:模型内一律归一化到 [0,1]²(长边 = 1 - 2·pad,短边居中),y 向下;渲染器只做「模型 → 目标框」
// 的仿射映射(X = cx + (x - .5)·S,S = 框长边),窗口尺寸变化不重建拓扑——旧神经场每次 resize 都换一张网。
// 模型形状:{ v, kind, aspect, points: [[x, y, mag], …], edges: [[i, j, role], …], hub }。
// mag 是视星等(越小越亮);role:0 桥接/无角色,1 主干 trunk,2 记忆 memory,3 行动 action;hub 为 -1 表示无汇聚点。

export const MODEL_VERSION = 1;
export const MAX_POINTS = 200;
export const MAX_EDGES = 400;
export const ROLE = { bridge: 0, trunk: 1, memory: 2, action: 3 };

export function seededRandom(seed) {
  let value = seed >>> 0;
  return () => {
    value += 0x6d2b79f5;
    let mixed = value;
    mixed = Math.imul(mixed ^ (mixed >>> 15), mixed | 1);
    mixed ^= mixed + Math.imul(mixed ^ (mixed >>> 7), mixed | 61);
    return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296;
  };
}

const dist = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);
const edgeKey = (i, j) => (i < j ? `${i}:${j}` : `${j}:${i}`);
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const round = (v, digits) => Math.round(v * 10 ** digits) / 10 ** digits;

// ---------- 几何 ----------
function orient(a, b, c) { return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]); }
// 两条线段是否在内部相交(共享端点不算)。
export function segmentsCross(a, b, c, d) {
  const o1 = orient(a, b, c), o2 = orient(a, b, d), o3 = orient(c, d, a), o4 = orient(c, d, b);
  return ((o1 > 0 && o2 < 0) || (o1 < 0 && o2 > 0)) && ((o3 > 0 && o4 < 0) || (o3 < 0 && o4 > 0));
}
function projectOnSegment(p, a, b) {
  const dx = b[0] - a[0], dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy || 1;
  const t = clamp(((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2, 0, 1);
  const q = [a[0] + dx * t, a[1] + dy * t];
  return { t, point: q, distance: dist(p, q) };
}

// ---------- 图 ----------
// 欧氏最小生成树(Prim,O(n²);n ≤ 200 时约 4 万次运算)。返回 [[i, j, len], …]。
export function euclideanMst(points) {
  const n = points.length;
  if (n < 2) return [];
  const inTree = new Uint8Array(n);
  const best = new Float64Array(n).fill(Infinity);
  const parent = new Int32Array(n).fill(-1);
  best[0] = 0;
  const edges = [];
  for (let k = 0; k < n; k += 1) {
    let u = -1;
    for (let i = 0; i < n; i += 1) if (!inTree[i] && (u < 0 || best[i] < best[u])) u = i;
    inTree[u] = 1;
    if (parent[u] >= 0) edges.push([parent[u], u, best[u]]);
    for (let v = 0; v < n; v += 1) {
      if (inTree[v]) continue;
      const d = dist(points[u], points[v]);
      if (d < best[v]) { best[v] = d; parent[v] = u; }
    }
  }
  return edges;
}

function median(values) {
  if (!values.length) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

// 连通分量(并查集)。
export function components(n, edges) {
  const parent = Array.from({ length: n }, (_, i) => i);
  const find = (x) => { while (parent[x] !== x) { parent[x] = parent[parent[x]]; x = parent[x]; } return x; };
  for (const [i, j] of edges) parent[find(i)] = find(j);
  const groups = new Map();
  for (let i = 0; i < n; i += 1) {
    const root = find(i);
    if (!groups.has(root)) groups.set(root, []);
    groups.get(root).push(i);
  }
  return [...groups.values()];
}

// 在已有边之外补「像星座」的边:只连近邻、不与已有边交叉、与同端点已有边夹角 ≥ minAngle、总数 ≤ cap·n。
export function augmentEdges(points, edges, { k = 2, maxFactor = 1.25, minAngleDeg = 24, cap = 1.3 } = {}) {
  const n = points.length;
  const out = edges.map(([i, j]) => [i, j]);
  const seen = new Set(out.map(([i, j]) => edgeKey(i, j)));
  const limit = median(out.map(([i, j]) => dist(points[i], points[j]))) * maxFactor;
  const minCos = Math.cos((minAngleDeg * Math.PI) / 180);
  const adjacency = Array.from({ length: n }, () => []);
  for (const [i, j] of out) { adjacency[i].push(j); adjacency[j].push(i); }
  const angleOk = (i, j) => adjacency[i].every((m) => {
    const ax = points[j][0] - points[i][0], ay = points[j][1] - points[i][1];
    const bx = points[m][0] - points[i][0], by = points[m][1] - points[i][1];
    return (ax * bx + ay * by) / ((Math.hypot(ax, ay) * Math.hypot(bx, by)) || 1) < minCos;
  });
  const candidates = [];
  for (let i = 0; i < n; i += 1) {
    const near = [];
    for (let j = 0; j < n; j += 1) if (j !== i) near.push([j, dist(points[i], points[j])]);
    near.sort((a, b) => a[1] - b[1]);
    for (const [j, d] of near.slice(0, k)) if (d <= limit) candidates.push([i, j, d]);
  }
  candidates.sort((a, b) => a[2] - b[2]);
  const maxEdges = Math.min(MAX_EDGES, Math.round(n * cap));
  for (const [i, j] of candidates) {
    if (out.length >= maxEdges) break;
    const key = edgeKey(i, j);
    if (seen.has(key) || !angleOk(i, j) || !angleOk(j, i)) continue;
    if (out.some(([a, b]) => a !== i && a !== j && b !== i && b !== j && segmentsCross(points[i], points[j], points[a], points[b]))) continue;
    seen.add(key);
    out.push([i, j]);
    adjacency[i].push(j); adjacency[j].push(i);
  }
  return out;
}

// 从最亮的星出发的 BFS:画线入场的顺序、完成波的层次都用它。depth[e] 是边所在层(从 1 起)。
export function bfsOrder(model) {
  const n = model.points.length;
  const adjacency = Array.from({ length: n }, () => []);
  model.edges.forEach(([i, j], index) => { adjacency[i].push([j, index]); adjacency[j].push([i, index]); });
  const byBrightness = [...model.points.keys()].sort((a, b) => model.points[a][2] - model.points[b][2]);
  const visited = new Uint8Array(n);
  const edgeDone = new Uint8Array(model.edges.length);
  const edgeOrder = [];
  const depth = new Array(model.edges.length).fill(0);
  for (const root of byBrightness) {
    if (visited[root] || !adjacency[root].length) continue;
    visited[root] = 1;
    const queue = [[root, 0]];
    while (queue.length) {
      const [u, d] = queue.shift();
      for (const [v, e] of adjacency[u]) {
        if (edgeDone[e]) continue;
        edgeDone[e] = 1;
        depth[e] = d + 1;
        edgeOrder.push(e);
        if (!visited[v]) { visited[v] = 1; queue.push([v, d + 1]); }
      }
    }
  }
  return { edgeOrder, depth, root: byBrightness[0] ?? 0 };
}

// 以 start 为源的 BFS 层:完成波从 hub 按层荡开。返回每条边的层(从 1 起;不连通的边为 0)。
export function edgeDepthsFrom(model, start) {
  const n = model.points.length;
  const depth = new Array(model.edges.length).fill(0);
  if (!(start >= 0 && start < n)) return depth;
  const adjacency = Array.from({ length: n }, () => []);
  model.edges.forEach(([i, j], index) => { adjacency[i].push([j, index]); adjacency[j].push([i, index]); });
  const level = new Int32Array(n).fill(-1);
  level[start] = 0;
  const queue = [start];
  while (queue.length) {
    const u = queue.shift();
    for (const [v, e] of adjacency[u]) {
      if (!depth[e]) depth[e] = level[u] + 1;
      if (level[v] < 0) { level[v] = level[u] + 1; queue.push(v); }
    }
  }
  return depth;
}

// ---------- 归一化 ----------
// 任意坐标系的点放进单位框:长边 = 1 - 2·pad,短边居中。返回 aspect = 宽/高(供构图按比例取框)。
export function normalizePoints(raw, pad = 0.04) {
  const xs = raw.map((p) => p[0]), ys = raw.map((p) => p[1]);
  const minX = Math.min(...xs), maxX = Math.max(...xs), minY = Math.min(...ys), maxY = Math.max(...ys);
  const w = Math.max(1e-9, maxX - minX), h = Math.max(1e-9, maxY - minY);
  const scale = (1 - 2 * pad) / Math.max(w, h);
  const offX = (1 - w * scale) / 2, offY = (1 - h * scale) / 2;
  return {
    points: raw.map((p) => [round((p[0] - minX) * scale + offX, 3), round((p[1] - minY) * scale + offY, 3), ...p.slice(2)]),
    aspect: w / h,
  };
}

// ---------- (b) 真实星座:切平面(gnomonic)投影 ----------
// 投影中心取各星赤道坐标单位向量的均值(跨 0h 也不会错);x = -ξ(东在左),y = -η(北在上,屏幕 y 向下)。
export function projectStars(stars) {
  const toRad = Math.PI / 180;
  const ra = stars.map((s) => s[2] * 15 * toRad), dec = stars.map((s) => s[3] * toRad);
  let cx = 0, cy = 0, cz = 0;
  for (let i = 0; i < stars.length; i += 1) {
    cx += Math.cos(dec[i]) * Math.cos(ra[i]); cy += Math.cos(dec[i]) * Math.sin(ra[i]); cz += Math.sin(dec[i]);
  }
  const ra0 = Math.atan2(cy, cx), dec0 = Math.atan2(cz, Math.hypot(cx, cy));
  return stars.map((s, i) => {
    const cosc = Math.sin(dec0) * Math.sin(dec[i]) + Math.cos(dec0) * Math.cos(dec[i]) * Math.cos(ra[i] - ra0);
    const xi = (Math.cos(dec[i]) * Math.sin(ra[i] - ra0)) / cosc;
    const eta = (Math.cos(dec0) * Math.sin(dec[i]) - Math.sin(dec0) * Math.cos(dec[i]) * Math.cos(ra[i] - ra0)) / cosc;
    return [-xi, -eta, s[4]];
  });
}

export function starPresetModel(preset) {
  const index = new Map(preset.stars.map((s, i) => [s[0], i]));
  const { points, aspect } = normalizePoints(projectStars(preset.stars), 0.06);
  return {
    v: MODEL_VERSION,
    kind: "stars",
    aspect,
    points: points.map(([x, y, mag]) => [x, y, round(mag, 1)]),
    edges: preset.edges.map(([a, b]) => [index.get(a), index.get(b), ROLE.bridge]),
    hub: -1,
  };
}

// ---------- (a) 矢量笔画 → 星座 ----------
// 端点是亮星;端点离别的笔画 ≤ junction 时在对方笔画上插一颗「T 字交汇」星并连桥(近平行的笔画不桥接,
// 否则标志的三条电路臂会被连成锯齿);笔画内部按 spacing(±20% 抖动)补暗星;距离 < merge 的星合并;
// 剩余分量用最短跨分量边桥接(≤ bridge)。边带语义角色,hub 取离 source.hub 最近的星。
export function strokesToConstellation(source, { seed = 1, density = 1 } = {}) {
  const rand = seededRandom(seed);
  const size = source.viewBox;
  const merge = size * (source.mergeTolerance ?? 0.06), junction = size * (source.junctionTolerance ?? 0.14), bridge = size * 0.3;
  const spacing = (size * 0.15) / clamp(density, 0.5, 2);
  const strokes = source.strokes.map((s) => ({ ...s, keys: [{ t: 0 }, { t: 1 }] }));
  const bridges = [];
  strokes.forEach((s, si) => {
    for (const end of [0, 1]) {
      const p = end ? s.points.at(-1) : s.points[0];
      strokes.forEach((o, oi) => {
        if (oi === si) return;
        const u = [s.points.at(-1)[0] - s.points[0][0], s.points.at(-1)[1] - s.points[0][1]];
        const v = [o.points.at(-1)[0] - o.points[0][0], o.points.at(-1)[1] - o.points[0][1]];
        if (Math.abs(u[0] * v[0] + u[1] * v[1]) / ((Math.hypot(...u) * Math.hypot(...v)) || 1) > Math.cos(Math.PI / 9)) return;
        const hit = projectOnSegment(p, o.points[0], o.points.at(-1));
        if (hit.distance > junction || hit.t < 0.08 || hit.t > 0.92) return;
        o.keys.push({ t: hit.t });
        bridges.push({ from: [si, end], to: [oi, hit.t] });
      });
    }
  });
  const raw = [];
  const strokeStars = strokes.map((s) => {
    const a = s.points[0], b = s.points.at(-1);
    const length = dist(a, b);
    const ts = [...new Set(s.keys.map((k) => round(k.t, 3)))].sort((x, y) => x - y);
    const all = [];
    for (let k = 0; k < ts.length; k += 1) {
      all.push({ t: ts[k], key: true });
      if (k === ts.length - 1) break;
      const count = Math.max(0, Math.round(((ts[k + 1] - ts[k]) * length) / spacing) - 1);
      for (let m = 1; m <= count; m += 1) {
        const jitter = ((rand() - 0.5) * 0.4) / (count + 1);
        all.push({ t: ts[k] + (ts[k + 1] - ts[k]) * (m / (count + 1) + jitter), key: false });
      }
    }
    const nx = -(b[1] - a[1]) / (length || 1), ny = (b[0] - a[0]) / (length || 1);
    return all.map(({ t, key }) => {
      const off = key ? 0 : (rand() - 0.5) * size * 0.018;
      const mag = key ? 1.4 + (2 - s.weight) * 0.5 + rand() * 0.4 : 2.6 + (2 - s.weight) * 0.7 + rand() * 1.1;
      raw.push([a[0] + (b[0] - a[0]) * t + nx * off, a[1] + (b[1] - a[1]) * t + ny * off, mag]);
      return { index: raw.length - 1, t };
    });
  });
  // 合并近邻(保留更亮者的位置)。
  const alias = raw.map((_, i) => i);
  const resolve = (i) => { while (alias[i] !== i) i = alias[i]; return i; };
  for (let i = 0; i < raw.length; i += 1) {
    for (let j = i + 1; j < raw.length; j += 1) {
      const ri = resolve(i), rj = resolve(j);
      if (ri === rj || dist(raw[ri], raw[rj]) >= merge) continue;
      const [keep, drop] = raw[ri][2] <= raw[rj][2] ? [ri, rj] : [rj, ri];
      alias[drop] = keep;
    }
  }
  const edges = new Map();
  const addEdge = (i, j, role = ROLE.bridge) => { const a = resolve(i), b = resolve(j); if (a !== b) edges.set(edgeKey(a, b), [a, b, role]); };
  strokeStars.forEach((stars, si) => {
    for (let k = 1; k < stars.length; k += 1) addEdge(stars[k - 1].index, stars[k].index, ROLE[strokes[si].role] ?? ROLE.bridge);
  });
  for (const { from, to } of bridges) {
    const fromStars = strokeStars[from[0]];
    const a = from[1] ? fromStars.at(-1).index : fromStars[0].index;
    const target = strokeStars[to[0]].reduce((best, s) => (Math.abs(s.t - to[1]) < Math.abs(best.t - to[1]) ? s : best));
    addEdge(a, target.index);
  }
  const keepIds = [...new Set(raw.map((_, i) => resolve(i)))];
  const remap = new Map(keepIds.map((id, i) => [id, i]));
  const points = keepIds.map((id) => raw[id]);
  const edgeList = [...edges.values()].map(([a, b, role]) => [remap.get(a), remap.get(b), role]);
  for (;;) {
    const groups = components(points.length, edgeList);
    if (groups.length < 2) break;
    const owner = new Int32Array(points.length);
    groups.forEach((g, gi) => g.forEach((i) => { owner[i] = gi; }));
    let best = null;
    for (let i = 0; i < points.length; i += 1) for (let j = i + 1; j < points.length; j += 1) {
      if (owner[i] === owner[j]) continue;
      const d = dist(points[i], points[j]);
      if (!best || d < best[2]) best = [i, j, d];
    }
    if (!best || best[2] > bridge) break;
    edgeList.push([best[0], best[1], ROLE.bridge]);
  }
  let hub = -1;
  if (source.hub) hub = points.reduce((best, p, i) => (best < 0 || dist(p, source.hub) < dist(points[best], source.hub) ? i : best), -1);
  const normalized = normalizePoints(points, 0.05);
  return {
    v: MODEL_VERSION,
    kind: "vector",
    aspect: normalized.aspect,
    points: normalized.points.map(([x, y, mag]) => [x, y, round(mag, 1)]),
    edges: edgeList,
    hub,
  };
}

// ---------- (c) 图片 → 星座 ----------
// 输入 ImageData 形状 {width, height, data(RGBA)}(调用方已缩到长边 ≤ 192)。只返回点集,不留像素。
// 1 墨迹 ink:透明图用 alpha;不透明图用与边框中位亮度之差(深底浅字、浅底深字都成立),按 p95 归一。
// 2 判型:墨迹二值化后做倒角距离变换估笔画宽。笔画宽(4×墨迹平均距离)< 短边 14% 且墨迹 < 50% → 笔画型
//   (标志/文字/线稿):显著度取中轴线,一条笔画一串星;否则块状(照片/剪影):只取 Sobel 轮廓,内部不出星。
// 3 显著度×随机扰动降序,贪心泊松盘;二分半径使点数 ≈ count。
// 4 MST → 剪掉 > 2.6×中位长的边 → < 3 颗的碎簇降为散星 → 剪挂在交汇点上的短毛刺 → 近共线的度 2 节点降为
//   散星(线变长、星变疏,读起来像星座而不是网格)→ 块状图再补少量不交叉的近邻边。
export function inkMap(image) {
  const { width: w, height: h, data } = image;
  const n = w * h;
  const lum = new Float32Array(n), alpha = new Float32Array(n);
  let transparent = 0;
  for (let i = 0; i < n; i += 1) {
    const r = data[i * 4], g = data[i * 4 + 1], b = data[i * 4 + 2], a = data[i * 4 + 3];
    lum[i] = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    alpha[i] = a / 255;
    if (a < 250) transparent += 1;
  }
  const ink = new Float32Array(n);
  if (transparent > n * 0.02) {
    for (let i = 0; i < n; i += 1) ink[i] = alpha[i];
    return ink;
  }
  const border = [];
  for (let x = 0; x < w; x += 1) border.push(lum[x], lum[(h - 1) * w + x]);
  for (let y = 0; y < h; y += 1) border.push(lum[y * w], lum[y * w + w - 1]);
  const bg = median(border);
  for (let i = 0; i < n; i += 1) ink[i] = Math.abs(lum[i] - bg) / 255;
  const p95 = percentile(ink, 0.95);
  if (p95 < 0.04) return new Float32Array(n);
  for (let i = 0; i < n; i += 1) ink[i] = Math.min(1, ink[i] / p95);
  return ink;
}

// 倒角 3-4 距离变换(两遍扫描,O(n)):到最近背景像素的距离(像素)。
export function distanceTransform(mask, w, h) {
  const d = new Float32Array(w * h);
  for (let i = 0; i < d.length; i += 1) d[i] = mask[i] ? 1e9 : 0;
  const at = (x, y) => (x < 0 || y < 0 || x >= w || y >= h ? 0 : d[y * w + x]);
  for (let y = 0; y < h; y += 1) for (let x = 0; x < w; x += 1) {
    const i = y * w + x;
    if (d[i]) d[i] = Math.min(d[i], at(x - 1, y) + 3, at(x, y - 1) + 3, at(x - 1, y - 1) + 4, at(x + 1, y - 1) + 4);
  }
  for (let y = h - 1; y >= 0; y -= 1) for (let x = w - 1; x >= 0; x -= 1) {
    const i = y * w + x;
    if (d[i]) d[i] = Math.min(d[i], at(x + 1, y) + 3, at(x, y + 1) + 3, at(x + 1, y + 1) + 4, at(x - 1, y + 1) + 4);
  }
  for (let i = 0; i < d.length; i += 1) d[i] /= 3;
  return d;
}

// 中轴线近似:沿四个方向之一是局部极大(≥ 两侧且严格大于其一),且半径 ≥ minRadius。
export function ridgeMask(dt, w, h, minRadius = 1.5) {
  const out = new Uint8Array(w * h);
  const dirs = [[1, 0], [0, 1], [1, 1], [1, -1]];
  for (let y = 1; y < h - 1; y += 1) for (let x = 1; x < w - 1; x += 1) {
    const v = dt[y * w + x];
    if (v < minRadius) continue;
    for (const [dx, dy] of dirs) {
      const a = dt[(y - dy) * w + x - dx], b = dt[(y + dy) * w + x + dx];
      if (v >= a && v >= b && (v > a || v > b)) { out[y * w + x] = 1; break; }
    }
  }
  return out;
}

export function saliencyMap(image) {
  const { width: w, height: h } = image;
  const n = w * h;
  const ink = inkMap(image);
  const mask = new Uint8Array(n);
  let inked = 0;
  for (let i = 0; i < n; i += 1) if (ink[i] >= 0.5) { mask[i] = 1; inked += 1; }
  const score = new Float32Array(n);
  if (inked < n * 0.004) return { score, mode: "empty" };
  const dt = distanceTransform(mask, w, h);
  const ridge = ridgeMask(dt, w, h);
  // 笔画宽:横截面是三角形距离剖面,墨迹像素的平均距离 ≈ 宽/4(圆盘 ≈ 半径/3);按面积加权,对毛边不敏感。
  let dtSum = 0, ridgeCount = 0;
  for (let i = 0; i < n; i += 1) { if (mask[i]) dtSum += dt[i]; if (ridge[i]) ridgeCount += 1; }
  const strokeWidth = (4 * dtSum) / inked;
  if (ridgeCount && strokeWidth < 0.14 * Math.min(w, h) && inked < n * 0.5) {
    const half = Math.max(1, strokeWidth / 2);
    for (let i = 0; i < n; i += 1) if (ridge[i]) score[i] = 0.6 + 0.4 * Math.min(1, dt[i] / half);
    return { score, mode: "centerline", strokeWidth };
  }
  const blurred = boxBlur(boxBlur(ink, w, h), w, h);
  const edge = new Float32Array(n);
  for (let y = 1; y < h - 1; y += 1) for (let x = 1; x < w - 1; x += 1) {
    const at = (dx, dy) => blurred[(y + dy) * w + x + dx];
    const gx = -at(-1, -1) - 2 * at(-1, 0) - at(-1, 1) + at(1, -1) + 2 * at(1, 0) + at(1, 1);
    const gy = -at(-1, -1) - 2 * at(0, -1) - at(1, -1) + at(-1, 1) + 2 * at(0, 1) + at(1, 1);
    edge[y * w + x] = Math.hypot(gx, gy);
  }
  const e98 = percentile(edge, 0.98) || 1;
  // 只取轮廓:墨迹项只在同强度边缘之间做偏好,不让大块实心内部变成候选(否则星会铺满剪影内部)。
  for (let i = 0; i < n; i += 1) score[i] = Math.min(1, edge[i] / e98) * (0.8 + 0.2 * blurred[i]);
  return { score, mode: "outline", strokeWidth };
}
function boxBlur(src, w, h) {
  const out = new Float32Array(src.length);
  for (let y = 0; y < h; y += 1) for (let x = 0; x < w; x += 1) {
    let sum = 0, count = 0;
    for (let dy = -1; dy <= 1; dy += 1) for (let dx = -1; dx <= 1; dx += 1) {
      const xx = x + dx, yy = y + dy;
      if (xx < 0 || yy < 0 || xx >= w || yy >= h) continue;
      sum += src[yy * w + xx]; count += 1;
    }
    out[y * w + x] = sum / count;
  }
  return out;
}
function percentile(values, q) {
  const sample = [];
  const step = Math.max(1, Math.floor(values.length / 4096));
  for (let i = 0; i < values.length; i += step) sample.push(values[i]);
  sample.sort((a, b) => a - b);
  return sample[Math.min(sample.length - 1, Math.floor(sample.length * q))];
}

// 贪心泊松盘:候选按给定顺序,接受与已选点距离 ≥ r 的点。网格加速,O(候选数)。
export function poissonSelect(candidates, r, limit = Infinity) {
  const cell = r / Math.SQRT2;
  const grid = new Map();
  const picked = [];
  for (const c of candidates) {
    if (picked.length >= limit) break;
    const gx = Math.floor(c[0] / cell), gy = Math.floor(c[1] / cell);
    let ok = true;
    for (let dy = -2; dy <= 2 && ok; dy += 1) for (let dx = -2; dx <= 2 && ok; dx += 1) {
      const other = grid.get(`${gx + dx},${gy + dy}`);
      if (other && dist(other, c) < r) ok = false;
    }
    if (!ok) continue;
    grid.set(`${gx},${gy}`, c);
    picked.push(c);
  }
  return picked;
}

// 近共线的度 2 节点降为散星:删它的两条边、直连两邻居(合并后的边 ≤ maxLen 且不与其它边交叉)。
export function simplifyChains(points, edges, { maxTurnDeg = 24, maxLen = Infinity } = {}) {
  const adj = Array.from({ length: points.length }, () => new Set());
  for (const [i, j] of edges) { adj[i].add(j); adj[j].add(i); }
  const straight = -Math.cos((maxTurnDeg * Math.PI) / 180);
  const live = () => { const out = []; adj.forEach((s, i) => s.forEach((j) => { if (i < j) out.push([i, j]); })); return out; };
  let changed = true;
  while (changed) {
    changed = false;
    for (let p = 0; p < points.length; p += 1) {
      if (adj[p].size !== 2) continue;
      const [a, b] = [...adj[p]];
      if (adj[a].has(b)) continue;
      const ax = points[a][0] - points[p][0], ay = points[a][1] - points[p][1];
      const bx = points[b][0] - points[p][0], by = points[b][1] - points[p][1];
      const cos = (ax * bx + ay * by) / ((Math.hypot(ax, ay) * Math.hypot(bx, by)) || 1);
      if (cos > straight || dist(points[a], points[b]) > maxLen) continue;
      if (live().some(([i, j]) => i !== a && j !== a && i !== b && j !== b && i !== p && j !== p && segmentsCross(points[a], points[b], points[i], points[j]))) continue;
      adj[p].clear(); adj[a].delete(p); adj[b].delete(p);
      adj[a].add(b); adj[b].add(a);
      changed = true;
    }
  }
  return live();
}

export const IMAGE_STAR_COUNT = 64;
export const IMAGE_MAX_SIDE = 192;

export function imageToConstellation(image, { count = IMAGE_STAR_COUNT, seed = 7 } = {}) {
  const rand = seededRandom(seed);
  const { width: w, height: h } = image;
  const { score, mode } = saliencyMap(image);
  // 没有轮廓(纯色、均匀半透明、极低对比)直接拒绝,由界面提示换一张。
  if (mode === "empty" || percentile(score, 0.99) - percentile(score, 0.5) < 0.1) return null;
  const threshold = mode === "centerline" ? 0.5 : Math.max(0.12, percentile(score, 0.82));
  const candidates = [];
  for (let y = 1; y < h - 1; y += 1) for (let x = 1; x < w - 1; x += 1) {
    const s = score[y * w + x];
    if (s >= threshold) candidates.push([x + rand() - 0.5, y + rand() - 0.5, s * (0.85 + rand() * 0.3), s]);
  }
  candidates.sort((a, b) => b[2] - a[2]);
  if (candidates.length < 8) return null;
  let lo = 1, hi = Math.max(w, h) / 3, picked = [];
  for (let iter = 0; iter < 18; iter += 1) {
    const r = (lo + hi) / 2;
    picked = poissonSelect(candidates, r);
    if (picked.length > count) lo = r; else hi = r;
  }
  picked = poissonSelect(candidates, hi, count);
  if (picked.length < 8) return null;
  const points = picked.map(([x, y]) => [x, y]);
  const mst = euclideanMst(points);
  const med = median(mst.map((e) => e[2]));
  let edges = mst.filter((e) => e[2] <= med * 2.6).map(([i, j]) => [i, j]);
  const small = new Set(components(points.length, edges).filter((g) => g.length < 3).flat());
  edges = edges.filter(([i, j]) => !small.has(i) && !small.has(j));
  const degree0 = new Array(points.length).fill(0);
  for (const [i, j] of edges) { degree0[i] += 1; degree0[j] += 1; }
  edges = edges.filter(([i, j]) => {
    const leaf = degree0[i] === 1 ? i : degree0[j] === 1 ? j : -1;
    const hub = leaf === i ? j : i;
    return !(leaf >= 0 && degree0[hub] >= 3 && dist(points[i], points[j]) < med * 0.8);
  });
  edges = simplifyChains(points, edges, { maxTurnDeg: 24, maxLen: med * 3.2 });
  if (mode === "outline") edges = augmentEdges(points, edges, { k: 1, maxFactor: 1.6, minAngleDeg: 30, cap: 0.75 });
  // 星等:连线端点 / 交汇 / 拐点亮,线上其余按显著度,散星最暗。
  const degree = new Array(points.length).fill(0);
  for (const [i, j] of edges) { degree[i] += 1; degree[j] += 1; }
  const rank = [...picked.keys()].sort((a, b) => picked[b][3] - picked[a][3]);
  const mag = new Array(points.length);
  rank.forEach((id, r) => {
    const base = 2.2 + 1.6 * Math.pow(r / points.length, 0.8);
    mag[id] = degree[id] === 0 ? base + 1.2 : degree[id] === 2 ? base : base - 0.9;
  });
  const normalized = normalizePoints(points.map((p, i) => [p[0], p[1], mag[i]]), 0.04);
  return {
    v: MODEL_VERSION,
    kind: "image",
    mode,
    aspect: normalized.aspect,
    points: normalized.points.map(([x, y, m]) => [x, y, round(m, 1)]),
    edges: edges.map(([i, j]) => [i, j, ROLE.bridge]),
    hub: -1,
  };
}

// ---------- 彗星路线 ----------
// 事件语义 → 有向边序列 [[from, to, edgeIndex], …]:
//   recall  记忆臂尖 → 沿记忆边汇入 hub(检索 / 召回:经验流向决策点)
//   action  hub → 沿行动边走到末端(工具执行:决策变成行动)
//   trunk   主干从上到下(思考 / 流式回复:主干在推进)
//   wander  从最亮的 3 颗星之一随机游走 4~6 条边(没有角色的星座 / 图片,以及上述角色缺失时)
export function cometRoute(model, kind, rand = Math.random) {
  const n = model.points.length;
  const adj = Array.from({ length: n }, () => []);
  model.edges.forEach(([i, j, role = 0], e) => { adj[i].push([j, e, role]); adj[j].push([i, e, role]); });
  const hasRole = (r) => model.edges.some((e) => e[2] === r);
  const walk = (start, allow, stopAt, maxSteps = 32) => {
    const route = [];
    const seen = new Set([start]);
    let at = start;
    for (let k = 0; k < maxSteps; k += 1) {
      if (at === stopAt) break;
      const options = adj[at].filter(([v, , role]) => allow(role) && !seen.has(v));
      if (!options.length) break;
      const next = stopAt >= 0
        ? options.reduce((best, o) => (dist(model.points[o[0]], model.points[stopAt]) < dist(model.points[best[0]], model.points[stopAt]) ? o : best))
        : options[Math.floor(rand() * options.length)];
      route.push([at, next[0], next[1]]);
      seen.add(next[0]);
      at = next[0];
    }
    return route;
  };
  const leavesOf = (role) => [...Array(n).keys()].filter((i) => adj[i].length === 1 && adj[i][0][2] === role);
  // Closed agent modules have no leaf tips. Find a real path to the shared hub
  // instead of greedy walking, which can get trapped on the far side of a loop.
  const routeTo = (start, allow, target) => {
    const queue = [start], previous = new Map([[start, null]]);
    for (const at of queue) {
      if (at === target) break;
      for (const [v, e, role] of adj[at]) if (allow(role) && !previous.has(v)) {
        previous.set(v, [at, v, e]);
        queue.push(v);
      }
    }
    if (!previous.has(target)) return [];
    const route = [];
    for (let at = target; previous.get(at); at = previous.get(at)[0]) route.push(previous.get(at));
    return route.reverse();
  };
  const farthestRoleNode = (role) => [...Array(n).keys()]
    .filter((i) => i !== model.hub && adj[i].some((edge) => edge[2] === role))
    .sort((a, b) => dist(model.points[b], model.points[model.hub]) - dist(model.points[a], model.points[model.hub]))[0];
  if (kind === "recall" && hasRole(ROLE.memory) && model.hub >= 0) {
    const tips = leavesOf(ROLE.memory);
    const start = tips.length ? tips[Math.floor(rand() * tips.length)] : farthestRoleNode(ROLE.memory);
    const route = routeTo(start, (r) => r === ROLE.memory, model.hub);
    if (route.length) return route;
  }
  if (kind === "action" && hasRole(ROLE.action) && model.hub >= 0) {
    const route = walk(model.hub, (r) => r === ROLE.action, -1);
    if (route.length) return route;
  }
  if (kind === "trunk" && hasRole(ROLE.trunk)) {
    const ends = leavesOf(ROLE.trunk).sort((a, b) => model.points[a][1] - model.points[b][1]);
    if (ends.length >= 2) return walk(ends[0], (r) => r === ROLE.trunk, ends.at(-1));
    if (model.hub >= 0) {
      const route = routeTo(farthestRoleNode(ROLE.trunk), (r) => r === ROLE.trunk, model.hub);
      if (route.length) return route;
    }
  }
  const bright = [...Array(n).keys()].filter((i) => adj[i].length).sort((a, b) => model.points[a][2] - model.points[b][2]);
  const start = bright[Math.floor(rand() * Math.min(3, bright.length))] ?? 0;
  return walk(start, () => true, -1, 4 + Math.floor(rand() * 3));
}

// ---------- 偏好与持久化校验 ----------
// 偏好 JSON 从 app.json / localStorage 回来都要过这里:坏数据回落默认,绝不抛。
export function sanitizeModel(model) {
  if (!model || typeof model !== "object" || !Array.isArray(model.points) || !Array.isArray(model.edges)) return null;
  const points = model.points.slice(0, MAX_POINTS)
    .filter((p) => Array.isArray(p) && p.slice(0, 3).every(Number.isFinite))
    .map(([x, y, m]) => [clamp(x, 0, 1), clamp(y, 0, 1), clamp(m, -1, 6)]);
  if (points.length < 3) return null;
  const edges = model.edges.slice(0, MAX_EDGES)
    .filter((e) => Array.isArray(e) && Number.isInteger(e[0]) && Number.isInteger(e[1]) && e[0] !== e[1]
      && e[0] >= 0 && e[1] >= 0 && e[0] < points.length && e[1] < points.length)
    .map(([i, j, role]) => [i, j, [1, 2, 3].includes(role) ? role : 0]);
  const hub = Number.isInteger(model.hub) && model.hub >= 0 && model.hub < points.length ? model.hub : -1;
  const aspect = Number.isFinite(model.aspect) ? clamp(model.aspect, 0.2, 5) : 1;
  return {
    v: MODEL_VERSION,
    kind: String(model.kind || "image").slice(0, 16),
    mode: ["centerline", "outline"].includes(model.mode) ? model.mode : undefined,
    aspect, points, edges, hub,
    name: String(model.name || "").slice(0, 60),
  };
}

export const BACKDROP_PRESETS = ["kanzei", "big-dipper", "orion", "cassiopeia", "custom"];
export const BACKDROP_DEFAULTS = Object.freeze({ enabled: true, preset: "kanzei", density: 1, opacity: 1, custom: null });
export function normalizeBackdropPrefs(value = {}) {
  const v = value && typeof value === "object" ? value : {};
  const custom = sanitizeModel(v.custom);
  let preset = BACKDROP_PRESETS.includes(v.preset) ? v.preset : BACKDROP_DEFAULTS.preset;
  if (preset === "custom" && !custom) preset = BACKDROP_DEFAULTS.preset;
  return {
    enabled: v.enabled !== false,
    preset,
    density: Number.isFinite(v.density) ? clamp(v.density, 0, 2) : BACKDROP_DEFAULTS.density,
    opacity: Number.isFinite(v.opacity) ? clamp(v.opacity, 0.2, 1.5) : BACKDROP_DEFAULTS.opacity,
    custom,
  };
}
// 落盘形状:坐标 3 位小数、星等 1 位小数;64 颗星约 2KB(后端上限 64KB,见 prefs.rs BACKDROP_MAX_BYTES)。
export function serializeBackdropPrefs(prefs) {
  const p = normalizeBackdropPrefs(prefs);
  const c = p.custom;
  return {
    enabled: p.enabled,
    preset: p.preset,
    density: round(p.density, 2),
    opacity: round(p.opacity, 2),
    custom: c ? {
      v: c.v, kind: c.kind, ...(c.mode ? { mode: c.mode } : {}), name: c.name, aspect: round(c.aspect, 3),
      points: c.points.map(([x, y, m]) => [round(x, 3), round(y, 3), round(m, 1)]),
      edges: c.edges.map(([i, j, role]) => [i, j, role]),
      hub: c.hub,
    } : null,
  };
}

// 偏好 → 要画的模型。data = { KANZEI_LOGO_STROKES, STAR_PRESETS }(由调用方从 22-constellation-data.js 传入)。
export function resolveBackdropModel(prefs, data) {
  const p = normalizeBackdropPrefs(prefs);
  if (p.preset === "custom" && p.custom) return p.custom;
  if (data.STAR_PRESETS[p.preset]) return starPresetModel(data.STAR_PRESETS[p.preset]);
  return strokesToConstellation(data.KANZEI_LOGO_STROKES, { seed: 5, density: Math.max(0.5, p.density) });
}

// ---------- 构图 ----------
// 输入都是相对画布的矩形 {x, y, w, h}。返回 {box, alpha, placement, avoid, capped}:
//   box      模型映射到的目标框(按模型 aspect 取);
//   avoid    正文所在的矩形(已外扩):渲染器把它们从绘制区里剪掉(evenodd),正文底下一个像素都不画;
//   capped   星座框本身压在正文上(只剩右上角水印可放)——此时不剪,渲染器把整张星座先画进离屏层,
//            再以 watermarkAlpha 单一不透明度合成(见下方「水印」),正文压在任何像素上仍 ≥ 4.5:1。
// 规则:空态(welcome)/ 语音(voice)画进 art 槽(OC 开时在人物身后、稍淡);槽不可见就放文案右侧空白;
// 对话态(conversation)直接 hidden,不占消息背景。
export const GUTTER_MIN_W = 160;
export const GUTTER_MIN_H = 160;
export const BOX_MAX = 360;
const inflate = (r, by) => ({ x: r.x - by, y: r.y - by, w: r.w + 2 * by, h: r.h + 2 * by });
export function rectsIntersect(a, b) {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}
// 正文列:最近若干条消息的横向并集,纵向铺满区域(消息会滚动经过整列)。
export function columnFromRects(rects, area) {
  const usable = rects.filter((r) => r && r.w > 0 && r.h > 0);
  if (!usable.length) return null;
  const left = Math.min(...usable.map((r) => r.x)), right = Math.max(...usable.map((r) => r.x + r.w));
  return { x: left, y: area.y, w: right - left, h: area.h };
}
export function layoutBackdrop({ area, mode, slot = null, ocInSlot = false, copy = null, aspect = 1 }) {
  // 有消息的对话保持纯净;装饰只出现在欢迎页和语音舞台。
  if (mode === "conversation") return { box: { x: 0, y: 0, w: 0, h: 0 }, alpha: 0, placement: "hidden", avoid: [], capped: false };
  const fit = (box, maxW = box.w, maxH = box.h) => {
    const w = Math.min(maxW, maxH * aspect), h = w / aspect;
    return { x: box.x + (box.w - w) / 2, y: box.y + (box.h - h) / 2, w, h };
  };
  const avoid = copy ? [inflate(copy, 12)] : [];
  const done = (box, alpha, placement) => ({ box, alpha, placement, avoid, capped: avoid.some((r) => rectsIntersect(r, box)) });
  if ((mode === "welcome" || mode === "voice") && slot && slot.w > 120 && slot.h > 120) {
    if (ocInSlot) {
      // 人物站在槽里:星座退到人物身后的上 3/4,右对齐,稍淡。
      const behind = { x: slot.x + slot.w * 0.25, y: slot.y, w: slot.w * 0.75, h: slot.h * 0.75 };
      return done(fit(behind), 0.7, "slot");
    }
    const inset = { x: slot.x + slot.w * 0.06, y: slot.y + slot.h * 0.04, w: slot.w * 0.88, h: slot.h * 0.92 };
    return done(fit(inset, Math.min(inset.w, BOX_MAX * 1.3), Math.min(inset.h, BOX_MAX * 1.3)), 1, "slot");
  }
  if ((mode === "welcome" || mode === "voice") && copy) {
    const x = copy.x + copy.w + 32;
    const side = { x, y: area.y + 24, w: area.x + area.w - 24 - x, h: area.h - 48 };
    if (side.w >= GUTTER_MIN_W && side.h >= GUTTER_MIN_H) return done(fit(side, Math.min(side.w, BOX_MAX), Math.min(side.h, BOX_MAX)), 0.9, "side");
  }
  const side = clamp(area.w * 0.3, 160, 320);
  const corner = { x: area.x + area.w - side - 28, y: area.y + 28, w: side, h: Math.min(side, area.h * 0.5) };
  return done(fit(corner), mode === "welcome" ? 0.6 : 0.5, "corner");
}

// ---------- 不透明度与正文对比度 ----------
// 每一笔 alpha 都经 layerAlpha 夹到 [0,1]。水印(layout.capped)不再逐层夹:同一像素上星尘、星、连线、光点头、
// 尾迹、点亮的边能叠 4 层以上(复核实测 1280@1.5 光点压星处合成 .376,--dim 跌到 3.91),逐层上限挡不住。
// 改为「离屏层 + 单一不透明度」:整张星座先画进离屏层(层内再怎么叠,alpha 也 ≤ 1、颜色是若干 token 色的
// 凸组合),再以 watermarkAlpha(≤ watermarkCap)合成到画布——正文底下任一像素 = chat-bg 与某个混色按 ≤ cap
// 混合,和叠了几层无关。kind:star(星与星尘)、line(连线)、pulse(光点 / 波,--accent)、error(失败,--err)。
export const TEXT_CONTRAST_FLOOR = 4.5;
export const WATERMARK_KINDS = ["star", "line", "pulse", "error"];
export const WATERMARK_MARGIN = 0.01; // 8 位量化与离屏重采样的余量(≈ 2.5/255)
export const WATERMARK_LINE_BOOST = 2; // 水印层里连线相对星点提亮,星座骨架在很低的总不透明度下仍读得出来
export function layerAlpha(value) {
  return clamp(Number.isFinite(value) ? value : 0, 0, 1);
}
// 各层的增益上限:渲染器与冒烟共用。hubError 是失败会话 hub 的静态淡 --err。
export const GAIN = { star: 1.2, voice: 1.5, line: 1.25, dust: 1, comet: 0.95, lit: 0.55, ripple: 0.8, hubError: 0.5 };
// WCAG 2.x 相对亮度与对比度;颜色是 [r, g, b](0-255)。
export function relativeLuminance([r, g, b]) {
  const lin = (c) => { const v = c / 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}
export function contrastRatio(a, b) {
  const [hi, lo] = [relativeLuminance(a), relativeLuminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
export function composite(top, alpha, base) {
  return base.map((c, i) => top[i] * alpha + c * (1 - alpha));
}
// 叠色不透明度的上限:bg 上叠 overlay@a 之后,texts 里每一种字色的对比度都 ≥ floor(二分,单调)。
// 原本就不到 floor 的字色不参与(那是配色门禁 ③ 的事,这里不替它背锅)。
export function maxOverlayAlpha(texts, bg, overlay, floor = TEXT_CONTRAST_FLOOR) {
  const judged = texts.filter((t) => contrastRatio(t, bg) >= floor);
  const ok = (a) => judged.every((t) => contrastRatio(t, composite(overlay, a, bg)) >= floor);
  if (ok(1)) return 1;
  let lo = 0, hi = 1;
  for (let i = 0; i < 24; i += 1) { const mid = (lo + hi) / 2; if (ok(mid)) lo = mid; else hi = mid; }
  return lo;
}
// n 种颜色的混合权重网格(单纯形上步长 1/steps 的全部格点)。
export function simplexWeights(n, steps) {
  const out = [];
  const walk = (prefix, left) => {
    if (prefix.length === n - 1) { out.push([...prefix, left].map((w) => w / steps)); return; }
    for (let k = 0; k <= left; k += 1) walk([...prefix, k], left - k);
  };
  walk([], steps);
  return out;
}
export function mixColors(colors, weights) {
  return [0, 1, 2].map((c) => colors.reduce((sum, color, i) => sum + color[c] * weights[i], 0));
}
// 水印合成不透明度的上限:先取各色单独叠加时的安全上限的最小值;亮色主题里「亮度 ≥ 下限」对混色不是凸的,
// 再在 4 色单纯形网格上逐个混色复核,不够就二分收紧;最后扣量化余量。
export function watermarkCap(palette, floor = TEXT_CONTRAST_FLOOR, steps = 6) {
  const colors = WATERMARK_KINDS.map((kind) => palette[kind]);
  const judged = palette.texts.filter((t) => contrastRatio(t, palette.chatBg) >= floor);
  const mixes = simplexWeights(colors.length, steps).map((w) => mixColors(colors, w));
  const ok = (a) => mixes.every((color) => {
    const pixel = composite(color, a, palette.chatBg);
    return judged.every((t) => contrastRatio(t, pixel) >= floor);
  });
  let hi = Math.min(...colors.map((color) => maxOverlayAlpha(palette.texts, palette.chatBg, color, floor)));
  if (!ok(hi)) {
    let lo = 0;
    for (let i = 0; i < 24; i += 1) { const mid = (lo + hi) / 2; if (ok(mid)) lo = mid; else hi = mid; }
    hi = lo;
  }
  return Math.max(0, hi - WATERMARK_MARGIN);
}
// 水印最终的合成不透明度:不透明度偏好只能往下调,开到 150% 也不越过上限。
export function watermarkAlpha(cap, opacity = 1) {
  return clamp(cap, 0, 1) * clamp(Number.isFinite(opacity) ? opacity : 1, 0.2, 1);
}

// ---------- 活动态 ----------
// 只有 thinking / executing / replying 有常驻光点,才算「忙」(≤ 30 帧/秒);blocked(本轮失败,停在失败会话上)
// 与 complete 没有常驻动画——复核实测失败会话曾一直按 26 帧/秒重画。
export const BUSY_ACTIVITIES = ["thinking", "executing", "replying"];
export function activityBusy(activity) {
  return BUSY_ACTIVITIES.includes(activity);
}
// hub 的静态着色:运行中且画静帧(减少动态效果)时着 --accent 表达「在跑」;本轮失败着淡 --err;其余不着色。
// 动画模式下运行态由光点表达,hub 不着色。强调色只表示运行中(ui_color_semantics),失败不能画成强调色。
export function hubTone(activity, { still = false } = {}) {
  if (activityBusy(activity)) return still ? "pulse" : null;
  return activity === "blocked" ? "error" : null;
}

// ---------- 调度 ----------
// 空闲 ≤ 8 帧/秒(闪烁与漂移),有光点 / 波 / 补间 / 入场时 ≤ 30 帧/秒;窗口隐藏、不在对话视图、背景关闭、
// 减少动态效果时一律 null(零定时器、零 rAF;减少动态时只在布局 / 主题 / 偏好 / 活动态变化时画静帧)。
export const IDLE_FRAME_MS = 125;
export const BUSY_FRAME_MS = 33;
export const LAYOUT_WATCH_MS = 500;
export function frameDelay({ hidden, viewActive, enabled, reduced, busy }) {
  if (hidden) return null;
  if (!viewActive || !enabled || reduced) return null;
  return busy ? BUSY_FRAME_MS : IDLE_FRAME_MS;
}
// 布局观察(≤ 2Hz 读一次各块位置):同样只在可见、在对话视图、背景开启时跑。
export function watchDelay({ hidden, viewActive, enabled }) {
  return hidden || !viewActive || !enabled ? null : LAYOUT_WATCH_MS;
}
// 有新东西要画时要不要把待发的那一帧提前:只有「还在等定时器、且离触发还比忙帧间隔更久、且现在忙」才提前。
// 已排好的 rAF 永不取消;离触发不到一个忙帧间隔的也不动——否则每 16ms 一次的流式事件会反复取消重排,
// 渲染器被饿死(复核实测 16ms / 25ms 连发时 0 帧/秒),拖动改窗口尺寸时画布也一直空白。
export function shouldHurry({ timer, raf, busy, dueIn }) {
  return Boolean(timer && !raf && busy && dueIn > BUSY_FRAME_MS);
}
// 流式分片(assistant_streaming / reasoning_active 每个分片一次,约 16ms)只写活动态;唤醒渲染器每 33ms 至多一次。
export const WAKE_THROTTLE_MS = BUSY_FRAME_MS;
