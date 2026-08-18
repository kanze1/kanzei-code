// ---------- R-184 B 面:任务级并行线路 ----------
// 这里不另造状态仓库。agent 协作块和用户并列视图都读 collaboration_snapshot,
// 其 branch / phase / tool / changed_files 来自后端当前运行态和 git 现场。
let collaborationLines = [];
let linesRefreshInFlight = false;
let linesRefreshTimer = null;
let linesRefreshQueued = false;
const LINES_REFRESH_IDLE_MS = 8000;
const LINES_REFRESH_RUNNING_MS = 3500;

function lineAgentCodes(lines) {
  const codes = new Map();
  let branchIndex = 0;
  for (const line of [...lines].sort((a, b) => a.process_id.localeCompare(b.process_id))) {
    if (!line.worktree_path) {
      codes.set(line.process_id, "M");
      continue;
    }
    codes.set(line.process_id, String.fromCharCode(65 + (branchIndex % 26)));
    branchIndex += 1;
  }
  return codes;
}

// R-247:开线区只列当前可领取(todo/open)、未阻塞的 R/D 条目。最终 claim 仍由
// 后端 WorkTool 原子校验；这里负责让用户先选事实对象，不在前端复制调度器。
function renderLineWorkItemOptions(snapshot = null) {
  const select = $("lines-work-item");
  const add = $("lines-add");
  if (!select || !add) return;
  const source = snapshot ?? (typeof latestDocsSnapshot !== "undefined" ? latestDocsSnapshot : null);
  const candidates = [
    ...(source?.requirements ?? []),
    ...(source?.defects ?? []),
  ].filter((entry) => !entry.closed && !entry.blocked && ["todo", "open"].includes(entry.status));
  const previous = select.value;
  const placeholder = document.createElement("option");
  placeholder.value = "";
  placeholder.textContent = t(candidates.length ? "选择未被持有的条目…" : "当前没有可开线条目");
  select.replaceChildren(placeholder);
  for (const entry of candidates) {
    const option = document.createElement("option");
    option.value = entry.id;
    option.textContent = `${entry.id} · ${entry.title}`;
    select.appendChild(option);
  }
  select.value = candidates.some((entry) => entry.id === previous) ? previous : "";
  select.disabled = worktreeLineCreateInFlight || candidates.length === 0;
  add.disabled = worktreeLineCreateInFlight || !select.value;
}

function normalizedChangedFile(path) {
  return String(path).replaceAll("\\", "/").toLocaleLowerCase();
}

function lineConflictPairs(lines) {
  const byFile = new Map();
  for (const line of lines) {
    for (const file of line.changed_files ?? []) {
      const key = normalizedChangedFile(file);
      const entry = byFile.get(key) ?? { file, lines: [] };
      if (!entry.lines.some((candidate) => candidate.process_id === line.process_id)) entry.lines.push(line);
      byFile.set(key, entry);
    }
  }
  const pairs = new Map();
  for (const { file, lines: owners } of byFile.values()) {
    for (let left = 0; left < owners.length; left += 1) {
      for (let right = left + 1; right < owners.length; right += 1) {
        const a = owners[left];
        const b = owners[right];
        const ids = [a.process_id, b.process_id].sort();
        const key = ids.join("\0");
        const pair = pairs.get(key) ?? { left: a, right: b, files: [] };
        pair.files.push(file);
        pairs.set(key, pair);
      }
    }
  }
  return [...pairs.values()];
}

function formatLineTokens(value) {
  const count = Number(value || 0);
  if (count < 1000) return String(count);
  return `${(count / 1000).toFixed(count < 10000 ? 1 : 0)}k`;
}

function lineFact(label, value, className = "") {
  const row = document.createElement("div");
  row.className = `line-fact ${className}`.trim();
  const key = document.createElement("span");
  key.className = "line-fact-label";
  key.textContent = label;
  const content = document.createElement("span");
  content.className = "line-fact-value";
  content.textContent = value;
  content.title = value;
  row.append(key, content);
  return row;
}

// 每线鞭挞 + 每线模型:原先这两样只有「当前打开的那条线」有(输入框上方那一份),
// 要给 N 条线配不同模型、或让某条后台线开始/停止自主推进,就得切 N 次线——而并行
// 线路页正是唯一能一屏看全所有线的地方。控件本身不持有状态:开关/暂停/本轮后停/上限
// 一律经 setLineAutoState 落到该线存档 + 它自己的后端 auto_state;模型经
// queueProcessUpdate 落该线 process(run_prompt 的 model 回落读的就是它)。
let linesModelCatalog = null;
let linesModelCatalogProject = null;
async function loadLinesModelCatalog() {
  if (linesModelCatalog && linesModelCatalogProject === currentProject) return linesModelCatalog;
  const forProject = currentProject;
  try {
    const models = await invoke("models_list", { projectDir: forProject });
    if (currentProject !== forProject) return linesModelCatalog ?? [];
    linesModelCatalog = models ?? [];
    linesModelCatalogProject = forProject;
  } catch (error) {
    // 探测不到不等于用不了(D-167):目录留空,下拉仍保留该线已记住的模型。
    linesModelCatalog = linesModelCatalog ?? [];
    log(`${t("模型列表获取失败")}:${error}`, "warn");
  }
  return linesModelCatalog;
}

