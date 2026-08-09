// R-054:拖拽重排(手动模式限定)。拖完提交完整 ID 序——注意 order 必须覆盖
// 全部条目,所以在有筛选时禁止拖拽(顺序不完整会被引擎拒绝)。
let dragReqId = null;
function reqDragEnabled(filters = reqFilters) {
  return filters.sort === "manual" && filters.status === "all" && filters.priority === "all" && filters.complexity === "all" && filters.tag === "all" && (filters.blocked ?? "all") === "all";
}
// R-123 职责分离 + D-211 修订:侧边栏 = 浏览与取活(只读详情 + 切状态 + 拖拽改序),
// 独立文档页 = 深度管理(字段编辑、批量操作、对照);排序不再独属文档页——侧栏的
// 锁提示/解锁按钮照常渲染,拖拽能力必须两侧一致,否则"解锁了却拖不动"(D-211)。
// 编辑表单与批量操作仍只在文档页(deepManage 门控)。
function docSurface(listEl) {
  return String(listEl?.id ?? "").startsWith("documents-") ? "documents" : "sidebar";
}

// 批量操作选中集:id → kind。跨重绘保留,否则 agent 的一次刷新就把选择清空了。
const batchSelection = new Map();

function syncBatchBar() {
  const bar = $("documents-batch-bar");
  if (!bar) return;
  // 条目可能因筛选/归档而消失,选中集要随之收敛,不然会对不存在的条目发批量请求。
  const alive = new Set(
    [...document.querySelectorAll(".documents-list .doc-item[data-doc-id]")].map((n) => n.dataset.docId),
  );
  for (const id of [...batchSelection.keys()]) if (!alive.has(id)) batchSelection.delete(id);
  bar.classList.toggle("hidden", batchSelection.size === 0);
  $("documents-batch-count").textContent = `${t("已选")} ${batchSelection.size}`;
  // 状态选项按选中集的类型给:需求与缺陷状态机不同,混选时只允许改标签。
  const kinds = new Set(batchSelection.values());
  const statusSelect = $("documents-batch-status");
  const options = kinds.size === 1 ? documentStatusOptions[[...kinds][0]].slice(1) : [];
  statusSelect.innerHTML =
    `<option value="">${kinds.size > 1 ? t("混选类型,仅可改标签") : t("改状态…")}</option>` +
    options.map(([value, label]) => `<option value="${value}">${localizeDynamic(label)}</option>`).join("");
  statusSelect.disabled = kinds.size !== 1;
}

async function applyBatch() {
  const status = $("documents-batch-status").value;
  const tag = $("documents-batch-tag").value;
  if (!status && !tag) {
    toast(t("先选择要改的状态或标签"));
    return;
  }
  const targets = [...batchSelection.entries()];
  let ok = 0;
  const failures = [];
  for (const [id, kind] of targets) {
    try {
      await invoke("docs_update", {
        projectDir: currentProject,
        kind,
        action: "update",
        id,
        ...(status ? { status } : {}),
        ...(tag ? { fields: { "标签": tag } } : {}),
      });
      ok += 1;
    } catch (error) {
      // 逐条独立提交:一条失败(比如状态机不允许后退)不该把整批回滚掉,
      // 但必须逐条报出来,否则用户以为全成功了。
      failures.push(`${id}: ${error}`);
    }
  }
  batchSelection.clear();
  if (failures.length) toastError(`${t("批量操作部分失败")}(${ok}/${targets.length}):${failures.join(";")}`);
  else toast(`${t("批量操作完成")}:${ok}`);
  refreshDocs();
}

