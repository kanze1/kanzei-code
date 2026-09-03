import { defer } from "./01-core.js";
import { $, invoke, uiPrefsLoad } from "./01-core.js";
import { localizeDynamic, t } from "./02-i18n.js";
import {
  activeSessionId,
  currentProject,
  log,
  processItems,
  running,
  sessionState,
  setRunning,
  toast,
  transitionSession,
} from "./03-shell.js";
import { __kzProcessAutoState, normalizeAutoState, processAutoState } from "./08-compose-runtime.js";
import { state } from "./08-compose.js";
import { refreshProcesses } from "./09-sessions.js";

// ---------- 自动续跑状态与渲染 ----------
// 鞭挞状态:自动续跑轮次(手动发送归零);旧 maxRounds 仅保留兼容读取。
export const DEFAULT_AUTO_CONTINUE_MAX = 10;
export let autoRounds = 0;
export let autoPaused = false;

// D-504:轮次真源是会话状态；autoRounds 只保留为当前活动线的渲染镜像。
export function currentAutoRounds(sessionId = activeSessionId) {
  return Number(sessionId ? sessionState(sessionId)?.auto_rounds ?? 0 : 0) || 0;
}
export function setAutoRounds(sessionId, value) {
  const rounds = Number(value) || 0;
  if (sessionId) sessionState(sessionId).auto_rounds = rounds;
  if (!sessionId || sessionId === activeSessionId) autoRounds = rounds;
  return rounds;
}
export let autoStopAfterRound = false;
export const autoContinueTimers = new Map();
// 自动续跑的 IPC 会在后端真正结束前立即返回；用在途集合挡住同一会话的
// 重复 kz:done/定时器事件，终态事件再释放。否则重复事件会不断追加 queue 输入。
export const autoContinueInFlight = new Set();
export let autoStopReason = "";
// 连续无实质动作的轮数:第一次只追加推进指令,第二次才刹车。
// R-169:判定已下沉 harness auto_run 状态机,前端只保留镜像赋值。
export let noActionRounds = 0;
export function setAutoPaused(value) { autoPaused = Boolean(value); return autoPaused; }
export function setAutoStopAfterRound(value) { autoStopAfterRound = Boolean(value); return autoStopAfterRound; }
export function setNoActionRounds(value) { noActionRounds = Number(value) || 0; return noActionRounds; }
export function setAutoHint(value) { autoHint = String(value ?? ""); return autoHint; }
// R-264 B3：classic runtime 只提供状态访问器；测试钩子本身由 08-compose.js
// 以 ESM export 暴露。provider 不暴露可写变量，避免 smoke 重新依赖共享词法作用域。
export const __kzAutoTestState = {
  rounds: () => autoRounds,
  noAction: () => noActionRounds,
  stopReason: () => autoStopReason,
  timerSessions: () => [...autoContinueTimers.keys()],
  retryLabel: (id) => autoContinueTimers.get(id)?.retryLabel ?? null,
  setAutoState: (id, value) => globalThis.__kzProcessAutoState?.set(id, value),
  getAutoState: (id) => globalThis.__kzProcessAutoState?.get(id),
  setRounds: (value) => { setAutoRounds(activeSessionId, value); },
  setStopAfterRound: (value) => { autoStopAfterRound = value; },
  setPaused: (value) => { autoPaused = value; },
  paused: () => autoPaused,
  reset: () => {
    autoRounds = 0;
    noActionRounds = 0;
    autoStopAfterRound = false;
    autoPaused = false;
  },
  cancelTimers: () => {
    for (const sessionId of [...autoContinueTimers.keys()]) cancelAutoContinueTimer(sessionId);
  },
};
globalThis.__kzAutoTestState = __kzAutoTestState;
// R-170:继续文案降级为用户意图载体(方案 A,评估结论 continue_prompt_dissection.md §5)。
// 引擎规则(取活/批次/阻塞/验收/节奏)全部归 system prompt 与 harness 状态机,
// 文案只保留极简意图句;textarea 承载用户附加意图,删空回落此默认。
export const DEFAULT_CONTINUE_PROMPT = "继续推进，规则按系统提示执行。";

// R-169:NUDGE 文案生成已下沉 harness(nudge_prompt),前端不再持有模板。
// 无动作判定与推进指令由引擎给出,前端只负责在收到 Nudge 动作时发送。

