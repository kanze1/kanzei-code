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
  const depMode = dependencyViewOpen;
  reqList.classList.toggle("hidden", depMode || (!both && documentsKind !== "req"));
  defectList.classList.toggle("hidden", depMode || (!both && documentsKind !== "defect"));
  $("documents-scroll")?.classList.toggle("compare", both);
  $("documents-tab-req").className = documentsKind === "req" ? "primary" : "ghost";
  $("documents-tab-defect").className = documentsKind === "defect" ? "primary" : "ghost";
  const compareTab = $("documents-tab-both");
  if (compareTab) compareTab.className = both ? "primary" : "ghost";
  renderDependencyView(snapshot);
  syncBatchBar();
}
// 依赖视图(R-111):按依赖拓扑分层展示需求+缺陷。可做层 = 依赖全部满足(已关闭或
// 不依赖任何未完成条目)的条目;被阻塞层 = 还有未完成依赖的条目。点击任意条目
// 高亮它的依赖链——向上(它依赖谁)与向下(谁依赖它),整条链一眼可读。
// 数据来自批1 的 docs_snapshot 每条目 dependencies/dependents 字段(「依赖:」语义,
// refs 不参与),与引擎取活/阻塞判断同源,不做第二套解析。
let dependencyViewOpen = false;
function renderDependencyView(snapshot) {
  const depView = $("documents-dep-view");
  const toggle = $("documents-dep-toggle");
  if (!depView || !toggle) return;
  if (!dependencyViewOpen) {
    depView.classList.add("hidden");
    toggle.classList.remove("primary");
    toggle.classList.add("ghost");
    return;
  }
  toggle.classList.add("primary");
  toggle.classList.remove("ghost");
  depView.classList.remove("hidden");
  const reqs = snapshot?.requirements ?? [];
  const defs = snapshot?.defects ?? [];
  const entries = [...reqs, ...defs];
  const byId = new Map(entries.map((e) => [e.id, e]));
  const done = new Set(entries.filter((e) => e.closed || e.status === "done" || e.status === "fixed").map((e) => e.id));
  const hasDeps = (e) => Array.isArray(e.dependencies) && e.dependencies.length > 0;
  const depsDone = (e) => (e.dependencies ?? []).every((id) => done.has(id));
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
function highlightDependencyChain(clicked, entry, byId) {
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
// 取活焦点(D-207):在做的条目与 agent 下一个会拿的条目。数据取 scheduler 序的
// snapshot(可执行在前+block_reasons),按当前 work-priority 跨需求/缺陷两队计算——
// 这就是 agent 实际会走的顺序。结果只依赖数据,与视图排序/分组/筛选无关,
// 所以无论用户怎么调整视图,标记始终落在同一批条目上:所见即取活。
// 口径:active 是取活序第一个可执行的 doing/fixing(单条)——单线程下 agent 一次只推
// 一条,多余的可执行 doing/fixing 只是"已取未动"的历史状态,不是正在做;blocked
// 条目不计 WIP、不占运行焦点。next 是取活序第一个可开工条目(requirement-first
// 先需求后缺陷)。
let agentFocus = { active: null, next: null };
// 运行事实(D-207 三修):本轮里 agent 实际动过谁——req/defect 更新与批次提交都带
// 条目 ID,这是"正在做"的真源。文件状态推断只是兜底:缺陷优先下一条挂着 fixing
// 的旧缺陷(如 D-202)会永远赢得指针,而 agent 实际在推别的条目(用户实测指着
// 缺陷、实做 R-117/R-122)。运行证据一到就覆盖推断;run 结束或新一轮开跑时清空。
let runtimeFocusId = null;
function setRuntimeFocus(id) {
  if (id) runtimeFocusId = id;
}
function clearRuntimeFocus() {
  runtimeFocusId = null;
}
function computeAgentFocus(snapshot) {
  const focus = { active: null, next: null };
  if (!snapshot) return focus;
  const reqs = snapshot.requirements ?? [];
  const defs = snapshot.defects ?? [];
  // 运行事实优先:证据指向的条目仍开放才算数(已关闭说明那条刚收尾,回落推断)。
  if (runtimeFocusId) {
    const evidence = [...reqs, ...defs].find(
      (entry) => entry.id === runtimeFocusId && !entry.closed
    );
    if (evidence) focus.active = evidence.id;
  }
  const queues =
    selectedWorkPriority() === "requirement-first"
      ? [[reqs, "doing"], [defs, "fixing"]]
      : [[defs, "fixing"], [reqs, "doing"]];
  // 正在做 = 取活序里第一个可执行的 doing/fixing(单条)。blocked 不计:§1.1 阻塞项
  // 不进 WIP、不占运行焦点——agent 会跳过它继续取下一个可开工条目,渲染必须与
  // 取活一致(否则 R-157 类阻塞 doing 会被标成「agent 正在做」,而实际它推不动)。
  if (!focus.active) {
    for (const [list, status] of queues) {
      const hit = list.find((entry) => entry.status === status && !entry?.blocked);
      if (hit) {
        focus.active = hit.id;
        break;
      }
    }
  }
  // 下一个 = 取活序里第一个可开工的 open/todo。状态与 active(doing/fixing)不重叠,
  // 无需跳过 active 本身;它就是 agent 完成当前条目后下一个会拿起的。
  const firstWorkable = (list, openStatus) =>
    list.find(
      (entry) =>
        entry.status === openStatus &&
        !(Array.isArray(entry.block_reasons) && entry.block_reasons.length)
    );
  const nextQueues =
    selectedWorkPriority() === "requirement-first"
      ? [[reqs, "todo"], [defs, "open"]]
      : [[defs, "open"], [reqs, "todo"]];
  for (const [list, openStatus] of nextQueues) {
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
