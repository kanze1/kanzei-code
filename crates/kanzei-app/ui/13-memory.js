import { defer } from "./01-core.js";
import { escapeHtml } from "./04-markdown.js";
import { $, confirmDialog, invoke, on, replayExperienceFacts, uiConsoleLog } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, toast, toastError } from "./03-shell.js";
import { latestDocsSnapshot } from "./12-docs-pages.js";
import { neuralFlowEmit } from "./22-neural-flow.js";

// ---------- 记忆页(R-107):透明化——架构总览/条目/账单/检索/整理 ----------
export let memorySelection = null;
export async function refreshMemory() {
  if (!currentProject) {
    $("memory-arch").innerHTML = `<p class="dim">${t("先在左侧「项目」里添加并选择一个目录")}</p>`;
    return;
  }
  try {
    const [overview, billData, recallData, candidates, flags, controlPlane] = await Promise.all([
      invoke("memory_overview", { projectDir: currentProject }),
      invoke("memory_context_bill", { projectDir: currentProject }),
      invoke("memory_recalls", { projectDir: currentProject, limit: 20 }),
      invoke("memory_note_candidates", { projectDir: currentProject }),
      invoke("memory_value_flags", { projectDir: currentProject }),
      invoke("memory_control_plane", { projectDir: currentProject }),
    ]);
    renderMemoryArch(overview);
    replayExperienceFacts(controlPlane?.experience_facts);
    renderMemoryControlPlane(controlPlane);
    renderMemoryBill(billData);
    renderMemoryRecalls(recallData);
    renderMemoryCandidates(candidates);
    renderMemoryValueFlags(flags);
    const total = (overview?.scopes ?? []).reduce((sum, scope) => sum + Number(scope.total ?? 0), 0);
    neuralFlowEmit?.("memory_snapshot", { memory_count: total, candidate_count: candidates?.length ?? 0 });
    if (memorySelection) await loadMemoryList(memorySelection.scope, memorySelection.category);
  } catch (err) {
    toastError(`${t("记忆页加载失败")}:${err}`, { retry: refreshMemory });
  }
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
    [t("召回/采纳"), `${recall.recalled ?? 0}/${recall.fetched ?? 0}`],
    [t("召回关联"), `${recall.events_linked ?? 0}/${recall.events_total ?? 0} · ${t("悬空")} ${recall.events_orphaned ?? 0}`],
    [t("价值画像"), `${effects.length} ${t("条")}`],
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
    $("metrics-rounds").innerHTML = `<p class="dim">${t("先在左侧「项目」里添加并选择一个目录")}</p>`;
    return;
  }
  try {
    const [data, cats] = await Promise.all([
      invoke("run_metrics", { projectDir: currentProject, limit: 20 }),
      invoke("run_metrics_by_category", { projectDir: currentProject, limit: 200 }),
    ]);
    const incidentData = latestDocsSnapshot?.incident_metrics
      ?? (await invoke("docs_snapshot", { projectDir: currentProject })).incident_metrics;
    renderMetrics(data?.rounds ?? []);
    renderMetricsCategories(cats ?? {});
    renderIncidentMetrics(incidentData, "metrics-incident");
  } catch (err) {
    toastError(`${t("运行画像加载失败")}:${err}`, { retry: refreshMetrics });
  }
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
    const detail = document.createElement("pre");
    detail.className = "memory-candidate-detail dim";
    detail.textContent = item.detail || "";
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
        refreshMemory();
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
        refreshMemory();
      } catch (err) {
        toastError(`${t("丢弃失败")}:${err}`);
      }
    });
    actions.append(adopt, drop);
    row.append(head, detail, actions);
    box.appendChild(row);
  }
}

