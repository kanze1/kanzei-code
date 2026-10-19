import { defer } from "./00-dom.js";
import { invoke } from "./00-ipc.js";
import { on } from "./01-runtime-events.js";
import { uiPrefsLoad } from "./00-ui-preferences.js";
import { currentProject, toastError } from "./03-shell.js";
import { refreshProcesses, renderProjects, lastProjectPrefs } from "./09-sessions.js";
import { loadConversation } from "./15-views-misc.js";
import { active_space, restore_workspace_preferences, restore_active_workspace, sync_workspace_visibility } from "./03-workspaces.js";
import { workbenchNavigationGuard, openProjectSpace } from "./12-workbench.js";
import { refreshWorkspace } from "./12-docs-pages.js";
import { openGeneralChat } from "./03-general-scope.js";

let runtimeSync = null;
function synchronizeRuntime() {
  if (runtimeSync) return runtimeSync;
  runtimeSync = (async () => {
    const recoverStartup = !currentProject && !lastProjectPrefs.projects?.length;
    const isCurrent = workbenchNavigationGuard();
    const prefs = await invoke("projects_get");
    if (!isCurrent()) return;
    renderProjects(prefs, { activate: false });
    // An older, busy owner can reject the initial project/prefs requests. Once
    // it retires, restore the actual workspace instead of leaving an empty UI.
    if (recoverStartup && document.body.dataset.appReady === "true") {
      const settings = await uiPrefsLoad(true);
      await restore_workspace_preferences();
      if (!isCurrent()) return;
      sync_workspace_visibility();
      await refreshWorkspace();
      await restore_active_workspace({ isCurrent });
      if (!isCurrent() || active_space !== "dev") return;
      if (settings.ui_layout?.prefs?.conversation_mode === "general" || !prefs.projects?.length) await openGeneralChat();
      else if (prefs.current) await openProjectSpace(prefs.current);
    } else {
      await refreshProcesses();
      if (isCurrent() && document.body.dataset.appReady === "true") await loadConversation(null, null, true);
    }
  })().finally(() => { runtimeSync = null; });
  return runtimeSync;
}

defer(() => on("kz:runtime-resync", event => {
  if (event.payload.connected) void synchronizeRuntime().catch(toastError);
}));
