import { t } from "./02-i18n.js";
import { $, defer, invoke } from "./01-core.js";
import { closeSurface, openDialog } from "./00-surface.js";
import { currentProject, toast, toastError } from "./03-shell.js";
import { renderMarkdownInto } from "./04-markdown.js";
import { sendText } from "./08-compose-runtime.js";
import { refreshDocs } from "./14-docs-actions.js";

let view = null;
let request = 0;

export function conventionsPrompt(exists) {
  return "请根据本项目生成简短的项目约束与规范：先读取项目清单、README、已有规范、测试与发布配置，保留用户决定；规则附实际文件来源，未确定事项标为待定。只写项目特有的技术栈、架构约束、真实构建/测试入口，不复制通用取活、登记和进度流程。" +
    (exists ? "先 conventions get，基于最新 hash 调用 conventions propose 提交建议稿，保留现有规则供用户对照；不要 patch 或覆盖用户内容。" : "通过 conventions create 新建 .kanzei/project/conventions.md，完成后说明用户可在规范页编辑。") +
    "本次只整理规范，不顺带修改业务代码或推进队列。";
}

export async function generateConventions() {
  const project = currentProject;
  if (!project) return;
  try {
    const snapshot = await invoke("conventions_read", { projectDir: project });
    if (currentProject !== project) return;
    if (snapshot.proposal) return toast(t("已有建议稿，请先在规范页审阅、保存或放弃。"));
    await sendText(conventionsPrompt(snapshot.exists));
  } catch (error) { toastError(String(error)); }
}

function paint() {
  const editor = $("conventions-editor");
  const preview = $("conventions-preview");
  editor.classList.toggle("hidden", !view.editing);
  preview.classList.toggle("hidden", view.editing);
  $("conventions-save").classList.toggle("hidden", !view.editing);
  $("conventions-edit").classList.toggle("hidden", view.editing);
  $("conventions-proposal").classList.toggle("hidden", !view.snapshot.proposal || view.editing);
  $("conventions-discard").classList.toggle("hidden", !view.snapshot.proposal);
  $("conventions-notice").textContent = view.draft
    ? t("正在编辑建议稿。请保留你要沿用的现有规则；保存后才生效。")
    : view.snapshot.exists ? t("当前生效规则。保存后 Agent 在下一步读取。") : t("尚未生成规则。可手动填写，或让 Agent 根据项目生成。");
  if (!view.editing) renderMarkdownInto(preview, view.snapshot.content || t("尚无项目规范。"));
}

export async function openConventions() {
  const project = currentProject;
  if (view?.editing && view.project === project) {
    openDialog($("conventions-dialog"), { initialFocus: "#conventions-editor" });
    return;
  }
  const generation = ++request;
  try {
    const snapshot = await invoke("conventions_read", { projectDir: project });
    if (generation !== request || project !== currentProject) return;
    view = { project, snapshot, editing: false, draft: false, expected: snapshot.hash };
    $("conventions-current").classList.add("hidden");
    $("conventions-editor").value = snapshot.content;
    paint();
    openDialog($("conventions-dialog"), { initialFocus: "#conventions-close" });
  } catch (error) { toastError(String(error)); }
}

defer(() => {
  $("conventions-close").addEventListener("click", () => closeSurface($("conventions-dialog")));
  $("conventions-edit").addEventListener("click", () => {
    view.editing = true;
    paint();
    $("conventions-editor").focus();
  });
  $("conventions-proposal").addEventListener("click", () => {
    const proposal = view.snapshot.proposal;
    // A stale draft remains readable but cannot silently replace a newer user edit.
    if (proposal.base_hash !== view.snapshot.hash) {
      toast(t("建议生成后规范已修改。请对照当前规则合并；保存会保留本次打开后的并发修改保护。"));
    }
    view.draft = true;
    view.editing = true;
    $("conventions-editor").value = proposal.content;
    const current = $("conventions-current");
    current.classList.remove("hidden");
    renderMarkdownInto(current, view.snapshot.content);
    paint();
  });
  $("conventions-save").addEventListener("click", async () => {
    const pending = view;
    const button = $("conventions-save");
    button.disabled = true;
    try {
      const content = $("conventions-editor").value;
      const hash = await invoke("conventions_save", {
        projectDir: pending.project, content, expectedHash: pending.expected,
        proposalHash: pending.draft ? pending.snapshot.proposal.hash : null,
      });
      if (view !== pending) return;
      pending.snapshot = { ...pending.snapshot, content, hash, exists: true, proposal: pending.draft ? null : pending.snapshot.proposal };
      pending.expected = hash;
      pending.editing = false;
      pending.draft = false;
      $("conventions-current").classList.add("hidden");
      paint();
      toast(t("项目规范已保存"));
      if (currentProject === pending.project) void refreshDocs();
    } catch (error) { toastError(String(error)); } // Keep unsaved editor text on conflict.
    finally { button.disabled = false; }
  });
  $("conventions-discard").addEventListener("click", async () => {
    const pending = view;
    try {
      await invoke("conventions_discard", { projectDir: pending.project, expectedHash: pending.snapshot.proposal.hash });
      if (view !== pending) return;
      pending.snapshot.proposal = null;
      pending.draft = false;
      pending.editing = false;
      $("conventions-editor").value = pending.snapshot.content;
      $("conventions-current").classList.add("hidden");
      paint();
    } catch (error) { toastError(String(error)); }
  });
  $("conventions-generate").addEventListener("click", () => {
    closeSurface($("conventions-dialog"));
    void generateConventions();
  });
});