// R-150:空闲整理清单。零采纳候选(召回≥3 采纳=0)与复发候选只展示+可点开详情,
// 处置不在这里静默删——点条目打开详情页走既有墓碑机制(降级/修订/归档)。
// D-217:stale 积压(已归档条目数)也进清单——归档保留墓碑正文可回看复查。
export function renderMemoryValueFlags(data) {
  const box = $("memory-value-flags");
  const count = $("memory-flags-count");
  if (!box) return;
  box.innerHTML = "";
  const zero = Array.isArray(data?.zeroAdopt) ? data.zeroAdopt : [];
  const recur = Array.isArray(data?.recurring) ? data.recurring : [];
  const staleArchived = Number(data?.staleArchived) || 0;
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
    empty.textContent = t("暂无零采纳或复发候选");
    box.appendChild(empty);
    return;
  }
  if (zero.length) {
    const h = document.createElement("p");
    h.className = "memory-flags-head";
    h.textContent = `${t("零采纳候选")} (${zero.length})`;
    box.appendChild(h);
    for (const item of zero) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "memory-flag-row zero-adopt";
      row.innerHTML =
        `<span class="memory-row-id">${escapeHtml(item.id)}</span>` +
        `<span class="memory-row-title">${escapeHtml(item.title)}</span>` +
        `<span class="dim">${t("召回")} ${item.recalled}/${t("采纳")} ${item.fetched}</span>`;
      row.addEventListener("click", () => openMemoryDetailById(item.scope, item.id));
      box.appendChild(row);
    }
  }
  if (recur.length) {
    const h = document.createElement("p");
    h.className = "memory-flags-head";
    h.textContent = `${t("复发候选")} (${recur.length})`;
    box.appendChild(h);
    for (const item of recur) {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "memory-flag-row recurring";
      row.innerHTML =
        `<span class="memory-row-id">${escapeHtml(item.id)}</span>` +
        `<span class="memory-row-title">${escapeHtml(item.title)}</span>` +
        `<span class="dim">${t("召回")} ${item.recalled}/${t("采纳")} ${item.fetched}</span>`;
      row.addEventListener("click", () => openMemoryDetailById(item.scope, item.id));
      box.appendChild(row);
    }
  }
}

// 从清单跳详情:按 scope+id 定位条目并复用现有详情渲染。
export async function openMemoryDetailById(scope, id) {
  try {
    const list = await invoke("memory_entries", { projectDir: currentProject, scope, category: null });
    const entry = (list || []).find((e) => e.id === id);
    if (entry) showMemoryDetail(scope, entry);
  } catch (err) {
    toastError(`${t("记忆条目加载失败")}:${err}`);
  }
}

