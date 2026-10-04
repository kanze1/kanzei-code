import { $, defer } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, navigate_view, running } from "./03-shell.js";
import { workbenchProject, openProjectResource, openProjectSpace } from "./12-workbench.js";
import { projectDisplayName } from "./09-sessions.js";
import { openTasksPanel } from "./06-agent-panel.js";
import { closeSurface } from "./00-surface.js";

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
  for (const [view, label] of [["memory", "记忆管理"], ["settings", "设置"]]) {
    const button = document.querySelector(`.activity-item[data-view="${view}"]`);
    if (!button) continue;
    button.append(node("span", "", t(label))); footer.append(button);
  }
  const theme = $("theme-toggle"); if (theme) footer.append(theme);
  $("workbench-navigation").append(footer);

  const more = $("composer-more-menu");
  for (const id of ["autorun-bar", "subagent-control", "side-question-open", "project-handoff"]) if ($(id)) more.prepend($(id));
  const runtime = $("runtime-indicator"); if (runtime) more.append(runtime);
  // The terminal has the same session-bound activity surface as subagents.
  $("terminal-monitor-open")?.addEventListener("click", event => {
    event.preventDefault(); event.stopImmediatePropagation(); closeSurface(more); openTasksPanel();
  }, true);

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
