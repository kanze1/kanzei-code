// 工作台复核视图：消费合并后的摘要/详情，操作始终携带原项目/决策身份。
import { invoke } from "./01-core.js";
import { localizedStatusWord, t } from "./02-i18n.js";
import { currentProject, navigate_view, toastError } from "./03-shell.js";
import { lineAgent } from "./08-auto.js";

let consoleTab = "projects";
let projectFilter = "";
let reviewFilter = "pending";
let currentSnapshot = null;
let actions = null;
let focusDecisionId = null;
let view = null;
// A checked box means approval of the version the user actually saw.
const selectedDecisions = new Map();
const drafts = new Map();
const pendingWrites = new Set();
const decisionKey = (project, decision) => `${project.path}\n${decision.id}`;
const node = (tag, text, cls) => {
  const el = document.createElement(tag);
  if (text !== undefined) el.textContent = text;
  if (cls) el.className = cls;
  return el;
};
function button(text, run, cls = "ghost") {
  const el = node("button", text, cls);
  el.type = "button";
  el.addEventListener("click", run);
  return el;
}
function labeledSelect(label, values, selected, change) {
  const el = node("select");
  el.setAttribute("aria-label", label);
  for (const [value, text] of values) {
    const option = node("option", text);
    option.value = value;
    el.append(option);
  }
  el.value = selected;
  el.addEventListener("change", () => change(el.value));
  return el;
}
const pendingDecision = (d) => d.status === "decided" && !d.review;
const correctedUnit = (p, u) => (p.decisions ?? []).some((d) => d.work_unit_id === u.unit_id && d.review?.action === "correct");
const acceptedUnit = (p, u) => (p.work_acceptances ?? []).some((a) => a.unit_id === u.unit_id && a.source_sequence === u.source_sequence);
const pendingDelivery = (p, u) => u.status === "done" && !correctedUnit(p, u) && !acceptedUnit(p, u);

export function workspaceConsoleBusy() {
  return pendingWrites.size > 0;
}

export function workspaceConsoleState() { return { tab: consoleTab, projectFilter, reviewFilter }; }

export function setWorkspaceConsoleState(next) {
  const previous = JSON.stringify(workspaceConsoleState());
  if (["projects", "decisions", "deliveries"].includes(next.tab)) consoleTab = next.tab;
  if (typeof next.projectFilter === "string") projectFilter = next.projectFilter;
  if (["pending", "all"].includes(next.reviewFilter)) reviewFilter = next.reviewFilter;
  rerender();
  if (previous !== JSON.stringify(workspaceConsoleState())) {
    document.dispatchEvent(new CustomEvent("kz:workspace-console-changed", { detail: workspaceConsoleState() }));
  }
}

export function openDecisionReview(projectPath, decisionId) {
  focusDecisionId = decisionId;
  setWorkspaceConsoleState({ tab: "decisions", reviewFilter: "all", projectFilter: projectPath });
  navigate_view("workspace");
  rerender();
}

