import { $, defer, invoke, mergeWorkspaceState, readJson, uiPrefsLoad, uiPrefsSave, writeJson } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeProcessId, currentProject, navigate_view, processItems, toastError } from "./03-shell.js";
import { activate_execution_root, processSwitchGeneration, refreshProcesses, renderParallelTaskStatus, switchProcess } from "./09-sessions.js";
import { workbenchNavigationGuard } from "./12-workbench.js";
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
let workspace_restore = null;
const dev_views = new Set(["project", "documents", "lines", "arch", "metrics"]);
const composer_drafts = new Map();
let composer_scope = "";

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

// A Softwire send may finish after its native editor was restored or cached by
// navigation. Acknowledge only the submitted snapshot, never a newer draft.
export function acknowledge_composer_draft(project, processId, sent) {
  const scope = JSON.stringify([project, processId]);
  const matches = draft => draft?.text === sent.text
    && JSON.stringify(draft.attachments || []) === JSON.stringify(sent.attachments);
  if (matches(composer_drafts.get(scope))) composer_drafts.delete(scope);
  if (scope === composer_scope && document.body.dataset.softwireComposer !== "true"
    && matches({ text: $("prompt").value, attachments })) {
    $("prompt").value = "";
    $("prompt").style.height = "auto";
    setAttachments([]); renderAttachments();
  }
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
  // Caller 常带本窗旧分区快照；仅变化字段属于此次写入，避免跨窗回灌陈值。
  const changed_fields = (previous, fields) => Object.fromEntries(Object.entries(fields)
    .filter(([key, value]) => !Object.hasOwn(previous ?? {}, key) || previous[key] !== value));
  const delta = {};
  if (project && patch.dev) {
    const dev = changed_fields(workspace_preferences[project]?.dev, patch.dev);
    if (Object.keys(dev).length) delta[project] = { dev };
  }
  const previous = workspace_preferences[library_preferences_key] ?? {};
  const library = {};
  if (patch.space && patch.space !== previous.space) library.space = patch.space;
  if (patch.research) {
    const research = changed_fields(previous.research, patch.research);
    if (Object.keys(research).length) library.research = research;
  }
  if (patch.research?.topic_id) {
    const topic = patch.research.topic_id;
    const fields = changed_fields(previous.topic_states?.[topic], patch.research);
    if (Object.keys(fields).length) library.topic_states = { [topic]: fields };
  }
  if (Object.keys(library).length) delta[library_preferences_key] = library;
  if (!Object.keys(delta).length) return;
  // 在排队前冻结每次操作；较晚的导航不能改写已经排队的字段。
  const snapshot = JSON.parse(JSON.stringify(delta));
  workspace_preferences = mergeWorkspaceState(workspace_preferences, snapshot);
  if (workspace_restore) workspace_restore.edits = mergeWorkspaceState(workspace_restore.edits, snapshot);
  writeJson("kz-workspaces", workspace_preferences);
  workspace_save = workspace_save.catch(() => {}).then(() => uiPrefsSave({ workspace_state: snapshot }));
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
  const restoring = { edits: {} };
  workspace_restore = restoring;
  const saved = await uiPrefsLoad();
  if (workspace_restore !== restoring) return;
  if (saved.workspace_state && typeof saved.workspace_state === "object") {
    workspace_preferences = { ...workspace_preferences, ...saved.workspace_state };
  }
  workspace_preferences = mergeWorkspaceState(workspace_preferences, restoring.edits);
  workspace_restore = null;

}

export async function restore_active_workspace() {
  // Research workspaces are retired; existing material remains on disk.
  sync_workspace_visibility();
}

export function process_space(item) {
  return item?.profile === "research" ? "research" : "dev";
}

export function preferred_workspace_process(items, space = project_workspace().space) {
  const saved = project_workspace()[space];
  const candidates = items.filter((item) => process_space(item) === space
    && (space !== "research" || (item.research_topic || "") === saved.topic));
  return candidates.find((item) => item.id === saved.process_id)
    ?? (space === "research" ? candidates.find((item) => item.research_topic === saved.topic) : null)
    ?? candidates[0];
}

// Navigation remembers the selected conversation; it does not designate an owner.
export function selected_workspace_process(items = processItems, project = currentProject) {
  const saved = project_workspace(project).dev;
  const candidates = items.filter(item => item.profile !== "research");
  return candidates.find(item => item.id === (project === currentProject ? activeProcessId : saved.process_id))
    ?? candidates.find(item => item.id === saved.process_id) ?? candidates[0];
}

export function workspace_processes(items) {
  const topic = project_workspace().research.topic;
  return (items ?? []).filter((item) => process_space(item) === active_space
    && (active_space !== "research" || (item.research_topic || "") === topic));
}

export function view_allowed(view, project = currentProject) {
  // Overview/inbox browse an explicitly selected project without changing
  // the execution root of the general conversation.
  if (view === "project" && active_space === "dev" && project && !isGeneralChat(project)) return true;
  if (isGeneralChat() && active_space === "dev" && dev_views.has(view)) return false;
  return view === "research" ? false : !dev_views.has(view) || active_space === "dev";
}

export function remember_workspace_view(view) {
  if (["workspace", "project", "settings", "skills"].includes(view) || !currentProject) return;
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
  const navigationCurrent = workbenchNavigationGuard();
  let selection = processSwitchGeneration;
  const same_context = () => project === currentProject && active_space === space && is_current()
    && navigationCurrent() && selection === processSwitchGeneration
    && (space !== "research" || project_workspace().research.topic === (topic || ""));
  const item = await invoke("process_create", {
    projectDir: project,
    profile: active_space === "research" ? "research" : isGeneralChat() ? "dev" : overrides.discussion ? "readonly" : "dev",
    researchTopic: active_space === "research" ? topic || undefined : undefined,
    subagentMode: "auto",
    ...(overrides.model ? { model: overrides.model } : {}),
    ...(overrides.reasoning ? { reasoning: overrides.reasoning } : {}),
  });
  if (!same_context()) return;
  await refreshProcesses();
  if (!same_context()) return;
  const switching = switchProcess(item.id, true);
  selection = processSwitchGeneration;
  await switching;
  if (!same_context() || activeProcessId !== item.id) return;
  if (active_space === "research") {
    save_research_workspace({ page: "chat" });
    navigate_view("chat");
  }
  return item;
}

export async function switch_workspace(space, { isCurrent = () => true } = {}) {
  if (space !== "dev" || !isCurrent()) return false;
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
