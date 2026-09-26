// 共享图渲染器:vendored force-graph 1.51.4(canvas 2D,MIT)的一层薄封装。
//
// 现在给记忆知识图谱用(24-memory-graph.js);R-368 B5 的引用邻域图与 R-307 B3 的依赖 DAG
// 以后走同一个渲染器(layout: "dag-lr"),不再各写一套手写 SVG(决策见
// docs/design/memory_knowledge_graph.md §7 与 doc_reference_graph.md)。
//
// 约定:
// - 懒加载:第一次打开图谱才 fetch vendor 文件;用 CommonJS 垫片执行——17-files.js 加载的 Monaco
//   loader 定义了全局 define.amd,普通 <script> 会让 UMD 走 AMD 分支,window.ForceGraph 永远拿不到。
// - 颜色只从 --graph-* token 读(getComputedStyle),主题切换(kz:theme)重读后重画;脚本里不写字面量颜色。
// - 关闭库自带的浮动提示(nodeLabel 返回空串):悬停信息写进页面状态栏,不另起浮层(弹层只走 00-surface.js)。
// - 任何锚点先做有限值校验:d3 四叉树遇到 Infinity/NaN 会死循环、界面卡死。
import { convexHull, neighborsOf, placeLabels } from "./24-memory-graph-model.js";

export const FORCE_GRAPH_URL = "vendor/force-graph/force-graph-1.51.4.min.js";

let forceGraphPromise = null;

/** 加载 force-graph 构造器(只加载一次;失败后下次重试)。 */
export function loadForceGraph() {
  if (!forceGraphPromise) {
    forceGraphPromise = (async () => {
      const response = await fetch(FORCE_GRAPH_URL);
      if (!response.ok) throw new Error(`HTTP ${response.status} ${FORCE_GRAPH_URL}`);
      const source = await response.text();
      const module = { exports: {} };
      // define 显式传 undefined:UMD 包装先判 CommonJS(module/exports)再判 define.amd。
      new Function("module", "exports", "define", `${source}\n//# sourceURL=${FORCE_GRAPH_URL}`)(module, module.exports, undefined);
      const factory = module.exports?.default ?? module.exports;
      if (typeof factory !== "function") throw new Error("force-graph 没有导出构造函数");
      return factory;
    })().catch((error) => {
      forceGraphPromise = null;
      throw error;
    });
  }
  return forceGraphPromise;
}

/** 环境里有没有可用的 2D canvas(假 DOM 冒烟里没有,直接走文本视图)。 */
export function canvasSupported() {
  try {
    return typeof document?.createElement === "function" && Boolean(document.createElement("canvas")?.getContext?.("2d"));
  } catch {
    return false;
  }
}

const PALETTE_TOKENS = {
  fact: "--graph-fact",
  sop: "--graph-sop",
  habit: "--graph-habit",
  preference: "--graph-preference",
  context: "--graph-context",
  crate: "--graph-crate",
  module: "--graph-module",
  concept: "--graph-concept",
  edge: "--graph-edge",
  edgeWeak: "--graph-edge-weak",
  label: "--graph-label",
  labelDim: "--graph-label-dim",
  focus: "--graph-focus",
  halo: "--graph-halo",
  bg: "--graph-bg",
  hit: "--graph-hit",
  font: "--sans",
};

/** 读 --graph-* token 的计算值(随 html[data-theme] 变)。 */
export function readGraphPalette(root = document.documentElement) {
  const style = getComputedStyle(root);
  return Object.fromEntries(Object.entries(PALETTE_TOKENS).map(([key, token]) => [key, style.getPropertyValue(token).trim()]));
}

const finite = (value, fallback = 0) => (Number.isFinite(value) ? value : fallback);
const endOf = (end) => (end && typeof end === "object" ? end : null);

