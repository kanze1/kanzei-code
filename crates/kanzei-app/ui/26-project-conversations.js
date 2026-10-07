import { renderRequirementDocument } from "./26-requirement-contract.js";
import { $, defer, invoke } from "./01-core.js";
import { isGeneralChat } from "./03-general-scope.js";
import { localizedDocStatus, localizedStage, t } from "./02-i18n.js";
import { openPopover, openDialog, closeSurface } from "./00-surface.js";
import { workChoices } from "./30-workspace-model.js";
import { selectedWork, selectWork } from "./31-work-selection.js";
import { currentProject, activeProcessId, activeSessionId, processItems, sessionStates, pendingQuestionSessions, attachments, toast, toastError } from "./03-shell.js";
import { active_space, selected_workspace_process, ensure_workspace_process } from "./03-workspaces.js";
import { switchProcess } from "./09-sessions.js";
import { openProjectSpace, openWorkbenchItem, workbenchProject } from "./12-workbench.js";
import { kindWord, processName } from "./12-session-tree.js";
import { fillTemplate } from "./04-structured-parse.js";
import { backlogTally } from "./12-docs-pages.js";
import { transitionEntryStatus } from "./11-docs-list.js";
import { startNewConversation } from "./15-views-misc.js";
import { sendText, processUpdateQueues } from "./08-compose-runtime.js";
import { lineAgent, awaitingUserSessions } from "./08-auto.js";
import { renderWorkFocus, node, button } from "./25-softwire-view.js";
import { executionActivity, sameProject } from "./25-softwire-model.js";
import { needsRequirementResearch, requirementResearchAction, requirementResearchStatus, requirementStart, registrationFailureDetails, openRequirementResearch } from "./26-requirement-research.js";

// Project facts and conversation drafts have different lifetimes. A failed read
// must never become an empty project, and async replies retain their recipient.
const projects = new Map();
let rail, switcher, onboarding, detailId = null, rendered = "", scheduled = false, handoffScope = null;
const active = () => active_space === "dev" && !isGeneralChat() && document.body.dataset.view === "chat";
const item = () => processItems.find(p => p.id === activeProcessId);
function facts(project = currentProject) {
  if (!projects.has(project)) projects.set(project, { docs: null, snapshot: null, error: "", registrationError: "", failedRegistration: null, busy: false, request: null, capture: null });
  return projects.get(project);
}
const entries = state => [...(state.docs?.requirements || []), ...(state.docs?.defects || [])];
const archivedCount = state => Number(state.docs?.archived?.req || 0) + Number(state.docs?.archived?.defect || 0);
const isNew = state => state.docs && !entries(state).length && !archivedCount(state);
function closeRail() {
  if (rail.classList.contains("is-open")) closeSurface(rail);
  // The same element is a permanent wide-screen rail and a narrow popover.
  // Closing the drawer must not leave the permanent rail hidden on navigation.
  rail.classList.remove("is-open", "hidden");
  rail.removeAttribute("popover");
}
function openRail(anchor) {
  rail.classList.add("is-open");
  openPopover(anchor, rail, { onClose: () => {
    rail.classList.remove("is-open", "hidden");
    rail.removeAttribute("popover");
    // UX-038:Esc/点外关掉交接表单后,表单还留在常驻右栏里替换了需求栏——收起时把栏恢复成需求列表。
    if (handoffScope) { handoffScope = null; detailId = null; rendered = ""; paint(); }
  } });
}

export async function refreshConversationWork(project = currentProject) {
  if (!project || isGeneralChat(project)) return;
  const state = facts(project);
  if (state.request) return state.request;
  state.request = (async () => {
    try {
      const [docs, snapshot] = await Promise.all([
        invoke("docs_snapshot", { projectDir: project }), invoke("workspace_snapshot", { projectDir: project }),
      ]);
      state.docs = docs; state.snapshot = snapshot.projects?.find(p => sameProject(p.path, project)); state.error = "";
      if (sameProject(project, currentProject)) {
        const selected = selectedWork(project, activeProcessId);
        if (selected && state.snapshot?.lines?.some(line => line.id === activeProcessId && line.current_item_id === selected)) selectWork(project, activeProcessId, null);
      }
    } catch (error) { state.error = String(error); }
    finally { state.request = null; if (sameProject(project, currentProject)) { rendered = ""; paint(); } }
  })();
  return state.request;
}

