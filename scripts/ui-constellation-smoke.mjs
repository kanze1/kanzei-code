#!/usr/bin/env node
// UI2-0926 #10 对话背景(星座背景)冒烟:纯函数 + 静态契约。设计见 docs/design/ui_chat_backdrop.md。
// 单独跑:node scripts/ui-constellation-smoke.mjs;ui-runtime-smoke.mjs 末尾也会链式 import 它。
//
// 变异守卫:KZ_SMOKE_MUTATE=<下表 id> 时读 22-constellation-core.js 源码、删改被守护的那一处(必须恰好命中
// 一处,否则退出码 2),再以 data: URL 导入改过的模块跑同一套断言——期望本次运行失败。core 零 import,
// 所以可以脱离文件系统单独求值。不认识的 id(ui-runtime-smoke 自己的变异)一律忽略。
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(import.meta.dirname, "..");
const UI = resolve(root, "crates/kanzei-app/ui");
const coreSource = readFileSync(resolve(UI, "22-constellation-core.js"), "utf8");

const MUTATIONS = {
  // 近平行笔画不做 T 字桥接。删了它,标志三条电路臂的臂尖被连成锯齿(⑤ 臂尖断言)。
  bgParallelBridge: {
    pattern: /\n[ \t]*if \(Math\.abs\(u\[0\] \* v\[0\] \+ u\[1\] \* v\[1\]\) \/ \(\(Math\.hypot\(\.\.\.u\) \* Math\.hypot\(\.\.\.v\)\) \|\| 1\) > Math\.cos\(Math\.PI \/ 9\)\) return;/,
    replace: "",
  },
  // 块状图只取轮廓。把墨迹项加回显著度,实心圆盘内部也出星(⑦b 圆盘内部断言)。
  bgOutlineInterior: {
    pattern: /score\[i\] = Math\.min\(1, edge\[i\] \/ e98\) \* \(0\.8 \+ 0\.2 \* blurred\[i\]\);/,
    replace: "score[i] = Math.min(1, edge[i] / e98 + 0.6 * ink[i]) * (0.8 + 0.2 * blurred[i]);",
  },
  // 窗口隐藏时零定时器。删了它,最小化后仍按帧唤醒(⑪)。
  bgHiddenPause: {
    pattern: /\n[ \t]*if \(hidden\) return null;/,
    replace: "",
  },
  // 持久化校验丢弃越界下标。删了它,坏数据带着越界边进渲染器(⑧)。
  bgSanitizeBounds: {
    pattern: / && e\[0\] < points\.length && e\[1\] < points\.length/,
    replace: "",
  },
  // 水印压在正文上时逐层夹 alpha。删了它,正文压在最亮星点上跌破 4.5:1(⑬)。
  bgTextCap: {
    pattern: /\n[ \t]*if \(layout\?\.capped\) return Math\.min\(v, caps\?\.\[kind\] \?\? 0\);/,
    replace: "",
  },
  // 对话态把正文列登记为避让区。删了它,沟槽 / 星尘的剪裁不再挖掉正文列(⑨)。
  bgColumnAvoid: {
    pattern: /const text = mode === "conversation" \? column : copy;/,
    replace: "const text = copy;",
  },
};

const mutateId = process.env.KZ_SMOKE_MUTATE ?? "";
const mutation = MUTATIONS[mutateId];
let coreUrl = pathToFileURL(resolve(UI, "22-constellation-core.js")).href;
if (mutation) {
  const hits = (coreSource.match(new RegExp(mutation.pattern.source, "g")) ?? []).length;
  if (hits !== 1) {
    console.error(`变异 ${mutateId} 没有恰好命中一处被守护的源码(实得 ${hits} 处):护栏已经失效,先修变异表`);
    process.exit(2);
  }
  coreUrl = `data:text/javascript;base64,${Buffer.from(coreSource.replace(mutation.pattern, mutation.replace)).toString("base64")}`;
  console.error(`[KZ_SMOKE_MUTATE=${mutateId}] 已改动 22-constellation-core.js 被守护的源码,期望本次运行**失败**`);
}

