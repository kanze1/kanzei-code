// 侧栏项目/会话的右键菜单、键盘、拖动排序与会话动作(UX-023~026/029/034/038)。
//
// 所有交互都挂在列表根上做事件委托:会话行每 3 秒可能被重建,行本身不带监听。
//  - 右键:只在命中条目([data-ctx])上拦截,走 00-surface 的 openMenu(虚拟锚点);输入框、消息正文等
//    不在条目上的地方保留原生菜单。Shift+F10 / 菜单键同效(键盘触发的 contextmenu 以行矩形为锚点)。
//  - F2 重命名,Alt+↑/↓ 上移/下移,方向键在行间移动焦点。
//  - 拖动排序用 pointer 事件(不依赖 HTML5 拖放),只在同一个列表、同一个置顶分组内移动;
//    不做跨项目拖会话(改成右键「交给其它项目的主对话…」)。
import { openMenu } from "./00-surface.js";
import { isGeneralChat } from "./03-general-scope.js";
import { $, confirmDialog, defer, discardSessionPane, inputDialog, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, activeProcessId, navigate_view, toast } from "./03-shell.js";
import { fillTemplate } from "./04-structured-parse.js";
import { openProjectModelsDialog } from "./08-project-models.js";
import {
  closeParallelProcess, lastProjectPrefs, processRunning, projectDisplayName, refreshProcesses, removeProject,
  renameProject, renderProjects, shortProjectPath, switchProcess,
} from "./09-sessions.js";
import {
  activateRow, buildRows, closedOpen, dropClosedSessions, dropRemoteSessions, forgetSessionPrefs, invalidateSessionTree, kindWord,
  loadRemoteSessions, closeSessionHistory, historyElement, historyProjectPath, mainSessionOf, newDiscussionIn, newTaskIn, openSession,
  openSessionHistory, orderedProjects, orderRows, projectPinned, projectRunningCount, setClosedOpen, setProjectPinned,
  setSessionDragging, setSessionOrder, setSessionPinned, setSidebarOpen, sidebarOpen,
} from "./12-session-tree.js";
import { openProjectSpace, renderWorkbenchNavigation } from "./12-workbench.js";
import { deleteConversationsForProcess, forgetDeletedSession } from "./15-views-misc.js";
import { openHandoffForm } from "./26-project-conversations.js";
import { sameProject } from "./25-softwire-model.js";

// ---------- 小工具 ----------
const closest = (target, selector) => target?.closest?.(selector) ?? null;
const failure = (error) => toast(String(error), { kind: "err" });
function liveRow(project, id) {
  return buildRows(project, { includeClosed: true }).find((row) => row.id === id) ?? null;
}
function rowRunning(row) {
  if (!row.execution) return false;
  return sameProject(row.project, currentProject) ? processRunning(row.item) : Boolean(row.item.running);
}
async function copyText(text, label) {
  try {
    await navigator.clipboard.writeText(String(text));
    toast(`${t("已复制")}:${label}`, { kind: "ok" });
  } catch (error) {
    failure(`${t("复制失败")}:${error}`);
  }
}
/// 列表变了之后让两处视图(侧栏树、历史弹层)和对话页头一起跟上。
async function refreshAfterChange(project) {
  if (sameProject(project, currentProject)) await refreshProcesses();
  else await loadRemoteSessions(project, { force: true });
  // 关闭 / 删除都可能改变「已关闭」清单(关掉的线路进去,真删的不会进):重读。
  await dropClosedSessions(project);
  invalidateSessionTree();
  document.dispatchEvent(new CustomEvent("kz:sessions-changed"));
}

