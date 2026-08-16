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
    state = {
      phase: "idle",
      running: false,
      converged: false,
      auto_pending: false,
      live_running: null,
      local_start_pending: false,
      terminal_status: "",
      stage: "空闲",
      detail: "",
    };
    sessionStates.set(sessionId, state);
  }
  return state;
}

let running = false;
let runControlPending = false;
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
    // 已经在这个视图里就别再重载一遍:设置页尤其致命——再点一次侧栏图标,
    // 填了一半没保存的表单会静悄悄回滚成磁盘值。
    const previousView = document.querySelector(".view.active")?.id;
    document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
    $(`view-${view}`).classList.add("active");
    if (view === "settings" && previousView !== "view-settings") loadSettings();
    if (view === "workspace") refreshWorkspace();
    if (view === "documents") refreshDocs();
    if (view === "research") refreshResearch();
    if (view === "memory") refreshMemory();
    if (view === "metrics") refreshMetrics();
    if (view === "files") showFilesView();
    if (view === "arch") refreshArch();
    // 工作树清单现在是线路页的一块内容(侧栏只读),进页面要一并拉。
    if (view === "lines") { refreshLines(); refreshWorktrees(); }
    if (view !== "files") filesViewLeft();
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

// R-187:提示音配置——总开关 + 分事件开关 + 音量(0-1)。持久化在 localStorage,
// 设置页「提示音」区块可改,默认全部开启、音量 0.12(与原固定音量一致)。
const SOUND_STORAGE_KEY = "kz-sound-settings";
function readSoundSettings() {
  try {
    const parsed = JSON.parse(localStorage.getItem(SOUND_STORAGE_KEY) || "null");
    if (parsed && typeof parsed === "object") {
      return {
        enabled: parsed.enabled !== false,
        volume: Number.isFinite(parsed.volume) ? Math.min(1, Math.max(0, parsed.volume)) : 0.12,
        completed: parsed.completed !== false,
        failed: parsed.failed !== false,
        stopped: parsed.stopped !== false,
      };
    }
  } catch {
    /* 损坏的配置回退默认 */
  }
  return { enabled: true, volume: 0.12, completed: true, failed: true, stopped: true };
}
function saveSoundSettings(settings) {
  localStorage.setItem(SOUND_STORAGE_KEY, JSON.stringify(settings));
}
function soundEnabledFor(kind) {
  const s = readSoundSettings();
  if (!s.enabled) return false;
  if (kind === "failed") return s.failed;
  if (kind === "stopped") return s.stopped;
  return s.completed;
}

