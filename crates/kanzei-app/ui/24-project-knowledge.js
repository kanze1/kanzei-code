import { invoke } from "./01-core.js";
import { t } from "./02-i18n.js";

const KINDS = { responsibility: "职责", contract: "接口约束", constraint: "项目约束", environment: "环境", procedure: "操作流程", pitfall: "踩坑", fact: "事实" };
const rendered = new Map();

document.addEventListener("kz:language", () => {
  for (const [host, state] of rendered) renderProjectKnowledge(host, state.knowledge, state.options);
});

// 架构、记忆与文件浏览共用一份层级投影。节点展开时才创建下一级 DOM。
export function renderProjectKnowledge(host, knowledge, { area = null, onMemory = () => {} } = {}) {
  if (!host) return;
  const previous = rendered.get(host);
  const openAreas = new Set(previous?.knowledge.project_root === knowledge?.project_root
    ? [...host.querySelectorAll("details[open][data-area]")].map(item => item.dataset.area) : []);
  host.replaceChildren();
  if (!knowledge) { rendered.delete(host); return; }
  rendered.set(host, { knowledge, options: { area, onMemory } });
  const label = document.createElement("label");
  label.className = "knowledge-switch";
  const toggle = document.createElement("input");
  toggle.type = "checkbox";
  toggle.checked = knowledge.enabled === true;
  const caption = document.createElement("span");
  caption.textContent = t("启用层级项目知识");
  label.append(toggle, caption);
  host.append(label);
  const hint = document.createElement("p");
  hint.className = "dim";
  hint.textContent = t(knowledge.enabled ? "已启用：按模块加载职责、依赖与经验；关闭会保留记忆。" : "复杂项目可启用并初始化；轻量项目默认关闭。已有记忆会保留。");
  host.append(hint);
  toggle.addEventListener("change", async () => {
    const before = knowledge.enabled === true;
    toggle.disabled = true;
    host.setAttribute("aria-busy", "true");
    try {
      const next = await invoke("project_knowledge_configure", {
        projectDir: knowledge.project_root, codeDir: knowledge.code_root, enabled: toggle.checked,
      });
      // 请求期间换了项目/重新渲染时，旧响应不覆盖新的视图。
      if (rendered.get(host)?.knowledge !== knowledge) return;
      for (const [target, state] of rendered) {
        if (state.knowledge.project_root === knowledge.project_root && state.knowledge.code_root === knowledge.code_root) {
          renderProjectKnowledge(target, next, state.options);
        }
      }
      document.dispatchEvent(new CustomEvent("kz:project-knowledge", { detail: { project: knowledge.project_root } }));
    } catch (error) {
      toggle.checked = before;
      const { toastError } = await import("./03-shell.js");
      toastError(`${t("保存失败")}: ${error}`);
    } finally {
      toggle.disabled = false;
      host.removeAttribute("aria-busy");
    }
  });
  if (!knowledge.enabled) return;
  const areas = knowledge.areas ?? [];
  const memories = new Map((knowledge.memories ?? []).map(memory => [memory.id, memory]));
  const children = new Map();
  for (const item of areas) {
    const parent = item.parent ?? "";
    if (!children.has(parent)) children.set(parent, []);
    children.get(parent).push(item);
  }
  const countMemories = (item, seen = new Set()) => {
    if (seen.has(item.id)) return new Set();
    const next = new Set([...seen, item.id]);
    const ids = new Set(item.memory_ids ?? []);
    for (const child of children.get(item.id) ?? []) for (const id of countMemories(child, next)) ids.add(id);
    return ids;
  };
  const line = (parent, text, className = "dim") => {
    const row = document.createElement("p");
    row.className = className;
    row.textContent = text;
    parent.append(row);
    return row;
  };
  const memoryRow = (parent, memory, currentArea = null) => {
    if (!memory) return;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ghost knowledge-memory";
    const link = memory.links?.find(item => item.area === currentArea);
    const inferred = link?.strength === "weak" ? ` · ${t("推断关联")}` : "";
    button.textContent = `${memory.id} · ${t(KINDS[memory.kind] ?? "事实")} · ${memory.title}${inferred}`;
    button.title = `${memory.description}\n${memory.path}\n${memory.source}`;
    button.addEventListener("click", () => onMemory(memory));
    parent.append(button);
    if (memory.status !== "active" || memory.missing_areas?.length) {
      line(parent, `${t("待复核")} · ${memory.status}${memory.missing_areas?.length ? ` · ${memory.missing_areas.join(", ")}` : ""}`);
    }
  };
  const branch = (parent, item, open = false, seen = new Set()) => {
    if (seen.has(item.id)) return;
    const nextSeen = new Set([...seen, item.id]);
    const box = document.createElement("details");
    box.className = "knowledge-area";
    box.dataset.area = item.id;
    const summary = document.createElement("summary");
    summary.textContent = `${item.label} · ${countMemories(item).size} ${t("条记忆")}`;
    summary.title = item.id;
    box.append(summary);
    let built = false;
    const populate = () => {
      if (built || !box.open) return;
      built = true;
      line(box, item.id);
      for (const [label, links] of [["依赖", item.dependencies], ["被依赖", item.dependents]]) {
        if (links?.length) line(box, `${t(label)}: ${links.join(", ")}`);
      }
      for (const purpose of item.purposes ?? []) line(box, `${purpose.path} · ${purpose.text} · ${t("AI 用途摘要")}`);
      for (const id of item.memory_ids ?? []) memoryRow(box, memories.get(id), item.id);
      for (const child of children.get(item.id) ?? []) branch(box, child, false, nextSeen);
      if (!item.memory_ids?.length && !item.purposes?.length && !children.get(item.id)?.length) line(box, t("尚无该模块的经验记录"));
    };
    box.addEventListener("toggle", populate);
    box.open = open || openAreas.has(item.id);
    populate();
    parent.append(box);
  };
  line(host, t("按项目结构展开职责、约束与经验；推断关联保留依据。"));
  const roots = area ? areas.filter(item => item.id === area) : children.get("") ?? [];
  for (const root of roots) branch(host, root, Boolean(area));
  if (!area && knowledge.unassigned?.length) {
    const box = document.createElement("details");
    box.className = "knowledge-area";
    const summary = document.createElement("summary");
    summary.textContent = `${t("项目级与未归类")} · ${knowledge.unassigned.length}`;
    box.append(summary);
    for (const id of knowledge.unassigned) memoryRow(box, memories.get(id));
    host.append(box);
  }
  if (!roots.length && !knowledge.unassigned?.length) line(host, t("项目知识将在探索和修改中积累"));
}

export async function openKnowledgeMemory(project, memory) {
  const [{ navigate_view, currentProject }, scope, ui] = await Promise.all([
    import("./03-shell.js"), import("./03-memory-scope.js"), import("./13-memory.js"),
  ]);
  // 切项目后旧视图按钮不再能把详情写进新项目。
  if (currentProject !== project) return;
  if (!(await ui.confirmLeaveMemoryDetail())) return;
  if ((await import("./03-shell.js")).currentProject !== project) return;
  scope.setMemoryProject(project);
  navigate_view("memory");
  await ui.refreshMemory();
  const entry = await invoke("memory_entry_get", { projectDir: project, scope: "project", id: memory.id });
  const shell = await import("./03-shell.js");
  if (scope.memoryProject !== project || shell.currentProject !== project) return;
  ui.showMemoryDetail("project", entry, { readOnly: Boolean(entry.archived) });
}