async function returnToSelectedConversation(project = currentProject) {
  if (!await openProjectSpace(project, "chat")) return null;
  const main = selected_workspace_process() || await ensure_workspace_process();
  if (!main) { toastError(t("对话暂不可用，请刷新后重试")); return null; }
  await switchProcess(main.id);
  return currentProject === project && activeProcessId === main.id ? main : null;
}

export async function continueRequirement(entry) {
  const project = currentProject;
  const owner = facts(project).snapshot?.lines?.find(line => line.current_item_id === entry.id);
  const main = selected_workspace_process();
  // Existing independent work retains its owner; opening a card never steals it.
  if (owner && owner.id !== main?.id) {
    await switchProcess(owner.id); toast(t("已打开负责此需求的独立任务")); return;
  }
  const target = await returnToSelectedConversation(project);
  if (!target) return;
  // Starting a managed requirement explicitly selects the autonomous workflow.
  if ($("profile-select").value !== "dev-auto") {
    $("profile-select").value = "dev-auto";
    $("profile-select").dispatchEvent(new Event("change"));
    try { await processUpdateQueues.get(target.id); }
    catch { return; }
    if (currentProject !== project || activeProcessId !== target.id) return;
  }
  const { prompt, ...options } = requirementStart(entry);
  await sendText(prompt, options);
  if (needsRequirementResearch(entry)) toast(t("已开始调研；需要你回答的问题会出现在「待我处理」。"));
  detailId = null; rendered = ""; paint();
}

async function registerRequirement(start, retry = null) {
  const project = currentProject, processId = activeProcessId, input = $("prompt");
  const text = retry?.text || input.value.trim(), state = facts(project);
  if (!text || state.busy) { if (!text) input.focus(); return; }
  if (attachments.length) { toast(t("请先登记文字需求，附件留在输入框，登记后可以一起发送")); return; }
  state.busy = true; state.registrationError = ""; paint();
  try {
    // A successful capture followed by a failed refresh is retryable without
    // creating another requirement from the same original description.
    if (state.capture?.text !== text) {
      const receipt = await invoke("quick_req", { projectDir: project, description: text, kind: "req" });
      const ids = [...String(receipt).matchAll(/^R-\d+/gm)].map(match => match[0]);
      const id = ids[0];
      if (!id) throw new Error(t("未取得需求编号，已保留原文，请查看需求列表"));
      const partial = String(receipt).includes("部分登记失败:");
      state.capture = { text, id, ids, partial, receipt: String(receipt) };
    }
    state.failedRegistration = null;
    await refreshConversationWork(project);
    if (state.capture.partial) {
      state.registrationError = state.capture.receipt;
      state.failedRegistration = { text, start: false };
      state.capture = null;
      return;
    }
    const entry = state.docs?.requirements?.find(entry => entry.id === state.capture.id);
    if (!entry || state.error) {
      state.error = state.error || fillTemplate(t("已登记 {id}，但未读到条目；请重试读取，原文已保留"), { id: state.capture.id });
      return;
    }
    if (currentProject !== project || activeProcessId !== processId) { toast(`${entry.id} ${t("已登记")}`); return; }
    if (input.value.trim() === text) input.value = "";
    const capturedIds = state.capture.ids || [entry.id];
    state.capture = null;
    toast(`${capturedIds.join("、")} ${t("已登记")}`);
    if (start && capturedIds.length === 1) await continueRequirement(entry);
  } catch (error) { state.registrationError = String(error); state.failedRegistration = { text, start }; }
  finally { state.busy = false; if (currentProject === project) { rendered = ""; paint(); } }
}

