async function refreshDocs() {
  if (!currentProject) return;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    renderDocsSnapshot(snapshot);
    await refreshConversationList();
    await refreshTests();
    await refreshWorktrees();
  } catch (err) {
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
    try {
      renderDocsSnapshot(await invoke("docs_snapshot", { projectDir: currentProject }));
    } catch (err) {
      console.error(err);
    }
  }, 400);
}

$("documents-project-select").addEventListener("change", (event) => {
  if (event.target.value && event.target.value !== currentProject) selectWorkspaceProject(event.target.value);
});

$("documents-tab-req").addEventListener("click", () => { documentsKind = "req"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
$("documents-tab-defect").addEventListener("click", () => { documentsKind = "defect"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
$("documents-tab-both").addEventListener("click", () => { documentsKind = "both"; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });
$("documents-dep-toggle").addEventListener("click", () => { dependencyViewOpen = !dependencyViewOpen; if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot); });

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
function applyDocFilter(field, value) {
  for (const kind of docFilterTargets()) documentFilters[kind][field] = value;
  saveDocFilters();
  if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot);
}
// 交付方式(插入/排队)是个人习惯,全局记一份即可,不按项目分。
$("delivery-select").addEventListener("change", (event) => {
  localStorage.setItem("kz-delivery", event.target.value);
});
$("documents-status-filter").addEventListener("change", (e) => applyDocFilter("status", e.target.value));
$("documents-priority-filter").addEventListener("change", (e) => applyDocFilter("priority", e.target.value));
$("documents-tag-filter").addEventListener("change", (e) => applyDocFilter("tag", e.target.value));
$("documents-blocked-filter").addEventListener("change", (e) => applyDocFilter("blocked", e.target.value));
// 分组开关(用户定调:按受控标签分组展示,含侧边栏):关掉即回纯开发顺序+拖拽。
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
bindGroupToggle("req-group-toggle", "kz-grouped-req", (op) => {
  if (op === "toggle") reqFilters.grouped = !reqFilters.grouped;
  return reqFilters.grouped;
});
bindGroupToggle("defect-group-toggle", "kz-grouped-defect", (op) => {
  if (op === "toggle") defectFilters.grouped = !defectFilters.grouped;
  return defectFilters.grouped;
});
bindGroupToggle("documents-group-toggle", "kz-grouped-docs", (op) => {
  if (op === "toggle") {
    const next = !documentFilters.req.grouped;
    documentFilters.req.grouped = next;
    documentFilters.defect.grouped = next;
  }
  return documentFilters.req.grouped;
});
// 筛选折叠(用户定调:侧边栏筛选再收一层):默认收起,状态持久化。
for (const [btnId, rowId, storageKey] of [
  ["req-filter-toggle", "req-filter-row", "kz-filters-req"],
  ["defect-filter-toggle", "defect-filter-row", "kz-filters-defect"],
]) {
  const btn = $(btnId);
  const row = $(rowId);
  if (!btn || !row) continue;
  const apply = (open) => {
    row.classList.toggle("hidden", !open);
    btn.setAttribute("aria-expanded", String(open));
    btn.classList.toggle("active", open);
  };
  apply(localStorage.getItem(storageKey) === "1");
  btn.addEventListener("click", () => {
    const open = row.classList.contains("hidden");
    localStorage.setItem(storageKey, open ? "1" : "0");
    apply(open);
  });
}
for (const [id, key] of [["req-status-filter", "status"], ["req-priority-filter", "priority"], ["req-complexity-filter", "complexity"], ["req-tag-filter", "tag"], ["req-blocked-filter", "blocked"], ["req-sort", "sort"]]) {
  $(id).addEventListener("change", (event) => {
    reqFilters[key] = event.target.value;
    if (key === "sort") localStorage.setItem("kz-req-sort", event.target.value);
    saveDocFilters();
    refreshDocs();
  });
}
$("defect-tag-filter").addEventListener("change", (event) => {
  defectFilters.tag = event.target.value;
  saveDocFilters();
  refreshDocs();
});
$("defect-blocked-filter").addEventListener("change", (event) => {
  defectFilters.blocked = event.target.value;
  saveDocFilters();
  refreshDocs();
});
