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
    state.className = `line-running-state ${lineRunning ? "running" : "idle"}`;
    state.textContent = lineRunning ? t("运行中") : t("空闲");
    head.append(identity, state);

    const claim = document.createElement("p");
    claim.className = "line-claim";
    claim.textContent = line.claim || t("未声明条目");

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
    lane.append(head, claim, facts, changed, actions);
    const preservedPanel = preservedHarvestPanels.get(line.process_id);
    if (preservedPanel) lane.appendChild(preservedPanel);
    target.appendChild(lane);
  }
  renderLineConflicts(collaborationLines);
  // D-304:协作快照是条目「被取得」标记的唯一事实源；刷新线路后同步重绘当前文档行。
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
  const trackerClaim = harvestClaimId(line.claim);
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
  const lines = [t("认领条目"), t("当前阶段"), t("改动文件")];
  reportBody.textContent = `${line.claim || t("未声明条目")} · ${line.phase || t("空闲")} · ${(line.changed_files ?? []).length} ${t("个文件")}`;
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
      const files = (info.files ?? []).join("\n");
      const body = [files ? `${t("文件")}:\n${files}` : "", info.diff ? `${t("差异")}:\n${info.diff}` : t("工作树干净,没有未提交差异")].filter(Boolean).join("\n\n");
      diffOutput.textContent = body;
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
    gateButton.disabled = false;
    mergeButton.disabled = false;
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
      // 合并成功不等于一定可回写:没有严格的 R-/D- claim 时不能制造可点击的
      // 失败入口。合并结果已经落地,但 tracker 记录要由主代理手动登记。
      if (trackerClaim) {
        writebackButton.disabled = false;
        writebackButton.textContent = t("回写 tracker");
        writebackHint.textContent = `${trackerClaim} · ${t("由")} ${agentCode} ${t("线交付")}`;
      } else {
        writebackButton.disabled = true;
        writebackButton.textContent = t("无有效条目");
        writebackHint.textContent = t("当前线路未声明有效的 R-xxx / D-xxx 条目，合并已完成；请用主代理的 tracker 工具手动登记");
        writebackHint.classList.add("warn-text");
      }
    } catch (error) {
      mergeButton.disabled = false;
      mergeButton.textContent = t("合并到主线");
      toastError(`${t("合并失败")}:${error}`);
    }
  });
  mergeBody.append(mergeButton);
  mergeStep.append(mergeHead, mergeBody);
  panel.appendChild(mergeStep);

  // 格5 回写 tracker:合并完成后把线交付落主根一份(设计文档 §5 ⑤)。
  // 只追加进展不改状态;claim 不是条目 ID 时后端拒绝,不让用户误以为已登记。
  // 未合并前禁用——②不可跳过同时约束③④⑤(设计文档 §10 验收5)。
  const writebackStep = document.createElement("div");
  writebackStep.className = "harvest-step";
  const writebackHead = document.createElement("div");
  writebackHead.className = "harvest-step-head";
  writebackHead.innerHTML = `<span class="harvest-step-no">5</span><strong>${t("回写 tracker")}</strong>`;
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
    return window.confirm(`${t("合并前检查失败")}:${error}\n${t("仍要继续进入 Git 合并吗")}`);
  }
  if (currentProject !== forProject) return false;
  renderLines(lines);
  const matching = lineConflictPairs(lines).filter((pair) =>
    [pair.left, pair.right].some((line) => line.worktree_path === item.path || line.branch === item.branch),
  );
  if (matching.length) {
    document.querySelector('.activity-item[data-view="lines"]').click();
    const count = matching.reduce((total, pair) => total + pair.files.length, 0);
    return window.confirm(
      `${t("检测到跨线文件交集")}:${count} ${t("项")}\n${t("文本层已检查 · 语义层未检查")}\n${t("仍要继续进入 Git 合并吗")}`,
    );
  }
  return window.confirm(`${t("当前未发现跨线文件交集")}。${t("文本层已检查 · 语义层未检查")}。\n${t("继续进入 Git 合并吗")}`);
}

$("lines-refresh").addEventListener("click", () => void refreshLines());
$("lines-add").addEventListener("click", createWorktreeLine);
