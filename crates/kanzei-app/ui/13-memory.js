import { defer } from "./01-core.js";
import { escapeHtml, renderMarkdownInto } from "./04-markdown.js";
import { $, confirmDialog, invoke, isImeComposing, on, replayExperienceFacts, uiConsoleLog } from "./01-core.js";
import { languageIsEnglish, localizedDocStatus, t } from "./02-i18n.js";
import { activeProcessId, currentProject, navigate_view, processItems, toast, toastError } from "./03-shell.js";
import { neuralFlowEmit } from "./22-neural-flow.js";
import { richText } from "./04-structured.js";
import { fillTemplate } from "./04-structured-parse.js";
import { switchProcess } from "./09-sessions.js";

// ---------- 记忆页(R-107/R-332):管理工作区 + 透明化诊断 ----------
export let memorySelection = { scope: "project", category: "all" };
let memoryCurrentEntryId = null;
let memoryListEntries = [];
let memoryRefreshGeneration = 0;
let memoryListGeneration = 0;
let memoryRenderedProject = null;
const memoryEntryCache = new Map();
let diagnosticsProject = null;
let diagnosticsPending = null;
const memoryManagerFilters = { scope: "project", category: "all", status: "active", sort: "updated" };
// 列表正显示的搜索词(空 = 普通列表)。搜索结果态下点条目只开详情,不重绘列表、不改筛选。
let memorySearchQuery = "";
// 待整理笔记(收件箱草稿)与最近一次整理批次:顶栏「全部整理」、「待整理」页签角标、整理失败提示共用。
let memoryPending = { project: null, notes: [], control: null };
const memoryArchivedCache = new Map();
const MEMORY_SEARCH_LIMIT = 30;
const MEMORY_FILTER_KEYS = { "memory-scope-filter": "scope", "memory-category-filter": "category", "memory-status-filter": "status", "memory-sort-filter": "sort" };

// 状态/分类枚举 → 展示词(04-status-words.js 那一张表);option value、CSS 类与回传后端的参数仍是原始枚举。
const memoryWord = (value) => localizedDocStatus(value);
// 条目「来源」是自由文本(「inbox note 2026-08-07」「memory-manager」「user」…):把里面的内部词换成展示词,日期/编号原样留着。
const MEMORY_SOURCE_WORDS = [
  [/\binbox note\b/g, "待整理", "Pending"], [/\binbox\b/g, "待整理", "Pending"], [/\bmemory-manager\b/g, "记忆管理子代理", "Memory subagent"],
  [/\bquarantine\b/g, "隔离区", "Quarantine"], [/\bmigration\b/g, "迁移", "Migration"], [/^user\b/, "用户", "User"],
];
function memorySourceText(source) {
  const english = languageIsEnglish();
  let text = String(source ?? "");
  for (const [pattern, zh, en] of MEMORY_SOURCE_WORDS) text = text.replace(pattern, () => (english ? en : zh));
  return text;
}

function memoryFilterDefinitions() {
  return [
    ["memory-scope-filter", [["project", t("项目记忆")], ["global", t("全局记忆")], ["all", t("全部")]]],
    ["memory-category-filter", [["all", t("全部分类")], ...["fact", "sop", "habit", "preference"].map((category) => [category, memoryWord(category)])]],
    ["memory-status-filter", [["active", memoryWord("active")], ["candidate", memoryWord("candidate")], ["shadow", memoryWord("shadow")], ["archived", memoryWord("archived")], ["all", t("全部(未归档)")]]],
    ["memory-sort-filter", [["updated", t("最近更新")], ["hits", t("命中最多")], ["title", t("标题")], ["id", t("ID")]]],
  ];
}

function setupMemoryManagerFilters() {
  for (const [id, options] of memoryFilterDefinitions()) {
    const select = $(id);
    if (!select || select.options.length) continue;
    const key = MEMORY_FILTER_KEYS[id];
    for (const [value, label] of options) {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = label;
      select.appendChild(option);
    }
    select.value = memoryManagerFilters[key];
    select.addEventListener("change", () => void onMemoryFilterChange(select, key));
  }
}

// 切换语言后选项文案跟着换(选项只在首次建一次,不重算就会一直停在建时的语言)。
function relabelMemoryManagerFilters() {
  for (const [id, options] of memoryFilterDefinitions()) {
    const select = $(id);
    if (!select) continue;
    for (const [value, label] of options) {
      const option = [...select.options].find((item) => item.value === value);
      if (option) option.textContent = label;
    }
  }
}

async function onMemoryFilterChange(select, key) {
  const previous = memoryManagerFilters[key];
  if (key === "sort") {
    memoryManagerFilters.sort = select.value;
    // 搜索结果按相关度排,不跟排序走;普通列表重排时保住选中与滚动位置。
    if (!memorySearchQuery) void loadMemoryList(memoryManagerFilters.scope, memoryManagerFilters.category, { preserveSelection: true });
    return;
  }
  // 范围/分类/状态变了,当前详情就不再属于这份列表:先确认没有未保存的修改(取消就把下拉还原)。
  if (!(await confirmLeaveMemoryDetail())) {
    select.value = previous;
    return;
  }
  memoryManagerFilters[key] = select.value;
  clearMemorySearch();
  memorySelection = { scope: memoryManagerFilters.scope, category: memoryManagerFilters.category };
  memoryCurrentEntryId = null;
  hideMemoryDetail();
  loadMemoryList(memoryManagerFilters.scope, memoryManagerFilters.category);
  // 记忆图谱(24-memory-graph.js)跟着同一组筛选重算可见子图。
  document.dispatchEvent(new CustomEvent("kz:memory-filters", { detail: { project: currentProject, filters: memoryFilterSnapshot() } }));
}

/** 详情里有未保存的修改时,离开(换条目/关详情/换筛选)前确认;没改动或确认放弃返回 true。 */
export function memoryDetailDirty() {
  const box = $("memory-detail");
  if (box?.dataset.dirty !== "true") return false;
  // 输入过但又改回原样不算改动:以表单当前值对原值的比较为准(没有比较函数就信输入标记)。
  return typeof box._memoryDirtyCheck === "function" ? Boolean(box._memoryDirtyCheck()) : true;
}

export async function confirmLeaveMemoryDetail() {
  if (!memoryDetailDirty()) return true;
  const box = $("memory-detail");
  const title = box?.querySelector(".memory-detail-title")?.textContent?.trim();
  const ok = await confirmDialog({
    title: t("放弃未保存的修改?"),
    message: `${title ? `「${title}」` : ""}${t("有改动还没保存，离开后会丢失。")}`,
    okText: t("放弃修改"),
    danger: true,
  });
  if (ok && box) delete box.dataset.dirty;
  return Boolean(ok);
}

/** 退出搜索态(筛选变了、清除搜索):清空输入框与命中高亮,不动列表(调用方接着重载)。 */
function clearMemorySearch() {
  if (!memorySearchQuery && !$("memory-search-input")?.value) return;
  memorySearchQuery = "";
  const input = $("memory-search-input");
  if (input) input.value = "";
  const clear = $("memory-search-clear");
  if (clear) clear.hidden = true;
  document.dispatchEvent(new CustomEvent("kz:memory-search-hits", { detail: { project: currentProject, ids: [], query: "" } }));
}

/** 当前 范围/分类/状态/排序 筛选的快照(记忆图谱与列表共用同一组筛选)。 */
export function memoryFilterSnapshot() {
  return { ...memoryManagerFilters };
}

function syncMemoryManagerFilters() {
  setupMemoryManagerFilters();
  for (const [id, value] of [["memory-scope-filter", memoryManagerFilters.scope], ["memory-category-filter", memoryManagerFilters.category], ["memory-status-filter", memoryManagerFilters.status], ["memory-sort-filter", memoryManagerFilters.sort]]) {
    const select = $(id);
    if (select) select.value = value;
  }
}

export function hideMemoryDetail() {
  const box = $("memory-detail");
  if (box) {
    box.classList.add("hidden");
    box.replaceChildren();
    delete box.dataset.graphNode;
    delete box.dataset.dirty;
    delete box.dataset.readonly;
    delete box.dataset.memoryId;
  }
  $("memory-reader-empty")?.classList.remove("hidden");
  $("memory-scroll")?.classList.remove("has-memory-selection");
  document.dispatchEvent(new CustomEvent("kz:memory-selection-cleared", { detail: { project: currentProject } }));
}

/** 记忆图谱点了非记忆节点:放掉当前选中的记忆,详情栏交给图谱画节点详情。 */
export function releaseMemoryDetail() {
  memoryCurrentEntryId = null;
  hideMemoryDetail();
  document.querySelectorAll("#memory-list .memory-row.selected").forEach((row) => row.classList.remove("selected"));
}

// 记忆图谱注册的区域提供者:{ areasFor(scope, id) → [{ area, provenance, via }] | null, ensureAreaOptions() → Promise<[{ id, label, group }]> }。
// 列表模式下图谱没加载过也能设区域:ensureAreaOptions 会按需取一次图谱载荷(后端有 stat 缓存)。
let memoryAreaProvider = null;
export function setMemoryAreaProvider(provider) {
  memoryAreaProvider = provider;
}

export async function refreshMemory({ force = false } = {}) {
  const project = currentProject;
  const generation = ++memoryRefreshGeneration;
  if (project !== memoryRenderedProject) {
    memoryRenderedProject = project;
    memoryCurrentEntryId = null;
    memoryListEntries = [];
    memoryEntryCache.clear();
    memoryArchivedCache.clear();
    memoryPending = { project: null, notes: [], control: null };
    ++memoryPendingGeneration;
    dismissMemoryNotice();
    diagnosticsProject = null;
    diagnosticsPending = null;
    ++memoryListGeneration;
    clearMemorySearch();
    hideMemoryDetail();
    if (project) $("memory-reader-empty").innerHTML = `<strong>${t("选择一条记忆")}</strong><p>${t("在这里阅读全文，或通过管理对话修改。")}</p>`;
    document.dispatchEvent(new CustomEvent("kz:memory-project", { detail: { project } }));
    for (const id of ["memory-recalls", "memory-list", "memory-candidates", "memory-value-flags", "memory-arch"]) $(id)?.replaceChildren();
    renderMemoryPendingState();
  }
  syncMemoryNoProject();
  if (!currentProject) return;
  try {
    setupMemoryManagerFilters();
    if (force) { memoryEntryCache.clear(); memoryArchivedCache.clear(); diagnosticsProject = null; diagnosticsPending = null; }
    await loadMemoryList(memoryManagerFilters.scope, memoryManagerFilters.category, { preserveSelection: true });
    if (project !== currentProject || generation !== memoryRefreshGeneration) return;
    // 待整理笔记数与整理失败提示不挂在「使用记录」页签后面:列表一出来就取,顶栏与页签角标才不必等用户去翻。
    void loadMemoryPending();
    neuralFlowEmit?.("memory_snapshot", { memory_count: memoryListEntries.length });
    if (!$("memory-insights")?.classList.contains("hidden")) void loadMemoryDiagnostics();
  } catch (err) {
    if (project !== currentProject || generation !== memoryRefreshGeneration) return;
    toastError(`${t("记忆页加载失败")}:${err}`, { retry: refreshMemory });
  }
}

