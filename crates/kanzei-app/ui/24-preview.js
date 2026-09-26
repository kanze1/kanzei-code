// UI2-0926 #8 网页预览停靠面板(docs/design/preview_pane.md §前端;用户原话「网页渲染的工具有吗？对标GPT的」)。
//
// 面板本体是 Tauri 子 webview(原生窗口,后端 preview_* 命令),这里只管它周围的 HTML:
//   · 开关与持久化(ui_layout.preview:open/recent/device/scheme/preserve_log/console_open,宽度走 splits.preview);
//   · 地址栏规范化(5173 / :3000/x / localhost:8080 / https / 项目路径)、后退/前进/刷新/停止、设备与深浅色;
//   · 位置同步:#preview-host 的矩形(CSS px = Tauri 逻辑像素,应用不缩放)经 preview_set_bounds 交给后端,
//     ResizeObserver(占位框/对话视图/主区)+ 窗口尺寸 + 侧栏开合都会触发,按帧合并;
//   · 可见性:只在对话视图、面板开着、页面活着、没有错误页、窄屏时停在「预览」页签才显示;每次上报都带当前线 processId,
//     后端据此决定代理的 browser 走面板还是无头(面板可见且绑定本线才走面板);
//   · 遮挡冻结:原生面板永远画在 HTML 之上。订阅 00-surface 的 onSurfaceChange,栈里有模态、或任一弹层矩形与占位框相交,
//     先截一帧放进 #preview-freeze 再隐藏原生面板;不再遮挡时去抖恢复。后台任务侧栏的抽屉盖过来也算;
//   · 控制台(≤500 条、按级别筛、url:line:col、错误计数角标)、空态(本地开发服务 + 最近 5 个地址)、
//     连接被拒的错误页(「服务没在跑？」+ 看后台进程)、批注模式(点选元素 → 输入框附件 + 【网页批注】)、
//     写文件后自动刷新本项目的静态页、Ctrl+Shift+B、rail 开关;
//   · 对话里的入口:工具截图缩略图、交付卡片的图片缩略图与「预览」、html/svg 代码块「预览」、localhost 链接进面板。
// 纯函数(normalizeAddress / fitDevice / extractToolImages / previewColumnFor …)导出给冒烟直接测。
import { closeSurface, isModalOpen, onSurfaceChange, openMenu, surfaceElements } from "./00-surface.js";
import { installSplit } from "./00-frame.js";
import { $, confirmDialog, defer, invoke, on } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeProcessId, currentProject, log, navigate_view, toast } from "./03-shell.js";
import { layoutPref, onLayoutChange, setLayoutPref } from "./03-layout.js";
import { addMarkdownHook } from "./04-markdown.js";
import { openTasksPanel, reconcileTasksPanel } from "./06-agent-panel.js";
import { addPickAttachment, addPngAttachment } from "./08-compose-runtime.js";
import { openRuntimeImage } from "./15-views-misc.js";
import { filesDoc } from "./17-files-editor.js";

export const PREVIEW_MIN = 360;
export const PREVIEW_CHAT_MIN = 420;
export const PREVIEW_NARROW = 800;
export const PREVIEW_DEFAULT_RATIO = 0.48;
export const PREVIEW_CONSOLE_MAX = 500;
export const PREVIEW_RECENT_MAX = 5;
export const PREVIEW_RELOAD_DEBOUNCE_MS = 300;
export const PREVIEW_UNFREEZE_MS = 120;
export const PREVIEW_CAPTURE_TIMEOUT_MS = 5000;
/// 设备预设(CSS px);fill = 占满面板。后端按「设备 × 缩放」居中摆放并 set_zoom,冻结帧按同一口径摆位。
export const PREVIEW_DEVICES = { fill: null, phone: [390, 844], tablet: [768, 1024], desktop: [1280, 800] };
const DEVICE_ORDER = ["fill", "phone", "tablet", "desktop"];
const SCHEMES = ["auto", "light", "dark"];
const WRITE_TOOLS = new Set(["write", "edit", "insert", "multiedit", "apply_patch"]);
const CODE_PREVIEW_LANGS = new Set(["html", "htm", "svg", "xhtml"]);
const DELIVER_IMAGE_TYPES = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp", bmp: "image/bmp", svg: "image/svg+xml" };
const TOOL_IMAGE_REL = /^\.kanzei\/artifacts\/tool-images\/[\w.-]+\.png$/i;