const setText = (el, value) => { if (el.textContent !== value) el.textContent = value; };
function enterOnce(row) {
  row.classList.add("console-enter");
  row.addEventListener("animationend", () => row.classList.remove("console-enter"), { once: true });
}
function ensureView(root) {
  if (view?.root === root && view.summary.parentNode === root) return view;
  // 计数条(UX-063/B23):原来是四个 32px 大数字,看着像按钮却点不动。现在是紧凑的角标按钮,
  // 点哪个就切到它说的那一页签:正在推进→项目进展,机器验证中/待你试用→交付与验证,待复核决策→决策复核。
  const summary = node("div", undefined, "console-summary");
  const metrics = [];
  for (const [label, target] of [[t("正在推进"), "projects"], [t("机器验证中"), "deliveries"], [t("待你试用"), "deliveries"], [t("待复核决策"), "decisions"]]) {
    const metric = node("button", undefined, "console-metric");
    metric.type = "button";
    metric.dataset.target = target;
    metric.addEventListener("click", () => setWorkspaceConsoleState({ tab: target }));
    const value = node("strong", "0");
    metrics.push(value);
    metric.append(value, node("span", label));
    summary.append(metric);
  }
  const toolbar = node("div", undefined, "console-toolbar");
  const tabs = node("div", undefined, "console-tabs");
  tabs.setAttribute("role", "group");
  tabs.setAttribute("aria-label", t("控制台视图"));
  for (const [key, title] of [["projects", t("项目进展")], ["decisions", t("决策复核")], ["deliveries", t("交付与验证")]]) {
    const tab = button(title, () => setWorkspaceConsoleState({ tab: key }), "console-tab");
    tab.dataset.consoleTab = key;
    tabs.append(tab);
  }
  const projectSelect = labeledSelect(t("筛选项目"), [], projectFilter, (v) => setWorkspaceConsoleState({ projectFilter: v }));
  // 没有「刷新」按钮(B10):页面每 8 秒轮询、事件到达也会刷新,点它只会改一下时间戳。
  toolbar.append(tabs, projectSelect);
  const freshness = node("div", undefined, "console-freshness dim");
  const errors = node("div");
  const decisions = node("section");
  const deliveries = node("section");
  const empty = node("p", t("添加项目后，这里会显示真实进展、交付和决策。"), "console-empty");
  const controls = node("div", undefined, "console-section-head");
  const reviewSelect = labeledSelect(t("决策状态"), [["pending", t("待复核")], ["all", t("全部决策")]], reviewFilter, (v) => setWorkspaceConsoleState({ reviewFilter: v }));
  const accept = button(t("通过所选"), async () => {
    accept.disabled = true;
    const chosen = chosenDecisions();
    for (const entry of chosen) await reviewOne(entry.project, entry.displayed, "accept", null, false);
    await actions.refresh();
  });
  controls.append(reviewSelect, accept, node("span", t("通过只记录本次复核；纠正时可选择记忆范围。"), "dim"));
  const decisionEmpty = node("p", t("暂无待复核决策。自动运行遇到选择时会在这里留档。"), "console-empty");
  decisions.append(controls, decisionEmpty);
  const deliveryEmpty = node("p", t("尚无交付记录。已有的交付内容和验证结果会直接显示在这里。"), "console-empty");
  deliveries.append(node("p", t("机器完成后继续推进独立工作；人工试用单独记账。纠正后的内容需要新的验证。"), "console-explainer dim"), deliveryEmpty);
  root.replaceChildren(summary, toolbar, freshness, errors, decisions, deliveries, empty);
  view = { root, summary, metrics, tabs, projectSelect, freshness, errors, decisions, deliveries, empty,
    reviewSelect, accept, decisionEmpty, deliveryEmpty, decisionRows: new Map(), deliveryRows: new Map() };
  return view;
}

