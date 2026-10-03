import { openMenu } from "./00-surface.js";
import { isGeneralChat, syncGeneralChatView } from "./03-general-scope.js";
import { $, defer, invoke } from "./01-core.js";
import { localizedDocStatus, localizedStage, t } from "./02-i18n.js";
import { currentProject, activeSessionId, processItems, sessionStates, navigate_view, syncBackBars, toast, toastError } from "./03-shell.js";
import { active_space, switch_workspace, workspace_switch_pending } from "./03-workspaces.js";
import { enterProject, lastProjectPrefs, projectDisplayName } from "./09-sessions.js";
import { jumpToEntry } from "./11-docs-list.js";
import { lastWorkspaceSnapshot, refreshWorkspace } from "./12-docs-pages.js";
import { setWorkspaceConsoleState } from "./12-decision-console.js";
import { openConventions } from "./15-conventions.js";
import { executionActivity, activityLabels, sameProject } from "./25-softwire-model.js";
import { awaitingUserSessions } from "./08-auto.js";
import { lineName, openSessionHistory, orderedProjects, projectApprovalCount, renderSidebarSessions, setSidebarOpen, sidebarOpen, syncSessionActivity, wrapProjectRow } from "./12-session-tree.js";

// Browsing a project does not select an execution root or create a session.
export let browsingProject = null;
let projectNavigationGeneration = 0;
let goalFormGeneration = 0;
let openingProject = null;
let scrolledProject = null;
let questionSessions = new Set();
export function setWorkbenchQuestions(rows) { questionSessions = new Set(rows.map(q => q.sessionId)); renderProjectActivity(); }
export function workbenchQuestionWaiting(sessionId) { return questionSessions.has(sessionId); }
export function cancelProjectNavigation() { projectNavigationGeneration += 1; openingProject = null; renderProjectActivity(); }
export function workbenchNavigationGuard() {
  const generation = projectNavigationGeneration;
  return () => generation === projectNavigationGeneration;
}
export function setBrowsingProject(path) { browsingProject = path || null; }
export function workbenchProject() { return browsingProject || (isGeneralChat() ? null : currentProject || lastProjectPrefs.current); }

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
  const global = view === "workspace" || view === "settings" || (isGeneralChat() && !browsingProject);
  document.body.dataset.appScope = global ? "global" : "project";
  document.body.dataset.projectPreview = String(view === "project");
  const path = workbenchProject();
  const name = $("project-space-name");
  if (name) name.textContent = path ? projectDisplayName(path) : t("未选择项目");
  for (const button of document.querySelectorAll(".workbench-project-link")) {
    const selected = !global && active_space === "dev" && button.dataset.path === path;
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
    const local = sameProject(path, currentProject) && processItems.length;
    const lines = local ? processItems : project?.lines || [];
    const unavailable = !local && (!project || Boolean(project.error) || ["stale", "unavailable"].includes(project.freshness));
    const phases = lines.filter(line => line.profile !== "research").map(line => executionActivity(line, sessionStates.get(line.session_id), {
      waiting: questionSessions.has(line.session_id) || awaitingUserSessions.has(line.session_id), unavailable,
    }).state);
    const state = ["running", "starting", "stopping", "attention", "failed", "waiting", "unknown", "stopped"].find(phase => phases.includes(phase))
      || (unavailable ? "unknown" : "idle");
    button.dataset.activity = state;
    const loading = sameProject(openingProject, path), count = phases.filter(phase => phase === "attention").length;
    button.dataset.loading = String(loading);
    button.setAttribute("aria-busy", String(loading));
    button.querySelector(".workbench-project-activity").textContent = ({ waiting: "Ⅱ", attention: "?", stopping: "■", stopped: "■", failed: "!", unknown: "—" })[state] || "";
    const badge = button.querySelector(".workbench-project-attention");
    badge.hidden = !count || state === "attention"; badge.textContent = String(count);
    const approvals = projectApprovalCount(path);
    const dot = button.querySelector(".workbench-project-approval");
    if (dot) dot.hidden = !approvals;
    if (approvals) button.dataset.approval = "true"; else delete button.dataset.approval;
    const label = t(activityLabels[state]) + (count && state !== "attention" ? ` · ${count} ${t("待你回复")}` : "")
      + (approvals ? ` · ${approvals} ${t("条对话等你批准")}` : "") + (loading ? ` · ${t("正在打开")}` : "");
    button.title = `${path}\n${label}`;
    button.setAttribute("aria-label", `${projectDisplayName(path)} · ${label}`);
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
  const same = active_space === "dev" && !workspace_switch_pending && !openingProject && currentProject === path && activeSessionId;
  if (!same) return openProjectSpace(path, "chat");
  setBrowsingProject(path);
  if (document.body.dataset.view === "chat") reconcileWorkbenchView("chat");
  else navigate_view("chat");
  return true;
}

