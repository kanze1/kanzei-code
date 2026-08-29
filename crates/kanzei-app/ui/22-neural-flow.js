import { defer } from "./01-core.js";
import { $ } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeSessionId } from "./03-shell.js";

// R-285 金色神经流。
const MASCOT_SRC = "./assets/mascot-a.png";
const MASCOT_ASPECT = [128, 192];
// 刺青对各类事件的抢边权重。记忆类主导叙事,工具/运行类也必须亮(每次调用工具
// 都要能看见她身上过电),但压低一档,免得刺青长亮到失去区分度。
const MASCOT_BIAS = {
  recall: 1.25, crystal: 1.25, write: 1.15,
  tool: 0.9, complete: 0.85, run: 0.85, error: 0.7,
  stream: 0.5, ambient: 0.45,
};
// 刺青纹样。主脉络 veins 是三次贝塞尔,承载 pulse;ticks/pins/blocks/shards 是装饰,
// 不参与脉冲 —— 否则光点会在装饰碎线上乱跳。坐标是贴图包围盒内的归一化值。
const MASCOT_INK = {
  ring: { x: 0.641, y: 0.452, r: 0.0135 },
  // 端点:port 缝进节点场,ring 作为 hub 承载收敛光环。
  nodes: {
    neck:     { x: 0.672, y: 0.330, weight: 0.85 },
    ring:     { x: 0.641, y: 0.452, weight: 1.00, hub: true },
    shL:      { x: 0.514, y: 0.521, weight: 0.70, port: true },
    shR:      { x: 0.786, y: 0.504, weight: 0.70, port: true },
    dissolve: { x: 0.646, y: 0.618, weight: 0.85, port: true },
  },
  veins: [
    { id: "spine", from: "neck", to: "ring", w: [1.5, 1.05],
      p: [[0.672, 0.330], [0.652, 0.368], [0.658, 0.408], [0.643, 0.443]] },
    { id: "archL", from: "ring", to: "shL", w: [1.25, 0.75],
      p: [[0.626, 0.462], [0.594, 0.492], [0.556, 0.503], [0.514, 0.521]] },
    { id: "archR", from: "ring", to: "shR", w: [1.25, 0.75],
      p: [[0.657, 0.459], [0.702, 0.477], [0.741, 0.492], [0.786, 0.504]] },
    { id: "desc", from: "ring", to: "dissolve", w: [1.35, 0.65],
      p: [[0.643, 0.469], [0.655, 0.516], [0.634, 0.560], [0.646, 0.618]] },
  ],
  // [脉络, [[脉络上的 t, 外挑长度, 朝向], ...]] —— 左右刻意不对称。
  ticks: [
    ["spine", [[0.22, 0.020, 1], [0.42, 0.014, -1], [0.58, 0.024, 1], [0.78, 0.012, -1]]],
    ["archL", [[0.30, 0.016, 1], [0.62, 0.022, -1], [0.82, 0.012, 1]]],
    ["archR", [[0.24, 0.020, -1], [0.52, 0.014, 1], [0.74, 0.024, -1], [0.90, 0.011, 1]]],
    ["desc", [[0.30, 0.018, -1], [0.66, 0.013, 1]]],
  ],
  pins: [
    ["archL", [[0.030, 0.014], [0.052, 0.030], [-0.018, 0.014]]],
    ["archR", [[0.034, 0.010], [0.020, 0.030], [0.014, -0.016]]],
  ],
  blocks: [[0.600, 0.545, 4, 0.020, 0.009], [0.688, 0.532, 3, 0.016, 0.008], [0.628, 0.596, 3, 0.014, 0.007]],
  // 脸颊碎纹:呼应贴图里本来就有的刺青,极短,绝不延伸上脸。
  shards: [
    [[0.682, 0.258], [0.687, 0.255], [0.692, 0.252], [0.700, 0.248]],
    [[0.685, 0.267], [0.690, 0.270], [0.695, 0.272], [0.703, 0.275]],
    [[0.690, 0.257], [0.692, 0.252], [0.693, 0.249], [0.695, 0.243]],
  ],
};

function cubicAt(p, t) {
  const u = 1 - t;
  return {
    x: u * u * u * p[0][0] + 3 * u * u * t * p[1][0] + 3 * u * t * t * p[2][0] + t * t * t * p[3][0],
    y: u * u * u * p[0][1] + 3 * u * u * t * p[1][1] + 3 * u * t * t * p[2][1] + t * t * t * p[3][1],
  };
}

