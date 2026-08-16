// R-054:拖拽重排(手动模式限定)。拖完提交完整 ID 序——注意 order 必须覆盖
// 全部条目,所以在有筛选时禁止拖拽(顺序不完整会被引擎拒绝)。
let dragReqId = null;
function reqDragEnabled(filters = NEUTRAL_DOC_FILTERS) {
  return filters.sort === "manual" && filters.status === "all" && filters.priority === "all" && filters.complexity === "all" && filters.tag === "all" && (filters.blocked ?? "all") === "all";
}
// 职责分离(用户定调,替代 R-123 的两 surface 平分):侧栏只留「当前在做」焦点卡片
// (只读详情 + 切状态,见 renderFocusPanel),完整需求/缺陷列表连同筛选、排序、分组、
// 拖拽改序、字段编辑、批量操作、依赖视图整体收进单页视图。renderDocList 仍保留
// surface 概念——idea/source/finding 这些侧栏轻列表还走它,它们没有深度管理能力。
// D-211 的教训继续有效:锁提示渲染出来了就必须真的能解锁并拖动,承诺与能力不能脱节。
function docSurface(listEl) {
  return String(listEl?.id ?? "").startsWith("documents-") ? "documents" : "sidebar";
}

// 批量操作选中集:id → kind。跨重绘保留,否则 agent 的一次刷新就把选择清空了。
const batchSelection = new Map();
const archiveLoaders = new Map();

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
  // D-256:进入循环前认领项目。批量操作进行中切项目时,整批继续写**认领时的旧项目**;
  // 循环内不得重读 currentProject——await 之间它可能已被用户切走,重读会把旧项目的
  // 条目 id 写进新项目(2026-08-11 用户拍板:按认领项目做完,不是中止)。
  const batchProjectDir = currentProject;
  let ok = 0;
  const failures = [];
  for (const [id, kind] of targets) {
    try {
      await invoke("docs_update", {
        projectDir: batchProjectDir,
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
  // D-256:批量期间用户切走了项目,整批实际落在认领时的旧项目——必须明说落地在哪,
  // 免得用户以为刚才那批改的是当前项目(切走的那个)。
  if (batchProjectDir !== currentProject) {
    toast(`${t("这批改动落在")} ${batchProjectDir}`);
  }
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
function revealEntryNode(target) {
  // 只掀开确实会藏住条目的两类容器,不对任意祖先去 hidden——那会顺手展开整个视图。
  for (let node = target; node; node = node.parentElement) {
    if (node.classList?.contains("doc-archive-list")) node.classList.remove("hidden");
    if (node.classList?.contains("sidebar-section")) node.classList.remove("collapsed");
  }
  target.scrollIntoView({ behavior: "smooth", block: "center" });
  target.classList.add("ref-highlight");
  setTimeout(() => target.classList.remove("ref-highlight"), 1200);
}
// 单页视图里承载条目的容器。按容器 id 判定而不是按 #view-documents 祖先:侧栏的
// 目标/来源/发现列表同样挂 data-doc-id,笼统地「不在侧栏就当在单页」会把跳转一个目标
// 也变成切视图。
const DOCUMENTS_ENTRY_CONTAINERS = ["documents-req-list", "documents-defect-list", "documents-dep-view"];
const inDocumentsPage = (item) => DOCUMENTS_ENTRY_CONTAINERS.some((id) => item.closest(`#${id}`));
// 跨视图跳转的高亮必须活过随后的那次刷新。openDocumentsView() 会触发 refreshDocs(),
// 它 await 的是一次真实 IPC(真机毫秒级),而 setTimeout(…, 0) 就在当下这一轮跑——
// 顺序必然是"先高亮、后重绘",高亮落在旧节点上,紧接着 renderDocList 的
// `el.innerHTML = ""` 把该节点连同 scrollIntoView 的落点一起清掉:用户被切到单页视图,
// 却看不出到底是哪一条(D-166 的另一种复发形态)。
// 所以不猜时机:把待高亮 id 存起来,由重绘收尾(renderDocsSnapshot)消费。
let pendingJumpId = null;
function consumePendingJump() {
  if (!pendingJumpId) return;
  const ref = pendingJumpId;
  // 只给一次机会:目标此刻若被筛掉或已不在列表里就作罢,不留一个会在将来某次
  // 无关刷新上突然亮起来的悬挂高亮。
  pendingJumpId = null;
  const target = [...document.querySelectorAll("[data-doc-id]")]
    .filter((item) => item.dataset.docId === ref)
    .find(inDocumentsPage);
  if (target) revealEntryNode(target);
}
// 刷新失败(目录被删、文件被锁、解析失败)时 renderDocsSnapshot 根本不会跑,上面那次
// 消费就永远不会发生。留着这个 id 正是上面「不留悬挂高亮」的反面:之后任意一次无关刷新
// ——agent 触发的 refreshDocsSoon、或用户下次再进文档页——都会把它兑现,用户没点跳转
// 条目却自己亮了。所以跳转的失败路径必须显式作废(D-211:承诺与实现不能脱节)。
function clearPendingJump() {
  pendingJumpId = null;
}
// D-413:研究工件里两类「本该可点」的字段——文献 URL 与代码域证据锚(file:line)。
// 判据放这里单点定义,渲染侧只问「这个字段是不是可打开的」,不各自认字符串。
function researchLinkField(key, value) {
  const k = String(key).toLowerCase();
  const v = String(value ?? "").trim();
  if (!v) return false;
  if (k === "url" || /^https?:\/\//i.test(v)) return true;
  // 证据锚形态:`path/to/file.rs:12` 或 `path/to/file.rs:12-34`(允许多个,取第一个)。
  return (k === "证据锚" || k === "anchor") && /[\w./\\-]+\.\w+:\d+/.test(v);
}

/// 造一个真入口按钮:文献进内置 viewer(用户 2026-08-16 定调,不跳出应用),
/// 代码域按 file:line 打开文件预览。取不到内容时如实报错,不静默变成死按钮。
function researchOpenLink(key, value) {
  const raw = String(value).trim();
  const btn = document.createElement("button");
  btn.className = "ref-link";
  btn.type = "button";
  const urlMatch = raw.match(/https?:\/\/\S+/i);
  const anchorMatch = raw.match(/([\w./\\-]+\.\w+):(\d+)/);
  if (urlMatch) {
    const url = urlMatch[0];
    btn.textContent = url;
    btn.title = t("在应用内打开");
    btn.addEventListener("click", async (event) => {
      event.stopPropagation();
      btn.disabled = true;
      try {
        const page = await invoke("webfetch_preview", { url });
        openRuntimeMarkdown(page.title || url, page.text || "");
      } catch (error) {
        toastError(`${t("打开失败")}:${error}`);
      } finally {
        btn.disabled = false;
      }
    });
  } else if (anchorMatch) {
    const [, path, line] = anchorMatch;
    btn.textContent = `${path}:${line}`;
    btn.title = t("打开文件并定位");
    btn.addEventListener("click", (event) => {
      event.stopPropagation();
      // 视图切换没有全局函数,入口是 activitybar 上那颗按钮(03-shell.js 绑在
      // `.activity-item[data-view]` 上)。点它而不是自己 toggle class:视图切换
      // 还带着侧栏、高亮、刷新等一串副作用,复刻一遍必然漏。
      document.querySelector('.activity-item[data-view="files"]')?.click();
      openFilePreview({ path: path.replace(/\\/g, "/") });
    });
  } else {
    btn.textContent = raw;
    btn.disabled = true;
  }
  return btn;
}

async function jumpToEntry(ref) {
  const findAll = () =>
    [...document.querySelectorAll("[data-doc-id]")].filter((item) => item.dataset.docId === ref);
  let matches = findAll();
  if (!matches.length) {
    const kind = ref.startsWith("R-") ? "req" : ref.startsWith("D-") ? "defect" : ref.startsWith("I-") ? "idea" : ref.startsWith("S-") ? "source" : ref.startsWith("F-") ? "finding" : null;
    const loader = kind ? archiveLoaders.get(kind) : null;
    if (loader) {
      await loader.load();
      matches = findAll();
    }
  }
  if (!matches.length) {
    toast(`${t("找不到")} ${ref}`);
    return;
  }
  // 完整列表整体搬进单页视图后,绝大多数条目只存在于 #view-documents 里。该视图未激活时
  // 它的祖先是 display:none,scrollIntoView 对隐藏祖先无效——不先把视图切过去,跳转就精确
  // 复刻 D-166 的「点了没反应」。侧栏焦点卡片里的那一条仍按老路径就地高亮。
  const inDocuments = matches.find(inDocumentsPage);
  const documentsActive = $("view-documents")?.classList.contains("active");
  if (inDocuments && !documentsActive) {
    openDocumentsView();
    // 切视图会触发 refreshDocs 重绘,重绘换掉的是节点本身:高亮加在旧节点上等于没加。
    // 交给重绘收尾时消费(consumePendingJump),不用定时器去赌 IPC 的快慢。
    // 没有项目就不会有刷新,也就没有重绘会换掉节点——那种情况直接就地高亮。
    if (currentProject) {
      pendingJumpId = ref;
      return;
    }
  }
  // 同一条目可能同时存在于侧栏焦点卡片和单页列表:优先跳单页那个(完整上下文都在那边),
  // 没有就取当前可见的,再没有就取第一个并把挡住它的容器逐层打开。
  const target = inDocuments ?? matches.find((item) => item.offsetParent) ?? matches[0];
  if (sidebarCollapsed && target.closest("#sidebar")) {
    sidebarCollapsed = false;
    localStorage.setItem("kz-sidebar-collapsed", "0");
    syncSidebar();
  }
  revealEntryNode(target);
}

// R-247:「被取得」只读 docs_snapshot 暴露的 tracker 取得线事实。
// 显式取得线按 branch 对上协作快照；doing/fixing 且无字段是 D-354 定义的默认线持有。
// 运行/空闲不改变持有关系，prompt 文本和前端排序均不参与。
function claimedCollaborationLineFor(entry) {
  const lines = typeof collaborationLines !== "undefined" && Array.isArray(collaborationLines)
    ? collaborationLines
    : [];
  const codes = typeof lineAgentCodes === "function" ? lineAgentCodes(lines) : new Map();
  const explicitOwner = String(entry.claimed_by ?? "").trim();
  if (explicitOwner) {
    // D-360:取得线必须此刻真在线。claimed_by 是历史事实,线可能早退出了(进程崩掉、
    // 用户关窗)。此前这里在找不到线时照样渲染徽标、把代号打成 "?"——而「被哪条线
    // 取得」正是这个徽标存在的全部意义,答不出就不该显示。
    const owned = lines.find((candidate) => candidate?.branch === explicitOwner);
    if (!owned) return null;
    return {
      line: owned,
      // 线在线却没分到代号属于异常兜底:退回分支名,绝不再出现问号。
      code: codes.get(owned.process_id) || explicitOwner,
      owner: explicitOwner,
    };
  }
  // 没有「取得线」字段 = 默认线持有——这是 D-354 定的编码(work.rs:989「默认线不写
  // 字段」)。但同一个形态也是「根本没人拿」,所以解码要补两个前提,缺一个就不是事实:
  //   ① 默认线此刻真的在线。引擎没运行时列表里一条线都没有,原来的代码却照样把每条
  //      doing 标成被取得(用户截图:kzapp 已退出,5 条 doing 全带着「● ? 被取得」)。
  //   ② 这一条正是默认线占着的那个 WIP。单线程下 agent 一次只推一条,其余可执行
  //      doing 只是「已取未动」的历史状态;原来的代码按 status ∈ {doing,fixing} 一刀切,
  //      于是 5 条 doing 同时归属同一条线——而一条线最多持有一条。
  // 判据直接用主线的线路焦点(12-docs-pages.js 的取活焦点真源),不在这里另写一套
  // 「谁是当前 WIP」的推断。即使用户当前查看并行线,主线的标记也不能跟着漂移。
  const primary = lines.find((candidate) => !candidate?.worktree_path);
  if (!primary) return null;
  const primaryFocus = typeof globalThis.focusForProcess === "function"
    ? globalThis.focusForProcess(primary.process_id)
    : null;
  const focused = primaryFocus?.active === entry.id;
  if (!focused) return null;
  return {
    line: primary,
    code: codes.get(primary.process_id) || primary.branch || t("默认线"),
    owner: primary.branch || t("默认线"),
  };
}

function renderDocList(el, entries, kind, archivedCount = 0, reqFilterState = NEUTRAL_DOC_FILTERS, archivedEntries = []) {
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
      // 写回一律落到底层 documentFilters[kind]。传进来的 reqFilterState 可能是对照页的
      // **中性显示副本**(见 12-docs-pages.js 的 neutralizedDocFilters):写副本等于按钮
      // 点了没反应,也不会落盘。idea/source/finding 没有筛选状态(它们那三张列表也渲染
      // 不出本按钮——不走 req/defect 的筛选分支就不可能"被筛空"),取不到就不写,免得
      // 踩到冻结的 NEUTRAL_DOC_FILTERS 上抛异常。
      const filterState = documentFilters[kind];
      if (!filterState) return;
      for (const key of ["status", "priority", "complexity", "tag", "blocked"]) {
        if (key in filterState) filterState[key] = "all";
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
          $("documents-group-toggle")?.click();
        }
        // 同「清除筛选」:写底层 documentFilters[kind],不写对照页的中性显示副本,
        // 否则解锁按钮点了没反应(锁提示还挂着,拖拽仍然锁着)。本按钮只在 req/defect
        // 上渲染(上面 kind 判断),documentFilters 里必有对应队列;取不到就不写。
        const filterState = documentFilters[kind];
        if (!filterState) return;
        if ("sort" in filterState && filterState.sort !== "manual") {
          filterState.sort = "manual";
        }
        for (const key of ["status", "priority", "complexity", "tag", "blocked"]) {
          if (key in filterState) filterState[key] = "all";
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
    // 当前在做保留运行证据焦点；排队顺序不再被前端推断成「下一个」。
    const isAgentActive = !entry.closed && agentFocus.active === entry.id;
    item.className = `doc-item${entry.closed ? " closed" : ""}${blocked ? " blocked" : ""}${externalBlocked ? " external-blocked" : ""}${/^P[0-3]$/.test(pri) ? ` pri-${pri}` : ""}${isAgentActive ? " agent-active" : ""}`;
    if (isAgentActive) item.title = t("agent 正在做这一条");
    item.dataset.docId = entry.id;

    const row = document.createElement("div");
    row.className = "doc-row";
    row.setAttribute("role", "button");
    row.tabIndex = 0;
    // D-362:文档页把可选徽标(被取得/批次格/阻塞/待澄清)收进标题右侧的 doc-flags,
    // 不再插在优先级前面。它们有无与宽窄各不相同,插在前面就把优先级/复杂度/标题
    // 三列逐行推歪(实测 13 行出现 7 个不同的优先级横坐标,列表没法横向扫读)。
    // 挪到标题之后,三列起点只由固定宽度的勾选框/优先级/复杂度决定,行行一致;
    // 徽标自己在右端聚成一簇。侧栏不动——那里行窄、条目少,原地更紧凑(验收②)。
    const onDocsPage = surface === "documents";
    const flags = [];
    const placeFlag = (node) => (onDocsPage ? flags.push(node) : row.appendChild(node));
    // 批量操作只在文档页:侧栏一行要尽量轻,多一个勾选框就多一层视觉噪音。
    if (onDocsPage && (kind === "req" || kind === "defect") && !entry.closed) {
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
    } else if (onDocsPage && (kind === "req" || kind === "defect")) {
      // 已关闭条目没有勾选框:留一个等宽占位,否则这一行的三列整体左移一个框宽。
      const space = document.createElement("span");
      space.className = "doc-pick-space";
      space.setAttribute("aria-hidden", "true");
      row.appendChild(space);
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
    // D-413 续:研究来源要**一键直达正文**,不该先展开再在字段里找链接。
    // (且展开后若开着编辑器,字段走的是编辑输入而非只读链接——两个改动会互相抵消,
    //  见下方 researchLinkField 的 hasEditor 豁免。)行内 ↗ 是最短路径:点一下就
    //  在应用内看到这篇文献/这段代码。
    if (kind === "source" || kind === "finding") {
      const openable = (entry.fields ?? []).find(([k, v]) => researchLinkField(k, v));
      if (openable) {
        const open = document.createElement("button");
        open.className = "icon-btn doc-open-src";
        open.type = "button";
        open.textContent = "↗";
        open.title = t("在应用内打开");
        open.setAttribute("aria-label", `${t("在应用内打开")} ${entry.id}`);
        open.addEventListener("click", (event) => {
          event.stopPropagation();
          researchOpenLink(openable[0], String(openable[1])).click();
        });
        placeFlag(open);
      }
    }
    const claimed = claimedCollaborationLineFor(entry);
    if (claimed) {
      const claimBadge = document.createElement("span");
      claimBadge.className = "doc-claim-fact";
      claimBadge.textContent = `● ${claimed.code} ${t("被取得")}`;
      claimBadge.title = `${entry.id} · ${t("取得线")}: ${claimed.owner}${claimed.line?.phase ? ` · ${claimed.line.phase}` : ""}`;
      placeFlag(claimBadge);
    }
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
        placeFlag(meter);
      }
    }
    if (blocked || externalBlocked) {
      const blockedBadge = document.createElement("span");
      blockedBadge.className = "blocked-badge";
      blockedBadge.textContent = t("阻塞");
      blockedBadge.title = blockedReasons.length ? blockedReasons.join("；") : t("阻塞原因");
      placeFlag(blockedBadge);
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
        placeFlag(clarify);
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
    if (kind === "req" && surface === "documents") {
      const complexityBadge = document.createElement("span");
      complexityBadge.className = "complexity-badge";
      complexityBadge.textContent = cx === "小" || cx === "中" || cx === "大" ? `${t("复杂度")}:${t(cx)}` : t("未评估");
      row.appendChild(complexityBadge);
    }
    const title = document.createElement("span");
    title.className = "title";
    title.textContent = entry.title;
    row.appendChild(title);
    // D-362:文档页的可选徽标在标题之后成簇落地(标题 flex:1 会把它们顶到右端)。
    // 空数组不建容器,免得每行多一个空节点。
    if (flags.length) {
      const flagBox = document.createElement("span");
      flagBox.className = "doc-flags";
      for (const node of flags) flagBox.appendChild(node);
      row.appendChild(flagBox);
    }
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
    // R-123 的「字段编辑只在独立文档页」是针对 req/defect 说的——它们**有**文档页。
    // source/finding 压根没有文档页(index.html 无 view-sources/view-findings),
    // 沿用同一句话等于把研究工件的编辑入口锁死在一个不存在的地方:用户实测
    // 「打开也没法编辑」正是这么来的(D-413)。故按「有没有页」分流:有页的去页里改,
    // 没页的就在侧栏详情里改。研究工件是研究模式的核心资产,不能只读。
    const kindHasDocPage = kind === "req" || kind === "defect";
    const researchKind = kind === "source" || kind === "finding";
    const deepManage =
      !entry.closed && (researchKind || (surface === "documents" && kindHasDocPage));
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
      // D-413:可打开字段(URL / 证据锚)与 refs 一样,即使开着编辑器也要再给一份
      // **只读链接**。否则 deepManage 一开,这些字段只剩编辑输入框,「点开文献」
      // 的入口凭空消失——本轮实测就是这么栽的:两个改动各自正确,合起来互相抵消。
      const isOpenable = researchLinkField(key, value);
      if (hasEditor && !isRefs && !isOpenable) continue;
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
      } else if (researchLinkField(key, value)) {
        // D-413:研究工件的 `URL:`(文献)与 `证据锚:`(代码域)在数据里是结构化的,
        // 渲染成纯文本等于「看得见打不开」——用户实测的头号痛点。这里与 refs 同
        // 手法给一个真入口:文献进内置 viewer(用户定调,不跳出应用),代码域按
        // file:line 打开文件并定位行。
        f.append(`${key}: `);
        f.appendChild(researchOpenLink(key, String(value)));
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
    // 想法专属(R-252):inbox 显示「拆解」按钮(派 idea_split 子代理产出 R/D),
    // 已拆解(split)显示产出的 refs 编号。拆解由人点按钮触发,不做自动拆解。
    if (kind === "idea" && !entry.closed) {
      if (entry.status === "inbox") {
        const splitRow = document.createElement("div");
        splitRow.className = "doc-progress";
        const btn = document.createElement("button");
        btn.className = "ghost mini";
        btn.textContent = t("拆解成需求/缺陷");
        btn.title = t("派子代理把这条想法拆成 R-/D- 条目,拆解后显示产出编号");
        btn.addEventListener("click", async (e) => {
          e.stopPropagation();
          btn.disabled = true;
          try {
            const msg = await invoke("idea_split", { projectDir: currentProject, id: entry.id });
            log(msg);
            toast(msg);
            refreshDocs();
          } catch (err) {
            toastError(String(err));
            btn.disabled = false;
          }
        });
        splitRow.appendChild(btn);
        detail.appendChild(splitRow);
      } else {
        const refs = entry.fields?.find(([k]) => k === "refs")?.[1];
        if (refs) {
          const refsRow = document.createElement("div");
          refsRow.className = "doc-progress";
          refsRow.textContent = `${t("已拆解产出")}: ${refs}`;
          refsRow.title = t("这些 R-/D- 条目由本想法拆解而来");
          detail.appendChild(refsRow);
        }
      }
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
    let archiveLoaded = false;
    const renderArchiveEntries = (items) => {
      archive.replaceChildren();
      for (const entry of items) {
        const row = document.createElement("div");
        row.className = "archived-entry";
        // 归档条目也要挂 id:被引用的条目多半正是已经做完归档的那些,没有它跳转必然落空。
        row.dataset.docId = entry.id;
        row.textContent = `${entry.id} ${entry.title} [${entry.status}]`;
        archive.appendChild(row);
      }
      archiveLoaded = true;
    };
    const loadArchive = async () => {
      if (archiveLoaded) return;
      const items = await invoke("docs_archive_entries", { projectDir: currentProject, kind });
      renderArchiveEntries(items);
    };
    archiveLoaders.set(kind, { load: loadArchive });
    foot.addEventListener("click", () => {
      archive.classList.toggle("hidden");
      const expanded = !archive.classList.contains("hidden");
      foot.setAttribute("aria-expanded", String(expanded));
      foot.textContent = `${archivedCount} ${t("条")} ${t("已归档")} ${expanded ? "▾" : "▸"}`;
      if (expanded && !archiveLoaded) {
        archive.textContent = t("加载中…");
        loadArchive()
          .catch((error) => {
            archive.textContent = `${t("读取归档失败")}:${error}`;
          });
      }
    });
    foot.addEventListener("dblclick", () => openDocViewer(`${kind}-archive`));
    el.append(foot, archive);
  }
}
