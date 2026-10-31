import { subscribeRuntimeEvent } from "./00-event-subscriptions.js";
import { $, defer } from "./00-dom.js";
import { openMenu } from "./00-surface.js";
import { t } from "./02-i18n.js";
import { currentProject, activeProcessId, activeSessionId, sessionState, toastError } from "./03-shell.js";
import { isGeneralChat, rememberConversationMode } from "./03-general-scope.js";
import { layoutPref, setLayoutPref } from "./03-layout.js";
import { openGeneralChat } from "./29-general-chat.js";
import { cancelProjectNavigation, workbenchNavigationGuard } from "./12-workbench.js";
import { closePreviewDock } from "./24-preview.js";
import { refreshQq, qqSnapshot, openQqPeer, showQqSection } from "./33-qq.js";
import { refreshBili, biliSnapshot, openBiliConversation, showBiliSection, browseBili } from "./35-bili.js";
import { socialActive, setSocialActive, setSocialManagement } from "./34-social-state.js";

let platform = "qq", panelOpen = false, opening = 0;
const rootKey = root => String(root || "").replaceAll("\\", "/").replace(/\/$/, "").toLowerCase();
const peerKey = peer => `qq:${peer.identity.account}:${peer.identity.kind}:${peer.identity.peer}`;
function selectedPeer() { return qqSnapshot()?.peers.find(peer => rootKey(peer.root) === rootKey(currentProject) && peer.process_id === activeProcessId); }
function selectedBili() { return !!biliSnapshot()?.conversation_root && rootKey(biliSnapshot().conversation_root) === rootKey(currentProject); }
function announcePanel() { setSocialManagement(panelOpen && socialActive() ? platform : null); }
function sync() {
  if (currentProject && !isGeneralChat()) { setSocialActive(false); panelOpen = false; }
  const visible = socialActive() && document.body.dataset.view === "chat";
  $("social-toolbar").hidden = !visible; $("social-manager").hidden = !visible || !panelOpen;
  document.body.dataset.social = String(visible);
  $("view-chat").dataset.socialPanel = visible && panelOpen ? "open" : "closed";
  $("social-nav").classList.toggle("active", visible); $("social-nav").setAttribute("aria-pressed", String(visible));
  $("social-manage").setAttribute("aria-expanded", String(visible && panelOpen));
  const peer = selectedPeer(), bili = selectedBili();
  $("social-target-name").textContent = peer ? peer.title : bili ? t("B站浏览") : t("个人助手");
  $("social-target-kind").textContent = peer ? peer.identity.kind === "group" ? t("QQ 群聊") : t("QQ 好友") : bili ? t("B站") : t("无项目对话");
  const state = activeSessionId ? sessionState(activeSessionId) : null;
  $("social-live-state").textContent = state?.running ? `${t(state.stage || "运行中")}${state.detail ? ` ${state.detail}` : ""}` : state?.terminal_status ? t(state.terminal_status) : "";
  const busy = qqSnapshot()?.peers.some(item => sessionState(item.session_id).running) || selectedBili() && state?.running;
  $("social-nav").dataset.running = String(!!busy);
  $("social-qq-status").dataset.connected = String(!!qqSnapshot()?.connected);
  $("social-bili-status").dataset.connected = String(!!biliSnapshot()?.account);
  $("social-browse").hidden = !biliSnapshot()?.account;
  for (const name of ["qq", "bili"]) {
    $(`${name}-management`).hidden = platform !== name;
    const button = $(`social-platform-${name}`); button.classList.toggle("active", platform === name); button.setAttribute("aria-selected", String(platform === name));
  }
  if (visible) $("workbench-general-chat")?.classList.remove("active");
  announcePanel();
}
async function openTarget(target = "personal", { restore = false } = {}) {
  const own = ++opening; cancelProjectNavigation(); const valid=workbenchNavigationGuard(); setSocialActive(true);
  if (restore) await Promise.all([refreshQq(), refreshBili()]);
  if (own !== opening || !valid()) return false;
  const peer = qqSnapshot()?.peers.find(peer => peerKey(peer) === target);
  if (peer) { if(!await openQqPeer(peer)) return false; }
  else if (target === "bili" && biliSnapshot()?.account) { if(!await openBiliConversation()) return false; }
  else if (!await openGeneralChat()) { setSocialActive(false); sync(); return false; }
  if (own !== opening) return;
  setSocialActive(true); rememberConversationMode("social"); setLayoutPref("social", "last_target", peer ? peerKey(peer) : target === "bili" && biliSnapshot()?.account ? "bili" : "personal"); sync(); return true;
}
export async function openSocial() { await openTarget(layoutPref("social", "last_target") || "personal", { restore: true }); }
async function manage(nextPlatform, section) {
  if ((!socialActive() || document.body.dataset.view !== "chat") && !await openTarget()) return;
  closePreviewDock(); platform = nextPlatform; panelOpen = true; sync();
  if (platform === "qq") { await refreshQq(); showQqSection(section); }
  else { await refreshBili(); showBiliSection(section); }
  if ($("social-manager").contains(document.activeElement)) return;
  $("social-manager-close").focus({ preventScroll: true });
}
function closeManager() { panelOpen = false; sync(); if (socialActive()) $("social-manage").focus({ preventScroll: true }); }
function picker() {
  const peer = selectedPeer();
  const items = [{ label: t("个人助手"), desc: t("无项目对话"), checked: !peer && !selectedBili(), onSelect: () => { void openTarget().catch(error => toastError(String(error))); } }];
  for (const [kind, label] of [["friend", "QQ 好友"], ["group", "QQ 群聊"]]) {
    const peers = qqSnapshot()?.peers.filter(item => item.identity.kind === kind) || [];
    if (peers.length) { items.push({ heading: t(label) }); for (const item of peers) items.push({ label: item.title, desc: sessionState(item.session_id).running ? `${t(sessionState(item.session_id).stage || "运行中")} ${sessionState(item.session_id).detail || ""}` : item.identity.peer, checked: !!peer && peerKey(peer) === peerKey(item), onSelect: () => { void openTarget(peerKey(item)).catch(error => toastError(String(error))); } }); }
  }
  items.push("separator", biliSnapshot()?.account ? { label: t("B站浏览"), checked: selectedBili(), onSelect: () => { void openTarget("bili").catch(error => toastError(String(error))); } } : { label: t("登录 B站"), onSelect: () => { void manage("bili", "login").catch(error => toastError(String(error))); } });
  items.push({ label: t("管理连接"), onSelect: () => { void manage("qq", "connection").catch(error => toastError(String(error))); } });
  openMenu($("social-target"), items, { label: t("切换社交会话") });
}
defer(() => {
  $("social-nav").addEventListener("click", () => { void openSocial().catch(error => toastError(String(error))); });
  $("social-target").addEventListener("click", picker);
  $("social-manage").addEventListener("click", () => { if (panelOpen) closeManager(); else void manage(selectedBili() ? "bili" : "qq").catch(error => toastError(String(error))); });
  $("social-manager-close").addEventListener("click", closeManager);
  $("social-browse").addEventListener("click", () => { const button=$("social-browse");button.disabled=true;void browseBili().catch(error=>toastError(String(error))).finally(()=>{button.disabled=false;}); });
  for (const name of ["qq", "bili"]) $(`social-platform-${name}`).addEventListener("click", () => { void manage(name).catch(error => toastError(String(error))); });
  $("social-qq-status").addEventListener("click", () => { void manage("qq", "connection").catch(error => toastError(String(error))); });
  $("social-bili-status").addEventListener("click", () => { void manage("bili", "login").catch(error => toastError(String(error))); });
  $("workbench-general-chat")?.addEventListener("click", () => { setSocialActive(false); panelOpen = false; sync(); }, true);
  document.addEventListener("kz:social-manage-close", closeManager);
  document.addEventListener("kz:social-updated", sync);
  document.addEventListener("kz:session-state-changed", sync);
  document.addEventListener("kz:conversation-selected", sync);
  document.addEventListener("kz:view-changed", () => { if (document.body.dataset.view !== "chat") panelOpen = false; sync(); });
  document.addEventListener("kz:language", sync);
  $("social-manager").addEventListener("keydown", event => { if (event.key === "Escape") { event.preventDefault(); closeManager(); } });
  new ResizeObserver(() => { $("view-chat").dataset.socialNarrow = String($("view-chat").getBoundingClientRect().width < 940); }).observe($("view-chat"));
  new MutationObserver(() => { if ($("view-chat").dataset.preview === "open" && panelOpen) closeManager(); }).observe($("view-chat"), { attributes: true, attributeFilter: ["data-preview"] });
  for(const event of ["kz:turn","kz:tool-start","kz:tool-end","kz:status","kz:done","kz:idle","kz:stopped"]) subscribeRuntimeEvent(event,sync);
  sync();
});
