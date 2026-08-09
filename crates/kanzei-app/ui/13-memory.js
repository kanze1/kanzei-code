// ---------- 记忆页(R-107):透明化——架构总览/条目/账单/检索/整理 ----------
let memorySelection = null;
async function refreshMemory() {
  if (!currentProject) {
    $("memory-arch").innerHTML = `<p class="dim">${t("先在左侧「项目」里添加并选择一个目录")}</p>`;
    return;
  }
  try {
    const [overview, billData, recallData, candidates] = await Promise.all([
      invoke("memory_overview", { projectDir: currentProject }),
      invoke("memory_context_bill", { projectDir: currentProject }),
      invoke("memory_recalls", { projectDir: currentProject, limit: 20 }),
      invoke("memory_note_candidates", { projectDir: currentProject }),
    ]);
    renderMemoryArch(overview);
    renderMemoryBill(billData);
    renderMemoryRecalls(recallData);
    renderMemoryCandidates(candidates);
    if (memorySelection) await loadMemoryList(memorySelection.scope, memorySelection.category);
  } catch (err) {
    toastError(`${t("记忆页加载失败")}:${err}`, { retry: refreshMemory });
  }
}

// ---------- R-127 运行画像面板 ----------
// 判断 agent 跑得好不好此前全靠翻轨迹。数据源早就有(RunSummary 的 context_report、
// summarize_tools、summarize_metrics),缺的只是把它们汇到一处。
async function refreshMetrics() {
  if (!currentProject) {
    $("metrics-rounds").innerHTML = `<p class="dim">${t("先在左侧「项目」里添加并选择一个目录")}</p>`;
    return;
  }
  try {
    const data = await invoke("run_metrics", { projectDir: currentProject, limit: 20 });
    renderMetrics(data?.rounds ?? []);
  } catch (err) {
    toastError(`${t("运行画像加载失败")}:${err}`, { retry: refreshMetrics });
  }
}

function renderMetrics(rounds) {
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
      ? `${t("终端")} ${m.terminal_calls ?? 0} · git ${m.git_calls ?? 0}(${m.git_groups ?? 0} ${t("组")}) · edit ${m.edit_misses ?? 0}/${m.edit_calls ?? 0} ${t("未命中")} · ${t("子代理")} ${m.subagent_calls ?? 0} · ${t("失败")} ${m.failed_calls ?? 0}/${m.total_calls ?? 0} · ${t("上下文")} ${contextTotal}${redundantLine(m)}`
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

function redundantLine(m) {
  const total = (m.redundant_git ?? 0) + (m.redundant_test ?? 0) + (m.redundant_task ?? 0);
  if (total === 0) return "";
  const parts = [];
  if (m.redundant_git) parts.push(`git×${m.redundant_git}`);
  if (m.redundant_test) parts.push(`test×${m.redundant_test}`);
  if (m.redundant_task) parts.push(`task×${m.redundant_task}`);
  return ` · ${t("冗余提醒")} ${parts.join(" ")}`;
}

// ---------- R-126 UI 自查探针:在真实运行中的窗口里取样 ----------
// 后端工具发 kz:ui-probe,这里取样后用 ui_probe_result 回传。取的是用户眼前这个
// 窗口的实际渲染结果——不是重新起一个空白页,那样查不出任何真实的渲染问题。
const UI_PROBE_NODE_LIMIT = 60;

function describeNode(el, depth) {
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

function probeDom(selector) {
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

function probeConsole() {
  if (!uiConsoleLog.length) return "自加载以来没有 console 错误或警告。";
  return uiConsoleLog
    .map((e) => `[${e.level}] ${new Date(e.at).toLocaleTimeString()} ${e.text}`)
    .join("\n");
}

function probeStyle(selector) {
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

// R-124:待确认候选。SOP 是用户的常用模板,不能由 agent 自己决定入库——
// 所以候选只停在这里,采纳/丢弃都是用户一键的事。
function renderMemoryCandidates(list) {
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
      try {
        await invoke("memory_consolidate", { projectDir: currentProject });
        toast(t("已交给记忆管理子代理提炼"));
        refreshMemory();
      } catch (err) {
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

// R-125:召回明细。没有这块界面就没有任何评估手段——记忆有没有用只能凭感觉。
function renderMemoryRecalls(data) {
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

function renderMemoryArch(overview) {
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

async function loadMemoryList(scope, category) {
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
      row.className = `memory-row${entry.status === "stale" ? " stale" : ""}${dormant ? " dormant" : ""}`;
      row.dataset.memoryId = entry.id;
      const lastHit = entry.lastHitAt
        ? `${t("最近命中")} ${new Date(entry.lastHitAt).toLocaleDateString()}`
        : t("从未命中");
      row.innerHTML =
        `<span class="memory-row-id">${escapeHtml(entry.id)}</span>` +
        `<span class="memory-row-title">${escapeHtml(entry.title)}</span>` +
        `<span class="dim">${escapeHtml(entry.description)}</span>` +
        `<span class="memory-row-meta dim">${escapeHtml(entry.status)} · ${t("命中")} ${entry.hits} · ${lastHit} · ${escapeHtml(entry.updated)}` +
        `${dormant ? ` · <em class="memory-dormant-flag">${t("长期零命中")}</em>` : ""}</span>`;
      row.addEventListener("click", () => showMemoryDetail(scope, entry));
      container.appendChild(row);
    }
  } catch (err) {
    toastError(`${t("记忆条目加载失败")}:${err}`);
  }
}

function showMemoryDetail(scope, entry) {
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
  const body = document.createElement("textarea");
  body.value = entry.body;
  body.rows = 8;
  body.setAttribute("aria-label", t("记忆正文"));
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
        body: body.value,
        status: null,
      });
      toast(t("记忆已保存"));
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
    if (!window.confirm(`${t("确认删除")} ${entry.id}?${t("此操作不可撤销")}`)) return;
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
  box.append(meta, profile, title, desc, body, actions);
}

// 条目"年纪":用于零命中判定——刚写下来还没被检索过不算没用。
function memoryAgeDays(updated) {
  const stamp = Date.parse(`${updated}T00:00:00Z`);
  if (Number.isNaN(stamp)) return 0;
  return Math.max(0, Math.floor((Date.now() - stamp) / 86_400_000));
}

function renderMemoryBill(data) {
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

$("memory-search-input").addEventListener("keydown", async (event) => {
  if (event.key !== "Enter") return;
  const query = event.target.value.trim();
  if (!query || !currentProject) return;
  try {
    const hits = await invoke("memory_search_page", { projectDir: currentProject, query });
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
    toastError(`${t("记忆检索失败")}:${err}`);
  }
});

$("memory-consolidate-btn").addEventListener("click", async () => {
  if (!currentProject) return;
  try {
    const result = await invoke("memory_consolidate", { projectDir: currentProject });
    toast(result.pending ? t("inbox 尚有草稿未消化") : t("inbox 已整理完毕"));
    refreshMemory();
  } catch (err) {
    toastError(`${t("整理失败")}:${err}`);
  }
});
