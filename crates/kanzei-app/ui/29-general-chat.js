import { $, defer, invoke } from "./01-core.js";
import { openMenu } from "./00-surface.js";
import { t } from "./02-i18n.js";
import { activeProcessId, currentProject, running, navigate_view, syncNewChatEnabled, toast, toastError } from "./03-shell.js";
import { isGeneralChat, setGeneralChatRoot, rememberConversationMode, registerGeneralChatController } from "./03-general-scope.js";
import { switch_workspace, sync_workspace_visibility, create_workspace_process } from "./03-workspaces.js";
import { activate_execution_root, lastProjectPrefs, projectDisplayName, refreshProcesses, refreshPendingInputs, switchProcess } from "./09-sessions.js";
import { loadModels } from "./08-models.js";
import { loadConversation } from "./15-views-misc.js";
import { cancelProjectNavigation, openProjectSpace, setBrowsingProject, workbenchNavigationGuard, reconcileWorkbenchView } from "./12-workbench.js";

registerGeneralChatController(openGeneralChat);
let openingSequence = 0, linkInFlight = false;
export async function openGeneralChat({ newChat = false } = {}) {
  const sequence = ++openingSequence;
  $("workbench-general-chat")?.setAttribute("aria-busy", "true");
  if ($("general-chat-link")) $("general-chat-link").disabled = true;
  cancelProjectNavigation();
  const valid = workbenchNavigationGuard();
  try {
    if (!await switch_workspace("dev", { isCurrent: valid }) || !valid()) return false;
    const root = await invoke("general_chat_open");
    if (!valid()) return false;
    setGeneralChatRoot(root);
    setBrowsingProject(null);
    activate_execution_root(root);
    await refreshProcesses();
    if (!valid() || !isGeneralChat()) return false;
    if (newChat) await create_workspace_process(null, valid, { discussion: true });
    if (!valid() || !isGeneralChat()) return false;
    if (!activeProcessId) throw new Error(t("对话列表加载失败，请重试"));
    await loadConversation();
    if (!valid() || !isGeneralChat()) return false;
    await loadModels();
    if (!valid() || !isGeneralChat()) return false;
    await refreshPendingInputs();
    if (!valid() || !isGeneralChat()) return false;
    rememberConversationMode("general");
    sync_workspace_visibility();
    syncNewChatEnabled();
    navigate_view("chat", { prepared: true });
    $("prompt")?.focus();
    return true;
  } catch (error) {
    if (valid()) toastError(`${t("打开无项目对话失败")}: ${error}`);
    return false;
  } finally {
    if (sequence === openingSequence) {
      $("workbench-general-chat")?.removeAttribute("aria-busy");
      if ($("general-chat-link")) $("general-chat-link").disabled = linkInFlight;
    }
  }
}

defer(() => {
  // Discover existing history without switching the user's current project or creating a chat.
  void invoke("general_chat_location").then(root => {
    if (root) {
      setGeneralChatRoot(root);
      document.dispatchEvent(new CustomEvent("kz:general-history-ready"));
    }
  }).catch(error => toastError(String(error)));
  $("workbench-general-chat")?.addEventListener("click", () => void openGeneralChat());
  $("general-chat-link")?.addEventListener("click", () => {
    if (!isGeneralChat()) return;
    if (running) { toast(t("请结束当前回复后再关联项目")); return; }
    const project = currentProject, recipient = activeProcessId, valid = workbenchNavigationGuard();
    const same = () => valid() && currentProject === project && activeProcessId === recipient;
    const projects = lastProjectPrefs.projects || [];
    if (!projects.length) { toast(t("先添加一个项目，再关联对话")); return; }
    openMenu($("general-chat-link"), projects.map(path => ({ label: projectDisplayName(path), onSelect: async () => {
      if (!same()) return;
      const button = $("general-chat-link"); linkInFlight = true; button.disabled = true;
      try {
        const linked = await invoke("general_chat_link", { processId: recipient, projectDir: path });
        if (!same()) { toast(t("对话已关联到项目，原对话保留")); return; }
        if (!await openProjectSpace(path, "chat")) return;
        if (currentProject !== path) return;
        await switchProcess(linked.id, true);
        toast(t("对话已关联到项目，原对话保留"));
      } catch (error) { toastError(String(error)); }
      finally { linkInFlight = false; button.disabled = $("workbench-general-chat")?.hasAttribute("aria-busy") || false; }
    } })), { label: t("关联项目") });
  });
  document.addEventListener("kz:conversation-selected", () => reconcileWorkbenchView(document.body.dataset.view));
});
