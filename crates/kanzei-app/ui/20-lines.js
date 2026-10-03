import { defer } from "./01-core.js";
import { motionSync } from "./01-core.js";
import { $, confirmDialog, invoke } from "./01-core.js";
import { languageIsEnglish, localizedStage, t } from "./02-i18n.js";
import {
  activeProcessId,
  currentProject,
  ensureChatView,
  log,
  processItems,
  running,
  sessionState,
  toastError,
  transitionSession,
} from "./03-shell.js";
import { buildDiffTree, renderDiff } from "./06-activity.js";
import { fillTemplate, parseUnifiedDiff } from "./04-structured-parse.js";
import { askActive, askQueues } from "./07-events.js";
import { armStoppingWatchdog, awaitingUserSessions, cancelAutoContinueTimer, setAutoRounds } from "./08-auto.js";
import { lineAutoConfig, queueProcessUpdate, setLineAutoState, updateLocalProcessItem } from "./08-compose-runtime.js";
import { state } from "./08-compose.js";
import { projectDefaultModel, syncModelSelectToActiveLine } from "./08-models.js";
import {
  closeParallelProcess,
  createWorktreeLine,
  processRunning,
  refreshWorktrees,
  switchProcess,
  worktreeLineCreateInFlight,
} from "./09-sessions.js";
import { jumpToEntry } from "./11-docs-list.js";
import { latestDocsSnapshot, renderDocuments } from "./12-docs-pages.js";
import { refreshDocs } from "./14-docs-actions.js";
import { refreshGit } from "./15-views-misc.js";

// ---------- R-184 B 面:任务级并行线路 ----------
// 这里不另造状态仓库。agent 协作块和用户并列视图都读 collaboration_snapshot,
// 其 branch / phase / tool / changed_files 来自后端当前运行态和 git 现场。
export let collaborationLines = [];
export let linesRefreshInFlight = false;
export let linesRefreshTimer = null;
export let linesRefreshQueued = false;
export const LINES_REFRESH_IDLE_MS = 8000;
export const LINES_REFRESH_RUNNING_MS = 3500;