export function renderDecisionConsole(snapshot, callbacks) {
  currentSnapshot = snapshot;
  actions = callbacks;
  const root = document.getElementById("workspace-console");
  if (!root) return;
  const ui = ensureView(root);
  const scroll = document.getElementById("workspace-scroll");
  const scrollTop = scroll?.scrollTop ?? 0;
  const focused = root.contains(document.activeElement) ? document.activeElement : null;
  const selection = window.getSelection?.();
  const selected = selection && !selection.isCollapsed ? selection.anchorNode?.parentElement : null;
  const visibleRow = [...root.querySelectorAll(".console-decision, .console-delivery")].find((row) => !row.hidden && row.getBoundingClientRect().height > 0 && row.getBoundingClientRect().bottom > (scroll?.getBoundingClientRect().top ?? 0));
  const anchor = ui.lastTab === consoleTab ? (focused || (selected && root.contains(selected) ? selected : visibleRow)) : null;
  const anchorTop = anchor?.getBoundingClientRect().top;
  ui.lastTab = consoleTab;
  const projects = snapshot.projects ?? [];
  if (projectFilter && !projects.some((p) => p.path === projectFilter)) projectFilter = "";
  const selectedProjects = projects.filter((p) => !projectFilter || p.path === projectFilter);
  const counts = [
    projects.filter((p) => (p.running_lines ?? 0) > 0).length,
    projects.reduce((n, p) => n + (p.counts?.verifying ?? (p.work_units ?? []).filter((u) => u.status === "verifying").length), 0),
    projects.reduce((n, p) => n + (p.counts?.ready_to_try ?? (p.work_units ?? []).filter((u) => pendingDelivery(p, u)).length), 0),
    projects.reduce((n, p) => n + (p.counts?.decisions ?? (p.decisions ?? []).filter(pendingDecision).length), 0),
  ];
  counts.forEach((count, i) => setText(ui.metrics[i], String(count)));
  for (const tab of ui.tabs.children) tab.setAttribute("aria-pressed", String(tab.dataset.consoleTab === consoleTab));
  const options = [["", t("全部项目")], ...projects.map((p) => [p.path, p.name])];
  const optionSignature = JSON.stringify(options);
  if (ui.projectOptions !== optionSignature) {
    ui.projectSelect.replaceChildren(...options.map(([value, label]) => { const option = node("option", label); option.value = value; return option; }));
    ui.projectOptions = optionSignature;
  }
  ui.projectSelect.value = projectFilter;
  ui.reviewSelect.value = reviewFilter;
  setText(ui.freshness, snapshot.observed_at ? `${t("更新于")} ${new Date(snapshot.observed_at).toLocaleTimeString()}` : t("尚无更新时间"));
  const errors = selectedProjects.filter((p) => p.error && consoleTab !== "projects").map((p) => `${p.name} · ${t("项目读取失败")}: ${p.error}`);
  if (ui.errorSignature !== JSON.stringify(errors)) {
    ui.errors.replaceChildren(...errors.map((error) => node("p", error, "console-error")));
    ui.errorSignature = JSON.stringify(errors);
  }
  ui.empty.hidden = projects.length > 0;
  ui.decisions.hidden = consoleTab !== "decisions";
  ui.deliveries.hidden = consoleTab !== "deliveries";
  const cards = document.getElementById("workspace-projects");
  if (cards) {
    cards.hidden = consoleTab !== "projects";
    cards.classList.add("console-projects");
    for (const card of cards.children) card.hidden = Boolean(projectFilter && card.dataset.path !== projectFilter);
  }
  if (consoleTab === "decisions") renderDecisions(projects);
  if (consoleTab === "deliveries") renderDeliveries(projects);
  // Preserve the reader's position even when a new row / update notice is inserted above it.
  if (scroll && anchor?.isConnected && !anchor.hidden && anchor.getBoundingClientRect().height > 0 && anchorTop !== undefined) {
    const shift = anchor.getBoundingClientRect().top - anchorTop;
    if (shift) scroll.scrollTop = scrollTop + shift;
  }
  if (consoleTab === "decisions" && focusDecisionId) {
    const target = [...ui.decisionRows.values()].find((entry) => entry.displayed.id === focusDecisionId && entry.project.path === projectFilter)?.row;
    if (target) { target.tabIndex = -1; target.focus(); target.scrollIntoView({ block: "nearest" }); focusDecisionId = null; }
  }
}
function rerender() { if (currentSnapshot && actions) renderDecisionConsole(currentSnapshot, actions); }

function chosenDecisions() {
  return [...view.decisionRows.values()].filter((entry) => !entry.row.hidden && !entry.updated
    && pendingDecision(entry.displayed) && selectedDecisions.get(entry.key) === entry.displayed.revision);
}