function docDragEnabled(kind, listEl, filterState) {
  // 拖拽改序(手动+无筛选限定):侧栏与独立文档页一致提供(D-211——锁提示与解锁按钮
  // 两侧都渲染,但 R-123 曾把排序收进文档页,侧栏 draggable 永不设置:解锁后锁提示
  // 消失、条目"能选"却拖不动。用户实测复现,验收要求两侧均可拖,承诺与实际必须一致)。
  if (kind === "req") return reqDragEnabled(filterState);
  if (kind !== "defect") return false;
  // tag/blocked 同样让列表不完整——commitDocOrder 提交的是完整 ID 序,
  // 缺条目的顺序会被引擎拒绝,不能只查 status/priority。
  return ["status", "priority", "tag", "blocked"].every(
    (key) => (filterState[key] ?? "all") === "all"
  );
}
async function commitDocOrder(listEl, kind) {
  const order = [...listEl.querySelectorAll(".doc-item[data-doc-id]")].map((el) => el.dataset.docId);
  try {
    const msg = await invoke("docs_update", {
      projectDir: currentProject,
      kind,
      action: "reorder",
      id: "",
      order,
    });
    log(msg);
    refreshDocs();
  } catch (err) {
    toastError(`排序保存失败:${err}`);
    refreshDocs();
  }
}

// 引用跳转。目标可能被筛选藏起来、在折叠分区里、在收起的侧栏里,或者已经归档——
// 旧实现只认当前可见节点(offsetParent !== null),这四种情况一律静默失败:点了没反应,
// 也没有任何提示,看起来就是"引用是死链"(D-166)。
function jumpToEntry(ref) {
  const matches = [...document.querySelectorAll("[data-doc-id]")].filter(
    (item) => item.dataset.docId === ref,
  );
  if (!matches.length) {
    toast(`${t("找不到")} ${ref}`);
    return;
  }
  // 同一条目可能同时存在于侧栏和独立文档页:优先跳当前视图里那个,都不可见就取第一个
  // 并把挡住它的容器逐层打开。
  const target = matches.find((item) => item.offsetParent) ?? matches[0];
  if (sidebarCollapsed && target.closest("#sidebar")) {
    sidebarCollapsed = false;
    localStorage.setItem("kz-sidebar-collapsed", "0");
    syncSidebar();
  }
  // 只掀开确实会藏住条目的两类容器,不对任意祖先去 hidden——那会顺手展开整个视图。
  for (let node = target; node; node = node.parentElement) {
    if (node.classList?.contains("doc-archive-list")) node.classList.remove("hidden");
    if (node.classList?.contains("sidebar-section")) node.classList.remove("collapsed");
  }
  target.scrollIntoView({ behavior: "smooth", block: "center" });
  target.classList.add("ref-highlight");
  setTimeout(() => target.classList.remove("ref-highlight"), 1200);
}

