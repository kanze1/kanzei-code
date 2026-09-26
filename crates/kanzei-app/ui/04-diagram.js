// ---------- 图渲染(UI2-0926 #7,docs/design/architecture_diagrams.md) ----------
// 全应用唯一的图引擎入口:架构页的大图,以及聊天、文档查看器、研究页 markdown 里闭合的 ```mermaid 围栏。
// - 引擎:vendor/mermaid(12.0.0 ESM 分块版),第一次用到时才 import;架构页空闲期预加载。
// - 配色只由 kanzei 注入:themeVariables 与五个语义类(entry/ext/store/focus/muted)的 classDef 在渲染时从
//   style.css 的 --diagram-* token 读出——本文件不写任何字面量颜色;源码里的 %%{init} 指令与 frontmatter 的
//   config 段被抹成空行(行数不变)。
// - 安全:securityLevel strict + htmlLabels false;插入前按标签与属性查一遍 SVG(script / foreignObject / on* /
//   javascript: 链接),插入后再按 DOM 查一遍。
// - 点击:strict 下 mermaid 不绑定 click。这里解析 `click <id> "<路径[:行]>|<条目号>" ["<提示>"]`,把这些行抹成空行
//   (strict 下 mermaid 仍会把 href 包成 <a>,点了会让 WebView 自己导航),映射到 SVG 节点后走注入的导航。
// - 行号:mermaid 解析前剥掉 frontmatter、整行 %% 注释与开头空行,jison 报的是剥完之后的行号;prepareDiagramSource
//   给出 lineMap,错误卡、修复提示与源码高亮都换回原文行号。
// - 串行队列(mermaid.render 不可重入)+ LRU 缓存(键 = 主题 + 布局覆盖 + 源码);未闭合的围栏(<pre data-open>)永不渲染。
// 零 import:04-markdown.js 在 Node 冒烟里被直接 import,这里不得牵出应用其它模块。翻译、导航、源码查看器、
// 放大查看与 toast 由 04-structured.js / 15-views-misc.js 经 setDiagramHost 注入。模块顶层不碰 document。

