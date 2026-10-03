// 侧栏会话树与「对话与历史」弹层(UX-003/004/021/024/029/031/033/036)。
//
// 数据来源:当前项目读 processItems(3 秒轮询已在更新);其它项目展开时按需 process_list(project_dir)
// 拉取并缓存(见 loadRemoteSessions)。名字/类型/最近活动来自后端 ProcessInfo
// (label = 展示名、kind、title_custom、updated_at),旧形状(label 仍是 pN/「默认」)按本地规则补。
// 偏好存 app.json 的 ui_layout(03-layout.js,不用 localStorage):
//   session_pins[进程 id] = true      会话置顶
//   session_order[项目路径] = [进程 id…]  手动排序(整组替换)
//   sidebar_open[项目路径] = bool      项目行展开态(没记过:当前项目展开,其它收起)
//   project_pins[项目路径] = true     项目置顶(顺序本身是 projects 数组顺序,见 projects_reorder)
//
// 3 秒轮询会反复重绘:列表里的行不绑私有监听(点击/右键/拖动/键盘全部由 12-session-menus.js 事件委托在
// 列表根上),重绘只在「行内容签名」变了才重建,运行态只改行上的 data-activity。
import { closeSurface, isSurfaceOpen, openPopover } from "./00-surface.js";
import { $, defer, invoke } from "./01-core.js";
import { localizedStage, t } from "./02-i18n.js";
import { layoutPref, onLayoutChange, setLayoutPref } from "./03-layout.js";
import {
  activeProcessId, activeSessionId, currentProject, ensureChatView, expandSidebar, processItems, sessionStates, toast,
} from "./03-shell.js";
import { active_space } from "./03-workspaces.js";
import { askActive, askQueues } from "./07-events.js";
import { awaitingUserSessions } from "./08-auto.js";
import { createWorktreeLine, lastProjectPrefs, processRunning, projectDisplayName, switchProcess } from "./09-sessions.js";
import { generalChatRoot, isGeneralChat, openGeneralChat } from "./03-general-scope.js";
import { conversationRecord, matchesConversation, uniqueConversations } from "./12-conversation-model.js";
import { lastWorkspaceSnapshot } from "./12-docs-pages.js";
import { openProjectSpace, workbenchProject, workbenchQuestionWaiting } from "./12-workbench.js";
import { conversationItemsByProcess, openClosedConversation, openConversationForProcess, startNewConversation } from "./15-views-misc.js";
import { executionActivity, sameProject } from "./25-softwire-model.js";

/// 侧栏每个项目下最多直接列几条(其余进「更多」);当前打开的对话即使排在后面也会补上。
export const SESSION_VISIBLE = 8;
/// 其它项目的 process_list 缓存多久算旧(轮询搭 3 秒节拍,过期才真发请求)。
const REMOTE_TTL_MS = 20000;

const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
};
/// 没有标题的对话段:后端落的兜底标题与界面兜底名是同一句「未命名对话」,英文界面按词表翻译(用户自己起的标题原样)。
export function segmentTitle(title) {
  const raw = String(title ?? "").trim();
  return raw && raw !== "未命名对话" ? raw : t("未命名对话");
}

// ---------- 偏好 ----------
export const sessionPinned = (id) => layoutPref("session_pins", id) === true;
export function setSessionPinned(id, on) { setLayoutPref("session_pins", id, on ? true : null); }
export const projectPinned = (path) => layoutPref("project_pins", path) === true;
export function setProjectPinned(path, on) { setLayoutPref("project_pins", path, on ? true : null); }
export function sessionOrderPref(project) {
  const value = layoutPref("session_order", project);
  return Array.isArray(value) ? value.filter((id) => typeof id === "string") : [];
}
export function setSessionOrder(project, ids) { setLayoutPref("session_order", project, ids.length ? ids : null); }
export function sidebarOpen(path) {
  const value = layoutPref("sidebar_open", path);
  return typeof value === "boolean" ? value : sameProject(path, workbenchProject());
}
export function setSidebarOpen(path, open) { setLayoutPref("sidebar_open", path, Boolean(open)); }
/// 删除/移除后顺手清掉该 id 在本地偏好里的残留(后端 process_purge 同时清 app.json 里的键)。
export function forgetSessionPrefs(project, id) {
  setSessionPinned(id, false);
  const order = sessionOrderPref(project);
  if (order.includes(id)) setSessionOrder(project, order.filter((other) => other !== id));
}

