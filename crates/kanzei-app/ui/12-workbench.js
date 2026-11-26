import { openMenu } from "./00-surface.js";
import { isGeneralChat, syncGeneralChatView } from "./03-general-scope.js";
import { $, defer } from "./00-dom.js";
import { invoke, listen } from "./00-ipc.js";
import { t } from "./02-i18n.js";
import { currentProject, processItems, sessionStates, navigate_view, syncBackBars, toastError } from "./03-shell.js";
import { active_space, switch_workspace, workspace_switch_pending } from "./03-workspaces.js";
import { enterProject, lastProjectPrefs, projectDisplayName, removeProject, renameProject, shortProjectPath } from "./09-sessions.js";
import { executionActivity, activityLabels, sameProject } from "./00-project-identity.js";
import { awaitingUserSessions } from "./08-auto.js";
import { openSessionHistory, orderedProjects, projectApprovalCount, renderSidebarSessions, setSidebarOpen, sidebarOpen, syncSessionActivity, wrapProjectRow } from "./12-session-tree.js";

// Browsing a project does not select an execution root or create a session.
export let browsingProject = null;
export let lastWorkspaceSnapshot = null;
let workspaceRefreshGeneration = 0;
let workspaceRefreshTimer = null;
let projectNavigationGeneration = 0;
let openingProject = null;
let scrolledProject = null;
let questionSessions = new Set();
let inboxCounts = new Map();
export function setWorkbenchInboxCounts(counts) { inboxCounts = new Map(counts); renderProjectActivity(); }
export function setWorkbenchQuestions(rows) { questionSessions = new Set(rows.map(q => q.sessionId)); renderProjectActivity(); }
export function workbenchQuestionWaiting(sessionId) { return questionSessions.has(sessionId); }
export function cancelProjectNavigation() { projectNavigationGeneration += 1; openingProject = null; renderProjectActivity(); }
export function workbenchNavigationGuard() {
  const generation = projectNavigationGeneration;
  return () => generation === projectNavigationGeneration;
}
export function setBrowsingProject(path) { browsingProject = path || null; }
export function workbenchProject() { return browsingProject || (isGeneralChat() ? null : currentProject || lastProjectPrefs.current); }
export function lineAuthorityLabel(item) { return item?.worktree_path ? t("独立任务") : t("主对话"); }

export function renderWorkspace(snapshot = lastWorkspaceSnapshot) {
  const root = $("workspace-projects");
  if (!root) return;
  root.replaceChildren();
  const projects = snapshot?.projects ?? [];
  if (!projects.length) {
    const empty = node("div", "workspace-empty");
    empty.append(
      node("p", "", t("还没有添加项目")),
      node("p", "dim", t("添加一个已有文件夹或新建项目以开始对话。")),
    );
    root.append(empty);
    return;
  }
  for (const project of projects) {
    const card = node("article", `workspace-card${sameProject(project.path, currentProject) ? " current" : ""}`);
    card.dataset.path = project.path;
    const head = node("div", "workspace-card-head");
    const titleBtn = action(project.name || projectDisplayName(project.path), () => void openProjectSpace(project.path, "chat"), "workspace-card-title");
    const menuBtn = action("⋯", (event) => {
      event.stopPropagation();
      openMenu(event.currentTarget, [
        { label: t("打开对话"), onSelect: () => void openProjectSpace(project.path, "chat") },
        { label: `${t("重命名项目")}…`, onSelect: () => void renameProject(project.path).then(() => refreshWorkspace()) },
        "separator",
        { label: `${t("移除项目")}…`, danger: true, onSelect: () => void removeProject(project.path).then(() => refreshWorkspace()) },
      ], { placement: "bottom-end", label: project.name || projectDisplayName(project.path) });
    }, "ghost mini");
    menuBtn.title = t("项目操作");
    head.append(titleBtn, menuBtn);

    const meta = node("div", "workspace-card-meta dim");
    const runningCount = Number(project.running_lines) || 0;
    const statusText = project.error
      ? t("项目目录暂时不可访问")
      : runningCount > 0 ? `${runningCount} ${t("条运行中")}` : t("空闲");
    meta.append(
      node("span", `workspace-card-status${runningCount > 0 ? " running" : ""}`, statusText),
      node("span", "workspace-card-path", shortProjectPath(project.path)),
    );
    card.append(head, meta);
    const recent = project.recent_progress?.summary || project.recent_progress?.text;
    if (recent) card.append(node("p", "workspace-card-recent dim", String(recent)));
    root.append(card);
  }
}