export async function openProjectSpace(path, view = "chat", options = {}) {
  if (!path) { navigate_view("workspace"); return false; }
  if (view === "chat" && active_space === "dev" && !workspace_switch_pending && !openingProject
    && !options.reload && currentProject === path && activeSessionId && document.body.dataset.view === "chat") {
    setBrowsingProject(path); reconcileWorkbenchView("chat"); return true;
  }
  const generation = ++projectNavigationGeneration;
  setBrowsingProject(path);
  openingProject = path; renderProjectActivity();
  try {
    if (active_space !== "dev" || workspace_switch_pending) {
      if (!await switch_workspace("dev", { isCurrent: () => generation === projectNavigationGeneration })) return false;
      if (generation !== projectNavigationGeneration) return false;
    }
    if (view === "project" && !options.activate) {
      navigate_view("project", { prepared: true });
      renderProjectOverview(lastWorkspaceSnapshot);
      await refreshWorkspace();
      return generation === projectNavigationGeneration;
    }
    const prefs = await invoke("projects_select", { path });
    if (generation !== projectNavigationGeneration) return false;
    await enterProject(prefs, { view, activate: options.activate, isCurrent: () => generation === projectNavigationGeneration });
    return generation === projectNavigationGeneration;
  } catch (error) { toastError(`${t("切换项目失败")}: ${error}`); return false; }
  finally { if (generation === projectNavigationGeneration) { openingProject = null; renderProjectActivity(); } }
}

export async function openWorkbenchItem(path, item) {
  if (!await openProjectSpace(path, "documents")) return;
  if (currentProject !== path || workbenchProject() !== path) return;
  await jumpToEntry(item.id, { expand: true });
}

export function renderCurrentItems(project) {
  const root = node("div", "workbench-current-items");
  if (project.freshness === "unavailable" || (project.error && project.freshness !== "stale")) {
    root.append(node("p", "dim", t("项目读取失败")));
    return root;
  }
  const items = project.current_items ?? [];
  for (const item of items) {
    const button = action("", () => void openWorkbenchItem(project.path, item), "workbench-item");
    button.dataset.key = JSON.stringify([project.path, item.kind, item.id]);
    button.append(node("span", "workbench-item-id", item.id), node("span", "workbench-item-title", item.title));
    const state = node("span", `workbench-item-state${item.running ? " running" : ""}`, `${localizedDocStatus(item.status || "")} · ${item.running ? t("推进中") : t("待继续")}`);
    button.append(state);
    if (item.priority) button.append(node("span", "workbench-item-priority", item.priority));
    if (item.batches?.total > 0) {
      const batch = node("span", "workbench-batch", `${t("批次")} ${item.batches.done}/${item.batches.total}`);
      batch.title = item.batches.source === "git" ? t("已完成的开发批次，依据提交记录") : t("登记的批次进度");
      button.append(batch);
    }
    const owners = (item.owner_lines ?? []).map((line) => lineName(line)).filter(Boolean);
    if (owners.length) button.append(node("span", "workbench-item-owner", owners.join(" / ")));
    root.append(button);
  }
  if (!items.length) root.append(node("p", "dim", project.running_lines ? t("有任务运行，尚未绑定条目") : t("暂无推进中的条目")));
  if (project.current_items_total > items.length) root.append(action(`+${project.current_items_total - items.length} ${t("查看全部工作")}`, () => void openProjectSpace(project.path, "documents"), "ghost mini"));
  return root;
}