function buildLineModelSelect(item) {
  const select = document.createElement("select");
  select.className = "ctx-select line-model-select";
  select.title = t("模型改动下一轮生效");
  const current = item.model || "";
  const seen = new Set();
  const add = (value, label) => {
    if (seen.has(value)) return;
    seen.add(value);
    select.appendChild(new Option(label, value));
  };
  add("", t("模型:agent 默认"));
  for (const model of linesModelCatalog ?? []) add(model.id, model.label);
  // 该线记住的直指模型即使不在探测清单里也必须可见,否则一次刷新就把它从下拉里抹掉,
  // 用户以为自己没设过(D-167 同源)。
  if (current) add(current, `${current}(${t("已记住")})`);
  select.value = current;
  select.addEventListener("change", async () => {
    const value = select.value;
    updateLocalProcessItem(item.id, { model: value || null });
    try {
      await queueProcessUpdate(item.id, { model: value });
      log(`${item.label} ${t("该线模型已切换")}:${value || t("模型:agent 默认")}`);
      // 改的若是当前线,顶栏那一份要跟着走——两处显示同一条线却不一致最难查。
      if (item.id === activeProcessId && typeof loadModels === "function") {
        await loadModels();
        syncModelSelectToActiveLine();
      }
    } catch (error) {
      toastError(`${t("模型切换失败")}:${error}`);
    }
  });
  return select;
}

function buildLineAutoControls(line) {
  const box = document.createElement("div");
  box.className = "line-autorun";
  const item = processItems.find((process) => process.id === line.process_id);
  if (!item) {
    // 协作快照里有、进程列表里还没有:两份数据之间的刷新窗口。不画半截控件。
    box.classList.add("pending");
    box.textContent = t("线路状态同步中…");
    return box;
  }
  const config = lineAutoConfig(line.process_id);
  const rounds = Number(sessionState(item.session_id)?.auto_rounds ?? 0) || 0;

  const toggle = document.createElement("label");
  toggle.className = "line-auto-toggle";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = config.enabled;
  checkbox.addEventListener("change", () => {
    void setLineAutoState(line.process_id, { enabled: checkbox.checked });
  });
  const toggleText = document.createElement("span");
  toggleText.textContent = t("鞭挞");
  toggle.append(checkbox, toggleText);

  const progress = document.createElement("span");
  progress.className = "line-auto-rounds";
  progress.textContent = `${rounds}/${config.maxRounds}`;
  progress.title = t("鞭挞上限(轮)");

  const max = document.createElement("input");
  max.type = "number";
  max.className = "line-auto-max";
  max.min = "1";
  max.max = "100";
  max.value = String(config.maxRounds);
  max.title = t("鞭挞上限(轮)");
  max.addEventListener("change", () => {
    const value = Number.parseInt(max.value, 10);
    const clamped = Number.isFinite(value) ? Math.min(100, Math.max(1, value)) : config.maxRounds;
    max.value = String(clamped);
    void setLineAutoState(line.process_id, { maxRounds: clamped });
  });

  const pause = document.createElement("button");
  pause.type = "button";
  pause.className = `ghost mini line-auto-pause${config.paused ? " active" : ""}`;
  pause.textContent = config.paused ? t("继续鞭挞") : t("暂停鞭挞");
  pause.disabled = !config.enabled;
  pause.addEventListener("click", () => {
    void setLineAutoState(line.process_id, { paused: !config.paused });
  });

  const stopRound = document.createElement("button");
  stopRound.type = "button";
  stopRound.className = `ghost mini line-auto-stop-round${config.stopAfterRound ? " active" : ""}`;
  stopRound.textContent = t("本轮后停");
  stopRound.disabled = !config.enabled;
  stopRound.addEventListener("click", () => {
    void setLineAutoState(line.process_id, { stopAfterRound: !config.stopAfterRound });
  });

  const modelLabel = document.createElement("span");
  modelLabel.className = "line-auto-model-label";
  modelLabel.textContent = t("模型");
  box.append(toggle, progress, max, pause, stopRound, modelLabel, buildLineModelSelect(item));
  return box;
}

function renderLineConflicts(lines) {
  const target = $("lines-conflict-list");
  target.replaceChildren();
  const pairs = lineConflictPairs(lines);
  $("lines-conflicts").classList.toggle("has-conflicts", pairs.length > 0);
  if (!pairs.length) {
    const clean = document.createElement("p");
    clean.className = "lines-conflict-empty";
    clean.textContent = t("当前未发现跨线文件交集");
    target.appendChild(clean);
    return;
  }
  for (const pair of pairs) {
    const detail = document.createElement("details");
    detail.className = "line-conflict";
    const summary = document.createElement("summary");
    summary.textContent = `${pair.left.label} ↔ ${pair.right.label} · ${pair.files.length} ${t("个重叠文件")}`;
    const files = document.createElement("ul");
    for (const file of pair.files) {
      const item = document.createElement("li");
      item.textContent = file;
      files.appendChild(item);
    }
    detail.append(summary, files);
    target.appendChild(detail);
  }
}