// Observation times / other projects' heartbeats are deliberately absent from this key.
const decisionVersion = (d) => JSON.stringify([d.revision, d.status, d.question, d.resolution, d.missing_fact, d.review, d.process_id, d.session_id, d.work_unit_id]);
function renderDecisions(projects) {
  const keys = new Set();
  const all = projects.flatMap((project) => (project.decisions ?? []).map((decision) => ({ project, decision })));
  all.sort((a, b) => b.decision.created_at - a.decision.created_at);
  for (const { project, decision } of all) {
    const key = decisionKey(project, decision);
    keys.add(key);
    let entry = view.decisionRows.get(key);
    if (!entry) {
      entry = createDecision(project, decision);
      view.decisionRows.set(key, entry);
      const following = [...view.decisions.querySelectorAll(".console-decision")].find((row) => Number(row.dataset.createdAt) < decision.created_at);
      view.decisions.insertBefore(entry.row, following ?? null);
    }
    entry.project = project;
    entry.latest = decision;
    entry.updated = entry.version !== decisionVersion(decision);
    if (entry.updated) selectedDecisions.delete(key);
    entry.notice.hidden = !entry.updated;
    entry.row.dataset.updatePending = String(entry.updated);
    const d = entry.displayed;
    entry.row.hidden = Boolean(projectFilter && project.path !== projectFilter)
      || (reviewFilter !== "all" && d.review && (d.review.scope === "once" || d.review.preference_id) && !entry.updated);
    syncDecisionControls(entry);
    if (entry.rework && d.review?.rework_input_id) setText(entry.rework, reworkLabel(project.rework?.[d.review.rework_input_id]));
  }
  for (const [key, entry] of view.decisionRows) {
    if (!keys.has(key) && !projects.some((p) => p.path === entry.project.path && (p.error || !Array.isArray(p.decisions)))) {
      entry.row.remove(); view.decisionRows.delete(key); selectedDecisions.delete(key);
    } else if (!keys.has(key)) {
      entry.row.hidden = Boolean(projectFilter && entry.project.path !== projectFilter)
        || (reviewFilter !== "all" && entry.displayed.review && (entry.displayed.review.scope === "once" || entry.displayed.review.preference_id) && !entry.updated);
    }
  }
  const chosen = chosenDecisions();
  setText(view.accept, `${t("通过所选")} (${chosen.length})`);
  view.accept.disabled = !chosen.length || pendingWrites.size > 0;
  view.decisionEmpty.hidden = [...view.decisionRows.values()].some((entry) => !entry.row.hidden);
}

function createDecision(project, d) {
  const key = decisionKey(project, d);
  const row = node("article", undefined, "console-decision");
  enterOnce(row);
  row.dataset.decisionId = d.id;
  row.dataset.projectPath = project.path;
  row.dataset.createdAt = d.created_at;
  const notice = node("div", undefined, "console-update-notice");
  notice.hidden = true;
  const entry = { key, row, notice, project, latest: d, displayed: structuredClone(d), version: decisionVersion(d), body: node("div"), editor: node("div"), composing: false };
  const update = button(t("查看更新"), () => {
    if (entry.composing || pendingWrites.has(key)) return;
    entry.displayed = structuredClone(entry.latest);
    entry.version = decisionVersion(entry.latest);
    const draft = drafts.get(key);
    if (draft) { draft.revision = entry.displayed.revision; draft.requestId = null; }
    renderDecisionBody(entry);
    rerender();
  });
  notice.append(node("span", t("决策已有更新，请查看后重新复核。")), update);
  row.append(notice, entry.body, entry.editor);
  renderDecisionBody(entry);
  return entry;
}

// 来源行(UX-065):项目 · 对话名 · 工作单元。原来直接拼进程 id 的前缀,界面上就是一个裸的「d」。
// 对话名取线路自己的显示名(后端按 用户命名‖首条消息‖类型+序号 给出);找不到那条线时,主对话的进程 id
// 以 d| 开头,就说「主对话」;其余一律不显示,绝不露内部 id。
function decisionOrigin(project, d) {
  const line = (project.lines ?? []).find((item) => (d.process_id && item.id === d.process_id) || (d.session_id && item.session_id === d.session_id));
  const name = line?.title || (line?.label === "默认" ? t("对话") : line?.label);
  return [project.name, name, d.work_unit_id].filter(Boolean).join(" · ");
}

function reworkLabel(status) {
  return { pending: t("修改意见已排入原对话"), promoted: t("修改意见已交给原对话"), running: t("原对话正在修改"), completed: t("修改轮已结束，请查看新的验证结果"), failed: t("修改轮失败"), cancelled: t("修改已取消") }[status] ?? t("修改状态暂不可用");
}

