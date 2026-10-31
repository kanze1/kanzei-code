import { switch_workspace, sync_workspace_visibility } from "./03-workspaces.js";
import { setConversationRoot, rememberConversationMode } from "./03-general-scope.js";
import { activate_execution_root, refreshProcesses, switchProcess, refreshPendingInputs } from "./09-sessions.js";
import { cancelProjectNavigation, setBrowsingProject, workbenchNavigationGuard } from "./12-workbench.js";
import { loadModels } from "./08-models.js";
import { loadConversation } from "./15-views-misc.js";
import { navigate_view, syncNewChatEnabled } from "./03-shell.js";
let generation = 0;
export async function openChannelConversation(resolve, mode) {
  const own = ++generation; cancelProjectNavigation(); const navigation = workbenchNavigationGuard();
  const current = () => navigation() && own === generation;
  if (!await switch_workspace("dev-auto", { isCurrent: current }) || !current()) return null;
  const target = await resolve(); if (!current()) return null;
  setBrowsingProject(null); setConversationRoot(target.root); activate_execution_root(target.root);
  await refreshProcesses(); if (!current()) return null;
  await switchProcess(target.process_id, true); if (!current()) return null;
  await loadConversation(); if (!current()) return null;
  await loadModels(); if (!current()) return null;
  await refreshPendingInputs(); if (!current()) return null;
  rememberConversationMode(mode); sync_workspace_visibility(); syncNewChatEnabled(); navigate_view("chat", { prepared: true });
  return target;
}