function lineStatusKey(line, lineRunning) {
  const status = line.status;
  if (["running", "suspected_stuck", "failed", "completed", "stopped", "idle"].includes(status)) {
    return status;
  }
  // 兼容旧快照/测试桩：后端升级前仍按既有 running 字段显示，不猜测卡住。
  return lineRunning ? "running" : "idle";
}

function lineStatusLabel(status) {
  return {
    running: t("运行中"),
    suspected_stuck: t("疑似卡住"),
    failed: t("失败"),
    completed: t("完成"),
    stopped: t("已停止"),
    idle: t("空闲"),
  }[status] || t("空闲");
}

function renderLines(lines) {
  collaborationLines = lines ?? [];
  const target = $("lines-list");
  // 线路页按快照重绘是必要的,但收活面板不是快照字段:它承载用户已经加载的
  // diff、门禁结果和确认状态。按 process_id 暂存并复挂,否则自动刷新会把用户
  // 正在进行的收活流程整块销毁。线路消失时没有对应 lane,面板自然不会复挂。
  const preservedHarvestPanels = new Map(
    [...target.querySelectorAll(".line-lane[data-process-id] .line-harvest")]
      .map((panel) => [panel.closest(".line-lane")?.dataset.processId, panel])
      .filter(([processId]) => processId),
  );
  const expandedChangedFiles = new Set(
    [...target.querySelectorAll(".line-lane[data-process-id]")]
      .filter((lane) => lane.querySelector(".line-changed-files")?.open)
      .map((lane) => lane.dataset.processId),
  );
  target.replaceChildren();
  const lineIsRunning = (line) => {
    const item = processItems.find((process) => process.id === line.process_id);
    return item ? processRunning(item) : Boolean(line.running);
  };
  const runningCount = collaborationLines.filter(lineIsRunning).length;
  const changedCount = new Set(
    collaborationLines.flatMap((line) => (line.changed_files ?? []).map(normalizedChangedFile)),
  ).size;
  $("lines-summary").textContent = `${runningCount} ${t("条运行中")} · ${collaborationLines.length} ${t("条线路")} · ${changedCount} ${t("个改动文件")}`;

  if (!collaborationLines.length) {
    const empty = document.createElement("div");
    empty.className = "lines-empty";
    empty.textContent = t("当前项目还没有可显示的线路");
    target.appendChild(empty);
    renderLineConflicts([]);
    return;
  }

  const codes = lineAgentCodes(collaborationLines);
  for (const line of collaborationLines) {
    const lineRunning = lineIsRunning(line);
    const lane = document.createElement("article");
    const code = codes.get(line.process_id) ?? "?";
    const accentIndex = code === "M" ? 0 : ((code.charCodeAt(0) - 64) % 4) + 1;
    lane.className = `line-lane line-accent-${accentIndex}${line.process_id === activeProcessId ? " active" : ""}`;
    lane.dataset.processId = line.process_id;

    const head = document.createElement("header");
    head.className = "line-lane-head";
    const identity = document.createElement("div");
    identity.className = "line-identity";
    const badge = document.createElement("span");
    badge.className = "line-agent-code";
    badge.textContent = code;
    const titleWrap = document.createElement("div");
    const title = document.createElement("h3");
    title.textContent = line.label;
    const processId = document.createElement("code");
    processId.textContent = line.process_id;
    titleWrap.append(title, processId);
    identity.append(badge, titleWrap);
    const state = document.createElement("span");
    const statusKey = lineStatusKey(line, lineRunning);
    state.className = `line-running-state ${statusKey.replace("_", "-")}`;
    state.textContent = lineStatusLabel(statusKey);
    head.append(identity, state);

    const claim = document.createElement("p");
    claim.className = "line-claim";
    claim.textContent = line.claim || t("未取得条目");
    if (line.claim_error) {
      claim.classList.add("error");
      claim.title = line.claim_error;
    }

    const facts = document.createElement("div");
    facts.className = "line-facts";
    facts.append(
      lineFact(t("阶段"), line.phase || t("空闲")),
      lineFact(t("当前工具"), line.current_tool || "—"),
      lineFact(t("分支"), line.branch || "—", "mono"),
      lineFact(t("工作树"), line.worktree_path || t("主工作区"), "mono"),
      lineFact(t("步数"), String(line.steps || 0)),
      lineFact(t("令牌"), `${formatLineTokens(line.input_tokens)} ↓ / ${formatLineTokens(line.output_tokens)} ↑`),
    );

    const changed = document.createElement("details");
    changed.className = "line-changed-files";
    const changedSummary = document.createElement("summary");
    const files = line.changed_files ?? [];
    changedSummary.textContent = `${files.length} ${t("个改动文件")}`;
    changed.appendChild(changedSummary);
    if (files.length) {
      const list = document.createElement("ul");
      for (const file of files) {
        const item = document.createElement("li");
        item.textContent = file;
        list.appendChild(item);
      }
      changed.appendChild(list);
    }
    if (line.changed_files_error) {
      const error = document.createElement("p");
      error.className = "line-file-error";
      error.textContent = line.changed_files_error;
      changed.appendChild(error);
    }
    changed.open = expandedChangedFiles.has(line.process_id);

    const actions = document.createElement("div");
    actions.className = "line-lane-actions";
    const open = document.createElement("button");
    open.type = "button";
    open.className = "ghost mini";
    open.textContent = line.process_id === activeProcessId ? t("当前线路") : t("切换到此线路");
    open.disabled = line.process_id === activeProcessId;
    open.addEventListener("click", async () => {
      await switchProcess(line.process_id);
      renderLines(collaborationLines);
    });
    actions.appendChild(open);
    // R-184 P5 收活五格:只有真实工作树上的线才有「收活」入口(主树没有可合并分支)。
    if (line.worktree_path) {
      const harvest = document.createElement("button");
      harvest.type = "button";
      harvest.className = "ghost mini line-harvest-toggle";
      harvest.textContent = t("收活");
      harvest.disabled = lineRunning;
      harvest.title = lineRunning ? t("线路运行中，停止并等待收口后才能收活") : "";
      harvest.addEventListener("click", () => {
        const panel = lane.querySelector(".line-harvest");
        if (panel) {
          panel.remove();
          return;
        }
        lane.appendChild(buildHarvestPanel(line, forProject(), code));
      });
      actions.appendChild(harvest);
    }
    if (!line.process_id.startsWith("d|")) {
      const close = document.createElement("button");
      close.type = "button";
      close.className = "ghost mini danger line-close";
      close.textContent = t("关闭线路");
      close.addEventListener("click", () => void closeParallelProcess(line.process_id));
      actions.appendChild(close);
    }
    lane.append(head, claim, facts, buildLineAutoControls(line), changed, actions);
    const preservedPanel = preservedHarvestPanels.get(line.process_id);
    if (preservedPanel) lane.appendChild(preservedPanel);
    target.appendChild(lane);
  }
  renderLineConflicts(collaborationLines);
  // R-247:协作快照与文档快照都来自同一 tracker 取得线；线路代号变化后同步重绘 badge。
  if (typeof latestDocsSnapshot !== "undefined" && latestDocsSnapshot && typeof renderDocuments === "function") {
    renderDocuments(latestDocsSnapshot);
  }
}