export async function refreshWorkspace() {
  const generation = ++workspaceRefreshGeneration;
  try {
    const snapshot = await invoke("workspace_overview");
    if (generation !== workspaceRefreshGeneration) return snapshot;
    lastWorkspaceSnapshot = snapshot;
    renderWorkspace(snapshot);
    renderProjectActivity();
    return snapshot;
  } catch (error) {
    if (generation === workspaceRefreshGeneration) toastError(`${t("工作区刷新失败")}: ${error}`);
    return null;
  }
}

export function refreshWorkspaceSoon(delay = 250) {
  if (workspaceRefreshTimer) clearTimeout(workspaceRefreshTimer);
  workspaceRefreshTimer = setTimeout(() => {
    workspaceRefreshTimer = null;
    void refreshWorkspace();
  }, delay);
}

function node(tag, className, text) {
  const el = document.createElement(tag);
  if (className) el.className = className;
  if (text != null) el.textContent = text;
  return el;
}
function action(label, callback, className = "ghost") {
  const el = node("button", className, label);
  el.type = "button";
  el.addEventListener("click", callback);
  return el;
}

export function reconcileWorkbenchView(view) {
  const global = ["workspace", "settings", "memory", "skills"].includes(view) || (isGeneralChat() && !browsingProject);
  document.body.dataset.appScope = global ? "global" : "project";
  document.body.dataset.projectPreview = String(view === "project");
  const path = workbenchProject();
  const name = $("project-space-name");
  if (name) name.textContent = path ? projectDisplayName(path) : t("未选择项目");
  for (const button of document.querySelectorAll(".workbench-project-link")) {
    const selected = !global && active_space === "dev-auto" && button.dataset.path === path;
    button.classList.toggle("active", selected);
    button.setAttribute("aria-current", selected ? "page" : "false");
    // UX-031:项目多时,选中(或新增后自动选中)的项目行滚进侧栏可视区;只在选中项变了时滚,不抢用户手动滚动。
    if (selected && button.dataset.path !== scrolledProject) { scrolledProject = button.dataset.path; button.scrollIntoView?.({ block: "nearest" }); }
  }
  $("workbench-home")?.classList.toggle("active", view === "workspace");
  if (view !== "project") setAttentionNavActive(false);
  syncBackBars();
  syncGeneralChatView(view, t);
}
/// 「待我处理」页(项目概览的收件箱页签)开着时,侧栏高亮「待我处理」这一项,项目行与会话行不再充当当前位置(D12)。
/// 25-softwire.js 每次重画时按当前页签调用;离开概览视图时 reconcileWorkbenchView 清掉。
export function setAttentionNavActive(on) {
  $("workbench-attention")?.classList.toggle("active", Boolean(on));
  if (on) document.body.dataset.attentionOpen = "true"; else delete document.body.dataset.attentionOpen;
}

