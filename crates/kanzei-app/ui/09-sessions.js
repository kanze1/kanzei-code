import { renderWorkbenchNavigation, openProjectSpace, setBrowsingProject } from "./12-workbench.js";
import { isGeneralChat, rememberConversationMode } from "./03-general-scope.js";
import { dropClosedSessions, processName, renderSidebarSessions, syncSessionActivity } from "./12-session-tree.js";
import { prepareDocsProject } from "./12-docs-pages.js";
import { closeSurface, isSurfaceOpen, openDialog, openMenu } from "./00-surface.js";
import { defer } from "./01-core.js";
import { motionSync, promptBox } from "./01-core.js";
import { setProcessItems } from "./03-shell.js";
import { setActiveProcessId, setActiveSessionId } from "./03-shell.js";
import { $, confirmDialog, inputDialog, invoke } from "./01-core.js";
import { localizeDynamic, localizedStage, t } from "./02-i18n.js";
import { layoutPref, setLayoutPref } from "./03-layout.js";
import { fillTemplate } from "./04-structured-parse.js";
import {
  activeProcessId,
  activeSessionId,
  applySessionMeta,
  currentProject,
  ensureChatView,
  navigate_view,
  log,
  processItems,
  running,
  setCurrentProject,
  sessionState,
  sessionStates,
  setRunPending,
  setRunning,
  setStopping,
  toast,
  toastError,
  transitionSession,
} from "./03-shell.js";
import { addMessage } from "./05-chat-render.js";
import { onSubagentChange, SA_ACTIVE, subagentRunsFor, subagentStateWord } from "./05-subagents.js";
import { bgClear } from "./06-activity.js";
import { agentPanelSync } from "./06-agent-panel.js";
import { previewLineSync } from "./24-preview.js";
import { askActive, askQueueFor, askQueues, hideAsk, pumpAsk } from "./07-events.js";
import {
  awaitingUserSessions,
  cancelAutoContinueTimer,
  clearAutoNotices,
  renderAutoStatus,
  restoreGoalState,
  syncAutoRunState,
  syncWorkPriorityControl,
} from "./08-auto.js";
import {
  applyAutoUiState,
  applyProfileValue,
  persistProcessAutoState,
  persistProcessProfiles,
  processAutoState,
  processProfileUi,
  queueProcessUpdate,
  rememberAutoUiState,
  updateLocalProcessItem,
} from "./08-compose-runtime.js";
import { state } from "./08-compose.js";
import { addMenuCheckColumn } from "./08-model-picker.js";
import { loadModels, modelCatalogProject, restoreProjectPrefs, syncModelSelectToActiveLine } from "./08-models.js";
import { lineAuthorityLabel, refreshWorkspace } from "./12-docs-pages.js";
import { refreshDocs } from "./14-docs-actions.js";
import {
  loadConversation,
  refreshConversationLists,
  refreshGit,
  renderLineConversationHistory,
} from "./15-views-misc.js";
import { forProject, refreshLines } from "./20-lines.js";
import { active_space, adopt_process_workspace, create_workspace_process, preferred_workspace_process, project_workspace, workspace_processes } from "./03-workspaces.js";

import { sync_composer_scope } from "./03-workspaces.js";
import { workspace_switch_pending } from "./03-workspaces.js";
import { reset_research_project } from "./19-research.js";
import { remember_development_project, switch_workspace } from "./03-workspaces.js";
import { reset_files_scope } from "./17-files.js";
// UI-0926 #10:测试记录行展开后的结构化字段。

export let worktreeItems = [];
export let worktreeLineCreateInFlight = false;
export let worktreeLineCreateSequence = 0;
// 工作树清单只服务并行线路页(renderLinesWorktrees):侧栏曾有一块「隔离工作树」只读分区,开发档里永远 display:none
// 却每次刷新照样重建,已删(UX-069 / B5)。这里只留清单状态(worktreeItems)与线路页的重绘。
export function renderWorktrees(items) {
  worktreeItems = items ?? [];
  if (typeof renderLinesWorktrees === "function") renderLinesWorktrees();
}
// R-177 内容③:清单真源是 `git worktree list --porcelain`(后端 worktree_list),
// 前端不再持有任何清单状态。原先存在 localStorage["kz-worktrees:*"] 里,于是三件事
// 都做不到:手工 `git worktree add` 出来的树看不见、换机器/清缓存后清单归零、而树
// 还在磁盘上。一次 IPC 拿全,也不用再逐条 worktree_diff。
//
// D-251 的性质**照旧守住**:projectDir 在 await **之前**认领,回来后再认一次,
// 替旧项目拉回来的清单不画进新项目的面板。
export async function refreshWorktrees() {
  if (!currentProject) return renderWorktrees([]);
  const forProject = currentProject;
  let live = [];
  try { live = await invoke("worktree_list", { projectDir: forProject }); }
  catch (error) { log(`${t("工作树清单读取失败")}:${error}`, "warn"); }
  if (currentProject !== forProject) return;
  renderWorktrees(live);
}
// D-251:合并/放弃是一次真实 IPC,用户可能在它落地前切走。projectDir 必须在 await
// **之前**认领——按 currentProject 取就会拿新项目当参数去操作旧项目的工作树。
// 清单本身不再需要维护(真源在 git),末尾重刷一次即可。
export async function handleWorktreeAction(item, action) {
  const forProject = currentProject;
  const active = processItems.find((process) => process.id === activeProcessId);
  const discardingActiveLine = action === "discard" && active?.worktree_path === item.path;
  try {
    if (action === "diff") {
      if (item.clean) {
        toast(t("工作树干净,没有未提交差异"));
      } else {
        const file_list = item.files.join("\n");
        const diff = item.diff?.trim() || t("未跟踪文件尚未包含在 git diff 中");
        log(`${item.branch}\n${t("文件列表")}:\n${file_list}\n\n${t("实际差异")}:\n${diff}`, "info");
        toast(t("工作树差异已写入运行日志"), { action: { label: t("查看日志"), onClick: () => { if ($("log-panel").classList.contains("hidden")) $("log-toggle").click(); } } });
      }
      return;
    }
    if (action === "harvest") {
      if (!item.bound_process) {
        toastError(t("该工作树没有绑定独立任务，不能合并收尾"));
        return;
      }
      if (item.bound_process !== activeProcessId) await switchProcess(item.bound_process);
      document.querySelector('.activity-item[data-view="lines"]')?.click();
      await refreshLines();
      [...document.querySelectorAll("#lines-list .line-lane")]
        .find((lane) => lane.dataset.processId === item.bound_process)
        ?.querySelector(".line-harvest-toggle")?.click();
      return;
    }
    if (action === "discard" && !(await confirmDialog({ title: t("放弃工作树"), message: `${item.branch}？${t("未提交改动会阻止删除并保留现场")}` }))) return;
    const result = await invoke("worktree_discard", { projectDir: forProject, worktreePath: item.path });
    if (String(result).length > 160) {
      log(String(result), "info");
      toast(t("工作树操作完成，详细结果已写入运行日志"), { action: { label: t("查看日志"), onClick: () => { if ($("log-panel").classList.contains("hidden")) $("log-toggle").click(); } } });
    } else {
      toast(result);
    }
    if (action === "discard") {
      // 放弃现在会在后端原子注销绑定进程。三份 UI 投影必须一起刷新；只刷新
      // git 工作树清单会把一个已不存在 cwd 的旧线路页签留在前端。
      await Promise.all([refreshProcesses(), refreshWorktrees(), refreshLines(), refreshDocs()]);
      if (currentProject !== forProject) return;
      if (discardingActiveLine && !processItems.some((process) => process.id === activeProcessId)) {
        const fallback = preferred_workspace_process(processItems);
        if (fallback) await switchProcess(fallback.id);
      }
    } else {
      await refreshWorktrees();
    }
    refreshGit();
  } catch (error) {
    toastError(String(error), { retry: () => handleWorktreeAction(item, action) });
  }
}
// 并行线路页的工作树清单:侧栏只读之后,差异/收活/放弃都落在这里。
// 已绑定线路的行给「收活」(跳到对应 lane 展开收活六格),孤儿树给「差异 / 放弃」——
// 后者是它今天唯一的出口,侧栏按钮撤掉后必须在这里补上,否则只能回 git 命令行收拾。
export function renderLinesWorktrees() {
  const list = $("lines-worktree-list");
  if (!list) return;
  list.replaceChildren();
  if (!worktreeItems.length) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("暂无独立任务的工作树");
    list.appendChild(empty);
    return;
  }
  for (const item of worktreeItems) {
    const row = document.createElement("div");
    row.className = "lines-worktree-row";
    const main = document.createElement("div");
    main.className = "lines-worktree-main";
    const branch = document.createElement("div");
    branch.className = "lines-worktree-branch";
    branch.textContent = item.branch;
    const meta = document.createElement("div");
    meta.className = "lines-worktree-meta";
    const bound = processItems.find((process) => process.id === item.bound_process);
    const boundText = bound ? processName(bound) : t("未绑定独立任务");
    meta.textContent = `${boundText} · ${item.clean ? t("干净") : `${item.files.length} ${t("项改动")}`}`;
    meta.title = item.path;
    main.append(branch, meta);
    const actions = document.createElement("div");
    actions.className = "lines-worktree-actions";
    const running = bound && processRunning(bound);
    const add = (text, cls, handler, disabled, why) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = `ghost mini ${cls}`;
      button.textContent = text;
      if (disabled) {
        button.disabled = true;
        button.title = why;
      } else {
        button.addEventListener("click", handler);
      }
      actions.appendChild(button);
    };
    add(t("差异"), "lines-worktree-diff", () => handleWorktreeAction(item, "diff"), false, "");
    if (item.bound_process) {
      add(t("合并收尾"), "worktree-harvest", () => handleWorktreeAction(item, "harvest"),
        running, t("独立任务运行中，请先停止并等它收尾，才能操作工作树"));
    }
    add(t("放弃"), "worktree-discard", () => handleWorktreeAction(item, "discard"),
      running, t("独立任务运行中，请先停止并等它收尾，才能操作工作树"));
    row.append(main, actions);
    list.appendChild(row);
  }
}
export async function createWorktreeLine(event) {
  if (!currentProject || worktreeLineCreateInFlight) return;
  const forProject = currentProject;
  // B26:线路页按钮与输入区「更多」里的同名项是同一个入口;条目绑定是可选的——只有从线路页发起时才读那里的选择器,
  // 不选就是不绑定(原来线路页必须选条目才能点、「更多」里又选不了条目,是两套互斥的流程)。
  const fromLinesView = (event?.currentTarget?.id || event?.target?.id) === "lines-add";
  const workItemId = fromLinesView ? String($("lines-work-item")?.value ?? "").trim() : "";
  // R-179 内容④:建线成本提示(D6 定案)——每树独立 target/ = 磁盘占用 ×N,
  // 首次冷编译需数分钟。让用户在建线前知道代价,不是悄悄发生。
  const binding = workItemId ? `\n${t("绑定条目")}:${workItemId}` : "";
  const addButtons = [$("worktree-add"), $("lines-add")].filter(Boolean);
  const workItemSelect = $("lines-work-item");
  // 真实 DOM 取内层 i18n span；运行时测试桩没有复建 id 节点的子树，回退到按钮本身。
  const linesAddLabel = $("lines-add")?.querySelector("[data-i18n-key]") || $("lines-add");
  const restore = () => {
    worktreeLineCreateInFlight = false;
    for (const button of addButtons) {
      button.disabled = false;
      button.removeAttribute("aria-busy");
    }
    if (workItemSelect) workItemSelect.disabled = workItemSelect.options.length <= 1;
    if (linesAddLabel) linesAddLabel.textContent = t("新建独立任务");
  };
  // facts 与确认都在同一次准入内;项目身份在第一个 await 前认领。
  // D-418:in-flight + 禁用/aria-busy/创建中反馈提前到异步读取
  // 前,期间防重入防误操作(await 期间重复点击不会二次 process_create);
  // 取消/失败统一走 restore。
  worktreeLineCreateInFlight = true;
  for (const button of addButtons) {
    button.disabled = true;
    button.setAttribute("aria-busy", "true");
  }
  if (workItemSelect) workItemSelect.disabled = true;
  if (linesAddLabel) linesAddLabel.textContent = t("创建中…");
  try {
    // UI2-0926 #13:先查原项目的仓库事实,不把迟到响应用于另一个项目。
    let facts = projectFactsFor === forProject ? projectFacts : await refreshProjectFacts(forProject);
    if (currentProject !== forProject) return;
    if (worktreeBlockedReason(facts)) facts = (await refreshProjectFacts(forProject)) ?? facts;
    if (currentProject !== forProject) return;
    const blocked = worktreeBlockedReason(facts);
    if (blocked) {
      const fix = gitFixActions(facts)[0];
      toast(blocked, { kind: "warn", ...(fix ? { action: { label: fix.label, onClick: () => {
        if (currentProject === forProject) void fix.run();
      } } } : {}) });
      return;
    }
    // target/ 与冷编译是 Rust 工程的代价,只在识别到 rust 栈时才这么说。
    const cost = facts?.stacks?.includes("rust")
      ? t("每个独立任务有自己的 target/ 目录,磁盘占用随任务数成倍增加;首次冷编译需数分钟")
      : t("每个独立任务是一份完整的工作树检出,磁盘占用随任务数增加");
    if (!(await confirmDialog({ title: t("新建独立任务"), message: `${t("新建独立任务会创建一个独立工作树")}:${cost}。${binding}\n${t("继续创建吗")}` }))) return;
    if (currentProject !== forProject) return;
    const name = `line-${Date.now()}-${worktreeLineCreateSequence += 1}`;
    // 建线必须原子完成「建 worktree + 注册进程绑定」；只调用 worktree_create 会留下
    // 一棵没有会话身份的孤树，看得见却不能并行跑任务。
    const item = await invoke("process_create", {
      projectDir: forProject,
      worktreeName: name,
      phasePipeline: false,
      trackerWrites: false,
      ...(workItemId ? { workItemId } : {}),
    });
    if (currentProject !== forProject) return;
    await Promise.all([refreshProcesses(), refreshWorktrees(), refreshLines(), refreshDocs()]);
    if (currentProject !== forProject) return;
    await switchProcess(item.id);
    if (currentProject !== forProject) return;
    // 新开的独立任务已是当前对话:从线路页等非对话页发起时跳回对话,别只在后台切了线(UX-052 / D15)。
    ensureChatView();
    // 提示里写任务的显示名(用户命名 ‖ 首条消息 ‖「独立任务 N」),不露自动生成的分支标识 line-<时间戳>-N。
    const created = processItems.find((candidate) => candidate.id === item.id) ?? item;
    toast(`${t("独立任务已创建")}:${processName(created)}${workItemId ? ` · ${workItemId}` : ""}`);
  } catch (error) {
    toastError(`${t("创建独立任务失败")}:${error}`);
  } finally {
    restore();
  }
}
defer(() => {
  $("worktree-add").addEventListener("click", createWorktreeLine);
});

