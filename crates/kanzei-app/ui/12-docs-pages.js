import { openProjectSpace, renderCurrentItems, renderProjectOverview, renderProjectActivity } from "./12-workbench.js";
import { closeSurface, isSurfaceOpen, openMenu } from "./00-surface.js";
import { defer } from "./01-core.js";
import { localizeDynamic } from "./02-i18n.js";
import { $, invoke, on } from "./01-core.js";
import { applyLanguage, localizedDocStatus, t } from "./02-i18n.js";
import {
  activeProcessId,
  activeSessionId,
  processItems,
  running,
  toastError,
} from "./03-shell.js";
import { selectedWorkPriority } from "./08-auto.js";
import { state } from "./08-compose.js";
import { removeProject, renameProject, switchProject } from "./09-sessions.js";
import {
  NEUTRAL_DOC_FILTERS,
  entryBlocked,
  saveDocFilters,
  syncTagFilter,
} from "./10-docs-core.js";
import { batchSelection, consumePendingJump, renderDocList, syncBatchBar } from "./11-docs-list.js";
import { renderIncidentMetrics } from "./13-memory.js";
import { applyDocFilter, clearDocFilters, refreshDocs } from "./14-docs-actions.js";
import { syncConventionsEntry } from "./15-conventions.js";
import { renderLineWorkItemOptions } from "./20-lines.js";
import { renderDecisionConsole, workspaceConsoleBusy, workspaceConsoleState } from "./12-decision-console.js";

export function formatWorkspaceTime(value) {
  if (!value) return t("暂无时间");
  return new Date(Number(value)).toLocaleString();
}

export async function selectWorkspaceProject(path) {
  // D-355:Workspace 卡片、文档页下拉、侧栏项目菜单、命令面板复用同一切换事务(switchProject → enterProject)——
  // 目标 process_list → active session → conversation_get 原子链一致。
  if (await switchProject(path, { view: "documents" })) refreshWorkspace();
}

// 卡片 ⋯:重命名/移除(UI2-0926 #1:项目的增删改收在项目总览页,侧栏只剩项目卡菜单)。
export let workspaceMenuHandle = null;
export function openWorkspaceCardMenu(anchor, project) {
  const handle = openMenu(anchor, [
    { label: `${t("重命名项目")}…`, onSelect: () => void renameProject(project.path) },
    { label: t("移除项目"), desc: t("只解除登记,不会删除磁盘文件。"), danger: true, onSelect: () => void removeProject(project.path) },
  ], {
    placement: "bottom-end",
    label: `${t("更多操作")} ${project.name}`,
    onClose: () => {
      if (workspaceMenuHandle === handle) workspaceMenuHandle = null;
    },
  });
  workspaceMenuHandle = handle && !handle.closed ? handle : null;
  return workspaceMenuHandle;
}

export let lastWorkspaceSnapshot = null;
let workspaceRefreshEpoch = 0;
let workspaceRequests = 0;
export function renderWorkspace(snapshot) {
  lastWorkspaceSnapshot = snapshot;
  renderProjectActivity();
  const root = $("workspace-projects");
  if (!root) return;
  const existing = new Map([...root.children].map((el) => [el.dataset.path, el]));
  const keep = new Set();
  for (const project of snapshot.projects ?? []) {
    keep.add(project.path);
    let card = existing.get(project.path);
    const revision = JSON.stringify([project.content_revision, project.name, project.status, project.running_lines, project.current_items, project.counts, project.recent_progress, project.error, project.freshness, t("空闲")]);
    if (card?._revision === revision) continue;
    if (!card) {
      card = document.createElement("section");
      root.appendChild(card);
      // 整张卡都是入口(UX-063):卡片带着手形光标,原来却只有标题那一个按钮能点。
      // 落在按钮/链接/输入上的点击归它们自己,其余空白处视同点标题。
      card.addEventListener("click", (event) => {
        if (event.target?.closest?.("button, a, input, select, textarea, summary")) return;
        if (card.dataset.path) void openProjectSpace(card.dataset.path);
      });
    }
    // Only the changed project's summary is replaced; all review form DOM is independent.
    if (workspaceMenuHandle?.anchor && card.contains(workspaceMenuHandle.anchor)) closeSurface(workspaceMenuHandle);
    card._revision = revision;
    card.dataset.path = project.path;
    card.className = `workspace-card${project.current ? " current" : ""}${project.error ? " workspace-unavailable" : ""}`;
    card.replaceChildren();
    const head = document.createElement("div");
    head.className = "workspace-card-head";
    const open = document.createElement("button");
    open.type = "button";
    open.className = "workspace-card-open";
    open.textContent = project.name;
    open.setAttribute("aria-label", `${t("选择工作区项目")} ${project.name}`);
    open.addEventListener("click", () => void openProjectSpace(project.path));
    const status = document.createElement("span");
    status.className = `workspace-status ${project.running_lines ? "running" : project.status}`;
    status.textContent = project.error ? t("项目读取失败") : project.running_lines
      ? `${t("运行中")} · ${project.running_lines} ${t("个对话")}` : t("空闲");
    const more = document.createElement("button");
    more.type = "button";
    more.className = "icon-btn workspace-card-more";
    more.textContent = "⋯";
    more.title = t("更多操作");
    more.setAttribute("aria-haspopup", "menu");
    more.setAttribute("aria-expanded", "false");
    more.setAttribute("aria-label", `${t("更多操作")} ${project.name}`);
    more.addEventListener("click", () => openWorkspaceCardMenu(more, project));
    head.append(open, status, more);
    const path = document.createElement("div");
    path.className = "dim workspace-path";
    path.textContent = project.path;
    const activity = document.createElement("div");
    activity.className = "workspace-activity dim";
    activity.textContent = project.recent_progress?.label
      ? `${t("最近进展")}: ${project.recent_progress.label}` : "";
    const counts = project.counts ?? {};
    const meta = document.createElement("div");
    meta.className = "workspace-meta dim";
    meta.textContent = `${t("待你试用")} ${counts.ready_to_try ?? 0} · ${t("待复核决策")} ${counts.decisions ?? 0}`;
    if (project.freshness === "stale") meta.textContent += ` · ${t("数据可能已过期")}`;
    card.append(head, path, renderCurrentItems(project), activity, meta);
  }
  for (const [path, card] of existing) if (!keep.has(path)) card.remove();
  renderDecisionConsole(snapshot, {
    refresh: refreshWorkspace, openProject: (path) => openProjectSpace(path),
    beforeMutation: () => { workspaceRefreshEpoch += 1; },
  });
  renderProjectOverview(snapshot);
}