// ---------- 打开方式(资源管理器 / 外部工具) ----------
let toolsCache = null;
let toolsAt = 0;
let toolsLoading = null;
export function loadOpenTools({ force = false } = {}) {
  if (toolsLoading) return toolsLoading;
  if (!force && toolsCache && Date.now() - toolsAt < 60000) return Promise.resolve(toolsCache);
  toolsLoading = invoke("open_tools_list")
    .then((list) => { toolsCache = Array.isArray(list) ? list : []; toolsAt = Date.now(); return toolsCache; })
    .catch(() => toolsCache ?? [])
    .finally(() => { toolsLoading = null; });
  return toolsLoading;
}
/// 菜单要同步构建:有缓存就用(后台悄悄刷新),第一次最多等 700ms(探测命令是子进程,可能慢)。
async function toolsForMenu() {
  if (toolsCache) {
    void loadOpenTools();
    return toolsCache;
  }
  return Promise.race([loadOpenTools(), new Promise((resolve) => setTimeout(() => resolve([]), 700))]);
}
function toolItems(tools, run) {
  const usable = tools.filter((tool) => tool.available !== false);
  if (!usable.length) {
    return [{
      label: `${t("在设置里添加打开方式")}…`,
      onSelect: () => {
        navigate_view("settings");
        document.dispatchEvent(new CustomEvent("kz:open-settings-section", { detail: { id: "sg-open-tools" } }));
      },
    }];
  }
  return usable.map((tool) => ({
    label: fillTemplate(t("用 {tool} 打开"), { tool: tool.builtin ? t(tool.label) : tool.label || tool.id }),
    onSelect: () => run(tool),
  }));
}
async function revealPath(project, processId) {
  try {
    await invoke("reveal_path", { projectDir: project, ...(processId ? { processId } : {}) });
  } catch (error) { failure(error); }
}
async function openWithTool(tool, project, processId) {
  try {
    await invoke("open_with", { toolId: tool.id, projectDir: project, ...(processId ? { processId } : {}) });
  } catch (error) { failure(error); }
}

// ---------- 会话动作 ----------
export async function renameSession(project, id) {
  const row = liveRow(project, id);
  if (!row) return;
  const value = await inputDialog({
    title: t("重命名对话"),
    message: t("留空则恢复默认名称"),
    value: row.name,
    placeholder: kindWord(row.kind),
    maxLength: 60, // 与后端 naming.rs 的 TITLE_MAX_CHARS 一致
  });
  if (value === null) return;
  const title = value.trim();
  if (title === row.name && !row.custom) return;
  try {
    await invoke("process_rename", { projectDir: project, processId: id, title });
  } catch (error) { failure(error); return; }
  await refreshAfterChange(project);
  toast(title ? t("已重命名") : t("已恢复默认名称"), { kind: "ok" });
}
export function toggleSessionPin(project, id) {
  const row = liveRow(project, id);
  if (!row || !row.capabilities.reorder) return;
  setSessionPinned(id, !row.pinned);
  invalidateSessionTree();
}
export function moveSession(project, id, delta) {
  const rows = orderRows(project);
  const row = rows.find((candidate) => candidate.id === id);
  if (!row || !row.capabilities.reorder) return false;
  const group = rows.filter((candidate) => candidate.capabilities.reorder && candidate.pinned === row.pinned);
  const from = group.findIndex((candidate) => candidate.id === id);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= group.length) return false;
  [group[from], group[to]] = [group[to], group[from]];
  const rest = rows.filter((candidate) => candidate.capabilities.reorder);
  const pinned = row.pinned ? group : rest.filter((candidate) => candidate.pinned);
  const others = row.pinned ? rest.filter((candidate) => !candidate.pinned) : group;
  setSessionOrder(project, [...pinned, ...others].map((candidate) => candidate.id));
  invalidateSessionTree();
  return true;
}
/// 拖动落下:visibleIds 是这一组里**可见行**的新顺序(列表只列前 N 条,其余保持相对位置)。
function reorderSessions(project, pinned, visibleIds) {
  const rows = orderRows(project).filter((row) => row.capabilities.reorder);
  const group = rows.filter((row) => row.pinned === pinned).map((row) => row.id);
  const shown = new Set(visibleIds);
  const slots = group.map((id, index) => (shown.has(id) ? index : -1)).filter((index) => index >= 0);
  slots.forEach((slot, order) => { group[slot] = visibleIds[order]; });
  const pinnedIds = pinned ? group : rows.filter((row) => row.pinned).map((row) => row.id);
  const otherIds = pinned ? rows.filter((row) => !row.pinned).map((row) => row.id) : group;
  setSessionOrder(project, [...pinnedIds, ...otherIds]);
  invalidateSessionTree();
}

