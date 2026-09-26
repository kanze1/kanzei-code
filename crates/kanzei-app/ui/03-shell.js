import { toast as surfaceToast } from "./00-surface.js";
import { defer } from "./01-core.js";
import { $, invoke, renderingBackground, uiPrefsLoad, uiPrefsSave } from "./01-core.js";
import { I18N_EN, localizeDynamic, t } from "./02-i18n.js";
import { parseErrorText } from "./04-structured-parse.js";
import { renderErrorDetail } from "./04-structured.js";
import { fastStatusText } from "./06-activity.js";
import { agentClosePanel } from "./06-agent-panel.js";
import { autoContinueTimers, clearStoppingWatchdog } from "./08-auto.js";
import { send } from "./08-compose-runtime.js";
import { state } from "./08-compose.js";
import { refreshWorktrees } from "./09-sessions.js";
import { clearJumpReveal, clearPendingJump } from "./11-docs-list.js";
import { refreshWorkspace } from "./12-docs-pages.js";
import { refreshMemory, refreshMetrics } from "./13-memory.js";
import { refreshDocs } from "./14-docs-actions.js";
import { loadSettings } from "./16-settings.js";
import { filesViewLeft, showFilesView } from "./17-files.js";
import { refreshArch } from "./19-arch.js";
import { refreshResearch } from "./19-research.js";
import { refreshLines } from "./20-lines.js";
import { active_space, remember_workspace_view, view_allowed } from "./03-workspaces.js";
import { open_research_chat } from "./19-research-navigation.js";

