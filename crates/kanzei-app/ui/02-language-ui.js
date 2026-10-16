// View coordination belongs at the composition root, not in the translator.
import { $, defer } from "./00-dom.js";
import { t } from "./02-i18n.js";
import { currentProject, syncSidebar } from "./03-shell.js";
import { renderAgentAudit, syncDynamicUiLanguage } from "./06-activity.js";
import { reconcileTasksPanel } from "./06-agent-panel.js";
import { askActive, updateAskQueueStatus } from "./07-events.js";
import { refreshWorktrees } from "./09-sessions.js";
import { lastWorkspaceSnapshot, renderWorkspace } from "./12-docs-pages.js";
import { refreshDocs } from "./14-docs-actions.js";
import { refreshConversationList } from "./15-views-misc.js";
import { persistLanguagePreference, renderProviders } from "./16-settings.js";

defer(() => {
  document.addEventListener("kz:language", () => {
    syncDynamicUiLanguage();
    reconcileTasksPanel();
    syncSidebar();
    renderAgentAudit();
    if (document.querySelector("#providers-table tbody")?.children.length) renderProviders();
    $("status-tokens").title = t("点击查看上下文成分");
    if (lastWorkspaceSnapshot) renderWorkspace(lastWorkspaceSnapshot);
    if (document.body.classList.contains("documents-active")) refreshDocs();
    if (currentProject) {
      refreshWorktrees();
      refreshConversationList();
    }
    if (askActive) $("ask-title").textContent = t("权限请求");
    updateAskQueueStatus();
  });
  document.addEventListener("kz:language-preference", event => persistLanguagePreference(event.detail.language));
});
