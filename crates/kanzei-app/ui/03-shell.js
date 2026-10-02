import { showSchedules } from "./24-schedules.js";
import { isGeneralChat, openGeneralChat, setConversationRoot } from "./03-general-scope.js";
import { onEscapeFallthrough, surfaceElements, toast as surfaceToast } from "./00-surface.js";
import { defer } from "./01-core.js";
import { $, invoke, renderingBackground, uiPrefsLoad, uiPrefsSave } from "./01-core.js";
import { I18N_EN, localizeDynamic, t } from "./02-i18n.js";
import { fillTemplate, parseErrorText } from "./04-structured-parse.js";
import { renderErrorDetail } from "./04-structured.js";
import { fastStatusText } from "./06-activity.js";
import { reconcileTasksPanel } from "./06-agent-panel.js";
import { autoContinueTimers, clearStoppingWatchdog } from "./08-auto.js";
import { send } from "./08-compose-runtime.js";
import { state } from "./08-compose.js";
import { lastProjectPrefs, refreshWorktrees, switchProcess } from "./09-sessions.js";
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
import { cancelProjectNavigation, openProjectSpace, reconcileWorkbenchView, renderProjectOverview, renderProjectActivity, setBrowsingProject, workbenchProject } from "./12-workbench.js";
import { lastWorkspaceSnapshot } from "./12-docs-pages.js";
import { layoutPref, onLayoutChange, prefObject, setLayoutPref, setPrefObject } from "./03-layout.js";
import { sameProject } from "./25-softwire-model.js";

// 布局分隔条(侧栏宽、日志高、文件树宽、记忆列表宽)归 00-frame.js 的 installSplit,在 03-layout.js 统一安装:
// 尺寸写成 <html> 上的 --kz-split-<id>,不再写元素内联 width(内联宽度压过 #sidebar.collapsed{width:0},
// 拖过宽度再收起侧栏会留一整条空栏——UI2-0926 #4 的缺陷 A)。
export let activeProcessId = null;
export function setActiveProcessId(v) { activeProcessId = v; }
export let activeSessionId = null;
export function setActiveSessionId(v) {
  activeSessionId = v;
  syncElapsed();
}
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
export function setCurrentProject(v) { currentProject = v; setConversationRoot(v); }
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