export function setupResize(elementId, key, side, min, max) {
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
defer(() => {
  setupResize("sidebar", "kz-sidebar-width", "right", 220, 460);
});
export let activeProcessId = null;
export function setActiveProcessId(v) { activeProcessId = v; }
export let activeSessionId = null;
export function setActiveSessionId(v) { activeSessionId = v; }
export let processItems = [];
export function setProcessItems(value) { processItems = value; }

// R-086:每个会话独立的运行状态机。控制事件按 sessionId 更新对应状态机,视图只
// 投影活动会话的状态——后台会话的 idle/stopped 先落这里,切回时从状态机重建,
// 而不是依赖事件在"恰好活动"时才会被处理。待答队列另有 askQueues,不放这里。
export const sessionStates = new Map();
export function sessionState(sessionId) {
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

export let running = false;
export let runControlPending = false;
export let currentProject = null;
export function setCurrentProject(v) { currentProject = v; }
export let currentAssistant = null;
export let currentReasoning = null;
export function setCurrentAssistant(value) { currentAssistant = value; }
export function setCurrentReasoning(value) { currentReasoning = value; }
export let attachments = [];
export let lastRequest = null;
export function setAttachments(value) { attachments = Array.isArray(value) ? value : []; }
export function setLastRequest(value) { lastRequest = value; }
export let runTokens = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };
export function setRunTokens(value) { runTokens = value; }

// ---------- 视图切换 ----------
let viewLoadTimer = null;
let viewLoadGeneration = 0;
// 只绑带 data-view 的按钮:rail 上还有侧栏开合这类布局开关,它们不是视图。
export function navigate_view(view) {
  if (!view_allowed(view)) return;
  const item = document.querySelector(`.activity-item[data-view="${view}"]`);
  if (!item || !$(`view-${view}`)) return;
  document.body.dataset.view = view;
  if (view !== "chat") {
    // UI-0926 #8:经 agentClosePanel 收起,agentPanelOpen 与 DOM 保持一致(先关子代理面板,
    // 它会按 activityPanelOpen 同步活动面板;随后照旧把活动面板也收起)。
    agentClosePanel();
    $("bg-panel")?.classList.add("hidden");
  }
  remember_workspace_view(view);
  document.querySelectorAll(".activity-item[data-view]").forEach((i) => {
    i.classList.remove("active");
    i.removeAttribute("aria-current");
  });
  item.classList.add("active");
  item.setAttribute("aria-current", "page");
  document.body.classList.toggle("documents-active", view === "documents");
  // 已经在这个视图里就别再重载一遍:设置页尤其致命——再点一次侧栏图标,
  // 填了一半没保存的表单会静悄悄回滚成磁盘值。
  const previousView = document.querySelector(".view.active")?.id;
  if (previousView === `view-${view}`) return;
  clearTimeout(viewLoadTimer);
  const generation = ++viewLoadGeneration;
  if (view !== "documents") {
    clearPendingJump();
    clearJumpReveal();
  }
  document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
  $(`view-${view}`).classList.add("active");
  if (view !== "files") filesViewLeft();
  document.dispatchEvent(new CustomEvent("kz:view-changed", { detail: { view } }));
  // 先让选中态和新页面绘制；快速连续切换只加载最后一个页面。
  requestAnimationFrame(() => {
    if (generation !== viewLoadGeneration) return;
    viewLoadTimer = setTimeout(() => {
      if (generation !== viewLoadGeneration) return;
      const loaders = { settings: loadSettings, workspace: refreshWorkspace, documents: refreshDocs,
        research: refreshResearch, memory: refreshMemory, metrics: refreshMetrics,
        files: showFilesView, arch: refreshArch, lines: refreshLines };
      void loaders[view]?.();
      if (view === "lines") void refreshWorktrees();
    }, 0);
  });
}
defer(() => {
  document.querySelectorAll(".activity-item[data-view]").forEach((item) => {
    item.addEventListener("click", () => active_space === "research" && item.dataset.view === "chat" ? open_research_chat() : navigate_view(item.dataset.view));
  });
});

// ---------- toast ----------
// 一句话反馈。本地化在这里做,显示交给 00-surface.js 的 toast 区域(最多 3 条、err 用 role=alert);
// 长错误走 toastError → 日志面板,不交给会自动消失的 toast。kind: info|ok|warn|err。
export let errorRetry = null;
export function toast(text, { kind = "info" } = {}) {
  const source = String(text);
  const translated = Object.prototype.hasOwnProperty.call(I18N_EN, source) ? t(source) : source;
  return surfaceToast(localizeDynamic(translated), { kind });
}
export function reportPersistentError(text, { retry = null } = {}) {
  log(text, "err");
  errorRetry = retry;
  $("log-retry").classList.toggle("hidden", typeof retry !== "function");
  $("log-panel").classList.remove("hidden");
}
export function toastError(text, options = {}) {
  reportPersistentError(text, options);
}

export let completionAudioContext = null;
export const baseTitle = document.title;

// R-187:提示音配置——总开关 + 分事件开关 + 音量(0-1)。持久化在 localStorage,
// 设置页「提示音」区块可改,默认全部开启、音量 0.12(与原固定音量一致)。
export const SOUND_STORAGE_KEY = "kz-sound-settings";
export function readSoundSettings() {
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
export function saveSoundSettings(settings) {
  localStorage.setItem(SOUND_STORAGE_KEY, JSON.stringify(settings));
}
export function soundEnabledFor(kind) {
  const s = readSoundSettings();
  if (!s.enabled) return false;
  if (kind === "failed") return s.failed;
  if (kind === "stopped") return s.stopped;
  return s.completed;
}

export function playRunNotice(kind) {
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
    log(`${t("完成提示音不可用")}:${error}`, "warn");
  }
}

export let notificationPermissionPrompted = false;
export function explainNotificationFallback(message) {
  log(message, "warn");
  $("log-panel").classList.remove("hidden");
  toast(message);
}
export async function ensureNotificationPermission() {
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

export function notifyRunState(kind, text) {
  flashStatusDot(kind);
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
        log(`${t("系统通知不可用")}:${error}`, "warn");
      }
    }
  }
}

export function resetTitleOnFocus() {
  if (!document.hidden && document.hasFocus()) document.title = baseTitle;
}
defer(() => {
  document.addEventListener("visibilitychange", resetTitleOnFocus);
});
defer(() => {
  window.addEventListener("focus", resetTitleOnFocus);
});
export let activityPanelOpen = localStorage.getItem("kz-activity-panel") === "1";
export function setActivityPanelOpen(value) { activityPanelOpen = Boolean(value); }

