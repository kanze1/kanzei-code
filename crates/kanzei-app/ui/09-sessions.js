import { openMenu } from "./00-surface.js";
import { defer } from "./01-core.js";
import { motionSync } from "./01-core.js";
import { setProcessItems } from "./03-shell.js";
import { setActiveProcessId, setActiveSessionId } from "./03-shell.js";
import { $, confirmDialog, inputDialog, invoke } from "./01-core.js";
import { localizeDynamic, t } from "./02-i18n.js";
import {
  activeProcessId,
  activeSessionId,
  applySessionMeta,
  currentProject,
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
import { bgClear } from "./06-activity.js";
import { agentPanelSync } from "./06-agent-panel.js";
import { askActive, askQueueFor, hideAsk, pumpAsk } from "./07-events.js";
import {
  cancelAutoContinueTimer,
  clearAutoNotices,
  renderAutoStatus,
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
import { jumpToEntry } from "./11-docs-list.js";
import { latestDocsSnapshot, lineAuthorityLabel, refreshWorkspace, renderFocusPanel } from "./12-docs-pages.js";
import { refreshDocs } from "./14-docs-actions.js";
import {
  loadConversation,
  refreshConversationLists,
  refreshGit,
  renderLineConversationHistory,
} from "./15-views-misc.js";
import { forProject, refreshLines, revealLinesSection } from "./20-lines.js";
import { active_space, adopt_process_workspace, create_workspace_process, preferred_workspace_process, project_workspace, workspace_processes } from "./03-workspaces.js";

import { sync_composer_scope } from "./03-workspaces.js";
import { workspace_switch_pending } from "./03-workspaces.js";
import { reset_research_project } from "./19-research.js";
import { remember_development_project, switch_workspace } from "./03-workspaces.js";
import { reset_files_scope } from "./17-files.js";
// UI-0926 #10:测试记录行展开后的结构化字段。
import { normalizeTrackerFields, renderTestRecordFields } from "./04-structured.js";

export let worktreeItems = [];
export let worktreeLineCreateInFlight = false;
export let worktreeLineCreateSequence = 0;
// UI2-0926 侧栏密度:侧栏最多列 6 棵(有改动的排前),其余一条「查看全部」跳到并行线路页的工作树清单。
// 用户现场 12 棵树 × 两行,把「各线当前在做」挤出了一屏。
export const SIDEBAR_WORKTREE_LIMIT = 6;
export function renderWorktrees(items) {
  worktreeItems = items ?? [];
  const list = $("worktree-list");
  list.replaceChildren();
  const count = $("worktree-count");
  if (!worktreeItems.length) {
    // 计数一起清:空列表时它曾停在上一个项目的旧值上。线路页的清单也同步成空态。
    if (count) count.textContent = "";
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("暂无隔离工作树");
    list.appendChild(empty);
    if (typeof renderLinesWorktrees === "function") renderLinesWorktrees();
    return;
  }
  // 侧栏只做**只读呈现**:分支 + 改动量,不放操作按钮(至多一条「查看全部」跳转)。差异/收活/放弃全部迁到
  // 并行线路页——那里才有线路上下文(哪条线在跑、收活六格在哪)。侧栏的职责是
  // 「扫一眼有几棵、脏不脏」;把三颗按钮塞进两百来像素宽的行里既挤又容易误点。
  const dirty = worktreeItems.filter((item) => !item.clean).length;
  if (count) {
    count.textContent = dirty
      ? `${worktreeItems.length} · ${dirty} ${t("棵有改动")}`
      : String(worktreeItems.length);
  }
  // 有改动的排前(两组内各自保持 git worktree list 的原顺序):要处理的先看到。
  const ordered = [...worktreeItems.filter((item) => !item.clean), ...worktreeItems.filter((item) => item.clean)];
  for (const item of ordered.slice(0, SIDEBAR_WORKTREE_LIMIT)) {
    const row = document.createElement("div");
    row.className = `worktree-entry${item.clean ? "" : " dirty"}`;
    row.title = item.path;
    const head = document.createElement("div");
    head.className = "worktree-branch";
    head.textContent = item.branch;
    const meta = document.createElement("div");
    meta.className = "worktree-meta";
    meta.textContent = item.clean ? t("干净") : `${item.files.length} ${t("项改动")}`;
    row.append(head, meta);
    list.appendChild(row);
  }
  if (ordered.length > SIDEBAR_WORKTREE_LIMIT) {
    const more = document.createElement("button");
    more.type = "button";
    more.className = "worktree-more";
    more.textContent = `${t("查看全部隔离工作树")} (${ordered.length}) →`;
    more.addEventListener("click", () => {
      // 落点等线路页这轮刷新画完线路卡再滚(见 revealLinesSection);已在线路页则就地滚。
      const arriving = !$("view-lines")?.classList.contains("active");
      navigate_view("lines");
      revealLinesSection("lines-worktrees", { afterRefresh: arriving });
    });
    list.appendChild(more);
  }
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
        $("log-panel").classList.remove("hidden");
        toast(t("工作树差异已写入运行日志"));
      }
      return;
    }
    if (action === "harvest") {
      if (!item.bound_process) {
        toastError(t("该工作树没有绑定线路，不能进入收活流程"));
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
      $("log-panel").classList.remove("hidden");
      toast(t("工作树操作完成，详细结果已写入运行日志"));
    } else {
      toast(result);
    }
    if (action === "discard") {
      // 放弃现在会在后端原子注销绑定进程。三份 UI 投影必须一起刷新；只刷新
      // git 工作树清单会把一个已不存在 cwd 的旧线路页签留在前端。
      await Promise.all([refreshProcesses(), refreshWorktrees(), refreshLines(), refreshDocs()]);
      if (currentProject !== forProject) return;
      if (discardingActiveLine && !processItems.some((process) => process.id === activeProcessId)) {
        const fallback = processItems.find((process) => process.id.startsWith("d|")) || processItems[0];
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
    empty.textContent = t("暂无隔离工作树");
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
    const boundText = bound ? `${t("线路")} ${bound.label || bound.id}` : t("未绑定线路");
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
      add(t("收活"), "worktree-harvest", () => handleWorktreeAction(item, "harvest"),
        running, t("线路运行中，停止并等待收口后才能操作工作树"));
    }
    add(t("放弃"), "worktree-discard", () => handleWorktreeAction(item, "discard"),
      running, t("线路运行中，停止并等待收口后才能操作工作树"));
    row.append(main, actions);
    list.appendChild(row);
  }
}
export async function createWorktreeLine(event) {
  if (!currentProject || worktreeLineCreateInFlight) return;
  const fromLinesView = (event?.currentTarget?.id || event?.target?.id) === "lines-add";
  const workItemId = fromLinesView ? String($("lines-work-item")?.value ?? "").trim() : "";
  if (fromLinesView && !workItemId) {
    toastError(t("请先选择要绑定的条目"));
    return;
  }
  // R-179 内容④:建线成本提示(D6 定案)——每树独立 target/ = 磁盘占用 ×N,
  // 首次冷编译需数分钟。让用户在建线前知道代价,不是悄悄发生。
  const binding = workItemId ? `\n${t("开线条目")}:${workItemId}` : "";
  const addButtons = [$("worktree-add"), $("lines-add")].filter(Boolean);
  const workItemSelect = $("lines-work-item");
  // 真实 DOM 取内层 i18n span；运行时测试桩没有复建 id 节点的子树，回退到按钮本身。
  const linesAddLabel = $("lines-add")?.querySelector("[data-i18n-key]") || $("lines-add");
  const restore = () => {
    worktreeLineCreateInFlight = false;
    for (const button of addButtons) {
      button.disabled = button.id === "lines-add" && !String(workItemSelect?.value ?? "").trim();
      button.removeAttribute("aria-busy");
    }
    if (workItemSelect) workItemSelect.disabled = workItemSelect.options.length <= 1;
    if (linesAddLabel) linesAddLabel.textContent = t("按条目开线");
  };
  // D-418:确认弹窗异步化——in-flight + 禁用/aria-busy/创建中反馈提前到 confirm
  // 前,弹窗期间防重入防误操作(await 期间重复点击不会二次 process_create);
  // 取消/失败统一走 restore。
  worktreeLineCreateInFlight = true;
  for (const button of addButtons) {
    button.disabled = true;
    button.setAttribute("aria-busy", "true");
  }
  if (workItemSelect) workItemSelect.disabled = true;
  if (linesAddLabel) linesAddLabel.textContent = t("创建中…");
  if (!(await confirmDialog({ title: t("创建并行线路"), message: `${t("创建并行线路将新建独立工作树")}:${t("每线独立 target/ 目录,磁盘占用随线路数成倍增加;首次冷编译需数分钟")}。${binding}\n${t("继续创建吗")}` }))) {
    restore();
    return;
  }
  // 同 handleWorktreeAction(D-251):projectDir 在 await 前认领。
  const forProject = currentProject;
  const name = `line-${Date.now()}-${worktreeLineCreateSequence += 1}`;
  try {
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
    await switchProcess(item.id);
    toast(`${t("并行线路已创建")}:${item.branch || name}${workItemId ? ` · ${workItemId}` : ""}`);
  } catch (error) {
    toastError(`${t("创建并行线路失败")}:${error}`);
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
export function syncCollaboratorToolsVisibility(items) {
  const tools = $("collaboration-tools");
  if (!tools) return;
  // 单线程时没有跨线协作对象,隐藏勘察/复核与 task 工具开关；保留真实进程列表
  // 作为唯一判据,不从当前视图或活动面板反推线路数。
  const lineCount = Array.isArray(items) ? items.length : 0;
  tools.classList.toggle("hidden", active_space !== "dev" || lineCount <= 1);
}
export async function closeParallelProcess(processId) {
  const item = processItems.find((candidate) => candidate.id === processId);
  if (!item || item.id.startsWith("d|")) return;
  const forProject = currentProject;
  const wasActive = processId === activeProcessId;
  const runningNow = processRunning(item);
  // 关闭只注销身份(processes → retired_processes),这条线的对话一条不删;关闭后界面上
  // 不再有它的「历史对话」入口,要删得趁现在。弹窗必须把这件事说出来。
  const warning = `${runningNow
    ? t("线路仍在运行，关闭会先停止并等待收口。")
    : t("关闭会注销线路身份。已合并且干净的工作树会自动回收；有独有内容的工作树会保留。")}\n${t("这条线的对话历史仍保留在本地数据库，关闭后界面不再显示；要删除请先在它的「历史对话」里勾选删除。")}`;
  if (!(await confirmDialog({ title: t("关闭线路"), message: `${item.label} (${item.id})？\n${warning}` }))) return;
  cancelAutoContinueTimer(item.session_id);
  if (runningNow) transitionSession(item.session_id, "stopping");
  try {
    const result = await invoke("process_close", { processId });
    if (currentProject !== forProject) return;
    if (wasActive) {
      setActiveProcessId(null);
      setActiveSessionId(null);
    }
    await Promise.all([refreshProcesses(), refreshWorktrees(), refreshLines(), refreshDocs()]);
    if (currentProject !== forProject) return;
    if (wasActive && activeProcessId) await switchProcess(activeProcessId, true);
    refreshGit();
    toast(result || `${t("已关闭线路")} ${item.id}`);
  } catch (error) {
    transitionSession(item.session_id, item.running ? "running" : "idle");
    toastError(`${t("关闭线路失败")}:${error}`);
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
  const label = stoppingNow ? t("停止中…")
    : runningNow ? stage || t("运行中")
      : pendingNow ? t("鞭挞等待")
        : t("空闲");
  const name = `${lineAuthorityLabel(item)} · ${item.label}${item.branch ? ` · ${item.branch}` : ""}`;
  const title = runningNow
    ? `${name}\n${[stage || t("运行中"), state.detail].filter(Boolean).join(" · ")}`
    : pendingNow
      ? `${name}\n${t("等待下一轮")}`
      : `${name}\n${t("点击切换到此线路")}`;
  return { runningNow, pendingNow, stoppingNow, label, title };
}
export function renderParallelTaskStatus(items) {
  const target = $("parallel-task-status");
  const count = $("parallel-task-count");
  if (!target || !count) return;
  const processes = workspace_processes(items);
  count.textContent = processes.length ? `${processes.length} ${t("条任务")}` : "";
  target.replaceChildren();
  if (!processes.length) {
    if (active_space === "research") {
      const empty = document.createElement("p");
      empty.className = "dim";
      empty.textContent = t("暂无课题会话");
      target.appendChild(empty);
    }
    return;
  }
  for (const item of processes) {
    const state = sessionState(item.session_id);
    if (item.running && item.stage && (!state.stage || state.stage === "空闲")) state.stage = item.stage;
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
    head.append(`${lineAuthorityLabel(item)} · ${item.label}`);
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
    if (!item.id.startsWith("d|")) {
      // 关闭是低频的危险动作:图标按钮,悬停/聚焦这一条线路时才出现(CSS),读屏名称带线路名。
      const close = document.createElement("button");
      close.type = "button";
      close.className = "icon-btn parallel-line-close";
      close.textContent = "×";
      close.title = t("关闭线路");
      close.setAttribute("aria-label", `${t("关闭线路")} ${item.label}`);
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
  syncLineFocusLive();
}
export function refreshParallelTaskProjection(sessionId) {
  if (!sessionId) return;
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  if (!item) return;
  const row = [...document.querySelectorAll(".parallel-task-row")]
    .find((candidate) => candidate.dataset.processId === item.id);
  if (!row) return;
  const view = parallelTaskView(item);
  const { runningNow, pendingNow, stoppingNow } = view;
  row.classList.toggle("running", runningNow);
  row.classList.toggle("auto-pending", pendingNow);
  renderParallelTaskState(row, view);
  row.title = view.title;
  if (item.id === activeProcessId && pendingNow) {
    setRunPending(`${t("鞭挞")} · ${t("等待下一轮")}`);
  } else if (item.id === activeProcessId && stoppingNow) {
    setStopping(t("停止中…"));
  } else if (item.id === activeProcessId && running !== runningNow) {
    syncedRunningProcessId = item.id;
    syncedRunningState = runningNow;
    setRunning(runningNow, runningNow ? t("运行中") : t("空闲"));
  }
  syncLineFocusLive();
}
/// #7:线路行状态 = 行首独立的字形节点(.kz-glyph,只呼吸)+ 行尾的状态词(.parallel-task-state)。
/// 字形文本 ●/◐/○ 原样保留(读屏读状态词,字形 aria-hidden;既有断言读字形)。两个节点
/// **原地更新**:逐事件投影不再重建它们,呼吸动画不会每个 kz:status 都从第 0 帧重来;
/// 整表重绘新建的节点由 motionSync 对齐到全局相位。
export function renderParallelTaskState(row, { runningNow, pendingNow, stoppingNow, label }) {
  const state = stoppingNow ? "stopping" : runningNow ? "running" : pendingNow ? "pending" : "idle";
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
/// #7:侧栏「各线当前在做」卡片——线路进程真在跑才标 is-live(标题前呼吸点 + 批次格扫光)。
/// 运行态只认 processRunning(状态机优先),与线路行同一判据。
export function syncLineFocusLive() {
  for (const section of document.querySelectorAll("#focus-body .line-focus")) {
    const item = processItems.find((candidate) => candidate.id === section.dataset.processId);
    section.classList.toggle("is-live", Boolean(item && processRunning(item)));
  }
}
export function renderProcesses(items) {
  const previousItems = processItems;
  const previousProcessKey = processItems.map((item) => item.id).join("\u0000");
  const previousProcessId = activeProcessId;
  setProcessItems(items ?? []);
  syncCollaboratorToolsVisibility(processItems);
  const liveIds = new Set(processItems.map((item) => item.id));
  // 只清当前项目中确认已注销的线路；切项目时旧项目配置继续保留。身份虽已由后端
  // 保证永不复用，这里仍回收 timer/session/profile，避免长期运行积累死缓存。
  for (const removed of previousItems.filter((item) =>
    item.origin_project === currentProject && !liveIds.has(item.id))) {
    cancelAutoContinueTimer(removed.session_id);
    sessionStates.delete(removed.session_id);
    processAutoState.delete(removed.id);
    processProfileUi.delete(removed.id);
  }
  persistProcessAutoState();
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
      if (state.phase !== "stopping") transitionSession(item.session_id, "running");
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
  renderParallelTaskStatus(processItems);
  if (typeof renderFocusPanel === "function" && typeof latestDocsSnapshot !== "undefined" && latestDocsSnapshot) {
    renderFocusPanel(latestDocsSnapshot);
  }
  // 「勘察复核」= 阶段流水线总闸,默认关(后端 ProcessInfo.phase_pipeline 同默认)。
  $("process-phase-pipeline").checked = active?.phase_pipeline ?? false;
  $("process-subagents").checked = active?.subagents_enabled ?? true;
  // 分支线写主根 tracker 必须由用户显式打开；默认线直接写主根，不展示无意义开关。
  const trackerWrap = $("process-tracker-writes-wrap");
  const trackerToggle = $("process-tracker-writes");
  const isWorktreeLine = Boolean(active?.worktree_path);
  trackerWrap.classList.toggle("hidden", !isWorktreeLine);
  trackerToggle.disabled = !isWorktreeLine;
  trackerToggle.checked = isWorktreeLine && (active?.tracker_writes ?? false);
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
      if (currentProject === forProject) log(`${t("进程列表刷新失败")}:${err}`, "warn");
    } finally {
      processRefreshInFlight.delete(forProject);
    }
  })();
  processRefreshInFlight.set(forProject, promise);
  return promise;
}

export async function refreshPendingAsks() {
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
  // 不在请求发出前清空消息:切线期间保留旧内容,等目标线程的历史完整恢复后
  // renderRecoveredMessages 一次性替换,避免慢请求/竞态把主对话显示成空白。
  bgClear();
  await loadConversation(null, switchGeneration);
  if (!isCurrentSwitch()) return;
  await refreshPendingAsks();
  if (!isCurrentSwitch()) return;
  await refreshDocs();
  if (!isCurrentSwitch()) return;
  // 模型目录按项目缓存:同项目内切线不再重复探测 models_list(每个 provider 最多 6 秒);
  // 芯片回显已在上面切线那一刻发出,这里只在目录属于别的项目时补拉一次(loadModels 会顺带重问)。
  if (modelCatalogProject !== currentProject) await loadModels();
  if (!isCurrentSwitch()) return;
  void refreshGit(activeProcessId);
  refreshPendingInputs();
  void refreshProcesses();
  log(`${t("已切换到进程")} ${target.label}`);
}

defer(() => {
  $("process-add").addEventListener("click", async () => {
    if (!currentProject) return;
    try {
      // 新进程与默认进程同一默认:勘察复核关(要显式打开才强制走七阶段)。
      await create_workspace_process(active_space === "research" ? project_workspace().research.topic : null);
    } catch (err) {
      toastError(`${t("创建进程失败")}:${err}`);
    }
  });
});

defer(() => {
  $("process-phase-pipeline").addEventListener("change", async (event) => {
    if (!activeProcessId) return;
    try {
      await queueProcessUpdate(activeProcessId, { phasePipeline: event.target.checked });
      updateLocalProcessItem(activeProcessId, { phase_pipeline: event.target.checked });
      await refreshProcesses();
      log(event.target.checked ? t("勘察复核已开启:每个任务强制走勘察→实现→复核") : t("勘察复核已关闭:恢复一问一答,模型仍可自己派子代理"));
    } catch (err) {
      event.target.checked = !event.target.checked;
      toastError(`${t("更新进程能力失败")}:${err}`);
    }
    renderAutoStatus();
  });
});

defer(() => {
  $("process-subagents").addEventListener("change", async (event) => {
    if (!activeProcessId) return;
    try {
      await queueProcessUpdate(activeProcessId, { subagentsEnabled: event.target.checked });
      updateLocalProcessItem(activeProcessId, { subagents_enabled: event.target.checked });
      await refreshProcesses();
      log(event.target.checked ? t("子代理已开启") : t("子代理已关闭:新一轮工具面不含 task"));
    } catch (err) {
      event.target.checked = !event.target.checked;
      toastError(`${t("更新进程能力失败")}:${err}`);
    }
  });
});

defer(() => {
  $("process-tracker-writes").addEventListener("change", async (event) => {
    const active = processItems.find((item) => item.id === activeProcessId);
    if (!activeProcessId || !active?.worktree_path) return;
    try {
      await queueProcessUpdate(activeProcessId, { trackerWrites: event.target.checked });
      updateLocalProcessItem(activeProcessId, { tracker_writes: event.target.checked });
      await refreshProcesses();
      log(event.target.checked ? t("当前分支线已允许写主根追踪器") : t("当前分支线已恢复为只读主根追踪器"));
    } catch (err) {
      event.target.checked = !event.target.checked;
      toastError(`${t("更新追踪器写入权限失败")}:${err}`);
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

export function activate_execution_root(root) {
  const previousProject = currentProject;
  setCurrentProject(root);
  syncWorkPriorityControl();
  // R-115:按项目记的偏好(模型/思考强度/筛选)要跟着项目切换回填,
  // 也覆盖了启动这一次——currentProject 在这里才第一次确定。
  restoreProjectPrefs();
  checkProjectIsolation();
  if (previousProject !== currentProject) {
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

export async function switchProject(path) {
  try {
    await enterProject(await invoke("projects_select", { path }));
    return true;
  } catch (error) {
    toastError(`${t("切换项目失败")}:${error}`);
    return false;
  }
}

// 项目总览页开着时,改名/移除后卡片要跟着换;不开着就不白跑一次 workspace_snapshot。
function refreshWorkspaceIfOpen() {
  if ($("view-workspace")?.classList.contains("active")) void refreshWorkspace();
}

export async function renameProject(path, prefs = lastProjectPrefs) {
  const nextName = await inputDialog({
    title: t("项目显示名"),
    value: projectDisplayName(path, prefs),
  });
  if (nextName === null || !nextName.trim()) return;
  try {
    renderProjects(await invoke("projects_rename", { path, name: nextName.trim() }));
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
  if (!(await confirmDialog({ title: t("移除项目"), message: `“${name}”吗？${t("只解除登记,不会删除磁盘文件。")}` }))) return;
  try {
    const wasCurrent = currentProject === path;
    const next = await invoke("projects_remove", { path });
    if (wasCurrent) {
      await enterProject(next, wasOnOverview ? { view: "workspace" } : {});
    } else {
      renderProjects(next);
    }
    refreshWorkspaceIfOpen();
  } catch (err) {
    toastError(String(err));
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

// 「新建项目…」暂时沿用既有的初始化流程(路径 + 显示名两次输入);新建项目弹窗另有计划替换它。
export async function initProject() {
  const path = await inputDialog({
    title: t("新项目目录路径(不存在时会创建)"),
  });
  if (path === null || !path.trim()) return;
  const name = await inputDialog({
    title: t("项目显示名(可留空)"),
    value: baseName(path.trim()),
  });
  if (name === null) return;
  try {
    const prefs = await invoke("projects_init", {
      path: path.trim(),
      name: name.trim() || null,
    });
    await enterProject(prefs, { notice: t("已初始化并切换到新项目") });
    toast(t("项目初始化完成"));
    refreshWorkspaceIfOpen();
  } catch (err) {
    toastError(String(err));
  }
}

export let projectMenuHandle = null;
/// 项目卡的菜单:各项目(✓ 标当前,第二行是末两段路径)| 打开文件夹… 新建项目… | 项目总览。
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
      onSelect: entry.current ? undefined : () => void switchProject(entry.path),
    })),
    ...(entries.length ? ["separator"] : []),
    { label: t("打开文件夹…"), onSelect: () => void addProjectFolder() },
    { label: t("新建项目…"), onSelect: () => void initProject() },
    "separator",
    { label: t("项目总览"), onSelect: () => navigate_view("workspace") },
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

export function renderProjects(prefs) {
  lastProjectPrefs = prefs ?? { current: null, projects: [], names: {} };
  remember_development_project(prefs.current);
  if (active_space === "dev") activate_execution_root(prefs.current);
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
  refreshProcesses();
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
  if (active_space === "research") await switch_workspace("dev");
  const previous = currentProject;
  renderProjects(prefs);
  if (previous !== currentProject) {
    // 运行状态属于会话:切项目后必须按目标项目重算,否则旧项目的 kz:done 被会话过滤器
    // 丢弃,新项目会永久卡在「运行中」(发送键禁用)。refreshProcesses 会带回真实状态。
    setRunning(false, t("空闲"));
    // 鞭挞控制台的停机原因/一次性提示是全局单例文本。不清的话,A 项目的
    // 「需求与缺陷已清空,自动推进已停止」会原样挂在 B 项目的控制台上,
    // 用户据此以为 B 也没活可干了。
    if (typeof clearAutoNotices === "function") clearAutoNotices();
    await loadConversation();
    if (options.notice) addMessage("notice", options.notice);
  }
  await refreshDocs();
  await loadModels();
  refreshGit();
  await refreshPendingInputs();
  if (previous !== currentProject) {
    // options.view:调用方要留在某页(总览页里移除当前项目),否则回到目标项目记住的视图。
    const workspace = project_workspace();
    navigate_view(options.view ?? workspace[workspace.space].view);
  }
}

// 项目总览页头的两个入口(命令面板的「打开文件夹…」「新建项目…」也点它们)。
defer(() => {
  $("project-init")?.addEventListener("click", () => void initProject());
  $("project-add")?.addEventListener("click", () => void addProjectFolder());
});

// ---------- 队列输入 ----------
export function renderPendingInputs(items) {
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
        toastError(`${t("撤销失败")}:${err}`);
      }
    });
    entry.append(prompt, delivery, cancel);
    list.appendChild(entry);
  }
}

export async function refreshPendingInputs() {
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
    log(`${t("队列刷新失败")}:${err}`, "warn");
  }
}

export function renderTestRuns(snapshot) {
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
    // UI-0926 #10:行头可点,展开后是结构化字段(命令列表、收尾时间、源码指纹路径…);
    // 字段只在首次展开时构建。字段同时接受 {key,value}(真实 IPC)与 [k,v] 两种形状。
    row.dataset.docId = record.id;
    const head = document.createElement("button");
    head.type = "button";
    head.className = "sv-test-head";
    head.setAttribute("aria-expanded", "false");
    head.textContent = `${record.status === "passed" ? "✓" : record.status === "failed" ? "×" : record.status === "running" ? "●" : "○"} ${record.id} ${record.title}`;
    head.title = normalizeTrackerFields(record.fields ?? []).map((field) => `${field.key}: ${field.value}`).join("\n");
    const detail = document.createElement("div");
    detail.className = "sv-test-detail hidden";
    head.addEventListener("click", () => {
      if (!detail.children.length) detail.appendChild(renderTestRecordFields(record.fields ?? []));
      const closed = detail.classList.toggle("hidden");
      head.setAttribute("aria-expanded", String(!closed));
    });
    row.appendChild(head);
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
        chip.addEventListener("click", () => jumpToEntry(refId, { expand: true }));
        refRow.appendChild(chip);
      }
      row.appendChild(refRow);
    }
    row.appendChild(detail);
    list.appendChild(row);
  }
}

export async function refreshTests() {
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
    log(`${t("测试记录刷新失败")}:${error}`, "warn");
  }
}

defer(() => {
  $("tests-refresh").addEventListener("click", refreshTests);
});
