function setupResize(elementId, key, side, min, max) {
  const element = $(elementId);
  if (!element) return;
  const saved = Number.parseInt(localStorage.getItem(key), 10);
  if (Number.isFinite(saved)) element.style.width = `${Math.min(max, Math.max(min, saved))}px`;
  const handle = document.createElement("div");
  handle.className = "resize-handle";
  handle.title = t("拖动调整面板宽度");
  handle.tabIndex = 0;
  handle.setAttribute("role", "separator");
  handle.setAttribute("aria-orientation", "vertical");
  handle.setAttribute("aria-label", t("调整面板宽度"));
  element.appendChild(handle);
  const syncHandle = () => {
    const rect = element.getBoundingClientRect();
    handle.style.top = `${rect.top}px`;
    handle.style.height = `${rect.height}px`;
    handle.style.left = `${(side === "right" ? rect.right : rect.left) - 2}px`;
  };
  const setWidth = (width) => {
    const next = Math.min(max, Math.max(min, Math.round(width)));
    element.style.width = `${next}px`;
    localStorage.setItem(key, String(next));
    syncHandle();
  };
  const resetWidth = () => {
    localStorage.removeItem(key);
    element.style.width = "";
    syncHandle();
  };
  syncHandle();
  if ("ResizeObserver" in window) new ResizeObserver(syncHandle).observe(element);
  window.addEventListener("resize", syncHandle);
  let dragging = false;
  handle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    dragging = true;
    handle.classList.add("dragging");
    handle.setPointerCapture(event.pointerId);
    document.body.style.cursor = "col-resize";
  });
  handle.addEventListener("pointermove", (event) => {
    if (!dragging) return;
    const rect = element.getBoundingClientRect();
    setWidth(side === "right" ? event.clientX - rect.left : rect.right - event.clientX);
  });
  handle.addEventListener("keydown", (event) => {
    if (!["ArrowLeft", "ArrowRight", "Home"].includes(event.key)) return;
    event.preventDefault();
    if (event.key === "Home") return resetWidth();
    const rect = element.getBoundingClientRect();
    const delta = side === "right" ? (event.key === "ArrowRight" ? 8 : -8) : (event.key === "ArrowLeft" ? 8 : -8);
    setWidth(rect.width + delta);
  });
  handle.addEventListener("dblclick", resetWidth);
  const stop = () => {
    dragging = false;
    handle.classList.remove("dragging");
    document.body.style.cursor = "";
  };
  handle.addEventListener("pointerup", stop);
  handle.addEventListener("pointercancel", stop);
  handle.addEventListener("lostpointercapture", stop);
}
setupResize("sidebar", "kz-sidebar-width", "right", 220, 460);
setupResize("todo-panel", "kz-todo-width", "left", 240, 520);
setupResize("bg-panel", "kz-activity-width", "left", 240, 520);
let activeProcessId = null;
let activeSessionId = null;
let processItems = [];

// R-086:每个会话独立的运行状态机。控制事件按 sessionId 更新对应状态机,视图只
// 投影活动会话的状态——后台会话的 idle/stopped 先落这里,切回时从状态机重建,
// 而不是依赖事件在"恰好活动"时才会被处理。待答队列另有 askQueues,不放这里。
const sessionStates = new Map();
function sessionState(sessionId) {
  let state = sessionStates.get(sessionId);
  if (!state) {
    state = { running: false, converged: false };
    sessionStates.set(sessionId, state);
  }
  return state;
}

let running = false;
let currentProject = null;
let currentAssistant = null;
let currentReasoning = null;
let attachments = [];
let lastRequest = null;
let runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };

// ---------- 视图切换 ----------
// 只绑带 data-view 的按钮:rail 上还有侧栏开合这类布局开关,它们不是视图。
document.querySelectorAll(".activity-item[data-view]").forEach((item) => {
  item.addEventListener("click", () => {
    document.querySelectorAll(".activity-item[data-view]").forEach((i) => {
      i.classList.remove("active");
      i.removeAttribute("aria-current");
    });
    item.classList.add("active");
    item.setAttribute("aria-current", "page");
    const view = item.dataset.view;
    document.body.classList.toggle("documents-active", view === "documents");
    document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
    $(`view-${view}`).classList.add("active");
    if (view === "settings") loadSettings();
    if (view === "workspace") refreshWorkspace();
    if (view === "documents") refreshDocs();
    if (view === "memory") refreshMemory();
    if (view === "metrics") refreshMetrics();
    if (view === "files") refreshFiles();
    if (view === "arch") refreshArch();
    if (view === "lines") refreshLines();
  });
});

// ---------- toast ----------
let toastTimer = null;
let errorRetry = null;
function toast(text) {
  const el = $("toast");
  const source = String(text);
  const translated = Object.prototype.hasOwnProperty.call(I18N_EN, source) ? t(source) : source;
  el.textContent = localizeDynamic(translated);
  el.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.add("hidden"), 2600);
}
function reportPersistentError(text, { retry = null } = {}) {
  log(text, "err");
  errorRetry = retry;
  $("log-retry").classList.toggle("hidden", typeof retry !== "function");
  $("log-panel").classList.remove("hidden");
}
function toastError(text, options = {}) {
  reportPersistentError(text, options);
}

let completionAudioContext = null;
const baseTitle = document.title;

function playRunNotice(kind) {
  try {
    const AudioCtor = window.AudioContext || window.webkitAudioContext;
    if (!AudioCtor) return;
    completionAudioContext ??= new AudioCtor();
    if (completionAudioContext.state === "suspended") completionAudioContext.resume().catch(() => {});
    const now = completionAudioContext.currentTime;
    const frequencies = kind === "failed" ? [220, 165] : kind === "stopped" ? [330] : [523, 659];
    frequencies.forEach((frequency, index) => {
      const oscillator = completionAudioContext.createOscillator();
      const gain = completionAudioContext.createGain();
      oscillator.frequency.value = frequency;
      oscillator.type = "sine";
      gain.gain.setValueAtTime(0.0001, now + index * 0.11);
      gain.gain.exponentialRampToValueAtTime(0.12, now + index * 0.11 + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + index * 0.11 + 0.1);
      oscillator.connect(gain).connect(completionAudioContext.destination);
      oscillator.start(now + index * 0.11);
      oscillator.stop(now + index * 0.11 + 0.11);
    });
  } catch (error) {
    log(`完成提示音不可用:${error}`, "warn");
  }
}

let notificationPermissionPrompted = false;
function explainNotificationFallback(message) {
  log(message, "warn");
  $("log-panel").classList.remove("hidden");
  toast(message);
}
async function ensureNotificationPermission() {
  if (notificationPermissionPrompted) return false;
  notificationPermissionPrompted = true;
  if (!("Notification" in window)) {
    explainNotificationFallback(t("当前环境不支持系统通知，完成提示将保留在应用内"));
    return false;
  }
  if (Notification.permission === "granted") return true;
  if (Notification.permission === "denied") {
    explainNotificationFallback(t("系统通知权限已拒绝，请在系统设置中允许后重试"));
    return false;
  }
  try {
    const permission = await Notification.requestPermission();
    if (permission !== "granted") explainNotificationFallback(t("系统通知权限未授予，完成提示将保留在应用内"));
    return permission === "granted";
  } catch (error) {
    explainNotificationFallback(`${t("系统通知权限请求失败")}:${error}`);
    return false;
  }
}

function notifyRunState(kind, text) {
  const labels = { completed: t("运行完成"), failed: t("运行失败"), stopped: t("运行已停止") };
  const label = labels[kind] || t("运行状态");
  toast(`${label}: ${text}`);
  playRunNotice(kind);
  if (!document.hasFocus() || document.hidden) {
    document.title = `🔔 ${label} · ${baseTitle}`;
    if ("Notification" in window && Notification.permission === "granted") {
      try {
        new Notification(label, { body: text, tag: "kanzei-run-state" });
      } catch (error) {
        log(`系统通知不可用:${error}`, "warn");
      }
    }
  }
}