export function renderProjectOverview(snapshot) {
  const event = new CustomEvent("kz:project-overview", { cancelable: true, detail: { snapshot, project: workbenchProject() } });
  document.dispatchEvent(event);
  if (event.defaultPrevented) return;
  const root = $("project-overview-content");
  if (!root) return;
  const path = workbenchProject();
  const project = snapshot?.projects?.find((p) => p.path === path);
  const revision = JSON.stringify([path, project?.content_revision, project?.freshness, project?.error, t("概览")]);
  if (root._revision === revision) return;
  const executionOpen = root.querySelector(".project-execution")?.open;
  root._revision = revision;
  root.replaceChildren();
  const header = node("header", "project-overview-head");
  header.append(node("p", "workbench-eyebrow", t("项目概览")), node("h1", "", path ? projectDisplayName(path) : t("未选择项目")), node("p", "dim", path || ""));
  root.append(header);
  if (!project) { root.append(node("p", "dim", t("正在读取项目进展…"))); return; }
  if (project.error) root.append(node("p", "workbench-error", `${t("项目读取失败")}: ${project.error}`));
  const section = node("section", "project-overview-section");
  section.append(node("h2", "", t("当前工作")), renderCurrentItems(project));
  root.append(section);
  const counts = project.counts ?? {};
  const attention = node("section", "project-overview-section");
  attention.append(node("h2", "", t("待我处理")));
  for (const [tab, label, count] of [["deliveries", "待你试用", counts.ready_to_try], ["decisions", "待复核决策", counts.decisions], ["decisions", "待补充事实", counts.missing_facts]]) {
    attention.append(action(`${t(label)} · ${count ?? 0}`, () => {
      setWorkspaceConsoleState({ tab, projectFilter: path });
      navigate_view("workspace");
    }, "workbench-attention-link"));
  }
  root.append(attention);
  const execution = node("details", "project-overview-section project-execution");
  execution.open = Boolean(executionOpen);
  execution.append(node("summary", "", `${t("执行状态")} · ${project.running_lines ?? 0} ${t("个对话运行中")}`));
  for (const line of project.lines ?? []) execution.append(node("p", "dim", `${lineName(line)} · ${line.running ? localizedStage(line.stage || "运行中") : t("空闲")}`));
  execution.append(action(t("管理执行与子代理"), () => void openProjectSpace(path, "lines")));
  root.append(execution);
  if (project.recent_progress) root.append(node("p", "workbench-recent dim", `${t("最近进展")}: ${project.recent_progress.label}`));
}

/// 打开某个项目需求页的「想法」页签(「新目标」写进的就是那里)。
export async function openProjectIdeas(path) {
  if (!await openProjectSpace(path, "documents")) return;
  if (currentProject === path && document.body.dataset.view === "documents") $("documents-tab-ideas")?.click();
}

export async function openProjectResource(path, view) {
  if (view === "chat") {
    if (await openProjectSpace(path, "chat")) openSessionHistory($("workbench-chat-history"), path);
    return;
  }
  // 开发规范不是页面而是一张对话框(UX-007):概览需求行旁的入口。先确保这个项目就是当前项目,规范读写都按当前项目走。
  if (view === "conventions") {
    if (currentProject !== path && !await openProjectSpace(path, "project")) return;
    return openConventions();
  }
  if (view !== "research") return openProjectSpace(path, view);
  return openWorkbenchSpace("research");
}