// ---------- R-030:项目内独立进程 ----------
export let syncedRunningProcessId = null;
export let syncedRunningState = null;
// 已向后端补拉过待答队列的会话,防止每次进程列表刷新都打一次 pending_asks_get。
export let askSyncedSession = null;
// D-355:process_list 单飞去项目化。旧的全局 inFlight/queued 不携带请求所属项目——
// 项目 A 的 process_list 在途时切到 B,B 的 refreshProcesses 命中 A 的 inFlight 后返回
// A 的 Promise,loadConversation 误等它,等到的却是「A 的列表完成」而 B 的 activeProcessId
// 仍是 null,于是 B 的 conversation_get 永远不发出,切仓库后目标对话不恢复。现在按项目
// 键控:同项目去重(合并并发调用),跨项目各自独立请求,返回的 Promise 恒为「本项目
// 列表刷新完成」——等待方(loadConversation)等到的就是 B 自己的列表。
export const processRefreshInFlight = new Map();
export let processSwitchGeneration = 0;
export function processRunning(item) {
  const state = sessionState(item.session_id);
  // 终态事件已经收敛时,旧轮询里的 running=true 不能把线路重新点亮；
  // 未收敛时则合并事件缓存与后端快照,覆盖事件丢失/乱序的窗口。
  return state.converged ? state.running : state.running || Boolean(item.running);
}
export async function closeParallelProcess(processId) {
  const item = processItems.find((candidate) => candidate.id === processId);
  if (!item) return;
  const forProject = currentProject;
  const runningNow = processRunning(item);
  // 关闭只注销身份(processes → retired_processes),这条线的对话一条不删,在搜索与历史中只读查看。
  // 要连对话一起删,用右键「删除对话…」(真删);弹窗必须把这两件事分清。确认框只点名,不露内部 id(UX-035)。
  const name = processName(item);
  const warning = `${runningNow
    ? t("独立任务仍在运行，关闭会先停止它，等它收尾。")
    : t("关闭只会注销这个独立任务的登记。已合并且干净的工作树会自动回收；有独有内容的工作树会保留。")}\n${t("关闭后，可在搜索对话或历史中查看这段对话；要连对话一起删除，请用「删除对话…」。")}`;
  if (!(await confirmDialog({ title: t("关闭独立任务"), message: `「${name}」\n${warning}`, okText: t("关闭独立任务"), danger: true }))) return;
  cancelAutoContinueTimer(item.session_id);
  const closingPhase = processRunning(item) ? sessionState(item.session_id).phase : null;
  if (closingPhase) transitionSession(item.session_id, "stopping");
  try {
    const result = await invoke("process_close", { processId });
    if (currentProject !== forProject) return;
    const closingActive = activeProcessId === processId;
    if (closingActive) {
      setActiveProcessId(null);
      setActiveSessionId(null);
    }
    await Promise.all([refreshProcesses(), refreshWorktrees(), refreshLines(), refreshDocs()]);
    if (currentProject !== forProject) return;
    // 关掉的线路只是注销了身份,对话记录还在:在搜索与历史中查看。
    dropClosedSessions(forProject);
    if (closingActive && activeProcessId) await switchProcess(activeProcessId, true);
    refreshGit();
    // 后端回执只说处置结果(「已关闭」「已关闭,并回收…」),不带内部 id(UX-127):前面补上对话名。
    toast(`「${name}」${result ? String(result) : t("已关闭")}`);
  } catch (error) {
    if (closingPhase && sessionState(item.session_id).phase === "stopping") transitionSession(item.session_id, closingPhase);
    toastError(`${t("关闭独立任务失败")}:${error}`);
  }
}
/// 任务卡的一行状态:字形 + 单个状态词。整表重绘(renderParallelTaskStatus)与逐事件投影
/// (refreshParallelTaskProjection)共用这一份,两处不再各拼一遍文案(原来空闲时拼成
/// 「○ 空闲 · 空闲」,且两处写法随时会漂)。运行中的状态词就是当前阶段(取不到才写「运行中」);
/// 阶段细节(正在跑的命令等)只进这一行的 tooltip——侧栏一行放不下,下面的实时行也已经在说它。
export function parallelTaskView(item) {
  const state = sessionState(item.session_id);
  const runningNow = processRunning(item);
  const pendingNow = state.phase === "auto_pending" || (state.auto_pending === true && !runningNow);
  const stoppingNow = state.phase === "stopping";
  const stage = [state.stage, item.stage].find((value) => value && value !== "空闲") || "";
  // UI2-0926 #13 复核:模型在等你回答(引擎 Stop/AwaitingUser)。后台线停在这里时侧栏行原先只写「空闲」,
  // 一条在等人的线混在空闲线里很容易漏看——单独一个状态词 + 琥珀字形(attention:等你批准/回答)。
  const waitingNow = !runningNow && !pendingNow && !stoppingNow && awaitingUserSessions.has(item.session_id);
  // UX-146:这条线有权限请求在等你批准(排在队里没弹出来,或就是当前卡)。后端此刻仍算它「运行中」,
  // 线路行原先照写运行阶段——一条卡在批准上的线看起来和正常跑着的一样。批准等待优先于运行态显示。
  const askWaiting = (askQueues.get(item.session_id)?.length ?? 0) > 0 || askActive?.sessionId === item.session_id;
  const label = askWaiting ? t("等你批准")
    : stoppingNow ? t("停止中…")
    : runningNow ? localizedStage(stage) || t("运行中")
      : pendingNow ? t("鞭挞等待")
        : waitingNow ? t("在等你回答")
          : t("空闲");
  const name = `${lineAuthorityLabel(item)} · ${processName(item)}${item.branch ? ` · ${item.branch}` : ""}`;
  const title = askWaiting
    ? `${name}\n${t("有权限请求在等你批准,切到这个对话处理")}`
    : runningNow
    ? `${name}\n${[localizedStage(stage) || t("运行中"), state.detail].filter(Boolean).join(" · ")}`
    : pendingNow
      ? `${name}\n${t("等待下一轮")}`
      : waitingNow
        ? `${name}\n${t("模型在等你回答(回复后自动继续)")}`
        : `${name}\n${t("点击切换到此对话")}`;
  return { runningNow: runningNow && !askWaiting, pendingNow, stoppingNow, waitingNow: waitingNow || askWaiting, label, title };
}
/// 后端轮询回报的阶段补进会话状态机(实时事件还没到时的兜底):研究空间的会话列表与侧栏运行态点都读它。
function adoptReportedStage(item) {
  if (!item.running || !item.stage) return;
  const state = sessionState(item.session_id);
  if (!state.stage || state.stage === "空闲") state.stage = item.stage;
}
/// 「任务与对话」列表(#parallel-task-status)**只服务研究空间**:开发空间的会话在侧栏项目树里(12-session-tree.js),
/// 这一块在开发档里永远 display:none、却每次刷新照样重建,已不再渲染(UX-069 / B5)。
export function renderParallelTaskStatus(items) {
  const target = $("parallel-task-status");
  const count = $("parallel-task-count");
  if (!target || !count) return;
  if (active_space !== "research") {
    if (target.firstChild || target._renderSignature) { target.replaceChildren(); target._renderSignature = ""; }
    count.textContent = "";
    return;
  }
  const processes = workspace_processes(items);
  count.textContent = processes.length ? `${processes.length} ${t("条任务")}` : "";
  // UX-036:内容没变就不重建——3 秒一次的整块重建会丢掉行内的滚动位置、键盘焦点与展开中的历史列表。
  // 签名只含重建会改变的输入(身份、名字、状态词、活动线、界面语言);逐事件的状态投影照旧原地更新。
  const signature = JSON.stringify([active_space, activeProcessId, t("历史对话"), processes.map((item) => {
    adoptReportedStage(item);
    const view = parallelTaskView(item);
    return [item.id, processName(item), item.branch ?? "", lineAuthorityLabel(item), view.label, view.runningNow, view.pendingNow, view.stoppingNow, view.waitingNow];
  })]);
  if (processes.length && target._renderSignature === signature && target.firstChild) {
    for (const item of processes) {
      if (item.id === activeProcessId && parallelTaskView(item).pendingNow) setRunPending(`${t("鞭挞")} · ${t("等待下一轮")}`);
    }
    return;
  }
  target._renderSignature = processes.length ? signature : "";
  target.replaceChildren();
  if (!processes.length) {
    if (active_space === "research") {
      const empty = document.createElement("p");
      empty.className = "dim";
      empty.textContent = t("暂无课题对话");
      target.appendChild(empty);
    }
    return;
  }
  for (const item of processes) {
    adoptReportedStage(item);
    const view = parallelTaskView(item);
    const line = document.createElement("div");
    line.className = "parallel-line";
    line.dataset.processId = item.id;
    const row = document.createElement("button");
    row.type = "button";
    row.className = `parallel-task-row${item.id === activeProcessId ? " active" : ""}${view.runningNow ? " running" : ""}${view.pendingNow ? " auto-pending" : ""}`;
    row.dataset.processId = item.id;
    row.setAttribute("role", "listitem");
    // 一行:字形 +「身份 · 名称」(分支名暗色跟在后面、可省略号截断)+ 靠右的单个状态词。
    const head = document.createElement("span");
    head.className = "parallel-task-head";
    head.append(`${lineAuthorityLabel(item)} · ${processName(item)}`);
    if (item.branch) {
      const branch = document.createElement("span");
      branch.className = "parallel-task-branch";
      branch.textContent = item.branch;
      head.append(" ", branch);
    }
    row.appendChild(head);
    renderParallelTaskState(row, view);
    row.title = view.title;
    row.addEventListener("click", () => void switchProcess(item.id));
    line.appendChild(row);
    {
      // 关闭是低频的危险动作:图标按钮,悬停/聚焦这一条线路时才出现(CSS),读屏名称带线路名。
      const close = document.createElement("button");
      close.type = "button";
      close.className = "icon-btn parallel-line-close";
      close.textContent = "×";
      close.title = t("关闭独立任务");
      close.setAttribute("aria-label", `${t("关闭独立任务")} ${processName(item)}`);
      close.addEventListener("click", (event) => {
        event.stopPropagation();
        void closeParallelProcess(item.id);
      });
      line.appendChild(close);
    }
    const history = document.createElement("div");
    history.className = "parallel-line-history";
    history.dataset.processId = item.id;
    line.appendChild(history);
    target.appendChild(line);
    if (typeof renderLineConversationHistory === "function") renderLineConversationHistory(item.id);
    if (item.id === activeProcessId && view.pendingNow) {
      setRunPending(`${t("鞭挞")} · ${t("等待下一轮")}`);
    }
  }
}
/// 逐事件投影:这条会话的运行态一变就同步给输入区的运行栏(活动线)与研究空间会话列表里的那一行。
/// 运行栏的投影不依赖列表里有没有这一行——开发空间没有那块列表(UX-069),不能因为找不到行就整个跳过。
export function refreshParallelTaskProjection(sessionId) {
  if (!sessionId) return;
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  if (!item) return;
  const view = parallelTaskView(item);
  const { runningNow, pendingNow, stoppingNow } = view;
  const row = [...document.querySelectorAll(".parallel-task-row")]
    .find((candidate) => candidate.dataset.processId === item.id);
  if (row) {
    row.classList.toggle("running", runningNow);
    row.classList.toggle("auto-pending", pendingNow);
    renderParallelTaskState(row, view);
    row.title = view.title;
  }
  // 开发空间的会话行在侧栏树里:等你回答 / 鞭挞等待 / 停止中这类相位变化要立刻落到那一行的运行态点上(不等 3 秒轮询)。
  syncSessionActivity();
  if (item.id === activeProcessId && pendingNow) {
    setRunPending(`${t("鞭挞")} · ${t("等待下一轮")}`);
  } else if (item.id === activeProcessId && stoppingNow) {
    setStopping(t("停止中…"));
  } else if (item.id === activeProcessId && running !== runningNow) {
    syncedRunningProcessId = item.id;
    syncedRunningState = runningNow;
    setRunning(runningNow, runningNow ? t("运行中") : t("空闲"));
  }
}
/// #7:线路行状态 = 行首独立的字形节点(.kz-glyph,只呼吸)+ 行尾的状态词(.parallel-task-state)。
/// 字形文本 ●/◐/○ 原样保留(读屏读状态词,字形 aria-hidden;既有断言读字形)。两个节点
/// **原地更新**:逐事件投影不再重建它们,呼吸动画不会每个 kz:status 都从第 0 帧重来;
/// 整表重绘新建的节点由 motionSync 对齐到全局相位。
export function renderParallelTaskState(row, { runningNow, pendingNow, stoppingNow, waitingNow = false, label }) {
  const state = stoppingNow ? "stopping" : runningNow ? "running" : pendingNow ? "pending" : waitingNow ? "attention" : "idle";
  const mark = runningNow ? "●" : pendingNow ? "◐" : "○";
  const words = String(label ?? "");
  let glyph = row.querySelector(".kz-glyph");
  let text = row.querySelector(".parallel-task-state");
  if (!glyph || !text) {
    glyph = document.createElement("span");
    glyph.className = "kz-glyph parallel-task-glyph";
    glyph.setAttribute("aria-hidden", "true");
    text = document.createElement("span");
    text.className = "parallel-task-state";
    row.prepend(glyph);
    row.appendChild(text);
  }
  if (glyph.dataset.state !== state) {
    glyph.dataset.state = state;
    motionSync(glyph);
  }
  if (glyph.textContent !== mark) glyph.textContent = mark;
  if (text.textContent !== words) text.textContent = words;
}
export function renderProcesses(items) {
  const previousItems = processItems;
  const previousProcessKey = processItems.map((item) => item.id).join("\u0000");
  const previousProcessId = activeProcessId;
  setProcessItems(items ?? []);
  const liveIds = new Set(processItems.map((item) => item.id));
  const retiredAutoState = {};
  // 只清当前项目中确认已注销的线路；切项目时旧项目配置继续保留。身份虽已由后端
  // 保证永不复用，这里仍回收 timer/session/profile，避免长期运行积累死缓存。
  for (const removed of previousItems.filter((item) =>
    item.origin_project === currentProject && !liveIds.has(item.id))) {
    cancelAutoContinueTimer(removed.session_id);
    sessionStates.delete(removed.session_id);
    processAutoState.delete(removed.id);
    retiredAutoState[removed.id] = null;
    processProfileUi.delete(removed.id);
  }
  persistProcessAutoState(retiredAutoState);
  persistProcessProfiles();
  const nextProcessKey = processItems.map((item) => item.id).join("\u0000");
  const previousSessionId = activeSessionId;
  // R-086:后端是运行态权威,先把返回的 running 校正进各会话状态机(事件可能
  // 丢失),视图只投影活动会话的状态机,而不是直接信某一次轮询的瞬时值。
  // 已收敛终态(converged)的会话不被旧轮询值翻回——事件在轮询采样之后才发
  // 出是正常时序,此刻 process_list 里仍是 running=true,但会话实际已结束。
  for (const item of processItems) {
    const state = sessionState(item.session_id);
    if (state.converged) continue;
    // 实时事件是运行态的高优先级来源。一次在飞的 process_list 请求可能在
    // 后端刚启动 session 前采样到 false，不能覆盖 kz:turn/status/tool 形成的
    // live_running=true；同理，用户刚点击发送时的本地启动意图也要跨过这个窗口。
    // R-206:全部经 transitionSession 折算 phase→running 兼容字段,不再手工直写。
    if (state.live_running === true || (state.local_start_pending && !item.running)) {
      // A snapshot sampled before launch is not evidence that startup finished.
      if (state.phase !== "stopping" && !(state.phase === "starting" && state.local_start_pending && !item.running)) transitionSession(item.session_id, "running");
    } else if (state.live_running === false) {
      if (["starting", "running"].includes(state.phase)) transitionSession(item.session_id, "idle");
    } else if (item.running) {
      transitionSession(item.session_id, "running", { local_start_pending: false });
    } else if (!["auto_pending", "stopping", "stopped", "failed"].includes(state.phase)) {
      transitionSession(item.session_id, "idle");
    }
  }
  if (!activeProcessId || !workspace_processes(processItems).some((item) => item.id === activeProcessId)) {
    const preferred = preferred_workspace_process(processItems);
    setActiveProcessId(preferred?.id ?? null);
  }
  const active = processItems.find((item) => item.id === activeProcessId);
  setActiveSessionId(active?.session_id ?? null);
  if (activeSessionId && activeSessionId !== previousSessionId) void restoreGoalState(activeSessionId);
  if (activeSessionId !== previousSessionId) resetPendingInputs();
  const activeProcessChanged = previousProcessId !== activeProcessId;
  if (activeProcessChanged && activeProcessId) {
    // 首次加载、切项目重建或活动线被回收后选 fallback 时，必须恢复目标线的
    // 完整设置；普通轮询不重复应用，避免覆盖用户刚修改的控件和鞭挞计数。
    adopt_process_workspace(active);
    applyAutoUiState(activeProcessId);
    applyProfileValue(active?.profile);
    // 模型下拉同属「该线的完整设置」:冷启动与兜底选中都走这里,不能只靠 switchProcess。
    syncModelSelectToActiveLine();
    applySessionMeta(activeSessionId);
    // UI-0926 #8:子代理侧栏与 rail 徽标跟着活动线走。
    agentPanelSync();
    // UI2-0926 #8:网页预览的可见性上报带上新活动线(代理的 browser 只在「面板显示着这条线」时走面板)。
    previewLineSync();
    // 兜底改选(活动线被注销/被工作空间过滤)时视图必须跟着换:只改 activeSessionId 不换
    // pane,新活动线的实时事件会走快路径写进旧线的 pane,新对话清的也是错的那块。
    // 首次选中、切项目(previousProcessId 为空)与切工作空间期间由调用方自己装载。
    if (previousProcessId && !workspace_switch_pending) void loadConversation();
  }
  if (activeSessionId && activeSessionId !== previousSessionId) void syncAutoRunState();
  // R-086:活动会话换人(含首次拿到进程列表——界面重载后就是这条路)时向后端
  // 补拉一次待答队列。后端 asks 表活得比 webview 久,不补拉的话重载前挂起的
  // 权限询问再也不会出现,而后端还在 await 它的答复。按会话去重,只拉一次。
  if (activeSessionId && activeSessionId !== askSyncedSession) {
    askSyncedSession = activeSessionId;
    refreshPendingAsks();
  }
  pumpAsk();
  // 按「线路身份 + 线路运行态」同步运行栏。旧实现只在活动进程换人时同步，
  // 导致同一条线路从空闲变运行后 stop 仍隐藏，底部状态也一直停在空闲。
  // 状态机的本地停止复位仍然优先；下一次真实 process_list/事件投影会校正它。
  const activeRunning = active ? processRunning(active) : false;
  const activeState = active ? sessionState(active.session_id) : null;
  const activePending = active ? (activeState.phase === "auto_pending" || (activeState.auto_pending === true && !activeRunning)) : false;
  const activeStopping = activeState?.phase === "stopping";
  const activeTerminalStatus = active ? sessionState(active.session_id).terminal_status : "";
  if (
    !activePending && !activeStopping && (activeProcessId !== syncedRunningProcessId ||
    activeRunning !== syncedRunningState ||
    running !== activeRunning)
  ) {
    syncedRunningProcessId = activeProcessId;
    syncedRunningState = activeRunning;
    setRunning(activeRunning, activeRunning ? t("运行中") : activeTerminalStatus || t("空闲"));
  }
  if (activeStopping) setStopping(t("停止中…"));
  if (activePending) setRunPending(`${t("鞭挞")} · ${t("等待下一轮")}`);
  for (const item of workspace_processes(processItems)) adoptReportedStage(item);
  renderParallelTaskStatus(processItems);
  // 侧栏会话树与历史弹层随进程列表更新(内容签名不变就不重建,见 12-session-tree.js)。
  renderSidebarSessions();
  // 「勘察复核」开关已从界面删除(UX-039):后端 phase_pipeline 字段本轮保留,界面不再读写它。
  // 「改主项目需求记录」是工作树线的属性,开关在并行线路页对应线路卡上(20-lines.js)。
  renderSubagentControl();
  // 鞭挞面板要跟着重算:自主推进开着而流水线关着时那里有一行提示。
  renderAutoStatus();
  if (previousProcessKey !== nextProcessKey && typeof refreshConversationLists === "function") {
    void refreshConversationLists();
  }
}