// R-125:召回明细。没有这块界面就没有任何评估手段——记忆有没有用只能凭感觉。
export function renderMemoryRecalls(data) {
  const box = $("memory-recalls");
  const rate = $("memory-recall-rate");
  if (!box) return;
  box.innerHTML = "";
  const rounds = data?.rounds ?? [];
  const total = data?.rounds_total ?? rounds.length;
  const used = data?.rounds_with_fetch ?? 0;
  // 采纳率放在标题上:一眼就能看出"召回了但没人用"是不是常态。
  rate.textContent = total ? `· ${t("采纳")} ${used}/${total}` : "";
  if (!rounds.length) {
    box.innerHTML = `<p class="dim">${t("还没有召回记录:开跑时若无记忆命中,这里就是空的")}</p>`;
    return;
  }
  for (const round of rounds) {
    const item = document.createElement("div");
    item.className = "memory-recall";
    const head = document.createElement("div");
    head.className = "memory-recall-head";
    const when = new Date(round.at).toLocaleString();
    const adopted = round.hits.filter((h) => h.fetched).length;
    head.innerHTML =
      `<span class="memory-recall-when">${escapeHtml(when)}</span>` +
      `<span class="dim">${round.hits.length} ${t("条命中")} · ${t("已采纳")} ${adopted} · ${t("注入")} ${round.injected_bytes}B</span>`;
    const prompt = document.createElement("div");
    prompt.className = "memory-recall-prompt dim";
    prompt.textContent = round.prompt_head;
    prompt.title = round.prompt_head;
    item.append(head, prompt);
    for (const hit of round.hits) {
      const row = document.createElement("div");
      row.className = `memory-recall-hit${hit.fetched ? " adopted" : ""}`;
      // 得分与片段一起给:「为什么召回这一条」必须能看出来,否则调不了检索。
      row.innerHTML =
        `<span class="memory-recall-id">${escapeHtml(hit.id)}</span>` +
        `<span class="memory-recall-title">${escapeHtml(hit.title)}</span>` +
        `<span class="dim">${hit.score.toFixed(2)}</span>` +
        `<span class="memory-recall-flag">${hit.fetched ? t("已采纳") : t("未拉取")}</span>`;
      const snip = document.createElement("div");
      snip.className = "memory-recall-snippet dim";
      snip.textContent = hit.snippet.replace(/\n/g, " ");
      row.appendChild(snip);
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
        memorySelection = { scope: scope.scope, category: cat };
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

export async function loadMemoryList(scope, category) {
  try {
    const list = await invoke("memory_entries", { projectDir: currentProject, scope, category });
    const container = $("memory-list");
    container.innerHTML = "";
    if (!list.length) {
      container.innerHTML = `<p class="dim">${t("该分类暂无记忆")}</p>`;
      return;
    }
    for (const entry of list) {
      const row = document.createElement("button");
      row.type = "button";
      // R-125:零命中要一眼看得出来——这是判断某条记忆该不该留的直接依据。
      // 只在条目有一定年纪时才标,刚写下来还没被检索过不算"没用"。
      const ageDays = memoryAgeDays(entry.updated);
      const dormant = (entry.hits ?? 0) === 0 && ageDays >= 3 && entry.status !== "stale";
      // R-150:零采纳标记——召回≥3 但从未拉正文 = 语义显著但决策无关。
      const zeroAdopt = (entry.recalled ?? 0) >= 3 && (entry.fetched ?? 0) === 0 && entry.status !== "stale";
      row.className = `memory-row${entry.status === "stale" ? " stale" : ""}${dormant ? " dormant" : ""}${zeroAdopt ? " zero-adopt" : ""}${entry.category === "sop" ? " sop" : ""}`;
      row.dataset.memoryId = entry.id;
      const lastHit = entry.lastHitAt
        ? `${t("最近命中")} ${new Date(entry.lastHitAt).toLocaleDateString()}`
        : t("从未命中");
      // R-150:召回/采纳率并入 meta——采纳率 = fetched/recalled。
      const recallMeta = (entry.recalled ?? 0) > 0
        ? ` · ${t("召回")} ${entry.recalled}/${t("采纳")} ${entry.fetched}`
        : "";
      row.innerHTML =
        `<span class="memory-row-id">${escapeHtml(entry.id)}</span>` +
        `<span class="memory-row-title">${escapeHtml(entry.title)}</span>` +
        `${entry.category === "sop" ? `<em class="memory-row-cat sop">${t("SOP")}</em>` : ""}` +
        `<span class="dim">${escapeHtml(entry.description)}</span>` +
        `<span class="memory-row-meta dim">${escapeHtml(entry.status)} · ${t("命中")} ${entry.hits}${recallMeta} · ${lastHit} · ${escapeHtml(entry.updated)}` +
        `${dormant ? ` · <em class="memory-dormant-flag">${t("长期零命中")}</em>` : ""}` +
        `${zeroAdopt ? ` · <em class="memory-zero-adopt-flag">${t("零采纳候选")}</em>` : ""}</span>`;
      row.addEventListener("click", () => showMemoryDetail(scope, entry));
      container.appendChild(row);
    }
  } catch (err) {
    toastError(`${t("记忆条目加载失败")}:${err}`);
  }
}

export function showMemoryDetail(scope, entry) {
  const box = $("memory-detail");
  box.classList.remove("hidden");
  box.innerHTML = "";
  const meta = document.createElement("p");
  meta.className = "dim";
  const refsText = (entry.refs && entry.refs.length) ? ` · ${t("引用来源")} ${entry.refs.join(" ")}` : "";
  meta.textContent = `${entry.id} · ${entry.status} · ${t("来源")} ${entry.source} · ${entry.path}${refsText}`;
  const title = document.createElement("input");
  title.value = entry.title;
  title.setAttribute("aria-label", t("记忆标题"));
  const desc = document.createElement("input");
  desc.value = entry.description;
  desc.setAttribute("aria-label", t("召回钩子"));
  // R-129:正文从单一 textarea 改为「摘要 + 分段阅读」,减少长文认知负荷;
  // 点「编辑正文」才切换回 textarea,保存仍走既有 memory_entry_save。
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
      await invoke("memory_entry_save", {
        projectDir: currentProject,
        scope,
        id: entry.id,
        title: title.value,
        description: desc.value,
        body: readMemoryBody(bodyBox, bodyText),
        status: null,
      });
      toast(t("记忆已保存"));
      // 就地重渲染详情:阅读视图要显示刚保存的新正文,不能停在旧值上。
      showMemoryDetail(scope, { ...entry, title: title.value, description: desc.value, body: readMemoryBody(bodyBox, bodyText) });
      refreshMemory();
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
        projectDir: currentProject,
        scope,
        id: entry.id,
        title: null,
        description: null,
        body: null,
        status: entry.status === "active" ? "stale" : "active",
      });
      box.classList.add("hidden");
      refreshMemory();
    } catch (err) {
      toastError(`${t("记忆保存失败")}:${err}`);
    }
  });
  // 零命中的条目要能直接删掉,不能只有"标记失效"——stale 仍占索引与列表。
  const deleteBtn = document.createElement("button");
  deleteBtn.type = "button";
  deleteBtn.className = "ghost danger";
  deleteBtn.textContent = t("删除");
  deleteBtn.title = t("从磁盘删除该记忆文件,不可撤销");
  deleteBtn.addEventListener("click", async () => {
    if (!(await confirmDialog({ title: t("确认删除"), message: `${entry.id}?${t("此操作不可撤销")}`, okText: t("删除"), danger: true }))) return;
    try {
      await invoke("memory_entry_delete", { projectDir: currentProject, scope, id: entry.id });
      toast(t("已删除"));
      box.classList.add("hidden");
      refreshMemory();
    } catch (err) {
      toastError(`${t("删除失败")}:${err}`);
    }
  });
  // 效果画像:这条到底被用过没有,直接摆在详情里,不用去列表里对。
  const profile = document.createElement("p");
  profile.className = "dim memory-profile";
  const lastHit = entry.lastHitAt ? new Date(entry.lastHitAt).toLocaleString() : t("从未命中");
  profile.textContent = `${t("累计命中")} ${entry.hits ?? 0} · ${t("最近命中")} ${lastHit}`;
  const actions = document.createElement("div");
  actions.className = "memory-detail-actions";
  actions.append(save, staleBtn, deleteBtn);
  box.append(meta, profile, title, desc, bodyBox, actions);
}