function showCapture(kind = "req") {
  document.dispatchEvent(new CustomEvent("kz:open-work-module", { detail: { project: currentProject, module: "work", capture: kind } }));
}
function renderDetail(state, entry) {
  rail.replaceChildren();
  rail.append(button("← " + t("需求"), () => { detailId = null; rendered = ""; paint(); }, "ghost"));
  rail.append(node("small", `${entry.id} · ${localizedDocStatus(entry.status)}`), node("h3", entry.title));
  if (entry.id.startsWith("R-")) rail.append(renderRequirementDocument(entry));
  for (const [key, value] of entry.id.startsWith("R-") ? [] : entry.fields || []) {
    if (!["验收", "外部验收", "阻塞", "原始描述", "范围"].includes(key) || !value) continue;
    rail.append(node("h4", t(key)), node("p", value, "project-work-fact"));
  }
  const actions = node("div", null, "project-work-actions");
  if (entry.prior_art) {
    rail.append(node("h4", t(requirementResearchStatus(entry))));
    if (needsRequirementResearch(entry)) rail.append(node("p", t("先完成调研再实施；需要你回答的问题会出现在「待我处理」。"), "project-work-fact"));
    if (entry.prior_art.issue) rail.append(node("p", entry.prior_art.issue, "project-work-fact"));
    if (entry.prior_art.path) actions.append(button(t("查看调研文件"), () => void openRequirementResearch(currentProject, entry).catch(error => toastError(String(error))), "ghost"));
  }
  if (!entry.closed && entry.status !== "awaiting_external") actions.prepend(button(t(entry.status === "draft" ? "补充草稿" : needsRequirementResearch(entry) ? requirementResearchAction(entry) : "继续此需求"), () => void continueRequirement(entry), "primary"));
  if (!entry.closed) {
    const project = currentProject, kind = entry.id.startsWith("D-") ? "defect" : "req";
    if (entry.status === "awaiting_external") actions.prepend(button(t("退回开发"), async () => {
      if (await transitionEntryStatus(entry, kind, kind === "defect" ? "fixing" : "doing", { project })) {
        if (sameProject(project, currentProject)) { detailId = null; rendered = ""; }
        await refreshConversationWork(project);
      }
    }, "primary"));
    actions.append(button(t(kind === "defect" ? "不再修复" : "取消需求"), async () => {
      if (await transitionEntryStatus(entry, kind, kind === "defect" ? "wontfix" : "dropped", { project })) {
        if (sameProject(project, currentProject)) { detailId = null; rendered = ""; }
        await refreshConversationWork(project);
      }
    }, "ghost"));
  }
  actions.append(button(t("完整记录") + " ↗", () => void openWorkbenchItem(currentProject, entry), "ghost"));
  rail.append(actions);
}