export async function refreshProcesses() {
  if (!currentProject) return null;
  const forProject = currentProject;
  // 同项目在途请求直接复用:合并并发调用,后端不会同时吃两份同项目清单。
  const existing = processRefreshInFlight.get(forProject);
  if (existing) return existing;
  const promise = (async () => {
    try {
      const items = await invoke("process_list", { projectDir: forProject });
      if (currentProject === forProject) renderProcesses(items);
    } catch (err) {
      if (currentProject === forProject) log(`${t("对话列表刷新失败")}:${err}`, "warn");
    } finally {
      processRefreshInFlight.delete(forProject);
    }
  })();
  processRefreshInFlight.set(forProject, promise);
  return promise;
}

export async function refreshPendingAsks() {
  if (!currentProject || !activeSessionId) return;
  const forProject = currentProject;
  const forProcess = activeProcessId;
  const forSession = activeSessionId;
  try {
    const pending = await invoke("pending_asks_get", {
      projectDir: forProject,
      processId: forProcess,
    });
    const queue = askQueueFor(forSession);
    const known = new Set(queue.map((item) => item.id));
    if (askActive?.sessionId === forSession) known.add(askActive.id);
    for (const payload of pending || []) {
      if (!known.has(payload.id)) {
        queue.push(payload);
        known.add(payload.id);
      }
    }
    pumpAsk();
  } catch (err) {
    if (askSyncedSession === forSession) askSyncedSession = null;
    log(`${t("待处理权限询问恢复失败")}:${err}`, "warn");
  }
}


