// 可移动/可调尺寸的框与分隔条的唯一入口(零 import)。设计见 docs/design/ui_surface_stack.md §4.6。
//
// 两种东西,同一套规矩:
// ① 框(frame):弹窗(<dialog class="k-dialog">)与停靠卡片(.k-card)。在 index.html 上写 data-kz-frame*
//    属性,启动时 bindFrames 接线;拖标题栏移动、拖边/角调尺寸、双击标题栏复位、框内 Alt+Shift+方向键
//    调尺寸、Alt+Shift+Home 复位。菜单/浮层/提示/toast 一律不是框(ui-surface-rules 门禁 H)。
// ② 分隔条(split):布局两栏之间的一条边(侧栏宽、文件树宽、记忆列表宽、日志面板高、后台任务侧栏宽),
//    installSplit。
//
// 几何只写成 CSS 变量 + data-kz-placed 令牌,落位规则在 surface.css §10(框)与 style.css(分隔条);
// 不写元素内联 width/left:内联尺寸会压过 .collapsed 这类状态规则(侧栏收起后留空栏的缺陷)。
// 零 import:样例页与假 DOM 冒烟要能单独加载;需要翻译的文案由调用方传入。
// 持久化经可注入的存储(setFrameStore):应用里接到 ui_prefs 的 ui_layout(本机 WebView2 的 localStorage
// 重启即丢,D-404);样例页与冒烟用默认的 localStorage 存储。

const MARGIN = 8; // 框离视口边的最小留白:骑边手柄外露 6px,留 8px 手柄也还在视口里
const KEY_STEP = 24;
const SPLIT_KEY_STEP = 8;
const MOVE_THRESHOLD = 3; // 按下后移动超过 3px 才算拖动,否则是普通点击(不捕获、不改 click 目标)
const EDGES_ALL = ["n", "e", "s", "w", "ne", "se", "sw", "nw"];
// 拖动区里这些元素照常工作,按下它们不开始移动。
const INTERACTIVE = "button, a[href], input, select, textarea, label, summary, [contenteditable='true'], [contenteditable=''], [role='button'], [role='menuitem'], [role='tab'], [role='option'], [data-kz-no-drag]";

const hasDocument = typeof document !== "undefined" && document !== null;
const frames = new Set();
const splits = new Set();

function isInside(node, container) {
  for (let n = node; n; n = n.parentNode) if (n === container) return true;
  return false;
}
const clamp = (value, lo, hi) => Math.min(Math.max(value, lo), Math.max(lo, hi));
const viewport = () => ({ vw: window.innerWidth, vh: window.innerHeight });

