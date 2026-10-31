import { $, defer } from "./00-dom.js";
import { installWindowChrome } from "./30-window-chrome.js";
import { views } from "./00-views.js";
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
  header.append(identity); installWindowChrome(header);
  const sidebarToggle = $("rail-sidebar-toggle");
  if (sidebarToggle) header.prepend(sidebarToggle);
  const switcher = $("project-work-switch"); if (switcher) header.append(switcher);
  const context = $("composer-context"); if (context) header.append(context);
  for (const id of ["tasks-toggle", "preview-toggle"]) if ($(id)) actions.append($(id));
  header.append(actions);

  const footer = node("div", "workspace-sidebar-footer"); footer.id = "workspace-sidebar-footer";
  const globalNavigation = node("nav", "workspace-global-navigation");
  globalNavigation.setAttribute("aria-label", t("全局导航"));
  for (const [selector, label] of [["#qq-nav", "QQ 机器人"], ['.activity-item[data-view="memory"]', "记忆"], ["#skills-nav", "Skills"], ['.activity-item[data-view="settings"]', "设置"]]) {
    const button = document.querySelector(selector);
    if (button) { const text = node("span", "", t(label)); text.dataset.i18nKey = label; button.append(text); globalNavigation.append(button); }
  }
  const utilities = node("div", "workspace-appearance");
  const theme = $("theme-toggle"); if (theme) utilities.append(theme);
  footer.append(globalNavigation, utilities);
  $("workbench-navigation").append(footer);

  const advancedMenu = $("composer-more-menu");
  const handoff = $("project-handoff"); if (handoff) advancedMenu.append(handoff);
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
  advancedMenu.prepend(bindings);
  bindMenus(advancedMenu);
  $("auto-continue")?.setAttribute("role", "switch");

  const projectLabel = $("settings-tools-project");
  const links = $("settings-project-links");
  for (const [label, view] of [["运行记录", "metrics"], ["项目规范", "conventions"], ["测试记录", "tests"], ["并行线路", "lines"]]) {
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
    const workspace = ["chat", "project"].includes(view);
    identity.textContent = workspace && project ? projectDisplayName(project) : t(views[view]?.label || "kanzei");
    identity.title = identity.textContent;
    context.hidden = view !== "chat";
    actions.hidden = false;
    if ($("delivery-select")) $("delivery-select").hidden = !running;
    projectLabel.textContent = project ? projectDisplayName(project) : t("请先选择项目");
  };
  for (const event of ["kz:view-changed", "kz:conversation-selected", "kz:session-state-changed", "kz:project-overview", "kz:language"]) document.addEventListener(event, sync);
  sync();
});
