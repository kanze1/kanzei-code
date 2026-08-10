// ---------- 文档条目(可展开 + 状态流转) ----------
// 完整需求/缺陷列表已整体收进单页视图(用户定调:侧栏只显示当前在做),侧栏不再持有
// 一份筛选状态——两份状态各自漂移的话,同一条件下两处会给出不同条目集合。
// goal/source/finding 三类列表本来就没有筛选控件,统一用这份中性口径当占位;
// 冻结是为了防止有人顺手往共享占位里写值,把三个列表一起搞坏。
const NEUTRAL_DOC_FILTERS = Object.freeze({
  status: "all", priority: "all", complexity: "all", tag: "all", blocked: "all", sort: "manual", grouped: false,
});

// R-115:筛选条件按项目持久化。此前只有 sort 与 grouped 落盘,状态/优先级/复杂度/
// 标签/阻塞五项每次重启都回"全部"——盯着某一类条目做事时,重开一次全白设。
// 只回填已知字段:localStorage 里的旧结构或手改内容不该污染筛选状态。
// 字段清单直接取自 DOC_FILTER_DEFAULTS(12-docs-pages.js)的键,不另抄一份:清单与默认值
// 一旦分成两处,漏加一个字段就成了"存得下、复位不掉"。惰性求值是因为 10-docs-core.js
// 先于 12-docs-pages.js 执行,顶层直接读那个 const 会撞 TDZ。
const filterFields = (kind) => Object.keys(DOC_FILTER_DEFAULTS[kind]);
function saveDocFilters() {
  const pick = (state, kind) => Object.fromEntries(filterFields(kind).map((f) => [f, state[f]]));
  writeJson(prefKey("filters"), {
    docReq: pick(documentFilters.req, "req"),
    docDefect: pick(documentFilters.defect, "defect"),
  });
}
function restoreDocFilters() {
  const saved = readJson(prefKey("filters"), {});
  // 先复位再叠加,顺序不能反:documentFilters 是模块级状态,换项目不会重建它。只叠加
  // "保存里存在的字段"的话,新项目没保存过的字段仍挂着上一个项目的值;而 syncDocumentFilters
  // 里 D-169 的标签回落一触发就 saveDocFilters(),把上一个项目的**整套**口径写进新项目的键
  // ——用户在新项目从没设过,列表却少了一批,重启也回不来。
  // 复位只覆盖按项目持久化的那些字段(DOC_FILTER_DEFAULTS 的键);grouped 按 kz-grouped-docs
  // 全局记,不随项目走,不能被这里清掉。
  const apply = (state, data, kind) => {
    Object.assign(state, DOC_FILTER_DEFAULTS[kind]);
    if (!data || typeof data !== "object") return;
    for (const field of filterFields(kind)) {
      if (typeof data[field] === "string") state[field] = data[field];
    }
  };
  // 旧结构降级读取:侧栏筛选退休前,偏好里可能只有 req/defect 两支(R-115 之前写入的
  // 更是只有前者)。直接换键会让用户重开一次全白设——正是 R-115 当初要治的病。
  apply(documentFilters.req, saved.docReq ?? saved.req, "req");
  apply(documentFilters.defect, saved.docDefect ?? saved.defect, "defect");
  syncDocFilterControls();
}
// 状态回填到控件上,否则下拉显示"全部"而实际在筛选,看起来就像列表丢了条目。
// 控件只有单页视图那一套;当前标签页是哪一队,就回填哪一队的值(tests 页没有筛选口径,
// docFilterTargets() 返回空,回落到需求那支即可,反正控件此时是禁用的)。
function syncDocFilterControls() {
  const primary = documentFilters[docFilterTargets()[0]] ?? documentFilters.req;
  const pairs = [
    ["documents-status-filter", primary.status],
    ["documents-complexity-filter", primary.complexity ?? "all"],
    ["documents-priority-filter", primary.priority],
    ["documents-blocked-filter", primary.blocked],
    ["documents-sort", primary.sort ?? "manual"],
    ["documents-tag-filter", primary.tag],
  ];
  for (const [id, value] of pairs) {
    const el = $(id);
    if (el && [...el.options].some((o) => o.value === value)) el.value = value;
  }
}
// 跨视图跳转:复用导航栏按钮的既有切换逻辑(含 active/aria-current/documents-active
// 与该视图的数据刷新),不重复实现一套视图激活。
function openDocumentsView() {
  const btn = document.querySelector('.activity-item[data-view="documents"]');
  if (btn) {
    btn.click();
    return;
  }
  document.querySelectorAll(".view").forEach((view) => view.classList.remove("active"));
  $("view-documents")?.classList.add("active");
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
function filterRequirements(entries, filters = NEUTRAL_DOC_FILTERS) {
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
