import { closeSurface } from "./00-surface.js";
import { $, defer, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, log, toast, toastError } from "./03-shell.js";
import { openDocumentsView } from "./10-docs-core.js";
import { latestDocsSnapshot, renderDocuments, setDocumentsKind } from "./12-docs-pages.js";
import { refreshTests } from "./12-docs-tests.js";
import { refreshDocs } from "./14-docs-actions.js";
import { openConventions } from "./15-conventions.js";

// ---------- 需求页:切页签 + 新建(记需求/记缺陷/记想法) ----------
// UX-053 / B7 / B19:原来「记需求/记缺陷」的表单挂在被隐藏的侧栏里(实测 0×0,命令面板点了毫无反应),
// 想法收件箱也只在那里,「新目标」写进去后无处可见。现在三种记录都是需求页工具条上「＋ 新建」菜单里的
// 可见表单,命令面板的同名动作也点这三个菜单项(见 21-palette.js)——一处实现,点哪都落到同一个可见的表单。

/// 切到需求页并摆出某个页签(req/defect/tests/ideas)。页面还没打开就先打开:切视图会触发一次
/// refreshDocs,重绘按这里设好的页签画;已经在页面里就地重绘。☷ 需求弹层点「缺陷 3」就走这里(UX-054)。
export function openDocumentsKind(kind) {
  setDocumentsKind(kind);
  if (!$("view-documents")?.classList.contains("active")) openDocumentsView();
  else if (latestDocsSnapshot) renderDocuments(latestDocsSnapshot);
  if (kind === "tests") void refreshTests();
}

const QUICK_KINDS = Object.freeze({
  // kind → [表单标题(同时是菜单项文案), 名词, 需求页页签]
  req: ["记需求", "需求", "req"],
  defect: ["记缺陷", "缺陷", "defect"],
  idea: ["记想法", "想法", "ideas"],
});

function quickForm(kind, carriedText) {
  const [titleKey, noun] = QUICK_KINDS[kind];
  const form = document.createElement("div");
  // 表单住在页面自己的槽位里,不在会被整表重绘的列表容器内,所以后台刷新不会冲掉正在写的内容。
  form.className = "documents-quick-form";
  form.dataset.kind = kind;
  const title = document.createElement("div");
  title.className = "documents-quick-title";
  title.textContent = t(titleKey);
  const input = document.createElement("textarea");
  input.rows = 3;
  input.value = carriedText;
  input.setAttribute("aria-label", t(titleKey));
  input.placeholder = kind === "idea"
    ? t("想法描述,原样录入,不过模型")
    : `${t("自然语言描述")}${t(noun)}`;
  const hint = document.createElement("div");
  hint.className = "dim documents-quick-hint";
  hint.textContent = kind === "idea"
    ? `${t("录入后可点「拆解成需求/缺陷」")} · Ctrl+Enter ${t("提交")} · Esc ${t("取消")}`
    : `${t("独立子代理后台进行")},${t("不打断当前对话")} · Ctrl+Enter ${t("提交")} · Esc ${t("取消")}`;
  const bar = document.createElement("div");
  bar.className = "documents-quick-bar";
  const cancelBtn = document.createElement("button");
  cancelBtn.type = "button";
  cancelBtn.className = "ghost mini";
  cancelBtn.textContent = t("取消");
  cancelBtn.addEventListener("click", () => form.remove());
  const submitBtn = document.createElement("button");
  submitBtn.type = "button";
  submitBtn.className = "primary mini";
  submitBtn.textContent = t("提交");
  const submit = async () => {
    const text = input.value.trim();
    if (!text) {
      toast(t("先写点描述"));
      return;
    }
    // 项目在提交那一刻认领:await 期间用户切走了,这条仍落在写它的那个项目(D-256 同口径)。
    const project = currentProject;
    if (!project) return;
    // 失败时表单必须还在:提交前销毁会让用户写的描述无处可寻。
    submitBtn.disabled = true;
    cancelBtn.disabled = true;
    input.disabled = true;
    try {
      let message;
      if (kind === "idea") {
        message = await invoke("docs_update", {
          projectDir: project,
          kind: "idea",
          action: "add",
          id: "",
          title: text.replace(/\s*\n\s*/g, " "),
        });
        log(message);
      } else {
        toast(`${t("记录中")}${t(noun)}…(${t("独立子代理后台进行")})`);
        message = await invoke("quick_req", { projectDir: project, description: text, kind });
      }
      form.remove();
      toast(`${t("已记录")}:${message}`);
      if (project === currentProject) refreshDocs();
    } catch (err) {
      submitBtn.disabled = false;
      cancelBtn.disabled = false;
      input.disabled = false;
      toastError(`${t("记录失败(内容已保留,可重试)")}:${err}`);
    }
  };
  submitBtn.addEventListener("click", submit);
  input.addEventListener("keydown", (event) => {
    if (event.key === "Escape") form.remove();
    else if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) void submit();
  });
  bar.append(cancelBtn, submitBtn);
  form.append(title, input, hint, bar);
  return form;
}

/// 在需求页对应页签上打开新建表单(kind: req/defect/idea)。同类再点是幂等(保留已写内容并聚焦),
/// 换一类就换掉表单、把已写的字带过去。
export function openDocumentsQuick(kind) {
  if (!QUICK_KINDS[kind]) return;
  if (!currentProject) {
    toast(t("先在左侧「项目」里添加并选择一个目录"));
    return;
  }
  openDocumentsKind(QUICK_KINDS[kind][2]);
  const slot = $("documents-quick-slot");
  if (!slot) return;
  const opened = slot.querySelector(".documents-quick-form");
  if (opened?.dataset.kind === kind) {
    opened.querySelector("textarea")?.focus();
    return;
  }
  const carried = opened?.querySelector("textarea")?.value ?? "";
  opened?.remove();
  const form = quickForm(kind, carried);
  slot.appendChild(form);
  form.querySelector("textarea")?.focus();
}

defer(() => {
  const menu = $("documents-new-menu");
  for (const [id, kind] of [["documents-new-req", "req"], ["documents-new-defect", "defect"], ["documents-new-idea", "idea"]]) {
    $(id)?.addEventListener("click", () => {
      if (menu) closeSurface(menu);
      openDocumentsQuick(kind);
    });
  }
  // 开发规范的可见入口(B19/UX-007):原来只在被隐藏的侧栏分区里,生成/打开/审阅都到不了。
  $("documents-conventions-open")?.addEventListener("click", () => {
    const more = $("documents-more-menu");
    if (more) closeSurface(more);
    void openConventions();
  });
});