// ---------- 行模型 ----------
export function kindWord(kind) {
  return kind === "conversation" ? t("对话") : kind === "discussion" ? t("讨论") : t("独立任务");
}
const LEGACY_LABEL = /^(默认|p\d*|d|主对话|讨论|独立任务|对话|新对话|未命名对话)(?:\s+\d+)?$/;
export function sessionKindOf(item) {
  if (item.profile === "research") return "research";
  return item.profile === "readonly" ? "discussion" : "conversation";
}
/// 对话统一使用持久化标题;旧类型名和编号不再充当标题。
export function sessionDisplayName(item) {
  const title = String(item.title ?? "").trim();
  if (title) return title;
  const label = String(item.label ?? "").trim();
  if (label && (item.title_custom || !LEGACY_LABEL.test(label))) return label;
  return t("新对话");
}
export function sessionTime(item) {
  const value = item.updated_at ?? item.updatedAt;
  return Number.isFinite(Number(value)) && Number(value) > 0 ? Number(value) : 0;
}
/// 相对时间(只进 tooltip 与弹层副行):刚刚/N 分钟前/N 小时前/N 天前,再早写日期。
export function relativeTime(ms, now = Date.now()) {
  if (!ms) return "";
  const diff = Math.max(0, now - ms);
  const minute = 60000;
  if (diff < minute) return t("刚刚");
  if (diff < 60 * minute) return `${Math.floor(diff / minute)} ${t("分钟前")}`;
  if (diff < 24 * 60 * minute) return `${Math.floor(diff / (60 * minute))} ${t("小时前")}`;
  if (diff < 14 * 24 * 60 * minute) return `${Math.floor(diff / (24 * 60 * minute))} ${t("天前")}`;
  return new Date(ms).toLocaleDateString();
}

/// 工作空间快照里的线(其它项目尚未拉 process_list 时的兜底,只有线级现场)。
function snapshotLines(project) {
  const entry = lastWorkspaceSnapshot?.projects?.find((p) => sameProject(p.path, project));
  return (entry?.lines ?? []).map((line) => ({ ...line, origin_project: project, project_dir: project }));
}
const remote = new Map(); // 项目路径 → { items, at, loading, error }
export function remoteSessionState(project) { return remote.get(project) ?? null; }
export function sessionItemsOf(project) {
  const items = sameProject(project, currentProject) ? processItems
    : remote.get(project)?.items ?? snapshotLines(project);
  return items.filter((item) => item.profile !== "research" && item.kind !== "research"
    && !item.deleted && !item.archived && !item.closed
    && !["closed", "archived", "deleted"].includes(item.lifecycle || item.status));
}
/// 一条会话的展示名(旧代码里直接读 item.label 的地方改用它:label 在旧后端里是 pN/「默认」)。
export function processName(item) {
  return sessionDisplayName(item);
}
/// 工作空间快照里一条线(只有线级字段,没有主对话判定)的展示名。
export function lineName(line) {
  return sessionDisplayName(line, sessionKindOf(line, null));
}
export function buildRows(project, { includeClosed = false } = {}) {
  const items = sessionItemsOf(project);
  const rows = items.map((item) => {
    const kind = sessionKindOf(item);
    return conversationRecord(project, item, {
      kind, general: isGeneralChat(project), name: sessionDisplayName(item, kind), pinned: sessionPinned(item.id),
    });
  });
  if (includeClosed) rows.push(...closedSessionsOf(project).map((item) => closedRow(project, item)));
  return uniqueConversations(rows);
}

function closedRow(project, item) {
  return conversationRecord(project, item, { closed: true, general: isGeneralChat(project), name: closedName(item) });
}
/// 显示顺序:置顶组、其余组。组内:手动排过的按手动序,没排过的(含新建的)排在最前,
/// 再按编号新→旧(稳定:轮询刷新的 updated_at 不会让行跳来跳去)。
export function orderRows(project, rows = buildRows(project)) {
  const manual = new Map(sessionOrderPref(project).map((id, index) => [id, index]));
  const compare = (a, b) => {
    const ia = manual.has(a.id) ? manual.get(a.id) : -1;
    const ib = manual.has(b.id) ? manual.get(b.id) : -1;
    return ia - ib || b.ordinal - a.ordinal || b.updatedAt - a.updatedAt || String(a.id).localeCompare(String(b.id));
  };
  return [...rows.filter((row) => row.pinned).sort(compare), ...rows.filter((row) => !row.pinned).sort(compare)];
}
/// 项目显示顺序:置顶的在前,其余照 projects 数组顺序(projects_reorder 写的就是这个数组)。
export function orderedProjects(prefs) {
  const paths = prefs?.projects ?? [];
  return [...paths.filter((path) => projectPinned(path)), ...paths.filter((path) => !projectPinned(path))];
}
export function projectRunningCount(project) {
  return sessionItemsOf(project).filter((item) => (sameProject(project, currentProject) ? processRunning(item) : item.running)).length;
}
/// 这条会话有没有权限请求在队里等你批准(UX-146)。正在对话的那条自己有答复卡,不算——要提醒的是「别处在等你」。
export function sessionAwaitsApproval(sessionId) {
  if (!sessionId || sessionId === activeSessionId) return false;
  return (askQueues.get(sessionId)?.length ?? 0) > 0 || askActive?.sessionId === sessionId;
}
/// 一个项目里有几条对话在等批准(项目行的琥珀小圆点用它;其它项目读缓存的 process_list 或工作空间快照里的线)。
export function projectApprovalCount(project) {
  return sessionItemsOf(project).filter((item) => sessionAwaitsApproval(item.session_id)).length;
}