export function selectedAgent() {
  const mode = $("profile-select").value;
  if (mode === "dev-pair") return { profile: "dev", agent: "dev-pair" };
  if (mode === "dev-auto") return { profile: "dev", agent: "dev" };
  return { profile: "research", agent: "research" };
}

// R-322:门禁强度的**唯一真源是后端** `intensity_for_agent`(crates/kanzei-app/src/
// auto_run.rs)。这里只是回显,判据必须与那边逐条一致——两处漂开就会出现「界面说
// 结伴、引擎按自主跑」,而这正是本条目要消除的那类不可见错位。改一边必须改另一边。
export function harnessIntensityOf(agentName) {
  return agentName === "dev" ? "autonomous" : "paired";
}

// 逐条列出这一档下引擎会不会插手。用户抱怨的是「区别不够明显」,所以不能只写
// 「轻/重」两个字——要把具体让渡了什么写出来。
export function renderHarnessIntensity() {
  // R-342:模式芯片的配色搭同一趟车。它与门禁强度是同一件事的两个面：
  // 芯片告诉你现在是哪个人格,强度告诉你这个人格下引擎会插手到什么程度。
  // 不另起监听器:三处调用点(冷启动/进程回显/用户切换)已经覆盖全,多挂一个只会多一处漏调。
  const chip = $("profile-select");
  if (chip) chip.dataset.mode = chip.value;
  const badge = $("harness-intensity-badge");
  const desc = $("harness-intensity-desc");
  if (!badge || !desc) return;
  const intensity = harnessIntensityOf(selectedAgent().agent);
  badge.dataset.intensity = intensity;
  if (intensity === "autonomous") {
    badge.textContent = t("重");
    desc.textContent = t("无人监督:引擎会追加推进指令、插入验收核查轮、标注冗余调用");
  } else {
    badge.textContent = t("轻");
    desc.textContent = t("有人监督:引擎不推进、不插核查轮、不标冗余;模型说完成即停");
  }
}
export function workPriorityStorageKey() {
  return `kz-work-priority:${currentProject || "default"}`;
}
export function selectedWorkPriority() {
  return $("work-priority-select").value === "requirement-first" ? "requirement-first" : "defect-first";
}
export function syncWorkPriorityControl() {
  const saved = localStorage.getItem(workPriorityStorageKey());
  $("work-priority-select").value = saved === "requirement-first" ? saved : "defect-first";
  // D-404:localStorage 数据文件缺失时重启即丢;后端 app.json 是权威,有值则覆盖。
  void uiPrefsLoad().then((p) => {
    const v = p.work_priority?.[currentProject || "default"];
    if (v === "requirement-first" || v === "defect-first") $("work-priority-select").value = v;
  });
}

// 取活序只有一个真源:这个开关 → localStorage → work_priority → 引擎的
// resolve_work_decision → <resolved-control-state>。
//
// 这里原先还把开关镜像成一条 preference 记忆(开发重心),理由写着「提示词由记忆
// 生成,所以开关与提示词不可能再互相矛盾」。那个理由在当时成立,后来不成立了:
// 权威提示词现在由 run.rs work_priority_guidance 从**枚举**生成,而 preference
// 记忆走的是另一条路——它以「STANDING DIRECTIVES(obey these; they are the
// user's own words)」的抬头全文常驻注入,与 <resolved-control-state> 里那句
// 「do not re-arbitrate queue priority from tracker prose」正面对撞。
//
// 于是模型每轮同时收到两条都自称最高优先级的指令。而那条记忆改不动队列顺序
// (引擎只读枚举),它唯一能起的作用是怂恿模型借 WorkInput.reason 偏离引擎裁决,
// 并把一条过期理由写进审计记录。实测后果:同一条规则被反复复活三代
// (M-002 → M-063 → M-070),每次退役后开关一切就再生一条。
//
// 所以镜像写入去掉,开关只写自己那份枚举。回显由上面 syncWorkPriorityControl
// 从 localStorage 做——它本来就在做,记忆那份是会覆盖它的第二个回显源。