// ---------- 纯函数 ----------
function decodeSafe(text) {
  try { return decodeURIComponent(text); } catch { return text; }
}
/// 地址栏输入 → { kind: "url"|"path"|"invalid", target, display }。url 交给后端直接导航;path(项目相对或绝对路径)
/// 由后端换成 127.0.0.1 静态服务的地址(远程源,没有 IPC)。后端还会再校验一遍,这里只负责「说人话的补全」。
export function normalizeAddress(raw) {
  const text = String(raw ?? "").trim().replace(/^["'<]+|["'>]+$/g, "").trim();
  const url = (target) => ({ kind: "url", target, display: target });
  const path = (target) => ({ kind: "path", target, display: target });
  if (!text) return { kind: "invalid", reason: "empty" };
  if (/^\d{2,5}$/.test(text)) return url(`http://localhost:${text}/`);
  let m = text.match(/^:(\d{2,5})([/?#].*)?$/);
  if (m) return url(`http://localhost:${m[1]}${m[2]?.startsWith("/") ? m[2] : `/${m[2] ?? ""}`}`);
  m = text.match(/^(localhost|127\.0\.0\.1|\[::1\]|0\.0\.0\.0)(:\d{2,5})?([/?#].*)?$/i);
  if (m) {
    const host = m[1].toLowerCase() === "0.0.0.0" ? "localhost" : m[1].toLowerCase();
    return url(`http://${host}${m[2] ?? ""}${m[3]?.startsWith("/") ? m[3] : `/${m[3] ?? ""}`}`);
  }
  m = text.match(/^(https?):\/\/([^/?#\s]+)(.*)$/i);
  if (m) return url(`${m[1].toLowerCase()}://${m[2]}${m[3] ? (m[3].startsWith("/") ? m[3] : `/${m[3]}`) : "/"}`);
  if (/^about:blank$/i.test(text)) return url("about:blank");
  if (/^data:text\/html[,;]/i.test(text)) return url(text);
  m = text.match(/^file:\/\/\/?(.+)$/i);
  if (m) {
    const rest = decodeSafe(m[1]);
    return path(/^[A-Za-z]:[\\/]/.test(rest) ? rest : `/${rest.replace(/^\/+/, "")}`);
  }
  // 其它 scheme(javascript:、tauri:、ftp:…)一律不认;盘符路径 C:\ 不算 scheme。
  if (/^[a-z][\w+.-]*:/i.test(text) && !/^[A-Za-z]:[\\/]/.test(text)) return { kind: "invalid", reason: "scheme" };
  const domainLike = /^[\w-]+(?:\.[\w-]+)+(?::\d{2,5})?(?:[/?#].*)?$/;
  if (/^[A-Za-z]:[\\/]/.test(text) || /^\.{0,2}[\\/]/.test(text) || /\.(?:html?|xhtml|svg)$/i.test(text)) return path(text);
  if (/[\\/]/.test(text) && !domainLike.test(text)) return path(text);
  if (domainLike.test(text) && !/\s/.test(text)) {
    m = text.match(/^([^/?#]+)(.*)$/);
    return url(`https://${m[1]}${m[2] ? (m[2].startsWith("/") ? m[2] : `/${m[2]}`) : "/"}`);
  }
  return { kind: "invalid", reason: "unknown" };
}
/// 本机地址(localhost / 127.0.0.1 / [::1]):对话里点这类链接直接在面板打开。
export function isLocalPreviewUrl(url) {
  return /^https?:\/\/(?:localhost|127\.0\.0\.1|\[::1\])(?::\d+)?(?:[/?#]|$)/i.test(String(url ?? "").trim());
}
/// 静态服务的项目文件地址:http://127.0.0.1:<port>/t/<token>/r/<root>/<rel>(片段是 /s/<id>.html)。
export function isStaticServerUrl(url) {
  return /^http:\/\/127\.0\.0\.1:\d+\/t\/[^/?#]+\/r\//i.test(String(url ?? ""));
}
/// 地址栏与控制台里显示的地址:静态服务地址去掉随机 token,只留项目相对路径(token 每次启动都变,对人没有意义)。
export function displayUrl(url) {
  const text = String(url ?? "");
  let m = text.match(/^http:\/\/127\.0\.0\.1:\d+\/t\/[^/?#]+\/r\/[^/?#]+\/([^?#]*)(.*)$/i);
  if (m) return `${decodeSafe(m[1]) || "./"}${m[2]}`;
  m = text.match(/^http:\/\/127\.0\.0\.1:\d+\/t\/[^/?#]+\/s\//i);
  if (m) return t("(代码片段)");
  return text;
}
/// 设备模式的摆位(与后端 fit_device 同一口径):scale = min(1, 宽比, 高比),居中。fill 或未知预设 = 整个占位框。
export function fitDevice(host, preset) {
  const box = { x: Number(host?.x) || 0, y: Number(host?.y) || 0, w: Math.max(0, Number(host?.w) || 0), h: Math.max(0, Number(host?.h) || 0) };
  const size = PREVIEW_DEVICES[preset];
  if (!size || !box.w || !box.h) return { ...box, scale: 1 };
  const [dw, dh] = size;
  const scale = Math.min(1, box.w / dw, box.h / dh);
  const w = dw * scale;
  const h = dh * scale;
  return { x: box.x + (box.w - w) / 2, y: box.y + (box.h - h) / 2, w, h, scale };
}
/// 预览栏宽度(与 style.css 的 clamp(360px, var(--kz-split-preview, 48%), max(360px, 100% - 420px)) 同一口径);
/// 对话视图窄于 800px 时预览占满、没有并排的栏,返回 0。
export function previewColumnFor(viewWidth, splitPx = null) {
  const view = Number(viewWidth) || 0;
  if (view < PREVIEW_NARROW) return 0;
  const want = Number(splitPx) > 0 ? Number(splitPx) : view * PREVIEW_DEFAULT_RATIO;
  return Math.round(Math.min(Math.max(want, PREVIEW_MIN), Math.max(PREVIEW_MIN, view - PREVIEW_CHAT_MIN)));
}
/// 工具正文里的截图标记行(后端 persist_tool_images 追加):`[tool-image] .kanzei/artifacts/tool-images/<sha>.png`。
/// 返回标记里的相对路径与去掉标记之后的正文(摘要器与展开区只看后者)。
export function extractToolImages(content) {
  const text = typeof content === "string" ? content : "";
  if (!text.includes("[tool-image]")) return { images: [], content: text };
  const images = [];
  const kept = [];
  for (const line of text.split(/\r?\n/)) {
    const m = line.match(/^\[tool-image\]\s+(\S.*?)\s*$/);
    const rel = m ? m[1].replace(/\\/g, "/") : "";
    if (m && TOOL_IMAGE_REL.test(rel)) {
      if (!images.includes(rel)) images.push(rel);
    } else {
      kept.push(line);
    }
  }
  if (!images.length) return { images, content: text };
  return { images, content: kept.join("\n").replace(/\n+$/, "") };
}
/// browser 工具结果 → 「在预览中打开」的目标。走面板的结果不给(用户已经看着它);path 与 html 优先于输出里的 url
/// (静态服务地址带每次启动都变的 token,历史回放时早已失效)。
export function browserPreviewTarget(input, content) {
  const text = typeof content === "string" ? content : "";
  if (/^backend:\s*pane/m.test(text)) return null;
  const src = input && typeof input === "object" && !Array.isArray(input) ? input : {};
  if (typeof src.path === "string" && src.path.trim()) return { kind: "path", target: src.path.trim() };
  if (typeof src.html === "string" && src.html.trim()) return { kind: "html", html: src.html };
  const url = (typeof src.url === "string" && src.url.trim()) || text.match(/\burl:\s*([^\s)]+)/i)?.[1] || "";
  return url && /^(?:https?:|about:blank)/i.test(url) ? { kind: "url", target: url } : null;
}
/// 控制台条目的类别:error / warn / network / info(按级别与文本判;后端的级别名大小写不一)。
export function consoleKind(entry) {
  const level = String(entry?.level ?? "").toLowerCase();
  const text = String(entry?.text ?? "");
  if (/net|network|request/.test(level) || /\bnet::ERR_|Failed to load resource|\bHTTP [45]\d\d\b/.test(text)) return "network";
  if (/^(?:err|error|exception|assert|fatal)/.test(level)) return "error";
  if (/^warn/.test(level)) return "warn";
  return "info";
}
export function consoleMatches(entry, level) {
  if (!level || level === "all") return true;
  return entry.kind === level || (level === "error" && entry.kind === "network" && /error/i.test(String(entry.level)));
}

// ---------- 状态 ----------
const state = {
  open: false, // 停靠面板开着(用户意图,持久化)
  alive: false, // 后端面板已创建(preview_open 成功过,且没有被 preview_close)
  url: "", title: "", loading: false, canBack: false, canForward: false,
  device: "fill", scheme: "auto", error: null,
  narrow: false, tab: "preview", // 对话视图窄于 800px:预览占满,工具栏出现「对话 | 预览」
  consoleOpen: false, level: "all", preserveLog: false,
  picking: false, staticProject: null,
};
const consoleEntries = [];
const consoleSeqs = new Set();
let consoleErrors = 0;
let sent = { visible: null, processId: undefined };
let lastBounds = null;
let frozen = false;
let freezePending = false;
let freezeGen = 0;
let holdFreeze = false;
let unfreezeTimer = null;
let evaluateQueued = false;
let boundsQueued = false;
let reloadTimer = null;
let splitApi = null;
let userToggled = false;

function currentView() {
  return document.body?.dataset?.view || "chat";
}
function el(tag, className = "", text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}
function warn(what, err) {
  log(`${t("网页预览")}:${what} ${err}`, "warn");
}
function round2(value) {
  return Math.round(Number(value) * 100) / 100;
}
function viewWidth() {
  return $("view-chat")?.getBoundingClientRect?.().width || 0;
}
function hostRect() {
  const r = $("preview-host")?.getBoundingClientRect?.();
  if (!r || !(r.width > 0) || !(r.height > 0)) return null;
  return { x: round2(r.left), y: round2(r.top), w: round2(r.width), h: round2(r.height) };
}
function sameRect(a, b) {
  return Boolean(a && b) && a.x === b.x && a.y === b.y && a.w === b.w && a.h === b.h;
}
function rectsIntersect(host, r) {
  if (!host || !r || !(r.width > 0) || !(r.height > 0)) return false;
  return r.left < host.x + host.w && r.right > host.x && r.top < host.y + host.h && r.bottom > host.y;
}
function withTimeout(promise, ms) {
  let timer = null;
  return Promise.race([
    promise.finally(() => clearTimeout(timer)),
    new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("timeout")), ms); }),
  ]);
}

/// 冒烟与调试:面板此刻的状态快照。
export function previewState() {
  return { ...state, sent: { ...sent }, frozen, bounds: lastBounds ? { ...lastBounds } : null, consoleCount: consoleEntries.length, consoleErrors };
}
/// 预览此刻占住的列宽(06-agent-panel 的停靠判据用):面板关着、不在对话视图时为 0。
export function previewColumnWidth(width) {
  if (!state.open || currentView() !== "chat") return 0;
  return previewColumnFor(width, splitApi?.value?.() ?? null);
}

// ---------- 布局 ----------
function measureNarrow() {
  const width = viewWidth();
  if (width > 0) state.narrow = width < PREVIEW_NARROW;
}
function syncSafeRight() {
  const root = document.documentElement?.style;
  if (!root) return;
  const shown = state.open && currentView() === "chat" && !state.narrow;
  const rect = shown ? $("preview-dock")?.getBoundingClientRect?.() : null;
  const px = rect && rect.width > 0 ? Math.max(0, Math.round((window.innerWidth || 0) - rect.left)) : 0;
  if (px > 0) root.setProperty("--surface-safe-right", `${px}px`);
  else root.removeProperty("--surface-safe-right");
}
function applyLayout() {
  const view = $("view-chat");
  if (!view) return;
  if (state.open) view.dataset.preview = "open";
  else delete view.dataset.preview;
  if (state.open && state.narrow) view.dataset.previewNarrow = state.tab;
  else delete view.dataset.previewNarrow;
  syncSafeRight();
  syncChrome();
}
function onLayoutResize() {
  const wasNarrow = state.narrow;
  measureNarrow();
  if (wasNarrow !== state.narrow) {
    if (!state.narrow) state.tab = "preview";
    applyLayout();
  } else {
    syncSafeRight();
  }
  scheduleBounds();
  scheduleEvaluate();
}

// ---------- 位置与可见性 ----------
function scheduleBounds() {
  if (boundsQueued) return;
  boundsQueued = true;
  requestAnimationFrame(() => {
    boundsQueued = false;
    sendBounds();
  });
}
function sendBounds() {
  if (!state.open || !state.alive) return;
  const rect = hostRect();
  if (!rect) return;
  if (frozen) placeFreeze();
  if (sameRect(lastBounds, rect)) return;
  lastBounds = rect;
  void invoke("preview_set_bounds", rect).catch((err) => warn("preview_set_bounds", err));
}
function sendVisible(visible) {
  const processId = activeProcessId ?? null;
  if (sent.visible === visible && sent.processId === processId) return;
  sent = { visible, processId };
  if (visible) sendBounds();
  void invoke("preview_set_visible", { visible, processId }).catch((err) => warn("preview_set_visible", err));
}
/// 原生面板该不该露出来(不算遮挡):开着、页面活着、没有错误页、在对话视图、窄屏时停在「预览」页签。
function baseVisible() {
  return state.open && state.alive && !state.error && currentView() === "chat" && !(state.narrow && state.tab === "chat");
}
/// 有没有 HTML 浮层压在占位框上:模态(整窗遮罩)一定算;其余弹层按矩形相交判;后台任务侧栏的抽屉态也算。
function isOccluded() {
  if (isModalOpen()) return true;
  const host = hostRect();
  if (!host) return false;
  for (const { el: node } of surfaceElements()) {
    if (rectsIntersect(host, node?.getBoundingClientRect?.())) return true; // 相交判定
  }
  const drawer = $("tasks-panel");
  return Boolean(drawer && !drawer.classList.contains("hidden") && drawer.dataset.dock === "drawer" && rectsIntersect(host, drawer.getBoundingClientRect?.()));
}
function scheduleEvaluate() {
  if (evaluateQueued) return;
  evaluateQueued = true;
  requestAnimationFrame(() => {
    evaluateQueued = false;
    evaluate();
  });
}
/// 可见性与冻结的唯一决策点(按帧合并调用)。
function evaluate() {
  if (holdFreeze) return;
  if (!baseVisible()) {
    cancelFreeze();
    showFreeze(false);
    frozen = false;
    if (state.alive) sendVisible(false);
    return;
  }
  if (isOccluded()) {
    clearTimeout(unfreezeTimer);
    unfreezeTimer = null;
    if (!frozen && !freezePending) void startFreeze();
    return;
  }
  if (freezePending) cancelFreeze();
  if (frozen) {
    if (unfreezeTimer) return;
    unfreezeTimer = setTimeout(() => {
      unfreezeTimer = null;
      if (holdFreeze || !baseVisible() || isOccluded()) {
        scheduleEvaluate();
        return;
      }
      frozen = false;
      showFreeze(false);
      sendVisible(true);
    }, PREVIEW_UNFREEZE_MS);
    return;
  }
  sendVisible(true);
}
function cancelFreeze() {
  freezeGen += 1;
  freezePending = false;
}
async function capture(options = {}) {
  try {
    const shot = await withTimeout(invoke("preview_capture", options), PREVIEW_CAPTURE_TIMEOUT_MS + 1000);
    return shot && typeof shot.png === "string" && shot.png ? shot : null;
  } catch (err) {
    warn("preview_capture", err);
    return null;
  }
}
/// 冻结:先截当前画面(面板可见时才截得到,隐藏态截图会挂起)放进 #preview-freeze,再隐藏原生面板。
/// force = 自家工具栏的菜单:菜单还没开就先冻住,开出来时不会先被原生面板盖一下。
async function startFreeze({ force = false } = {}) {
  const gen = ++freezeGen;
  freezePending = true;
  const shot = sent.visible === true ? await capture() : null;
  if (gen !== freezeGen) return false;
  freezePending = false;
  if (!baseVisible() || (!force && !isOccluded())) {
    scheduleEvaluate();
    return false;
  }
  frozen = true;
  setFreezeImage(shot);
  showFreeze(true);
  sendVisible(false);
  return true;
}
function setFreezeImage(shot) {
  const img = $("preview-freeze");
  if (!img) return;
  if (shot?.png) img.setAttribute("src", `data:image/png;base64,${shot.png}`);
  else img.removeAttribute("src");
  placeFreeze();
}
function placeFreeze() {
  const img = $("preview-freeze");
  const stage = $("preview-stage")?.getBoundingClientRect?.();
  const host = hostRect();
  if (!img?.style || !stage || !host) return;
  const fit = fitDevice({ x: host.x - stage.left, y: host.y - stage.top, w: host.w, h: host.h }, state.device);
  img.style.left = `${round2(fit.x)}px`;
  img.style.top = `${round2(fit.y)}px`;
  img.style.width = `${round2(fit.w)}px`;
  img.style.height = `${round2(fit.h)}px`;
}
function showFreeze(on) {
  $("preview-freeze")?.classList.toggle("hidden", !on);
}
/// 自家工具栏开菜单:先冻结再开(不等 onSurfaceChange 那一帧),关掉后由 evaluate 恢复。
async function withFrozen(fn) {
  holdFreeze = true;
  try {
    if (baseVisible() && sent.visible === true && !frozen) await startFreeze({ force: true });
    fn();
  } finally {
    holdFreeze = false;
    scheduleEvaluate();
  }
}

// ---------- 打开 / 关闭 ----------
function recentList() {
  const value = layoutPref("preview", "recent");
  return Array.isArray(value) ? value.filter((item) => typeof item === "string" && item).slice(0, PREVIEW_RECENT_MAX) : [];
}
function rememberRecent(display) {
  const list = recentList().filter((item) => item !== display);
  list.unshift(display);
  setLayoutPref("preview", "recent", list.slice(0, PREVIEW_RECENT_MAX));
  renderRecent();
}
function setOpen(open, { persist = true } = {}) {
  if (state.open === open) return;
  state.open = open;
  if (persist) {
    userToggled = true;
    setLayoutPref("preview", "open", open);
  }
  if (open) {
    measureNarrow();
    state.tab = "preview";
    if (!state.alive) void refreshDevUrls();
    renderRecent();
  } else if (state.picking) {
    void setPicking(false);
  }
  applyLayout();
  syncToggle();
  reconcileTasksPanel();
  splitApi?.sync?.();
  scheduleBounds();
  scheduleEvaluate();
}
/// 打开面板(不导航):rail、Ctrl+Shift+B、命令面板。
export function openPreviewDock() {
  if (currentView() !== "chat") navigate_view("chat");
  if (!state.open) setOpen(true);
  else if (state.narrow && state.tab !== "preview") setTab("preview");
}
export function closePreviewDock() {
  setOpen(false);
}
/// rail 开关与 Ctrl+Shift+B:关着(或不在对话视图)就打开;窄屏停在「对话」页签时切回预览;否则收起(页面保留)。
export function togglePreview() {
  if (!state.open || currentView() !== "chat") {
    openPreviewDock();
    return;
  }
  if (state.narrow && state.tab === "chat") {
    setTab("preview");
    return;
  }
  closePreviewDock();
}
function setTab(tab) {
  state.tab = tab === "chat" ? "chat" : "preview";
  applyLayout();
  scheduleBounds();
  scheduleEvaluate();
}
/// 在面板里打开一个目标(地址栏、开发服务列表、最近地址、工具卡片、交付卡片、代码块、localhost 链接、文件页)。
export async function openPreviewTarget(input, { record = true } = {}) {
  const target = normalizeAddress(input);
  if (target.kind === "invalid") {
    toast(`${t("无法识别的地址")}:${String(input ?? "").trim()}`, { kind: "warn" });
    return false;
  }
  if (currentView() !== "chat") navigate_view("chat");
  if (!state.open) setOpen(true);
  if (state.narrow && state.tab !== "preview") setTab("preview");
  setAddress(target.display);
  state.error = null;
  const bounds = hostRect() ?? lastBounds ?? { x: 0, y: 0, w: 0, h: 0 };
  const processId = activeProcessId ?? null;
  try {
    await invoke("preview_open", { target: target.target, processId, bounds });
  } catch (err) {
    state.error = { kind: "open", text: String(err) };
    renderError();
    syncChrome();
    toast(`${t("网页预览打开失败")}:${err}`, { kind: "warn" });
    return false;
  }
  state.alive = true;
  if (bounds.w > 0) lastBounds = bounds;
  // 已有面板时 preview_open 只导航,不保证把被隐藏(错误页/冻结)的面板重新露出来:可见性不做假设,下一帧 evaluate 明确上报一次。
  sent = { visible: null, processId: undefined };
  if (target.kind === "path") state.staticProject = currentProject;
  if (state.device !== "fill" || state.scheme !== "auto") sendDevice();
  if (record) rememberRecent(target.display);
  renderError();
  syncChrome();
  scheduleEvaluate();
  return true;
}
/// 释放页面(「更多」菜单):后端关掉子 webview,回到起始页。
async function releasePage() {
  cancelFreeze();
  frozen = false;
  showFreeze(false);
  try { await invoke("preview_close"); } catch (err) { warn("preview_close", err); }
  state.alive = false;
  state.url = "";
  state.title = "";
  state.error = null;
  state.loading = false;
  state.picking = false;
  sent = { visible: null, processId: undefined };
  lastBounds = null;
  resetConsole();
  setAddress("");
  renderError();
  syncChrome();
  void refreshDevUrls();
}
function navPreview(action) {
  if (!state.alive) return Promise.resolve();
  return invoke("preview_nav", { action }).catch((err) => warn(`preview_nav ${action}`, err));
}
function sendDevice() {
  void invoke("preview_device", { preset: state.device, scheme: state.scheme }).catch((err) => warn("preview_device", err));
}
function setDevice(preset) {
  if (!DEVICE_ORDER.includes(preset)) return;
  state.device = preset;
  setLayoutPref("preview", "device", preset);
  if (state.alive) sendDevice();
  if (frozen) placeFreeze();
  syncChrome();
}
function setScheme(scheme) {
  if (!SCHEMES.includes(scheme)) return;
  state.scheme = scheme;
  setLayoutPref("preview", "scheme", scheme);
  if (state.alive) sendDevice();
  syncChrome();
}

// ---------- 后端事件 ----------
function onPreviewState(payload) {
  const p = payload && typeof payload === "object" ? payload : {};
  const previousUrl = state.url;
  if (typeof p.url === "string") state.url = p.url;
  if (typeof p.title === "string") state.title = p.title;
  state.loading = Boolean(p.loading);
  state.canBack = Boolean(p.canBack);
  state.canForward = Boolean(p.canForward);
  if (DEVICE_ORDER.includes(p.device)) state.device = p.device;
  if (SCHEMES.includes(p.scheme)) state.scheme = p.scheme;
  state.error = p.error && typeof p.error === "object" ? { kind: String(p.error.kind || "other"), text: String(p.error.text || "") } : null;
  if (state.url !== previousUrl) {
    // 本项目的静态页:写文件后自动刷新只认它(开发服务靠自己的 HMR)。
    state.staticProject = isStaticServerUrl(state.url) ? (state.staticProject ?? currentProject) : null;
    if (!state.preserveLog) resetConsole({ refetch: true });
  }
  setAddress(displayUrl(state.url));
  renderError();
  syncChrome();
  scheduleEvaluate();
}
function onPreviewPick(payload) {
  const p = payload && typeof payload === "object" ? payload : {};
  state.picking = false;
  syncChrome();
  addPickAttachment({ png: p.png, url: displayUrl(state.url), selector: p.selector, text: p.text, tag: p.tag });
  toast(t("批注已放进输入框,补一句修改意见就能发送"));
}
async function setPicking(on) {
  if (on && !state.alive) return;
  state.picking = Boolean(on);
  syncChrome();
  try {
    await invoke("preview_pick", { on: state.picking });
  } catch (err) {
    state.picking = false;
    syncChrome();
    toast(`${t("批注模式启动失败")}:${err}`, { kind: "warn" });
  }
}

// ---------- 写文件后自动刷新 ----------
function isProjectStaticUrl(url) {
  return isStaticServerUrl(url) && state.staticProject === currentProject;
}
/// 07-events 的 kz:tool-end 处理器调用:本项目的静态页开着时,write/edit/insert 成功后防抖 300ms 刷新一次。
export function previewNoteToolEnd(payload) {
  if (!payload?.ok || !WRITE_TOOLS.has(payload.name) || !state.alive || !isProjectStaticUrl(state.url)) return;
  clearTimeout(reloadTimer);
  reloadTimer = setTimeout(() => { reloadTimer = null; if (state.alive && isProjectStaticUrl(state.url)) void navPreview("reload"); }, PREVIEW_RELOAD_DEBOUNCE_MS);
}
/// 09-sessions 切线时调用:可见性上报带的 processId 跟着换(后端据此把代理的 browser 路由到面板)。
export function previewLineSync() {
  scheduleEvaluate();
}

// ---------- 控制台 ----------
function resetConsole({ refetch = false } = {}) {
  consoleEntries.length = 0;
  consoleSeqs.clear();
  consoleErrors = 0;
  renderConsole();
  syncConsoleBadge();
  if (refetch && state.alive) void fetchConsole();
}
async function fetchConsole() {
  try {
    const result = await invoke("preview_console", { sinceSeq: 0 });
    addConsoleEntries(result?.entries);
  } catch (err) {
    warn("preview_console", err);
  }
}
export function addConsoleEntries(entries) {
  let added = false;
  for (const raw of Array.isArray(entries) ? entries : []) {
    const seq = Number(raw?.seq);
    if (!Number.isFinite(seq) || consoleSeqs.has(seq)) continue;
    const entry = {
      seq,
      ts: Number(raw.ts) || 0,
      level: String(raw.level ?? "log"),
      text: String(raw.text ?? ""),
      url: String(raw.url ?? ""),
      line: Number(raw.line) || 0,
      col: Number(raw.col) || 0,
    };
    entry.kind = consoleKind(entry);
    consoleEntries.push(entry);
    consoleSeqs.add(seq);
    if (entry.kind === "error") consoleErrors += 1;
    added = true;
  }
  if (!added) return;
  consoleEntries.sort((a, b) => a.seq - b.seq);
  while (consoleEntries.length > PREVIEW_CONSOLE_MAX) {
    const old = consoleEntries.shift();
    consoleSeqs.delete(old.seq);
    if (old.kind === "error") consoleErrors -= 1;
  }
  renderConsole();
  syncConsoleBadge();
}
/// 条目来源:{ full: 完整地址:行:列(悬停看), short: 文件名:行:列(列里显示;长地址截尾会把行号截掉) }。
export function consoleWhere(entry) {
  if (!entry?.url) return { full: "", short: "" };
  const where = displayUrl(entry.url);
  const pos = entry.line > 0 ? `:${entry.line}${entry.col > 0 ? `:${entry.col}` : ""}` : "";
  const bare = where.replace(/[?#].*$/, "").replace(/\/+$/, "");
  const name = bare.split("/").pop() || bare;
  return { full: `${where}${pos}`, short: `${name}${pos}` };
}
function consoleRow(entry) {
  const row = el("div", "pv-log");
  row.dataset.level = entry.kind;
  row.append(el("span", "pv-log-text", entry.text));
  const where = consoleWhere(entry);
  if (where.full) {
    const src = el("span", "pv-log-src", where.short);
    src.title = where.full;
    row.append(src);
  }
  return row;
}
function renderConsole() {
  const list = $("preview-console-list");
  if (!list) return;
  if (!state.consoleOpen) {
    list.replaceChildren();
    return;
  }
  const shown = consoleEntries.filter((entry) => consoleMatches(entry, state.level));
  list.replaceChildren(...shown.map(consoleRow));
  if (!shown.length) list.append(el("div", "pv-log-empty", consoleEntries.length ? t("这个级别没有记录") : t("还没有控制台输出")));
  list.scrollTop = list.scrollHeight;
  for (const button of $("preview-console")?.querySelectorAll?.("[data-level]") ?? []) {
    button.setAttribute("aria-pressed", String(button.dataset.level === state.level));
  }
}
function syncConsoleBadge() {
  const badge = $("preview-console-badge");
  const toggle = $("preview-console-toggle");
  const count = Math.max(0, consoleErrors);
  if (badge) {
    const text = count > 99 ? "99+" : String(count);
    badge.textContent = count ? text : "";
    badge.classList.toggle("hidden", !count);
  }
  if (toggle) {
    const label = count ? `${t("控制台")} · ${count} ${t("个错误")}` : t("控制台");
    if (toggle.getAttribute("aria-label") !== label) toggle.setAttribute("aria-label", label);
  }
}
function setConsoleOpen(open) {
  state.consoleOpen = Boolean(open);
  setLayoutPref("preview", "console_open", state.consoleOpen);
  $("preview-console")?.classList.toggle("hidden", !state.consoleOpen);
  if (state.consoleOpen && state.alive && !consoleEntries.length) void fetchConsole();
  renderConsole();
  syncChrome();
  scheduleBounds();
}

// ---------- 界面同步 ----------
function setAddress(text) {
  const input = $("preview-address");
  if (!input || document.activeElement === input) return;
  input.value = text ?? "";
}
function syncToggle() {
  const toggle = $("preview-toggle");
  if (!toggle) return;
  const on = state.open && currentView() === "chat";
  toggle.classList.toggle("active", on);
  toggle.setAttribute("aria-expanded", String(on));
}
function deviceLabel(preset) {
  const size = PREVIEW_DEVICES[preset];
  const name = { fill: t("自适应"), phone: t("手机"), tablet: t("平板"), desktop: t("桌面") }[preset] ?? preset;
  return size ? `${name} ${size[0]}×${size[1]}` : name;
}
function schemeLabel(scheme) {
  return { auto: t("跟随系统"), light: t("浅色"), dark: t("深色") }[scheme] ?? scheme;
}
function setDisabled(id, disabled) {
  const node = $(id);
  if (node) node.disabled = Boolean(disabled);
}
function syncChrome() {
  setDisabled("preview-back", !state.alive || !state.canBack);
  setDisabled("preview-forward", !state.alive || !state.canForward);
  setDisabled("preview-reload", !state.alive);
  setDisabled("preview-pick", !state.alive);
  const reload = $("preview-reload");
  if (reload) {
    const key = state.loading ? "停止加载" : "重新加载";
    if (reload.dataset.i18nAriaLabel !== key) {
      reload.dataset.i18nAriaLabel = key;
      reload.dataset.i18nTitle = key;
      reload.setAttribute("aria-label", t(key));
      reload.title = t(key);
    }
    reload.dataset.loading = state.loading ? "1" : "0";
  }
  const pick = $("preview-pick");
  if (pick) pick.setAttribute("aria-pressed", String(state.picking));
  const consoleToggle = $("preview-console-toggle");
  if (consoleToggle) consoleToggle.setAttribute("aria-pressed", String(state.consoleOpen));
  const device = $("preview-device");
  if (device) {
    const label = `${t("视口与配色")}:${deviceLabel(state.device)} · ${schemeLabel(state.scheme)}`;
    if (device.getAttribute("aria-label") !== label) {
      device.setAttribute("aria-label", label);
      device.title = label;
    }
    device.dataset.device = state.device;
  }
  const stage = $("preview-stage");
  if (stage) stage.dataset.device = state.device;
  for (const [id, tab] of [["preview-tab-chat", "chat"], ["preview-tab-preview", "preview"]]) {
    $(id)?.setAttribute("aria-pressed", String(state.tab === tab));
  }
  $("preview-empty")?.classList.toggle("hidden", state.alive);
  syncToggle();
}
function renderError() {
  const box = $("preview-error");
  if (!box) return;
  const error = state.alive || state.error?.kind === "open" ? state.error : null;
  box.classList.toggle("hidden", !error);
  if (!error) return;
  const where = displayUrl(state.url);
  const copy = {
    connection_refused: [t("服务没在跑？"), `${where ? `${where} ` : ""}${t("拒绝连接。开发服务可能还没启动,或者已经退出。")}`],
    unsafe_port: [t("这个端口被浏览器禁止"), t("浏览器不允许访问这个端口(1、6000 这类保留端口),换一个端口再试。")],
    blocked: [t("这个地址被拦下了"), t("预览面板不打开应用内部地址、file: 与脚本链接;项目里的文件直接输入路径即可。")],
    open: [t("网页预览打开失败"), ""],
  }[error.kind] ?? [t("页面打不开"), ""];
  const title = $("preview-error-title");
  if (title) title.textContent = copy[0];
  const text = $("preview-error-text");
  if (text) text.textContent = copy[1];
  const code = $("preview-error-code");
  if (code) {
    code.textContent = error.text;
    code.classList.toggle("hidden", !error.text);
  }
  $("preview-error-tasks")?.classList.toggle("hidden", error.kind !== "connection_refused");
  $("preview-error-retry")?.classList.toggle("hidden", error.kind === "open" || error.kind === "blocked");
}
function listButton(main, sub, onClick) {
  const button = el("button", "pv-item");
  button.type = "button";
  button.append(el("span", "pv-item-main", main));
  if (sub) button.append(el("span", "pv-item-sub", sub));
  button.title = sub ? `${main}\n${sub}` : main;
  button.addEventListener("click", onClick);
  return button;
}
function renderRecent() {
  const list = $("preview-recent-list");
  if (!list) return;
  const items = recentList();
  list.replaceChildren(...items.map((item) => listButton(item, "", () => void openPreviewTarget(item))));
  $("preview-recent")?.classList.toggle("hidden", !items.length);
}
function renderDevUrls(urls) {
  const list = $("preview-dev-list");
  if (!list) return;
  const items = (Array.isArray(urls) ? urls : []).filter((item) => typeof item?.url === "string" && item.url);
  list.replaceChildren(...items.map((item) => listButton(item.url, String(item.command ?? ""), () => void openPreviewTarget(item.url))));
  $("preview-dev-none")?.classList.toggle("hidden", items.length > 0);
}
async function refreshDevUrls() {
  const project = currentProject;
  if (!project) {
    renderDevUrls([]);
    return;
  }
  let result = null;
  try {
    result = await invoke("preview_dev_urls", { projectDir: project });
  } catch (err) {
    warn("preview_dev_urls", err);
  }
  if (project !== currentProject) return;
  renderDevUrls(result?.urls);
}

// ---------- 菜单 ----------
function openDeviceMenu(anchor) {
  void withFrozen(() => openMenu(anchor, [
    { heading: t("视口") },
    ...DEVICE_ORDER.map((preset) => ({ label: deviceLabel(preset), checked: state.device === preset, onSelect: () => setDevice(preset) })),
    "separator",
    { heading: t("配色") },
    ...SCHEMES.map((scheme) => ({ label: schemeLabel(scheme), checked: state.scheme === scheme, onSelect: () => setScheme(scheme) })),
  ], { placement: "bottom-end", label: t("视口与配色") }));
}
function openMoreMenu(anchor) {
  const dead = !state.alive;
  void withFrozen(() => openMenu(anchor, [
    { label: t("截图放进输入框"), disabled: dead, onSelect: () => void captureToChat() },
    { label: t("在系统浏览器打开"), disabled: dead, onSelect: () => void invoke("preview_open_external").catch((err) => warn("preview_open_external", err)) },
    { label: t("打开开发者工具"), disabled: dead, onSelect: () => void invoke("preview_open_devtools").catch((err) => warn("preview_open_devtools", err)) },
    { label: t("保留日志"), desc: t("换页时不清空控制台"), checked: state.preserveLog, onSelect: () => setPreserveLog(!state.preserveLog) },
    "separator",
    { label: t("清除本站数据"), desc: t("只清当前网站的 Cookie 与存储"), danger: true, disabled: dead, onSelect: () => void clearSiteData() },
    { label: t("关闭页面"), desc: t("释放页面进程,回到起始页"), disabled: dead, onSelect: () => void releasePage() },
  ], { placement: "bottom-end", label: t("更多") }));
}
function setPreserveLog(on) {
  state.preserveLog = Boolean(on);
  setLayoutPref("preview", "preserve_log", state.preserveLog);
}
async function clearSiteData() {
  const ok = await confirmDialog({
    title: t("清除本站数据"),
    message: `${t("清除当前网站的 Cookie、localStorage 与缓存,登录状态会丢失;kanzei 自己的设置不受影响。")}\n${displayUrl(state.url)}`,
    okText: t("清除"),
    danger: true,
  });
  if (!ok) return;
  try {
    await invoke("preview_clear_site_data");
    toast(t("已清除本站数据"), { kind: "ok" });
  } catch (err) {
    toast(`${t("清除本站数据失败")}:${err}`, { kind: "warn" });
  }
}
async function captureToChat() {
  const shot = await capture();
  if (!shot) {
    toast(t("截图失败"), { kind: "warn" });
    return;
  }
  addPngAttachment(`preview-shot-${Date.now()}.png`, shot.png);
  toast(t("截图已放进输入框附件"));
}

// ---------- 对话里的入口 ----------
const toolImageCache = new Map();
const TOOL_IMAGE_CACHE_MAX = 48;
function cachedImage(key, load) {
  if (toolImageCache.has(key)) return toolImageCache.get(key);
  const promise = load().then((src) => {
    if (!src) toolImageCache.delete(key);
    return src;
  }, () => {
    toolImageCache.delete(key);
    return null;
  });
  toolImageCache.set(key, promise);
  if (toolImageCache.size > TOOL_IMAGE_CACHE_MAX) toolImageCache.delete(toolImageCache.keys().next().value);
  return promise;
}
/// 工具截图(.kanzei/artifacts/tool-images/<sha>.png)→ data URL;按项目 + 路径缓存,失败不缓存(下次重试)。
export function loadToolImage(rel, projectDir = currentProject) {
  return cachedImage(`tool\n${projectDir}\n${rel}`, async () => {
    const result = await invoke("tool_image", { projectDir, rel });
    return typeof result?.png === "string" && result.png ? `data:image/png;base64,${result.png}` : null;
  });
}
function loadDeliveredImage(path, projectDir = currentProject) {
  const ext = String(path).toLowerCase().match(/\.([a-z0-9]+)$/)?.[1] ?? "png";
  return cachedImage(`deliver\n${projectDir}\n${path}`, async () => {
    const result = await invoke("delivered_image", { projectDir, path });
    return typeof result?.png === "string" && result.png ? `data:${DELIVER_IMAGE_TYPES[ext] ?? "image/png"};base64,${result.png}` : null;
  });
}
let lazyObserver = null;
/// 缩略图进视口才取图(历史里几十张截图不一次性全拉);没有 IntersectionObserver 的环境直接取。
function whenVisible(node, fn) {
  if (typeof IntersectionObserver !== "function") {
    fn();
    return;
  }
  lazyObserver ??= new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      lazyObserver.unobserve(entry.target);
      entry.target._kzLoad?.();
    }
  }, { rootMargin: "200px" });
  node._kzLoad = fn;
  lazyObserver.observe(node);
}
function thumbButton(label, load, title) {
  const button = el("button", "pv-thumb");
  button.type = "button";
  button.setAttribute("aria-label", label);
  button.title = label;
  const img = el("img", "pv-thumb-img");
  img.alt = "";
  button.append(img);
  whenVisible(button, () => {
    void load().then((src) => {
      if (src) {
        img.setAttribute("src", src);
        button.classList.add("is-ready");
      } else {
        button.classList.add("is-missing");
        button.append(el("span", "pv-thumb-missing", t("图片不可用")));
      }
    });
  });
  button.addEventListener("click", async (event) => {
    event.stopPropagation?.();
    const src = await load();
    if (src) openRuntimeImage(title, src);
    else toast(t("图片不可用(可能已被清理)"), { kind: "warn" });
  });
  return button;
}
function previewActionButton(target) {
  const button = el("button", "ghost mini pv-open-in");
  button.type = "button";
  button.textContent = t("在预览中打开");
  button.addEventListener("click", (event) => {
    event.stopPropagation?.();
    if (target.kind === "html") void previewSnippet(target.html, button);
    else void openPreviewTarget(target.target);
  });
  return button;
}
/// 05-chat-render 的 fillToolBlock 调用:工具行下方的截图缩略图(点开进查看器)与 browser 的「在预览中打开」。
/// 挂在行头之后、折叠详情之前:不展开也看得见模型看到的画面。
export function mountToolShots(block, images, { action = null } = {}) {
  const wrap = block?.wrap;
  if (!wrap || (!images?.length && !action)) return null;
  const strip = el("div", "tool-shots");
  images.slice(0, 4).forEach((rel, index) => {
    strip.append(thumbButton(`${t("查看截图")} ${index + 1}`, () => loadToolImage(rel), `${t("截图")} · ${block.name ?? ""}`));
  });
  if (action) strip.append(previewActionButton(action));
  if (block.detail?.parentNode === wrap) wrap.insertBefore(strip, block.detail);
  else wrap.append(strip);
  if (images?.length) wrap.classList.add("has-shot");
  return strip;
}
/// 06-activity 的 renderFileCard 调用:交付的图片显示缩略图(兑现 deliver 说明里的「图片可内联预览」),.html 加「预览」。
export function decorateFileCard(card, actions, display) {
  const path = String(display?.path ?? "");
  const ext = path.toLowerCase().match(/\.([a-z0-9]+)$/)?.[1] ?? "";
  if (DELIVER_IMAGE_TYPES[ext]) {
    const name = display.name || path;
    const thumb = thumbButton(`${t("查看图片")} ${name}`, () => loadDeliveredImage(path), name);
    thumb.classList.add("file-card-thumb");
    card.insertBefore(thumb, actions);
  }
  if (ext === "html" || ext === "htm") {
    const button = el("button", "ghost mini file-card-preview", t("预览"));
    button.type = "button";
    button.setAttribute("aria-label", `${t("在网页预览中打开")} ${display.name || path}`);
    button.addEventListener("click", () => void openPreviewTarget(path));
    actions.prepend(button);
  }
}
async function previewSnippet(html, invoker) {
  // 查看器(模态)里的代码块:先关掉查看器,否则面板一出来就被模态冻结。
  const dialog = invoker?.closest?.("dialog");
  if (dialog?.open) closeSurface(dialog);
  try {
    const result = await invoke("preview_snippet", { html: String(html ?? "") });
    if (!result?.url) throw new Error(t("没有拿到片段地址"));
    await openPreviewTarget(result.url, { record: false });
  } catch (err) {
    toast(`${t("预览失败")}:${err?.message ?? err}`, { kind: "warn" });
  }
}
/// renderMarkdownInto 的钩子:闭合的 ```html / ```svg 代码块右上角加「预览」(流式写到一半的 data-open 围栏不加)。
export function decorateCodeBlocks(root) {
  for (const pre of root?.querySelectorAll?.("pre.code") ?? []) {
    if (pre.dataset?.open === "true" || pre.getAttribute?.("data-open") === "true" || pre.dataset?.kzPreview === "1") continue;
    const code = pre.querySelector?.("code");
    const lang = String(code?.className ?? "").match(/language-([\w+-]+)/)?.[1]?.toLowerCase() ?? "";
    if (!CODE_PREVIEW_LANGS.has(lang)) continue;
    pre.dataset.kzPreview = "1";
    pre.classList.add("has-preview");
    const button = el("button", "ghost mini code-preview", t("预览"));
    button.type = "button";
    button.setAttribute("aria-label", t("在网页预览中打开这段代码"));
    button.addEventListener("click", (event) => {
      event.stopPropagation?.();
      void previewSnippet(code?.textContent ?? "", button);
    });
    pre.append(button);
  }
}

// ---------- 接线 ----------
function adoptPrefs() {
  const device = layoutPref("preview", "device");
  if (DEVICE_ORDER.includes(device)) state.device = device;
  const scheme = layoutPref("preview", "scheme");
  if (SCHEMES.includes(scheme)) state.scheme = scheme;
  state.preserveLog = layoutPref("preview", "preserve_log") === true;
  const consoleOpen = layoutPref("preview", "console_open") === true;
  if (consoleOpen !== state.consoleOpen) {
    state.consoleOpen = consoleOpen;
    $("preview-console")?.classList.toggle("hidden", !consoleOpen);
  }
  // 启动时按上次的开合恢复(只恢复面板与起始页,不自动加载上次的页面:开发服务可能已经不在了)。本机 localStorage
  // 重启即丢(D-404),真值是稍后到达的后端 ui_layout:用户本次启动还没动过开关之前,每次偏好到达都按它对齐。
  const wantOpen = layoutPref("preview", "open") === true;
  if (!userToggled && wantOpen !== state.open) setOpen(wantOpen, { persist: false });
  renderRecent();
  syncChrome();
}
function wireToolbar() {
  $("preview-toggle")?.addEventListener("click", () => togglePreview());
  $("preview-close")?.addEventListener("click", () => closePreviewDock());
  $("preview-back")?.addEventListener("click", () => void navPreview("back"));
  $("preview-forward")?.addEventListener("click", () => void navPreview("forward"));
  $("preview-reload")?.addEventListener("click", () => void navPreview(state.loading ? "stop" : "reload"));
  $("preview-device")?.addEventListener("click", (event) => openDeviceMenu(event?.currentTarget ?? $("preview-device")));
  $("preview-more")?.addEventListener("click", (event) => openMoreMenu(event?.currentTarget ?? $("preview-more")));
  $("preview-pick")?.addEventListener("click", () => void setPicking(!state.picking));
  $("preview-console-toggle")?.addEventListener("click", () => setConsoleOpen(!state.consoleOpen));
  $("preview-console-close")?.addEventListener("click", () => setConsoleOpen(false));
  $("preview-console-clear")?.addEventListener("click", () => {
    resetConsole();
    if (state.alive) void invoke("preview_console_clear").catch((err) => warn("preview_console_clear", err));
  });
  for (const button of $("preview-console")?.querySelectorAll?.("[data-level]") ?? []) {
    button.addEventListener("click", () => {
      state.level = button.dataset.level || "all";
      renderConsole();
    });
  }
  $("preview-tab-chat")?.addEventListener("click", () => setTab("chat"));
  $("preview-tab-preview")?.addEventListener("click", () => setTab("preview"));
  $("preview-dev-refresh")?.addEventListener("click", () => void refreshDevUrls());
  $("preview-error-retry")?.addEventListener("click", () => void navPreview("reload"));
  $("preview-error-tasks")?.addEventListener("click", (event) => openTasksPanel({ invoker: event?.currentTarget ?? null }));
  $("files-open-preview")?.addEventListener("click", () => {
    const path = filesDoc?.path;
    if (path) void openPreviewTarget(path);
  });
  const address = $("preview-address");
  // 地址栏自己的键:Enter 打开;Esc 放弃编辑、恢复当前地址(局部 Esc,挂在输入框自己身上)。
  address?.addEventListener("keydown", (event) => {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault?.();
      void openPreviewTarget(address.value);
    } else if (event.key === "Escape" && !state.picking) {
      address.value = displayUrl(state.url);
      address.blur?.();
    }
  });
  // 面板里的 Esc:批注模式下取消点选(焦点在工具栏上时;焦点在页面里时由后端回传)。
  $("preview-dock")?.addEventListener("keydown", (event) => {
    if (event.key !== "Escape" || !state.picking) return;
    event.preventDefault?.();
    event.stopPropagation?.();
    void setPicking(false);
  });
}

defer(() => {
  addMarkdownHook((root) => decorateCodeBlocks(root));
  wireToolbar();
  splitApi = installSplit($("preview-dock"), {
    id: "preview",
    side: "left",
    min: PREVIEW_MIN,
    max: () => Math.max(PREVIEW_MIN, Math.round(viewWidth() - PREVIEW_CHAT_MIN)),
    title: t("拖动调整预览宽度 · 双击复位"),
    titleKey: "拖动调整预览宽度 · 双击复位",
    ariaLabel: t("调整网页预览宽度"),
    ariaKey: "调整网页预览宽度",
    onChange: () => {
      syncSafeRight();
      scheduleBounds();
      reconcileTasksPanel();
    },
  });
  installSplit($("preview-console"), {
    id: "preview-console",
    side: "top",
    min: 96,
    max: () => Math.max(96, Math.round(($("preview-dock")?.getBoundingClientRect?.().height || 600) * 0.7)),
    title: t("拖动调整控制台高度 · 双击复位"),
    titleKey: "拖动调整控制台高度 · 双击复位",
    ariaLabel: t("调整控制台高度"),
    ariaKey: "调整控制台高度",
    onChange: () => scheduleBounds(),
  });
  on("kz:preview-state", (event) => onPreviewState(event.payload));
  on("kz:preview-console", (event) => addConsoleEntries(event.payload?.entries));
  on("kz:preview-pick", (event) => onPreviewPick(event.payload));
  onSurfaceChange(() => scheduleEvaluate());
  onLayoutChange((section) => {
    if (section === "*") adoptPrefs();
  });
  document.addEventListener("kz:view-changed", () => { applyLayout(); scheduleBounds(); scheduleEvaluate(); });
  document.addEventListener("kz:tasks-layout", () => { syncSafeRight(); scheduleBounds(); scheduleEvaluate(); });
  document.addEventListener("kz:language", () => { syncChrome(); renderConsole(); syncConsoleBadge(); renderError(); });
  // 对话里的 localhost 链接:在面板打开(Ctrl/⌘+点击照旧交给系统)。
  document.addEventListener("click", (event) => {
    if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const link = event.target?.closest?.("a[href]");
    if (!link || !link.closest?.("#messages")) return;
    const href = link.getAttribute?.("href") ?? "";
    if (!isLocalPreviewUrl(href)) return;
    event.preventDefault?.();
    void openPreviewTarget(href);
  });
  // Ctrl/⌘+Shift+B:开关网页预览(与 ChatGPT 桌面端一致)。模态开着时让路。
  window.addEventListener("keydown", (event) => {
    if (isModalOpen() || !(event.ctrlKey || event.metaKey) || !event.shiftKey || event.altKey) return;
    if (String(event.key).toLowerCase() !== "b") return;
    event.preventDefault?.();
    togglePreview();
  });
  if (typeof ResizeObserver === "function") {
    const observer = new ResizeObserver(() => onLayoutResize());
    for (const id of ["preview-host", "view-chat", "main"]) {
      const node = $(id);
      if (node) observer.observe(node);
    }
  }
  window.addEventListener("resize", () => onLayoutResize());
  adoptPrefs();
  applyLayout();
});
