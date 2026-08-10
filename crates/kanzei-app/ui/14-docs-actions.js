// await 前后各认一次项目。这份快照是替 forProject 取的:await 期间用户切走了,它就是
// **上一个项目**的数据,照旧渲染的后果不止"闪一下上个项目的列表"——syncDocumentFilters
// 会拿新项目的筛选去旧项目的条目里判「这标签还在不在」,判否就回落并落盘(D-169 那段),
// 而落盘走的是**新项目**的键:用户在新项目从没设过筛选,列表却永久少一批,重启也回不来。
// 挂起的跳转高亮不必在这条路径上作废:切项目必然跟着 selectWorkspaceProject 自己那次
// refreshDocs,由那次重绘消费(目标不在新项目的列表里就自然作罢),不会留成悬挂高亮。
async function refreshDocs() {
  if (!currentProject) return;
  const forProject = currentProject;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: forProject });
    if (currentProject !== forProject) return;
    renderDocsSnapshot(snapshot);
    await refreshConversationList();
    await refreshTests();
    await refreshWorktrees();
  } catch (err) {
    // 这次刷新没能走到 renderDocsSnapshot,跨视图跳转挂起的高亮就永远等不到人消费。
    // 留着它 = 悬挂高亮:之后任意一次无关刷新都会把它兑现,用户没点跳转条目却自己亮了。
    // 但只作废**属于自己那次刷新**的挂起跳转:成功路径上面已按项目收敛,失败路径必须对称
    // (D-250)。替旧项目发出的刷新若在用户切走之后才抛错,新项目刚排上的高亮不该被它连坐。
    if (currentProject === forProject) clearPendingJump();
    // 报错不设项目守卫:刷新确实失败了,与用户此刻停在哪个项目无关,该看见就得看见。
    toastError(`项目文档刷新失败:${err}`, { retry: refreshDocs });
  }
}

// agent 在运行中改需求/缺陷/目标时,侧栏必须跟着动:否则状态、计数和状态流转按钮
// 会一直停在开跑前的样子,要等本轮结束才更新(D-098)。合并 400ms 内的连续变更。
let docsLiveTimer = null;
function refreshDocsSoon() {
  if (!currentProject) return;
  clearTimeout(docsLiveTimer);
  docsLiveTimer = setTimeout(async () => {
    docsLiveTimer = null;
    // 重绘会清空列表容器:用户正在写快记或正在拖拽排序时先让路,稍后再刷。
    if (document.querySelector(".quickreq-form") || document.querySelector(".doc-item.dragging")) {
      refreshDocsSoon();
      return;
    }
    // 同 refreshDocs:await 前后各认一次项目。这条路径由 agent 的文档变更事件驱动,
    // 合并窗口本身就有 400ms,在飞的概率比手动刷新更高。
    const forProject = currentProject;
    try {
      const snapshot = await invoke("docs_snapshot", { projectDir: forProject });
      if (currentProject !== forProject) return;
      renderDocsSnapshot(snapshot);
    } catch (err) {
      // 同 refreshDocs:没重绘成 = 挂起的跳转高亮没人消费,不作废就会在下一次
      // 无关刷新上突然亮起来。同样只作废属于自己那次刷新的(D-250):这条路径由 agent 的
      // 文档变更事件驱动、自带 400ms 合并窗口,定时器落地时用户早就可能切走了,
      // 撞上的概率比 refreshDocs 更高,不能只在上面那处加守卫。
      if (currentProject === forProject) clearPendingJump();
      console.error(err);
    }
  }, 400);
}

$("documents-project-select").addEventListener("change", (event) => {
  if (event.target.value && event.target.value !== currentProject) selectWorkspaceProject(event.target.value);
});