export async function switchProcess(processId, forceReload = false) {
  if (processId === activeProcessId && !forceReload) return;
  const target = processItems.find((item) => item.id === processId);
  if (!target) return;
  const switchGeneration = ++processSwitchGeneration;
  const forProject = currentProject;
  const isCurrentSwitch = () =>
    switchGeneration === processSwitchGeneration && currentProject === forProject && activeProcessId === processId;
  // 后端只保存 dev/research;前端的 dev-auto 档位由 profile-select 的 change 事件
  // 在**用户改动时**绑定到当时的进程,这里不再重复写一次。
  //
  // D-290:原来这里拿 `$("profile-select").value` 当「旧进程的用户意图」写盘。选择器
  // 的值在回显期间是算出来的,不是用户选的——启动竞态里它可能还停在 dev-pair,于是
  // 一次切线就把存档里的 dev-auto 覆盖成 dev-pair,下次冷启动读回 dev-pair,再顺手
  // 关掉鞭挞……用户的表现就是「每次打开都要重新设模式和鞭挞」。回显不得写盘。
  if (activeProcessId) {
    rememberAutoUiState(activeProcessId);
  }
  // R-267:切走不再需要存快照——pane 留在 DOM 里,内容原样还在。
  hideAsk(true);
  setActiveProcessId(processId);
  setActiveSessionId(target.session_id);
  void restoreGoalState(target.session_id);
  if (activeProcessId !== processId || activeSessionId !== target.session_id) return false;
  resetPendingInputs();
  adopt_process_workspace(target);
  applyAutoUiState(activeProcessId);
  applyProfileValue(target.profile);
  // UI-0926 #3:输入框上方的模型/思考芯片立刻跟到目标线(先标在途,再问 model_effective),
  // 不等下面那一串 loadConversation/refreshDocs——否则切线后好几秒还显示上一条线的临时值。
  syncModelSelectToActiveLine();
  // 状态栏模型/上下文上限回放该线最近一次 kz:meta,不再停留在上一条线的值。
  applySessionMeta(activeSessionId);
  // UI-0926 #8:子代理侧栏换成新线路的数据(详情不属于新线路时回到列表),徽标重算。
  agentPanelSync();
  previewLineSync();
  void syncAutoRunState();
  // 下面有一次显式 await refreshPendingAsks(),先认领这个会话,免得 renderProcesses
  // 里的补拉守卫又打一次 pending_asks_get(结果会被 id 去重,只是白跑一趟)。
  askSyncedSession = target.session_id;
  pumpAsk();
  // R-086:运行态投影自该会话的状态机(后台终态已收敛),不直接信 processItems
  // 的瞬时轮询值——事件先到状态机,切回时看到的才是准的。
  if (sessionState(target.session_id).phase === "stopping") setStopping(t("停止中…"));
  else if (sessionState(target.session_id).phase === "auto_pending") setRunPending(`${t("鞭挞")} · ${t("等待下一轮")}`);
  else setRunning(processRunning(target), processRunning(target) ? t("运行中") : t("空闲"));
  renderProcesses(processItems);
  document.dispatchEvent(new CustomEvent("kz:conversation-selected"));
  // 不在请求发出前清空消息:切线期间保留旧内容,等目标线程的历史完整恢复后
  // renderRecoveredMessages 一次性替换,避免慢请求/竞态把主对话显示成空白。
  bgClear();
  await loadConversation(null, switchGeneration);
  if (!isCurrentSwitch()) return;
  await refreshPendingAsks();
  if (!isCurrentSwitch()) return;
  if (!isGeneralChat()) await refreshDocs();
  if (!isCurrentSwitch()) return;
  // 模型目录按项目缓存:同项目内切线不再重复探测 models_list(每个 provider 最多 6 秒);
  // 芯片回显已在上面切线那一刻发出,这里只在目录属于别的项目时补拉一次(loadModels 会顺带重问)。
  if (modelCatalogProject !== currentProject) await loadModels();
  if (!isCurrentSwitch()) return;
  if (!isGeneralChat()) void refreshGit(activeProcessId);
  refreshPendingInputs();
  void refreshProcesses();
  log(`${t("已切换到对话")} ${processName(target)}`);
}