// 诊断只在用户打开时读取；列表不再等待 6 组磁盘/数据库查询。
async function loadMemoryDiagnostics() {
  const project = currentProject;
  if (!project || diagnosticsProject === project) return;
  if (diagnosticsPending?.project === project) return diagnosticsPending.promise;
  const request = { project };
  request.promise = (async () => {
    const jobs = [
      ["memory_overview", {}, renderMemoryArch],
      ["memory_recalls", { limit: 20 }, renderMemoryRecalls],
      ["memory_value_flags", {}, renderMemoryValueFlags],
    ];
    const results = await Promise.allSettled(jobs.map(async ([command, args, render]) => {
      const data = await invoke(command, { projectDir: project, ...args });
      if (project === currentProject && diagnosticsPending === request) render(data);
    }));
    if (project !== currentProject || diagnosticsPending !== request) return;
    const errors = results.filter(result => result.status === "rejected");
    if (errors.length) toastError(`${t("记忆页加载失败")}: ${errors.map(result => result.reason).join("; ")}`, { retry: loadMemoryDiagnostics });
    else diagnosticsProject = project;
  })().finally(() => { if (diagnosticsPending === request) diagnosticsPending = null; });
  diagnosticsPending = request;
  return request.promise;
}

export async function getMemoryEntries(project, scope) {
  const key = `${project}\0${scope}`;
  const cached = memoryEntryCache.get(key);
  if (cached && (cached.pending || Date.now() - cached.at < 15000)) return cached.promise;
  const entry = { at: Date.now(), pending: true };
  entry.promise = invoke("memory_entries", { projectDir: project, scope, category: null })
    .then(rows => { entry.pending = false; entry.at = Date.now(); return rows || []; })
    .catch(error => { if (memoryEntryCache.get(key) === entry) memoryEntryCache.delete(key); throw error; });
  memoryEntryCache.set(key, entry);
  return entry.promise;
}

const MEMORY_TABS = ["read", "chat", "pending", "insights"];

export function showMemoryTab(tab) {
  $("memory-scroll").dataset.tab = tab;
  for (const name of MEMORY_TABS) {
    const active = name === tab;
    $(`memory-${name}-tab`)?.setAttribute("aria-selected", String(active));
    if ($(`memory-${name}-tab`)) $(`memory-${name}-tab`).tabIndex = active ? 0 : -1;
    $(`memory-${name === "read" ? "reader" : name}`)?.classList.toggle("hidden", !active);
  }
  if (tab === "insights") void loadMemoryDiagnostics();
  if (tab === "pending") void loadMemoryPending();
  if (tab === "chat") document.dispatchEvent(new CustomEvent("kz:memory-chat-open"));
  document.dispatchEvent(new CustomEvent("kz:memory-tab", { detail: { tab } }));
}

// 没有项目时(研究档里常见):整页换成可行动的空态,不留一排没有选项的下拉框和指向不存在的「项目」的提示。
function syncMemoryNoProject() {
  const none = !currentProject;
  $("memory-scroll")?.classList.toggle("no-project", none);
  $("memory-no-project")?.classList.toggle("hidden", !none);
  if (!none) return;
  const inResearch = Boolean(document.querySelector('[data-workspace="research"][aria-pressed="true"]'));
  $("memory-empty-dev")?.classList.toggle("hidden", !inResearch);
}

// ---------- 待整理笔记、整理失败与撤销提示 ----------
let memoryPendingGeneration = 0;
let memoryConsolidating = false;
// 临时提示 { text, label, run, timer }:归档后的「撤销」等。整理失败的提示是常驻的(取自最近一次整理批次)。
let memoryNotice = null;
let memoryFailureDismissed = "";

async function loadMemoryPending() {
  const project = currentProject;
  if (!project) return;
  const generation = ++memoryPendingGeneration;
  const [notes, control] = await Promise.allSettled([
    invoke("memory_note_candidates", { projectDir: project }),
    invoke("memory_control_plane", { projectDir: project }),
  ]);
  if (project !== currentProject || generation !== memoryPendingGeneration) return;
  memoryPending = {
    project,
    notes: notes.status === "fulfilled" && Array.isArray(notes.value) ? notes.value : [],
    control: control.status === "fulfilled" ? control.value : null,
  };
  if (control.status === "fulfilled") replayExperienceFacts(control.value?.experience_facts);
  renderMemoryCandidates(memoryPending.notes);
  renderMemoryPendingState();
  if (notes.status === "rejected") toastError(`${t("记忆页加载失败")}:${notes.reason}`, { retry: loadMemoryPending });
}

// 待整理数同时写在页签角标和顶栏「全部整理」上;没有可整理的笔记时按钮置灰,不再让人点一个空转的 LLM 调用。
function renderMemoryPendingState() {
  const loaded = memoryPending.project === currentProject;
  const count = loaded ? memoryPending.notes.length : 0;
  const tabCount = $("memory-pending-count");
  if (tabCount) {
    tabCount.textContent = count ? String(count) : "";
    tabCount.hidden = !count;
  }
  const buttonCount = $("memory-consolidate-count");
  if (buttonCount) buttonCount.textContent = memoryConsolidating ? ` ${t("整理中…")}` : count ? ` (${count})` : "";
  const button = $("memory-consolidate-btn");
  if (button) button.disabled = !currentProject || memoryConsolidating || (loaded && !count);
  syncMemoryNotice();
}

function showMemoryNotice(text, { label = "", run = null, ttl = 12000, kind = "info" } = {}) {
  clearTimeout(memoryNotice?.timer);
  memoryNotice = { text, label, run, kind, timer: ttl ? setTimeout(() => { memoryNotice = null; syncMemoryNotice(); }, ttl) : 0 };
  syncMemoryNotice();
}

function dismissMemoryNotice() {
  clearTimeout(memoryNotice?.timer);
  memoryNotice = null;
  syncMemoryNotice();
}

function memoryFailureNotice() {
  const batch = memoryPending.project === currentProject ? memoryPending.control?.batch : null;
  if (!batch || batch.status !== "failed" || batch.batch_id === memoryFailureDismissed) return null;
  const reason = batch.failure_reason ? `:${batch.failure_reason}` : "";
  return { text: `${t("上次整理没有成功")}${reason}`, label: t("重试整理"), run: () => $("memory-consolidate-btn")?.click(), kind: "err", batchId: batch.batch_id };
}

function syncMemoryNotice() {
  const box = $("memory-notice");
  if (!box) return;
  const notice = memoryNotice ?? memoryFailureNotice();
  box.classList.toggle("hidden", !notice);
  if (!notice) return;
  box.dataset.kind = notice.kind || "info";
  $("memory-notice-text").textContent = notice.text;
  const action = $("memory-notice-action");
  action.classList.toggle("hidden", !notice.run);
  action.textContent = notice.label || "";
  action.onclick = notice.run ? () => { dismissMemoryNotice(); notice.run(); } : null;
  $("memory-notice-close").onclick = () => {
    if (!memoryNotice && notice.batchId) memoryFailureDismissed = notice.batchId;
    dismissMemoryNotice();
  };
}


// ---------- 运行画像页(R-127) ----------
// 判断 agent 跑得好不好此前全靠翻轨迹。数据源早就有(RunSummary 的 context_report、
// summarize_tools、summarize_metrics),缺的只是把它们汇到一处。
// run_metrics 聚合项目下全部对话(主对话、讨论、独立任务),每轮带 sessionId,点一轮就跳到它所在的对话。
// 版面:上面是「近 N 轮均值 + 迷你柱」(一根柱一轮,左旧右新),下面是逐轮列表。
// 均值口径:只统计有度量的轮次。早于度量落地的轮次 metrics 是空对象,把它们算成 0 会把均值整体
// 压低,得出「冗余在下降」的假结论——所以它们既不进均值也不画柱,列表里单独写明。
const METRICS_ROUNDS = 20;
let metricsGeneration = 0;
let metricsRenderedProject = null;

const metricsNum = (value) => Number(value) || 0;
const metricsMean = (values) => (values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : 0);
const metricsSum = (rows, pick) => rows.reduce((sum, row) => sum + metricsNum(pick(row)), 0);
const metricsOne = (value) => value.toFixed(1);
const metricsPercent = (value) => `${Math.round(value * 100)}%`;
const metricsCount = (value) => Math.round(value).toLocaleString();

/// 迷你柱图的口径(每次渲染现建:文案要跟着语言走)。value 取单轮的数,mean 求有度量轮次的均值,
/// show 把数写成字,sub 给均值下面补一行说明(可省),bad 标「越高越糟」的失败类(柱子用失败红)。
function metricTiles() {
  return [
    { id: "steps", label: t("步数"), hint: t("每轮里模型与工具往返的步数。"), value: (r) => metricsNum(r.steps), show: metricsOne },
    { id: "terminal", label: t("终端调用"), hint: t("每轮调用 bash / 终端类工具的次数。"), value: (r) => metricsNum(r.metrics.terminal_calls), show: metricsOne },
    {
      id: "git", label: t("git 查询组"),
      hint: t("连续的 git 查询并成一组:次数相同,分散在各处比挤成一组糟得多。"),
      value: (r) => metricsNum(r.metrics.git_groups), show: metricsOne,
    },
    {
      id: "edit", label: t("改文件未命中率"), bad: true,
      hint: t("edit 调用里没找到要改的原文的比例,按总次数算,不是各轮比例的平均。"),
      value: (r) => (metricsNum(r.metrics.edit_calls) ? metricsNum(r.metrics.edit_misses) / metricsNum(r.metrics.edit_calls) : 0),
      mean: (rows) => {
        const calls = metricsSum(rows, (r) => r.metrics.edit_calls);
        return calls ? metricsSum(rows, (r) => r.metrics.edit_misses) / calls : 0;
      },
      show: metricsPercent,
      sub: (rows) => fillTemplate(t("共 {n} 次 edit"), { n: metricsSum(rows, (r) => r.metrics.edit_calls) }),
    },
    {
      id: "failed", label: t("失败调用"), bad: true,
      hint: t("工具调用返回错误的次数;被工具按设计拒绝的不算。"),
      value: (r) => metricsNum(r.metrics.failed_calls), show: metricsOne,
      sub: (rows) => {
        const total = metricsSum(rows, (r) => r.metrics.total_calls);
        return total ? fillTemplate(t("占全部调用 {rate}"), { rate: metricsPercent(metricsSum(rows, (r) => r.metrics.failed_calls) / total) }) : "";
      },
    },
    { id: "subagent", label: t("子代理调用"), hint: t("每轮派出的子代理数。"), value: (r) => metricsNum(r.metrics.subagent_calls), show: metricsOne },
    { id: "output", label: t("输出 token"), hint: t("模型每轮生成的 token 数。"), value: (r) => metricsNum(r.outputTokens), show: metricsCount },
  ];
}

function metricsTime(at) {
  return new Date(at).toLocaleString();
}