// ---------- 存储 ----------
// 接口:get(kind, id, hint) → 值或 null;set(kind, id, value|null)。kind 是 "frames" 或 "splits";
// hint 是分隔条的旧 localStorage 键(侧栏沿用 kz-sidebar-width,保住已存的宽度)。
// 读写一律吞异常:隐私模式、配额、缩略图环境里放弃持久化,本次会话照常可用。
function localGet(key) {
  try { return localStorage.getItem(key); } catch { return null; }
}
function localSet(key, value) {
  try {
    if (value == null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch { /* 放弃持久化 */ }
}
export const localFrameStore = {
  get(kind, id, hint) {
    if (kind === "frames") return localGet(`kz-frame:${id}`);
    return localGet(hint || `kz-split:${id}`);
  },
  set(kind, id, value, hint) {
    if (kind === "frames") localSet(`kz-frame:${id}`, value == null ? null : JSON.stringify(value));
    else localSet(hint || `kz-split:${id}`, value == null ? null : String(value));
  },
};
let store = localFrameStore;
function storeGet(kind, id, hint) {
  try { return store.get(kind, id, hint); } catch { return null; }
}
function storeSet(kind, id, value, hint) {
  try { store.set(kind, id, value, hint); } catch { /* 放弃持久化 */ }
}
/// 换存储(应用启动时接到 ui_prefs);之后调 refreshFrames() 按新存储重放一次。
export function setFrameStore(next) {
  store = next && typeof next.get === "function" && typeof next.set === "function" ? next : localFrameStore;
}

// ---------- 纯函数(假 DOM 冒烟直接测) ----------
// ① 拖某条边(或整体移动)后的新矩形(视口坐标)。先分支 move:"move" 里含字母 e。
export function dragRect(start, edge, dx, dy, { minW, minH, vw, vh }) {
  const maxW = vw - 2 * MARGIN;
  const maxH = vh - 2 * MARGIN;
  const mw = Math.min(minW, maxW);
  const mh = Math.min(minH, maxH);
  if (edge === "move") {
    return {
      left: clamp(start.left + dx, MARGIN, vw - MARGIN - start.width),
      top: clamp(start.top + dy, MARGIN, vh - MARGIN - start.height),
      width: start.width,
      height: start.height,
    };
  }
  let left = start.left;
  let top = start.top;
  let right = start.left + start.width;
  let bottom = start.top + start.height;
  if (edge.includes("e")) right = clamp(right + dx, left + mw, vw - MARGIN);
  if (edge.includes("w")) left = clamp(left + dx, MARGIN, right - mw);
  if (edge.includes("s")) bottom = clamp(bottom + dy, top + mh, vh - MARGIN);
  if (edge.includes("n")) top = clamp(top + dy, MARGIN, bottom - mh);
  return { left, top, width: right - left, height: bottom - top };
}

// ② 矩形 → 偏好。每轴锚在离得近的那条视口边(右半边的框记 r,窗口变宽时仍贴右);
//    stretchY 的框同时记 t 与 b,高度跟窗口走;只有被拖过的轴才记显式尺寸 w/h,
//    没拖过的轴尺寸仍归原 CSS/内容(权限卡的高度随内容)。kw/kh 是已知尺寸,只用于夹紧。
export function prefFromRect(rect, { vw, vh, stretchY = false, sized = {}, prev = null }) {
  const round = Math.round;
  const pref = { v: 1 };
  if (rect.left + rect.width / 2 > vw / 2) pref.r = round(vw - rect.left - rect.width);
  else pref.l = round(rect.left);
  if (stretchY) {
    pref.t = round(rect.top);
    pref.b = round(vh - rect.top - rect.height);
  } else if (rect.top + rect.height / 2 > vh / 2) pref.b = round(vh - rect.top - rect.height);
  else pref.t = round(rect.top);
  if (sized.w || prev?.w != null) pref.w = round(rect.width);
  if (!stretchY && (sized.h || prev?.h != null)) pref.h = round(rect.height);
  pref.kw = round(rect.width);
  pref.kh = round(rect.height);
  return pref;
}

// ③ 偏好 → 落地值(按当前视口夹紧;偏好本身不改,窗口变回来时恢复用户摆的位置)。
export function placeFromPref(pref, { vw, vh, minW, minH, stretchY = false }) {
  const maxW = vw - 2 * MARGIN;
  const maxH = vh - 2 * MARGIN;
  const out = { tokens: ["pos"], vars: {} };
  const w = pref.w != null ? clamp(pref.w, Math.min(minW, maxW), maxW) : Math.min(pref.kw ?? 0, maxW);
  const left = clamp(pref.l != null ? pref.l : vw - (pref.r ?? MARGIN) - w, MARGIN, vw - MARGIN - w);
  if (pref.l != null) out.vars.l = left;
  else out.vars.r = vw - left - w;
  if (pref.w != null) {
    out.vars.w = w;
    out.tokens.push("w");
  }
  if (stretchY) {
    const t = clamp(pref.t ?? MARGIN, MARGIN, vh - MARGIN - minH);
    out.vars.t = t;
    out.vars.b = clamp(pref.b ?? MARGIN, MARGIN, vh - t - minH);
    return out;
  }
  const h = pref.h != null ? clamp(pref.h, Math.min(minH, maxH), maxH) : Math.min(pref.kh ?? 0, maxH);
  const top = clamp(pref.t != null ? pref.t : vh - (pref.b ?? MARGIN) - h, MARGIN, vh - MARGIN - h);
  if (pref.t != null) out.vars.t = top;
  else out.vars.b = vh - top - h;
  if (pref.h != null) {
    out.vars.h = h;
    out.tokens.push("h");
  }
  return out;
}

// 读到的偏好不合法(旧版本、手改、截断)一律当作没有:回到默认摆放,不抛错。接受 JSON 串或对象。
export function parsePref(raw) {
  if (!raw) return null;
  let pref = raw;
  if (typeof raw === "string") {
    try { pref = JSON.parse(raw); } catch { return null; }
  }
  if (!pref || typeof pref !== "object" || pref.v !== 1) return null;
  for (const k of ["l", "r", "t", "b", "w", "h", "kw", "kh"]) {
    if (pref[k] != null && !(Number.isFinite(pref[k]) && pref[k] >= 0 && pref[k] < 20000)) return null;
  }
  if ((pref.l == null) === (pref.r == null)) return null;
  if (pref.t == null && pref.b == null) return null;
  return pref;
}

// ---------- 框 ----------
function writePlacement(el, placement) {
  for (const k of ["l", "r", "t", "b", "w", "h"]) {
    if (placement && placement.vars[k] != null) el.style.setProperty(`--kz-frame-${k}`, `${Math.round(placement.vars[k])}px`);
    else el.style.removeProperty(`--kz-frame-${k}`);
  }
  if (placement) el.setAttribute("data-kz-placed", placement.tokens.join(" "));
  else el.removeAttribute("data-kz-placed");
}
function parseEdges(value) {
  const raw = String(value ?? "").trim();
  if (!raw) return [];
  if (raw === "all") return EDGES_ALL.slice();
  return raw.split(/\s+/).filter((edge) => EDGES_ALL.includes(edge));
}
function optionsFrom(el, opts) {
  const d = el.dataset ?? {};
  const min = String(opts.min ?? d.kzFrameMin ?? "").trim().split(/\s+/).map(Number);
  return {
    id: opts.id ?? d.kzFrame ?? "",
    move: opts.move ?? d.kzFrameMove ?? null,
    edges: parseEdges(opts.edges ?? d.kzFrameEdges),
    minW: min[0] > 0 ? min[0] : 280,
    minH: min[1] > 0 ? min[1] : 160,
    stretchY: (opts.stretch ?? d.kzFrameStretch) === "y",
    // keep="w":移动时也记下当前宽度(默认宽度随锚点走的框,挪开后宽度不该跳回收缩宽度)。
    keepW: String(opts.keep ?? d.kzFrameKeep ?? "").split(/\s+/).includes("w"),
    persist: opts.persist ?? d.kzFramePersist !== "no",
  };
}
function readPref(frame) {
  return frame.o.persist ? parsePref(storeGet("frames", frame.o.id)) : frame.sessionPref;
}
function savePref(frame, pref) {
  if (frame.o.persist) storeSet("frames", frame.o.id, pref ?? null);
  else frame.sessionPref = pref;
}
function limits(frame) {
  return { ...viewport(), minW: frame.o.minW, minH: frame.o.minH, stretchY: frame.o.stretchY };
}
function apply(frame) {
  const pref = readPref(frame);
  writePlacement(frame.el, pref ? placeFromPref(pref, limits(frame)) : null);
}
function frameOf(target) {
  return target?._kzFrame ?? (target?.el ? target : null);
}
// 回到 CSS 默认摆放(居中的弹窗、贴在输入区上方的权限卡),并忘掉记住的位置与尺寸。
export function resetFrame(target) {
  const frame = frameOf(target);
  if (!frame) return;
  savePref(frame, null);
  writePlacement(frame.el, null);
}
function rectOf(el) {
  const r = el.getBoundingClientRect();
  return { left: r.left, top: r.top, width: r.width, height: r.height };
}

function begin(frame, event, edge, captureTarget) {
  frame.drag = {
    edge,
    id: event.pointerId,
    x: event.clientX,
    y: event.clientY,
    start: rectOf(frame.el),
    sized: {
      w: edge === "move" ? frame.o.keepW : /[ew]/.test(edge),
      h: edge !== "move" && /[ns]/.test(edge),
    },
    prev: readPref(frame),
    pref: null,
  };
  try { captureTarget.setPointerCapture(event.pointerId); } catch { /* 合成事件没有活动指针 */ }
  document.documentElement.dataset.kzFrameDrag = edge;
}
function track(frame, event) {
  const drag = frame.drag;
  if (!drag || event.pointerId !== drag.id) return;
  const lim = limits(frame);
  const rect = dragRect(drag.start, drag.edge, event.clientX - drag.x, event.clientY - drag.y, lim);
  drag.pref = prefFromRect(rect, { ...lim, sized: drag.sized, prev: drag.prev });
  writePlacement(frame.el, placeFromPref(drag.pref, lim));
}
function finishDrag(frame) {
  const drag = frame.drag;
  frame.drag = null;
  if (hasDocument) delete document.documentElement.dataset.kzFrameDrag;
  if (drag?.pref) savePref(frame, drag.pref);
}

// opts(与属性一一对应,属性优先级低于显式参数):
//   id      data-kz-frame          必填;持久化键 frames.<id>(默认存储为 localStorage kz-frame:<id>)
//   move    data-kz-frame-move     拖动区选择器(":scope" = 整个框);缺省 = 不可移动
//   edges   data-kz-frame-edges    "all" 或空格分隔的 n e s w ne se sw nw;缺省 = 不可调尺寸
//   min     data-kz-frame-min      "宽 高"(px),缺省 "280 160";最大恒为视口减 2×8px
//   stretch data-kz-frame-stretch  "y" = 纵向贴上下两边拉伸,高度跟窗口走
//   keep    data-kz-frame-keep     "w" = 移动时也记下当前宽度(默认宽度随锚点走的权限卡)
//   persist data-kz-frame-persist  "no" = 不记住,每次关闭复位(确认框/输入框);缺省记住
export function installFrame(el, opts = {}) {
  if (!el) return null;
  if (el._kzFrame) return el._kzFrame;
  const frame = { el, o: optionsFrom(el, opts), drag: null, pending: null, sessionPref: null, downInMove: false };
  if (!frame.o.id) return null;
  el._kzFrame = frame;
  frames.add(frame);
  // 显式参数也回写成属性:surface.css §10 按 [data-kz-frame] / [data-kz-frame-edges] 选中。
  el.setAttribute("data-kz-frame", frame.o.id);
  if (frame.o.edges.length) el.setAttribute("data-kz-frame-edges", frame.o.edges.join(" "));
  for (const edge of frame.o.edges) {
    const handle = document.createElement("div");
    handle.className = "k-frame-edge";
    handle.dataset.edge = edge;
    handle.setAttribute("aria-hidden", "true"); // 不进 Tab 序、不被读屏念;键盘走 Alt+Shift+方向键
    el.appendChild(handle);
  }
  if (frame.o.edges.length) {
    el.setAttribute("aria-keyshortcuts", "Alt+Shift+ArrowLeft Alt+Shift+ArrowRight Alt+Shift+ArrowUp Alt+Shift+ArrowDown Alt+Shift+Home");
  }
  // 拖动区挂标记,surface.css 据此给移动光标(整框可拖的 :scope 不标:正文照常是文字光标)。
  if (frame.o.move && frame.o.move !== ":scope") {
    for (const grip of el.querySelectorAll?.(frame.o.move) ?? []) grip.setAttribute("data-kz-frame-grip", "");
  }

  // 一次手势 = pointerdown 到 pointerup。过程中的 move/up 挂在 document 捕获阶段:
  // 甩得快时指针第一步就离开了框,只挂在框上会收不到,残留的「待移动」会被下一次悬停误触发;
  // 越过阈值之前不做指针捕获,否则普通点击的 click/dblclick 目标会被改成框本身。
  const onMove = (event) => {
    const pending = frame.pending;
    if (pending && !frame.drag && event.pointerId === pending.id) {
      if (Math.abs(event.clientX - pending.x) + Math.abs(event.clientY - pending.y) < MOVE_THRESHOLD) return;
      begin(frame, { pointerId: pending.id, clientX: pending.x, clientY: pending.y }, "move", el);
    }
    track(frame, event);
  };
  const onUp = (event) => {
    const id = frame.drag?.id ?? frame.pending?.id;
    if (id !== undefined && event.pointerId !== id) return;
    document.removeEventListener("pointermove", onMove, true);
    document.removeEventListener("pointerup", onUp, true);
    document.removeEventListener("pointercancel", onUp, true);
    frame.pending = null;
    if (frame.drag) finishDrag(frame);
  };
  const listen = () => {
    document.addEventListener("pointermove", onMove, true);
    document.addEventListener("pointerup", onUp, true);
    document.addEventListener("pointercancel", onUp, true);
  };
  el.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    // 上一次手势没等到 up(窗口失焦、系统吞了事件):先收尾,不带着旧状态开始新手势。
    if (frame.drag || frame.pending) onUp({ pointerId: frame.drag?.id ?? frame.pending.id });
    frame.downInMove = false;
    const handle = event.target?.closest?.(".k-frame-edge");
    if (handle && handle.parentNode === el) {
      event.preventDefault?.();
      begin(frame, event, handle.dataset.edge, handle);
      listen();
      return;
    }
    if (!frame.o.move || event.target?.closest?.(INTERACTIVE)) return;
    const region = frame.o.move === ":scope" ? el : event.target?.closest?.(frame.o.move);
    if (!region || !isInside(region, el)) return;
    // 点在 ::backdrop 上的事件 target 也是 dialog 本身:坐标不在框内的不算拖动区(拖遮罩不许挪弹窗)。
    const box = el.getBoundingClientRect();
    if (event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom) return;
    frame.pending = { id: event.pointerId, x: event.clientX, y: event.clientY };
    frame.downInMove = true;
    listen();
  });
  el.addEventListener("lostpointercapture", (event) => {
    if (frame.drag && event.pointerId === frame.drag.id) onUp(event);
  });
  // 双击拖动区复位。判据用按下时记的标记,不看 dblclick 的 target(拖动后捕获会改写它)。
  el.addEventListener("dblclick", () => {
    if (frame.downInMove) resetFrame(frame);
  });
  // 键盘:焦点在框内时 Alt+Shift+←→ 调宽、↑↓ 调高、Home 复位。挂在框自己身上,不挂 document。
  el.addEventListener("keydown", (event) => {
    if (!frame.o.edges.length || !event.altKey || !event.shiftKey || event.ctrlKey || event.metaKey) return;
    if (event.key === "Home") {
      event.preventDefault?.();
      resetFrame(frame);
      return;
    }
    const dir = { ArrowRight: [1, 0], ArrowLeft: [-1, 0], ArrowDown: [0, 1], ArrowUp: [0, -1] }[event.key];
    if (!dir || (dir[1] && frame.o.stretchY)) return;
    if (dir[0] && !frame.o.edges.some((edge) => /[ew]/.test(edge))) return;
    if (dir[1] && !frame.o.edges.some((edge) => /[ns]/.test(edge))) return;
    event.preventDefault?.();
    const lim = limits(frame);
    const start = rectOf(el);
    const rightHalf = start.left + start.width / 2 > lim.vw / 2;
    const edge = dir[0] ? (rightHalf ? "w" : "e") : "s";
    const dx = dir[0] * KEY_STEP * (rightHalf ? -1 : 1);
    const rect = dragRect(start, edge, dx, dir[1] * KEY_STEP, lim);
    savePref(frame, prefFromRect(rect, { ...lim, sized: { w: Boolean(dir[0]), h: Boolean(dir[1]) }, prev: readPref(frame) }));
    apply(frame);
  });
  // 不记住的框:关闭即复位(dialog 的 close;popover 的 toggle→closed)。
  if (!frame.o.persist) {
    el.addEventListener("close", () => resetFrame(frame));
    el.addEventListener("toggle", (event) => {
      if (event.newState === "closed") resetFrame(frame);
    });
  }
  apply(frame);
  return frame;
}