// Overview is a bounded, read-only projection. Full review evidence is loaded
// only when its tab is open; cached detail survives unrelated summary updates.
export async function refreshWorkspace() {
  const epoch = ++workspaceRefreshEpoch;
  workspaceRequests += 1;
  try {
    const overview = await invoke("workspace_overview");
    if (epoch !== workspaceRefreshEpoch) return;
    const consoleState = workspaceConsoleState();
    const inConsole = $("view-workspace")?.classList.contains("active");
    let detail = null;
    if (inConsole && consoleState.tab !== "projects") {
      detail = await invoke("workspace_snapshot", { projectDir: consoleState.projectFilter || null });
      if (epoch !== workspaceRefreshEpoch) return;
    }
    const previous = new Map((lastWorkspaceSnapshot?.projects ?? []).map((p) => [p.path, p]));
    const detailed = new Map((detail?.projects ?? []).map((p) => [p.path, p]));
    const snapshot = { ...overview, projects: (overview?.projects ?? []).map((p) => {
      const detailProject = detailed.get(p.path);
      const merged = { ...previous.get(p.path), ...detailProject, ...p };
      if (detailProject?.error) { merged.error = detailProject.error; merged.freshness = "stale"; }
      return merged;
    }) };
    renderWorkspace(snapshot);
  } catch (error) {
    if (epoch === workspaceRefreshEpoch) {
      const freshness = document.querySelector(".console-freshness");
      if (freshness) freshness.textContent = t("刷新失败，保留上次内容");
      toastError(`${t("工作区刷新失败")}:${error}`, { retry: refreshWorkspace });
    }
  } finally { workspaceRequests -= 1; }
}

let workspaceInvalidationTimer = null;
export function refreshWorkspaceSoon() {
  if (document.hidden || !["workspace", "project"].includes(document.body.dataset.view)) return;
  clearTimeout(workspaceInvalidationTimer);
  workspaceInvalidationTimer = setTimeout(() => { workspaceInvalidationTimer = null; void refreshWorkspace(); }, 400);
}

