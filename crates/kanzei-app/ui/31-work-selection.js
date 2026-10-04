import { invoke } from "./01-core.js";
import { layoutPref, setLayoutPref } from "./03-layout.js";
import { workChoices } from "./30-workspace-model.js";

const key = (project, process) => JSON.stringify([String(project || "").replace(/\\/g, "/").replace(/^\/\/\?\//, "").replace(/\/$/, "").toLowerCase(), process]);
export function selectedWork(project, process) { return layoutPref("selected_work", key(project, process)) || null; }
export function selectWork(project, process, id) {
  setLayoutPref("selected_work", key(project, process), id || null);
  document.dispatchEvent(new CustomEvent("kz:work-selection", { detail: { project, process, id } }));
}
// Recheck immediately before submission. The visible snapshot can be stale and
// another conversation may have claimed the item since the user opened the picker.
export async function validateSelectedWork(project, process) {
  const id = selectedWork(project, process);
  if (!id) return null;
  const [docs, snapshot] = await Promise.all([invoke("docs_snapshot", { projectDir: project }), invoke("workspace_snapshot", { projectDir: project })]);
  const normalize = value => String(value || "").replace(/\\/g, "/").toLowerCase();
  const lines = snapshot.projects?.find(row => normalize(row.path) === normalize(project))?.lines || [];
  if (selectedWork(project, process) !== id) throw new Error("工作选择已改变，请重新发送。");
  const entry = workChoices([...(docs.requirements || []), ...(docs.defects || [])], lines, process).find(row => row.id === id);
  if (!entry) throw new Error(`${id} 已结束或不再可选，请重新选择工作。`);
  if (!entry.selectable) throw new Error(`${id} ${entry.owner ? "已由其他对话负责" : "已阻塞"}，请重新选择工作。`);
  return id;
}