export function renderWorkbenchNavigation(prefs = lastProjectPrefs) {
  const root = $("workbench-project-list");
  if (!root) return;
  // 每个项目是一组「项目行 + 它的会话列表」(12-session-tree.js);顺序 = 置顶在前,其余照 projects 数组序。
  const existing = new Map([...root.children].map((el) => [el.dataset.path, el]));
  const keep = new Set();
  let previous = null;
  for (const path of orderedProjects(prefs)) {
    let group = existing.get(path);
    if (!group) {
      const button = action("", () => void openProjectRow(path), "workbench-project-link");
      button.dataset.path = path;
      const label = node("span", "workbench-project-name"), activity = node("span", "workbench-project-activity");
      const attention = node("span", "workbench-project-attention"); attention.hidden = true;
      // 等你批准的琥珀小圆点(UX-146):别的项目 / 别的线有权限请求在排队时亮起,与运行态点互不顶替(运行中也要看得见)。
      const approval = node("span", "workbench-project-approval"); approval.hidden = true;
      activity.setAttribute("aria-hidden", "true"); attention.setAttribute("aria-hidden", "true"); approval.setAttribute("aria-hidden", "true");
      button.append(label, activity, attention, approval);
      group = wrapProjectRow(path, button);
    }
    // 只在位置不对时才挪 DOM:无谓的 insertBefore 会丢掉悬停与焦点。
    const expected = previous ? previous.nextSibling : root.firstChild;
    if (group !== expected) root.insertBefore(group, expected);
    previous = group;
    group._link.querySelector(".workbench-project-name").textContent = projectDisplayName(path, prefs);
    keep.add(path);
  }
  for (const [path, group] of existing) if (!keep.has(path)) group.remove();
  renderSidebarSessions();
  if (browsingProject && !(prefs.projects ?? []).includes(browsingProject)) browsingProject = null;
  reconcileWorkbenchView(document.body.dataset.view || "workspace");
  renderProjectActivity();
}

export function renderProjectActivity() {
  for (const button of document.querySelectorAll(".workbench-project-link")) {
    const path = button.dataset.path;
    const project = lastWorkspaceSnapshot?.projects?.find(p => sameProject(p.path, path));
    const local = sameProject(path, currentProject);
    const lines = local ? processItems : project?.lines || [];
    const unavailable = !local && (!project || Boolean(project.error) || ["stale", "unavailable"].includes(project.freshness));
    const phases = lines.filter(line => line.profile !== "research").map(line => executionActivity(line, sessionStates.get(line.session_id), {
      waiting: questionSessions.has(line.session_id) || awaitingUserSessions.has(line.session_id), unavailable,
    }).state);
    const state = ["running", "starting", "stopping", "attention", "failed", "waiting", "unknown", "stopped"].find(phase => phases.includes(phase))
      || (unavailable ? "unknown" : "idle");
    button.dataset.activity = state;
    const loading = sameProject(openingProject, path), count = [...inboxCounts].find(([project]) => sameProject(project, path))?.[1] || 0;
    button.dataset.loading = String(loading);
    button.setAttribute("aria-busy", String(loading));
    const indicator = button.querySelector(".workbench-project-activity");
    indicator.hidden = ["idle", "unknown"].includes(state);
    indicator.textContent = ({ waiting: "Ⅱ", attention: "?", stopping: "■", stopped: "■", failed: "!" })[state] || "";
    const badge = button.querySelector(".workbench-project-attention");
    badge.hidden = !count; badge.textContent = String(count);
    const approvals = projectApprovalCount(path);
    const dot = button.querySelector(".workbench-project-approval");
    if (dot) dot.hidden = !approvals;
    if (approvals) button.dataset.approval = "true"; else delete button.dataset.approval;
    const label = t(activityLabels[state]) + (count ? `  ${count} ${t("待我处理")}` : "")
      + (approvals ? `  ${approvals} ${t("条对话等你批准")}` : "") + (loading ? `  ${t("正在打开")}` : "");
    button.title = `${path}\n${label}`;
    button.setAttribute("aria-label", `${projectDisplayName(path)}  ${label}`);
  }
  syncSessionActivity();
}