export function syncActivityPanel() {
  $("bg-panel").classList.toggle("hidden", !activityPanelOpen);
  const toggle = $("activity-toggle");
  toggle.classList.toggle("active", activityPanelOpen);
  // 开关搬到 rail 后按钮内容是 SVG:再写 textContent 会把图标整个抹掉,
  // 状态只走 class + aria-pressed + title。
  toggle.setAttribute("aria-pressed", activityPanelOpen ? "true" : "false");
  toggle.title = activityPanelOpen ? t("隐藏右侧活动面板") : t("显示右侧活动面板");
}

export function closeActivityPanel() {
  activityPanelOpen = false;
  localStorage.setItem("kz-activity-panel", "0");
  syncActivityPanel();
  $("activity-toggle")?.focus();
}

defer(() => {
  $("activity-toggle").addEventListener("click", () => {
    activityPanelOpen = !activityPanelOpen;
    localStorage.setItem("kz-activity-panel", activityPanelOpen ? "1" : "0");
    if (activityPanelOpen) agentClosePanel();
    syncActivityPanel();
  });
  $("bg-close")?.addEventListener("click", closeActivityPanel);
});
defer(() => {
  syncActivityPanel();
});

export let sidebarCollapsed = localStorage.getItem("kz-sidebar-collapsed") === "1";
export function setSidebarCollapsed(value) { sidebarCollapsed = Boolean(value); }
export function syncSidebar() {
  const sidebar = $("sidebar");
  sidebar.classList.toggle("collapsed", sidebarCollapsed);
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
export function transitionSession(sessionId, phase, detail = {}) {
  if (!sessionId) return null;
  const state = sessionState(sessionId);
  // 真终态一到就把停止看门狗撤掉,免得它在 10 秒后对一条已经正常收尾的会话
  // 再喊一次「未收到确认」。
  if (phase !== "stopping" && typeof clearStoppingWatchdog === "function") clearStoppingWatchdog(sessionId);
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
  // 「新对话」按钮 title 按活动线忙闲说明点下去会怎样:相位一变就跟上(只在值变时写)。
  if (sessionId === activeSessionId) syncNewChatEnabled();
  return state;
}
export function toggleSidebar() {
  sidebarCollapsed = !sidebarCollapsed;
  localStorage.setItem("kz-sidebar-collapsed", sidebarCollapsed ? "1" : "0");
  syncSidebar();
}
defer(() => {
  $("rail-sidebar-toggle")?.addEventListener("click", toggleSidebar);
});
// 悬浮模式(≤900px,缩放放大同样会触发)下侧栏盖在主区上:点侧栏外的任意
// 位置就收起,不再需要先找到被盖住的开关。
// matchMedia 在冒烟 harness 里不存在:回退成"永不悬浮",真实浏览器不受影响。
export const sidebarOverlayQuery = typeof window.matchMedia === "function"
  ? window.matchMedia("(max-width: 900px)")
  : { matches: false };
defer(() => {
  document.addEventListener("pointerdown", (event) => {
    if (sidebarCollapsed || !sidebarOverlayQuery.matches) return;
    if (event.target.closest("#sidebar, #activitybar")) return;
    sidebarCollapsed = true;
    localStorage.setItem("kz-sidebar-collapsed", "1");
    syncSidebar();
  });
});
/// 进入悬浮态(≤900px)时先把抽屉收起来。悬浮的侧栏 z-index 高于输入区上下文行,
/// 展开着就把「当前项目 / 模型 / 思考强度」整条盖掉——顶栏删除后那一行是**唯一**
/// 能看到自己在对哪个项目、用哪个模型说话的地方,被盖住时 Ctrl+Enter 是盲发。
/// 只在**跨过断点的那一刻**收,不动用户在宽窗口下的选择。
export function collapseSidebarForOverlay(matches) {
  if (!matches || sidebarCollapsed) return;
  sidebarCollapsed = true;
  localStorage.setItem("kz-sidebar-collapsed", "1");
  syncSidebar();
}
defer(() => {
  if (typeof sidebarOverlayQuery.addEventListener === "function") {
    sidebarOverlayQuery.addEventListener("change", (event) => collapseSidebarForOverlay(event.matches));
  };
});
defer(() => {
  collapseSidebarForOverlay(sidebarOverlayQuery.matches);
});
defer(() => {
  syncSidebar();
});

// ---------- 运行日志面板 ----------
export const LOG_MAX = 300;
export function log(text, cls = "") {
  const lines = $("log-lines");
  const line = document.createElement("div");
  line.className = `log-line ${cls}`;
  const time = new Date().toTimeString().slice(0, 8);
  line.textContent = `${time}  ${localizeDynamic(text)}`;
  if (cls === "err") {
    const detail = logErrorDetail(text);
    if (detail) line.append(detail);
  }
  lines.appendChild(line);
  while (lines.childElementCount > LOG_MAX) lines.firstElementChild.remove();
  lines.scrollTop = lines.scrollHeight;
}
/// UI-0926 #10:错误日志里的 provider JSON 错误体 / 错误链折叠成结构化详情,原文仍在行内。
/// 日志文案常带「自动放行失败:」这类短前缀,前缀后的部分也试一次。
function logErrorDetail(text) {
  const raw = String(text ?? "");
  const cut = raw.search(/[:：]/);
  const candidates = cut > 0 && cut <= 24 ? [raw, raw.slice(cut + 1).trim()] : [raw];
  for (const candidate of candidates) {
    const info = parseErrorText(candidate);
    if (!info.json && !info.chain.length) continue;
    const details = document.createElement("details");
    details.className = "sv-log-error";
    const summary = document.createElement("summary");
    summary.textContent = t("错误详情");
    details.append(summary, renderErrorDetail(candidate));
    return details;
  }
  return null;
}
defer(() => {
  $("log-toggle").addEventListener("click", () => $("log-panel").classList.toggle("hidden"));
});
defer(() => {
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
});
defer(() => {
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
      toastError(`${t("复制运行日志失败")}:${error}`);
    }
  });
});
defer(() => {
  $("log-clear").addEventListener("click", () => ($("log-lines").innerHTML = ""));
});

