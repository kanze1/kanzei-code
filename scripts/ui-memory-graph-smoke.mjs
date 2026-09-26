#!/usr/bin/env node
// 记忆图谱冒烟(docs/design/memory_knowledge_graph.md §11)。
//
//   node scripts/ui-memory-graph-smoke.mjs            纯模型测试 + vendor 校验 + 变异自检(秒级,无浏览器)
//   node scripts/ui-memory-graph-smoke.mjs --browser  另起无头 Edge 打开 ui-preview 的 memory-graph 场景做真渲染断言
//   KZ_GRAPH_MUTATE=<id> node scripts/ui-memory-graph-smoke.mjs
//                                                     用删掉被守护代码的模型跑全套测试,期望非零退出(手工复核守卫)
//
// ① 纯模型(ui/24-memory-graph-model.js,零 import):默认视图、各筛选、N 跳邻域、层带锚点有限值、凸包、
//    标签去重叠、检索、文本视图分组、文案 key 都在 I18N_EN。夹具是本仓真实记忆的子集(ui-preview/memory-graph-fixture.mjs)。
// ② vendor:force-graph 文件的 SHA-256 与 README 声明一致(换行按 LF 归一再算——Windows 上 autocrlf 检出会改成 CRLF),
//    LICENSE 是 MIT 原文,24-graph-view.js 的 FORCE_GRAPH_URL 指向这个文件。
// ③ 变异守卫:每个变异删掉一处被守护的代码(必须恰好命中一处),对应测试必须变红——恒绿的断言与真护栏长得一样。
// ④ --browser:vendor 请求 200、零 pageerror/console.error、布局稳定且 settleMs < 3000、画布非空白、
//    程序化悬停写状态栏、文本视图的记忆条目数等于可见记忆数、切主题后画布像素变化、~700 节点/1800 边(区域表补到
//    真实量级 200+)稳定 < 3000ms;复核补:默认视图里记忆编号标签可见、没选中时详情栏收成竖排页签栏、选中后展开、
//    悬停时的边标签不与节点标签重叠、dag-lr 遇环不抛错且能稳定、减少动效时布局全在首帧前跑完(ticks = 0)。
/* global window, document */
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const UI = path.join(root, "crates/kanzei-app/ui");
const MODEL_PATH = path.join(UI, "24-memory-graph-model.js");
const VENDOR_DIR = path.join(UI, "vendor/force-graph");
const VENDOR_FILE = "force-graph-1.51.4.min.js";
const VENDOR_SHA256 = "1008539bb9e171a0dc343453366451a1b3a6ded06028ef4f978608b658ba2d0a";
const { MEMORY_GRAPH_FIXTURE: FIXTURE } = await import(pathToFileURL(path.join(root, "scripts/ui-preview/memory-graph-fixture.mjs")).href);

// ---------- 变异表:pattern 必须在模型源码里恰好命中一处 ----------
const MUTATIONS = {
  // 没有可见 crate 时未归类锚点回退为 0。删掉回退,Math.max() 给出 -Infinity,d3 四叉树死循环。
  anchor_finite: {
    pattern: /const right = xs\.length \? Math\.max\(\.\.\.xs\) \+ COL_W \* 0\.8 : 0;/,
    replace: "const right = Math.max(...xs) + COL_W * 0.8;",
    test: "bandAnchors 对空输入、单个 crate、非数字带号都给有限坐标",
  },
  // 邻域遍历不走 contains(除非中心是 crate)。删掉,一跳就把整个 crate 的模块拉进来。
  ego_contains: {
    pattern: /if \(edge\.rel === "contains"\) return from === center && byId\.get\(center\)\?\.kind === "crate";/,
    replace: 'if (edge.rel === "contains") return true;',
    test: "egoSubgraph 的 1 跳与 2 跳计数,contains 只在中心为 crate 时走",
  },
  // 默认图层不含「提及」(弱边太多,默认画面会成一团)。改成默认带上,默认视图计数必须变。
  default_mentions: {
    pattern: /export const DEFAULT_LAYERS = Object\.freeze\(\["about", "refs", "supersedes", "concepts"\]\);/,
    replace: 'export const DEFAULT_LAYERS = Object.freeze(["about", "refs", "supersedes", "concepts", "mentions"]);',
    test: "默认视图:只含可见记忆、不含提及、crate 只在有记忆时出现",
  },
  // 初始抖动必须确定(复核:Math.random 让同一份数据每次打开排出不同的图)。换回随机数,同一 id 两次结果不同。
  jitter_seeded: {
    pattern: /return \[fmix\(h\) \/ 4294967296 - 0\.5, fmix\(h \^ 0x9e3779b9\) \/ 4294967296 - 0\.5\];/,
    replace: "return [Math.random() - 0.5, Math.random() - 0.5];",
    test: "确定性抖动:同一 id 恒定、落在 [-0.5, 0.5)、连号 id 散开",
  },
  // 哈希后要过 fmix32 雪崩。去掉它,连号 id(M-001、M-002…)的抖动几乎相同,新节点排成一条斜线。
  jitter_avalanche: {
    pattern: /return \[fmix\(h\) \/ 4294967296 - 0\.5, fmix\(h \^ 0x9e3779b9\) \/ 4294967296 - 0\.5\];/,
    replace: "return [h / 4294967296 - 0.5, ((h ^ 0x9e3779b9) >>> 0) / 4294967296 - 0.5];",
    test: "确定性抖动:同一 id 恒定、落在 [-0.5, 0.5)、连号 id 散开",
  },
};