function resetTitleOnFocus() {
  if (!document.hidden && document.hasFocus()) document.title = baseTitle;
}
document.addEventListener("visibilitychange", resetTitleOnFocus);
window.addEventListener("focus", resetTitleOnFocus);
let activityPanelOpen = localStorage.getItem("kz-activity-panel") === "1";

function syncActivityPanel() {
  $("bg-panel").classList.toggle("hidden", !activityPanelOpen);
  const toggle = $("activity-toggle");
  toggle.classList.toggle("active", activityPanelOpen);
  toggle.textContent = activityPanelOpen ? `${t("活动")} ✓` : t("活动");
  toggle.title = activityPanelOpen ? t("隐藏右侧活动面板") : t("显示右侧活动面板");
}

$("activity-toggle").addEventListener("click", () => {
  activityPanelOpen = !activityPanelOpen;
  localStorage.setItem("kz-activity-panel", activityPanelOpen ? "1" : "0");
  syncActivityPanel();
});
syncActivityPanel();

let sidebarCollapsed = localStorage.getItem("kz-sidebar-collapsed") === "1";
function syncSidebar() {
  const sidebar = $("sidebar");
  const toggle = $("sidebar-toggle");
  sidebar.classList.toggle("collapsed", sidebarCollapsed);
  toggle.classList.toggle("active", sidebarCollapsed);
  toggle.setAttribute("aria-expanded", sidebarCollapsed ? "false" : "true");
  toggle.title = localizeDynamic(sidebarCollapsed ? "展开左侧栏" : "折叠左侧栏");
  // rail 上的常驻开关与顶栏按钮同步同一状态:窄视口下侧栏悬浮盖住顶栏时,
  // rail 是唯一还能点到的开关(用户实测缩放后"没有关闭和打开")。
  const rail = $("rail-sidebar-toggle");
  if (rail) {
    rail.classList.toggle("active", !sidebarCollapsed);
    rail.setAttribute("aria-expanded", sidebarCollapsed ? "false" : "true");
    rail.title = localizeDynamic(sidebarCollapsed ? "打开侧栏" : "收起侧栏");
  }
}
function toggleSidebar() {
  sidebarCollapsed = !sidebarCollapsed;
  localStorage.setItem("kz-sidebar-collapsed", sidebarCollapsed ? "1" : "0");
  syncSidebar();
}
$("sidebar-toggle").addEventListener("click", toggleSidebar);
$("rail-sidebar-toggle")?.addEventListener("click", toggleSidebar);
// 悬浮模式(≤900px,缩放放大同样会触发)下侧栏盖在主区上:点侧栏外的任意
// 位置就收起,不再需要先找到被盖住的开关。
// matchMedia 在冒烟 harness 里不存在:回退成"永不悬浮",真实浏览器不受影响。
const sidebarOverlayQuery = typeof window.matchMedia === "function"
  ? window.matchMedia("(max-width: 900px)")
  : { matches: false };
document.addEventListener("pointerdown", (event) => {
  if (sidebarCollapsed || !sidebarOverlayQuery.matches) return;
  if (event.target.closest("#sidebar, #activitybar, #sidebar-toggle")) return;
  sidebarCollapsed = true;
  localStorage.setItem("kz-sidebar-collapsed", "1");
  syncSidebar();
});
syncSidebar();

// ---------- 运行日志面板 ----------
const LOG_MAX = 300;
function log(text, cls = "") {
  const lines = $("log-lines");
  const line = document.createElement("div");
  line.className = `log-line ${cls}`;
  const time = new Date().toTimeString().slice(0, 8);
  line.textContent = `${time}  ${localizeDynamic(text)}`;
  lines.appendChild(line);
  while (lines.childElementCount > LOG_MAX) lines.firstElementChild.remove();
  lines.scrollTop = lines.scrollHeight;
}
$("log-toggle").addEventListener("click", () => $("log-panel").classList.toggle("hidden"));
$("log-retry").addEventListener("click", async () => {
  if (typeof errorRetry !== "function") return;
  const retry = errorRetry;
  $("log-retry").disabled = true;
  try {
    await retry();
  } finally {
    $("log-retry").disabled = false;
  }
});
$("log-copy").addEventListener("click", async () => {
  const text = $("log-lines").innerText.trim();
  if (!text) {
    toast(t("暂无可复制的运行日志"));
    return;
  }
  try {
    await navigator.clipboard.writeText(text);
    toast(t("运行日志已复制"));
  } catch (error) {
    toastError(`复制运行日志失败:${error}`);
  }
});
$("log-clear").addEventListener("click", () => ($("log-lines").innerHTML = ""));