// ---------- 导航栈(UX-001、D1~D20) ----------
// 离开一个页面时把「来处」{ view, project, processId, space, scroll } 压栈;非对话页头的「← 返回」、Esc、Alt+←、
// 鼠标侧键、再点 ⚙ 全部经 navigate_back 弹栈。栈在 navigate_view 里统一捕获——不论入口是 rail、命令面板、
// 对话里的 chip,还是别处的 `.activity-item[data-view].click()`,都有来处可回,调用点不用改。
// 规则:回到根视图(对话 / 研究)清栈;目标已在栈里就折叠回去(不重复入栈);返回途中的到达不入栈。
// 偏好不落盘:栈只活在这次运行里。
const NAV_STACK_MAX = 24;
const navStack = [];
let navCurrent = null;
let navBacking = false;
let navScrollRestore = null;
const ROOT_VIEWS = new Set(["chat", "research"]);
export function isRootView(view) { return ROOT_VIEWS.has(view); }
// 页头不注入返回条的非根视图:项目概览的纵向空间是算死的(图与三条回复正好排在输入卡之上,冒烟断言「不出现外层滚动」),
// 它自己有「对话 · 概览」页签这一对来回出口;栈与 Esc / Alt+← / 鼠标侧键照常对它生效。要给它加返回条,先让概览页让出高度。
const BARLESS_VIEWS = new Set(["project"]);
// 各页真正滚动的那一层:离开时记 scrollTop,返回时在页面数据回来后复原。
const VIEW_SCROLLERS = {
  workspace: "workspace-scroll", project: "project-overview-content", lines: "lines-scroll", documents: "documents-scroll",
  memory: "memory-scroll", metrics: "metrics-scroll", arch: "arch-scroll", settings: "settings-scroll",
};
const VIEW_LABELS = {
  chat: "对话", workspace: "所有项目", project: "项目概览", lines: "并行线路", documents: "需求", memory: "记忆",
  files: "文件", arch: "架构", metrics: "运行画像", settings: "设置", research: "研究",
};
export function viewLabel(view) { return t(VIEW_LABELS[view] || view); }
function navSnapshot(view) {
  return { view, project: workbenchProject() || currentProject || null, processId: activeProcessId, space: active_space, scroll: 0 };
}
function readViewScroll(view) {
  const el = VIEW_SCROLLERS[view] ? $(VIEW_SCROLLERS[view]) : null;
  return el ? Math.round(el.scrollTop || 0) : 0;
}
function trackNavigation(view) {
  const here = navSnapshot(view);
  const from = navCurrent ?? navSnapshot(document.querySelector(".view.active")?.id?.slice(5) || view);
  navCurrent = here;
  if (from.view === view) return;
  from.scroll = readViewScroll(from.view);
  // 离开对话时补记「此刻在哪条会话」:到达时的值可能已被项目内切会话改掉。项目已变就别动(那是新项目的会话)。
  if (from.view === "chat" && sameProject(from.project, here.project)) from.processId = activeProcessId;
  if (navBacking) return;
  if (isRootView(view)) { navStack.length = 0; return; }
  const seen = navStack.findLastIndex((entry) => entry.view === view && entry.space === here.space && sameProject(entry.project, here.project));
  if (seen >= 0) { navStack.length = seen; return; }
  navStack.push(from);
  if (navStack.length > NAV_STACK_MAX) navStack.shift();
}
function entryUsable(entry) {
  if (!entry || entry.space !== active_space || !view_allowed(entry.view) || entry.view === document.body.dataset.view) return false;
  // 已被移除的项目回不去了:跳过这一项继续往下找。
  const projects = lastProjectPrefs.projects ?? [];
  return !entry.project || entry.space !== "dev" || ["workspace", "settings"].includes(entry.view) || !projects.length || projects.some((path) => sameProject(path, entry.project));
}
// 栈空时的兜底:研究空间回研究页;开发空间回(浏览中的)项目对话;没有项目就无处可回。
function fallbackEntry() {
  if (active_space === "research") return { view: "research", project: null, processId: null, space: "research", scroll: 0 };
  const project = workbenchProject() || currentProject;
  if (!project) return null;
  return { view: "chat", project, processId: sameProject(project, currentProject) ? activeProcessId : null, space: "dev", scroll: 0 };
}
function backTarget() {
  for (let i = navStack.length - 1; i >= 0; i -= 1) if (entryUsable(navStack[i])) return navStack[i];
  const fallback = fallbackEntry();
  return fallback && entryUsable(fallback) ? fallback : null;
}
/// 当前页能不能「返回」:非根视图且有处可回。Esc / Alt+← / 命令面板「返回」共用这一判据。
export function backAvailable() {
  return !isRootView(document.body.dataset.view) && Boolean(backTarget());
}
async function goToEntry(entry) {
  navBacking = true;
  navScrollRestore = entry.scroll > 0 ? { view: entry.view, top: entry.scroll, at: Date.now() } : null;
  try {
    const global = entry.view === "workspace" || entry.view === "settings";
    if (active_space === "dev" && !global && entry.project) {
      // 项目与来处不同:走统一的进项目事务(它内部再调 navigate_view,此时 navBacking 仍为真,不会重复入栈)。
      if (entry.view === "project" || !sameProject(entry.project, currentProject)) await openProjectSpace(entry.project, entry.view);
      else { setBrowsingProject(entry.project); navigate_view(entry.view); }
    } else navigate_view(entry.view);
    // 回对话时回到离开时的那条会话(讨论 / 独立任务),而不是项目默认的主对话。
    if (entry.view === "chat" && entry.processId && entry.processId !== activeProcessId && processItems.some((item) => item.id === entry.processId)) {
      await switchProcess(entry.processId);
    }
  } finally {
    navBacking = false;
  }
}
/// 页内层级的「先退一层」钩子:某些页自己有二级页(概览里的需求与计划 / 记忆 / 历史… 详情),它们不进导航栈;
/// Esc / Alt+← / 侧键走 navigate_back 时先问这些钩子——有一级可退就只退这一级(回到概览),没有才退页。
/// 钩子返回 true = 已消费。与页内「←」同一个动作,所以两种「返回」层级一致。
const innerBackHandlers = [];
export function onInnerBack(handler) {
  if (typeof handler === "function") innerBackHandlers.push(handler);
}
export async function navigate_back() {
  if (navBacking) return false;
  for (const handler of innerBackHandlers) {
    try { if (handler()) return true; } catch (error) { console.warn(error); }
  }
  if (isRootView(document.body.dataset.view)) return false;
  let target = null;
  while (navStack.length && !target) {
    const entry = navStack.pop();
    if (entryUsable(entry)) target = entry;
  }
  target ??= backTarget();
  if (!target) return false;
  await goToEntry(target);
  return true;
}
/// 在非对话页触发「新讨论」/ 切会话 / 切线路之后调用:不在对话页就跳回去,别让动作只在后台生效、页面纹丝不动
/// (UX-005、UX-052)。已在对话页是空操作。命令面板、Ctrl+K / Ctrl+Shift+N、侧栏「新对话」按钮已接;
/// 对话与历史菜单、线路页「切换到此线路」等调用点在各自文件里接。
export function ensureChatView() {
  if (document.body.dataset.view === "chat") return false;
  navigate_view("chat");
  return true;
}
function restoreScrollAfter(view, loading) {
  const pending = navScrollRestore;
  if (!pending || pending.view !== view) return;
  navScrollRestore = null;
  if (!(pending.top > 0) || Date.now() - pending.at > 10000) return;
  const apply = () => requestAnimationFrame(() => {
    const el = VIEW_SCROLLERS[view] ? $(VIEW_SCROLLERS[view]) : null;
    if (el) el.scrollTop = pending.top;
  });
  Promise.resolve(loading).then(apply, apply);
}
function notifyViewUnavailable() {
  toast(t("当前空间不提供这个页面,请先在侧栏切换空间"));
}