function mutatedModelUrl(id) {
  const mutation = MUTATIONS[id];
  assert.ok(mutation, `未知的 KZ_GRAPH_MUTATE=${id}(可用:${Object.keys(MUTATIONS).join(" / ")})`);
  const source = readFileSync(MODEL_PATH, "utf8");
  const hits = source.match(new RegExp(mutation.pattern.source, "g"))?.length ?? 0;
  assert.equal(hits, 1, `变异 ${id} 没有恰好命中一处被守护的源码(实得 ${hits} 处):护栏已经失效,先修变异表`);
  const dir = mkdtempSync(path.join(tmpdir(), "kz-graph-mutate-"));
  const file = path.join(dir, `model-${id}.mjs`);
  writeFileSync(file, source.replace(mutation.pattern, mutation.replace));
  return { url: pathToFileURL(file).href, cleanup: () => rmSync(dir, { recursive: true, force: true }) };
}

// ---------- 小型合成载荷(形状同后端 memory_graph)----------
function node(id, kind, extra = {}) {
  return {
    id, kind, label: extra.label ?? id, title: extra.title ?? id, description: null, scope: null, category: null,
    status: null, archived: false, updated: null, hits: 0, areas: [], primary_area: null, area_provenance: null, degree: 0, ...extra,
  };
}
function edge(source, target, rel, extra = {}) {
  return { source, target, rel, strength: "strong", provenance: null, via: null, anchor: null, ...extra };
}
function memory(id, extra = {}) {
  return node(id, "memory", { scope: "project", category: "fact", status: "active", ...extra });
}
const SMALL = {
  areas: [
    { id: "area:tools", kind: "crate", label: "tools", parent: null, depth: 1, band: 2, memories: 2 },
    { id: "area:tools/edit", kind: "module", label: "edit", parent: "area:tools", depth: 1, band: 2, memories: 2 },
    { id: "area:tools/bash", kind: "module", label: "bash", parent: "area:tools", depth: 1, band: 2, memories: 1 },
    { id: "area:base", kind: "crate", label: "base", parent: null, depth: 0, band: 0, memories: 0 },
  ],
  nodes: [
    node("area:tools", "crate"), node("area:tools/edit", "module"), node("area:tools/bash", "module"), node("area:base", "crate"),
    memory("M-1", { areas: ["area:tools/edit"], primary_area: "area:tools/edit", area_provenance: "tool", title: "edit 报错先读" }),
    memory("M-2", { areas: ["area:tools/edit", "area:tools/bash"], primary_area: "area:tools/edit", area_provenance: "path" }),
    memory("M-3", { status: "candidate", category: "sop", areas: [], title: "未归类候选" }),
    memory("M-4", { archived: true, status: "deprecated", areas: ["area:tools/bash"], primary_area: "area:tools/bash", area_provenance: "tool" }),
    memory("U-1", { scope: "global", category: "preference", areas: [] }),
    node("R-1", "requirement", { status: "doing" }),
    node("fp:edit|x", "fingerprint"),
  ],
  edges: [
    edge("area:tools", "area:tools/edit", "contains"), edge("area:tools", "area:tools/bash", "contains"),
    edge("area:tools", "area:base", "depends_on"),
    edge("M-1", "area:tools/edit", "about", { strength: "weak", provenance: "tool" }),
    edge("M-2", "area:tools/edit", "about", { provenance: "path" }), edge("M-2", "area:tools/bash", "about", { provenance: "path" }),
    edge("M-4", "area:tools/bash", "about", { strength: "weak", provenance: "tool" }),
    edge("M-1", "R-1", "refs"), edge("M-3", "R-1", "mentions", { strength: "weak" }),
    edge("M-1", "fp:edit|x", "has_fingerprint"), edge("M-2", "fp:edit|x", "has_fingerprint"),
    edge("M-2", "M-4", "supersedes"),
  ],
};