/// 删除对话 = 真删(不可恢复)。确认框点名标题、标出当前对话;运行中拒绝并写明原因;带工作树的独立任务
/// 按关闭独立任务的语义处理(已合并且干净的树才回收,不静默)。
export async function deleteSession(project, id) {
  const row = liveRow(project, id);
  if (!row || !row.capabilities.delete) return;
  if (rowRunning(row)) {
    toast(t("运行中,先停止再删除对话"), { kind: "warn" });
    return;
  }
  const current = sameProject(project, currentProject) && id === activeProcessId;
  const list = [t("对话消息、工具调用与结果"), t("运行轨迹与子代理记录")];
  if (row.worktree) list.push(t("独立任务的工作树:已合并且干净的才会回收,有独有内容的会保留"));
  list.push(t("保留:用量统计、已提炼的记忆、需求与缺陷记录"));
  const ok = await confirmDialog({
    title: t("删除对话"),
    message: `「${row.name}」${current ? ` ${t("(当前打开的对话)")}` : ""}\n${t("删除后无法恢复。")}`,
    list,
    okText: t("删除"),
    danger: true,
  });
  if (!ok) return;
  let outcome = "";
  try {
    outcome = await invoke("process_purge", { projectDir: project, processId: id });
  } catch (error) {
    failure(error);
    return;
  }
  forgetSessionPrefs(project, id);
  await refreshAfterChange(project);
  forgetDeletedSession(row.session_id);
  // 工作树因有未合并内容被保留时,后端回执里写了保留在哪:原样给用户(不静默)。
  const kept = typeof outcome === "string" && outcome.includes("仍保留") ? outcome : "";
  toast(kept || `${t("已删除")}:${row.name}`, { kind: kept ? "warn" : "ok" });
}
/// 清空主对话:主对话不能注销,只删它的全部对话段(留下一个空的主对话)。
export async function clearMainConversation(project, id) {
  const row = liveRow(project, id);
  if (!row) return;
  if (rowRunning(row)) {
    toast(t("运行中,先停止再清空对话"), { kind: "warn" });
    return;
  }
  let segments = [];
  try {
    const items = await invoke("conversation_list", { projectDir: project, processId: id });
    segments = (items ?? []).flatMap((item) => item.sequences ?? [item.sequence]).filter((value) => Number.isFinite(Number(value)));
  } catch (error) { failure(error); return; }
  if (!segments.length) {
    toast(t("这段对话本来就是空的"));
    return;
  }
  await deleteConversationsForProcess(id, segments, {
    project, title: t("清空对话"), subject: row.name, okText: t("清空"),
  });
}
export async function closeSession(project, id) {
  const row = liveRow(project, id);
  if (!row || row.kind !== "task") return;
  if (sameProject(project, currentProject)) { await closeParallelProcess(id); return; }
  const warning = rowRunning(row)
    ? t("独立任务仍在运行，关闭会先停止它，等它收尾。")
    : t("关闭只会注销这个独立任务的登记。已合并且干净的工作树会自动回收；有独有内容的工作树会保留。");
  if (!await confirmDialog({
    title: t("关闭独立任务"),
    message: `「${row.name}」\n${warning}\n${t("关闭后，可在搜索对话或历史中查看这段对话；要连对话一起删除，请用「删除对话…」。")}`,
    okText: t("关闭独立任务"),
    danger: true,
  })) return;
  try {
    await invoke("process_close", { processId: id });
  } catch (error) { failure(`${t("关闭独立任务失败")}:${error}`); return; }
  discardSessionPane(row.session_id);
  await refreshAfterChange(project);
}
async function handoffToProject(source, targetProject) {
  await loadRemoteSessions(targetProject);
  const main = mainSessionOf(targetProject);
  if (!main) { toast(t("目标项目的主对话暂不可用"), { kind: "warn" }); return; }
  if (!await openSession(source.project, source.id)) return;
  openHandoffForm({ target: { project: targetProject, main, name: projectDisplayName(targetProject) } });
}

// ---------- 项目动作 ----------
export function toggleProjectPin(path) {
  setProjectPinned(path, !projectPinned(path));
  renderWorkbenchNavigation();
}
async function applyProjectOrder(paths) {
  try {
    await invoke("projects_reorder", { paths });
    renderProjects(await invoke("projects_get"), { activate: false });
  } catch (error) { failure(error); }
}
export async function moveProject(path, delta) {
  const order = orderedProjects(lastProjectPrefs);
  const pinned = projectPinned(path);
  const group = order.filter((candidate) => projectPinned(candidate) === pinned);
  const from = group.indexOf(path);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= group.length) return false;
  [group[from], group[to]] = [group[to], group[from]];
  await applyProjectOrder(pinned ? [...group, ...order.filter((candidate) => !projectPinned(candidate))]
    : [...order.filter((candidate) => projectPinned(candidate)), ...group]);
  return true;
}
async function reorderProjectsByDrag(pinned, visibleOrder) {
  const order = orderedProjects(lastProjectPrefs);
  const group = order.filter((candidate) => projectPinned(candidate) === pinned);
  const shown = new Set(visibleOrder);
  const slots = group.map((path, index) => (shown.has(path) ? index : -1)).filter((index) => index >= 0);
  slots.forEach((slot, rank) => { group[slot] = visibleOrder[rank]; });
  await applyProjectOrder(pinned ? [...group, ...order.filter((candidate) => !projectPinned(candidate))]
    : [...order.filter((candidate) => projectPinned(candidate)), ...group]);
}
async function exportProjectData(path) {
  try {
    const outputDir = await invoke("export_pick_dir");
    if (!outputDir) return;
    const result = await invoke("export_project_data", {
      options: { projectDir: path, outputDir, includeMemory: true, includeRequirements: true, includeDefects: true, includeConfig: true },
    });
    toast(`${t("导出完成")}: ${result.path}`, { kind: "ok" });
  } catch (error) { failure(`${t("导出失败")}:${error}`); }
}

