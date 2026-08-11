// ---------- R-184 B 面:任务级并行线路 ----------
// 这里不另造状态仓库。agent 协作块和用户并列视图都读 collaboration_snapshot,
// 其 branch / phase / tool / changed_files 来自后端当前运行态和 git 现场。
let collaborationLines = [];
let linesRefreshInFlight = false;

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
  target.replaceChildren();
  const runningCount = collaborationLines.filter((line) => line.running).length;
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
    state.className = `line-running-state ${line.running ? "running" : "idle"}`;
    state.textContent = line.running ? t("运行中") : t("空闲");
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

    const actions = document.createElement("div");
    actions.className = "line-lane-actions";
    const open = document.createElement("button");
    open.type = "button";
    open.className = "ghost mini";
    open.textContent = line.process_id === activeProcessId ? t("当前线路") : t("切换到此线路");
    open.disabled = line.process_id === activeProcessId;
    open.addEventListener("click", async () => {
      await refreshProcesses();
      await switchProcess(line.process_id);
      renderLines(collaborationLines);
    });
    actions.appendChild(open);
    lane.append(head, claim, facts, changed, actions);
    target.appendChild(lane);
  }
  renderLineConflicts(collaborationLines);
}

async function refreshLines() {
  if (!currentProject || linesRefreshInFlight) {
    if (!currentProject) renderLines([]);
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
  }
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

$("lines-refresh").addEventListener("click", refreshLines);
$("lines-add").addEventListener("click", createWorktreeLine);
setInterval(() => {
  if ($("view-lines").classList.contains("active")) refreshLines();
}, 2500);