// ---------- R-184 P5:收活五格(设计文档 §5) ----------
// ① 读报告 → ② 人读 diff → ③ 跑门禁 → ④ 合并 → ⑤ 回写 tracker。
// ② 不可跳过:未点「我已读过 diff」时 ③④ 全部禁用(⑤ 由批5 接入)。
// 格3 门禁由 kanzei 跑(worktree_gate:fmt/clippy/test/前端冒烟),不能信线自己说的绿。
function forProject() {
  return currentProject;
}

function harvestClaimId(value) {
  const match = String(value ?? "").trim().match(/^(R|D)-(\d+)(?:\s|$)/);
  return match ? `${match[1]}-${match[2]}` : "";
}

function buildHarvestPanel(line, projectDir, agentCode) {
  const panel = document.createElement("div");
  panel.className = "line-harvest";
  const harvestState = {
    mergeCompleted: false,
    mergeGateRan: false,
    mergeGatePassed: false,
    postMergeGatePassed: false,
  };
  let trackerClaim = "";
  const title = document.createElement("h4");
  title.className = "line-harvest-title";
  title.textContent = `${t("收活")} · ${line.label} (${line.branch || line.process_id})`;
  panel.appendChild(title);

  // 格1 读报告:线的事实区已经展示,这里把「要合什么」摆成一段可读清单。
  const report = document.createElement("div");
  report.className = "harvest-step";
  const reportHead = document.createElement("div");
  reportHead.className = "harvest-step-head";
  reportHead.innerHTML = `<span class="harvest-step-no">1</span><strong>${t("读报告")}</strong><span class="harvest-step-state ok">${t("已展示")}</span>`;
  const reportBody = document.createElement("div");
  reportBody.className = "harvest-step-body";
  reportBody.textContent = `${line.claim || t("未声明条目")} · ${line.phase || t("空闲")} · ${(line.changed_files ?? []).length} ${t("个文件")}`;
  const trackerPicker = document.createElement("label");
  trackerPicker.className = "harvest-tracker-picker";
  const trackerPickerText = document.createElement("span");
  trackerPickerText.textContent = t("对话回写条目");
  const trackerSelect = document.createElement("select");
  trackerSelect.className = "harvest-tracker-select";
  trackerSelect.disabled = true;
  trackerSelect.appendChild(new Option(t("读取线路对话中…"), ""));
  trackerPicker.append(trackerPickerText, trackerSelect);
  reportBody.appendChild(trackerPicker);
  report.append(reportHead, reportBody);
  panel.appendChild(report);

  // 格2 人读 diff:必须显式确认,是语义层唯一防线(设计文档 §5 ②)。
  const diffStep = document.createElement("div");
  diffStep.className = "harvest-step";
  const diffHead = document.createElement("div");
  diffHead.className = "harvest-step-head";
  diffHead.innerHTML = `<span class="harvest-step-no">2</span><strong>${t("人读 diff")}</strong>`;
  const diffBody = document.createElement("div");
  diffBody.className = "harvest-step-body";
  const diffLoad = document.createElement("button");
  diffLoad.type = "button";
  diffLoad.className = "ghost mini harvest-diff-load";
  diffLoad.textContent = t("加载差异");
  const diffOutput = document.createElement("pre");
  diffOutput.className = "harvest-diff";
  diffOutput.hidden = true;
  const readConfirm = document.createElement("button");
  readConfirm.type = "button";
  readConfirm.className = "primary mini harvest-read-confirm";
  readConfirm.disabled = true;
  readConfirm.textContent = t("我已读过 diff");
  diffLoad.addEventListener("click", async () => {
    diffLoad.disabled = true;
    diffLoad.textContent = t("加载中…");
    try {
      const info = await invoke("worktree_diff", { projectDir, worktreePath: line.worktree_path });
      // R-179 验收①:接入 06-activity.js 的既有目录树渲染器 buildDiffTree,
      // 不新造查看器。porcelain 行形如 ` M src/foo.rs`(状态列 + 空格)——剥掉
      // 状态列取路径;增删计数从 diff 文本按文件统计(简化:该文件块内 + 开头
      // 行数 / - 开头行数)。
      const treeFiles = (info.files ?? []).map((raw) => {
        const path = raw.replace(/^[MADRCU?! ]{2} /, "").trim();
        return { path, additions: 0, deletions: 0 };
      });
      const diffPanel = document.createElement("div");
      diffPanel.className = "harvest-diff-tree";
      diffPanel.replaceChildren(typeof buildDiffTree === "function" ? buildDiffTree(treeFiles) : document.createTextNode(treeFiles.map((f) => f.path).join("\n")));
      const rawDiff = info.diff ? `${t("差异")}:\n${info.diff}` : t("工作树干净,没有未提交差异");
      const rawPre = document.createElement("details");
      rawPre.className = "harvest-diff-raw";
      const rawSummary = document.createElement("summary");
      rawSummary.textContent = t("原始差异文本");
      const rawBody = document.createElement("pre");
      rawBody.className = "harvest-diff";
      rawBody.textContent = rawDiff;
      rawPre.append(rawSummary, rawBody);
      diffOutput.replaceChildren(diffPanel, rawPre);
      diffOutput.hidden = false;
      readConfirm.disabled = false;
      diffLoad.textContent = t("重新加载");
    } catch (error) {
      diffLoad.disabled = false;
      diffLoad.textContent = t("加载差异");
      toastError(`${t("差异读取失败")}:${error}`);
    }
  });
  readConfirm.addEventListener("click", () => {
    diffStep.dataset.read = "1";
    diffStep.classList.add("confirmed");
    readConfirm.disabled = true;
    readConfirm.textContent = t("已确认");
    // R-222:格2 确认只解锁格3(门禁)——合并必须等门禁跑过或显式覆盖。
    gateButton.disabled = false;
  });
  diffBody.append(diffLoad, diffOutput, readConfirm);
  diffStep.append(diffHead, diffBody);
  panel.appendChild(diffStep);

  // 格3 跑门禁:kanzei 亲自跑,失败不阻断(看全貌),成败在面板上逐步骤可见。
  const gateStep = document.createElement("div");
  gateStep.className = "harvest-step";
  const gateHead = document.createElement("div");
  gateHead.className = "harvest-step-head";
  gateHead.innerHTML = `<span class="harvest-step-no">3</span><strong>${t("跑门禁")}</strong>`;
  const gateBody = document.createElement("div");
  gateBody.className = "harvest-step-body";
  const gateButton = document.createElement("button");
  gateButton.type = "button";
  gateButton.className = "ghost mini harvest-gate-run";
  gateButton.disabled = true;
  gateButton.textContent = t("运行门禁");
  const gateOutput = document.createElement("div");
  gateOutput.className = "harvest-gate-result";
  gateButton.addEventListener("click", async () => {
    gateButton.disabled = true;
    gateButton.textContent = t("运行中…");
    gateOutput.replaceChildren();
    try {
      const steps = await invoke("worktree_gate", { projectDir, worktreePath: line.worktree_path });
      for (const step of steps) {
        const row = document.createElement("div");
        row.className = `harvest-gate-step ${step.ok ? "ok" : "err"}`;
        row.dataset.gateName = step.name;
        const mark = document.createElement("span");
        mark.className = "harvest-gate-mark";
        mark.textContent = step.ok ? "✓" : "✗";
        const name = document.createElement("code");
        name.textContent = step.name;
        const detail = document.createElement("pre");
        detail.textContent = step.summary || t("(无输出)");
        row.append(mark, name, detail);
        gateOutput.appendChild(row);
      }
      const anyFail = steps.some((step) => !step.ok);
      harvestState.mergeGateRan = true;
      harvestState.mergeGatePassed = !anyFail;
      if (anyFail) {
        const warn = document.createElement("p");
        warn.className = "harvest-gate-warn";
        warn.textContent = t("门禁未通过:请先在线上修复,再重跑");
        gateOutput.appendChild(warn);
      } else {
        const pass = document.createElement("p");
        pass.className = "harvest-gate-pass";
        pass.textContent = t("门禁通过");
        gateOutput.appendChild(pass);
        // R-222:门禁通过才解锁合并(防线①:门禁是合并前置)。
        mergeButton.disabled = false;
      }
      gateButton.disabled = false;
      gateButton.textContent = t("重跑门禁");
    } catch (error) {
      gateButton.disabled = false;
      gateButton.textContent = t("运行门禁");
      toastError(`${t("门禁执行失败")}:${error}`);
    }
  });
  gateBody.append(gateButton, gateOutput);
  gateStep.append(gateHead, gateBody);
  panel.appendChild(gateStep);

  // 格4 合并:复用既有 worktree_merge(含 merge-tree 预检 + --no-ff)。
  const mergeStep = document.createElement("div");
  mergeStep.className = "harvest-step";
  const mergeHead = document.createElement("div");
  mergeHead.className = "harvest-step-head";
  mergeHead.innerHTML = `<span class="harvest-step-no">4</span><strong>${t("合并")}</strong>`;
  const mergeBody = document.createElement("div");
  mergeBody.className = "harvest-step-body";
  const mergeButton = document.createElement("button");
  mergeButton.type = "button";
  mergeButton.className = "ghost mini harvest-merge-run";
  mergeButton.disabled = true;
  mergeButton.textContent = t("合并到主线");
  mergeButton.addEventListener("click", async () => {
    // R-222 防线①:合并前置门禁——状态来自 JS 对象，dataset 只用于展示/调试。
    const gateOk = harvestState.mergeGatePassed;
    const gateRan = harvestState.mergeGateRan;
    if (!gateRan || !gateOk) {
      const reason = gateRan ? t("门禁未通过") : t("门禁未运行");
      const ok = await confirmDialog({
        title: t("覆盖确认"),
        message: `${reason}。${t("合并前请先在线上跑通门禁;仍要继续合并吗")}\n${t("覆盖确认将记录到活动轨迹")}`,
        okText: t("确认"),
        danger: true,
      });
      if (!ok) return;
      // 覆盖确认落轨迹:活动面板能回溯「谁在什么状态下强行合并」。
      console.info(`[harvest-override] merge without passing gate (${reason}) for ${line.branch || line.process_id}`);
      if (window.__activityLog) {
        window.__activityLog.push({
          kind: "harvest-override",
          at: new Date().toISOString(),
          branch: line.branch || line.process_id,
          reason,
        });
      }
    }
    const item = { path: line.worktree_path, branch: line.branch };
    const ok = await confirmWorktreeMerge(item, projectDir);
    if (!ok) return;
    mergeButton.disabled = true;
    mergeButton.textContent = t("合并中…");
    try {
      const result = await invoke("worktree_merge", { projectDir, worktreePath: line.worktree_path });
      mergeStep.classList.add("confirmed");
      const done = document.createElement("p");
      done.className = "harvest-merge-done";
      done.textContent = result;
      mergeBody.replaceChildren(done);
      const stateTag = document.createElement("span");
      stateTag.className = "harvest-step-state ok";
      stateTag.textContent = t("已合并");
      mergeHead.appendChild(stateTag);
      // 合并结果与候选读取可任意先后到达；统一由同一函数投影第 5 格。
      harvestState.mergeCompleted = true;
      // R-222 防线②:合并成功后解锁「合并后全量」步骤(格5 前)。
      postMergeButton.disabled = false;
      syncWritebackAvailability();
      // R-247:后端已在合并成功后释放取得线；立即刷新两份只读投影，不能等下一次
      // 定时轮询让 backlog 和泳道短暂继续显示旧持有者。
      void refreshDocs();
      void refreshLines();
    } catch (error) {
      mergeButton.disabled = false;
      mergeButton.textContent = t("合并到主线");
      toastError(`${t("合并失败")}:${error}`);
    }
  });
  mergeBody.append(mergeButton);
  mergeStep.append(mergeHead, mergeBody);
  panel.appendChild(mergeStep);

  // R-222 防线②:合并后全量——两条线各自绿≠合起来绿(设计文档 §5 ④)。
  // 合并成功后在**主根**跑全量门禁,结果可见;通过后解锁格5 回写。
  const postMergeStep = document.createElement("div");
  postMergeStep.className = "harvest-step";
  const postMergeHead = document.createElement("div");
  postMergeHead.className = "harvest-step-head";
  postMergeHead.innerHTML = `<span class="harvest-step-no">5</span><strong>${t("合并后全量")}</strong>`;
  const postMergeBody = document.createElement("div");
  postMergeBody.className = "harvest-step-body";
  const postMergeButton = document.createElement("button");
  postMergeButton.type = "button";
  postMergeButton.className = "ghost mini harvest-postmerge-run";
  postMergeButton.disabled = true;
  postMergeButton.textContent = t("合并后全量(主根)");
  const postMergeOutput = document.createElement("div");
  postMergeOutput.className = "harvest-gate-result";
  postMergeButton.addEventListener("click", async () => {
    postMergeButton.disabled = true;
    postMergeButton.textContent = t("运行中…");
    postMergeOutput.replaceChildren();
    try {
      const steps = await invoke("worktree_post_merge_gate", { projectDir });
      for (const step of steps) {
        const row = document.createElement("div");
        row.className = `harvest-gate-step ${step.ok ? "ok" : "err"}`;
        const mark = document.createElement("span");
        mark.className = "harvest-gate-mark";
        mark.textContent = step.ok ? "✓" : "✗";
        const name = document.createElement("code");
        name.textContent = step.name;
        const detail = document.createElement("pre");
        detail.textContent = step.summary || t("(无输出)");
        row.append(mark, name, detail);
        postMergeOutput.appendChild(row);
      }
      const anyFail = steps.some((step) => !step.ok);
      if (anyFail) {
        const warn = document.createElement("p");
        warn.className = "harvest-gate-warn";
        warn.textContent = t("合并后全量未通过:请修复主根后重跑");
        postMergeOutput.appendChild(warn);
        postMergeButton.disabled = false;
        postMergeButton.textContent = t("重跑合并后全量");
      } else {
        const pass = document.createElement("p");
        pass.className = "harvest-gate-pass";
        pass.textContent = t("合并后全量通过");
        postMergeOutput.appendChild(pass);
        harvestState.postMergeGatePassed = true;
        postMergeStep.classList.add("confirmed");
        const stateTag = document.createElement("span");
        stateTag.className = "harvest-step-state ok";
        stateTag.textContent = t("已通过");
        postMergeHead.appendChild(stateTag);
        postMergeButton.textContent = t("已通过");
        // 合并后全量通过才解锁格5 回写。
        syncWritebackAvailability();
      }
    } catch (error) {
      postMergeButton.disabled = false;
      postMergeButton.textContent = t("重跑合并后全量");
      toastError(`${t("合并后全量执行失败")}:${error}`);
    }
  });
  postMergeBody.append(postMergeButton, postMergeOutput);
  postMergeStep.append(postMergeHead, postMergeBody);
  panel.appendChild(postMergeStep);

  // 格6 回写 tracker:合并完成后把线交付落主根一份(设计文档 §5 ⑤)。
  // 只追加进展不改状态;claim 不是条目 ID 时后端拒绝,不让用户误以为已登记。
  // 合并后全量通过前禁用——R-222:回写以合并后全量绿为前置。
  const writebackStep = document.createElement("div");
  writebackStep.className = "harvest-step";
  const writebackHead = document.createElement("div");
  writebackHead.className = "harvest-step-head";
  writebackHead.innerHTML = `<span class="harvest-step-no">6</span><strong>${t("回写 tracker")}</strong>`;
  const writebackBody = document.createElement("div");
  writebackBody.className = "harvest-step-body";
  const writebackHint = document.createElement("p");
  writebackHint.className = "harvest-writeback-hint";
  writebackHint.textContent = t("合并完成后可回写");
  const writebackButton = document.createElement("button");
  writebackButton.type = "button";
  writebackButton.className = "primary mini harvest-writeback-run";
  writebackButton.disabled = true;
  writebackButton.textContent = t("需先合并");
  const writebackOutput = document.createElement("pre");
  writebackOutput.className = "harvest-writeback-output";
  writebackOutput.hidden = true;
  writebackButton.addEventListener("click", async () => {
    if (writebackButton.dataset.done === "1") return;
    writebackButton.disabled = true;
    writebackButton.textContent = t("回写中…");
    try {
      const result = await invoke("worktree_harvest_writeback", {
        projectDir,
        worktreePath: line.worktree_path,
        claim: trackerClaim,
        agentCode: agentCode,
        branch: line.branch || "",
      });
      writebackStep.classList.add("confirmed");
      writebackButton.dataset.done = "1";
      writebackButton.textContent = t("已回写");
      writebackOutput.textContent = result;
      writebackOutput.hidden = false;
      const stateTag = document.createElement("span");
      stateTag.className = "harvest-step-state ok";
      stateTag.textContent = t("已回写");
      writebackHead.appendChild(stateTag);
      refreshLines();
      refreshWorktrees();
      refreshGit();
    } catch (error) {
      writebackButton.disabled = false;
      writebackButton.textContent = t("重试回写");
      toastError(`${t("回写失败")}:${error}`);
    }
  });
  writebackBody.append(writebackHint, writebackButton, writebackOutput);
  writebackStep.append(writebackHead, writebackBody);
  panel.appendChild(writebackStep);

  function syncWritebackAvailability() {
    // R-222:回写需 合并完成 + 合并后全量通过 双前置(防线②)。
    if (!harvestState.mergeCompleted) return;
    const postMergeOk = harvestState.postMergeGatePassed;
    if (!postMergeOk) {
      writebackButton.disabled = true;
      writebackButton.textContent = t("需先跑合并后全量");
      writebackHint.textContent = t("合并后全量通过后才能回写 tracker");
      writebackHint.classList.add("warn-text");
      return;
    }
    if (trackerClaim) {
      writebackButton.disabled = false;
      writebackButton.textContent = t("回写 tracker");
      writebackHint.textContent = `${trackerClaim} · ${t("由")} ${agentCode} ${t("线交付")}`;
      writebackHint.classList.remove("warn-text");
    } else {
      writebackButton.disabled = true;
      writebackButton.textContent = t("无有效条目");
      writebackHint.textContent = t("当前线路对话中没有可确认的活动 R-xxx / D-xxx 条目，合并已完成；请用主代理的 tracker 工具手动登记");
      writebackHint.classList.add("warn-text");
    }
  }

  trackerSelect.addEventListener("change", () => {
    trackerClaim = harvestClaimId(trackerSelect.value);
    syncWritebackAvailability();
  });

  void (async () => {
    try {
      const candidates = await invoke("worktree_harvest_candidates", {
        projectDir,
        processId: line.process_id,
      });
      trackerSelect.replaceChildren();
      if (!candidates.length) {
        trackerSelect.appendChild(new Option(t("未从线路对话找到活动条目"), ""));
        trackerSelect.disabled = true;
      } else if (candidates.length === 1) {
        trackerSelect.appendChild(new Option(candidates[0], candidates[0]));
        trackerSelect.value = candidates[0];
        trackerSelect.disabled = true;
        trackerClaim = candidates[0];
      } else {
        trackerSelect.appendChild(new Option(t("请选择本次交付条目"), ""));
        for (const candidate of candidates) trackerSelect.appendChild(new Option(candidate, candidate));
        trackerSelect.disabled = false;
        trackerClaim = "";
      }
      syncWritebackAvailability();
    } catch (error) {
      trackerSelect.replaceChildren(new Option(t("线路对话条目读取失败"), ""));
      trackerSelect.disabled = true;
      trackerClaim = "";
      syncWritebackAvailability();
      log(`${t("线路对话条目读取失败")}:${error}`, "warn");
    }
  })();

  return panel;
}