// 采样 + 累积弧长:pulse 必须按弧长走,按 t 走会在曲率大的地方忽快忽慢。
function buildVein(vein, box) {
  const steps = 48;
  const pts = [];
  for (let i = 0; i <= steps; i += 1) {
    const q = cubicAt(vein.p, i / steps);
    pts.push({ x: box.x + q.x * box.w, y: box.y + q.y * box.h });
  }
  const lens = [0];
  for (let i = 1; i < pts.length; i += 1) {
    lens.push(lens[i - 1] + Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y));
  }
  return { pts, lens, total: lens[lens.length - 1] || 1, w: vein.w };
}

function inkPointAt(ink, t) {
  const target = Math.max(0, Math.min(1, t)) * ink.total;
  const { lens, pts } = ink;
  let lo = 1;
  let hi = lens.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (lens[mid] < target) lo = mid + 1; else hi = mid;
  }
  const span = lens[lo] - lens[lo - 1] || 1;
  const f = (target - lens[lo - 1]) / span;
  const a = pts[lo - 1];
  const b = pts[lo];
  return { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f };
}

// 装饰几何一次算好,每帧只遍历绘制。
function buildDecor(box, veins) {
  const at = (u, v) => ({ x: box.x + u * box.w, y: box.y + v * box.h });
  const lines = [];
  const squares = [];
  for (const [id, ticks] of MASCOT_INK.ticks) {
    const ink = veins.get(id);
    if (!ink) continue;
    for (const [t, len, side] of ticks) {
      const a = inkPointAt(ink, Math.max(0, t - 0.01));
      const b = inkPointAt(ink, Math.min(1, t + 0.01));
      const dx = b.x - a.x;
      const dy = b.y - a.y;
      const norm = Math.max(1e-6, Math.hypot(dx, dy));
      const ex = a.x + (-dy / norm) * side * len * box.h;
      const ey = a.y + (dx / norm) * side * len * box.h;
      lines.push({ x1: a.x, y1: a.y, x2: ex, y2: ey, w: 0.75, alpha: 0.9 });
      squares.push({ x: ex, y: ey, r: 1.05 });
    }
  }
  for (const [id, pins] of MASCOT_INK.pins) {
    const ink = veins.get(id);
    if (!ink) continue;
    const e = ink.pts[ink.pts.length - 1];
    for (const [dx, dy] of pins) {
      lines.push({ x1: e.x, y1: e.y, x2: e.x + dx * box.w, y2: e.y + dy * box.h, w: 0.7, alpha: 0.8, taper: true });
    }
  }
  for (const [cx, cy, count, gap, len] of MASCOT_INK.blocks) {
    for (let j = 0; j < count; j += 1) {
      const a = at(cx, cy + j * gap);
      lines.push({ x1: a.x, y1: a.y, x2: a.x + len * (1 + 0.35 * (j % 2)) * box.w, y2: a.y, w: 0.7, alpha: 0.85 });
    }
  }
  const shards = MASCOT_INK.shards.map((p) => {
    const out = [];
    for (let i = 0; i <= 12; i += 1) {
      const q = cubicAt(p, i / 12);
      out.push({ x: box.x + q.x * box.w, y: box.y + q.y * box.h });
    }
    return out;
  });
  const ring = at(MASCOT_INK.ring.x, MASCOT_INK.ring.y);
  return { lines, squares, shards, ring: { ...ring, r: MASCOT_INK.ring.r * box.h } };
}
// 顶层声明作为所有 classic script 的统一事件入口；初始化前保持可安全调用。
export let neuralFlowEmit = null;
export function setNeuralFlowEmit(value) { neuralFlowEmit = value; }
// 视觉层只消费真实运行事件；Canvas 丢帧、隐藏或关闭不会反向改变任何业务状态。
defer(() => {
  const chatCanvas = $("neural-flow-chat");
  const memoryCanvas = $("neural-flow-memory");
  const stateLabel = $("memory-flow-state");
  const reducedMotion = typeof window.matchMedia === "function"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const fields = [];
  let frameHandle = 0;
  let stateTimer = null;
  let lastStreamPulseAt = 0;

  function flowNow() {
    return typeof performance !== "undefined" && typeof performance.now === "function" ? performance.now() : Date.now();
  }

  function seededRandom(seed) {
    let value = seed >>> 0;
    return () => {
      value += 0x6d2b79f5;
      let mixed = value;
      mixed = Math.imul(mixed ^ (mixed >>> 15), mixed | 1);
      mixed ^= mixed + Math.imul(mixed ^ (mixed >>> 7), mixed | 61);
      return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296;
    };
  }

  function themeColor(name, fallback) {
    if (typeof getComputedStyle !== "function") return fallback;
    const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return value || fallback;
  }

  function quadraticPoint(a, b, bend, progress) {
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const length = Math.max(1, Math.hypot(dx, dy));
    const mx = (a.x + b.x) / 2 - (dy / length) * bend;
    const my = (a.y + b.y) / 2 + (dx / length) * bend;
    const inv = 1 - progress;
    return {
      x: inv * inv * a.x + 2 * inv * progress * mx + progress * progress * b.x,
      y: inv * inv * a.y + 2 * inv * progress * my + progress * progress * b.y,
      mx,
      my,
    };
  }

  class NeuralField {
    constructor(canvas, variant, seed) {
      this.canvas = canvas;
      this.variant = variant;
      this.random = seededRandom(seed);
      this.context = typeof canvas?.getContext === "function" ? canvas.getContext("2d") : null;
      this.width = 0;
      this.height = 0;
      this.nodes = [];
      this.edges = [];
      this.pulses = [];
      this.bursts = [];
      this.energy = variant === "memory" ? 0.22 : 0.12;
      this.lastFrameAt = 0;
      // 贴图未就绪时刺青节点不注入,否则空气里悬着一串回路。
      this.mascotImage = null;
      this.mascotEdgeFrom = Number.POSITIVE_INFINITY;
      this.mascotHubIndex = -1;
      this.mascotDecor = null;
      this.mascotScale = 1;
      if (!this.context) return;
      this.resize();
      if (typeof ResizeObserver === "function") {
        this.resizeObserver = new ResizeObserver(() => {
          this.resize();
          if (reducedMotion) this.draw(flowNow());
        });
        this.resizeObserver.observe(canvas);
      }
    }

    resize() {
      const rect = this.canvas.getBoundingClientRect();
      const width = Math.max(1, rect.width || this.canvas.clientWidth || 1);
      const height = Math.max(1, rect.height || this.canvas.clientHeight || 1);
      const pixelRatio = Math.min(1.75, Math.max(1, window.devicePixelRatio || 1));
      const pixelWidth = Math.round(width * pixelRatio);
      const pixelHeight = Math.round(height * pixelRatio);
      if (this.canvas.width !== pixelWidth || this.canvas.height !== pixelHeight) {
        this.canvas.width = pixelWidth;
        this.canvas.height = pixelHeight;
      }
      this.context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
      const changed = Math.abs(width - this.width) > 1 || Math.abs(height - this.height) > 1;
      this.width = width;
      this.height = height;
      if (changed || !this.nodes.length) this.buildTopology();
    }

    buildTopology() {
      const count = this.variant === "memory" ? 46 : 38;
      this.nodes = [];
      for (let index = 0; index < count; index += 1) {
        const focus = this.variant === "chat" && index < 29;
        const x = this.variant === "memory"
          ? 0.05 + this.random() * 0.9
          : focus ? 0.48 + this.random() * 0.56 : 0.12 + this.random() * 0.46;
        const y = this.variant === "memory"
          ? 0.16 + this.random() * 0.7
          : 0.06 + this.random() * 0.88;
        this.nodes.push({
          x: x * this.width,
          y: y * this.height,
          radius: 0.7 + this.random() * 1.45,
          phase: this.random() * Math.PI * 2,
          weight: 0.35 + this.random() * 0.65,
        });
      }
      this.edges = [];
      const seen = new Set();
      this.nodes.forEach((node, index) => {
        const nearest = this.nodes
          .map((other, otherIndex) => ({
            otherIndex,
            distance: otherIndex === index ? Number.POSITIVE_INFINITY : Math.hypot(other.x - node.x, other.y - node.y),
          }))
          .sort((a, b) => a.distance - b.distance)
          .slice(0, index % 5 === 0 ? 3 : 2);
        nearest.forEach(({ otherIndex, distance }) => {
          const key = index < otherIndex ? `${index}:${otherIndex}` : `${otherIndex}:${index}`;
          if (seen.has(key)) return;
          seen.add(key);
          this.edges.push({
            from: index,
            to: otherIndex,
            bend: (this.random() - 0.5) * Math.min(34, distance * 0.24),
            weight: 0.25 + this.random() * 0.75,
            phase: this.random(),
            speed: 0.000035 + this.random() * 0.000035,
            signal: this.random() > (this.variant === "memory" ? 0.82 : 0.9),
          });
        });
      });
      this.injectMascot();
    }

    // 整数倍放大 + 右侧出血;非整数倍会让像素网格宽窄不一。
    mascotBox() {
      const [pw, ph] = MASCOT_ASPECT;
      // 只按高度算会让窄窗口下角色过大出血,宽度同样要设上限。
      const scale = Math.max(1, Math.round(Math.min((this.height * 0.94) / ph, (this.width * 0.46) / pw)));
      const h = Math.min(ph * scale, this.height * 0.98);
      const w = h * (pw / ph);
      // 贴图左侧约 29% 是透明留白,按贴图宽算出血会让人物实际缩在里面,这里按 0.94 补偿。
      return { x: this.width - w * 0.94, y: this.height * 0.02, w, h };
    }

    injectMascot() {
      if (this.variant !== "chat" || !this.mascotImage) return;
      const box = this.mascotBox();
      const index = new Map();
      for (const [id, spec] of Object.entries(MASCOT_INK.nodes)) {
        index.set(id, this.nodes.length);
        if (spec.hub) this.mascotHubIndex = this.nodes.length;
        this.nodes.push({
          x: box.x + spec.x * box.w,
          y: box.y + spec.y * box.h,
          radius: spec.hub ? 2 : 1.05,
          phase: this.random() * Math.PI * 2,
          weight: spec.weight,
          mascot: true,
        });
      }
      const veins = new Map();
      this.mascotEdgeFrom = this.edges.length;
      for (const vein of MASCOT_INK.veins) {
        const ink = buildVein(vein, box);
        veins.set(vein.id, ink);
        this.edges.push({
          from: index.get(vein.from), to: index.get(vein.to), bend: 0,
          weight: 0.92, phase: this.random(), speed: 0.00004 + this.random() * 0.00002,
          signal: true, mascot: true, ink,
        });
      }
      this.mascotDecor = buildDecor(box, veins);
      this.mascotScale = box.h / 512;
      // port 缝进原有节点场:信号从她身上溢出到整片场,而不是停在轮廓里。
      for (const [id, spec] of Object.entries(MASCOT_INK.nodes)) {
        if (!spec.port) continue;
        const portIndex = index.get(id);
        const port = this.nodes[portIndex];
        this.nodes
          .map((node, i) => ({ i, distance: node.mascot ? Number.POSITIVE_INFINITY : Math.hypot(node.x - port.x, node.y - port.y) }))
          .sort((a, b) => a.distance - b.distance)
          .slice(0, 2)
          .forEach(({ i, distance }) => {
            if (!Number.isFinite(distance)) return;
            this.edges.push({
              from: portIndex, to: i,
              bend: (this.random() - 0.5) * Math.min(26, distance * 0.2),
              weight: 0.58, phase: this.random(), speed: 0.00004, signal: true, mascot: true,
            });
          });
      }
    }

    // 刺青层:主脉络带宽度渐变,装饰一次画完。静息流光沿弧长跑。
    drawInk(ctx, base, hot, energy, breathing, now) {
      if (!this.mascotDecor) return;
      const s = this.mascotScale || 1;
      const live = 0.26 * (0.72 + energy * 0.5) * breathing;
      ctx.save();
      ctx.lineCap = "round";
      for (const edge of this.edges) {
        if (!edge.ink) continue;
        const { pts, w } = edge.ink;
        for (let i = 0; i < pts.length - 1; i += 1) {
          const f = i / (pts.length - 2);
          ctx.beginPath();
          ctx.moveTo(pts[i].x, pts[i].y);
          ctx.lineTo(pts[i + 1].x, pts[i + 1].y);
          ctx.strokeStyle = base;
          ctx.globalAlpha = live;
          ctx.lineWidth = (w[0] + (w[1] - w[0]) * f) * s;
          ctx.stroke();
        }
        const progress = reducedMotion ? edge.phase : (edge.phase + now * edge.speed) % 1;
        const fade = Math.sin(progress * Math.PI);
        const head = inkPointAt(edge.ink, progress);
        const tail = inkPointAt(edge.ink, Math.max(0, progress - 0.05));
        ctx.beginPath();
        ctx.moveTo(tail.x, tail.y);
        ctx.lineTo(head.x, head.y);
        ctx.strokeStyle = hot;
        ctx.globalAlpha = 0.34 * (0.6 + fade * 0.4);
        ctx.lineWidth = 1.2 * s;
        ctx.shadowColor = hot;
        ctx.shadowBlur = 6;
        ctx.stroke();
        ctx.shadowBlur = 0;
      }
      const decor = this.mascotDecor;
      for (const line of decor.lines) {
        ctx.beginPath();
        ctx.moveTo(line.x1, line.y1);
        ctx.lineTo(line.x2, line.y2);
        ctx.strokeStyle = base;
        ctx.globalAlpha = live * line.alpha;
        ctx.lineWidth = line.w * s;
        ctx.stroke();
      }
      ctx.fillStyle = base;
      for (const sq of decor.squares) {
        ctx.globalAlpha = live * 0.95;
        ctx.fillRect(sq.x - sq.r * s, sq.y - sq.r * s, sq.r * 2 * s, sq.r * 2 * s);
      }
      for (const shard of decor.shards) {
        ctx.beginPath();
        ctx.moveTo(shard[0].x, shard[0].y);
        for (let i = 1; i < shard.length; i += 1) ctx.lineTo(shard[i].x, shard[i].y);
        ctx.strokeStyle = hot;
        ctx.globalAlpha = live * 0.62;
        ctx.lineWidth = 0.8 * s;
        ctx.stroke();
      }
      // 环留缺口:是接口,不是准星。
      ctx.beginPath();
      ctx.arc(decor.ring.x, decor.ring.y, decor.ring.r, 0.61, 5.67);
      ctx.strokeStyle = hot;
      ctx.globalAlpha = live * 1.1;
      ctx.lineWidth = 1.3 * s;
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(decor.ring.x, decor.ring.y, 1.4 * s, 0, Math.PI * 2);
      ctx.fillStyle = hot;
      ctx.fill();
      ctx.restore();
    }

    // 求边上某点:刺青按弧长,普通边仍走二次贝塞尔。
    edgePoint(edge, reverse, progress) {
      if (edge.ink) return inkPointAt(edge.ink, reverse ? 1 - progress : progress);
      const from = this.nodes[reverse ? edge.to : edge.from];
      const to = this.nodes[reverse ? edge.from : edge.to];
      return quadraticPoint(from, to, reverse ? -edge.bend : edge.bend, progress);
    }

    activeInView() {
      const view = this.canvas.closest?.(".view");
      return !view || view.classList.contains("active");
    }

    trigger(kind, intensity = 0.7) {
      if (!this.context) return;
      const now = flowNow();
      const strength = Math.max(0.15, Math.min(1, intensity));
      this.energy = Math.max(this.energy, strength);
      const count = kind === "stream" ? 1 : kind === "crystal" ? 8 : kind === "error" ? 3 : 5;
      for (let index = 0; index < count; index += 1) {
        const edgeIndex = this.pickEdge(kind, index);
        this.pulses.push({
          edgeIndex,
          reverse: kind === "recall" || (kind === "crystal" && index % 2 === 0),
          startedAt: now + index * (kind === "crystal" ? 54 : 68),
          duration: 520 + this.random() * 620,
          strength,
          kind,
        });
      }
      if (["crystal", "complete", "error"].includes(kind)) {
        const nodeIndex = this.pickAnchor(kind);
        this.bursts.push({ nodeIndex, startedAt: now + (kind === "crystal" ? 260 : 80), kind, strength });
      }
      if (reducedMotion) this.draw(now);
    }

    pickEdge(kind, offset) {
      if (!this.edges.length) return 0;
      const ranked = this.edges.map((edge, index) => {
        const from = this.nodes[edge.from];
        const to = this.nodes[edge.to];
        const centerX = (from.x + to.x) / 2 / Math.max(1, this.width);
        const centerY = (from.y + to.y) / 2 / Math.max(1, this.height);
        let score = this.random() * 0.35;
        if (kind === "recall") score += centerX * 0.8 + (1 - centerY) * 0.2;
        else if (kind === "write" || kind === "crystal") score += centerX * 0.55 + centerY * 0.25;
        else if (kind === "run") score += (1 - centerX) * 0.45 + centerY * 0.2;
        else score += edge.weight * 0.45;
        if (edge.mascot) score += MASCOT_BIAS[kind] ?? 0.75;
        return { index, score };
      }).sort((a, b) => b.score - a.score);
      return ranked[offset % Math.min(ranked.length, 12)].index;
    }

    pickAnchor(kind) {
      // 收敛类事件的光环落在锁骨挂饰上,让"记忆固化"有个明确的落点。
      if (this.mascotHubIndex >= 0 && (kind === "crystal" || kind === "complete")) return this.mascotHubIndex;
      const ranked = this.nodes.map((node, index) => {
        const nx = node.x / Math.max(1, this.width);
        const ny = node.y / Math.max(1, this.height);
        const score = kind === "error"
          ? Math.abs(nx - 0.68) + Math.abs(ny - 0.5)
          : Math.abs(nx - 0.78) + Math.abs(ny - 0.42);
        return { index, score };
      }).sort((a, b) => a.score - b.score);
      return ranked[0]?.index ?? 0;
    }

    draw(now) {
      if (!this.context || !this.width || !this.height) return;
      const ctx = this.context;
      ctx.clearRect(0, 0, this.width, this.height);
      const base = themeColor("--memory-flow", "#c9962e");
      const hot = themeColor("--memory-flow-hot", "#f0d58c");
      const error = themeColor("--err", "#d0684e");
      const isMemory = this.variant === "memory";
      const idleAlpha = isMemory ? 0.22 : 0.075;
      const breathing = reducedMotion ? 0.72 : 0.74 + Math.sin(now / 1500) * 0.14;
      const energy = Math.max(this.variant === "memory" ? 0.18 : 0.08, this.energy);

      if (this.mascotImage) {
        const box = this.mascotBox();
        const light = document.documentElement.getAttribute("data-theme") === "light";
        ctx.save();
        // 像素画放大必须关平滑;亮色档金色贴图压在浅底上会跳,改走 multiply 当暗印记。
        ctx.imageSmoothingEnabled = false;
        ctx.globalCompositeOperation = light ? "multiply" : "source-over";
        ctx.globalAlpha = (light ? 0.25 : 0.22) * (0.86 + energy * 0.34) * breathing;
        ctx.drawImage(this.mascotImage, box.x, box.y, box.w, box.h);
        ctx.restore();
      }

      this.drawInk(ctx, base, hot, energy, breathing, now);

      ctx.lineCap = "round";
      for (const edge of this.edges) {
        if (edge.ink) continue;   // 刺青主脉络已在 drawInk 画过,别重复描一遍
        const from = this.nodes[edge.from];
        const to = this.nodes[edge.to];
        const point = quadraticPoint(from, to, edge.bend, 0.5);
        ctx.save();
        ctx.beginPath();
        ctx.moveTo(from.x, from.y);
        ctx.quadraticCurveTo(point.mx, point.my, to.x, to.y);
        ctx.strokeStyle = edge.weight > 0.78 ? hot : base;
        ctx.globalAlpha = (edge.mascot ? 0.26 : idleAlpha) * (0.55 + edge.weight * 0.45) * (0.75 + energy * 0.5);
        ctx.lineWidth = (isMemory ? 0.72 : 0.48) + edge.weight * (isMemory ? 0.52 : 0.42);
        if (edge.weight > 0.72) {
          ctx.shadowColor = base;
          ctx.shadowBlur = isMemory ? 5 : 2.5;
        }
        ctx.stroke();
        ctx.restore();

        // 静息时仍有少量定向流光，让“神经流”不依赖业务事件也能一眼读出流向。
        if (edge.signal) {
          const ambientProgress = reducedMotion ? edge.phase : (edge.phase + now * edge.speed) % 1;
          const ambientFade = Math.sin(ambientProgress * Math.PI);
          const tailProgress = Math.max(0, ambientProgress - 0.045);
          const tail = quadraticPoint(from, to, edge.bend, tailProgress);
          const head = quadraticPoint(from, to, edge.bend, ambientProgress);
          ctx.save();
          ctx.beginPath();
          ctx.moveTo(tail.x, tail.y);
          ctx.lineTo(head.x, head.y);
          ctx.strokeStyle = hot;
          ctx.lineWidth = isMemory ? 1.6 : 0.9;
          ctx.globalAlpha = (isMemory ? 0.5 : 0.18) * (0.68 + ambientFade * 0.32);
          ctx.shadowColor = hot;
          ctx.shadowBlur = isMemory ? 10 : 5;
          ctx.stroke();
          ctx.beginPath();
          ctx.arc(head.x, head.y, (isMemory ? 1.05 : 0.7) + edge.weight * 0.5, 0, Math.PI * 2);
          ctx.fillStyle = hot;
          ctx.globalAlpha = (isMemory ? 0.72 : 0.3) * (0.72 + ambientFade * 0.28);
          ctx.fill();
          ctx.restore();
        }
      }

      for (const node of this.nodes) {
        const pulse = reducedMotion ? 1 : 0.82 + Math.sin(now / 1250 + node.phase) * 0.18;
        ctx.save();
        ctx.beginPath();
        ctx.arc(node.x, node.y, node.radius * pulse, 0, Math.PI * 2);
        ctx.fillStyle = node.weight > 0.76 ? hot : base;
        ctx.globalAlpha = (idleAlpha * 1.55 + energy * 0.055) * breathing;
        if (node.weight > 0.72) {
          ctx.shadowColor = node.weight > 0.76 ? hot : base;
          ctx.shadowBlur = isMemory ? 8 : 4;
        }
        ctx.fill();
        ctx.restore();
      }

      const nextPulses = [];
      for (const pulse of this.pulses) {
        const progress = (now - pulse.startedAt) / pulse.duration;
        if (progress < 0) { nextPulses.push(pulse); continue; }
        if (progress >= 1) continue;
        const edge = this.edges[pulse.edgeIndex];
        if (!edge) continue;
        const point = this.edgePoint(edge, pulse.reverse, progress);
        const color = pulse.kind === "error" ? error : pulse.kind === "complete" || pulse.kind === "crystal" ? hot : base;
        const fade = Math.sin(progress * Math.PI);
        ctx.save();
        ctx.beginPath();
        if (edge.ink) {
          const trace = edge.ink.pts;
          ctx.moveTo(trace[0].x, trace[0].y);
          for (let i = 1; i < trace.length; i += 1) ctx.lineTo(trace[i].x, trace[i].y);
        } else {
          const from = this.nodes[pulse.reverse ? edge.to : edge.from];
          const to = this.nodes[pulse.reverse ? edge.from : edge.to];
          ctx.moveTo(from.x, from.y);
          const edgeMidpoint = quadraticPoint(from, to, pulse.reverse ? -edge.bend : edge.bend, 0.5);
          ctx.quadraticCurveTo(edgeMidpoint.mx, edgeMidpoint.my, to.x, to.y);
        }
        ctx.strokeStyle = color;
        ctx.lineWidth = 0.8 + pulse.strength * 0.7;
        ctx.globalAlpha = (0.1 + pulse.strength * 0.12) * fade;
        ctx.shadowColor = color;
        ctx.shadowBlur = 5 + pulse.strength * 6;
        ctx.stroke();

        const trailSteps = 5;
        for (let step = trailSteps; step >= 1; step -= 1) {
          const trailProgress = Math.max(0, progress - step * 0.026);
          const trailPoint = this.edgePoint(edge, pulse.reverse, trailProgress);
          ctx.beginPath();
          ctx.arc(trailPoint.x, trailPoint.y, 0.55 + pulse.strength * 0.6, 0, Math.PI * 2);
          ctx.fillStyle = color;
          ctx.globalAlpha = fade * (1 - step / (trailSteps + 1)) * 0.52;
          ctx.fill();
        }
        ctx.beginPath();
        ctx.arc(point.x, point.y, 1.45 + pulse.strength * 2.35, 0, Math.PI * 2);
        ctx.fillStyle = color;
        ctx.globalAlpha = 0.68 + fade * 0.32;
        ctx.shadowColor = color;
        ctx.shadowBlur = 12 + pulse.strength * 18;
        ctx.fill();
        ctx.restore();
        nextPulses.push(pulse);
      }
      this.pulses = nextPulses;

      const nextBursts = [];
      for (const burst of this.bursts) {
        const progress = (now - burst.startedAt) / 1050;
        if (progress < 0) { nextBursts.push(burst); continue; }
        if (progress >= 1) continue;
        const node = this.nodes[burst.nodeIndex];
        const color = burst.kind === "error" ? error : hot;
        ctx.save();
        ctx.beginPath();
        ctx.arc(node.x, node.y, 5 + progress * 25 * burst.strength, 0, Math.PI * 2);
        ctx.strokeStyle = color;
        ctx.lineWidth = 1.45;
        ctx.globalAlpha = (1 - progress) * 0.86;
        ctx.shadowColor = color;
        ctx.shadowBlur = 18;
        ctx.stroke();
        ctx.restore();
        nextBursts.push(burst);
      }
      this.bursts = nextBursts;
      ctx.globalAlpha = 1;
      this.energy += ((this.variant === "memory" ? 0.18 : 0.08) - this.energy) * 0.026;
    }
  }

  function addField(canvas, variant, seed) {
    if (!canvas) return;
    const field = new NeuralField(canvas, variant, seed);
    if (field.context) fields.push(field);
  }

  addField(chatCanvas, "chat", 28501);
  addField(memoryCanvas, "memory", 28502);

  // 贴图取不到就保持纯节点场;代言人是增强,不是运行前提。
  if (typeof Image === "function") {
    const mascot = new Image();
    mascot.decoding = "async";
    mascot.addEventListener("load", () => {
      fields.forEach((field) => {
        if (field.variant !== "chat") return;
        field.mascotImage = mascot;
        field.buildTopology();
        if (reducedMotion) field.draw(flowNow());
      });
    });
    mascot.addEventListener("error", () => {});
    mascot.src = MASCOT_SRC;
  }

  function setMemoryState(label, settleAfterMs = 0) {
    if (!stateLabel) return;
    stateLabel.textContent = t(label);
    clearTimeout(stateTimer);
    if (settleAfterMs > 0) {
      stateTimer = setTimeout(() => { stateLabel.textContent = t("静息"); }, settleAfterMs);
    }
  }

  const specs = {
    run_started: ["run", "运行中", 0.82],
    reasoning_active: ["stream", "运行中", 0.42],
    assistant_streaming: ["stream", "运行中", 0.32],
    tool_started: ["tool", "运行中", 0.64],
    tool_progressed: ["tool", "运行中", 0.42],
    tool_completed: ["complete", "收敛", 0.62],
    run_completed: ["complete", "收敛", 0.86],
    run_failed: ["error", "受阻", 0.9],
    run_stopped: ["error", "受阻", 0.55],
    context_compacted: ["crystal", "收敛", 0.72],
    memory_snapshot: ["ambient", null, 0.24],
    memory_search_started: ["recall", "检索中", 0.8],
    memory_search_completed: ["complete", "收敛", 0.68],
    memory_consolidation_started: ["write", "整理中", 0.88],
    memory_consolidation_partial: ["write", "整理中", 0.58],
    memory_consolidation_completed: ["crystal", "收敛", 1],
    memory_consolidation_failed: ["error", "受阻", 0.9],
    memory_candidate_discarded: ["error", "回收中", 0.52],
    memory_cleanup_started: ["write", "整理中", 0.72],
    memory_cleanup_completed: ["complete", "收敛", 0.72],
    memory_cleanup_failed: ["error", "受阻", 0.82],
    memory_search_failed: ["error", "受阻", 0.82],
    memory_recall_retrieved: ["recall", "检索中", 0.9],
    memory_recall_injected: ["recall", "注入中", 1],
    memory_candidate_created: ["write", "生长中", 0.86],
    memory_candidate_promoted: ["crystal", "收敛", 1],
    research_source_retrieved: ["recall", "检索中", 0.82],
    research_section_verified: ["crystal", "收敛", 0.9],
    voice_listening: ["recall", "运行中", 0.68],
    voice_speaking: ["complete", "运行中", 0.68],
    voice_interrupted: ["error", "受阻", 0.62],
  };

  neuralFlowEmit = (eventType, detail = {}) => {
    if (detail.session_id && activeSessionId && detail.session_id !== activeSessionId) return;
    if (eventType === "assistant_streaming") {
      const now = flowNow();
      if (now - lastStreamPulseAt < 180) return;
      lastStreamPulseAt = now;
    }
    let spec = specs[eventType];
    if (eventType === "tool_started") {
      const name = String(detail.tool_name ?? "").toLowerCase();
      if (name === "memory_search") spec = ["recall", "检索中", 0.9];
      else if (["memory_note", "memory_add", "memory_update"].includes(name)) spec = ["write", "生长中", 0.86];
    }
    if (eventType === "tool_completed" && detail.ok === false) spec = ["error", "受阻", 0.82];
    if (!spec) return;
    const [kind, label, intensity] = spec;
    if (label) setMemoryState(label, ["收敛", "受阻", "回收中"].includes(label) ? 1900 : 0);
    fields.forEach((field) => field.trigger(kind, detail.intensity ?? intensity));
  };

  function renderFrame(now) {
    if (!document.hidden) {
      for (const field of fields) {
        if (!field.activeInView()) continue;
        const active = field.pulses.length || field.bursts.length || field.energy > 0.24;
        const interval = active ? 16 : 42;
        if (now - field.lastFrameAt >= interval) {
          field.lastFrameAt = now;
          field.draw(now);
        }
      }
    }
    frameHandle = requestAnimationFrame(renderFrame);
  }

  if (fields.length) {
    if (reducedMotion) fields.forEach((field) => field.draw(flowNow()));
    else frameHandle = requestAnimationFrame(renderFrame);
  }

  window.addEventListener?.("beforeunload", () => {
    if (frameHandle) cancelAnimationFrame(frameHandle);
    fields.forEach((field) => field.resizeObserver?.disconnect());
  });
});

// R-264 B9：未迁移的 classic 消费者仍通过兼容桥读取 live ESM 入口。
defer(() => {
  Object.assign(globalThis, { neuralFlowEmit });
});
