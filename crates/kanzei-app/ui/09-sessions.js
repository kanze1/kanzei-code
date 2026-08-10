let worktreeItems = [];
function renderWorktrees(items) {
  worktreeItems = items ?? [];
  const list = $("worktree-list");
  list.replaceChildren();
  if (!worktreeItems.length) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("暂无隔离工作树");
    list.appendChild(empty);
    return;
  }
  for (const item of worktreeItems) {
    const row = document.createElement("div");
    row.className = "worktree-entry";
    const label = document.createElement("div");
    label.textContent = `${item.branch} · ${item.clean ? t("干净") : `${item.files.length} ${t("项改动")}`}`;
    label.title = item.path;
    const actions = document.createElement("div");
    for (const [text, action] of [[t("差异"), "diff"], [t("合并"), "merge"], [t("放弃"), "discard"]]) {
      const button = document.createElement("button");
      button.className = `ghost mini ${action === "merge" ? "worktree-merge" : ""}`;
      button.textContent = text;
      button.addEventListener("click", () => handleWorktreeAction(item, action));
      actions.appendChild(button);
    }
    row.append(label, actions);
    list.appendChild(row);
  }
}
async function refreshWorktrees() {
  if (!currentProject) return renderWorktrees([]);
  const saved = JSON.parse(localStorage.getItem(`kz-worktrees:${currentProject}`) || "[]");
  const live = [];
  for (const path of saved) {
    try { live.push(await invoke("worktree_diff", { projectDir: currentProject, worktreePath: path })); }
    catch (error) { log(`工作树已不可用:${path} · ${error}`, "warn"); }
  }
  renderWorktrees(live);
}
async function handleWorktreeAction(item, action) {
  try {
    if (action === "diff") {
      if (item.clean) {
        toast(t("工作树干净,没有未提交差异"));
      } else {
        const file_list = item.files.join("\n");
        const diff = item.diff?.trim() || t("未跟踪文件尚未包含在 git diff 中");
        log(`${item.branch}\n${t("文件列表")}:\n${file_list}\n\n${t("实际差异")}:\n${diff}`, "info");
        $("log-panel").classList.remove("hidden");
        toast(t("工作树差异已写入运行日志"));
      }
      return;
    }
    if (action === "discard" && !window.confirm(`${t("放弃工作树")} ${item.branch}？${t("未提交改动会阻止删除并保留现场")}`)) return;
    const command = action === "merge" ? "worktree_merge" : "worktree_discard";
    const result = await invoke(command, { projectDir: currentProject, worktreePath: item.path });
    if (String(result).length > 160) {
      log(String(result), "info");
      $("log-panel").classList.remove("hidden");
      toast(t("工作树操作完成，详细结果已写入运行日志"));
    } else {
      toast(result);
    }
    if (action === "discard") {
      const paths = JSON.parse(localStorage.getItem(`kz-worktrees:${currentProject}`) || "[]").filter((path) => path !== item.path);
      localStorage.setItem(`kz-worktrees:${currentProject}`, JSON.stringify(paths));
    }
    await refreshWorktrees();
    refreshGit();
  } catch (error) {
    toastError(String(error), { retry: () => handleWorktreeAction(item, action) });
  }
}("click", refreshWorktrees);
$("worktree-add").addEventListener("click", async () => {
  if (!currentProject) return;
  const name = `thread-${new Date().toISOString().replace(/[-:TZ.]/g, "").slice(0, 14)}`;
  try {
    const item = await invoke("worktree_create", { projectDir: currentProject, name });
    const paths = JSON.parse(localStorage.getItem(`kz-worktrees:${currentProject}`) || "[]");
    paths.push(item.path);
    localStorage.setItem(`kz-worktrees:${currentProject}`, JSON.stringify(paths));
    toast(`${t("隔离工作树已创建")}:${item.path}`);
    await refreshWorktrees();
  } catch (error) {
    toastError(`创建工作树失败:${error}`);
  }
});