function clusterForce(clusterOf, anchors, unassigned, strengthOf) {
  let nodes = [];
  const force = (alpha) => {
    for (const node of nodes) {
      if (node.fx != null) continue;
      const group = clusterOf(node);
      if (group === undefined) {
        node.vx -= finite(node.x) * 0.02 * alpha;
        node.vy -= finite(node.y) * 0.02 * alpha;
        continue;
      }
      const anchor = (group && anchors.get(group)) || unassigned;
      const ax = finite(anchor?.x);
      const ay = finite(anchor?.y);
      const strength = strengthOf(node);
      node.vx += (ax - finite(node.x)) * strength * alpha;
      node.vy += (ay - finite(node.y)) * strength * alpha;
    }
  };
  force.initialize = (list) => {
    nodes = list;
  };
  return force;
}

function radialForce(hopOf, step) {
  let nodes = [];
  const force = (alpha) => {
    for (const node of nodes) {
      const hop = hopOf.get(node.id) ?? 3;
      if (hop === 0 || node.fx != null) continue;
      const x = finite(node.x);
      const y = finite(node.y);
      const distance = Math.hypot(x, y) || 1;
      const k = ((hop * step - distance) / distance) * alpha * 0.3;
      node.vx += x * k;
      node.vy += y * k;
    }
  };
  force.initialize = (list) => {
    nodes = list;
  };
  return force;
}

/**
 * 在 host 里建一个图视图。返回 Promise<view>;库加载失败会 reject(调用方降级为文本视图)。
 * opts:
 *   nodeStyle(n) → { shape, color, fill, dashed, alpha, glyph }(color 是调色板 key)
 *   nodeRadius(n) → 世界坐标半径
 *   nodeLabel(n, scale, state) → 要显示的标签文字(空串 = 不显示);state = { hovered, lit, selected, hit }
 *   labelPriority(n, state) → 标签抢位优先级(大者先放)
 *   linkStyle(l) → { dashed, arrow }
 *   relLabel(rel) → 边标签文字
 *   clusterOf(n) → crate 级分组 id(null = 未归类锚点;undefined = 不受聚类力,只受弱向心力)
 *   hullGroup(n) → 外壳分组 id 或 null
 *   onHover(n|null) / onClick(n) / onDblClick(n) / onBackgroundClick() / onSettled({ settleMs, nodes, links })
 */