export const SEMANTIC_CLASSES = ["entry", "ext", "store", "focus", "muted"];
const TOKEN_NAMES = {
  canvas: "--diagram-canvas",
  cluster: "--diagram-cluster",
  clusterBorder: "--diagram-cluster-border",
  node: "--diagram-node",
  nodeBorder: "--diagram-node-border",
  text: "--diagram-text",
  muted: "--diagram-muted",
  edge: "--diagram-edge",
  accent: "--diagram-accent",
  accentSoft: "--diagram-accent-soft",
};
const HEX = /^#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const CACHE_MAX = 24;
const PAD = 16;
const FIT_MAX = 1.15;
/// 适应缩放的下限:13px 标签缩放后至少 11 个设备像素(1 倍屏 0.85 ≈ 11px;1.5 倍屏 0.6 ≈ 11.7 设备像素),
/// 再小就读不清,宁可留着拖动。
const fitMin = () => {
  const dpr = typeof window !== "undefined" && window.devicePixelRatio > 0 ? window.devicePixelRatio : 1;
  return Math.min(0.85, Math.max(0.6, 11 / (13 * dpr)));
};
/// 抹掉的行换成空行:mermaid 不删中间的空行,行号不漂移(开头的空行由 lineMap 兜住)。
const BLANKED = "";
/// mermaid 12 的 cleanupComments 按行删的注释:`%%` 后不是 `{`。单独一个 `%%` mermaid 不删、会画成节点,这里一并抹掉。
const COMMENT_LINE = /^\s*%%(?!\{)[^\n]*$/;
const FRONTMATTER_FENCE = /^---\s*$/;
const ZOOM_MIN = 0.3;
const ZOOM_MAX = 3;
const INLINE_MAX_H = 520;
/// 页面模式画布的最低高度:矮图(适应后只有两三百像素)不再撑出大片空白把下方文档树挤下去。
const PAGE_MIN_H = 240;

// ---------- 宿主注入 ----------
const host = {
  t: (key) => key,
  openPath(_path, _line) {},
  openRef(_id) {},
  openSource: null,
  openLarge: null,
  toast: null,
};
export function setDiagramHost(partial) {
  for (const [key, value] of Object.entries(partial ?? {})) {
    if (typeof value === "function") host[key] = value;
  }
}
function t(key) {
  try {
    return host.t(key);
  } catch {
    return key;
  }
}
function fill(template, vars) {
  return String(template).replace(/\{(\w+)\}/g, (hit, key) => (key in vars ? String(vars[key]) : hit));
}

// ---------- 引擎 ----------
let engine = null;
let enginePromise = null;
let configuredSignature = "";
/// 冒烟接缝(同 setRenderMarkdown):注入桩引擎 { initialize?, parse, render, mount? };传 null 恢复真引擎。
export function setDiagramEngine(value) {
  engine = value ?? null;
  enginePromise = value ? Promise.resolve(value) : null;
  configuredSignature = "";
  cache.clear();
}
function loadEngine() {
  if (enginePromise) return enginePromise;
  enginePromise = import("./vendor/mermaid/mermaid.esm.min.mjs")
    .then((module) => {
      const mermaid = module.default ?? module;
      engine = {
        initialize: (config) => mermaid.initialize(config),
        parse: (text) => mermaid.parse(text),
        render: (id, text) => mermaid.render(id, text),
      };
      return engine;
    })
    .catch((error) => {
      enginePromise = null;
      throw error;
    });
  return enginePromise;
}
/// 空闲时预加载引擎(架构页激活时调用);失败静默,真正渲染时再报。
export function preloadDiagramEngine() {
  const run = () => loadEngine().catch(() => {});
  if (typeof requestIdleCallback === "function") requestIdleCallback(run, { timeout: 2000 });
  else setTimeout(run, 200);
}

// ---------- 主题 ----------
export function currentDiagramTheme() {
  return document.documentElement.getAttribute("data-theme") === "light" ? "light" : "dark";
}
export function diagramTokens() {
  const out = {};
  if (typeof getComputedStyle !== "function") return out;
  const style = getComputedStyle(document.documentElement);
  for (const [key, name] of Object.entries(TOKEN_NAMES)) out[key] = style.getPropertyValue(name).trim();
  out.font = style.getPropertyValue("--sans").trim();
  return out;
}
/// mermaid 的 themeVariables 只认 hex:token 不是 hex 的一律不传(ui-diagram-smoke 另查 token 都是 hex)。
export function themeVariablesFromTokens(tokens, theme) {
  const hex = (value) => (HEX.test(value ?? "") ? value : undefined);
  const vars = {
    darkMode: theme !== "light",
    background: hex(tokens.canvas),
    primaryColor: hex(tokens.node),
    mainBkg: hex(tokens.node),
    primaryBorderColor: hex(tokens.nodeBorder),
    nodeBorder: hex(tokens.nodeBorder),
    primaryTextColor: hex(tokens.text),
    textColor: hex(tokens.text),
    nodeTextColor: hex(tokens.text),
    lineColor: hex(tokens.edge),
    clusterBkg: hex(tokens.cluster),
    secondaryColor: hex(tokens.cluster),
    tertiaryColor: hex(tokens.cluster),
    clusterBorder: hex(tokens.clusterBorder),
    titleColor: hex(tokens.muted),
    edgeLabelBackground: hex(tokens.canvas),
    fontFamily: tokens.font || undefined,
    fontSize: "13px",
    // 扁平卡片:不要 neo 外观默认的投影与渐变描边(暗色下是一圈发灰的光晕)。
    dropShadow: "none",
    useGradient: false,
    strokeWidth: 1,
  };
  for (const key of Object.keys(vars)) if (vars[key] === undefined) delete vars[key];
  return vars;
}
/// 五个语义类:渲染时按当前 token 追加到 flowchart 源码末尾(追加在末尾,错误行号不漂移)。
export function semanticClassDefs(tokens) {
  const ok = (value) => HEX.test(value ?? "");
  if (![tokens.text, tokens.canvas, tokens.muted, tokens.cluster, tokens.nodeBorder, tokens.accent, tokens.accentSoft].every(ok)) return [];
  return [
    `classDef entry stroke:${tokens.text},stroke-width:1.5px`,
    `classDef ext fill:${tokens.canvas},stroke:${tokens.muted},stroke-dasharray:4 3,color:${tokens.muted}`,
    `classDef store fill:${tokens.cluster},stroke:${tokens.nodeBorder}`,
    `classDef focus fill:${tokens.accentSoft},stroke:${tokens.accent},stroke-width:1.5px`,
    "classDef muted opacity:0.72",
  ];
}
function mermaidConfig(tokens, theme, layout = null) {
  return {
    startOnLoad: false,
    securityLevel: "strict",
    htmlLabels: false,
    theme: "base",
    look: "neo",
    layout: "elk",
    themeVariables: themeVariablesFromTokens(tokens, theme),
    fontFamily: tokens.font || undefined,
    flowchart: { curve: "basis", padding: 12, nodeSpacing: 32, rankSpacing: 44, diagramPadding: 12, useMaxWidth: false, htmlLabels: false, wrappingWidth: 360 },
    // mergeEdges 默认合并同向边(手写图与约简的 crate 图更紧凑);单张图可经 mountDiagram 的 layout 覆盖(「全部依赖」关掉它)。
    elk: { nodePlacementStrategy: "BRANDES_KOEPF", considerModelOrder: "NODES_AND_EDGES", mergeEdges: true, ...(layout ?? {}) },
    sequence: { useMaxWidth: false },
    state: { useMaxWidth: false },
    class: { useMaxWidth: false },
    er: { useMaxWidth: false },
    gantt: { useMaxWidth: false },
    maxEdges: 200,
    maxTextSize: 50000,
    suppressErrorRendering: true,
    deterministicIds: true,
  };
}

// ---------- 源码预处理 ----------
const CLICK_LINE = /^click\s+([^\s"]+)\s+"([^"]+)"(?:\s+"([^"]*)")?\s*$/;
const REF_TARGET = /^[RDISTF]-\d+$/;
/// 只认 `click <id> "<目标>" ["<提示>"]`;目标是条目号(R-123)或项目相对路径(可带 :行 / :行-行)。
/// 绝对路径、盘符、`..` 越界一律不当链接。
export function parseClickLine(line) {
  const match = String(line ?? "").trim().match(CLICK_LINE);
  if (!match) return null;
  const [, id, target, tip = ""] = match;
  if (REF_TARGET.test(target)) return { id, target, ref: target, path: null, line: null, tip };
  const parts = target.match(/^(.+?)(?::(\d+)(?:-\d+)?)?$/);
  const path = (parts?.[1] ?? "").replace(/\\/g, "/");
  if (!path || /^\/|^[A-Za-z]:|(^|\/)\.\.(\/|$)|^[a-z][\w+.-]*:\/\//i.test(path)) return null;
  return { id, target, ref: null, path, line: parts?.[2] ? Number(parts[2]) : null, tip };
}
export function parseClickDirectives(source) {
  const out = [];
  String(source ?? "").replace(/\r\n?/g, "\n").split("\n").forEach((line, index) => {
    const click = parseClickLine(line);
    if (click) out.push({ ...click, sourceLine: index + 1 });
  });
  return out;
}
/// 图种:frontmatter 与注释之后第一行的首个词(flowchart/graph 归一为 flowchart)。
export function diagramKind(source) {
  const lines = String(source ?? "").replace(/\r\n?/g, "\n").split("\n");
  let index = 0;
  if (lines[0]?.trim() === "---") {
    const end = lines.findIndex((line, i) => i > 0 && line.trim() === "---");
    index = end > 0 ? end + 1 : 0;
  }
  for (; index < lines.length; index += 1) {
    const line = lines[index].trim();
    if (!line || line.startsWith("%%")) continue;
    const word = line.split(/\s+/)[0];
    return word === "graph" ? "flowchart" : word;
  }
  return "";
}
/// 送进引擎前的源码:click 行、%%{…}%% 指令、整行 %% 注释、frontmatter 的 config 段都换成空行(行数不变),
/// flowchart 末尾追加语义类。lineMap[k] = mermaid 剥掉 frontmatter 与开头空行之后第 k+1 行在原文里的行号。
export function prepareDiagramSource(source, tokens = null) {
  const lines = String(source ?? "").replace(/\r\n?/g, "\n").split("\n");
  const clicks = [];
  // frontmatter 只认第一行起的 `---` … `---`(mermaid 同样只认开头);config 段(含缩进子行)抹掉,title 等照留。
  const fmEnd = FRONTMATTER_FENCE.test(lines[0] ?? "") ? lines.findIndex((line, i) => i > 0 && FRONTMATTER_FENCE.test(line)) : -1;
  let inConfig = false;
  let inDirective = false;
  const kept = lines.map((line, index) => {
    if (fmEnd > 0 && index <= fmEnd) {
      if (index === 0 || index === fmEnd) return line;
      if (/^config\s*:/.test(line)) {
        inConfig = true;
        return BLANKED;
      }
      if (inConfig && (/^\s/.test(line) || !line.trim())) return BLANKED;
      inConfig = false;
      return line;
    }
    const trimmed = line.trim();
    if (inDirective) {
      if (trimmed.includes("}%%")) inDirective = false;
      return BLANKED;
    }
    if (trimmed.startsWith("%%{")) {
      inDirective = !trimmed.includes("}%%");
      return BLANKED;
    }
    if (/^click\s/.test(trimmed)) {
      const click = parseClickLine(trimmed);
      if (click) clicks.push({ ...click, sourceLine: index + 1 });
      return BLANKED;
    }
    if (COMMENT_LINE.test(line)) return BLANKED;
    // 行内的 %%{…}%% 指令(`a --> b %%{init: …}%%`)同样只由 kanzei 注入配置。
    return line.includes("%%{") ? line.replace(/%%\{.*?\}%%/g, "") : line;
  });
  const lineMap = [];
  let started = false;
  kept.forEach((line, index) => {
    if (fmEnd > 0 && index <= fmEnd) return;
    if (!started && !line.trim()) return;
    started = true;
    lineMap.push(index + 1);
  });
  const kind = diagramKind(source);
  let text = kept.join("\n");
  if (kind === "flowchart" && tokens) {
    const defs = semanticClassDefs(tokens);
    if (defs.length) text += `\n${defs.join("\n")}`;
  }
  return { text, clicks, kind, lineCount: lines.length, lineMap };
}

// ---------- 渲染(串行队列 + LRU) ----------
const cache = new Map();
let queue = Promise.resolve();
let seq = 0;
function cacheGet(key) {
  const value = cache.get(key);
  if (value) {
    cache.delete(key);
    cache.set(key, value);
  }
  return value ?? null;
}
function cachePut(key, value) {
  cache.set(key, value);
  while (cache.size > CACHE_MAX) cache.delete(cache.keys().next().value);
}
const layoutKey = (layout) => (layout ? JSON.stringify(layout) : "");
const cacheKey = (source, theme, layout) => `${theme}\u0000${layoutKey(layout)}\u0000${source}`;
export function cachedDiagram(source, theme = currentDiagramTheme(), layout = null) {
  return cacheGet(cacheKey(source, theme, layout));
}
/// 插入前的字符串闸门:只看标签与属性,不看文字——标签正文里写「JavaScript: 前端」或 `online=true` 是正常内容。
export function svgSafetyIssues(svg) {
  const text = String(svg ?? "");
  const found = [];
  if (/<script\b/i.test(text)) found.push("script");
  if (/<foreignObject\b/i.test(text)) found.push("foreignObject");
  if (/<(?:iframe|object|embed)\b/i.test(text)) found.push("embed");
  if (/<[a-z][^<>]*\son[a-z]+\s*=/i.test(text)) found.push("on*");
  if (/<[a-z][^<>]*\s(?:xlink:)?href\s*=\s*["']?\s*javascript:/i.test(text)) found.push("javascript:");
  return found;
}
function errorInfo(error, prepared) {
  const raw = String(error?.message ?? error ?? "").trim();
  let line = Number(error?.hash?.loc?.first_line) || (Number.isFinite(error?.hash?.line) ? error.hash.line + 1 : 0);
  if (!line) line = Number(raw.match(/\bline (\d+)/i)?.[1] ?? 0);
  // jison 报的是 mermaid 剥掉 frontmatter 与开头空行之后的行号:经 lineMap 换回原文行号。
  // 错在追加的语义类里(不该发生)时,钉到原文最后一行。
  const original = (n) => prepared.lineMap?.[n - 1] ?? Math.min(n, prepared.lineCount);
  if (line) line = original(line);
  // mermaid 的报错带源码摘录与 ^ 指示行,取最后一行实质内容(Expecting … got …)。
  const meaningful = raw.split("\n").map((s) => s.trim()).filter((s) => s && !/^[-\s]*\^$/.test(s) && !/^Parse error on line \d+:?$/i.test(s));
  // langium 系图种(pie 等)把行号写在消息里(`Lexer error on line 4`),同样换成原文行号,免得与卡片上的行号打架。
  let message = (meaningful.at(-1) ?? raw).replace(/\b(line )(\d+)/gi, (_, word, n) => `${word}${original(Number(n))}`);
  // jison 的「Expecting 'A', 'B', … 十几项, got 'X'」只留前三项:要紧的是 got 什么,不是全部候选。
  const expecting = message.match(/^Expecting (.+), got (.+)$/);
  if (expecting) {
    const items = expecting[1].split(/,\s*/);
    message = `Expecting ${items.slice(0, 3).join(", ")}${items.length > 3 ? ", …" : ""}, got ${expecting[2]}`;
  }
  return { line: line || null, message: message.slice(0, 240) };
}
/// 渲染一段 mermaid:→ { svg, id, clicks, kind } 或 { error: { line, message }, clicks }。
/// layout:这张图的 ELK 覆盖项(如 { mergeEdges: false }),进缓存键。
export function renderDiagram(source, { theme = currentDiagramTheme(), layout = null } = {}) {
  const key = cacheKey(source, theme, layout);
  const hit = cacheGet(key);
  if (hit) return Promise.resolve(hit);
  const job = queue.then(() => renderNow(source, theme, key, layout));
  queue = job.catch(() => {});
  return job;
}
async function renderNow(source, theme, key, layout) {
  const again = cacheGet(key);
  if (again) return again;
  const tokens = diagramTokens();
  const prepared = prepareDiagramSource(source, tokens);
  let current;
  try {
    current = await loadEngine();
  } catch (error) {
    // 引擎加载失败不进缓存:下次再试。
    return { error: { line: null, message: `${t("图引擎加载失败")}:${String(error?.message ?? error)}` }, clicks: prepared.clicks, kind: prepared.kind };
  }
  let result;
  try {
    const config = mermaidConfig(tokens, theme, layout);
    const signature = JSON.stringify([config.themeVariables, config.elk]);
    if (signature !== configuredSignature && typeof current.initialize === "function") {
      current.initialize(config);
      configuredSignature = signature;
    }
    await current.parse(prepared.text);
    seq += 1;
    const id = `kzd-${seq}`;
    const { svg } = await current.render(id, prepared.text);
    const unsafe = svgSafetyIssues(svg);
    result = unsafe.length
      ? { error: { line: null, message: `${t("图里有不安全的内容,已拒绝显示")}(${unsafe.join(", ")})` }, clicks: prepared.clicks, kind: prepared.kind }
      : { svg, id, clicks: prepared.clicks, kind: prepared.kind };
  } catch (error) {
    result = { error: errorInfo(error, prepared), clicks: prepared.clicks, kind: prepared.kind };
  }
  cachePut(key, result);
  return result;
}

// ---------- 节点映射与交互 ----------
function nodeIdOf(g) {
  const dataId = g.getAttribute?.("data-id");
  if (dataId) return dataId;
  const match = String(g.id ?? g.getAttribute?.("id") ?? "").match(/flowchart-(.+)-\d+$/);
  return match ? match[1] : null;
}
function isAttached(el) {
  if (!el) return false;
  if (typeof el.isConnected === "boolean") return el.isConnected;
  for (let node = el; node; node = node.parentNode) {
    if (node === document.body || node === document.documentElement) return true;
  }
  return false;
}
/// 节点、边与点击映射。返回 { nodes: Map<id, g>, edges: [{ el, from, to }], mapped, unmapped }。
export function bindDiagram(svg, clicks = [], { root = null } = {}) {
  const nodes = new Map();
  for (const g of svg.querySelectorAll("g.node")) {
    const id = nodeIdOf(g);
    if (id && !nodes.has(id)) nodes.set(id, g);
  }
  const edges = [];
  for (const path of svg.querySelectorAll("path")) {
    const dataId = path.getAttribute?.("data-id") ?? "";
    if (!dataId.startsWith("L_")) continue;
    // L_<from>_<to>_<序号>:id 里可能也有下划线,用已知节点集合消解。
    const body = dataId.slice(2).replace(/_\d+$/, "");
    for (let i = body.indexOf("_"); i > 0; i = body.indexOf("_", i + 1)) {
      const from = body.slice(0, i);
      const to = body.slice(i + 1);
      if (nodes.has(from) && nodes.has(to)) {
        edges.push({ el: path, from, to });
        break;
      }
    }
  }
  const neighbours = new Map([...nodes.keys()].map((id) => [id, { nodes: new Set([id]), edges: new Set() }]));
  for (const edge of edges) {
    neighbours.get(edge.from)?.nodes.add(edge.to);
    neighbours.get(edge.to)?.nodes.add(edge.from);
    neighbours.get(edge.from)?.edges.add(edge.el);
    neighbours.get(edge.to)?.edges.add(edge.el);
  }
  const mapped = [];
  const unmapped = [];
  for (const click of clicks) {
    const g = nodes.get(click.id);
    if (!g) {
      unmapped.push(click);
      continue;
    }
    mapped.push(click);
    const rows = [...(g.querySelectorAll?.("tspan.row") ?? [])].map((row) => row.textContent);
    const label = (rows.length ? rows.join(" ") : String(g.textContent ?? "")).replace(/\s+/g, " ").trim() || click.id;
    const hint = click.tip || click.target;
    g.classList.add("is-link");
    g.setAttribute("tabindex", "0");
    g.setAttribute("role", "link");
    g.setAttribute("aria-label", `${label} · ${hint}`);
    g.setAttribute("title", hint);
    const activate = () => {
      if (click.ref) host.openRef(click.ref);
      else host.openPath(click.path, click.line);
    };
    g.addEventListener("click", (event) => {
      event.preventDefault?.();
      event.stopPropagation?.();
      activate();
    });
    g.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault?.();
      activate();
    });
  }
  // 悬停/聚焦:相邻节点与边保持,其余淡化(只动 opacity,不跟 mermaid 的 #id 样式抢优先级)。
  const scope = root ?? svg;
  const clear = () => {
    scope.classList?.remove("is-focusing");
    for (const el of svg.querySelectorAll(".is-related")) el.classList.remove("is-related");
  };
  const focus = (id) => {
    const near = neighbours.get(id);
    if (!near) return;
    clear();
    scope.classList?.add("is-focusing");
    for (const nodeId of near.nodes) nodes.get(nodeId)?.classList.add("is-related");
    for (const el of near.edges) el.classList.add("is-related");
  };
  for (const [id, g] of nodes) {
    g.addEventListener("pointerenter", () => focus(id));
    g.addEventListener("pointerleave", clear);
    g.addEventListener("focus", () => focus(id));
    g.addEventListener("blur", clear);
  }
  return { nodes, edges, mapped, unmapped };
}

// ---------- 查看器 ----------
const mounted = new Set();
let themeObserver = null;
function watchTheme() {
  if (themeObserver || typeof MutationObserver !== "function") return;
  themeObserver = new MutationObserver((records) => {
    if (!records.some((record) => record.attributeName === "data-theme")) return;
    const theme = currentDiagramTheme();
    for (const view of [...mounted]) {
      if (!isAttached(view.root)) {
        mounted.delete(view);
        continue;
      }
      if (view.theme !== theme) view.rerender();
    }
  });
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
}
function el(tag, className = "", text = null) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== null && text !== undefined) node.textContent = String(text);
  return node;
}
function button(act, label, title) {
  const node = el("button", "kz-diagram-btn", label);
  node.type = "button";
  node.dataset.act = act;
  node.setAttribute("title", title);
  node.setAttribute("aria-label", title);
  return node;
}
function naturalSize(svg) {
  const width = parseFloat(svg.getAttribute?.("width") ?? "");
  const height = parseFloat(svg.getAttribute?.("height") ?? "");
  if (width > 0 && height > 0) return { w: width, h: height };
  const box = String(svg.getAttribute?.("viewBox") ?? "").split(/[\s,]+/).map(Number);
  if (box.length === 4 && box[2] > 0 && box[3] > 0) return { w: box[2], h: box[3] };
  return { w: 0, h: 0 };
}
function uniqueSvg(result) {
  // 同一份缓存 SVG 可能同时出现在多处(聊天里同一张图两次、主题切换重挂):换一个 id 前缀,
  // 免得箭头 marker 的 url(#…) 解析到另一份里去。
  seq += 1;
  const fresh = `kzd-${seq}`;
  return { svg: String(result.svg).replace(new RegExp(`${result.id}(?![0-9])`, "g"), fresh), id: fresh };
}
function domUnsafe(svg) {
  if (svg.querySelector?.("script, foreignObject")) return true;
  for (const node of svg.querySelectorAll?.("*") ?? []) {
    for (const attr of node.attributes ?? []) {
      if (/^on/i.test(attr.name)) return true;
      if (/^(?:xlink:)?href$/i.test(attr.name) && /^\s*javascript:/i.test(attr.value ?? "")) return true;
    }
  }
  return false;
}
export function fixHint({ path, fileLine, message, lineText }) {
  const where = path ? fill(t("{path} 第 {line} 行"), { path, line: fileLine ?? "?" }) : fill(t("这段 mermaid 第 {line} 行"), { line: fileLine ?? "?" });
  const tail = lineText ? `\n${t("该行")}:${lineText}` : "";
  return `${fill(t("修复 {where} 的 mermaid 语法:{message}"), { where, message })}${tail}`;
}

/// 挂一张图。mode: "page"(架构页:适应/缩放/平移,高度 clamp(240, 图高, 70vh))|"inline"(markdown 里:
/// 宽度撑满、最高 520、不劫持滚轮,可「放大」到查看器)。path/sourceLine 让错误行号换算成文件行号。
/// layout:ELK 覆盖项;variant:写到 figure 的 data-variant,样式按它区分(如 "deps-full" 把传递边画淡)。
/// 返回控制器 { root, theme, rerender(), setSource(src, { layout, variant }), fit(), destroy(), result }。
export function mountDiagram(container, source, options = {}) {
  const mode = options.mode === "page" ? "page" : "inline";
  const figure = el("figure", "kz-diagram");
  figure.dataset.mode = mode;
  figure.dataset.state = "loading";
  if (options.variant) figure.dataset.variant = options.variant;
  const bar = el("div", "kz-diagram-bar");
  bar.setAttribute("role", "toolbar");
  bar.setAttribute("aria-label", t("图工具"));
  const zoomLabel = el("span", "kz-diagram-zoom", "");
  bar.append(
    button("fit", t("适应"), t("适应画布")),
    button("zoom-out", "−", t("缩小")),
    zoomLabel,
    button("zoom-in", "+", t("放大")),
    button("actual", "1:1", t("实际大小")),
    button("source", t("源码"), t("查看源码")),
    button("copy", t("复制"), t("复制图源码")),
  );
  if (mode === "inline") bar.append(button("expand", "⤢", t("在查看器里放大")));
  // 页面模式可把工具条放进宿主给的位置(架构页放在标签栏右侧),不压在图上。
  if (options.toolbarHost) bar.classList.add("is-docked");
  const canvas = el("div", "kz-diagram-canvas");
  canvas.tabIndex = 0;
  canvas.setAttribute("aria-label", options.title || t("图"));
  const stage = el("div", "kz-diagram-stage");
  canvas.append(stage);
  const status = el("p", "kz-diagram-status", t("正在绘图…"));
  const hint = el("p", "kz-diagram-hint hidden", t("拖动查看全部 · Ctrl+滚轮缩放"));
  const errorBox = el("div", "kz-diagram-error hidden");
  errorBox.setAttribute("role", "alert");
  figure.append(canvas, status, hint, errorBox);
  if (options.toolbarHost) options.toolbarHost.append(bar);
  else figure.prepend(bar);
  container.replaceChildren?.();
  container.append(figure);

  const view = {
    root: figure,
    theme: currentDiagramTheme(),
    source: String(source ?? ""),
    layout: options.layout ?? null,
    result: null,
    graph: null,
    scale: 1,
    x: 0,
    y: 0,
    natural: { w: 0, h: 0 },
    /// 用户动过缩放/平移后,窗口变化不再自动「适应」(点「适应」或双击背景复位)。
    userMoved: false,
    rerender,
    setSource,
    fit,
    destroy,
  };
  const apply = () => {
    stage.style.transform = `translate(${Math.round(view.x)}px, ${Math.round(view.y)}px) scale(${view.scale})`;
    zoomLabel.textContent = `${Math.round(view.scale * 100)}%`;
  };
  const canvasSize = () => ({ w: canvas.clientWidth || 0, h: canvas.clientHeight || 0 });
  const clampPan = () => {
    const { w, h } = canvasSize();
    const cw = view.natural.w * view.scale;
    const ch = view.natural.h * view.scale;
    view.x = cw + 2 * PAD <= w ? (w - cw) / 2 : Math.min(PAD, Math.max(w - cw - PAD, view.x));
    view.y = ch + 2 * PAD <= h ? (h - ch) / 2 : Math.min(PAD, Math.max(h - ch - PAD, view.y));
    // 图比画布大时才显示「可拖动」光标(放得下的图不该顶着一只抓手)。
    if (cw + 2 * PAD > w || ch + 2 * PAD > h) figure.dataset.pannable = "true";
    else delete figure.dataset.pannable;
  };
  function fit() {
    if (!view.natural.w) return;
    const width = canvasSize().w || view.natural.w + 2 * PAD;
    const viewport = typeof window !== "undefined" ? window.innerHeight || 800 : 800;
    const maxH = mode === "page" ? Math.max(PAGE_MIN_H, viewport * 0.7) : INLINE_MAX_H;
    const raw = Math.min(FIT_MAX, (width - 2 * PAD) / view.natural.w, (maxH - 2 * PAD) / view.natural.h);
    const floor = fitMin();
    view.scale = Math.max(floor, raw);
    const height = Math.min(maxH, Math.max(mode === "page" ? PAGE_MIN_H : 120, view.natural.h * view.scale + 2 * PAD));
    figure.style.setProperty("--kz-diagram-h", `${Math.round(height)}px`);
    hint.classList.toggle("hidden", raw >= floor);
    view.x = 0;
    view.y = 0;
    view.userMoved = false;
    clampPan();
    apply();
  }
  const zoomAt = (factor, cx = null, cy = null) => {
    const { w, h } = canvasSize();
    const px = cx ?? w / 2;
    const py = cy ?? h / 2;
    const next = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, view.scale * factor));
    const k = next / view.scale;
    view.x = px - (px - view.x) * k;
    view.y = py - (py - view.y) * k;
    view.scale = next;
    view.userMoved = true;
    clampPan();
    apply();
  };
  function showError(error) {
    figure.dataset.state = "error";
    bar.dataset.state = "error";
    status.classList.add("hidden");
    const localLine = error?.line ?? null;
    const fileLine = localLine && options.sourceLine ? options.sourceLine + localLine - 1 : localLine;
    const lineText = localLine ? view.source.replace(/\r\n?/g, "\n").split("\n")[localLine - 1]?.trim() ?? "" : "";
    errorBox.replaceChildren(
      el("strong", "kz-diagram-error-title", t("图渲染失败")),
      el("p", "kz-diagram-error-msg", `${fileLine ? `${fill(t("第 {line} 行"), { line: fileLine })}:` : ""}${error?.message ?? ""}`),
    );
    const actions = el("div", "kz-diagram-error-actions");
    const src = button("source", t("查看源码"), t("查看源码"));
    const copyHint = button("copy-hint", t("复制修复提示"), t("复制一句可直接交给 agent 的修复提示"));
    copyHint.addEventListener("click", () => copyText(fixHint({ path: options.path, fileLine, message: error?.message ?? "", lineText })));
    src.addEventListener("click", () => openSource(localLine));
    actions.append(src, copyHint);
    errorBox.append(actions);
    errorBox.classList.remove("hidden");
    // markdown 里的坏图:原文照样看得见(不吞内容)。
    if (mode === "inline") {
      const pre = el("pre", "code kz-diagram-fallback");
      pre.append(el("code", "language-mermaid", view.source));
      errorBox.append(pre);
    }
    options.onRendered?.({ error, view });
  }
  function showResult(result) {
    view.result = result;
    if (result.error) {
      showError(result.error);
      return;
    }
    const { svg: markup } = uniqueSvg(result);
    let svg = null;
    if (typeof engine?.mount === "function") svg = engine.mount(stage, markup);
    else {
      stage.innerHTML = markup;
      svg = stage.querySelector("svg");
    }
    if (!svg || domUnsafe(svg)) {
      stage.replaceChildren?.();
      showError({ line: null, message: t("图里有不安全的内容,已拒绝显示") });
      return;
    }
    figure.dataset.state = "ready";
    bar.dataset.state = "ready";
    status.classList.add("hidden");
    errorBox.classList.add("hidden");
    view.natural = naturalSize(svg);
    view.graph = bindDiagram(svg, result.clicks ?? [], { root: figure });
    figure.dataset.nodes = String(view.graph.nodes.size);
    figure.dataset.edges = String(view.graph.edges.length);
    fit();
    options.onRendered?.({ result, view });
  }
  function rerender() {
    view.theme = currentDiagramTheme();
    figure.dataset.state = "loading";
    bar.dataset.state = "loading";
    const hit = cachedDiagram(view.source, view.theme, view.layout);
    if (hit) {
      showResult(hit);
      return Promise.resolve(hit);
    }
    status.classList.remove("hidden");
    const wanted = view.source;
    const theme = view.theme;
    const layout = view.layout;
    return renderDiagram(wanted, { theme, layout }).then(
      (result) => {
        if (view.source !== wanted || view.theme !== theme || view.layout !== layout) return result;
        showResult(result);
        return result;
      },
      (error) => {
        const result = { error: { line: null, message: String(error?.message ?? error) }, clicks: [] };
        if (view.source === wanted) showResult(result);
        return result;
      },
    );
  }
  function setSource(next, { layout, variant } = {}) {
    view.source = String(next ?? "");
    if (layout !== undefined) view.layout = layout ?? null;
    if (variant !== undefined) {
      if (variant) figure.dataset.variant = variant;
      else delete figure.dataset.variant;
    }
    return rerender();
  }
  function destroy() {
    mounted.delete(view);
    bar.remove();
    figure.remove();
  }
  function openSource(line = null) {
    const payload = { title: options.title || t("图源码"), source: view.source, line, startLine: options.sourceLine || 1, path: options.path || null };
    if (host.openSource) {
      host.openSource(payload);
      return;
    }
    // 没注册查看器(样例页/冒烟):在图下方展开源码。
    let pre = figure.querySelector(".kz-diagram-source");
    if (pre) {
      pre.remove();
      return;
    }
    pre = el("pre", "code kz-diagram-source");
    pre.append(el("code", "", view.source));
    figure.append(pre);
  }
  function copyText(text) {
    const done = () => host.toast?.(t("已复制"), "ok");
    try {
      const pending = navigator.clipboard?.writeText(text);
      if (pending?.then) pending.then(done, () => host.toast?.(t("复制失败"), "err"));
      else done();
    } catch {
      host.toast?.(t("复制失败"), "err");
    }
  }
  bar.addEventListener("click", (event) => {
    const act = event.target?.closest?.("[data-act]")?.dataset?.act ?? event.target?.dataset?.act;
    if (!act) return;
    if (act === "fit") fit();
    else if (act === "zoom-in") zoomAt(1.25);
    else if (act === "zoom-out") zoomAt(0.8);
    else if (act === "actual") zoomAt(1 / view.scale);
    else if (act === "source") openSource(view.result?.error?.line ?? null);
    else if (act === "copy") copyText(view.source);
    else if (act === "expand") host.openLarge?.({ title: options.title || t("图"), source: view.source, path: options.path || null, sourceLine: options.sourceLine || 1 });
  });
  // 平移:在背景上按住拖(节点上的按下留给点击)。手势挂 document,甩得再快也跟得上;不捕获指针(J4)。
  canvas.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || !figure.dataset.pannable || event.target?.closest?.("g.node.is-link")) return;
    const start = { x: event.clientX, y: event.clientY, vx: view.x, vy: view.y };
    let moved = false;
    const move = (e) => {
      const dx = e.clientX - start.x;
      const dy = e.clientY - start.y;
      if (!moved && Math.abs(dx) + Math.abs(dy) < 3) return;
      moved = true;
      view.userMoved = true;
      figure.dataset.panning = "true";
      view.x = start.vx + dx;
      view.y = start.vy + dy;
      clampPan();
      apply();
    };
    const up = () => {
      delete figure.dataset.panning;
      document.removeEventListener("pointermove", move, true);
      document.removeEventListener("pointerup", up, true);
      document.removeEventListener("pointercancel", up, true);
    };
    document.addEventListener("pointermove", move, true);
    document.addEventListener("pointerup", up, true);
    document.addEventListener("pointercancel", up, true);
  });
  canvas.addEventListener("dblclick", (event) => {
    if (event.target?.closest?.("g.node.is-link")) return;
    fit();
  });
  // Ctrl+滚轮 / 触控板捏合缩放;普通滚轮不劫持(页面照常滚动)。
  canvas.addEventListener("wheel", (event) => {
    if (!event.ctrlKey) return;
    event.preventDefault?.();
    const rect = canvas.getBoundingClientRect();
    zoomAt(event.deltaY < 0 ? 1.1 : 1 / 1.1, event.clientX - rect.left, event.clientY - rect.top);
  }, { passive: false });
  canvas.addEventListener("keydown", (event) => {
    if (event.target !== canvas) return;
    const step = 48;
    if (event.key === "+" || event.key === "=") zoomAt(1.25);
    else if (event.key === "-") zoomAt(0.8);
    else if (event.key === "0") fit();
    else if (event.key === "ArrowLeft") view.x += step;
    else if (event.key === "ArrowRight") view.x -= step;
    else if (event.key === "ArrowUp") view.y += step;
    else if (event.key === "ArrowDown") view.y -= step;
    else return;
    event.preventDefault?.();
    clampPan();
    apply();
  });

  // 画布尺寸变了(窗口缩放、分隔条、侧栏停靠)且用户没手动缩放过:重新适应。
  if (typeof ResizeObserver === "function") {
    let lastWidth = 0;
    new ResizeObserver(() => {
      const width = canvas.clientWidth || 0;
      if (!width || Math.abs(width - lastWidth) < 2) return;
      lastWidth = width;
      if (figure.dataset.state === "ready" && !view.userMoved) fit();
    }).observe(canvas);
  }
  // 流式渲染每帧都会重挂一遍,已摘下的旧视图在这里顺手清掉(主题切换时也会清)。
  if (mounted.size > 48) for (const old of mounted) if (!isAttached(old.root)) mounted.delete(old);
  mounted.add(view);
  watchTheme();
  rerender();
  return view;
}