export async function refreshMetrics() {
  const trend = $("metrics-trend");
  const list = $("metrics-rounds");
  if (!list) return;
  if (!currentProject) {
    trend?.replaceChildren();
    clearContextBill();
    list.innerHTML = `<p class="dim">${t("先在左侧「项目」里添加并选择一个目录")}</p>`;
    metricsRenderedProject = null;
    return;
  }
  const project = currentProject;
  const generation = ++metricsGeneration;
  // 换了项目:旧项目的柱图和列表先清掉,别在加载期间顶着上一个项目的数。
  if (project !== metricsRenderedProject) {
    trend?.replaceChildren();
    clearContextBill();
    list.replaceChildren();
    metricsRenderedProject = project;
  }
  trend?.classList.toggle("hidden", !trend.childNodes.length);
  // 加载指示:⟳ 转圈;页面里还没有内容时再写一句(再次进入页面,旧内容先留着,不闪)。
  const refresh = $("metrics-refresh");
  refresh?.setAttribute("aria-busy", "true");
  if (!list.childNodes.length) list.innerHTML = `<p class="dim">${t("正在读取运行画像…")}</p>`;
  // 上下文账单(B30)与逐轮画像并行读;账单读不到只是不显示这一块,不拖垮画像。
  const billRequest = invoke("memory_context_bill", { projectDir: project }).catch(() => null);
  try {
    const data = await invoke("run_metrics", { projectDir: project, limit: METRICS_ROUNDS });
    if (project !== currentProject || generation !== metricsGeneration) return;
    renderMetrics(data?.rounds ?? []);
    const bill = await billRequest;
    if (project !== currentProject || generation !== metricsGeneration) return;
    renderContextBill(bill);
  } catch (err) {
    if (project !== currentProject || generation !== metricsGeneration) return;
    // 出错时不留上一次的数:看起来像「当前就是这样」比空白更误导。
    trend?.replaceChildren();
    trend?.classList.add("hidden");
    clearContextBill();
    list.innerHTML = `<p class="dim">${t("运行画像加载失败")}</p>`;
    toastError(`${t("运行画像加载失败")}:${err}`, { retry: refreshMetrics });
  } finally {
    if (generation === metricsGeneration) refresh?.removeAttribute("aria-busy");
  }
}

// ---------- 上下文账单(B30,原在记忆页) ----------
// 最近一轮喂给模型的上下文由哪些来源组成、各占多少字符:数据是 memory_context_bill 的 bill([[来源, 字符数], …],
// 主对话最近一轮的 context_report)。它与记忆无关,放在运行画像里和逐轮画像一起看;「最近轮次」那一半早就是
// 逐轮画像本身,不再另摆一份。来源名是引擎内部路径(agent/system 之类),界面写中文名,原名留在悬停里。
const BILL_TOP_ROWS = 6;
export const CONTEXT_BILL_LABELS = Object.freeze({
  "agent/system": "系统提示词",
  "core/env": "运行环境",
  "core/project-state": "项目状态",
  "core/skills": "技能清单",
  "dev/control": "开工控制",
  "dev/conventions": "开发规范",
  "dev/decisions": "项目决策",
  "dev/design-index": "设计文档索引",
  "dev/ideas": "想法收件箱",
  "dev/memory": "项目记忆",
  "dev/verification-policy": "验证策略",
  "memory/hints": "记忆提示",
  "scout/brief": "勘察简报",
  "runtime/decisions": "运行时决定",
  "runtime/collaboration": "协作状态",
  "tools/catalog": "延迟工具目录",
  "tools/schema": "工具参数定义",
  "team/inbox": "团队待整理",
  "team/memory": "团队记忆",
  "research/docs": "研究文档",
  "research/workflow": "研究流程",
});
export function contextBillLabel(name) {
  const raw = String(name ?? "");
  if (raw.startsWith("tools/loaded:")) return `${t("已加载工具")} ${raw.slice("tools/loaded:".length)}`;
  return CONTEXT_BILL_LABELS[raw] ? t(CONTEXT_BILL_LABELS[raw]) : raw;
}
function clearContextBill() {
  const box = $("metrics-bill");
  if (!box) return;
  box.replaceChildren();
  box.classList.add("hidden");
}
export function renderContextBill(data) {
  const box = $("metrics-bill");
  if (!box) return;
  const rows = (Array.isArray(data?.bill) ? data.bill : [])
    .map((entry) => [String(entry?.[0] ?? ""), Number(entry?.[1]) || 0])
    .filter(([name, chars]) => name && chars > 0)
    .sort((a, b) => b[1] - a[1]);
  box.replaceChildren();
  if (!rows.length) {
    // 没有账单(还没跑过一轮,或读不到)就整块不出现,不留一个空框。
    box.classList.add("hidden");
    return;
  }
  const total = rows.reduce((sum, [, chars]) => sum + chars, 0);
  const head = document.createElement("div");
  head.className = "metrics-bill-head";
  const title = document.createElement("strong");
  title.textContent = t("上下文账单");
  const note = document.createElement("span");
  note.className = "dim";
  const at = Number(data?.episodes?.[0]?.at);
  note.textContent = fillTemplate(t("主对话最近一轮 · 共 {n} 字符{time}"), {
    n: metricsCount(total),
    time: Number.isFinite(at) && at > 0 ? ` · ${metricsTime(at)}` : "",
  });
  head.append(title, note);
  box.appendChild(head);
  const list = document.createElement("div");
  list.className = "metrics-bill-list";
  const addRow = (parent, name, chars, label = contextBillLabel(name)) => {
    const percent = total ? Math.round((chars / total) * 100) : 0;
    const row = document.createElement("div");
    row.className = "metrics-bill-row";
    row.dataset.source = name;
    row.title = name;
    const text = document.createElement("span");
    text.className = "metrics-bill-name";
    text.textContent = label;
    const size = document.createElement("span");
    size.className = "metrics-bill-size dim";
    size.textContent = `${metricsCount(chars)} · ${percent}%`;
    const bar = document.createElement("span");
    bar.className = "metrics-bill-bar";
    bar.setAttribute("aria-hidden", "true");
    bar.style.setProperty("width", `${Math.max(percent, 2)}%`);
    row.append(text, size, bar);
    parent.appendChild(row);
  };
  for (const [name, chars] of rows.slice(0, BILL_TOP_ROWS)) addRow(list, name, chars);
  box.appendChild(list);
  const rest = rows.slice(BILL_TOP_ROWS);
  if (rest.length) {
    const more = document.createElement("details");
    more.className = "metrics-bill-more";
    const summary = document.createElement("summary");
    summary.textContent = fillTemplate(t("其余 {n} 项 · {chars} 字符"), { n: rest.length, chars: metricsCount(rest.reduce((sum, [, chars]) => sum + chars, 0)) });
    more.appendChild(summary);
    for (const [name, chars] of rest) addRow(more, name, chars);
    box.appendChild(more);
  }
  box.classList.remove("hidden");
}

/// 一轮属于哪段对话:主对话 / 独立任务或讨论的名字;对话已经不在列表里(线路关闭了)就说「已关闭」。
function metricsRoundLine(round) {
  const item = processItems.find((entry) => entry.session_id === round.sessionId);
  if (item) return item.title || item.label || t("对话");
  return t("已关闭的任务");
}

/// 跳到这一轮所在的对话。对话已被关闭(不在 process_list 里)就说清楚,不假装能打开。
async function openMetricsRound(round) {
  const target = round.sessionId
    ? processItems.find((entry) => entry.session_id === round.sessionId)
    : processItems.find((entry) => entry.id === activeProcessId);
  if (!target) {
    toast(t("这一轮所在的对话已关闭,没法跳转"));
    return;
  }
  navigate_view("chat");
  if (target.id !== activeProcessId) await switchProcess(target.id);
}

function metricsBars(tile, chronological) {
  const host = document.createElement("div");
  host.className = "metrics-bars";
  host.setAttribute("aria-hidden", "true");
  const values = chronological.map((round) => (round.measured ? tile.value(round) : null));
  const max = Math.max(0, ...values.filter((value) => value != null));
  chronological.forEach((round, index) => {
    const value = values[index];
    const bar = document.createElement("span");
    bar.className = "metrics-bar";
    if (value == null) bar.classList.add("is-empty");
    else if (value === 0 || max === 0) bar.classList.add("is-zero");
    else bar.style.setProperty("height", `${Math.max(10, Math.round((value / max) * 100))}%`);
    bar.title = `${metricsTime(round.at)} · ${value == null ? t("该轮早于度量落地,无画像") : tile.show(value)}`;
    host.appendChild(bar);
  });
  return host;
}

function metricsTrend(rounds) {
  const trend = $("metrics-trend");
  trend.replaceChildren();
  // 趋势是「有度量的轮次」的均值;早于度量落地的轮次要说清楚没算进去。
  const measured = rounds.filter((round) => round.measured);
  const skipped = rounds.length - measured.length;
  const head = document.createElement("div");
  head.className = "metrics-trend-head";
  const title = document.createElement("strong");
  title.textContent = fillTemplate(t("最近 {n} 轮"), { n: rounds.length });
  const note = document.createElement("span");
  note.className = "dim";
  note.textContent = !measured.length
    ? t("这些轮次都早于度量落地,暂时没有可统计的均值")
    : skipped
      ? fillTemplate(t("均值只统计有度量的 {n} 轮,另有 {skipped} 轮早于度量落地,不计入"), { n: measured.length, skipped })
      : fillTemplate(t("均值统计这 {n} 轮;柱图每根一轮,左旧右新"), { n: measured.length });
  head.append(title, note);
  trend.appendChild(head);
  trend.classList.remove("hidden");
  if (!measured.length) return;
  const chronological = [...rounds].reverse();
  for (const tile of metricTiles()) {
    const mean = tile.mean ? tile.mean(measured) : metricsMean(measured.map(tile.value));
    const cell = document.createElement("div");
    cell.className = `metrics-cell${tile.bad ? " is-bad" : ""}`;
    cell.title = tile.hint;
    const name = document.createElement("span");
    name.className = "dim";
    name.textContent = tile.label;
    const value = document.createElement("strong");
    value.textContent = tile.show(mean);
    cell.append(name, value);
    const sub = tile.sub?.(measured);
    if (sub) {
      const line = document.createElement("span");
      line.className = "metrics-cell-sub dim";
      line.textContent = sub;
      cell.appendChild(line);
    }
    cell.appendChild(metricsBars(tile, chronological));
    trend.appendChild(cell);
  }
}

function metricsRoundRow(round) {
  const item = document.createElement("div");
  item.className = `metrics-round${round.outcome === "halted" ? " halted" : ""}`;
  item.setAttribute("role", "button");
  item.tabIndex = 0;
  const m = round.metrics || {};
  const contextTotal = Object.values(round.context || {}).reduce(
    (sum, entry) => sum + (Array.isArray(entry) ? entry[1] : Number(entry) || 0),
    0,
  );
  const outcome = { completed: t("已完成"), halted: t("已停止") }[round.outcome] ?? round.outcome;
  const head = document.createElement("div");
  head.className = "metrics-round-head";
  const when = document.createElement("span");
  when.textContent = metricsTime(round.at);
  const line = document.createElement("span");
  line.className = "metrics-round-line";
  line.textContent = metricsRoundLine(round);
  const totals = document.createElement("span");
  totals.className = "dim metrics-round-totals";
  totals.textContent = [
    outcome,
    fillTemplate(t("{n} 步"), { n: round.steps }),
    fillTemplate(t("输入 {input} · 输出 {output} token"), { input: metricsCount(metricsNum(round.inputTokens)), output: metricsCount(metricsNum(round.outputTokens)) }),
  ].join(" · ");
  head.append(when, line, totals);
  const prompt = document.createElement("div");
  prompt.className = "metrics-round-prompt dim";
  prompt.textContent = round.prompt;
  const stats = document.createElement("div");
  stats.className = "metrics-round-stats dim";
  stats.textContent = round.measured
    ? [
      fillTemplate(t("终端 {n} 次"), { n: m.terminal_calls ?? 0 }),
      fillTemplate(t("git {n} 次({groups} 组)"), { n: m.git_calls ?? 0, groups: m.git_groups ?? 0 }),
      fillTemplate(t("改文件 {misses}/{calls} 次未命中"), { misses: m.edit_misses ?? 0, calls: m.edit_calls ?? 0 }),
      m.edit_rejections ? fillTemplate(t("{n} 次被工具拒绝"), { n: m.edit_rejections }) : "",
      fillTemplate(t("子代理 {n} 次"), { n: m.subagent_calls ?? 0 }),
      fillTemplate(t("失败 {failed}/{total} 次"), { failed: m.failed_calls ?? 0, total: m.total_calls ?? 0 }),
      fillTemplate(t("上下文 {n} 字"), { n: contextTotal }),
    ].filter(Boolean).join(" · ") + redundantLine(m)
    : t("该轮早于度量落地,无画像");
  const tools = document.createElement("div");
  tools.className = "metrics-round-tools dim";
  tools.textContent = Object.entries(round.tools || {})
    .sort((a, b) => b[1] - a[1])
    .map(([name, count]) => `${name}×${count}`)
    .join("  ");
  const open = document.createElement("span");
  open.className = "metrics-round-open";
  open.setAttribute("aria-hidden", "true");
  open.textContent = "›";
  item.append(head, prompt, stats, tools, open);
  const label = `${t("打开这一轮所在的对话")}:${round.prompt}`;
  item.title = t("打开这一轮所在的对话");
  item.setAttribute("aria-label", label);
  item.addEventListener("click", () => void openMetricsRound(round));
  item.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    void openMetricsRound(round);
  });
  return item;
}