defer(() => {
  on("kz:workspace-invalidated", refreshWorkspaceSoon);
  document.addEventListener("kz:workspace-console-changed", () => {
    if ($("view-workspace")?.classList.contains("active")) void refreshWorkspace();
  });
  setInterval(() => {
    const visible = $("view-workspace")?.classList.contains("active") || $("view-project")?.classList.contains("active");
    if (!document.hidden && visible && !workspaceRequests && !workspaceConsoleBusy()) void refreshWorkspace();
  }, 8000);
});
// 需求页的四个页签:req 进行中 / defect 缺陷 / tests 测试 / ideas 想法。
// (「对照」页签已移除:两列无列标题、筛选置灰,真正的交叉引用是依赖视图,见 B12。)
export const DOCUMENTS_KINDS = Object.freeze(["req", "defect", "tests", "ideas"]);
export let documentsKind = "req";
export function setDocumentsKind(v) { documentsKind = DOCUMENTS_KINDS.includes(v) ? v : "req"; }
export let latestDocsSnapshot = null;
export function prepareDocsProject() {
  latestDocsSnapshot = null;
  batchSelection.clear();
  clearDocsLoadError();
  for (const id of ["view-documents"]) {
    const el = $(id);
    if (el) { el.inert = true; el.setAttribute("aria-busy", "true"); }
  }
}
// 读取失败(UX-066):切项目后 prepareDocsProject 把页面置为 inert 等新快照,失败时没人来解除,
// 页面就一直灰着、新项目名下还留着上个项目的列表。失败路径必须解除 inert、藏掉旧列表,并给一个带重试的错误条。
export function releaseDocsBusy() {
  for (const id of ["view-documents"]) {
    const el = $(id);
    if (el) { el.inert = false; el.setAttribute("aria-busy", "false"); }
  }
}
export function showDocsLoadError(message, retry) {
  releaseDocsBusy();
  const box = $("documents-error");
  if (!box) return;
  box.replaceChildren();
  const text = document.createElement("span");
  text.textContent = `${t("读取项目需求失败")}:${message}`;
  box.appendChild(text);
  if (typeof retry === "function") {
    const again = document.createElement("button");
    again.type = "button";
    again.className = "ghost mini";
    again.textContent = t("重试");
    again.addEventListener("click", () => void retry());
    box.appendChild(again);
  }
  box.classList.remove("hidden");
  // 没有新快照时,页面上的列表是上一个项目的(或已过期):藏起来,免得读成新项目的数据。
  if (!latestDocsSnapshot) $("documents-scroll")?.classList.add("documents-stale");
}
export function clearDocsLoadError() {
  const box = $("documents-error");
  if (box) { box.replaceChildren(); box.classList.add("hidden"); }
  $("documents-scroll")?.classList.remove("documents-stale");
}
// 每队按项目持久化的筛选字段与它们的默认值,全仓只此一份:documentFilters 的初值、
// saveDocFilters 的落盘字段表、restoreDocFilters 换项目时的复位,三处共用。写第二份
// 默认值迟早会漂(docstore.rs 的注释专门写过这个教训),漂了就是"存了却复位不到"。
// grouped 不在这里:它按 kz-grouped-docs 全局记、不随项目走(见 bindGroupToggle),
// 换项目时不该被复位。
export const DOC_FILTER_DEFAULTS = Object.freeze({
  req: Object.freeze({ status: "all", priority: "all", complexity: "all", tag: "all", blocked: "all", sort: "manual" }),
  defect: Object.freeze({ status: "all", priority: "all", tag: "all", blocked: "all" }),
});
export const documentFilters = {
  req: { ...DOC_FILTER_DEFAULTS.req, grouped: localStorage.getItem("kz-grouped-docs") !== "0" },
  defect: { ...DOC_FILTER_DEFAULTS.defect, grouped: localStorage.getItem("kz-grouped-docs") !== "0" },
};
// 选项的 value 是引擎的状态枚举(筛选/批量改状态按它比对、回传),**显示词**一律经 localizedDocStatus
// (04-status-words.js 那张表)——选项的第二项只是枚举原文,不是给人看的。
export const documentStatusOptions = {
  req: [["all", "全部状态"], ["todo", "todo"], ["doing", "doing"], ["done", "done"], ["dropped", "dropped"]],
  defect: [["all", "全部状态"], ["open", "open"], ["fixing", "fixing"], ["fixed", "fixed"], ["wontfix", "wontfix"]],
};
export function documentStatusOptionLabel(value, label) {
  return value === "all" ? t(label) : localizedDocStatus(value);
}
// 终态:转过去条目就归档成只读,界面里改不回来(引擎只许前进,见 docstore transition_allowed)。
// 状态流转按钮对这些目标先确认。与 DocKind.terminal 逐字对应;ideas 的 split 是拆解产物,不走这里。
export const DOC_TERMINAL_STATUSES = Object.freeze({
  req: Object.freeze(["done", "dropped"]),
  defect: Object.freeze(["fixed", "wontfix"]),
  idea: Object.freeze(["split", "dropped"]),
});
// 取活口径的合法 lifecycle:从状态筛选选项派生,不再抄第三份状态表。
// 与引擎 DocKind.statuses(crates/kanzei-memory/src/docstore.rs)逐字对应。
export const DOC_LIFECYCLE_STATUSES = Object.freeze(Object.fromEntries(
  Object.entries(documentStatusOptions).map(([kind, options]) => [
    kind,
    Object.freeze(options.map(([value]) => value).filter((value) => value !== "all")),
  ]),
));
// 筛选只作用于当前页签那一个队列。
// 注意适用范围:这是 **applyDocFilter(用户主动调控件)** 的写入目标。
// syncDocumentFilters 的回填/纠正**不得**写别的队列——那条路径上用户什么都没做,
// 只是切了个标签页,写下去就是"看一眼就改掉状态"(见 syncDocumentFilters 里标签那段)。
export function docFilterTargets() {
  // 测试记录/想法没有筛选口径:documentFilters 里没有 "tests"/"ideas" 这两档,返回空让 applyDocFilter
  // 与筛选回填统统空转,而不是拿 undefined 去读 .status 把整条渲染链炸掉。
  if (documentsKind === "tests" || documentsKind === "ideas") return [];
  return [documentsKind];
}
// 缺陷页对复杂度/排序的「不带筛选」是**显示口径**,不是「把用户的筛选清掉」。曾经有过真的把
// documentFilters.req/defect 写成 all 并落盘的实现:用户在需求页设好 status=doing + 复杂度=大,
// 切去别的页签瞄一眼,回来筛选就永久没了、重启也回不来——R-115「筛选按项目持久化」的直接回归。
// 改法:渲染用中性副本,底层状态一律不动、不落盘。
// 只覆盖状态里**确实存在**的键,不凭空造键:缺陷队列没有复杂度/排序口径,凭空写进去
// 会让锁提示列出一个 docDragEnabled 根本不看的条件(D-211 反向脱节)。
// 返回值与渲染、拖拽判定共用同一个对象——"列表完不完整"和"能不能拖"必须同源。
export function neutralizedDocFilters(state) {
  if (!state) return NEUTRAL_DOC_FILTERS;
  // 复杂度与排序是需求专有口径:非需求页按 all/manual 渲染。
  const overrides = documentsKind === "req" ? {} : { complexity: "all", sort: "manual" };
  const changed = Object.keys(overrides).filter((field) => field in state && state[field] !== overrides[field]);
  if (!changed.length) return state;
  const copy = { ...state };
  for (const field of changed) copy[field] = overrides[field];
  return copy;
}
export function syncDocumentFilters(snapshot) {
  // 页签决定筛选面板里有哪些控件(UX-061):测试/想法没有筛选口径,整个「筛选」入口不出现;
  // 缺陷没有复杂度/排序,这两格直接不显示——不再摆一排置灰的控件。
  const filterable = documentsKind === "req" || documentsKind === "defect";
  $("documents-filter-toggle")?.classList.toggle("hidden", !filterable);
  if (!filterable) {
    const menu = $("documents-filter-menu");
    if (menu && isSurfaceOpen(menu)) closeSurface(menu);
    renderActiveFilterChips();
    return;
  }
  for (const field of document.querySelectorAll("#documents-filter-menu [data-docs-kinds]")) {
    field.classList.toggle("hidden", !field.dataset.docsKinds.split(" ").includes(documentsKind));
  }
  $("documents-sort-note")?.classList.toggle("hidden", documentsKind !== "req");
  const statusFilter = $("documents-status-filter");
  const priorityFilter = $("documents-priority-filter");
  const complexityFilter = $("documents-complexity-filter");
  const sortSelect = $("documents-sort");
  const tagFilter = $("documents-tag-filter");
  const blockedFilter = $("documents-blocked-filter");
  const filters = documentFilters[documentsKind];
  const entries = documentsKind === "req" ? (snapshot.requirements ?? []) : (snapshot.defects ?? []);
  statusFilter.innerHTML = documentStatusOptions[documentsKind]
    .map(([value, label]) => `<option value="${value}">${documentStatusOptionLabel(value, label)}</option>`)
    .join("");
  statusFilter.value = filters.status;
  priorityFilter.value = filters.priority ?? "all";
  blockedFilter.value = filters.blocked ?? "all";
  if (complexityFilter) complexityFilter.value = documentsKind === "req" ? (filters.complexity ?? "all") : "all";
  if (sortSelect) sortSelect.value = documentsKind === "req" ? (filters.sort ?? "manual") : "manual";
  // 标签:保存的标签在当前这一队里根本不存在(改过名、清空了、换了项目),下拉只剩回落成「全部」
  // 一条路。这时**筛选状态必须跟着回落并落盘**,否则列表被一个界面上看不见的条件筛空
  // (D-169:看起来就是条目凭空掉了),而且内存改了、落盘没改的话,重启后那条看不见的筛选还会原样回来。
  // 纠正只作用于**该标签所属的那一队**,也就是当前页签的那一支:标签的存废只能由本队自己的条目判定。
  //     空列表不算「值失效」:这一队一条条目都没有(项目刚建好、读盘失败降级成空、
  //     或一次截断的瞬态快照)时,标签下拉必然只剩「全部」——但「列表被看不见的条件筛空」
  //     这个前提根本不成立,列表本来就是空的,没有任何理由改用户的口径,更没有理由落盘。
  //     不加这道守卫,一次瞬态空快照就能永久清掉用户设好的标签筛选:内存与落盘一起变成
  //     「全部」,数据恢复后也回不来,全程零用户动作。这只**收窄**不封死——截断读到"部分
  //     条目"时 entries.length 仍 > 0,治根(快照非原子/空文件当有效结果)属 R-138。
  const tagValue = syncTagFilter(tagFilter, entries, filters.tag ?? "all");
  if (entries.length && "tag" in filters && filters.tag !== tagValue) {
    filters.tag = tagValue;
    saveDocFilters();
  }
  renderActiveFilterChips();
}
// 生效的筛选(UI-0926 #4):筛选控件收进「筛选」浮层后,列表上方用 chip 把当前生效的每一项说破,
// × 单项复位、「清除全部」一键复位,触发器上带生效项数。筛选不在眼前时,被筛短的列表最容易被
// 当成条目丢了(D-169)。测试记录/想法页没有筛选,不出 chip。
const FILTER_CHIP_FIELDS = [
  ["status", "状态", "documents-status-filter"],
  ["priority", "优先级", "documents-priority-filter"],
  ["complexity", "复杂度", "documents-complexity-filter"],
  ["tag", "标签", "documents-tag-filter"],
  ["blocked", "执行状态", "documents-blocked-filter"],
  ["sort", "排序", "documents-sort"],
];
export function activeDocFilters() {
  if (documentsKind !== "req" && documentsKind !== "defect") return [];
  const kind = docFilterTargets()[0];
  const filters = documentFilters[kind];
  const defaults = DOC_FILTER_DEFAULTS[kind];
  if (!filters || !defaults) return [];
  return FILTER_CHIP_FIELDS
    .filter(([field]) => field in defaults && (filters[field] ?? defaults[field]) !== defaults[field])
    .map(([field, labelKey, selectId]) => {
      const value = filters[field];
      // 显示值取控件里那一项的文字(已本地化,状态/标签/执行状态各有自己的叫法),取不到才用原值。
      const option = [...($(selectId)?.options ?? [])].find((candidate) => candidate.value === value);
      return { field, labelKey, value, shown: option?.textContent?.trim() || localizeDynamic(value), reset: defaults[field] };
    });
}
export function renderActiveFilterChips() {
  const active = activeDocFilters();
  const count = $("documents-filter-count");
  if (count) count.textContent = active.length ? String(active.length) : "";
  const row = $("documents-active-filters");
  if (!row) return;
  row.replaceChildren();
  row.classList.toggle("hidden", !active.length);
  if (!active.length) return;
  for (const item of active) {
    const label = t(item.labelKey);
    const chip = document.createElement("span");
    chip.className = "documents-filter-chip";
    chip.dataset.field = item.field;
    const key = document.createElement("span");
    key.className = "documents-filter-chip-key";
    key.textContent = label;
    const text = document.createElement("span");
    text.append(key, document.createTextNode(`: ${item.shown}`));
    const clear = document.createElement("button");
    clear.type = "button";
    clear.className = "documents-filter-chip-clear";
    clear.textContent = "×";
    clear.setAttribute("aria-label", `${t("清除筛选")} ${label}`);
    clear.addEventListener("click", () => applyDocFilter(item.field, item.reset));
    chip.append(text, clear);
    row.appendChild(chip);
  }
  const clearAll = document.createElement("button");
  clearAll.type = "button";
  clearAll.className = "ghost mini documents-filter-clear-all";
  clearAll.textContent = t("清除全部");
  clearAll.addEventListener("click", clearDocFilters);
  row.appendChild(clearAll);
}
export function renderDocuments(snapshot) {
  latestDocsSnapshot = snapshot;
  if (typeof renderLineWorkItemOptions === "function") renderLineWorkItemOptions(snapshot);
  // tab 直调不经 renderDocsSnapshot,work-priority 可能刚切过——重算一次,幂等。
  agentFocus = computeAgentFocus(snapshot, activeSessionId, activeProcessItem());
  // 文档列表的「被取得」标记必须按主线焦点计算,不能因为用户当前查看并行线
  // 就把主线的 WIP 口径换掉。
  computeLineAgentFocuses(snapshot);
  const reqList = $("documents-req-list");
  const defectList = $("documents-defect-list");
  const ideaList = $("documents-idea-list");
  if (!reqList || !defectList) return;
  // 有快照说明读取成功:上一次读取失败留下的错误条与「藏旧列表」状态到此作废(UX-066)。
  clearDocsLoadError();
  syncDocumentFilters(snapshot);
  // 几处都把原始条目交给 renderDocList 自己筛:这里曾经预筛一遍缺陷再传进去,
  // 等于同一套筛选写了两份,改一处漏一处就会两边对不上(R-123 验收 ④)。
  // 传中性副本而不是底层状态:缺陷页要的是「显示上不带复杂度/排序」,不是「把用户的筛选清掉」。
  // 渲染、拖拽判定、锁提示三处拿的都是同一个副本,所以"列表完不完整"与"能不能拖"仍同源。
  renderDocList(reqList, snapshot.requirements ?? [], "req", snapshot.archived?.req ?? 0, neutralizedDocFilters(documentFilters.req), snapshot.archived_entries?.req ?? []);
  renderDocList(defectList, snapshot.defects ?? [], "defect", snapshot.archived?.defect ?? 0, neutralizedDocFilters(documentFilters.defect), snapshot.archived_entries?.defect ?? []);
  // 想法收件箱(B19):原先只在被隐藏的侧栏里,「新目标」写进去看不到也拆解不了;现在是需求页的一个页签。
  if (ideaList) renderDocList(ideaList, snapshot.ideas ?? [], "idea", snapshot.archived?.idea ?? 0, NEUTRAL_DOC_FILTERS, snapshot.archived_entries?.idea ?? []);
  const defectMetrics = $("defect-incident-metrics");
  if (defectMetrics) {
    const defectTab = documentsKind === "defect";
    defectMetrics.classList.toggle("hidden", !defectTab);
    if (defectTab) renderIncidentMetrics(snapshot.incident_metrics, "defect-incident-metrics");
  }
  const isTests = documentsKind === "tests";
  const isIdeas = documentsKind === "ideas";
  // 需求/缺陷是带筛选、批量、依赖视图的「管理列表」;测试与想法是各自一套轻视图。
  const managed = documentsKind === "req" || documentsKind === "defect";
  const depMode = dependencyViewOpen && managed;
  reqList.classList.toggle("hidden", depMode || documentsKind !== "req");
  defectList.classList.toggle("hidden", depMode || documentsKind !== "defect");
  ideaList?.classList.toggle("hidden", !isIdeas);
  $("documents-tests")?.classList.toggle("hidden", !isTests);
  // 页签是一组分段按钮:当前页签 primary 类(样式在 .documents-tabs 下按中性选中态画)+ aria-pressed。
  for (const [id, on] of [["documents-tab-req", documentsKind === "req"], ["documents-tab-defect", documentsKind === "defect"], ["documents-tab-tests", isTests], ["documents-tab-ideas", isIdeas]]) {
    const tab = $(id);
    if (!tab) continue;
    tab.className = on ? "primary" : "ghost";
    tab.setAttribute("aria-pressed", String(on));
  }
  // 「更多」菜单按页签生成(UX-061):每项带 data-docs-kinds,只显示对当前页签有意义的。
  for (const item of document.querySelectorAll("#documents-more-menu [data-docs-kinds]")) {
    item.classList.toggle("hidden", !item.dataset.docsKinds.split(" ").includes(documentsKind));
  }
  // 依赖视图是页内切换,只对需求/缺陷有意义:测试/想法页整个开关不出现,但**不清 dependencyViewOpen**
  // ——切回需求页时用户原来的选择还在。
  $("documents-dep-toggle")?.classList.toggle("hidden", !managed);
  if (!managed) $("documents-dep-view")?.classList.add("hidden");
  else renderDependencyView(snapshot);
  syncBatchBar();
  if (!managed) $("documents-batch-bar")?.classList.add("hidden");
}
// 依赖视图(R-111):按依赖拓扑分层展示需求+缺陷。可做层 = 无依赖或同一 docs_snapshot
// 未给该依赖返回引擎 block_reasons;被阻塞层 = 至少一个依赖仍有引擎阻塞原因。
// 数据来自批1 的 dependencies/dependents 字段。分层消费快照里的依赖型 block_reasons,
// 不从仅含活跃条目的列表重新构造 done 集合;归档终态依赖也因此与引擎同判(D-750)。
// 其它用户阻塞/显式停车仍保持原字段和理由,本视图只分层依赖是否满足(M-006)。
export let dependencyViewOpen = false;
export function setDependencyViewOpen(v) { dependencyViewOpen = v; }
export function renderDependencyView(snapshot) {
  const depView = $("documents-dep-view");
  const toggle = $("documents-dep-toggle");
  if (!depView || !toggle) return;
  // 页内切换按钮:按下态写 aria-pressed,样式跟 .documents-view-toggle[aria-pressed=true]。
  toggle.setAttribute("aria-pressed", String(dependencyViewOpen));
  if (!dependencyViewOpen) {
    depView.classList.add("hidden");
    return;
  }
  depView.classList.remove("hidden");
  const reqs = snapshot?.requirements ?? [];
  const defs = snapshot?.defects ?? [];
  const entries = [...reqs, ...defs];
  const byId = new Map(entries.map((e) => [e.id, e]));
  const hasDeps = (e) => Array.isArray(e.dependencies) && e.dependencies.length > 0;
  // 各依赖是否阻塞由 docs_snapshot 与调度器基于 active+archive 同算,不要从 UI 的 active-only entries 重建 done 集合。
  // 环上条目引擎只报一条「循环依赖: …」、不再逐个报「未完成依赖」(scheduling.rs block_reasons),
  // 环上永远等不到依赖完成,所以有环理由即整体判未满足,与引擎同判(D-750 环例外)。
  const depsDone = (e) => {
    const reasons = e.block_reasons ?? [];
    if (reasons.some((reason) => String(reason).startsWith("循环依赖:"))) return false;
    return (e.dependencies ?? []).every((id) =>
      !reasons.some(
        (reason) => reason === `未完成依赖: ${id}` || reason === `依赖不存在: ${id}`,
      ),
    );
  };
  const layers = { ready: [], blocked: [] };
  for (const e of entries) {
    if (!hasDeps(e) || depsDone(e)) layers.ready.push(e);
    else layers.blocked.push(e);
  }
  const renderLayer = (list, title, cls) => {
    const head = document.createElement("h3");
    head.className = `dep-layer-head ${cls}`;
    head.textContent = `${title}(${list.length})`;
    const wrap = document.createElement("div");
    wrap.className = "dep-layer";
    for (const e of list) {
      const row = document.createElement("div");
      row.className = `dep-entry${e.closed ? " closed" : ""}`;
      row.dataset.docId = e.id;
      row.setAttribute("role", "button");
      row.tabIndex = 0;
      const kind = e.id.startsWith("D-") ? "defect" : "req";
      const st = document.createElement("span");
      st.className = `st st-${e.status || "todo"}`;
      st.textContent = e.id;
      const title = document.createElement("span");
      title.className = "dep-entry-title";
      title.textContent = e.title;
      const meta = [];
      if (hasDeps(e)) meta.push(`${t("依赖")} ${(e.dependencies ?? []).length}`);
      if (Array.isArray(e.dependents) && e.dependents.length) meta.push(`${t("被依赖")} ${e.dependents.length}`);
      if (meta.length) {
        const m = document.createElement("span");
        m.className = "dim dep-meta";
        m.textContent = meta.join(" · ");
        row.append(st, title, m);
      } else {
        row.append(st, title);
      }
      row.addEventListener("click", () => highlightDependencyChain(row, e, byId));
      row.addEventListener("keydown", (event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        row.click();
      });
      wrap.appendChild(row);
    }
    depView.append(head, wrap);
  };
  depView.replaceChildren();
  renderLayer(layers.ready, t("可做(依赖已满足)"), "ready");
  renderLayer(layers.blocked, t("被阻塞(还有未完成依赖)"), "blocked");
  // 全部无依赖时给一行说明,避免空视图像没渲染。
  if (!layers.ready.length && !layers.blocked.length) {
    const empty = document.createElement("p");
    empty.className = "dim";
    empty.textContent = t("暂无依赖关系");
    depView.appendChild(empty);
  }
}
export function highlightDependencyChain(clicked, entry, byId) {
  const depView = $("documents-dep-view");
  if (!depView) return;
  const rows = [...depView.querySelectorAll(".dep-entry")];
  rows.forEach((r) => {
    r.classList.remove("dep-lit", "dep-dim");
    r.style.opacity = "";
  });
  const lit = new Set();
  const walkUp = (id, visited) => {
    if (visited.has(id)) return;
    visited.add(id);
    lit.add(id);
    const e = byId.get(id);
    if (!e) return;
    for (const dep of e.dependencies ?? []) walkUp(dep, visited);
  };
  const walkDown = (id, visited) => {
    if (visited.has(id)) return;
    visited.add(id);
    lit.add(id);
    for (const e of byId.values()) {
      if ((e.dependencies ?? []).includes(id)) walkDown(e.id, visited);
    }
  };
  walkUp(entry.id, new Set());
  walkDown(entry.id, new Set());
  for (const r of rows) {
    if (lit.has(r.dataset.docId)) r.classList.add("dep-lit");
    else {
      r.classList.add("dep-dim");
      r.style.opacity = "0.45";
    }
  }
  clicked.scrollIntoView({ block: "nearest" });
}
// 取活焦点按**线路**计算。项目文档快照仍是共享真源,但 claimed_by 将条目归到
// 具体分支线;默认线则只消费未被分支线取得的条目。运行证据也按 session_id 保存,
// 避免切线后上一条线的运行焦点留在当前侧栏。
export let agentFocus = { active: null, activeSource: null };
export const runtimeFocusBySession = new Map();
export const lineAgentFocusByProcessId = new Map();
export function activeProcessItem() {
  return typeof processItems !== "undefined"
    ? processItems.find((item) => item.id === activeProcessId) ?? null
    : null;
}
export function setRuntimeFocus(id, sessionId = activeSessionId) {
  if (sessionId && id) runtimeFocusBySession.set(sessionId, { id, stale: false });
}
// 轮开始不再删除运行证据,只降级为「上轮遗留」:一轮的前半段(勘察/写码/测试)
// 不会产生任何 tracker/提交事件,删掉就是每轮开头一段"未绑定条目"的空窗,
// 面板看起来像不刷新。上轮条目仍是最好的猜测,新证据到达时自然覆盖。
export function markRuntimeFocusStale(sessionId = activeSessionId) {
  const focus = sessionId ? runtimeFocusBySession.get(sessionId) : null;
  if (focus) focus.stale = true;
}
export function processIsPrimary(process) {
  return !process
    || process.authority === "primary"
    || process.id?.startsWith("d|")
    || (!process.authority && !process.worktree_path);
}
export function focusEntriesForProcess(snapshot, process) {
  const entries = [...(snapshot?.requirements ?? []), ...(snapshot?.defects ?? [])];
  if (!process) return entries;
  const branch = String(process.branch ?? "").trim();
  if (processIsPrimary(process)) {
    return entries.filter((entry) => !String(entry.claimed_by ?? "").trim());
  }
  return branch
    ? entries.filter((entry) => String(entry.claimed_by ?? "").trim() === branch)
    : [];
}
export function computeAgentFocus(snapshot, sessionId = activeSessionId, process = null) {
  // activeSource:焦点卡片要能说出「凭什么指这一条」——runtime = 本轮运行证据命中,
  // order = 按取活序推断,claim = 线路取得事实,null = 没有在做的条目。
  const focus = { active: null, activeSource: null };
  if (!snapshot) return focus;
  const scopedEntries = focusEntriesForProcess(snapshot, process);
  const reqs = scopedEntries.filter((entry) => (snapshot.requirements ?? []).includes(entry));
  const defs = scopedEntries.filter((entry) => (snapshot.defects ?? []).includes(entry));
  const runtimeFocus = sessionId ? runtimeFocusBySession.get(sessionId) : null;
  // 运行事实优先:证据指向的条目仍开放且属于当前线路才算数。上轮遗留的证据
  // 降级标注(runtime-stale),但仍优于取活序推断——它至少指过真实工作。
  if (runtimeFocus?.id) {
    const evidence = scopedEntries.find(
      (entry) => entry.id === runtimeFocus.id && !entry.closed
    );
    if (evidence) {
      focus.active = evidence.id;
      focus.activeSource = runtimeFocus.stale ? "runtime-stale" : "runtime";
    }
  }
  const queues =
    selectedWorkPriority() === "requirement-first"
      ? [[reqs, "doing"], [defs, "fixing"]]
      : [[defs, "fixing"], [reqs, "doing"]];
  // 线路已有 claimed_by 时,只在该线路的条目里按取活序选一条;默认线只看未取得项。
  // 正在做 = 取活序里第一个可执行的 doing/fixing(单条)。blocked 不计:§1.1 阻塞项
  // 不进 WIP、不占运行焦点——agent 会跳过它继续取下一个可开工条目,渲染必须与
  // 取活一致(否则 R-157 类阻塞 doing 会被标成「agent 正在做」,而实际它推不动)。
  if (!focus.active) {
    for (const [list, status] of queues) {
      const hit = list.find((entry) => entry.status === status && !entry?.blocked);
      if (hit) {
        focus.active = hit.id;
        focus.activeSource = process && !processIsPrimary(process) ? "claim" : "order";
        break;
      }
    }
  }
  return focus;
}