// 非对话页头:JS 统一注入常驻「← 返回」(写明去向),不逐个视图改 HTML。
function backBarFor(view) {
  const host = $(`view-${view}`);
  if (!host) return null;
  let bar = host.querySelector(".view-back-bar");
  if (bar) return bar;
  bar = document.createElement("div");
  bar.className = "view-back-bar";
  const back = document.createElement("button");
  back.type = "button";
  back.className = "ghost mini view-back";
  back.addEventListener("click", () => void navigate_back());
  bar.append(back);
  host.insertBefore(bar, host.firstChild);
  return bar;
}
export function syncBackBars() {
  const view = document.body.dataset.view;
  if (!view || isRootView(view) || BARLESS_VIEWS.has(view)) return;
  const bar = backBarFor(view);
  if (!bar) return;
  const target = backTarget();
  const back = bar.querySelector(".view-back");
  const label = target ? viewLabel(target.view) : "";
  bar.hidden = !target;
  back.textContent = `← ${t("返回")}${label ? ` · ${label}` : ""}`;
  // 去向写进模板(英文是「Back to X」,不能把「返回」和页名硬拼成「Back Chat」)。
  const backName = label ? fillTemplate(t("返回 {page}"), { page: label }) : t("返回");
  back.title = `${backName} (Esc / Alt+←)`;
  back.setAttribute("aria-label", backName);
  back.setAttribute("aria-keyshortcuts", "Escape Alt+ArrowLeft");
}
function surfaceBlocksNav() {
  return surfaceElements().some((surface) => ["modal", "menu", "popover"].includes(surface.type));
}
function isTextEntryTarget(node) {
  const tag = String(node?.tagName || "").toLowerCase();
  if (tag === "textarea" || node?.isContentEditable === true) return true;
  return tag === "input" && !/^(checkbox|radio|button|submit|reset|range|color|file|image)$/i.test(String(node.type || ""));
}
defer(() => {
  // Esc(无弹层时)回上一页:挂在 00-surface 的 Esc 兜底上——弹层栈优先,元素自己处理过的 Esc(preventDefault)也先于它。
  // 文字输入框 / 下拉里的这一下 Esc 只失焦;Monaco 里的 Esc 归编辑器。
  onEscapeFallthrough((event) => {
    if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey || event.keyCode === 229) return false;
    if (surfaceBlocksNav()) return false;
    const target = event.target;
    if (target?.closest?.(".monaco-editor")) return false;
    // 窄窗悬浮抽屉是盖在主区上的覆盖层:Esc 先收抽屉,再按一次才退页(不然抽屉留在原处、页面却退了)。
    if (sidebarOverlayQuery.matches && !sidebarCollapsed) {
      applySidebarState(true, { remember: false });
      return true;
    }
    if (!backAvailable()) return false;
    if (isTextEntryTarget(target) || String(target?.tagName || "").toLowerCase() === "select") { target.blur?.(); return true; }
    void navigate_back();
    return true;
  });
  // Alt+←:同一个栈(任何位置都认,包括输入框——Alt+← 在文字里没有局部含义;处理过的比如 Monaco 会 preventDefault 让路)。
  document.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowLeft" || !event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.defaultPrevented || event.isComposing || !backAvailable() || surfaceBlocksNav()) return;
    event.preventDefault();
    void navigate_back();
  });
  // 鼠标侧键(后退键 = button 3):同一个栈。mouseup 上 preventDefault 一并拦掉 WebView 自带的历史后退。
  document.addEventListener("mouseup", (event) => {
    if (event.button !== 3) return;
    event.preventDefault();
    if (backAvailable() && !surfaceBlocksNav()) void navigate_back();
  });
  document.addEventListener("kz:language", () => syncBackBars());
  // 切视图后焦点不能掉到 body(UX-124):原来那一页里聚焦的元素随旧视图隐藏,键盘用户只好从页首重新 Tab。
  document.addEventListener("kz:view-changed", (event) => focusMainAfterViewChange(event.detail?.view));
  // 侧栏「新对话」在非对话页(需求 / 概览 / 文件…)点下去:动作照旧由 startNewConversation 做,这里负责把页面带回对话,
  // 不然只在后台悄悄建了条讨论(UX-005)。研究空间的新建课题会话自己会跳,不在此处理。
  $("new-chat")?.addEventListener("click", () => { if (active_space === "dev") ensureChatView(); });
});