// ---------- 菜单 ----------
/// 清理菜单项:去掉头尾与连续的分隔线(分组可能因条件整组为空)。
function tidy(items) {
  const out = [];
  for (const item of items) {
    if (!item) continue;
    if (item === "separator" && (!out.length || out[out.length - 1] === "separator")) continue;
    out.push(item);
  }
  while (out[out.length - 1] === "separator") out.pop();
  return out;
}
/// 虚拟锚点:右键落点(或键盘触发时行矩形左下角)处一个 1×1 的透明块,菜单对它定位。
/// 锚点放进它所在的弹层(历史弹层是 light-dismiss 的,锚点在里面才不会因为开菜单先把弹层关掉)。
function makeAnchor(point, host) {
  const anchor = document.createElement("div");
  anchor.className = "kz-ctx-anchor";
  anchor.setAttribute("aria-hidden", "true");
  anchor.style.setProperty("left", `${Math.round(point.x)}px`);
  anchor.style.setProperty("top", `${Math.round(point.y)}px`);
  (host ?? document.body).appendChild(anchor);
  return anchor;
}
function showMenu(point, host, items, { label, focusKey } = {}) {
  const anchor = makeAnchor(point, host);
  const handle = openMenu(anchor, tidy(items), {
    placement: "bottom-start",
    label,
    onClose: () => {
      anchor.remove();
      // 菜单里的焦点会随菜单消失:回到触发它的那一行(行可能被重建,按 key 找)。
      if (focusKey) requestAnimationFrame(() => focusRowByKey(focusKey));
    },
  });
  if (!handle || handle.closed) anchor.remove();
  else {
    handle.el.classList.add("is-context");
    // openMenu 把焦点给第一项,而第一项(「打开」)常是禁用的:改给第一个可用项。
    handle.el.querySelector(".k-menu-item:not([aria-disabled='true'])")?.focus?.();
  }
  return handle;
}
function focusRowByKey(key) {
  const roots = [$("workbench-project-list"), $("workbench-general-list"), historyElement()];
  for (const root of roots) {
    for (const node of root?.querySelectorAll?.("[data-key], [data-ctx='project']") ?? []) {
      const nodeKey = node.dataset.key ?? `project\u001f${node.dataset.path}`;
      if (nodeKey === key && !(node.offsetParent === null)) {
        (node.dataset.ctx === "project" ? node.querySelector(".workbench-project-link") : node.firstChild)?.focus?.({ preventScroll: true });
        return;
      }
    }
  }
}

/// 菜单项弹出的对话框(重命名 / 删除 / 关闭 / 移除…)关掉之后,焦点回到触发的那一行:菜单已经不在了,
/// 对话框记下的「打开者」是个被摘掉的菜单项,不接手键盘用户就掉到 body、要从页首重新 Tab。
function afterDialog(key, work) {
  void Promise.resolve(work).finally(() => requestAnimationFrame(() => focusRowByKey(key)));
}

// 常用动作保持一屏；复制、排序、交付与外部工具放到二级菜单。
function compactContextMenu(point, host, items, options, primaryLabels) {
  const primary = items.filter(item => item && primaryLabels.includes(item.label));
  const details = items.filter(item => item === "separator" || (item && !primaryLabels.includes(item.label)));
  return showMenu(point, host, [
    ...primary.filter(item => !item.danger),
    { label: `${t("更多操作")}…`, onSelect: () => showMenu(point, host, details, options) },
    "separator",
    ...primary.filter(item => item.danger),
  ], options);
}