// ---------- 测试 ----------
function buildTests(m) {
  const ids = (graph) => graph.nodes.map((n) => n.id).sort();
  const defaults = { scope: "project", category: "all", status: "all", archived: false, layers: m.DEFAULT_LAYERS, area: "" };
  return {
    "默认视图:只含可见记忆、不含提及、crate 只在有记忆时出现"() {
      const small = m.visibleGraph(SMALL, defaults);
      assert.deepEqual(ids(small), ["M-1", "M-2", "M-3", "R-1", "area:tools", "area:tools/bash", "area:tools/edit", "fp:edit|x"]);
      assert.ok(!small.links.some((l) => l.rel === "mentions"), "默认图层不画提及");
      assert.ok(!small.links.some((l) => l.rel === "depends_on"), "依赖由层带位置表达,不画线");
      assert.ok(!small.nodes.some((n) => n.id === "area:base"), "没有记忆的 crate 不出现");
      const expected = FIXTURE.nodes.filter((n) => n.kind === "memory" && n.scope === "project" && !n.archived).length;
      const real = m.visibleGraph(FIXTURE, defaults);
      assert.equal(real.nodes.filter((n) => n.kind === "memory").length, expected, "夹具默认视图的记忆数");
      assert.ok(!real.links.some((l) => l.rel === "mentions"), "夹具默认视图不含提及");
      const visible = new Set(real.nodes.map((n) => n.id));
      assert.ok(real.links.every((l) => visible.has(l.source) && visible.has(l.target)), "边的两端都可见");
      const withMentions = m.visibleGraph(FIXTURE, { ...defaults, layers: [...m.DEFAULT_LAYERS, "mentions"] });
      assert.ok(withMentions.links.some((l) => l.rel === "mentions"), "打开提及图层后才出现提及边");
    },
    "筛选:范围、分类、状态、含归档、图层、区域"() {
      assert.deepEqual(m.visibleGraph(SMALL, { ...defaults, scope: "global" }).nodes.filter((n) => n.kind === "memory").map((n) => n.id), ["U-1"]);
      assert.deepEqual(m.visibleGraph(SMALL, { ...defaults, scope: "all", category: "sop" }).nodes.filter((n) => n.kind === "memory").map((n) => n.id), ["M-3"]);
      assert.deepEqual(m.visibleGraph(SMALL, { ...defaults, status: "active" }).nodes.filter((n) => n.kind === "memory").map((n) => n.id), ["M-1", "M-2"]);
      assert.ok(!m.visibleGraph(SMALL, defaults).nodes.some((n) => n.id === "M-4"), "归档默认不出现");
      assert.ok(m.visibleGraph(SMALL, { ...defaults, archived: true }).nodes.some((n) => n.id === "M-4"), "含归档后出现");
      assert.ok(m.visibleGraph(SMALL, { ...defaults, status: "active", archived: true }).nodes.some((n) => n.id === "M-4"), "归档条目只看「含归档」开关,不受状态筛选影响");
      assert.ok(!m.visibleGraph(SMALL, { ...defaults, status: "stale" }).nodes.some((n) => n.kind === "memory"), "状态「失效」只筛活动目录里的失效条目");
      const noConcepts = m.visibleGraph(SMALL, { ...defaults, layers: ["about", "refs", "supersedes"] });
      assert.ok(!noConcepts.nodes.some((n) => n.kind === "fingerprint"), "关掉指纹图层,概念节点消失");
      const noAbout = m.visibleGraph(SMALL, { ...defaults, layers: ["refs"] });
      assert.ok(!noAbout.nodes.some((n) => n.kind === "module"), "关掉关于图层,模块不再出现");
      assert.deepEqual(m.visibleGraph(SMALL, { ...defaults, area: "area:tools/bash" }).nodes.filter((n) => n.kind === "memory").map((n) => n.id), ["M-2"]);
      assert.deepEqual(m.visibleGraph(SMALL, { ...defaults, area: "area:tools" }).nodes.filter((n) => n.kind === "memory").map((n) => n.id), ["M-1", "M-2"], "crate 级区域包含其模块");
    },
    "egoSubgraph 的 1 跳与 2 跳计数,contains 只在中心为 crate 时走"() {
      const one = m.egoSubgraph(SMALL, "M-1", 1, m.DEFAULT_LAYERS);
      assert.deepEqual(ids(one), ["M-1", "R-1", "area:tools/edit", "fp:edit|x"]);
      assert.equal(one.hopOf.get("M-1"), 0);
      const two = m.egoSubgraph(SMALL, "M-1", 2, m.DEFAULT_LAYERS);
      assert.deepEqual(ids(two), ["M-1", "M-2", "R-1", "area:tools/edit", "fp:edit|x"], "2 跳到 M-2,不经 contains 走到 crate");
      assert.ok(!two.links.some((l) => l.rel === "contains"), "中心不是 crate 时不画 contains");
      const crate = m.egoSubgraph(SMALL, "area:tools", 1, m.DEFAULT_LAYERS);
      assert.deepEqual(ids(crate), ["area:tools", "area:tools/bash", "area:tools/edit"], "中心是 crate 时沿 contains 走一跳");
      assert.equal(m.egoSubgraph(SMALL, "nope", 2).nodes.length, 0);
    },
    "bandAnchors 对空输入、单个 crate、非数字带号都给有限坐标"() {
      const finitePoint = (p) => Number.isFinite(p.x) && Number.isFinite(p.y);
      const empty = m.bandAnchors([]);
      assert.equal(empty.anchors.size, 0);
      assert.ok(finitePoint(empty.unassigned), `空输入的未归类锚点必须有限:${JSON.stringify(empty.unassigned)}`);
      const single = m.bandAnchors([{ id: "area:a", kind: "crate", band: 3 }]);
      assert.deepEqual(single.anchors.get("area:a"), { x: 0, y: 0 });
      assert.ok(finitePoint(single.unassigned));
      const odd = m.bandAnchors([{ id: "area:x", kind: "crate", band: "nan" }, { id: "area:y", kind: "crate", band: Infinity }, { id: "area:z", kind: "crate", band: 2 }]);
      assert.ok([...odd.anchors.values(), odd.unassigned].every(finitePoint), "非数字带号按 0 带处理");
      const layered = m.bandAnchors(SMALL.areas);
      assert.ok(layered.anchors.get("area:tools").y < layered.anchors.get("area:base").y, "入口层在上、基础层在下");
      assert.ok(!layered.anchors.has("area:tools/edit"), "模块不钉锚点");
    },
    "凸包:正方形加内点只留 4 个顶点"() {
      const hull = m.convexHull([[0, 0], [10, 0], [10, 10], [0, 10], [5, 5], [3, 7]]);
      assert.equal(hull.length, 4);
      assert.deepEqual(m.convexHull([[1, 1], [2, 2]]).length, 2);
      assert.equal(m.convexHull([[0, 0], [NaN, 1], [1, 1], [0, 1]]).length, 3, "非有限点剔除");
    },
    "标签去重叠:高优先级先占位,重叠的被剔除"() {
      const placed = m.placeLabels([
        { id: "low", x: 50, y: 50, w: 40, h: 12, priority: 1 },
        { id: "high", x: 55, y: 52, w: 40, h: 12, priority: 9 },
        { id: "far", x: 400, y: 400, w: 40, h: 12, priority: 0 },
        { id: "bad", x: NaN, y: 0, w: 1, h: 1, priority: 99 },
      ], 32);
      assert.deepEqual([...placed].sort(), ["far", "high"]);
    },
    "检索:按 id、标题、区域命中,记忆排前"() {
      assert.deepEqual(m.searchNodes(SMALL, "m-1"), ["M-1"]);
      assert.deepEqual(m.searchNodes(SMALL, "报错"), ["M-1"]);
      assert.deepEqual(m.searchNodes(SMALL, "tools/bash"), ["M-2", "M-4", "area:tools/bash"]);
      assert.deepEqual(m.searchNodes(SMALL, "   "), []);
    },
    "文本视图:按 crate → 模块分组,条目数等于可见记忆数"() {
      const visible = m.visibleGraph(FIXTURE, defaults);
      const groups = m.groupForTextView(visible.nodes, FIXTURE);
      const count = groups.reduce((sum, g) => sum + g.memories.length + g.modules.reduce((s, mod) => s + mod.memories.length, 0), 0);
      assert.equal(count, visible.nodes.filter((n) => n.kind === "memory").length);
      assert.ok(groups.length >= 2);
      const small = m.groupForTextView(m.visibleGraph(SMALL, defaults).nodes, SMALL);
      assert.equal(small.at(-1).id, "", "未归类组排最后");
      assert.deepEqual(small.at(-1).memories.map((n) => n.id), ["M-3"]);
    },
    "节点外观:分类取色、候选空心、归档半透明虚线"() {
      assert.equal(m.nodeStyle(memory("A")).fill, true);
      assert.equal(m.nodeStyle(memory("A", { category: "sop" })).color, "sop");
      assert.equal(m.nodeStyle(memory("A", { status: "candidate" })).fill, false);
      const faded = m.nodeStyle(memory("A", { archived: true, status: "deprecated" }));
      assert.ok(faded.dashed && faded.alpha < 1);
      assert.equal(m.nodeStyle(node("R-1", "requirement")).glyph, "R");
      assert.equal(m.nodeStyle(node("fp:x", "fingerprint")).shape, "diamond");
      assert.ok(m.nodeRadius(memory("A", { hits: 1000 })) <= 12, "命中多的记忆变大但有上限");
    },
    "确定性抖动:同一 id 恒定、落在 [-0.5, 0.5)、连号 id 散开"() {
      assert.deepEqual(m.seededJitter("M-009"), m.seededJitter("M-009"), "同一 id 必须给同一抖动");
      assert.notDeepEqual(m.seededJitter("M-009"), m.seededJitter("M-010"));
      const pts = Array.from({ length: 200 }, (_, i) => m.seededJitter(`M-${String(i).padStart(3, "0")}`));
      assert.ok(pts.every(([x, y]) => x >= -0.5 && x < 0.5 && y >= -0.5 && y < 0.5), "抖动超出 [-0.5, 0.5)");
      const cells = new Set(pts.map(([x, y]) => `${Math.floor((x + 0.5) * 4)},${Math.floor((y + 0.5) * 4)}`));
      assert.equal(cells.size, 16, `200 个连号 id 应铺满 4×4 格,实得 ${cells.size} 格`);
      const meanStep = pts.slice(1).reduce((sum, p, i) => sum + Math.hypot(p[0] - pts[i][0], p[1] - pts[i][1]), 0) / (pts.length - 1);
      assert.ok(meanStep > 0.25, `连号 id 相邻两点的平均距离只有 ${meanStep.toFixed(3)}(没过雪崩的 FNV-1a 会排成一条斜线)`);
    },
    "文案 key 全在 I18N_EN"() {
      const i18n = readFileSync(path.join(UI, "02-i18n.js"), "utf8");
      const body = i18n.match(/export const I18N_EN = \{([\s\S]*?)\n\};/)?.[1] ?? "";
      const keys = new Set([...body.matchAll(/"((?:\\.|[^"])*)"\s*:/g)].map((x) => x[1]));
      assert.ok(keys.size > 500, "I18N_EN 解析失败");
      const missing = [m.REL_KEYS, m.PROVENANCE_KEYS, m.KIND_KEYS, m.LAYER_KEYS].flatMap((table) => Object.values(table)).filter((key) => !keys.has(key));
      assert.deepEqual([...new Set(missing)], [], "图谱文案 key 未进 I18N_EN(英文界面会直接显示中文)");
      assert.deepEqual(Object.keys(m.REL_KEYS).sort(), ["about", "basis", "cites", "contains", "depends_on", "derived_from", "has_fingerprint", "has_subject", "implements", "mentions", "refs", "supersedes"], "关系词表与后端 GraphEdge.rel 对齐");
    },
  };
}