// 右栏/页签/按钮是 defer 里建的静态骨架:每次 paint 重写一遍文案,切语言后立刻跟上。
function relabel() {
  rail.setAttribute("aria-label", t("项目需求"));
  switcher.setAttribute("aria-label", t("对话与管理"));
  for (const b of switcher.querySelectorAll("[data-work-surface]")) b.textContent = b.dataset.workSurface === "chat" ? t("对话") : t("管理");
  const req = switcher.querySelector(".project-work-toggle"); if (req) req.textContent = "☷ " + t("需求");
  $("project-return-main").textContent = t("返回对话");
  $("project-handoff").textContent = t("交给其它对话") + " ↗";
  const cancel = $("project-start-actions")?.lastElementChild; if (cancel) cancel.textContent = t("先讨论");
}
function paint() {
  if (!rail) return;
  if (switcher) relabel();
  const discussion = item()?.profile === "readonly";
  document.body.dataset.conversationKind = discussion ? "discussion" : "conversation";
  const current = item();
  $("project-conversation-kind").textContent = current ? processName(current) : "";
  const view = document.body.dataset.view;
  switcher.hidden = active_space !== "dev" || !["chat", "project"].includes(view);
  for (const b of switcher.querySelectorAll("[data-work-surface]")) b.setAttribute("aria-pressed", String(b.dataset.workSurface === view));
  $("project-return-main").hidden = true;
  $("project-handoff").hidden = processItems.filter(p => p.id !== activeProcessId && p.profile !== "research").length === 0;
  const state = facts();
  const activity = executionActivity(item(), sessionStates.get(activeSessionId), { waiting: awaitingUserSessions.has(activeSessionId) || pendingQuestionSessions.has(activeSessionId) });
  const signal = $("project-overview-signal"); signal.dataset.state = activity.state;
  signal.textContent = localizedStage(activity.label); signal.hidden = !item() || discussion;
  if (!active()) return;
  const scope = currentProject + "|" + activeProcessId;
  if (handoffScope === scope) return;
  handoffScope = null;
  const newProject = Boolean(isNew(state) && !state.error && !discussion && !["running", "starting", "stopping", "waiting", "attention"].includes(activity.state));
  const pendingCapture = Boolean(state.capture) && !discussion;
  const capturedEntry = state.docs?.requirements?.find(entry => entry.id === state.capture?.id);
  onboarding.hidden = !newProject && !pendingCapture && !state.registrationError && !(state.error && (!state.docs || isNew(state)));
  $("project-start-actions").hidden = !newProject && !pendingCapture;
  $("project-register").disabled = state.busy;
  $("project-register").textContent = state.busy ? t("登记中…") : pendingCapture ? t(needsRequirementResearch(capturedEntry) ? requirementResearchAction(capturedEntry) : "开始此需求") : t("登记并开始");
  $("project-onboarding-copy").textContent = state.registrationError ? t("登记未完成，原文已保留。") : state.error || (pendingCapture ? fillTemplate(t(needsRequirementResearch(capturedEntry) ? "{id} 已登记，先完成调研再开始实施。" : "{id} 已登记，可以开始推进。"), { id: state.capture.id }) : t("先说说你想做什么，登记成需求后开始推进。"));
  $("project-registration-details").hidden = !state.registrationError;
  $("project-registration-detail-copy").textContent = registrationFailureDetails(state.registrationError);
  $("project-registration-details").querySelector("summary").textContent = t("查看登记详情");
  $("project-onboarding-retry").hidden = !state.error && !state.registrationError;
  $("project-onboarding-retry").textContent = state.registrationError ? t("重试登记") : t("重试读取");
  $("project-onboarding-retry").disabled = state.busy;
  const signature = JSON.stringify([currentProject, activeProcessId, state.docs, state.snapshot?.current_items, state.error, state.registrationError, activity, detailId, selectedWork(currentProject, activeProcessId)]);
  if (signature === rendered) return;
  rendered = signature;
  rail.replaceChildren();
  if (!state.docs) {
    rail.append(node("p", state.error ? t("需求读取失败") : t("读取需求…")));
    if (state.error) rail.append(button(t("重试"), () => void refreshConversationWork(), "ghost"));
    return;
  }
  const all = entries(state), lines = state.snapshot?.lines || [];
  const owner = lines.find(line => line.id === activeProcessId);
  const currentId = owner?.current_item_id || item()?.current_item_id;
  const selectedId = selectedWork(currentProject, activeProcessId);
  const choices = workChoices(all, lines, activeProcessId);
  const currentEntry = all.find(entry => entry.id === currentId);
  const chosenEntry = all.find(entry => entry.id === selectedId);
  const nextEntry = chosenEntry?.id !== currentEntry?.id ? chosenEntry : null;
  const busy = ["running", "starting", "stopping", "waiting"].includes(activity.state);
  const displayedCurrent = busy ? currentEntry : chosenEntry || currentEntry;
  const candidate = (busy && nextEntry) || choices.find(entry => entry.selectable && entry.id !== displayedCurrent?.id);
  for (const [position, label, entry] of [["current", t(busy ? "正在进行" : chosenEntry ? "当前选取" : "当前工作"), displayedCurrent],
    ["next", t(nextEntry && busy ? "已选工作 · 下一轮生效" : "下一个候选"), candidate]]) {
    const slot = node("section", null, "work-focus-slot"); slot.append(node("small", label));
    slot.dataset.position = position;
    slot.dataset.hasWork = String(Boolean(entry));
    if (entry) {
      const link = button("", () => void openWorkbenchItem(currentProject, entry), "work-focus-entry");
      link.title = `${entry.id} · ${entry.title}`;
      link.setAttribute("aria-label", link.title);
      link.append(node("span", entry.id, "work-focus-id"), node("span", entry.title, "work-focus-title"));
      slot.append(link);
    } else slot.append(node("span", t("暂无"), "work-focus-empty"));
    rail.append(slot);
  }
  const actions = node("div", null, "work-focus-actions");
  if (!discussion) actions.append(button(t("选择工作"), () => openWorkPicker(state), "ghost work-focus-choose"));
  actions.append(button(t("全部工作") + " ↗", () => void openProjectSpace(currentProject, "project"), "ghost work-focus-all"));
  rail.append(actions);
  if (state.error) rail.append(button(t("读取失败 · 重试"), () => void refreshConversationWork(), "ghost"));
}

