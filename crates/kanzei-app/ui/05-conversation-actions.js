import { openDialog, closeSurface } from "./00-surface.js";
import { invoke, promptBox } from "./01-core.js";
import { activeProcessId, activeSessionId, currentProject, running, toast, toastError } from "./03-shell.js";
import { t } from "./02-i18n.js";
import { processSwitchGeneration, refreshProcesses, switchProcess } from "./09-sessions.js";
import { workbenchNavigationGuard } from "./12-workbench.js";
import { loadConversation } from "./15-views-misc.js";

let actionGeneration = 0;
export function conversationActionButton(message) {
  const button = document.createElement("button");
  button.type = "button"; button.className = "copy-btn";
  button.textContent = "↶"; button.title = t("回退或分叉"); button.setAttribute("aria-label", t("回退或分叉"));
  button.addEventListener("click", () => { void showConversationActions(message).catch(error => toastError(String(error))); });
  return button;
}

async function showConversationActions(message) {
  if (running) { toast(t("对话正在运行，请结束后再操作")); return; }
  const text = message.querySelector(".message-body")?.dataset.raw || "";
  if (!text.trim() || message.dataset.sessionId !== activeSessionId) return;
  const following = [...message.parentElement.querySelectorAll(".msg.user")];
  const index = following.indexOf(message);
  const occurrenceFromEnd = following.slice(index + 1).filter(item => item.querySelector(".message-body")?.dataset.raw === text).length;
  const owner = activeSessionId;
  const generation = ++actionGeneration;
  const args = { projectDir: currentProject, processId: activeProcessId, text, occurrenceFromEnd };
  const navigationCurrent = workbenchNavigationGuard(), sourceGeneration = processSwitchGeneration;
  const sourceCurrent = () => generation === actionGeneration && navigationCurrent() && currentProject === args.projectDir
    && activeSessionId === owner && activeProcessId === args.processId && processSwitchGeneration === sourceGeneration;
  let preview;
  try { preview = await invoke("conversation_action", { ...args, action: "preview" }); }
  catch (error) { if (sourceCurrent()) toastError(String(error)); return; }
  if (!sourceCurrent()) return;
  const dialog = document.getElementById("conversation-action-overlay");
  dialog.replaceChildren();
  
  dialog.setAttribute("aria-label", t("回退或分叉"));
  const title = document.createElement("h3"); title.textContent = t("从这条消息重新开始");
  const detail = document.createElement("p");
  detail.textContent = `${t("保留此前消息")}：${preview.keptMessages} · ${t("文件检查点")}：${preview.files.length}`;
  const files = document.createElement("ul");
  for (const file of preview.files) {
    const row = document.createElement("li");
    row.textContent = `${file.path} · ${!file.restorable ? t("无法还原") : file.external_change ? t("外部修改，默认跳过") : file.pre_exists ? t("还原原内容") : t("删除本轮新建文件")}`;
    files.appendChild(row);
  }
  const warning = document.createElement("p"); warning.textContent = preview.unhandled.join("；");
  const forceLabel = document.createElement("label"), force = document.createElement("input");
  force.type = "checkbox";
  forceLabel.append(force, document.createTextNode(t("强制还原外部修改的文件（先保存当前内容）")));
  const actions = document.createElement("div"); actions.className = "conversation-action-buttons";
  dialog.append(title, detail, files, warning, forceLabel, actions);
  if (preview.worktree) {
    const note = document.createElement("p"); note.textContent = t("分叉只复制对话，新线使用项目主目录，不复制当前工作树代码。"); dialog.appendChild(note);
  }
  const close = () => closeSurface(dialog);
  for (const [action, label] of [["conversation", "只回退对话"], ["code", "只回退代码"], ["both", "回退对话和代码"], ["fork", "从这里分叉"]]) {
    const control = document.createElement("button"); control.type = "button"; control.textContent = t(label);
    control.addEventListener("click", async () => {
      if (!sourceCurrent()) { close(); return; }
      let draft = promptBox.value;
      for (const child of actions.children) child.disabled = true;
      try {
        const result = await invoke("conversation_action", { ...args, action, expectedHash: preview.sourceHash, force: force.checked });
        if (!sourceCurrent()) return;
        close();
        if (result.forked) {
          await refreshProcesses();
          if (!sourceCurrent()) return;
          const switching = switchProcess(result.processId, true);
          const targetGeneration = processSwitchGeneration, targetSession = activeSessionId;
          draft = promptBox.value;
          await switching;
          if (generation !== actionGeneration || !navigationCurrent() || currentProject !== args.projectDir || activeProcessId !== result.processId
            || activeSessionId !== targetSession || processSwitchGeneration !== targetGeneration) return;
        } else {
          if (action !== "code") await loadConversation(null, null, true);
          if (!sourceCurrent()) return;
        }
        // A completion may restore its prompt only while that recipient's input
        // still matches the snapshot preceding the asynchronous operation.
        if (promptBox.value === draft) { promptBox.value = result.prompt; promptBox.focus(); }
        const skipped = result.skipped?.length || 0;
        toast(skipped ? `${t("已完成，跳过文件")}：${result.skipped.map(item => `${item.path}: ${item.reason}`).join("；")}` : t("已完成"));
      } catch (error) {
        if (sourceCurrent()) toastError(String(error));
        for (const child of actions.children) child.disabled = false;
      }
    });
    actions.appendChild(control);
  }
  const cancel = document.createElement("button"); cancel.type = "button"; cancel.textContent = t("取消"); cancel.addEventListener("click", close); actions.appendChild(cancel);
  openDialog(dialog, { onClose: () => dialog.replaceChildren() });
}
