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
// 每队按项目持久化的筛选字段与它们的默认值,全仓只此一份:documentFilters 的初值、
// saveDocFilters 的落盘字段表、restoreDocFilters 换项目时的复位,三处共用。写第二份
// 默认值迟早会漂(docstore.rs 的注释专门写过这个教训),漂了就是"存了却复位不到"。
// grouped 不在这里:它按 kz-grouped-docs 全局记、不随项目走(见 bindGroupToggle),
// 换项目时不该被复位。
const DOC_FILTER_DEFAULTS = Object.freeze({
  req: Object.freeze({ status: "all", priority: "all", complexity: "all", tag: "all", blocked: "all", sort: "manual" }),
  defect: Object.freeze({ status: "all", priority: "all", tag: "all", blocked: "all" }),
});
const documentFilters = {
  req: { ...DOC_FILTER_DEFAULTS.req, grouped: localStorage.getItem("kz-grouped-docs") !== "0" },
  defect: { ...DOC_FILTER_DEFAULTS.defect, grouped: localStorage.getItem("kz-grouped-docs") !== "0" },
};
const documentStatusOptions = {
  req: [["all", "全部状态"], ["todo", "todo"], ["doing", "doing"], ["done", "done"], ["dropped", "dropped"]],
  defect: [["all", "全部状态"], ["open", "open"], ["fixing", "fixing"], ["fixed", "fixed"], ["wontfix", "wontfix"]],
};
// 「对照」模式下筛选同时作用于两个队列——并排看的前提就是同一套条件,
// 各筛各的等于没在对照。单类型模式只作用于当前那一个。
// 注意适用范围:这是 **applyDocFilter(用户主动调控件)** 的写入目标。
// syncDocumentFilters 的回填/纠正**不得**照着它跨队列写——那条路径上用户什么都没做,
// 只是切了个标签页,写下去就是"看一眼就改掉状态"(见 syncDocumentFilters 里标签那段)。
function docFilterTargets() {
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
function neutralizedDocFilters(state) {
  if (!state) return NEUTRAL_DOC_FILTERS;
  const overrides = {};
  // 对照页两队状态机不同,状态筛选只提供「全部」,渲染必须跟着中性。
  // 标签同理:两队各有各的标签口径,并排看时按谁的都不对(按需求那支渲染,缺陷队列就被
  // 一个用户从没在缺陷页设过的条件筛掉一批)。所以对照页的标签也走中性副本,与
  // status/complexity/sort 同一套机制——**只改显示,不动任何一队的底层状态**。
  if (documentsKind === "both") {
    overrides.status = "all";
    overrides.tag = "all";
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
function syncDocumentFilters(snapshot) {
  const statusFilter = $("documents-status-filter");
  const priorityFilter = $("documents-priority-filter");
  const complexityFilter = $("documents-complexity-filter");
  const sortSelect = $("documents-sort");
  const tagFilter = $("documents-tag-filter");
  const blockedFilter = $("documents-blocked-filter");
  // 禁用要说破(D-210/D-211 一路的教训):控件真的置灰,不做静默无效。
  const isTests = documentsKind === "tests";
  for (const el of [priorityFilter, tagFilter, blockedFilter]) if (el) el.disabled = isTests;
  if (isTests) {
    for (const el of [statusFilter, complexityFilter, sortSelect]) if (el) el.disabled = true;
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
  priorityFilter.value = filters.priority ?? "all";
  blockedFilter.value = filters.blocked ?? "all";
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
  // 传中性副本而不是底层状态:对照页要的是「显示上不带筛选」,不是「把用户的筛选清掉」。
  // 渲染、拖拽判定、锁提示三处拿的都是同一个副本,所以"列表完不完整"与"能不能拖"仍同源。
  renderDocList(reqList, snapshot.requirements ?? [], "req", snapshot.archived?.req ?? 0, neutralizedDocFilters(documentFilters.req), snapshot.archived_entries?.req ?? []);
  renderDocList(defectList, snapshot.defects ?? [], "defect", snapshot.archived?.defect ?? 0, neutralizedDocFilters(documentFilters.defect), snapshot.archived_entries?.defect ?? []);
  // 「对照」把两个队列并排摆出来:需求与缺陷互相引用,分成两个标签页时对不起来。
  const isTests = documentsKind === "tests";
  const both = documentsKind === "both";
  const depMode = dependencyViewOpen && !isTests;
  reqList.classList.toggle("hidden", isTests || depMode || (!both && documentsKind !== "req"));
  defectList.classList.toggle("hidden", isTests || depMode || (!both && documentsKind !== "defect"));
  $("documents-tests")?.classList.toggle("hidden", !isTests);
  $("documents-scroll")?.classList.toggle("compare", both);
  $("documents-tab-req").className = documentsKind === "req" ? "primary" : "ghost";
  $("documents-tab-defect").className = documentsKind === "defect" ? "primary" : "ghost";
  const testsTab = $("documents-tab-tests");
  if (testsTab) testsTab.className = isTests ? "primary" : "ghost";
  const compareTab = $("documents-tab-both");
  if (compareTab) compareTab.className = both ? "primary" : "ghost";
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
  // activeSource:焦点卡片要能说出「凭什么指这一条」——runtime = 本轮运行证据命中,
  // order = 按取活序推断,null = 没有在做的条目。纯追加字段,不改单条语义。
  const focus = { active: null, next: null, activeSource: null };
  if (!snapshot) return focus;
  const reqs = snapshot.requirements ?? [];
  const defs = snapshot.defects ?? [];
  // 运行事实优先:证据指向的条目仍开放才算数(已关闭说明那条刚收尾,回落推断)。
  if (runtimeFocusId) {
    const evidence = [...reqs, ...defs].find(
      (entry) => entry.id === runtimeFocusId && !entry.closed
    );
    if (evidence) {
      focus.active = evidence.id;
      focus.activeSource = "runtime";
    }
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
        focus.activeSource = "order";
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

// ---------- 侧栏焦点卡片(用户定调:侧栏只显示当前在做,且要显示得完整一点) ----------
// 数据全部来自 docs_snapshot 已有字段,不需要后端配合;渲染只依赖 agentFocus,
// 与单页视图的排序/分组/筛选无关——用户怎么调视图,这里指的都是 agent 真会走的那条。
function focusEntryOf(snapshot, id) {
  if (!id) return null;
  const req = (snapshot?.requirements ?? []).find((entry) => entry.id === id);
  if (req) return { entry: req, kind: "req" };
  const defect = (snapshot?.defects ?? []).find((entry) => entry.id === id);
  if (defect) return { entry: defect, kind: "defect" };
  return null;
}
function focusMetaChip(text, title) {
  const chip = document.createElement("span");
  chip.className = "focus-chip";
  chip.textContent = text;
  if (title) chip.title = title;
  return chip;
}
function buildFocusCard(entry, kind) {
  const card = document.createElement("div");
  const pri = (entry.priority || "").toUpperCase();
  const blocked = entryBlocked(entry);
  card.className = `focus-card${blocked ? " blocked" : ""}${/^P[0-3]$/.test(pri) ? ` pri-${pri}` : ""}`;
  card.dataset.docId = entry.id;
  card.title = t("agent 正在做这一条");

  // 头部:类型、编号、状态。状态一律用文字表达,颜色只做冗余强化(D-105:不能只靠颜色)。
  const head = document.createElement("div");
  head.className = "focus-head";
  head.append(focusMetaChip(kind === "defect" ? t("缺陷") : t("需求与工作")));
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
  card.appendChild(head);

  // 完整标题:侧栏列表行当年只能截断显示,焦点卡片没有这个约束,给全。
  const title = document.createElement("div");
  title.className = "focus-title";
  title.setAttribute("data-i18n-raw", "");
  title.textContent = entry.title;
  card.appendChild(title);

  // 批次进度:图形给概览,「批次 3/11」文字给准数(D-105 同理,不能只靠格子)。
  const total = entry.batches?.total ?? 1;
  const done = Math.min(entry.batches?.done ?? 0, total);
  const meterRow = document.createElement("div");
  meterRow.className = "focus-meta";
  if (total > 1) {
    const cells = Math.min(total, 12);
    const filled = total <= cells ? done : Math.round((done / total) * cells);
    const meter = document.createElement("span");
    meter.className = "complexity-meter batch-meter";
    meter.style.setProperty("--cells", String(cells));
    meter.setAttribute("role", "img");
    const label = `${t("批次")} ${done}/${total}`;
    meter.setAttribute("aria-label", label);
    meter.title = label;
    for (let i = 1; i <= cells; i += 1) {
      const cell = document.createElement("span");
      cell.className = `complexity-cell${i <= filled ? " filled" : ""}`;
      cell.setAttribute("aria-hidden", "true");
      meter.appendChild(cell);
    }
    meterRow.append(meter, focusMetaChip(`${t("批次")} ${done}/${total}`));
  }
  const cx = (entry.complexity || "").trim();
  meterRow.append(
    focusMetaChip(
      /^P[0-3]$/.test(pri) ? pri : t("未设"),
      t("优先级仅参考,不影响取活顺序"),
    ),
    focusMetaChip(["小", "中", "大"].includes(cx) ? `${t("复杂度")}:${t(cx)}` : t("未评估")),
  );
  const deps = Array.isArray(entry.dependencies) ? entry.dependencies.length : 0;
  const dependents = Array.isArray(entry.dependents) ? entry.dependents.length : 0;
  if (deps) meterRow.append(focusMetaChip(`${t("依赖")} ${deps}`));
  if (dependents) meterRow.append(focusMetaChip(`${t("被依赖")} ${dependents}`));
  card.appendChild(meterRow);

  // 焦点依据:D-207 三修的对外可见面——凭运行证据还是凭取活序,必须说出来,不让人猜。
  const source = document.createElement("div");
  source.className = "focus-source";
  source.textContent = `${t("依据")}: ${
    agentFocus.activeSource === "runtime" ? t("本轮运行证据") : t("取活顺序推断")
  }`;
  card.appendChild(source);

  // 阻塞原因逐条列出:阻塞是「推不动」的唯一合法解释,理由必须可读。
  const blockReasons = Array.isArray(entry.block_reasons) ? entry.block_reasons : [];
  if (blocked) {
    const box = document.createElement("div");
    box.className = "doc-blocked-detail";
    const heading = document.createElement("strong");
    heading.textContent = t("阻塞原因");
    box.appendChild(heading);
    for (const reason of blockReasons.length ? blockReasons : [t("缺少阻塞原因")]) {
      const line = document.createElement("div");
      line.setAttribute("data-i18n-raw", "");
      line.textContent = `• ${reason}`;
      box.appendChild(line);
    }
    card.appendChild(box);
  }

  // 业务字段:取前 3 条(进展/验收/复现之类),值截到 160 字——卡片是概览不是全文。
  for (const [key, value] of (entry.fields ?? []).slice(0, 3)) {
    const field = document.createElement("div");
    field.className = "doc-field";
    field.setAttribute("data-i18n-raw", "");
    const text = String(value ?? "");
    field.textContent = `${key}: ${text.length > 160 ? `${text.slice(0, 160)}…` : text}`;
    card.appendChild(field);
  }

  // 状态流转留在侧栏:取活时要能直接切,这条链路不能因为列表搬家而断掉。
  const actions = document.createElement("div");
  actions.className = "doc-actions";
  for (const next of entry.nextStatuses ?? []) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ghost mini";
    button.textContent = `→ ${t("转")} ${localizedDocStatus(next)}`;
    button.addEventListener("click", async () => {
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
        log(`状态流转失败:${err}`, "warn");
      }
    });
    actions.appendChild(button);
  }
  const openInList = document.createElement("button");
  openInList.type = "button";
  openInList.className = "ghost mini";
  openInList.textContent = t("在完整列表中查看");
  openInList.addEventListener("click", () => jumpToEntry(entry.id));
  actions.appendChild(openInList);
  card.appendChild(actions);
  return card;
}
function renderFocusPanel(snapshot) {
  const body = $("focus-body");
  if (!body) return;
  body.replaceChildren();
  const active = focusEntryOf(snapshot, agentFocus.active);
  if (active) {
    body.appendChild(buildFocusCard(active.entry, active.kind));
  } else {
    const empty = document.createElement("div");
    empty.className = "focus-empty";
    const line = document.createElement("div");
    line.textContent = t("当前没有在做的条目");
    const why = document.createElement("div");
    why.className = "dim";
    why.textContent = t("队列已清空或全部被阻塞");
    const open = document.createElement("button");
    open.type = "button";
    open.className = "ghost mini";
    open.textContent = t("查看完整列表");
    open.addEventListener("click", openDocumentsView);
    empty.append(line, why, open);
    body.appendChild(empty);
  }
  // 「下一个」只在 computeAgentFocus 真给出时渲染:拿不到就不留空壳,不编。
  const next = focusEntryOf(snapshot, agentFocus.next);
  if (next) {
    const row = document.createElement("div");
    row.className = "focus-next";
    row.dataset.docId = next.entry.id;
    const label = document.createElement("span");
    label.className = "focus-next-label";
    label.textContent = `${t("下一个")}:`;
    const text = document.createElement("span");
    text.className = "focus-next-title";
    text.setAttribute("data-i18n-raw", "");
    text.textContent = `${next.entry.id} ${next.entry.title}`;
    text.title = `${next.entry.id} ${next.entry.title}`;
    const jump = document.createElement("button");
    jump.type = "button";
    jump.className = "ghost mini";
    jump.textContent = t("在完整列表中查看");
    jump.addEventListener("click", () => jumpToEntry(next.entry.id));
    row.append(label, text, jump);
    body.appendChild(row);
  }
  const backlog = $("focus-backlog");
  if (!backlog) return;
  const reqs = snapshot?.requirements ?? [];
  const defects = snapshot?.defects ?? [];
  const openReq = reqs.filter((entry) => !entry.closed).length;
  const openDefect = defects.filter((entry) => !entry.closed).length;
  const blockedCount = [...reqs, ...defects].filter((entry) => !entry.closed && entryBlocked(entry)).length;
  backlog.textContent = `${t("待办")} ${openReq} ${t("需求")} · ${openDefect} ${t("缺陷")} · ${blockedCount} ${t("阻塞")}`;
}

/// 只重绘文档列表与计数(不含历史/测试/工作树):供运行中高频刷新使用。
function renderDocsSnapshot(snapshot) {
  agentFocus = computeAgentFocus(snapshot);
  renderFocusPanel(snapshot);
  renderDocList($("goal-list"), snapshot.goals ?? [], "goal", snapshot.archived?.goal ?? 0, NEUTRAL_DOC_FILTERS, snapshot.archived_entries?.goal ?? []);
  renderDocuments(snapshot);
  renderDocList($("source-list"), snapshot.sources ?? [], "source", snapshot.archived?.source ?? 0, NEUTRAL_DOC_FILTERS, snapshot.archived_entries?.source ?? []);
  renderDocList($("finding-list"), snapshot.findings ?? [], "finding", snapshot.archived?.finding ?? 0, NEUTRAL_DOC_FILTERS, snapshot.archived_entries?.finding ?? []);
  $("research-count").textContent = `${(snapshot.sources ?? []).length + (snapshot.findings ?? []).length}`;
  $("req-count").textContent = `${snapshot.requirements.filter((r) => !r.closed).length}`;
  $("defect-count").textContent = `${snapshot.defects.filter((d) => !d.closed).length}`;
  $("goal-count").textContent = `${(snapshot.goals ?? []).filter((g) => g.status === "active").length}`;
  renderConventions(snapshot.conventions);
  applyLanguage();
  // 重绘换掉了节点:跨视图跳转挂起的高亮在这里落地,它等的就是这次刷新。
  consumePendingJump();
}
