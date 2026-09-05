import { $, defer, invoke, readJson, uiPrefsLoad, uiPrefsSave, writeJson } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeProcessId, currentProject, navigate_view, processItems, toastError } from "./03-shell.js";
import { refreshProcesses, renderParallelTaskStatus, switchProcess } from "./09-sessions.js";
import { attachments, setAttachments } from "./03-shell.js";
import { renderAttachments } from "./08-compose-runtime.js";
import { sync_research_process_context } from "./19-research.js";

// 导航偏好不承担运行配置；任务的 profile 与课题绑定来自 process_list。
export let active_space = "dev";
export let workspace_switch_pending = false;
export let workspace_preferences = readJson("kz-workspaces", {});
let workspace_save = Promise.resolve();
const dev_views = new Set(["documents", "lines", "arch", "metrics"]);
const composer_drafts = new Map();
let composer_scope = "";

export function sync_composer_scope() {
  const scope = currentProject && activeProcessId ? JSON.stringify([currentProject, activeProcessId]) : "";
  if (scope === composer_scope) return;
  if (composer_scope) composer_drafts.set(composer_scope, { text: $("prompt").value, attachments: [...attachments] });
  composer_scope = scope;
  const draft = composer_drafts.get(scope);
  $("prompt").value = draft?.text || "";
  $("prompt").style.height = "auto";
  setAttachments([...(draft?.attachments || [])]);
  renderAttachments();
}

export function project_workspace(project = currentProject) {
  const saved = workspace_preferences[project] ?? {};
  return {
    ...saved,
    space: saved.space === "research" ? "research" : "dev",
    dev: { view: "chat", ...saved.dev },
    research: { view: "research", page: "overview", topic: "", category: "research", ...saved.research },
  };
}

export function save_workspace(patch, project = currentProject) {
  if (!project) return;
  workspace_preferences[project] = { ...project_workspace(project), ...patch };
  writeJson("kz-workspaces", workspace_preferences);
  workspace_save = workspace_save.then(() => uiPrefsSave({ workspace_state: { ...workspace_preferences } }));
}

export function save_research_workspace(patch) {
  const saved = project_workspace();
  save_workspace({ research: { ...saved.research, ...patch } });
}

export async function restore_workspace_preferences() {
  const saved = await uiPrefsLoad();
  if (saved.workspace_state && typeof saved.workspace_state === "object") {
    workspace_preferences = { ...workspace_preferences, ...saved.workspace_state };
  }
}

export function process_space(item) {
  return item?.profile === "research" ? "research" : "dev";
}

export function preferred_workspace_process(items, space = project_workspace().space) {
  const saved = project_workspace()[space];
  const candidates = items.filter((item) => process_space(item) === space);
  return candidates.find((item) => item.id === saved.process_id)
    ?? (space === "research" ? candidates.find((item) => item.research_topic === saved.topic) : candidates.find((item) => item.id.startsWith("d|")))
    ?? candidates[0];
}

export function workspace_processes(items) {
  const topic = project_workspace().research.topic;
  return (items ?? []).filter((item) => process_space(item) === active_space
    && (active_space !== "research" || (item.research_topic || "") === topic));
}

export function view_allowed(view) {
  return view === "research" ? active_space === "research" : !dev_views.has(view) || active_space === "dev";
}

export function remember_workspace_view(view) {
  const saved = project_workspace();
  save_workspace({ [active_space]: { ...saved[active_space], view } });
}

export function adopt_process_workspace(item) {
  if (!item) return;
  sync_composer_scope();
  active_space = process_space(item);
  const saved = project_workspace();
  const scope = { ...saved[active_space], process_id: item.id };
  if (active_space === "research") {
    scope.topic = item.research_topic || "";
    if (!scope.topic) scope.category = "unbound";
    else if (scope.category === "unbound") scope.category = "research";
  }
  save_workspace({ space: active_space, [active_space]: scope });
  if (active_space === "research") sync_research_process_context(item);
  sync_workspace_visibility();
}

export function sync_workspace_visibility() {
  document.body.dataset.space = active_space;
  for (const button of document.querySelectorAll("[data-workspace]")) {
    const selected = button.dataset.workspace === active_space;
    button.classList.toggle("active", selected);
    button.setAttribute("aria-pressed", String(selected));
    button.disabled = workspace_switch_pending;
  }
  for (const element of document.querySelectorAll("[data-space-only]")) {
    element.classList.toggle("hidden", element.dataset.spaceOnly !== active_space);
  }
  for (const button of document.querySelectorAll(".activity-item[data-view]")) {
    button.classList.toggle("hidden", !view_allowed(button.dataset.view));
  }
  const context = $("research-chat-context");
  if (context) {
    const item = processItems.find((item) => item.id === activeProcessId);
    context.textContent = item?.research_topic ? `${t("研究课题")}: ${item.research_topic}` : t("未绑定课题的研究对话");
  }
  const active_view = document.querySelector(".view.active")?.id.slice(5);
  if (active_view && !view_allowed(active_view)) navigate_view(active_space === "research" ? "research" : "chat");
  if ($("parallel-task-status")) renderParallelTaskStatus(processItems);
}

export async function create_workspace_process(topic = null, is_current = () => true) {
  if (!currentProject) return;
  const project = currentProject;
  const space = active_space;
  const same_context = () => project === currentProject && active_space === space && is_current()
    && (space !== "research" || project_workspace().research.topic === (topic || ""));
  const item = await invoke("process_create", {
    projectDir: project,
    profile: active_space === "research" ? "research" : "dev",
    researchTopic: active_space === "research" ? topic || undefined : undefined,
    phasePipeline: false,
  });
  if (!same_context()) return;
  await refreshProcesses();
  if (!same_context()) return;
  await switchProcess(item.id);
  if (!same_context() || activeProcessId !== item.id) return;
  if (active_space === "research") {
    save_research_workspace({ page: "chat" });
    navigate_view("chat");
  }
  return item;
}

export async function switch_workspace(space) {
  if (!currentProject || workspace_switch_pending || !["dev", "research"].includes(space)) return;
  if (space === active_space) return;
  const project = currentProject;
  const saved = project_workspace();
  workspace_switch_pending = true;
  sync_workspace_visibility();
  try {
    await refreshProcesses();
    if (currentProject !== project) return;
    let target = preferred_workspace_process(processItems, space);
    if (!target) {
      target = await invoke("process_create", { projectDir: project, profile: space, phasePipeline: false });
      if (currentProject !== project) return;
      await refreshProcesses();
    }
    if (currentProject !== project) return;
    await switchProcess(target.id, target.id === activeProcessId);
    if (currentProject !== project) return;
    if (space === "research" && saved.research.view !== "chat") {
      save_research_workspace({ topic: saved.research.topic, category: saved.research.category });
    }
    navigate_view(view_allowed(saved[space].view) ? saved[space].view : space === "research" ? "research" : "chat");
  } catch (error) {
    toastError(`${t("切换工作空间失败")}: ${error}`);
  } finally {
    workspace_switch_pending = false;
    sync_workspace_visibility();
  }
}

defer(() => {
  for (const button of document.querySelectorAll("[data-workspace]")) {
    button.addEventListener("click", () => void switch_workspace(button.dataset.workspace));
  }
});