export function lineAgentCodes(lines) {
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

// R-247:可绑定的条目只列当前可领取(todo/open)、未阻塞的 R/D 条目。最终 claim 仍由
// 后端 WorkTool 原子校验；这里负责让用户先选事实对象，不在前端复制调度器。
export function renderLineWorkItemOptions(snapshot = null) {
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
  placeholder.textContent = t(candidates.length ? "不绑定条目" : "没有可绑定的条目");
  select.replaceChildren(placeholder);
  for (const entry of candidates) {
    const option = document.createElement("option");
    option.value = entry.id;
    option.textContent = `${entry.id} · ${entry.title}`;
    select.appendChild(option);
  }
  select.value = candidates.some((entry) => entry.id === previous) ? previous : "";
  select.disabled = worktreeLineCreateInFlight || candidates.length === 0;
  // B26:绑定条目是可选的,「新建独立任务」不再要求先选条目才能点;只在建线进行中禁用(防重入)。
  add.disabled = worktreeLineCreateInFlight;
}

export function normalizedChangedFile(path) {
  return String(path).replaceAll("\\", "/").toLocaleLowerCase();
}

export function lineConflictPairs(lines) {
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

export function formatLineTokens(value) {
  const count = Number(value || 0);
  if (count < 1000) return String(count);
  return `${(count / 1000).toFixed(count < 10000 ? 1 : 0)}k`;
}

export function lineFact(label, value, className = "") {
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

// 线路页每条线都有鞭挞与模型控件:原先这两样只有「当前打开的那条线」有(输入框上方那一份),
// 要给 N 条线配不同模型、或让某条后台线开始/停止自主推进,就得切 N 次线——而并行
// 线路页正是唯一能一屏看全所有线的地方。鞭挞控件本身不持有状态:开关/暂停/本轮后停
// 一律经 setLineAutoState 落到该线存档 + 它自己的后端 auto_state;模型经
// queueProcessUpdate 落该线 process(run_prompt 的 model 回落读的就是它)。
export let linesModelCatalog = null;
export let linesModelCatalogProject = null;
export async function loadLinesModelCatalog() {
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

export function buildLineModelSelect(item) {
  const select = document.createElement("select");
  // UI2-0926 #11:.ctx-select 随输入区控件体系(.kz-ctl)删除;线路页的模型下拉回到普通 select 外观。
  select.className = "line-model-select";
  select.title = t("模型改动下一轮生效");
  select.setAttribute("aria-label", t("模型"));
  // 整块重绘后按这个键把焦点还给同一个控件(见 renderLines 的 captureLinesFocus)。
  select.dataset.focusKey = "model";
  const current = item.model || "";
  const seen = new Set();
  const add = (value, label) => {
    if (seen.has(value)) return;
    seen.add(value);
    select.appendChild(new Option(label, value));
  };
  // UI-0926 #3:空选项写明「跟随默认」解析到哪个模型(与输入框芯片同源:model_effective)。
  add("", `${t("跟随默认")} · ${projectDefaultModel ?? ""}`.replace(/ · $/, ""));
  // 角色项(primary/fast/compact)不再列出:它们的去向由项目/全局配置决定,「跟随默认」已经说清;
  // 只有这条线当前存的就是某个角色(旧版遗留)时才列出,方便看见并清掉。
  for (const model of linesModelCatalog ?? []) {
    if (["primary", "fast", "compact"].includes(model.id) && model.id !== current) continue;
    add(model.id, model.label);
  }
  // 该线记住的直指模型即使不在探测清单里也必须可见,否则一次刷新就把它从下拉里抹掉,
  // 用户以为自己没设过(D-167 同源)。
  if (current) add(current, `${current}(${t("已记住")})`);
  select.value = current;
  select.addEventListener("change", async () => {
    const value = select.value;
    updateLocalProcessItem(item.id, { model: value || null });
    try {
      await queueProcessUpdate(item.id, { model: value });
      log(`${item.label} ${t("该对话模型已切换")}:${value || t("跟随默认")}`);
      // 改的若是当前线,输入框上方的芯片要跟着走——两处显示同一条线却不一致最难查。
      // 目录没变,只重问一次「下一轮将使用」。
      if (item.id === activeProcessId) await syncModelSelectToActiveLine();
    } catch (error) {
      toastError(`${t("模型切换失败")}:${error}`);
    }
  });
  return select;
}

export function buildLineAutoControls(line) {
  const box = document.createElement("div");
  box.className = "line-autorun";
  const item = processItems.find((process) => process.id === line.process_id);
  if (!item) {
    // 协作快照里有、进程列表里还没有:两份数据之间的刷新窗口。不画半截控件。
    box.classList.add("pending");
    box.textContent = t("独立任务状态同步中…");
    return box;
  }
  const config = lineAutoConfig(line.process_id);
  const rounds = Number(sessionState(item.session_id)?.auto_rounds ?? 0) || 0;

  const toggle = document.createElement("label");
  toggle.className = "line-auto-toggle";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = config.enabled;
  checkbox.dataset.focusKey = "auto";
  checkbox.addEventListener("change", () => {
    void setLineAutoState(line.process_id, { enabled: checkbox.checked });
  });
  const toggleText = document.createElement("span");
  toggleText.textContent = t("鞭挞");
  toggle.append(checkbox, toggleText);

  const progress = document.createElement("span");
  progress.className = "line-auto-rounds";
  progress.textContent = `${rounds}`;
  progress.title = t("鞭挞轮次");

  const pause = document.createElement("button");
  pause.type = "button";
  pause.className = `ghost mini line-auto-pause${config.paused ? " active" : ""}`;
  pause.textContent = config.paused ? t("恢复鞭挞") : t("暂停鞭挞");
  pause.disabled = !config.enabled;
  pause.dataset.focusKey = "pause";
  pause.addEventListener("click", () => {
    void setLineAutoState(line.process_id, { paused: !config.paused });
  });

  const stopRound = document.createElement("button");
  stopRound.type = "button";
  stopRound.className = `ghost mini line-auto-stop-round${config.stopAfterRound ? " active" : ""}`;
  stopRound.textContent = t("本轮后停");
  stopRound.disabled = !config.enabled;
  stopRound.dataset.focusKey = "stop-round";
  stopRound.addEventListener("click", () => {
    void setLineAutoState(line.process_id, { stopAfterRound: !config.stopAfterRound });
  });

  const modelLabel = document.createElement("span");
  modelLabel.className = "line-auto-model-label";
  modelLabel.textContent = t("模型");
  box.append(toggle, progress, pause, stopRound);
  if (item.worktree_path) box.append(buildTrackerWritesToggle(item));
  box.append(modelLabel, buildLineModelSelect(item));
  return box;
}

// 「改主项目需求记录」是工作树线的属性(默认关,读取始终可用)。原先挤在输入区「任务设置」菜单里,只对当前线生效、
// 线路少于两条时还整组藏着;搬到线路卡上:哪条线开了写权限一眼可见,改它也不用先切过去。
export function buildTrackerWritesToggle(item) {
  const label = document.createElement("label");
  label.className = "line-auto-toggle line-tracker-writes";
  label.title = t("允许这个独立任务修改主项目唯一的需求、缺陷、想法、决策等记录。默认关闭；读取始终可用。");
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = Boolean(item.tracker_writes);
  checkbox.dataset.focusKey = "tracker";
  checkbox.addEventListener("change", async () => {
    const enabled = checkbox.checked;
    try {
      await queueProcessUpdate(item.id, { trackerWrites: enabled });
      updateLocalProcessItem(item.id, { tracker_writes: enabled });
      log(`${item.label} ${enabled ? t("已允许改主项目需求记录") : t("已恢复为只读主项目需求记录")}`);
    } catch (error) {
      checkbox.checked = !enabled;
      toastError(`${t("更新需求记录写入权限失败")}:${error}`);
    }
  });
  const text = document.createElement("span");
  text.textContent = t("改主项目需求记录");
  label.append(checkbox, text);
  return label;
}

// B27:跨线文件交集不再单占一整节(无冲突时恒为一行字 + 「文本层/语义层」黑话),改成各线路卡上的徽标——
// 谁和谁改了同一批文件、是哪几个,在改动文件那一行直接看到。返回 process_id → [{ other, files }]。
export function lineOverlapsFor(lines) {
  const byLine = new Map();
  for (const pair of lineConflictPairs(lines)) {
    for (const [self, other] of [[pair.left, pair.right], [pair.right, pair.left]]) {
      const list = byLine.get(self.process_id) ?? [];
      list.push({ other, files: pair.files });
      byLine.set(self.process_id, list);
    }
  }
  return byLine;
}

// 线路卡标题:用户命名(sessions.title,后端补上 title 字段后即生效)优先,主对话叫「主对话」,其余回落后端 label。
export function lineName(line, item = processItems.find((process) => process.id === line.process_id)) {
  const title = String(item?.title ?? "").trim();
  if (title) return title;
  return line.label;
}

// 这条线此刻是不是在等人:有挂起的权限询问(等批准),或模型自己停下来等回答(引擎 Stop/AwaitingUser)。
// 前端的待答队列是这件事的真源;后端 line_status 只看事件流与工作树有没有动静。
export function lineWaitingFor(item) {
  if (!item?.session_id) return "";
  const queue = askQueues.get(item.session_id);
  if ((queue && queue.length > 0) || askActive?.sessionId === item.session_id) return "approval";
  return awaitingUserSessions.has(item.session_id) ? "answer" : "";
}

export function lineStatusKey(line, lineRunning, waiting = "", stopping = false) {
  // 已点「停止」、等对话收尾:卡片立刻进「停止中」,不等下一轮轮询(此前点了毫无反馈、按钮还能重复点)。
  if (stopping) return "stopping";
  // 等批准/等回答的线不是「卡住」:权限询问挂着时事件流与工作树都静默,后端会把它判成 suspected_stuck(UX-072)。
  if (waiting === "approval") return "awaiting_approval";
  if (waiting === "answer" && !lineRunning) return "awaiting_answer";
  const status = line.status;
  if (["running", "suspected_stuck", "failed", "completed", "stopped", "idle"].includes(status)) {
    return status;
  }
  // 兼容旧快照/测试桩：后端升级前仍按既有 running 字段显示，不猜测卡住。
  return lineRunning ? "running" : "idle";
}

export function lineStatusLabel(status) {
  return {
    running: t("运行中"),
    stopping: t("停止中…"),
    awaiting_approval: t("在等你批准"),
    awaiting_answer: t("在等你回答"),
    suspected_stuck: t("疑似卡住"),
    failed: t("失败"),
    completed: t("完成"),
    stopped: t("已停止"),
    idle: t("空闲"),
  }[status] || t("空闲");
}

/// 这条线的会话是否正处在「停止中」(已发出停止、等对话收尾)。
export function lineIsStopping(item) {
  return Boolean(item?.session_id) && sessionState(item.session_id)?.phase === "stopping";
}

// 运行中线路的「停止」。当前线直接点输入区那颗 #stop——状态栏、按钮与鞭挞计时的收口都在它的处理器里;
// 后台线没有那套 UI,走与 25-softwire 的 stop() 同一条路:先收鞭挞计时与轮次,进「停止中」,再发 stop_run。
export async function stopLine(line) {
  const item = processItems.find((process) => process.id === line.process_id);
  if (!item) return;
  if (item.id === activeProcessId && $("stop")) {
    $("stop").click();
    return;
  }
  const forProject = currentProject;
  cancelAutoContinueTimer(item.session_id);
  setAutoRounds(item.session_id, 0);
  transitionSession(item.session_id, "stopping");
  armStoppingWatchdog(item.session_id);
  try {
    await invoke("stop_run", { projectDir: forProject, processId: item.id });
    log(`${lineName(line, item)} ${t("停止指令已确认，等待对话结束")}`);
  } catch (error) {
    transitionSession(item.session_id, processRunning(item) ? "running" : "idle");
    toastError(`${t("停止指令失败")}:${error}`);
  } finally {
    void refreshLines();
  }
}

// ---------- 整块重绘的两道保险(UX-071) ----------
// 线路页每 3.5s(有线在跑)/8s 按快照整块重建线路卡。重建会吃掉用户正在用的控件:
// ①正展开着的原生模型下拉被直接关掉、选了一半的值丢掉;②键盘焦点回到 body。
// 所以:内容没变就不重建;用户停在下拉/输入框上时先推迟,等他离开(focusout)再补;其余控件(按钮、勾选)
// 重建后按 data-focus-key 把焦点还给同一个位置。
let lastLinesSignature = "";
let pendingLinesRender = null;

function linesBusyControl() {
  const list = $("lines-list");
  const el = typeof document !== "undefined" ? document.activeElement : null;
  if (!el || !list || typeof list.contains !== "function" || !list.contains(el)) return null;
  const tag = String(el.tagName || "").toLowerCase();
  if (tag === "select" || tag === "textarea") return el;
  return tag === "input" && !["checkbox", "radio", "button"].includes(el.type) ? el : null;
}

function captureLinesFocus() {
  const list = $("lines-list");
  const el = typeof document !== "undefined" ? document.activeElement : null;
  if (!el || !list || typeof list.contains !== "function" || !list.contains(el)) return null;
  const key = el.dataset?.focusKey;
  const processId = el.closest?.(".line-lane")?.dataset?.processId;
  return key && processId ? { processId, key } : null;
}

function restoreLinesFocus(saved) {
  if (!saved) return;
  const lane = [...$("lines-list").querySelectorAll(".line-lane")].find((node) => node.dataset.processId === saved.processId);
  const control = [...(lane?.querySelectorAll("[data-focus-key]") ?? [])].find((node) => node.dataset.focusKey === saved.key);
  if (control && !control.disabled) control.focus?.({ preventScroll: true });
}

export function renderLines(lines) {
  collaborationLines = lines ?? [];
  const target = $("lines-list");
  const lineItem = (line) => processItems.find((process) => process.id === line.process_id);
  const lineIsRunning = (line) => {
    const item = lineItem(line);
    return item ? processRunning(item) : Boolean(line.running);
  };
  const overlaps = lineOverlapsFor(collaborationLines);
  const runningCount = collaborationLines.filter(lineIsRunning).length;
  const changedCount = new Set(
    collaborationLines.flatMap((line) => (line.changed_files ?? []).map(normalizedChangedFile)),
  ).size;
  // 线路页的名词统一:每一条是一个「任务」(主对话也是其中一条);「线路」只留页面标题。
  let summary = fillTemplate(t("{running} 个运行中 · 共 {total} 个任务 · {files} 个改动文件"), {
    running: runningCount, total: collaborationLines.length, files: changedCount,
  });
  const overlapPairs = lineConflictPairs(collaborationLines).length;
  if (overlapPairs) summary += ` · ⚠ ${fillTemplate(t("{n} 组任务有文件重叠"), { n: overlapPairs })}`;
  const summaryNode = $("lines-summary");
  if (summaryNode.textContent !== summary) summaryNode.textContent = summary;

  if (!collaborationLines.length) {
    pendingLinesRender = null;
    lastLinesSignature = "";
    const empty = document.createElement("div");
    empty.className = "lines-empty";
    empty.textContent = t("还没有可显示的任务。主对话开始工作后会出现在这里;点右上角「新建独立任务」可以再开一个并行的。");
    target.replaceChildren();
    target.appendChild(empty);
    return;
  }

  // 视图模型签名:画一张线路卡用到的全部输入。与上次画的一致就不动 DOM(既省抖动,也不会重置展开态)。
  const entries = [...(latestDocsSnapshot?.requirements || []), ...(latestDocsSnapshot?.defects || [])];
  const signature = JSON.stringify([
    activeProcessId, languageIsEnglish(), linesModelCatalog?.length ?? -1, projectDefaultModel,
    collaborationLines.map((line) => {
      const item = lineItem(line);
      const claimedIds = String(line.claim || "").match(/\b[RD]-[\w-]+\b/g) || [];
      return [
        line, lineIsRunning(line), lineName(line, item), lineWaitingFor(item), lineIsStopping(item),
        item ? [lineAutoConfig(line.process_id), sessionState(item.session_id)?.auto_rounds ?? 0, item.model ?? null, item.tracker_writes ?? false]
          : null,
        claimedIds.map((id) => entries.find((entry) => entry.id === id)?.title ?? null),
        (overlaps.get(line.process_id) ?? []).map((overlap) => [overlap.other.process_id, overlap.files]),
      ];
    }),
  ]);
  const laneCount = target.querySelectorAll(".line-lane").length;
  if (signature === lastLinesSignature && laneCount === collaborationLines.length) {
    pendingLinesRender = null;
    return;
  }
  // 用户正停在某个线路卡的下拉/输入框里:这一轮不重建,记下来等 focusout 再补。
  if (linesBusyControl()) {
    pendingLinesRender = collaborationLines;
    return;
  }
  pendingLinesRender = null;
  lastLinesSignature = signature;
  const savedFocus = captureLinesFocus();

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
  const expandedDetails = new Set([...target.querySelectorAll(".line-runtime-details[open]")]
    .map(panel => panel.closest(".line-lane")?.dataset.processId));
  const expandedClaims = new Set([...target.querySelectorAll(".line-held-items[open]")]
    .map(panel => panel.closest(".line-lane")?.dataset.processId));
  target.replaceChildren();

  const codes = lineAgentCodes(collaborationLines);
  for (const line of collaborationLines) {
    const item = lineItem(line);
    const lineRunning = lineIsRunning(line);
    const lane = document.createElement("article");
    const code = codes.get(line.process_id) ?? "?";
    // 线路身份 = 字母代号本身(M/A/B…),不再按代号取色——身份色曾撞上琥珀/绿这些状态色。
    lane.className = `line-lane${line.process_id === activeProcessId ? " active" : ""}`;
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
    title.textContent = lineName(line, item);
    const processId = document.createElement("code");
    processId.textContent = line.worktree_path ? t("独立工作树") : t("主工作区");
    processId.title = line.process_id;
    titleWrap.append(title, processId);
    identity.append(badge, titleWrap);
    const state = document.createElement("span");
    const stopping = lineIsStopping(item);
    const statusKey = lineStatusKey(line, lineRunning, lineWaitingFor(item), stopping);
    state.className = `line-running-state ${statusKey.replaceAll("_", "-")}`;
    state.textContent = lineStatusLabel(statusKey);
    // #7:线路页按快照整块重绘,新节点对齐全局相位,扩散环不会每次重绘都从头来。
    motionSync(state);
    head.append(identity, state);

    const claim = document.createElement("div");
    claim.className = "line-claim";
    const claimedIds = String(line.claim || "").match(/\b[RD]-[\w-]+\b/g) || [];
    const claimText = id => {
      const entry = entries.find(entry => entry.id === id);
      return entry ? `${id} · ${entry.title}` : id;
    };
    // UI-0926 #4:快照里查得到的条目做成链接,一点直达单页里展开的详情;查不到(线路工作树里
    // 刚登记、还没合并进主列表)就只写文字,不给一个点了落空的链接。
    const claimNode = id => {
      if (!entries.some(entry => entry.id === id)) return document.createTextNode(claimText(id));
      const link = document.createElement("button");
      link.type = "button";
      link.className = "ref-link line-claim-link";
      link.textContent = claimText(id);
      link.title = t("点击查看详情");
      link.addEventListener("click", () => void jumpToEntry(id, { expand: true }));
      return link;
    };
    if (claimedIds.length > 1) {
      const held = document.createElement("details");
      held.className = "line-held-items";
      held.open = expandedClaims.has(line.process_id);
      const summary = document.createElement("summary");
      summary.textContent = `${t(line.worktree_path ? "工作树内负责" : "共享工作区内负责")} ${claimedIds.length} ${t("条")}`;
      const list = document.createElement("ul");
      for (const id of claimedIds) { const item = document.createElement("li"); item.appendChild(claimNode(id)); list.appendChild(item); }
      held.append(summary, list); claim.appendChild(held);
    } else if (claimedIds.length) claim.appendChild(claimNode(claimedIds[0]));
    else claim.textContent = t(line.claim || "未绑定条目"); // 后端给的「未绑定条目」是中文字面量,英文界面要翻
    if (line.claim_error) {
      claim.classList.add("error");
      claim.title = line.claim_error;
    }

    const facts = document.createElement("div");
    facts.className = "line-facts";
    facts.append(
      lineFact(t("阶段"), localizedStage(line.phase || "空闲")),
      lineFact(t("当前工具"), line.current_tool || "—"),
      lineFact(t("步数"), String(line.steps || 0)),
      lineFact(t("令牌"), `${formatLineTokens(line.input_tokens)} ↓ / ${formatLineTokens(line.output_tokens)} ↑`),
    );
    // 鞭挞与模型是这页的主要用途(一屏横向比对、操控每条线),放在卡面上;折叠区只留分支/工作树这类低频事实(内部编号不外露,只在标题行 tooltip 里)。
    const controls = buildLineAutoControls(line);
    const runtimeDetails = document.createElement("details");
    runtimeDetails.className = "line-runtime-details";
    runtimeDetails.open = expandedDetails.has(line.process_id);
    const runtimeSummary = document.createElement("summary");
    runtimeSummary.textContent = t("独立任务详情");
    runtimeDetails.append(runtimeSummary,
      lineFact(t("分支"), line.branch || "—", "mono"),
      lineFact(t("工作树"), line.worktree_path || t("主工作区"), "mono"));

    const changed = document.createElement("details");
    changed.className = "line-changed-files";
    const changedSummary = document.createElement("summary");
    const files = line.changed_files ?? [];
    changedSummary.textContent = `${files.length} ${t("个改动文件")}`;
    // 文件交集徽标(B27):与其它线改了同一批文件时亮在改动文件那一行,点开清单里重叠的文件标琥珀。
    const overlap = overlaps.get(line.process_id) ?? [];
    const sharedFiles = new Set(overlap.flatMap((entry) => entry.files.map(normalizedChangedFile)));
    if (overlap.length) {
      const conflictBadge = document.createElement("span");
      conflictBadge.className = "line-conflict-badge";
      conflictBadge.textContent = `⚠ ${fillTemplate(t("{n} 个文件与 {who} 重叠"), {
        n: sharedFiles.size,
        who: overlap.map((entry) => lineName(entry.other)).join("、"),
      })}`;
      conflictBadge.title = t("同一个文件被两个独立任务同时改了,合并时可能冲突。这里只比对文件名,改动在逻辑上是否冲突要读 diff 判断。");
      changedSummary.append(" ", conflictBadge);
    }
    changed.appendChild(changedSummary);
    if (files.length) {
      const list = document.createElement("ul");
      for (const file of files) {
        const entry = document.createElement("li");
        entry.textContent = file;
        if (sharedFiles.has(normalizedChangedFile(file))) entry.className = "overlap";
        list.appendChild(entry);
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
    // 运行中的线路可以直接在这里停(原来只能先切过去、再点输入区的停止)。
    if (lineRunning) {
      const stopButton = document.createElement("button");
      stopButton.type = "button";
      stopButton.className = "ghost mini danger line-stop";
      stopButton.textContent = stopping ? t("停止中…") : t("停止");
      stopButton.disabled = stopping; // 停止指令已发出:别再点第二次(重复 stop_run)
      stopButton.dataset.focusKey = "stop";
      stopButton.setAttribute("aria-label", `${t("停止")} ${lineName(line, item)}`);
      stopButton.addEventListener("click", () => void stopLine(line));
      actions.appendChild(stopButton);
    }
    const open = document.createElement("button");
    open.type = "button";
    open.className = "ghost mini";
    open.textContent = line.process_id === activeProcessId ? t("当前对话") : t("打开对话");
    open.disabled = line.process_id === activeProcessId;
    open.dataset.focusKey = "open";
    open.addEventListener("click", async () => {
      await switchProcess(line.process_id);
      // 「打开对话」就是要去那条线的对话:线路页上切完不跳转 = 白切(UX-052 / D15)。
      ensureChatView();
      renderLines(collaborationLines);
    });
    actions.appendChild(open);
    // R-184 P5 收活五格:只有真实工作树上的线才有「收活」入口(主树没有可合并分支)。
    if (line.worktree_path) {
      const harvest = document.createElement("button");
      harvest.type = "button";
      harvest.className = "ghost mini line-harvest-toggle";
      harvest.textContent = t("合并收尾");
      harvest.disabled = lineRunning;
      harvest.title = lineRunning ? t("独立任务运行中，请先停止并等它收尾，才能合并收尾") : "";
      harvest.dataset.focusKey = "harvest";
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
    {
      const close = document.createElement("button");
      close.type = "button";
      close.className = "ghost mini danger line-close";
      close.textContent = t("关闭独立任务");
      close.dataset.focusKey = "close";
      close.addEventListener("click", () => void closeParallelProcess(line.process_id));
      actions.appendChild(close);
    }
    const footer = document.createElement("div");
    footer.className = "line-lane-footer";
    footer.append(changed, actions);
    lane.append(head, claim, facts, controls, runtimeDetails, footer);
    const preservedPanel = preservedHarvestPanels.get(line.process_id);
    if (preservedPanel) lane.appendChild(preservedPanel);
    target.appendChild(lane);
  }
  restoreLinesFocus(savedFocus);
  // R-247:协作快照与文档快照都来自同一 tracker 取得线；线路代号变化后同步重绘 badge。
  if ($("view-documents")?.classList.contains("active") && latestDocsSnapshot && typeof renderDocuments === "function") {
    renderDocuments(latestDocsSnapshot);
  }
}

// ---------- R-184 P5:收活五格(设计文档 §5) ----------
// ① 读报告 → ② 人读 diff → ③ 跑门禁 → ④ 合并 → ⑤ 回写 tracker。
// ② 不可跳过:未点「我已读过 diff」时 ③④ 全部禁用(⑤ 由批5 接入)。
// 格3 门禁由 kanzei 跑(worktree_gate:fmt/clippy/test/前端冒烟),不能信线自己说的绿。
export function forProject() {
  return currentProject;
}

export function harvestClaimId(value) {
  const match = String(value ?? "").trim().match(/^(R|D)-(\d+)(?:\s|$)/);
  return match ? `${match[1]}-${match[2]}` : "";
}

export function buildHarvestPanel(line, projectDir, agentCode) {
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
  title.textContent = `${t("合并收尾")} · ${line.label}${line.branch ? ` (${line.branch})` : ""}`;
  panel.appendChild(title);

  // 格1 读报告:线的事实区已经展示,这里把「要合什么」摆成一段可读清单。
  const report = document.createElement("div");
  report.className = "harvest-step";
  const reportHead = document.createElement("div");
  reportHead.className = "harvest-step-head";
  reportHead.innerHTML = `<span class="harvest-step-no">1</span><strong>${t("读报告")}</strong><span class="harvest-step-state ok">${t("已展示")}</span>`;
  const reportBody = document.createElement("div");
  reportBody.className = "harvest-step-body";
  reportBody.textContent = `${t(line.claim || "未声明条目")} · ${localizedStage(line.phase || "空闲")} · ${(line.changed_files ?? []).length} ${t("个文件")}`;
  const trackerPicker = document.createElement("label");
  trackerPicker.className = "harvest-tracker-picker";
  const trackerPickerText = document.createElement("span");
  trackerPickerText.textContent = t("要更新的条目");
  const trackerSelect = document.createElement("select");
  trackerSelect.className = "harvest-tracker-select";
  trackerSelect.disabled = true;
  trackerSelect.appendChild(new Option(t("读取独立任务对话中…"), ""));
  trackerPicker.append(trackerPickerText, trackerSelect);
  reportBody.appendChild(trackerPicker);
  report.append(reportHead, reportBody);
  panel.appendChild(report);

  // 格2 人读 diff:必须显式确认,是语义层唯一防线(设计文档 §5 ②)。
  const diffStep = document.createElement("div");
  diffStep.className = "harvest-step";
  const diffHead = document.createElement("div");
  diffHead.className = "harvest-step-head";
  diffHead.innerHTML = `<span class="harvest-step-no">2</span><strong>${t("查看改动")}</strong>`;
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
      // UI-0926 #10:增删计数与逐文件差异从 unified diff 解析(parseUnifiedDiff),
      // 不再全是 +0/−0;每个文件一个可折叠的着色 diff,原始文本仍保留在最后。
      const parsedDiff = parseUnifiedDiff(info.diff ?? "");
      const countsByPath = new Map(parsedDiff.map((file) => [file.path, file]));
      const treeFiles = (info.files ?? []).map((raw) => {
        const path = raw.replace(/^[MADRCU?! ]{2} /, "").trim();
        const counts = countsByPath.get(path);
        return { path, additions: counts?.additions ?? 0, deletions: counts?.deletions ?? 0 };
      });
      const diffPanel = document.createElement("div");
      diffPanel.className = "harvest-diff-tree";
      diffPanel.replaceChildren(typeof buildDiffTree === "function" ? buildDiffTree(treeFiles) : document.createTextNode(treeFiles.map((f) => f.path).join("\n")));
      const fileDiffs = document.createElement("div");
      fileDiffs.className = "sv-diff-files";
      for (const file of parsedDiff) {
        if (!file.lines.length) continue;
        const item = document.createElement("details");
        item.dataset.path = file.path;
        const head = document.createElement("summary");
        head.textContent = `${file.path}  +${file.additions} −${file.deletions}`;
        item.append(head, renderDiff(file, { compact: true }));
        fileDiffs.appendChild(item);
      }
      const rawDiff = info.diff ? `${t("差异")}:\n${info.diff}` : t("工作树干净,没有未提交差异");
      const rawPre = document.createElement("details");
      rawPre.className = "harvest-diff-raw";
      const rawSummary = document.createElement("summary");
      rawSummary.textContent = t("原始差异文本");
      const rawBody = document.createElement("pre");
      rawBody.className = "harvest-diff";
      rawBody.textContent = rawDiff;
      rawPre.append(rawSummary, rawBody);
      diffOutput.replaceChildren(diffPanel, ...(fileDiffs.children.length ? [fileDiffs] : []), rawPre);
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
  gateHead.innerHTML = `<span class="harvest-step-no">3</span><strong>${t("运行检查")}</strong>`;
  const gateBody = document.createElement("div");
  gateBody.className = "harvest-step-body";
  const gateButton = document.createElement("button");
  gateButton.type = "button";
  gateButton.className = "ghost mini harvest-gate-run";
  gateButton.disabled = true;
  gateButton.textContent = t("运行检查");
  const gateOutput = document.createElement("div");
  gateOutput.className = "harvest-gate-result";
  gateButton.addEventListener("click", async () => {
    harvestState.mergeGateRan = false;
    harvestState.mergeGatePassed = false;
    harvestState.mergeGateRunning = true;
    mergeButton.disabled = true;
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
        warn.textContent = t("检查未通过:请先在该独立任务里修复,再重新检查");
        gateOutput.appendChild(warn);
      } else {
        const pass = document.createElement("p");
        pass.className = "harvest-gate-pass";
        pass.textContent = t("检查通过");
        gateOutput.appendChild(pass);
        // R-222:检查通过才解锁合并(防线①:门禁是合并前置)。
        mergeButton.disabled = false;
      }
      gateButton.disabled = false;
      gateButton.textContent = t("重新检查");
    } catch (error) {
      gateButton.disabled = false;
      gateButton.textContent = t("运行检查");
      toastError(`${t("检查执行失败")}:${error}`);
    } finally {
      harvestState.mergeGateRunning = false;
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
    if (harvestState.mergeGateRunning) return;
    // R-222 防线①:合并前置门禁——状态来自 JS 对象，dataset 只用于展示/调试。
    const gateOk = harvestState.mergeGatePassed;
    const gateRan = harvestState.mergeGateRan;
    if (!gateRan || !gateOk) {
      const reason = gateRan ? t("检查未通过") : t("检查未运行");
      const ok = await confirmDialog({
        title: t("覆盖确认"),
        message: `${reason}。${t("合并前请先在该独立任务里跑通检查;仍要继续合并吗")}\n${t("覆盖确认将记录到活动轨迹")}`,
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
  postMergeHead.innerHTML = `<span class="harvest-step-no">5</span><strong>${t("合并后全量检查")}</strong>`;
  const postMergeBody = document.createElement("div");
  postMergeBody.className = "harvest-step-body";
  const postMergeButton = document.createElement("button");
  postMergeButton.type = "button";
  postMergeButton.className = "ghost mini harvest-postmerge-run";
  postMergeButton.disabled = true;
  postMergeButton.textContent = t("合并后全量检查");
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
        warn.textContent = t("合并后全量检查未通过:请先修复主项目,再重新检查");
        postMergeOutput.appendChild(warn);
        postMergeButton.disabled = false;
        postMergeButton.textContent = t("重新运行合并后全量检查");
      } else {
        const pass = document.createElement("p");
        pass.className = "harvest-gate-pass";
        pass.textContent = t("合并后全量检查通过");
        postMergeOutput.appendChild(pass);
        harvestState.postMergeGatePassed = true;
        postMergeStep.classList.add("confirmed");
        const stateTag = document.createElement("span");
        stateTag.className = "harvest-step-state ok";
        stateTag.textContent = t("已通过");
        postMergeHead.appendChild(stateTag);
        postMergeButton.textContent = t("已通过");
        // 合并后全量检查通过才解锁格5 回写。
        syncWritebackAvailability();
      }
    } catch (error) {
      postMergeButton.disabled = false;
      postMergeButton.textContent = t("重新运行合并后全量检查");
      toastError(`${t("合并后全量检查执行失败")}:${error}`);
    }
  });
  postMergeBody.append(postMergeButton, postMergeOutput);
  postMergeStep.append(postMergeHead, postMergeBody);
  panel.appendChild(postMergeStep);

  // 格6 回写 tracker:合并完成后把线交付落主根一份(设计文档 §5 ⑤)。
  // 只追加进展不改状态;claim 不是条目 ID 时后端拒绝,不让用户误以为已登记。
  // 合并后全量检查通过前禁用——R-222:回写以合并后全量绿为前置。
  const writebackStep = document.createElement("div");
  writebackStep.className = "harvest-step";
  const writebackHead = document.createElement("div");
  writebackHead.className = "harvest-step-head";
  writebackHead.innerHTML = `<span class="harvest-step-no">6</span><strong>${t("更新需求记录")}</strong>`;
  const writebackBody = document.createElement("div");
  writebackBody.className = "harvest-step-body";
  const writebackHint = document.createElement("p");
  writebackHint.className = "harvest-writeback-hint";
  writebackHint.textContent = t("合并完成后可更新");
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
    writebackButton.textContent = t("更新中…");
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
      writebackButton.textContent = t("已记录");
      writebackOutput.textContent = result;
      writebackOutput.hidden = false;
      const stateTag = document.createElement("span");
      stateTag.className = "harvest-step-state ok";
      stateTag.textContent = t("已记录");
      writebackHead.appendChild(stateTag);
      refreshLines();
      refreshWorktrees();
      refreshGit();
    } catch (error) {
      writebackButton.disabled = false;
      writebackButton.textContent = t("重试更新");
      toastError(`${t("更新失败")}:${error}`);
    }
  });
  writebackBody.append(writebackHint, writebackButton, writebackOutput);
  writebackStep.append(writebackHead, writebackBody);
  panel.appendChild(writebackStep);

  function syncWritebackAvailability() {
    // R-222:回写需 合并完成 + 合并后全量检查通过 双前置(防线②)。
    if (!harvestState.mergeCompleted) return;
    const postMergeOk = harvestState.postMergeGatePassed;
    if (!postMergeOk) {
      writebackButton.disabled = true;
      writebackButton.textContent = t("需先运行合并后全量检查");
      writebackHint.textContent = t("合并后全量检查通过后才能更新需求记录");
      writebackHint.classList.add("warn-text");
      return;
    }
    if (trackerClaim) {
      writebackButton.disabled = false;
      writebackButton.textContent = t("更新需求记录");
      writebackHint.textContent = `${trackerClaim} · ${t("由")} ${lineName(line)} ${t("交付")}`;
      writebackHint.classList.remove("warn-text");
    } else {
      writebackButton.disabled = true;
      writebackButton.textContent = t("无有效条目");
      writebackHint.textContent = t("这个独立任务的对话里没有可确认的进行中条目(R-xxx / D-xxx)，合并已完成；请让主对话手动登记这次交付");
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
        trackerSelect.appendChild(new Option(t("没有从这个独立任务的对话里找到进行中的条目"), ""));
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
      trackerSelect.replaceChildren(new Option(t("独立任务对话条目读取失败"), ""));
      trackerSelect.disabled = true;
      trackerClaim = "";
      syncWritebackAvailability();
      log(`${t("独立任务对话条目读取失败")}:${error}`, "warn");
    }
  })();

  return panel;
}

// UI2-0926 从别处跳进线路页、要落在页内某一段时的落点(侧栏「查看全部隔离工作树 →」)。线路卡由 refreshLines
// 异步画出:切视图后立刻 scrollIntoView,随后插进来的线路卡会把目标段往下推,停在半路(1280@1.5 实测只露出
// 标题和一行)。所以切进来时先记下落点,等本轮刷新收尾再滚——成功失败都滚,目标段(工作树清单)不靠这次 IPC;
// 本来就在线路页时切视图不触发刷新,就地滚。离开线路页后落点作废,不会在下次进来时突然跳。
let pendingLinesAnchor = null;
export function revealLinesSection(id, { afterRefresh = true } = {}) {
  if (!$("view-lines")?.classList.contains("active")) return;
  if (afterRefresh) pendingLinesAnchor = id;
  else $(id)?.scrollIntoView?.({ block: "start" });
}
function consumeLinesAnchor() {
  const id = pendingLinesAnchor;
  pendingLinesAnchor = null;
  if (id && $("view-lines")?.classList.contains("active")) $(id)?.scrollIntoView?.({ block: "start" });
}

export async function refreshLines() {
  if (!currentProject) {
    renderLines([]);
    consumeLinesAnchor();
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
    const catalog = loadLinesModelCatalog();
    if (currentProject !== forProject) return;
    const lines = await invoke("collaboration_snapshot", { projectDir: forProject });
    if (currentProject !== forProject) return;
    renderLines(lines);
    // 模型目录晚到:renderLines 的签名含目录长度,目录一到就按新选项重画;用户正停在下拉上时它自己会推迟。
    void catalog.then(() => {
      if (currentProject === forProject && $("view-lines")?.classList.contains("active")) renderLines(collaborationLines);
    });
  } catch (error) {
    if (currentProject === forProject) {
      log(`${t("并行线路读取失败")}:${error}`, "warn");
      $("lines-summary").textContent = `${t("并行线路读取失败")}:${error}`;
    }
  } finally {
    linesRefreshInFlight = false;
    consumeLinesAnchor();
    if (linesRefreshQueued) {
      linesRefreshQueued = false;
      scheduleLinesRefresh(250);
    } else {
      const running = collaborationLines.some((line) => line.running);
      scheduleLinesRefresh(running ? LINES_REFRESH_RUNNING_MS : LINES_REFRESH_IDLE_MS);
    }
  }
}

export function scheduleLinesRefresh(delay) {
  if (linesRefreshTimer) clearTimeout(linesRefreshTimer);
  linesRefreshTimer = setTimeout(() => {
    linesRefreshTimer = null;
    if ($("view-lines").classList.contains("active")) void refreshLines();
  }, delay);
}

export async function confirmWorktreeMerge(item, forProject) {
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
      title: t("检测到独立任务之间的文件重叠"),
      message: `${conflictNote}${t("检测到独立任务之间的文件重叠")}:${count} ${t("项")}\n${t("这只比对了改动的文件名,逻辑上是否冲突要读 diff 判断")}\n${t("仍要继续进入 Git 合并吗")}`,
      okText: t("确认"),
      danger: true,
    });
  }
  return confirmDialog({
    title: t("继续进入 Git 合并吗"),
    message: `${conflictNote}${t("当前未发现独立任务之间的文件重叠")}。${t("这只比对了改动的文件名,逻辑上是否冲突要读 diff 判断")}。\n${t("继续进入 Git 合并吗")}`,
    okText: t("确认"),
  });
}