// ---------- R-030:项目内独立进程 ----------
let syncedRunningProcessId = null;
// 已向后端补拉过待答队列的会话,防止每次进程列表刷新都打一次 pending_asks_get。
let askSyncedSession = null;
function renderProcesses(items) {
  processItems = items ?? [];
  const previousSessionId = activeSessionId;
  // R-086:后端是运行态权威,先把返回的 running 校正进各会话状态机(事件可能
  // 丢失),视图只投影活动会话的状态机,而不是直接信某一次轮询的瞬时值。
  // 已收敛终态(converged)的会话不被旧轮询值翻回——事件在轮询采样之后才发
  // 出是正常时序,此刻 process_list 里仍是 running=true,但会话实际已结束。
  for (const item of processItems) {
    const state = sessionState(item.session_id);
    if (!state.converged) state.running = Boolean(item.running);
  }
  if (!activeProcessId || !processItems.some((item) => item.id === activeProcessId)) {
    const preferred = processItems.find((item) => item.id.startsWith("d|")) || processItems[0];
    activeProcessId = preferred?.id ?? null;
  }
  const active = processItems.find((item) => item.id === activeProcessId);
  activeSessionId = active?.session_id ?? null;
  if (activeSessionId && activeSessionId !== previousSessionId) void syncAutoRunState();
  // R-086:活动会话换人(含首次拿到进程列表——界面重载后就是这条路)时向后端
  // 补拉一次待答队列。后端 asks 表活得比 webview 久,不补拉的话重载前挂起的
  // 权限询问再也不会出现,而后端还在 await 它的答复。按会话去重,只拉一次。
  if (activeSessionId && activeSessionId !== askSyncedSession) {
    askSyncedSession = activeSessionId;
    refreshPendingAsks();
  }
  pumpAsk();
  // 活动进程换人时按状态机重算运行态(切项目/进程后旧会话的终态也经状态机
  // 收敛,不会丢)。只在身份变化时同步,避免与"停止"按钮的本地即时复位互相打架。
  const activeRunning = activeSessionId ? sessionState(activeSessionId).running : false;
  if (activeProcessId !== syncedRunningProcessId) {
    syncedRunningProcessId = activeProcessId;
    setRunning(activeRunning, activeRunning ? t("运行中") : t("空闲"));
  }
  const tabs = $("process-tabs");
  tabs.replaceChildren();
  for (const item of processItems) {
    const tab = document.createElement("button");
    tab.type = "button";
    // R-086:标签的 ● 从该会话状态机取——后台会话的 done 已收敛终态,不依赖
    // 这次轮询是否恰好拉到了最新 running。
    const itemRunning = sessionState(item.session_id).running;
    tab.className = `process-tab${item.id === activeProcessId ? " active" : ""}${itemRunning ? " running" : ""}`;
    tab.textContent = `${item.label}${itemRunning ? " ●" : ""}`;
    tab.title = `${item.id}${item.model ? ` · ${item.model}` : ""}`;
    tab.addEventListener("click", () => switchProcess(item.id));
    tabs.appendChild(tab);
  }
  $("process-subagent").checked = active?.subagent ?? true;
}

async function refreshProcesses() {
  if (!currentProject) return;
  try {
    renderProcesses(await invoke("process_list", { projectDir: currentProject }));
  } catch (err) {
    log(`${t("进程列表刷新失败")}:${err}`, "warn");
  }
}

async function refreshPendingAsks() {
  if (!currentProject || !activeSessionId) return;
  try {
    const pending = await invoke("pending_asks_get", {
      projectDir: currentProject,
      processId: activeProcessId,
    });
    const queue = askQueueFor(activeSessionId);
    const known = new Set(queue.map((item) => item.id));
    if (askActive?.sessionId === activeSessionId) known.add(askActive.id);
    for (const payload of pending || []) {
      if (!known.has(payload.id)) {
        queue.push(payload);
        known.add(payload.id);
      }
    }
    pumpAsk();
  } catch (err) {
    log(`${t("待处理权限询问恢复失败")}:${err}`, "warn");
  }
}


async function switchProcess(processId) {
  if (processId === activeProcessId) return;
  const target = processItems.find((item) => item.id === processId);
  if (!target) return;
  // 后端只保存 dev/research；切换前先把前端的 dev-auto 档位绑定到旧进程，
  // 这样回切时不会因后端 profile=dev 而退回 dev-pair。
  if (activeProcessId) processProfileUi.set(activeProcessId, $("profile-select").value);
  hideAsk(true);
  activeProcessId = processId;
  activeSessionId = target.session_id;
  void syncAutoRunState();
  // 下面有一次显式 await refreshPendingAsks(),先认领这个会话,免得 renderProcesses
  // 里的补拉守卫又打一次 pending_asks_get(结果会被 id 去重,只是白跑一趟)。
  askSyncedSession = target.session_id;
  pumpAsk();
  // R-086:运行态投影自该会话的状态机(后台终态已收敛),不直接信 processItems
  // 的瞬时轮询值——事件先到状态机,切回时看到的才是准的。
  setRunning(
    sessionState(target.session_id).running,
    sessionState(target.session_id).running ? t("运行中") : t("空闲")
  );
  renderProcesses(processItems);
  clearChat();
  bgClear();
  renderTodoPanel([], 0, 0);
  await loadConversation();
  await refreshPendingAsks();
  await refreshDocs();
  await loadModels();
  // 模型下拉按进程回显:未设置覆盖时回到 agent 默认(空值),不保留上一个进程的选择。
  $("model-select").value = target.model || "";
  if (target.profile) applyProfileValue(target.profile);
  refreshGit();
  refreshPendingInputs();
  refreshProcesses();
  log(`${t("已切换到进程")} ${target.label}`);
}