// R-129:正文从单一 textarea 改为「摘要 + 分段阅读」。摘要行取首段去换行截 140 字,
// 让长文先有可扫读的要点;分段列表按空行拆段,超长段折叠 + 展开按钮,一段一块不糊成整片。
// D-204 批2(验收②查看展示):sop 条目正文按「步骤 + 判断依据」结构渲染——
// 以「1. / 2. / 3. …」编号开头的行识别为结构化小节,渲染成可扫读的步骤块
// (标题加粗 + 间距),不再是糊成一片的纯文本段落。
export function renderMemoryBodyRead(container, bodyText) {
  container.innerHTML = "";
  const text = String(bodyText ?? "");
  const paragraphs = splitMemoryParagraphs(text);
  // 摘要行:首段(去换行)截 140 字。没有正文时明说,不给空壳。
  const first = paragraphs[0] || "";
  const flat = first.replace(/\s+/g, " ").trim();
  const summary = document.createElement("div");
  summary.className = "memory-body-summary";
  summary.innerHTML =
    `<span class="memory-body-summary-label">${escapeHtml(t("正文摘要"))}</span>` +
    `<span class="memory-body-summary-text">${escapeHtml(flat ? flat.slice(0, 140) + (flat.length > 140 ? "…" : "") : t("无正文"))}</span>`;
  container.appendChild(summary);
  const list = document.createElement("div");
  list.className = "memory-body-paragraphs";
  if (paragraphs.length) {
    for (const para of paragraphs) {
      const block = document.createElement("div");
      block.className = "memory-body-para";
      // D-204 批2:编号开头(如 "1. 适用场景" / "2. 操作步骤:xxx")视为 SOP 结构化小节。
      const numHead = para.match(/^\s*(\d+)[.、]\s*(.*)$/);
      const isStep = numHead && numHead[1] && numHead[2].length < 60;
      if (isStep) {
        block.classList.add("memory-sop-step");
        const head = document.createElement("div");
        head.className = "memory-sop-step-head";
        head.textContent = `${numHead[1]}. ${numHead[2].split(":")[0]}`;
        block.appendChild(head);
      }
      // 折叠阈值:超过 6 行或超过 280 字就只露头,点开才看全——分段的意义就在
      // 先把长文切成可扫读的小块,而不是在详情里堆一片滚动文本。
      const tooLong = para.split("\n").length > 6 || para.length > 280;
      if (tooLong) block.classList.add("collapsed");
      const content = document.createElement("div");
      content.className = "memory-body-para-text";
      // 结构化小节:正文从编号后的冒号处剥离,标题单独一行,正文按内容渲染。
      content.textContent = isStep ? para.slice(para.indexOf(":") + 1).trim() : para;
      block.appendChild(content);
      if (tooLong) {
        const toggle = document.createElement("button");
        toggle.type = "button";
        toggle.className = "memory-body-toggle mini";
        toggle.textContent = t("展开全文");
        toggle.addEventListener("click", () => {
          const collapsing = block.classList.toggle("collapsed");
          toggle.textContent = collapsing ? t("展开全文") : t("收起");
        });
        block.appendChild(toggle);
      }
      list.appendChild(block);
    }
  } else {
    const empty = document.createElement("p");
    empty.className = "dim";
    empty.textContent = t("无正文");
    list.appendChild(empty);
  }
  container.appendChild(list);
  // 编辑入口:阅读视图是默认态,想改才切回 textarea(保存仍走上方 save 按钮)。
  const editRow = document.createElement("div");
  editRow.className = "memory-body-edit-row";
  const editBtn = document.createElement("button");
  editBtn.type = "button";
  editBtn.className = "ghost mini";
  editBtn.textContent = t("编辑正文");
  editBtn.addEventListener("click", () => {
    const ta = document.createElement("textarea");
    ta.rows = 8;
    ta.value = readMemoryBody(container, text);
    ta.setAttribute("aria-label", t("记忆正文"));
    container.replaceChildren(ta);
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
  $("memory-search-input").addEventListener("keydown", async (event) => {
    if (event.key !== "Enter") return;
    const query = event.target.value.trim();
    if (!query || !currentProject) return;
    neuralFlowEmit?.("memory_search_started", { query_length: query.length });
    try {
      const hits = await invoke("memory_search_page", { projectDir: currentProject, query });
      neuralFlowEmit?.("memory_search_completed", { hit_count: hits.length });
      memorySelection = null;
      const container = $("memory-list");
      container.innerHTML = hits.length ? "" : `<p class="dim">${t("没有命中的记忆")}</p>`;
      for (const hit of hits) {
        const row = document.createElement("div");
        row.className = "memory-row";
        row.innerHTML = `<span class="memory-row-id">${escapeHtml(hit.id)}</span><span class="memory-row-title">${escapeHtml(hit.title)}</span><span class="dim">${escapeHtml(hit.snippet)}</span><span class="memory-row-meta dim">${escapeHtml(hit.scope)}/${escapeHtml(hit.category)} · ${t("命中")} ${hit.hits}</span>`;
        container.appendChild(row);
      }
    } catch (err) {
      neuralFlowEmit?.("memory_search_failed");
      toastError(`${t("记忆检索失败")}:${err}`);
    }
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
      refreshMemory();
    } catch (err) {
      neuralFlowEmit?.("memory_consolidation_failed");
      toastError(`${t("整理失败")}:${err}`);
    }
  });
});