// ---------- 行的运行态 ----------
export function rowActivity(row) {
  if (!row.execution) return { state: "idle", label: "" };
  const live = sessionStates.get(row.session_id);
  const waiting = awaitingUserSessions.has(row.session_id) || workbenchQuestionWaiting(row.session_id);
  return executionActivity({ running: row.item.running, stage: row.item.stage }, live, { waiting });
}
/// 这条对话此刻有「要让人看见」的状态:运行中 / 启动 / 停止中 / 等你回复,或有权限请求在等你批准。
function rowNeedsEye(row) {
  return ["running", "starting", "stopping", "attention"].includes(rowActivity(row).state) || sessionAwaitsApproval(row.session_id);
}
function rowTitle(row, activity, approval = false) {
  const lines = [row.name];
  if (row.lifecycle === "closed") lines.push(t("已关闭的对话,只读查看"));
  if (row.branch) lines.push(row.branch);
  const when = relativeTime(row.updatedAt);
  if (when) lines.push(when);
  if (activity.state !== "idle") lines.push(localizedStage(activity.label));
  if (approval) lines.push(t("有权限请求在等你批准"));
  return lines.join("\n");
}

/// 签名没变时复用旧行:只把 wrap._row 换成最新数据,再同步运行态点。
function refreshRowData(container, rows) {
  const fresh = new Map(rows.map((row) => [row.id, row]));
  for (const wrap of container.children) {
    if (!wrap._row) continue;
    wrap._row = fresh.get(wrap._row.id) ?? wrap._row;
    syncRowActivity(wrap);
  }
}
/// 一条会话行(侧栏树与历史弹层共用;纯数据,不绑监听)。
export function buildSessionRow(row, { history = false } = {}) {
  const closed = row.lifecycle === "closed";
  const wrap = el("div", `workbench-session${history ? " in-history" : ""}${closed ? " is-closed" : ""}`);
  wrap.dataset.ctx = closed ? "closed" : "session";
  // 所有活动对话共用置顶和排序规则。
  if (row.capabilities.reorder) wrap.dataset.drag = "session";
  wrap.dataset.project = row.project;
  wrap.dataset.processId = row.id;
  wrap.dataset.kind = row.kind;
  wrap.dataset.pinned = String(row.pinned);
  wrap.dataset.key = row.identity;
  wrap.dataset.conversationId = row.identity;
  wrap.dataset.title = row.name;
  wrap._row = row;
  if (closed) wrap._closed = row.item;
  const button = el("button", "workbench-session-link");
  button.type = "button";
  const name = el("span", "workbench-session-name", row.name);
  button.append(name);
  if (row.pinned) {
    const pin = el("span", "workbench-session-pin");
    pin.setAttribute("aria-hidden", "true");
    button.append(pin);
  }
  // 等你批准的小圆点(琥珀=需要注意):别的对话在等权限批准时这里亮一枚,不必逐个点开去看(UX-146)。
  const approval = el("span", "workbench-session-approval");
  approval.hidden = true;
  approval.setAttribute("aria-hidden", "true");
  button.append(approval);
  if (history) {
    const when = relativeTime(row.updatedAt);
    if (when) {
      const time = el("span", "workbench-session-time", when);
      time.setAttribute("aria-hidden", "true");
      button.append(time);
      wrap._time = time;
    }
  }
  button.setAttribute("aria-haspopup", "menu");
  wrap.append(button);
  syncRowActivity(wrap);
  return wrap;
}
export function syncRowActivity(wrap) {
  const row = wrap._row;
  const button = wrap.firstChild;
  if (!row || !button) return;
  const activity = rowActivity(row);
  if (button.dataset.activity !== activity.state) button.dataset.activity = activity.state;
  const active = row.capabilities.send && sameProject(row.project, currentProject) && row.id === activeProcessId;
  const current = active ? "true" : "false";
  if (button.getAttribute("aria-current") !== current) button.setAttribute("aria-current", current);
  const approval = row.capabilities.send && sessionAwaitsApproval(row.session_id);
  const dot = button.querySelector(".workbench-session-approval");
  if (dot && dot.hidden === approval) dot.hidden = !approval;
  if (approval) { if (button.dataset.approval !== "true") button.dataset.approval = "true"; } else if (button.dataset.approval) delete button.dataset.approval;
  const title = rowTitle(row, activity, approval);
  if (button.title !== title) button.title = title;
  // 历史弹层行的相对时间原地更新(updated_at 随运行不断刷新,不能为它重建整个列表)。
  if (wrap._time) {
    const when = relativeTime(row.updatedAt);
    if (when && wrap._time.textContent !== when) wrap._time.textContent = when;
  }
  const description = `${t("对话")}${activity.state !== "idle" ? ` · ${localizedStage(activity.label)}` : ""}${approval ? ` · ${t("等你批准")}` : ""}`;
  if (button.getAttribute("aria-description") !== description) button.setAttribute("aria-description", description);
}