$("documents-tab-req").addEventListener("click", () => { documentsKind = "req"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
$("documents-tab-defect").addEventListener("click", () => { documentsKind = "defect"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
// 测试记录已从侧栏搬到这里:切过去顺手刷一次,否则看到的是上次 refreshDocs 的旧快照。
$("documents-tab-tests").addEventListener("click", () => {
  documentsKind = "tests";
  if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot);
  refreshTests();
});
$("documents-tab-both").addEventListener("click", () => { documentsKind = "both"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
$("documents-dep-toggle").addEventListener("click", () => { dependencyViewOpen = !dependencyViewOpen; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
// 侧栏焦点卡片旁的「打开完整需求与缺陷列表」:列表不在侧栏之后的可发现性入口。
$("focus-open-documents").addEventListener("click", openDocumentsView);

async function runDefectReview() {
  if (!currentProject) {
    toast(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  const button = $("defect-review");
  const status = $("defect-review-status");
  button.disabled = true;
  status.textContent = t("正在审查缺陷…");
  try {
    const result = await invoke("defect_review", { projectDir: currentProject });
    if (result.empty) {
      status.textContent = t("当前没有活动缺陷");
      toast(t("当前没有活动缺陷"));
      return;
    }
    status.textContent = t("审查完成");
    openRuntimeMarkdown(t("缺陷自动审查报告"), result.report);
  } catch (err) {
    status.textContent = t("审查失败");
    toastError(`${t("审查失败")}:${err}`, { retry: runDefectReview });
  } finally {
    button.disabled = false;
  }
}
$("defect-review").addEventListener("click", runDefectReview);

$("bg-type-filter").addEventListener("change", (e) => {
  bgFilters.type = e.target.value;
  localStorage.setItem("kz-bg-type", bgFilters.type);
  applyBgFilters();
});
$("bg-status-filter").addEventListener("change", (e) => {
  bgFilters.status = e.target.value;
  localStorage.setItem("kz-bg-status", bgFilters.status);
  applyBgFilters();
});
$("bg-type-filter").value = bgFilters.type;
$("bg-status-filter").value = bgFilters.status;

$("documents-batch-apply").addEventListener("click", applyBatch);
$("documents-batch-clear").addEventListener("click", () => { batchSelection.clear(); if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
// 筛选写进 docFilterTargets() 给出的每个队列:对照模式下两边共用同一套条件。
// 但只写给**确实拥有该字段**的队列:缺陷没有复杂度/排序口径(documentFilters.defect
// 里根本没这两个键)。凭空写进去,锁提示的 `key in reqFilterState` 就会列出
// 「复杂度=大」,而 docDragEnabled 的缺陷分支只看 status/priority/tag/blocked——
// 提示说锁了、实际仍可拖,是 D-211 的反向脱节。
function applyDocFilter(field, value) {
  for (const kind of docFilterTargets()) {
    if (!(field in documentFilters[kind])) continue;
    documentFilters[kind][field] = value;
  }
  saveDocFilters();
  if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot);
}
// 交付方式(插入/排队)是个人习惯,全局记一份即可,不按项目分。
$("delivery-select").addEventListener("change", (event) => {
  localStorage.setItem("kz-delivery", event.target.value);
});
$("documents-status-filter").addEventListener("change", (e) => applyDocFilter("status", e.target.value));
$("documents-priority-filter").addEventListener("change", (e) => applyDocFilter("priority", e.target.value));
$("documents-complexity-filter").addEventListener("change", (e) => applyDocFilter("complexity", e.target.value));
$("documents-tag-filter").addEventListener("change", (e) => applyDocFilter("tag", e.target.value));
$("documents-blocked-filter").addEventListener("change", (e) => applyDocFilter("blocked", e.target.value));
// 排序走 applyDocFilter:它只改 documentFilters(显示口径),永远不会碰 docs_update(reorder)。
// 能改取活顺序的入口只有一个——手动排序下的拖拽,见 commitDocOrder。
$("documents-sort").addEventListener("change", (e) => applyDocFilter("sort", e.target.value));
// 分组开关(用户定调:按受控标签分组展示):关掉即回纯开发顺序+拖拽。
// 完整列表只剩单页视图一处,开关也只剩这一个。
function bindGroupToggle(id, storageKey, apply) {
  const btn = $(id);
  if (!btn) return;
  const sync = (on) => {
    btn.setAttribute("aria-pressed", String(on));
    btn.classList.toggle("active", on);
  };
  sync(apply(null));
  btn.addEventListener("click", () => {
    const on = apply("toggle");
    localStorage.setItem(storageKey, on ? "1" : "0");
    sync(on);
    if (latestDocsSnapshot) renderDocsSnapshot(latestDocsSnapshot);
  });
}
bindGroupToggle("documents-group-toggle", "kz-grouped-docs", (op) => {
  if (op === "toggle") {
    const next = !documentFilters.req.grouped;
    documentFilters.req.grouped = next;
    documentFilters.defect.grouped = next;
  }
  return documentFilters.req.grouped;
});