const core = await import(coreUrl);
const data = await import(pathToFileURL(resolve(UI, "22-constellation-data.js")).href);
const {
  cometRoute, euclideanMst, components, augmentEdges, segmentsCross, projectStars, starPresetModel, strokesToConstellation,
  imageToConstellation, poissonSelect, sanitizeModel, normalizeBackdropPrefs, serializeBackdropPrefs, layoutBackdrop, bfsOrder,
  seededRandom, frameDelay, watchDelay, resolveBackdropModel, peakAlphas, textSafeCaps, rectsIntersect, columnFromRects,
  edgeDepthsFrom, IDLE_FRAME_MS, BUSY_FRAME_MS, TEXT_CONTRAST_FLOOR, OVERLAP_LAYERS,
} = core;
const { STAR_PRESETS, KANZEI_LOGO_STROKES, POLARIS } = data;

const failures = [];
const check = (label, fn) => {
  try { fn(); } catch (error) { failures.push(`${label}: ${error.message}`); }
};
const d = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);
const validModel = (m) => {
  assert.ok(m && Array.isArray(m.points) && Array.isArray(m.edges), "模型形状不对");
  for (const p of m.points) assert.ok(p.length >= 3 && p[0] >= 0 && p[0] <= 1 && p[1] >= 0 && p[1] <= 1, `点越界 ${p}`);
  for (const [i, j] of m.edges) assert.ok(Number.isInteger(i) && Number.isInteger(j) && i !== j && i < m.points.length && j < m.points.length, `边越界 ${i}-${j}`);
};

// ① MST:n-1 条边、连通、总长 = Kruskal 最优
check("① MST", () => {
  const rand = seededRandom(3);
  const pts = Array.from({ length: 40 }, () => [rand(), rand()]);
  const mst = euclideanMst(pts);
  assert.equal(mst.length, 39);
  assert.equal(components(40, mst).length, 1);
  const all = [];
  for (let i = 0; i < 40; i += 1) for (let j = i + 1; j < 40; j += 1) all.push([i, j, d(pts[i], pts[j])]);
  all.sort((a, b) => a[2] - b[2]);
  const parent = [...Array(40).keys()];
  const find = (x) => (parent[x] === x ? x : (parent[x] = find(parent[x])));
  let kruskal = 0;
  for (const [i, j, w] of all) if (find(i) !== find(j)) { parent[find(i)] = find(j); kruskal += w; }
  const total = mst.reduce((s, e) => s + e[2], 0);
  assert.ok(Math.abs(total - kruskal) < 1e-9, `MST 总长 ${total} ≠ Kruskal ${kruskal}`);
});

// ② 补边不交叉、不重复、≤ 1.3n
check("② augmentEdges", () => {
  const rand = seededRandom(11);
  const pts = Array.from({ length: 60 }, () => [rand(), rand()]);
  const base = euclideanMst(pts).map(([i, j]) => [i, j]);
  const edges = augmentEdges(pts, base);
  assert.ok(edges.length > base.length, "补边应至少加一条");
  assert.ok(edges.length <= Math.round(60 * 1.3));
  const keys = new Set(edges.map(([i, j]) => (i < j ? `${i}:${j}` : `${j}:${i}`)));
  assert.equal(keys.size, edges.length, "补边出现重复");
  for (let a = base.length; a < edges.length; a += 1) for (let b = 0; b < edges.length; b += 1) {
    if (a === b) continue;
    const [i, j] = edges[a], [k, l] = edges[b];
    if (new Set([i, j, k, l]).size < 4) continue;
    assert.ok(!segmentsCross(pts[i], pts[j], pts[k], pts[l]), `补边 ${i}-${j} 与 ${k}-${l} 交叉`);
  }
});

// ③ 星表(球面):天枢—天璇 5.37°、天枢—北极星 28.7°、天璇→天枢大圆离北极星 < 2.5°;投影后角距保持、北在上
check("③ 北斗星表", () => {
  const big = STAR_PRESETS["big-dipper"];
  const vec = (s) => { const ra = s[2] * 15 * Math.PI / 180, de = s[3] * Math.PI / 180; return [Math.cos(de) * Math.cos(ra), Math.cos(de) * Math.sin(ra), Math.sin(de)]; };
  const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const toDeg = 180 / Math.PI;
  const [du, me, po] = [vec(big.stars[0]), vec(big.stars[1]), vec(POLARIS)];
  assert.ok(Math.abs(Math.acos(dot(du, me)) * toDeg - 5.37) < 0.3, "天枢—天璇角距不对");
  const n = cross(me, du);
  const miss = Math.asin(Math.abs(dot(n, po)) / Math.hypot(...n)) * toDeg;
  assert.ok(miss < 2.5, `指极星大圆离北极星 ${miss.toFixed(2)}°`);
  assert.ok(Math.abs(Math.acos(dot(du, po)) * toDeg - 28.7) < 0.3, "天枢—北极星角距不对");
  const projected = projectStars(big.stars);
  const sep = d(projected[0], projected[1]) * toDeg;
  assert.ok(Math.abs(sep - 5.37) < 0.15, `投影后天枢—天璇 ${sep.toFixed(2)}°`);
  assert.ok(projected[0][1] < projected[1][1], "北应在上(天枢 y 更小)");
});

