import { $, defer, invoke, readJson, uiPrefsLoad, uiPrefsSave, writeJson } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeProcessId, currentProject, navigate_view, processItems, toastError } from "./03-shell.js";
import { activate_execution_root, refreshProcesses, renderParallelTaskStatus, switchProcess } from "./09-sessions.js";
import { attachments, setAttachments } from "./03-shell.js";
import { renderAttachments } from "./08-compose-runtime.js";
import { sync_research_process_context } from "./19-research.js";
import { enter_research_library, research_topic_label } from "./03-research-library.js";
import { isGeneralChat, syncGeneralChatView } from "./03-general-scope.js";

// 导航偏好不承担运行配置；任务的 profile 与课题绑定来自 process_list。
export let active_space = "dev";
export let development_project = null;
export let workspace_switch_pending = false;
let workspace_transition = null;
export let workspace_preferences = readJson("kz-workspaces", {});
let workspace_save = Promise.resolve();
const dev_views = new Set(["project", "documents", "lines", "arch", "metrics"]);
const composer_drafts = new Map();
let composer_scope = "";
let startup_space;
const library_preferences_key = "@research-library";

export function remember_development_project(project) {
  development_project = project;
}

export function sync_composer_scope() {
  const scope = currentProject && activeProcessId ? JSON.stringify([currentProject, activeProcessId]) : "";
  if (scope === composer_scope) return;
  document.dispatchEvent(new CustomEvent("kz:before-composer-scope"));
  if (composer_scope) composer_drafts.set(composer_scope, { text: $("prompt").value, attachments: [...attachments] });
  composer_scope = scope;
  const draft = composer_drafts.get(scope);
  $("prompt").value = draft?.text || "";
  $("prompt").style.height = "auto";
  setAttachments([...(draft?.attachments || [])]);
  renderAttachments();
}

export function project_workspace(project = currentProject) {
  const saved = workspace_preferences[project || development_project] ?? {};
  const library = workspace_preferences[library_preferences_key];
  return {
    ...saved,
    space: active_space,
    dev: { view: "chat", ...saved.dev },
    research: { view: "research", page: "overview", topic: "", category: "research", ...(library?.research ?? saved.research) },
  };
}

export function save_workspace(patch, project = currentProject) {
  if (project && patch.dev) workspace_preferences[project] = { ...workspace_preferences[project], dev: patch.dev };
  workspace_preferences[library_preferences_key] = {
    ...workspace_preferences[library_preferences_key],
    ...(patch.space ? { space: patch.space } : {}),
    ...(patch.research ? { research: patch.research } : {}),
  };
  if (patch.research?.topic_id) {
    const library = workspace_preferences[library_preferences_key];
    library.topic_states = { ...library.topic_states, [patch.research.topic_id]: patch.research };
  }
  writeJson("kz-workspaces", workspace_preferences);
  workspace_save = workspace_save.catch(() => {}).then(() => uiPrefsSave({ workspace_state: { ...workspace_preferences } }));
}

export function save_research_workspace(patch) {
  const saved = project_workspace();
  const changed = Object.hasOwn(patch, "topic_id") && patch.topic_id !== saved.research.topic_id;
  const previous = changed
    ? { view: "research", page: "overview", process_id: null, ...workspace_preferences[library_preferences_key]?.topic_states?.[patch.topic_id] }
    : saved.research;
  save_workspace({ research: { ...previous, ...patch } });
}

export async function restore_workspace_preferences() {
  const saved = await uiPrefsLoad();
  if (saved.workspace_state && typeof saved.workspace_state === "object") {
    workspace_preferences = { ...workspace_preferences, ...saved.workspace_state };
  }
  startup_space = workspace_preferences[library_preferences_key]?.space;
}

export async function restore_active_workspace(options = {}) {
  const space = startup_space ?? workspace_preferences[development_project]?.space;
  if (space === "research") await switch_workspace("research", options);
  else sync_workspace_visibility();
}

export function process_space(item) {
  return item?.profile === "research" ? "research" : "dev";
}

export function preferred_workspace_process(items, space = project_workspace().space) {
  const saved = project_workspace()[space];
  const candidates = items.filter((item) => process_space(item) === space
    && (space !== "research" || (item.research_topic || "") === saved.topic));
  if (space === "dev" && !isGeneralChat()) return main_workspace_process(candidates);
  return candidates.find((item) => item.id === saved.process_id)
    ?? (space === "research" ? candidates.find((item) => item.research_topic === saved.topic) : candidates.find((item) => item.id.startsWith("d|")))
    ?? candidates[0];
}

// Discussions never replace the project's execution owner. Preserve a migrated
// user's chosen execution line; independent tasks remain explicitly selectable.
export function main_workspace_process(items = processItems, project = currentProject) {
  const saved = project_workspace(project).dev;
  const candidates = items.filter(item => !["research", "readonly"].includes(item.profile));
  return candidates.find(item => item.id === saved.main_process_id)
    ?? candidates.find(item => item.id === saved.process_id)
    ?? candidates.find(item => item.id.startsWith("d|")) ?? candidates[0];
}

export function workspace_processes(items) {
  const topic = project_workspace().research.topic;
  return (items ?? []).filter((item) => process_space(item) === active_space
    && (active_space !== "research" || (item.research_topic || "") === topic));
}