export function computeLineAgentFocuses(snapshot) {
  const lines = typeof processItems !== "undefined" && processItems.length ? processItems : [null];
  lineAgentFocusByProcessId.clear();
  return lines.map((line) => {
    const focus = computeAgentFocus(snapshot, line?.session_id, line);
    if (line?.id) lineAgentFocusByProcessId.set(line.id, focus);
    return { line, focus };
  });
}
export function focusForProcess(processId) {
  return lineAgentFocusByProcessId.get(processId) ?? null;
}

// 待办计数(项目概览与对话页需求栏用):三个数字必须构成一棵加法树——总数 = Σ(可执行 + 阻塞)。
// 口径**逐条复刻引擎** backlog_status(crates/kanzei-tools/src/tracker/scheduling.rs),
// 前端一个字都不另造:closed 是后端按 DocKind.terminal 判的,blocked 是
// schedule_for_display 的 !block_reasons.is_empty(),与 kz work next 同源。
export function backlogTally(entries, kind) {
  const legal = DOC_LIFECYCLE_STATUSES[kind] ?? [];
  const tally = { active: 0, workable: 0, blocked: 0, invalid: 0 };
  for (const entry of entries) {
    if (entry?.closed) continue;
    const status = String(entry?.status ?? "");
    // D-332:非法 lifecycle 被引擎隔离为 integrity 错误,不算活动条目也不参与取活。
    // 这里单独计数而不是静默丢弃——丢了会让总数对不上列表长度,又是一次「数字对不上」。
    if (status && !legal.includes(status)) { tally.invalid += 1; continue; }
    tally.active += 1;
    if (entryBlocked(entry)) tally.blocked += 1;
    else tally.workable += 1;
  }
  return tally;
}
// 线路身份的叫法与判据全仓只此一处(研究空间的会话列表 09-sessions.js 用它给行首标身份)。
export function lineAuthorityLabel(process) {
  if (process?.profile === "research") return t("研究对话");
  return processIsPrimary(process) ? t("主对话") : t("独立任务");
}
/// 只重绘文档列表与计数(不含历史/测试/工作树):供运行中高频刷新使用。
export function renderDocsSnapshot(snapshot) {
  releaseDocsBusy();
  renderDocuments(snapshot);
  $("req-count").textContent = `${snapshot.requirements.filter((r) => !r.closed).length}`;
  $("defect-count").textContent = `${snapshot.defects.filter((d) => !d.closed).length}`;
  $("idea-count").textContent = `${(snapshot.ideas ?? []).filter((g) => g.status === "inbox").length}`;
  syncConventionsEntry(snapshot.conventions);
  applyLanguage();
  // 重绘换掉了节点:跨视图跳转挂起的高亮在这里落地,它等的就是这次刷新。
  consumePendingJump();
}