defer(() => {
  $("process-add").addEventListener("click", async () => {
    if (!currentProject) return;
    try {
      await create_workspace_process(active_space === "research" ? project_workspace().research.topic : null);
    } catch (err) {
      toastError(`${t("新建对话失败")}:${err}`);
    }
  });
});

// 配置是下一轮的准入开关,运行态来自子代理事件；两者不能混成一个「开/关」。
// 在途写入按线路保存,轮询回显和切线都不能改写这次点击的目标或值。
export const subagentUpdates = new Map();
export function renderSubagentControl() {
  const active = processItems.find((item) => item.id === activeProcessId);
  const toggle = $("process-subagents");
  const pending = subagentUpdates.has(activeProcessId);
  const enabled = subagentUpdates.get(activeProcessId) ?? active?.subagents_enabled ?? true;
  toggle.checked = Boolean(active) && enabled;
  toggle.disabled = !active || pending;
  $("process-subagents-wrap").setAttribute("aria-busy", String(pending));
  $("process-subagents-value").textContent = !active ? t("未选对话") : pending ? t("保存中…")
    : processRunning(active) ? t(enabled ? "下轮开启" : "下轮关闭") : t(enabled ? "启用" : "停用");
  const runs = active ? subagentRunsFor(active.session_id) : [];
  const live = runs.filter((run) => SA_ACTIVE.has(run.state));
  const waiting = live.filter((run) => run.state === "waiting").length;
  const stopping = live.filter((run) => run.state === "stopping").length;
  const executing = live.length - waiting - stopping;
  const stateLabels = [];
  if (executing) stateLabels.push(fillTemplate(t("{count} 运行"), { count: executing }));
  if (stopping) stateLabels.push(fillTemplate(t("{count} 停止中"), { count: stopping }));
  if (waiting) stateLabels.push(fillTemplate(t("{count} 等待批准"), { count: waiting }));
  const last = runs[runs.length - 1];
  const status = $("subagent-control-state");
  const nextText = stateLabels.join(" · ") || (last
    ? fillTemplate(t("最近: {state}"), { state: subagentStateWord(last.state) }) : t("未派遣"));
  if (status.textContent !== nextText) status.textContent = nextText;
  status.title = nextText;
  status.dataset.state = waiting ? "waiting" : stopping ? "stopping" : executing ? "running"
    : ["failed", "timeout", "rejected", "interrupted"].includes(last?.state) ? "failed" : "idle";
}

defer(() => {
  onSubagentChange((run) => {
    if (!run || run.sessionId === activeSessionId) renderSubagentControl();
  });
  $("process-subagents").addEventListener("change", async (event) => {
    const processId = activeProcessId;
    const project = currentProject;
    const enabled = event.target.checked;
    if (!processId || subagentUpdates.has(processId)) return;
    subagentUpdates.set(processId, enabled);
    renderSubagentControl();
    try {
      await queueProcessUpdate(processId, { subagentsEnabled: enabled });
      // 写入前发出的列表可能迟到；先等它收敛,再以成功回执更新这条线。
      await processRefreshInFlight.get(project);
      updateLocalProcessItem(processId, { subagents_enabled: enabled });
      if (activeProcessId === processId) log(enabled ? t("子代理已开启:模型可按需委派任务") : t("子代理已关闭:本对话不再派出子代理"));
    } catch (err) {
      toastError(`${t("更新对话设置失败")}:${err}`);
      // 失败后重读实际状态,兼容后端已更新内存但落盘失败的情况。
      await processRefreshInFlight.get(project);
      if (currentProject === project) await refreshProcesses();
    } finally {
      subagentUpdates.delete(processId);
      renderSubagentControl();
    }
  });
});