async function refreshLines() {
  if (!currentProject) {
    renderLines([]);
    return;
  }
  if (linesRefreshInFlight) {
    linesRefreshQueued = true;
    return;
  }
  const forProject = currentProject;
  linesRefreshInFlight = true;
  try {
    // 模型目录按项目缓存,不随每次线路刷新重探(8 秒一轮的探测既慢又白费);
    // 目录空也照画——每线下拉至少有「agent 默认」与该线已记住的模型。
    await loadLinesModelCatalog();
    if (currentProject !== forProject) return;
    const lines = await invoke("collaboration_snapshot", { projectDir: forProject });
    if (currentProject !== forProject) return;
    renderLines(lines);
  } catch (error) {
    if (currentProject === forProject) {
      log(`${t("并行线路读取失败")}:${error}`, "warn");
      $("lines-summary").textContent = `${t("并行线路读取失败")}:${error}`;
    }
  } finally {
    linesRefreshInFlight = false;
    if (linesRefreshQueued) {
      linesRefreshQueued = false;
      scheduleLinesRefresh(250);
    } else {
      const running = collaborationLines.some((line) => line.running);
      scheduleLinesRefresh(running ? LINES_REFRESH_RUNNING_MS : LINES_REFRESH_IDLE_MS);
    }
  }
}

