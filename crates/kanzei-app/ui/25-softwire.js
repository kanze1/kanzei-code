import { $, defer, invoke, listen, on } from "./01-core.js";
import { mountManagement, hideManagement } from "./30-management.js";
import { autoAllowEnabled, layoutPref, setLayoutPref } from "./03-layout.js";
import { languageIsEnglish, localizedStage, localizedStatusWord, t } from "./02-i18n.js";
import { fillTemplate } from "./04-structured-parse.js";
import { currentProject, activeProcessId, activeSessionId, processItems, sessionStates, transitionSession, navigate_view, toast, toastError,
  replacePendingQuestions, questionWaitRevision, noteQuestionReply, onInnerBack } from "./03-shell.js";
import { openProjectSpace, openProjectResource, workbenchProject, openWorkbenchItem, setWorkbenchQuestions, setAttentionNavActive } from "./12-workbench.js";
import { lastWorkspaceSnapshot, backlogTally } from "./12-docs-pages.js";
import { projectDisplayName, switchProcess } from "./09-sessions.js";
import { subagentRunsFor, onSubagentChange, createSubagentView, renderSubagentTimeline } from "./05-subagents.js";
import { lineAgent, cancelAutoContinueTimer, setAutoRounds, awaitingUserSessions } from "./08-auto.js";
import { askActive, askQueues } from "./07-events.js";
import { createComposer } from "./25-softwire-composer.js";
import { createRunControl } from "./25-softwire-run.js";
import { active_space, create_workspace_process } from "./03-workspaces.js";
import { deliveryState, loadDeliveredFiles, onDeliveriesChanged } from "./06-deliveries.js";
import { renderFileCard } from "./06-activity.js";
import { compactionInProgress } from "./03-session-stage.js";
import { moduleNames, sameProject, targetKey, conversationKey, selectWork, inboxFromProjects, receiptLabel, executionActivity, activitySymbols, isMultiReply, interactionKind, isLegacyReviewPlaceholder, readableText } from "./25-softwire-model.js";
import { node, button, lineLabel, createWorkspace, renderGraph, detailHeading, facts, evidence, renderMessages, renderInboxList, renderInteraction, stopSignals } from "./25-softwire-view.js";

