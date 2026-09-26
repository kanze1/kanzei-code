// 欢迎页 / 语音舞台装饰:默认品牌用 SVG,星座预设用 Canvas2D。消息对话不绘制。设计见 docs/design/ui_chat_backdrop.md。
//
// 纯表现层:只消费 22-neural-flow.js 转发的真实运行事件与 OC 状态 store 的运行真源(createOcStateStore),
// 不接管指针、不反向改变任何业务状态。颜色全部取 token(--backdrop-*、--accent、--err),两套主题自动跟随。
// 欢迎 / 语音文案保护:①构图时登记避让区,绘制时 evenodd 剪掉;
// ②只剩右上角水印可放、星座本身压在正文上时,整张星座先画进离屏层,再以 watermarkAlpha 单一不透明度合成
// (层内再怎么叠也 ≤ 1),正文压在任何像素上仍 ≥ 4.5:1(ui-constellation-smoke ⑬ 复算混色,
// ui-constellation-browser-smoke 逐像素实测)。
// 调度:空闲 ≤ 8 帧/秒(setTimeout → rAF,不再每个 vsync 唤醒)、有光点 / 波 / 补间时 ≤ 30 帧/秒;已排好的帧
// 不被新事件取消(shouldHurry);窗口隐藏、不在对话视图、背景关闭时零定时器零 rAF;减少动态效果时只画静帧。
// 全程零 shadowBlur。
import { createOcStateStore } from "./22-oc-companion.js";
import {
  activityBusy, bfsOrder, cometRoute, edgeDepthsFrom, frameDelay, hubTone, imageToConstellation, layerAlpha,
  layoutBackdrop, normalizeBackdropPrefs, poissonSelect, resolveBackdropModel, sanitizeModel, seededRandom, shouldHurry,
  watchDelay, watermarkAlpha, watermarkCap,
  GAIN, IMAGE_MAX_SIDE, IMAGE_STAR_COUNT, WAKE_THROTTLE_MS, WATERMARK_LINE_BOOST,
} from "./22-constellation-core.js";
import { KANZEI_LOGO_STROKES, STAR_PRESETS } from "./22-constellation-data.js";
import { createBrandBackdrop } from "./22-brand-backdrop.js";

const DATA = { KANZEI_LOGO_STROKES, STAR_PRESETS };
const STILL_T = 2400; // 静帧(减少动态 / 预览截图)固定的时刻,保证可复现
const DRAW_IN_MS = 1600;
const TWEEN_MS = 600;
const DPR_CAP = 1.5;
// 下一帧从上一帧的 rAF 时间戳起算、提前 2ms 起定时器:定时器从 draw 结束才起算时,33ms 常常刚好错过第 2 个 vsync,
// 忙时实际只有 20~24 帧/秒;空闲 125ms 同理。提前量小于一个 vsync,帧率上限仍是 1000 / (间隔 − 2)。
const FRAME_SLACK_MS = 2;
const MAX_COMETS = 4;
const WAVE_STEP_MS = 80;
const WAVE_MS = 600;
const RIPPLE_MS = 900;
const LIT_MS = 1200;
// 解码前按像素数拒绝:文件体积 20MB 以内的 PNG 也可能是 2 万 × 2 万(RGBA 约 1.6GB),Image.decode 会按原始分辨率解码。
export const IMAGE_MAX_PIXELS = 40e6;
const TEXT_TOKENS = ["--fg", "--fg-strong", "--dim", "--accent-text", "--ok", "--err", "--warn"];
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const ease = (p) => 1 - (1 - p) ** 3;
const clock = () => (typeof performance !== "undefined" && typeof performance.now === "function" ? performance.now() : Date.now());

