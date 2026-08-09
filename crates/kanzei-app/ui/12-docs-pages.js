function formatWorkspaceTime(value) {
  if (!value) return t("暂无时间");
  return new Date(Number(value)).toLocaleString();
}

async function selectWorkspaceProject(path) {
  try {
    const previous = currentProject;
    renderProjects(await invoke("projects_select", { path }));
    if (previous !== path) {
      setRunning(false, "空闲");
      clearChat();
      bgClear();
      renderTodoPanel([], 0, 0);
      await loadConversation();
      await refreshDocs();
      await loadModels();
      refreshGit();
      await refreshPendingInputs();
      await refreshProcesses();
    }
    refreshWorkspace();
  } catch (error) {
    toastError(`切换项目失败:${error}`);
  }
}

let lastWorkspaceSnapshot = null;
function renderWorkspace(snapshot) {
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
    status.textContent = project.status === "running" ? t("运行中") : project.status === "failed" ? t("失败") : t("空闲");
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
    card.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      card.click();
    });
    card.addEventListener("click", () => selectWorkspaceProject(project.path));
    root.appendChild(card);
  }
}

async function refreshWorkspace() {
  try {
    renderWorkspace(await invoke("workspace_snapshot"));
  } catch (error) {
    toastError(`工作区刷新失败:${error}`, { retry: refreshWorkspace });
  }
}
let documentsKind = "req";
let latestDocsSnapshot = null;
const documentFilters = {
  req: { status: "all", priority: "all", complexity: "all", tag: "all", blocked: "all", sort: "manual", grouped: localStorage.getItem("kz-grouped-docs") !== "0" },
  defect: { status: "all", priority: "all", tag: "all", blocked: "all", grouped: localStorage.getItem("kz-grouped-docs") !== "0" },
};
const documentStatusOptions = {
  req: [["all", "全部状态"], ["todo", "todo"], ["doing", "doing"], ["done", "done"], ["dropped", "dropped"]],
  defect: [["all", "全部状态"], ["open", "open"], ["fixing", "fixing"], ["fixed", "fixed"], ["wontfix", "wontfix"]],
};
// 「对照」模式下筛选同时作用于两个队列——并排看的前提就是同一套条件,
// 各筛各的等于没在对照。单类型模式只作用于当前那一个。
function docFilterTargets() {
  return documentsKind === "both" ? ["req", "defect"] : [documentsKind];
}
function syncDocumentFilters(snapshot) {
  const statusFilter = $("documents-status-filter");
  const priorityFilter = $("documents-priority-filter");
  const tagFilter = $("documents-tag-filter");
  const blockedFilter = $("documents-blocked-filter");
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
  statusFilter.value = documentsKind === "both" ? "all" : filters.status;
  priorityFilter.value = filters.priority ?? "all";
  blockedFilter.value = filters.blocked ?? "all";
  // 回落必须写回状态,不能只改下拉的显示值。
  filters.tag = syncTagFilter(tagFilter, entries, filters.tag ?? "all");
}
function renderDocuments(snapshot) {
  latestDocsSnapshot = snapshot;
  // tab 直调不经 renderDocsSnapshot,work-priority 可能刚切过——重算一次,幂等。
  agentFocus = computeAgentFocus(snapshot);
  const reqList = $("documents-req-list");
  const defectList = $("documents-defect-list");
  if (!reqList || !defectList) return;
  syncDocumentFilters(snapshot);
  // 两处都把原始条目交给 renderDocList 自己筛:这里曾经预筛一遍缺陷再传进去,
  // 等于同一套筛选写了两份,改一处漏一处就会两边对不上(R-123 验收 ④)。
  renderDocList(reqList, snapshot.requirements ?? [], "req", snapshot.archived?.req ?? 0, documentFilters.req, snapshot.archived_entries?.req ?? []);
  renderDocList(defectList, snapshot.defects ?? [], "defect", snapshot.archived?.defect ?? 0, documentFilters.defect, snapshot.archived_entries?.defect ?? []);
  // 「对照」把两个队列并排摆出来:需求与缺陷互相引用,分成两个标签页时对不起来。
  const both = documentsKind === "both";
  reqList.classList.toggle("hidden", !both && documentsKind !== "req");
  defectList.classList.toggle("hidden", !both && documentsKind !== "defect");
  $("documents-scroll")?.classList.toggle("compare", both);
  $("documents-tab-req").className = documentsKind === "req" ? "primary" : "ghost";
  $("documents-tab-defect").className = documentsKind === "defect" ? "primary" : "ghost";
  const compareTab = $("documents-tab-both");
  if (compareTab) compareTab.className = both ? "primary" : "ghost";
  syncBatchBar();
}
// 取活焦点(D-207):在做的条目与 agent 下一个会拿的条目。数据取 scheduler 序的
// snapshot(可执行在前+block_reasons),按当前 work-priority 跨需求/缺陷两队计算——
// 这就是 agent 实际会走的顺序。结果只依赖数据,与视图排序/分组/筛选无关,
// 所以无论用户怎么调整视图,标记始终落在同一批条目上:所见即取活。
// 口径:active 只标「可执行的 doing/fixing」——blocked 条目不计 WIP、不占运行焦点;
// next 是 active 之后取活序第一个可开工条目(requirement-first 先需求后缺陷)。
let agentFocus = { active: new Set(), next: null };
function computeAgentFocus(snapshot) {
  const focus = { active: new Set(), next: null };
  if (!snapshot) return focus;
  const reqs = snapshot.requirements ?? [];
  const defs = snapshot.defects ?? [];
  // 在做的 = 可执行的 doing/fixing。blocked doing 不计:§1.1 阻塞项不进 WIP、
  // 不占运行焦点——agent 会跳过它继续取下一个可开工条目,渲染必须与取活一致
  // (否则 R-157 类阻塞 doing 会被标成「agent 正在做」,而实际它推不动)。
  for (const entry of reqs)
    if (entry.status === "doing" && !entry?.blocked) focus.active.add(entry.id);
  for (const entry of defs)
    if (entry.status === "fixing" && !entry?.blocked) focus.active.add(entry.id);
  // 下一个 = 取活序里第一个可开工且还没在做的条目。WIP 规则下 agent 会先做完
  // 高亮的那些;这一条是它们之后第一个被拿起的。
  const firstWorkable = (list, openStatus) =>
    list.find(
      (entry) =>
        entry.status === openStatus &&
        !(Array.isArray(entry.block_reasons) && entry.block_reasons.length)
    );
  const queues =
    selectedWorkPriority() === "requirement-first"
      ? [[reqs, "todo"], [defs, "open"]]
      : [[defs, "open"], [reqs, "todo"]];
  for (const [list, openStatus] of queues) {
    const hit = firstWorkable(list, openStatus);
    if (hit) {
      focus.next = hit.id;
      break;
    }
  }
  return focus;
}