// 焦点丢了(落在 body / 已摘除 / 已隐藏的元素上)才接手:对话页回输入框,其它页落到视图根(tabindex=-1,不画描边)。
// 焦点还在一个看得见的元素上(比如刚点的 rail 按钮)就不动;模态与菜单开着时归它们管。
function focusIsLost() {
  const active = document.activeElement;
  if (!active || active === document.body || active === document.documentElement) return true;
  return !active.isConnected || !(active.offsetParent || active.getClientRects?.().length);
}
export function focusMainAfterViewChange(view = document.body.dataset.view) {
  requestAnimationFrame(() => {
    if (!view || document.body.dataset.view !== view || !focusIsLost() || surfaceBlocksNav()) return;
    if (view === "chat") {
      const prompt = $("prompt");
      if (prompt && !prompt.disabled) { prompt.focus({ preventScroll: true }); return; }
    }
    const host = $(`view-${view}`);
    if (!host) return;
    if (!host.hasAttribute("tabindex")) host.setAttribute("tabindex", "-1");
    host.focus({ preventScroll: true });
  });
}

// 只绑带 data-view 的按钮:rail 上还有侧栏开合这类布局开关,它们不是视图。
export function navigate_view(view, { prepared = false, reload = false } = {}) {
  if (!prepared && view === "chat" && active_space === "dev" && !currentProject && !workbenchProject()) return openGeneralChat();
  if (!prepared && ["workspace", "settings"].includes(view)) cancelProjectNavigation();
  // A project selected in the workbench is only a browsing identity. Activate
  // execution explicitly before a project tool can use currentProject.
  if (!prepared && !["workspace", "settings", "project"].includes(view) && active_space === "dev"
      && workbenchProject() && (currentProject !== workbenchProject() || (view === "chat" && !activeSessionId))) {
    return openProjectSpace(workbenchProject(), view);
  }
  if (!view_allowed(view)) { notifyViewUnavailable(); return; }
  const item = document.querySelector(`.activity-item[data-view="${view}"]`);
  if ((!item && view !== "project") || !$(`view-${view}`)) return;
  document.body.dataset.view = view;
  reconcileWorkbenchView(view);
  // UI2-0926 #14:后台任务侧栏只在对话视图显示。切视图只触发重算、不改侧栏状态——此前只给面板加
  // hidden、开关状态不变,下一条工具事件又把它弹回来,浮在文件等视图上(缺陷 A)。切回对话按状态恢复。
  reconcileTasksPanel();
  remember_workspace_view(view);
  document.querySelectorAll(".activity-item[data-view]").forEach((i) => {
    i.classList.remove("active");
    i.removeAttribute("aria-current");
  });
  item?.classList.add("active");
  item?.setAttribute("aria-current", "page");
  document.body.classList.toggle("documents-active", view === "documents");
  // 已经在这个视图里就别再重载一遍:设置页尤其致命——再点一次侧栏图标,
  // 填了一半没保存的表单会静悄悄回滚成磁盘值。
  const previousView = document.querySelector(".view.active")?.id;
  if (previousView === `view-${view}` && !reload) { navCurrent ??= navSnapshot(view); return; }
  trackNavigation(view);
  clearTimeout(viewLoadTimer);
  const generation = ++viewLoadGeneration;
  if (view !== "documents") {
    clearPendingJump();
    clearJumpReveal();
  }
  document.querySelectorAll(".view").forEach((v) => v.classList.remove("active"));
  $(`view-${view}`).classList.add("active");
  syncBackBars();
  if (view !== "files") filesViewLeft();
  document.dispatchEvent(new CustomEvent("kz:view-changed", { detail: { view } }));
  // 先让选中态和新页面绘制；快速连续切换只加载最后一个页面。
  requestAnimationFrame(() => {
    if (generation !== viewLoadGeneration) return;
    viewLoadTimer = setTimeout(() => {
      if (generation !== viewLoadGeneration) return;
      const loaders = { settings: loadSettings, workspace: refreshWorkspace, project: () => renderProjectOverview(lastWorkspaceSnapshot), documents: refreshDocs,
        research: refreshResearch, memory: refreshMemory, metrics: refreshMetrics,
        files: showFilesView, arch: refreshArch, lines: refreshLines };
      const loading = loaders[view]?.();
      if (view === "lines") void refreshWorktrees();
      restoreScrollAfter(view, loading);
    }, 0);
  });
}
defer(() => {
  document.querySelectorAll(".activity-item[data-view]").forEach((item) => {
    item.addEventListener("click", (event) => {
      // 再点 ⚙ 返回(D10)。只认真人点击:别处用 `.click()` 程序化点它是「确保在设置页」,不能当成返回。
      if (item.dataset.view === "settings" && event.isTrusted && document.body.dataset.view === "settings") { void navigate_back(); return; }
      if (active_space === "research" && item.dataset.view === "chat") void open_research_chat();
      else navigate_view(item.dataset.view);
    });
  });
});

// ---------- toast ----------
// 一句话反馈。本地化在这里做,显示交给 00-surface.js 的 toast 区域(最多 3 条、err 用 role=alert);
// 长错误走 toastError → 日志面板,不交给会自动消失的 toast。kind: info|ok|warn|err。
export let errorRetry = null;
export function toast(text, { kind = "info", action } = {}) {
  const source = String(text);
  const translated = Object.prototype.hasOwnProperty.call(I18N_EN, source) ? t(source) : source;
  return surfaceToast(localizeDynamic(translated), { kind, action });
}
export function reportPersistentError(text, { retry = null } = {}) {
  log(text, "err");
  errorRetry = retry;
  $("log-retry").classList.toggle("hidden", typeof retry !== "function");
  $("log-panel").classList.remove("hidden");
  scrollLogToEnd();
}
export function toastError(text, options = {}) {
  reportPersistentError(text, options);
}