export async function openSessionContextMenu(wrap, point, host) {
  const project = wrap.dataset.project;
  const id = wrap.dataset.processId;
  const row = liveRow(project, id);
  if (!row) return null;
  const general = isGeneralChat(project);
  const tools = general ? [] : await toolsForMenu();
  const live = liveRow(project, id);
  if (!live) return null;
  const main = live.kind === "main";
  const task = !general && live.kind === "task";
  const discussion = live.kind === "discussion";
  const running = rowRunning(live);
  const here = sameProject(project, currentProject) && id === activeProcessId && document.body.dataset.view === "chat";
  const group = orderRows(project).filter((candidate) => candidate.capabilities.reorder && candidate.pinned === live.pinned);
  const index = group.findIndex((candidate) => candidate.id === id);
  const runningWhy = running ? t("运行中,先停止再删除") : "";
  const items = [
    { label: t("打开"), disabled: here, onSelect: () => void openSession(project, id) },
    { label: t(general ? "新对话" : "在此项目新建讨论"), onSelect: () => void newDiscussionIn(project) },
    "separator",
    { label: `${t("重命名")}…`, kbd: "F2", onSelect: () => afterDialog(wrap.dataset.key, renameSession(project, id)) },
    live.capabilities.reorder && { label: live.pinned ? t("取消置顶") : t("置顶"), onSelect: () => toggleSessionPin(project, id) },
    live.capabilities.reorder && { label: t("上移"), kbd: "Alt+↑", disabled: index <= 0, onSelect: () => moveSession(project, id, -1) },
    live.capabilities.reorder && { label: t("下移"), kbd: "Alt+↓", disabled: index < 0 || index >= group.length - 1, onSelect: () => moveSession(project, id, 1) },
    "separator",
    !general && { label: t("在资源管理器中打开"), onSelect: () => void revealPath(project, id) },
    ...(general ? [] : toolItems(tools, (tool) => void openWithTool(tool, project, id))),
    "separator",
    { label: t("复制对话名"), onSelect: () => void copyText(live.name, live.name) },
    { label: t("复制对话 ID"), onSelect: () => void copyText(live.session_id, t("对话 ID")) },
    live.worktree && { label: t("复制工作树路径"), onSelect: () => void copyText(live.worktree, t("工作树路径")) },
    live.branch && { label: t("复制分支名"), desc: live.branch, onSelect: () => void copyText(live.branch, live.branch) },
    "separator",
    !general && discussion && { label: `${t("交给主对话")}…`, onSelect: async () => { if (await openSession(project, id)) openHandoffForm({}); } },
    !general && !main && { label: `${t("交给其它项目的主对话")}…`, onSelect: () => chooseHandoffProject(point, host, live) },
    task && { label: t("查看独立任务详情"), onSelect: () => void openProjectSpace(project, "lines") },
    "separator",
    task && { label: `${t("关闭独立任务")}…`, danger: true, onSelect: () => afterDialog(wrap.dataset.key, closeSession(project, id)) },
    !main && { label: `${t("删除对话")}…`, danger: true, disabled: running, desc: runningWhy || undefined, onSelect: () => afterDialog(wrap.dataset.key, deleteSession(project, id)) },
    main && {
      label: `${t("清空对话")}…`, danger: true, disabled: running,
      desc: running ? t("运行中,先停止再清空对话") : undefined,
      onSelect: () => afterDialog(wrap.dataset.key, clearMainConversation(project, id)),
    },
  ];
  return compactContextMenu(point, host, items, { label: live.name, focusKey: wrap.dataset.key }, [
    `${t("重命名")}…`, t("置顶"), t("取消置顶"),
    `${t("关闭独立任务")}…`, `${t("删除对话")}…`, `${t("清空对话")}…`,
  ]);
}
function chooseHandoffProject(point, host, source) {
  const others = (lastProjectPrefs.projects ?? []).filter((path) => !sameProject(path, source.project));
  if (!others.length) { toast(t("没有其它项目可交付")); return; }
  showMenu(point, host, [
    { heading: t("交给哪个项目的主对话") },
    ...others.map((path) => ({
      label: projectDisplayName(path), desc: shortProjectPath(path), onSelect: () => void handoffToProject(source, path),
    })),
  ], { label: t("选择目标项目") });
}