/// 只重绘文档列表与计数(不含历史/测试/工作树):供运行中高频刷新使用。
function renderDocsSnapshot(snapshot) {
  agentFocus = computeAgentFocus(snapshot);
  reqFilters.tag = syncTagFilter($("req-tag-filter"), snapshot.requirements ?? [], reqFilters.tag);
  defectFilters.tag = syncTagFilter($("defect-tag-filter"), snapshot.defects ?? [], defectFilters.tag);
  renderDocList($("req-list"), snapshot.requirements, "req", snapshot.archived?.req ?? 0, reqFilters, snapshot.archived_entries?.req ?? []);
  renderDocList($("defect-list"), snapshot.defects, "defect", snapshot.archived?.defect ?? 0, defectFilters, snapshot.archived_entries?.defect ?? []);
  renderDocList($("goal-list"), snapshot.goals ?? [], "goal", snapshot.archived?.goal ?? 0, reqFilters, snapshot.archived_entries?.goal ?? []);
  renderDocuments(snapshot);
  renderDocList($("source-list"), snapshot.sources ?? [], "source", snapshot.archived?.source ?? 0, reqFilters, snapshot.archived_entries?.source ?? []);
  renderDocList($("finding-list"), snapshot.findings ?? [], "finding", snapshot.archived?.finding ?? 0, reqFilters, snapshot.archived_entries?.finding ?? []);
  $("research-count").textContent = `${(snapshot.sources ?? []).length + (snapshot.findings ?? []).length}`;
  $("req-count").textContent = `${snapshot.requirements.filter((r) => !r.closed).length}`;
  $("defect-count").textContent = `${snapshot.defects.filter((d) => !d.closed).length}`;
  $("goal-count").textContent = `${(snapshot.goals ?? []).filter((g) => g.status === "active").length}`;
  renderConventions(snapshot.conventions);
  applyLanguage();
}