/// 点侧栏的项目行(UX-022 / D16、D17)。点的就是当前项目时**保留当前会话**:正开着的讨论 / 独立任务不再被一律踢回
/// 主对话,也不整项目重载——已在它的对话页就原地不动;在它的需求 / 概览 / 文件页,或从所有项目、设置这类全局页
/// 过来,就带回它正在用的那条会话。进入别的项目时恢复它上次选中的对话。
export async function openProjectRow(path) {
  if (!path) return false;
  const open = !sidebarOpen(path);
  setSidebarOpen(path, open);
  renderSidebarSessions();
  if (!open) return true;
  const same = active_space === "dev-auto" && !workspace_switch_pending && !openingProject && currentProject === path;
  if (!same) return openProjectSpace(path, "chat");
  setBrowsingProject(path);
  if (document.body.dataset.view === "chat") reconcileWorkbenchView("chat");
  else navigate_view("chat");
  return true;
}

export async function openProjectSpace(path, view = "chat", options = {}) {
  if (!path) { navigate_view("workspace"); return false; }
  if (view === "chat" && active_space === "dev-auto" && !workspace_switch_pending && !openingProject
    && !options.reload && currentProject === path && document.body.dataset.view === "chat") {
    setBrowsingProject(path); reconcileWorkbenchView("chat"); return true;
  }
  const generation = ++projectNavigationGeneration;
  setBrowsingProject(path);
  openingProject = path; renderProjectActivity();
  try {
    if (active_space !== "dev-auto" || workspace_switch_pending) {
      if (!await switch_workspace("dev-auto", { isCurrent: () => generation === projectNavigationGeneration })) return false;
      if (generation !== projectNavigationGeneration) return false;
    }
    const prefs = await invoke("projects_select", { path });
    if (generation !== projectNavigationGeneration) return false;
    await enterProject(prefs, { view, activate: options.activate, isCurrent: () => generation === projectNavigationGeneration });
    return generation === projectNavigationGeneration;
  } catch (error) { toastError(`${t("切换项目失败")}: ${error}`); return false; }
  finally { if (generation === projectNavigationGeneration) { openingProject = null; renderProjectActivity(); } }
}

export async function openProjectResource(path, view) {
  if (view === "chat") {
    if (await openProjectSpace(path, "chat")) openSessionHistory($("workbench-chat-history"), path);
    return;
  }
  return openProjectSpace(path, view);
}

export async function openWorkbenchSpace(space) {
  const generation = ++projectNavigationGeneration;
  const isCurrent = () => generation === projectNavigationGeneration;
  if (!await switch_workspace(space, { isCurrent })) return false;
  if (!isCurrent()) return false;
  if (workbenchProject()) return openProjectSpace(workbenchProject());
  navigate_view("workspace");
  return true;
}

defer(() => {
  // 权限请求排队 / 被答复(07-events.js):侧栏项目行与会话行的「等你批准」圆点立刻跟上,不等 3 秒轮询。
  document.addEventListener("kz:asks-changed", () => renderProjectActivity());
  document.addEventListener("kz:workspace-navigation", event => {
    event.preventDefault(); void openWorkbenchSpace(event.detail.space);
  });
  $("workbench-home")?.addEventListener("click", () => { navigate_view("workspace"); });
  // 品牌栏的搜索图标打开全局对话历史;弹层本体在 12-session-tree.js,走 openPopover 的唯一写法(UX-003)。
  $("workbench-chat-history")?.addEventListener("click", (event) => openSessionHistory(event.currentTarget, null));
  // 「项目」标题右侧的「＋」:添加已有文件夹 / 新建项目(与所有项目页头两个按钮同一实现,点它们本人);零项目时这是侧栏唯一入口。
  $("workbench-add-project")?.addEventListener("click", (event) => {
    openMenu(event.currentTarget, [
      { label: t("添加项目文件夹…"), onSelect: () => $("project-add")?.click() },
      { label: t("新建项目…"), onSelect: () => $("project-init")?.click() },
    ], { placement: "bottom-end", label: t("添加项目") });
  });
  void listen("kz:workspace-invalidated", () => refreshWorkspaceSoon());
  setInterval(() => {
    if (!document.hidden && document.body.dataset.view === "workspace") void refreshWorkspace();
  }, 8000);
});
