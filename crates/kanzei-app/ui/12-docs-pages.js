import { closeSurface, openMenu } from "./00-surface.js";
import { defer } from "./01-core.js";
import { motionSync } from "./01-core.js";
import { localizeDynamic } from "./02-i18n.js";
import { $, invoke } from "./01-core.js";
import { applyLanguage, localizedDocStatus, resolveUiLanguage, t } from "./02-i18n.js";
import {
  activeProcessId,
  activeSessionId,
  currentProject,
  log,
  processItems,
  running,
  toastError,
} from "./03-shell.js";
import { splitTimeline } from "./04-structured-parse.js";
import { normalizeTrackerFields } from "./04-structured.js";
import { selectedWorkPriority } from "./08-auto.js";
import { state } from "./08-compose.js";
import { enterProject } from "./09-sessions.js";
import { syncLineFocusLive } from "./09-sessions.js";
import {
  NEUTRAL_DOC_FILTERS,
  entryBlocked,
  openDocumentsView,
  saveDocFilters,
  syncTagFilter,
} from "./10-docs-core.js";
import { consumePendingJump, jumpToEntry, renderDocList, syncBatchBar } from "./11-docs-list.js";
import { renderIncidentMetrics } from "./13-memory.js";
import { applyDocFilter, clearDocFilters, refreshDocs } from "./14-docs-actions.js";
import { renderConventions } from "./15-views-misc.js";
import { collaborationLines, renderLineWorkItemOptions } from "./20-lines.js";

export function formatWorkspaceTime(value) {
  if (!value) return t("暂无时间");
  return new Date(Number(value)).toLocaleString();
}

export async function selectWorkspaceProject(path) {
  try {
    // D-355:Workspace 卡片与文档页下拉复用同一切换事务(enterProject)——
    // 目标 process_list → active session → conversation_get 原子链一致。
    await enterProject(await invoke("projects_select", { path }));
    refreshWorkspace();
  } catch (error) {
    toastError(`${t("切换项目失败")}:${error}`);
  }
}

export let lastWorkspaceSnapshot = null;
export function renderWorkspace(snapshot) {
  lastWorkspaceSnapshot = snapshot;
  const root = $("workspace-projects");
  root.replaceChildren();
  for (const project of snapshot.projects ?? []) {
    const card = document.createElement("section");
    card.className = `workspace-card${project.current ? " current" : ""}`;
    card.setAttribute("role", "button");
    card.tabIndex = 0;
    card.setAttribute("aria-label", `${t("选择工作区项目")} ${project.name}`);
    if (project.current) card.setAttribute("aria-current", "page");
    const head = document.createElement("div");
    head.className = "workspace-card-head";
    const title = document.createElement("strong");
    title.textContent = project.name;
    const status = document.createElement("span");
    status.className = `workspace-status ${project.status}`;
    // 项目级状态用**线级事实**兜一层:会话 status 可能停在旧值,而「有几条线真在跑」
    // 是当下的。两者不一致时以线为准——用户问的是「现在」。
    const runningLines = project.running_lines ?? 0;
    status.textContent = runningLines
      ? `${t("运行中")} · ${runningLines} ${t("条线")}`
      : project.status === "running" ? t("运行中") : project.status === "failed" ? t("失败") : t("空闲");
    if (runningLines) status.classList.add("running");
    head.append(title, status);
    const path = document.createElement("div");
    path.className = "dim workspace-path";
    path.textContent = project.path;
    const conversation = project.conversation;
    const summary = document.createElement("div");
    summary.className = "workspace-summary";
    summary.textContent = conversation
      ? `${t("当前对话")}: ${conversation.title} · ${conversation.message_count} ${t("条")}`
      : `${t("当前对话")}: ${t("暂无")}`;
    const activity = document.createElement("div");
    activity.className = "workspace-activity dim";
    const trace = (project.recent_activity ?? []).flatMap((item) => item.events ?? []);
    activity.textContent = trace.length
      ? `${t("最近活动")}: ${trace.slice(0, 3).map((item) => item.text || item.name || t("运行事件")).join(" · ")}`
      : `${t("最近活动")}: ${t("暂无")}`;
    const queue = document.createElement("div");
    queue.className = "workspace-meta dim";
    queue.textContent = `${t("排队")} ${project.pending_count ?? 0} ${t("条")} · ${t("更新于")} ${formatWorkspaceTime(project.updated_at)}`;
    card.append(head, path, summary, activity, queue);
    // 线级现场:工作区的存在理由就是回答「另外那个项目现在怎么样」。项目名 + 当前对话
    // 侧栏里全有,只有「哪条线在跑、跑到哪个阶段」是这里独有的。
    const lines = project.lines ?? [];
    if (lines.length) {
      const box = document.createElement("div");
      box.className = "workspace-lines";
      for (const line of lines) {
        const row = document.createElement("div");
        row.className = `workspace-line${line.running ? " running" : ""}`;
        const dot = document.createElement("span");
        dot.className = "workspace-line-dot";
        dot.setAttribute("aria-hidden", "true");
        dot.textContent = line.running ? "●" : "○";
        if (line.running) motionSync(dot);
        const name = document.createElement("span");
        name.className = "workspace-line-name";
        name.textContent = line.label || line.id;
        const stage = document.createElement("span");
        stage.className = "workspace-line-stage dim";
        // 运行态才说阶段;空闲线报阶段只会让人误以为它在动。
        stage.textContent = line.running ? (line.stage || t("运行中")) : t("空闲");
        row.append(dot, name, stage);
        if (line.branch) {
          const branch = document.createElement("span");
          branch.className = "workspace-line-branch dim";
          branch.textContent = line.branch;
          row.appendChild(branch);
        }
        box.appendChild(row);
      }
      card.appendChild(box);
    }
    card.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      card.click();
    });
    card.addEventListener("click", () => selectWorkspaceProject(project.path));
    root.appendChild(card);
  }
}

