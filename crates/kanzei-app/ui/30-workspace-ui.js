import { $, defer } from "./01-core.js";
import { bindMenus } from "./00-surface.js";
import { t } from "./02-i18n.js";
import { currentProject, navigate_view, running } from "./03-shell.js";
import { workbenchProject, openProjectResource, openProjectSpace } from "./12-workbench.js";
import { projectDisplayName } from "./09-sessions.js";

const node = (tag, cls, text) => { const element = document.createElement(tag); if (cls) element.className = cls; if (text) element.textContent = text; return element; };
defer(() => {
  document.body.classList.add("familiar-workspace");
  const header = node("header", "workspace-header"); header.id = "workspace-header";
  const identity = node("strong", "workspace-project-name"); identity.id = "workspace-project-name";
  const actions = node("div", "workspace-header-actions");
  header.append(identity); $("main").prepend(header);
  const switcher = $("project-work-switch"); if (switcher) header.append(switcher);
  const context = $("composer-context"); if (context) header.append(context);
  for (const id of ["tasks-toggle", "preview-toggle"]) if ($(id)) actions.append($(id));
  header.append(actions);

  const footer = node("div", "workspace-sidebar-footer"); footer.id = "workspace-sidebar-footer";
  const memory = document.querySelector('.activity-item[data-view="memory"]');
  if (memory) { memory.append(node("span", "", t("记忆管理"))); footer.append(memory); }
  $("workbench-navigation").append(footer);

  // Keep legacy DOM hooks for saved layouts; the composer no longer exposes them.
  const retired = node("div", "retired-conversation-controls"); retired.hidden = true;
  $("composer").append(retired);
  for (const id of ["composer-more", "composer-more-menu", "side-question-open", "terminal-monitor-open", "sop-picker", "worktree-add", "summarize-btn", "copy-context", "chat-search-toggle", "project-handoff", "runtime-indicator", "auto-continue-wrap"]) {
    const control = $(id); if (control) retired.append(control);
  }
  const left = $("composer-left");
  const subagents = $("subagent-control"); if (subagents) left.append(subagents);
  const goalMenu = $("goal-menu");
  const goal = $("auto-goal"), goalLabel = goal?.previousElementSibling;
  if (goalLabel) goalMenu.append(goalLabel); if (goal) goalMenu.append(goal);
  const goalActions = node("div", "goal-actions");
  for (const [id, label] of [["goal-start", "开始目标"], ["goal-clear", "结束目标"]]) {
    const button = node("button", "ghost mini", t(label)); button.type = "button"; button.id = id; button.dataset.i18nKey = label; goalActions.append(button);
  }
  goalMenu.append(goalActions); $("composer").append(goalMenu);
  const bindings = node("div", "conversation-bindings");
  for (const [id, label, menu] of [["goal-picker", "Goal", "goal-menu"]]) {
    const button = node("button", "kz-ctl", label); button.id = id; button.type = "button"; button.dataset.kzMenu = menu;
    button.setAttribute("aria-haspopup", "true"); button.setAttribute("aria-expanded", "false"); bindings.append(button);
  }
  left.append(bindings);
  bindMenus(left);
  const runtime = $("runtime-indicator");
  $("auto-continue")?.setAttribute("role", "switch");

  const projectLabel = $("settings-tools-project");
  const links = $("settings-project-links");
  for (const [label, view] of [["运行记录", "metrics"], ["开发规范", "conventions"], ["测试记录", "tests"], ["并行线路", "lines"]]) {
    const button = node("button", "ghost", t(label)); button.type = "button";
    button.dataset.resource = view;
    button.addEventListener("click", async () => {
      const project = workbenchProject() || currentProject;
      if (!project) { navigate_view("workspace"); return; }
      if (view === "tests") { if (await openProjectSpace(project, "documents")) $("documents-tab-tests")?.click(); }
      else await openProjectResource(project, view);
    }); links.append(button);
  }
  const sync = () => {
    const view = document.body.dataset.view, project = workbenchProject();
    header.hidden = !["chat", "project"].includes(view);
    identity.textContent = project ? projectDisplayName(project) : t("对话");
    context.hidden = view !== "chat";
    actions.hidden = view !== "chat";
    if (runtime) runtime.hidden = view !== "chat";
    if ($("delivery-select")) $("delivery-select").hidden = !running;
    projectLabel.textContent = project ? projectDisplayName(project) : t("请先选择项目");
  };
  for (const event of ["kz:view-changed", "kz:conversation-selected", "kz:session-state-changed", "kz:project-overview"]) document.addEventListener(event, sync);
  sync();
});