function renderDecisionBody(entry) {
  const { key, project, displayed: d, body: row } = entry;
  row.replaceChildren();
  entry.rework = null;
  const head = node("div", undefined, "console-decision-head");
  const check = node("input");
  entry.check = check;
  check.type = "checkbox";
  check.setAttribute("aria-label", `${t("选择决策")} ${d.question}`);
  check.addEventListener("change", () => {
    if (check.checked) selectedDecisions.set(key, d.revision); else selectedDecisions.delete(key);
    rerender();
  });
  const status = d.review?.action === "correct" ? t("已纠正") : d.review?.action === "accept" ? t("已复核") : d.status === "deciding" ? t("模型决策中") : d.status === "needs_input" ? t("待补充事实") : t("待复核");
  head.append(check, node("strong", d.question), node("span", status, `console-status ${d.status}`));
  row.append(head, node("div", decisionOrigin(project, d), "console-origin dim"));
  if (d.resolution) {
    row.append(node("p", d.resolution.answer, "console-answer"));
    const details = node("dl", undefined, "console-reason");
    details.append(node("dt", t("选择理由")), node("dd", d.resolution.rationale), node("dt", t("影响范围")), node("dd", d.resolution.impact));
    if (d.resolution.preference_refs?.length) details.append(node("dt", t("参考偏好")), node("dd", d.resolution.preference_refs.join(" · ")));
    row.append(details);
  }
  if (d.missing_fact) row.append(node("p", d.missing_fact, "console-missing"));
  if (d.review) {
    if (d.review.feedback) row.append(node("p", `${t("你的纠正")}: ${d.review.feedback}`));
    if (d.review.rework_input_id) {
      entry.rework = node("p", reworkLabel(project.rework?.[d.review.rework_input_id]), "dim console-rework");
      row.append(entry.rework);
    }
    if (d.review.preference_id) row.append(node("p", `${d.review.scope === "global" ? t("全局偏好") : t("项目偏好")} · ${d.review.preference_id}`, "console-preference"));
    else if (d.review.scope !== "once") row.append(button(t("偏好待保存，点击重试"), () => void retryPreference(project, d)));
  }
  const operations = node("div", undefined, "console-operations");
  if (pendingDecision(d)) {
    const accept = button(t("本次通过"), () => void reviewOne(entry.project, entry.displayed, "accept"));
    accept.dataset.decisionAccept = "true";
    operations.append(accept);
  }
  if (d.status !== "deciding") operations.append(button(d.status === "needs_input" ? t("补充事实") : t("纠正决定"), () => {
    drafts.set(key, drafts.get(key) ?? { feedback: "", scope: "once", open: true, revision: d.revision });
    drafts.get(key).open = true;
    syncDecisionControls(entry);
    entry.editor.querySelector("textarea")?.focus();
  }));
  operations.append(button(t("进入原项目"), () => void actions.openProject(project.path)));
  row.append(operations);
}

function syncDecisionControls(entry) {
  const { key, displayed: d } = entry;
  entry.check.checked = selectedDecisions.get(key) === d.revision;
  entry.check.disabled = entry.updated || !pendingDecision(d) || pendingWrites.has(key);
  const draft = drafts.get(key);
  if (draft?.open && !entry.editor.firstChild) {
    const form = node("form", undefined, "decision-editing");
    const input = node("textarea");
    input.value = draft.feedback;
    input.required = true;
    input.maxLength = 8000;
    input.rows = 3;
    input.setAttribute("aria-label", t("希望如何修改"));
    input.placeholder = t("写下希望怎样修改，以及适用的条件。");
    input.addEventListener("compositionstart", () => { entry.composing = true; });
    input.addEventListener("compositionend", () => { entry.composing = false; });
    input.addEventListener("input", () => { draft.feedback = input.value; draft.requestId = null; });
    const scopes = labeledSelect(t("纠正适用范围"), [["once", t("仅本次")], ["project", t("项目偏好")], ["global", t("全局偏好")]], draft.scope, (v) => { draft.scope = v; draft.requestId = null; });
    const submit = node("button", t("保存纠正并排入原对话"), "primary");
    submit.type = "submit";
    submit.disabled = pendingWrites.has(key);
    form.addEventListener("submit", (event) => { event.preventDefault(); if (!entry.composing) void reviewOne(entry.project, entry.displayed, "correct", draft); });
    form.append(input, scopes, submit, button(t("取消"), () => { draft.open = false; rerender(); }));
    entry.editor.append(form);
  }
  if (!draft?.open && entry.editor.firstChild) entry.editor.replaceChildren();
  for (const b of entry.row.querySelectorAll("button")) b.disabled = pendingWrites.has(key) || (b.dataset.decisionAccept === "true" && entry.updated);
}