export function renderMetrics(rounds) {
  const trend = $("metrics-trend");
  const list = $("metrics-rounds");
  list.replaceChildren();
  if (!rounds.length) {
    // 空态只留一句话:趋势区整块隐藏,不留空灰条。
    trend.replaceChildren();
    trend.classList.add("hidden");
    list.innerHTML = `<p class="dim">${t("还没有轮次记录:跑一轮后这里会出现画像")}</p>`;
    return;
  }
  metricsTrend(rounds);
  for (const round of rounds) list.appendChild(metricsRoundRow(round));
}

export function redundantLine(m) {
  const total = (m.redundant_git ?? 0) + (m.redundant_test ?? 0) + (m.redundant_task ?? 0);
  if (total === 0) return "";
  const parts = [];
  if (m.redundant_git) parts.push(`${t("重复 git 查询")}×${m.redundant_git}`);
  if (m.redundant_test) parts.push(`${t("重复跑测试")}×${m.redundant_test}`);
  if (m.redundant_task) parts.push(`${t("重复派子代理")}×${m.redundant_task}`);
  return ` · ${t("冗余提醒")} ${parts.join(" ")}`;
}

defer(() => {
  $("metrics-refresh")?.addEventListener("click", () => void refreshMetrics());
});

// 事件分类的显示名(值是 i18n 键):界面只显示中文名,英文枚举是后端的内部分类。
export const INCIDENT_CLASS_LABELS = Object.freeze({
  execution_incident: "瞬时失手",
  development_defect: "开发缺陷",
  product_defect: "产品缺陷",
  regression: "回归",
});

export function formatIncidentDuration(metrics) {
  const samples = Number(metrics?.repair_duration_samples ?? 0);
  if (!samples) return t("暂无");
  const average = Number(metrics?.repair_duration_ms_average ?? 0);
  if (average < 1000) return `${Math.round(average)}ms`;
  return `${(average / 1000).toFixed(1)}s`;
}

export function renderIncidentMetrics(data, targetId) {
  const box = $(targetId);
  if (!box) return;
  box.replaceChildren();
  const byClass = data?.by_class ?? {};
  const heading = document.createElement("div");
  heading.className = "metrics-trend-head dim";
  heading.textContent = t("事件分类指标");
  const table = document.createElement("table");
  table.className = "metrics-cat-table";
  const head = document.createElement("tr");
  for (const label of [t("类型"), t("数量"), t("平均修复时长"), t("逃逸率"), t("晋升")]) {
    const cell = document.createElement("th");
    cell.textContent = label;
    head.appendChild(cell);
  }
  table.appendChild(head);
  for (const className of Object.keys(INCIDENT_CLASS_LABELS)) {
    const metrics = byClass[className] ?? {};
    const row = document.createElement("tr");
    const values = [
      t(INCIDENT_CLASS_LABELS[className]),
      Number(metrics.occurrences ?? 0),
      formatIncidentDuration(metrics),
      `${(Number(metrics.escaped_rate ?? 0) * 100).toFixed(0)}%`,
      Number(metrics.promotions ?? 0),
    ];
    for (const value of values) {
      const cell = document.createElement("td");
      cell.textContent = String(value);
      row.appendChild(cell);
    }
    table.appendChild(row);
  }
  const overall = data?.overall ?? {};
  const summary = document.createElement("div");
  summary.className = "dim";
  summary.textContent = `${t("总事件")} ${Number(data?.total_occurrences ?? 0)} · ${t("总体逃逸率")} ${(Number(overall.escaped_rate ?? 0) * 100).toFixed(0)}% · ${t("晋升事件")} ${Number(data?.promotion_events ?? 0)}`;
  const replay = data?.historical_replay ?? {};
  const replayNote = document.createElement("div");
  replayNote.className = "dim";
  replayNote.textContent = `${t("历史样本回放")} ${Number(replay.consistent_count ?? 0)}/${Number(replay.sample_count ?? 0)} ${replay.consistent ? t("一致") : t("不一致")} · ${t("瞬时失手排除")} ${Number(replay.execution_incidents_excluded ?? 0)}`;
  box.append(heading, table, summary, replayNote);
}

// ---------- R-126 UI 自查探针:在真实运行中的窗口里取样 ----------
// 后端工具发 kz:ui-probe,这里取样后用 ui_probe_result 回传。取的是用户眼前这个
// 窗口的实际渲染结果——不是重新起一个空白页,那样查不出任何真实的渲染问题。
export const UI_PROBE_NODE_LIMIT = 60;

export function describeNode(el, depth) {
  const indent = "  ".repeat(depth);
  const cls = el.className && typeof el.className === "string" ? `.${el.className.trim().split(/\s+/).join(".")}` : "";
  const id = el.id ? `#${el.id}` : "";
  // 只取本节点的直接文本,不含子节点——否则每层都把整棵子树的文字重复一遍。
  const own = [...el.childNodes]
    .filter((n) => n.nodeType === 3)
    .map((n) => n.nodeValue.trim())
    .filter(Boolean)
    .join(" ")
    .slice(0, 80);
  const box = el.getBoundingClientRect?.();
  const hidden = box && box.width === 0 && box.height === 0 ? " [不可见]" : "";
  return `${indent}<${el.tagName.toLowerCase()}${id}${cls}>${hidden}${own ? ` "${own}"` : ""}`;
}

export function probeDom(selector) {
  const roots = [...document.querySelectorAll(selector)];
  if (!roots.length) return `没有匹配 \`${selector}\` 的元素(选择器写错,或该区域此刻未渲染)。`;
  const lines = [];
  let truncated = false;
  const walk = (el, depth) => {
    if (lines.length >= UI_PROBE_NODE_LIMIT) {
      truncated = true;
      return;
    }
    lines.push(describeNode(el, depth));
    for (const child of el.children) walk(child, depth + 1);
  };
  for (const root of roots.slice(0, 5)) walk(root, 0);
  const head = `匹配 ${roots.length} 个${roots.length > 5 ? "(只展开前 5 个)" : ""}:`;
  // 截断必须可见:静默截断会让 agent 以为看到了全部(既有 conventions 的教训)。
  return `${head}\n${lines.join("\n")}${truncated ? `\n… 已截断(上限 ${UI_PROBE_NODE_LIMIT} 个节点)` : ""}`;
}

export function probeConsole() {
  if (!uiConsoleLog.length) return "自加载以来没有 console 错误或警告。";
  return uiConsoleLog
    .map((e) => `[${e.level}] ${new Date(e.at).toLocaleTimeString()} ${e.text}`)
    .join("\n");
}

export function probeStyle(selector) {
  const els = [...document.querySelectorAll(selector)].slice(0, 5);
  if (!els.length) return `没有匹配 \`${selector}\` 的元素。`;
  // 只给与"为什么没显示/为什么挤成一团"相关的属性,不倾倒整个 computed style。
  const keys = [
    "display", "position", "visibility", "opacity", "overflow",
    "flexDirection", "gridTemplateColumns", "width", "height", "maxHeight",
    "margin", "padding", "whiteSpace", "textOverflow", "zIndex",
  ];
  return els
    .map((el, index) => {
      const style = window.getComputedStyle(el);
      const box = el.getBoundingClientRect();
      const props = keys.map((k) => `${k}=${style[k]}`).join(" ");
      return `#${index + 1} ${describeNode(el, 0).trim()}\n  盒模型: ${Math.round(box.width)}×${Math.round(box.height)} @ (${Math.round(box.left)},${Math.round(box.top)})\n  ${props}`;
    })
    .join("\n");
}

defer(() => {
  on("kz:ui-probe", (event) => {
    const { id, kind, arg } = event.payload ?? {};
    let result;
    try {
      if (kind === "dom") result = probeDom(arg);
      else if (kind === "console") result = probeConsole();
      else if (kind === "style") result = probeStyle(arg);
      else result = `未知探针类型: ${kind}`;
    } catch (err) {
      // 探针自身出错也要如实回传,不能让后端悬到超时。
      result = `探针执行失败: ${err}`;
    }
    invoke("ui_probe_result", { id, result }).catch(() => {});
  });
});

// 去重用的机器指纹 [fp:…]:读的人看不懂也用不上,展示时拿掉(原文仍在文件与「编辑正文」里,保存不会丢)。
export function stripMemoryFingerprint(text) {
  return String(text ?? "").replace(/[ \t]*\[fp:[^\]]*\]/g, "").trim();
}
/// 标题里的机器指纹也要拿掉:自动落入的记忆标题形如「摘要 — [fp:…]」,去掉指纹后还剩一个悬空的破折号,一并收掉。
/// 只用于展示(列表行、详情大标题、通知);「编辑」框里仍是原文,保存不会丢。
export function displayMemoryTitle(title) {
  return stripMemoryFingerprint(title).replace(/\s+[—–-]\s*$/, "").trim();
}

// 一次整理的失败原因(成功返回空串):批次里任一条带 error,或整体被中止。
function memoryConsolidationFailure(report) {
  const failed = (report?.batches ?? []).find((batch) => batch.error);
  return failed?.error || report?.stopped_reason || "";
}

