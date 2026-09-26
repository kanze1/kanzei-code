// UI2-0926 #7 架构图浏览器门禁(docs/design/architecture_diagrams.md §8),verify 的 ui_diagram 步。
// 单独运行:node scripts/ui-diagram-smoke.mjs [--only <路径片段>]
//
// 假 DOM 冒烟跑不了真 mermaid,真实渲染质量全在这里:无头 Edge(playwright-core channel msedge,与
// ui-surface-gallery-smoke 同一路线)打开 ui/gallery.html(只带 app.css 与零 import 模块),直接 import
// 04-diagram.js,把下列图在暗/亮两套主题下按「页面模式、宽 960」各渲染一遍:
//   - docs/architecture/*.md 的主图(架构页标签页里的那几张);
//   - docs/**/*.md 里所有 ```mermaid 围栏(聊天/文档查看器里同一个渲染器);
//   - crate 图生成器的 golden(crates/kanzei-tools/tests/fixtures/arch_diagram/*.mmd,含引号/反引号/截断)。
// 逐张检查:① 能解析能渲染;② 每条 click 都映射到 SVG 节点、目标路径在仓里存在;③ 节点包围盒两两不重叠;
// ④ 标签文字不溢出节点形状;⑤ 适应后主标签有效字号 ≥ 11px、次行 ≥ 10px;⑥ 图自然尺寸 ≤ 2400×1600;
// ⑦ 标签对节点底 ≥ 4.5(次要类 muted 刻意淡化,≥ 3),边对画布 ≥ 3;⑧ 没有 foreignObject/script/on*;
// ⑨ --diagram-* token 解析结果都是 hex(mermaid 的 themeVariables 只认 hex)。
// 截图写入 dist/ui-diagrams/<theme>-<名字>.png(dist 已在 .gitignore)。
// selfTestDiagramGate:语法错误、click 指向不存在的节点、带 foreignObject 的 SVG、人为重叠的包围盒、
// 标签溢出、低对比 token 覆盖——每个坏样例必须被对应检查器抓到,任何检查器恒绿即失败。
/* global window, document, getComputedStyle */
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(root, "dist", "ui-diagrams");
const WIDTH = 960;
const LIMITS = { maxW: 2400, maxH: 1600, primaryPx: 11, secondaryPx: 10, text: 4.5, mutedText: 3, edge: 3 };
const only = process.argv.includes("--only") ? process.argv[process.argv.indexOf("--only") + 1] : null;
const { parseClickDirectives, svgSafetyIssues } = await import(pathToFileURL(path.join(root, "crates/kanzei-app/ui/04-diagram.js")).href);

// ---------- 颜色 ----------
export function parseColor(text) {
  const value = String(text ?? "").trim();
  const hex = value.match(/^#([0-9a-f]{3,8})$/i)?.[1];
  if (hex) {
    const full = hex.length <= 4 ? [...hex].map((c) => c + c).join("") : hex;
    return {
      r: parseInt(full.slice(0, 2), 16),
      g: parseInt(full.slice(2, 4), 16),
      b: parseInt(full.slice(4, 6), 16),
      a: full.length === 8 ? parseInt(full.slice(6, 8), 16) / 255 : 1,
    };
  }
  const m = value.match(/rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+%?))?\s*\)/);
  if (m) {
    const alpha = m[4] === undefined ? 1 : m[4].endsWith("%") ? Number(m[4].slice(0, -1)) / 100 : Number(m[4]);
    return { r: Number(m[1]), g: Number(m[2]), b: Number(m[3]), a: alpha };
  }
  return null;
}
const channel = (v) => {
  const s = v / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
};
const luminance = ({ r, g, b }) => 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
export function contrast(fg, bg) {
  const a = luminance(fg);
  const b = luminance(bg);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}
/// 半透明色叠到不透明底上。
export function composite(top, under) {
  const a = top.a ?? 1;
  return { r: top.r * a + under.r * (1 - a), g: top.g * a + under.g * (1 - a), b: top.b * a + under.b * (1 - a), a: 1 };
}