// ---------- 状态栏 ----------
export let statusTextSource = "";
export let statusRunning = false;
// ---------- #7 运行活动投影:状态栏点 + 输入框上方的「思考中… 12s」活动行 ----------
// 全局相位只投影到 html[data-kz-activity](running|pending|stopping|idle),只由这里写,
// CSS 只读它(思考块扫光、文档页在做条目等都按它门控)。turnPhase 是本轮细分相位,
// 由 07-events 的事件写入点推进:等首 token / 思考 / 生成 / 工具。
export let turnPhase = "idle";
// 细分相位属于哪条线:setRunning 据此区分「同一条线的运行态纠偏(保留相位)」与「换线(重置)」。
export let turnPhaseSession = null;
export const TURN_DETAIL_PHASES = new Set(["waiting", "thinking", "generating", "tool"]);
export function setTurnPhase(phase) {
  // 后台会话的渲染不得改写活动行——它和状态栏一样只属于活动会话(R-267 同一守卫)。
  if (typeof renderingBackground !== "undefined" && renderingBackground) return;
  // 「停止中」粘滞:停止发出后迟到的文本/思考/工具事件不得把活动行翻回运行态(停止按钮还写着
  // 「停止中…」)。退出 stopping 只经 setRunning / setRunPending / clearRunPending。
  if (turnPhase === "stopping") return;
  turnPhase = phase;
  turnPhaseSession = activeSessionId;
  renderTurnActivity();
}
export function activityKey() {
  if (statusRunning) return turnPhase === "stopping" ? "stopping" : "running";
  return runControlPending ? "pending" : "idle";
}
// kz:text 逐 delta 都会走到这里:只在值真的变了才写,不给样式重算与 MutationObserver 添无用功。
function setDataIfChanged(el, key, value) {
  if (el && el.dataset[key] !== value) el.dataset[key] = value;
}
export function renderTurnActivity() {
  const activity = activityKey();
  setDataIfChanged(document.documentElement, "kzActivity", activity);
  const dot = $("status-dot");
  if (dot) {
    const dotClass = `dot kz-dot ${statusRunning ? "run" : "idle"}`;
    if (dot.className !== dotClass) dot.className = dotClass;
    setDataIfChanged(dot, "state", activity);
  }
  const row = $("turn-activity");
  if (!row) return;
  const phase = activity !== "running" ? activity : TURN_DETAIL_PHASES.has(turnPhase) ? turnPhase : "working";
  setDataIfChanged(row, "phase", phase);
  row.classList.toggle("hidden", activity === "idle");
  setDataIfChanged($("turn-activity-glyph"), "state", phase === "waiting" ? "waiting" : activity);
  // 文案复用状态栏的存源(不新增文案),切语言时照样经 localizeDynamic 重算。
  const label = $("turn-activity-label");
  const text = localizeDynamic(statusTextSource) || t("运行中");
  if (label && label.textContent !== text) label.textContent = text;
  renderTurnElapsed();
}
export function renderTurnElapsed() {
  const el = $("turn-activity-elapsed");
  if (!el) return;
  const text = elapsedTimer && runStart > 0 ? `${Math.floor((Date.now() - runStart) / 1000)}s` : "";
  if (el.textContent !== text) el.textContent = text;
}
// 轮末一次性反馈:完成弹一下出绿环,失败抖一下出红环。停止是用户自己按的,不播。
export let statusDotFlashTimer = null;
export function flashStatusDot(kind) {
  if (typeof renderingBackground !== "undefined" && renderingBackground) return;
  if (kind !== "completed" && kind !== "failed") return;
  const dot = $("status-dot");
  if (!dot) return;
  clearTimeout(statusDotFlashTimer);
  delete dot.dataset.flash;
  void dot.offsetWidth;
  dot.dataset.flash = kind;
  statusDotFlashTimer = setTimeout(() => {
    delete dot.dataset.flash;
    statusDotFlashTimer = null;
  }, 700);
}
// 窗口隐藏(最小化/切走)时全部动画暂停:html[data-kz-motion="paused"] 统一 animation-play-state。
export function syncMotionVisibility() {
  document.documentElement.dataset.kzMotion = document.hidden ? "paused" : "live";
}
defer(() => {
  syncMotionVisibility();
  document.addEventListener("visibilitychange", syncMotionVisibility);
});
export function setStatus(text, isRunning) {
  // R-267:后台会话的渲染不得改写状态栏——那是活动会话的位置。
  if (typeof renderingBackground !== "undefined" && renderingBackground) return;
  statusTextSource = String(text ?? "");
  statusRunning = !!isRunning;
  $("status-text").textContent = localizeDynamic(statusTextSource);
  $("status-mode").textContent = statusRunning ? t("运行中") : t("空闲");
  // 去重:空闲时两格都是「空闲」、刚开跑时都是「运行中」——同一个词不在状态栏并排写两遍。
  $("status-text").classList.toggle("hidden", $("status-text").textContent === $("status-mode").textContent);
  $("statusbar").classList.toggle("running", statusRunning);
  renderTurnActivity();
}