export async function refreshWorkspace() {
  try {
    renderWorkspace(await invoke("workspace_snapshot"));
  } catch (error) {
    toastError(`${t("工作区刷新失败")}:${error}`, { retry: refreshWorkspace });
  }
}
export let documentsKind = "req";
export function setDocumentsKind(v) { documentsKind = v; }
export let latestDocsSnapshot = null;
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
export const documentStatusOptions = {
  req: [["all", "全部状态"], ["todo", "todo"], ["doing", "doing"], ["done", "done"], ["dropped", "dropped"]],
  defect: [["all", "全部状态"], ["open", "open"], ["fixing", "fixing"], ["fixed", "fixed"], ["wontfix", "wontfix"]],
};
// 取活口径的合法 lifecycle:从状态筛选选项派生,不再抄第三份状态表。
// 与引擎 DocKind.statuses(crates/kanzei-memory/src/docstore.rs)逐字对应。
export const DOC_LIFECYCLE_STATUSES = Object.freeze(Object.fromEntries(
  Object.entries(documentStatusOptions).map(([kind, options]) => [
    kind,
    Object.freeze(options.map(([value]) => value).filter((value) => value !== "all")),
  ]),
));
// 「对照」模式下筛选同时作用于两个队列——并排看的前提就是同一套条件,
// 各筛各的等于没在对照。单类型模式只作用于当前那一个。
// 注意适用范围:这是 **applyDocFilter(用户主动调控件)** 的写入目标。
// syncDocumentFilters 的回填/纠正**不得**照着它跨队列写——那条路径上用户什么都没做,
// 只是切了个标签页,写下去就是"看一眼就改掉状态"(见 syncDocumentFilters 里标签那段)。
export function docFilterTargets() {
  // 测试记录没有筛选口径:documentFilters 里没有 "tests" 这一档,返回空让 applyDocFilter
  // 与筛选回填统统空转,而不是拿 undefined 去读 .status 把整条渲染链炸掉。
  if (documentsKind === "tests") return [];
  return documentsKind === "both" ? ["req", "defect"] : [documentsKind];
}
// 对照页(以及缺陷页对复杂度/排序)的「不带筛选」是**显示口径**,不是「把用户的筛选
// 清掉」。这里曾经真的把 documentFilters.req/defect 写成 all 并落盘:用户在需求页设好
// status=doing + 复杂度=大,只是切去对照页瞄一眼,回来筛选就永久没了、重启也回不来
// ——R-115「筛选按项目持久化」在这条路径上的直接回归。
// 改法:渲染用中性副本,底层状态一律不动、不落盘。控件那边照旧显示 all/manual 并置灰,
// 因为渲染确实按中性走,承诺与实际一致(D-211)。
// 只覆盖状态里**确实存在**的键,不凭空造键:缺陷队列没有复杂度/排序口径,凭空写进去
// 会让锁提示列出一个 docDragEnabled 根本不看的条件(D-211 反向脱节)。
// 返回值与渲染、拖拽判定共用同一个对象——"列表完不完整"和"能不能拖"必须同源。
export function neutralizedDocFilters(state) {
  if (!state) return NEUTRAL_DOC_FILTERS;
  const overrides = {};
  // 对照页两队状态机不同,状态筛选只提供「全部」,渲染必须跟着中性。
  // 标签同理:两队各有各的标签口径,并排看时按谁的都不对(按需求那支渲染,缺陷队列就被
  // 一个用户从没在缺陷页设过的条件筛掉一批)。所以对照页的标签也走中性副本,与
  // status/complexity/sort 同一套机制——**只改显示,不动任何一队的底层状态**。
  if (documentsKind === "both") {
    overrides.status = "all";
    overrides.tag = "all";
    // D-244:对照页是只读对照视图——优先级/阻塞筛选同样是「按谁的都不对」:
    // 需求 P0~P3 与缺陷的优先级口径不同(缺陷的 priority 是可选的 P0~P3,
    // 需求则是必填),阻塞更只有缺陷队列才有持久化意义。并排看时按任何一队
    // 的 priority/blocked 筛,另一队就会被一个用户从没在那队设过的条件筛掉,
    // 与 status/tag 当初治好的是同一个病。走中性副本,只改显示、不动底层。
    overrides.priority = "all";
    overrides.blocked = "all";
  }
  // 复杂度与排序是需求专有口径:非需求页控件置灰并显示 all/manual。
  if (documentsKind !== "req") {
    overrides.complexity = "all";
    overrides.sort = "manual";
  }
  const changed = Object.keys(overrides).filter((field) => field in state && state[field] !== overrides[field]);
  if (!changed.length) return state;
  const copy = { ...state };
  for (const field of changed) copy[field] = overrides[field];
  return copy;
}
export function syncDocumentFilters(snapshot) {
  const statusFilter = $("documents-status-filter");
  const priorityFilter = $("documents-priority-filter");
  const complexityFilter = $("documents-complexity-filter");
  const sortSelect = $("documents-sort");
  const tagFilter = $("documents-tag-filter");
  const blockedFilter = $("documents-blocked-filter");
  // 禁用要说破(D-210/D-211 一路的教训):控件真的置灰,不做静默无效。
  const isTests = documentsKind === "tests";
  // D-244:对照页是只读对照视图——priority/blocked 与 status/tag 同机制,
  // 置灰并显示中性 all(渲染确实按中性走,承诺与实际一致),底层状态不动。
  const priorityBlockedNeutral = documentsKind === "both";
  for (const el of [tagFilter]) if (el) el.disabled = isTests;
  if (priorityFilter) priorityFilter.disabled = isTests || priorityBlockedNeutral;
  if (blockedFilter) blockedFilter.disabled = isTests || priorityBlockedNeutral;
  if (isTests) {
    for (const el of [statusFilter, complexityFilter, sortSelect]) if (el) el.disabled = true;
    renderActiveFilterChips();
    return;
  }
  const primary = docFilterTargets()[0];
  const filters = documentFilters[primary];
  const entries =
    documentsKind === "both"
      ? [...(snapshot.requirements ?? []), ...(snapshot.defects ?? [])]
      : documentsKind === "req"
        ? (snapshot.requirements ?? [])
        : (snapshot.defects ?? []);
  // 对照模式下两个队列的状态机不同,状态筛选只提供"全部",避免给出对另一边无意义的值。
  const statusOptions =
    documentsKind === "both" ? [["all", "全部状态"]] : documentStatusOptions[documentsKind];
  statusFilter.innerHTML = statusOptions
    .map(([value, label]) => `<option value="${value}">${localizeDynamic(label)}</option>`)
    .join("");
  statusFilter.disabled = documentsKind === "both";
  // 对照页只给「全部状态」一个选项,列表也确实按中性口径渲染(neutralizedDocFilters),
  // 所以这里显示 all 与实际一致。**但不动底层状态**:清掉它等于用户去对照页看一眼就
  // 丢了自己的筛选(R-115 回归)。切回单队列页时下面这行会把原值原样填回来。
  statusFilter.value = documentsKind === "both" ? "all" : filters.status;
  // D-244:对照页只读——priority/blocked 显示中性 all(与实际渲染一致),
  // 切回单队列页时下面这行会把原值原样填回来(底层状态从未被改)。
  priorityFilter.value = priorityBlockedNeutral ? "all" : (filters.priority ?? "all");
  blockedFilter.value = priorityBlockedNeutral ? "all" : (filters.blocked ?? "all");
  // 复杂度与排序是需求专有口径(缺陷队列既没有复杂度筛选也不参与排序):
  // 对照/缺陷标签页下置灰并显示中性值,免得摆着一个调了不生效的控件。同样只改显示。
  const reqOnly = documentsKind === "req";
  if (complexityFilter) {
    complexityFilter.disabled = !reqOnly;
    complexityFilter.value = reqOnly ? (filters.complexity ?? "all") : "all";
  }
  if (sortSelect) {
    sortSelect.disabled = !reqOnly;
    sortSelect.value = reqOnly ? (filters.sort ?? "manual") : "manual";
  }
  // 标签有两种"显示成全部",必须区分清楚——混为一谈就会互相冒充:
  //
  // (a)【临时不显示】对照页把标签一并中性化(见 neutralizedDocFilters):渲染确实不带
  //     标签筛选,所以控件置灰并显示「全部标签」,承诺与实际一致(D-211)。这是显示口径,
  //     **两队的底层状态一个字节都不许动、更不落盘**。此前这里跨队列写回,实测两种坏法:
  //     缺陷页设「后端」→ 点对照 → 缺陷队列的标签被清成「全部」并落盘,切回去筛选没了;
  //     需求页设「核心」→ 点对照 → 「核心」被写进缺陷队列并落盘,用户从没在缺陷页设过,
  //     缺陷列表却永久少了一批。去对照页瞄一眼就改掉用户状态,正是 R-115 持久化在这条
  //     路径上的直接回归,与 status/complexity/sort 当初治好的是同一个病。
  //
  // (b)【值失效】保存的标签在当前这一队里根本不存在(改过名、清空了、换了项目),下拉只
  //     剩回落成「全部」一条路。这时**筛选状态必须跟着回落并落盘**,否则列表被一个界面上
  //     看不见的条件筛空(D-169:看起来就是条目凭空掉了),而且内存改了、落盘没改的话,
  //     重启后那条看不见的筛选还会原样回来。
  //     纠正只作用于**该标签所属的那一队**,也就是当前这个单队列页的那一支:标签的存废只能
  //     由本队自己的条目判定,绝不跨队列写。对照页的 entries 是两队合并的,拿它去判缺陷队列
  //     标签的存废本身就不成立——所以对照页一律不纠正,交给用户切回该队时再说。
  const tagNeutral = documentsKind === "both";
  if (tagFilter) tagFilter.disabled = tagNeutral;
  //     空列表不算「值失效」:这一队一条条目都没有(项目刚建好、读盘失败降级成空、
  //     或一次截断的瞬态快照)时,标签下拉必然只剩「全部」——但「列表被看不见的条件筛空」
  //     这个前提根本不成立,列表本来就是空的,没有任何理由改用户的口径,更没有理由落盘。
  //     不加这道守卫,一次瞬态空快照就能永久清掉用户设好的标签筛选:内存与落盘一起变成
  //     「全部」,数据恢复后也回不来,全程零用户动作。这只**收窄**不封死——截断读到"部分
  //     条目"时 entries.length 仍 > 0,治根(快照非原子/空文件当有效结果)属 R-138。
  const tagValue = syncTagFilter(tagFilter, entries, tagNeutral ? "all" : filters.tag ?? "all");
  if (!tagNeutral && entries.length && "tag" in filters && filters.tag !== tagValue) {
    filters.tag = tagValue;
    saveDocFilters();
  }
  renderActiveFilterChips();
}
// 生效的筛选(UI-0926 #4):筛选控件收进「筛选」浮层后,列表上方用 chip 把当前生效的每一项说破,
// × 单项复位、「清除全部」一键复位,触发器上带生效项数。筛选不在眼前时,被筛短的列表最容易被
// 当成条目丢了(D-169)。对照页按中性口径显示、测试记录页没有筛选,两处都不出 chip。
const FILTER_CHIP_FIELDS = [
  ["status", "状态", "documents-status-filter"],
  ["priority", "优先级", "documents-priority-filter"],
  ["complexity", "复杂度", "documents-complexity-filter"],
  ["tag", "标签", "documents-tag-filter"],
  ["blocked", "执行状态", "documents-blocked-filter"],
  ["sort", "排序", "documents-sort"],
];
export function activeDocFilters() {
  if (documentsKind === "tests" || documentsKind === "both") return [];
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
  if (!reqList || !defectList) return;
  syncDocumentFilters(snapshot);
  // 两处都把原始条目交给 renderDocList 自己筛:这里曾经预筛一遍缺陷再传进去,
  // 等于同一套筛选写了两份,改一处漏一处就会两边对不上(R-123 验收 ④)。
  // 传中性副本而不是底层状态:对照页要的是「显示上不带筛选」,不是「把用户的筛选清掉」。
  // 渲染、拖拽判定、锁提示三处拿的都是同一个副本,所以"列表完不完整"与"能不能拖"仍同源。
  renderDocList(reqList, snapshot.requirements ?? [], "req", snapshot.archived?.req ?? 0, neutralizedDocFilters(documentFilters.req), snapshot.archived_entries?.req ?? []);
  renderDocList(defectList, snapshot.defects ?? [], "defect", snapshot.archived?.defect ?? 0, neutralizedDocFilters(documentFilters.defect), snapshot.archived_entries?.defect ?? []);
  const defectMetrics = $("defect-incident-metrics");
  if (defectMetrics) {
    const defectTab = documentsKind === "defect";
    defectMetrics.classList.toggle("hidden", !defectTab);
    if (defectTab) renderIncidentMetrics(snapshot.incident_metrics, "defect-incident-metrics");
  }
  // 「对照」把两个队列并排摆出来:需求与缺陷互相引用,分成两个标签页时对不起来。
  const isTests = documentsKind === "tests";
  const both = documentsKind === "both";
  const depMode = dependencyViewOpen && !isTests;
  reqList.classList.toggle("hidden", isTests || depMode || (!both && documentsKind !== "req"));
  defectList.classList.toggle("hidden", isTests || depMode || (!both && documentsKind !== "defect"));
  $("documents-tests")?.classList.toggle("hidden", !isTests);
  $("documents-scroll")?.classList.toggle("compare", both);
  // 页签是一组分段按钮:当前页签 primary 类(样式在 .documents-tabs 下按中性选中态画)+ aria-pressed。
  for (const [id, on] of [["documents-tab-req", documentsKind === "req"], ["documents-tab-defect", documentsKind === "defect"], ["documents-tab-tests", isTests], ["documents-tab-both", both]]) {
    const tab = $(id);
    if (!tab) continue;
    tab.className = on ? "primary" : "ghost";
    tab.setAttribute("aria-pressed", String(on));
  }
  // 依赖视图对测试记录没有意义:禁用按钮(说破)并强制隐藏面板,但**不清 dependencyViewOpen**
  // ——切回需求页时用户原来的选择还在。
  const depToggle = $("documents-dep-toggle");
  if (depToggle) {
    depToggle.disabled = isTests;
    depToggle.setAttribute("aria-disabled", String(isTests));
  }
  if (isTests) $("documents-dep-view")?.classList.add("hidden");
  else renderDependencyView(snapshot);
  syncBatchBar();
  if (isTests) $("documents-batch-bar")?.classList.add("hidden");
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
  // 开关收进「更多」菜单后是一个勾选型菜单项:状态写 aria-checked(菜单项的选中样式由弹层层画)。
  toggle.setAttribute("aria-checked", String(dependencyViewOpen));
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
// 不会产生任何 tracker/提交事件,删掉就是每轮开头一段"未取得条目"的空窗,
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

// ---------- 侧栏焦点卡片(按线路显示各自当前条目) ----------
// 数据全部来自 docs_snapshot + 当前 process_list,不把一条线路的运行事实投影到另一条。
export function focusEntryOf(snapshot, id) {
  if (!id) return null;
  const req = (snapshot?.requirements ?? []).find((entry) => entry.id === id);
  if (req) return { entry: req, kind: "req" };
  const defect = (snapshot?.defects ?? []).find((entry) => entry.id === id);
  if (defect) return { entry: defect, kind: "defect" };
  return null;
}
export function focusMetaChip(text, title) {
  const chip = document.createElement("span");
  chip.className = "focus-chip";
  chip.textContent = text;
  if (title) chip.title = title;
  return chip;
}
// 焦点依据(D-207 三修的对外可见面):凭运行证据还是凭取活序,必须说得出来。卡片上用
// 左边框线型区分(实线 = 实证,虚线 = 推断,见 .focus-card.src-*),文字进 tooltip 第二行。
export const FOCUS_SOURCES = new Set(["runtime", "runtime-stale", "order", "claim"]);
export function focusSourceLabel(focusSource) {
  return focusSource === "runtime" ? t("本轮运行证据")
    : focusSource === "runtime-stale" ? t("上轮运行证据")
      : focusSource === "claim" ? t("取得线")
        : t("取活顺序推断");
}
/// 焦点卡 tooltip:卡面只留编号/状态/标题/批次/优先级,其余按行收进这里——依据、复杂度、
/// 依赖、最新一段进展(R-282:|| 切段只取首段,限 160 字)、阻塞原因,末行提示点击直达详情。
/// 字段按 04-structured 的口径解析(与单页详情同源),不在这里另认一套字段形状。
export function focusCardTooltip(entry, focusSource) {
  const lines = [`${entry.id} ${entry.title}`, `${t("依据")}: ${focusSourceLabel(focusSource)}`];
  const cx = (entry.complexity || "").trim();
  lines.push(`${t("复杂度")}: ${["小", "中", "大"].includes(cx) ? t(cx) : t("未评估")}`);
  const deps = Array.isArray(entry.dependencies) ? entry.dependencies.length : 0;
  const dependents = Array.isArray(entry.dependents) ? entry.dependents.length : 0;
  if (deps || dependents) {
    lines.push([deps ? `${t("依赖")} ${deps}` : "", dependents ? `${t("被依赖")} ${dependents}` : ""].filter(Boolean).join(" · "));
  }
  const progress = normalizeTrackerFields(entry.fields ?? []).find((field) => field.key === "进展")?.value ?? "";
  const latest = splitTimeline(progress)[0];
  if (latest) {
    const chars = [...`${latest.date ? `${latest.date} ` : ""}${latest.text}`];
    lines.push(`${t("进展")}: ${chars.length > 160 ? `${chars.slice(0, 160).join("")}…` : chars.join("")}`);
  }
  if (entryBlocked(entry)) {
    const reasons = Array.isArray(entry.block_reasons) ? entry.block_reasons : [];
    lines.push(`${t("阻塞原因")}: ${reasons.length ? reasons.join("；") : t("缺少阻塞原因")}`);
  }
  lines.push(t("点击查看详情"));
  return lines.join("\n");
}
// 「⋯」状态流转菜单(00-surface openMenu,唯一写法)。句柄留着:焦点区真的重建时先收起它。
export let focusMenuHandle = null;
export function closeFocusMenu() {
  const handle = focusMenuHandle;
  focusMenuHandle = null;
  if (handle && !handle.closed) closeSurface(handle);
}
export async function updateFocusStatus(entry, kind, next) {
  try {
    log(await invoke("docs_update", {
      projectDir: currentProject,
      kind,
      action: "update",
      id: entry.id,
      status: next,
    }));
    refreshDocs();
  } catch (err) {
    toastError(String(err));
    log(`${t("状态流转失败")}:${err}`, "warn");
  }
}
/// 侧栏「各线当前在做」卡片。整卡就是一个点击目标(标题按钮 .focus-open 用 ::after 撑满
/// 卡片),点了直达单页里**已展开**的详情;字段原文、类型/复杂度/依赖 chip、底部状态按钮与
/// 「在完整列表中查看」都不再常驻——信息进 tooltip,状态流转进「⋯」菜单(取活时切状态
/// 多点一次,这是 UI-0926 #4 按「简洁」诉求做的取舍)。
export function buildFocusCard(entry, kind, focusSource = agentFocus.activeSource) {
  const card = document.createElement("div");
  const pri = (entry.priority || "").toUpperCase();
  const hasPri = /^P[0-3]$/.test(pri);
  const blocked = entryBlocked(entry);
  const source = FOCUS_SOURCES.has(focusSource) ? focusSource : "order";
  card.className = `focus-card src-${source}${blocked ? " blocked" : ""}${hasPri ? ` pri-${pri}` : ""}`;
  card.dataset.docId = entry.id;

  // 头部:编号、状态、阻塞、优先级、⋯。状态一律用文字表达,颜色只做冗余强化(D-105)。
  const head = document.createElement("div");
  head.className = "focus-head";
  const idEl = document.createElement("span");
  idEl.className = "focus-id";
  idEl.setAttribute("data-i18n-raw", "");
  idEl.textContent = entry.id;
  const status = document.createElement("span");
  status.className = `st st-${entry.status || "todo"}`;
  status.textContent = localizedDocStatus(entry.status || "todo") + (entry.severity ? `/${entry.severity}` : "");
  head.append(idEl, status);
  if (blocked) {
    const badge = document.createElement("span");
    badge.className = "blocked-badge";
    badge.textContent = t("阻塞");
    head.appendChild(badge);
  }
  const spacer = document.createElement("span");
  spacer.className = "focus-head-spacer";
  // 优先级在这里只读(静态 span):循环调整留给单页列表行,侧栏误点一下就改掉优先级不划算。
  const priBadge = document.createElement("span");
  priBadge.className = `pri-badge ${hasPri ? pri : "unset"} static`;
  priBadge.textContent = hasPri ? pri : t("未设");
  priBadge.title = t("优先级仅参考,不影响取活顺序");
  head.append(spacer, priBadge);
  const nextStatuses = entry.nextStatuses ?? [];
  if (nextStatuses.length) {
    const more = document.createElement("button");
    more.type = "button";
    more.className = "icon-btn focus-more";
    more.textContent = "⋯";
    more.setAttribute("aria-label", `${t("更多操作")} ${entry.id}`);
    more.setAttribute("aria-haspopup", "menu");
    more.setAttribute("aria-expanded", "false");
    more.title = t("更多操作");
    more.addEventListener("click", (event) => {
      event?.stopPropagation?.();
      const handle = openMenu(more, nextStatuses.map((next) => ({
        label: `${t("转")} ${localizedDocStatus(next)}`,
        onSelect: () => void updateFocusStatus(entry, kind, next),
      })), {
        placement: "bottom-end",
        label: `${entry.id} ${t("更多操作")}`,
        onClose: () => {
          if (focusMenuHandle === handle) focusMenuHandle = null;
        },
      });
      // 同一锚点再点一次 = 收起(openMenu 的切换语义),收起后返回的是已关闭的句柄。
      focusMenuHandle = handle && !handle.closed ? handle : null;
      if (focusMenuHandle) more.setAttribute("aria-controls", focusMenuHandle.el.id);
    });
    head.appendChild(more);
  }
  card.appendChild(head);

  // 标题 = 整卡的点击目标(两行截断,完整标题在 aria-label 与 tooltip 首行)。
  const open = document.createElement("button");
  open.type = "button";
  open.className = "focus-open";
  open.setAttribute("data-i18n-raw", "");
  open.textContent = entry.title;
  open.setAttribute("aria-label", `${entry.id} ${entry.title} · ${t("打开详情")}`);
  open.title = focusCardTooltip(entry, source);
  open.addEventListener("click", () => void jumpToEntry(entry.id, { expand: true }));
  card.appendChild(open);

  // 批次进度:只在多批次时占一行。图形给概览,「批次 3/11」文字给准数(D-105 同理)。
  const total = entry.batches?.total ?? 1;
  const done = Math.min(entry.batches?.done ?? 0, total);
  if (total > 1) {
    const meterRow = document.createElement("div");
    meterRow.className = "focus-meta";
    const cells = Math.min(total, 12);
    const filled = total <= cells ? done : Math.round((done / total) * cells);
    // #7:正在推的那一格(线真在跑时扫光);全部完成或格子已满时没有。
    const current = done < total && filled < cells ? filled + 1 : 0;
    const meter = document.createElement("span");
    meter.className = "complexity-meter batch-meter";
    meter.style.setProperty("--cells", String(cells));
    meter.setAttribute("role", "img");
    const label = `${t("批次")} ${done}/${total}`;
    // 不挂 title:.focus-open::after 覆盖整卡,子元素的 tooltip 永远悬停不到(卡面已有批次文字)。
    meter.setAttribute("aria-label", label);
    for (let i = 1; i <= cells; i += 1) {
      const cell = document.createElement("span");
      cell.className = `complexity-cell${i <= filled ? " filled" : i === current ? " current" : ""}`;
      cell.setAttribute("aria-hidden", "true");
      meter.appendChild(cell);
    }
    meterRow.append(meter, focusMetaChip(`${t("批次")} ${done}/${total}`));
    card.appendChild(meterRow);
  }

  // 阻塞是「推不动」的唯一合法解释:卡面给一行首条原因,全部原因在整卡 tooltip(focusCardTooltip)
  // 里——这一行被 .focus-open::after 覆盖,自己挂 title 悬停不到。
  if (blocked) {
    const reasons = Array.isArray(entry.block_reasons) ? entry.block_reasons : [];
    const reason = document.createElement("div");
    reason.className = "focus-block-reason";
    reason.setAttribute("data-i18n-raw", "");
    reason.textContent = `${t("阻塞")}: ${reasons[0] ?? t("缺少阻塞原因")}`;
    card.appendChild(reason);
  }
  return card;
}
// 侧栏待办计数:三个数字必须构成一棵加法树——总数 = Σ(可执行 + 阻塞)。
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
// 一个「标签 数字」对。标签挂 data-i18n-key 交给 applyDataI18nKeys 就地翻译,
// 不依赖 refreshDocs 重渲——切语言时侧栏未必在文档视图里,拿不到那次刷新。
// 值为 0 时挂 is-zero:阻塞/非法只有非零才着色(ui_color_semantics.md),零一律灰。
export function backlogStat(labelKey, value, cls) {
  const stat = document.createElement("span");
  stat.className = `backlog-stat ${cls}${Number(value) === 0 ? " is-zero" : ""}`;
  const label = document.createElement("span");
  label.className = "backlog-label";
  label.dataset.i18nKey = labelKey;
  label.textContent = t(labelKey);
  const num = document.createElement("b");
  num.className = "backlog-num";
  num.textContent = String(value);
  stat.append(label, num);
  return stat;
}
export function backlogRow(kindKey, kind, tally) {
  const row = document.createElement("div");
  row.className = "backlog-row";
  row.dataset.kind = kind;
  const name = document.createElement("span");
  name.className = "backlog-kind";
  name.dataset.i18nKey = kindKey;
  name.textContent = t(kindKey);
  row.append(
    name,
    backlogStat("可执行", tally.workable, "workable"),
    backlogStat("阻塞", tally.blocked, "blocked"),
  );
  return row;
}
// 线路身份的叫法与判据全仓只此一处:侧栏任务卡(09-sessions.js)与焦点区线路头共用,
// 同一条线路在两个区的叫法不再各说各的(原来一边「并行线」、一边「并行线路」,且主代理的判据也不同)。
export function lineAuthorityLabel(process) {
  if (process?.profile === "research") return t("研究对话");
  return processIsPrimary(process) ? t("主代理") : t("并行线");
}
// 焦点区签名:renderProcesses 随 process_list 每 3 秒轮询一次都会走到这里。内容没变就不重建——
// 整块重建会冲掉正悬停的 tooltip,也会换掉「⋯」菜单的锚点。签名直接取渲染用到的全部输入
// (线路身份、焦点与依据、整条条目、取得声明、空态原因、界面语言),漏一项就是「不刷新」。
export let lastFocusPanelSignature = "";
export function renderFocusPanel(snapshot) {
  const body = $("focus-body");
  if (!body) return;
  agentFocus = computeAgentFocus(snapshot, activeSessionId, activeProcessItem());
  const lineFocuses = computeLineAgentFocuses(snapshot);
  // 空态文案必须与待办统计同源:此前固定写「队列已清空或全部被阻塞」,和两行外的
  // 「可执行 14」直接自相矛盾——没有 doing 条目 ≠ 队列空 ≠ 全阻塞,三种情况分开说。
  const reqTally = backlogTally(snapshot?.requirements ?? [], "req");
  const defectTally = backlogTally(snapshot?.defects ?? [], "defect");
  const workableTotal = reqTally.workable + defectTally.workable;
  const blockedTotal = reqTally.blocked + defectTally.blocked;
  const queueNote = workableTotal > 0
    ? `${workableTotal} ${t("条可执行待取活")}`
    : blockedTotal > 0
      ? t("可执行队列全部被阻塞")
      : t("队列已清空");
  const emptyReason = workableTotal > 0 ? `${t("暂无进行中条目")} · ${queueNote}` : queueNote;
  const collabs = typeof collaborationLines !== "undefined" && Array.isArray(collaborationLines) ? collaborationLines : [];
  const models = lineFocuses.map(({ line, focus }) => {
    const active = focusEntryOf(snapshot, focus.active);
    // 绑工作树的并行线:取活事实写在它自己树的台账里,主树快照到 merge 前都看不见——
    // 不能因此显示「未取得条目」误导成没在干活。collaboration_snapshot 的 claim 就是该线
    // 自己声明的条目,优先用它;声明里的编号在主快照里查得到就做成可点的链接。
    const collab = !active && line && !processIsPrimary(line)
      ? collabs.find((item) => item.branch && item.branch === line.branch)
      : null;
    const claim = collab?.claim ? String(collab.claim) : "";
    const claimId = claim.match(/\b[RD]-\d+\b/)?.[0] ?? "";
    return { line, focus, active, claim, claimRef: claimId && focusEntryOf(snapshot, claimId) ? claimId : "" };
  });
  const hasAnyActive = models.some((model) => model.active);
  const signature = JSON.stringify([
    resolveUiLanguage(),
    emptyReason,
    queueNote,
    models.map(({ line, focus, active, claim, claimRef }) => [
      line ? [line.id, line.label, line.branch ?? "", lineAuthorityLabel(line)] : null,
      focus.activeSource,
      active ? [active.kind, active.entry] : null,
      claim,
      claimRef,
    ]),
  ]);
  if (signature !== lastFocusPanelSignature || !body.children.length) {
    lastFocusPanelSignature = signature;
    // 卡片节点要换了:开着的「⋯」菜单锚在旧节点上,先收起,免得悬在一个已摘除的锚点旁边。
    closeFocusMenu();
    body.replaceChildren();
    for (const { line, focus, active, claim, claimRef } of models) {
      const section = document.createElement("section");
      section.className = "line-focus";
      if (line?.id) {
        section.dataset.processId = line.id;
        motionSync(section);
      }
      // 线路头只写「身份 · 名称」;分支名是次要信息,进 tooltip。
      const heading = document.createElement("div");
      heading.className = "line-focus-head";
      heading.textContent = line ? `${lineAuthorityLabel(line)} · ${line.label}` : t("主代理");
      if (line?.branch) heading.title = line.branch;
      section.appendChild(heading);
      if (active) {
        section.appendChild(buildFocusCard(active.entry, active.kind, focus.activeSource));
      } else {
        // 空线路只占一行:全局的「为什么没在做」只在末尾说一次,不在每条线路下重复。
        const empty = document.createElement("div");
        empty.className = "focus-empty line-focus-empty";
        if (claim) {
          const claimTitle = t("取活事实在线路工作树内,合并后进入主列表");
          if (claimRef) {
            const link = document.createElement("button");
            link.type = "button";
            link.className = "ref-link focus-claim-link";
            link.setAttribute("data-i18n-raw", "");
            link.textContent = claim;
            link.title = `${claimTitle}\n${t("点击查看详情")}`;
            link.addEventListener("click", () => void jumpToEntry(claimRef, { expand: true }));
            empty.appendChild(link);
          } else {
            empty.setAttribute("data-i18n-raw", "");
            empty.textContent = claim;
            empty.title = claimTitle;
          }
        } else {
          empty.textContent = t("未取得条目");
          empty.title = emptyReason;
        }
        section.appendChild(empty);
      }
      body.appendChild(section);
    }
    if (!hasAnyActive) {
      const empty = document.createElement("div");
      empty.className = "focus-empty focus-empty-global";
      const text = document.createElement("span");
      text.textContent = `${t("当前没有在做的条目")} · ${queueNote}`;
      const open = document.createElement("button");
      open.type = "button";
      open.className = "ghost mini";
      open.textContent = t("查看完整列表");
      open.addEventListener("click", openDocumentsView);
      empty.append(text, open);
      body.appendChild(empty);
    }
  }
  // #7:线真在跑才标 is-live(整块重绘不丢相位:section 已 motionSync;跳过重建时照样同步)。
  syncLineFocusLive();
  const backlog = $("focus-backlog");
  if (!backlog) return;
  // 原来这里是一行三个数字:`待办 22 需求 · 6 缺陷 · 22 阻塞`。三个数字是三种分母
  // (只需求 / 只缺陷 / 需求∪缺陷),却排成并列结构;更糟的是本机数据下合并阻塞数
  // 恰好等于未关闭需求数,整行可以被读成「22 条需求,其中 22 条阻塞」。而用户真正
  // 要的量——「现在还有几条能立刻开工」——根本不在屏幕上,得心算 28−22,还算不出
  // 是需求那边有 6 条还是缺陷那边有 6 条(实际缺陷一条都推不动)。
  // 与顶部空态文案同一份统计:两处各算一遍迟早漂移成又一次「数字对不上」。
  const req = reqTally;
  const defect = defectTally;
  const total = document.createElement("div");
  total.className = "backlog-total";
  total.appendChild(backlogStat("待办总数", req.active + defect.active, "total"));
  backlog.replaceChildren(total, backlogRow("需求", "req", req), backlogRow("缺陷", "defect", defect));
  const invalid = req.invalid + defect.invalid;
  // 只在真出现时才占一行。总数刻意不含它们(引擎也不取活),但列表里看得见,
  // 不点名就又变成「数字对不上」。
  if (invalid) {
    const row = document.createElement("div");
    row.className = "backlog-row invalid";
    row.appendChild(backlogStat("状态异常", invalid, "invalid"));
    backlog.appendChild(row);
  }
}

/// 只重绘文档列表与计数(不含历史/测试/工作树):供运行中高频刷新使用。
export function renderDocsSnapshot(snapshot) {
  agentFocus = computeAgentFocus(snapshot, activeSessionId, activeProcessItem());
  renderFocusPanel(snapshot);
  renderDocList($("idea-list"), snapshot.ideas ?? [], "idea", snapshot.archived?.idea ?? 0, NEUTRAL_DOC_FILTERS, snapshot.archived_entries?.idea ?? []);
  renderDocuments(snapshot);
  $("req-count").textContent = `${snapshot.requirements.filter((r) => !r.closed).length}`;
  $("defect-count").textContent = `${snapshot.defects.filter((d) => !d.closed).length}`;
  $("idea-count").textContent = `${(snapshot.ideas ?? []).filter((g) => g.status === "inbox").length}`;
  renderConventions(snapshot.conventions);
  applyLanguage();
  // 重绘换掉了节点:跨视图跳转挂起的高亮在这里落地,它等的就是这次刷新。
  consumePendingJump();
}