export async function createGraphView(host, opts = {}) {
  const ForceGraph = await loadForceGraph();
  let palette = readGraphPalette();
  let data = { nodes: [], links: [] };
  let neighbors = new Map();
  let hoverId = null;
  let selectedId = null;
  let hits = null;
  let layout = "clusters";
  let anchors = new Map();
  let unassigned = { x: 0, y: 0 };
  let hopOf = new Map();
  let labelSet = new Set();
  let labelText = new Map();
  let settleStarted = 0;
  let settledOnce = false;
  let destroyed = false;
  let lastClick = { id: null, at: 0 };
  let redrawTimer = 0;

  const fg = ForceGraph()(host);
  const size = () => {
    const width = Math.max(1, Math.floor(host.clientWidth));
    const height = Math.max(1, Math.floor(host.clientHeight));
    fg.width(width).height(height);
  };
  size();

  const lit = (id) => !hoverId || id === hoverId || neighbors.get(hoverId)?.has(id);
  const stateOf = (node) => ({
    hovered: node.id === hoverId,
    lit: Boolean(hoverId) && lit(node.id),
    selected: node.id === selectedId,
    hit: Boolean(hits?.has(node.id)),
  });
  const baseRadius = (node) => finite(opts.nodeRadius?.(node), 5);
  // 缩小看全局时节点不能缩成看不见的点:按节点种类给一个屏幕像素下限。
  const MIN_SCREEN_RADIUS = { crate: 6, module: 3.5, memory: 3 };
  let currentScale = 1;
  const radius = (node) => Math.max(baseRadius(node), (MIN_SCREEN_RADIUS[node.kind] ?? 2.5) / Math.max(currentScale, 0.05));
  const incident = (link, id) => id && (endOf(link.source)?.id === id || endOf(link.target)?.id === id);

  const fontFamily = () => palette.font || "sans-serif";

  function paintNode(node, ctx, scale) {
    if (!Number.isFinite(node.x) || !Number.isFinite(node.y)) return;
    const style = opts.nodeStyle?.(node) ?? { shape: "circle", color: "context", fill: true, alpha: 1 };
    const r = radius(node);
    const color = palette[style.color] || palette.context;
    const dimByHover = hoverId && !lit(node.id) ? 0.12 : 1;
    const dimBySearch = hits && hits.size && !hits.has(node.id) && node.id !== selectedId ? 0.35 : 1;
    ctx.save();
    ctx.globalAlpha = finite(style.alpha, 1) * dimByHover * dimBySearch;
    ctx.beginPath();
    if (style.shape === "diamond") {
      ctx.moveTo(node.x, node.y - r);
      ctx.lineTo(node.x + r, node.y);
      ctx.lineTo(node.x, node.y + r);
      ctx.lineTo(node.x - r, node.y);
      ctx.closePath();
    } else {
      ctx.arc(node.x, node.y, r, 0, 2 * Math.PI);
    }
    if (style.fill) {
      ctx.fillStyle = color;
      ctx.fill();
    } else {
      ctx.fillStyle = palette.bg;
      ctx.fill();
      ctx.lineWidth = Math.max(1.2, r * 0.3);
      ctx.strokeStyle = color;
      if (style.dashed) ctx.setLineDash([2, 1.6]);
      ctx.stroke();
      ctx.setLineDash([]);
    }
    if (style.fill && style.dashed) {
      ctx.lineWidth = 1;
      ctx.strokeStyle = color;
      ctx.setLineDash([2, 1.6]);
      ctx.beginPath();
      ctx.arc(node.x, node.y, r + 1.5, 0, 2 * Math.PI);
      ctx.stroke();
      ctx.setLineDash([]);
    }
    if (style.glyph) {
      ctx.fillStyle = palette.bg;
      ctx.font = `600 ${r * 1.25}px ${fontFamily()}`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(style.glyph, node.x, node.y + r * 0.06);
    }
    if (hits?.has(node.id)) {
      ctx.globalAlpha = 1;
      ctx.lineWidth = 2 / scale;
      ctx.strokeStyle = palette.hit;
      ctx.beginPath();
      ctx.arc(node.x, node.y, r + 5 / scale + 1, 0, 2 * Math.PI);
      ctx.stroke();
    }
    if (node.id === selectedId || node.id === hoverId) {
      ctx.globalAlpha = 1;
      ctx.lineWidth = (node.id === selectedId ? 2 : 1.4) / scale;
      ctx.strokeStyle = palette.focus;
      ctx.beginPath();
      ctx.arc(node.x, node.y, r + 2.5 / scale + 0.5, 0, 2 * Math.PI);
      ctx.stroke();
    }
    ctx.restore();
  }

  function framePost(ctx, scale) {
    // 标签最后画:压在节点与连线之上,描一圈底色描边保证在连线上也读得清。
    ctx.save();
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    ctx.lineJoin = "round";
    for (const node of data.nodes) {
      if (!labelSet.has(node.id)) continue;
      const text = labelText.get(node.id);
      if (!text || !Number.isFinite(node.x) || !Number.isFinite(node.y)) continue;
      const state = stateOf(node);
      const strong = node.kind === "crate";
      const fontPx = (strong ? 13 : 11) / scale;
      ctx.globalAlpha = hoverId && !lit(node.id) ? 0.25 : 1;
      ctx.font = `${strong ? "600 " : ""}${fontPx}px ${fontFamily()}`;
      ctx.lineWidth = 3 / scale;
      ctx.strokeStyle = palette.bg;
      const y = node.y + radius(node) + 2 / scale;
      ctx.strokeText(text, node.x, y);
      ctx.fillStyle = strong || state.hovered || state.selected || state.lit ? palette.label : palette.labelDim;
      ctx.fillText(text, node.x, y);
    }
    ctx.restore();
  }

  function paintLink(link, ctx, scale) {
    const source = endOf(link.source);
    const target = endOf(link.target);
    if (!source || !target || !Number.isFinite(source.x) || !Number.isFinite(target.x)) return;
    const style = opts.linkStyle?.(link) ?? { dashed: link.strength === "weak", arrow: link.strength === "strong" };
    const on = incident(link, hoverId) || incident(link, selectedId);
    const dx = target.x - source.x;
    const dy = target.y - source.y;
    const length = Math.hypot(dx, dy) || 1;
    const ux = dx / length;
    const uy = dy / length;
    const x0 = source.x + ux * radius(source);
    const y0 = source.y + uy * radius(source);
    const x1 = target.x - ux * radius(target);
    const y1 = target.y - uy * radius(target);
    ctx.save();
    ctx.globalAlpha = hoverId && !on ? 0.08 : on ? 1 : 0.75;
    ctx.strokeStyle = link.strength === "weak" && !on ? palette.edgeWeak : palette.edge;
    ctx.lineWidth = (on ? 1.6 : 0.8) / Math.max(scale, 0.35);
    if (style.dashed) ctx.setLineDash([3 / Math.max(scale, 0.5), 3 / Math.max(scale, 0.5)]);
    ctx.beginPath();
    ctx.moveTo(x0, y0);
    ctx.lineTo(x1, y1);
    ctx.stroke();
    ctx.setLineDash([]);
    if (style.arrow && length > 12) {
      const size = 3.2;
      ctx.fillStyle = ctx.strokeStyle;
      ctx.beginPath();
      ctx.moveTo(x1, y1);
      ctx.lineTo(x1 - ux * size * 1.6 + uy * size * 0.8, y1 - uy * size * 1.6 - ux * size * 0.8);
      ctx.lineTo(x1 - ux * size * 1.6 - uy * size * 0.8, y1 - uy * size * 1.6 + ux * size * 0.8);
      ctx.closePath();
      ctx.fill();
    }
    // 边标签只给度数不大的悬停/选中节点画:模块、crate 这类枢纽一悬停就是十几条「关于」,叠成一团反而读不出。
    const labelOwner = incident(link, hoverId) ? hoverId : selectedId;
    if (on && opts.relLabel && (neighbors.get(labelOwner)?.size ?? 0) <= 8) {
      const label = opts.relLabel(link.rel);
      if (label) {
        const fontPx = 10 / scale;
        ctx.font = `${fontPx}px ${fontFamily()}`;
        const mx = (x0 + x1) / 2;
        const my = (y0 + y1) / 2;
        const width = ctx.measureText(label).width;
        ctx.globalAlpha = 1;
        ctx.fillStyle = palette.bg;
        ctx.fillRect(mx - width / 2 - 2 / scale, my - fontPx * 0.65, width + 4 / scale, fontPx * 1.3);
        ctx.fillStyle = palette.labelDim;
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(label, mx, my);
      }
    }
    ctx.restore();
  }

  function framePre(ctx, scale) {
    currentScale = scale || 1;
    // ① 标签抢位:屏幕坐标里按优先级贪心放,重叠就不画(否则缩小时满屏叠字)。
    const transform = ctx.getTransform();
    const pixelRatio = transform.a / (scale || 1) || 1;
    const cands = [];
    labelText = new Map();
    for (const node of data.nodes) {
      if (!Number.isFinite(node.x) || !Number.isFinite(node.y)) continue;
      const state = stateOf(node);
      const text = opts.nodeLabel?.(node, scale, state) ?? "";
      if (!text) continue;
      labelText.set(node.id, text);
      const fontCss = node.kind === "crate" ? 13 : 11;
      ctx.font = `${node.kind === "crate" ? "600 " : ""}${fontCss * pixelRatio}px ${fontFamily()}`;
      const width = ctx.measureText(text).width + 4 * pixelRatio;
      const height = (fontCss + 3) * pixelRatio;
      const sx = transform.a * node.x + transform.e;
      const sy = transform.d * (node.y + radius(node)) + transform.f + (2 * pixelRatio) + height / 2;
      cands.push({ id: node.id, x: sx, y: sy, w: width, h: height, priority: opts.labelPriority?.(node, state) ?? 0 });
    }
    labelSet = placeLabels(cands, 64 * pixelRatio);
    // ② 区域外壳:同一 crate 的节点画一圈淡色圆角凸包;放大或邻域模式下隐藏。
    if (layout !== "clusters" || scale > 2 || !opts.hullGroup) return;
    const groups = new Map();
    for (const node of data.nodes) {
      const group = opts.hullGroup(node);
      if (!group || !Number.isFinite(node.x) || !Number.isFinite(node.y)) continue;
      if (!groups.has(group)) groups.set(group, []);
      groups.get(group).push([node.x, node.y]);
    }
    ctx.save();
    ctx.globalAlpha = 0.05;
    ctx.fillStyle = palette.halo;
    ctx.strokeStyle = palette.halo;
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    ctx.lineWidth = 36;
    for (const points of groups.values()) {
      if (points.length < 2) continue;
      const hull = convexHull(points);
      ctx.beginPath();
      hull.forEach(([x, y], index) => (index ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
    }
    ctx.restore();
  }

  fg.nodeId("id")
    .linkSource("source")
    .linkTarget("target")
    .nodeLabel(() => "")
    .linkLabel(() => "")
    .nodeRelSize(1)
    .nodeVal((node) => radius(node) ** 2)
    .nodeCanvasObject(paintNode)
    .nodePointerAreaPaint((node, color, ctx) => {
      if (!Number.isFinite(node.x) || !Number.isFinite(node.y)) return;
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(node.x, node.y, radius(node) + 2, 0, 2 * Math.PI);
      ctx.fill();
    })
    .linkCanvasObjectMode(() => "replace")
    .linkCanvasObject(paintLink)
    .onRenderFramePre(framePre)
    .onRenderFramePost(framePost)
    .enablePointerInteraction(true)
    .enableNodeDrag(true)
    .d3AlphaDecay(0.035)
    .cooldownTicks(200)
    .onNodeHover((node) => {
      hoverId = node ? node.id : null;
      host.style.cursor = node ? "pointer" : "";
      opts.onHover?.(node ?? null);
    })
    .onNodeClick((node) => {
      const now = performance.now();
      if (lastClick.id === node.id && now - lastClick.at < 320) {
        lastClick = { id: null, at: 0 };
        opts.onDblClick?.(node);
        return;
      }
      lastClick = { id: node.id, at: now };
      opts.onClick?.(node);
    })
    .onBackgroundClick(() => opts.onBackgroundClick?.())
    .onEngineStop(() => {
      if (settledOnce || destroyed) return;
      settledOnce = true;
      opts.onSettled?.({ settleMs: Math.round(performance.now() - settleStarted), nodes: data.nodes.length, links: data.links.length });
    });
  fg.d3Force("center", null);

  const resizeObserver = typeof ResizeObserver === "function" ? new ResizeObserver(() => size()) : null;
  resizeObserver?.observe(host);

  function redraw() {
    if (destroyed) return;
    fg.autoPauseRedraw(false);
    clearTimeout(redrawTimer);
    redrawTimer = setTimeout(() => {
      if (!destroyed) fg.autoPauseRedraw(true);
    }, 120);
  }

  const onTheme = () => {
    palette = readGraphPalette();
    redraw();
  };
  document.addEventListener("kz:theme", onTheme);
  document.addEventListener("kz:language", redraw);

  const view = {
    /**
     * 换数据。nodes/links 由调用方克隆(节点对象上的 x/y 在两次 setData 之间保留 = 位置保持)。
     * options: { layout:"clusters"|"ego"|"dag-lr", anchors:Map, unassigned:{x,y}, hopOf:Map, center:id }
     */
    setData(next, options = {}) {
      layout = options.layout ?? "clusters";
      anchors = options.anchors ?? new Map();
      unassigned = options.unassigned ?? { x: 0, y: 0 };
      hopOf = options.hopOf ?? new Map();
      data = { nodes: next.nodes ?? [], links: next.links ?? [] };
      neighbors = neighborsOf(data.links);
      for (const node of data.nodes) {
        node.fx = undefined;
        node.fy = undefined;
        if (layout === "clusters" && node.kind === "crate") {
          const anchor = anchors.get(node.id);
          if (anchor && Number.isFinite(anchor.x) && Number.isFinite(anchor.y)) {
            node.fx = anchor.x;
            node.fy = anchor.y;
          }
        }
        if (layout === "ego" && node.id === options.center) {
          node.fx = 0;
          node.fy = 0;
        }
        if (!Number.isFinite(node.x) || !Number.isFinite(node.y)) {
          const group = opts.clusterOf?.(node);
          const anchor = (layout === "clusters" && group && anchors.get(group)) || (layout === "clusters" ? unassigned : { x: 0, y: 0 });
          node.x = finite(anchor.x) + (Math.random() - 0.5) * 80;
          node.y = finite(anchor.y) + (Math.random() - 0.5) * 80;
        }
      }
      fg.dagMode(layout === "dag-lr" ? "lr" : null);
      fg.d3Force("cluster", layout === "ego" ? radialForce(hopOf, 130) : layout === "clusters" ? clusterForce((n) => opts.clusterOf?.(n), anchors, unassigned, (n) => (n.kind === "module" ? 0.06 : 0.035)) : null);
      fg.warmupTicks(Math.min(120, Math.floor(60000 / Math.max(1, data.nodes.length))));
      settleStarted = performance.now();
      settledOnce = false;
      fg.graphData(data);
      // 模块像花瓣一样围着 crate 散开,记忆再围着模块:模块斥力大、记忆斥力小。
      const CHARGE = { crate: -300, module: -140, memory: -28 };
      fg.d3Force("charge")?.strength((node) => CHARGE[node.kind] ?? -24);
      fg.d3Force("link")
        ?.distance((link) => {
          if (link.rel === "contains") return 70;
          if (link.rel === "about") return endOf(link.target)?.kind === "module" ? 34 : link.strength === "weak" ? 60 : 48;
          return 55;
        })
        .strength((link) => {
          if (link.rel === "mentions" || link.rel === "cites") return 0.08;
          if (link.rel === "contains") return 0.7;
          if (link.rel === "about" && link.strength === "weak") return 0.3;
          return 0.45;
        });
      fg.d3ReheatSimulation();
    },
    highlight(ids) {
      hits = ids && ids.length ? new Set(ids) : null;
      redraw();
    },
    select(id) {
      selectedId = id ?? null;
      redraw();
    },
    hover(id) {
      hoverId = id ?? null;
      redraw();
    },
    focus(id, zoom = 2, ms = 500) {
      const node = data.nodes.find((n) => n.id === id);
      if (!node || !Number.isFinite(node.x)) return;
      fg.centerAt(node.x, node.y, ms);
      fg.zoom(zoom, ms);
    },
    fit(ms = 400) {
      const filter = layout === "clusters" ? (node) => node.kind === "memory" || node.kind === "crate" || node.kind === "module" : undefined;
      fg.zoomToFit(ms, 36, filter);
    },
    reheat() {
      fg.d3ReheatSimulation();
    },
    redraw,
    pause() {
      fg.pauseAnimation();
    },
    resume() {
      fg.resumeAnimation();
      size();
    },
    resize: size,
    zoom() {
      return fg.zoom();
    },
    /** 画布上的节点位置(冒烟与调试用)。 */
    positions() {
      return data.nodes.map((n) => ({ id: n.id, x: n.x, y: n.y }));
    },
    destroy() {
      destroyed = true;
      clearTimeout(redrawTimer);
      document.removeEventListener("kz:theme", onTheme);
      document.removeEventListener("kz:language", redraw);
      resizeObserver?.disconnect();
      try {
        fg.pauseAnimation();
        fg._destructor?.();
      } catch {
        /* 库内部销毁失败不影响页面:画布随 host 清空一并回收 */
      }
      host.replaceChildren();
      data = { nodes: [], links: [] };
    },
  };
  return view;
}