// ---------- 纯判据(页面只负责量,判断全在这里,自检直接喂假数据) ----------
export function tokenViolations(tokens) {
  return Object.entries(tokens).filter(([, value]) => !/^#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.test(value)).map(([name, value]) => `${name} = ${JSON.stringify(value)} 不是 hex`);
}
export function overlapViolations(nodes) {
  const out = [];
  for (let i = 0; i < nodes.length; i += 1) {
    for (let j = i + 1; j < nodes.length; j += 1) {
      const a = nodes[i].box;
      const b = nodes[j].box;
      const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
      const h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
      if (w > 1 && h > 1) out.push(`节点 ${nodes[i].id} 与 ${nodes[j].id} 重叠 ${w.toFixed(1)}×${h.toFixed(1)}px`);
    }
  }
  return out;
}
export function overflowViolations(nodes, tolerance = 1.5) {
  return nodes
    .filter((n) => n.text && n.shape)
    .filter((n) => n.text.x < n.shape.x - tolerance || n.text.y < n.shape.y - tolerance
      || n.text.x + n.text.w > n.shape.x + n.shape.w + tolerance || n.text.y + n.text.h > n.shape.y + n.shape.h + tolerance)
    .map((n) => `节点 ${n.id} 的标签溢出形状(文字 ${n.text.w.toFixed(0)}×${n.text.h.toFixed(0)},形状 ${n.shape.w.toFixed(0)}×${n.shape.h.toFixed(0)})`);
}
export function fontViolations(nodes, scale) {
  const out = [];
  for (const n of nodes) {
    if (n.primaryPx && n.primaryPx * scale < LIMITS.primaryPx - 0.05) out.push(`节点 ${n.id} 主标签有效字号 ${(n.primaryPx * scale).toFixed(1)}px < ${LIMITS.primaryPx}`);
    if (n.secondaryPx && n.secondaryPx * scale < LIMITS.secondaryPx - 0.05) out.push(`节点 ${n.id} 次行有效字号 ${(n.secondaryPx * scale).toFixed(1)}px < ${LIMITS.secondaryPx}`);
  }
  return out;
}
export function contrastViolations(nodes, edges, canvas) {
  const out = [];
  const canvasColor = parseColor(canvas);
  for (const n of nodes) {
    const under = parseColor(n.under) ?? canvasColor;
    const fill = parseColor(n.fill);
    const bg = fill ? composite(fill, under) : under;
    const opacity = n.opacity ?? 1;
    for (const [kind, color] of [["标签", n.color], ["次行", n.secondaryColor]]) {
      const fg = parseColor(color);
      if (!fg) continue;
      const seen = composite({ ...composite(fg, bg), a: opacity }, under);
      const min = n.muted ? LIMITS.mutedText : LIMITS.text;
      const ratio = contrast(seen, composite({ ...bg, a: opacity }, under));
      if (ratio < min) out.push(`节点 ${n.id} 的${kind}对比度 ${ratio.toFixed(2)} < ${min}`);
    }
  }
  for (const e of edges) {
    const stroke = parseColor(e.stroke);
    if (!stroke || !canvasColor) continue;
    const ratio = contrast(composite(stroke, canvasColor), canvasColor);
    if (ratio < LIMITS.edge) out.push(`边 ${e.id} 对画布对比度 ${ratio.toFixed(2)} < ${LIMITS.edge}`);
  }
  return out;
}
export function sizeViolations(natural) {
  return natural.w > LIMITS.maxW || natural.h > LIMITS.maxH ? [`图自然尺寸 ${Math.round(natural.w)}×${Math.round(natural.h)} 超过 ${LIMITS.maxW}×${LIMITS.maxH},按子系统拆图`] : [];
}
export function clickViolations(result, { checkPaths }) {
  const out = result.unmapped.map((c) => `click ${c.id} 在 SVG 里找不到节点(第 ${c.sourceLine} 行)`);
  if (checkPaths) {
    for (const c of result.clicks) {
      if (c.path && !existsSync(path.join(root, c.path))) out.push(`click ${c.id} 的目标不存在:${c.path}`);
    }
  }
  return out;
}
export function judge(result, { checkPaths }) {
  if (result.error) return [`渲染失败:第 ${result.error.line ?? "?"} 行 ${result.error.message}`];
  return [
    ...clickViolations(result, { checkPaths }),
    ...overlapViolations(result.nodes),
    ...overflowViolations(result.nodes),
    ...fontViolations(result.nodes, result.scale),
    ...sizeViolations(result.natural),
    ...contrastViolations(result.nodes, result.edges, result.canvas),
    ...result.unsafe.map((what) => `SVG 含不安全内容:${what}`),
  ];
}

// ---------- 取图 ----------
function fencesOf(text) {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const out = [];
  for (let i = 0; i < lines.length; i += 1) {
    if (lines[i].trim() !== "```mermaid") continue;
    const end = lines.findIndex((line, j) => j > i && line.trim() === "```");
    if (end < 0) break;
    out.push({ line: i + 2, source: lines.slice(i + 1, end).join("\n") });
    i = end;
  }
  return out;
}
async function walk(dir) {
  const out = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...(await walk(full)));
    else if (entry.name.endsWith(".md")) out.push(full);
  }
  return out;
}
async function collectCases() {
  const cases = [];
  const seen = new Set();
  const archDir = path.join(root, "docs", "architecture");
  const files = (await walk(path.join(root, "docs"))).sort();
  for (const file of files) {
    const rel = path.relative(root, file).replace(/\\/g, "/");
    const fences = fencesOf(await readFile(file, "utf8"));
    const isArch = file.startsWith(archDir + path.sep);
    fences.forEach((fence, index) => {
      const key = `${rel}:${fence.line}`;
      if (seen.has(key)) return;
      seen.add(key);
      // docs/architecture 只有首个围栏是标签页主图;其余围栏与别处文档一样按 markdown 里的图处理。
      cases.push({ name: `${rel.replace(/^docs\//, "").replace(/[\\/]/g, "_").replace(/\.md$/, "")}-${fence.line}`, path: rel, line: fence.line, source: fence.source, checkPaths: true, arch: isArch && index === 0 });
    });
  }
  const goldenDir = path.join(root, "crates", "kanzei-tools", "tests", "fixtures", "arch_diagram");
  for (const name of ["crates_reduced.mmd", "crates_full.mmd"]) {
    const source = await readFile(path.join(goldenDir, name), "utf8");
    // golden 的 click 目标指向夹具工作区(不在本仓),只查映射不查路径。
    cases.push({ name: `golden_${name.replace(/\.mmd$/, "")}`, path: `crates/kanzei-tools/tests/fixtures/arch_diagram/${name}`, line: 1, source: source.replace(/\r\n?/g, "\n"), checkPaths: false, arch: true });
  }
  return only ? cases.filter((c) => c.path.includes(only) || c.name.includes(only)) : cases;
}

