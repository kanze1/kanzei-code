import { defer } from "./01-core.js";
import { escapeHtml, renderMarkdown } from "./04-markdown.js";
import { $, confirmDialog, invoke, on, replayExperienceFacts, uiConsoleLog } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, toast, toastError } from "./03-shell.js";
import { latestDocsSnapshot } from "./12-docs-pages.js";
import { neuralFlowEmit } from "./22-neural-flow.js";
import { richText } from "./04-structured.js";

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

function memoryFilterLabel(value) {
  return value === "all" ? t("全部") : t(value);
}

function setupMemoryManagerFilters() {
  const definitions = [
    ["memory-scope-filter", [["project", t("项目记忆")], ["global", t("全局记忆")], ["all", t("全部")]]],
    ["memory-category-filter", [["all", t("全部分类")], ["fact", "fact"], ["sop", "sop"], ["habit", "habit"], ["preference", "preference"], ["episode", "episode"]]],
    ["memory-status-filter", [["active", "active"], ["candidate", "candidate"], ["shadow", "shadow"], ["stale", "stale"], ["all", t("全部状态")]]],
    ["memory-sort-filter", [["updated", t("最近更新")], ["hits", t("命中最多")], ["title", t("标题")], ["id", t("ID")]]],
  ];
  for (const [id, options] of definitions) {
    const select = $(id);
    if (!select || select.options.length) continue;
    for (const [value, label] of options) {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = label;
      select.appendChild(option);
    }
    select.value = memoryManagerFilters[id.replace("memory-", "").replace("-filter", "")];
    select.addEventListener("change", () => {
      const key = id === "memory-scope-filter" ? "scope" : id === "memory-category-filter" ? "category" : id === "memory-status-filter" ? "status" : "sort";
      memoryManagerFilters[key] = select.value;
      memorySelection = { scope: memoryManagerFilters.scope, category: memoryManagerFilters.category };
      memoryCurrentEntryId = null;
      hideMemoryDetail();
      loadMemoryList(memoryManagerFilters.scope, memoryManagerFilters.category);
      // 记忆图谱(24-memory-graph.js)跟着同一组筛选重算可见子图。
      document.dispatchEvent(new CustomEvent("kz:memory-filters", { detail: { project: currentProject, filters: memoryFilterSnapshot() } }));
    });
  }
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
    diagnosticsProject = null;
    diagnosticsPending = null;
    ++memoryListGeneration;
    hideMemoryDetail();
    if (project) $("memory-reader-empty").innerHTML = `<strong>${t("选择一条记忆")}</strong><p>${t("在这里阅读全文，或通过管理对话修改。")}</p>`;
    document.dispatchEvent(new CustomEvent("kz:memory-project", { detail: { project } }));
    for (const id of ["memory-recalls", "memory-control-plane", "memory-list", "memory-candidates", "memory-value-flags", "memory-bill", "memory-arch"]) $(id)?.replaceChildren();
  }
  if (!currentProject) {
    $("memory-reader-empty").textContent = t("先在左侧「项目」里添加并选择一个目录");
    return;
  }
  try {
    setupMemoryManagerFilters();
    if (force) { memoryEntryCache.clear(); diagnosticsProject = null; diagnosticsPending = null; }
    await loadMemoryList(memoryManagerFilters.scope, memoryManagerFilters.category, { preserveSelection: true });
    if (project !== currentProject || generation !== memoryRefreshGeneration) return;
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
      ["memory_context_bill", {}, renderMemoryBill],
      ["memory_recalls", { limit: 20 }, renderMemoryRecalls],
      ["memory_note_candidates", {}, renderMemoryCandidates],
      ["memory_value_flags", {}, renderMemoryValueFlags],
      ["memory_control_plane", {}, data => { replayExperienceFacts(data?.experience_facts); renderMemoryControlPlane(data); }],
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

export function showMemoryTab(tab) {
  $("memory-scroll").dataset.tab = tab;
  for (const name of ["read", "chat", "insights"]) {
    const active = name === tab;
    $(`memory-${name}-tab`)?.setAttribute("aria-selected", String(active));
    if ($(`memory-${name}-tab`)) $(`memory-${name}-tab`).tabIndex = active ? 0 : -1;
    $(`memory-${name === "read" ? "reader" : name}`)?.classList.toggle("hidden", !active);
  }
  if (tab === "insights") void loadMemoryDiagnostics();
  if (tab === "chat") document.dispatchEvent(new CustomEvent("kz:memory-chat-open"));
  document.dispatchEvent(new CustomEvent("kz:memory-tab", { detail: { tab } }));
}

export function renderMemoryControlPlane(data) {
  const box = $("memory-control-plane");
  if (!box) return;
  box.innerHTML = "";
  const batch = data?.batch || {};
  const recall = data?.recall || {};
  const effects = Array.isArray(data?.effects) ? data.effects : [];
  const facts = [
    [t("待整理 backlog"), data?.backlog ?? 0],
    [t("最老等待"), data?.oldest_waiting || t("暂无")],
    [t("晋升缺口"), data?.promotion_gaps ?? 0],
    [t("召回 / 注入 / 正文读取"), `${recall.recalled ?? 0} / ${recall.injected ?? 0} / ${recall.read_observed ? recall.read : t("未知")}`],
    [t("召回关联"), `${recall.events_linked ?? 0}/${recall.events_total ?? 0} · ${t("悬空")} ${recall.events_orphaned ?? 0}`],
    [t("收益评估"), effects.length ? `${effects.length} ${t("条")}` : t("尚无有效对照")],
  ];
  const summary = document.createElement("div");
  summary.className = "memory-control-summary";
  for (const [label, value] of facts) {
    const cell = document.createElement("div");
    cell.className = "memory-control-cell";
    const name = document.createElement("span");
    name.className = "dim";
    name.textContent = label;
    const content = document.createElement("strong");
    content.textContent = String(value);
    cell.append(name, content);
    summary.appendChild(cell);
  }
  box.appendChild(summary);
  const status = document.createElement("div");
  status.className = `memory-control-status${batch.status === "failed" ? " failed" : ""}`;
  status.textContent = batch.batch_id
    ? `${t("最近批次")} ${batch.batch_id} · ${batch.status || t("未知")}${batch.pending_after == null ? "" : ` · ${t("剩余")} ${batch.pending_after}`}${batch.failure_reason ? ` · ${batch.failure_reason}` : ""}`
    : t("尚无整理批次");
  box.appendChild(status);
  if (batch.status === "failed" || batch.failure_reason) {
    const retry = document.createElement("button");
    retry.type = "button";
    retry.className = "ghost mini";
    retry.textContent = t("重试整理");
    retry.addEventListener("click", () => $("memory-consolidate-btn")?.click());
    box.appendChild(retry);
  }
  if (effects.length) {
    const effectList = document.createElement("div");
    effectList.className = "memory-control-effects dim";
    effectList.textContent = effects
      .slice(0, 5)
      .map((effect) => `${effect.memory_id}: ${Number(effect.effect_mean).toFixed(2)} ± ${Number(effect.effect_ci).toFixed(2)} (n=${effect.eval_n})`)
      .join(" · ");
    box.appendChild(effectList);
  }
}

// ---------- R-127 运行画像面板 ----------
// 判断 agent 跑得好不好此前全靠翻轨迹。数据源早就有(RunSummary 的 context_report、
// summarize_tools、summarize_metrics),缺的只是把它们汇到一处。
export async function refreshMetrics() {
  if (!currentProject) {
    $("metrics-tasks").innerHTML = `<p class="dim">${t("先在左侧「项目」里添加并选择一个目录")}</p>`;
    $("metrics-rounds").innerHTML = `<p class="dim">${t("先在左侧「项目」里添加并选择一个目录")}</p>`;
    return;
  }
  try {
    const [data, cats, taskData] = await Promise.all([
      invoke("run_metrics", { projectDir: currentProject, limit: 20 }),
      invoke("run_metrics_by_category", { projectDir: currentProject, limit: 200 }),
      invoke("run_metrics_by_task", { projectDir: currentProject }),
    ]);
    const incidentData = latestDocsSnapshot?.incident_metrics
      ?? (await invoke("docs_snapshot", { projectDir: currentProject })).incident_metrics;
    renderTaskMetrics(taskData ?? {});
    renderMetrics(data?.rounds ?? []);
    renderMetricsCategories(cats ?? {});
    renderIncidentMetrics(incidentData, "metrics-incident");
  } catch (err) {
    toastError(`${t("运行画像加载失败")}:${err}`, { retry: refreshMetrics });
  }
}

export function renderTaskMetrics(data) {
  const box = $("metrics-tasks");
  if (!box) return;
  box.replaceChildren();
  const completed = data.completed_tasks ?? [];
  const inProgress = data.in_progress_tasks ?? [];
  const trend = data.trend ?? {};
  const heading = document.createElement("div");
  heading.className = "metrics-trend-head dim";
  heading.textContent = `${t("任务画像")} · ${trend.closed_task_count ?? 0} ${t("已关闭任务")} · ${inProgress.length} ${t("进行中任务")}`;
  box.appendChild(heading);
  if (!completed.length && !inProgress.length) {
    const empty = document.createElement("p");
    empty.className = "dim";
    empty.textContent = t("暂无任务画像");
    box.appendChild(empty);
    return;
  }
  appendTaskGroup(box, t("已关闭任务"), completed);
  appendTaskGroup(box, t("进行中任务"), inProgress);
}

function appendTaskGroup(box, label, tasks) {
  if (!tasks.length) return;
  const group = document.createElement("section");
  group.className = "metrics-task-group";
  const title = document.createElement("h2");
  title.className = "metrics-trend-head";
  title.textContent = `${label} (${tasks.length})`;
  group.appendChild(title);
  for (const task of tasks) {
    const details = document.createElement("details");
    details.className = "metrics-task-card";
    const summary = document.createElement("summary");
    const status = {
      completed: "已完成",
      failed: "失败",
      cancelled: "已取消",
      abandoned: "已放弃",
      in_progress: "进行中",
    }[task.status] ?? task.status ?? "未知";
    summary.textContent = `${task.title || task.task_id} · ${t(status)} · ${task.round_count ?? 0} ${t("轮")}`;
    details.appendChild(summary);
    const meta = document.createElement("div");
    meta.className = "metrics-round-stats dim";
    meta.textContent = `${task.task_id} · ${t("会话")} ${task.session_ids?.join(", ") || "—"} · ${t("输入")} ${task.input_count ?? 0} · ${t("步")} ${task.steps_sum ?? 0} · ↑${task.input_tokens_sum ?? 0} ↓${task.output_tokens_sum ?? 0}`;
    details.appendChild(meta);
    const rounds = document.createElement("div");
    rounds.className = "metrics-task-rounds";
    for (const round of task.rounds ?? []) {
      const item = document.createElement("div");
      item.className = "metrics-round-tools dim";
      item.textContent = `${new Date(round.created_at).toLocaleString()} · ${round.session_id} · ${round.outcome} · ${round.steps} ${t("步")} · ↑${round.input_tokens} ↓${round.output_tokens}`;
      rounds.appendChild(item);
    }
    details.appendChild(rounds);
    group.appendChild(details);
  }
  box.appendChild(group);
}

// R-240:按需求类型(R-/D-)与复杂度(小/中/大)聚合的 token 指标,方便针对
// 上下文与 harness 做优化分析。
export function renderMetricsCategories(cats) {
  const box = $("metrics-categories");
  box.innerHTML = "";
  const groups = cats.groups ?? [];
  if (!groups.length) {
    return;
  }
  const head = document.createElement("div");
  head.className = "metrics-trend-head dim";
  head.textContent = t("按分类聚合(类型 × 复杂度)");
  const table = document.createElement("table");
  table.className = "metrics-cat-table";
  const headRow = document.createElement("tr");
  for (const cell of [t("类型"), t("复杂度"), t("轮数"), t("输入合计"), t("输出合计"), t("平均输入"), t("平均输出")]) {
    const th = document.createElement("th");
    th.textContent = cell;
    headRow.appendChild(th);
  }
  table.appendChild(headRow);
  for (const g of groups) {
    const tr = document.createElement("tr");
    for (const value of [
      g.kind, g.complexity, g.count,
      g.sumInput, g.sumOutput,
      Math.round(g.avgInput), Math.round(g.avgOutput),
    ]) {
      const td = document.createElement("td");
      td.textContent = value;
      tr.appendChild(td);
    }
    table.appendChild(tr);
  }
  const unc = cats.uncategorized ?? { count: 0 };
  if (unc.count > 0) {
    const tr = document.createElement("tr");
    tr.className = "dim";
    for (const value of [t("未归类"), "—", unc.count, unc.sumInput ?? 0, unc.sumOutput ?? 0, "—", "—"]) {
      const td = document.createElement("td");
      td.textContent = value;
      tr.appendChild(td);
    }
    table.appendChild(tr);
  }
  box.append(head, table);
}

export function renderMetrics(rounds) {
  const trend = $("metrics-trend");
  const list = $("metrics-rounds");
  trend.innerHTML = "";
  list.innerHTML = "";
  if (!rounds.length) {
    list.innerHTML = `<p class="dim">${t("还没有轮次记录:跑一轮后这里会出现画像")}</p>`;
    return;
  }
  // 趋势:只统计确实度量过的轮次。把"早于度量落地"的轮次算成 0 会把趋势整体压低,
  // 得出"冗余在下降"的假结论。
  const measured = rounds.filter((r) => r.measured);
  if (measured.length) {
    const avg = (pick) => measured.reduce((sum, r) => sum + (pick(r) || 0), 0) / measured.length;
    const cells = [
      [t("平均终端调用"), avg((r) => r.metrics.terminal_calls).toFixed(1)],
      [t("平均 git 查询组"), avg((r) => r.metrics.git_groups).toFixed(1)],
      [t("edit 未命中率"), `${(avg((r) => (r.metrics.edit_calls ? r.metrics.edit_misses / r.metrics.edit_calls : 0)) * 100).toFixed(0)}%`],
      [t("平均步数"), avg((r) => r.steps).toFixed(1)],
      [t("平均输出 token"), Math.round(avg((r) => r.outputTokens))],
    ];
    trend.innerHTML =
      `<div class="metrics-trend-head dim">${t("近")} ${measured.length} ${t("轮均值")}</div>` +
      cells
        .map(([name, value]) => `<div class="metrics-cell"><span class="dim">${escapeHtml(name)}</span><strong>${escapeHtml(String(value))}</strong></div>`)
        .join("");
  }
  for (const round of rounds) {
    const item = document.createElement("div");
    item.className = `metrics-round${round.outcome === "halted" ? " halted" : ""}`;
    const m = round.metrics || {};
    const contextTotal = Object.values(round.context || {}).reduce(
      (sum, entry) => sum + (Array.isArray(entry) ? entry[1] : Number(entry) || 0),
      0,
    );
    const head = document.createElement("div");
    head.className = "metrics-round-head";
    head.innerHTML =
      `<span>${escapeHtml(new Date(round.at).toLocaleString())}</span>` +
      `<span class="dim">${escapeHtml(round.outcome)} · ${round.steps} ${t("步")} · ↑${round.inputTokens} ↓${round.outputTokens}</span>`;
    const prompt = document.createElement("div");
    prompt.className = "metrics-round-prompt dim";
    prompt.textContent = round.prompt;
    const stats = document.createElement("div");
    stats.className = "metrics-round-stats dim";
    stats.textContent = round.measured
      ? `${t("终端")} ${m.terminal_calls ?? 0} · git ${m.git_calls ?? 0}(${m.git_groups ?? 0} ${t("组")}) · edit ${m.edit_misses ?? 0}/${m.edit_calls ?? 0} ${t("故障")} (${m.edit_rejections ?? 0} ${t("受控拒绝")}) · ${t("子代理")} ${m.subagent_calls ?? 0} · ${t("失败")} ${m.failed_calls ?? 0}/${m.total_calls ?? 0} · ${t("上下文")} ${contextTotal}${redundantLine(m)}`
      : t("该轮早于度量落地,无画像");
    const tools = document.createElement("div");
    tools.className = "metrics-round-tools dim";
    tools.textContent = Object.entries(round.tools || {})
      .sort((a, b) => b[1] - a[1])
      .map(([name, count]) => `${name}×${count}`)
      .join("  ");
    item.append(head, prompt, stats, tools);
    list.appendChild(item);
  }
}

export function redundantLine(m) {
  const total = (m.redundant_git ?? 0) + (m.redundant_test ?? 0) + (m.redundant_task ?? 0);
  if (total === 0) return "";
  const parts = [];
  if (m.redundant_git) parts.push(`git×${m.redundant_git}`);
  if (m.redundant_test) parts.push(`test×${m.redundant_test}`);
  if (m.redundant_task) parts.push(`task×${m.redundant_task}`);
  return ` · ${t("冗余提醒")} ${parts.join(" ")}`;
}

export const INCIDENT_CLASS_LABELS = Object.freeze({
  execution_incident: "execution_incident",
  development_defect: "development_defect",
  product_defect: "product_defect",
  regression: "regression",
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
      INCIDENT_CLASS_LABELS[className],
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

// R-124:待确认候选。SOP 是用户的常用模板,不能由 agent 自己决定入库——
// 所以候选只停在这里,采纳/丢弃都是用户一键的事。
export function renderMemoryCandidates(list) {
  const box = $("memory-candidates");
  const count = $("memory-candidate-count");
  if (!box) return;
  box.innerHTML = "";
  const items = Array.isArray(list) ? list : [];
  count.textContent = items.length ? `· ${items.length}` : "";
  if (!items.length) {
    box.innerHTML = `<p class="dim">${t("暂无待确认候选")}</p>`;
    return;
  }
  for (const item of items) {
    const row = document.createElement("div");
    row.className = `memory-candidate${item.hint === "sop" ? " sop" : ""}`;
    row.dataset.fingerprint = item.fingerprint || "";
    const head = document.createElement("div");
    head.className = "memory-candidate-head";
    head.innerHTML =
      `<span class="memory-candidate-hint">${escapeHtml(item.hint || "note")}</span>` +
      `<span class="memory-candidate-summary">${escapeHtml(item.summary || "")}</span>`;
    // UI-0926 #10:候选正文是 markdown(列表/代码/路径),按 markdown 渲染而不是原文堆字。
    const detail = document.createElement("div");
    detail.className = "memory-candidate-detail dim md sv-md";
    detail.innerHTML = renderMarkdown(item.detail || "");
    const actions = document.createElement("div");
    actions.className = "memory-candidate-actions";
    const adopt = document.createElement("button");
    adopt.type = "button";
    adopt.className = "primary mini";
    adopt.textContent = t("采纳");
    adopt.title = t("交给记忆管理子代理提炼成条目");
    adopt.addEventListener("click", async () => {
      adopt.disabled = true;
      neuralFlowEmit?.("memory_consolidation_started", { fingerprint: item.fingerprint });
      try {
        const result = await invoke("memory_consolidate", { projectDir: currentProject });
        neuralFlowEmit?.(result?.pending ? "memory_consolidation_partial" : "memory_consolidation_completed", { fingerprint: item.fingerprint });
        toast(t("已交给记忆管理子代理提炼"));
        refreshMemory({ force: true });
      } catch (err) {
        neuralFlowEmit?.("memory_consolidation_failed", { fingerprint: item.fingerprint });
        adopt.disabled = false;
        toastError(`${t("提炼失败")}:${err}`);
      }
    });
    const drop = document.createElement("button");
    drop.type = "button";
    drop.className = "ghost mini danger";
    drop.textContent = t("丢弃");
    drop.title = t("直接移出候选箱,不再进入提炼范围");
    drop.addEventListener("click", async () => {
      try {
        await invoke("memory_note_discard", {
          projectDir: currentProject,
          scope: item.scope,
          fingerprint: item.fingerprint,
        });
        neuralFlowEmit?.("memory_candidate_discarded", { fingerprint: item.fingerprint });
        toast(t("已丢弃"));
        refreshMemory({ force: true });
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
        `<span class="memory-row-title">${escapeHtml(item.title)}</span>` +
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
        `<span class="memory-row-title">${escapeHtml(item.title)}</span>` +
        `<span class="dim">${t("召回")} ${item.recalled} · ${t("注入")} ${item.injected} · ${t("正文读取")} ${item.read_observed ? item.read : t("未知")}</span>`;
      row.addEventListener("click", () => openMemoryDetailById(item.scope, item.id));
      box.appendChild(row);
    }
  }
}

// 从清单跳详情:按 scope+id 定位条目并复用现有详情渲染。
export async function openMemoryDetailById(scope, id) {
  const project = currentProject;
  const generation = ++memoryListGeneration;
  try {
    const list = await getMemoryEntries(project, scope);
    if (project !== currentProject || generation !== memoryListGeneration) return;
    const entry = (list || []).find((e) => e.id === id);
    if (entry) {
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
        `<span class="memory-recall-title">${escapeHtml(hit.title)}</span>` +
        `<span class="memory-recall-flag">${escapeHtml(state)}</span>`;
      item.appendChild(row);
    }
    box.appendChild(item);
  }
}

export function renderMemoryArch(overview) {
  const arch = $("memory-arch");
  arch.innerHTML = "";
  let inboxPending = 0;
  for (const scope of overview.scopes || []) {
    inboxPending += scope.inboxPending || 0;
    const card = document.createElement("div");
    card.className = "memory-scope-card";
    const head = document.createElement("div");
    head.className = "memory-scope-head";
    const label = scope.scope === "global" ? t("全局记忆") : t("项目记忆");
    head.innerHTML = `<strong>${label}</strong> <span class="dim">${scope.total} ${t("条")} · ${t("命中")} ${scope.hitsTotal} · ${escapeHtml(scope.root)}</span>`;
    card.appendChild(head);
    const grid = document.createElement("div");
    grid.className = "memory-cat-grid";
    for (const [cat, info] of Object.entries(scope.categories || {})) {
      const cell = document.createElement("button");
      cell.type = "button";
      cell.className = "memory-cat-cell";
      cell.setAttribute("aria-label", `${label} ${cat}`);
      const staleNote = info.stale ? `${info.stale} stale` : "";
      cell.innerHTML = `<span class="memory-cat-name">${escapeHtml(cat)}</span><span class="memory-cat-count">${info.active}</span><span class="dim">${staleNote} ${escapeHtml(info.last || "")}</span>`;
      cell.addEventListener("click", () => {
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
  $("memory-inbox-badge").textContent = inboxPending ? `inbox ${inboxPending} ${t("条待整理")}` : "";
}

export async function loadMemoryList(scope, category, { preserveSelection = false } = {}) {
  const project = currentProject;
  const generation = ++memoryListGeneration;
  try {
    memoryManagerFilters.scope = scope || "project";
    memoryManagerFilters.category = category || "all";
    syncMemoryManagerFilters();
    const scopes = memoryManagerFilters.scope === "all" ? ["project", "global"] : [memoryManagerFilters.scope];
    const results = await Promise.all(scopes.map(async itemScope => (await getMemoryEntries(project, itemScope)).map(entry => ({ ...entry, scope: itemScope }))));
    if (project !== currentProject || generation !== memoryListGeneration) return;
    memoryListEntries = results.flat().map((entry) => ({ ...entry, scope: entry.scope || memoryManagerFilters.scope }));
    const filtered = memoryListEntries
      .filter((entry) => memoryManagerFilters.category === "all" || entry.category === memoryManagerFilters.category)
      .filter((entry) => memoryManagerFilters.status === "all" || entry.status === memoryManagerFilters.status)
      .sort((a, b) => {
        if (memoryManagerFilters.sort === "hits") return (b.hits ?? 0) - (a.hits ?? 0) || a.id.localeCompare(b.id, undefined, { numeric: true });
        if (memoryManagerFilters.sort === "title") return a.title.localeCompare(b.title) || a.id.localeCompare(b.id, undefined, { numeric: true });
        if (memoryManagerFilters.sort === "id") return a.id.localeCompare(b.id, undefined, { numeric: true });
        return String(b.updated || "").localeCompare(String(a.updated || "")) || a.id.localeCompare(b.id, undefined, { numeric: true });
      });
    renderMemoryList(filtered);
    if (preserveSelection && memoryCurrentEntryId) {
      const current = memoryListEntries.find((entry) => entry.id === memoryCurrentEntryId && entry.scope === memorySelection.scope);
      if (current && $("memory-detail")?.dataset.dirty !== "true") showMemoryDetail(current.scope || scope, current, { reveal: false });
      else if (!current) hideMemoryDetail();
    }
  } catch (err) {
    if (project !== currentProject || generation !== memoryListGeneration) return;
    toastError(`${t("记忆条目加载失败")}:${err}`);
  }
}

export function renderMemoryList(entries, { search = false } = {}) {
  const container = $("memory-list");
  const count = $("memory-list-count");
  const state = $("memory-list-state");
  container.innerHTML = "";
  count.textContent = `${entries.length} ${t("条")}`;
  state.textContent = search ? t("搜索结果") : `${memoryFilterLabel(memoryManagerFilters.scope)} / ${memoryFilterLabel(memoryManagerFilters.category)}`;
  if (!entries.length) {
    container.innerHTML = `<p class="dim">${t(search ? "没有命中的记忆" : "该筛选暂无记忆")}</p>`;
    return;
  }
  for (const entry of entries) {
    const row = document.createElement("button");
    row.type = "button";
    const ageDays = memoryAgeDays(entry.updated);
    const dormant = !search && (entry.hits ?? 0) === 0 && ageDays >= 3 && entry.status !== "stale";
    const zeroRead = !search && (entry.read_observed ?? 0) >= 3 && (entry.read ?? 0) === 0 && entry.status !== "stale";
    row.className = `memory-row${entry.id === memoryCurrentEntryId ? " selected" : ""}${entry.status === "stale" ? " stale" : ""}${dormant ? " dormant" : ""}${zeroRead ? " zero-read" : ""}${entry.category === "sop" ? " sop" : ""}`;
    row.dataset.memoryId = entry.id;
    row.dataset.scope = entry.scope || memoryManagerFilters.scope;
    row.classList.toggle("selected", entry.id === memoryCurrentEntryId && row.dataset.scope === memorySelection.scope);
    const lastHit = entry.last_hit_at ? `${t("最近命中")} ${new Date(entry.last_hit_at).toLocaleDateString()}` : t("从未命中");
    const recallMeta = (entry.recalled ?? 0) > 0 ? ` · ${t("召回")} ${entry.recalled} · ${t("注入")} ${entry.injected ?? 0} · ${t("正文读取")} ${entry.read_observed ? entry.read : t("未知")}` : "";
    const snippet = search ? entry.snippet : entry.description;
    row.innerHTML =
      `<span class="memory-row-top"><span class="memory-row-id">${escapeHtml(entry.id)}</span><span class="memory-row-title">${escapeHtml(entry.title)}</span><span class="memory-status-badge ${escapeHtml(entry.status || "")}">${escapeHtml(entry.status || "")}</span></span>` +
      `${entry.category === "sop" ? `<em class="memory-row-cat sop">${t("SOP")}</em>` : ""}` +
      `<span class="dim memory-row-description">${escapeHtml(snippet || "")}</span>` +
      `<span class="memory-row-meta dim">${escapeHtml(entry.scope || memoryManagerFilters.scope)}/${escapeHtml(entry.category || "")} · ${t("命中")} ${entry.hits ?? 0}${recallMeta} · ${lastHit} · ${escapeHtml(entry.updated || "")}` +
      `${dormant ? ` · <em class="memory-dormant-flag">${t("长期零命中")}</em>` : ""}` +
      `${zeroRead ? ` · <em class="memory-zero-read-flag">${t("多次注入但未记录正文读取")}</em>` : ""}</span>`;
    row.addEventListener("click", () => {
      if (search) openMemoryDetailById(entry.scope, entry.id);
      else showMemoryDetail(entry.scope || memoryManagerFilters.scope, entry);
    });
    container.appendChild(row);
  }
}

export function showMemoryDetail(scope, entry, { reveal = true, readOnly = false } = {}) {
  const box = $("memory-detail");
  if (!box) return;
  const project = currentProject;
  if (reveal) showMemoryTab("read");
  $("memory-reader-empty")?.classList.add("hidden");
  $("memory-scroll")?.classList.add("has-memory-selection");
  delete box.dataset.dirty;
  box.oninput = () => { box.dataset.dirty = "true"; };
  document.dispatchEvent(new CustomEvent("kz:memory-selected", { detail: { project, scope, id: entry.id, title: entry.title } }));
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
  headingTitle.textContent = entry.title || entry.id;
  const close = document.createElement("button");
  close.type = "button";
  close.className = "ghost mini";
  close.textContent = t("关闭详情");
  close.addEventListener("click", () => {
    memoryCurrentEntryId = null;
    hideMemoryDetail();
    document.querySelectorAll("#memory-list .memory-row.selected").forEach((row) => row.classList.remove("selected"));
  });
  heading.append(headingTitle, close);
  const meta = document.createElement("div");
  meta.className = "memory-detail-meta";
  // 来源与引用里的条目编号/路径做成可点 chip(R-/D- 跳条目,路径开文件)。
  meta.replaceChildren(
    document.createTextNode(`${entry.id} · ${entry.scope || scope} / ${entry.category || ""} · ${entry.status} · ${t("来源")} `),
    richText(entry.source || t("未知")),
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
  const field = (labelText, control) => {
    const wrapper = document.createElement("label");
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
  recall.textContent = entry.description;
  const metadata = document.createElement("details");
  metadata.className = "memory-meta-editor";
  const metadataSummary = document.createElement("summary");
  metadataSummary.textContent = t("编辑标题与召回条件");
  metadata.append(metadataSummary, field(t("标题"), title), field(t("召回钩子"), desc));
  const bodyBox = document.createElement("div");
  bodyBox.className = "memory-body-read";
  const bodyText = String(entry.body ?? "");
  renderMemoryBodyRead(bodyBox, bodyText);
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
      await refreshMemory({ force: true });
    } catch (err) {
      toastError(`${t("记忆保存失败")}:${err}`);
    }
  });
  const staleBtn = document.createElement("button");
  staleBtn.type = "button";
  staleBtn.className = "ghost";
  staleBtn.textContent = entry.status === "active" ? t("标记失效") : t("恢复启用");
  staleBtn.addEventListener("click", async () => {
    try {
      await invoke("memory_entry_save", {
        projectDir: project,
        scope,
        id: entry.id,
        title: null,
        description: null,
        body: null,
        status: entry.status === "active" ? "stale" : "active",
      });
      if (project !== currentProject) return;
      memoryCurrentEntryId = null;
      hideMemoryDetail();
      refreshMemory({ force: true });
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
    if (!(await confirmDialog({ title: t("确认删除"), message: `${entry.id}?${t("此操作不可撤销")}`, okText: t("删除"), danger: true }))) return;
    try {
      await invoke("memory_entry_delete", { projectDir: project, scope, id: entry.id });
      if (project !== currentProject) return;
      toast(t("已删除"));
      memoryCurrentEntryId = null;
      hideMemoryDetail();
      refreshMemory({ force: true });
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
  actions.append(save, discuss, staleBtn, deleteBtn);
  if (readOnly) {
    // 归档条目只读:不给保存/失效/删除/改标题,正文不给编辑入口(归档目录里的文件不在写路径上)。
    box.dataset.readonly = "true";
    bodyBox.querySelector(".memory-body-edit-row")?.remove();
    box.append(heading, meta, profile, recall, field(t("正文"), bodyBox));
    return;
  }
  delete box.dataset.readonly;
  box.append(heading, meta, profile, recall, renderMemoryAreaRow(project, scope, entry), metadata, field(t("正文"), bodyBox), actions);
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
  const renderChips = (shown, { pending = false } = {}) => {
    list.replaceChildren();
    if (!shown.length) {
      const none = document.createElement("span");
      none.className = "dim";
      none.textContent = pending ? "…" : t("未归类");
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
  };
  // 推断出的区域来自图谱载荷;列表模式下还没取过就先显示字段,取到后再补上推断(带依据)。
  const inferred = memoryAreaProvider?.areasFor?.(scope, entry.id) ?? null;
  renderChips(inferred ?? fieldAreas, { pending: !inferred && Boolean(memoryAreaProvider) });
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
    renderChips(memoryAreaProvider?.areasFor?.(scope, entry.id) ?? fieldAreas);
  }).catch(() => renderChips(fieldAreas));
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
  article.innerHTML = renderMarkdown(text || t("无正文"));
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

export function renderMemoryBill(data) {
  const bill = $("memory-bill");
  bill.innerHTML = "";
  const entries = Array.isArray(data.bill) ? data.bill : [];
  if (!entries.length) {
    bill.innerHTML = `<p class="dim">${t("暂无账单数据(跑一轮后生成)")}</p>`;
  } else {
    const total = entries.reduce((sum, item) => sum + (item[1] || 0), 0);
    for (const [name, chars] of entries) {
      const pct = total ? Math.round((chars / total) * 100) : 0;
      const row = document.createElement("div");
      row.className = "memory-bill-row";
      row.innerHTML = `<span class="memory-bill-name">${escapeHtml(name)}</span><span class="dim">${chars} · ${pct}%</span><span class="memory-bill-bar" style="width:${Math.max(pct, 2)}%"></span>`;
      bill.appendChild(row);
    }
  }
  const eps = $("memory-episodes");
  eps.innerHTML = "";
  const episodes = data.episodes || [];
  if (!episodes.length) {
    eps.innerHTML = `<p class="dim">${t("暂无轮次记录")}</p>`;
    return;
  }
  for (const ep of episodes) {
    const tools = Object.entries(ep.tools || {})
      .map(([name, count]) => `${name}×${count}`)
      .join(" ");
    const row = document.createElement("div");
    row.className = "memory-episode";
    row.innerHTML = `<span class="memory-episode-prompt">${escapeHtml(ep.prompt)}</span><span class="dim">${escapeHtml(ep.outcome)} · ${ep.steps} steps${tools ? " · " + escapeHtml(tools) : ""}</span>`;
    eps.appendChild(row);
  }
}

defer(() => {
  const tabs = ["read", "chat", "insights"];
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
  $("memory-refresh")?.addEventListener("click", () => refreshMemory({ force: true }));
  document.addEventListener("kz:memory-changed", event => {
    if (event.detail?.project === currentProject) void refreshMemory({ force: true });
  });
  const input = $("memory-search-input");
  const clear = $("memory-search-clear");
  const runSearch = async () => {
    const project = currentProject;
    const generation = ++memoryListGeneration;
    const query = input.value.trim();
    if (!query || !currentProject) {
      clear.hidden = true;
      memoryCurrentEntryId = null;
      hideMemoryDetail();
      document.dispatchEvent(new CustomEvent("kz:memory-search-hits", { detail: { project, ids: [], query: "" } }));
      await loadMemoryList(memoryManagerFilters.scope, memoryManagerFilters.category);
      return;
    }
    clear.hidden = false;
    neuralFlowEmit?.("memory_search_started", { query_length: query.length });
    try {
      const hits = await invoke("memory_search_page", { projectDir: project, query });
      if (project !== currentProject || generation !== memoryListGeneration) return;
      neuralFlowEmit?.("memory_search_completed", { hit_count: hits.length });
      memoryCurrentEntryId = null;
      hideMemoryDetail();
      renderMemoryList(hits.map((hit) => ({ ...hit, scope: hit.scope || "project" })), { search: true });
      document.dispatchEvent(new CustomEvent("kz:memory-search-hits", { detail: { project, ids: hits.map((hit) => hit.id), query } }));
    } catch (err) {
      if (project !== currentProject || generation !== memoryListGeneration) return;
      neuralFlowEmit?.("memory_search_failed");
      toastError(`${t("记忆检索失败")}:${err}`);
    }
  };
  input.addEventListener("keydown", async (event) => {
    if (event.key === "Enter") await runSearch();
  });
  input.addEventListener("input", () => {
    clear.hidden = !input.value.trim();
  });
  clear.addEventListener("click", async () => {
    input.value = "";
    clear.hidden = true;
    document.dispatchEvent(new CustomEvent("kz:memory-search-hits", { detail: { project: currentProject, ids: [], query: "" } }));
    await loadMemoryList(memoryManagerFilters.scope, memoryManagerFilters.category);
  });
});

defer(() => {
  $("memory-consolidate-btn").addEventListener("click", async () => {
    if (!currentProject) return;
    neuralFlowEmit?.("memory_consolidation_started");
    try {
      const result = await invoke("memory_consolidate", { projectDir: currentProject });
      neuralFlowEmit?.(result?.pending ? "memory_consolidation_partial" : "memory_consolidation_completed", { pending: Boolean(result?.pending) });
      toast(result.pending ? t("inbox 尚有草稿未消化") : t("inbox 已整理完毕"));
      refreshMemory({ force: true });
    } catch (err) {
      neuralFlowEmit?.("memory_consolidation_failed");
      toastError(`${t("整理失败")}:${err}`);
    }
  });
});