// ---------- markdown 里的 ```mermaid ----------
/// 把 root 里闭合的 mermaid 围栏换成图。未闭合(<pre data-open>,流式中还没写完)的永不渲染。
/// 缓存命中:同步替换(流式每帧重设 innerHTML 也不闪);未命中:先留着代码块,渲染完再换——
/// 那时若代码块已被下一帧替掉,就对 root 再跑一遍(缓存已热,同步命中)。
export function hydrateDiagrams(root, { streaming = false } = {}) {
  if (!root?.querySelectorAll) return 0;
  let replaced = 0;
  for (const code of root.querySelectorAll("code.language-mermaid")) {
    const pre = code.parentNode;
    if (!pre || pre.tagName !== "PRE" || pre.classList?.contains("kz-diagram-fallback") || pre.classList?.contains("kz-diagram-source")) continue;
    if (pre.dataset?.open === "true" || pre.getAttribute?.("data-open") === "true") continue;
    const source = code.textContent ?? "";
    if (!source.trim()) continue;
    const theme = currentDiagramTheme();
    const swap = () => {
      const holder = document.createElement("div");
      holder.className = "kz-diagram-host";
      pre.replaceWith(holder);
      mountDiagram(holder, source, { mode: "inline" });
      replaced += 1;
    };
    if (cachedDiagram(source, theme)) {
      swap();
      continue;
    }
    renderDiagram(source, { theme }).then(() => {
      if (isAttached(pre) && (code.textContent ?? "") === source) swap();
      else if (isAttached(root)) hydrateDiagrams(root, { streaming });
    }, () => {});
  }
  return replaced;
}