// ---------- 侧栏树 ----------
let dragging = false;
let renderPending = false;
export function setSessionDragging(value) {
  dragging = Boolean(value);
  if (!dragging && renderPending) { renderPending = false; renderSidebarSessions(); }
}
const signatures = new Map(); // 项目路径 → 上次画的签名
let historySignature = "";

/// 项目名称本身负责打开项目与展开会话列表。
export function wrapProjectRow(path, linkButton) {
  const group = el("div", "workbench-project");
  group.dataset.path = path;
  group.dataset.ctx = "project-group";
  const row = el("div", "workbench-project-row");
  row.dataset.ctx = "project";
  row.dataset.drag = "project";
  row.dataset.path = path;
  linkButton.setAttribute("aria-haspopup", "menu");
  row.append(linkButton);
  const list = el("div", "workbench-session-list");
  list.setAttribute("role", "group");
  list.hidden = true;
  group.append(row, list);
  group._row = row;
  group._list = list;
  group._link = linkButton;
  return group;
}

function moreButton(path, hidden) {
  const button = el("button", "workbench-session-more", `${t("更多")} (${hidden})…`);
  button.type = "button";
  button.dataset.act = "more";
  button.dataset.project = path;
  button.title = t("对话与历史");
  return button;
}
function noteRow(text) {
  const note = el("div", "workbench-session-note", text);
  note.setAttribute("role", "status");
  return note;
}

function restoreFocus(container, key) {
  if (!key) return;
  for (const wrap of container.children) {
    if (wrap.dataset?.key === key) { wrap.firstChild?.focus?.({ preventScroll: true }); return; }
    if (wrap.dataset?.act === key) { wrap.focus?.({ preventScroll: true }); return; }
  }
}
function focusKeyIn(container) {
  const active = document.activeElement;
  if (!active || !container.contains?.(active)) return null;
  const holder = active.closest?.("[data-key]");
  return holder?.dataset.key ?? (active.dataset?.act === "more" ? "more" : null);
}

export function renderSidebarSessions() {
  const root = $("workbench-project-list");
  if (!root) return;
  if (dragging) { renderPending = true; return; }
  for (const group of root.children) {
    const path = group.dataset?.path;
    const list = group._list;
    if (!path || !list) continue;
    const open = sidebarOpen(path);
    const name = projectDisplayName(path);
    group.dataset.open = String(open);
    group.dataset.pinned = String(projectPinned(path));
    group._row.dataset.pinned = String(projectPinned(path));
    group._link.setAttribute("aria-expanded", String(open));
    list.hidden = !open;
    list.setAttribute("aria-label", `${name} · ${t("对话")}`);
    if (!open) {
      if (list.firstChild) list.replaceChildren();
      signatures.delete(path);
      continue;
    }
    const local = sameProject(path, currentProject);
    if (!local) void loadRemoteSessions(path);
    const rows = orderRows(path);
    const active = local ? activeProcessId : null;
    // 前 N 条之外,当前打开的、正在跑的、等你回复 / 等你批准的对话也必须在树里看得见:关键状态不能被折进「更多」。
    const visible = rows.filter((row, index) => index < SESSION_VISIBLE || row.id === active || rowNeedsEye(row));
    const hidden = rows.length - visible.length;
    const state = remote.get(path);
    const status = !rows.length ? (local ? "" : state?.error ? "error" : state?.items ? "empty" : "loading") : "";
    const signature = JSON.stringify([
      visible.map((row) => [row.id, row.name, row.kind, row.pinned, row.custom]), hidden, status, t("独立任务"),
    ]);
    if (signatures.get(path) === signature) {
      // 行没变:不重建,但把行上挂的数据换成这一轮的新值(运行态/分支等来自最新的进程列表),再对一遍运行态点。
      refreshRowData(list, visible);
      continue;
    }
    signatures.set(path, signature);
    const focusKey = focusKeyIn(list);
    const children = visible.map((row) => buildSessionRow(row));
    if (hidden > 0) children.push(moreButton(path, hidden));
    if (status === "loading") children.push(noteRow(t("加载中…")));
    else if (status === "error") children.push(noteRow(t("对话列表读取失败")));
    else if (status === "empty") children.push(noteRow(t("暂无对话")));
    list.replaceChildren(...children);
    restoreFocus(list, focusKey);
  }
  renderGeneralSessions();
  syncHistoryIfOpen();
}