function renderDocList(el, entries, kind, archivedCount = 0, reqFilterState = reqFilters, archivedEntries = []) {
  const surface = docSurface(el);
  // 筛掉了多少条:用于"被筛空"时说清原因。列表凭空变空是最容易被当成数据丢失的
  // 一类现象,必须给出条数与一键清除,而不是留一片空白(D-169)。
  const totalBeforeFilter = entries.length;
  // 筛选一律在这里做,调用方不得再预筛一遍——两处口径必须同源,否则侧栏与文档页
  // 会在同一筛选条件下给出不同的条目集合(R-123 验收 ④)。
  if (kind === "req") entries = filterRequirements(entries, reqFilterState);
  if (kind === "defect") {
    entries = entries
      .filter((entry) => reqFilterState.status === "all" || entry.status === reqFilterState.status)
      .filter((entry) => reqFilterState.priority === "all" || entry.priority === reqFilterState.priority)
      .filter((entry) => reqFilterState.tag === "all" || entryTags(entry).includes(reqFilterState.tag))
      .filter((entry) => matchesBlockedFilter(entry, reqFilterState.blocked ?? "all"));
  }
  // 分组视图(用户定调):按受控词表分组展示;组内保持文件顺序。
  // 分组改变了视觉顺序≠文件顺序,拖拽在分组视图下必须禁用(否则会提交错乱顺序)。
  const groupHeaders = new Map();
  const isGrouped =
    (kind === "req" || kind === "defect") &&
    reqFilterState.grouped &&
    (reqFilterState.tag ?? "all") === "all";
  if (isGrouped) {
    const buckets = new Map();
    for (const entry of entries) {
      const tag = docGroupTag(entry);
      if (!buckets.has(tag)) buckets.set(tag, []);
      buckets.get(tag).push(entry);
    }
    const ordered = [];
    for (const tag of [...DOC_TAG_ORDER, "其他"]) {
      const bucket = buckets.get(tag);
      if (!bucket || !bucket.length) continue;
      groupHeaders.set(ordered.length, `${tag} · ${bucket.length}`);
      ordered.push(...bucket);
    }
    entries = ordered;
  }
  // 展开状态是 DOM 局部的,重绘会全部收起;运行中会频繁重绘,必须跨重绘保留,
  // 否则用户刚展开的条目会被 agent 的一次状态更新弹回去。
  const expandedIds = new Set(
    [...el.querySelectorAll(".doc-item[data-doc-id]")]
      .filter((item) => {
        const detail = item.querySelector(".doc-detail");
        return detail && !detail.classList.contains("hidden");
      })
      .map((item) => item.dataset.docId)
  );
  el.innerHTML = "";
  // 被筛空:必须说清"有多少条被藏起来了"并给一键清除。此前这种情况下如果还有
  // 归档条目,连"(空)"都不显示——纯一片空白,看起来就是需求全没了。
  if (entries.length === 0 && totalBeforeFilter > 0) {
    const hint = document.createElement("div");
    hint.className = "doc-empty doc-filtered-empty";
    hint.append(`${totalBeforeFilter} ${t("条被当前筛选隐藏")} · `);
    const clear = document.createElement("button");
    clear.type = "button";
    clear.className = "ghost mini";
    clear.textContent = t("清除筛选");
    clear.addEventListener("click", () => {
      for (const key of ["status", "priority", "complexity", "tag", "blocked"]) {
        if (key in reqFilterState) reqFilterState[key] = "all";
      }
      saveDocFilters();
      refreshDocs();
    });
    hint.appendChild(clear);
    el.appendChild(hint);
    return;
  }
  if (entries.length === 0 && archivedCount === 0) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = `(${t("空")})`;
    el.appendChild(empty);
    return;
  }
  // 拖不动必须说出原因(D-207/D-210):拖拽禁用是对的(分组/筛选/排序视图下视觉
  // 顺序≠文件顺序,提交会错乱),静默禁用是错的。第一版只笼统说"有筛选"——
  // R-115 之后五项筛选按项目持久化,某天设过的复杂度/阻塞筛选重启后还挂着,
  // 用户实测"我没筛选也拖不了"却无从定位。必须**点名到具体条件**并给一键解锁。
  if ((kind === "req" || kind === "defect") && entries.length > 1) {
    const locks = [];
    if (isGrouped) locks.push(t("分组视图"));
    if (kind === "req" && reqFilterState.sort !== "manual") {
      const sortNames = { id: "ID", complexity: t("复杂度"), status: t("状态"), priority: t("优先级") };
      locks.push(`${t("排序")}=${sortNames[reqFilterState.sort] ?? reqFilterState.sort}`);
    }
    const filterNames = { status: t("状态"), priority: t("优先级"), complexity: t("复杂度"), tag: t("标签"), blocked: t("阻塞") };
    for (const key of ["status", "priority", "complexity", "tag", "blocked"]) {
      if (key in reqFilterState && (reqFilterState[key] ?? "all") !== "all") {
        locks.push(`${filterNames[key]}=${reqFilterState[key]}`);
      }
    }
    if (locks.length) {
      const dragHint = document.createElement("div");
      dragHint.className = "drag-hint";
      const text = document.createElement("span");
      text.textContent = `${t("拖拽调序已锁")}: ${locks.join(" · ")}`;
      text.title = text.textContent;
      const unlock = document.createElement("button");
      unlock.type = "button";
      unlock.className = "ghost mini";
      unlock.textContent = t("解锁");
      unlock.title = t("关闭分组、切回手动排序并清除全部筛选,恢复拖拽调序");
      unlock.addEventListener("click", () => {
        if (isGrouped) {
          // 走现有开关按钮:持久化、按钮 active 态、重渲染全在它的 handler 里。
          const toggleId =
            surface === "documents"
              ? "documents-group-toggle"
              : kind === "req"
                ? "req-group-toggle"
                : "defect-group-toggle";
          $(toggleId)?.click();
        }
        if ("sort" in reqFilterState && reqFilterState.sort !== "manual") {
          reqFilterState.sort = "manual";
          if (reqFilterState === reqFilters) localStorage.setItem("kz-req-sort", "manual");
        }
        for (const key of ["status", "priority", "complexity", "tag", "blocked"]) {
          if (key in reqFilterState) reqFilterState[key] = "all";
        }
        saveDocFilters();
        syncDocFilterControls();
        refreshDocs();
      });
      dragHint.append(text, unlock);
      el.appendChild(dragHint);
    }
  }
  let position = 0;
  for (const entry of entries) {
    if (groupHeaders.has(position)) {
      const head = document.createElement("div");
      head.className = "doc-group-head";
      const [groupTag, groupCount] = groupHeaders.get(position).split(" · ");
      head.textContent = `${localizeDynamic(groupTag)} · ${groupCount}`;
      el.appendChild(head);
    }
    position += 1;
    const item = document.createElement("div");
    // 优先级着色(pri-P0 红 / P1 黄 / P2 蓝 / P3 灰):扫一眼就知道轻重。
    const pri = (entry.priority || "").toUpperCase();
    // 阻塞状态由后端调度器计算,前端只负责展示,保证列表顺序与 agent 取活一致。
    const blockedReasons = Array.isArray(entry.block_reasons) ? entry.block_reasons : [];
    const blocked = entryBlocked(entry);
    const externalBlocked = (entry.fields ?? []).some(([key, value]) =>
      ["阻塞", "blocked", "blocking"].includes(String(key).toLowerCase())
      && /外部|external|blocked/i.test(String(value))
    );
    // 取活焦点标记(D-207):在做的高亮呼吸,下一个次亮。基于数据而非视图计算,
    // 任何排序/分组/筛选下都标同一批条目——用户随便调视图,仍知道 agent 会怎么走。
    const isAgentActive = !entry.closed && agentFocus.active.has(entry.id);
    const isAgentNext = !entry.closed && agentFocus.next === entry.id;
    item.className = `doc-item${entry.closed ? " closed" : ""}${blocked ? " blocked" : ""}${externalBlocked ? " external-blocked" : ""}${/^P[0-3]$/.test(pri) ? ` pri-${pri}` : ""}${isAgentActive ? " agent-active" : ""}${isAgentNext ? " agent-next" : ""}`;
    if (isAgentActive) item.title = t("agent 正在做这一条");
    else if (isAgentNext) item.title = t("agent 下一个会拿这一条(按取活顺序)");
    item.dataset.docId = entry.id;

    const row = document.createElement("div");
    row.className = "doc-row";
    row.setAttribute("role", "button");
    row.tabIndex = 0;
    // 批量操作只在文档页:侧栏一行要尽量轻,多一个勾选框就多一层视觉噪音。
    if (surface === "documents" && (kind === "req" || kind === "defect") && !entry.closed) {
      const pick = document.createElement("input");
      pick.type = "checkbox";
      pick.className = "doc-pick";
      pick.checked = batchSelection.has(entry.id);
      pick.setAttribute("aria-label", `${t("选择")} ${entry.id}`);
      pick.addEventListener("click", (event) => event.stopPropagation());
      pick.addEventListener("change", () => {
        if (pick.checked) batchSelection.set(entry.id, kind);
        else batchSelection.delete(entry.id);
        syncBatchBar();
      });
      row.appendChild(pick);
    }
    row.setAttribute("aria-label", `${entry.id} ${entry.title}，${t("按 Enter 展开详情")}`);
    row.title = `${entry.id} ${entry.title}(${t("点击展开")})`;
    const id = document.createElement("span");
    id.className = "id";
    // R-054(用户拍板):需求行内不显示 R-xxx(乱序观感),只显示位置序号;身份收进展开详情。
    if (kind === "req") {
      id.className = "pos";
      id.textContent = `#${position}`;
    } else {
      id.textContent = entry.id;
    }
    const st = document.createElement("span");
    st.className = `st st-${entry.status || "todo"}`;
    st.textContent = localizedDocStatus(entry.status || "todo") + (entry.severity ? `/${entry.severity}` : "");
    // 复杂度(R-051):侧栏用三格电量图标表达体量，与左侧优先级色带同色并放在最前面。
    const cx = (entry.complexity || "").trim();
    if (["小", "中", "大"].includes(cx)) {
      item.classList.add(`cx-${cx === "小" ? "s" : cx === "中" ? "m" : "l"}`);
      row.title = `${row.title} · ${t("复杂度")}:${t(cx)}`;
    }
    // 批次进度格(R-160):格数 = 该条目的批次总数(复杂度给默认,条目可显式声明),
    // 已填 = 做完的批次。此前这里画的是"复杂度等级"——一个从头到尾不会变的静态值,
    // 于是一条大条目干一整天,界面上一格都不动,看着就像没推进。
    // 格数与已填一律取后端算好的 entry.batches,不在前端另存一份复杂度→格数的映射。
    if ((kind === "req" || kind === "defect") && !entry.closed) {
      const total = entry.batches?.total ?? 1;
      const done = Math.min(entry.batches?.done ?? 0, total);
      if (total > 1) {
        // 批次很多时不逐格画(11 格在侧栏里每格只剩几像素),按比例压到 12 格,
        // 精确数字放进 title/aria——图形给概览,文字给准数。
        const cells = Math.min(total, 12);
        const filled = total <= cells ? done : Math.round((done / total) * cells);
        const meter = document.createElement("span");
        meter.className = "complexity-meter batch-meter";
        // 轨道等分成几格由这里决定,CSS 只管固定总长(见 style.css 的 --cells)。
        meter.style.setProperty("--cells", String(cells));
        meter.setAttribute("role", "img");
        const label = `${t("批次")} ${done}/${total}${cx ? ` · ${t("复杂度")}:${t(cx)}` : ""}`;
        meter.setAttribute("aria-label", label);
        meter.title = label;
        for (let i = 1; i <= cells; i += 1) {
          const cell = document.createElement("span");
          cell.className = `complexity-cell${i <= filled ? " filled" : ""}`;
          cell.setAttribute("aria-hidden", "true");
          meter.appendChild(cell);
        }
        row.appendChild(meter);
      }
    }
    if (blocked || externalBlocked) {
      const blockedBadge = document.createElement("span");
      blockedBadge.className = "blocked-badge";
      blockedBadge.textContent = t("阻塞");
      blockedBadge.title = blockedReasons.length ? blockedReasons.join("；") : t("阻塞原因");
      row.appendChild(blockedBadge);
    }
    // D-205 验收③:待澄清徽标——快记推断不出复现时如实写「复现: 待澄清: …」,这类
    // 条目等用户补话,侧栏必须一眼可辨,否则伪复现坑下游(D-204:用户说"SOP 易用程度"
    // 被编成"查看 SOP 时")。只认"复现"字段以「待澄清」开头的形态,不误标其他内容。
    if (kind === "defect" && !entry.closed) {
      const reproField = (entry.fields ?? []).find(([key]) => String(key).includes("复现"));
      if (reproField && String(reproField[1] ?? "").trim().startsWith("待澄清")) {
        const clarify = document.createElement("span");
        clarify.className = "clarify-badge";
        clarify.textContent = t("待澄清");
        clarify.title = String(reproField[1]).slice(0, 160);
        row.appendChild(clarify);
      }
    }
    // 拖拽重排:需求仅手动且无筛选；缺陷仅完整列表，避免提交不完整顺序。
    // 分组视图下禁用(视觉顺序≠文件顺序);关掉分组开关即恢复拖拽。
    // 松手落在行间隙时 drop 不触发,只靠 drop 会静默丢单。
    if (!isGrouped && docDragEnabled(kind, el, reqFilterState)) {
      item.draggable = true;
      item.addEventListener("dragstart", (e) => {
        dragReqId = entry.id;
        item.classList.add("dragging");
        el.dataset.orderBefore = [...el.querySelectorAll(".doc-item[data-doc-id]")]
          .map((n) => n.dataset.docId)
          .join(",");
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", entry.id);
      });
      item.addEventListener("dragend", () => {
        item.classList.remove("dragging");
        dragReqId = null;
        const now = [...el.querySelectorAll(".doc-item[data-doc-id]")]
          .map((n) => n.dataset.docId)
          .join(",");
        if (now !== el.dataset.orderBefore) commitDocOrder(el, kind);
      });
      item.addEventListener("dragover", (e) => {
        e.preventDefault();
        const dragging = el.querySelector(".doc-item.dragging");
        if (!dragging || dragging === item) return;
        const rect = item.getBoundingClientRect();
        const before = e.clientY < rect.top + rect.height / 2;
        el.insertBefore(dragging, before ? item : item.nextSibling);
      });
    }
    if (kind === "req" || kind === "defect") {
      const badge = document.createElement("button");
      badge.className = `pri-badge ${/^P[0-3]$/.test(pri) ? pri : "unset"}`;
      badge.textContent = /^P[0-3]$/.test(pri) ? pri : t("未设");
      badge.title = t("点击循环调整优先级(仅参考,不影响取活)");
      badge.addEventListener("click", async (event) => {
        event.stopPropagation();
        const order = ["P0", "P1", "P2", "P3"];
        const next = order[(order.indexOf(pri) + 1) % order.length];
        try {
          await invoke("docs_update", { projectDir: currentProject, kind, action: "update", id: entry.id, priority: next });
          toast(`${entry.id} ${t("优先级已调整为")} ${next}`);
          refreshDocs();
        } catch (error) {
          toastError(`优先级保存失败:${error}`);
        }
      });
      row.appendChild(badge);
    }
    if (kind === "req" && el.id !== "req-list") {
      const complexityBadge = document.createElement("span");
      complexityBadge.className = "complexity-badge";
      complexityBadge.textContent = cx === "小" || cx === "中" || cx === "大" ? `${t("复杂度")}:${t(cx)}` : t("未评估");
      row.appendChild(complexityBadge);
    }
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = entry.title;
    row.appendChild(title);
    item.appendChild(row);

    // 展开面板:完整标题、字段、合法的状态流转按钮(与硬门禁同一套规则)。
    const detail = document.createElement("div");
    detail.className = expandedIds.has(entry.id) ? "doc-detail" : "doc-detail hidden";
    const full = document.createElement("div");
    full.className = "doc-full-title";
    // 行内不显示需求 ID(R-054),身份收进展开详情——这里必须给全。
    full.textContent = kind === "req" ? `${entry.id} · ${entry.title}` : entry.title;
    detail.appendChild(full);
    // R-123:字段编辑只在独立文档页。侧栏展开详情因此退回只读呈现,高度显著下降,
    // 回到"浏览与取活"的本职;编辑能力没有丢,在文档页有完整入口。
    const deepManage = surface === "documents" && (kind === "req" || kind === "defect") && !entry.closed;
    if (deepManage) {
      const editBox = document.createElement("div");
      editBox.className = "doc-edit";
      // 每格都带可见字段名:字段集是自由的,只靠顺序或 tooltip 认不出改的是哪一条。
      // 段落型字段(内容/验收/复现/进展)按值长度自动升级为 textarea——不硬编码字段名。
      const addRow = (label, value, hint) => {
        const row = document.createElement("label");
        row.className = "doc-edit-row";
        const name = document.createElement("span");
        name.className = "doc-edit-key";
        name.textContent = label;
        const multiline = value.length > 60 || value.includes("\n");
        const control = document.createElement(multiline ? "textarea" : "input");
        control.value = value;
        control.title = hint;
        if (multiline) control.rows = Math.min(10, Math.max(3, Math.ceil(value.length / 42)));
        row.append(name, control);
        editBox.appendChild(row);
        return control;
      };
      const titleInput = addRow(t("标题"), entry.title, t("编辑标题"));
      const fieldInputs = [];
      for (const [key, value] of entry.fields ?? []) {
        fieldInputs.push([key, addRow(key, value, `${t("编辑字段")}: ${key}`)]);
      }
      const save = document.createElement("button");
      save.type = "button";
      save.className = "primary mini";
      save.textContent = t("保存修改");
      save.addEventListener("click", async (event) => {
        event.stopPropagation();
        try {
          await invoke("docs_update", {
            projectDir: currentProject,
            kind,
            action: "update",
            id: entry.id,
            title: titleInput.value,
            fields: Object.fromEntries(fieldInputs.map(([key, input]) => [key, input.value])),
          });
          toast(t("已保存"));
          refreshDocs();
        } catch (error) {
          toastError(`记录保存失败:${error}`);
        }
      });
      const editActions = document.createElement("div");
      editActions.className = "doc-edit-actions";
      editActions.appendChild(save);
      editBox.appendChild(editActions);
      editBox.addEventListener("click", (event) => event.stopPropagation());
      detail.appendChild(editBox);
    }
    // 编辑表单已经把每个字段连名带值摆出来了,再渲染一遍只读列表就是同一份内容显示两遍;
    // 「阻塞字段: X」这条理由更是直接重复 阻塞 字段的原文(D-165)。有编辑表单时:
    // 阻塞原因只留调度器推导出来的理由(依赖/阶段/循环),只读列表只留 refs(它是可跳转的链接)。
    const hasEditor = deepManage;
    if (blocked || externalBlocked) {
      const reasons = hasEditor
        ? blockedReasons.filter((reason) => !String(reason).startsWith("阻塞字段:"))
        : blockedReasons;
      const shown = reasons.length ? reasons : hasEditor ? [] : [t("缺少阻塞原因")];
      if (shown.length) {
        const blockBox = document.createElement("div");
        blockBox.className = "doc-blocked-detail";
        const blockTitle = document.createElement("strong");
        blockTitle.textContent = t("阻塞原因");
        blockBox.appendChild(blockTitle);
        for (const reason of shown) {
          const line = document.createElement("div");
          line.textContent = `• ${reason}`;
          blockBox.appendChild(line);
        }
        detail.appendChild(blockBox);
      }
    }
    for (const [key, value] of entry.fields ?? []) {
      const isRefs = key.toLowerCase() === "refs";
      if (hasEditor && !isRefs) continue;
      const f = document.createElement("div");
      f.className = "doc-field";
      if (isRefs) {
        f.append(`${key}: `);
        for (const ref of String(value).split(/[\s,]+/).filter(Boolean)) {
          const link = document.createElement("button");
          link.className = "ref-link";
          link.textContent = ref;
          link.addEventListener("click", (event) => {
            event.stopPropagation();
            jumpToEntry(ref);
          });
          f.appendChild(link);
          f.append(" ");
        }
      } else {
        f.textContent = `${key}: ${value}`;
      }
      detail.appendChild(f);
    }
    // 复杂度是元数据编辑,同样归文档页;侧栏保留三格电量图标做只读呈现。
    if (deepManage) {
      const complexityRow = document.createElement("div");
      complexityRow.className = "doc-progress";
      const complexitySelect = document.createElement("select");
      complexitySelect.innerHTML = `<option value="">${t("未评估")}</option><option>小</option><option>中</option><option>大</option>`;
      complexitySelect.value = cx;
      complexitySelect.title = kind === "defect" ? t("设置缺陷复杂度") : t("设置需求复杂度");
      complexitySelect.addEventListener("click", (event) => event.stopPropagation());
      complexitySelect.addEventListener("change", async () => {
        try {
          await invoke("docs_update", { projectDir: currentProject, kind, action: "update", id: entry.id, fields: { "复杂度": complexitySelect.value } });
          toast(t("复杂度已保存"));
          refreshDocs();
        } catch (error) {
          toastError(`复杂度保存失败:${error}`);
        }
      });
      complexityRow.append(`${t("复杂度")}: `, complexitySelect);
      detail.appendChild(complexityRow);
    }
    // 目标专属:状态速记(写入 fields.状态,同时保留计划字段用于展示)。
    if (kind === "goal" && !entry.closed) {
      const progressRow = document.createElement("div");
      progressRow.className = "doc-progress";
      const input = document.createElement("input");
      input.placeholder = t("记录状态/调整方向,回车保存");
      input.addEventListener("click", (e) => e.stopPropagation());
      input.addEventListener("keydown", async (e) => {
        if (e.key !== "Enter" || !input.value.trim()) return;
        try {
          const msg = await invoke("docs_update", {
            projectDir: currentProject,
            kind,
            action: "update",
            id: entry.id,
            fields: { "状态": input.value.trim() },
          });
          log(msg);
          refreshDocs();
        } catch (err) {
          toastError(String(err));
        }
      });
      progressRow.appendChild(input);
      detail.appendChild(progressRow);
    }
    if ((entry.nextStatuses ?? []).length > 0) {
      const actions = document.createElement("div");
      actions.className = "doc-actions";
      for (const next of entry.nextStatuses) {
        const btn = document.createElement("button");
        btn.className = "ghost mini";
        btn.textContent = `→ ${t("转")} ${localizedDocStatus(next)}`;
        btn.addEventListener("click", async (e) => {
          e.stopPropagation();
          try {
            const msg = await invoke("docs_update", {
              projectDir: currentProject,
              kind,
              action: "update",
              id: entry.id,
              status: next,
            });
            log(msg);
            refreshDocs();
          } catch (err) {
            toastError(String(err));
            log(`状态流转失败:${err}`, "warn");
          }
        });
        actions.appendChild(btn);
      }
      detail.appendChild(actions);
    }
    item.appendChild(detail);
    row.addEventListener("keydown", (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      detail.classList.toggle("hidden");
      row.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    });
    row.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    row.addEventListener("click", () => {
      detail.classList.toggle("hidden");
      row.setAttribute("aria-expanded", String(!detail.classList.contains("hidden")));
    });
    el.appendChild(item);
  }
  // 已完成项归档在 *-archive.md,不占侧边栏;一行入口可翻历史。
  if (archivedCount > 0) {
    const foot = document.createElement("button");
    foot.type = "button";
    foot.className = "doc-archive-toggle";
    foot.setAttribute("aria-label", `${t("展开已归档条目")}，共 ${archivedCount} ${t("条")}`);
    foot.setAttribute("aria-expanded", "false");
    foot.title = `${t("展开已归档条目")};${t("双击打开归档文件")}`;
    foot.textContent = `${archivedCount} ${t("条")} ${t("已归档")} ▸`;
    const archive = document.createElement("div");
    archive.className = "doc-archive-list hidden";
    for (const entry of archivedEntries) {
      const row = document.createElement("div");
      row.className = "archived-entry";
      // 归档条目也要挂 id:被引用的条目多半正是已经做完归档的那些,没有它跳转必然落空。
      row.dataset.docId = entry.id;
      row.textContent = `${entry.id} ${entry.title} [${entry.status}]`;
      archive.appendChild(row);
    }
    foot.addEventListener("click", () => {
      archive.classList.toggle("hidden");
      const expanded = !archive.classList.contains("hidden");
      foot.setAttribute("aria-expanded", String(expanded));
      foot.textContent = `${archivedCount} ${t("条")} ${t("已归档")} ${expanded ? "▾" : "▸"}`;
    });
    foot.addEventListener("dblclick", () => openDocViewer(`${kind}-archive`));
    el.append(foot, archive);
  }
}