function openWorkPicker(state) {
  const project = currentProject, process = activeProcessId;
  let dialog = $("work-picker");
  if (!dialog) { dialog = node("dialog", null, "k-surface k-dialog work-picker"); dialog.id = "work-picker"; dialog.setAttribute("aria-label", t("选择当前工作")); document.body.append(dialog); }
  dialog.replaceChildren();
  const head = node("header"); head.append(node("strong", t("选择当前工作")), button("×", () => closeSurface(dialog), "ghost"));
  const search = node("input"); search.type = "search"; search.placeholder = t("搜索编号或标题"); search.setAttribute("aria-label", search.placeholder);
  const list = node("div", null, "work-picker-list");
  const choose = id => { if (project === currentProject && process === activeProcessId) selectWork(project, process, id); closeSurface(dialog); rendered = ""; paint(); };
  const draw = () => {
    list.replaceChildren(button(t("自动选择 · 由任务队列决定"), () => choose(null), "ghost"));
    for (const entry of workChoices(entries(state), state.snapshot?.lines || [], process)) {
      if (!`${entry.id} ${entry.title}`.toLowerCase().includes(search.value.toLowerCase())) continue;
      const row = button(`${entry.id} · ${entry.title}`, () => choose(entry.id), "ghost");
      row.disabled = !entry.selectable;
      row.append(node("small", entry.owner ? t("由其他对话负责") : entry.blocked ? t("已阻塞") : localizedDocStatus(entry.status)));
      row.setAttribute("aria-pressed", String(selectedWork(project, process) === entry.id)); list.append(row);
    }
  };
  search.addEventListener("input", draw);
  dialog.append(head, node("p", t("选择在下一次发送或自动续跑时生效；当前轮次保持不变。"), "dim"), search, list); draw();
  openDialog(dialog, { initialFocus: search });
}

/// 交给其它对话的表单(右栏)。默认:当前对话 → 本项目的对话;传 target = { project, items, name } 则交给「其它项目」
/// 的对话(右键菜单「交给其它项目的对话…」,替代跨项目拖动)。提示词里写对话名、不带内部会话 id(UX-038)。
export function openHandoffForm({ target = null } = {}) {
  const project = currentProject, source = item();
  const candidates = (target ? target.items : processItems).filter(p => p.id !== source?.id && p.profile !== "research");
  const initial = candidates[0];
  const toProject = target ? target.project : project;
  if (!source || !initial) return;
  const recipient = node("select"); recipient.setAttribute("aria-label", t("选择对话"));
  for (const candidate of candidates) {
    const option = node("option", processName(candidate)); option.value = candidate.id; recipient.append(option);
  }
  const sourceKind = "conversation";
  const sourceName = processName(source);
  const form = node("form", null, "project-handoff-form");
  const input = node("textarea"); input.required = true; input.placeholder = target ? fillTemplate(t("写下要交给「{name}」对话的结论或下一步…"), { name: target.name }) : t("写下要交给其它对话的结论或下一步…");
  input.setAttribute("aria-label", t("交给其它对话的结论"));
  const selected = String(window.getSelection() || "").trim();
  input.value = selected || $("prompt").value;
  const requirement = node("select"); requirement.setAttribute("aria-label", t("关联需求"));
  const none = node("option", t("不指定需求")); none.value = ""; requirement.append(none);
  // 关联需求是本项目的条目;交给别的项目时没有对应清单,不提供。
  if (!target) for (const entry of entries(facts(project)).filter(e => !e.closed)) {
    const option = node("option", `${entry.id} · ${entry.title}`); option.value = entry.id; requirement.append(option);
  }
  const error = node("p", "", "project-form-error"); error.setAttribute("role", "status");
  const submit = button(target ? t("发送") : t("发送给对话"), () => {}, "primary"); submit.type = "submit";
  form.append(recipient, node("h3", target ? fillTemplate(t("交给「{name}」的对话"), { name: target.name }) : t("交给其它对话")), node("p", t("将附上当前对话上下文；你也可以补充结论或下一步。"), "dim"),
    ...(target ? [] : [requirement]), input, error, submit,
    button(t("取消"), () => { handoffScope = null; detailId = null; rendered = ""; paint(); closeRail(); }, "ghost"));
  rail.replaceChildren(form); openRail($("project-handoff")); input.focus();
  // Keep edits intact while background status and requirement refreshes arrive.
  rendered = "handoff";
  handoffScope = project + "|" + source.id;
  form.addEventListener("submit", async event => {
    event.preventDefault(); if (submit.disabled || !input.value.trim()) return;
    submit.disabled = true;
    const text = input.value.trim();
    const main = candidates.find(p => p.id === recipient.value);
    if (!main) { submit.disabled = false; return; }
    try {
      await invoke("run_prompt", { projectDir: toProject, processId: main.id, ...lineAgent(main), model: main.model || null, delivery: "queue",
        handoffSource: { projectDir: project, processId: source.id },
        prompt: `来自${kindWord(sourceKind)}「${sourceName}」${requirement.value ? `，关联 ${requirement.value}` : ""}。用户明确交给其它对话的结论：\n\n${text}`, executionBatch: true, autonomous: false });
      if (currentProject === project && activeProcessId === source.id) {
        if ($("prompt").value.trim() === text) $("prompt").value = "";
        handoffScope = null; rail.classList.remove("is-open"); rendered = "";
        if (!target) await switchProcess(main.id);
        paint();
      }
      toast(target ? fillTemplate(t("已送给「{name}」的对话，运行中会按队列处理"), { name: target.name }) : t("已送给对话，运行中会按队列处理"));
    } catch (e) { error.textContent = String(e); submit.disabled = false; }
  });
}

