// ---------- 侧边栏文档(可展开 + 状态流转) ----------
const reqFilters = { status: "all", priority: "all", complexity: "all", tag: "all", blocked: "all", sort: localStorage.getItem("kz-req-sort") || "manual", grouped: localStorage.getItem("kz-grouped-req") !== "0" };
const defectFilters = { status: "all", priority: "all", tag: "all", blocked: "all", grouped: localStorage.getItem("kz-grouped-defect") !== "0" };

// R-115:筛选条件按项目持久化。此前只有 sort 与 grouped 落盘,状态/优先级/复杂度/
// 标签/阻塞五项每次重启都回"全部"——盯着某一类条目做事时,重开一次全白设。
// 只回填已知字段:localStorage 里的旧结构或手改内容不该污染筛选状态。
const FILTER_FIELDS = {
  req: ["status", "priority", "complexity", "tag", "blocked", "sort"],
  defect: ["status", "priority", "tag", "blocked"],
};
function saveDocFilters() {
  const pick = (state, fields) => Object.fromEntries(fields.map((f) => [f, state[f]]));
  writeJson(prefKey("filters"), {
    req: pick(reqFilters, FILTER_FIELDS.req),
    defect: pick(defectFilters, FILTER_FIELDS.defect),
    docReq: pick(documentFilters.req, FILTER_FIELDS.req),
    docDefect: pick(documentFilters.defect, FILTER_FIELDS.defect),
  });
}
function restoreDocFilters() {
  const saved = readJson(prefKey("filters"), {});
  const apply = (state, data, fields) => {
    if (!data || typeof data !== "object") return;
    for (const field of fields) {
      if (typeof data[field] === "string") state[field] = data[field];
    }
  };
  apply(reqFilters, saved.req, FILTER_FIELDS.req);
  apply(defectFilters, saved.defect, FILTER_FIELDS.defect);
  apply(documentFilters.req, saved.docReq, FILTER_FIELDS.req);
  apply(documentFilters.defect, saved.docDefect, FILTER_FIELDS.defect);
  syncDocFilterControls();
}
// 状态回填到控件上,否则下拉显示"全部"而实际在筛选,看起来就像列表丢了条目。
function syncDocFilterControls() {
  const pairs = [
    ["req-status-filter", reqFilters.status],
    ["req-complexity-filter", reqFilters.complexity],
    ["req-priority-filter", reqFilters.priority],
    ["req-blocked-filter", reqFilters.blocked],
    ["req-sort", reqFilters.sort],
    ["defect-blocked-filter", defectFilters.blocked],
  ];
  for (const [id, value] of pairs) {
    const el = $(id);
    if (el && [...el.options].some((o) => o.value === value)) el.value = value;
  }
}
// 标签受控词表(conventions §1.35,用户定调):分组顺序即展示顺序。
const DOC_TAG_ORDER = ["核心", "后端", "前端", "模型", "发布", "流程"];
function docGroupTag(entry) {
  const tags = entryTags(entry);
  return DOC_TAG_ORDER.find((tag) => tags.includes(tag)) || "其他";
}
const priorityRank = { P0: 0, P1: 1, P2: 2, P3: 3 };
const statusRank = { doing: 0, todo: 1, done: 2, dropped: 3 };
const complexityRank = { "小": 0, "中": 1, "大": 2 };
function entryTags(entry) {
  const field = (entry.fields ?? []).find(([key]) => ["标签", "tags", "tag"].includes(String(key).toLowerCase()));
  return String(field?.[1] || "").split(/[\s,]+/).map((tag) => tag.trim()).filter(Boolean);
}
function tagOptions(entries) {
  // 受控词表优先,词表外的存量标签跟在后面(过渡期可见,便于归一)。
  const seen = new Set(entries.flatMap(entryTags));
  const extras = [...seen].filter((tag) => !DOC_TAG_ORDER.includes(tag)).sort((a, b) => a.localeCompare(b));
  return [...DOC_TAG_ORDER.filter((tag) => seen.has(tag)), ...extras];
}
// 返回实际生效的值:保存的标签在当前项目里可能根本不存在,那时下拉会回落成
// "全部",但**筛选状态必须跟着回落**——否则状态里还留着那个标签,列表被筛空,
// 而界面显示"没有筛选",看起来就是"条目凭空掉了"(D-169)。
function syncTagFilter(select, entries, selected = "all") {
  select.replaceChildren(new Option(localizeDynamic("全部标签"), "all"));
  for (const tag of tagOptions(entries)) select.appendChild(new Option(localizeDynamic(tag), tag));
  select.value = selectedOptions(select, selected);
  return select.value;
}
function selectedOptions(select, selected) {
  return [...select.options].some((option) => option.value === selected) ? selected : "all";
}

function entryBlocked(entry) {
  return Boolean(entry?.blocked);
}
function matchesBlockedFilter(entry, value) {
  return value === "all" || (value === "blocked" ? entryBlocked(entry) : !entryBlocked(entry));
}
function filterRequirements(entries, filters = reqFilters) {
  const filtered = entries
    .filter((entry) => filters.status === "all" || entry.status === filters.status)
    .filter((entry) => filters.priority === "all" || entry.priority === filters.priority)
    .filter((entry) => filters.tag === "all" || entryTags(entry).includes(filters.tag))
    .filter((entry) => matchesBlockedFilter(entry, filters.blocked ?? "all"));
  const complexityValue = (entry) => entry.complexity || "unassessed";
  const complexityFiltered = filtered.filter((entry) => filters.complexity === "all" || complexityValue(entry) === filters.complexity);
  // 手动模式(R-054 默认):文件顺序即开发顺序,不做任何排序。
  if (filters.sort === "manual") return complexityFiltered;
  return complexityFiltered.sort((a, b) => {
    if (filters.sort === "id") return a.id.localeCompare(b.id, undefined, { numeric: true });
    if (filters.sort === "complexity") return (complexityRank[complexityValue(a)] ?? 99) - (complexityRank[complexityValue(b)] ?? 99) || a.id.localeCompare(b.id, undefined, { numeric: true });
    if (filters.sort === "status") {
      return (statusRank[a.status] ?? 99) - (statusRank[b.status] ?? 99) || a.id.localeCompare(b.id, undefined, { numeric: true });
    }
    return (priorityRank[a.priority] ?? 99) - (priorityRank[b.priority] ?? 99)
      || (statusRank[a.status] ?? 99) - (statusRank[b.status] ?? 99)
      || a.id.localeCompare(b.id, undefined, { numeric: true });
  });
}