// 鞭挞状态**槽位化**:轮次(数)/阶段(状态机)/原因(为什么停)各占各的元素。
// 旧实现把三样揉进一条自由文本塞进 #auto-status,而 renderAutoStatus 的默认参数是
// autoStopReason——于是任何无参重绘(renderProcesses → renderAutoStatus())都等于
// 「把 DOM 回写成上一次的停止原因」。实测链路:kz:done 先 setAutoStopReason("本轮完成"),
// 再写「3/34 · 等待下一轮」,紧接着 kz:idle → renderProcesses → 无参重绘,轮次在下一帧
// 就被抹回「本轮完成」。所以「跑到第几轮」这条最该一眼可见的信息,真跑起来时看不见。
export let autoHint = "";
export const AUTO_PHASE_LABEL = {
  off: "鞭挞已关闭", running: "推进中", pending: "等待下一轮", paused: "已暂停", idle: "待命",
};
export function autoRunPhase() {
  if (!$("auto-continue")?.checked) return "off";
  const state = activeSessionId ? sessionState(activeSessionId) : null;
  if (state && ["starting", "running"].includes(state.phase)) return "running";
  if (state && state.phase === "auto_pending") return "pending";
  if (autoPaused) return "paused";
  return "idle";
}
export function renderAutoRun() {
  const bar = $("autorun-bar");
  if (!bar) return;
  autoRounds = currentAutoRounds();
  const phase = autoRunPhase();
  const armed = $("auto-continue")?.checked === true;
  bar.dataset.phase = phase;
  bar.classList.toggle("armed", armed);
  $("auto-round-now").textContent = String(autoRounds);
  const progress = $("auto-progress");
  progress.style.removeProperty("--auto-progress");
  progress.setAttribute("aria-label", `${t("鞭挞轮次")} ${autoRounds}`);
  $("auto-phase").textContent = t(AUTO_PHASE_LABEL[phase]);
  // 原因槽:一次性提示优先(无动作/验收核查/未续跑),否则显示停机原因;推进中不显示。
  const reason = autoHint || (["off", "idle", "paused"].includes(phase) ? autoStopReason : "");
  const el = $("auto-status");
  el.textContent = reason ? localizeDynamic(reason) : "";
  el.classList.toggle("hidden", !reason);
  el.classList.toggle("ok", /已清空|全部被阻塞/.test(reason));
  $("auto-resume").classList.toggle("hidden", !(autoStopReason && !autoHint && ["off", "idle"].includes(phase)));
  const pause = $("auto-pause");
  if (pause) pause.setAttribute("aria-pressed", String(autoPaused));
}
// 兼容既有 9 个调用点:带参 = 写一次性提示槽,无参 = 纯重绘(不再回写原因)。
export function renderAutoStatus(text) {
  if (text !== undefined) autoHint = text;
  renderAutoRun();
}
// R-170:继续文案 = 用户附加意图 + 极简默认兜底。开发重心/引擎规则已由
// run.rs work_priority_guidance + memory preference 注入 system prompt,不再拼接。
export function continuePrompt() {
  return $("continue-prompt").value.trim() || DEFAULT_CONTINUE_PROMPT;
}

// 「停止中…」是个只能靠后端终态事件走出去的状态:发送禁用、停止禁用、
// 轮询也不复位(09-sessions.js 只在 phase 属于 starting/running 时才收敛)。
// 事件桥断开、后端异常退出或事件丢失(D-005 那类)之后按一次停止,这条线就
// 永久焊死——重启应用才能用,而用户多半只会以为「还在收尾」而一直等下去。
// 给它一个看门狗:超时未收到确认就自行落到停止态,并把「没收到确认」说出来,
// 而不是假装什么都没发生。
export const STOPPING_WATCHDOG_MS = 10000;
export const stoppingWatchdogs = new Map();
export function armStoppingWatchdog(sessionId) {
  if (!sessionId || typeof setTimeout !== "function") return;
  clearStoppingWatchdog(sessionId);
  stoppingWatchdogs.set(sessionId, setTimeout(() => {
    stoppingWatchdogs.delete(sessionId);
    if (sessionState(sessionId).phase !== "stopping") return;
    transitionSession(sessionId, "stopped");
    if (sessionId === activeSessionId) setRunning(false, t("已停止"));
    log(t("停止已发出但未收到后端确认,已按停止处理"), "warn");
    toast(t("停止已发出但未收到后端确认,已按停止处理"));
    if (typeof refreshProcesses === "function") refreshProcesses();
  }, STOPPING_WATCHDOG_MS));
}
export function clearStoppingWatchdog(sessionId) {
  const timer = stoppingWatchdogs.get(sessionId);
  if (timer === undefined) return;
  clearTimeout(timer);
  stoppingWatchdogs.delete(sessionId);
}