export async function openProjectContextMenu(row, point, host) {
  const path = row.dataset.path;
  if (!path) return null;
  const tools = await toolsForMenu();
  const order = orderedProjects(lastProjectPrefs);
  const pinned = projectPinned(path);
  const group = order.filter((candidate) => projectPinned(candidate) === pinned);
  const index = group.indexOf(path);
  const running = projectRunningCount(path);
  const name = projectDisplayName(path);
  const items = [
    { label: t("打开"), onSelect: () => void openProjectSpace(path, "chat", { main: true }) },
    { label: t("新建讨论"), onSelect: () => void newDiscussionIn(path) },
    { label: t("新建独立任务"), onSelect: () => void newTaskIn(path) },
    "separator",
    { label: `${t("重命名项目")}…`, kbd: "F2", desc: t("只改显示名,不改磁盘文件夹"), onSelect: () => afterDialog(`project\u001f${path}`, renameProject(path)) },
    { label: pinned ? t("取消置顶") : t("置顶"), onSelect: () => toggleProjectPin(path) },
    { label: t("上移"), kbd: "Alt+↑", disabled: index <= 0, onSelect: () => void moveProject(path, -1) },
    { label: t("下移"), kbd: "Alt+↓", disabled: index < 0 || index >= group.length - 1, onSelect: () => void moveProject(path, 1) },
    "separator",
    { label: t("在资源管理器中打开"), onSelect: () => void revealPath(path) },
    ...toolItems(tools, (tool) => void openWithTool(tool, path)),
    { label: t("复制路径"), desc: shortProjectPath(path), onSelect: () => void copyText(path, t("项目路径")) },
    "separator",
    { label: t("项目概览"), onSelect: () => void openProjectSpace(path, "project") },
    { label: t("需求"), onSelect: () => void openProjectSpace(path, "documents") },
    { label: t("并行线路"), onSelect: () => void openProjectSpace(path, "lines") },
    { label: `${t("项目模型")}…`, onSelect: () => void openProjectModelsDialog(path, { name }) },
    { label: `${t("导出工作资料")}…`, onSelect: () => void exportProjectData(path) },
    "separator",
    {
      label: `${t("移除项目")}…`, danger: true, disabled: running > 0,
      desc: running > 0 ? t("有运行中的对话,先停止它们") : t("只解除登记,不删除磁盘文件"),
      onSelect: () => afterDialog(`project\u001f${path}`, removeProject(path).then(() => { dropRemoteSessions(path); invalidateSessionTree(); })),
    },
  ];
  return compactContextMenu(point, host, items, { label: name, focusKey: `project\u001f${path}` }, [
    t("新建讨论"), `${t("重命名项目")}…`, t("置顶"), t("取消置顶"),
    t("在资源管理器中打开"), `${t("移除项目")}…`,
  ]);
}

// ---------- 事件委托 ----------
let keyOpenedAt = 0;
function rowPoint(node) {
  const rect = node.getBoundingClientRect();
  return { x: rect.left + 16, y: rect.bottom - 4 };
}
function entryOf(target) {
  return closest(target, "[data-ctx='session'], [data-ctx='project'], [data-ctx='segment'], [data-ctx='closed']");
}
function openEntryMenu(entry, point, host) {
  if (entry.dataset.ctx === "session") return openSessionContextMenu(entry, point, host);
  if (entry.dataset.ctx === "project") return openProjectContextMenu(entry, point, host);
  if (entry.dataset.ctx === "segment") return openSegmentMenu(entry, point, host);
  if (entry.dataset.ctx === "closed") return openClosedMenu(entry, point, host);
  return null;
}
/// 历史与当前对话共享管理动作；“只读”只限制继续执行。
function openClosedMenu(entry, point, host) {
  const name = entry.dataset.title || "";
  return showMenu(point, host, [
    { label: t("查看(只读)"), onSelect: () => void activateRow(entry) },
    { label: `${t("重命名")}…`, kbd: "F2", onSelect: () => afterDialog(entry.dataset.key, renameSession(entry.dataset.project, entry.dataset.processId)) },
    { label: t("复制对话名"), onSelect: () => void copyText(name, name) },
    "separator",
    { label: `${t("删除对话")}…`, danger: true, onSelect: () => afterDialog(entry.dataset.key, deleteSession(entry.dataset.project, entry.dataset.processId)) },
  ], { label: name, focusKey: entry.dataset.key });
}
function openSegmentMenu(entry, point, host) {
  const project = entry.dataset.project;
  const processId = entry.dataset.processId;
  const sequences = JSON.parse(entry.dataset.seqs || "[]");
  const title = entry.dataset.title;
  return showMenu(point, host, [
    { label: t("查看(只读)"), onSelect: () => void activateRow(entry) },
    "separator",
    {
      label: `${t("删除这段历史")}…`, danger: true,
      onSelect: () => afterDialog(entry.dataset.key, deleteConversationsForProcess(processId, sequences, { project, subject: title })),
    },
  ], { label: title, focusKey: entry.dataset.key });
}
function rowOfFocus() {
  return entryOf(document.activeElement);
}
function activeLink(event) {
  return closest(event.target, ".workbench-project-link, .workbench-session-link, .workbench-session-more, .workbench-closed-toggle");
}
function visibleLinks(root) {
  return [...root.querySelectorAll(".workbench-project-link, .workbench-session-link, .workbench-session-more, .workbench-closed-toggle")]
    .filter((node) => node.offsetParent !== null);
}