function playRunNotice(kind) {
  if (!soundEnabledFor(kind)) return;
  try {
    const AudioCtor = window.AudioContext || window.webkitAudioContext;
    if (!AudioCtor) return;
    completionAudioContext ??= new AudioCtor();
    if (completionAudioContext.state === "suspended") completionAudioContext.resume().catch(() => {});
    const now = completionAudioContext.currentTime;
    const frequencies = kind === "failed" ? [220, 165] : kind === "stopped" ? [330] : [523, 659];
    const volume = readSoundSettings().volume;
    frequencies.forEach((frequency, index) => {
      const oscillator = completionAudioContext.createOscillator();
      const gain = completionAudioContext.createGain();
      oscillator.frequency.value = frequency;
      oscillator.type = "sine";
      gain.gain.setValueAtTime(0.0001, now + index * 0.11);
      gain.gain.exponentialRampToValueAtTime(volume, now + index * 0.11 + 0.01);
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
  // 开关搬到 rail 后按钮内容是 SVG:再写 textContent 会把图标整个抹掉,
  // 状态只走 class + aria-pressed + title。
  toggle.setAttribute("aria-pressed", activityPanelOpen ? "true" : "false");
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
  if (toggle) {
    toggle.classList.toggle("active", sidebarCollapsed);
    toggle.setAttribute("aria-expanded", sidebarCollapsed ? "false" : "true");
    toggle.title = localizeDynamic(sidebarCollapsed ? "展开左侧栏" : "折叠左侧栏");
  }
  // rail 上的常驻开关与顶栏按钮同步同一状态:窄视口下侧栏悬浮盖住顶栏时,
  // rail 是唯一还能点到的开关(用户实测缩放后"没有关闭和打开")。
  const rail = $("rail-sidebar-toggle");
  if (rail) {
    rail.classList.toggle("active", !sidebarCollapsed);
    rail.setAttribute("aria-expanded", sidebarCollapsed ? "false" : "true");
    rail.title = localizeDynamic(sidebarCollapsed ? "打开侧栏" : "收起侧栏");
  }
}

// 多线路只允许从具名会话状态投影运行控件。保留旧布尔字段供现有视图读取，
// 但所有新增竞态路径都通过这一入口同时更新 phase 与兼容字段。
function transitionSession(sessionId, phase, detail = {}) {
  if (!sessionId) return null;
  const state = sessionState(sessionId);
  state.phase = phase;
  state.running = ["starting", "running", "stopping"].includes(phase);
  state.auto_pending = phase === "auto_pending";
  if (["starting", "running"].includes(phase)) {
    state.converged = false;
    state.live_running = phase === "running" ? true : null;
    state.terminal_status = "";
  } else if (phase === "stopping") {
    // R-206:stopping 是用户已发出的控制意图——清掉实时事件权威(live_running),
    // 否则 09-sessions 轮询校正会把停止中的会话翻回运行中(状态闪跳)。
    state.live_running = false;
  } else if (["idle", "stopped", "failed", "auto_pending"].includes(phase)) {
    // auto_pending 是**轮终态**,不是运行中的中间态:kz:done 已到、本轮事件流已经结束,
    // 下一轮由 kz:turn(不在 SESSION_PROGRESS_EVENTS 里,专管解除收敛)或 armAutoContinue
    // 的 "starting" 宣告。它必须和 idle/stopped/failed 一样收敛。
    //
    // 漏掉它的代价是鞭挞**确定性饿死**(实测 21:40:57 运行完成 → 21:41:29 报「上一轮尚未
    // 结束」,正好 32 秒 = 首次 2s + 15×2s 重试耗尽):
    //   ① 本轮 "running" 置 live_running=true、converged=false;
    //   ② kz:done(Continue) → "auto_pending",旧相位表三个分支一个都不匹配,
    //      于是 converged 仍 false、live_running 仍 true;
    //   ③ kz:idle 到达时 01-core 算 targetPhase = auto_pending ? "auto_pending" : "idle",
    //      又回到 auto_pending —— 唯一那次能收敛的机会也被自己吃掉;
    //   ④ ≤3s 后 process_list 轮询走 09-sessions 校正:converged 为 false 不跳过,
    //      live_running===true 命中第一分支 → transitionSession(sid,"running") 复活;
    //   ⑤ armAutoContinue 每 2 秒复查 processRunning 恒为 true,16 次后放弃。
    // 而 09-sessions 末尾那条 `!["auto_pending","stopping",...].includes(phase)` 的例外
    // 说明作者本来就把 auto_pending 当静止态,只是相位表这边没跟上。
    state.converged = true;
    state.live_running = false;
    state.local_start_pending = false;
    state.terminal_status = phase === "stopped" ? "已停止" : phase === "failed" ? "出错" : "";
  }
  Object.assign(state, detail);
  return state;
}
function toggleSidebar() {
  sidebarCollapsed = !sidebarCollapsed;
  localStorage.setItem("kz-sidebar-collapsed", sidebarCollapsed ? "1" : "0");
  syncSidebar();
}
$("sidebar-toggle")?.addEventListener("click", toggleSidebar);
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
  // R-267:后台会话的渲染不得改写状态栏——那是活动会话的位置。
  if (typeof renderingBackground !== "undefined" && renderingBackground) return;
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
  // R-267:首响应计时属于活动会话的这一轮,后台会话的事件不参与。
  if (typeof renderingBackground !== "undefined" && renderingBackground) return;
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
  runControlPending = false;
  const send = $("send");
  send.disabled = false;
  send.title = value ? t("运行中可插入或排队，按交付方式发送") : "";
  send.setAttribute("aria-label", value ? t("运行中可插入或排队，按交付方式发送") : "发送");
  const stop = $("stop");
  stop.disabled = false;
  stop.classList.toggle("hidden", !value);
  stop.textContent = t("停止");
  setStatus(statusText ?? (value ? t("运行中") : t("空闲")), value);
}

function setStopping(statusText) {
  running = true;
  runControlPending = false;
  $("send").disabled = true;
  const stop = $("stop");
  stop.disabled = false;
  stop.classList.remove("hidden");
  stop.disabled = true;
  stop.textContent = t("停止中…");
  setStatus(statusText ?? t("停止中…"), true);
}

// 鞭挞在两轮之间等待时，后端会话已经结束本轮但自动续跑定时器仍可取消。
// 这不是 idle：停止按钮必须继续可用，且不能把 running 伪装成真实执行。
function setRunPending(statusText) {
  runControlPending = true;
  const stop = $("stop");
  stop.disabled = false;
  stop.classList.remove("hidden");
  stop.textContent = t("停止鞭挞");
  setStatus(statusText ?? t("等待下一轮"), false);
}

function clearRunPending() {
  runControlPending = false;
  const stop = $("stop");
  stop.classList.toggle("hidden", !running);
  stop.textContent = t("停止");
}

// ---------- R-189 主题切换:暗/亮持久化 ----------
// 默认暗色(现状);切亮色改 html[data-theme="light"],CSS token 组接管换色;
// 原生控件 color-scheme 已随 token 组同步;Monaco 主题由 17-files.js 读这里。
const THEME_STORAGE_KEY = "kz-theme";
function currentTheme() {
  return document.documentElement.getAttribute("data-theme") === "light" ? "light" : "dark";
}
function applyTheme(theme) {
  document.documentElement.setAttribute("data-theme", theme);
  localStorage.setItem(THEME_STORAGE_KEY, theme);
  // D-404:后端 app.json 持久化(WebView2 localStorage 数据文件缺失时重启不丢)。
  void uiPrefsSave({ theme });
  // Monaco 已初始化时同步编辑器主题(vs-dark/vs)。
  if (typeof monaco !== "undefined" && monaco.editor) {
    monaco.editor.setTheme(theme === "light" ? "vs" : "vs-dark");
  }
  const btn = $("theme-toggle");
  if (btn) {
    // D-405:activitybar 图标按钮,不显示文本;用太阳/月亮图标表达当前主题。
    btn.setAttribute("aria-label", theme === "light" ? t("切换到暗色主题") : t("切换到亮色主题"));
    btn.title = theme === "light" ? t("切换到暗色主题") : t("切换到亮色主题");
  }
  const sun = $("theme-icon-sun");
  const moon = $("theme-icon-moon");
  if (sun) sun.classList.toggle("hidden", theme !== "light");
  if (moon) moon.classList.toggle("hidden", theme !== "dark");
}
function initTheme() {
  const saved = localStorage.getItem(THEME_STORAGE_KEY);
  applyTheme(saved === "light" || saved === "dark" ? saved : "dark");
  // D-404:localStorage 旧值可能已丢;后端 app.json 是权威,有值则覆盖。
  void uiPrefsLoad().then((p) => {
    if (p.theme === "light" || p.theme === "dark") applyTheme(p.theme);
  });
}
initTheme();
$("theme-toggle")?.addEventListener("click", () => applyTheme(currentTheme() === "light" ? "dark" : "light"));

// ---------- R-190 常驻 fast 模型状态 ----------
// 状态栏 #status-fast 显示 fast 子代理模型运行态:未托管时隐藏,托管时显示
// fastStatusText 的短文案并随真实探测每 10 秒刷新——Ollama 服务停掉后状态
// 自动翻红、重新起来后自动转回就绪,无需重开任何视图。
const FAST_STATUS_POLL_MS = 10000;
let fastStatusTimer = null;
async function refreshFastStatusBar() {
  const el = $("status-fast");
  if (!el) return;
  let s;
  try {
    s = await invoke("fast_model_status");
  } catch {
    return; // 命令不可用(旧引擎):保持现状不报错。
  }
  if (!s.managed) {
    el.classList.add("hidden");
    el.textContent = "";
    return;
  }
  const st = typeof fastStatusText === "function" ? fastStatusText(s) : null;
  const short = st
    ? s.ready
      ? `✓ ${t("子代理就绪")}`
      : !s.installed
        ? `⚠ ${t("Ollama 未安装")}`
        : !s.serviceUp
          ? `⚠ ${t("Ollama 服务未运行")}`
          : `⚠ ${t("模型未拉取")}`
    : "";
  el.textContent = short;
  el.classList.remove("hidden");
  el.classList.toggle("warn-text", Boolean(st?.warn));
}
function startFastStatusBar() {
  clearInterval(fastStatusTimer);
  void refreshFastStatusBar();
  fastStatusTimer = setInterval(() => void refreshFastStatusBar(), FAST_STATUS_POLL_MS);
}
// 依赖 06-agent-panel.js 的 fastStatusText;06 在 03 之后加载,轮询首跑在
// DOM 就绪且脚本全部加载后,这里直接启动(函数调用发生在事件循环,届时已定义)。
startFastStatusBar();

