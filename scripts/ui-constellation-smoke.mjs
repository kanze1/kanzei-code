#!/usr/bin/env node
// UI2-0926 #10 对话背景(星座背景)冒烟:纯函数 + 静态契约。设计见 docs/design/ui_chat_backdrop.md。
// 单独跑:node scripts/ui-constellation-smoke.mjs;ui-runtime-smoke.mjs 末尾也会链式 import 它。
// 渲染器层面的承诺(帧预算、暂停、流式事件下不饿死、正文下像素、水印合成)在真浏览器里另由
// scripts/ui-constellation-browser-smoke.mjs 逐项实测(ui-lint-smoke 末尾调用)。
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
  // 近平行笔画不做 T 字桥接。用独立笔画夹具守住,不依赖品牌图案(⑤b)。
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
  // 水印合成不透明度:不透明度偏好只能往下调。放开到 150%,正文压在水印最亮处跌破 4.5:1(⑬)。
  bgTextCap: {
    pattern: /clamp\(Number\.isFinite\(opacity\) \? opacity : 1, 0\.2, 1\)/,
    replace: "clamp(Number.isFinite(opacity) ? opacity : 1, 0.2, 1.5)",
  },
  // 水印上限扣掉量化余量(8 位 alpha 与离屏重采样)。改成加上余量,星核压在正文上跌破 4.5:1(⑬)。
  bgWatermarkMargin: {
    pattern: /return Math\.max\(0, hi - WATERMARK_MARGIN\);/,
    replace: "return Math.max(0, hi + WATERMARK_MARGIN);",
  },
  // 只有 thinking / executing / replying 算忙。改回「非 idle 都算忙」,失败会话一直 30 帧/秒重画(⑯)。
  bgBusyActivity: {
    pattern: /return BUSY_ACTIVITIES\.includes\(activity\);/,
    replace: 'return activity !== "idle";',
  },
  // 失败会话的 hub 着 --err。改回「非 idle / complete 都着强调色」,失败被画成「在跑」(⑯)。
  bgHubTone: {
    pattern: /if \(activityBusy\(activity\)\) return still \? "pulse" : null;/,
    replace: 'if (activity !== "idle" && activity !== "complete") return still ? "pulse" : null;',
  },
  // 已排好的 rAF / 快到点的定时器不被新事件取消。删了判据,16ms 连发的流式事件把渲染器饿死(⑰)。
  bgHurryRaf: {
    pattern: /return Boolean\(timer && !raf && busy && dueIn > BUSY_FRAME_MS\);/,
    replace: "return Boolean(timer && busy);",
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
  seededRandom, frameDelay, watchDelay, resolveBackdropModel, watermarkCap, watermarkAlpha, rectsIntersect, columnFromRects,
  edgeDepthsFrom, activityBusy, hubTone, shouldHurry, IDLE_FRAME_MS, BUSY_FRAME_MS, TEXT_CONTRAST_FLOOR, WATERMARK_KINDS,
  WAKE_THROTTLE_MS,
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

// ⑤ 三模块标志:单连通、18~40 颗、三类角色路径都汇入共享空间。
check("⑤ kanzei 标志星座", () => {
  const m = strokesToConstellation(KANZEI_LOGO_STROKES, { seed: 5 });
  validModel(m);
  assert.equal(components(m.points.length, m.edges).length, 1, "标志星座应单连通");
  assert.ok(m.points.length >= 18 && m.points.length <= 40, `标志星数 ${m.points.length}`);
  assert.ok(m.points.filter((p) => p[2] <= 2.2).length >= 7, "端点/交汇亮星不足");
  const count = (r) => m.edges.filter((e) => e[2] === r).length;
  assert.ok(count(2) >= 6 && count(1) >= 2 && count(3) >= 2, `角色边数 trunk=${count(1)} memory=${count(2)} action=${count(3)}`);
  const hubEdges = m.edges.filter(([i, j]) => i === m.hub || j === m.hub);
  assert.ok(hubEdges.length >= 3, `hub 度数 ${hubEdges.length}`);
  for (const role of [1, 2, 3]) assert.ok(hubEdges.some((e) => e[2] === role), `共享空间缺少角色 ${role} 的输入`);
});

// ⑤b 通用笔画转换仍需保护近平行臂;换品牌不能让已有变异守卫失效。
check("⑤b 平行笔画", () => {
  const m = strokesToConstellation({
    viewBox: 64, hub: [21, 33],
    strokes: [
      { points: [[14, 8], [14, 56]], weight: 2, role: "trunk" },
      { points: [[21, 33], [44, 56]], weight: 2, role: "action" },
      { points: [[21.5, 31.5], [43, 8]], weight: 1, role: "memory" },
      { points: [[25.5, 35], [50, 8]], weight: 1, role: "memory" },
      { points: [[29.5, 38.5], [57, 8]], weight: 1, role: "memory" },
    ],
  }, { seed: 5 });
  const tips = [...m.points.keys()].filter((i) => m.points[i][0] > 0.5).sort((a, b) => m.points[a][1] - m.points[b][1]).slice(0, 3);
  assert.ok(!m.edges.some(([i, j]) => tips.includes(i) && tips.includes(j)), "平行臂尖之间被连了线");
  for (const tip of tips) assert.equal(m.edges.filter(([i, j]) => i === tip || j === tip).length, 1, "每颗臂尖应只连自己那条臂");
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

// ⑨ 构图:对话态关闭装饰;空态进槽或文案右侧空白。
check("⑨ 构图", () => {
  const area = { x: 0, y: 0, w: 1248, h: 760 };
  for (const w of [640, 928, 1005, 1248, 2400]) {
    const hidden = layoutBackdrop({ area: { ...area, w }, mode: "conversation" });
    assert.equal(hidden.placement, "hidden", `宽度 ${w} 的消息对话必须关闭装饰`);
    assert.equal(hidden.alpha, 0);
    assert.equal(hidden.box.w, 0);
    assert.equal(hidden.capped, false);
  }
  // 空态:进 art 槽且在槽内;文案登记为避让区
  const copy = { x: 190, y: 250, w: 400, h: 220 };
  const slot = layoutBackdrop({ area, mode: "welcome", slot: { x: 620, y: 180, w: 440, h: 380 }, copy, aspect: 1.2 });
  assert.equal(slot.placement, "slot");
  assert.ok(slot.box.x >= 620 && slot.box.x + slot.box.w <= 1060 && slot.box.y >= 180 && slot.box.y + slot.box.h <= 560);
  assert.ok(slot.avoid.length === 1 && !slot.capped);
  // 槽不可见(OC 关 / 窄容器):文案右侧空白够就放那里;语音模式同理
  const side = layoutBackdrop({ area, mode: "welcome", copy: { x: 354, y: 260, w: 540, h: 220 }, aspect: 1 });
  assert.equal(side.placement, "side");
  assert.ok(!rectsIntersect(side.box, { x: 354, y: 260, w: 540, h: 220 }));
  const voiceSide = layoutBackdrop({ area, mode: "voice", copy: { x: 304, y: 200, w: 640, h: 300 }, aspect: 1 });
  assert.equal(voiceSide.placement, "side", `语音模式 OC 关时应放文案右侧,实际 ${voiceSide.placement}`);
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

// ⑬ 正文对比度:水印压在正文上时,整张星座画进离屏层再以 watermarkAlpha 单一不透明度合成。这里按真实的
//    合成过程逐层复算:离屏层里任意叠放(星尘、连线、星、尾迹、光点头、失败 hub……每层 alpha ∈ (0,1]),
//    得到预乘色 Cp 与层 alpha A,再以 g 合成到 chat-bg:pixel = Cp·g + bg·(1 − g·A)。所有原本 ≥ 4.5 的字色
//    压在任何这样的像素上仍 ≥ 4.5(不透明度偏好开到 150%);上限本身不能小到看不见(≥ .06);
//    不走离屏、每层都按上限直接叠 5 层时必须有像素跌破 4.5(证明离屏合成是承重的)。
//    亮度 / 对比度 / 合成用本文件自己的实现独立复算,不借 core 的函数。
const hexRgb = (value) => { const m = String(value).match(/^#([0-9a-fA-F]{6})$/); assert.ok(m, `不是 6 位十六进制:${value}`); return [0, 2, 4].map((i) => parseInt(m[1].slice(i, i + 2), 16)); };
const resolveHex = (tokens, name, seen = new Set()) => {
  const value = tokens[name];
  const alias = value?.match(/^var\((--[a-z0-9-]+)\)$/);
  return alias && !seen.has(alias[1]) ? resolveHex(tokens, alias[1], seen.add(name)) : hexRgb(value);
};
const lum = ([r, g, b]) => [r, g, b].reduce((s, c, i) => { const v = c / 255; return s + [0.2126, 0.7152, 0.0722][i] * (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4); }, 0);
const ratio = (a, b) => { const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x); return (hi + 0.05) / (lo + 0.05); };
const TEXT_TOKENS = ["--fg", "--fg-strong", "--dim", "--accent-text", "--ok", "--err", "--warn"];
// 离屏层内 source-over 逐层叠(预乘):layers = [[rgb, alpha], …] → { cp: 预乘色, a: 层 alpha }
const stack = (layers) => layers.reduce(({ cp, a }, [rgb, alpha]) => ({
  cp: cp.map((c, i) => rgb[i] * alpha + c * (1 - alpha)),
  a: alpha + a * (1 - alpha),
}), { cp: [0, 0, 0], a: 0 });
const onBg = ({ cp, a }, g, bg) => bg.map((c, i) => cp[i] * g + c * (1 - g * a));
check("⑬ 正文对比度", () => {
  const watermark = layoutBackdrop({ area: { x: 0, y: 0, w: 600, h: 560 }, mode: "welcome", copy: { x: 40, y: 60, w: 520, h: 300 }, aspect: 1 });
  assert.ok(watermark.capped, "判据前提:窄屏欢迎装饰应为 capped");
  const violations = [];
  let directBreaks = 0;
  for (const [theme, tokens] of [["暗色", darkTokens], ["亮色", lightTokens]]) {
    const palette = {
      star: resolveHex(tokens, "--backdrop-star"), line: resolveHex(tokens, "--backdrop-line"),
      pulse: resolveHex(tokens, "--accent"), error: resolveHex(tokens, "--err"),
      starAlpha: Number(tokens["--backdrop-star-alpha"]), lineAlpha: Number(tokens["--backdrop-line-alpha"]), dustAlpha: Number(tokens["--backdrop-dust-alpha"]),
      chatBg: resolveHex(tokens, "--chat-bg"), texts: TEXT_TOKENS.map((name) => resolveHex(tokens, name)),
    };
    const cap = watermarkCap(palette, TEXT_CONTRAST_FLOOR);
    if (!(cap >= 0.06)) violations.push(`${theme} 水印上限 ${cap.toFixed(3)} < .06,水印看不见`);
    const g = watermarkAlpha(cap, 1.5);
    if (g > cap + 1e-12) violations.push(`${theme} 不透明度偏好 150% 时水印合成 ${g.toFixed(3)} 越过了上限 ${cap.toFixed(3)}`);
    const color = Object.fromEntries(WATERMARK_KINDS.map((kind) => [kind, palette[kind]]));
    // 具名的最坏叠放:光点压在星上(星尘 + 连线 + 星 + 尾迹 + 光点头)、失败 hub 叠在星上、点亮的边叠在底线上
    const named = {
      "光点压在星上": [[color.star, 1], [color.line, 1], [color.star, 1], [color.pulse, 1], [color.pulse, 1]],
      "失败 hub 叠星": [[color.star, 1], [color.line, 1], [color.error, 1], [color.error, 1]],
      "点亮的边叠底线": [[color.line, 1], [color.pulse, 0.74]],
      "只有星尘与星": [[color.star, 0.43], [color.star, 1]],
    };
    // 随机叠放:1~7 层,颜色任取,alpha 任取(种子固定,可复现)
    const rand = seededRandom(2026);
    const stacks = Object.entries(named);
    for (let k = 0; k < 4000; k += 1) {
      const depth = 1 + Math.floor(rand() * 7);
      stacks.push([`随机#${k}`, Array.from({ length: depth }, () => [color[WATERMARK_KINDS[Math.floor(rand() * WATERMARK_KINDS.length)]], 0.05 + rand() * 0.95])]);
    }
    for (const [label, layers] of stacks) {
      const pixel = onBg(stack(layers), g, palette.chatBg);
      TEXT_TOKENS.forEach((name, index) => {
        const text = palette.texts[index];
        if (ratio(text, palette.chatBg) < TEXT_CONTRAST_FLOOR) return;
        const got = ratio(text, pixel);
        if (got < TEXT_CONTRAST_FLOOR - 1e-9 && violations.length < 12) violations.push(`${theme} ${name} 压在水印「${label}」像素上 ${got.toFixed(2)} < 4.5`);
      });
    }
    // 判据自检:不走离屏,每层按上限直接叠到画布上(光点压在星上那 5 层)
    const direct = named["光点压在星上"].reduce((bg, [rgb]) => bg.map((c, i) => rgb[i] * cap + c * (1 - cap)), palette.chatBg);
    TEXT_TOKENS.forEach((name, index) => {
      const text = palette.texts[index];
      if (ratio(text, palette.chatBg) >= TEXT_CONTRAST_FLOOR && ratio(text, direct) < TEXT_CONTRAST_FLOOR) directBreaks += 1;
    });
  }
  assert.deepEqual(violations, [], `正文对比度判据未通过:\n${violations.join("\n")}`);
  assert.ok(directBreaks > 0, "判据自检:逐层直接叠也没有任何像素跌破 4.5——判据失去意义(token 被改得太淡?)");
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

// ⑮ 静态契约:渲染器每一笔 alpha 都经 layerAlpha;水印走离屏层 + watermarkAlpha 单一合成;剪裁挖掉避让区;
//    旧神经场的对话变体已删除
check("⑮ 静态契约", () => {
  const renderer = readFileSync(resolve(UI, "22-constellation.js"), "utf8").replace(/\/\/[^\n]*/g, "");
  const assigns = [...renderer.matchAll(/globalAlpha\s*=\s*([^;]+);/g)].map((m) => m[1].trim());
  assert.ok(assigns.length >= 6, "渲染器里找不到 globalAlpha 赋值(判据定位失效)");
  const raw = assigns.filter((expr) => expr !== "1" && !/^this\.a\(/.test(expr) && !/^layerAlpha\(/.test(expr) && !/^watermarkAlpha\(/.test(expr));
  assert.deepEqual(raw, [], "globalAlpha 必须经 this.a(…)/layerAlpha(…)/watermarkAlpha(…) 赋值");
  assert.equal(assigns.filter((expr) => /^watermarkAlpha\(/.test(expr)).length, 1, "水印只能在一处以 watermarkAlpha 合成");
  assert.match(renderer, /if \(shown\.capped\) paintWatermark\(frame\);/, "capped 构图必须走离屏水印");
  assert.match(renderer, /painter\.paint\(lctx, frame\);\s*ctx\.globalAlpha = watermarkAlpha\([^;]+;\s*ctx\.drawImage\(layer,/, "水印必须先画进离屏层、再以 watermarkAlpha 一次合成");
  assert.match(renderer, /clip\("evenodd"\)/, "渲染器必须用 evenodd 剪裁挖掉正文避让区");
  assert.match(renderer, /layout\.avoid/, "剪裁必须来自 layout.avoid");
  assert.doesNotMatch(renderer, /shadowBlur/, "星座背景不用 shadowBlur(每笔 Skia 模糊,改用预渲染星光小图)");
  const flow = readFileSync(resolve(UI, "22-neural-flow.js"), "utf8");
  assert.doesNotMatch(flow, /connectPortrait|addField\(chatCanvas|variant === "chat"/, "22-neural-flow.js 仍残留对话区神经场变体");
  assert.match(flow, /addField\(memoryCanvas, "memory"/, "记忆页神经场必须保留");
});

// ⑯ 活动态:只有 thinking / executing / replying 算忙;blocked(失败)与 complete 不算;
//    hub 静帧着色:运行中且静帧 → 强调色;失败 → --err(任何模式);失败绝不着强调色
check("⑯ 活动态", () => {
  for (const busy of ["thinking", "executing", "replying"]) assert.equal(activityBusy(busy), true, `${busy} 应算忙`);
  for (const calm of ["idle", "blocked", "complete", undefined]) assert.equal(activityBusy(calm), false, `${calm} 不应算忙(失败会话停在那里会一直 30 帧/秒)`);
  assert.equal(hubTone("executing", { still: true }), "pulse");
  assert.equal(hubTone("executing", { still: false }), null, "动画模式下运行态由光点表达");
  for (const still of [true, false]) {
    assert.equal(hubTone("blocked", { still }), "error", "失败会话 hub 应着淡 --err");
    assert.equal(hubTone("complete", { still }), null);
    assert.equal(hubTone("idle", { still }), null);
  }
});

// ⑰ 帧调度:新事件只把「还在等、离触发还比忙帧更久」的定时器提前;已排好的 rAF、快到点的定时器不动;
//    流式唤醒节流 = 一个忙帧
check("⑰ 帧不被饿死", () => {
  assert.equal(shouldHurry({ timer: 7, raf: 0, busy: true, dueIn: IDLE_FRAME_MS }), true, "空闲等待中来了忙事件应提前");
  assert.equal(shouldHurry({ timer: 7, raf: 0, busy: true, dueIn: BUSY_FRAME_MS - 16 }), false, "一个忙帧内就会触发的定时器不能取消重排");
  assert.equal(shouldHurry({ timer: 0, raf: 9, busy: true, dueIn: 0 }), false, "已排好的 rAF 永不取消");
  assert.equal(shouldHurry({ timer: 7, raf: 9, busy: true, dueIn: IDLE_FRAME_MS }), false, "已排好的 rAF 永不取消");
  assert.equal(shouldHurry({ timer: 7, raf: 0, busy: false, dueIn: IDLE_FRAME_MS }), false, "不忙就按空闲节奏");
  assert.ok(WAKE_THROTTLE_MS >= BUSY_FRAME_MS, "流式唤醒节流不能比忙帧更密");
  // 16ms 一次的流式事件连发 1 秒:按 shouldHurry 模拟「定时器待发时每个事件问一次要不要提前」,帧数应接近 1000/33
  let due = IDLE_FRAME_MS, frames = 0;
  for (let now = 0; now < 1000; now += 1) {
    if (now % 16 === 0 && shouldHurry({ timer: 1, raf: 0, busy: true, dueIn: due - now })) due = now + BUSY_FRAME_MS;
    if (now >= due) { frames += 1; due = now + BUSY_FRAME_MS; }
  }
  assert.ok(frames >= 20, `16ms 连发流式事件时 1 秒只画了 ${frames} 帧(渲染器被饿死)`);
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