// R-124:待整理笔记。笔记是运行中顺手记下的原料,不会自己入库——
// 「采纳」只让记忆管理子代理提炼这一条(不再顺带把整箱都整理掉),「丢弃」直接移出,都是用户一键的事。
export function renderMemoryCandidates(list) {
  const box = $("memory-candidates");
  const count = $("memory-candidate-count");
  if (!box) return;
  box.innerHTML = "";
  const items = Array.isArray(list) ? list : [];
  if (count) count.textContent = items.length ? `· ${items.length}` : "";
  if (!items.length) {
    box.innerHTML = `<p class="dim">${t("暂无待整理条目")}</p>`;
    return;
  }
  for (const item of items) {
    // 逐条操作靠摘要原文定位:指纹可能为空(摘要末尾没有 [...]),空串会匹配所有笔记,绝不能当键。
    const key = item.summary || item.fingerprint || "";
    const scope = item.scope || "project";
    const row = document.createElement("div");
    row.className = `memory-candidate${item.hint === "sop" ? " sop" : ""}`;
    row.dataset.fingerprint = item.fingerprint || "";
    const head = document.createElement("div");
    head.className = "memory-candidate-head";
    const shownSummary = stripMemoryFingerprint(item.summary);
    head.innerHTML =
      `<span class="memory-candidate-hint">${escapeHtml(item.hint ? memoryWord(item.hint) : t("待整理"))}</span>` +
      `<span class="memory-candidate-summary">${escapeHtml(shownSummary)}</span>`;
    head.title = shownSummary;
    // UI-0926 #10:候选正文是 markdown(列表/代码/路径),按 markdown 渲染而不是原文堆字。
    const detail = document.createElement("div");
    detail.className = "memory-candidate-detail dim md sv-md";
    renderMarkdownInto(detail, stripMemoryFingerprint(item.detail));
    const actions = document.createElement("div");
    actions.className = "memory-candidate-actions";
    const adopt = document.createElement("button");
    adopt.type = "button";
    adopt.className = "primary mini";
    adopt.textContent = t("采纳");
    adopt.title = t("只让记忆管理子代理提炼这一条");
    if (scope !== "project" || !key) {
      adopt.disabled = true;
      adopt.title = t("只有项目记忆的待整理条目可以单条采纳");
    }
    const drop = document.createElement("button");
    drop.type = "button";
    drop.className = "ghost mini danger";
    drop.textContent = t("丢弃");
    drop.title = t("直接移出待整理,不再进入提炼范围");
    drop.disabled = !key;
    adopt.addEventListener("click", async () => {
      const project = currentProject;
      adopt.disabled = true;
      drop.disabled = true;
      neuralFlowEmit?.("memory_consolidation_started", { fingerprint: item.fingerprint });
      try {
        const result = await invoke("memory_consolidate_note", { projectDir: project, scope, summary: key });
        const failure = memoryConsolidationFailure(result?.report);
        if (failure) throw new Error(failure);
        neuralFlowEmit?.("memory_consolidation_completed", { fingerprint: item.fingerprint });
        toast(t("已提炼成记忆条目"));
      } catch (err) {
        neuralFlowEmit?.("memory_consolidation_failed", { fingerprint: item.fingerprint });
        adopt.disabled = false;
        drop.disabled = false;
        toastError(`${t("提炼失败")}:${err?.message ?? err}`);
      } finally {
        if (project === currentProject) void refreshMemory({ force: true });
      }
    });
    drop.addEventListener("click", async () => {
      const project = currentProject;
      try {
        await invoke("memory_note_discard", { projectDir: project, scope, fingerprint: key });
        neuralFlowEmit?.("memory_candidate_discarded", { fingerprint: item.fingerprint });
        toast(t("已丢弃"));
        if (project === currentProject) void refreshMemory({ force: true });
      } catch (err) {
        toastError(`${t("丢弃失败")}:${err}`);
      }
    });
    actions.append(adopt, drop);
    row.append(head, detail, actions);
    box.appendChild(row);
  }
}

// 使用复查清单：已观测注入至少 3 次而未记录正文读取，或高频召回；可点开详情。
// 处置不在这里静默删——点条目打开详情页走既有墓碑机制(降级/修订/归档)。
// D-217:stale 积压(已归档条目数)也进清单——归档保留墓碑正文可回看复查。
export function renderMemoryValueFlags(data) {
  const box = $("memory-value-flags");
  const count = $("memory-flags-count");
  if (!box) return;
  box.innerHTML = "";
  const zero = Array.isArray(data?.zero_read) ? data.zero_read : [];
  const recur = Array.isArray(data?.frequent) ? data.frequent : [];
  const staleArchived = Number(data?.stale_archived) || 0;
  const total = zero.length + recur.length;
  count.textContent = total ? `· ${total}` : "";
  if (staleArchived > 0) {
    const p = document.createElement("p");
    p.className = "memory-flags-head stale-archived";
    p.textContent = `${t("已归档待复查")} (${staleArchived})`;
    box.appendChild(p);
  }
  if (!total) {
    const empty = document.createElement("p");
    empty.className = "dim";
    empty.textContent = t("暂无需要复查的使用记录");
    box.appendChild(empty);
    return;
  }
  if (zero.length) {
    const h = document.createElement("p");
    h.className = "memory-flags-head";
    h.textContent = `${t("多次注入但未记录正文读取")} (${zero.length})`;
    box.appendChild(h);
    for (const item of zero) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "memory-flag-row zero-read";
      row.innerHTML =
        `<span class="memory-row-id">${escapeHtml(item.id)}</span>` +
        `<span class="memory-row-title">${escapeHtml(displayMemoryTitle(item.title))}</span>` +
        `<span class="dim">${t("召回")} ${item.recalled} · ${t("注入")} ${item.injected} · ${t("正文读取")} ${item.read_observed ? item.read : t("未知")}</span>`;
      row.addEventListener("click", () => openMemoryDetailById(item.scope, item.id));
      box.appendChild(row);
    }
  }
  if (recur.length) {
    const h = document.createElement("p");
    h.className = "memory-flags-head";
    h.textContent = `${t("高频召回")} (${recur.length})`;
    box.appendChild(h);
    for (const item of recur) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "memory-flag-row frequent";
      row.innerHTML =
        `<span class="memory-row-id">${escapeHtml(item.id)}</span>` +
        `<span class="memory-row-title">${escapeHtml(displayMemoryTitle(item.title))}</span>` +
        `<span class="dim">${t("召回")} ${item.recalled} · ${t("注入")} ${item.injected} · ${t("正文读取")} ${item.read_observed ? item.read : t("未知")}</span>`;
      row.addEventListener("click", () => openMemoryDetailById(item.scope, item.id));
      box.appendChild(row);
    }
  }
}

// 从清单跳详情:按 scope+id 定位条目并复用现有详情渲染。
export async function openMemoryDetailById(scope, id) {
  if (!(await confirmLeaveMemoryDetail())) return;
  const project = currentProject;
  const generation = ++memoryListGeneration;
  try {
    const list = await getMemoryEntries(project, scope);
    if (project !== currentProject || generation !== memoryListGeneration) return;
    const entry = (list || []).find((e) => e.id === id);
    if (entry) {
      clearMemorySearch();
      memoryManagerFilters.scope = scope;
      memoryManagerFilters.category = entry.category || "all";
      memoryManagerFilters.status = "all";
      memoryCurrentEntryId = id;
      memorySelection = { scope, category: entry.category || "all" };
      syncMemoryManagerFilters();
      renderMemoryList(list, { search: false });
      showMemoryDetail(scope, entry);
    }
  } catch (err) {
    if (project !== currentProject || generation !== memoryListGeneration) return;
    toastError(`${t("记忆条目加载失败")}:${err}`);
  }
}

// 搜索结果里点一条:只打开它的详情——结果列表与筛选保持原样(此前整页换成全量列表、筛选被改写,找到的上下文丢了)。
async function openMemorySearchHit(hit) {
  if (!(await confirmLeaveMemoryDetail())) return;
  const project = currentProject;
  const scope = hit.scope || "project";
  try {
    const list = await getMemoryEntries(project, scope);
    const entry = (list || []).find((item) => item.id === hit.id) ?? await invoke("memory_entry_get", { projectDir: project, scope, id: hit.id });
    if (project !== currentProject) return;
    showMemoryDetail(scope, { ...entry, scope }, { readOnly: Boolean(entry.archived), reveal: !memoryChatTabActive() });
  } catch (err) {
    if (project !== currentProject) return;
    toastError(`${t("记忆条目加载失败")}:${err}`);
  }
}

// 管理对话页签开着时,点列表只是换「当前选中的条目」(对话的上下文),不把人拽回阅读页签。
const memoryChatTabActive = () => $("memory-scroll")?.dataset.tab === "chat";

// 召回、注入与正文读取分别呈现；历史未知与未记录读取不能混同。
export function renderMemoryRecalls(data) {
  const box = $("memory-recalls");
  const rate = $("memory-recall-rate");
  if (!box) return;
  box.innerHTML = "";
  const rounds = data?.rounds ?? [];
  rate.textContent = rounds.length ? `· ${t("最近")} ${rounds.length} ${t("次检索")}` : "";
  if (!rounds.length) {
    box.innerHTML = `<p class="dim">${t("暂无检索记录。下一次运行或搜索后可在这里查看。")}</p>`;
    return;
  }
  const triggers = { memory_search: t("任务检索"), event_recall: t("失败触发"), user_search: t("手动搜索") };
  for (const round of rounds) {
    const item = document.createElement("details");
    item.className = "memory-recall";
    item.open = round === rounds[0];
    const head = document.createElement("summary");
    head.className = "memory-recall-head";
    const hits = round.hits ?? [];
    head.textContent = `${new Date(round.at).toLocaleString()} · ${triggers[round.trigger_type] || round.trigger_type} · ${hits.length} ${t("条命中")}`;
    const prompt = document.createElement("div");
    prompt.className = "memory-recall-prompt dim";
    prompt.textContent = `${round.prompt_head || round.query || t("未知")} · ${t("运行")} ${round.run_id || t("未知")} · ${t("轮次")} ${round.episode_id ?? t("未关联")}`;
    const query = document.createElement("p");
    query.className = "dim";
    query.textContent = `${t("检索词")}: ${round.query || t("未知")} · ${round.policy_action} · ${round.total_ms} ms`;
    item.append(head, prompt, query);
    if (!hits.length) {
      const empty = document.createElement("p");
      empty.className = "dim";
      empty.textContent = t("本次检索没有命中记忆");
      item.appendChild(empty);
    }
    for (const hit of hits) {
      const row = document.createElement("div");
      row.className = `memory-recall-hit${hit.read === true ? " read" : ""}`;
      const state = !hit.injected ? t("未注入") : hit.read === true ? t("已读取正文") : hit.read === false ? t("已注入 · 尚未记录正文读取") : t("已注入 · 历史读取未知");
      row.innerHTML =
        `<span class="memory-recall-id">${escapeHtml(hit.id)}</span>` +
        `<span class="memory-recall-title">${escapeHtml(displayMemoryTitle(hit.title))}</span>` +
        `<span class="memory-recall-flag">${escapeHtml(state)}</span>`;
      item.appendChild(row);
    }
    box.appendChild(item);
  }
}

export function renderMemoryArch(overview) {
  const arch = $("memory-arch");
  arch.innerHTML = "";
  for (const scope of overview.scopes || []) {
    const card = document.createElement("div");
    card.className = "memory-scope-card";
    const head = document.createElement("div");
    head.className = "memory-scope-head";
    const label = scope.scope === "global" ? t("全局记忆") : t("项目记忆");
    const archivedNote = scope.archived ? ` · ${t("已归档")} ${scope.archived}` : "";
    head.innerHTML = `<strong>${label}</strong> <span class="dim">${scope.total} ${t("条")} · ${t("命中")} ${scope.hitsTotal}${archivedNote} · ${escapeHtml(scope.root)}</span>`;
    card.appendChild(head);
    const grid = document.createElement("div");
    grid.className = "memory-cat-grid";
    for (const [cat, info] of Object.entries(scope.categories || {})) {
      const cell = document.createElement("button");
      cell.type = "button";
      cell.className = "memory-cat-cell";
      cell.setAttribute("aria-label", `${label} ${memoryWord(cat)}`);
      // 候选(待采纳)与失效分开写:此前 `总数 - 启用` 被一律标成 stale,候选条目看起来像坏了。
      const noteParts = [info.candidate ? `${info.candidate} ${memoryWord("candidate")}` : "", info.stale ? `${info.stale} ${memoryWord("stale")}` : "", info.last || ""].filter(Boolean);
      cell.innerHTML = `<span class="memory-cat-name">${escapeHtml(memoryWord(cat))}</span><span class="memory-cat-count">${info.active}</span><span class="dim">${escapeHtml(noteParts.join(" · "))}</span>`;
      cell.addEventListener("click", async () => {
        if (!(await confirmLeaveMemoryDetail())) return;
        clearMemorySearch();
        memoryManagerFilters.scope = scope.scope;
        memoryManagerFilters.category = cat;
        memoryManagerFilters.status = "active";
        memorySelection = { scope: scope.scope, category: cat };
        memoryCurrentEntryId = null;
        hideMemoryDetail();
        loadMemoryList(scope.scope, cat);
      });
      grid.appendChild(cell);
    }
    card.appendChild(grid);
    if ((scope.integrity || []).length) {
      const warn = document.createElement("p");
      warn.className = "memory-warn";
      warn.textContent = `⚠ ${scope.integrity.join("; ")}`;
      card.appendChild(warn);
    }
    arch.appendChild(card);
  }
}