export function installSessionInteractions(root, { host = null } = {}) {
  if (!root || root._kzSessionBound) return;
  root._kzSessionBound = true;

  root.addEventListener("click", (event) => {
    const menu = closest(event.target, "[data-act='row-menu']");
    if (menu) {
      event.stopPropagation();
      const entry = entryOf(menu);
      if (entry) void openEntryMenu(entry, rowPoint(menu), closest(entry, "[popover]") ?? host);
      return;
    }
    const more = closest(event.target, ".workbench-session-more");
    if (more) {
      openSessionHistory(more, more.dataset.project);
      return;
    }
    const closedToggle = closest(event.target, "[data-act='closed-toggle']");
    if (closedToggle) {
      setClosedOpen(closedToggle.dataset.project, !closedOpen(closedToggle.dataset.project));
      invalidateSessionTree();
      return;
    }
    const act = closest(event.target, "[data-act]")?.dataset.act;
    if (act === "new-discussion" || act === "new-task") {
      const project = historyProjectPath();
      if (!project) return;
      closeSessionHistory();
      void (act === "new-discussion" ? newDiscussionIn(project) : newTaskIn(project));
      return;
    }
    const link = closest(event.target, ".workbench-session-link");
    const wrap = link && closest(link, "[data-ctx='session'], [data-ctx='segment'], [data-ctx='closed']");
    if (wrap) void activateRow(wrap);
  });

  root.addEventListener("contextmenu", (event) => {
    const entry = entryOf(event.target);
    if (!entry) return; // 条目之外:保留原生菜单
    event.preventDefault();
    // 键盘触发(Shift+F10 / 菜单键):button 不是 2,没有指针落点,改用行矩形。
    if (event.button !== 2 && performance.now() - keyOpenedAt < 400) return;
    const point = event.button === 2 ? { x: event.clientX, y: event.clientY } : rowPoint(entry);
    void openEntryMenu(entry, point, closest(entry, "[popover]") ?? host);
  });

  root.addEventListener("keydown", (event) => {
    if (event.isComposing) return;
    const entry = rowOfFocus();
    const menuKey = event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey);
    if (menuKey && entry && root.contains(entry)) {
      event.preventDefault();
      keyOpenedAt = performance.now();
      void openEntryMenu(entry, rowPoint(entry), closest(entry, "[popover]") ?? host);
      return;
    }
    if (!entry || !root.contains(entry) || closest(event.target, "input, textarea, select")) return;
    if (entry.dataset.ctx === "project" && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
      event.preventDefault();
      const path = entry.dataset.path;
      const open = event.key === "ArrowRight";
      if (open !== sidebarOpen(path)) {
        setSidebarOpen(path, open);
        invalidateSessionTree();
        if (open && !sameProject(path, currentProject)) void loadRemoteSessions(path);
      }
      return;
    }
    if (event.key === "F2") {
      event.preventDefault();
      if (entry.dataset.ctx === "session" || entry.dataset.ctx === "closed") void renameSession(entry.dataset.project, entry.dataset.processId);
      else if (entry.dataset.ctx === "project") void renameProject(entry.dataset.path);
      return;
    }
    if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      const delta = event.key === "ArrowUp" ? -1 : 1;
      if (entry.dataset.ctx === "session") moveSession(entry.dataset.project, entry.dataset.processId, delta);
      else if (entry.dataset.ctx === "project") void moveProject(entry.dataset.path, delta);
      return;
    }
    // 方向键:在行间移动焦点(树只有一个 Tab 停靠点太窄,逐行 Tab 又太长)。
    if ((event.key === "ArrowDown" || event.key === "ArrowUp") && !event.altKey && !event.ctrlKey && !event.metaKey) {
      const links = visibleLinks(root);
      const from = links.indexOf(activeLink(event));
      if (from < 0) return;
      const next = links[from + (event.key === "ArrowDown" ? 1 : -1)];
      if (next) { event.preventDefault(); next.focus(); }
    }
  });

  installRowDrag(root);
}

