import { invoke } from "./00-ipc.js";
import { openDialog, closeSurface } from "./00-surface.js";
import { projectDocumentsChanged, readProjectDocuments } from "./01-project-resources.js";
import { t } from "./02-i18n.js";
import { toastError } from "./03-shell.js";
import { kindLabels } from "./26-work-tree.js";

const el = (tag, label, className = "") => {
  const node = document.createElement(tag); node.textContent = label; node.className = className; return node;
};
const lines = value => value.split(/\r?\n/).map(line => line.trim()).filter(Boolean);

/** Only human intent is collected here; the agent assesses the graph through work tools. */
export function openWorkRevision({ project, entry, unit }) {
  const dialog = el("dialog", "", "k-surface k-dialog work-plan-dialog");
  dialog.setAttribute("aria-label", t("调整方向"));
  const form = el("form", "", "work-plan-form");
  const label = el("label", t("新的方向")), direction = document.createElement("textarea");
  direction.rows = 4; direction.required = true; direction.setAttribute("aria-label", t("新的方向")); label.append(direction);
  const error = el("p", "", "work-plan-error"); error.setAttribute("role", "alert");
  const actions = el("div", "", "work-plan-actions");
  const cancel = el("button", t("取消"), "ghost"); cancel.type = "button";
  const submit = el("button", t("交给 Agent 调整")); submit.type = "submit";
  let busy = false;
  cancel.onclick = () => closeSurface(dialog);
  actions.append(cancel, submit);
  form.append(el("h3", t("调整方向")), el("p", unit.objective, "dim"), label,
    el("p", t("Agent 评估受影响工作，保留有效结果并修正计划"), "dim"), error, actions);
  form.onsubmit = async event => {
    event.preventDefault(); if (busy || !direction.value.trim()) return;
    busy = true; submit.disabled = true; cancel.disabled = true; direction.readOnly = true; error.textContent = "";
    try {
      const sent = await (await import("./26-project-conversations.js")).reviseWorkNode(entry, unit, project, direction.value.trim());
      if (sent === true) closeSurface(dialog);
      else error.textContent = t("未发送，方向草稿已保留");
    } catch (cause) { error.textContent = String(cause); }
    finally { busy = false; submit.disabled = false; cancel.disabled = false; direction.readOnly = false; }
  };
  dialog.append(form); document.body.append(dialog);
  openDialog(dialog, { initialFocus: direction, onEscape: () => { if (!busy) closeSurface(dialog); }, onClose: () => dialog.remove() });
}