// 已归档视图的条目缓存(同 getMemoryEntries:15 秒内复用,写操作后 refreshMemory({ force }) 会清掉)。
async function getArchivedEntries(project, scope) {
  const key = `${project}\0${scope}`;
  const cached = memoryArchivedCache.get(key);
  if (cached && (cached.pending || Date.now() - cached.at < 15000)) return cached.promise;
  const entry = { at: Date.now(), pending: true };
  entry.promise = invoke("memory_archived_entries", { projectDir: project, scope })
    .then((rows) => { entry.pending = false; entry.at = Date.now(); return rows || []; })
    .catch((error) => { if (memoryArchivedCache.get(key) === entry) memoryArchivedCache.delete(key); throw error; });
  memoryArchivedCache.set(key, entry);
  return entry.promise;
}

// 搜索结果态下的后台刷新:结果列表不动,只把已打开的详情换成最新内容(保存/管理对话改了它)。
async function refreshOpenMemoryDetail() {
  const project = currentProject;
  const box = $("memory-detail");
  if (!project || !memoryCurrentEntryId || memoryDetailDirty() || box?.dataset.graphNode || box?.dataset.readonly === "true") return;
  const scope = memorySelection.scope;
  try {
    const list = await getMemoryEntries(project, scope);
    if (project !== currentProject) return;
    const current = (list || []).find((entry) => entry.id === memoryCurrentEntryId);
    if (current) showMemoryDetail(scope, { ...current, scope }, { reveal: false });
    else {
      memoryCurrentEntryId = null;
      hideMemoryDetail();
    }
  } catch {
    /* 刷新失败就保留旧详情,不打断阅读 */
  }
}

export async function loadMemoryList(scope, category, { preserveSelection = false } = {}) {
  // 搜索结果态:后台刷新不许把结果列表换回普通列表(退出搜索靠清除/改筛选)。
  if (memorySearchQuery) return refreshOpenMemoryDetail();
  const project = currentProject;
  const generation = ++memoryListGeneration;
  try {
    memoryManagerFilters.scope = scope || "project";
    memoryManagerFilters.category = category || "all";
    syncMemoryManagerFilters();
    const archivedView = memoryManagerFilters.status === "archived";
    const fetchEntries = archivedView ? getArchivedEntries : getMemoryEntries;
    const scopes = memoryManagerFilters.scope === "all" ? ["project", "global"] : [memoryManagerFilters.scope];
    const results = await Promise.all(scopes.map(async itemScope => (await fetchEntries(project, itemScope)).map(entry => ({ ...entry, scope: itemScope }))));
    if (project !== currentProject || generation !== memoryListGeneration) return;
    memoryListEntries = results.flat().map((entry) => ({ ...entry, scope: entry.scope || memoryManagerFilters.scope }));
    const filtered = memoryListEntries
      .filter((entry) => memoryManagerFilters.category === "all" || entry.category === memoryManagerFilters.category)
      .filter((entry) => archivedView || memoryManagerFilters.status === "all" || entry.status === memoryManagerFilters.status)
      .sort((a, b) => {
        if (memoryManagerFilters.sort === "hits") return (b.hits ?? 0) - (a.hits ?? 0) || a.id.localeCompare(b.id, undefined, { numeric: true });
        if (memoryManagerFilters.sort === "title") return a.title.localeCompare(b.title) || a.id.localeCompare(b.id, undefined, { numeric: true });
        if (memoryManagerFilters.sort === "id") return a.id.localeCompare(b.id, undefined, { numeric: true });
        return String(b.updated || "").localeCompare(String(a.updated || "")) || a.id.localeCompare(b.id, undefined, { numeric: true });
      });
    renderMemoryList(filtered, { keepScroll: preserveSelection });
    if (preserveSelection && memoryCurrentEntryId) {
      const current = memoryListEntries.find((entry) => entry.id === memoryCurrentEntryId && entry.scope === memorySelection.scope);
      if (current && !memoryDetailDirty()) showMemoryDetail(current.scope || scope, current, { reveal: false, readOnly: archivedView });
      else if (!current) hideMemoryDetail();
    }
  } catch (err) {
    if (project !== currentProject || generation !== memoryListGeneration) return;
    toastError(`${t("记忆条目加载失败")}:${err}`);
  }
}

const MEMORY_SCOPE_WORDS = { project: "项目记忆", global: "全局记忆" };

export function renderMemoryList(entries, { search = false, keepScroll = false, truncated = false } = {}) {
  const container = $("memory-list");
  const count = $("memory-list-count");
  const state = $("memory-list-state");
  const archivedView = !search && memoryManagerFilters.status === "archived";
  // 整页重绘会把滚动位置冲回顶部:保存/刷新后的重绘回到原位,换筛选才回顶。
  const scrollTop = keepScroll ? container.scrollTop : 0;
  container.innerHTML = "";
  count.textContent = `${entries.length} ${t("条")}`;
  state.textContent = search ? `${t("搜索")}「${memorySearchQuery}」` : archivedView ? t("已归档 · 只读，可恢复") : "";
  if (!entries.length) {
    container.innerHTML = `<p class="dim">${t(search ? "没有命中的记忆" : archivedView ? "没有已归档的记忆" : "该筛选暂无记忆")}</p>`;
    return;
  }
  for (const entry of entries) {
    const row = document.createElement("button");
    row.type = "button";
    const ageDays = memoryAgeDays(entry.updated);
    const live = !search && !entry.archived && entry.status !== "stale";
    const dormant = live && (entry.hits ?? 0) === 0 && ageDays >= 3;
    const zeroRead = live && (entry.read_observed ?? 0) >= 3 && (entry.read ?? 0) === 0;
    row.className = `memory-row${entry.id === memoryCurrentEntryId ? " selected" : ""}${entry.status === "stale" || entry.archived ? " stale" : ""}${dormant ? " dormant" : ""}${zeroRead ? " zero-read" : ""}${entry.category === "sop" ? " sop" : ""}`;
    row.dataset.memoryId = entry.id;
    row.dataset.scope = entry.scope || memoryManagerFilters.scope;
    row.title = displayMemoryTitle(entry.title) || entry.id;
    row.classList.toggle("selected", entry.id === memoryCurrentEntryId && row.dataset.scope === memorySelection.scope);
    const lastHit = entry.last_hit_at ? `${t("最近命中")} ${new Date(entry.last_hit_at).toLocaleDateString()}` : t("从未命中");
    const recallMeta = (entry.recalled ?? 0) > 0 ? `${t("召回")} ${entry.recalled} · ${t("注入")} ${entry.injected ?? 0} · ${t("正文读取")} ${entry.read_observed ? entry.read : t("未知")}` : "";
    const snippet = stripMemoryFingerprint(search ? entry.snippet : entry.description);
    // 「启用」是常态,不给它画徽标;候选/试运行/已归档才值得在行上标出来。
    const badge = entry.status && entry.status !== "active" ? `<span class="memory-status-badge ${escapeHtml(entry.status)}">${escapeHtml(memoryWord(entry.status))}</span>` : "";
    const flags = `${dormant ? `<em class="memory-dormant-flag">${t("长期零命中")}</em> · ` : ""}${zeroRead ? `<em class="memory-zero-read-flag">${t("多次注入但未记录正文读取")}</em> · ` : ""}`;
    const scopeWord = t(MEMORY_SCOPE_WORDS[entry.scope || memoryManagerFilters.scope] ?? "项目记忆");
    const metaText = [`${scopeWord}/${memoryWord(entry.category)}`, `${t("命中")} ${entry.hits ?? 0}`, recallMeta, lastHit, entry.updated || ""].filter(Boolean).join(" · ");
    row.innerHTML =
      `<span class="memory-row-top"><span class="memory-row-id">${escapeHtml(entry.id)}</span><span class="memory-row-title">${escapeHtml(displayMemoryTitle(entry.title) || entry.id)}</span>${badge}</span>` +
      `<span class="dim memory-row-description">${escapeHtml(snippet)}</span>` +
      `<span class="memory-row-meta dim">${flags}${escapeHtml(metaText)}</span>`;
    row.addEventListener("click", async () => {
      if (search) {
        void openMemorySearchHit(entry);
        return;
      }
      const scope = entry.scope || memoryManagerFilters.scope;
      // 点的还是当前这条:改了一半的内容原样留着,别拿它去触发「放弃修改」的确认。
      if (entry.id === memoryCurrentEntryId && scope === memorySelection.scope && memoryDetailDirty()) return;
      if (!(await confirmLeaveMemoryDetail())) return;
      showMemoryDetail(scope, entry, { reveal: !memoryChatTabActive(), readOnly: Boolean(entry.archived) });
    });
    container.appendChild(row);
  }
  if (search && truncated) {
    const note = document.createElement("p");
    note.className = "dim memory-list-note";
    note.textContent = `${t("只显示前")} ${entries.length} ${t("条，换个更具体的关键词缩小范围")}`;
    container.appendChild(note);
  }
  container.scrollTop = scrollTop;
}

// 搬回记忆库:「撤销归档」与「已归档」视图里的「恢复」共用。status = 搬回后的状态(撤销时是归档前的状态)。
async function restoreMemoryEntry(project, scope, id, status, title) {
  try {
    await invoke("memory_entry_restore", { projectDir: project, scope, id, status });
    if (project !== currentProject) return;
    dismissMemoryNotice();
    toast(`${t("已恢复")}「${title || id}」`);
    memoryCurrentEntryId = null;
    hideMemoryDetail();
    document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project } }));
  } catch (err) {
    toastError(`${t("恢复失败")}:${err}`);
  }
}

// 搜索结果态下归档/删除了一条:把它从结果里拿掉(普通列表会整体重载,结果列表不会)。
function dropMemoryRow(scope, id) {
  const row = [...document.querySelectorAll("#memory-list .memory-row")].find((item) => item.dataset.memoryId === id && item.dataset.scope === scope);
  if (!row || !memorySearchQuery) return;
  row.remove();
  const remaining = document.querySelectorAll("#memory-list .memory-row").length;
  const count = $("memory-list-count");
  if (count) count.textContent = `${remaining} ${t("条")}`;
}