function renderGeneralSessions() {
  const list = $("workbench-general-list");
  const project = generalChatRoot();
  if (!list) return;
  if (!project) { list.replaceChildren(); return; }
  void loadRemoteSessions(project);
  const rows = orderRows(project);
  const visible = rows.filter((row, index) => index < SESSION_VISIBLE || row.id === activeProcessId || rowNeedsEye(row));
  const state = remote.get(project);
  const signature = JSON.stringify([visible.map(row => [row.id, row.name, row.pinned]), rows.length, state?.error, t("对话")]);
  const key = `general:${project}`;
  if (signatures.get(key) === signature) { refreshRowData(list, visible); return; }
  signatures.set(key, signature);
  const focus = focusKeyIn(list);
  const children = visible.map(row => buildSessionRow(row));
  if (rows.length > visible.length) children.push(moreButton(project, rows.length - visible.length));
  if (!rows.length && state?.error) children.push(noteRow(t("对话列表读取失败")));
  list.replaceChildren(...children);
  restoreFocus(list, focus);
}
/// 列表数据/偏好变了(置顶、排序、重命名、删除):丢掉签名,两处视图一起重画。
export function invalidateSessionTree() {
  signatures.clear();
  historySignature = "";
  renderSidebarSessions();
}
function syncHistoryActivity() {
  const body = historyBody();
  if (body && isSurfaceOpen(historyElement())) for (const wrap of body.children) if (wrap._row) syncRowActivity(wrap);
}
/// 运行态变化(kz:session-state-changed 等高频路径)只改行上的 data-activity,不重建。
export function syncSessionActivity() {
  const root = $("workbench-project-list");
  if (!root) return;
  for (const group of root.children) {
    if (!group._list || group._list.hidden) continue;
    for (const wrap of group._list.children) if (wrap._row) syncRowActivity(wrap);
  }
  for (const wrap of $("workbench-general-list")?.children ?? []) if (wrap._row) syncRowActivity(wrap);
  syncHistoryActivity();
}

/// 其它项目的会话:展开时拉一次、之后随轮询按 TTL 刷新;失败只在行下写一句,不弹错误。
export function loadRemoteSessions(project, { force = false } = {}) {
  if (!project || sameProject(project, currentProject)) return Promise.resolve();
  const entry = remote.get(project) ?? {};
  if (entry.loading) return entry.loading;
  if (!force && entry.at && Date.now() - entry.at < REMOTE_TTL_MS) return Promise.resolve();
  const loading = (async () => {
    try {
      const items = await invoke("process_list", { projectDir: project });
      if (remote.get(project)?.loading !== loading) return;
      remote.set(project, { items: Array.isArray(items) ? items : [], at: Date.now(), loading: null, error: "" });
    } catch (error) {
      if (remote.get(project)?.loading !== loading) return;
      remote.set(project, { ...(remote.get(project) ?? {}), at: Date.now(), loading: null, error: String(error) });
    }
    signatures.delete(project);
    renderSidebarSessions();
  })();
  remote.set(project, { ...entry, loading });
  return loading;
}
export function dropRemoteSessions(project) {
  remote.delete(project);
  signatures.delete(project);
}

