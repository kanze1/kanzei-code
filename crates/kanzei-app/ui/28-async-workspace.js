import { $, confirmDialog, defer, invoke, on, promptBox, uiPrefsLoad } from "./01-core.js";
import { t } from "./02-i18n.js";
import { openPopover, openMenu, closeSurface } from "./00-surface.js";
import { installFrame } from "./00-frame.js";
import { activeProcessId, activeSessionId, currentProject, attachments, toastError } from "./03-shell.js";
import { refreshProcesses, projectDisplayName, renderProjects, lastProjectPrefs } from "./09-sessions.js";
import { loadConversation } from "./15-views-misc.js";
import { active_space, restore_workspace_preferences, restore_active_workspace, sync_workspace_visibility } from "./03-workspaces.js";
import { workbenchNavigationGuard, openProjectSpace } from "./12-workbench.js";
import { refreshWorkspace } from "./12-docs-pages.js";
import { openGeneralChat } from "./03-general-scope.js";

const node = (tag, cls, text = "") => { const el = document.createElement(tag); el.className = cls; el.textContent = text; return el; };
// 静态文案:建面板时按当前语言写入,同时挂 data-i18n-key——切语言时 applyDataI18nKeys 会按 key 重算(UX-155:原先这里全是裸中文)。
const label = (tag, cls, key) => { const el = node(tag, cls, t(key)); el.dataset.i18nKey = key; return el; };
let panel, scope, tab = "question", body, input, status, send, requestBusy = false, visible = false, refreshSerial = 0;
let refreshTimer;
const drafts = new Map();
let runtimeSync = null;
function synchronizeRuntime() {
  if (runtimeSync) return runtimeSync;
  runtimeSync = (async () => {
    const recoverStartup = !currentProject && !lastProjectPrefs.projects?.length;
    const isCurrent = workbenchNavigationGuard();
    const prefs = await invoke("projects_get");
    if (!isCurrent()) return;
    renderProjects(prefs, { activate: false });
    // An older, busy owner can reject the initial project/prefs requests. Once
    // it retires, restore the actual workspace instead of leaving an empty UI.
    if (recoverStartup && document.body.dataset.appReady === "true") {
      const settings = await uiPrefsLoad(true);
      await restore_workspace_preferences();
      if (!isCurrent()) return;
      sync_workspace_visibility();
      await refreshWorkspace();
      await restore_active_workspace({ isCurrent });
      if (!isCurrent() || active_space !== "dev") return;
      if (settings.ui_layout?.prefs?.conversation_mode === "general" || !prefs.projects?.length) await openGeneralChat();
      else if (prefs.current) await openProjectSpace(prefs.current);
    } else {
      await refreshProcesses();
      if (isCurrent() && document.body.dataset.appReady === "true") await loadConversation(null, null, true);
    }
  })().finally(() => { runtimeSync = null; });
  return runtimeSync;
}
function args() { return { projectDir: scope.project, processId: scope.process }; }
function scopeKey() { return `${scope.project}|${scope.process || ""}`; }
function sameScope(value) { return value.sessionId === scope?.session; }
function build() {
  if (panel) return;
  panel = node("section", "async-panel k-surface k-card hidden"); panel.id = "async-workspace"; panel.setAttribute("role", "dialog"); panel.setAttribute("aria-label", t("旁路提问与终端日志")); panel.dataset.i18nAriaLabel = "旁路提问与终端日志";
  const head = node("header", "async-head");
  const title = node("strong", "async-title"); title.id = "async-recipient";
  const close = node("button", "ghost", "×"); close.setAttribute("aria-label", t("收起")); close.dataset.i18nAriaLabel = "收起"; close.onclick = () => closeSurface(panel);
  head.append(title, close);
  const tabs = node("nav", "async-tabs");
  for (const [key, name] of [["question", "旁路提问"], ["logs", "终端日志"]]) {
    const button = label("button", "ghost", name); button.dataset.tab = key;
    button.onclick = () => { tab = key; void refresh(); }; tabs.append(button);
  }
  body = node("div", "async-body"); body.setAttribute("aria-live", "polite");
  const form = node("form", "async-compose");
  input = node("textarea", "async-input"); input.rows = 2; input.placeholder = t("旁路提一个问题，主任务继续运行…"); input.dataset.i18nPlaceholder = "旁路提一个问题，主任务继续运行…";
  input.setAttribute("aria-label", t("旁路提问")); input.dataset.i18nAriaLabel = "旁路提问";
  input.oninput = () => drafts.set(scopeKey(), input.value);
  status = node("span", "muted"); status.setAttribute("role", "status");
  send = label("button", "primary", "发送"); send.type = "submit";
  const foot = node("div", "async-compose-footer"); foot.append(status, send); form.append(input, foot);
  form.onsubmit = event => { event.preventDefault(); void submit(); };
  input.onkeydown = event => { if (event.key === "Enter" && !event.shiftKey && !event.isComposing) { event.preventDefault(); void submit(); } };
  panel.append(head, tabs, body, form); document.body.append(panel);
  installFrame(panel, { id: "async-workspace", move: ".async-head", min: "280 160" });
}
export async function openAsyncWorkspace(initial = "", selected = "question") {
  if (!currentProject || !activeSessionId) return;
  build(); scope = { project: currentProject, process: activeProcessId, session: activeSessionId }; tab = selected;
  $("async-recipient").textContent = `${projectDisplayName(currentProject)} · ${activeProcessId?.startsWith("d|") ? t("主对话") : t("当前对话")}`;
  input.value = initial || drafts.get(scopeKey()) || ""; visible = true;
  openPopover(null, panel, { manual: true, onClose: () => { visible = false; ++refreshSerial; } });
  await refresh(); if (selected === "question") input.focus();
}
async function submit() {
  const question = input.value.trim(); if (!question || requestBusy) return;
  const target = { ...args() }, key = scopeKey(); requestBusy = true; send.disabled = true; status.textContent = t("发送中…");
  try {
    await invoke("side_question_send", { ...target, id: crypto.randomUUID(), question });
    if (scopeKey() === key && input.value.trim() === question) { input.value = ""; drafts.delete(key); }
    status.textContent = ""; await refresh();
  } catch (error) { status.textContent = String(error); }
  finally { requestBusy = false; send.disabled = false; }
}
function renderQuestions(values) {
  body.replaceChildren();
  for (const value of values) {
    const row = node("article", "async-answer"); row.dataset.id = value.id;
    row.append(node("div", "async-question", value.question), node("div", "async-answer-text", value.answer || t("思考中…")));
    const state = node("small", "muted", ({ running: t("回答中"), stopped: t("已停止"), failed: t("未完成") })[value.status] || "");
    if (value.status === "running") { const stop = node("button", "ghost mini", t("停止回答")); stop.onclick = async () => { try { await invoke("side_question_stop", { ...args(), id: value.id }); } catch (e) { toastError(e); } }; row.append(stop); }
    row.append(state); body.append(row);
  }
  if (!values.length) body.append(node("p", "muted", t("可参考当前对话提问。回答留在这里。")));
}
function renderLogs(values) {
  body.replaceChildren();
  for (const value of values) {
    const row = node("article", "async-terminal"); const head = node("div", "async-terminal-head");
    // 头里写「终端 N」,内部 id(bg-1…)只放在悬停提示里(UX-127)。
    const ordinal = /(\d+)\s*$/.exec(String(value.id ?? ""))?.[1];
    const idLabel = node("code", "", ordinal ? `${t("终端")} ${ordinal}` : String(value.id ?? "")); idLabel.title = String(value.id ?? "");
    head.append(idLabel, node("small", value.running ? "is-active" : "muted", value.running ? t("运行中") : `${t("已结束")} ${value.exit ?? ""}`));
    const watch = node("button", "ghost mini", value.subscribed ? t("取消订阅") : t("订阅给主对话")); watch.disabled = !value.running && !value.subscribed;
    watch.onclick = async () => { watch.disabled = true; try { await invoke("terminal_monitor", { ...args(), action: value.subscribed ? "unwatch" : "watch", id: value.id }); await refresh(); } catch (error) { status.textContent = String(error); watch.disabled = false; } };
    head.append(watch); row.append(head, node("div", "muted async-command", value.command), node("pre", "async-log", value.output || t("等待输出…"))); body.append(row);
  }
  if (!values.length) body.append(node("p", "muted", t("后台终端启动后，日志会显示在这里。")));
}
async function refresh() {
  if (!visible || !scope) return;
  const serial = ++refreshSerial, selected = tab;
  panel.querySelector("form").hidden = selected !== "question";
  for (const button of panel.querySelectorAll("[data-tab]")) button.setAttribute("aria-pressed", String(button.dataset.tab === selected));
  try {
    const values = await invoke(selected === "question" ? "side_question_list" : "terminal_monitor", { ...args(), ...(selected === "logs" ? { action: "list" } : {}) });
    if (serial !== refreshSerial || !visible) return;
    const top = body.scrollTop, atEnd = body.scrollHeight - body.clientHeight - top < 40;
    if (selected === "question") renderQuestions(values || []); else renderLogs(values || []);
    body.scrollTop = atEnd ? body.scrollHeight : top;
  } catch (error) { if (serial === refreshSerial) status.textContent = String(error); }
}
defer(() => {
  $("side-question-open")?.addEventListener("click", () => void openAsyncWorkspace());
  $("terminal-monitor-open")?.addEventListener("click", () => void openAsyncWorkspace("", "logs"));
  promptBox.addEventListener("kz:compose-send", event => {
    const text = promptBox.value.trim(); if (!/^\/btw(?:\s|$)/.test(text)) return;
    event.preventDefault(); if (attachments.length) { toastError(t("旁路提问只接收文字，附件仍保留在主输入框")); return; }
    promptBox.value = ""; void openAsyncWorkspace(text.replace(/^\/btw\s*/, ""));
  });
  on("kz:side-question", event => {
    if (!visible || !sameScope(event.payload) || tab !== "question" || refreshTimer) return;
    refreshTimer = setTimeout(() => { refreshTimer = null; void refresh(); }, 120);
  });
  const badge = $("runtime-indicator");
  on("kz:runtime-resync", event => {
    if (event.payload.connected) {
      void synchronizeRuntime().catch(toastError);
      void refresh();
    }
    if (badge) { badge.dataset.connected = String(!!event.payload.connected); badge.title = event.payload.connected ? t("后台已连接；关闭界面后任务继续") : t("正在重连后台"); }
  });
  // 走 00-surface 的 openMenu(锚在 ◉ 上,k-menu 的定位/键盘/焦点都现成):此前手搭的 section + openPopover 缺 k-popover 与
  // placement,弹层落在窗口左上角(0,0)盖住侧栏图标、离按钮很远。
  badge?.addEventListener("click", () => {
    openMenu(badge, [
      { heading: t("关闭窗口后继续运行，重开会接回当前任务。") },
      {
        label: t("停止后台任务并退出"),
        danger: true,
        // UX-048:原先点开就直接停掉全部后台任务并退出,没有任何确认;这是不可逆的一击,先问一句(默认焦点在取消)。
        onSelect: async () => {
          const ok = await confirmDialog({ title: t("停止后台任务并退出"), message: t("会停止所有正在运行的任务并关闭 kanzei;下次打开不会接回它们。"), okText: t("停止并退出"), danger: true });
          if (!ok) return;
          try { await invoke("runtime_shutdown"); } catch (error) { toastError(error); }
        },
      },
    ], { placement: "top-end", label: t("后台常驻") });
  });
  setInterval(() => { if (visible && !document.hidden && tab === "logs") void refresh(); }, 2500);
  void invoke("runtime_status").then(value => { if (value?.mode === "detached" && badge) { badge.hidden = false; badge.dataset.connected = "true"; badge.title = t("后台已连接；关闭界面后任务继续"); } }).catch(() => {});
});