// ---------- 项目管理 ----------
export function baseName(path) {
  const parts = path.replaceAll("\\", "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

export function syncDocumentsProjectSelect(prefs) {
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
export let isolationReported = false;
let isolationCheckGeneration = 0;
export async function reportIsolationAcrossProjects() {
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

export async function checkProjectIsolation() {
  const box = $("project-shared-warn");
  const generation = ++isolationCheckGeneration;
  if (!box || !currentProject) return;
  const project = currentProject;
  let info;
  try {
    info = await invoke("project_root_info", { projectDir: project });
  } catch {
    return;
  }
  if (generation !== isolationCheckGeneration || currentProject !== project) return;
  // 无损修复过就只留一行日志,不打扰——用户看到的内容没有任何变化。
  if (info.autoRepaired) log(`${t("已为本项目建立独立空间")}:${info.selected}`);
  box.classList.toggle("hidden", !info.shared);
  // 事实横幅在隔离告警在场时让位:告警显隐一变就重排一次。
  renderProjectFactsBanner();
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
    if (currentProject !== project) return;
    try {
      await invoke("project_detach", { projectDir: project });
      if (currentProject !== project) return;
      toast(t("已建立独立空间"));
      // 分离改变了项目根:文档、会话、记忆都要按新根重取,否则界面还停在旧根的数据上。
      await refreshDocs();
      if (currentProject !== project) return;
      await loadConversation();
      if (currentProject !== project) return;
      isolationReported = false; // 允许再体检一次,看还有没有别的项目共用
      checkProjectIsolation();
    } catch (err) {
      toastError(`${t("建立独立空间失败")}:${err}`);
    }
  });
  box.append(text, act);
}

export function activate_execution_root(root) {
  const previousProject = currentProject;
  setCurrentProject(root);
  if (previousProject !== currentProject) {
    // 项目事实和可操作列表都是旧项目的投影,等待新结果期间不能沿用。
    projectFacts = null;
    projectFactsFor = null;
    projectFactsGeneration += 1;
    isolationCheckGeneration += 1;
    $("project-shared-warn")?.classList.add("hidden");
    renderProjectFactsBanner(null, root);
    syncWorktreeEntry(null);
    renderWorktrees([]);
    resetPendingInputs();
  }
  syncWorkPriorityControl();
  // R-115:按项目记的偏好(模型/思考强度/筛选)要跟着项目切换回填,
  // 也覆盖了启动这一次——currentProject 在这里才第一次确定。
  restoreProjectPrefs();
  if (!isGeneralChat(root)) {
    checkProjectIsolation();
    void refreshProjectFacts(root);
  }
  if (previousProject !== currentProject) {
    prepareDocsProject();
    // 旧项目的权限卡不能原样盖在新项目的对话上(UX-144):请求放回它自己会话的队首,切回那个项目时再弹。
    hideAsk(true);
    setActiveProcessId(null);
    setActiveSessionId(null);
    setProcessItems([]);
    setRunning(false, t("空闲"));
    clearAutoNotices();
    sync_composer_scope();
    reset_files_scope();
    reset_research_project();
  }
}

// ---------- UI2-0926 #1:项目只有一个切换入口 ----------
// 侧栏头部的项目卡就是项目菜单(openProjectMenu);侧栏不再另挂一份「项目」列表(原来项目卡只是那份列表的
// 开合把手,用户看到的是两个切换器)。菜单、项目总览卡片、命令面板读的都是这一份 projects_get 偏好,
// 切换都走 switchProject——一处实现,守卫(D-355 的切项目事务)只写一遍。
// 菜单只读偏好缓存,不调 workspace_snapshot:后者每个项目都要开库、跑 process_list、读轨迹,太重。
export let lastProjectPrefs = { current: null, projects: [], names: {} };
export function projectDisplayName(path, prefs = lastProjectPrefs) {
  if (isGeneralChat(path)) return t("无项目对话");
  return prefs?.names?.[path] || baseName(path);
}
export function projectMenuEntries(prefs = lastProjectPrefs) {
  return (prefs?.projects ?? []).map((path) => ({ path, name: projectDisplayName(path, prefs), current: path === prefs.current }));
}
/// 菜单里的第二行只放路径末两段(完整路径进 title):侧栏宽 280px,整条 Windows 路径只剩省略号。
export function shortProjectPath(path) {
  const parts = String(path ?? "").replaceAll("\\", "/").split("/").filter(Boolean);
  return parts.length > 2 ? `…/${parts.slice(-2).join("/")}` : parts.join("/");
}

export async function switchProject(path, options = {}) {
  return openProjectSpace(path, options.view ?? "chat", { ...options, reload: true });
}

// 项目总览页开着时,改名/移除后卡片要跟着换;不开着就不白跑一次 workspace_snapshot。
function refreshWorkspaceIfOpen() {
  if ($("view-workspace")?.classList.contains("active")) void refreshWorkspace();
}

export async function renameProject(path, prefs = lastProjectPrefs) {
  const nextName = await inputDialog({
    title: t("重命名项目"),
    message: t("只修改在 kanzei 里的显示名,不会改磁盘上的文件夹名。"),
    value: projectDisplayName(path, prefs),
  });
  if (nextName === null) return;
  if (!nextName.trim()) { toast(t("项目名称不能为空"), { kind: "warn" }); return; }
  try {
    renderProjects(await invoke("projects_rename", { path, name: nextName.trim() }), { activate: document.body.dataset.appScope !== "global" && document.body.dataset.view !== "project" });
    refreshWorkspaceIfOpen();
  } catch (err) {
    toastError(String(err));
  }
}

export async function removeProject(path, prefs = lastProjectPrefs) {
  const name = projectDisplayName(path, prefs);
  // 移除入口在项目总览卡片 ⋯ 里:移除的恰是当前项目时要换到下一个项目,但用户仍在管理项目,
  // 不能被 enterProject 带到下一个项目记住的视图(实测落在对话页),留在总览页并刷新卡片。
  const wasOnOverview = Boolean($("view-workspace")?.classList.contains("active"));
  if (!(await confirmDialog({
    title: t("移除项目"),
    message: `「${name}」\n${t("只解除登记,不会删除磁盘上的任何文件;对话记录和需求仍保存在项目目录的 .kanzei 里,重新添加该文件夹即可找回。")}`,
    okText: t("移除"),
    danger: true,
  }))) return;
  try {
    const wasCurrent = currentProject === path;
    const next = await invoke("projects_remove", { path });
    if (wasCurrent) {
      await enterProject(next, wasOnOverview ? { view: "workspace" } : {});
    } else {
      renderProjects(next);
    }
    // 项目没了:它在侧栏树里的展开态、置顶与会话排序一并丢掉。
    for (const section of ["sidebar_open", "project_pins", "session_order"]) setLayoutPref(section, path, null);
    refreshWorkspaceIfOpen();
  } catch (err) {
    // 有运行中的线路时后端拒绝并说明原因:一句话提示,不去开持久错误面板。
    toast(String(err), { kind: "err" });
  }
}

export async function addProjectFolder() {
  try {
    const prefs = await invoke("projects_pick");
    if (prefs) await enterProject(prefs);
    refreshWorkspaceIfOpen();
  } catch (err) {
    toastError(String(err));
  }
}

// 「新建项目…」(项目卡菜单、项目总览页头、命令面板)一律打开新建项目对话框。
export async function initProject() {
  return openNewProjectDialog();
}

// ---------- UI2-0926 #13:新建项目对话框 · 项目事实横幅 · 并行线入口 ----------
// 「MD文件保存」现场:✦ 只让人手敲一个路径和名字,然后建目录和 .kanzei——不初始化 Git、不写运行时文件的
// 忽略规则、事后也不提示「这里没有 Git,并行线/提交/差异条都用不了」。事实由后端 project_facts 一处探测
// (与 agent 上下文里的 <project-state> 同源),这里只负责呈现。docs/design/project_workspace.md。

/// 新建项目的默认位置:上次用过的位置 → 当前项目的上级目录。
export function defaultNewProjectParent(project = currentProject) {
  const saved = layoutPref("new_project", "parent");
  if (typeof saved === "string" && saved.trim()) return saved;
  const path = String(project || "");
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return cut > 0 ? path.slice(0, cut) : "";
}
/// 「将创建:<位置>\<名称>」:分隔符跟着位置的写法走。
export function newProjectTargetPath(parent, name) {
  const base = String(parent ?? "").trim().replace(/[\\/]+$/, "");
  const leaf = String(name ?? "").trim();
  if (!base || !leaf) return "";
  return `${base}${base.includes("/") && !base.includes("\\") ? "/" : "\\"}${leaf}`;
}
function syncNewProjectPreview() {
  const preview = $("new-project-preview");
  if (!preview) return;
  const target = newProjectTargetPath($("new-project-parent")?.value, $("new-project-name")?.value);
  preview.textContent = target ? fillTemplate(t("将创建:{path}"), { path: target }) : "";
}
/// 错误条在标题正下方(小窗口里对话框会滚动,放在底部的错误看不见);`field` = 出错的输入框:标 aria-invalid、聚焦并滚进视野(UX-163)。
function showNewProjectError(message, field = null) {
  const box = $("new-project-error");
  if (!box) return;
  box.textContent = message || "";
  box.classList.toggle("hidden", !message);
  for (const id of ["new-project-name", "new-project-parent"]) {
    const input = $(id);
    if (!input) continue;
    if (message && field === id) input.setAttribute("aria-invalid", "true"); else input.removeAttribute("aria-invalid");
  }
  if (!message) return;
  box.scrollIntoView?.({ block: "nearest" });
  if (field) $(field)?.focus?.();
}
/// 后端的错误按内容猜该指哪个输入框:位置相关的指「位置」,其余(重名、非空目录、非法名字)指「项目名称」。
function newProjectErrorField(message) {
  return /位置|不存在|拒绝访问|权限/.test(String(message)) ? "new-project-parent" : "new-project-name";
}
let newProjectHandle = null;
// 点 Esc 关掉、没提交的内容留着,下次打开接着填;只有「取消」或创建成功才清(UX-163)。
let newProjectDraft = null;
let newProjectDiscardDraft = false;
function rememberNewProjectDraft() {
  newProjectDraft = newProjectDiscardDraft ? null : {
    name: $("new-project-name").value, parent: $("new-project-parent").value,
    git: $("new-project-git").checked, desc: $("new-project-desc").value,
  };
  newProjectDiscardDraft = false;
}
export function openNewProjectDialog() {
  const overlay = $("new-project-overlay");
  if (!overlay) return null;
  if (isSurfaceOpen(overlay)) return newProjectHandle;
  const draft = newProjectDraft;
  $("new-project-name").value = draft?.name ?? "";
  $("new-project-parent").value = draft?.parent || defaultNewProjectParent();
  $("new-project-git").checked = draft?.git ?? true;
  $("new-project-desc").value = draft?.desc ?? "";
  newProjectDiscardDraft = false;
  showNewProjectError("");
  syncNewProjectPreview();
  newProjectHandle = openDialog(overlay, {
    initialFocus: $("new-project-name"),
    onClose: () => { rememberNewProjectDraft(); newProjectHandle = null; },
  });
  return newProjectHandle;
}
/// 创建完成后的一句反馈:Git 做到哪一步要说清(首提交需要本机 Git 身份;没有首提交就开不了并行线)。
export function newProjectFeedback(result) {
  if (result?.gitError) return { text: `${t("项目已建好,但 Git 初始化失败")}:${result.gitError}`, kind: "warn" };
  if (result?.git?.committed) return { text: t("项目已建好:Git 已初始化并做了首次提交"), kind: "ok" };
  if (result?.git?.identity_missing) {
    return { text: t("项目已建好,Git 已初始化;本机还没有配置 Git 身份(user.name / user.email),没有做首次提交——新建独立任务要等第一次提交,配好身份后点页头下面横幅里的「首次提交」"), kind: "warn" };
  }
  if (result?.git?.commit_error) {
    return { text: `${t("项目已建好,Git 已初始化,但首次提交没有成功")}:${String(result.git.commit_error).split("\n")[0]}`, kind: "warn" };
  }
  if (result?.git) return { text: t("项目已建好,Git 已初始化"), kind: "ok" };
  return { text: t("项目已建好(没有初始化 Git)"), kind: "ok" };
}
let newProjectSubmitting = false;
export async function submitNewProject() {
  if (newProjectSubmitting) return;
  const name = $("new-project-name").value.trim();
  const parent = $("new-project-parent").value.trim();
  if (!name) return showNewProjectError(t("项目名称不能为空"), "new-project-name");
  if (!parent) return showNewProjectError(t("先选择项目的位置"), "new-project-parent");
  showNewProjectError("");
  const create = $("new-project-create");
  newProjectSubmitting = true;
  create.disabled = true;
  create.setAttribute("aria-busy", "true");
  try {
    const description = $("new-project-desc").value.trim();
    const result = await invoke("projects_create", {
      parent,
      name,
      gitInit: $("new-project-git").checked,
      description: description || null,
    });
    setLayoutPref("new_project", "parent", parent);
    // 没做成首次提交的原因挂到「还没有提交」横幅上(toast 一闪就没了)。
    if (result?.git?.identity_missing || result?.git?.commit_error) {
      projectCommitNote = { project: result.prefs?.current ?? result.path, text: result.git.identity_missing ? gitInitFeedback(result).text : String(result.git.commit_error) };
    }
    newProjectDiscardDraft = true;
    closeSurface($("new-project-overlay"));
    await enterProject(result.prefs, { notice: t("已新建并切换到新项目") });
    // 一句话描述只进输入框当草稿,不自动发送(用户 2026-09-26 定):先看一眼、改一改再发。
    if (result.description) {
      promptBox.value = result.description;
      promptBox.dispatchEvent(new Event("input", { bubbles: true }));
      promptBox.focus?.();
    }
    const feedback = newProjectFeedback(result);
    toast(feedback.text, { kind: feedback.kind });
    refreshWorkspaceIfOpen();
  } catch (err) {
    showNewProjectError(String(err), newProjectErrorField(err));
  } finally {
    newProjectSubmitting = false;
    create.disabled = false;
    create.removeAttribute("aria-busy");
  }
}
defer(() => {
  const overlay = $("new-project-overlay");
  if (!overlay) return;
  for (const id of ["new-project-name", "new-project-parent"]) {
    $(id).addEventListener("input", () => {
      showNewProjectError("");
      syncNewProjectPreview();
    });
    $(id).addEventListener("keydown", (event) => {
      if (event.key !== "Enter" || event.isComposing) return;
      event.preventDefault?.();
      void submitNewProject();
    });
  }
  $("new-project-browse").addEventListener("click", async () => {
    try {
      const picked = await invoke("export_pick_dir");
      if (!picked) return;
      $("new-project-parent").value = picked;
      syncNewProjectPreview();
    } catch (err) {
      showNewProjectError(String(err));
    }
  });
  $("new-project-cancel").addEventListener("click", () => { newProjectDiscardDraft = true; closeSurface(overlay); });
  $("new-project-create").addEventListener("click", () => void submitNewProject());
});

/// 当前项目的事实(横幅、并行线入口、Rust 专属文案共用)。project_facts 失败时为 null(界面不猜)。
export let projectFacts = null;
export let projectFactsFor = null;
let projectFactsGeneration = 0;
export async function refreshProjectFacts(project = currentProject) {
  if (!project) return null;
  const generation = ++projectFactsGeneration;
  let facts = null;
  try {
    facts = await invoke("project_facts", { projectDir: project });
  } catch {
    facts = null;
  }
  // 切项目后迟到的结果不得画进新项目。
  if (generation !== projectFactsGeneration || project !== currentProject) return null;
  projectFacts = facts;
  projectFactsFor = project;
  renderProjectFactsBanner(facts, project);
  syncWorktreeEntry(facts);
  return facts;
}
/// 横幅一次只说一件事:上级仓库(风险)> 不是 Git 仓库 > 仓库还没有提交 > 空项目。
/// 「还没有提交」排在空项目前面(复核 minor):新建项目时本机没配 Git 身份就只 init 不提交,并行线要等第一次
/// 提交——原先只在一闪而过的 toast 里说过,之后横幅只剩「空项目」,原因只藏在建线入口的 title 里。
export function projectFactBannerKind(facts) {
  if (!facts) return null;
  if (facts.git?.state === "parent") return "parent";
  if (facts.git?.state === "none") return "no-git";
  if (facts.git?.state === "repo" && facts.git?.has_commits === false) return "no-commit";
  if (facts.layout === "greenfield") return "greenfield";
  return null;
}
const FACTS_DISMISS = "project_facts";
function factsDismissKey(project, kind) {
  return `${project}|${kind}`;
}
/// 项目事实的「下一步」:按 Git 状态给能直接做的事。横幅、开线的页内阻断条、开线被拦的 toast、「无 Git」芯片菜单共用这一份。
/// 「初始化并首次提交」是主推的一步(建库 + 默认提交信息提交一次——独立任务的工作树要从 HEAD 分出,没有提交就开不了);
/// 「仅初始化 Git」留给想先自己整理文件、稍后再提交的人(UX-128)。
export function gitFixActions(facts = projectFacts) {
  const git = facts?.git;
  if (git?.state === "none" || git?.state === "parent") {
    const nested = git.state === "parent";
    return [
      { id: "init-commit", label: t("初始化并首次提交"), primary: true, run: () => initProjectGit({ nested, commit: true }) },
      { id: "init", label: t("仅初始化 Git"), run: () => initProjectGit({ nested }) },
    ];
  }
  if (git?.state === "repo" && git.has_commits === false) {
    return [{ id: "commit", label: t("首次提交"), primary: true, run: () => initProjectGit({ commit: true }) }];
  }
  return [];
}
/// 上一次「初始化 / 首次提交」没做成的原因(缺 Git 身份、文件过多、签名失败…):挂在「还没有提交」横幅上,
/// 让用户看着它就知道下一步,而不是一闪而过的 toast 之后什么都没有(UX-163)。
let projectCommitNote = null; // { project, text }
export function renderProjectFactsBanner(facts = projectFacts, project = currentProject) {
  const box = $("project-facts");
  if (!box) return;
  const kind = projectFactBannerKind(facts);
  const dismissed = Boolean(kind) && layoutPref(FACTS_DISMISS, factsDismissKey(project, kind)) === true;
  // D-170 隔离告警在场时让位:那是更严重的一类问题,两条叠着只会稀释它。
  const sharedShown = $("project-shared-warn") && !$("project-shared-warn").classList.contains("hidden");
  if (!kind || dismissed || sharedShown) {
    box.classList.add("hidden");
    box.replaceChildren();
    delete box.dataset.kind;
    return;
  }
  box.dataset.kind = kind;
  const text = document.createElement("div");
  text.className = "project-facts-text";
  const actions = document.createElement("div");
  actions.className = "project-facts-actions";
  const action = (label, handler, { primary = false, ghost = false } = {}) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = primary ? "primary mini" : ghost ? "ghost mini" : "mini";
    button.textContent = label;
    button.addEventListener("click", handler);
    actions.appendChild(button);
    return button;
  };
  const message = document.createElement("div");
  if (kind === "parent") {
    message.textContent = fillTemplate(t("此目录位于上级仓库 {path} 内:kanzei 不会操作上级仓库,独立任务、提交与改动统计都不可用"), { path: facts.git.toplevel });
  } else if (kind === "no-git") {
    message.textContent = t("此目录不是 Git 仓库:独立任务/工作树、提交与改动统计不可用");
  } else if (kind === "no-commit") {
    message.textContent = t("Git 已初始化但还没有提交:新建独立任务要等第一次提交");
  } else {
    message.textContent = t("空项目:agent 会直接在这个目录里搭工程");
  }
  text.appendChild(message);
  if (kind === "no-commit" && projectCommitNote?.project === project && projectCommitNote.text) {
    const note = document.createElement("div");
    note.className = "project-facts-note";
    note.textContent = projectCommitNote.text;
    text.appendChild(note);
  }
  for (const fix of gitFixActions(facts)) action(fix.label, () => void fix.run(), { primary: fix.primary, ghost: !fix.primary });
  // 纯告知的两类(空项目、还没有提交)只有「知道了」;有风险/有动作的两类是「不再提示」。
  action(kind === "greenfield" || kind === "no-commit" ? t("知道了") : t("不再提示"), () => {
    setLayoutPref(FACTS_DISMISS, factsDismissKey(project, kind), true);
    renderProjectFactsBanner(facts, project);
  }, { ghost: true });
  box.replaceChildren(text, actions);
  box.classList.remove("hidden");
}
/// 初始化(并可选首次提交)的结果说成人话:做成了什么、没做成的话下一步怎么办。
export function gitInitFeedback(result, { commit = false } = {}) {
  const git = result?.git ?? {};
  if (git.committed) {
    return { text: git.created === false ? t("已完成首次提交:现在可以新建独立任务了") : t("已初始化 Git 并做了首次提交:现在可以新建独立任务了"), kind: "ok" };
  }
  if (git.identity_missing) return { text: t("Git 已初始化,但本机还没有配置 Git 身份,没法提交。在终端运行 git config --global user.name 你的名字 和 git config --global user.email 你的邮箱,再点「首次提交」"), kind: "warn", note: true };
  if (git.commit_error) return { text: `${t("Git 已初始化,但首次提交没有成功")}\n${git.commit_error}`, kind: "err", note: true };
  if (git.created === false && !commit) return { text: t("本项目已经是 Git 仓库,没有重复初始化"), kind: "info" };
  return { text: t("已初始化 Git 仓库(还没有提交:第一次提交之后才能新建独立任务)"), kind: "ok" };
}
/// 横幅、阻断条与「无 Git」芯片共用的「初始化 Git」。位于上级仓库内时先确认(嵌套仓库合法,但要说清);
/// `commit` = 「初始化并首次提交」(UX-128):非空项目会 git add -A,先把会提交什么说清,免得把 node_modules / 密钥一并提进去。
export async function initProjectGit({ nested = false, commit = false } = {}) {
  const project = currentProject;
  if (!project) return null;
  // 先说清要做什么再动手:嵌套仓库要解释一遍;非空项目首次提交会 git add -A。两件事同时要问时合成一个确认框。
  const nestedNote = nested ? t("会在本项目目录里新建一个独立的 Git 仓库(嵌套在上级仓库里)。上级仓库不受影响,但它会把这个目录看成一个未跟踪的子仓库。") : "";
  const commitNote = commit && !(projectFactsFor === project && projectFacts?.layout === "greenfield")
    ? t("会把这个目录里所有没被 .gitignore 忽略的文件提交进版本库(提交信息「初始化项目(kanzei)」)。请先确认 .gitignore 已排除 node_modules、target 这类大目录和不该入库的密钥文件。") : "";
  if ((nestedNote || commitNote) && !(await confirmDialog({
    title: nestedNote ? t("在此初始化独立仓库") : t("首次提交"),
    message: `${[nestedNote, commitNote].filter(Boolean).join("\n\n")}\n${t("继续吗?")}`,
    ...(commitNote ? { okText: nestedNote ? t("初始化并提交") : t("提交") } : {}),
  }))) return null;
  try {
    const result = await invoke("project_git_init", { projectDir: project, initialCommit: commit });
    const feedback = gitInitFeedback(result, { commit });
    // 没做成的原因要留到横幅上(toast 一闪就没了);做成了就清掉旧的说明。
    projectCommitNote = feedback.note ? { project, text: feedback.kind === "err" ? String(result?.git?.commit_error ?? "") : feedback.text } : null;
    if (feedback.kind === "err") toastError(feedback.text);
    else toast(feedback.text, { kind: feedback.kind, ...(feedback.kind === "warn" ? { timeout: 12000 } : {}) });
    if (project !== currentProject) return result;
    projectFacts = result?.facts ?? null;
    projectFactsFor = project;
    renderProjectFactsBanner(projectFacts, project);
    syncWorktreeEntry(projectFacts);
    refreshGit();
    return result;
  } catch (err) {
    toastError(`${t("初始化 Git 失败")}:${err}`);
    return null;
  }
}
/// 独立任务(工作树)需要一个有提交的独立仓库;不满足时说出原因。facts 未知时不拦(交给后端)。
export function worktreeBlockedReason(facts) {
  const git = facts?.git;
  if (!git) return "";
  if (git.state === "none") return t("新建独立任务需要 Git:本项目还不是 Git 仓库,先初始化 Git");
  if (git.state === "parent") return t("新建独立任务需要本项目自己的 Git 仓库:它只是位于上级仓库内,先在此初始化独立仓库");
  if (git.state === "repo" && !git.has_commits) return t("新建独立任务需要仓库里至少有一次提交(工作树从 HEAD 分出),先做首次提交");
  return "";
}
/// 缓存的项目事实和 git_status 对不上了(agent 刚在终端里 init / 提交过,横幅与开线入口还按旧事实拦人,UX-129)。
/// git_status 的 last 非空 = 仓库里有提交。只比对项目根自己的状态(线路工作树的状态不说明项目根)。
export function projectFactsStale(status, project = currentProject) {
  if (!projectFacts || projectFactsFor !== project || !status?.repo) return false;
  const git = projectFacts.git;
  if (status.repo === "own") return git?.state !== "repo" || Boolean(git.has_commits) !== Boolean(status.last);
  return git?.state !== status.repo;
}
/// 开线的页内阻断条(并行线路页,UX-130):原因 + 能直接做的下一步常驻在那里,不再只是 2.6 秒的 toast 和会被 i18n 覆盖的 title。
function renderWorktreeBlockBar(facts, reason) {
  const bar = $("lines-git-block");
  if (!bar) return;
  if (!reason) {
    bar.classList.add("hidden");
    bar.replaceChildren();
    return;
  }
  const text = document.createElement("div");
  text.className = "project-facts-text";
  text.textContent = reason;
  const actions = document.createElement("div");
  actions.className = "project-facts-actions";
  for (const fix of gitFixActions(facts)) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = fix.primary ? "primary mini" : "ghost mini";
    button.textContent = fix.label;
    button.addEventListener("click", () => void fix.run());
    actions.appendChild(button);
  }
  bar.replaceChildren(text, actions);
  bar.classList.remove("hidden");
}
/// 建线入口的禁用态与原因(aria-disabled + title,不动 disabled:那是建线在途的防重入位)。
export function syncWorktreeEntry(facts = projectFacts) {
  const reason = worktreeBlockedReason(facts);
  for (const button of [$("worktree-add"), $("lines-add")].filter(Boolean)) {
    if (reason) {
      if (button.dataset.blockedReason === undefined) {
        // 入口自带的用途提示(B26 后两颗「新建独立任务」都有)先记下,解除禁用时还回去。
        if (button.title) button.dataset.defaultTitle = button.title;
        // 静态 title 归 data-i18n-title 管:语言一应用它就把阻断原因盖回用途提示(UX-130)。拦着的时候摘掉,解除时还回去。
        const i18nKey = button.getAttribute("data-i18n-title");
        if (i18nKey !== null) {
          button.dataset.defaultI18nTitle = i18nKey;
          button.removeAttribute("data-i18n-title");
        }
      }
      button.setAttribute("aria-disabled", "true");
      button.dataset.blockedReason = reason;
      button.title = reason;
    } else if (button.dataset.blockedReason) {
      button.removeAttribute("aria-disabled");
      delete button.dataset.blockedReason;
      if (button.dataset.defaultI18nTitle !== undefined) {
        button.setAttribute("data-i18n-title", button.dataset.defaultI18nTitle);
        delete button.dataset.defaultI18nTitle;
      }
      if (button.dataset.defaultTitle) button.title = button.dataset.defaultTitle;
      else button.removeAttribute("title");
    }
  }
  renderWorktreeBlockBar(facts, reason);
}
defer(() => {
  // 语言切换:原因文案是按当时语言翻好的,重算一遍(切语言会把静态文案换掉,这里让阻断原因跟上)。
  document.addEventListener("kz:language", () => {
    renderProjectFactsBanner();
    syncWorktreeEntry();
  });
});