function scheduleLinesRefresh(delay) {
  if (linesRefreshTimer) clearTimeout(linesRefreshTimer);
  linesRefreshTimer = setTimeout(() => {
    linesRefreshTimer = null;
    if ($("view-lines").classList.contains("active")) void refreshLines();
  }, delay);
}

async function confirmWorktreeMerge(item, forProject) {
  let lines;
  try {
    lines = await invoke("collaboration_snapshot", { projectDir: forProject });
  } catch (error) {
    return confirmDialog({ title: t("合并前检查失败"), message: `${error}\n${t("仍要继续进入 Git 合并吗")}`, okText: t("确认"), danger: true });
  }
  if (currentProject !== forProject) return false;
  renderLines(lines);
  // R-179 验收③:merge-tree 冲突预检的可读形态——列出冲突文件,而不是一句
  // 「有冲突」。worktree_merge_preview 返回冲突文件列表(空 = 无冲突)。
  let gitConflicts = [];
  try {
    const preview = await invoke("worktree_merge_preview", {
      projectDir: forProject,
      worktreePath: item.path,
    });
    gitConflicts = Array.isArray(preview) ? preview : [];
  } catch {
    gitConflicts = []; // 预检失败不阻断:实际合并时后端仍会拒绝并保留现场。
  }
  const conflictNote = gitConflicts.length
    ? `${t("Git 合并冲突文件")}:\n${gitConflicts.join("\n")}\n`
    : "";
  const matching = lineConflictPairs(lines).filter((pair) =>
    [pair.left, pair.right].some((line) => line.worktree_path === item.path || line.branch === item.branch),
  );
  if (matching.length) {
    document.querySelector('.activity-item[data-view="lines"]').click();
    const count = matching.reduce((total, pair) => total + pair.files.length, 0);
    return confirmDialog({
      title: t("检测到跨线文件交集"),
      message: `${conflictNote}${t("检测到跨线文件交集")}:${count} ${t("项")}\n${t("文本层已检查 · 语义层未检查")}\n${t("仍要继续进入 Git 合并吗")}`,
      okText: t("确认"),
      danger: true,
    });
  }
  return confirmDialog({
    title: t("继续进入 Git 合并吗"),
    message: `${conflictNote}${t("当前未发现跨线文件交集")}。${t("文本层已检查 · 语义层未检查")}。\n${t("继续进入 Git 合并吗")}`,
    okText: t("确认"),
  });
}

// 侧栏的 ↻ 撤掉之后,这一颗要同时刷线路与工作树清单——否则孤儿树没有任何刷新入口。
$("lines-refresh").addEventListener("click", () => { void refreshLines(); void refreshWorktrees(); });
$("lines-work-item").addEventListener("change", () => renderLineWorkItemOptions());
$("lines-add").addEventListener("click", createWorktreeLine);