// 概览整页文案:长句走 t()(词条在 02-i18n.js),短词用 en(中文, English) 直接给两种说法,避开同字词条的别义。
const en = (zh, english) => (languageIsEnglish() ? english : zh);
const MODULE_EN = { work: "Requirements & plan", batch: "Verification & delivery", main: "Main conversation", memory: "Memory", tools: "Tool management", tasks: "Subtasks", history: "History & compaction", inbox: "Needs attention" };
const moduleLabel = module => (languageIsEnglish() ? MODULE_EN[module] ?? moduleNames[module] : moduleNames[module]);
const states = new Map(), projectDetails = new Map(), sending = new Set(), replies = new Map();
const editingReplies = new Set();
let root, composer, runner, current, questions = [], generation = 0, initialized = false;
let questionsRequest = null, inboxRequest = null, painted = "", notificationBaseline = false;
const seenQuestions = new Set();
const presentedQuestions = new Set();
const questionOrigins = new Map();
let presentingQuestion = false;
function stateFor(project) {
  if (!states.has(project)) states.set(project, { project, summary: null, detail: null, tab: "network", module: null,
    selection: null, evidence: null, interaction: null, returnTo: null, tools: [], compactions: new Map(), compacting: new Map(),
    messages: new Map(), loadedConversation: new Set(), historyRequests: new Map(), historyErrors: new Map(), traces: new Map(),
    batchEvidence: { rows: [], loaded: false, error: "", request: null }, usage: { rounds: [], loaded: false, error: "", request: null, at: 0 },
    request: null, error: "", revision: 0 });
  return states.get(project);
}
const surface = () => $("sw-surface");
const visible = () => document.body.dataset.view === "project" && active_space === "dev";
function allMessages() {
  const merged = new Map([...projectDetails.values()].map(p => [p.path, p]));
  return inboxFromProjects([...merged.values()], questions).filter(m => !replies.get(m.key)?.done);
}
function developmentState(state) {
  const project = { ...state?.summary, ...state?.detail };
  project.current_items = (project.current_items || []).map(item => {
    const entry = [...(state.docs?.requirements || []), ...(state.docs?.defects || [])].find(entry => entry.id === item.id);
    return { ...entry, ...item, blocked: entry?.blocked ?? item.blocked, block_reasons: entry?.block_reasons ?? item.block_reasons };
  });
  return { ...project, lines: (project.lines || []).filter(line => !["research", "readonly"].includes(line.profile)).map(line => {
    const live = sessionStates.get(line.session_id);
    const activity = executionActivity(line, live, {
      // 有权限请求挂着(等批准)同样是「在等你」:概览转琥珀、停掉橙色波形,不再显示「调用中」(含当前正在对话的那条)。
      waiting: awaitingUserSessions.has(line.session_id) || (askQueues.get(line.session_id)?.length ?? 0) > 0 || askActive?.sessionId === line.session_id || questions.some(q => !q.background && !q.agentId && q.sessionId === line.session_id && sameProject(q.projectDir, state.project)),
      unavailable: state.runtimeUnavailable || Boolean(project.error) || ["stale", "unavailable"].includes(project.freshness),
    });
    return { ...line, running: live && (live.converged || live.running || live.auto_pending) ? live.running : line.running,
      stage: activity.label, activity: activity.state };
  }) };
}
function work(state = current) { return selectWork(developmentState(state), state?.selection?.processId); }
function target(state = current) {
  if (state.interaction) {
    const m = state.interaction, result = replies.get(m.key);
    return { project: m.project, processId: m.processId, sessionId: m.sessionId, module: "inbox", interactionId: m.key,
      label: projectDisplayName(m.project) + " / " + t(({ question: "回复提问", decision: "复核决策", delivery: "交付反馈" })[interactionKind(m)]),
      placeholder: interactionKind(m) === "question" ? t("填写你的选择或补充说明…") : "",
      busy: sending.has(m.key), completed: Boolean(result?.done), sendStatus: result?.error || receiptLabel(result?.status) };
  }
  const selected = state.selection || {};
  const module = selected.module || "main";
  const { line } = work(state);
  const bound = developmentState(state).lines.find(l => l.id === selected.processId);
  return { project: state.project, ...selected, module,
    label: projectDisplayName(state.project) + " / " + (module === "tasks" && selected.child ? t("子任务") + " " + selected.child.description : lineLabel(bound?.label || "主对话") + (module !== "main" ? " · " + moduleLabel(module) : "")),
    running: Boolean(bound?.running || (!selected.processId && line?.running)),
    readOnly: selected.child ? t("这个子任务暂不支持直接续聊，可返回子任务管理后交给主对话处理。") : !state.summary && !state.detail ? t("正在读取对话…") : "",
    busy: sending.has(targetKey({ project: state.project, ...selected, module })),
  };
}
function bindSelection(state, module, extra = {}) {
  const { line, item, unit } = work(state);
  state.selection = { module, processId: state.selection?.processId || line?.id || null,
    sessionId: state.selection?.sessionId || line?.session_id || null,
    objectId: module === "work" ? item?.id : module === "batch" ? unit?.unit_id : "", ...extra };
}
function reconcileSelection(state) {
  const excluded = (state.detail?.lines || state.summary?.lines || []).find(line => line.id === state.selection?.processId && ["research", "readonly"].includes(line.profile));
  if (excluded) state.selection = null;
  if (!state.selection && sameProject(state.project, currentProject)) {
    const active = developmentState(state).lines.find(line => line.id === activeProcessId);
    if (active) bindSelection(state, "main", { processId: active.id, sessionId: active.session_id });
  }
  if (!state.selection && (state.summary || state.detail)) bindSelection(state, "main");
}
function mount(state) {
  const changed = current !== state || !root?.isConnected;
  reconcileSelection(state);
  if (!root) root = createWorkspace({ module: selectModule, tab: selectTab, refresh: () => void refreshCurrent(true), interaction: openInteraction });
  if (runner.element.parentElement !== $("sw-run-slot")) $("sw-run-slot").append(runner.element);
  if (root.parentElement !== $("project-overview-content")) $("project-overview-content").replaceChildren(root);
  current = state;
  void loadDeliveredFiles(state.project);
  void loadBatchEvidence(state);
  void loadTrace(state);
  root.dataset.project = state.project;
  composer.mount($("sw-dock-slot"), target(state));
  paint(changed);
  void presentForegroundQuestion();
}
function paint(force = false) {
  if (!current || !visible() || !root?.isConnected) return;
  const management = current.tab === "network" && !current.module && !current.evidence && !current.interaction;
  root.hidden = management;
  if (management) {
    mountManagement(current.project, current.docs, developmentState(current), current.error);
    return;
  }
  hideManagement();
  const state = current, selected = work(state), { item, unit, line } = selected;
  $("sw-title").textContent = projectDisplayName(state.project);
  $("sw-requirement").textContent = item ? item.id + " · " + localizedStatusWord(item.status) : t("项目概览");
  const batches = item?.batches;
  $("sw-batch").textContent = unit ? unit.unit_id + "  ›" : batches?.total ? fillTemplate(t("批次 {done} / {total}"), { done: batches.done, total: batches.total }) + "  ›" : t("当前批次") + "  ›";
  const deliveries = deliveryState(state.project);
  $("sw-checkpoint").textContent = state.batchEvidence.rows.length ? "◷ " + fillTemplate(t("{n} 条批次记录"), { n: state.batchEvidence.rows.length }) : deliveries.rows.length ? "▤ " + fillTemplate(t("{n} 个交付文件"), { n: deliveries.rows.length }) : unit?.last_checkpoint ? "◷ " + t("已保存检查点") : unit?.status === "done" ? "✓ " + t("机器已完成") : state.batchEvidence.error ? t("批次记录读取失败") : deliveries.error ? t("交付读取失败") : deliveries.loaded ? t("暂无交付文件") : t("读取交付中…");
  $("sw-runtime").textContent = localizedStage(line?.stage || (line?.running ? "运行中" : "空闲"));
  $("sw-runtime").dataset.state = line?.activity || "idle";
  $("sw-runtime").dataset.symbol = activitySymbols[line?.activity || "idle"];
  runner.sync({ ...line, project: state.project });
  root.querySelectorAll("[data-stage]").forEach(el => { el.dataset.active = String(el.dataset.stage === unit?.status); });
  root.querySelectorAll("[data-tab]").forEach(el => el.setAttribute("aria-pressed", String(el.dataset.tab === state.tab)));
  setAttentionNavActive(state.tab === "inbox");
  $("sw-message-count").textContent = String(allMessages().length || "");
  root.dataset.depth = state.evidence ? "evidence" : state.module ? "module" : "work";
  root.dataset.tab = state.tab || "network";
  surface().dataset.layout = state.tab === "network" && !state.module && !state.evidence && !state.interaction ? "network" : "detail";
  composer.mount($("sw-dock-slot"), target(state));
  const key = JSON.stringify([state.project, state.tab, state.module, state.evidence?.id, state.interaction?.key]);
  if (state.evidence) {
    if (force || painted !== key) evidence(surface(), state.evidence.title, state.evidence.content, state.evidence.metadata, closeEvidence);
  } else if (state.interaction) {
    if (force || painted !== key) renderInteraction(surface(), state.interaction, {
      back: returnFromMessage, choice: chooseReply, explain: editReply, refresh: refreshInteraction,
    });
    const result = replies.get(state.interaction.key);
    const busy = sending.has(state.interaction.key), completed = Boolean(result?.done);
    const manual = editingReplies.has(state.interaction.key) || Boolean(composer.value().trim() && !isLegacyReviewPlaceholder(state.interaction, composer.value())) || isMultiReply(state.interaction);
    surface().querySelectorAll("[data-reply-choice]").forEach(button => {
      button.disabled = busy || completed;
      if (interactionKind(state.interaction) === "question") button.setAttribute("aria-pressed", String(composer.value().split("\n").includes(button.dataset.replyChoice)));
      button.dataset.instant = String(!manual && button.dataset.replyEdit !== "true");
    });
    const explain = surface().querySelector(".sw-reply-explain");
    if (explain) { explain.disabled = busy || completed; explain.setAttribute("aria-pressed", String(manual)); }
    const receipt = surface().querySelector(".sw-interaction-receipt");
    if (receipt) receipt.textContent = result ? (result.text || "") + "\n" + (result.error || receiptLabel(result.status)) : "";
    const retry = surface().querySelector(".sw-interaction-refresh");
    if (retry) retry.hidden = !result?.error;
  } else if (state.tab === "inbox") {
    const signature = JSON.stringify(allMessages().map(m => [m.key, m.revision]));
    if (force || painted !== key || surface().dataset.items !== signature) {
      renderInboxList(surface(), allMessages(), openInteraction); surface().dataset.items = signature;
    }
  } else if (state.tab === "conversation") {
    if (painted !== key || force) surface().replaceChildren();
    renderMessages(surface(), messagesFor(state));
  } else if (state.module) {
    if (force || painted !== key) renderModule();
    if (state.childView?.container.isConnected) renderSubagentTimeline(state.childView);
  } else {
    void loadTrace(state); void loadUsage(state);
    const project = developmentState(state);
    const messages = messagesFor(state, { ...target(state), module: "main" });
    const replyError = state.historyErrors.get(conversationKey({ ...target(state), module: "main" }));
    void loadConversation(state, { ...target(state), module: "main" });
    const compacting = state.compacting.get(line?.session_id) ?? /压缩|compact/.test(line?.stage || "");
    const graphRevision = JSON.stringify([item, unit, deliveries.rows.map(row => [row.id, row.status]), state.batchEvidence.rows.map(row => row.id), project.current_items, project.lines, state.backlog, toolsFor(state), traceFor(state), state.compactions.get(line?.session_id), compacting, tasksFor(state).map(t => [t.id, t.state]), messages.slice(-4), replyError, state.error, allMessages().map(m => m.key),
      state.usage.rounds.map(r => r.at), state.usage.loaded, state.usage.error, layoutPref("overview", "usage_open")]);
    if (force || painted !== key || surface().dataset.graph !== graphRevision) {
      renderGraph(surface(), { ...selected, project, messages, replyError, deliveryCount: deliveries.rows.length, batchEvidenceCount: state.batchEvidence.rows.length, backlog: state.backlog, usage: state.usage, tools: toolsFor(state), trace: traceFor(state), tasks: tasksFor(state), running: line?.running, compacting, compaction: state.compactions.get(line?.session_id),
        waiting: allMessages().find(m => sameProject(m.project, state.project) && m.sessionId === line?.session_id && m.kind === "question"), error: state.error || messages.at(-1)?.error },
      { module: selectModule, interaction: openInteraction, list: kind => void openList(state, kind),
        usageOpen: () => layoutPref("overview", "usage_open") === true,
        usageToggle: () => { if (state.usage.error) void loadUsage(state, true); setLayoutPref("overview", "usage_open", layoutPref("overview", "usage_open") === true ? null : true); paint(true); surface().querySelector("#sw-usage-detail:not([hidden])")?.scrollIntoView?.({ block: "nearest" }); },
        usageRetry: () => void loadUsage(state, true),
        conversation: () => void openNativeConversation(state),
        retryReplies: () => { state.loadedConversation.delete(conversationKey(target(state))); void loadConversation(state); },
        reply: () => void openNativeConversation(state),
        resource: view => void openProjectResource(state.project, view), item: item => void openWorkbenchItem(state.project, item),
        capture: kind => { state.capture = { kind, text: "" }; selectModule("work"); },
        line: selectedLine => {
          bindSelection(state, "main", { processId: selectedLine.id, sessionId: selectedLine.session_id, objectId: "" }); paint(true);
        } });
      surface().dataset.graph = graphRevision;
    }
  }
  painted = key;
}
function toolsFor(state) { return state.tools.filter(t => t.sessionId === (state.selection?.sessionId || work(state).line?.session_id)); }
function traceDestination(state) {
  const line = work(state).line;
  return { project: state.project, processId: state.selection?.processId || line?.id, sessionId: state.selection?.sessionId || line?.session_id };
}
function traceFor(state) { return state.traces.get(conversationKey(traceDestination(state))) || { loaded: false, error: "" }; }
function tasksFor(state) {
  const live = subagentRunsFor(state.selection?.sessionId || work(state).line?.session_id) || [];
  const historical = toolsFor(state).filter(t => t.name === "task" && !live.some(run => run.id === t.id)).map(t => ({
    id: t.id, description: t.input?.description || t.summary || t.id, state: t.pending ? "历史未记录结果" : t.ok === false ? "失败" : "完成", historical: t,
  }));
  return [...live, ...historical];
}
function messagesFor(state, destination = target(state)) {
  const key = conversationKey(destination);
  if (!state.messages.has(key)) state.messages.set(key, []);
  return state.messages.get(key);
}
function selectTab(tab) {
  if (!current) return;
  if (tab === "conversation") { void openNativeConversation(current); return; }
  if (current.interaction) { returnFromMessage(); if (tab === "network") return; }
  current.tab = tab; current.evidence = null;
  if (tab === "network") { current.module = null; bindSelection(current, "main"); }
  if (tab === "inbox") void loadInbox();
  if (tab === "conversation") void loadConversation(current);
  paint(true);
}
function selectModule(module) {
  if (!current) return;
  if (module === "main") { void openNativeConversation(current); return; }
  if (module === "batch") { void loadDeliveredFiles(current.project, { force: true }); void loadBatchEvidence(current, true); }
  current.interaction = null; current.evidence = null; current.module = module; current.tab = "network";
  current.evidenceRequest = (current.evidenceRequest || 0) + 1;
  bindSelection(current, module);
  paint(true);
  surface().focus();
  if (["tools", "history", "tasks"].includes(module)) void loadTrace(current, true);
  if (module === "main") { current.tab = "conversation"; paint(true); void loadConversation(current); }
}
function showEvidence(title, content, metadata = "", id = crypto.randomUUID()) {
  current.evidence = { id, title, content: String(content), metadata, scroll: surface().scrollTop };
  paint(true); surface().scrollTop = 0; surface().focus();
}
function closeEvidence() {
  const scroll = current.evidence?.scroll || 0;
  current.evidence = null; paint(true); surface().scrollTop = scroll;
  surface().querySelector("button")?.focus();
}
function renderModule() {
  const state = current, module = state.module, { item, unit, line } = work(state);
  const host = surface();
  detailHeading(host, moduleLabel(module), () => { state.module = null; state.selection.child = null; paint(true); });
  const link = (title, content, metadata) => host.append(button(title + "  ›", () => showEvidence(title, content, metadata), "sw-evidence-link"));
  if (module === "work") {
    if (state.capture) renderCapture(host, state);
    facts(host, [[t("需求"), item?.title], [en("目标", "Objective"), unit?.objective], [t("范围"), unit?.scope], [en("验收", "Acceptance"), unit?.acceptance]]);
    if (item) host.append(button(t("打开需求原文"), () => void openWorkbenchItem(state.project, item)));
    else host.append(node("p", t("尚未绑定需求，可以在下方说明要推进的工作。"), "sw-empty"));
  } else if (module === "batch") {
    renderBatchEvidence(host, state);
    renderDeliveries(host, state);
    host.append(node("h3", t("当前批次"), "sw-section-title"));
    facts(host, [[en("工作单元", "Work unit"), unit?.unit_id], [t("阶段"), unit?.status && localizedStatusWord(unit.status, "unit")], [t("最近检查点"), unit?.last_checkpoint?.summary], [t("下一步"), unit?.last_checkpoint?.next_action], [t("阻塞原因"), unit?.blocked_reason || unit?.terminal_reason]]);
    if (unit) {
      link(t("检查点与版本"), readableText(unit.last_checkpoint || { base_revision: unit.base_revision }, "", languageIsEnglish()), t("记录版本") + " " + unit.source_sequence);
      link(t("验收与验证依据"), readableText({ acceptance: unit.acceptance, verification: unit.verification, evidence: unit.evidence }, "", languageIsEnglish()), unit.unit_id);
    } else if (!state.batchEvidence.rows.length) host.append(node("p", t("尚无工作单元检查点"), "sw-evidence-meta"));
    host.append(button(t("请求收尾本批"), () => composer.insert(en("请收尾当前批次：验证现有修改，保存真实检查点；只提交可归属本批且验证通过的修改，再回写进展。", "Please wrap up the current batch: verify the existing changes and save a real checkpoint; commit only the changes that belong to this batch and pass verification, then record the progress."))));
  } else if (module === "memory") {
    // B28/C10:记忆的浏览、搜索、整理和与记忆管理者的对话只在记忆页(一份条目列表、一段管理对话);
    // 概览这里原先另带一份最多 80 条、无搜索的列表和一个独立的管理对话输入,两处各写各的。现在只做入口。
    host.append(node("p", t("记忆的浏览、搜索、整理,以及和记忆管理者的对话,都在记忆页。"), "sw-empty"),
      button(t("打开记忆页"), () => void openProjectResource(state.project, "memory"), "sw-link sw-memory-open"));
  } else if (module === "tools") {
    const trace = traceFor(state);
    if (trace.error) host.append(node("p", t("工具记录读取失败：") + trace.error, "sw-error"), button(t("重试读取工具记录"), () => void loadTrace(state, true)));
    if (!toolsFor(state).length && !trace.error) host.append(node("p", trace.loaded ? t("记录中未见调用") : t("记录未加载，正在读取…"), "sw-empty"));
    for (const tool of toolsFor(state).slice(-30).reverse()) host.append(button((tool.pending ? "◌ " : tool.ok === false ? "! " : "✓ ") + tool.name + " · " + (tool.summary || tool.id), () => {
      showEvidence(tool.name, JSON.stringify({ input: tool.input ?? tool.summary ?? t("历史未保存完整参数"), result: tool.content ?? tool.preview ?? t("尚无结果"), code: tool.code, duration_ms: tool.durationMs }, null, 2), t("调用") + " " + tool.id);
    }, "sw-evidence-link"));
  } else if (module === "tasks") {
    const tasks = tasksFor(state);
    if (!tasks.length) host.append(node("p", t("这个对话还没有子任务"), "sw-empty"));
    // 子任务状态是后端枚举(running/starting…),列表里不再中英混排(UX-159)。
    const taskStates = { running: "执行中", starting: "启动中", stopping: "停止中", waiting: "待批准", completed: "完成", done: "完成", failed: "失败", cancelled: "已取消", stopped: "已停止" };
    for (const task of tasks) host.append(button(task.description + " · " + t(taskStates[task.state] || task.state), () => {
      bindSelection(state, "tasks", { child: task, objectId: task.id }); composer.mount($("sw-dock-slot"), target(state));
      detailHeading(host, task.description, () => { state.selection.child = null; paint(true); });
      if (task.historical) host.append(node("pre", JSON.stringify(task.historical, null, 2), "sw-evidence"));
      else {
        const body = node("div"); host.append(body);
        state.childView = createSubagentView(body, task); renderSubagentTimeline(state.childView);
      }
    }, "sw-evidence-link"));
  } else if (module === "history") {
    if (state.compactions.has(line?.session_id)) link(t("最近压缩记录"), JSON.stringify(state.compactions.get(line.session_id), null, 2), t("运行记录；查看不等于召回"));
    else host.append(node("p", t("尚无可用压缩记录"), "sw-empty"));
    host.append(button(t("查看完整对话"), () => void openNativeConversation(state)));
  }
  if (!["memory", "tasks"].includes(module)) host.append(button(fillTemplate(t("与 {name} 讨论"), { name: lineLabel(line?.label || "主对话") }), () => selectTab("conversation"), "sw-discuss"));
}
function renderDeliveries(host, state) {
  const data = deliveryState(state.project), section = node("section", null, "sw-deliveries");
  section.setAttribute("aria-label", t("交付文件"));
  const heading = node("div", null, "sw-deliveries-heading");
  heading.append(node("h3", t("交付文件") + (data.rows.length ? " · " + data.rows.length : "")),
    button("↻", () => void loadDeliveredFiles(state.project, { force: true }), "sw-deliveries-refresh"));
  heading.lastElementChild.setAttribute("aria-label", t("刷新交付文件"));
  section.append(heading);
  if (data.error) section.append(node("p", t("交付读取失败：") + data.error, "sw-error"));
  if (!data.rows.length && !data.error) section.append(node("p", data.loaded ? t("暂无交付文件") : t("正在读取交付文件…"), "sw-evidence-meta"));
  const grid = node("div", null, "sw-delivery-grid");
  for (const row of data.rows) {
    const card = renderFileCard(row, { projectDir: state.project });
    const line = developmentState(state).lines.find(line => line.session_id === row.session_id);
    const origin = node("span", [line?.label, row.created_at ? new Date(row.created_at).toLocaleString() : ""].filter(Boolean).join(" · "), "file-card-origin");
    card.append(origin); grid.append(card);
  }
  section.append(grid); host.append(section);
}
function renderBatchEvidence(host, state) {
  const data = state.batchEvidence, section = node("section", null, "sw-batch-evidence");
  section.setAttribute("aria-label", t("批次记录"));
  section.append(node("h3", t("批次记录")), node("p", t("交付记录与 Git 提交记录；任务验收以实际验证和试用结果为准。"), "sw-evidence-meta"));
  if (data.error) section.append(node("p", t("批次记录读取失败：") + data.error, "sw-error"), button(t("重试读取批次记录"), () => void loadBatchEvidence(state, true)));
  if (!data.rows.length && !data.error) section.append(node("p", data.loaded ? t("尚无已登记批次记录") : t("正在读取批次记录…"), "sw-evidence-meta"));
  for (const row of data.rows) {
    const label = row.kind === "git_checkpoint" ? t("Git 提交已记录") : t("交付记录已登记");
    section.append(button([label, row.entry_id, row.commit?.slice(0, 12), row.summary || row.target].filter(Boolean).join(" · "),
      () => showEvidence(label, readableText(row, "", languageIsEnglish()), [row.created_at ? new Date(row.created_at).toLocaleString() : "", row.run_id].filter(Boolean).join(" · "), row.id), "sw-evidence-link"));
  }
  host.append(section);
}
async function loadBatchEvidence(state, force = false) {
  const data = state.batchEvidence;
  if (data.request || data.loaded && !force) return data.request;
  data.request = invoke("batch_evidence", { projectDir: state.project }).then(rows => {
    data.rows = Array.isArray(rows) ? rows : []; data.loaded = true; data.error = "";
  }).catch(error => { data.error = String(error); data.loaded = false; })
    .finally(() => { data.request = null; if (current === state) paint(state.module === "batch"); });
  return data.request;
}
onDeliveriesChanged(project => {
  if (current && sameProject(current.project, project)) paint(current.module === "batch");
});
async function openList(state, kind) {
  if (!await openProjectSpace(state.project, "documents")) return;
  if (sameProject(currentProject, state.project) && document.body.dataset.view === "documents") $("documents-tab-" + kind)?.click();
}
function renderCapture(host, state) {
  const draft = state.capture, form = node("form", null, "sw-capture-form");
  const input = node("textarea"); input.rows = 3; input.value = draft.text;
  input.placeholder = draft.kind === "req" ? t("描述要实现的需求…") : t("描述遇到的问题…");
  input.setAttribute("aria-label", input.placeholder); input.addEventListener("input", () => { draft.text = input.value; });
  const submit = button(draft.busy ? t("记录中…") : t("记录")), cancel = button(t("取消"), () => { state.capture = null; paint(true); });
  submit.type = "submit"; submit.disabled = input.disabled = cancel.disabled = Boolean(draft.busy);
  const controls = node("div"); controls.append(submit, cancel);
  form.append(input, controls); if (draft.error) form.append(node("p", draft.error, "sw-error"));
  form.addEventListener("submit", async event => {
    event.preventDefault(); if (draft.busy || !draft.text.trim()) return;
    draft.busy = true; draft.error = ""; paint(true);
    try {
      const receipt = await invoke("quick_req", { projectDir: state.project, description: draft.text, kind: draft.kind });
      if (String(receipt).includes("部分登记失败:")) { await refreshCurrent(true); throw new Error(String(receipt)); }
      if (state.capture === draft) state.capture = null;
      toast(t("已记录")); await refreshCurrent(true);
    } catch (error) { draft.error = String(error); }
    finally { draft.busy = false; if (current === state) paint(true); }
  });
  host.append(form);
}
async function prepareRunLine(destination) {
  const state = current;
  if (!state || !sameProject(state.project, destination.project)) return null;
  const previous = target(state), selection = { ...state.selection };
  if (!await openProjectSpace(destination.project, "project", { activate: true })) return null;
  if (current !== state || !visible()) return null;
  const candidates = processItems.filter(p => p.profile !== "research");
  const line = destination.id ? candidates.find(p => p.id === destination.id) : candidates[0]
    || await create_workspace_process(null, () => current === state && visible());
  if (!line || destination.session_id && line.session_id !== destination.session_id) throw new Error(t("对话已变化，请刷新后重试。"));
  if (line.id !== activeProcessId) await switchProcess(line.id);
  if (current !== state || !visible() || activeProcessId !== line.id) return null;
  const next = { ...previous, processId: line.id, sessionId: line.session_id };
  adoptRecipient(state, previous, next);
  bindSelection(state, previous.module, { ...selection, processId: line.id, sessionId: line.session_id });
  const lines = state.detail?.lines || state.summary?.lines;
  if (lines && !lines.some(item => item.id === line.id)) lines.push(line);
  mount(state);
  return line;
}
async function refreshCurrent(force = false) {
  const state = current;
  if (!state || (!visible() && !force)) return;
  if (state.request) return state.request;
  if (force) state.loadedConversation.delete(conversationKey(target(state)));
  if (force) void loadDeliveredFiles(state.project, { force: true });
  if (force) { void loadBatchEvidence(state, true); void loadTrace(state, true); }
  state.request = (async () => {
    try {
      const [data, docs] = await Promise.all([
        invoke("workspace_snapshot", { projectDir: state.project }),
        invoke("docs_snapshot", { projectDir: state.project }).catch(() => null),
      ]);
      const detail = data.projects?.find(p => sameProject(p.path, state.project));
      if (!detail) throw new Error(t("项目状态不可用"));
      if (detail.error) throw new Error(detail.error);
      state.detail = detail; state.docs = docs; projectDetails.set(detail.path, detail); state.error = ""; state.runtimeUnavailable = false;
      reconcileSelection(state);
      state.backlog = docs ? { req: backlogTally(docs.requirements || [], "req"), defect: backlogTally(docs.defects || [], "defect") } : { error: true };
      if (!state.selection) bindSelection(state, "main");
      await refreshQuestions();
    } catch (error) { state.error = String(error); state.runtimeUnavailable = true; }
    finally { state.request = null; if (current === state) paint(); }
  })();
  return state.request;
}
async function refreshQuestions() {
  if (questionsRequest) return questionsRequest;
  const waitRevision = questionWaitRevision;
  let stale = false;
  questionsRequest = invoke("softwire_questions").then(rows => {
    const incoming = Array.isArray(rows) ? rows : [];
    if (!replacePendingQuestions(incoming, waitRevision)) { stale = true; return; }
    if (notificationBaseline && incoming.some(q => !seenQuestions.has(q.sessionId + ":" + q.id))) toast(t("有新的待我处理事项"));
    for (const q of incoming) seenQuestions.add(q.sessionId + ":" + q.id);
    questions = incoming; notificationBaseline = true; setWorkbenchQuestions(incoming);
    const count = allMessages().length;
    $("workbench-attention-count").textContent = count ? String(count) : "";
    paint();
    void presentForegroundQuestion();
  }).catch(error => { if (current) current.error = t("待我处理读取失败：") + error; })
    .finally(() => { questionsRequest = null; if (stale) void refreshQuestions(); });
  return questionsRequest;
}
async function loadInbox() {
  if (inboxRequest) return inboxRequest;
  inboxRequest = (async () => {
    try {
      const result = await invoke("workspace_snapshot", { projectDir: null });
      for (const p of result.projects || []) if (!p.error) projectDetails.set(p.path, p);
      await refreshQuestions(); paint(true);
    } catch (error) { toastError(String(error)); }
    finally { inboxRequest = null; }
  })();
  return inboxRequest;
}
function openInteraction(message, nativeTarget = null) {
  if (message.kind === "question") presentedQuestions.add(message.key);
  if (nativeTarget) questionOrigins.set(message.key, nativeTarget);
  if (!current.interaction) current.returnTo = { tab: current.tab, module: current.module, selection: { ...current.selection }, evidence: current.evidence, scroll: surface().scrollTop };
  const origin = nativeTarget || questionOrigins.get(message.key);
  if (origin) current.returnTo.nativeTarget = origin;
  current.interaction = structuredClone(message); current.evidence = null; current.tab = "inbox";
  paint(true); surface().scrollTop = 0;
}
// 普通 question 正在等当前对话的回答,直接呈现可作答详情;后台问题仍只进待处理。
// 已看过/收起的同一版本不因轮询重开,回复始终使用原事项的项目、会话与 revision。
async function presentForegroundQuestion() {
  if (!initialized || presentingQuestion || document.hidden || active_space !== "dev") return;
  const view = document.body.dataset.view;
  if (!["chat", "project"].includes(view) || visible() && current?.interaction) return;
  const project = visible() ? current?.project : currentProject;
  // 讨论不参加开发概览的需求选择;它的提问仍属于原 native 会话。
  const nativeTarget = view === "chat" || sameProject(project, currentProject)
    && processItems.find(line => line.id === activeProcessId)?.profile === "readonly"
    ? { project: currentProject, processId: activeProcessId, sessionId: activeSessionId } : null;
  const sessionId = nativeTarget?.sessionId || current?.selection?.sessionId || work(current).line?.session_id;
  if (!project || !sessionId) return;
  const message = allMessages().find(m => m.kind === "question" && !m.source?.background && !m.source?.agentId
    && sameProject(m.project, project) && m.sessionId === sessionId && !presentedQuestions.has(m.key));
  if (!message) return;
  presentingQuestion = true;
  try {
    if (view === "chat" && !await openProjectSpace(project, "project")) return;
    if (!visible() || !current || current.interaction || !sameProject(current.project, project)
      || (nativeTarget ? !sameProject(currentProject, nativeTarget.project) || activeProcessId !== nativeTarget.processId || activeSessionId !== sessionId
        : (current.selection?.sessionId || work(current).line?.session_id) !== sessionId)
      || !allMessages().some(m => m.key === message.key)) return;
    openInteraction(message, nativeTarget);
  } catch (error) { toastError(String(error)); }
  finally { presentingQuestion = false; }
}
function returnFromMessage() {
  if (!current) return;
  const saved = current.returnTo;
  current.interaction = null; current.returnTo = null;
  if (saved) Object.assign(current, { tab: saved.tab, module: saved.module, selection: saved.selection, evidence: saved.evidence });
  else { current.tab = "network"; current.module = null; }
  paint(true); surface().scrollTop = saved?.scroll || 0;
  if (saved?.nativeTarget && sameProject(currentProject, saved.nativeTarget.project)
    && activeProcessId === saved.nativeTarget.processId && activeSessionId === saved.nativeTarget.sessionId) navigate_view("chat");
  else void presentForegroundQuestion();
}
function editReply() {
  if (!current?.interaction || sending.has(current.interaction.key) || replies.get(current.interaction.key)?.done) return;
  editingReplies.add(current.interaction.key);
  paint(); $("prompt").focus();
}
async function chooseReply(text, { edit = false } = {}) {
  const state = current, message = state?.interaction;
  if (!message || sending.has(message.key) || replies.get(message.key)?.done) return;
  const capture = composer.capture(), multi = isMultiReply(message);
  // Selecting an actual answer can replace the old UI's invalid approval placeholder.
  // Substantive drafts and explanations still require an explicit Send.
  if (isLegacyReviewPlaceholder(message, capture.text)) capture.text = "";
  const manual = edit || multi || editingReplies.has(message.key) || Boolean(capture.text.trim()) || capture.attachments.length > 0;
  if (multi) {
    const lines = capture.text ? capture.text.split("\n") : [];
    const index = lines.indexOf(text);
    if (index >= 0) lines.splice(index, 1); else lines.push(text);
    composer.replace(lines.join("\n"));
  } else if (manual) {
    if (interactionKind(message) === "question") {
      const labels = new Set((message.choices || []).map(option => typeof option === "string" ? option : option.label));
      const lines = capture.text ? capture.text.split("\n") : [];
      const previous = lines.findIndex(line => labels.has(line));
      const remaining = lines.filter(line => !labels.has(line));
      remaining.splice(previous < 0 ? remaining.length : previous, 0, text);
      composer.replace(remaining.join("\n"));
    } else if (!capture.text.split("\n").includes(text)) composer.insert(text);
  } else composer.replace(text);
  if (manual) { editReply(); return; }
  // Use exactly the visible choice and the captured original recipient/revision.
  await sendReply(state, composer.capture());
}
async function sendReply(state, capture) {
  const message = structuredClone(state.interaction), key = message.key;
  if (sending.has(key) || replies.get(key)?.done) return;
  const previous = replies.get(key);
  const requestId = previous?.text === capture.text ? previous.requestId : crypto.randomUUID();
  const result = { requestId, text: capture.text, status: "sending", done: false, error: "" };
  replies.set(key, result); sending.add(key); paint();
  try {
    if (capture.attachments.length) throw new Error(t("这类事项只能回复文字；附件与草稿已保留。"));
    if (message.kind === "question") {
      await invoke("softwire_answer_question", { projectDir: message.project, sessionId: message.sessionId, id: message.id,
        expectedRevision: String(message.revision), requestId, reply: capture.text });
      noteQuestionReply(message.sessionId, message.id);
      result.status = "delivered";
    } else if (message.kind === "decision") {
      if (isLegacyReviewPlaceholder(message, capture.text)) throw new Error(t("这条消息需要具体回答，请选择选项或补充说明。"));
      const action = interactionKind(message) === "decision" && [("本次通过"), t("本次通过")].includes(capture.text.trim()) ? "accept" : "correct";
      const project = projectDetails.get(message.project);
      const response = await invoke("decision_review", { projectDir: message.project, decisionId: message.id,
        agent: lineAgent(project?.lines?.find(line => line.session_id === message.sessionId)).agent,
        review: { request_id: requestId, expected_revision: message.revision, action, feedback: action === "accept" ? "" : capture.text, scope: "once" } });
      if (response.delivery?.error) throw new Error(t("回复已保存，恢复执行失败：") + response.delivery.error);
      result.status = action === "accept" ? "reviewed" : response.delivery?.status === "started" ? "resumed" : response.delivery?.status === "consumed" ? "delivered" : "queued";
    } else if (["验收通过", t("验收通过")].includes(capture.text.trim())) {
      await invoke("work_delivery_accept", { projectDir: message.project, unitId: message.id, sourceSequence: message.revision });
      result.status = "reviewed";
    } else {
      const latest = await invoke("workspace_snapshot", { projectDir: message.project });
      const unit = latest.projects?.find(p => sameProject(p.path, message.project))?.work_units?.find(u => u.unit_id === message.id);
      if (!unit || unit.source_sequence !== message.revision || unit.status !== "done" || unit.claimed_by !== message.processId)
        throw new Error(t("交付版本已变化，请查看最新事项后再回复。"));
      if (!message.processId || !message.sessionId) throw new Error(t("原执行的对话已不可用；修改意见已保留。"));
      await dispatchPrompt({ project: message.project, processId: message.processId, sessionId: message.sessionId }, capture);
      result.status = "queued";
    }
    result.done = true; composer.clear(capture); void refreshQuestions();
  } catch (error) {
    result.status = "failed";
    // question_expired: 是后端的机器前缀(冒烟用它判过期),界面只给人话部分。
    result.error = String(error).includes("missing facts require a reply") ? t("这条消息需要具体回答，请选择选项或补充说明。") : String(error).replace(/^(?:Error:\s*)?question_expired:\s*/, "");
  }
  finally { sending.delete(key); if (current === state) paint(); }
}
async function refreshInteraction() {
  const state = current, previous = state?.interaction;
  if (!previous) return;
  await loadInbox();
  if (current !== state || state.interaction !== previous) return;
  const latest = allMessages().find(m => m.kind === previous.kind && m.id === previous.id && sameProject(m.project, previous.project) && m.sessionId === previous.sessionId);
  if (!latest) { toast(t("这条事项已处理或已结束，草稿仍保留。")); return; }
  // Keep the user's text when explicitly rebasing onto the displayed revision.
  const draft = composer.value();
  state.interaction = structuredClone(latest); paint(true);
  composer.replace(draft);
}
function adoptRecipient(state, previous, next) {
  if (previous.processId === next.processId && previous.sessionId === next.sessionId) return;
  const oldKey = conversationKey(previous), newKey = conversationKey(next);
  if (state.messages.has(oldKey)) { state.messages.set(newKey, [...(state.messages.get(newKey) || []), ...state.messages.get(oldKey)]); state.messages.delete(oldKey); }
  if (state.loadedConversation.has(oldKey)) { state.loadedConversation.delete(oldKey); state.loadedConversation.add(newKey); }
  composer.rebind(previous, next);
  if (state.selection?.processId === previous.processId && state.selection?.sessionId === previous.sessionId) {
    state.selection = { ...state.selection, processId: next.processId, sessionId: next.sessionId };
    if (current === state) paint();
  }
}
async function dispatchPrompt(destination, capture, resolved) {
  const lines = await invoke("process_list", { projectDir: destination.project });
  const candidates = lines.filter(p => p.profile !== "research");
  let line = destination.processId ? candidates.find(p => p.id === destination.processId) : candidates[0];
  if (destination.processId && (!line || destination.sessionId && line.session_id !== destination.sessionId))
    throw new Error(t("原对话已变化，请重新选择对话。草稿已保留。"));
  if (!line) line = await invoke("process_create", { projectDir: destination.project, profile: "dev", phasePipeline: false });
  if (!line?.id || line.profile === "research") throw new Error(t("开发对话创建失败，草稿已保留。"));
  if (line) {
    const bound = { ...destination, processId: line.id, sessionId: line.session_id };
    resolved?.(destination, bound, line); destination = bound; capture.target = bound;
  }
  const starting = !line.running && !sessionStates.get(line.session_id)?.running;
  if (starting) transitionSession(line.session_id, "starting", { local_start_pending: true });
  try {
    await invoke("run_prompt", { prompt: capture.text, projectDir: destination.project,
      processId: destination.processId || null, ...lineAgent(line), researchTopic: line?.research_topic || undefined,
      delivery: "queue", model: line?.model || null, attachments: capture.attachments,
      autonomous: false, autoAllow: autoAllowEnabled() });
    if (starting && sessionStates.get(line.session_id)?.phase === "starting") transitionSession(line.session_id, "running", { local_start_pending: false, stage: "请求" });
  } catch (error) {
    if (starting && sessionStates.get(line.session_id)?.phase === "starting") transitionSession(line.session_id, "failed");
    throw error;
  }
}
async function sendCurrent() {
  if (!current || !composer.active()) return;
  const state = current, capture = composer.capture(), destination = capture.target;
  if (destination.readOnly || destination.completed || (!capture.text.trim() && !capture.attachments.length)) return;
  if (destination.interactionId) { await sendReply(state, capture); return; }
  const key = targetKey(destination);
  if (sending.has(key)) return;
  const rows = messagesFor(state, destination);
  const message = { id: crypto.randomUUID(), role: "user", text: capture.text, files: capture.attachments.map(file => file.file_name), status: "sending" };
  rows.push(message); sending.add(key);
  state.tab = destination.module === "main" ? "network" : "conversation"; state.evidence = null; paint(true);
  try {
    // The request always uses the captured project/process; no delayed UI state can retarget it.
    await dispatchPrompt(destination, capture, (previous, next, line) => {
      sending.add(targetKey(next));
      const lines = state.detail?.lines || state.summary?.lines;
      if (lines && !lines.some(item => item.id === line.id)) lines.push(line);
      adoptRecipient(state, previous, next);
    });
    message.status = destination.running ? "queued" : "delivered";
    composer.clear(capture);
  } catch (error) { message.status = "failed"; message.error = String(error); }
  finally { sending.delete(key); sending.delete(targetKey(capture.target)); if (current === state) paint(); void refreshCurrent(); }
}
// 运行用量(概览第 7 行):run_metrics 读最近 20 轮;30 秒内不重复读,一轮结束(done/idle/stopped)后强制刷新。
async function loadUsage(state, force = false) {
  const data = state.usage;
  if (data.request) return data.request;
  if (!force && data.at && performance.now() - data.at < 30000) return;
  data.request = invoke("run_metrics", { projectDir: state.project, limit: 20 }).then(result => {
    data.rounds = Array.isArray(result?.rounds) ? result.rounds : []; data.loaded = true; data.error = "";
  }).catch(error => { data.error = String(error); })
    .finally(() => { data.request = null; data.at = performance.now(); if (current === state) paint(); });
  return data.request;
}
async function loadTrace(state, force = false) {
  const destination = traceDestination(state), key = conversationKey(destination);
  if (!destination.processId) return;
  const previous = state.traces.get(key);
  if (previous?.request || !force && (previous?.loaded || previous?.error)) return previous?.request;
  const data = { loaded: previous?.loaded || false, error: "", request: null };
  state.traces.set(key, data);
  data.request = (async () => { try {
    const traces = await invoke("conversation_trace_get", { projectDir: destination.project, processId: destination.processId, sequence: null });
    for (const trace of traces) for (const event of trace.events || []) {
      if (event.kind === "tool.started" || event.kind === "tool.completed") recordTool(state, { ...event, sessionId: destination.sessionId }, true);
      if (String(event.kind).includes("compact")) state.compactions.set(destination.sessionId, event);
    }
    // The read may overlap new live events. Keep those objects until the whole
    // historical merge finishes, then retain the newest live calls at the tail.
    state.tools = [...state.tools.filter(tool => !tool.live), ...state.tools.filter(tool => tool.live)].slice(-60);
    data.loaded = true;
  } catch (error) { data.error = String(error); }
  finally { data.request = null; if (current === state && conversationKey(traceDestination(state)) === key) paint(state.module && ["tools", "history", "tasks"].includes(state.module)); }
  })();
  return data.request;
}
async function loadConversation(state, destination = { ...target(state) }) {
  const key = conversationKey(destination);
  if (!destination.processId) return;
  if (destination.interactionId || state.loadedConversation.has(key)) return;
  state.loadedConversation.add(key);
  const request = (state.historyRequests.get(key) || 0) + 1;
  state.historyRequests.set(key, request);
  try {
    const history = await invoke("conversation_get", { projectDir: destination.project, processId: destination.processId, beforeSequence: null });
    if (state.historyRequests.get(key) !== request) return;
    state.historyErrors.delete(key);
    const rows = messagesFor(state, destination);
    const projected = history.slice(-40).flatMap((m, i) => (m.text != null ? [m.text] : (m.parts || []).filter(p => p.type === "text").map(p => p.text))
      .filter(Boolean).map((text, part) => ({ id: "history-" + i + "-" + part, role: m.role, text, status: "completed", historical: true,
        recipient: "主对话" })));
    // A refreshed snapshot replaces old history; retain live replies not yet persisted.
    const live = rows.filter(m => !m.historical);
    let overlap = 0;
    for (let length = 1; length <= Math.min(projected.length, live.length); length++) {
      if (projected.slice(-length).every((message, i) => message.role === live[i].role && message.text === live[i].text)) overlap = length;
    }
    const tail = live.slice(overlap), last = projected.at(-1);
    if (last?.role === "assistant" && tail[0]?.role === "assistant") {
      if (last.text.startsWith(tail[0].text)) tail.shift();
      else if (tail[0].text.startsWith(last.text)) projected.pop();
    }
    rows.splice(0, rows.length, ...projected, ...tail);
    if (current === state && conversationKey(target(state)) === key) paint();
  } catch (error) {
    if (state.historyRequests.get(key) !== request) return;
    state.historyErrors.set(key, String(error));
    if (state.tab === "conversation") toastError(String(error));
    if (current === state) paint();
  }
}
function recordTool(state, event, historical = false) {
  let tool = state.tools.find(t => t.id === event.id && t.sessionId === event.sessionId);
  if (!tool) { tool = { id: event.id }; state.tools.push(tool); }
  if (!historical || !tool.live) Object.assign(tool, event, { pending: event.kind === "tool.started", interrupted: false });
  if (!historical) { tool.live = true; tool.observedAt = performance.now(); }
  if (!historical) state.tools = state.tools.slice(-60);
}
function receive(name, payload) {
  const session = payload.sessionId;
  const state = [...states.values()].find(s => s.selection?.sessionId === session || (s.detail?.lines || s.summary?.lines || []).some(l => l.session_id === session));
  if (!state) return;
  if (name === "kz:tool-start" || name === "kz:tool-end") {
    recordTool(state, { ...payload, kind: name === "kz:tool-start" ? "tool.started" : "tool.completed" });
  }
  if (name === "kz:compacted") { state.compactions.set(session, { ...payload }); state.compacting.set(session, false); }
  if (name === "kz:status") state.compacting.set(session, compactionInProgress(payload));
  if (["kz:text", "kz:reasoning", "kz:tool-start", "kz:tool-end"].includes(name)) state.compacting.set(session, false);
  if (name === "kz:text") {
    for (const [key, rows] of state.messages) {
      const identity = JSON.parse(key);
      if (identity[2] !== session) continue;
      let reply = rows.at(-1);
      if (reply?.role !== "assistant" || reply.status === "completed") { reply = { id: crypto.randomUUID(), role: "assistant", text: "" }; rows.push(reply); }
      reply.text += payload.text || payload.delta || ""; reply.status = "streaming";
    }
  }
  if (["kz:tool-start", "kz:turn", "kz:done", "kz:idle", "kz:stopped", "kz:error"].includes(name)) {
    for (const [key, rows] of state.messages) if (JSON.parse(key)[2] === session && rows.at(-1)?.role === "assistant") rows.at(-1).status = "completed";
  }
  if (["kz:turn", "kz:idle", "kz:stopped"].includes(name) || (name === "kz:error" && payload.terminal !== false)) {
    state.compacting.set(session, false);
    for (const tool of state.tools) if (tool.sessionId === session && tool.pending) { tool.pending = false; tool.interrupted = true; }
  }
  if (["kz:done", "kz:idle", "kz:stopped"].includes(name)) setTimeout(() => void loadUsage(state, true), 800);
  if (["kz:idle", "kz:stopped"].includes(name) || (name === "kz:error" && payload.terminal !== false)) {
    for (const line of [...(state.detail?.lines || []), ...(state.summary?.lines || [])]) if (line.session_id === session) { line.running = false; line.stage = name === "kz:error" ? "出错" : "空闲"; }
  }
  if (name === "kz:status") for (const line of state.detail?.lines || []) if (line.session_id === session) { line.stage = payload.stage; line.running = true; }
  if (current === state) { paint(); requestAnimationFrame(() => { if (current === state) paint(); }); }
}
async function openNativeConversation(state) {
  const destination = target(state);
  if (!await openProjectSpace(destination.project, "chat") || !sameProject(currentProject, destination.project) || document.body.dataset.view !== "chat") return;
  if (destination.processId && destination.processId !== activeProcessId) await switchProcess(destination.processId);
}
async function stop() {
  const state = current, destination = { ...target(state) };
  if (destination.interactionId) return;
  const previous = sessionStates.get(destination.sessionId);
  if (previous?.phase === "stopping") return;
  const previousState = previous ? { ...previous } : { phase: "running" };
  try {
    cancelAutoContinueTimer(destination.sessionId); setAutoRounds(destination.sessionId, 0);
    transitionSession(destination.sessionId, "stopping");
    await invoke("stop_run", { projectDir: destination.project, processId: destination.processId });
  } catch (error) {
    if (sessionStates.get(destination.sessionId)?.phase === "stopping") transitionSession(destination.sessionId, previousState.phase, previousState);
    toastError(String(error));
  }
}
defer(() => {
  if (!$("project-overview-content")) return;
  composer = createComposer({ stop, back: returnFromMessage, changed: () => { if (current?.interaction) paint(); } });
  runner = createRunControl({ resolve: prepareRunLine, changed: () => paint() });
  document.addEventListener("kz:before-composer-scope", () => composer.leave());
  initialized = true;
  document.addEventListener("kz:open-work-module", event => {
    const { project, module, capture } = event.detail;
    void openProjectSpace(project, "project").then(ok => {
      if (!ok || !current || !sameProject(current.project, project)) return;
      if (capture) current.capture = { kind: capture, text: "" };
      selectModule(module);
    });
  });
  document.addEventListener("kz:work-surface-switch", event => {
    if (event.detail.view === "project" && current) selectTab("network");
  });
  document.addEventListener("kz:project-overview", event => {
    if (!initialized || !event.detail.project) return;
    event.preventDefault();
    const state = stateFor(event.detail.project);
    state.summary = event.detail.snapshot?.projects?.find(p => sameProject(p.path, state.project)) || state.summary;
    if (visible()) { mount(state); if (!state.detail) void refreshCurrent(); }
  });
  document.addEventListener("kz:view-changed", event => {
    generation += 1;
    if (event.detail.view !== "project") {
      composer.leave(); runner.close();
      if (event.detail.view === "chat") void refreshQuestions();
    }
    else {
      const state = stateFor(workbenchProject()); state.loadedConversation.clear();
      if (sameProject(state.project, currentProject)) {
        const active = processItems.find(line => line.id === activeProcessId && !["research", "readonly"].includes(line.profile));
        if (active && state.selection?.processId !== active.id) {
          state.module = null; state.evidence = null; state.interaction = null; state.tab = "network";
          bindSelection(state, "main", { processId: active.id, sessionId: active.session_id });
        }
      }
      mount(state); void refreshCurrent();
    }
  });
  document.addEventListener("kz:compose-send", event => {
    if (!composer.active() || !visible()) return;
    event.preventDefault(); void sendCurrent();
  });
  document.addEventListener("kz:compose-stop", event => {
    if (!composer.active() || !visible()) return;
    event.preventDefault(); void stop();
  });
  document.addEventListener("kz:work-question", event => {
    event.preventDefault(); void refreshQuestions();
  });
  document.addEventListener("kz:open-work-inbox", event => {
    const project = workbenchProject() || lastWorkspaceSnapshot?.projects?.[0]?.path;
    if (!project) return;
    event.preventDefault();
    void openProjectSpace(project, "project").then(() => { if (current && sameProject(current.project, project)) selectTab("inbox"); });
  });
  for (const name of ["kz:tool-start", "kz:tool-end", "kz:text", "kz:reasoning", "kz:compacted", "kz:turn", "kz:done", "kz:status", "kz:idle", "kz:stopped", "kz:error"]) {
    void listen(name, event => receive(name, event.payload)).catch(error => toastError(String(error)));
  }
  on("kz:question-replied", () => void refreshQuestions());
  on("kz:ask-resolved", () => void refreshQuestions());
  onSubagentChange(() => paint());
  // 切语言:概览的静态骨架(页签、阶段条、各行名)是创建时写的,换语言要整页重建一次。
  document.addEventListener("kz:language", () => {
    if (!root) return;
    const state = current; root = null; painted = "";
    if (state && visible()) { mount(state); paint(true); }
  });
  document.addEventListener("kz:session-state-changed", () => {
    if (visible()) requestAnimationFrame(() => paint());
  });
  document.addEventListener("kz:sessions-changed", () => { void refreshQuestions(); });
  // 概览里的二级页(需求与计划 / 记忆 / 历史… 详情、证据、回复)不进导航栈:Esc / Alt+← / 侧键先退这一级(与页内「←」同一个动作),
  // 回到六行概览,再按一次才回对话。没有二级页开着就不接,走原来的退页。
  onInnerBack(() => {
    if (!visible() || !current) return false;
    const back = surface()?.querySelector(".sw-detail-head .sw-back");
    if (!back) return false;
    back.click();
    return true;
  });
  // 权限请求排队 / 被答复:概览的「在等你」转琥珀、停掉橙色波形要立刻跟上(不等 8 秒轮询)。
  document.addEventListener("kz:asks-changed", () => {
    if (visible()) requestAnimationFrame(() => paint());
  });
  document.addEventListener("visibilitychange", () => {
    document.body.dataset.documentHidden = String(document.hidden);
    if (document.hidden) stopSignals(root);
    if (!document.hidden && visible()) void refreshCurrent();
  });
  window.matchMedia?.("(prefers-reduced-motion: reduce)").addEventListener?.("change", event => { if (event.matches) stopSignals(root); });
  setInterval(() => { if (!document.hidden) { void refreshQuestions(); if (visible()) void refreshCurrent(); } }, 8000);
  setInterval(() => { if (!document.hidden && visible() && current) runner.sync({ ...work(current).line, project: current.project }); }, 1000);
  void refreshQuestions();
});