// ---------- 页面侧:挂图并量 ----------
async function measure(page, source, { theme }) {
  return page.evaluate(async ({ source, theme, width }) => {
    document.documentElement.setAttribute("data-theme", theme);
    const d = await import("/04-diagram.js");
    let host = document.getElementById("kz-diagram-gate");
    if (!host) {
      host = document.createElement("div");
      host.id = "kz-diagram-gate";
      document.body.prepend(host);
    }
    host.style.cssText = `width:${width}px;padding:0;margin:0;`;
    const view = d.mountDiagram(host, source, { mode: "page" });
    const started = performance.now();
    while (view.root.dataset.state === "loading" && performance.now() - started < 20000) await new Promise((r) => setTimeout(r, 25));
    const css = getComputedStyle(document.documentElement);
    const tokens = {};
    for (const name of ["--diagram-canvas", "--diagram-cluster", "--diagram-cluster-border", "--diagram-node", "--diagram-node-border", "--diagram-text", "--diagram-muted", "--diagram-edge", "--diagram-accent", "--diagram-accent-soft"]) tokens[name] = css.getPropertyValue(name).trim();
    const canvas = getComputedStyle(view.root).backgroundColor;
    if (view.root.dataset.state !== "ready") {
      return { error: view.result?.error ?? { line: null, message: `状态 ${view.root.dataset.state}` }, tokens };
    }
    const svg = view.root.querySelector("svg");
    const box = (el) => {
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    };
    const clusters = [...svg.querySelectorAll("g.cluster")].map((g) => {
      const rect = g.querySelector("rect, path, polygon");
      return rect ? { box: box(rect), fill: getComputedStyle(rect).fill } : null;
    }).filter(Boolean);
    const inside = (b, c) => b.x >= c.x && b.y >= c.y && b.x + b.w <= c.x + c.w && b.y + b.h <= c.y + c.h;
    const nodes = [...svg.querySelectorAll("g.node")].map((g) => {
      const shape = g.querySelector(":scope > rect, :scope > path, :scope > polygon, :scope > circle, :scope > ellipse, :scope > g > path, :scope > g > rect");
      const text = g.querySelector("text");
      const rows = [...g.querySelectorAll("text tspan.row")];
      const firstInner = rows[0]?.querySelector("tspan") ?? rows[0] ?? text;
      const secondInner = rows[1]?.querySelector("tspan") ?? rows[1] ?? null;
      const nodeBox = box(g);
      const container = clusters.filter((c) => inside(nodeBox, c.box)).sort((a, b) => a.box.w * a.box.h - b.box.w * b.box.h)[0];
      const shapeStyle = shape ? getComputedStyle(shape) : null;
      let fill = shapeStyle?.fill ?? null;
      const fillOpacity = shapeStyle ? Number(shapeStyle.fillOpacity || 1) : 1;
      if (fill && fillOpacity < 1) {
        const m = fill.match(/rgba?\(([^)]+)\)/);
        if (m) {
          const parts = m[1].split(/[,\s/]+/).filter(Boolean).map(Number);
          fill = `rgba(${parts[0]}, ${parts[1]}, ${parts[2]}, ${(parts[3] ?? 1) * fillOpacity})`;
        }
      }
      return {
        id: g.id,
        box: nodeBox,
        shape: shape ? box(shape) : null,
        text: text ? box(text) : null,
        primaryPx: firstInner ? parseFloat(getComputedStyle(firstInner).fontSize) : 0,
        secondaryPx: secondInner ? parseFloat(getComputedStyle(secondInner).fontSize) : 0,
        color: firstInner ? getComputedStyle(firstInner).fill : null,
        secondaryColor: secondInner ? getComputedStyle(secondInner).fill : null,
        fill,
        under: container?.fill ?? canvas,
        opacity: Number(getComputedStyle(g).opacity || 1) * Number(shapeStyle?.opacity || 1),
        muted: g.classList.contains("muted"),
      };
    });
    const edges = [...svg.querySelectorAll("path.flowchart-link")].map((p) => ({ id: p.getAttribute("data-id") || p.id, stroke: getComputedStyle(p).stroke }));
    const unsafe = [];
    if (svg.querySelector("foreignObject")) unsafe.push("foreignObject");
    if (svg.querySelector("script")) unsafe.push("script");
    for (const el of svg.querySelectorAll("*")) for (const attr of el.attributes) if (/^on/i.test(attr.name)) unsafe.push(`${el.tagName}[${attr.name}]`);
    return {
      tokens,
      canvas,
      scale: view.scale,
      natural: view.natural,
      nodes,
      edges,
      clicks: view.result.clicks ?? [],
      unmapped: view.graph.unmapped,
      mapped: view.graph.mapped.length,
      unsafe,
    };
  }, { source, theme, width: WIDTH });
}