export function showMemoryDetail(scope, entry, { reveal = true, readOnly = false } = {}) {
  const box = $("memory-detail");
  if (!box) return;
  const project = currentProject;
  readOnly = readOnly || Boolean(entry.archived);
  // 同一条重绘(保存/刷新之后)保住阅读区的滚动位置,换条目才回到顶部。
  const reader = $("memory-reader");
  const keepScroll = box.dataset.memoryId === entry.id && reader ? reader.scrollTop : 0;
  if (reveal) showMemoryTab("read");
  $("memory-reader-empty")?.classList.add("hidden");
  $("memory-scroll")?.classList.add("has-memory-selection");
  delete box.dataset.dirty;
  delete box.dataset.graphNode;
  box.dataset.memoryId = entry.id;
  box.oninput = (event) => {
    const tag = event?.target?.tagName;
    if (!tag || tag === "INPUT" || tag === "TEXTAREA") box.dataset.dirty = "true";
  };
  document.dispatchEvent(new CustomEvent("kz:memory-selected", { detail: { project, scope, id: entry.id, title: displayMemoryTitle(entry.title) } }));
  memoryCurrentEntryId = entry.id;
  memorySelection = { scope, category: entry.category || "all" };
  box.classList.remove("hidden");
  box.innerHTML = "";
  document.querySelectorAll("#memory-list .memory-row.selected").forEach((row) => row.classList.remove("selected"));
  const selected = [...document.querySelectorAll("#memory-list .memory-row")].find((row) => row.dataset.memoryId === entry.id && row.dataset.scope === scope);
  selected?.classList.add("selected");
  const heading = document.createElement("div");
  heading.className = "memory-detail-head";
  const headingTitle = document.createElement("div");
  headingTitle.className = "memory-detail-title";
  headingTitle.textContent = displayMemoryTitle(entry.title) || entry.id;
  const close = document.createElement("button");
  close.type = "button";
  close.className = "ghost mini";
  close.textContent = t("关闭详情");
  close.addEventListener("click", async () => {
    if (!(await confirmLeaveMemoryDetail())) return;
    memoryCurrentEntryId = null;
    hideMemoryDetail();
    document.querySelectorAll("#memory-list .memory-row.selected").forEach((row) => row.classList.remove("selected"));
  });
  heading.append(headingTitle, close);
  const meta = document.createElement("div");
  meta.className = "memory-detail-meta";
  // 来源与引用里的条目编号/路径做成可点 chip(R-/D- 跳条目,路径开文件)。
  const scopeWord = (entry.scope || scope) === "global" ? t("全局记忆") : t("项目记忆");
  meta.replaceChildren(
    document.createTextNode(`${entry.id} · ${scopeWord} / ${memoryWord(entry.category)} · ${memoryWord(entry.status)} · ${t("来源")} `),
    richText(memorySourceText(entry.source) || t("未知")),
  );
  if (entry.refs && entry.refs.length) meta.append(document.createTextNode(` · ${t("引用来源")} `), richText(entry.refs.join(" ")));
  if (readOnly) {
    const badge = document.createElement("span");
    badge.className = "memory-readonly-badge";
    badge.textContent = t("已归档(只读)");
    meta.append(document.createTextNode(" · "), badge);
  }
  const profile = document.createElement("p");
  profile.className = "dim memory-profile";
  const lastHit = entry.last_hit_at ? new Date(entry.last_hit_at).toLocaleString() : t("从未命中");
  profile.textContent = `${t("累计命中")} ${entry.hits ?? 0} · ${t("最近命中")} ${lastHit} · ${t("更新")} ${entry.updated || t("未知")}`;
  const field = (labelText, control, tag = "label") => {
    const wrapper = document.createElement(tag);
    wrapper.className = "memory-detail-field";
    const label = document.createElement("span");
    label.className = "memory-detail-label";
    label.textContent = labelText;
    wrapper.append(label, control);
    return wrapper;
  };
  const title = document.createElement("input");
  title.value = entry.title;
  title.setAttribute("aria-label", t("记忆标题"));
  const desc = document.createElement("textarea");
  desc.rows = 2;
  desc.value = entry.description;
  desc.setAttribute("aria-label", t("召回钩子"));
  const recall = document.createElement("p");
  recall.className = "memory-recall-description";
  recall.textContent = stripMemoryFingerprint(entry.description);
  const bodyBox = document.createElement("div");
  bodyBox.className = "memory-body-read";
  const bodyText = String(entry.body ?? "");
  renderMemoryBodyRead(bodyBox, bodyText);
  // 输入过但又改回原样不算「有改动」:离开前的确认以表单当前值对原值的比较为准。
  box._memoryDirtyCheck = () => title.value !== entry.title || desc.value !== entry.description || readMemoryBody(bodyBox, bodyText) !== bodyText;
  const restoreReaderScroll = () => {
    if (reader) reader.scrollTop = keepScroll;
  };
  if (readOnly) {
    // 归档条目只读:不给保存/删除/改标题,正文不给编辑入口(归档目录里的文件不在写路径上);能做的只有搬回记忆库。
    box.dataset.readonly = "true";
    bodyBox.querySelector(".memory-body-edit-row")?.remove();
    const restore = document.createElement("button");
    restore.type = "button";
    restore.className = "primary";
    restore.textContent = t("恢复为启用");
    restore.title = t("搬回记忆库,重新参与检索与注入");
    restore.addEventListener("click", () => void restoreMemoryEntry(project, scope, entry.id, "active", entry.title));
    const readOnlyActions = document.createElement("div");
    readOnlyActions.className = "memory-detail-actions";
    readOnlyActions.append(restore);
    box.append(heading, meta, profile, recall, field(t("正文"), bodyBox, "div"), readOnlyActions);
    restoreReaderScroll();
    return;
  }
  delete box.dataset.readonly;
  const metadata = document.createElement("details");
  metadata.className = "memory-meta-editor";
  const metadataSummary = document.createElement("summary");
  metadataSummary.textContent = t("编辑标题、召回条件与区域");
  metadata.append(metadataSummary, field(t("标题"), title), field(t("召回钩子"), desc), renderMemoryAreaRow(project, scope, entry));
  const save = document.createElement("button");
  save.type = "button";
  save.className = "primary";
  save.textContent = t("保存修改");
  save.addEventListener("click", async () => {
    try {
      const body = readMemoryBody(bodyBox, bodyText);
      await invoke("memory_entry_save", {
        projectDir: project,
        scope,
        id: entry.id,
        title: title.value,
        description: desc.value,
        body,
        status: null,
      });
      toast(t("记忆已保存"));
      if (project !== currentProject) return;
      delete box.dataset.dirty;
      memoryCurrentEntryId = entry.id;
      document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project } }));
    } catch (err) {
      toastError(`${t("记忆保存失败")}:${err}`);
    }
  });
  const setStatus = (status) => invoke("memory_entry_save", { projectDir: project, scope, id: entry.id, title: null, description: null, body: null, status });
  // 候选/试运行直接启用:越过「按证据晋升」这一步,所以单独叫「启用」并写明,不再冒充「恢复」。
  const enableBtn = document.createElement("button");
  enableBtn.type = "button";
  enableBtn.className = "ghost";
  enableBtn.textContent = t("立即启用");
  enableBtn.title = t("直接启用,不等记忆管理子代理按证据晋升");
  enableBtn.addEventListener("click", async () => {
    try {
      await setStatus("active");
      if (project !== currentProject) return;
      toast(t("已启用"));
      document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project } }));
    } catch (err) {
      toastError(`${t("记忆保存失败")}:${err}`);
    }
  });
  // 「标记失效」实际是归档(搬进 archive/,不再检索与注入):名字照实写,并给撤销与「已归档」视图。
  const archiveBtn = document.createElement("button");
  archiveBtn.type = "button";
  archiveBtn.className = "ghost";
  archiveBtn.textContent = t("归档");
  archiveBtn.title = t("移出检索与注入,文件保留在「已归档」里,随时可以恢复");
  archiveBtn.addEventListener("click", async () => {
    if (!(await confirmLeaveMemoryDetail())) return;
    try {
      await setStatus("deprecated");
      if (project !== currentProject) return;
      memoryCurrentEntryId = null;
      hideMemoryDetail();
      dropMemoryRow(scope, entry.id);
      showMemoryNotice(`${t("已归档")}「${displayMemoryTitle(entry.title) || entry.id}」`, {
        label: t("撤销归档"),
        run: () => void restoreMemoryEntry(project, scope, entry.id, entry.status, entry.title),
      });
      document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project } }));
    } catch (err) {
      toastError(`${t("记忆保存失败")}:${err}`);
    }
  });
  const deleteBtn = document.createElement("button");
  deleteBtn.type = "button";
  deleteBtn.className = "ghost danger";
  deleteBtn.textContent = t("删除");
  deleteBtn.title = t("从磁盘删除该记忆文件,不可撤销");
  deleteBtn.addEventListener("click", async () => {
    if (!(await confirmDialog({ title: t("确认删除"), message: `「${displayMemoryTitle(entry.title) || entry.id}」(${entry.id})?${t("此操作不可撤销")}`, okText: t("删除"), danger: true }))) return;
    try {
      await invoke("memory_entry_delete", { projectDir: project, scope, id: entry.id });
      if (project !== currentProject) return;
      toast(t("已删除"));
      memoryCurrentEntryId = null;
      hideMemoryDetail();
      dropMemoryRow(scope, entry.id);
      document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project } }));
    } catch (err) {
      toastError(`${t("删除失败")}:${err}`);
    }
  });
  const actions = document.createElement("div");
  actions.className = "memory-detail-actions";
  const discuss = document.createElement("button");
  discuss.type = "button";
  discuss.className = "ghost";
  discuss.textContent = t("用对话修改");
  discuss.addEventListener("click", () => { showMemoryTab("chat"); $("memory-chat-input")?.focus(); });
  actions.append(save, discuss);
  if (entry.status === "candidate" || entry.status === "shadow") actions.append(enableBtn);
  actions.append(archiveBtn, deleteBtn);
  box.append(heading, meta, profile, recall, field(t("正文"), bodyBox, "div"), metadata, actions);
  restoreReaderScroll();
}