// 侧栏的 ↻ 撤掉之后,这一颗要同时刷线路与工作树清单——否则孤儿树没有任何刷新入口。
defer(() => {
  $("lines-refresh").addEventListener("click", () => { void refreshLines(); void refreshWorktrees(); });
});
defer(() => {
  $("lines-work-item").addEventListener("change", () => renderLineWorkItemOptions());
});
defer(() => {
  $("lines-add").addEventListener("click", createWorktreeLine);
});
// 会话相位一变(点了「停止」→ 停止中 → 收到终态)线路卡立刻跟上,不等下一轮 3.5s / 8s 的轮询:
// 视图签名里带着「是否停止中」,没变化时 renderLines 不会动 DOM。只在线路页开着时重画。
defer(() => {
  document.addEventListener("kz:session-state-changed", () => {
    if ($("view-lines")?.classList.contains("active") && collaborationLines.length) renderLines(collaborationLines);
  });
});
// 用户从线路卡里的下拉/输入框离开后,补一次被推迟的重绘(为什么推迟见 renderLines 前面的 linesBusyControl)。
// 等一个宏任务:focusout 时 document.activeElement 还没落到新位置,立刻判会误以为还停在原控件上。
defer(() => {
  $("lines-list").addEventListener("focusout", () => {
    if (!pendingLinesRender) return;
    setTimeout(() => {
      if (pendingLinesRender) renderLines(pendingLinesRender);
    }, 0);
  });
});