// ---------- 已关闭的对话(UX-035) ----------
// 关闭独立任务只注销身份、对话记录一条不删(process_close),可在搜索与历史中查看；主侧栏不再显示它们。
// 只读查看(用既有的文字稿查看器,不进当前对话 pane)。清单来自只读命令 process_closed_list;展开态存 ui_layout.closed_open。
const closedCache = new Map(); // 项目路径 → { items, at, loading }
const CLOSED_TTL_MS = 60000;
export function closedSessionsOf(project) {
  const active = sessionItemsOf(project);
  return (closedCache.get(project)?.items ?? []).filter(item => !active.some(live => live.id === item.id || (item.session_id && live.session_id === item.session_id)));
}
export const closedOpen = (project) => layoutPref("closed_open", project) === true;
export function setClosedOpen(project, open) { setLayoutPref("closed_open", project, open ? true : null); }
/// 一条已关闭对话的展示名:用户命名 ‖ 首条消息前 48 字(后端 title)‖「已关闭的对话 N」。
export function closedName(entry) {
  const title = String(entry.title ?? "").trim();
  if (title) return title;
  const ordinal = Number(entry.ordinal) || 0;
  return `${t("对话")}${ordinal ? ` ${ordinal}` : ""}`;
}
export function loadClosedSessions(project, { force = false } = {}) {
  if (!project) return Promise.resolve();
  const entry = closedCache.get(project) ?? {};
  if (entry.loading) return entry.loading;
  if (!force && entry.at && Date.now() - entry.at < CLOSED_TTL_MS) return Promise.resolve();
  const loading = (async () => {
    let items = entry.items ?? [];
    let error = "";
    try {
      const list = await invoke("process_closed_list", { projectDir: project });
      items = Array.isArray(list) ? list : [];
    } catch (cause) { error = String(cause); }
    if (closedCache.get(project)?.loading !== loading) return;
    const before = JSON.stringify((entry.items ?? []).map((item) => [item.id, item.title]));
    closedCache.set(project, { items, at: Date.now(), loading: null, error });
    if (!entry.items || error !== (entry.error || "") || before !== JSON.stringify(items.map((item) => [item.id, item.title]))) {
      signatures.delete(project);
      historySignature = "";
      renderSidebarSessions();
    }
  })();
  closedCache.set(project, { ...entry, loading });
  return loading;
}
/// 关闭 / 删除线路之后:清单作废并重读(下一次重绘就带上刚关掉的那条)。
export function dropClosedSessions(project) {
  closedCache.delete(project);
  signatures.delete(project);
  historySignature = "";
  return loadClosedSessions(project, { force: true });
}
/// 分组头 + (展开时)已关闭的行。侧栏项目下与历史弹层里共用;没有已关闭对话就什么都不画。
function closedGroup(project, { history = false } = {}) {
  const items = closedSessionsOf(project);
  if (!items.length) return closedCache.get(project)?.error ? [noteRow(t("历史对话读取失败"))] : [];
  const open = closedOpen(project);
  const toggle = el("button", `workbench-closed-toggle${history ? " in-history" : ""}`);
  toggle.type = "button";
  toggle.dataset.act = "closed-toggle";
  toggle.dataset.project = project;
  toggle.setAttribute("aria-expanded", String(open));
  const caret = el("span", "workbench-closed-caret");
  caret.setAttribute("aria-hidden", "true");
  toggle.append(caret, document.createTextNode(`${t("已关闭")} (${items.length})`));
  toggle.title = t("历史对话可查看、重命名或删除");
  if (!open) return [toggle];
  const visible = history ? items : items.slice(0, 4);
  return [toggle, ...visible.map((entry) => buildSessionRow(closedRow(project, entry), { history })),
    ...(visible.length < items.length ? [moreButton(project, items.length - visible.length)] : [])];
}
const closedSignature = (project) => [closedOpen(project), closedCache.get(project)?.error, closedSessionsOf(project).map((item) => [item.id, item.title, item.ordinal])];

// ---------- 动作:打开会话 / 新建 ----------
/// 打开某项目的某段对话:别的项目先进入它,再切到目标会话;不在对话页就回到对话页。
export async function openSession(project, processId) {
  if (!project || !processId) return false;
  if (!sameProject(project, currentProject) || active_space !== "dev") {
    if (!await (isGeneralChat(project) ? openGeneralChat() : openProjectSpace(project, "chat"))) return false;
  }
  if (processId !== activeProcessId) await switchProcess(processId);
  ensureChatView(); // 切会话不能只在后台生效:不在对话页就跳回去(UX-052 / D15;已在对话页是空操作)。
  return true;
}
/// 在某项目里新建讨论 / 独立任务:别的项目先进入它;不在对话页先回到对话页(新对话要看得见)。
async function enterChat(project) {
  if (!sameProject(project, currentProject) || active_space !== "dev") {
    if (!await (isGeneralChat(project) ? openGeneralChat() : openProjectSpace(project, "chat"))) return false;
  }
  ensureChatView();
  return true;
}
export async function newDiscussionIn(project) {
  if (await enterChat(project)) await startNewConversation();
}
export async function newTaskIn(project) {
  if (isGeneralChat(project)) return newDiscussionIn(project);
  if (await enterChat(project)) await createWorktreeLine();
}