export let projectMenuHandle = null;
/// 项目卡的菜单:各项目(✓ 标当前,第二行是末两段路径)| 添加项目文件夹… 新建项目… | 项目总览。
/// 再点一次项目卡收起(openMenu 同锚点再调 = 收起)。✓ 列与模型芯片菜单共用 addMenuCheckColumn。
export function openProjectMenu() {
  const anchor = $("project-switch");
  if (!anchor) return null;
  const entries = projectMenuEntries();
  const items = [
    ...entries.map((entry) => ({
      label: entry.name,
      desc: shortProjectPath(entry.path),
      checked: entry.current,
      onSelect: () => void openProjectSpace(entry.path),
    })),
    ...(entries.length ? ["separator"] : []),
    { label: t("添加项目文件夹…"), onSelect: () => void addProjectFolder() },
    { label: t("新建项目…"), onSelect: () => void initProject() },
    "separator",
    { label: t("所有项目"), onSelect: () => navigate_view("workspace") },
  ];
  const handle = openMenu(anchor, items, {
    placement: "bottom-start",
    label: t("切换项目"),
    onClose: () => {
      if (projectMenuHandle === handle) projectMenuHandle = null;
    },
  });
  if (!handle || handle.closed) {
    projectMenuHandle = null;
    return null;
  }
  projectMenuHandle = handle;
  const menu = handle.el;
  menu.classList.add("project-menu");
  addMenuCheckColumn(menu);
  const buttons = [...menu.querySelectorAll(".k-menu-item")];
  entries.forEach((entry, index) => {
    if (!buttons[index]) return;
    buttons[index].title = entry.path;
    buttons[index].dataset.path = entry.path;
  });
  menu.querySelector('[aria-checked="true"]')?.focus?.();
  return handle;
}