// ---------- 自检:每个检查器都得抓到坏样例 ----------
async function selfTestDiagramGate(page) {
  const failures = [];
  const expectRed = (what, list) => { if (!list.length) failures.push(`自检失效:${what} 没被判红`); };
  // 语法错误 → 渲染失败。
  const broken = await measure(page, "flowchart LR\n  a[run(x)] --> b[\"乙\"]", { theme: "dark" });
  expectRed("语法错误", judge(broken, { checkPaths: false }));
  // click 指向不存在的节点 → 映射失败;目标路径不存在 → 路径失败。
  const ghost = await measure(page, "flowchart LR\n  a[\"甲\"] --> b[\"乙\"]\n  click nope \"crates/kanzei-app/ui/04-diagram.js\"", { theme: "dark" });
  expectRed("click 指向不存在的节点", clickViolations(ghost, { checkPaths: false }));
  const missing = await measure(page, "flowchart LR\n  a[\"甲\"] --> b[\"乙\"]\n  click a \"crates/no/such/file.rs\"", { theme: "dark" });
  expectRed("click 目标文件不存在", clickViolations(missing, { checkPaths: true }));
  // 带 foreignObject 的 SVG → 安全检查(渲染器插入前的字符串闸门)。
  expectRed("foreignObject", svgSafetyIssues('<svg><foreignObject><div onclick="x()"></div></foreignObject></svg>'));
  // 人为重叠的包围盒 / 溢出的标签 / 过小字号 / 过大尺寸。
  expectRed("重叠", overlapViolations([{ id: "a", box: { x: 0, y: 0, w: 100, h: 40 } }, { id: "b", box: { x: 60, y: 20, w: 100, h: 40 } }]));
  expectRed("标签溢出", overflowViolations([{ id: "a", shape: { x: 0, y: 0, w: 80, h: 30 }, text: { x: -6, y: 4, w: 96, h: 18 } }]));
  expectRed("字号过小", fontViolations([{ id: "a", primaryPx: 13, secondaryPx: 12 }], 0.5));
  expectRed("尺寸过大", sizeViolations({ w: 3000, h: 800 }));
  expectRed("token 非 hex", tokenViolations({ "--diagram-node": "color-mix(in srgb, #fff 10%, transparent)" }));
  // 低对比 token 覆盖:把节点文字色改成节点底色,真渲染一遍,对比度检查必须红。
  await page.evaluate(() => {
    const css = getComputedStyle(document.documentElement);
    document.documentElement.style.setProperty("--diagram-text", css.getPropertyValue("--diagram-node").trim());
  });
  const faint = await measure(page, "flowchart LR\n  a[\"低对比\"] --> b[\"低对比\"]", { theme: "dark" });
  await page.evaluate(() => document.documentElement.style.removeProperty("--diagram-text"));
  expectRed("低对比 token", faint.error ? [] : contrastViolations(faint.nodes, faint.edges, faint.canvas));
  return failures;
}