defer(() => {
  rail = node("aside", null, "project-chat-work"); rail.id = "project-chat-work";
  rail.setAttribute("aria-label", t("项目需求")); $("chat-area").after(rail);
  switcher = node("nav", null, "project-work-switch"); switcher.id = "project-work-switch"; switcher.setAttribute("aria-label", t("对话与概览"));
  for (const [view, label] of [["chat", t("对话")], ["project", t("概览")]]) {
    const b = button(label, async () => {
      document.dispatchEvent(new CustomEvent("kz:work-surface-switch", { detail: { view } }));
      await openProjectSpace(workbenchProject(), view);
    });
    b.dataset.workSurface = view; switcher.append(b);
  }
  const signal = button("", () => void openProjectSpace(workbenchProject(), "project"), "project-overview-signal"); signal.id = "project-overview-signal";
  const req = button("☷ " + t("需求"), () => { if (rail.classList.contains("is-open")) closeRail(); else openRail(req); }, "project-work-toggle");
  req.setAttribute("aria-controls", rail.id); req.setAttribute("aria-expanded", "false");
  const main = button(t("返回对话"), () => void returnToSelectedConversation(), "ghost"); main.id = "project-return-main";
  const forward = button(t("交给其它对话") + " ↗", () => openHandoffForm(), "ghost"); forward.id = "project-handoff";
  switcher.append(signal, node("span", null, "sw-spacer"), main, forward, req); $("composer").prepend(switcher);
  onboarding = node("section", null, "project-onboarding"); onboarding.id = "project-onboarding";
  const copy = node("p"); copy.id = "project-onboarding-copy";
  const details = node("details"); details.id = "project-registration-details"; details.hidden = true;
  const detailCopy = node("p", "", "project-work-fact"); detailCopy.id = "project-registration-detail-copy";
  details.append(node("summary", t("查看登记详情")), detailCopy);
  const actions = node("div"); actions.id = "project-start-actions";
  const register = button(t("登记并开始"), () => void registerRequirement(true), "primary"); register.id = "project-register";
  actions.append(register, button(t("先讨论"), () => void startNewConversation(), "ghost"));
  const retry = button(t("重试读取"), () => {
    const state = facts();
    if (state.registrationError && state.failedRegistration) void registerRequirement(state.failedRegistration.start, state.failedRegistration);
    else void refreshConversationWork();
  }, "ghost"); retry.id = "project-onboarding-retry";
  onboarding.append(copy, details, actions, retry); $("chat-area").prepend(onboarding);
  document.addEventListener("kz:compose-send", event => {
    if (!active() || item()?.profile === "readonly") return;
    const state = facts();
    if ((!onboarding.hidden && isNew(state)) || state.capture) {
      event.preventDefault();
      if (state.error && !state.capture) { toastError(t("先重试读取需求，输入已保留")); return; }
      void registerRequirement(true);
    }
  });
  const refresh = () => { closeRail(); detailId = null; rendered = ""; paint(); if (active()) void refreshConversationWork(); };
  document.addEventListener("kz:view-changed", refresh);
  document.addEventListener("kz:conversation-selected", refresh);
  // 重命名/删除对话后页头的对话名跟着变(12-session-menus.js 发出)。
  document.addEventListener("kz:sessions-changed", () => paint());
  document.addEventListener("kz:language", () => { rendered = ""; paint(); });
  document.addEventListener("kz:session-state-changed", () => {
    if (scheduled) return; scheduled = true;
    requestAnimationFrame(() => { scheduled = false; paint(); });
  });
  document.addEventListener("kz:tasks-layout", () => paint());
  document.addEventListener("kz:work-selection", () => { rendered = ""; paint(); });
  setInterval(() => { if (active() && !document.hidden && rendered !== "handoff") void refreshConversationWork(); }, 10000);
  paint(); if (active()) void refreshConversationWork();
});