defer(() => {
  const button = $("project-switch");
  if (!button) return;
  // 菜单按钮语义(index.html 里也写了;这里再落一次,00-surface 的 aria-expanded 同步认它)。
  button.setAttribute("aria-haspopup", "menu");
  button.setAttribute("aria-expanded", "false");
  button.addEventListener("click", () => openProjectMenu());
  button.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault?.();
    if (!projectMenuHandle) openProjectMenu();
  });
});

export function renderProjects(prefs, { activate = true } = {}) {
  prefs ??= { current: null, projects: [], names: {} };
  lastProjectPrefs = prefs ?? { current: null, projects: [], names: {} };
  remember_development_project(prefs.current);
  if (activate && active_space === "dev") activate_execution_root(prefs.current);
  const projectLabel = $("project-label");
  const currentProjectLabel = prefs.current
    ? (prefs.names?.[prefs.current] || baseName(prefs.current))
    : `(${localizeDynamic("未选择项目")})`;
  projectLabel.textContent = currentProjectLabel;
  projectLabel.title = prefs.current ?? localizeDynamic("未选择项目");
  projectLabel.setAttribute("aria-label", prefs.current
    ? `${currentProjectLabel}: ${prefs.current}`
    : localizeDynamic("未选择项目"));
  renderProjectSwitch(prefs);
  syncDocumentsProjectSelect(prefs);
  renderWorkbenchNavigation(prefs);
  if (activate) void refreshProcesses();
}

// 侧栏工作区头:当前项目身份的显示位 + 项目菜单的锚点(菜单见 openProjectMenu)。
export function renderProjectSwitch(prefs) {
  const nameEl = $("project-switch-name");
  const pathEl = $("project-switch-path");
  if (!nameEl || !pathEl) return;
  const current = prefs?.current ?? null;
  nameEl.textContent = current
    ? (prefs.names?.[current] || baseName(current))
    : localizeDynamic("未选择项目");
  pathEl.textContent = current ?? "";
  pathEl.title = current ?? "";
}

// D-355:切项目统一事务。侧栏点击、Workspace 卡片/文档页下拉、添加/移除/初始化项目
// 全部走这里,把「目标 process_list → 选定 active session → conversation_get」组成
// 同一个可等待的原子链:
//  - 目标历史完整返回前**不清空旧消息**——renderRecoveredMessages 在 conversation_get
//    落地时一次性替换,失败时旧内容仍在,不会出现「切换后空白 + 无法恢复」的假象;
//  - 迟到的旧项目响应由各自的 project/generation 守卫丢弃,不能覆盖新目标。
// 没切换项目(点当前项/重命名)时只刷周边,不重载对话。
export async function enterProject(prefs, options = {}) {
  const targetView = options.view ?? "chat";
  if (["project", "workspace"].includes(targetView) && !options.activate) {
    renderProjects(prefs, { activate: false });
    if (targetView === "project") await openProjectSpace(prefs.current, "project");
    else navigate_view("workspace");
    return;
  }
  const valid = options.isCurrent ?? (() => true);
  if (active_space === "research" || workspace_switch_pending) await switch_workspace("dev", { isCurrent: valid });
  if (!valid()) return;
  const previous = currentProject;
  const target = prefs.current;
  setBrowsingProject(target);
  renderProjects(prefs, { activate: false });
  activate_execution_root(target);
  rememberConversationMode("project");
  const isCurrent = () => valid() && currentProject === target;
  if (["chat", "documents", "lines"].includes(targetView) || options.activate) {
    await refreshProcesses();
    if (!isCurrent()) return;
  }
  if (targetView === "chat") {
    if (!processItems.length) {
      await invoke("process_create", { projectDir: target, profile: "dev" });
      if (!isCurrent()) return;
      await refreshProcesses();
      if (!isCurrent()) return;
    }
    await loadConversation();
    if (!isCurrent()) return;
    if (previous !== target && options.notice) addMessage("notice", options.notice);
    await loadModels();
    if (!isCurrent()) return;
    await refreshPendingInputs();
    if (!isCurrent()) return;
    void refreshGit();
    void refreshDocs();
  }
  if (!isCurrent()) return;
  if (options.activate && targetView === "project") {
    await loadModels();
    if (!isCurrent()) return;
  }
  navigate_view(targetView, { prepared: true, reload: previous !== target });
}

// 项目总览页头的两个入口(命令面板的「添加项目文件夹…」「新建项目…」也点它们)。
defer(() => {
  $("project-init")?.addEventListener("click", () => void initProject());
  $("project-add")?.addEventListener("click", () => void addProjectFolder());
});

// ---------- 队列输入 ----------
export function renderPendingInputs(items) {
  const forProject = currentProject;
  const forProcess = activeProcessId;
  const forSession = activeSessionId;
  const isCurrent = () => currentProject === forProject && activeProcessId === forProcess && activeSessionId === forSession;
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
      if (!isCurrent()) { void refreshPendingInputs(); return; }
      cancel.disabled = true;
      try {
        const changed = await invoke("cancel_input", {
          projectDir: forProject,
          inputId: item.input_id,
          processId: forProcess,
        });
        if (changed) {
          toast(t("已撤销排队输入"));
        }
        if (isCurrent()) await refreshPendingInputs();
      } catch (err) {
        toastError(`${t("撤销失败")}:${err}`);
      } finally {
        cancel.disabled = false;
      }
    });
    entry.append(prompt, delivery, cancel);
    list.appendChild(entry);
  }
}

let pendingInputsGeneration = 0;
function resetPendingInputs() {
  pendingInputsGeneration += 1;
  renderPendingInputs([]);
}
export async function refreshPendingInputs() {
  const generation = ++pendingInputsGeneration;
  const forProject = currentProject;
  const forProcess = activeProcessId;
  const forSession = activeSessionId;
  const isCurrent = () => generation === pendingInputsGeneration && currentProject === forProject && activeProcessId === forProcess && activeSessionId === forSession;
  if (!currentProject) {
    renderPendingInputs([]);
    return;
  }
  try {
    const items = await invoke("list_pending_inputs", {
      projectDir: forProject,
      processId: forProcess,
    });
    if (isCurrent()) renderPendingInputs(items);
  } catch (err) {
    if (isCurrent()) log(`${t("队列刷新失败")}:${err}`, "warn");
  }
}

// 测试记录(renderTestRuns / refreshTests)随「测试」页签搬到 12-docs-tests.js(UX-014/UX-060)。