export async function openWorkbenchSpace(space) {
  const generation = ++projectNavigationGeneration;
  const isCurrent = () => generation === projectNavigationGeneration;
  if (!await switch_workspace(space, { isCurrent })) return false;
  if (!isCurrent()) return false;
  if (space === "dev") {
    if (workbenchProject()) return openProjectSpace(workbenchProject());
    navigate_view("workspace");
  } else if (!["research", "chat"].includes(document.body.dataset.view)) navigate_view("research");
  return true;
}

defer(() => {
  // 权限请求排队 / 被答复(07-events.js):侧栏项目行与会话行的「等你批准」圆点立刻跟上,不等 3 秒轮询。
  document.addEventListener("kz:asks-changed", () => renderProjectActivity());
  document.addEventListener("kz:workspace-navigation", event => {
    event.preventDefault(); void openWorkbenchSpace(event.detail.space);
  });
  $("workbench-home")?.addEventListener("click", () => { setWorkspaceConsoleState({ tab: "projects", projectFilter: "" }); navigate_view("workspace"); });
  $("workbench-attention")?.addEventListener("click", () => {
    const event = new CustomEvent("kz:open-work-inbox", { cancelable: true });
    document.dispatchEvent(event);
    if (event.defaultPrevented) return;
    setWorkspaceConsoleState({ tab: "decisions", projectFilter: "" }); navigate_view("workspace");
  });
  // 「对话与历史」只有侧栏时钟图标这一个入口(UX-032);弹层本体在 12-session-tree.js,走 openPopover 的唯一写法(UX-003)。
  $("workbench-chat-history")?.addEventListener("click", (event) => openSessionHistory(event.currentTarget, null));
  // 「项目」标题右侧的「＋」:添加已有文件夹 / 新建项目(与所有项目页头两个按钮同一实现,点它们本人);零项目时这是侧栏唯一入口。
  $("workbench-add-project")?.addEventListener("click", (event) => {
    openMenu(event.currentTarget, [
      { label: t("添加项目文件夹…"), onSelect: () => $("project-add")?.click() },
      { label: t("新建项目…"), onSelect: () => $("project-init")?.click() },
    ], { placement: "bottom-end", label: t("添加项目") });
  });
  $("workbench-new-goal")?.addEventListener("click", () => {
    goalFormGeneration += 1;
    const form = $("workbench-goal-form");
    const select = $("workbench-goal-project");
    select.replaceChildren();
    for (const path of lastProjectPrefs.projects ?? []) { const option = node("option", "", projectDisplayName(path)); option.value = path; select.append(option); }
    // 默认写进用户正在用的项目(所有项目页没有「当前浏览项目」时退回当前项目,而不是下拉里的第一个)。
    const preferred = workbenchProject() || currentProject;
    if (preferred) select.value = preferred;
    form.classList.remove("hidden");
    $("workbench-goal-title").focus();
  });
  $("workbench-goal-cancel")?.addEventListener("click", () => { goalFormGeneration += 1; $("workbench-goal-form").classList.add("hidden"); });
  $("workbench-goal-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const path = $("workbench-goal-project").value;
    const title = $("workbench-goal-title").value.trim();
    if (!path || !title) return;
    const submit = $("workbench-goal-save");
    if (submit.disabled) return;
    const generation = goalFormGeneration;
    submit.disabled = true;
    try {
      await invoke("docs_update", { projectDir: path, kind: "idea", action: "add", id: "", title });
      if (generation === goalFormGeneration && $("workbench-goal-title").value.trim() === title && $("workbench-goal-project").value === path) {
        $("workbench-goal-title").value = "";
        $("workbench-goal-form").classList.add("hidden");
      }
      // 写进的是该项目「需求」页的「想法」页签;toast 给一颗直达按钮,不让用户自己去找(UX-006)。
      toast(t("目标已记录到项目想法(在该项目「需求」页的「想法」页签里),尚未启动执行。"), { action: { label: t("查看想法"), onClick: () => void openProjectIdeas(path) } });
      await refreshWorkspace();
    } catch (error) { toastError(String(error)); }
    finally { submit.disabled = false; }
  });
});