// ---------- 主题取色:token → rgb,星光小图 ----------
// getComputedStyle 只在主题变化后第一次取用时读一次(按 data-theme 缓存),每帧不读。
let themeKit = null;
function parseColor(probe, value) {
  let text = String(value || "").trim();
  if (probe && text) {
    probe.fillStyle = "#000000";
    probe.fillStyle = text;
    text = String(probe.fillStyle);
  }
  let m = text.match(/^#([0-9a-f]{6})$/i);
  if (m) return [0, 2, 4].map((i) => parseInt(m[1].slice(i, i + 2), 16));
  m = text.match(/^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/i);
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}
function rgba([r, g, b], a) {
  return `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${a})`;
}
function makeSprite(rgb, size = 64) {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  const g = canvas.getContext?.("2d");
  if (!g) return null;
  const grad = g.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  grad.addColorStop(0, rgba(rgb, 1));
  grad.addColorStop(0.12, rgba(rgb, 1));
  grad.addColorStop(0.28, rgba(rgb, 0.33));
  grad.addColorStop(1, rgba(rgb, 0));
  g.fillStyle = grad;
  g.fillRect(0, 0, size, size);
  return canvas;
}
export function getThemeKit() {
  const key = document.documentElement.getAttribute("data-theme") || "dark";
  if (themeKit?.key === key) return themeKit;
  const probeCanvas = document.createElement("canvas");
  probeCanvas.width = probeCanvas.height = 1;
  const probe = probeCanvas.getContext?.("2d") ?? null;
  const style = typeof getComputedStyle === "function" ? getComputedStyle(document.documentElement) : null;
  const token = (name, fallback) => style?.getPropertyValue(name).trim() || fallback;
  const color = (name, fallback) => parseColor(probe, token(name, fallback)) ?? parseColor(null, fallback);
  const number = (name, fallback) => {
    const value = Number(token(name, ""));
    return Number.isFinite(value) && value > 0 ? value : fallback;
  };
  const palette = {
    star: color("--backdrop-star", "#ececec"),
    line: color("--backdrop-line", "#9a9a9a"),
    pulse: color("--accent", "#ff8700"),
    error: color("--err", "#ffad66"),
    chatBg: color("--chat-bg", "#181818"),
    texts: TEXT_TOKENS.map((name) => color(name, "#e5e5e5")),
    starAlpha: number("--backdrop-star-alpha", 0.62),
    lineAlpha: number("--backdrop-line-alpha", 0.22),
    dustAlpha: number("--backdrop-dust-alpha", 0.32),
  };
  themeKit = {
    key,
    palette,
    watermark: watermarkCap(palette),
    sprites: { star: makeSprite(palette.star), pulse: makeSprite(palette.pulse), error: makeSprite(palette.error) },
  };
  return themeKit;
}

// ---------- 画一帧 ----------
// Painter 只管「给定模型、构图与这一刻的状态,画出来」;调度、事件、测量在 createConstellationBackdrop 里。
// 每一笔 alpha 都走 this.a(value) → layerAlpha(夹到 [0,1])。水印(layout.capped)时 Painter 画在离屏层上,
// 用「水印配比」:最亮的星核为 1、连线提亮 WATERMARK_LINE_BOOST 倍,总不透明度交给合成那一步。
class Painter {
  constructor() {
    this.model = null;
    this.layout = null;
    this.kit = null;
    this.prefs = { density: 1, opacity: 1 };
    this.starScale = 1;
    this.lineWidth = 1;
  }

  setModel(model, seed = 17) {
    this.model = model;
    this.order = bfsOrder(model);
    this.orderIndex = new Int32Array(model.edges.length);
    this.order.edgeOrder.forEach((edge, index) => { this.orderIndex[edge] = index; });
    this.hub = model.hub >= 0 ? model.hub : this.order.root;
    this.hubDepth = edgeDepthsFrom(model, this.hub);
    const rand = seededRandom(seed);
    this.twinkle = model.points.map(() => ({ period: 3200 + rand() * 4300, phase: rand() * Math.PI * 2 }));
    // 星尘:围绕星座的一片天区(框的 1.7 倍),泊松分布、离中心越远越淡;按密度偏好取前缀(接受顺序随机,前缀仍均匀)。
    const candidates = Array.from({ length: 1600 }, () => [rand() * 1.7 - 0.35, rand() * 1.7 - 0.35, rand()]);
    candidates.sort((x, y) => y[2] - x[2]);
    this.dust = poissonSelect(candidates, 0.075)
      .map(([x, y]) => ({ x, y, period: 2600 + rand() * 5200, phase: rand() * Math.PI * 2, fade: Math.max(0, 1 - Math.hypot(x - 0.5, y - 0.5) / 0.85) }))
      .filter((dot) => dot.fade > 0.03);
  }

  a(value) {
    return layerAlpha(value);
  }

  // frame: { t, still, activity, drawIn, redraw: {edge, p} | null, lit: Float32Array | null, comets, ripples, voiceGain, hubTone }
  paint(ctx, frame) {
    const { model, layout, kit } = this;
    if (!model || !layout || !kit) return;
    const { palette, sprites } = kit;
    const watermark = Boolean(layout.capped);
    const base = watermark ? 1 / (palette.starAlpha * GAIN.star) : layout.alpha * this.prefs.opacity;
    const lineBase = watermark ? base * WATERMARK_LINE_BOOST : base;
    const box = layout.box;
    const scale = Math.max(box.w, box.h);
    const cx = box.x + box.w / 2, cy = box.y + box.h / 2;
    const t = frame.t;
    const drift = frame.still ? [0, 0] : [Math.sin((t / 97000) * Math.PI * 2) * 3, Math.cos((t / 131000) * Math.PI * 2) * 3];
    const map = (x, y, d) => [cx + (x - 0.5) * scale + d[0], cy + (y - 0.5) * scale + d[1]];
    ctx.save();
    if (!layout.capped && layout.avoid?.length) {
      // 正文避让区挖空:外框 + 避让矩形,evenodd 规则下避让矩形之内不可画。
      ctx.beginPath();
      ctx.rect(-4096, -4096, 16384, 16384);
      for (const r of layout.avoid) ctx.rect(r.x, r.y, r.w, r.h);
      ctx.clip("evenodd");
    }

    // ① 星尘:与星座反向漂移,形成一点视差。
    const dustCount = Math.round(this.dust.length * clamp(this.prefs.density, 0, 2) / 2);
    if (dustCount && sprites.star) {
      const dustDrift = [drift[0] * -0.4, drift[1] * -0.4];
      const size = 5.6 * this.starScale;
      for (let i = 0; i < dustCount; i += 1) {
        const dot = this.dust[i];
        const twinkle = frame.still ? 0.8 : 0.55 + 0.45 * Math.sin((t / dot.period) * Math.PI * 2 + dot.phase);
        const alpha = palette.dustAlpha * GAIN.dust * base * twinkle * dot.fade;
        if (alpha < 0.004) continue;
        const [x, y] = map(dot.x, dot.y, dustDrift);
        ctx.globalAlpha = this.a(alpha);
        ctx.drawImage(sprites.star, x - size / 2, y - size / 2, size, size);
      }
    }

    // ② 连线:一条 path 一次描边;两端各留空隙不压星;入场按 BFS 顺序逐条画出,空闲时偶尔重描一条。
    const pts = model.points.map((p) => map(p[0], p[1], drift));
    const total = model.edges.length || 1;
    const progressOf = (edge) => {
      let p = clamp(frame.drawIn * total - this.orderIndex[edge], 0, 1);
      if (frame.redraw && frame.redraw.edge === edge) p = Math.min(p, frame.redraw.p);
      return p;
    };
    const segment = (edge, progress) => {
      const [i, j] = model.edges[edge];
      const a = pts[i], b = pts[j];
      const len = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
      const gap = Math.min(4 * this.starScale, len * 0.2);
      const ux = (b[0] - a[0]) / len, uy = (b[1] - a[1]) / len;
      const end = gap + (len - 2 * gap) * progress;
      return [a[0] + ux * gap, a[1] + uy * gap, a[0] + ux * end, a[1] + uy * end];
    };
    ctx.lineCap = "round";
    ctx.lineWidth = this.lineWidth;
    ctx.strokeStyle = rgba(palette.line, 1);
    ctx.globalAlpha = this.a(palette.lineAlpha * lineBase * (activityBusy(frame.activity) ? GAIN.line : 1));
    ctx.beginPath();
    for (let edge = 0; edge < model.edges.length; edge += 1) {
      const progress = progressOf(edge);
      if (progress <= 0) continue;
      const [x0, y0, x1, y1] = segment(edge, progress);
      ctx.moveTo(x0, y0);
      ctx.lineTo(x1, y1);
    }
    ctx.stroke();

    // ③ 被点亮的边(完成波、回复时光点走过的边):强调色,逐条按强度。
    if (frame.lit) {
      ctx.strokeStyle = rgba(palette.pulse, 1);
      ctx.lineWidth = this.lineWidth * 1.4;
      for (let edge = 0; edge < model.edges.length; edge += 1) {
        const strength = frame.lit[edge];
        if (!(strength > 0.02)) continue;
        const [x0, y0, x1, y1] = segment(edge, progressOf(edge));
        ctx.globalAlpha = this.a(GAIN.lit * base * strength);
        ctx.beginPath();
        ctx.moveTo(x0, y0);
        ctx.lineTo(x1, y1);
        ctx.stroke();
      }
    }

    // ④ 星:半径随亮度,周期 3.2~7.5s 的正弦闪烁(暗星幅度大);语音播报时亮星随音量提亮。
    const voiceGain = clamp(frame.voiceGain ?? 1, 1, GAIN.voice);
    model.points.forEach((p, index) => {
      const [x, y] = pts[index];
      const { period, phase } = this.twinkle[index];
      const bright = clamp((4.8 - p[2]) / 4.3, 0, 1);
      const wave = frame.still ? 0.5 : 0.5 + 0.5 * Math.sin((t / period) * Math.PI * 2 + phase);
      const twinkle = 1 - (0.12 + 0.25 * (1 - bright)) * wave;
      const radius = (0.8 + bright * 1.9) * this.starScale;
      const size = radius * (bright > 0.7 ? 7 : 5);
      // hub 着色(hubTone):减少动态效果时没有光点,运行中改由 hub 着强调色表达「在跑」;本轮失败着淡 --err。
      const tone = index === this.hub ? frame.hubTone : null;
      const toned = tone === "pulse" ? sprites.pulse : tone === "error" ? sprites.error : null;
      const starAlpha = palette.starAlpha * base * (0.45 + 0.75 * bright) * twinkle * (bright > 0.5 ? voiceGain : 1);
      ctx.globalAlpha = this.a(toned ? (tone === "pulse" ? GAIN.comet : GAIN.hubError) * base : starAlpha);
      const sprite = toned ?? sprites.star;
      if (sprite) ctx.drawImage(sprite, x - size / 2, y - size / 2, size, size);
    });

    // ⑤ 光点:沿路线逐边走,渐变尾迹 + 强调色星光。
    for (const comet of frame.comets || []) {
      const elapsed = t - comet.start;
      const index = Math.floor(elapsed / comet.perEdge);
      const step = comet.route[index];
      if (!step || elapsed < 0) continue;
      const progress = (elapsed % comet.perEdge) / comet.perEdge;
      const a = pts[step[0]], b = pts[step[1]];
      const hx = a[0] + (b[0] - a[0]) * progress, hy = a[1] + (b[1] - a[1]) * progress;
      const tail = Math.max(0, progress - 0.45);
      const tx = a[0] + (b[0] - a[0]) * tail, ty = a[1] + (b[1] - a[1]) * tail;
      const grad = ctx.createLinearGradient(tx, ty, hx, hy);
      grad.addColorStop(0, rgba(palette.pulse, 0));
      grad.addColorStop(1, rgba(palette.pulse, 1));
      ctx.strokeStyle = grad;
      ctx.lineWidth = 1.8 * this.starScale;
      ctx.globalAlpha = this.a(0.8 * base);
      ctx.beginPath();
      ctx.moveTo(tx, ty);
      ctx.lineTo(hx, hy);
      ctx.stroke();
      if (sprites.pulse) {
        const size = 16 * this.starScale;
        ctx.globalAlpha = this.a(GAIN.comet * base);
        ctx.drawImage(sprites.pulse, hx - size / 2, hy - size / 2, size, size);
      }
    }

    // ⑥ 失败涟漪:hub 上一圈失败色,900ms 散开。
    for (const ripple of frame.ripples || []) {
      const p = (t - ripple.start) / RIPPLE_MS;
      if (p < 0 || p >= 1) continue;
      const [x, y] = pts[this.hub] ?? [cx, cy];
      ctx.strokeStyle = rgba(palette.error, 1);
      ctx.lineWidth = 1.2;
      ctx.globalAlpha = this.a(GAIN.ripple * base * (1 - p));
      ctx.beginPath();
      ctx.arc(x, y, 4 + 22 * ease(p), 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.restore();
    ctx.globalAlpha = 1;
  }
}

// ---------- 缩略图(设置页图案卡片) ----------
export function renderConstellationThumb(canvas, model, { padding = 6 } = {}) {
  const ctx = typeof canvas?.getContext === "function" ? canvas.getContext("2d") : null;
  if (!ctx || !model) return false;
  const rect = canvas.getBoundingClientRect();
  const w = rect.width || canvas.clientWidth || 92, h = rect.height || canvas.clientHeight || 60;
  const dpr = clamp(window.devicePixelRatio || 1, 1, 2);
  canvas.width = Math.round(w * dpr);
  canvas.height = Math.round(h * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  const aspect = model.aspect || 1;
  const bw = w - 2 * padding, bh = h - 2 * padding;
  const fw = Math.min(bw, bh * aspect), fh = fw / aspect;
  const painter = new Painter();
  painter.kit = getThemeKit();
  painter.setModel(model);
  painter.starScale = 0.6;
  painter.lineWidth = 0.8;
  painter.prefs = { density: 0, opacity: 1.3 };
  painter.layout = { box: { x: (w - fw) / 2, y: (h - fh) / 2, w: fw, h: fh }, alpha: 1, avoid: [], capped: false, placement: "thumb" };
  painter.paint(ctx, { t: STILL_T, still: true, activity: "idle", drawIn: 1 });
  return true;
}

// ---------- 图片 → 星座(只在本机转换,只留点集) ----------
// File → objectURL → Image 的 load(只解析头部拿到尺寸,Chromium 延迟解码)→ 像素数超 IMAGE_MAX_PIXELS 拒绝
// → createImageBitmap 直接解码到长边 192(SVG 等不支持时退回 drawImage)→ getImageData → imageToConstellation,
// 之后立刻释放像素、位图与 objectURL。
function imageError(code) {
  const error = new Error(`image ${code}`);
  error.code = code;
  return error;
}
export async function imageFileToConstellation(file, { count = IMAGE_STAR_COUNT } = {}) {
  if (!file) return null;
  if (file.size > 20 * 1024 * 1024) throw imageError("too-large");
  const url = URL.createObjectURL(file);
  let bitmap = null;
  try {
    const img = new Image();
    img.decoding = "async";
    await new Promise((resolve, reject) => {
      img.onload = resolve;
      img.onerror = () => reject(imageError("unreadable"));
      img.src = url;
    });
    const w0 = img.naturalWidth || IMAGE_MAX_SIDE, h0 = img.naturalHeight || IMAGE_MAX_SIDE;
    if (w0 * h0 > IMAGE_MAX_PIXELS) throw imageError("too-many-pixels");
    const scale = IMAGE_MAX_SIDE / Math.max(w0, h0);
    const w = Math.max(8, Math.round(w0 * scale)), h = Math.max(8, Math.round(h0 * scale));
    try {
      bitmap = typeof createImageBitmap === "function"
        ? await createImageBitmap(file, { resizeWidth: w, resizeHeight: h, resizeQuality: "medium" })
        : null;
    } catch {
      bitmap = null;
    }
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const g = canvas.getContext("2d", { willReadFrequently: true });
    g.drawImage(bitmap ?? img, 0, 0, w, h);
    const model = imageToConstellation(g.getImageData(0, 0, w, h), { count });
    canvas.width = canvas.height = 0;
    return model ? sanitizeModel({ ...model, name: String(file.name || "").slice(0, 60) }) : null;
  } finally {
    bitmap?.close?.();
    URL.revokeObjectURL(url);
  }
}

// ---------- 对话区背景 ----------
function modelKey(prefs) {
  const p = normalizeBackdropPrefs(prefs);
  if (p.preset === "custom" && p.custom) return `custom:${p.custom.points.length}:${p.custom.edges.length}:${p.custom.points[0]?.join(",")}`;
  return p.preset === "kanzei" ? `kanzei:${Math.max(0.5, p.density).toFixed(1)}` : p.preset;
}
function sameLayout(a, b) {
  if (!a || !b) return false;
  const near = (x, y) => Math.abs(x - y) < 1.5;
  return a.placement === b.placement && a.capped === b.capped && Math.abs(a.alpha - b.alpha) < 0.01
    && near(a.box.x, b.box.x) && near(a.box.y, b.box.y) && near(a.box.w, b.box.w) && near(a.box.h, b.box.h)
    && a.avoid.length === b.avoid.length && a.avoid.every((r, i) => near(r.x, b.avoid[i].x) && near(r.w, b.avoid[i].w) && near(r.y, b.avoid[i].y) && near(r.h, b.avoid[i].h));
}


// 水印离屏层的范围:星座框外扩星尘铺开的 0.85·S(星尘在框的 -0.35~1.35 之间)与星光 / 光点小图的半径,
// 夹到画布内,并对齐设备像素(离屏层与画布一比一,不做重采样)。
function watermarkBounds(layout, size) {
  const box = layout.box;
  const scale = Math.max(box.w, box.h);
  const half = 0.85 * scale + 28;
  const cx = box.x + box.w / 2, cy = box.y + box.h / 2;
  const snap = (v) => Math.round(v * size.dpr) / size.dpr;
  const x0 = snap(clamp(cx - half, 0, size.w)), y0 = snap(clamp(cy - half, 0, size.h));
  const x1 = snap(clamp(cx + half, 0, size.w)), y1 = snap(clamp(cy + half, 0, size.h));
  return x1 - x0 >= 1 && y1 - y0 >= 1 ? { x: x0, y: y0, w: x1 - x0, h: y1 - y0 } : null;
}

// waitForPrefs:启动时先不画,等偏好模块第一次发布(后端 app.json 的值,或 250ms 超时后的本地缓存 / 默认值)
// 再开始——关掉背景或换了图案的用户,每次启动不再先闪一下默认星座。
export function createConstellationBackdrop(canvas, { getSessionId = () => null, getRuntime = () => null, prefs = null, waitForPrefs = false } = {}) {
  const store = createOcStateStore({ getSessionId, getRuntime });
  let current = normalizeBackdropPrefs(prefs);
  const activityNow = () => {
    try { return store.current(); } catch { return "idle"; }
  };
  const ctx = typeof canvas?.getContext === "function" ? canvas.getContext("2d") : null;
  if (!ctx) {
    // 降级(冒烟假 DOM、拿不到 2D 上下文):只维护活动态与偏好,不画、不排任何定时器。
    return {
      degraded: true,
      emit(type, detail = {}) { store.emit(type, detail); },
      voice() {},
      setPrefs(next) { current = normalizeBackdropPrefs(next); },
      relayout() {},
      snapshot: () => ({ degraded: true, enabled: current.enabled, preset: current.preset, activity: activityNow(), placement: null, comets: 0 }),
      destroy() {},
    };
  }

  const painter = new Painter();
  const brand = createBrandBackdrop(canvas);
  let started = !waitForPrefs;
  let key = "";
  let layout = null;
  let tween = null;
  let size = { w: 0, h: 0, dpr: 1 };
  let drawInStart = -Infinity;
  let comets = [];
  let ripples = [];
  let waves = [];
  let litUntil = new Map();
  let redraw = null;
  let nextRedrawAt = clock() + 24000;
  let voice = { gain: 1, at: 0, lastListen: 0 };
  let lastActivity = "idle";
  let lastWake = -Infinity;
  let frameTimer = 0;
  let frameRaf = 0;
  let dueAt = 0;
  let lastFrameAt = -Infinity;
  let watchTimer = 0;
  let layer = null;
  let destroyed = false;
  const reducedQuery = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
  const reduced = () => Boolean(reducedQuery?.matches);
  // 预览页(scripts/ui-preview,html[data-kz-preview])固定时刻画静帧,截图可复现。
  const preview = () => document.documentElement.dataset.kzPreview != null;
  const still = () => reduced() || preview();
  const viewActive = () => {
    const view = canvas.closest?.(".view");
    return !view || view.classList.contains("active");
  };
  const active = () => started && !document.hidden && viewActive() && current.enabled;

  function ensureModel() {
    const next = modelKey(current);
    if (next === key && painter.model) return;
    key = next;
    painter.setModel(resolveBackdropModel(current, DATA));
    comets = [];
    waves = [];
    litUntil = new Map();
    redraw = null;
    drawInStart = clock();
  }

  // 返回画布是否换了尺寸——换尺寸会清空画布,调用方要同步补画一帧。
  function syncSize() {
    const rect = canvas.getBoundingClientRect();
    const dpr = clamp(window.devicePixelRatio || 1, 1, DPR_CAP);
    const w = Math.max(1, rect.width), h = Math.max(1, rect.height);
    let resized = false;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      resized = true;
    }
    size = { w, h, dpr };
    return { rect, resized };
  }

  // 实测各块位置(相对画布):对话区(减去可见的侧面板)、空态 art 槽与文案、正文列、OC 伴侣。
  // 侧面板:旧的 #bg-panel / #agent-panel 与 panels 组合并后的 #tasks-panel(停靠在对话区外时左缘落在画布右侧之外,
  // 自然不扣;窄窗口下变成叠在对话区上的抽屉时按左缘扣掉)。
  function measure(base) {
    const rel = (el) => {
      if (!el) return null;
      const b = el.getBoundingClientRect();
      if (!b.width || !b.height) return null;
      return { x: b.left - base.left, y: b.top - base.top, w: b.width, h: b.height };
    };
    let area = { x: 0, y: 0, w: base.width, h: base.height };
    for (const id of ["tasks-panel", "bg-panel", "agent-panel"]) {
      const panel = document.getElementById(id);
      if (!panel || panel.classList.contains("hidden")) continue;
      const r = rel(panel);
      if (r && r.x > area.w * 0.35 && r.x < area.w) area = { ...area, w: r.x };
    }
    const ocOn = document.documentElement.dataset.ocEnabled !== "false";
    const view = canvas.closest?.(".view");
    if (view?.classList.contains("voice-mode")) {
      const slot = ocOn ? rel(view.querySelector(".voice-art")) : null;
      return { area, mode: "voice", slot, ocInSlot: Boolean(slot), copy: rel(view.querySelector(".voice-stage-copy")) };
    }
    const pane = document.querySelector('#messages .msg-pane[data-active="1"]');
    const empty = pane?.querySelector(".empty-state");
    if (empty) {
      const slot = rel(empty.querySelector(".empty-art"));
      return { area, mode: "welcome", slot, ocInSlot: ocOn && Boolean(slot), copy: rel(empty.querySelector(".empty-copy")) };
    }
    return { area, mode: "conversation" };
  }

  function relayout() {
    if (!active()) return { changed: false, resized: false };
    ensureModel();
    const { rect: base, resized } = syncSize();
    if (!base.width || !base.height) return { changed: false, resized };
    const next = layoutBackdrop({ ...measure(base), aspect: painter.model.aspect || 1 });
    if (sameLayout(next, tween?.to ?? layout)) return { changed: false, resized };
    if (!layout || still() || next.placement === "hidden" || layout.placement === "hidden") {
      layout = next;
      tween = null;
    } else {
      tween = { from: tween ? currentLayout(clock()) : layout, to: next, start: clock() };
    }
    return { changed: true, resized };
  }

  function currentLayout(now) {
    if (!tween) return layout;
    const p = ease(clamp((now - tween.start) / TWEEN_MS, 0, 1));
    const lerp = (a, b) => a + (b - a) * p;
    const { from, to } = tween;
    const mixed = {
      ...to,
      alpha: lerp(from.alpha, to.alpha),
      box: { x: lerp(from.box.x, to.box.x), y: lerp(from.box.y, to.box.y), w: lerp(from.box.w, to.box.w), h: lerp(from.box.h, to.box.h) },
    };
    if (p >= 1) {
      layout = to;
      tween = null;
      return to;
    }
    return mixed;
  }

  function spawn(kind, { resident = false, speed = 1, lights = false } = {}) {
    if (!painter.model || comets.length >= MAX_COMETS || still()) return false;
    const route = cometRoute(painter.model, kind, Math.random);
    if (!route.length) return false;
    const scale = Math.max(layout?.box.w ?? 240, layout?.box.h ?? 240);
    const avg = route.reduce((sum, [i, j]) => sum + Math.hypot(painter.model.points[i][0] - painter.model.points[j][0], painter.model.points[i][1] - painter.model.points[j][1]), 0) / route.length;
    const perEdge = clamp(((avg * scale) / 110) * 1000, 260, 900) / speed;
    comets.push({ kind, route, perEdge, start: clock(), resident, lights });
    return true;
  }

  // 忙 = 画面上真有东西在动:光点 / 涟漪 / 波 / 补间 / 重描 / 入场 / 语音提亮,或处在有常驻光点的活动态。
  // 失败会话(blocked)与刚完成(complete)不算——它们没有常驻动画,按空闲 ≤ 8 帧/秒。
  function isBusy(now) {
    return Boolean(comets.length || ripples.length || waves.length || tween || redraw
      || now - drawInStart < DRAW_IN_MS || voice.gain > 1.01 || activityBusy(lastActivity));
  }

  // 每帧推进:剪掉结束的光点 / 波 / 涟漪,按活动态补常驻光点,安排空闲时的偶发重描。
  function advance(now, activity) {
    comets = comets.filter((c) => now - c.start < c.route.length * c.perEdge);
    ripples = ripples.filter((r) => now - r.start < RIPPLE_MS);
    const maxDepth = Math.max(1, ...painter.hubDepth);
    waves = waves.filter((w) => now - w.start < maxDepth * WAVE_STEP_MS + WAVE_MS);
    // 常驻光点表达「正在做什么」:思考 / 回复沿主干,执行沿行动那一笔。活动态换了,旧的走完本程再退场。
    const want = activity === "thinking" || activity === "replying" ? "trunk" : activity === "executing" ? "action" : null;
    for (const comet of comets) if (comet.resident && comet.kind !== want) comet.resident = false;
    if (want && !comets.some((c) => c.resident)) spawn(want, { resident: true, speed: activity === "replying" ? 1.6 : 1, lights: activity === "replying" });
    for (const comet of comets) {
      if (!comet.lights) continue;
      const step = comet.route[Math.floor((now - comet.start) / comet.perEdge)];
      if (step) litUntil.set(step[2], now + LIT_MS);
    }
    for (const [edge, until] of litUntil) if (until <= now) litUntil.delete(edge);
    if (redraw && now - redraw.start > 1600) redraw = null;
    if (activity === "idle" && !redraw && !comets.length && now >= nextRedrawAt && painter.model.edges.length) {
      redraw = { edge: Math.floor(Math.random() * painter.model.edges.length), start: now };
      nextRedrawAt = now + 24000 + Math.random() * 16000;
    }
    if (voice.gain > 1) voice.gain = 1 + (voice.gain - 1) * clamp(1 - (now - voice.at) / 600, 0, 1);
  }

  function litStrengths(now) {
    if (!waves.length && !litUntil.size) return null;
    const lit = new Float32Array(painter.model.edges.length);
    for (const wave of waves) {
      painter.hubDepth.forEach((depth, edge) => {
        const p = (now - wave.start - depth * WAVE_STEP_MS) / WAVE_MS;
        if (p > 0 && p < 1) lit[edge] = Math.max(lit[edge], Math.sin(p * Math.PI));
      });
    }
    for (const [edge, until] of litUntil) lit[edge] = Math.max(lit[edge], 0.8 * clamp((until - now) / LIT_MS, 0, 1));
    return lit;
  }

  // 预览静帧里的光点:同一时刻同一位置(与原型截图一致),只为让截图看得见运行态。
  function stillComets(activity) {
    const kinds = activity === "executing" ? ["recall", "action"] : activity === "thinking" || activity === "replying" ? ["trunk"] : [];
    const out = [];
    kinds.forEach((kind, c) => {
      const route = cometRoute(painter.model, kind, seededRandom(11 + c));
      if (!route.length) return;
      const perEdge = 700;
      const pos = (STILL_T / perEdge + c * 0.37) % route.length;
      out.push({ route, perEdge, start: STILL_T - pos * perEdge });
    });
    return out;
  }

  // 水印:整张星座画进离屏层(与画布同分辨率、只覆盖星座周围),再以单一不透明度合成——
  // 同一像素叠多少层都只算一次,正文底下任一像素的混色不透明度 ≤ watermarkAlpha。
  function paintWatermark(frame) {
    const bounds = watermarkBounds(painter.layout, size);
    if (!bounds) return;
    const pw = Math.max(1, Math.round(bounds.w * size.dpr)), ph = Math.max(1, Math.round(bounds.h * size.dpr));
    if (!layer) layer = document.createElement("canvas");
    if (layer.width !== pw || layer.height !== ph) {
      layer.width = pw;
      layer.height = ph;
    }
    const lctx = layer.getContext("2d");
    if (!lctx) return;
    lctx.setTransform(size.dpr, 0, 0, size.dpr, -bounds.x * size.dpr, -bounds.y * size.dpr);
    lctx.clearRect(bounds.x, bounds.y, bounds.w, bounds.h);
    painter.paint(lctx, frame);
    ctx.globalAlpha = watermarkAlpha(painter.kit.watermark, current.opacity);
    ctx.drawImage(layer, bounds.x, bounds.y, bounds.w, bounds.h);
    ctx.globalAlpha = 1;
  }

  function draw(now) {
    if (destroyed) return;
    const w = size.w, h = size.h;
    ctx.setTransform(size.dpr, 0, 0, size.dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    brand.hide();
    if (!started || !current.enabled || !painter.model) return;
    const activity = activityNow();
    lastActivity = activity;
    const isStill = still();
    if (!isStill) advance(now, activity);
    const shown = isStill ? layout : currentLayout(now);
    if (!shown || shown.placement === "hidden") return;
    painter.kit = getThemeKit();
    painter.prefs = current;
    painter.layout = shown;
    // 星点随框缩放:窄沟里的小徽记不能顶着 19px 的星光。
    painter.starScale = clamp(Math.max(shown.box.w, shown.box.h) / 180, 0.55, 1);
    const frame = {
      t: isStill ? STILL_T : now,
      still: isStill,
      activity,
      drawIn: isStill ? 1 : ease(clamp((now - drawInStart) / DRAW_IN_MS, 0, 1)),
      redraw: !isStill && redraw ? { edge: redraw.edge, p: redrawProgress(now) } : null,
      lit: isStill ? null : litStrengths(now),
      comets: isStill ? (preview() && activityBusy(activity) ? stillComets(activity) : []) : comets,
      ripples: isStill ? [] : ripples,
      voiceGain: voice.gain,
      hubTone: hubTone(activity, { still: isStill && !preview() }),
    };
    if (current.preset === "kanzei") {
      brand.paint({ layout: shown, size, frame, prefs: current, kit: painter.kit, model: painter.model });
      return;
    }
    if (shown.capped) paintWatermark(frame);
    else {
      if (layer) layer.width = layer.height = 0;
      layer = null;
      painter.paint(ctx, frame);
    }
  }

  function redrawProgress(now) {
    const e = now - redraw.start;
    return e < 500 ? 1 - ease(e / 500) : ease(clamp((e - 500) / 1100, 0, 1));
  }

  function cancelFrame() {
    if (frameTimer) clearTimeout(frameTimer);
    if (frameRaf) cancelAnimationFrame(frameRaf);
    frameTimer = 0;
    frameRaf = 0;
  }

  function schedule() {
    if (destroyed) return;
    const delay = frameDelay({ hidden: document.hidden, viewActive: viewActive(), enabled: current.enabled && started && layout?.placement !== "hidden", reduced: still(), busy: isBusy(clock()) });
    if (delay === null) {
      cancelFrame();
      return;
    }
    if (frameTimer || frameRaf) return;
    const now = clock();
    const wait = Math.max(0, delay - (now - lastFrameAt) - FRAME_SLACK_MS);
    dueAt = now + wait;
    frameTimer = setTimeout(() => {
      frameTimer = 0;
      frameRaf = requestAnimationFrame((stamp) => {
        frameRaf = 0;
        lastFrameAt = stamp;
        draw(stamp);
        schedule();
      });
    }, wait);
  }

  // 把空闲的 125ms 等待提前到忙帧;已排好的 rAF、以及一个忙帧内就会触发的定时器都不动(见 core 的 shouldHurry)。
  function hurry() {
    const now = clock();
    if (shouldHurry({ timer: frameTimer, raf: frameRaf, busy: isBusy(now), dueIn: dueAt - now })) {
      clearTimeout(frameTimer);
      frameTimer = 0;
    }
    schedule();
  }

  function scheduleWatch() {
    const delay = watchDelay({ hidden: document.hidden, viewActive: viewActive(), enabled: current.enabled && started });
    if (delay === null) {
      if (watchTimer) clearTimeout(watchTimer);
      watchTimer = 0;
      return;
    }
    if (watchTimer) return;
    // 预览页场景脚本切空态 / 对话态后很快截图,观察间隔缩短,免得截到上一个构图。
    watchTimer = setTimeout(() => {
      watchTimer = 0;
      const { changed, resized } = relayout();
      if (resized || (changed && (still() || layout?.placement === "hidden"))) draw(clock());
      if (!still()) hurry();
      scheduleWatch();
    }, preview() ? Math.min(delay, 120) : delay);
  }

  // 有新东西要画(事件、布局、偏好):静帧模式立即重画一帧;动画模式把空闲的等待提前。
  function wake() {
    if (!active()) return;
    if (still()) {
      draw(clock());
      return;
    }
    hurry();
  }

  function refresh() {
    if (destroyed || !started) return;
    if (!active()) {
      brand.hide();
      cancelFrame();
      scheduleWatch();
      if (!current.enabled) {
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.clearRect(0, 0, canvas.width, canvas.height);
      }
      return;
    }
    const { changed, resized } = relayout();
    scheduleWatch();
    if (changed && layout?.placement === "hidden") draw(clock());
    // 画布换了尺寸就已被清空:同步补画一帧(拖动改窗口尺寸 / 分隔条时 ResizeObserver 每帧都来,不能留空窗)。
    if (still() || resized) draw(clock());
    if (!still()) hurry();
  }

  function setPrefs(next) {
    const previous = current;
    const first = !started;
    started = true;
    current = normalizeBackdropPrefs(next);
    if (first || previous.enabled !== current.enabled || modelKey(previous) !== modelKey(current)) layout = null;
    // 关掉再打开:连线按 BFS 顺序重新画出来,而不是突然整张出现。
    if (!previous.enabled && current.enabled) drawInStart = clock();
    refresh();
  }

  function emit(type, detail = {}) {
    store.emit(type, detail);
    const activeId = getSessionId();
    // 后台会话的事件只写进 store(切回来时活动态仍对),不在当前画面放光点。
    if (detail.session_id && activeId && detail.session_id !== activeId) return;
    if (!started || !current.enabled || !painter.model) return;
    const now = clock();
    let spawned = false;
    if (!still()) {
      if (type === "tool_started") spawned = spawn(String(detail.tool_name ?? "").toLowerCase() === "memory_search" ? "recall" : "action");
      else if (["memory_search_started", "memory_recall_retrieved", "memory_recall_injected", "research_source_retrieved"].includes(type)) spawned = spawn("recall");
      if ((type === "tool_completed" && detail.ok === false) || ["run_failed", "memory_consolidation_failed", "memory_search_failed", "memory_cleanup_failed"].includes(type)) {
        comets = [];
        waves = [];
        litUntil.clear();
        redraw = null;
        ripples.push({ start: now });
        spawned = true;
      } else if (["run_completed", "context_compacted", "memory_consolidation_completed", "memory_candidate_promoted"].includes(type)) {
        if (type === "run_completed") comets = [];
        waves.push({ start: now });
        spawned = true;
      }
    }
    const activity = activityNow();
    const changed = activity !== lastActivity;
    lastActivity = activity;
    if (still()) {
      if (changed) wake();
      return;
    }
    // 流式分片(约 16ms 一次)只写活动态;唤醒每 WAKE_THROTTLE_MS 至多一次,真有新东西(活动态变了、放了光点)立即唤醒。
    if (changed || spawned || now - lastWake >= WAKE_THROTTLE_MS) {
      lastWake = now;
      wake();
    }
  }

  function voiceSignal(sessionId, phase, level) {
    if (sessionId !== getSessionId() || !current.enabled) return;
    const now = clock();
    if (phase === "speaking") {
      voice.gain = 1 + 0.5 * clamp(Number(level) || 0, 0, 1);
      voice.at = now;
      if (now - lastWake >= WAKE_THROTTLE_MS) {
        lastWake = now;
        wake();
      }
    } else if (phase === "listening" && now - voice.lastListen > 1100) {
      voice.lastListen = now;
      spawn("recall");
      wake();
    }
  }

  const onChange = () => refresh();
  const onPrefs = (event) => setPrefs(event?.detail ?? current);
  const events = ["kz:view-changed", "kz:voice-layout", "kz:oc-preference", "kz:oc-settings", "visibilitychange"];
  for (const name of events) document.addEventListener(name, onChange);
  document.addEventListener("kz:backdrop-settings", onPrefs);
  reducedQuery?.addEventListener?.("change", onChange);
  const themeObserver = typeof MutationObserver === "function" ? new MutationObserver(onChange) : null;
  themeObserver?.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  const resizeObserver = typeof ResizeObserver === "function" ? new ResizeObserver(onChange) : null;
  resizeObserver?.observe(canvas);
  // 只在空态与消息态切换时重排;流式正文更新不触发布局测量。
  let wasWelcome = Boolean(document.querySelector('#messages .msg-pane[data-active="1"] .empty-state'));
  const messageObserver = typeof MutationObserver === "function" ? new MutationObserver(() => {
    const welcome = Boolean(document.querySelector('#messages .msg-pane[data-active="1"] .empty-state'));
    if (welcome === wasWelcome) return;
    wasWelcome = welcome;
    refresh();
  }) : null;
  const messages = document.getElementById("messages");
  if (messages) messageObserver?.observe(messages, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-active"] });
  refresh();

  return {
    degraded: false,
    emit,
    voice: voiceSignal,
    setPrefs,
    relayout: refresh,
    snapshot: () => ({
      degraded: false,
      started,
      enabled: current.enabled,
      preset: current.preset,
      renderer: current.preset === "kanzei" ? "svg" : "canvas",
      activity: activityNow(),
      busy: isBusy(clock()),
      placement: layout?.placement ?? null,
      capped: Boolean(layout?.capped),
      box: layout?.box ?? null,
      avoid: layout?.avoid ?? [],
      watermarkAlpha: layout?.capped && painter.kit ? watermarkAlpha(painter.kit.watermark, current.opacity) : null,
      comets: comets.length,
      points: painter.model?.points.length ?? 0,
      edges: painter.model?.edges.length ?? 0,
    }),
    destroy() {
      destroyed = true;
      brand.destroy();
      cancelFrame();
      if (watchTimer) clearTimeout(watchTimer);
      watchTimer = 0;
      if (layer) layer.width = layer.height = 0;
      layer = null;
      for (const name of events) document.removeEventListener(name, onChange);
      document.removeEventListener("kz:backdrop-settings", onPrefs);
      reducedQuery?.removeEventListener?.("change", onChange);
      themeObserver?.disconnect();
      resizeObserver?.disconnect();
      messageObserver?.disconnect();
    },
  };
}