// ---------- 状态栏 ----------
let statusTextSource = "";
let statusRunning = false;
function setStatus(text, isRunning) {
  statusTextSource = String(text ?? "");
  statusRunning = !!isRunning;
  $("status-text").textContent = localizeDynamic(statusTextSource);
  $("status-mode").textContent = statusRunning ? t("运行中") : t("空闲");
  $("status-dot").className = `dot ${statusRunning ? "run" : "idle"}`;
  $("statusbar").classList.toggle("running", statusRunning);
}

// 运行计时 + 首响应看门狗:等太久时把"卡在哪"讲清楚。
let runStart = 0;
let firstSignal = false;
let elapsedTimer = null;
function startElapsed() {
  runStart = Date.now();
  firstSignal = false;
  clearInterval(elapsedTimer);
  elapsedTimer = setInterval(() => {
    const secs = Math.floor((Date.now() - runStart) / 1000);
    $("status-elapsed").textContent = `· ${secs}s`;
    if (!firstSignal && secs > 0 && secs % 15 === 0) {
      log(`仍在等待模型首个响应(已 ${secs}s)——订阅高峰或网络较慢时属正常;超时上限 15s 连接 / 180s 读`, "warn");
    }
  }, 1000);
}
function stopElapsed() {
  clearInterval(elapsedTimer);
  elapsedTimer = null;
  $("status-elapsed").textContent = "";
}
function markFirstSignal() {
  if (!firstSignal) {
    firstSignal = true;
    log(`模型开始响应(${((Date.now() - runStart) / 1000).toFixed(1)}s)`);
  }
}

let ctxLimit = null;
let ctxTokens = 0;
function renderTokens() {
  const t = runTokens;
  let text = t.input + t.output === 0
    ? ""
    : `in ${t.input} (cache r${t.cacheRead} w${t.cacheWrite}) · out ${t.output}`;
  const bar = $("ctx-bar");
  if (ctxTokens > 0) {
    const k = (ctxTokens / 1000).toFixed(1);
    if (ctxLimit) {
      const pct = Math.round((ctxTokens / ctxLimit) * 100);
      text += `${text ? " · " : ""}ctx ${k}k/${Math.round(ctxLimit / 1000)}k (${pct}%)`;
      $("status-tokens").classList.toggle("ctx-warn", pct >= 70);
      // 进度条:容量占用一眼可见,≥70% 变警示色(自动压缩阈值同源)。
      bar.classList.remove("hidden");
      bar.classList.toggle("warn", pct >= 70);
      $("ctx-bar-fill").style.width = `${Math.min(pct, 100)}%`;
      bar.title = `${t("上下文")} ${k}k / ${Math.round(ctxLimit / 1000)}k(${pct}%,≥70% ${t("自动压缩")})`;
    } else {
      text += `${text ? " · " : ""}ctx ${k}k`;
      bar.classList.add("hidden");
    }
  } else {
    bar.classList.add("hidden");
  }
  $("status-tokens").textContent = text;
}

function setRunning(value, statusText) {
  running = value;
  const send = $("send");
  send.disabled = false;
  send.title = value ? t("运行中可插入或排队，按交付方式发送") : "";
  send.setAttribute("aria-label", value ? t("运行中可插入或排队，按交付方式发送") : "发送");
  $("stop").classList.toggle("hidden", !value);
  setStatus(statusText ?? (value ? t("运行中") : t("空闲")), value);
}