$("process-add").addEventListener("click", async () => {
  if (!currentProject) return;
  try {
    const item = await invoke("process_create", { projectDir: currentProject, subagent: true });
    await refreshProcesses();
    await switchProcess(item.id);
  } catch (err) {
    toastError(`${t("创建进程失败")}:${err}`);
  }
});

$("process-subagent").addEventListener("change", async (event) => {
  if (!activeProcessId) return;
  try {
    await invoke("process_update", { processId: activeProcessId, subagent: event.target.checked });
    await refreshProcesses();
  } catch (err) {
    event.target.checked = !event.target.checked;
    toastError(`${t("更新进程能力失败")}:${err}`);
  }
});

// ---------- 项目管理 ----------
function baseName(path) {
  const parts = path.replaceAll("\\", "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

function syncDocumentsProjectSelect(prefs) {
  const select = $("documents-project-select");
  if (!select) return;
  select.replaceChildren();
  for (const path of prefs.projects ?? []) {
    select.appendChild(new Option(prefs.names?.[path] || baseName(path), path));
  }
  select.value = prefs.current ?? "";
  select.disabled = !(prefs.projects ?? []).length;
}

// D-170:所选目录若没有 .kanzei,后端会一路向上找,落到祖先目录上——
// 于是共用同一祖先的几个项目读的是同一份 requirements.md,需求在项目之间串。
// 存量项目改根会让会话 id 变化(历史看起来消失),所以不静默迁移:如实报出来,
// 给一键分离,由用户决定。
// 隔离问题往往一次影响多个项目(它们共用同一个祖先)。只在当前项目上提示会让
// 用户切一个发现一个,修到一半以为修完了。这里一次报全,只报一次。
let isolationReported = false;
async function reportIsolationAcrossProjects() {
  if (isolationReported) return;
  isolationReported = true;
  try {
    const report = await invoke("projects_isolation_report");
    for (const path of report.autoRepaired ?? []) {
      log(`${t("已为本项目建立独立空间")}:${path}`);
    }
    const shared = report.shared ?? [];
    if (shared.length) {
      log(
        `${t("以下项目仍与上级目录共用数据,切过去可一键分离")}:` +
          shared.map((s) => `${s.project} → ${s.resolved}`).join("；"),
        "warn",
      );
    }
  } catch {
    /* 体检失败不影响主流程 */
  }
}

async function checkProjectIsolation() {
  const box = $("project-shared-warn");
  if (!box || !currentProject) return;
  let info;
  try {
    info = await invoke("project_root_info", { projectDir: currentProject });
  } catch {
    return;
  }
  // 无损修复过就只留一行日志,不打扰——用户看到的内容没有任何变化。
  if (info.autoRepaired) log(`${t("已为本项目建立独立空间")}:${info.selected}`);
  box.classList.toggle("hidden", !info.shared);
  if (!info.shared) {
    // 顺带体检一次全部项目:受影响的往往不止当前这个,切一个发现一个太慢。
    reportIsolationAcrossProjects();
    return;
  }
  box.innerHTML = "";
  const text = document.createElement("div");
  text.textContent = `${t("本项目没有独立空间,正在使用上级目录的数据(与共用该上级的其它项目混在一起)")}:${info.resolved}`;
  const act = document.createElement("button");
  act.type = "button";
  act.className = "ghost mini";
  act.textContent = t("在此建立独立空间");
  act.title = t("只在本目录创建 .kanzei,不搬动上级目录的既有条目");
  act.addEventListener("click", async () => {
    try {
      await invoke("project_detach", { projectDir: currentProject });
      toast(t("已建立独立空间"));
      // 分离改变了项目根:文档、会话、记忆都要按新根重取,否则界面还停在旧根的数据上。
      await refreshDocs();
      await loadConversation();
      isolationReported = false; // 允许再体检一次,看还有没有别的项目共用
      checkProjectIsolation();
    } catch (err) {
      toastError(`${t("建立独立空间失败")}:${err}`);
    }
  });
  box.append(text, act);
}

function renderProjects(prefs) {
  const previousProject = currentProject;
  currentProject = prefs.current;
  syncWorkPriorityControl();
  // R-115:按项目记的偏好(模型/思考强度/筛选)要跟着项目切换回填,
  // 也覆盖了启动这一次——currentProject 在这里才第一次确定。
  restoreProjectPrefs();
  checkProjectIsolation();
  if (previousProject !== currentProject) {
    activeProcessId = null;
    activeSessionId = null;
  }
  const list = $("project-list");
  list.innerHTML = "";
  for (const path of prefs.projects) {
    const item = document.createElement("div");
    item.className = `project-item${path === prefs.current ? " active" : ""}`;
    item.setAttribute("role", "button");
    item.tabIndex = 0;
    item.setAttribute("aria-label", `${t("选择项目")} ${prefs.names?.[path] || baseName(path)}`);
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = prefs.names?.[path] || baseName(path);
    const pathEl = document.createElement("span");
    pathEl.className = "path";
    pathEl.textContent = path;
    const remove = document.createElement("button");
    remove.className = "icon-btn remove";
    remove.textContent = "×";
    remove.title = t("移除(不删除文件)");
    remove.setAttribute("aria-label", `${t("移除项目")} ${name.textContent}`);
    remove.addEventListener("click", async (e) => {
      e.stopPropagation();
      if (!window.confirm(`${t("移除项目")}“${name.textContent}”吗？${t("只解除登记,不会删除磁盘文件。")}`)) return;
      try {
        const wasCurrent = currentProject === path;
        const next = await invoke("projects_remove", { path });
        renderProjects(next);
        if (wasCurrent && currentProject !== path) {
          clearChat();
          bgClear();
          renderTodoPanel([], 0, 0);
          await loadConversation();
          await refreshDocs();
          await loadModels();
          refreshGit();
          await refreshPendingInputs();
        }
      } catch (err) {
        toastError(String(err));
      }
    });
    const rename = document.createElement("button");
    rename.className = "icon-btn rename";
    rename.textContent = "✎";
    rename.title = t("重命名项目(只修改显示名)");
    rename.setAttribute("aria-label", `${t("重命名项目")} ${name.textContent}`);
    rename.addEventListener("click", async (e) => {
      e.stopPropagation();
      const nextName = window.prompt(t("项目显示名"), prefs.names?.[path] || baseName(path));
      if (nextName === null || !nextName.trim()) return;
      try {
        renderProjects(await invoke("projects_rename", { path, name: nextName.trim() }));
      } catch (err) {
        toastError(String(err));
      }
    });
    item.append(name, pathEl, rename, remove);
    item.addEventListener("keydown", (e) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      item.click();
    });
    item.addEventListener("click", async () => {
      const previous = currentProject;
      renderProjects(await invoke("projects_select", { path }));
      if (previous && previous !== path) {
        // 运行状态属于会话:切项目后必须按目标项目重算,否则旧项目的 kz:done 被会话过滤器
        // 丢弃,新项目会永久卡在"运行中"(发送键禁用)。refreshProcesses 会带回真实状态。
        setRunning(false, "空闲");
        clearChat();
        bgClear();
        renderTodoPanel([], 0, 0);
        await loadConversation();
      }
      refreshDocs();
      loadModels();
      refreshGit();
      refreshPendingInputs();
    });
    list.appendChild(item);
  }
  $("project-label").textContent = prefs.current ?? `(${localizeDynamic("未选择项目")})`;
  syncDocumentsProjectSelect(prefs);
  refreshProcesses();
}

$("project-init").addEventListener("click", async () => {
  const path = window.prompt(t("新项目目录路径(不存在时会创建)"));
  if (path === null || !path.trim()) return;
  const name = window.prompt(t("项目显示名(可留空)"), baseName(path.trim()));
  if (name === null) return;
  try {
    const prefs = await invoke("projects_init", {
      path: path.trim(),
      name: name.trim() || null,
    });
    renderProjects(prefs);
    clearChat(t("已初始化并切换到新项目"));
    await loadConversation();
    await refreshDocs();
    await loadModels();
    refreshGit();
    await refreshPendingInputs();
    toast(t("项目初始化完成"));
  } catch (err) {
    toastError(String(err));
  }
});

$("project-add").addEventListener("click", async () => {
  try {
    const prefs = await invoke("projects_pick");
    if (prefs) {
      const previous = currentProject;
      renderProjects(prefs);
      if (previous !== currentProject) {
        clearChat();
        bgClear();
        renderTodoPanel([], 0, 0);
        await loadConversation();
        await refreshDocs();
        await loadModels();
        refreshGit();
        await refreshPendingInputs();
      } else {
        await refreshDocs();
      }
    }
  } catch (err) {
    toastError(String(err));
  }
});

// ---------- 队列输入 ----------
function renderPendingInputs(items) {
  const list = $("queue-list");
  const count = $("queue-count");
  // 排队条挂在 composer(用户定调:排队输入放到排队按钮那里),空队列整条隐藏。
  $("composer-queue")?.classList.toggle("hidden", !items.length);
  list.innerHTML = "";
  count.textContent = items.length ? `(${items.length})` : "";
  if (!items.length) {
    const empty = document.createElement("div");
    empty.className = "queue-empty";
    empty.textContent = t("暂无排队输入");
    list.appendChild(empty);
    return;
  }
  for (const item of items) {
    const entry = document.createElement("div");
    entry.className = "queue-entry";
    entry.title = item.prompt;
    const prompt = document.createElement("div");
    prompt.className = "queue-prompt";
    prompt.textContent = item.prompt;
    const delivery = document.createElement("span");
    delivery.className = "queue-delivery";
    delivery.textContent = item.delivery === "steer" ? "steer" : "queue";
    const cancel = document.createElement("button");
    cancel.className = "queue-cancel";
    cancel.textContent = t("撤销");
    cancel.title = t("撤销这条排队输入");
    cancel.addEventListener("click", async () => {
      cancel.disabled = true;
      try {
        const changed = await invoke("cancel_input", {
          projectDir: currentProject,
          inputId: item.input_id,
          processId: activeProcessId,
        });
        if (changed) {
          toast(t("已撤销排队输入"));
          await refreshPendingInputs();
        }
      } catch (err) {
        cancel.disabled = false;
        toastError(`撤销失败:${err}`);
      }
    });
    entry.append(prompt, delivery, cancel);
    list.appendChild(entry);
  }
}

async function refreshPendingInputs() {
  if (!currentProject) {
    renderPendingInputs([]);
    return;
  }
  try {
    renderPendingInputs(await invoke("list_pending_inputs", {
      projectDir: currentProject,
      processId: activeProcessId,
    }));
  } catch (err) {
    log(`队列刷新失败:${err}`, "warn");
  }
}

function renderTestRuns(snapshot) {
  const list = $("test-list");
  const records = [...(snapshot?.active ?? []), ...(snapshot?.archived ?? [])];
  list.replaceChildren();
  $("test-count").textContent = `${records.length}`;
  if (!records.length) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("暂无测试记录");
    list.appendChild(empty);
    return;
  }
  for (const record of records.slice().reverse()) {
    const row = document.createElement("div");
    row.className = `test-entry test-${record.status}`;
    row.textContent = `${record.status === "passed" ? "✓" : record.status === "failed" ? "×" : record.status === "running" ? "●" : "○"} ${record.id} ${record.title}`;
    row.title = (record.fields ?? []).map((field) => `${field.key}: ${field.value}`).join("\n");
    // R-130:测试→条目映射可见——关联的 R-/D- 条目号渲染成可点跳转的徽标,
    // 让「这条测试为哪个条目背书」一眼可见,点一下直接跳到该条目。
    const refs = record.refs ?? [];
    if (refs.length) {
      const refRow = document.createElement("div");
      refRow.className = "test-entry-refs";
      for (const refId of refs) {
        const chip = document.createElement("button");
        chip.type = "button";
        chip.className = "test-ref-chip";
        chip.textContent = refId;
        chip.title = `${t("跳转到")} ${refId}`;
        chip.addEventListener("click", () => jumpToEntry(refId));
        refRow.appendChild(chip);
      }
      row.appendChild(refRow);
    }
    list.appendChild(row);
  }
}

async function refreshTests() {
  if (!currentProject) {
    renderTestRuns({ active: [], archived: [] });
    return;
  }
  try {
    // R-130 验收③:批量导入/初始化有真实消费者——每次刷新前把旧记录里标题含
    // R-/D- 条目号的补写「关联」字段(幂等:已结构化的不动,无变化不写盘),
    // 再取快照渲染。旧记录因此也能在列表里带出可跳转的关联徽标。
    await invoke("test_runs_init_refs", { projectDir: currentProject });
    renderTestRuns(await invoke("test_runs_snapshot", { projectDir: currentProject }));
  } catch (error) {
    log(`测试记录刷新失败:${error}`, "warn");
  }
}

$("tests-refresh").addEventListener("click", refreshTests);
