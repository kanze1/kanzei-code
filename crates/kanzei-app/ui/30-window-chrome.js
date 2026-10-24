import { $ } from "./00-dom.js";
import { t } from "./02-i18n.js";
import { toastError } from "./03-shell.js";
import { leaveGuardStatus, onLeaveGuardChange } from "./00-leave-guard.js";

// The main window has one chrome, independent of the active view. Its close
// button requests the normal native close event; 00-leave-guard owns decisions.
export function installWindowChrome(workspaceHeader) {
  const chrome = document.createElement("header");
  chrome.id = "window-chrome";
  const mark = document.createElement("div"); mark.className = "window-app-mark";
  const logo = document.createElement("img"); logo.src = "assets/kanzei.svg"; logo.alt = "kanzei"; logo.draggable = false;
  mark.append(logo);
  const status = document.createElement("span"); status.id = "window-chrome-status"; status.setAttribute("role", "status"); status.hidden = true;
  const controls = document.createElement("div"); controls.id = "window-controls"; controls.setAttribute("role", "group");
  const buttons = {};
  const icons = {
    minimize: '<path d="M3 8h10"/>',
    maximize: '<rect x="3.5" y="3.5" width="9" height="9"/>',
    restore: '<path d="M5.5 5.5v-2h7v7h-2"/><rect x="3.5" y="5.5" width="7" height="7"/>',
    close: '<path d="m3.5 3.5 9 9m0-9-9 9"/>',
  };
  let maximized = false, busy = false, stateRequest = 0;
  const win = window.__TAURI__?.window?.getCurrentWindow?.();
  const available = Boolean(win?.minimize && win?.toggleMaximize && win?.startDragging && win?.close);
  const icon = name => `<svg aria-hidden="true" viewBox="0 0 16 16">${icons[name]}</svg>`;
  function relabel() {
    controls.setAttribute("aria-label", t("窗口控制"));
    for (const [action, key] of [["minimize", "最小化"], ["maximize", maximized ? "还原" : "最大化"], ["close", "关闭窗口"]]) {
      const button = buttons[action];
      button.setAttribute("aria-label", t(key)); button.title = t(key);
      button.disabled = !available || busy || (action === "close" && (!leaveGuardStatus.ready || leaveGuardStatus.closing));
      if (!available) button.title = t("窗口控制仅在桌面应用中可用");
      else if (action === "close" && !leaveGuardStatus.ready) button.title = leaveGuardStatus.error || t("正在准备关闭保护");
    }
    buttons.maximize.innerHTML = icon(maximized ? "restore" : "maximize");
    chrome.dataset.maximized = String(maximized);
  }
  function report(error) {
    status.textContent = `${t("窗口操作失败")}: ${String(error)}`; status.title = status.textContent; status.hidden = false;
    toastError(status.textContent);
  }
  async function syncMaximized() {
    const request = ++stateRequest;
    const value = await win.isMaximized();
    if (request !== stateRequest) return;
    maximized = value; relabel();
  }
  async function act(action) {
    if (!available || busy) return;
    busy = true; status.hidden = true; relabel();
    try {
      await win[action]();
      if (action === "toggleMaximize") await syncMaximized();
    } catch (error) { report(error); }
    finally { busy = false; relabel(); }
  }
  for (const [action, method] of [["minimize", "minimize"], ["maximize", "toggleMaximize"], ["close", "close"]]) {
    const button = document.createElement("button"); button.id = `window-${action}`; button.type = "button"; button.className = "window-control";
    button.innerHTML = icon(action); button.addEventListener("click", () => void act(method));
    buttons[action] = button; controls.append(button);
  }
  chrome.append(mark, workspaceHeader, status, controls); $("app").before(chrome);
  document.body.classList.add("has-window-chrome");
  // Do not annotate ancestors with data-tauri-drag-region: that would turn
  // descendant navigation and inputs into native drag targets as well.
  const interactive = 'button, input, textarea, select, a, nav, [role="button"], [role="tab"], [role="menuitem"], [contenteditable]:not([contenteditable="false"]), [data-no-window-drag]';
  chrome.addEventListener("mousedown", event => {
    if (event.button !== 0 || event.target.closest(interactive)) return;
    event.preventDefault();
    // Tauri's title-bar gesture uses mousedown.detail, not a later dblclick
    // (the native move loop may consume that browser event).
    void act(event.detail === 2 ? "toggleMaximize" : "startDragging");
  });
  onLeaveGuardChange(relabel); document.addEventListener("kz:language", relabel); relabel();
  if (available) {
    void syncMaximized().catch(report);
    void win.onResized(() => { void syncMaximized().catch(report); }).catch(report);
    window.addEventListener("focus", () => { void syncMaximized().catch(report); });
  }
}