// R-132:一键整理——对零采纳候选(召回≥3 采纳=0)批量降级 stale,可逆不删。
// 结果反馈:降级数量 + 跳过数量,明细进 toast 尾部。
defer(() => {
  $("memory-cleanup-btn").addEventListener("click", async () => {
    if (!currentProject) return;
    const btn = $("memory-cleanup-btn");
    btn.disabled = true;
    neuralFlowEmit?.("memory_cleanup_started");
    try {
      const result = await invoke("memory_cleanup_demote", { projectDir: currentProject });
      const demoted = Array.isArray(result?.demoted) ? result.demoted : [];
      const skipped = Array.isArray(result?.skipped) ? result.skipped : [];
      neuralFlowEmit?.("memory_cleanup_completed", { demoted_count: demoted.length, skipped_count: skipped.length });
      if (demoted.length) {
        const names = demoted.slice(0, 3).map((d) => d.title).join("、");
        toast(`${t("已降级")} ${demoted.length} ${t("条记忆为 stale")}${skipped.length ? `,${t("跳过")} ${skipped.length}` : ""}${demoted.length > 3 ? "…" : ""}${names ? `:${names}` : ""}`);
      } else {
        toast(skipped.length ? `${t("无候选可降级")},${t("跳过")} ${skipped.length}` : t("无零采纳候选需要整理"));
      }
      refreshMemory();
    } catch (err) {
      neuralFlowEmit?.("memory_cleanup_failed");
      toastError(`${t("整理失败")}:${err}`);
    } finally {
      btn.disabled = false;
    }
  });
});