// 运行计时 + 首响应看门狗:等太久时把"卡在哪"讲清楚。
export let runStart = 0;
export let firstSignal = false;
export let elapsedTimer = null;
export function roundElapsedSeconds(reportedMs) {
  const elapsedMs = Number(reportedMs);
  if (Number.isFinite(elapsedMs) && elapsedMs >= 0) return elapsedMs / 1000;
  if (runStart <= 0) return null;
  return (Date.now() - runStart) / 1000;
}
export function startElapsed() {
  runStart = Date.now();
  firstSignal = false;
  clearInterval(elapsedTimer);
  elapsedTimer = setInterval(() => {
    const secs = Math.floor((Date.now() - runStart) / 1000);
    $("status-elapsed").textContent = `· ${secs}s`;
    renderTurnElapsed();
    if (!firstSignal && secs > 0 && secs % 15 === 0) {
      log(`${t("仍在等待模型首个响应")}(${t("已")} ${secs}s)——${t("订阅高峰或网络较慢时属正常")};${t("超时上限")} 15s ${t("连接")} / 180s ${t("读")}`, "warn");
    }
  }, 1000);
  renderTurnElapsed();
}
export function stopElapsed() {
  clearInterval(elapsedTimer);
  elapsedTimer = null;
  $("status-elapsed").textContent = "";
  renderTurnElapsed();
}
export function markFirstSignal() {
  // R-267:首响应计时属于活动会话的这一轮,后台会话的事件不参与。
  if (typeof renderingBackground !== "undefined" && renderingBackground) return;
  if (!firstSignal) {
    firstSignal = true;
    log(`${t("模型开始响应")}(${((Date.now() - runStart) / 1000).toFixed(1)}s)`);
  }
}

