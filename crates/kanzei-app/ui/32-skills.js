import { $, confirmDialog, defer, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";

const node = (tag, cls, text) => { const element = document.createElement(tag); if (cls) element.className = cls; if (text != null) element.textContent = text; return element; };
const sourceLabels = { builtin: "内置", personal: "自定义", external: "外部" };
let entries = [], filter = "all", selected = null, busy = false, loading = 0, editing = 0;
let original = "";
const fields = () => ({ name: $("skill-name").value.trim(), description: $("skill-description").value.trim(), instructions: $("skill-instructions").value.trim(), manualOnly: $("skill-manual-only").checked, userInvocable: selected?.userInvocable ?? true });
const signature = () => JSON.stringify(fields());
const dirty = () => !$("skills-editor").hidden && signature() !== original && (!selected || selected.editable);
function error(text = "") { $("skills-editor-error").textContent = text; $("skills-editor-error").hidden = !text; }
function status(text) { $("skills-status").textContent = text; }
function syncEditor() {
  const writable = !selected || selected.editable;
  for (const id of ["skill-name", "skill-description", "skill-instructions"]) {
    $(id).readOnly = !writable || (id === "skill-name" && !!selected);
    $(id).disabled = busy;
  }
  $("skill-manual-only").disabled = busy || !writable;
  for (const id of ["skills-generate", "skills-generation-prompt", "skills-save", "skills-copy", "skills-delete"]) $(id).disabled = busy;
  $("skills-save").hidden = !writable;
  $("skills-copy").hidden = !selected;
  $("skills-delete").hidden = !selected?.editable;
  for (const id of ["skills-refresh", "skills-import", "skills-create", "skills-generate-open"]) $(id).disabled = busy;
}
async function canReplaceDraft() {
  return !dirty() || await confirmDialog({ title: t("放弃尚未保存的修改？"), message: t("当前技能草稿尚未保存。"), okText: t("放弃修改") });
}
function displayEditor(skill = null, generation = false) {
  editing += 1; selected = skill; busy = false; error();
  $("skills-editor").hidden = false;
  $("skills-editor-title").textContent = t(skill ? skill.editable ? "编辑 Skill" : "查看 Skill" : generation ? "AI 生成 Skill" : "创建 Skill");
  $("skills-generation").hidden = !generation;
  $("skills-generation-prompt").value = "";
  $("skill-name").value = skill?.name || "";
  $("skill-description").value = skill?.description || "";
  $("skill-instructions").value = skill?.instructions || "";
  $("skill-manual-only").checked = !!skill?.manualOnly;
  $("skills-editor-source").textContent = skill ? `${t(sourceLabels[skill.source] || "外部")} · $${skill.name}` : t("保存后在所有对话中可用");
  original = signature(); syncEditor(); renderList();
  $("skills-scroll").scrollTop = 0;
  (generation ? $("skills-generation-prompt") : skill ? $("skill-instructions") : $("skill-name")).focus({ preventScroll: true });
}
async function openSkill(name) {
  if (busy || !await canReplaceDraft()) return;
  const token = ++editing;
  try {
    const skill = await invoke("skills_read", { name });
    if (token !== editing) return;
    displayEditor(skill);
  } catch (failure) { if (token === editing) status(String(failure)); }
}
function renderList() {
  const list = $("skills-library-list"); list.replaceChildren();
  const query = $("skills-search").value.trim().toLocaleLowerCase();
  const matches = entries.filter(skill => (filter === "all" || skill.source === filter)
    && `${skill.name} ${skill.displayName} ${skill.description}`.toLocaleLowerCase().includes(query));
  for (const skill of matches) {
    const row = node("div", "skill-library-row"); row.dataset.skillName = skill.name;
    row.classList.toggle("selected", selected?.name === skill.name);
    const details = node("button", "skill-library-details"); details.type = "button";
    const headline = node("span", "skill-library-headline");
    headline.append(node("strong", "", t(skill.displayName || skill.name)), node("small", "skill-source", t(sourceLabels[skill.source] || "外部")));
    details.append(headline, node("span", "dim skill-library-description", t(skill.description)));
    if (skill.manualOnly) details.append(node("small", "dim", t("仅手动调用")));
    details.addEventListener("click", () => void openSkill(skill.name));
    const label = node("label", "skill-switch");
    const toggle = node("input"); toggle.type = "checkbox"; toggle.checked = skill.enabled; toggle.disabled = busy;
    toggle.setAttribute("role", "switch"); toggle.setAttribute("aria-label", `${t("启用技能")} ${skill.name}`);
    label.append(toggle, node("span", "skill-switch-track"));
    toggle.addEventListener("change", async () => {
      if (busy) { toggle.checked = skill.enabled; return; }
      const next = toggle.checked; busy = true; loading += 1; syncEditor();
      for (const input of list.querySelectorAll("input")) input.disabled = true;
      try {
        entries = await invoke("skills_set_enabled", { name: skill.name, enabled: next });
        status(t("全局设置已保存"));
        document.dispatchEvent(new CustomEvent("kz:skills-changed"));
      } catch (failure) { toggle.checked = skill.enabled; status(String(failure)); }
      finally { busy = false; syncEditor(); renderList(); }
    });
    row.append(details, label); list.append(row);
  }
  if (!matches.length) list.append(node("p", "skills-empty dim", t(query || filter !== "all" ? "没有匹配的技能" : "还没有技能，创建或导入一个开始使用。")));
  $("skills-library-list").setAttribute("aria-busy", String(busy));
}
export async function refreshSkills() {
  if (busy) return;
  const token = ++loading; status(t("正在读取…"));
  try {
    const skills = await invoke("skills_list");
    if (token !== loading) return;
    entries = skills;
    status(t("{n} 个技能，{enabled} 个已启用").replace("{n}", entries.length).replace("{enabled}", entries.filter(skill => skill.enabled).length));
    renderList();
  } catch (failure) { if (token === loading) status(String(failure)); }
}
async function saveSkill(event) {
  event.preventDefault(); if (busy || !$("skills-editor-form").reportValidity()) return;
  busy = true; loading += 1; error(); syncEditor(); renderList();
  try {
    const saved = await invoke("skills_save", { draft: fields(), expectedHash: selected?.revision || null });
    displayEditor(saved); status(t("Skill 已保存，所有对话都可使用"));
    document.dispatchEvent(new CustomEvent("kz:skills-changed"));
    await refreshSkills();
  } catch (failure) { error(String(failure)); }
  finally { busy = false; syncEditor(); renderList(); }
}
async function generateSkill() {
  if (busy) return;
  const description = $("skills-generation-prompt").value.trim();
  if (!description) { $("skills-generation-prompt").focus(); return; }
  const token = ++editing; busy = true; error(); syncEditor(); renderList();
  $("skills-generate").textContent = t("正在生成…");
  try {
    const draft = await invoke("skills_generate", { description });
    if (token !== editing) return;
    $("skill-name").value = draft.name; $("skill-description").value = draft.description; $("skill-instructions").value = draft.instructions;
    $("skill-manual-only").checked = !!draft.manualOnly;
    status(t("草稿已生成，检查后保存"));
  } catch (failure) { if (token === editing) error(String(failure)); }
  finally {
    if (token === editing) { busy = false; syncEditor(); renderList(); }
    $("skills-generate").textContent = t("生成草稿");
  }
}
async function importSkill() {
  if (busy || !await canReplaceDraft()) return;
  try {
    const directory = await invoke("export_pick_dir"); if (!directory) return;
    busy = true; loading += 1; syncEditor();
    const skill = await invoke("skills_import", { directory });
    displayEditor(skill); await refreshSkills(); status(t("Skill 已导入，引用资料已保留"));
  } catch (failure) { status(String(failure)); }
  finally { busy = false; syncEditor(); }
}
async function deleteSkill() {
  if (busy || !selected?.editable) return;
  const skill = selected;
  if (!await confirmDialog({ title: t("删除 Skill？"), message: `$${skill.name} · ${t("删除后将从所有对话的技能列表中移除。")}`, okText: t("删除"), danger: true })) return;
  busy = true; loading += 1; syncEditor(); error();
  try {
    entries = await invoke("skills_delete", { name: skill.name, expectedHash: skill.revision });
    editing += 1; $("skills-editor").hidden = true; selected = null; status(t("Skill 已删除"));
    document.dispatchEvent(new CustomEvent("kz:skills-changed"));
  } catch (failure) { error(String(failure)); }
  finally { busy = false; syncEditor(); renderList(); }
}
defer(() => {
  $("skills-refresh").addEventListener("click", () => void refreshSkills());
  $("skills-import").addEventListener("click", () => void importSkill());
  for (const [id, generation] of [["skills-create", false], ["skills-generate-open", true]]) $(id).addEventListener("click", async () => {
    if (!busy && await canReplaceDraft()) displayEditor(null, generation);
  });
  $("skills-editor-close").addEventListener("click", async () => {
    if (busy) return;
    if (!await canReplaceDraft()) return;
    editing += 1; busy = false; selected = null; $("skills-editor").hidden = true; syncEditor(); renderList();
  });
  $("skills-copy").addEventListener("click", async () => {
    if (busy || !selected) return;
    const draft = fields(), source = selected;
    const base = `${draft.name.slice(0, 54)}-custom`; let name = base, n = 2;
    while (entries.some(skill => skill.name === name)) name = `${base}-${n++}`;
    busy = true; loading += 1; syncEditor(); error();
    try {
      const copied = await invoke("skills_duplicate", { name: source.name, newName: name });
      displayEditor(copied);
      $("skill-description").value = draft.description; $("skill-instructions").value = draft.instructions; $("skill-manual-only").checked = draft.manualOnly;
      await refreshSkills(); status(t("已复制为自定义 Skill"));
    } catch (failure) { error(String(failure)); }
    finally { busy = false; syncEditor(); renderList(); }
  });
  $("skills-editor-form").addEventListener("submit", event => void saveSkill(event));
  $("skills-generate").addEventListener("click", () => void generateSkill());
  $("skills-delete").addEventListener("click", () => void deleteSkill());
  $("skills-search").addEventListener("input", renderList);
  for (const button of document.querySelectorAll("[data-skill-filter]")) button.addEventListener("click", () => {
    filter = button.dataset.skillFilter;
    for (const item of document.querySelectorAll("[data-skill-filter]")) item.setAttribute("aria-pressed", String(item === button));
    renderList();
  });
  document.addEventListener("kz:language", () => { renderList(); syncEditor(); });
});