// ---------- 「对话与历史」弹层 ----------
let historyNode = null;
let historyProject = null;
export function historyElement() {
  if (historyNode) return historyNode;
  // 与其它锚定浮层同一写法:.k-surface .k-popover + openPopover(UX-003:原先缺这两个类,
  // 锚点规则不生效,弹层钉在窗口左上角 360×69)。
  historyNode = el("div", "k-surface k-popover project-session-menu hidden");
  historyNode.id = "session-history";
  historyNode.setAttribute("role", "dialog");
  historyNode.setAttribute("aria-label", t("对话与历史"));
  historyNode.dataset.i18nAriaLabel = "对话与历史";
  const head = el("div", "session-history-head");
  const title = el("strong", "session-history-title", t("对话与历史"));
  title.id = "session-history-title";
  title.dataset.i18nKey = "对话与历史";
  const project = el("span", "session-history-project dim");
  project.id = "session-history-project";
  const spacer = el("span", "session-history-spacer");
  const discussion = el("button", "ghost mini");
  discussion.type = "button";
  discussion.dataset.act = "new-discussion";
  discussion.title = t("新对话");
  discussion.dataset.i18nTitle = "新对话";
  const discussionLabel = el("span", "", t("新对话"));
  discussionLabel.dataset.i18nKey = "新讨论";
  discussion.append("＋ ", discussionLabel);
  const task = el("button", "ghost mini");
  task.type = "button";
  task.dataset.act = "new-task";
  task.title = t("独立任务有自己的工作树,可以并行写代码");
  task.dataset.i18nTitle = "独立任务有自己的工作树,可以并行写代码";
  const taskLabel = el("span", "", t("新建独立任务"));
  taskLabel.dataset.i18nKey = "新建独立任务";
  task.append("＋ ", taskLabel);
  head.append(title, project, spacer, discussion, task);
  const body = el("div", "session-history-body");
  body.id = "session-history-body";
  body.setAttribute("role", "list");
  const search = el("input", "session-history-search");
  search.type = "search";
  search.placeholder = t("搜索名称或项目");
  search.setAttribute("aria-label", t("搜索对话"));
  search.autocomplete = "off";
  search.addEventListener("input", () => { historySignature = ""; paintHistory(); body.scrollTop = 0; });
  historyNode.append(head, search, body);
  historyNode._head = head;
  historyNode._body = body;
  historyNode._project = project;
  historyNode._search = search;
  document.body.append(historyNode);
  return historyNode;
}
export function historyBody() { return historyNode?._body ?? null; }
export function historyProjectPath() { return historyProject; }

