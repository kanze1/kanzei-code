import { closeSurface, isModalOpen, isSurfaceOpen, openPopover } from "./00-surface.js";
import { openGeneralChat } from "./03-general-scope.js";
import { defer } from "./01-core.js";
import { attachmentMime, resourceIcon, resourceLinks, resourceType } from "./04-resource-types.js";
import { setCurrentAssistant, setCurrentReasoning } from "./03-shell.js";
import { setCtxTokens, setRunTokens } from "./03-shell.js";
import { $, invoke, isImeComposing, promptBox, readJson, uiPrefsCache, uiPrefsLoad, uiPrefsSave, writeJson } from "./01-core.js";
import { localizeDynamic, t } from "./02-i18n.js";
import {
  activeProcessId,
  activeSessionId,
  attachments,
  setAttachments,
  clearRunPending,
  ctxTokens,
  currentAssistant,
  currentProject,
  currentReasoning,
  ensureChatView,
  ensureNotificationPermission,
  lastRequest,
  setLastRequest,
  log,
  processItems,
  renderTokens,
  reportPersistentError,
  runControlPending,
  runTokens,
  running,
  sessionState,
  setRunning,
  setStatus,
  setStopping,
  startElapsed,
  stopElapsed,
  toast,
  toastError,
  transitionSession,
} from "./03-shell.js";
import { addMessage, addUserMessage, outputChars, reportError } from "./05-chat-render.js";
import { setOutputChars } from "./05-chat-render.js";
import { hideAsk } from "./07-events.js";
import { tasksPanelUserRun } from "./06-agent-panel.js";
import {
  DEFAULT_AUTO_CONTINUE_MAX,
  DEFAULT_CONTINUE_PROMPT,
  armStoppingWatchdog,
  autoContinueBlockedReason,
  autoContinueInFlight,
  autoContinueMax,
  autoContinueTimers,
  autoHint,
  autoPaused,
  autoRounds,
  autoStopAfterRound,
  autoStopReason,
  awaitingUserSessions,
  cancelAutoContinueTimer,
  continuePrompt,
  currentAutoRounds,
  currentGoalText,
  lineAgent,
  markAwaitingUser,
  noActionRounds,
  releaseAutoContinue,
  renderAutoStatus,
  renderGoalState,
  renderHarnessIntensity,
  rememberWorkPrioritySelection,
  resetAutoRunState,
  selectedAgent,
  selectedWorkPriority,
  setAutoHint,
  setAutoPaused,
  setAutoRounds,
  setAutoStopAfterRound,
  setAutoStopReason,
  setNoActionRounds,
  syncAutoRunState,
  takeAwaitingUser,
  workPriorityKeyFor,
} from "./08-auto.js";
import { state } from "./08-compose.js";
import { processRunning, refreshParallelTaskProjection, refreshPendingInputs } from "./09-sessions.js";
import { syncResearchWorkspaceVisibility } from "./19-research.js";
import { collaborationLines, renderLines } from "./20-lines.js";
import { sync_workspace_visibility, acknowledge_composer_draft, ensure_workspace_process } from "./03-workspaces.js";
import { autoAllowEnabled } from "./03-layout.js";

// `running` 与上面四条不同:它是**瞬态**,不是用户意图。kz:done 有意不收回运行态
// (07-events.js:328「真正收回由 kz:idle/kz:stopped 负责」),所以 2 秒到点时上一轮
// 可能仍标着运行中——旧代码在这里静默放弃,一轮就此永远不来。正确语义是等它落地,
// 但要有头:等满 AUTO_CONTINUE_RUNNING_GRACE 次还在跑,就当卡住了,报出来。
export const AUTO_CONTINUE_RUNNING_GRACE = 15;
// 用户自己按下的刹车(关自动推进/暂停/本轮后停)不需要额外提示——界面上那个开关就是
// 解释。其余原因是**意外停摆**:线路被关掉、上一轮卡住不结束。后台线出这两种时
// 原实现只 log() 一行,而日志面板默认收起——用户看到的就是「并行跑着跑着没消息了」,
// 没有任何可见解释(用户 2026-08-16 报告)。这类原因必须浮到界面上。
export const AUTO_CONTINUE_INTENDED_STOPS = new Set(["自动推进已关闭", "已暂停", "本轮后停"]);
// 闸门拦下时收口:pending 必须落地,否则横幅与线路徽标一直显示「等待下一轮」。
export function abortAutoContinue(reason, sessionId = activeSessionId) {
  releaseAutoContinue(sessionId);
  if (sessionId) transitionSession(sessionId, "idle");
  const item = sessionId ? processItems.find((candidate) => candidate.session_id === sessionId) : null;
  if (sessionId === activeSessionId) {
    clearRunPending();
    renderAutoStatus(`${t("自动推进未续跑")}:${t(reason)}`);
  } else if (!AUTO_CONTINUE_INTENDED_STOPS.has(reason)) {
    reportPersistentError(`${item?.label ?? t("对话")} ${t("自动推进未续跑")}:${t(reason)}`);
  }
  log(`${t("自动推进未续跑")}:${t(reason)}`);
  if (sessionId) refreshParallelTaskProjection(sessionId);
}
// 续跑定时器:闸门在**触发时刻**复查(2 秒内用户可能暂停/切模式/新一轮已开跑)。
// generation 不符属于「被更新的一枪取代」,静默是对的——但那条路径的 pending
// 由取消方自己收口,不在这里处理。
// retryLabel:这一枪是 D-403 的失败退避重试(带展示文案),不是正常续跑。标记必须
// 跟着**定时器条目**走,因为终态错误处理器要据此放它一条生路(见 07-events.js kz:error)。
export function armAutoContinue(prompt, sessionId = activeSessionId, waited = 0, delayMs = 2000, retryLabel = null) {
  if (!sessionId) return;
  if (sessionState(sessionId).runtime_managed) { releaseAutoContinue(sessionId); return; }
  // 在飞 = 这条线的上一枪已经发出但还没收到它的终点事件。此时排下一枪会重复发送,
  // 所以返回是对的;但**不能静默**——标记漏释放时(后台线曾经就是)整条自动推进永久停摆,
  // 而界面钉在「等待下一轮」,日志里连一行线索都没有。
  if (autoContinueInFlight.has(sessionId)) {
    log(`${processItems.find((candidate) => candidate.session_id === sessionId)?.label ?? ""} ${t("自动推进未续跑")}:${t("上一枪仍在飞")}`.trim(), "warn");
    return;
  }
  // R-199:档位条件下沉引擎——armAutoContinue 不再检查 autoContinueAllowed(),
  // 引擎在 decide() 已判 Stop(ProfileMismatch) 且计数不 +1;前端不再持有
  // 引擎不知道的续跑否决权(计数与实际轮次不再漂移)。
  cancelAutoContinueTimer(sessionId);
  const generation = (sessionState(sessionId).auto_generation || 0) + 1;
  sessionState(sessionId).auto_generation = generation;
  const timer = setTimeout(async () => {
    const current = autoContinueTimers.get(sessionId);
    if (!current || current.generation !== generation) return;
    autoContinueTimers.delete(sessionId);
    const blocked = autoContinueBlockedReason(sessionId);
    if (blocked) {
      abortAutoContinue(blocked, sessionId);
      return;
    }
    const item = processItems.find((candidate) => candidate.session_id === sessionId);
    if (item && processRunning(item)) {
      if (waited < AUTO_CONTINUE_RUNNING_GRACE) {
        armAutoContinue(prompt, sessionId, waited + 1, 2000, retryLabel);
        return;
      }
      if (item.running) {
        // 后端也说在跑 = 上一轮真没结束,放弃是对的。
        abortAutoContinue("上一轮尚未结束", sessionId);
        return;
      }
      // 宽限耗尽、但**后端权威说没在跑**:那就是本地状态机被某条路径卡在了运行态,
      // 不是上一轮没结束。原实现在这里一律放弃,于是任何一次本地态卡死都升级成
      // 自动推进永久停摆——auto_pending 不收敛那个 bug 正是这么烧掉 32 秒的。
      // 后端是运行态权威(R-086):按它收敛本地态再继续,让这一类错误自愈而不是致命。
      log(`${t("自动推进")}:${t("本地运行态与后端不符,按后端空闲继续")}`, "warn");
      transitionSession(sessionId, "idle");
    }
    transitionSession(sessionId, "starting", { local_start_pending: true });
    if (sessionId === activeSessionId) clearRunPending();
    await sendAutoToSession(prompt, sessionId);
  }, delayMs);
  autoContinueTimers.set(sessionId, { timer, generation, retryLabel });
}
export function scheduleAutoContinue() {
  armAutoContinue(continuePrompt());
}