export let completionAudioContext = null;
export const baseTitle = document.title;

// R-187:提示音配置——总开关 + 分事件开关 + 音量(0-1)。持久化在 app.json 的 ui_layout.prefs.sound
// (与语言、主题同一通道;localStorage 在本机重启即丢,D-404,旧值只在 ui_layout 里还没有时读一次),
// 设置页「提示音」区块可改,默认全部开启、音量 0.12(与原固定音量一致)。
export const SOUND_STORAGE_KEY = "kz-sound-settings"; // 旧版 localStorage 键,只读一次做迁移
export function readSoundSettings() {
  const parsed = prefObject("sound", SOUND_STORAGE_KEY);
  if (parsed) {
    return {
      enabled: parsed.enabled !== false,
      volume: Number.isFinite(parsed.volume) ? Math.min(1, Math.max(0, parsed.volume)) : 0.12,
      completed: parsed.completed !== false,
      failed: parsed.failed !== false,
      stopped: parsed.stopped !== false,
    };
  }
  return { enabled: true, volume: 0.12, completed: true, failed: true, stopped: true };
}
export function saveSoundSettings(settings) {
  setPrefObject("sound", settings);
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
// 系统通知不可用只是「完成提示留在应用内」的说明,不是故障:写日志留底 + 一条 warn toast(按字数多停一会儿),
// 不再强行把日志面板弹开压在输入区下面(UX-045)。
export function explainNotificationFallback(message) {
  log(message, "warn");
  toast(message, { kind: "warn" });
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
// 活动面板与子代理面板合成了后台任务侧栏(UI2-0926 #14),开关、自动开合与徽标归 06-agent-panel.js。

// 侧栏收起:`sidebarCollapsed` 是**当下生效**的状态;用户偏好另存 ui_layout.shell.sidebar_collapsed(app.json,
// 本机 localStorage 重启即丢,D-404),旧 localStorage 键只在 ui_layout 还没有值时读一次。偏好只记**宽窗口下**
// 用户自己的开合;窄窗(≤900px)自动收起和悬浮抽屉的开合都是临时状态,不写偏好(UX-051)——否则窄窗缩一下,
// 宽屏回来侧栏还是收着的。ui_layout 的读取放在 defer 里:03-layout 与本模块处在同一条 import 环上,顶层读会撞 TDZ。
export let sidebarCollapsed = false;
export function setSidebarCollapsed(value) { sidebarCollapsed = Boolean(value); }
function readSidebarPreference() {
  const saved = layoutPref("shell", "sidebar_collapsed");
  if (typeof saved === "boolean") return saved;
  try { return localStorage.getItem("kz-sidebar-collapsed") === "1"; } catch { return false; }
}
function applySidebarState(collapsed, { remember }) {
  sidebarCollapsed = Boolean(collapsed);
  if (remember) setLayoutPref("shell", "sidebar_collapsed", sidebarCollapsed);
  syncSidebar();
}
/// 把侧栏展开(命令面板「切换项目」要聚焦侧栏项目列表时用)。窄窗里是临时展开,不改偏好。
export function expandSidebar() {
  if (sidebarCollapsed) applySidebarState(false, { remember: !sidebarOverlayQuery.matches });
}
export function syncSidebar() {
  const sidebar = $("sidebar");
  const hadFocus = sidebarCollapsed && sidebar.contains(document.activeElement);
  sidebar.classList.toggle("collapsed", sidebarCollapsed);
  // 收起 = 里面所有控件不可聚焦、读屏也读不到(UX-124):只靠宽度 0 / CSS 隐藏不够,工作台导航当年就漏了 9 个 Tab 停靠点。
  sidebar.inert = sidebarCollapsed;
  // 焦点正在被收起的侧栏里:交给主区,别掉到 body。
  if (hadFocus) focusMainAfterViewChange();
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
  if (!state.running) stopElapsed(sessionId);
  renderProjectActivity();
  document.dispatchEvent(new CustomEvent("kz:session-state-changed", { detail: { sessionId } }));
  // 「新对话」按钮 title 按活动线忙闲说明点下去会怎样:相位一变就跟上(只在值变时写)。
  if (sessionId === activeSessionId) syncNewChatEnabled();
  return state;
}
export function toggleSidebar() {
  applySidebarState(!sidebarCollapsed, { remember: !sidebarOverlayQuery.matches });
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
  // 点在 click 上收,不是 pointerdown:抽屉一开,主区被加了 padding-left 让位(style.css 900px 断点);按下时就收起来,
  // 主区立刻缩回去、按钮在松手前挪了位置,这一下 click 就落空——「抽屉开着点主区按钮,首击被吞、要点两次」(UX-132)。
  // click 在按钮自己的处理器跑完之后才到 document,此时再收,点击已经生效。
  document.addEventListener("click", (event) => {
    if (sidebarCollapsed || !sidebarOverlayQuery.matches) return;
    if (event.target.closest("#sidebar, #activitybar")) return;
    applySidebarState(true, { remember: false });
  });
});
/// 进入悬浮态(≤900px)时先把抽屉收起来。悬浮的侧栏 z-index 高于输入区上下文行,
/// 展开着就把「当前项目 / 模型 / 思考强度」整条盖掉——顶栏删除后那一行是**唯一**
/// 能看到自己在对哪个项目、用哪个模型说话的地方,被盖住时 Ctrl+Enter 是盲发。
/// 只在**跨过断点的那一刻**收,不动用户在宽窗口下的选择;这是临时收起,**不写偏好**(UX-051)。
export function collapseSidebarForOverlay(matches) {
  if (!matches || sidebarCollapsed) return;
  applySidebarState(true, { remember: false });
}
/// 回到宽窗口:侧栏恢复成用户自己的偏好(窄窗里临时收起 / 展开都不算数)。
export function restoreSidebarForWide(matches) {
  if (matches) return;
  applySidebarState(readSidebarPreference(), { remember: false });
}
defer(() => {
  if (typeof sidebarOverlayQuery.addEventListener === "function") {
    sidebarOverlayQuery.addEventListener("change", (event) => {
      if (event.matches) collapseSidebarForOverlay(true);
      else restoreSidebarForWide(false);
    });
  };
});
defer(() => {
  // 启动:先按用户偏好落地,再按当前宽度做窄窗收起。后端 ui_layout 稍后到达时按偏好重放一次(窄窗不动)。
  applySidebarState(readSidebarPreference(), { remember: false });
  collapseSidebarForOverlay(sidebarOverlayQuery.matches);
  onLayoutChange((section) => {
    if ((section === "*" || section === "shell") && !sidebarOverlayQuery.matches) applySidebarState(readSidebarPreference(), { remember: false });
  });
});
// 悬浮抽屉里选了项目 / 新对话 / 切了会话,抽屉就该让开:不然它盖着刚选中的内容(UX-152)。
// 点的是展开 / 收起类控件(带 aria-expanded)不算选择。会话行由别处重建,这里只认「侧栏里刚点过 + 随后选中了会话」。
let lastSidebarClick = 0;
defer(() => {
  // 捕获阶段记「刚点过侧栏」:会话行的点击由列表根上的委托同步处理、同步派发 kz:conversation-selected,
  // 冒泡到 #sidebar 时才记就晚了一步(抽屉里点会话收不起来)。
  $("sidebar")?.addEventListener("click", () => { lastSidebarClick = Date.now(); }, true);
  $("sidebar")?.addEventListener("click", (event) => {
    if (!sidebarOverlayQuery.matches || sidebarCollapsed) return;
    const target = event.target;
    if (target?.closest?.("[aria-expanded]")) return;
    if (target?.closest?.(".workbench-project-link, .workbench-nav-link, #new-chat")) setTimeout(() => collapseSidebarForOverlay(true), 0);
  });
  document.addEventListener("kz:conversation-selected", () => {
    if (Date.now() - lastSidebarClick < 1500) collapseSidebarForOverlay(sidebarOverlayQuery.matches);
  });
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
/// 日志面板显示出来之后再滚到最新一行:面板隐藏(display:none)时 log() 里那句 scrollTop 赋值落空,
/// 弹出来的面板会停在最旧一行,刚发生的错误在视口外(UX-011)。
function scrollLogToEnd() {
  const lines = $("log-lines");
  if (lines) lines.scrollTop = lines.scrollHeight;
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
  $("log-toggle").addEventListener("click", () => {
    $("log-panel").classList.toggle("hidden");
    scrollLogToEnd();
  });
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
  if (turnPhase !== "stopping" && pendingQuestionSessions.has(activeSessionId)) return "attention";
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
  const text = activity === "attention" ? t("待你回复") : localizeDynamic(statusTextSource) || t("运行中");
  if (label && label.textContent !== text) label.textContent = text;
  renderTurnElapsed();
}
export function renderTurnElapsed() {
  const el = $("turn-activity-elapsed");
  const clock = runClocks.get(activeSessionId);
  const text = elapsedTimer && clock && !clock.ended && !pendingQuestionSessions.has(activeSessionId)
    ? formatRunElapsed(clockElapsedMs(clock)) : "";
  if (el && el.textContent !== text) el.textContent = text;
  const status = $("status-elapsed");
  if (status) status.textContent = text ? "· " + text : "";
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
  const awaiting = activityKey() === "attention";
  $("status-text").textContent = localizeDynamic(statusTextSource);
  if (awaiting) $("status-text").textContent = t("待你回复");
  $("status-mode").textContent = statusRunning ? t("运行中") : t("空闲");
  if (awaiting) $("status-mode").textContent = t("待你回复");
  // 去重:空闲时两格都是「空闲」、刚开跑时都是「运行中」——同一个词不在状态栏并排写两遍。
  $("status-text").classList.toggle("hidden", $("status-text").textContent === $("status-mode").textContent);
  $("statusbar").classList.toggle("running", statusRunning && !awaiting);
  syncElapsed();
  renderTurnActivity();
}

// 运行计时 + 首响应看门狗:等太久时把"卡在哪"讲清楚。
export let runStart = 0;
export let firstSignal = false;
export let elapsedTimer = null;
let displayedClock = null;
// A clock belongs to an observed run, never to whichever project happens to be open.
// Monotonic time excludes clock adjustments; waiting for a human pauses accumulation.
const runClocks = new Map();
export let pendingQuestionSessions = new Set();
export let questionWaitRevision = 0;
let pendingClockQuestions = new Map();
const clockNow = () => globalThis.performance?.now?.() ?? Date.now();
const clockElapsedMs = clock => clock.elapsed + (clock.since === null ? 0 : Math.max(0, clockNow() - clock.since));
function pauseClock(clock) {
  if (!clock || clock.since === null) return;
  clock.elapsed = clockElapsedMs(clock); clock.since = null;
}
export function formatRunElapsed(ms) {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  if (seconds < 60) return seconds + "s";
  const part = n => String(n).padStart(2, "0");
  return seconds < 3600 ? Math.floor(seconds / 60) + ":" + part(seconds % 60)
    : Math.floor(seconds / 3600) + ":" + part(Math.floor(seconds / 60) % 60) + ":" + part(seconds % 60);
}
function projectQuestionWaits() {
  const before = pendingQuestionSessions;
  pendingQuestionSessions = new Set([...pendingClockQuestions.values()].map(q => q.sessionId));
  for (const sid of new Set([...before, ...pendingQuestionSessions])) {
    const clock = runClocks.get(sid);
    if (pendingQuestionSessions.has(sid)) pauseClock(clock);
    else if (clock && !clock.ended && clock.since === null) clock.since = clockNow();
  }
  // Keep the execution controls truthful: the backend may still own a suspended tool.
  setStatus(statusTextSource, statusRunning);
}
export function replacePendingQuestions(rows, revision = questionWaitRevision) {
  // A reply/live ask arriving during a poll wins over that older snapshot.
  if (revision !== questionWaitRevision) return false;
  pendingClockQuestions = new Map(rows.filter(q => q.sessionId && !q.background && !q.agentId).map(q => [q.sessionId + ":" + q.id, q]));
  projectQuestionWaits();
  return true;
}
export function noteQuestionReply(sessionId, id) {
  questionWaitRevision += 1;
  pendingClockQuestions.delete(sessionId + ":" + id);
  projectQuestionWaits();
}
export function trackRunElapsed(event, payload) {
  const sid = payload?.sessionId;
  if (!sid) return;
  if (sid !== activeSessionId && ["kz:text", "kz:reasoning", "kz:tool-start"].includes(event) && runClocks.has(sid)) runClocks.get(sid).firstSignal = true;
  if (event === "kz:turn") {
    if (payload.step === 1 || !runClocks.has(sid) || runClocks.get(sid).ended) startElapsed(sid);
  } else if (event === "kz:ask" && payload.kind === "question" && !payload.background && !payload.agentId && !["parallel", "autonomous"].includes(payload.source)) {
    questionWaitRevision += 1;
    pendingClockQuestions.set(sid + ":" + payload.id, payload);
    projectQuestionWaits();
  } else if (["kz:question-replied", "kz:ask-resolved"].includes(event)) noteQuestionReply(sid, payload.id);
  else if (["kz:done", "kz:idle", "kz:stopped"].includes(event) || event === "kz:error" && payload.terminal !== false) {
    stopElapsed(sid);
    questionWaitRevision += 1;
    for (const [key, q] of pendingClockQuestions) if (q.sessionId === sid) pendingClockQuestions.delete(key);
    projectQuestionWaits();
  }
}
export function roundElapsedSeconds(reportedMs) {
  const elapsedMs = Number(reportedMs);
  if (Number.isFinite(elapsedMs) && elapsedMs >= 0) return elapsedMs / 1000;
  const clock = runClocks.get(activeSessionId);
  if (runStart <= 0 || !clock) return null;
  return clockElapsedMs(clock) / 1000;
}
export function startElapsed(sessionId = activeSessionId) {
  if (!sessionId) return;
  runClocks.set(sessionId, { startedAt: Date.now(), since: pendingQuestionSessions.has(sessionId) ? null : clockNow(), elapsed: 0, firstSignal: false, ended: false });
  if (sessionId === activeSessionId) syncElapsed();
}
export function syncElapsed() {
  const clock = runClocks.get(activeSessionId);
  runStart = clock?.startedAt || 0;
  firstSignal = clock?.firstSignal || false;
  const ticking = statusRunning && clock && !clock.ended && clock.since !== null;
  if (ticking && displayedClock === clock && elapsedTimer) { renderTurnElapsed(); return; }
  clearInterval(elapsedTimer);
  elapsedTimer = null; displayedClock = clock;
  if (!ticking) { renderTurnElapsed(); return; }
  elapsedTimer = setInterval(() => {
    const secs = Math.floor(clockElapsedMs(clock) / 1000);
    renderTurnElapsed();
    if (!clock.firstSignal && secs > 0 && secs % 15 === 0) {
      log(`${t("仍在等待模型首个响应")}(${t("已")} ${secs}s)——${t("订阅高峰或网络较慢时属正常")};${t("超时上限")} 15s ${t("连接")} / 180s ${t("读")}`, "warn");
    }
  }, 1000);
  renderTurnElapsed();
}
export function stopElapsed(sessionId = activeSessionId) {
  const clock = runClocks.get(sessionId);
  if (clock) { pauseClock(clock); clock.ended = true; }
  if (sessionId === activeSessionId) syncElapsed();
}
export function markFirstSignal() {
  // R-267:首响应计时属于活动会话的这一轮,后台会话的事件不参与。
  if (typeof renderingBackground !== "undefined" && renderingBackground) return;
  const clock = runClocks.get(activeSessionId);
  if (clock && !clock.firstSignal) {
    clock.firstSignal = true;
    firstSignal = true;
    log(`${t("模型开始响应")}(${(clockElapsedMs(clock) / 1000).toFixed(1)}s)`);
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
  // profile 是后端的原始档位名(dev/research):中文界面写「开发」「研究」,不把英文枚举摆给人看。
  if (meta.profile) parts.push({ dev: t("开发"), research: t("研究") }[meta.profile] ?? meta.profile);
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
// 状态栏读数的紧凑写法:≥1000 用 k,小数一位(148200 → 148.2k)。
function compactTokens(value) {
  const n = Number(value) || 0;
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);
}
// UX-044:状态栏右侧空间有限、尾部会被省略号吃掉,所以把**最要紧的上下文占用放最前面**(用词也改成中文,
// 带占比),本轮用量排在后面;完整数字在点开的「上下文成分」气泡里。
export function renderTokens() {
  const tokens = runTokens;
  const usage = tokens.input + tokens.output === 0
    ? ""
    : [
      `${t("本轮输入")} ${compactTokens(tokens.input)}`,
      tokens.cacheRead > 0 ? `${t("缓存读")} ${compactTokens(tokens.cacheRead)}` : "",
      tokens.cacheWrite > 0 ? `${t("缓存写")} ${compactTokens(tokens.cacheWrite)}` : "",
      `${t("本轮输出")} ${compactTokens(tokens.output)}`,
    ].filter(Boolean).join(" · ");
  let head = "";
  const bar = $("ctx-bar");
  if (ctxTokens > 0) {
    const k = (ctxTokens / 1000).toFixed(1);
    const pending = ctxPending ? ` (${t("等待模型")})` : "";
    if (ctxLimit) {
      const pct = Math.round((ctxTokens / ctxLimit) * 100);
      head = `${t("上下文")} ${k}k/${Math.round(ctxLimit / 1000)}k (${pct}%)${pending}`;
      $("status-tokens").classList.toggle("ctx-warn", pct >= 70);
      // 进度条:容量占用一眼可见,≥70% 变警示色;压缩由运行器的实际预算决定。
      bar.classList.remove("hidden");
      bar.classList.toggle("warn", pct >= 70);
      bar.classList.toggle("pending", ctxPending);
      $("ctx-bar-fill").style.width = `${Math.min(pct, 100)}%`;
      bar.title = `${t("上下文")} ${k}k / ${Math.round(ctxLimit / 1000)}k(${pct}%)`;
    } else {
      head = `${t("上下文")} ${k}k${pending}`;
      bar.classList.add("hidden");
    }
  } else {
    bar.classList.add("hidden");
    head = ctxPending ? `${t("上下文")} ${t("等待模型")}` : "";
  }
  // 主读数(占用/上限/占比)与「本轮输入/缓存/输出」用量分两段:状态栏窄时(CSS 里 flex-wrap + 溢出裁掉)先让用量那段
  // 让位,主读数不再被截成「279.2k/40…」(UX-044 验收缺陷);完整数字在点开的「上下文成分」气泡与按钮提示里。
  const button = $("status-tokens");
  button.textContent = "";
  for (const [cls, value, lead] of [["ctx-main", head, ""], ["ctx-usage", usage, head ? " · " : ""]]) {
    if (!value) continue;
    const span = document.createElement("span");
    span.className = cls;
    span.textContent = lead + value;
    button.append(span);
  }
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
  const titleKey = active_space === "research" ? "新建课题对话，保留已有对话" : isGeneralChat() || !currentProject || document.body.dataset.appScope === "global" ? "新建对话，保留已有对话"
    : "新建只读讨论，主对话继续执行";
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
  // 画布类视图(记忆图谱)不走 CSS 级联,靠这个事件重读 --graph-* token 后重画。
  document.dispatchEvent(new CustomEvent("kz:theme", { detail: { theme } }));
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
      ? `✓ ${t("fast 模型就绪")}`
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

document.getElementById("workbench-schedules")?.addEventListener("click", () => { void showSchedules().catch(error => toastError(String(error))); });