/** A reviewable draft stays local until the common backend transaction commits. */
export function openWorkPlan({ project, unit, units }) {
  const dialog = el("dialog", "", "k-surface k-dialog work-plan-dialog");
  dialog.setAttribute("aria-label", t("裂变需求"));
  const form = el("form", "", "work-plan-form");
  const parentInfo = el("p", `${unit.unit_id} · ${unit.objective}`, "dim");
  form.append(el("h3", t("裂变需求")), parentInfo);
  const field = (parent, title, tag = "input", required = true) => {
    const label = el("label", t(title)), input = document.createElement(tag);
    input.required = required; input.setAttribute("aria-label", t(title));
    if (tag === "textarea") input.rows = 2;
    label.append(input); parent.append(label); return input;
  };
  const reason = field(form, "拆分理由", "textarea");
  const list = el("div", "", "work-plan-children"), rows = [];
  const catalog = new Map();
  for (const node of units) if (node.node?.custom_type) catalog.set(node.node.custom_type.name, node.node);
  const types = new Map([...catalog.values()].map((node, index) => [String(index), node]));
  const ancestors = new Set([unit.unit_id]);
  let parent = unit.node?.parent_id;
  while (parent && !ancestors.has(parent)) { ancestors.add(parent); parent = units.find(node => node.unit_id === parent)?.node?.parent_id; }
  let sequence = 0, busy = false;
  function lockDraft() {
    busy = true;
    const enabled = [...form.elements].filter(control => !control.disabled);
    for (const control of enabled) control.disabled = true;
    return () => { busy = false; for (const control of enabled) control.disabled = false; };
  }
  function renderCoverage(coverage, selected = new Set()) {
    coverage.replaceChildren(el("p", t("支撑父级验收")));
    return unit.acceptance.map(criterion => {
      const label = el("label", criterion), input = document.createElement("input"); input.type = "checkbox";
      input.checked = selected.has(criterion);
      label.prepend(input); coverage.append(label); return { criterion, input };
    });
  }
  function addChild() {
    const row = el("fieldset", "", "work-plan-child"), key = `child${++sequence}`;
    row.append(el("legend", `${t("子节点")} ${sequence}`));
    const objective = field(row, "工作目标");
    const kind = field(row, "节点类型", "select");
    for (const [value, label] of Object.entries(kindLabels)) kind.append(new Option(t(label), value));
    kind.value = "requirement";
    const acceptance = field(row, "完成条件，可稍后补充", "textarea", false);
    const coverage = el("div", "", "work-plan-coverage"), covers = renderCoverage(coverage);
    row.append(coverage);
    const advanced = el("details", "", "work-plan-advanced");
    advanced.append(el("summary", t("更多设置")));
    const custom = field(advanced, "扩展类型", "select", false);
    custom.append(new Option(t("使用基础类型"), ""));
    for (const [value, node] of types) custom.append(new Option(node.custom_type.name, value));
    custom.append(new Option(t("声明新类型"), "__new__"));
    const definition = el("div", "", "work-plan-type-definition"); definition.hidden = true;
    const name = field(definition, "类型名称"), description = field(definition, "类型定义", "textarea"), deliverable = field(definition, "预期产物", "textarea");
    name.maxLength = 40;
    for (const control of [name, description, deliverable]) control.disabled = true;
    custom.onchange = () => {
      definition.hidden = custom.value !== "__new__";
      for (const control of [name, description, deliverable]) control.disabled = definition.hidden;
      const existing = types.get(custom.value);
      kind.disabled = Boolean(existing); if (existing) kind.value = existing.kind;
    };
    advanced.append(definition);
    const verification = field(advanced, "验证方法", "textarea", false);
    const dependencies = field(advanced, "依赖结果", "select", false); dependencies.multiple = true;
    row.append(advanced);
    const remove = el("button", t("移除子节点"), "ghost"); remove.type = "button";
    const item = { key, row, objective, kind, custom, name, description, deliverable, acceptance, verification, coverage, covers, dependencies };
    remove.onclick = () => { rows.splice(rows.indexOf(item), 1); row.remove(); refreshDependencies(); };
    row.append(remove); rows.push(item); list.append(row); refreshDependencies();
    objective.focus();
  }
  function refreshDependencies() {
    for (const item of rows) {
      const selected = new Set([...item.dependencies.selectedOptions].map(option => option.value));
      item.dependencies.replaceChildren();
      const candidates = [...units.filter(node => !ancestors.has(node.unit_id) && node.status !== "superseded").map(node => ({ id: node.unit_id, title: node.objective })),
        ...rows.filter(other => other !== item).map(other => ({ id: `@${other.key}`, title: other.objective.value || `${t("子节点")} ${other.key.slice(5)}` }))];
      for (const candidate of candidates) item.dependencies.append(new Option(candidate.title, candidate.id, false, selected.has(candidate.id)));
    }
  }
  list.addEventListener("change", event => { if (rows.some(row => row.objective === event.target)) refreshDependencies(); });
  form.addEventListener("invalid", event => {
    const details = event.target.closest("details");
    if (details) details.open = true;
  }, true);
  const add = el("button", t("添加子节点"), "ghost"); add.type = "button"; add.onclick = addChild;
  const error = el("p", "", "work-plan-error"); error.setAttribute("role", "alert");
  const actions = el("div", "", "work-plan-actions");
  const refresh = el("button", t("重读父节点"), "ghost"); refresh.type = "button";
  refresh.onclick = async () => {
    if (busy) return;
    const unlock = lockDraft();
    try {
      const snapshot = await readProjectDocuments(project, { force: true });
      const current = snapshot.work_units.find(node => node.unit_id === unit.unit_id);
      if (!current) throw new Error(t("父节点已不在活动工作图中"));
      unit = current; units = snapshot.work_units;
      parentInfo.textContent = `${unit.unit_id} · ${unit.objective}`;
      for (const row of rows) row.covers = renderCoverage(row.coverage, new Set(row.covers.filter(cover => cover.input.checked).map(cover => cover.criterion)));
      refreshDependencies();
      error.textContent = t("父节点已更新，请核对覆盖关系后再提交");
    } catch (cause) { error.textContent = String(cause); }
    finally { unlock(); }
  };
  const cancel = el("button", t("取消"), "ghost"); cancel.type = "button"; cancel.onclick = () => closeSurface(dialog);
  const submit = el("button", t("应用裂变"), "primary"); submit.type = "submit";
  actions.append(refresh, cancel, submit); form.append(list, add, error, actions); dialog.append(form); addChild();
  form.onsubmit = async event => {
    event.preventDefault(); if (busy) return;
    error.textContent = "";
    if (!rows.length || rows.some(row => !row.covers.some(cover => cover.input.checked))) {
      error.textContent = t("每个子节点至少支撑一条父级验收"); return;
    }
    const children = rows.map(row => ({ key: row.key, objective: row.objective.value.trim(),
      acceptance: lines(row.acceptance.value), verification: lines(row.verification.value),
      dependencies: [...row.dependencies.selectedOptions].map(option => option.value),
      node: { kind: row.kind.value, covers: row.covers.filter(cover => cover.input.checked).map(cover => cover.criterion),
        ...(row.custom.value ? { custom_type: row.custom.value === "__new__" ? { name: row.name.value.trim(), description: row.description.value.trim(), deliverable: row.deliverable.value.trim() } : types.get(row.custom.value).custom_type } : {}) } }));
    const unlock = lockDraft(); submit.textContent = t("正在提交…");
    try {
      await invoke("work_node_decompose", { projectDir: project, id: unit.unit_id, expectedSequence: unit.source_sequence, reason: reason.value.trim(), children });
      closeSurface(dialog);
      await projectDocumentsChanged(project).catch(cause => toastError(`${t("裂变已提交，但刷新失败")}: ${cause}`));
    } catch (cause) {
      error.textContent = String(cause);
    } finally { unlock(); submit.textContent = t("应用裂变"); }
  };
  document.body.append(dialog);
  openDialog(dialog, { initialFocus: reason, onEscape: () => { if (!busy) closeSurface(dialog); }, onClose: () => dialog.remove() });
}