export async function sendAutoToSession(prompt, sessionId) {
  if (autoContinueInFlight.has(sessionId)) return;
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  if (!item) return abortAutoContinue("对话已关闭", sessionId);
  const research = item.profile === "research";
  autoContinueInFlight.add(sessionId);
  if (sessionId === activeSessionId) {
    addMessage("notice", `${t("自动推进已触发")} · ${sessionState(sessionId).auto_rounds || 0}`);
    setRunning(true, t("准备中"));
  }
  try {
    // UI2-0926 #13:① 按本线实际档位发(结伴线的续跑轮真按结伴档跑);② 项目根用 origin_project
    // (主根身份,后端已给 simplify 形态,这里再防一道旧形态);③ 取活顺序键与写入键同一个函数。
    const mode = lineAgent(item);
    const projectDir = String(item.origin_project || item.project_dir || currentProject).replace(/^\\\\\?\\(?!UNC\\)/, "");
    const workItemId = mode.agent === "dev" ? await (await import("./31-work-selection.js")).validateSelectedWork(projectDir, item.id) : null;
    const priority = uiPrefsCache?.work_priority?.[projectDir] ?? localStorage.getItem(workPriorityKeyFor(projectDir));
    await invoke("run_prompt", {
      prompt,
      projectDir,
      profile: mode.profile,
      agent: mode.agent,
      researchTopic: research ? item.research_topic : undefined,
      model: item.model || null,
      workPriority: priority === "requirement-first" ? "requirement-first" : "defect-first",
      delivery: "queue",
      attachments: [],
      processId: item.id,
      autonomous: true,
      ...(workItemId ? { workItemId, executionBatch: true } : {}),
      autoAllow: autoAllowEnabled(),
    });
    if (workItemId) {
      const selection = await import("./31-work-selection.js");
      if (selection.selectedWork(projectDir, item.id) === workItemId) selection.selectWork(projectDir, item.id, null);
    }
  } catch (error) {
    releaseAutoContinue(sessionId);
    transitionSession(sessionId, "failed");
    if (sessionId === activeSessionId) {
      reportError(String(error));
      setRunning(false, t("出错"));
    } else {
      reportPersistentError(`${item.label} ${t("自动推进续跑失败")}:${error}`);
    }
    refreshParallelTaskProjection(sessionId);
  }
}

export function handleBackgroundSessionDone(payload) {
  const sessionId = payload?.sessionId;
  if (!sessionId) return;
  // 在飞标记必须在这里释放。活动线走 07-events 的 kz:done/kz:idle 处理器释放,而后台线
  // 的控制事件在 01-core 路由层就被拦下(kz:done 只转到本函数,kz:idle 直接 return)——
  // 两条释放路径后台线一条都走不到。于是切走一条正在自动推进的线之后:那一轮的 kz:done 到达,
  // 本函数转 auto_pending 再 armAutoContinue,而 armAutoContinue 第一行的在飞守卫直接静默
  // 返回,下一轮永远不排。用户看到的就是「切走的线卡在等待下一轮再也不动」,且一个字都没有。
  releaseAutoContinue(sessionId);
  const action = payload.autoAction || { type: "NoContinue" };
  const state = sessionState(sessionId);
  state.auto_rounds = action.rounds ?? state.auto_rounds ?? 0;
  // UI2-0926 #13 复核:与活动线 kz:done 同一口径——本轮不是以「在等你回答」收口,等待标记就过期了。
  if (!(action.type === "Stop" && action.reason === "AwaitingUser") && takeAwaitingUser(sessionId)) refreshParallelTaskProjection(sessionId);
  if (["Continue", "Nudge", "GoalPending"].includes(action.type)) {
    transitionSession(sessionId, "auto_pending", { auto_rounds: state.auto_rounds });
    refreshParallelTaskProjection(sessionId);
    armAutoContinue(action.prompt || DEFAULT_CONTINUE_PROMPT, sessionId);
  } else if (action.type === "Stop") {
    transitionSession(sessionId, "idle");
    cancelAutoContinueTimer(sessionId);
    // UI2-0926 #13:后台线的模型在等你回答——自动推进保持开着,切过去回复后照常续跑。
    if (action.reason === "AwaitingUser") markAwaitingUser(sessionId);
    // 引擎判定该线不能再续跑(全阻塞/清空/档位不符)时,后台线自己的自动推进存档
    // 也要置关——否则切回该线时勾选框回显"开着",与引擎的实际停机对不上;
    // 本轮后停是一次性意图,同样要在所属线上落地取消,不能等用户切回来。
    if (["AllBlocked", "BacklogEmpty", "ProfileMismatch", "ResearchWaiting", "ResearchCompleted"].includes(action.reason)) {
      applyAutoStopToSession(sessionId, { enabled: false });
    } else if (action.reason === "StopAfterRound") {
      applyAutoStopToSession(sessionId, { stopAfterRound: false });
    }
    refreshParallelTaskProjection(sessionId);
  }
}

// 失败停摆的原因文案:活动线(07-events kz:auto-fail)与后台线(下面那个)必须同一份,
// 否则同一件事在两条线上说法不同。
export function autoFailStopReasonText(reason, message) {
  if (["ResearchWaiting", "ResearchCompleted"].includes(reason)) return message || t("请查看研究课题概览");
  if (reason === "RateLimited") return t("provider 限流(429)，自动推进已暂停，请等待后手动恢复");
  if (reason === "RepeatedFailure") return t("连续多轮运行失败,自动推进已停止(已发手机通知)");
  return t("运行失败:致命错误,自动推进已停止");
}

// D-403 的失败退避重试对后台线同样必须生效。kz:auto-fail 既不是控制事件、也不在
// BACKGROUND_RENDER_EVENTS 里,路由层原本把后台线的这条整条丢掉:在飞标记不释放、
// 重试不排、停摆原因不落——后台线断一次网就永久停摆,而它恰恰是没人看着的那条。
// 与 handleBackgroundSessionDone 同构:只动**所属线**的状态,绝不写当前线的控制台文本槽。
export function handleBackgroundAutoFail(payload) {
  const sessionId = payload?.sessionId;
  if (!sessionId) return;
  releaseAutoContinue(sessionId);
  const action = payload.autoAction || { type: "NoContinue" };
  sessionState(sessionId).auto_rounds = action.rounds ?? sessionState(sessionId).auto_rounds ?? 0;
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  const label = item?.label ?? t("对话");
  if (action.type === "RetryAfterFailure") {
    const delayMs = action.delayMs ?? 15000;
    const retryLabel = `${t("失败重试")} ${action.attempt}/${action.maxAttempts ?? 3} · ${Math.round(delayMs / 1000)}s`;
    transitionSession(sessionId, "auto_pending", {
      auto_rounds: action.rounds ?? sessionState(sessionId).auto_rounds ?? 0,
    });
    armAutoContinue(DEFAULT_CONTINUE_PROMPT, sessionId, 0, delayMs, retryLabel);
    log(`${label} ${t("自动推进")}:${retryLabel}`, "warn");
  } else if (action.type === "Stop") {
    transitionSession(sessionId, "idle");
    cancelAutoContinueTimer(sessionId);
    // 后台线停摆没人看着:必须浮到界面上(abortAutoContinue 同一口径),不能只 log 一行。
    reportPersistentError(`${label} ${t("自动推进停止")}:${autoFailStopReasonText(action.reason, action.message)}`);
  }
  refreshParallelTaskProjection(sessionId);
}

// R-169:全部阻塞/清空停止已下沉 harness backlog_status + auto_run 状态机,
// 判定结果随 kz:done 的 autoAction(Stop:AllBlocked/BacklogEmpty)带给前端执行。
export function renderComposerLinks() {
  const tray = $("composer-links");
  if (!tray) return;
  tray.replaceChildren();
  const seen = new Set();
  for (const link of resourceLinks(promptBox.value)) {
    if (seen.has(link.url)) continue;
    seen.add(link.url);
    const anchor = document.createElement("a"); anchor.className = "composer-link";
    anchor.href = link.url; anchor.target = "_blank"; anchor.rel = "noopener noreferrer"; anchor.title = link.url;
    const label = document.createElement("span"); label.textContent = link.label;
    anchor.append(resourceIcon(link.url), label); tray.append(anchor);
  }
  tray.classList.toggle("hidden", !seen.size);
}
export function renderAttachments() {
  renderComposerLinks();
  const box = $("attachments");
  box.innerHTML = "";
  box.classList.toggle("hidden", attachments.length === 0);
  // UI2-0926 #11:芯片 = 名字(省略号截断,悬停看全名)+ 单独的 × 移除键。原先整颗是按钮、点哪都删,容易误触;
  // 名字写进 inline-flex 的匿名文本也画不出省略号。
  attachments.forEach((item, index) => {
    const chip = document.createElement("span");
    chip.className = "attachment-chip";
    const name = document.createElement("span");
    name.className = "attachment-name";
    name.textContent = item.file_name;
    name.title = item.file_name;
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "attachment-remove";
    remove.textContent = "×";
    remove.title = t("移除附件");
    remove.setAttribute("aria-label", `${t("移除附件")} ${item.file_name}`);
    remove.addEventListener("click", () => { attachments.splice(index, 1); renderAttachments(); });
    chip.title = resourceType(item.file_name, item.media_type).label;
    chip.append(resourceIcon(item.file_name, item.media_type), name, remove);
    box.appendChild(chip);
  });
}

let pendingFileBytes = 0;
export function addFiles(files) {
  const target = attachments;
  for (const file of files) {
    const mime = attachmentMime(file.name, file.type);
    if (!mime) {
      toast(`${t("不支持的附件类型")}: ${file.name}`);
      continue;
    }
    const total = target.reduce((sum, item) => sum + (item.bytes || item.data.length * .75), pendingFileBytes);
    if (file.size > 16 * 1024 * 1024 || total + file.size > 18 * 1024 * 1024) {
      toastError(`${file.name}: ${t("单个附件最多 16 MB，每次合计最多 18 MB")}`); continue;
    }
    pendingFileBytes += file.size;
    const reader = new FileReader();
    reader.onload = () => {
      if (target !== attachments) { toast(`${file.name}: ${t("已切换对话，请重新添加附件")}`); return; }
      const dataUrl = String(reader.result);
      target.push({ file_name: file.name, media_type: mime, bytes: file.size, data: dataUrl.split(",", 2)[1] || "" });
      renderAttachments();
    };
    reader.onloadend = () => { pendingFileBytes -= file.size; };
    reader.onerror = () => toastError(`${t("读取附件失败")}: ${file.name}`);
    reader.readAsDataURL(file);
  }
}