function segmentRows(project) {
  // 只有当前项目读得到各线的段列表(refreshConversationLists 填的);别的项目只列会话本身。
  if (!sameProject(project, currentProject)) return [];
  const rows = [];
  for (const [processId, items] of conversationItemsByProcess) {
    const owner = buildRows(project).find((row) => row.id === processId);
    if (!owner || !Array.isArray(items) || items.length < 2) continue;
    const latest = Math.max(...items.map((item) => Number(item.sequence) || 0));
    for (const item of items) {
      if ((Number(item.sequence) || 0) === latest) continue;
      rows.push({ owner, item });
    }
  }
  const stamp = (entry) => Number(entry.item.created_at) || Number(entry.item.sequence) || 0;
  return rows.sort((a, b) => stamp(b) - stamp(a));
}
function buildSegmentRow({ owner, item }) {
  const wrap = el("div", "workbench-session in-history is-segment");
  wrap.dataset.ctx = "segment";
  wrap.dataset.project = owner.project;
  wrap.dataset.processId = owner.id;
  wrap.dataset.sequence = String(item.sequence);
  wrap.dataset.seqs = JSON.stringify(item.sequences ?? [item.sequence]);
  wrap.dataset.title = segmentTitle(item.title);
  wrap.dataset.key = `${owner.project}\u001f${owner.id}\u001f${item.sequence}`;
  const button = el("button", "workbench-session-link");
  button.type = "button";
  const count = Number.isFinite(item.message_count) ? ` (${item.message_count} ${t("条")})` : "";
  const name = el("span", "workbench-session-name", `${segmentTitle(item.title)}${count}`);
  const tag = el("span", "workbench-session-tag", owner.name);
  tag.setAttribute("aria-hidden", "true");
  button.append(name, tag);
  const created = Number(item.created_at);
  const when = created > 0 ? relativeTime(created) : String(item.updated_at ?? "");
  button.title = [segmentTitle(item.title), `${t("历史")} · ${owner.name}`, when].filter(Boolean).join("\n");
  button.setAttribute("aria-haspopup", "menu");
  wrap.append(button);
  return wrap;
}
function paintHistory() {
  const node = historyElement();
  const project = historyProject;
  const projects = project ? [project] : [...new Set([generalChatRoot(), ...(lastProjectPrefs.projects ?? []), currentProject].filter(Boolean))];
  const query = node._search.value;
  const general = isGeneralChat(project);
  node._head.querySelector(".session-history-title").textContent = t(project ? "对话与历史" : "搜索对话");
  node._head.querySelector("[data-act='new-discussion']").hidden = !project;
  const newDiscussion = node.querySelector("[data-act='new-discussion'] span");
  if (newDiscussion) { newDiscussion.textContent = t(general ? "新对话" : "新讨论"); newDiscussion.dataset.i18nKey = general ? "新对话" : "新讨论"; }
  const newTask = node.querySelector("[data-act='new-task']");
  if (newTask) newTask.hidden = !project || general;
  const groups = projects.map(path => {
    void loadRemoteSessions(path);
    void loadClosedSessions(path);
    const name = isGeneralChat(path) ? t("无项目对话") : projectDisplayName(path);
    const includeClosed = !project || Boolean(query.trim());
    const rows = orderRows(path, buildRows(path, { includeClosed })).filter(row => matchesConversation(row, query, name));
    const segments = segmentRows(path).filter(entry => matchesConversation({ name: segmentTitle(entry.item.title) }, query, name));
    return { path, name, rows, segments };
  });
  const signature = JSON.stringify([
    project, query, groups.map(group => [group.path, group.name,
      group.rows.map(row => [row.identity, row.name, row.lifecycle, row.pinned]),
      group.segments.map(({ owner, item }) => [owner.id, item.sequence, item.title, item.message_count]),
      closedSignature(group.path), remote.get(group.path)?.error]), t("独立任务"),
  ]);
  node._project.textContent = project ? (general ? t("无项目对话") : projectDisplayName(project)) : t("所有对话");
  if (signature === historySignature) { refreshRowData(node._body, groups.flatMap(group => group.rows)); return; }
  historySignature = signature;
  const body = node._body;
  const focusKey = focusKeyIn(body);
  const children = [];
  for (const group of groups) {
    if (!project && (group.rows.length || group.segments.length)) children.push(el("div", "session-history-heading", group.name));
    children.push(...group.rows.map(row => buildSessionRow(row, { history: true })));
    if (group.segments.length) {
      children.push(el("div", "session-history-heading", t("更早的历史对话")), ...group.segments.map(buildSegmentRow));
    }
    if (project && !query.trim()) children.push(...closedGroup(group.path, { history: true }));
    if (remote.get(group.path)?.error || closedCache.get(group.path)?.error) children.push(noteRow(`${group.name} · ${t("对话列表读取失败")}`));
  }
  if (!children.length) {
    const loading = projects.some(path => remote.get(path)?.loading || closedCache.get(path)?.loading);
    children.push(noteRow(t(loading ? "加载中…" : query.trim() ? "未找到对话" : "暂无对话")));
  }
  const scroll = body.scrollTop;
  body.replaceChildren(...children);
  body.scrollTop = scroll;
  restoreFocus(body, focusKey);
}
export function syncHistoryIfOpen() {
  if (historyNode && isSurfaceOpen(historyNode)) paintHistory();
}
export function closeSessionHistory() {
  if (historyNode) closeSurface(historyNode);
}
/// 打开/收起「对话与历史」(侧栏时钟图标、项目行的「更多」、命令面板共用这一个入口)。
export function openSessionHistory(anchor, project = workbenchProject() || currentProject) {
  const node = historyElement();
  if (isSurfaceOpen(node)) {
    const same = historyProject === project;
    closeSurface(node);
    if (same) return null;
  }
  historyProject = project;
  historySignature = "";
  node._search.value = "";
  paintHistory();
  const handle = openPopover(anchor ?? $("workbench-chat-history"), node, {
    placement: "right-start",
    onClose: () => { historySignature = ""; },
  });
  node._search.focus();
  return handle;
}
/// 命令面板用:从任何页面打开「对话与历史」(先回对话页、展开侧栏,锚点才看得见)。
export function openSessionHistoryAnywhere() {
  ensureChatView();
  expandSidebar();
  requestAnimationFrame(() => openSessionHistory($("workbench-chat-history"), workbenchProject() || currentProject));
}
/// 命令面板的「对话」候选:当前项目的全部会话(名字/类型/运行态与侧栏树同源)。
export function sessionPaletteEntries(project = currentProject) {
  if (!project) return [];
  return orderRows(project).map((row) => {
    const activity = rowActivity(row);
    return {
      label: row.name,
      detail: `${kindWord(row.kind)}${activity.state !== "idle" ? ` · ${localizedStage(activity.label)}` : ""}`,
      run: () => { closeSessionHistory(); void openSession(project, row.id); },
    };
  });
}
/// 点历史弹层/侧栏树里的一行:会话 → 打开;历史段 → 只读查看。
export async function activateRow(wrap) {
  const project = wrap.dataset.project;
  const processId = wrap.dataset.processId;
  if (wrap.dataset.ctx === "closed") {
    await openClosedConversation(project, wrap._closed ?? { id: processId }, wrap.dataset.title);
    return;
  }
  if (wrap.dataset.ctx === "segment") {
    await openConversationForProcess(processId, Number(wrap.dataset.sequence), { project, title: wrap.dataset.title });
    return;
  }
  closeSessionHistory();
  if (!await openSession(project, processId)) toast(t("没有打开这段对话"));
}

defer(() => {
  document.addEventListener("kz:general-history-ready", () => invalidateSessionTree());
  onLayoutChange(() => { signatures.clear(); renderSidebarSessions(); });
  document.addEventListener("kz:view-changed", () => closeSessionHistory());
  // 切语言:行里的「对话」「已关闭」等是渲染点写的 t(),按内容签名去重的重绘不会自己发现语言变了。
  document.addEventListener("kz:language", () => { signatures.clear(); renderSidebarSessions(); });
});