// ④ 猎户腰带三星近似共线且等距;参宿四在参宿七左上;所有预设坐标在 [0,1]
check("④ 猎户座与预设", () => {
  const m = starPresetModel(STAR_PRESETS.orion);
  const at = (id) => m.points[STAR_PRESETS.orion.stars.findIndex((s) => s[0] === id)];
  const [a, b, c] = [at("mintaka"), at("alnilam"), at("alnitak")];
  const ratio = d(a, b) / d(b, c);
  assert.ok(ratio > 0.85 && ratio < 1.25, `腰带间距比 ${ratio}`);
  const skew = Math.abs((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / (d(a, c) ** 2);
  assert.ok(skew < 0.08, `腰带不共线 ${skew}`);
  assert.ok(at("betelgeuse")[0] < at("rigel")[0] && at("betelgeuse")[1] < at("rigel")[1], "参宿四应在参宿七左上(东在左、北在上)");
  for (const preset of Object.values(STAR_PRESETS)) validModel(starPresetModel(preset));
});

// ⑤ 标志星座:单连通、18~40 颗、≥ 7 颗亮星;臂尖之间无边且每颗臂尖只连自己那条臂;角色边与 hub
check("⑤ kanzei 标志星座", () => {
  const m = strokesToConstellation(KANZEI_LOGO_STROKES, { seed: 5 });
  validModel(m);
  assert.equal(components(m.points.length, m.edges).length, 1, "标志星座应单连通");
  assert.ok(m.points.length >= 18 && m.points.length <= 40, `标志星数 ${m.points.length}`);
  assert.ok(m.points.filter((p) => p[2] <= 2.2).length >= 7, "端点/交汇亮星不足");
  const tips = [...m.points.keys()].filter((i) => m.points[i][0] > 0.5).sort((a, b) => m.points[a][1] - m.points[b][1]).slice(0, 3);
  assert.ok(!m.edges.some(([i, j]) => tips.includes(i) && tips.includes(j)), "臂尖之间被连了线");
  for (const tip of tips) assert.equal(m.edges.filter(([i, j]) => i === tip || j === tip).length, 1, "每颗臂尖应只连自己那条臂");
  const count = (r) => m.edges.filter((e) => e[2] === r).length;
  assert.ok(count(2) >= 6 && count(1) >= 2 && count(3) >= 2, `角色边数 trunk=${count(1)} memory=${count(2)} action=${count(3)}`);
  const hubEdges = m.edges.filter(([i, j]) => i === m.hub || j === m.hub);
  assert.ok(hubEdges.length >= 3, `hub 度数 ${hubEdges.length}`);
  assert.ok(hubEdges.some((e) => e[2] === 2) && hubEdges.some((e) => e[2] === 3), "hub 应同时连着记忆边与行动边");
});

// ⑥ 泊松选点:两两距离 ≥ r
check("⑥ 泊松选点", () => {
  const rand = seededRandom(9);
  const cand = Array.from({ length: 3000 }, () => [rand() * 100, rand() * 100]);
  const picked = poissonSelect(cand, 7);
  assert.ok(picked.length > 20);
  for (let i = 0; i < picked.length; i += 1) for (let j = i + 1; j < picked.length; j += 1) assert.ok(d(picked[i], picked[j]) >= 7 - 1e-9);
});

// ⑦ 图片 → 星座:合成圆环图的星都落在环上、点数 ≈ count;纯色 / 均匀半透明图返回 null
check("⑦ 图片圆环", () => {
  const w = 160, h = 120, px = new Uint8ClampedArray(w * h * 4);
  for (let y = 0; y < h; y += 1) for (let x = 0; x < w; x += 1) {
    const i = (y * w + x) * 4, on = Math.abs(Math.hypot(x - 80, y - 60) - 40) < 3;
    px[i] = px[i + 1] = px[i + 2] = on ? 20 : 235; px[i + 3] = 255;
  }
  const m = imageToConstellation({ width: w, height: h, data: px }, { count: 40 });
  assert.ok(m && m.points.length >= 30 && m.points.length <= 40, `圆环点数 ${m?.points.length}`);
  validModel(m);
  const radii = m.points.map((p) => Math.hypot(p[0] - 0.5, p[1] - 0.5));
  const mean = radii.reduce((s, r) => s + r, 0) / radii.length;
  assert.ok(radii.every((r) => Math.abs(r - mean) < 0.08), "星没落在圆环上");
  const flat = new Uint8ClampedArray(w * h * 4).fill(128);
  assert.equal(imageToConstellation({ width: w, height: h, data: flat }), null, "均匀半透明图应返回 null");
  for (let i = 3; i < flat.length; i += 4) flat[i] = 255;
  assert.equal(imageToConstellation({ width: w, height: h, data: flat }), null, "纯色图应返回 null");
});

// ⑦b 判型:实心圆盘走轮廓且内部不出星;细笔画 L 走中轴线且共线简化后边数 < 星数
check("⑦b 图片判型", () => {
  const w = 160, h = 160;
  const disk = new Uint8ClampedArray(w * h * 4);
  for (let y = 0; y < h; y += 1) for (let x = 0; x < w; x += 1) {
    const i = (y * w + x) * 4, inside = Math.hypot(x - 80, y - 80) < 55;
    disk[i] = disk[i + 1] = disk[i + 2] = inside ? 230 : 15; disk[i + 3] = 255;
  }
  const blob = imageToConstellation({ width: w, height: h, data: disk }, { count: 40 });
  assert.equal(blob.mode, "outline");
  const rs = blob.points.map((p) => Math.hypot(p[0] - 0.5, p[1] - 0.5));
  assert.ok(Math.min(...rs) > 0.36, `实心圆盘内部出了星(最小半径 ${Math.min(...rs).toFixed(2)})`);
  const stroke = new Uint8ClampedArray(w * h * 4);
  for (let y = 0; y < h; y += 1) for (let x = 0; x < w; x += 1) {
    const i = (y * w + x) * 4, on = (Math.abs(x - 40) < 6 && y > 20 && y < 140) || (Math.abs(y - 134) < 6 && x > 34 && x < 130);
    stroke[i] = stroke[i + 1] = stroke[i + 2] = on ? 20 : 240; stroke[i + 3] = 255;
  }
  const line = imageToConstellation({ width: w, height: h, data: stroke }, { count: 24 });
  assert.equal(line.mode, "centerline");
  assert.ok(line.edges.length < line.points.length, "共线简化后边数应少于星数(线上多余的星降为散星)");
});

// ⑦c 彗星路线:recall 从记忆臂尖汇入 hub 且沿记忆边,action 从 hub 沿行动边,trunk 自上而下;星表退回首尾相接的游走
check("⑦c 彗星路线", () => {
  const m = strokesToConstellation(KANZEI_LOGO_STROKES, { seed: 5 });
  const rand = seededRandom(1);
  const recall = cometRoute(m, "recall", rand);
  assert.ok(recall.length >= 2 && recall.at(-1)[1] === m.hub, "recall 路线没有汇入 hub");
  assert.ok(recall.slice(0, -1).every(([, , e]) => m.edges[e][2] === 2), "recall 路线在到 hub 前离开了记忆臂");
  const action = cometRoute(m, "action", rand);
  assert.ok(action.length >= 2 && action[0][0] === m.hub && action.every(([, , e]) => m.edges[e][2] === 3), "action 路线不对");
  const trunk = cometRoute(m, "trunk", rand);
  assert.ok(trunk.length >= 2 && m.points[trunk[0][0]][1] < m.points[trunk.at(-1)[1]][1], "trunk 应自上而下");
  const stars = starPresetModel(STAR_PRESETS.orion);
  const wander = cometRoute(stars, "recall", rand);
  assert.ok(wander.length >= 2);
  for (let k = 1; k < wander.length; k += 1) assert.equal(wander[k][0], wander[k - 1][1], "游走路线断开");
});

// ⑧ 偏好与持久化校验:坏数据不抛、回落默认;越界下标丢弃;序列化往返稳定且体积小
check("⑧ 偏好校验", () => {
  assert.deepEqual(normalizeBackdropPrefs(null), { enabled: true, preset: "kanzei", density: 1, opacity: 1, custom: null });
  assert.equal(normalizeBackdropPrefs({ preset: "custom" }).preset, "kanzei", "无自定义点集时 custom 回落默认");
  assert.equal(normalizeBackdropPrefs({ preset: "nebula", density: 9, opacity: 0 }).preset, "kanzei");
  assert.equal(normalizeBackdropPrefs({ density: 9 }).density, 2);
  assert.equal(normalizeBackdropPrefs({ opacity: 0 }).opacity, 0.2);
  assert.equal(normalizeBackdropPrefs({ enabled: false }).enabled, false);
  const bad = sanitizeModel({ points: [[0, 0, 1], [1, 1, 2], [0.5, 2, 3], ["x", 0, 0]], edges: [[0, 1], [1, 9], [2, 2], [0, 2], [-1, 0]] });
  assert.deepEqual(bad.edges, [[0, 1, 0], [0, 2, 0]], "越界 / 自环 / 负下标边应被丢弃");
  assert.equal(bad.points[2][1], 1, "坐标夹到 [0,1]");
  assert.equal(sanitizeModel({ points: "nope" }), null);
  assert.equal(sanitizeModel({ points: [[0, 0, 1]], edges: [] }), null, "少于 3 颗星不成星座");
  const ring = strokesToConstellation(KANZEI_LOGO_STROKES, { seed: 5 });
  const stored = serializeBackdropPrefs({ preset: "custom", custom: { ...ring, kind: "image", name: "logo.png" } });
  const again = serializeBackdropPrefs(JSON.parse(JSON.stringify(stored)));
  assert.deepEqual(again, stored, "序列化往返不稳定");
  assert.equal(stored.preset, "custom");
  assert.ok(JSON.stringify(stored).length < 8 * 1024, `偏好体积 ${JSON.stringify(stored).length} 字节,点集应远小于后端 64KB 上限`);
  assert.ok(!("pixels" in stored.custom) && !("data" in stored.custom), "偏好里不得出现像素");
});

// ⑨ 构图:对话态宽屏进沟槽且不与正文列相交、正文列登记为避让区;窄屏退右上角水印(capped);空态进槽;文案右侧空白
check("⑨ 构图", () => {
  // 1600@1.25 的对话区(侧栏收起后约 1248 宽),正文列 768 居中(chat 组的新列宽)
  const area = { x: 0, y: 0, w: 1248, h: 760 };
  const column = { x: 240, y: 0, w: 768, h: 760 };
  const g = layoutBackdrop({ area, mode: "conversation", column, aspect: 1 });
  assert.equal(g.placement, "gutter");
  assert.ok(!rectsIntersect(g.box, column), "沟槽星座压到了正文列");
  assert.ok(g.avoid.some((r) => r.x <= column.x && r.x + r.w >= column.x + column.w), "正文列没有登记为避让区(星尘会画进正文)");
  assert.equal(g.capped, false);
  // 1280@1.5:对话区约 928 宽,沟槽 80 → 右上角水印,压在正文上 → capped
  const narrow = layoutBackdrop({ area: { x: 0, y: 0, w: 928, h: 560 }, mode: "conversation", column: { x: 80, y: 0, w: 768, h: 560 }, aspect: 1 });
  assert.equal(narrow.placement, "corner");
  assert.ok(narrow.alpha <= 0.5 && narrow.capped, "窄屏水印必须低透明度且标记 capped");
  // OC 伴侣占着右下:沟槽高度止于人物上方
  const withOc = layoutBackdrop({ area, mode: "conversation", column, reserve: { x: 1060, y: 480, w: 164, h: 246 }, aspect: 1 });
  assert.ok(withOc.box.y + withOc.box.h <= 480 - 24 + 1e-6, "沟槽星座压到了 OC 伴侣");
  // 空态:进 art 槽且在槽内;文案登记为避让区
  const copy = { x: 190, y: 250, w: 400, h: 220 };
  const slot = layoutBackdrop({ area, mode: "welcome", slot: { x: 620, y: 180, w: 440, h: 380 }, copy, aspect: 1.2 });
  assert.equal(slot.placement, "slot");
  assert.ok(slot.box.x >= 620 && slot.box.x + slot.box.w <= 1060 && slot.box.y >= 180 && slot.box.y + slot.box.h <= 560);
  assert.ok(slot.avoid.length === 1 && !slot.capped);
  // 槽不可见(窄容器):文案右侧空白够就放那里
  const side = layoutBackdrop({ area, mode: "welcome", copy: { x: 354, y: 260, w: 540, h: 220 }, aspect: 1 });
  assert.equal(side.placement, "side");
  assert.ok(!rectsIntersect(side.box, { x: 354, y: 260, w: 540, h: 220 }));
  // 正文列 = 最近几条消息的横向并集
  const col = columnFromRects([{ x: 300, y: 10, w: 500, h: 40 }, { x: 240, y: 60, w: 768, h: 90 }, null], area);
  assert.deepEqual(col, { x: 240, y: 0, w: 768, h: 760 });
});

// ⑩ BFS 顺序覆盖所有边、从最亮星出发;从 hub 出发的层次单调
check("⑩ BFS", () => {
  const m = starPresetModel(STAR_PRESETS["big-dipper"]);
  const { edgeOrder, root } = bfsOrder(m);
  assert.equal(new Set(edgeOrder).size, m.edges.length);
  assert.equal(m.points[root][2], Math.min(...m.points.map((p) => p[2])));
  const logo = strokesToConstellation(KANZEI_LOGO_STROKES, { seed: 5 });
  const depth = edgeDepthsFrom(logo, logo.hub);
  assert.ok(depth.every((v) => v >= 1), "从 hub 出发应覆盖整个标志星座");
  assert.ok(logo.edges.some(([i, j], e) => (i === logo.hub || j === logo.hub) && depth[e] === 1), "hub 的边应在第 1 层");
});

// ⑪ 调度:隐藏 / 非对话视图 / 关闭 / 减少动态 → null(零定时器);空闲 125ms、忙 33ms;布局观察同理
check("⑪ 调度", () => {
  const base = { hidden: false, viewActive: true, enabled: true, reduced: false, busy: false };
  assert.equal(frameDelay(base), IDLE_FRAME_MS);
  assert.equal(IDLE_FRAME_MS >= 125 && BUSY_FRAME_MS >= 33, true, "空闲 ≤ 8 帧/秒、忙时 ≤ 30 帧/秒");
  assert.equal(frameDelay({ ...base, busy: true }), BUSY_FRAME_MS);
  assert.equal(frameDelay({ ...base, hidden: true, busy: true }), null, "窗口隐藏必须停");
  assert.equal(frameDelay({ ...base, viewActive: false }), null, "不在对话视图必须停");
  assert.equal(frameDelay({ ...base, enabled: false }), null, "关闭背景必须停");
  assert.equal(frameDelay({ ...base, reduced: true, busy: true }), null, "减少动态效果只画静帧");
  assert.equal(watchDelay({ hidden: true, viewActive: true, enabled: true }), null);
  assert.equal(watchDelay({ hidden: false, viewActive: false, enabled: true }), null);
  assert.ok(watchDelay({ hidden: false, viewActive: true, enabled: true }) >= 500, "布局观察 ≤ 2Hz");
});

// ⑫ token:两个主题块都定义 5 个 --backdrop-* token;颜色 token 是 6 位十六进制
const css = readFileSync(resolve(UI, "style.css"), "utf8");
const cssClean = css.replace(/\/\*[\s\S]*?\*\//g, "");
const tokenBlock = (pattern) => Object.fromEntries(
  [...(cssClean.match(pattern)?.[1] ?? "").matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]),
);
const darkTokens = tokenBlock(/:root\s*\{([^}]*)\}/);
const lightTokens = { ...darkTokens, ...tokenBlock(/\[data-theme="light"\]\s*\{([^}]*)\}/) };
const lightOnly = tokenBlock(/\[data-theme="light"\]\s*\{([^}]*)\}/);
const BACKDROP_TOKENS = ["--backdrop-star", "--backdrop-line", "--backdrop-star-alpha", "--backdrop-line-alpha", "--backdrop-dust-alpha"];
check("⑫ token", () => {
  for (const name of BACKDROP_TOKENS) {
    assert.ok(name in darkTokens, `:root 缺 ${name}`);
    assert.ok(name in lightOnly, `[data-theme="light"] 缺 ${name}(两套主题都要给值)`);
  }
  for (const tokens of [darkTokens, lightTokens]) {
    for (const name of ["--backdrop-star", "--backdrop-line"]) assert.match(tokens[name], /^#[0-9a-fA-F]{6}$/, `${name} 必须是 6 位十六进制`);
    for (const name of ["--backdrop-star-alpha", "--backdrop-line-alpha", "--backdrop-dust-alpha"]) {
      const v = Number(tokens[name]);
      assert.ok(v > 0 && v <= 1, `${name} 必须在 (0,1]`);
    }
  }
});

// ⑬ 正文对比度:水印压在正文上时,正文压在「最亮一个像素」(两层叠加、不透明度偏好开到 150%)上仍 ≥ 4.5:1;
//    上限本身不能小到看不见(≥ .04);不夹时同样的像素会跌破 4.5(证明这道夹子是承重的)。
//    亮度 / 对比度用本文件自己的实现独立复算,不借 core 的函数。
const hexRgb = (value) => { const m = String(value).match(/^#([0-9a-fA-F]{6})$/); assert.ok(m, `不是 6 位十六进制:${value}`); return [0, 2, 4].map((i) => parseInt(m[1].slice(i, i + 2), 16)); };
const resolveHex = (tokens, name, seen = new Set()) => {
  const value = tokens[name];
  const alias = value?.match(/^var\((--[a-z0-9-]+)\)$/);
  return alias && !seen.has(alias[1]) ? resolveHex(tokens, alias[1], seen.add(name)) : hexRgb(value);
};
const lum = ([r, g, b]) => [r, g, b].reduce((s, c, i) => { const v = c / 255; return s + [0.2126, 0.7152, 0.0722][i] * (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4); }, 0);
const ratio = (a, b) => { const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x); return (hi + 0.05) / (lo + 0.05); };
const TEXT_TOKENS = ["--fg", "--fg-strong", "--dim", "--accent-text", "--ok", "--err", "--warn"];
check("⑬ 正文对比度", () => {
  const watermark = layoutBackdrop({ area: { x: 0, y: 0, w: 928, h: 560 }, mode: "conversation", column: { x: 80, y: 0, w: 768, h: 560 }, aspect: 1 });
  assert.ok(watermark.capped, "判据前提:窄屏水印应为 capped");
  const loud = { opacity: 1.5 };
  const violations = [];
  let uncappedBreaks = 0;
  for (const [theme, tokens] of [["暗色", darkTokens], ["亮色", lightTokens]]) {
    const palette = {
      star: resolveHex(tokens, "--backdrop-star"), line: resolveHex(tokens, "--backdrop-line"),
      pulse: resolveHex(tokens, "--accent"), error: resolveHex(tokens, "--err"),
      starAlpha: Number(tokens["--backdrop-star-alpha"]), lineAlpha: Number(tokens["--backdrop-line-alpha"]), dustAlpha: Number(tokens["--backdrop-dust-alpha"]),
      chatBg: resolveHex(tokens, "--chat-bg"), texts: TEXT_TOKENS.map((name) => resolveHex(tokens, name)),
    };
    const caps = textSafeCaps(palette, TEXT_CONTRAST_FLOOR);
    // 下限 .04:亮色 --ok(白底 5.38:1)把星点单层上限压到约 .047——窄窗口的水印在亮色下本就是若有若无,
    // 这是「不压正文」换来的;再低就等于没画,说明 token 或增益被改坏了。
    for (const kind of ["star", "line"]) {
      if (!(caps[kind] >= 0.04)) violations.push(`${theme} ${kind} 的上限 ${caps[kind].toFixed(3)} < .04,水印看不见`);
    }
    const peaks = peakAlphas(palette, watermark, loud, caps);
    const free = peakAlphas(palette, { ...watermark, capped: false }, loud, caps);
    for (const kind of ["star", "line", "pulse", "error"]) {
      const combined = 1 - (1 - peaks[kind]) ** OVERLAP_LAYERS;
      const under = palette.chatBg.map((c, i) => palette[kind][i] * combined + c * (1 - combined));
      const freeCombined = 1 - (1 - free[kind]) ** OVERLAP_LAYERS;
      const freeUnder = palette.chatBg.map((c, i) => palette[kind][i] * freeCombined + c * (1 - freeCombined));
      TEXT_TOKENS.forEach((name, index) => {
        const text = palette.texts[index];
        if (ratio(text, palette.chatBg) < TEXT_CONTRAST_FLOOR) return;
        const got = ratio(text, under);
        if (got < TEXT_CONTRAST_FLOOR - 1e-9) violations.push(`${theme} ${name} 压在最亮的 ${kind} 像素(${combined.toFixed(3)})上 ${got.toFixed(2)} < 4.5`);
        if (ratio(text, freeUnder) < TEXT_CONTRAST_FLOOR) uncappedBreaks += 1;
      });
    }
  }
  assert.deepEqual(violations, [], `正文对比度判据未通过:\n${violations.join("\n")}`);
  assert.ok(uncappedBreaks > 0, "判据自检:不夹 alpha 时也没有任何像素跌破 4.5——判据失去意义(token 或增益被改得太淡?)");
});

// ⑭ 预设解析:每个预设都给出合法模型;custom 缺点集回落标志
check("⑭ 预设解析", () => {
  for (const preset of ["kanzei", "big-dipper", "orion", "cassiopeia"]) validModel(resolveBackdropModel({ preset }, data));
  assert.equal(resolveBackdropModel({ preset: "custom" }, data).kind, "vector");
  const custom = { v: 1, kind: "image", points: [[0.1, 0.1, 2], [0.5, 0.9, 2], [0.9, 0.2, 3]], edges: [[0, 1, 0], [1, 2, 0]], hub: -1 };
  assert.equal(resolveBackdropModel({ preset: "custom", custom }, data).points.length, 3);
  const sparse = resolveBackdropModel({ preset: "kanzei", density: 0 }, data);
  const dense = resolveBackdropModel({ preset: "kanzei", density: 2 }, data);
  assert.ok(dense.points.length > sparse.points.length, "星点密度应影响标志笔画内的暗星数");
});

// ⑮ 静态契约:渲染器每一笔 alpha 都经 layerAlpha(水印时才夹得住);剪裁挖掉避让区;旧神经场的对话变体已删除
check("⑮ 静态契约", () => {
  const renderer = readFileSync(resolve(UI, "22-constellation.js"), "utf8").replace(/\/\/[^\n]*/g, "");
  const assigns = [...renderer.matchAll(/globalAlpha\s*=\s*([^;]+);/g)].map((m) => m[1].trim());
  assert.ok(assigns.length >= 6, "渲染器里找不到 globalAlpha 赋值(判据定位失效)");
  const raw = assigns.filter((expr) => expr !== "1" && !/^this\.a\(/.test(expr) && !/^layerAlpha\(/.test(expr));
  assert.deepEqual(raw, [], "globalAlpha 必须经 this.a(…)/layerAlpha(…) 赋值(否则水印时不受正文对比度上限约束)");
  assert.match(renderer, /clip\("evenodd"\)/, "渲染器必须用 evenodd 剪裁挖掉正文避让区");
  assert.match(renderer, /layout\.avoid/, "剪裁必须来自 layout.avoid");
  assert.doesNotMatch(renderer, /shadowBlur/, "星座背景不用 shadowBlur(每笔 Skia 模糊,改用预渲染星光小图)");
  const flow = readFileSync(resolve(UI, "22-neural-flow.js"), "utf8");
  assert.doesNotMatch(flow, /connectPortrait|addField\(chatCanvas|variant === "chat"/, "22-neural-flow.js 仍残留对话区神经场变体");
  assert.match(flow, /addField\(memoryCanvas, "memory"/, "记忆页神经场必须保留");
});

// 被 ui-runtime-smoke.mjs 链式 import 时,失败必须让 import 本身 reject(否则宿主照打「通过」);单独跑时只设退出码。
const standalone = resolve(process.argv[1] ?? "") === resolve(import.meta.filename);
if (failures.length) {
  console.error(`星座背景冒烟失败(${failures.length}):`);
  for (const failure of failures) console.error(` - ${failure}`);
  process.exitCode = 1;
  if (mutation) console.error(`[KZ_SMOKE_MUTATE=${mutateId}] 变异按预期被抓住`);
  if (!standalone) throw new Error(`星座背景冒烟失败(${failures.length} 处)`);
} else {
  if (mutation) {
    console.error(`[KZ_SMOKE_MUTATE=${mutateId}] 变异后仍全绿:这道护栏是恒绿的,先修断言`);
    process.exitCode = 1;
  } else {
    const logo = strokesToConstellation(KANZEI_LOGO_STROKES, { seed: 5 });
    console.log(`星座背景冒烟通过:标志 ${logo.points.length} 星 / ${logo.edges.length} 边,3 个真实星座,图片判型,构图与正文对比度(≥ ${TEXT_CONTRAST_FLOOR}:1),调度与静态契约`);
  }
}