// 记忆图谱:详情里的「区域」行——当前区域(带依据徽标:字段/路径/工具/经由 D-xxx/关键词)+ 设为/清除区域。
// 只有字段(area:)是真源,其余是图谱按信号推断的;「设为区域」把推断写成字段,优先级最高。
function renderMemoryAreaRow(project, scope, entry) {
  const row = document.createElement("div");
  row.className = "memory-area-row";
  const label = document.createElement("span");
  label.className = "memory-detail-label";
  label.textContent = t("区域");
  const list = document.createElement("span");
  list.className = "memory-area-list";
  const fieldAreas = (entry.areas ?? []).map((area) => ({ area: `area:${area}`, provenance: "field", via: null }));
  // 字段(entry.areas,刚从磁盘读的真源)总排最前;推断区域来自图谱载荷,去掉与字段重复的和载荷里的字段边
  // ——载荷可能比条目旧一拍,字段以条目为准。inferred 为 null = 载荷还没取到(或刚被改动作废),末尾显示「…」。
  const renderChips = (inferred, { pending = false } = {}) => {
    const fieldSet = new Set(fieldAreas.map((item) => item.area));
    const shown = [...fieldAreas, ...(inferred ?? []).filter((item) => item.provenance !== "field" && !fieldSet.has(item.area))];
    list.replaceChildren();
    if (!shown.length && !pending) {
      const none = document.createElement("span");
      none.className = "dim";
      none.textContent = t("未归类");
      list.append(none);
    }
    for (const item of shown) {
      const chip = document.createElement("span");
      chip.className = "memory-area-chip";
      chip.dataset.provenance = item.provenance || "";
      const name = document.createElement("span");
      name.textContent = String(item.area || "").replace(/^area:/, "");
      const why = document.createElement("em");
      why.className = "memory-area-why";
      const provenanceLabel = { field: t("字段"), path: t("路径"), tool: t("工具"), via: t("经由"), keyword: t("关键词") }[item.provenance] ?? "";
      why.textContent = item.provenance === "via" && item.via ? `${provenanceLabel} ${item.via}` : provenanceLabel;
      chip.append(name, why);
      list.append(chip);
    }
    if (pending) {
      const more = document.createElement("span");
      more.className = "dim memory-area-pending";
      more.textContent = "…";
      list.append(more);
    }
  };
  // 推断出的区域来自图谱载荷;列表模式下还没取过(或刚改过区域、载荷作废)就先显示字段 +「…」,取到后补上推断。
  const inferred = memoryAreaProvider?.areasFor?.(scope, entry.id) ?? null;
  renderChips(inferred, { pending: !inferred && Boolean(memoryAreaProvider) });
  const select = document.createElement("select");
  select.id = "memory-area-select";
  select.setAttribute("aria-label", t("选择代码区域"));
  const pending = document.createElement("option");
  pending.value = "";
  pending.textContent = t("选择区域…");
  select.append(pending);
  const setBtn = document.createElement("button");
  setBtn.type = "button";
  setBtn.className = "ghost mini";
  setBtn.textContent = t("设为区域");
  const clearBtn = document.createElement("button");
  clearBtn.type = "button";
  clearBtn.className = "ghost mini";
  clearBtn.textContent = t("清除区域");
  clearBtn.disabled = !(entry.areas ?? []).length;
  const saveArea = async (areas) => {
    try {
      await invoke("memory_entry_save", { projectDir: project, scope, id: entry.id, title: null, description: null, body: null, status: null, area: areas });
      if (project !== currentProject) return;
      toast(areas.length ? t("区域已保存") : t("区域已清除"));
      document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project } }));
    } catch (err) {
      toastError(`${t("区域保存失败")}:${err}`);
    }
  };
  setBtn.addEventListener("click", () => {
    if (!select.value) {
      select.focus();
      return;
    }
    const current = (entry.areas ?? []).filter((area) => area !== select.value);
    void saveArea([select.value, ...current].slice(0, 4));
  });
  clearBtn.addEventListener("click", () => void saveArea([]));
  const fill = (options) => {
    let group = null;
    let groupName = null;
    for (const option of options ?? []) {
      if (option.group !== groupName) {
        groupName = option.group;
        group = document.createElement("optgroup");
        group.label = option.group;
        select.append(group);
      }
      const node = document.createElement("option");
      node.value = option.id;
      node.textContent = option.label;
      (group ?? select).append(node);
    }
  };
  void memoryAreaProvider?.ensureAreaOptions?.().then((options) => {
    if (row.isConnected === false) return;
    fill(options);
    renderChips(memoryAreaProvider?.areasFor?.(scope, entry.id) ?? null);
  }).catch(() => renderChips(null));
  const controls = document.createElement("span");
  controls.className = "memory-area-controls";
  controls.append(select, setBtn, clearBtn);
  row.append(label, list, controls);
  return row;
}


// Read the complete document with its Markdown hierarchy; editing remains explicit.
export function renderMemoryBodyRead(container, bodyText) {
  container.innerHTML = "";
  const text = String(bodyText ?? "");
  const article = document.createElement("article");
  article.className = "memory-body-document markdown";
  renderMarkdownInto(article, stripMemoryFingerprint(text) || t("无正文"));
  container.appendChild(article);
  // 编辑入口:阅读视图是默认态,编辑时提供取消,避免误入 textarea 后只能刷新页面恢复阅读。
  const editRow = document.createElement("div");
  editRow.className = "memory-body-edit-row";
  const editBtn = document.createElement("button");
  editBtn.type = "button";
  editBtn.className = "ghost mini";
  editBtn.textContent = t("编辑正文");
  editBtn.addEventListener("click", () => {
    const ta = document.createElement("textarea");
    ta.rows = 16;
    ta.value = readMemoryBody(container, text);
    ta.setAttribute("aria-label", t("记忆正文"));
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "ghost mini";
    cancel.textContent = t("取消编辑");
    cancel.addEventListener("click", () => renderMemoryBodyRead(container, text));
    container.replaceChildren(ta, cancel);
  });
  editRow.appendChild(editBtn);
  container.appendChild(editRow);
}

// R-129:取正文当前值——编辑模式(textarea 在场)读 textarea,阅读模式回原文。
export function readMemoryBody(container, fallback) {
  const ta = container.querySelector("textarea[aria-label]");
  return ta ? ta.value : String(fallback ?? "");
}

// R-129:按空行拆段(兼容 \r\n),只保留非空段。
export function splitMemoryParagraphs(text) {
  return String(text ?? "")
    .split(/\r?\n\s*\r?\n/)
    .map((s) => s.trim())
    .filter(Boolean);
}

// 条目"年纪":用于零命中判定——刚写下来还没被检索过不算没用。
export function memoryAgeDays(updated) {
  const stamp = Date.parse(`${updated}T00:00:00Z`);
  if (Number.isNaN(stamp)) return 0;
  return Math.max(0, Math.floor((Date.now() - stamp) / 86_400_000));
}



// 退出搜索态回到普通列表(清空输入框 → 重载列表;已打开的详情保留)。
async function exitMemorySearch() {
  clearMemorySearch();
  await loadMemoryList(memoryManagerFilters.scope, memoryManagerFilters.category, { preserveSelection: true });
}

defer(() => {
  const tabs = MEMORY_TABS;
  for (const [index, tab] of tabs.entries()) {
    const button = $(`memory-${tab}-tab`);
    button?.addEventListener("click", () => showMemoryTab(tab));
    button?.addEventListener("keydown", event => {
      if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
      event.preventDefault();
      const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : (index + (event.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length;
      showMemoryTab(tabs[next]); $(`memory-${tabs[next]}-tab`).focus();
    });
  }
  document.addEventListener("kz:memory-changed", event => {
    if (event.detail?.project === currentProject) void refreshMemory({ force: true });
  });
  // 切换语言:下拉选项只在首次建一次,文案跟着换;提示条与待整理状态按新语言重写。
  document.addEventListener("kz:language", () => {
    relabelMemoryManagerFilters();
    renderMemoryPendingState();
  });
  // 没有项目时的空态:直接在这里添加(复用侧栏那个按钮),研究档下还能一步回开发档。
  // 新建记忆:没有表单——到管理对话里说明要记住什么,由记忆管理子代理整理成条目(它会先查重、再写召回钩子)。
  $("memory-new-btn")?.addEventListener("click", () => {
    // 新建不是改选中的那条:先清掉管理对话的目标(上下文芯片),否则发送时 target 仍指向上一条,管理子代理可能去改它。
    document.dispatchEvent(new CustomEvent("kz:memory-selection-cleared", { detail: { project: currentProject } }));
    showMemoryTab("chat");
    const chatInput = $("memory-chat-input");
    if (!chatInput) return;
    if (!chatInput.value.trim()) {
      chatInput.value = t("请帮我新增一条记忆：");
      chatInput.dispatchEvent(new Event("input"));
    }
    chatInput.focus();
    chatInput.setSelectionRange?.(chatInput.value.length, chatInput.value.length);
  });
  $("memory-empty-add")?.addEventListener("click", () => $("project-add")?.click());
  $("memory-empty-dev")?.addEventListener("click", () => document.querySelector('[data-workspace="dev"]')?.click());
  const input = $("memory-search-input");
  const clear = $("memory-search-clear");
  let searchTimer = 0;
  const runSearch = async () => {
    clearTimeout(searchTimer);
    const project = currentProject;
    const query = input.value.trim();
    if (!project) return;
    if (!query) {
      if (memorySearchQuery) await exitMemorySearch();
      return;
    }
    if (query === memorySearchQuery) return;
    const generation = ++memoryListGeneration;
    clear.hidden = false;
    neuralFlowEmit?.("memory_search_started", { query_length: query.length });
    try {
      const hits = await invoke("memory_search_page", { projectDir: project, query, limit: MEMORY_SEARCH_LIMIT });
      if (project !== currentProject || generation !== memoryListGeneration) return;
      neuralFlowEmit?.("memory_search_completed", { hit_count: hits.length });
      // 搜索只换结果列表,不碰已经打开的详情(里面可能有没保存的修改)。
      memorySearchQuery = query;
      renderMemoryList(hits.map((hit) => ({ ...hit, scope: hit.scope || "project" })), { search: true, truncated: hits.length >= MEMORY_SEARCH_LIMIT });
      document.dispatchEvent(new CustomEvent("kz:memory-search-hits", { detail: { project, ids: hits.map((hit) => hit.id), query } }));
    } catch (err) {
      if (project !== currentProject || generation !== memoryListGeneration) return;
      neuralFlowEmit?.("memory_search_failed");
      toastError(`${t("记忆检索失败")}:${err}`);
    }
  };
  // 点「刷新」:普通列表整体重载;搜索结果态下重跑这次搜索(后台刷新不会动结果列表,这里是用户明确要新结果)。
  $("memory-refresh")?.addEventListener("click", () => {
    void refreshMemory({ force: true });
    if (!memorySearchQuery) return;
    memorySearchQuery = "";
    void runSearch();
  });
  input.addEventListener("keydown", async (event) => {
    if (event.key === "Enter" && !isImeComposing(event)) await runSearch();
  });
  // 边打边搜(去抖),不再只认 Enter;输入法选词中不触发,选定后(compositionend)再算。
  const scheduleSearch = (event) => {
    clear.hidden = !input.value.trim();
    clearTimeout(searchTimer);
    if (isImeComposing(event)) return;
    searchTimer = setTimeout(() => void runSearch(), 350);
  };
  input.addEventListener("input", scheduleSearch);
  input.addEventListener("compositionend", scheduleSearch);
  clear.addEventListener("click", async () => {
    clearTimeout(searchTimer);
    input.value = "";
    clear.hidden = true;
    await exitMemorySearch();
  });
});

defer(() => {
  const consolidate = async () => {
    const project = currentProject;
    if (!project || memoryConsolidating) return;
    memoryConsolidating = true;
    renderMemoryPendingState();
    neuralFlowEmit?.("memory_consolidation_started");
    try {
      const result = await invoke("memory_consolidate", { projectDir: project });
      // 整理失败(模型不可用、批次没进展)后端仍返回报告而不是报错:把原因说出来并给重试,不再只说「尚有草稿」。
      const failure = memoryConsolidationFailure(result?.report);
      neuralFlowEmit?.(failure ? "memory_consolidation_failed" : result?.pending ? "memory_consolidation_partial" : "memory_consolidation_completed", { pending: Boolean(result?.pending) });
      if (failure) toastError(`${t("整理失败")}:${failure}`, { retry: consolidate });
      else toast(result?.pending ? t("还有待整理条目没处理完") : t("待整理条目已全部整理"));
    } catch (err) {
      neuralFlowEmit?.("memory_consolidation_failed");
      toastError(`${t("整理失败")}:${err}`, { retry: consolidate });
    } finally {
      memoryConsolidating = false;
      if (project === currentProject) void refreshMemory({ force: true });
      else renderMemoryPendingState();
    }
  };
  $("memory-consolidate-btn")?.addEventListener("click", () => void consolidate());
});