/// 清掉鞭挞控制台里两条**跨线路/跨项目会串台**的文本槽。切线在 applyAutoUiState
/// 里顺手做了,切项目走 09-sessions.js enterProject 调这里。
export function clearAutoNotices() {
  autoHint = "";
  autoStopReason = "";
  renderAutoRun();
}

export function setAutoStopReason(reason) {
  autoStopReason = reason;
  autoHint = "";
  renderAutoRun();
}
// R-322 B2:dev 两档都能续跑,区别在门禁强度不在能不能跑;research 仍拒绝。
// 判据必须与后端 coordinator.rs 的 auto_allowed 一致(profile 级,不看 agent)。
export function autoContinueAllowed() {
  return $("profile-select").value !== "research";
}
// 兼容旧状态/配置的读取接口。`auto_max` 仍可被旧前端状态携带,但不再是停止条件或界面设置。
export function autoContinueMax() {
  const stored = Number.parseInt(localStorage.getItem("kz-auto-max"), 10);
  return Number.isFinite(stored) ? Math.min(100, Math.max(1, stored)) : DEFAULT_AUTO_CONTINUE_MAX;
}
// R-322 B3:目标条件(Claude Code /goal 的形状)。真源是后端 AutoRunController.goal;
// 输入框只是它的编辑入口,每次同步整串发过去,空串即撤销。
export function currentGoalText() {
  return $("auto-goal")?.value ?? "";
}
// 目标是**一次性意图**:达成或判定不可达后后端已清除,前端同步清空输入框,
// 否则下一段无关对话会被上一个目标继续驱动(D-111 同型教训)。
export function clearGoalInput() {
  const box = $("auto-goal");
  if (!box) return;
  box.value = "";
  renderGoalState();
  void syncAutoRunState();
}
export function renderGoalState() {
  const hint = $("auto-goal-state");
  if (!hint) return;
  const goal = currentGoalText().trim();
  hint.textContent = goal
    ? t("目标挂着:模型判定达成前不停;连续推不动会自动停")
    : t("留空 = 按有无实质动作决定是否继续");
}
export function syncAutoRunState() {
  if (!activeSessionId) return;
  return invoke("auto_state_update", {
    sessionId: activeSessionId,
    enabled: $("auto-continue").checked,
    paused: autoPaused,
    stopAfterRound: autoStopAfterRound,
    goal: currentGoalText(),
  });
}
export function resetAutoRunState() {
  if (activeSessionId) void invoke("auto_state_reset", { sessionId: activeSessionId });
}
export function cancelAutoContinueTimer(sessionId = activeSessionId) {
  if (!sessionId) return;
  const entry = autoContinueTimers.get(sessionId);
  if (entry?.timer) clearTimeout(entry.timer);
  autoContinueTimers.delete(sessionId);
}
export function releaseAutoContinue(sessionId) {
  if (sessionId) autoContinueInFlight.delete(sessionId);
}
// D-291:续跑闸门的**唯一**判据。原来这几个条件散在两处 setTimeout 里,任一不满足
// 就 `return` ——不发下一轮、不清 auto_pending、不清横幅、不留一个字。界面于是永久
// 钉在「鞭挞 · 等待下一轮」,而那一轮永远不会来(引擎侧还记着 rounds+1,两边状态从此
// 对不上)。闸门必须集中且**开口说话**:不续跑可以,不说为什么不行(D-004 口径)。
export function autoContinueBlockedReason(sessionId) {
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  if (!item) return "线路已关闭";
  if (sessionId === activeSessionId) {
    if (!$("auto-continue").checked) return "鞭挞已关闭";
    if (autoPaused) return "已暂停";
    if (autoStopAfterRound) return "本轮后停";
    return null;
  }
  const config = normalizeAutoState(processAutoState.get(item.id), item.id);
  if (!config.enabled) return "鞭挞已关闭";
  if (config.paused) return "已暂停";
  if (config.stopAfterRound) return "本轮后停";
  return null;
}