// ---------- 主流程 ----------
const cases = await collectCases();
if (!cases.length) {
  console.error("ui-diagram-smoke:一张图都没收集到(docs/architecture 与 golden 都应该有),判据退化");
  process.exit(1);
}
await mkdir(OUT, { recursive: true });
const { origin, close } = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const failures = [];
const report = [];
try {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 });
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(String(error?.message ?? error)));
  await page.goto(`${origin}/gallery.html`, { waitUntil: "load" });
  failures.push(...(await selfTestDiagramGate(page)));
  for (const theme of ["dark", "light"]) {
    for (const item of cases) {
      const result = await measure(page, item.source, { theme });
      const tokenIssues = tokenViolations(result.tokens ?? {});
      const issues = [...tokenIssues, ...judge(result, item)];
      const where = `${theme} ${item.path}:${item.line}`;
      for (const issue of issues) failures.push(`${where} ${issue}`);
      const shot = path.join(OUT, `${theme}-${item.name}.png`);
      await page.locator("#kz-diagram-gate").screenshot({ path: shot }).catch(() => {});
      report.push({ theme, path: item.path, line: item.line, ok: !issues.length, scale: result.scale, natural: result.natural, nodes: result.nodes?.length ?? 0, mapped: result.mapped ?? 0 });
    }
  }
  if (pageErrors.length) failures.push(...pageErrors.map((e) => `页面异常:${e}`));
  await context.close();
} finally {
  await browser.close();
  await close();
}
await writeFile(path.join(OUT, "report.json"), JSON.stringify(report, null, 2));
if (failures.length) {
  console.error(`ui-diagram-smoke 失败(${failures.length} 处):`);
  for (const failure of failures) console.error(` - ${failure}`);
  process.exit(1);
}
const archCount = cases.filter((c) => c.arch).length;
console.log(`ui-diagram-smoke 通过:${cases.length} 张图(架构页 ${archCount} 张)× 暗/亮 真渲染,点击映射/重叠/溢出/字号/尺寸/对比度/安全/token 全过,自检 10 个坏样例均判红;截图 ${path.relative(root, OUT)}`);