async function reviewOne(project, decision, action, draft = null, refresh = true) {
  const key = decisionKey(project, decision);
  if (pendingWrites.has(key)) return;
  if (action === "accept" && view?.decisionRows.get(key)?.updated) return;
  const requestId = draft?.requestId || crypto.randomUUID();
  if (draft) draft.requestId = requestId;
  pendingWrites.add(key);
  actions.beforeMutation();
  rerender();
  try {
    const result = await invoke("decision_review", { projectDir: project.path, decisionId: decision.id,
      agent: lineAgent(project.lines?.find(line => line.session_id === decision.session_id)).agent, review: {
      request_id: requestId, expected_revision: draft?.revision ?? decision.revision, action,
      feedback: draft?.feedback.trim() ?? "", scope: draft?.scope ?? "once",
    } });
    if (result.delivery?.error) throw new Error(`${t("回复已保存，恢复执行失败")}: ${result.delivery.error}`);
    const liveProject = currentSnapshot?.projects?.find((p) => p.path === project.path);
    const liveDecision = liveProject?.decisions?.find((d) => d.id === decision.id);
    if (liveDecision && liveDecision.revision <= result.decision.revision) Object.assign(liveDecision, result.decision);
    const entry = view?.decisionRows.get(key);
    if (entry) {
      entry.displayed = structuredClone(result.decision);
      entry.version = decisionVersion(result.decision);
      renderDecisionBody(entry);
    }
    selectedDecisions.delete(key);
    drafts.delete(key);
    if (result.preference_error) toastError(`${t("纠正已保存，偏好保存失败")}: ${result.preference_error}`);
    if (result.decision.review?.preference_id) {
      const affected = new Set([project.path]);
      if (result.decision.review.scope === "global" && currentProject) affected.add(currentProject);
      for (const projectPath of affected) document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project: projectPath } }));
    }
  } catch (error) { toastError(`${t("复核保存失败")}: ${error}`); }
  finally { pendingWrites.delete(key); rerender(); }
  if (refresh) await actions.refresh();
}
async function retryPreference(project, decision) {
  const r = decision.review;
  await reviewOne(project, decision, "correct", { feedback: r.feedback, scope: r.scope, requestId: r.request_id });
}