// UI2-0926 #8 网页预览:截图 / 批注裁图进附件(结构同 addFiles:{file_name, media_type, data(base64)})。
export function addPngAttachment(fileName, png) {
  if (typeof png !== "string" || !png) return false;
  attachments.push({ file_name: String(fileName || "preview.png"), media_type: "image/png", data: png });
  renderAttachments();
  return true;
}
let pickSeq = 0;
/// 批注模式点选一个元素(kz:preview-pick):局部截图进附件,输入框末尾写上「【网页批注】地址 / 元素 / 修改意见:」,
/// 光标停在最后,用户补一句就能发。不自动发送。
export function addPickAttachment({ png, url = "", selector = "", text = "", tag = "" } = {}) {
  pickSeq += 1;
  addPngAttachment(`preview-pick-${pickSeq}.png`, png);
  const snippet = String(text ?? "").replace(/\s+/g, " ").trim().slice(0, 60);
  const target = String(selector || tag || "").trim();
  const lines = [
    `${t("【网页批注】")}${url}`,
    `${t("元素")}:${target ? `\`${target}\`` : "—"}${snippet ? ` ("${snippet}")` : ""}`,
    `${t("修改意见")}:`,
  ];
  const block = lines.join("\n");
  const before = promptBox.value.replace(/\s+$/, "");
  promptBox.value = before ? `${before}\n\n${block}` : block;
  promptBox.dispatchEvent(new Event("input", { bubbles: true }));
  promptBox.focus();
  promptBox.setSelectionRange?.(promptBox.value.length, promptBox.value.length);
}

defer(() => {
  $("attach").addEventListener("click", () => $("attachment-input").click());
});
defer(() => {
  $("attachment-input").addEventListener("change", (e) => { addFiles(e.target.files); e.target.value = ""; });
});
defer(() => {
  promptBox.addEventListener("dragover", (e) => { e.preventDefault(); });
});
defer(() => {
  promptBox.addEventListener("drop", (e) => {
    e.preventDefault();
    if (e.dataTransfer.files.length) { addFiles(e.dataTransfer.files); return; }
    const text = e.dataTransfer.getData("text/uri-list") || e.dataTransfer.getData("text/plain");
    if (text) { promptBox.setRangeText(text, promptBox.selectionStart, promptBox.selectionEnd, "end"); promptBox.dispatchEvent(new Event("input", { bubbles:true })); }
  });
});
defer(() => {
  promptBox.addEventListener("paste", (e) => {
    const files = [...(e.clipboardData?.files || [])];
    if (files.length) { e.preventDefault(); addFiles(files); }
  });
});

// 发送用的模型 = **该线存的模型**,不是下拉的显示值。下拉是回显,而回显曾经会回落到旧全局
// 键(见 loadModels 的注释);自动推进续跑读的一直是 item.model。两条路不同源的后果是:同一条线
// 手动发一句和自动轮跑在两个不同的模型上,而界面上只有一个下拉,看不出来。用户在芯片菜单里
// 选模型时 setLineModel 已经先 updateLocalProcessItem,所以这里读到的就是刚选的那个值。
// UI-0926 #3:不再有任何 DOM 兜底——线路未知时交给后端按本线存档/agent 默认解析。
export function lineModelFor(processId) {
  const item = processItems.find((candidate) => candidate.id === processId);
  return item ? item.model || null : null;
}

const compactingSessions = new Set();

export async function sendText(prompt, { auto = false, promptAttachments = [], executionBatch = false, workItemId = null } = {}) {
  // 任何拒绝发送的理由都要说出来,绝不静默(D-004)。
  if (!prompt) return;
  if (running && auto) {
    toast(t("当前任务还在运行，自动推进将在本轮完成后继续"));
    return;
  }
  if (!currentProject) {
    if (!await openGeneralChat()) return;
  }
  if (compactingSessions.has(activeSessionId)) {
    toast(t("当前对话正在压缩，完成后再发送"));
    return;
  }
  if (!auto && /^\/compact(?:\s|$)/.test(prompt)) {
    if (!activeProcessId) { toast(t("当前没有可压缩的对话")); return; }
    if (running) { toast(t("对话正在运行，请结束后再压缩")); return; }
    const owner = activeSessionId;
    compactingSessions.add(owner);
    try {
      const result = await invoke("conversation_compact", {
        projectDir: currentProject, processId: activeProcessId,
        focus: prompt.replace(/^\/compact\s*/, "") || null,
      });
      toast(result.message);
      if (result.changed && owner === activeSessionId) setCtxTokens(result.after);
      return true;
    } catch (error) { toastError(String(error)); }
    finally { compactingSessions.delete(owner); }
    return;
  }
  if (!activeProcessId) {
    if (auto) return;
    try { if (!await ensure_workspace_process()) return; }
    catch (error) { toastError(String(error)); return; }
  }
  if (selectedAgent().agent !== "dev") { executionBatch = false; workItemId = null; }
  if (!workItemId && selectedAgent().agent === "dev") {
    const project = currentProject, process = activeProcessId;
    try {
      workItemId = await (await import("./31-work-selection.js")).validateSelectedWork(project, process);
      if (project !== currentProject || process !== activeProcessId) { toast(t("对话已切换，请在目标对话中重新发送")); return; }
      if (workItemId) executionBatch = true;
    } catch (error) { toastError(String(error)); return; }
  }
  if (!auto) { syncAutoContinueWithProfile(); await syncAutoRunState(); }
  const delivery = workItemId ? "queue" : $("delivery-select").value;
  if (!auto) void ensureNotificationPermission();
  // UI2-0926 #14:用户手动发消息 = 新的一次运行(后台任务侧栏解除本次运行的压制、确认上次的失败);自动推进续轮不算。
  if (!auto) {
    tasksPanelUserRun(activeSessionId);
    (await import("./24-preview.js")).previewUserRun();
  }
  if (running) {
    addMessage("user", prompt);
    log(`${t("运行中")}${delivery === "steer" ? t("插入") : t("排队")}:${prompt.slice(0, 80)}`);
    try {
      const mode = selectedAgent();
      await invoke("run_prompt", {
        executionBatch,
        workItemId,
        prompt,
        projectDir: currentProject,
        profile: mode.profile,
        agent: mode.agent,
        model: lineModelFor(activeProcessId),
        delivery,
        attachments: promptAttachments,
        processId: activeProcessId,
        autonomous: auto,
        autoAllow: autoAllowEnabled(),
      });
      toast(localizeDynamic(delivery === "steer" ? "已插入当前对话，将优先执行" : "已加入队列，将按顺序执行"));
      // Admission has succeeded. A later queue refresh failure must not leave
      // the same draft ready to submit a second time.
      await refreshPendingInputs().catch(error => reportError(String(error), { retryable: false }));
      return true;
    } catch (err) {
      reportError(String(err), { retryable: false });
    }
    return;
  }
  if (!auto) {
    setAutoRounds(activeSessionId, 0);
    setNoActionRounds(0);
    cancelAutoContinueTimer();
    // R-169:手动发送归零后端状态机计数。
    resetAutoRunState();
  }
  setCurrentAssistant(null);
  setCurrentReasoning(null);
  setRunTokens({ input: 0, output: 0, cacheRead: 0, cacheWrite: 0 });
  setCtxTokens(0);
  setOutputChars(0);
  renderTokens();
  const attachmentStatus = promptAttachments.length > 0
    ? `${auto ? `${t("自动推进")} ${autoRounds} · ` : ""}${t("正在发送")} ${promptAttachments.length} ${t("个附件")} · ${t("准备中")}`
    : auto ? `${t("自动推进")} ${autoRounds} · ${t("准备中")}` : t("准备中");
  if (auto) {
    addMessage("notice", `${t("自动推进已触发")} · ${autoRounds}`);
  } else {
    addUserMessage(prompt, promptAttachments);
  }
  const requestSessionId = activeSessionId;
  const requestProcessId = activeProcessId;
  const requestProject = currentProject;
  if (requestSessionId) {
    // R-206:全部经 transitionSession 折算;补充 detail 一次传入,不再手工直写。
    transitionSession(requestSessionId, "starting", {
      auto_pending: false,
      // run_prompt 的 IPC 返回前，旧的 process_list 快照可能仍是 false；
      // 在首个实时事件到达前不能让这份旧快照覆盖本次用户明确的启动意图。
      live_running: null,
      local_start_pending: true,
      terminal_status: "",
    });
  }
  clearRunPending();
  setRunning(true, attachmentStatus);
  // R-086/R-206:活动会话状态机已在上面 transitionSession("starting") 统一收敛,
  // 这里不再重复调用——重复写块是 R-197 叠在旧块上的残渣(见 R-206 验收④)。
  startElapsed();
  log(`${auto ? t("自动推进") : t("发送")}:${prompt.slice(0, 80)}`);
  try {
    const mode = selectedAgent();
    const request = {
      executionBatch,
      workItemId,
      prompt,
      projectDir: requestProject,
      profile: mode.profile,
      researchTopic: processItems.find((item) => item.id === requestProcessId)?.research_topic || undefined,
      agent: mode.agent,
      model: lineModelFor(requestProcessId),
      workPriority: selectedWorkPriority(),
      delivery,
      attachments: promptAttachments.map((item) => ({ ...item })),
      processId: requestProcessId,
      autonomous: auto,
      autoAllow: autoAllowEnabled(),
    };
    if (!auto) setLastRequest(request);
    await invoke("run_prompt", request);
    return true;
  } catch (err) {
    if (requestSessionId) transitionSession(requestSessionId, "failed");
    if (requestSessionId === activeSessionId) {
      reportError(String(err));
      stopElapsed();
      setRunning(false);
    } else {
      const failed = processItems.find((candidate) => candidate.session_id === requestSessionId);
      reportPersistentError(`${failed?.label || requestProcessId} ${t("发送失败")}:${err}`);
    }
    refreshParallelTaskProjection(requestSessionId);
  }
}

export const PROMPT_HISTORY_KEY = "kz-prompt-history";
export const PROMPT_HISTORY_LIMIT = 30;
export let promptHistory = (() => {
  try { return JSON.parse(localStorage.getItem(PROMPT_HISTORY_KEY) || "[]").filter((item) => typeof item === "string"); }
  catch (_) { return []; }
})();
export let promptHistoryIndex = -1;
export let promptHistoryDraft = "";

export function rememberPrompt(prompt) {
  const value = prompt.trim();
  if (!value) return;
  promptHistory = [value, ...promptHistory.filter((item) => item !== value)].slice(0, PROMPT_HISTORY_LIMIT);
  localStorage.setItem(PROMPT_HISTORY_KEY, JSON.stringify(promptHistory));
  promptHistoryIndex = -1;
}

/// 输入历史翻页(UX-041:与 shell 一致,↑ 回更早的、↓ 回更新的)。
/// promptHistory 是新→旧排的,promptHistoryIndex = -1 表示「还在自己的草稿上」。
/// direction:+1 = ↑ 往更早翻;-1 = ↓ 往更新翻,翻回 -1 时恢复翻页前的草稿。
export function navigatePromptHistory(direction) {
  if (promptHistory.length === 0) return false;
  const next = promptHistoryIndex + direction;
  if (next < -1 || next >= promptHistory.length) return false;
  if (promptHistoryIndex === -1) promptHistoryDraft = promptBox.value;
  promptHistoryIndex = next;
  promptBox.value = next === -1 ? promptHistoryDraft : promptHistory[next];
  renderComposerLinks();
  promptBox.setSelectionRange(promptBox.value.length, promptBox.value.length);
  return true;
}

/// 光标是否在可以翻历史的位置。空框随时可翻;草稿上要光标在最前(↑)/最后(↓),免得吞掉多行文字里的上下移动;
/// 已经在翻历史时,光标在首行(↑)/末行(↓)就继续翻——单行条目不用每翻一条先把光标挪到行首/行尾。
export function promptCaretOnEdge(up) {
  const { value, selectionStart, selectionEnd } = promptBox;
  if (selectionStart !== selectionEnd) return false;
  if (value === "") return true;
  if (promptHistoryIndex === -1) return up ? selectionStart === 0 : selectionStart === value.length;
  return up ? !value.slice(0, selectionStart).includes("\n") : !value.slice(selectionStart).includes("\n");
}

export let fileSuggestions = [];
export let fileSuggestionIndex = -1;
export let fileSuggestionToken = null;
export let fileSuggestionRequest = 0;

export function currentFileToken() {
  const cursor = promptBox.selectionStart;
  const before = promptBox.value.slice(0, cursor);
  const match = before.match(/(?:^|\s)@([^\s]*)$/);
  if (!match) return null;
  return { start: cursor - match[1].length - 1, end: cursor, query: match[1] };
}

export function hideFileSuggestions() {
  ++fileSuggestionRequest;
  fileSuggestions = [];
  fileSuggestionIndex = -1;
  fileSuggestionToken = null;
  closeSurface($("file-suggestions"));
  $("file-suggestions").replaceChildren();
}

export function renderFileSuggestions() {
  const box = $("file-suggestions");
  box.replaceChildren();
  fileSuggestions.forEach((path, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `file-suggestion${index === fileSuggestionIndex ? " active" : ""}`;
    button.textContent = `@${path}`;
    button.addEventListener("mousedown", (event) => {
      event.preventDefault();
      chooseFileSuggestion(index);
    });
    box.appendChild(button);
  });
  // 锚在输入框上沿的手动弹层:不点外关闭(焦点一直在输入框里),Esc 经弹层栈收起并清空候选。
  if (fileSuggestions.length) openPopover(promptBox, box, { manual: true, placement: "top-start", onEscape: hideFileSuggestions });
  else closeSurface(box);
}

export function chooseFileSuggestion(index = fileSuggestionIndex) {
  if (fileSuggestionToken?.project !== currentProject) {
    hideFileSuggestions();
    return;
  }
  const path = fileSuggestions[index];
  const token = currentFileToken() || fileSuggestionToken;
  if (!path || !token) return;
  promptBox.value = `${promptBox.value.slice(0, token.start)}@${path} ${promptBox.value.slice(token.end)}`;
  const cursor = token.start + path.length + 2;
  promptBox.focus();
  promptBox.setSelectionRange(cursor, cursor);
  hideFileSuggestions();
}

export async function refreshFileSuggestions() {
  const token = currentFileToken();
  if (!token || !currentProject) {
    hideFileSuggestions();
    return;
  }
  const project = currentProject;
  fileSuggestionToken = { ...token, project };
  const request = ++fileSuggestionRequest;
  try {
    const paths = await invoke("project_files", { projectDir: project, query: token.query });
    const current = currentFileToken();
    if (request !== fileSuggestionRequest || project !== currentProject || !current
      || current.start !== token.start || current.end !== token.end || current.query !== token.query) return;
    fileSuggestions = paths;
    fileSuggestionIndex = paths.length ? 0 : -1;
    renderFileSuggestions();
  } catch (error) {
    if (request !== fileSuggestionRequest || project !== currentProject) return;
    hideFileSuggestions();
    log(`${t("文件补全失败")}:${error}`, "warn");
  }
}

export let fileSuggestionTimer = null;
defer(() => {
  promptBox.addEventListener("input", () => {
    renderComposerLinks();
    promptHistoryIndex = -1;
    clearTimeout(fileSuggestionTimer);
    fileSuggestionTimer = setTimeout(refreshFileSuggestions, 80);
  });
});
// supplement:这条线此刻正在跑——消息会以「插入/排队」进当前这一轮,是**补充**而不是**接管**,自动推进照常开着
// (UX-046:原先任何手动消息都把自动推进静默关掉,用户只是顺口补一句话就丢了整条自动推进)。
// 只有线在空闲/等下一轮时发的消息才会另起一轮手动运行,那才是接管:关掉自动推进,并给一个一键重开。
export function stopAutoForManualInput({ supplement = false } = {}) {
  if (!$('auto-continue').checked) return false;
  // UI2-0926 #13:模型在等你回答时,手动发的这一条就是回答——自动推进保持开着,回答那一轮结束后照常续跑。
  if (takeAwaitingUser(activeSessionId)) {
    setAutoStopReason("");
    log(t("已回复模型的提问,自动推进在这一轮结束后继续"));
    return false;
  }
  if (supplement) {
    log(t("已向运行中的任务补充一条消息,自动推进保持开启"));
    return false;
  }
  $('auto-continue').checked = false;
  rememberAutoUiState(activeProcessId, ["enabled"]);
  setAutoRounds(activeSessionId, 0);
  setNoActionRounds(0);
  cancelAutoContinueTimer();
  // R-169:手动输入接管 = 关闭后端自主推进并归零计数。
  void syncAutoRunState({ enabled: false });
  resetAutoRunState();
  const message = t("收到手动输入，自动推进已停止");
  setAutoStopReason(message);
  addMessage("notice", message);
  // 接管是用户自己发起的,但可能只是想插一句——toast 带「重新开启」,不用再翻自动推进菜单。
  toast(message, {
    action: {
      label: t("重新开启"),
      onClick: () => {
        const toggle = $("auto-continue");
        if (toggle.checked) return;
        toggle.checked = true;
        toggle.dispatchEvent(new Event("change"));
      },
    },
  });
  log(message);
  return true;
}

let submittingDraft = false;
export async function send() {
  if (submittingDraft) return;
  if (pendingFileBytes > 0) { toast(t("附件正在读取，请稍候再发送")); return; }
  const composeEvent = new CustomEvent("kz:compose-send", { bubbles: true, cancelable: true });
  promptBox.dispatchEvent(composeEvent);
  if (composeEvent.defaultPrevented) return;
  const prompt = promptBox.value.trim();
  if (!prompt && attachments.length === 0) return;
  const submitted = { text: promptBox.value, attachments: [...attachments] };
  submittingDraft = true;
  try {
    if (!currentProject) {
      if (!await openGeneralChat()) return;
      // Opening the first scope restores its draft. Keep the captured input and
      // only submit after the navigation guard confirmed this recipient.
      promptBox.value = prompt;
      setAttachments(submitted.attachments);
      renderAttachments();
    }
    if (!activeProcessId && !/^\/compact(?:\s|$)/.test(prompt)) {
      try { if (!await ensure_workspace_process()) return; }
      catch (error) { toastError(String(error)); return; }
    }
    // The queue currently accepts text only. Keep the entire draft until it can be sent.
    if (running && attachments.length) {
      toast(t("当前任务运行中，附件和文字已保留；本轮结束后发送，或新建讨论"));
      return;
    }
    stopAutoForManualInput({ supplement: running });
    rememberPrompt(prompt);
    hideFileSuggestions();
    const project = currentProject, process = activeProcessId;
    const accepted = await sendText(prompt || t("看一下这些附件"), { promptAttachments: submitted.attachments });
    if (accepted) acknowledge_composer_draft(project, process, submitted);
  } finally { submittingDraft = false; }
}

defer(() => {
  $("send").addEventListener("click", send);
});
defer(() => {
  $("continue-btn").addEventListener("click", () => sendText(continuePrompt()));
});

let sopPickerRequest = 0;
export async function openSopPicker() {
  if (!currentProject) {
    toast(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  const panel = $("sop-picker-panel");
  const list = $("sop-list");
  // 再点一次「SOP」= 收起(锚点按钮不触发点外关闭,切换语义在这里)。
  if (isSurfaceOpen(panel)) {
    closeSurface(panel);
    return;
  }
  const project = currentProject;
  const request = ++sopPickerRequest;
  // UI2-0926 #11:SOP 住在「更多」菜单首项。菜单一关菜单项就不能当锚点,先收起菜单再以「更多」触发器为锚弹出。
  closeSurface($("composer-more-menu"));
  const handle = openPopover($("composer-more"), panel, { placement: "top-end" });
  list.replaceChildren();
  const loading = document.createElement("p");
  loading.className = "dim";
  loading.textContent = `${t("选择流程")}…`;
  list.appendChild(loading);
  try {
    const scopes = await Promise.all(["project", "global"].map((scope) =>
      invoke("memory_entries", { projectDir: project, scope, category: "sop" })
    ));
    if (request !== sopPickerRequest || handle.closed) return;
    if (project !== currentProject) { closeSurface(handle); return; }
    const entries = scopes.flat().filter((entry) => entry.status === "active");
    list.replaceChildren();
    if (!entries.length) {
      const empty = document.createElement("p");
      empty.className = "dim";
      empty.textContent = t("暂无可调用的流程");
      list.appendChild(empty);
      return;
    }
    for (const entry of entries) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "sop-entry";
      const title = document.createElement("strong");
      title.textContent = entry.title;
      const description = document.createElement("span");
      description.className = "dim";
      description.textContent = entry.description || entry.body?.slice(0, 120) || "";
      button.append(title, description);
      button.addEventListener("click", () => {
        if (request !== sopPickerRequest || handle.closed) return;
        if (project !== currentProject) { closeSurface(handle); return; }
        const content = String(entry.body || "").trim();
        promptBox.value = content;
        closeSurface(panel);
        stopAutoForManualInput({ supplement: running });
        promptBox.focus();
        if (!content) {
          toast(t("流程内容为空"));
          return;
        }
        rememberPrompt(content);
        const delivery = $("delivery-select");
        const previous = delivery.value;
        delivery.value = "queue";
        void sendText(content).finally(() => { delivery.value = previous; });
        toast(t("流程已填入继续输入"));
      });
      list.appendChild(button);
    }
  } catch (error) {
    if (request !== sopPickerRequest || handle.closed) return;
    if (project !== currentProject) { closeSurface(handle); return; }
    list.replaceChildren();
    const failed = document.createElement("p");
    failed.className = "dim";
    failed.textContent = `${t("流程加载失败")}: ${error}`;
    list.appendChild(failed);
  }
}
defer(() => {
  $("sop-picker").addEventListener("click", openSopPicker);
});
defer(() => {
  $("sop-picker-close").addEventListener("click", () => closeSurface($("sop-picker-panel")));
});

defer(() => {
  $("continue-toggle").addEventListener("click", () => {
    const panel = $("continue-panel");
    const open = panel.classList.toggle("hidden") === false;
    $("continue-toggle").setAttribute("aria-expanded", String(open));
    $("continue-toggle").textContent = t(open ? "收起" : "推进指令");
    // UI2-0926 #11:开关住在自动推进菜单里;展开编辑区时先收起菜单,焦点落进输入框下方的编辑区。
    if (open) {
      if (document.body.dataset.view !== "project") closeSurface($("autorun-menu"));
      $("continue-prompt").focus();
    }
  });
});
// 自动推进开关是线路级状态,唯一真源是 kz-process-auto-state(按 processId 分键)。
// 旧全局键 kz-auto-continue 让 A 项目的勾选漏进 B 项目的默认线并被固化——
// 启动回显、停机收口、无记录回落全部不得再碰全局键;存量键就地清除。
defer(() => {
  localStorage.removeItem("kz-auto-continue");
});
defer(() => {
  renderAutoStatus();
});
// R-170:LEGACY 升级机制已删除(规则剥离后无「历史默认需升级」契约错位)。
// 存什么读什么:用户自定义文案原样保留;删空回落极简默认。
let continuePromptGeneration = 0;
defer(() => {
  {
    const generation = continuePromptGeneration;
    const stored = (localStorage.getItem("kz-continue-prompt") || "").trim();
    $("continue-prompt").value = stored || DEFAULT_CONTINUE_PROMPT;
    // D-404:localStorage 可能重启即丢;后端 app.json 权威值覆盖。
    void uiPrefsLoad().then((p) => {
      if (generation === continuePromptGeneration && p.continue_prompt) $("continue-prompt").value = p.continue_prompt;
    });
  };
});
defer(() => {
  $("continue-prompt").addEventListener("input", () => { ++continuePromptGeneration; });
  $("continue-prompt").addEventListener("change", () => {
    ++continuePromptGeneration;
    const value = $("continue-prompt").value.trim();
    localStorage.setItem("kz-continue-prompt", value || DEFAULT_CONTINUE_PROMPT);
    $("continue-prompt").value = value || DEFAULT_CONTINUE_PROMPT;
    // D-404:后端持久化。
    void uiPrefsSave({ continue_prompt: value || DEFAULT_CONTINUE_PROMPT });
  });
});
// D-404:仍读取旧 auto_max 到 localStorage,供旧状态迁移链兼容;它不再驱动控件或停止条件。
defer(() => {
  void uiPrefsLoad().then((p) => {
    if (Number.isFinite(p.auto_max)) {
      const max = Math.min(100, Math.max(1, Number(p.auto_max)));
      localStorage.setItem("kz-auto-max", String(max));
    }
  });
});
// 「本轮后停」是一次性意图,不是偏好:绝不持久化。
// 曾经持久化过——勾一次后 localStorage 永远是 "1",每次启动都重新武装,
// 表现为"自动推进跑一轮就停,怎么都停不掉"(D-111)。这里顺手清掉存量键。
defer(() => {
  localStorage.removeItem("kz-auto-stop-round");
});
defer(() => {
  $("auto-stop-round").checked = false;
});
setAutoStopAfterRound(false);
// 启动时等 activeSessionId 就绪后同步所有控件；仅同步 enabled 会让已保存的轮数上限
// 在后端回落为默认 10，造成展示与实际安全上限不一致。
defer(() => {
  void syncAutoRunState();
});
// 停机原因徽标旁的一键复跑:达上限时引擎不关 enabled,清零轮次重排即可;
// AllBlocked/BacklogEmpty/ProfileMismatch 会把 checked 置 false,那时先把开关打开,
// 复用 change 分支既有的档位提示,不在这里再判一次。
defer(() => {
  $("auto-resume").addEventListener("click", () => {
    // UI2-0926 #13 复核:恢复自动推进即结束「在等你回答」——之后手动发的消息是真正的接管,照常关自动推进。
    takeAwaitingUser(activeSessionId);
    const toggle = $("auto-continue");
    if (!toggle.checked) {
      toggle.checked = true;
      toggle.dispatchEvent(new Event("change"));
      return;
    }
    setAutoRounds(activeSessionId, 0);
    setAutoStopReason("");
    resetAutoRunState();
    setStatus(`${t("自动推进恢复")},2 ${t("秒后继续")}…`, false);
    scheduleAutoContinue();
  });
});
// 面板开着时数字键直接命中对应行(参照 Claude 的 Mode 菜单)。只在菜单开着
// 且焦点不在输入框里时生效——否则会把用户在上限框里敲的数字吞掉。
// 菜单是 data-kz-menu 弹层(位置由 CSS 锚点定位保证在视口内,不再手算 left/bottom);
// 鼠标点开后焦点留在触发器上,所以触发器与菜单本体都要接这组快捷键。
export function autorunMenuShortcut(event) {
  const menu = $("autorun-menu");
  if (!isSurfaceOpen(menu)) return;
  const tag = String(event.target?.tagName || "").toLowerCase();
  if (tag === "input" || tag === "select" || tag === "textarea") return;
  const row = menu.querySelector(`.menu-row[data-shortcut="${event.key}"]`);
  if (!row) return;
  event.preventDefault();
  const control = row.querySelector('input[type="checkbox"]') || row.querySelector("button");
  // 自动推进没开时「暂停」「本轮后停」是置灰的(08-auto.js renderAutoRun):数字键同样不该绕过去。
  if (!control || control.disabled) return;
  if (control.tagName.toLowerCase() === "button") control.click();
  else {
    control.checked = !control.checked;
    control.dispatchEvent(new Event("change"));
  }
}
defer(() => {
  $("autorun-more").addEventListener("keydown", autorunMenuShortcut);
  $("autorun-menu").addEventListener("keydown", autorunMenuShortcut);
});
defer(() => {
  $("auto-pause").addEventListener("click", () => {
    setAutoPaused(!autoPaused);
    rememberAutoUiState(activeProcessId, ["paused"]);
    $("auto-pause").classList.toggle("active", autoPaused);
    $("auto-pause").textContent = autoPaused ? t("恢复自动推进") : t("暂停自动推进");
    // R-169:暂停状态同步后端状态机。
    void syncAutoRunState({ paused: autoPaused });
    if (autoPaused) cancelAutoContinueTimer();
    // BUG 修复:恢复时如果正处于轮间空闲,必须重新调度,否则自动推进静默死亡。
    // R-199/D-323:档位条件下沉引擎,恢复路径不再持有前端私有否决——非 dev-auto 时
    // 静默不调度会让引擎计数与状态不知情(验收①未兑现)。恢复一律重新调度,
    // 档位不对由引擎下轮 done 判 Stop(ProfileMismatch) 带 reason 可见收口。
    if (!autoPaused && !running && $("auto-continue").checked) {
      setStatus(`${t("自动推进恢复")},2 ${t("秒后继续")}…`, false);
      scheduleAutoContinue();
    }
    log(autoPaused ? t("自动推进已暂停") : t("自动推进已恢复"));
  });
});
defer(() => {
  $("auto-stop-round").addEventListener("change", () => {
    setAutoStopAfterRound($("auto-stop-round").checked);
    rememberAutoUiState(activeProcessId, ["stopAfterRound"]);
    // R-169:本轮后停同步后端状态机(D-111:不持久化,重启即清)。
    void syncAutoRunState({ stopAfterRound: autoStopAfterRound });
    log(autoStopAfterRound ? t("本轮结束后将停止自动推进") : t("已取消本轮后停"));
  });
});
// R-322 B3:目标条件。change(失焦/回车)才同步,不逐字符打后端。
// 不落 localStorage:目标是**一次性意图**,跟「本轮后停」同类(D-111)——
// 持久化会让它在下次开应用时静默复活,驱动一段跟它无关的对话。
defer(() => {
  $("auto-goal")?.addEventListener("change", () => {
    renderGoalState();
    syncAutoContinueWithProfile();
    void syncAutoRunState({ goal: currentGoalText(), enabled: $("auto-continue").checked });
    const goal = currentGoalText().trim();
    log(goal ? `${t("目标条件已设置")}:${goal}` : t("目标条件已清除"));
  });
});
// 旧 auto_max 控件已移除。历史配置仍由 normalizeAutoState 读取,但不再被用户编辑或用作停止门禁。
defer(() => {
  $("auto-continue").addEventListener("change", () => {
    if ($("auto-continue").checked && selectedAgent().profile === "dev" && $("profile-select").value === "dev-pair" && !currentGoalText().trim()) {
      $("profile-select").value = "dev-auto";
      $("profile-select").dispatchEvent(new Event("change"));
    }
    // UI2-0926 #13 复核:手动开关自动推进同样结束「在等你回答」(关了再开是用户重新表态,不再是等回答)。
    if (takeAwaitingUser(activeSessionId) && autoStopReason === t("模型在等你回答")) setAutoStopReason("");
    setAutoRounds(activeSessionId, 0);
    rememberAutoUiState(activeProcessId, ["enabled"]);
    // 开/关自动推进的这一刻就重绘状态槽(轮次、阶段、停机原因),而不是等下一轮结束才变。
    renderAutoStatus();
    if (!$('auto-continue').checked) cancelAutoContinueTimer();
    // R-169:开关同步后端状态机(enabled)。
    void syncAutoRunState({ enabled: $("auto-continue").checked });
    log($("auto-continue").checked ? t("自动推进已开启:每轮结束自动推进队列") : t("自动推进已关闭"));
    // BUG 修复(触发):空闲时勾上自动推进必须立刻抽第一鞭——原来只挂在"上一轮结束"上,
    // 冷启动勾选后永远没有第一轮,必须手点"继续"才动。
    if ($("auto-continue").checked && !running && !autoPaused) {
      setStatus(t("自动推进启动,2 秒后开始…"), false);
      scheduleAutoContinue();
    }
  });
});
// 研究区(来源/发现/report)只在 research 档出现。dev 档下这两条文档线零写入方
// (提示词里根本没有 source/finding 工具)、零消费者,常驻侧栏就是两个永远的「(空)」:
// 占着位置,还让人以为功能坏了。语义处置留给 R-221 research 模式重定位,这里先按档位收起。
// 不挂进 syncAutoContinueWithProfile:那个函数中间有早退分支,挂进去会漏调。
export function syncResearchSectionVisibility() {
  // R-322:门禁强度回显搭同一趟车。三处调用点(冷启动 / 进程回显 / 用户切换)
  // 一次覆盖全,不必再挂第四个监听——挂多了才是漏调的来源。
  // 放在早退之前:research-section 不存在时强度徽标仍要刷新。
  if (typeof renderHarnessIntensity === "function") renderHarnessIntensity();
  if (typeof renderGoalState === "function") renderGoalState();
  sync_workspace_visibility();
}
export const PROFILE_STORAGE_KEY = "kz-profile";
export const savedProfile = localStorage.getItem(PROFILE_STORAGE_KEY);
defer(() => {
  if (["dev-pair", "dev-auto"].includes(savedProfile)) {
    $("profile-select").value = savedProfile;
  };
});
// 冷启动也要对齐一次:HTML 里默认 hidden,存的档位若是 research 得把它放出来。
defer(() => {
  syncResearchSectionVisibility();
});
// 后端只认 dev/research(决定 agent 选择),dev-auto 是前端的自动推进档位,按进程单独记住,
// 否则切换进程回显时自主推进会被静默降级成结伴开发。
// R-115:这份映射必须落盘。早期只放在内存里,重启后它是空的,回退分支就把模式
// 降级成结伴开发——哪怕 kz-profile 里明明存着自主推进(D-155)。
export const PROCESS_PROFILE_KEY = "kz-process-profile";
export const LINE_MODES = ["dev-pair", "dev-auto", "research"];
// UI2-0926 #13:进程 id 去掉了 `\\?\` 前缀(schema v25),本地存的旧键同样归一(`d|\\?\C:\x` → `d|C:\x`)。
export function normalizeProcessKey(key) {
  return String(key).replace(/\|\\\\\?\\(?!UNC\\)/, "|");
}
// 两种写法并存时去前缀的那条赢:只有本版本才写去前缀的键,它一定是较新的写入(与后端 simplify_pref_keys 同一口径)。
export function normalizeProcessKeyed(entries) {
  const out = new Map();
  const legacy = [];
  for (const [key, value] of entries) {
    const simplified = normalizeProcessKey(key);
    if (simplified === key) out.set(key, value);
    else legacy.push([simplified, value]);
  }
  for (const [key, value] of legacy) if (!out.has(key)) out.set(key, value);
  return out;
}
export const processProfileUi = normalizeProcessKeyed(
  Object.entries(readJson(PROCESS_PROFILE_KEY, {})).filter(([, v]) => LINE_MODES.includes(v)),
);
export function persistProcessProfiles() {
  writeJson(PROCESS_PROFILE_KEY, Object.fromEntries(processProfileUi));
}

// 自动推进是线路级控制状态。旧实现只把 enabled 放在全局 localStorage，切到一条
// 尚未配置的并行线时会把主线的勾选状态直接写进新 session，甚至让旧线路的定时器
// 在新线路上发送继续指令。没有记录的并行线默认关闭，必须由用户在该线路主动开启。
export const PROCESS_AUTO_STATE_KEY = "kz-process-auto-state";
// UI2-0926 #13 复核:本地存的旧键同样归一——不归一的话 `d|\\?\…` 陈值合并进来、经 persist 写回 app.json,
// 下次启动又把用户升级后的设置(比如关掉的自动推进)改回去。
export const processAutoState = normalizeProcessKeyed(
  Object.entries(readJson(PROCESS_AUTO_STATE_KEY, {})).filter(([, value]) => value && typeof value === "object"),
);
// R-264 B3：为 08-compose.js 的 ESM 测试 facade 提供线路级状态访问。
export const __kzProcessAutoState = processAutoState;
globalThis.__kzProcessAutoState = processAutoState;
// UI2-0926 #13 复核:线档位与自动推进开关一样经后端落盘。自动推进续跑轮按线档位发(lineAgent),而档位原先只在
// localStorage(kz-profile / kz-process-profile),本机重启即丢(D-404)——自举线每装一次新版就从自主推进
// 掉回结伴,续跑轮没了 Nudge 与核查轮,还带上结伴提示与权限询问。档位随 process_auto_state 的 mode 字段进
// app.json,合并时回填 processProfileUi:后端记着 mode → 以它为准;本地有档位 → 补进后端;都没有但自动推进开着 →
// 记为自主推进(升级前续跑轮一律按自主档跑,这就是那条线一直以来实际的档位)。
export function restoreLineModes(delta = {}) {
  let changed = false;
  for (const [processId, entry] of processAutoState) {
    if (LINE_MODES.includes(entry?.mode)) {
      processProfileUi.set(processId, entry.mode);
      continue;
    }
    const local = processProfileUi.get(processId);
    const mode = LINE_MODES.includes(local) ? local : entry?.enabled === true ? "dev-auto" : null;
    if (!mode) continue;
    processAutoState.set(processId, { ...entry, mode });
    delta[processId] = { ...(delta[processId] || {}), mode };
    processProfileUi.set(processId, mode);
    changed = true;
  }
  persistProcessProfiles();
  return changed;
}
// D-404:localStorage 可能重启即丢;后端 app.json 是权威。合并完成前 persist 只写
// localStorage(禁止用本地旧值先覆盖后端权威值),合并后双写并刷新当前线控件。
export let uiPrefsAutoStateMerged = false;
const pendingAutoStateDelta = {};
/// 把后端(app.json)的 process_auto_state 合并进本地映射:后端是权威;键归一;同一次合并里回填线档位。
export function mergeBackendAutoState(savedMap) {
  const saved = normalizeProcessKeyed(
    Object.entries(savedMap || {}).filter(([, v]) => v && typeof v === "object"),
  );
  for (const [k, v] of saved) processAutoState.set(k, v);
  // Only explicit edits made while the initial read was pending override hydrate.
  const delta = { ...pendingAutoStateDelta };
  for (const [id, fields] of Object.entries(delta)) {
    if (fields === null) processAutoState.delete(id);
    else processAutoState.set(id, { ...(processAutoState.get(id) || {}), ...fields });
    delete pendingAutoStateDelta[id];
  }
  // 档位回填必须在 applyProfileValue / lineAgent 读 processProfileUi 之前(同一次合并里)。
  restoreLineModes(delta);
  uiPrefsAutoStateMerged = true;
  if (activeProcessId && $("auto-continue")) {
    applyAutoUiState(activeProcessId);
    // 启动时的第一次回显可能早于这次合并(用的是回落档位):按刚恢复的线档位再回显一次。
    applyProfileValue(processItems.find((item) => item.id === activeProcessId)?.profile);
  }
  persistProcessAutoState(delta);
}
defer(() => {
  void uiPrefsLoad().then((p) => mergeBackendAutoState(p.process_auto_state));
});
export function normalizeAutoState(value, _processId) {
  const storedMax = Number.parseInt(value?.maxRounds, 10);
  const legacyMax = Number.parseInt(localStorage.getItem("kz-auto-max"), 10);
  return {
    // 无记录 = 关。旧实现让默认线回落读全局 kz-auto-continue,于是 A 项目开自动推进、
    // B 项目首次打开就继承为开,还随 applyAutoUiState 落盘固化成 B 的"用户选择"。
    // 自动推进是否开启只能来自用户在**该线路**上的显式勾选。
    enabled: value?.enabled === true,
    paused: value?.paused === true,
    stopAfterRound: value?.stopAfterRound === true,
    maxRounds: Number.isFinite(storedMax)
      ? Math.min(100, Math.max(1, storedMax))
      : Number.isFinite(legacyMax) ? Math.min(100, Math.max(1, legacyMax)) : DEFAULT_AUTO_CONTINUE_MAX,
    // UI2-0926 #13 复核:线档位随自动推进存档落盘(见 restoreLineModes);没有记录就不带这个键。
    ...(LINE_MODES.includes(value?.mode) ? { mode: value.mode } : {}),
  };
}
export function persistProcessAutoState(delta = {}) {
  writeJson(PROCESS_AUTO_STATE_KEY, Object.fromEntries(processAutoState));
  if (!Object.keys(delta).length) return;
  if (!uiPrefsAutoStateMerged) {
    for (const [id, fields] of Object.entries(delta)) {
      pendingAutoStateDelta[id] = fields === null ? null : { ...(pendingAutoStateDelta[id] || {}), ...fields };
    }
    return;
  }
  void uiPrefsSave({ process_auto_state: delta });
}
// D-290:回显期间(applyProfileValue 把存档值刷回控件)一律不许落盘。控件在这一刻
// 显示的是**算出来的值**,不是用户意图;把它当意图写回去,一次算错就永久固化——
// 用户每次开 app 都得重设模式与自动推进,正是这条路径自我延续的结果。
export let applyingProfileEcho = false;
// UI2-0926 #13 复核:要记进存档的线档位。当前线读模式芯片(就是用户眼前、刚操作过的那个值),
// 其余线读本线记住的档位;都没有就沿用存档里原有的。
function lineModeToRemember(processId, previous) {
  if (processId === activeProcessId && selectedAgent().profile === "dev") return $("profile-select").value;
  const remembered = processProfileUi.get(processId);
  if (LINE_MODES.includes(remembered)) return remembered;
  return LINE_MODES.includes(previous?.mode) ? previous.mode : undefined;
}
export function rememberAutoUiState(processId = activeProcessId, fields = ["enabled", "paused", "stopAfterRound", "mode"]) {
  if (!processId || applyingProfileEcho) return {};
  const previous = normalizeAutoState(processAutoState.get(processId), processId);
  const mode = lineModeToRemember(processId, previous);
  const next = {
    enabled: $("auto-continue").checked,
    paused: autoPaused,
    stopAfterRound: autoStopAfterRound,
    // 旧配置只保留在状态投影中,不再发送给引擎作硬门禁。
    maxRounds: Number.isFinite(Number(previous?.maxRounds)) ? previous.maxRounds : autoContinueMax(),
    ...(mode ? { mode } : {}),
  };
  const patch = {};
  for (const field of fields) {
    if (next[field] !== undefined && next[field] !== previous[field]) patch[field] = next[field];
  }
  // First enable remembers the visible mode; otherwise legacy restore assumes dev-auto.
  if (patch.enabled === true && !previous.mode && mode) patch.mode = mode;
  processAutoState.set(processId, { ...previous, ...patch });
  persistProcessAutoState(Object.keys(patch).length ? { [processId]: patch } : {});
  return patch;
}
// 引擎停机收口(AllBlocked/BacklogEmpty/ProfileMismatch/本轮后停)必须落在**停机会话
// 所属的线**上:kz:done 可能来自后台线甚至另一个项目的线,直接改当前可见勾选框
// 会把别的线的用户选择清掉——全局键时代「跨项目唯一状态」的另一半病根。
export function applyAutoStopToSession(sessionId, patch) {
  const item = processItems.find((candidate) => candidate.session_id === sessionId);
  if (item) {
    const next = normalizeAutoState(processAutoState.get(item.id), item.id);
    Object.assign(next, patch);
    processAutoState.set(item.id, next);
    persistProcessAutoState({ [item.id]: patch });
  }
  if (!sessionId || sessionId === activeSessionId) {
    if (patch.enabled !== undefined) $("auto-continue").checked = patch.enabled;
    if (patch.stopAfterRound !== undefined) {
      setAutoStopAfterRound(patch.stopAfterRound);
      $("auto-stop-round").checked = patch.stopAfterRound;
    }
    void syncAutoRunState(patch);
  } else {
    // 非当前线:后端状态机也要知道,否则该线下轮 done 仍按旧开关判定。
    void invoke("auto_state_update", {
      sessionId,
      enabled: patch.enabled,
      stopAfterRound: patch.stopAfterRound,
    });
  }
}
export function applyAutoUiState(processId) {
  const next = normalizeAutoState(processAutoState.get(processId), processId);
  processAutoState.set(processId, next);
  $("auto-continue").checked = next.enabled;
  setAutoPaused(next.paused);
  setAutoStopAfterRound(next.stopAfterRound);
  $("auto-stop-round").checked = autoStopAfterRound;
  // 轮次是真实会话状态的只读投影,不再回显旧版上限控件。
  // 真源是会话状态里的 auto_rounds(07-events.js 每轮都写),这里读回来即可。
  const target = processItems.find((item) => item.id === processId);
  setAutoRounds(target?.session_id, currentAutoRounds(target?.session_id));
  // 停机原因与一次性提示是**上一条线/上一个项目**留下的文本,跨线路跨项目串台会让
  // 用户按别人的停机理由去判断当前线。切换即清,由新线自己的事件重新写。
  setAutoHint("");
  setAutoStopReason("");
  // UI2-0926 #13 复核:切到一条模型在等你回答的线(后台时收到的 Stop/AwaitingUser),同一句提示回到原因槽。
  if (target?.session_id && awaitingUserSessions.has(target.session_id)) setAutoStopReason(t("模型在等你回答"), "waiting");
  renderAutoStatus();
  persistProcessAutoState();
}

// 线路页要能直接操控任意一条线的自动推进；配置始终从 processAutoState 读取，顶栏 DOM 仅是投影。

export function lineAutoConfig(processId) {
  return normalizeAutoState(processAutoState.get(processId), processId);
}
const lineAutoStateQueues = new Map();
export function setLineAutoState(processId, patch) {
  const item = processItems.find((candidate) => candidate.id === processId);
  if (!item) return Promise.resolve(null);
  const snapshot = { ...patch };
  const current = lineAutoConfig(processId);
  const initialMode = snapshot.enabled === true && !current.mode ? lineModeToRemember(processId, current) : undefined;
  if (snapshot.enabled === false || snapshot.paused === true) cancelAutoContinueTimer(item.session_id);
  const previous = lineAutoStateQueues.get(processId) || Promise.resolve();
  const next = previous.catch(() => {}).then(() => applyLineAutoStateUpdate(item, snapshot, initialMode));
  lineAutoStateQueues.set(processId, next);
  next.finally(() => {
    if (lineAutoStateQueues.get(processId) === next) lineAutoStateQueues.delete(processId);
  }).catch(() => {});
  return next;
}
async function applyLineAutoStateUpdate(item, patch, initialMode) {
  const processId = item.id;
  if (!processItems.some((candidate) => candidate.id === processId && candidate.session_id === item.session_id)) return null;
  const requested = { ...lineAutoConfig(processId), ...patch };
  // The line's automatic switch selects autonomous mode; paired loops are started by Goal.
  if (patch.enabled === true && !lineAutoConfig(processId).enabled) patch = { ...patch, mode: "dev-auto" };
  // R-224 同价:研究线没有自主推进语义,从线路页开也一样拒绝。
  if (requested.enabled && (processProfileUi.get(processId) === "research" || item.profile === "research")) {
    toast(t("自动推进不适用于研究模式"));
    return null;
  }
  // A failed update must not appear enabled or schedule a model request. Capture the
  // session before awaiting; changing the visible project cannot retarget this action.
  if (!requested.enabled || requested.paused) cancelAutoContinueTimer(item.session_id);
  await invoke("auto_state_update", {
    sessionId: item.session_id,
    ...Object.fromEntries(["enabled", "paused", "stopAfterRound"].filter((field) => Object.hasOwn(patch, field)).map((field) => [field, patch[field]])),
  });
  if (!processItems.some((candidate) => candidate.id === processId && candidate.session_id === item.session_id)) return null;
  const delta = { ...patch };
  const current = lineAutoConfig(processId);
  if (delta.enabled === true && !delta.mode && !current.mode && initialMode) delta.mode = initialMode;
  const next = { ...current, ...delta };
  processAutoState.set(processId, next);
  persistProcessAutoState({ [processId]: delta });
  if (delta.mode) {
    processProfileUi.set(processId, delta.mode); persistProcessProfiles();
    if (processId === activeProcessId) $("profile-select").value = delta.mode;
  }
  if (processId === activeProcessId) {
    $("auto-continue").checked = next.enabled;
    setAutoPaused(next.paused);
    setAutoStopAfterRound(next.stopAfterRound);
    $("auto-stop-round").checked = next.stopAfterRound;
    $("auto-pause").classList.toggle("active", autoPaused);
    $("auto-pause").textContent = autoPaused ? t("恢复自动推进") : t("暂停自动推进");
    renderAutoStatus();
  }
  // 关/暂停立刻撤掉在途的那一枪;开且该线空闲就当场抽第一鞭——不然「开了没反应」要等到
  // 下一个轮末才可见(顶栏勾选走的正是这条语义,线路页不能比它弱)。
  if (!next.enabled || next.paused) {
    cancelAutoContinueTimer(item.session_id);
    if (sessionState(item.session_id).phase === "auto_pending") transitionSession(item.session_id, "idle");
  } else if (!processRunning(item) && sessionState(item.session_id).phase !== "auto_pending") {
    armAutoContinue(processId === activeProcessId ? continuePrompt() : DEFAULT_CONTINUE_PROMPT, item.session_id);
  }
  refreshParallelTaskProjection(item.session_id);
  if (typeof renderLines === "function" && typeof collaborationLines !== "undefined" && collaborationLines.length) {
    renderLines(collaborationLines);
  }
  return next;
}

// 进程级设置必须按线路串行落库。模型/profile/reasoning 原先是 fire-and-forget，
// 快速切换或刷新时旧请求可能晚于新请求完成，把刚选的值覆盖回去。
export const processUpdateQueues = new Map();
export function queueProcessUpdate(processId, fields) {
  const previous = processUpdateQueues.get(processId) || Promise.resolve();
  const next = previous
    .catch(() => {})
    .then(() => invoke("process_update", { processId, ...fields }));
  processUpdateQueues.set(processId, next);
  next.finally(() => {
    if (processUpdateQueues.get(processId) === next) processUpdateQueues.delete(processId);
  }).catch(() => {});
  return next;
}

export function updateLocalProcessItem(processId, fields) {
  const item = processItems.find((candidate) => candidate.id === processId);
  if (item) Object.assign(item, fields);
}

export function syncAutoContinueWithProfile() {
  const enabled = $("profile-select").value === "dev-auto" || Boolean(currentGoalText().trim());
  if ($("auto-continue").checked === enabled) return;
  $("auto-continue").checked = enabled;
  if (!enabled) cancelAutoContinueTimer();
  if (!applyingProfileEcho) {
    rememberAutoUiState(activeProcessId, ["enabled", "mode"]);
    void syncAutoRunState({ enabled });
  }
  renderAutoStatus();
}
export function applyProfileValue(backendProfile) {
  // D-290:没有进程身份就没有「该显示谁的档位」这个问题。此时既读不到本进程记忆,
  // 回退链又会算出 dev-pair,把控件刷成结伴开发 —— 随后 syncAutoContinueWithProfile
  // 顺手关掉自动推进,下一次 switchProcess 再把这个假值写进存档。启动竞态里 activeProcessId
  // 尚未就绪的那一瞬,就是整条降级链的起点。不知道就别动控件。
  if (!activeProcessId) return;
  const remembered = processProfileUi.get(activeProcessId);
  // 各对话先恢复自己的模式；没有记录时使用同一全局偏好和默认值。
  const globalChoice = localStorage.getItem(PROFILE_STORAGE_KEY);
  const fallback = ["dev-pair", "dev-auto"].includes(globalChoice)
    ? globalChoice
    : "dev-pair";
  if (backendProfile !== "research") $("profile-select").value = remembered && remembered !== "research" ? remembered : fallback;
  // 回显期间关掉的自动推进只是**跟随显示**,不是用户按下的开关:不落盘、不写全局键。
  applyingProfileEcho = true;
  try {
    syncAutoContinueWithProfile();
  } finally {
    applyingProfileEcho = false;
  }
  syncResearchSectionVisibility();
}
defer(() => {
  $("profile-select").addEventListener("change", () => {
    const value = $("profile-select").value;
    if (selectedAgent().profile !== "dev") return;
    syncResearchSectionVisibility();
    localStorage.setItem(PROFILE_STORAGE_KEY, value);
    if (activeProcessId) {
      processProfileUi.set(activeProcessId, value);
      persistProcessProfiles();
      const profile = "dev";
      updateLocalProcessItem(activeProcessId, { profile });
      queueProcessUpdate(activeProcessId, { profile })
        .catch((error) => reportPersistentError(`${t("对话模式保存失败")}:${error}`));
    }
    syncAutoContinueWithProfile();
    rememberAutoUiState(activeProcessId, ["mode"]);
  });
});
defer(() => {
  $("work-priority-select").addEventListener("change", () => {
    const value = selectedWorkPriority();
    // 只写这一处。引擎读的就是它(run.rs normalize_work_priority → WorkPriority
    // → resolve_work_decision);不再镜像成 preference 记忆,理由见文件上方说明。
    rememberWorkPrioritySelection(value);
    log(localizeDynamic(value === "requirement-first" ? "已切换为需求优先" : "已切换为缺陷优先"));
  });
});
defer(() => {
  $("stop").addEventListener("click", async () => {
    const targetSessionId = activeSessionId;
    const targetProcessId = activeProcessId;
    const targetProject = currentProject;
    cancelAutoContinueTimer(targetSessionId);
    setAutoRounds(activeSessionId, 0);
    if (runControlPending && !running) {
      if (targetSessionId) transitionSession(targetSessionId, "stopped");
      $("auto-continue").checked = false;
      rememberAutoUiState(activeProcessId, ["enabled"]);
      void syncAutoRunState({ enabled: false });
      clearRunPending();
      setRunning(false, t("已停止"));
      log(t("已停止自动推进等待"));
      return;
    }
    if (targetSessionId) transitionSession(targetSessionId, "stopping");
    setStopping(t("停止中…"));
    armStoppingWatchdog(targetSessionId);
    hideAsk();
    try {
      await invoke("stop_run", { projectDir: targetProject, processId: targetProcessId });
      log(t("停止指令已确认，等待对话结束"));
    } catch (err) {
      const item = processItems.find((candidate) => candidate.session_id === targetSessionId);
      if (targetSessionId) transitionSession(targetSessionId, item?.running ? "running" : "idle");
      if (targetSessionId === activeSessionId) {
        setRunning(Boolean(item?.running), item?.running ? t("运行中") : t("空闲"));
      }
      reportPersistentError(`${t("停止指令失败")}:${err}`);
    }
  });
});
defer(() => {
  // 补全列表开着时的 Esc 由弹层栈接走(renderFileSuggestions 注册的 onEscape),这里不再单独处理。
  promptBox.addEventListener("keydown", (e) => {
    if (isImeComposing(e)) return;
    if ((e.key === "Tab" || e.key === "Enter") && fileSuggestions.length > 0 && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      chooseFileSuggestion();
      return;
    }
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey) {
      const up = e.key === "ArrowUp";
      // 补全列表开着:上下键只管选候选,与光标在哪无关(光标在 @token 末尾,按位置判断会让 ↑ 永远不生效)。
      if (fileSuggestions.length > 0) {
        e.preventDefault();
        fileSuggestionIndex = (fileSuggestionIndex + (up ? -1 : 1) + fileSuggestions.length) % fileSuggestions.length;
        renderFileSuggestions();
        return;
      }
      // 输入历史:↑ 回更早一条、↓ 回更新一条(UX-041:原先方向反了,↑ 在空框里没反应)。
      if (promptCaretOnEdge(up) && navigatePromptHistory(up ? 1 : -1)) e.preventDefault();
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      send();
    } else if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  });
});

defer(() => {
  window.addEventListener("keydown", (e) => {
    // 模态开着时全局快捷键一律让路:确认框背后按 Ctrl+Shift+N 不该真的去点「新对话」。
    if (isModalOpen()) return;
    const modifier = e.ctrlKey || e.metaKey;
    if (!modifier || e.altKey) return;
    if (e.key.toLowerCase() === "k") {
      e.preventDefault();
      // 非对话页(需求 / 文件 / 设置…)输入框是藏着的,聚焦它是空操作:先带回对话页再聚焦(UX-126)。
      ensureChatView();
      promptBox.focus();
      return;
    }
    if (!e.shiftKey) return;
    if (e.key.toLowerCase() === "c") {
      e.preventDefault();
      if (!document.dispatchEvent(new CustomEvent("kz:compose-stop", { cancelable: true }))) return;
      $("stop").click();
    } else if (e.key.toLowerCase() === "n") {
      e.preventDefault();
      $("new-chat").click();
    }
  });
});