// 扫描 [data-kz-frame] 并接线。启动时调用一次;重复调用幂等。
export function bindFrames(root = hasDocument ? document : null) {
  for (const el of root?.querySelectorAll?.("[data-kz-frame]") ?? []) installFrame(el);
}
export function framePref(target) {
  const frame = frameOf(target);
  return frame ? readPref(frame) : null;
}

// ---------- 分隔条 ----------
// 尺寸写成 <html> 上的 CSS 变量 --kz-split-<id>(默认值在 style.css :root),视图 CSS 引用它;
// 手柄 .resize-handle 是 position:fixed、按窗格矩形同步(窗格本身可能是滚动容器,如 #sidebar)。
// opts: { id, side: "right"|"left"|"top"|"bottom", min, max(数字或返回数字的函数), key?(旧 localStorage 键),
//         title?, ariaLabel?, onChange?(px|null) }
export function installSplit(pane, { id, side = "right", min, max, key, title, ariaLabel, onChange } = {}) {
  if (!pane || !id || pane._kzSplit) return pane?._kzSplit ?? null;
  const cssVar = `--kz-split-${id}`;
  const rootStyle = document.documentElement.style;
  const vertical = side === "top" || side === "bottom";
  const num = (v) => (typeof v === "function" ? v() : v);
  const bound = (value) => Math.round(clamp(value, num(min), num(max)));
  pane.style.removeProperty?.(vertical ? "height" : "width"); // 旧版 setupResize 写过的内联尺寸
  const handle = document.createElement("div");
  handle.className = "resize-handle";
  handle.tabIndex = 0;
  handle.setAttribute("role", "separator");
  handle.setAttribute("aria-orientation", vertical ? "horizontal" : "vertical");
  if (title) handle.title = title;
  if (ariaLabel) handle.setAttribute("aria-label", ariaLabel);
  pane.appendChild(handle);
  const sync = () => {
    const r = pane.getBoundingClientRect();
    if (vertical) {
      handle.style.left = `${r.left}px`;
      handle.style.width = `${r.width}px`;
      handle.style.top = `${(side === "top" ? r.top : r.bottom) - 2}px`;
    } else {
      handle.style.top = `${r.top}px`;
      handle.style.height = `${r.height}px`;
      handle.style.left = `${(side === "right" ? r.right : r.left) - 2}px`;
    }
    handle.setAttribute("aria-valuemin", String(num(min)));
    handle.setAttribute("aria-valuemax", String(num(max)));
    handle.setAttribute("aria-valuenow", String(Math.round(vertical ? r.height : r.width)));
  };
  const stored = () => {
    const raw = storeGet("splits", id, key);
    const value = typeof raw === "number" ? raw : Number.parseInt(raw, 10);
    return Number.isFinite(value) && value > 0 ? value : null;
  };
  // 按存储重放(启动、换存储、窗口变化):存的是用户意图,落地值按当前上下限夹紧,不改写存储。
  const reapply = () => {
    const saved = stored();
    if (saved === null) rootStyle.removeProperty(cssVar);
    else rootStyle.setProperty(cssVar, `${bound(saved)}px`);
    sync();
  };
  const set = (value) => {
    const next = bound(value);
    rootStyle.setProperty(cssVar, `${next}px`);
    storeSet("splits", id, next, key);
    sync();
    onChange?.(next);
  };
  const reset = () => {
    rootStyle.removeProperty(cssVar);
    storeSet("splits", id, null, key);
    sync();
    onChange?.(null);
  };
  let dragging = false;
  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault?.();
    dragging = true;
    handle.classList.add("dragging");
    try { handle.setPointerCapture(event.pointerId); } catch { /* 合成事件 */ }
    document.documentElement.dataset.kzFrameDrag = vertical ? "row" : "col";
  });
  handle.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    const r = pane.getBoundingClientRect();
    if (side === "right") set(event.clientX - r.left);
    else if (side === "left") set(r.right - event.clientX);
    else if (side === "top") set(r.bottom - event.clientY);
    else set(event.clientY - r.top);
  });
  const stop = () => {
    dragging = false;
    handle.classList.remove("dragging");
    delete document.documentElement.dataset.kzFrameDrag;
  };
  handle.addEventListener("pointerup", stop);
  handle.addEventListener("pointercancel", stop);
  handle.addEventListener("lostpointercapture", stop);
  handle.addEventListener("dblclick", reset);
  handle.addEventListener("keydown", (event) => {
    if (event.key === "Home") {
      event.preventDefault?.();
      reset();
      return;
    }
    const grow = vertical ? { ArrowUp: side === "top", ArrowDown: side === "bottom" } : { ArrowRight: side === "right", ArrowLeft: side === "left" };
    if (!(event.key in grow)) return;
    event.preventDefault?.();
    const r = pane.getBoundingClientRect();
    const size = vertical ? r.height : r.width;
    set(size + (grow[event.key] ? SPLIT_KEY_STEP : -SPLIT_KEY_STEP));
  });
  // 窗格自身变尺寸、父容器变尺寸(侧栏开合让文件树整体左移但宽度不变)、窗口变化,都要重新对齐手柄。
  if (typeof ResizeObserver === "function") {
    const ro = new ResizeObserver(sync);
    ro.observe(pane);
    if (pane.parentElement) ro.observe(pane.parentElement);
  }
  reapply();
  const api = { id, set, reset, sync, reapply, value: stored, cssVar, key, pane, handle };
  pane._kzSplit = api;
  splits.add(api);
  return api;
}

/// 按当前存储重放全部框与分隔条(应用启动时后端偏好到达后调用一次;拖动中的框不动,等它自己收尾)。
export function refreshFrames() {
  for (const frame of frames) if (!frame.drag) apply(frame);
  for (const split of splits) split.reapply();
}

if (hasDocument && typeof window !== "undefined" && typeof window.addEventListener === "function") {
  // 窗口尺寸变了:所有框按偏好重新夹紧,分隔条按新上下限重放(都不改写偏好)。
  window.addEventListener("resize", () => {
    for (const frame of frames) if (!frame.drag) apply(frame);
    for (const split of splits) split.reapply();
  });
}