export function view_allowed(view) {
  if (isGeneralChat() && active_space === "dev" && dev_views.has(view)) return false;
  return view === "research" ? active_space === "research" : !dev_views.has(view) || active_space === "dev";
}

export function remember_workspace_view(view) {
  if (["workspace", "project", "settings"].includes(view) || !currentProject) return;
  const saved = project_workspace();
  save_workspace({ [active_space]: { ...saved[active_space], view } });
}

export function adopt_process_workspace(item) {
  if (!item) return;
  sync_composer_scope();
  active_space = process_space(item);
  const saved = project_workspace();
  const scope = { ...saved[active_space], process_id: item.id };
  if (active_space === "dev" && !scope.main_process_id) {
    scope.main_process_id = main_workspace_process(processItems)?.id;
  }
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
  syncGeneralChatView(undefined, t);
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
    context.textContent = item?.research_topic ? `${t("研究课题")}: ${research_topic_label(item.research_topic)}` : t("未绑定课题的研究对话");
  }
  const active_view = document.querySelector(".view.active")?.id.slice(5);
  if (active_view && !view_allowed(active_view)) navigate_view(active_space === "research" ? "research" : "chat");
  if ($("parallel-task-status")) renderParallelTaskStatus(processItems);
}

// Discussions retain model/reasoning preferences while getting a readonly identity.
export async function create_workspace_process(topic = null, is_current = () => true, overrides = {}) {
  if (active_space === "research" && !topic) {
    $("research-topic-form")?.classList.remove("hidden");
    $("research-topic-title")?.focus();
    return;
  }
  if (!currentProject) return;
  const project = currentProject;
  const space = active_space;
  const same_context = () => project === currentProject && active_space === space && is_current()
    && (space !== "research" || project_workspace().research.topic === (topic || ""));
  const item = await invoke("process_create", {
    projectDir: project,
    profile: active_space === "research" ? "research" : isGeneralChat() ? "dev" : overrides.discussion ? "readonly" : "dev",
    researchTopic: active_space === "research" ? topic || undefined : undefined,
    phasePipeline: false,
    ...(isGeneralChat() ? { subagentsEnabled: true } : overrides.discussion ? { subagentsEnabled: false } : {}),
    ...(overrides.model ? { model: overrides.model } : {}),
    ...(overrides.reasoning ? { reasoning: overrides.reasoning } : {}),
  });
  if (!same_context()) return;
  await refreshProcesses();
  if (!same_context()) return;
  await switchProcess(item.id, true);
  if (!same_context() || activeProcessId !== item.id) return;
  if (active_space === "research") {
    save_research_workspace({ page: "chat" });
    navigate_view("chat");
  }
  return item;
}

export async function switch_workspace(space, { isCurrent = () => true } = {}) {
  if (!["dev", "research"].includes(space) || !isCurrent()) return false;
  // A new project may request dev while a cancelled research load is still
  // awaiting IPC. Wait for that transition to restore its root before entering
  // the new project; returning early would leave its session in research scope.
  while (workspace_transition) {
    await workspace_transition;
    if (!isCurrent()) return false;
  }
  if (space === active_space) return true;
  let release_transition;
  const transition = new Promise((resolve) => { release_transition = resolve; });
  workspace_transition = transition;
  const previous_space = active_space;
  const previous_root = currentProject;
  if (active_space === "dev" && currentProject && !isGeneralChat()) development_project = currentProject;
  // 一次性接入旧的按项目保存的研究偏好。
  const saved_research = project_workspace().research;
  const restore_previous = async () => {
    active_space = previous_space;
    save_workspace({ space: previous_space, research: saved_research });
    if (currentProject !== previous_root) activate_execution_root(previous_root);
    // Keep the pending flag set: renderProcesses can restore the previous
    // line's controls without loading its conversation during cancellation.
    if (previous_root) await refreshProcesses();
  };
  const cancelled = async () => {
    if (isCurrent()) return false;
    await restore_previous();
    return true;
  };
  workspace_switch_pending = true;
  sync_workspace_visibility();
  try {
    active_space = space;
    save_workspace({ space, research: saved_research });
    if (space === "research") await enter_research_library(development_project, { isCurrent });
    else { activate_execution_root(development_project); await refreshProcesses(); }
    if (await cancelled()) return false;
    const target = preferred_workspace_process(processItems, space);
    if (target) await switchProcess(target.id, true);
    if (await cancelled()) return false;
    if (space === "research" && !target && project_workspace().research.page === "chat") {
      save_research_workspace({ page: "overview", view: "research" });
    }
    const saved = project_workspace()[space];
    navigate_view(space === "dev" && !currentProject ? "workspace" : space === "research" && !target ? "research" : view_allowed(saved.view) ? saved.view : "chat", { prepared: true });
    return true;
  } catch (error) {
    await restore_previous();
    if (isCurrent()) toastError(`${t("切换工作空间失败")}: ${error}`);
    return false;
  } finally {
    workspace_switch_pending = false;
    sync_workspace_visibility();
    if (workspace_transition === transition) workspace_transition = null;
    release_transition();
  }
}

defer(() => {
  for (const button of document.querySelectorAll("[data-workspace]")) {
    button.addEventListener("click", () => {
      const event = new CustomEvent("kz:workspace-navigation", { cancelable: true, detail: { space: button.dataset.workspace } });
      document.dispatchEvent(event);
      if (!event.defaultPrevented) void switch_workspace(button.dataset.workspace);
    });
  }
});