export let ctxLimit = null;
export function setCtxLimit(value) { ctxLimit = value; }
// 并行线各自的 kz:meta 按会话缓存:状态栏只反映当前活跃线。kz:meta 每条线只在
// run 启动时发一次,不缓存的话切线后状态栏要等到那条线下一轮才会变对。
export const sessionMetaCache = new Map();
export function applySessionMeta(sessionId) {
  const meta = sessionId ? sessionMetaCache.get(sessionId) : null;
  if (!meta) {
    // UI-0926 #3:这条线本次还没跑过:状态栏不能继续挂着别的线「上一轮实际使用」的模型。
    // 上下文上限同理:拿别的线的上限算这条线的占比是错的基准。先清空,等 model_effective
    // 回来按下一轮会用的模型补上(08-models.js refreshEffectiveModel)。
    const status = $("status-model");
    if (status) {
      status.textContent = "";
      status.title = "";
    }
    ctxLimit = null;
    return;
  }
  showRunMeta(meta);
  ctxLimit = meta.contextLimit ?? null;
}
// UI-0926 #3:思考档的显示名(输入框芯片、菜单、状态栏共用一份)。off = 不发档位,交给服务商。
export function reasoningLabel(value) {
  return {
    off: t("服务商默认"), none: t("无"), low: t("低"), medium: t("中"),
    high: t("高"), xhigh: t("超高"), max: t("最大"),
  }[value] ?? String(value ?? "");
}
// 状态栏 = 上一轮**实际**使用的「模型 · 思考档 · ⚡ · profile」(kz:meta 取自真正发出去的请求参数)。
// 输入框上方的芯片说的是「下一轮将使用」,两处口径不同,所以状态栏带 title 说明。
export function formatRunMeta(meta) {
  if (!meta) return "";
  const parts = [meta.model];
  if (meta.reasoning && meta.reasoning !== "off") parts.push(reasoningLabel(meta.reasoning));
  if (meta.codexFastMode) parts.push("⚡\uFE0E"); // 文字字形,随状态栏文字色(彩色 emoji 不受主题控制)
  if (meta.profile) parts.push(meta.profile);
  return parts.filter(Boolean).join(" · ");
}
export function showRunMeta(meta) {
  const status = $("status-model");
  if (!status) return;
  status.textContent = formatRunMeta(meta);
  status.title = t("上一轮实际使用");
}
export let ctxTokens = 0;
export let ctxPending = false;
export function setCtxTokens(value) { ctxTokens = value; }
export function setCtxPending(value) { ctxPending = value; }
export function renderTokens() {
  const tokens = runTokens;
  let text = tokens.input + tokens.output === 0
    ? ""
    : `in ${tokens.input} (cache r${tokens.cacheRead} w${tokens.cacheWrite}) · out ${tokens.output}`;
  const bar = $("ctx-bar");
  if (ctxTokens > 0) {
    const k = (ctxTokens / 1000).toFixed(1);
    const pending = ctxPending ? ` (${t("等待模型")})` : "";
    if (ctxLimit) {
      const pct = Math.round((ctxTokens / ctxLimit) * 100);
      text += `${text ? " · " : ""}ctx ${k}k/${Math.round(ctxLimit / 1000)}k (${pct}%)${pending}`;
      $("status-tokens").classList.toggle("ctx-warn", pct >= 70);
      // 进度条:容量占用一眼可见,≥70% 变警示色(自动压缩阈值同源)。
      bar.classList.remove("hidden");
      bar.classList.toggle("warn", pct >= 70);
      bar.classList.toggle("pending", ctxPending);
      $("ctx-bar-fill").style.width = `${Math.min(pct, 100)}%`;
      bar.title = `${t("上下文")} ${k}k / ${Math.round(ctxLimit / 1000)}k(${pct}%,≥70% ${t("自动压缩")})`;
    } else {
      text += `${text ? " · " : ""}ctx ${k}k${pending}`;
      bar.classList.add("hidden");
    }
  } else {
    bar.classList.add("hidden");
    if (ctxPending) text += `${text ? " · " : ""}ctx ${t("等待模型")}`;
  }
  $("status-tokens").textContent = text;
}