async function runTests(modelUrl) {
  const m = await import(modelUrl);
  const tests = buildTests(m);
  const failures = [];
  for (const [name, fn] of Object.entries(tests)) {
    try {
      await fn();
    } catch (error) {
      failures.push({ name, message: error.message.split("\n")[0] });
    }
  }
  return { failures, count: Object.keys(tests).length };
}

function checkVendor() {
  const bytes = readFileSync(path.join(VENDOR_DIR, VENDOR_FILE));
  const normalized = Buffer.from(bytes.toString("latin1").replace(/\r\n/g, "\n"), "latin1");
  const sha = createHash("sha256").update(normalized).digest("hex");
  assert.equal(sha, VENDOR_SHA256, `vendor/force-graph/${VENDOR_FILE} 的 SHA-256 变了(${sha}):换版本要同时改 README、24-graph-view.js 与本脚本常量`);
  assert.equal(normalized.length, 177599, "vendor 文件长度与上游 dist 不一致");
  const readme = readFileSync(path.join(VENDOR_DIR, "README.md"), "utf8");
  assert.ok(readme.includes(VENDOR_SHA256) && readme.includes("1.51.4") && readme.includes("MIT"), "vendor README 必须写明版本、许可与 SHA-256");
  const license = readFileSync(path.join(VENDOR_DIR, "LICENSE"), "utf8");
  assert.ok(license.includes("MIT License") && license.includes("Vasco Asturiano"), "vendor LICENSE 必须是上游 MIT 原文");
  const view = readFileSync(path.join(UI, "24-graph-view.js"), "utf8");
  assert.ok(view.includes(`vendor/force-graph/${VENDOR_FILE}`), "24-graph-view.js 的 FORCE_GRAPH_URL 必须指向 vendor 文件");
  assert.ok(/new Function\("module", "exports", "define"/.test(view), "force-graph 必须经 CommonJS 垫片加载(Monaco 的 define.amd 会劫持 UMD)");
  assert.ok(!/Math\.random/.test(view) && /seededJitter\(node\.id\)/.test(view), "24-graph-view.js 的初始位置必须用 seededJitter(节点 id),不得用 Math.random(同一份数据每次要排出同一张图)");
  assert.ok(/\.onDagError\(/.test(view), "24-graph-view.js 必须设 onDagError:库默认遇环直接 throw,引用图天然有环");
}

// ---------- 浏览器 ----------
function syntheticPayload(memories = 460, extraEdges = 1300) {
  const crates = ["app", "ui", "scripts", "tools", "memory", "core", "harness", "llm", "base"];
  const bands = { app: 3, ui: 3, scripts: 3, tools: 2, memory: 2, core: 1, harness: 1, llm: 1, base: 0 };
  const areas = [];
  const nodes = [];
  const edges = [];
  let seed = 7;
  const rand = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
  for (const c of crates) {
    areas.push({ id: `area:${c}`, kind: "crate", label: c, parent: null, depth: bands[c], band: bands[c], memories: 1 });
    nodes.push(node(`area:${c}`, "crate", { label: c }));
    for (let i = 0; i < 12; i += 1) {
      const id = `area:${c}/m${i}`;
      areas.push({ id, kind: "module", label: `m${i}`, parent: `area:${c}`, depth: bands[c], band: bands[c], memories: 1 });
      nodes.push(node(id, "module", { label: `m${i}` }));
      edges.push(edge(`area:${c}`, id, "contains"));
    }
  }
  // 真实注册表约 204 个区域,大部分模块没有记忆、只出现在 areas 里(「设为区域」下拉用):补到同一量级,
  // 聚类力与外壳按区域表查 crate 的开销才测得到(复核:每次新建区域 Map 时稳定时间多约 25%)。
  for (const c of crates) {
    for (let i = 12; i < 24; i += 1) areas.push({ id: `area:${c}/unused${i}`, kind: "module", label: `unused${i}`, parent: `area:${c}`, depth: bands[c], band: bands[c], memories: 0 });
  }
  const categories = ["fact", "sop", "habit", "preference"];
  for (let i = 0; i < memories; i += 1) {
    const c = crates[Math.floor(rand() * crates.length)];
    const area = `area:${c}/m${Math.floor(rand() * 12)}`;
    nodes.push(memory(`M-${1000 + i}`, { category: categories[i % 4], status: i % 5 ? "active" : "candidate", areas: [area], primary_area: area, area_provenance: "tool" }));
    edges.push(edge(`M-${1000 + i}`, area, "about", { strength: "weak", provenance: "tool" }));
  }
  for (let i = 0; i < 100; i += 1) nodes.push(node(`R-${100 + i}`, "requirement"));
  for (let i = 0; i < extraEdges; i += 1) {
    const a = `M-${1000 + Math.floor(rand() * memories)}`;
    const b = rand() < 0.5 ? `R-${100 + Math.floor(rand() * 100)}` : `M-${1000 + Math.floor(rand() * memories)}`;
    if (a !== b) edges.push(edge(a, b, "refs"));
  }
  return { version: 1, generated_at: 0, build_ms: 0, cache: "miss", areas, nodes, edges, stats: {}, warnings: [] };
}

/**
 * 用真鼠标悬停一个记忆节点:返回 { id, x, y, stable, heights }。库在动画帧里用影子画布认出指针下的节点,影子画布
 * 按 800ms 节流重画(适配视图之后可能还是旧的),节点也可能被别的节点盖住:先等一个节流周期,再逐个记忆节点试,
 * 状态栏写出它(悬停生效)就停。stable = 再等 500ms 状态栏仍是它、画布高度没变(悬停文字改变页脚高度 → 画布
 * 变高变矮 → 库清掉悬停 → 来回闪,复核修复时在 1600 宽实测到)。
 */
async function hoverMemory(page) {
  await page.waitForTimeout(900);
  const candidates = await page.evaluate(() => window.__kzMemoryGraph.positions().filter((p) => p.kind === "memory").map((p) => p.id));
  const status = () => page.evaluate(() => document.getElementById("memory-graph-status")?.textContent ?? "");
  const canvasHeight = () => page.evaluate(() => document.getElementById("memory-graph-canvas").getBoundingClientRect().height);
  const heightBefore = await canvasHeight();
  for (const id of candidates.slice(0, 12)) {
    const point = await page.evaluate((nodeId) => {
      const screen = window.__kzMemoryGraph.screenOf(nodeId);
      const box = document.querySelector("#memory-graph-canvas canvas").getBoundingClientRect();
      return screen ? { x: box.left + screen.x, y: box.top + screen.y } : null;
    }, id);
    if (!point) continue;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await page.mouse.move(point.x + (attempt % 2 ? 1 : -1), point.y);
      await page.waitForTimeout(80);
      if (!(await status()).startsWith(`${id} `)) continue;
      await page.waitForTimeout(500);
      const heightAfter = await canvasHeight();
      return { id, ...point, stable: (await status()).startsWith(`${id} `) && heightAfter === heightBefore, heights: [heightBefore, heightAfter] };
    }
  }
  return null;
}

/** 真拖一次记忆节点:返回 { id, moved(被拖节点位移), neighborsMoved(其它节点里位移 > 0.5 的个数), ticks(拖拽期间的模拟帧数) }。 */
async function dragMemory(page) {
  const grab = await hoverMemory(page);
  if (!grab) return null;
  const snapshot = () => page.evaluate(() => ({ ticks: window.__kzMemoryGraph.ticks(), positions: window.__kzMemoryGraph.positions() }));
  const before = await snapshot();
  await page.mouse.down();
  for (let step = 1; step <= 6; step += 1) {
    await page.mouse.move(grab.x + step * 8, grab.y + step * 6);
    await page.waitForTimeout(40);
  }
  await page.waitForTimeout(200);
  await page.mouse.up();
  await page.waitForTimeout(400);
  const after = await snapshot();
  const was = new Map(before.positions.map((p) => [p.id, p]));
  const shift = (p) => Math.hypot(p.x - was.get(p.id).x, p.y - was.get(p.id).y);
  return {
    id: grab.id,
    moved: shift(after.positions.find((p) => p.id === grab.id)),
    neighborsMoved: after.positions.filter((p) => p.id !== grab.id && was.has(p.id) && shift(p) > 0.5).length,
    ticks: after.ticks - before.ticks,
  };
}

async function runBrowser() {
  const { chromium } = await import("playwright-core");
  const { startPreviewServer } = await import("./ui-preview/server.mjs");
  const failures = [];
  const notes = [];
  const { origin, close } = await startPreviewServer({ port: 0 });
  const browser = await chromium.launch({ channel: "msedge", headless: true });
  try {
    const context = await browser.newContext({ viewport: { width: 1600, height: 960 }, deviceScaleFactor: 1, colorScheme: "dark" });
    const page = await context.newPage();
    const errors = [];
    let vendorStatus = null;
    page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));
    page.on("console", (message) => { if (message.type() === "error") errors.push(`console.error: ${message.text()}`); });
    page.on("response", (response) => { if (response.url().includes(VENDOR_FILE)) vendorStatus = response.status(); });
    await page.goto(`${origin}/?theme=dark&scene=memory-graph&hover=&select=`, { waitUntil: "load" });
    await page.waitForFunction(() => window.__kzPreview?.ready === true && window.__kzMemoryGraph?.ready === true, null, { timeout: 30000 });
    const info = await page.evaluate(() => ({ ...window.__kzMemoryGraph }));
    // 稳定后的节点坐标:下面减少动效的上下文(同一份数据)必须排出同一张图——初始抖动是确定的,
    // 两边都跑到 alpha < d3AlphaMin,总模拟步数相同(一边 warmup 120 + 逐帧,一边全在 warmup 里)。
    const restPositions = await page.evaluate(() => window.__kzMemoryGraph.positions());
    if (vendorStatus !== 200) failures.push(`vendor 请求状态 ${vendorStatus}(期望 200)`);
    if (info.mode !== "canvas") failures.push(`图谱没有走画布渲染(mode=${info.mode})`);
    if (!(info.nodes > 0 && info.memories > 0)) failures.push(`画布上没有节点:${JSON.stringify(info)}`);
    if (!(info.settleMs < 3000)) failures.push(`布局稳定耗时 ${info.settleMs}ms ≥ 3000`);
    const sample = () => page.evaluate(() => {
      const canvas = document.querySelector("#memory-graph-canvas canvas");
      const ctx = canvas?.getContext("2d");
      if (!ctx) return null;
      const { data } = ctx.getImageData(0, 0, canvas.width, canvas.height);
      let sum = 0;
      let sq = 0;
      let hash = 0;
      const n = data.length / 4;
      for (let i = 0; i < data.length; i += 4) {
        const v = data[i] * 0.3 + data[i + 1] * 0.59 + data[i + 2] * 0.11;
        sum += v;
        sq += v * v;
        hash = (hash * 31 + data[i] + data[i + 1] * 7 + data[i + 2] * 13) % 1000000007;
      }
      const mean = sum / n;
      return { variance: sq / n - mean * mean, hash };
    });
    const before = await sample();
    if (!before || !(before.variance > 1)) failures.push(`画布是空白的(方差 ${before?.variance})`);
    if (!(info.settleTicks > 0)) failures.push(`常规动效下首帧之后应还有模拟帧(settleTicks ${info.settleTicks}),否则下面减少动效的断言分辨不出`);
    // 复核:默认画面(无悬停、无选中)就要读得出是哪几条记忆(记忆编号阈值 0.5,适配缩放通常 0.6-0.8);
    // 没选中时详情栏收成竖排页签栏,画布拿到整行宽度。
    const rect = (selector) => page.evaluate((sel) => document.querySelector(sel)?.getBoundingClientRect().width ?? 0, selector);
    await page.waitForTimeout(150);
    const rest = await page.evaluate(() => window.__kzMemoryGraph.labels());
    const memoryLabels = rest.kinds.filter((kind) => kind === "memory").length;
    notes.push(`默认视图缩放 ${rest.zoom?.toFixed?.(2)},记忆编号标签 ${memoryLabels} 个`);
    if (!(memoryLabels >= 5)) failures.push(`默认视图(缩放 ${rest.zoom})只画出 ${memoryLabels} 个记忆编号标签(应 ≥ 5):记忆全是无名彩点`);
    const [sideRest, canvasRest, workspace] = [await rect("#memory-side"), await rect("#memory-graph-canvas"), await rect(".memory-manager-workspace")];
    if (!(sideRest < 60 && canvasRest > workspace * 0.9)) failures.push(`没选中时详情栏应收成竖排页签栏:详情栏 ${sideRest}px、画布 ${canvasRest}px / 工作区 ${workspace}px`);
    // 真鼠标悬停记忆节点:悬停要停得住(状态栏不闪、画布高度不变)。
    const hover = await hoverMemory(page);
    if (!hover || !hover.stable) failures.push(`真鼠标悬停记忆节点停不住(页脚高度随悬停文字变、画布跟着跳):${JSON.stringify(hover)}`);
    // 拖拽时邻居跟着动(选 force-graph 的理由;复核发现 d3AlphaMin 让稳定后的拖拽当帧就停)。
    const drag = await dragMemory(page);
    notes.push(`拖 ${drag?.id}:模拟 ${drag?.ticks} 帧、带动 ${drag?.neighborsMoved} 个节点`);
    if (!drag || !(drag.moved > 5) || !(drag.neighborsMoved > 0)) failures.push(`拖拽记忆节点时邻居没有跟着动:${JSON.stringify(drag)}`);
    await page.waitForFunction(() => window.__kzMemoryGraph.ticks() > 0, null, { timeout: 5000 }).catch(() => {});
    await page.waitForTimeout(2500); // 松手后等模拟衰减到门槛以下自然收工,后面的断言看的是静止画面
    await page.evaluate(() => window.__kzMemoryGraph.select("M-009"));
    await page.waitForTimeout(300);
    const sideOpen = await rect("#memory-side");
    if (!(sideOpen >= 300)) failures.push(`选中 M-009 后详情栏没有展开(宽 ${sideOpen}px)`);
    // 悬停时入射边的关系标签参与抢位:放得下的照样画(不是一律不画)。
    const edgeLabels = await page.evaluate(async () => {
      window.__kzMemoryGraph.hover("M-009");
      await new Promise((resolve) => setTimeout(resolve, 200));
      return window.__kzMemoryGraph.labels().edges;
    });
    if (!edgeLabels.length) failures.push("悬停 M-009 时一条边标签都没画出来(边标签抢位把它们全丢了?)");
    const hovered = await page.evaluate(() => {
      const ok = window.__kzMemoryGraph.hover("M-009");
      return { ok, text: document.getElementById("memory-graph-status")?.textContent ?? "" };
    });
    if (!hovered.ok || !hovered.text.includes("M-009")) failures.push(`程序化悬停后状态栏没有写节点信息:${JSON.stringify(hovered)}`);
    await page.evaluate(() => window.__kzMemoryGraph.hover(null));
    await page.evaluate(async () => {
      const shell = await import("/03-shell.js");
      shell.applyTheme("light");
    });
    await page.waitForTimeout(400);
    const after = await sample();
    if (!after || after.hash === before?.hash) failures.push("kz:theme 切到亮色后画布像素没有变化(画布没按 --graph-* token 重画)");
    await page.evaluate(async () => (await import("/03-shell.js")).applyTheme("dark"));
    // 重排后 crate 仍钉在层带上:同带 y 完全相同,不同 y 至多 4 个(库的 dagMode 设置会清掉复用节点的 fx/fy,
    // 曾让第二次 setData 之后整张架构骨架散掉)。
    await page.click("#memory-graph-archived");
    await page.waitForFunction(() => window.__kzMemoryGraph?.ready === true, null, { timeout: 30000 });
    const crateRows = await page.evaluate(() => window.__kzMemoryGraph.positions().filter((p) => p.kind === "crate").map((p) => Math.round(p.y * 1000) / 1000));
    if (!crateRows.length || new Set(crateRows).size > 4) failures.push(`重排后 crate 没有钉在层带上(y = ${crateRows.join(", ")})`);
    await page.click("#memory-graph-archived");
    await page.waitForFunction(() => window.__kzMemoryGraph?.ready === true, null, { timeout: 30000 });
    await page.click("#memory-graph-textview");
    await page.waitForTimeout(150);
    const tree = await page.evaluate(() => ({
      items: document.querySelectorAll('#memory-graph-list [role="treeitem"][data-memory-id]').length,
      memories: window.__kzMemoryGraph.memories,
      hidden: document.getElementById("memory-graph-list")?.classList.contains("hidden"),
    }));
    if (tree.hidden || tree.items !== tree.memories || !tree.items) failures.push(`文本视图条目数 ${tree.items} ≠ 可见记忆数 ${tree.memories}`);
    await page.click("#memory-graph-textview");
    // 规模:约 700 节点 / 1800 边,默认参数下 3 秒内稳定。
    const big = syntheticPayload();
    await page.evaluate(async (payload) => {
      window.__kzPreview.setCommand("memory_graph", () => payload);
      const status = document.getElementById("memory-status-filter");
      status.value = "all";
      status.dispatchEvent(new Event("change"));
      const { currentProject } = await import("/03-shell.js");
      window.__kzMemoryGraph.ready = false;
      document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project: currentProject } }));
    }, big);
    await page.waitForFunction(() => window.__kzMemoryGraph?.ready === true && window.__kzMemoryGraph.nodes > 500, null, { timeout: 30000 }).catch(() => {});
    const scale = await page.evaluate(() => ({ ...window.__kzMemoryGraph }));
    notes.push(`规模 ${scale.nodes} 节点 / ${scale.links} 边 稳定 ${scale.settleMs}ms`);
    if (!(scale.nodes >= 500)) failures.push(`规模测试没有加载合成大图(nodes=${scale.nodes})`);
    else if (!(scale.settleMs < 3000)) failures.push(`约 ${scale.nodes} 节点/${scale.links} 边布局稳定 ${scale.settleMs}ms ≥ 3000`);
    // dag-lr(R-368 B5 / R-307 B3 预留)遇环:引用图天然有环,库默认 throw「Invalid DAG structure」。
    // 带环的小图 setData({ layout: "dag-lr" }) 不得有 pageerror,且能稳定、坐标有限。
    const dag = await page.evaluate(async () => {
      const { createGraphView } = await import("/24-graph-view.js");
      const host = document.createElement("div");
      host.style.cssText = "position:fixed;left:0;top:0;width:320px;height:200px;";
      document.body.append(host);
      let settle = null;
      const settled = new Promise((resolve) => {
        settle = resolve;
      });
      const view = await createGraphView(host, { onSettled: (result) => settle(result) });
      let thrown = null;
      try {
        view.setData(
          {
            nodes: ["R-1", "R-2", "R-3", "D-1"].map((id) => ({ id, kind: id.startsWith("R") ? "requirement" : "defect" })),
            links: [["R-1", "R-2"], ["R-2", "R-3"], ["R-3", "R-1"], ["R-3", "D-1"]].map(([source, target]) => ({ source, target, rel: "refs", strength: "strong" })),
          },
          { layout: "dag-lr" },
        );
      } catch (error) {
        thrown = String(error);
      }
      const result = await Promise.race([settled, new Promise((resolve) => setTimeout(() => resolve(null), 8000))]);
      const finite = view.positions().every((p) => Number.isFinite(p.x) && Number.isFinite(p.y));
      view.destroy();
      host.remove();
      return { thrown, settled: Boolean(result), finite };
    });
    if (dag.thrown || !dag.settled || !dag.finite) failures.push(`dag-lr 带环小图:${JSON.stringify(dag)}`);
    for (const error of errors) failures.push(error);
    notes.unshift(`夹具 ${info.nodes} 节点 / ${info.links} 边,${info.memories} 条记忆,稳定 ${info.settleMs}ms`);
    await context.close();

    // 减少动效(prefers-reduced-motion: reduce):布局全在首帧之前跑完(settleTicks = 0),第一眼就是最终布局。
    // 这个上下文用用户的第二档屏幕 1280×690@1.5:画布只有 420px 高,适配缩放约 0.45,顺带看小屏默认画面的记忆编号。
    const calm = await browser.newContext({ viewport: { width: 1280, height: 690 }, deviceScaleFactor: 1.5, colorScheme: "dark", reducedMotion: "reduce" });
    const calmPage = await calm.newPage();
    const calmErrors = [];
    calmPage.on("pageerror", (error) => calmErrors.push(`减少动效 pageerror: ${error.message}`));
    await calmPage.goto(`${origin}/?theme=dark&scene=memory-graph&hover=&select=`, { waitUntil: "load" });
    await calmPage.waitForFunction(() => window.__kzPreview?.ready === true && window.__kzMemoryGraph?.ready === true, null, { timeout: 30000 });
    const calmInfo = await calmPage.evaluate(() => ({ ...window.__kzMemoryGraph }));
    notes.push(`减少动效:稳定 ${calmInfo.settleMs}ms、首帧后 ${calmInfo.settleTicks} 帧`);
    if (calmInfo.reducedMotion !== true) failures.push("减少动效的上下文里渲染器没读到 prefers-reduced-motion: reduce");
    if (calmInfo.settleTicks !== 0) failures.push(`减少动效时布局应在首帧前跑完(settleTicks 0),实得 ${calmInfo.settleTicks}`);
    if (!(calmInfo.settleMs < 3000)) failures.push(`减少动效时布局稳定 ${calmInfo.settleMs}ms ≥ 3000`);
    const calmPositions = new Map((await calmPage.evaluate(() => window.__kzMemoryGraph.positions())).map((p) => [p.id, p]));
    const drift = Math.max(...restPositions.map((p) => {
      const q = calmPositions.get(p.id);
      return q ? Math.hypot(q.x - p.x, q.y - p.y) : Infinity;
    }));
    notes.push(`减少动效与常规布局的最大坐标差 ${drift.toFixed(3)}`);
    if (!(drift < 0.5)) failures.push(`减少动效时布局没有跑到与常规动效同一张图(最大坐标差 ${drift}):warmup 没跑到收敛,或初始位置不确定`);
    // 拖拽:被拖的节点跟手,邻居不被带着动(减少动效时拖拽期间不跑模拟帧)。
    const calmDrag = await dragMemory(calmPage);
    if (!calmDrag || !(calmDrag.moved > 5)) failures.push(`减少动效时拖拽没有移动节点:${JSON.stringify(calmDrag)}`);
    else if (calmDrag.ticks !== 0 || calmDrag.neighborsMoved !== 0) failures.push(`减少动效时拖拽不应带动邻居:${JSON.stringify(calmDrag)}`);
    await calmPage.waitForTimeout(150);
    const small = await calmPage.evaluate(() => window.__kzMemoryGraph.labels());
    const smallMemoryLabels = small.kinds.filter((kind) => kind === "memory").length;
    notes.push(`1280×690 默认视图缩放 ${small.zoom?.toFixed?.(2)},记忆编号标签 ${smallMemoryLabels} 个`);
    if (!(smallMemoryLabels >= 5)) failures.push(`1280×690 默认视图(缩放 ${small.zoom})只画出 ${smallMemoryLabels} 个记忆编号标签(应 ≥ 5)`);
    failures.push(...calmErrors);
    await calm.close();
  } finally {
    await browser.close();
    await close();
  }
  return { failures, notes };
}

