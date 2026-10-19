import { $, defer } from "./00-dom.js";
import { openMenu } from "./00-surface.js";
import { invoke } from "./00-ipc.js";
import { t } from "./02-i18n.js";
import { memoryProject } from "./03-memory-scope.js";
import { activeProcessId, currentProject, toast, toastError } from "./03-shell.js";
import { processName } from "./12-session-tree.js";
import { memoryJobActive, refreshMemoryJobStatus } from "./13-memory-jobs.js";

const pending = new Set();
function syncDream() {
  $("memory-dream").disabled = !memoryProject || pending.has(memoryProject) || memoryJobActive(memoryProject);
}
async function dream(project, processId) {
  if (memoryProject !== project) return;
  pending.add(project); syncDream();
  try {
    const work = invoke("memory_dream", { projectDir: project, processId, requestId: crypto.randomUUID() });
    void refreshMemoryJobStatus();
    const result = await work;
    const status = result?.report?.status;
    if (status === "failed") toastError(result?.report?.stopped_reason || t("做梦失败，来源已保留"));
    else toast(({ completed: t("做梦完成，候选记忆待确认"), busy: t("已有整理作业在运行"), cancelled: t("整理已停止，可继续"), incomplete: t("整理未完成，可继续"), interrupted: t("上次整理中断，可继续") })[status] || t("做梦失败，来源已保留"));
    if (memoryProject === project) document.dispatchEvent(new CustomEvent("kz:memory-review", { detail: { project } }));
  } catch (error) { toastError(String(error)); }
  finally {
    pending.delete(project); await refreshMemoryJobStatus(); syncDream();
    document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project } }));
  }
}
export async function chooseDreamConversation() {
  const project = memoryProject;
  if (!project || pending.has(project)) return;
  pending.add(project); syncDream();
  try {
    const conversations = await invoke("process_list", { projectDir: project });
    if (memoryProject !== project) return;
    const items = conversations.filter(item => item.session_id).map(item => ({
      label: processName(item),
      desc: project === currentProject && item.id === activeProcessId ? t("当前对话") : undefined,
      onSelect: () => void dream(project, item.id),
    }));
    if (!items.length) { toast(t("这个项目还没有可做梦的对话")); return; }
    openMenu($("memory-dream"), [{ heading: t("选择做梦的来源对话") }, ...items], { placement: "bottom-end", label: t("做梦") });
  } catch (error) { toastError(String(error)); }
  finally { pending.delete(project); syncDream(); }
}
defer(() => {
  $("memory-dream").addEventListener("click", () => void chooseDreamConversation());
  for (const event of ["kz:view-changed", "kz:memory-filters", "kz:memory-job-state"]) document.addEventListener(event, syncDream);
  syncDream();
});