export function setRunning(value, statusText) {
  const wasRunning = running;
  running = value;
  runControlPending = false;
  const send = $("send");
  send.disabled = false;
  // #send 是图标按钮:空闲态的悬停提示与读屏名称同样走 t()(英文界面念 "Send",不念中文字面量)。
  // 动态值写回 data-i18n-*,切语言时由 applyDataI18nKeys 按它重算,不会被 index.html 的静态「发送」冲掉。
  send.dataset.i18nTitle = send.dataset.i18nAriaLabel = value ? "运行中可插入或排队，按交付方式发送" : "发送";
  send.title = value ? t("运行中可插入或排队，按交付方式发送") : t("发送");
  send.setAttribute("aria-label", value ? t("运行中可插入或排队，按交付方式发送") : t("发送"));
  const stop = $("stop");
  stop.disabled = false;
  stop.classList.toggle("hidden", !value);
  stop.textContent = t("停止");
  syncNewChatEnabled();
  // 同一条线已在运行时的纠偏(process_list 轮询、逐事件投影)保留本轮细分相位;
  // 新开跑、换线、从停止中/等下一轮回到运行才重置为等首 token。
  const keepPhase = value && wasRunning && TURN_DETAIL_PHASES.has(turnPhase) && turnPhaseSession === activeSessionId;
  if (!keepPhase) turnPhase = value ? "waiting" : "idle";
  turnPhaseSession = activeSessionId;
  setStatus(statusText ?? (value ? t("运行中") : t("空闲")), value);
}

/// 「新对话」按钮不再因运行而禁用。原先运行中/鞭挞轮间整段禁用,点击被浏览器静默
/// 吞掉,只有落进空闲空隙的那一下生效——「要点好几次」的来源之一。现在忙碌线点它
/// 会另开一条线路(15-views-misc.js startNewConversation),按钮只在本次新对话在途时
/// 禁用(aria-busy),防双击重复建线;title 按忙闲说清点下去会发生什么。
/// transitionSession 每个进度事件都会调到这里,所以只在值真变了时才写。
export function syncNewChatEnabled() {
  const fresh = $("new-chat");
  if (!fresh) return;
  const disabled = fresh.getAttribute("aria-busy") === "true";
  if (fresh.disabled !== disabled) fresh.disabled = disabled;
  // 与 startNewConversation 的分流同一判据:title 说「另开线路」时点下去必定另开线路。
  const busy = active_space === "dev" && activeLineBusy();
  const titleKey = active_space === "research" ? "新建课题会话，保留已有对话"
    : busy ? "当前线路运行中:点击将另开一条线路开启新对话"
    : "开一段新对话(旧对话保留在「历史对话」)";
  // 动态 title 必须同步写回 data-i18n-title:语言重应用(applyDataI18nKeys)按它重算,
  // 不写的话会被 index.html 的静态键冲回空闲文案,忙碌时的说明就看不到了。
  // 只在键变了时写:title 可能已被悬停提示层接管(移走),每个进度事件都写回会冒出原生提示。
  if (fresh.dataset.i18nTitle === titleKey) return;
  fresh.dataset.i18nTitle = titleKey;
  fresh.title = t(titleKey);
}