// ---------- 拖动排序 ----------
const DRAG_THRESHOLD = 5;
function installRowDrag(root) {
  let pending = null;
  let drag = null;
  const clearMarks = () => {
    for (const node of root.querySelectorAll(".drop-before, .drop-after")) node.classList.remove("drop-before", "drop-after");
  };
  const detach = () => {
    document.removeEventListener("pointermove", onMove, true);
    document.removeEventListener("pointerup", onUp, true);
    document.removeEventListener("pointercancel", onCancel, true);
    document.removeEventListener("keydown", onKey, true);
  };
  const finish = (commit) => {
    const state = drag;
    drag = null;
    pending = null;
    detach();
    if (!state) return;
    clearMarks();
    state.row.classList.remove("is-dragging");
    delete document.documentElement.dataset.kzRowDrag;
    // 拖动结束的那一下 click 要吞掉,否则落在行上会被当成「打开」。
    const swallow = (event) => { event.stopPropagation(); event.preventDefault(); };
    root.addEventListener("click", swallow, true);
    setTimeout(() => root.removeEventListener("click", swallow, true), 0);
    setSessionDragging(false);
    // 落在自己上沿/下沿(index 为 from 或 from+1)= 没动。
    const target = state.index > state.from ? state.index - 1 : state.index;
    if (commit && target !== state.from) {
      const ids = state.siblings.slice();
      const [moved] = ids.splice(state.from, 1);
      ids.splice(target, 0, moved);
      const kind = state.row.dataset.drag;
      if (kind === "project") void reorderProjectsByDrag(state.pinned, ids);
      else reorderSessions(state.row.dataset.project, state.pinned, ids);
    }
  };
  const begin = () => {
    const row = pending.row;
    const kind = row.dataset.drag;
    const group = kind === "project" ? row.parentNode?.parentNode : row.parentNode;
    if (!group) return null;
    const pinned = row.dataset.pinned === "true";
    const nodes = [...group.children].filter((node) => {
      const probe = kind === "project" ? node._row : node;
      return probe?.dataset?.drag === kind && probe.dataset.pinned === String(pinned);
    });
    const self = kind === "project" ? row.parentNode : row;
    const from = nodes.indexOf(self);
    if (nodes.length < 2 || from < 0) return null;
    const idOf = (node) => (kind === "project" ? node.dataset.path : node.dataset.processId);
    return { row, self, kind, pinned, nodes, from, index: from, siblings: nodes.map(idOf) };
  };
  const onMove = (event) => {
    if (!pending || event.pointerId !== pending.id) return;
    if (!drag) {
      if (Math.abs(event.clientX - pending.x) + Math.abs(event.clientY - pending.y) < DRAG_THRESHOLD) return;
      drag = begin();
      if (!drag) { pending = null; detach(); return; }
      setSessionDragging(true);
      drag.row.classList.add("is-dragging");
      document.documentElement.dataset.kzRowDrag = "1";
    }
    event.preventDefault?.();
    let index = drag.nodes.length;
    for (let i = 0; i < drag.nodes.length; i += 1) {
      const rect = (drag.kind === "project" ? drag.nodes[i]._row : drag.nodes[i]).getBoundingClientRect();
      if (event.clientY < rect.top + rect.height / 2) { index = i; break; }
    }
    drag.index = index;
    clearMarks();
    const mark = (node) => (drag.kind === "project" ? node._row : node);
    if (index < drag.nodes.length) mark(drag.nodes[index]).classList.add("drop-before");
    else mark(drag.nodes[drag.nodes.length - 1]).classList.add("drop-after");
  };
  // 松手在列表范围之外 = 取消(落点是按 clientY 对着列表行折算的,松在聊天区也会被折成「某个位置」);
  // 拖动中按 Esc 同样取消,并把这一下 Esc 吞掉(不能顺带触发「返回上一页」)。
  const onUp = (event) => {
    const box = root.getBoundingClientRect();
    const inside = event.clientX >= box.left && event.clientX <= box.right && event.clientY >= box.top && event.clientY <= box.bottom;
    finish(inside);
  };
  const onCancel = () => finish(false);
  const onKey = (event) => {
    if (event.key !== "Escape" || !drag) return;
    event.preventDefault();
    event.stopPropagation();
    finish(false);
  };
  root.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    const row = closest(event.target, "[data-drag]");
    if (!row || closest(event.target, ".workbench-session-more, [data-act]")) return;
    if (pending || drag) finish(false);
    pending = { row, id: event.pointerId, x: event.clientX, y: event.clientY };
    document.addEventListener("pointermove", onMove, true);
    document.addEventListener("pointerup", onUp, true);
    document.addEventListener("pointercancel", onCancel, true);
    document.addEventListener("keydown", onKey, true);
  });
}

defer(() => {
  installSessionInteractions($("workbench-project-list"));
  installSessionInteractions($("workbench-general-list"));
  installSessionInteractions(historyElement(), { host: historyElement() });
});