// ---------- 主流程 ----------
const mutateId = process.env.KZ_GRAPH_MUTATE ?? "";
if (mutateId) {
  const mutated = mutatedModelUrl(mutateId);
  console.error(`[KZ_GRAPH_MUTATE=${mutateId}] 已删除被守护的源码,期望本次运行**失败**`);
  const { failures } = await runTests(mutated.url);
  mutated.cleanup();
  for (const failure of failures) console.error(` - ${failure.name}:${failure.message}`);
  process.exit(failures.length ? 1 : 0);
}

const problems = [];
const { failures, count } = await runTests(pathToFileURL(MODEL_PATH).href);
for (const failure of failures) problems.push(`纯模型「${failure.name}」:${failure.message}`);
try {
  checkVendor();
} catch (error) {
  problems.push(`vendor:${error.message}`);
}
// 变异自检:每个变异都必须让它守护的那条测试变红。
for (const [id, mutation] of Object.entries(MUTATIONS)) {
  try {
    const mutated = mutatedModelUrl(id);
    const result = await runTests(mutated.url);
    mutated.cleanup();
    if (!result.failures.some((f) => f.name === mutation.test)) problems.push(`变异 ${id} 没被「${mutation.test}」抓住(守卫恒绿)`);
  } catch (error) {
    problems.push(`变异 ${id}:${error.message}`);
  }
}
let browserNotes = [];
if (process.argv.includes("--browser")) {
  try {
    const result = await runBrowser();
    browserNotes = result.notes;
    for (const failure of result.failures) problems.push(`浏览器:${failure}`);
  } catch (error) {
    problems.push(`浏览器冒烟未能运行:${error.stack || error.message}`);
  }
}
if (problems.length) {
  console.error(`记忆图谱冒烟失败(${problems.length} 处):`);
  for (const problem of problems) console.error(` - ${problem}`);
  process.exit(1);
}
console.log(`记忆图谱冒烟通过:纯模型 ${count} 组、vendor SHA-256 与许可一致、变异守卫 ${Object.keys(MUTATIONS).length} 个全部被抓住${browserNotes.length ? `;浏览器:${browserNotes.join(";")}` : ""}`);