/// 活动线是否「还没停」:运行中、停止中、鞭挞轮间等待,或续跑定时器已排上。
/// 这些状态下 runner(或马上要开跑的那一轮)握着旧段,新对话不能在它脚下开新段,
/// 要另开线路。按钮 title(syncNewChatEnabled)与点击分流(startNewConversation)共用它。
export function activeLineBusy() {
  if (running || runControlPending) return true;
  const phase = activeSessionId ? sessionState(activeSessionId).phase : "idle";
  return ["starting", "running", "stopping", "auto_pending"].includes(phase)
    || Boolean(activeSessionId && autoContinueTimers.has(activeSessionId));
}

export function setStopping(statusText) {
  running = true;
  runControlPending = false;
  $("send").disabled = true;
  const stop = $("stop");
  stop.disabled = false;
  stop.classList.remove("hidden");
  stop.disabled = true;
  stop.textContent = t("停止中…");
  syncNewChatEnabled();
  turnPhase = "stopping";
  setStatus(statusText ?? t("停止中…"), true);
}

// 鞭挞在两轮之间等待时，后端会话已经结束本轮但自动续跑定时器仍可取消。
// 这不是 idle：停止按钮必须继续可用，且不能把 running 伪装成真实执行。
export function setRunPending(statusText) {
  runControlPending = true;
  const stop = $("stop");
  stop.disabled = false;
  stop.classList.remove("hidden");
  stop.textContent = t("停止鞭挞");
  syncNewChatEnabled();
  turnPhase = "pending";
  setStatus(statusText ?? t("等待下一轮"), false);
}

export function clearRunPending() {
  runControlPending = false;
  const stop = $("stop");
  stop.classList.toggle("hidden", !running);
  stop.textContent = t("停止");
  syncNewChatEnabled();
  if (!running) turnPhase = "idle";
  renderTurnActivity();
}

// ---------- R-189 主题切换:暗/亮持久化 ----------
// 默认暗色(现状);切亮色改 html[data-theme="light"],CSS token 组接管换色;
// 原生控件 color-scheme 已随 token 组同步;Monaco 主题由 17-files.js 读这里。
export const THEME_STORAGE_KEY = "kz-theme";
export function currentTheme() {
  return document.documentElement.getAttribute("data-theme") === "light" ? "light" : "dark";
}
export function applyTheme(theme) {
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
export function initTheme() {
  const saved = localStorage.getItem(THEME_STORAGE_KEY);
  applyTheme(saved === "light" || saved === "dark" ? saved : "dark");
  // D-404:localStorage 旧值可能已丢;后端 app.json 是权威,有值则覆盖。
  void uiPrefsLoad().then((p) => {
    if (p.theme === "light" || p.theme === "dark") applyTheme(p.theme);
  });
}
defer(() => {
  initTheme();
});
defer(() => {
  $("theme-toggle")?.addEventListener("click", () => applyTheme(currentTheme() === "light" ? "dark" : "light"));
});

// ---------- R-190 常驻 fast 模型状态 ----------
// 状态栏 #status-fast 显示 fast 子代理模型运行态:未托管时隐藏,托管时显示
// fastStatusText 的短文案并随真实探测每 10 秒刷新——Ollama 服务停掉后状态
// 自动翻红、重新起来后自动转回就绪,无需重开任何视图。
export const FAST_STATUS_POLL_MS = 10000;
export let fastStatusTimer = null;
export async function refreshFastStatusBar() {
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
  const st = fastStatusText(s);
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
export function startFastStatusBar() {
  clearInterval(fastStatusTimer);
  void refreshFastStatusBar();
  fastStatusTimer = setInterval(() => void refreshFastStatusBar(), FAST_STATUS_POLL_MS);
}
// 依赖 06-activity.js 的 fastStatusText;06 在 03 之后加载,轮询首跑在
// DOM 就绪且脚本全部加载后,这里直接启动(函数调用发生在事件循环,届时已定义)。
defer(() => {
  startFastStatusBar();
});

// R-264 B10：21-palette.js 的渐进 ESM 兼容桥；最终由显式模块 import 取代。
defer(() => {
  Object.assign(globalThis, { log });
});