function renderDeliveries(projects) {
  const items = projects.flatMap((project) => (project.work_units ?? []).map((unit) => ({ project, unit })));
  const keys = new Set();
  for (const { project, unit } of items.sort((a, b) => b.unit.updated_at - a.unit.updated_at)) {
    const key = `${project.path}\n${unit.unit_id}`;
    keys.add(key);
    let entry = view.deliveryRows.get(key);
    if (!entry) {
      const row = node("details", undefined, "console-delivery");
      enterOnce(row);
      row.dataset.projectPath = project.path;
      row.dataset.unitId = unit.unit_id;
      // 折叠行带箭头与项目名(UX-064):原来既没有展开提示也看不出是哪个项目的,主操作还藏在展开后。
      const head = node("summary");
      const chevron = node("span", "▸", "console-chevron");
      chevron.setAttribute("aria-hidden", "true");
      const title = node("strong");
      const projectName = node("span", undefined, "console-delivery-project dim");
      const status = node("span", undefined, "console-status");
      const acceptance = node("span", undefined, "console-status");
      head.append(chevron, title, projectName, status, acceptance);
      const body = node("div");
      row.append(head, body);
      entry = { row, body, title, projectName, status, acceptance, project };
      view.deliveryRows.set(key, entry);
      view.deliveries.append(row);
    }
    entry.project = project;
    entry.row.hidden = Boolean(projectFilter && project.path !== projectFilter);
    const stale = correctedUnit(project, unit);
    const acceptance = stale ? t("纠正后待重新验证") : acceptedUnit(project, unit) ? t("试用已通过") : unit.status === "done" ? t("待你试用") : t("尚未交付");
    const job = (project.verification_jobs ?? []).find((item) => item.id === unit.background_verification?.job_id);
    const version = JSON.stringify([project.name, unit, acceptance, job]);
    if (entry.version === version) continue;
    entry.version = version;
    setText(entry.title, unit.objective);
    setText(entry.projectName, project.name);
    setText(entry.status, localizedStatusWord(unit.status, "unit"));
    setText(entry.acceptance, acceptance);
    entry.acceptance.classList.toggle("stale", stale);
    const row = entry.body;
    row.replaceChildren(node("p", `${project.name} · ${unit.unit_id} · ${unit.base_revision || t("未绑定版本")}`, "dim"));
    if (job) {
      const labels = { queued: t("验证排队中"), running: t("后台验证中"), passed: t("验证通过"), failed: t("验证失败"), timed_out: t("验证超时"), cancelled: t("验证已取消"), interrupted: t("验证已中断"), stale: t("验证结果已过期"), superseded: t("已替代") };
      row.append(node("p", `${labels[job.status] || job.status} · ${job.environment || ""}`, "console-status"));
      row.append(node("p", `${t("冻结版本")}: ${job.source_fingerprint}`, "console-evidence"));
      row.append(node("p", `${t("验证日志")}: ${job.log_path}`, "console-evidence"));
      if (job.error) row.append(node("p", job.error, "console-missing"));
      if (["queued", "running"].includes(job.status)) row.append(button(t("取消验证"), async (event) => {
        const trigger = event.currentTarget;
        trigger.disabled = true;
        actions.beforeMutation();
        try { await invoke("verification_cancel", { projectDir: project.path, jobId: job.id }); }
        catch (error) { toastError(`${t("取消验证失败")}: ${error}`); }
        finally { await actions.refresh(); if (trigger.isConnected) trigger.disabled = false; }
      }));
    }
    if (unit.blocked_reason) row.append(node("p", unit.blocked_reason, "console-missing"));
    for (const evidence of unit.evidence ?? []) row.append(node("p", `${evidence.criterion} · ${(evidence.evidence_refs ?? []).join(" · ")}`, "console-evidence"));
    if (!unit.evidence?.length) row.append(node("p", t("尚无验证结果"), "dim"));
    if (pendingDelivery(project, unit)) row.append(button(t("我已试用，通过"), async (event) => {
      const trigger = event.currentTarget;
      trigger.disabled = true;
      actions.beforeMutation();
      try { await invoke("work_delivery_accept", { projectDir: project.path, unitId: unit.unit_id, sourceSequence: unit.source_sequence }); }
      catch (error) { toastError(`${t("验收保存失败")}: ${error}`); }
      finally { await actions.refresh(); if (trigger.isConnected) trigger.disabled = false; }
    }));
  }
  for (const [key, entry] of view.deliveryRows) {
    if (!keys.has(key) && !projects.some((p) => p.path === entry.project.path && (p.error || !Array.isArray(p.work_units)))) {
      entry.row.remove(); view.deliveryRows.delete(key);
    } else if (!keys.has(key)) entry.row.hidden = Boolean(projectFilter && entry.project.path !== projectFilter);
  }
  view.deliveryEmpty.hidden = [...view.deliveryRows.values()].some((entry) => !entry.row.hidden);
}
