import { defer } from "./00-dom.js";
import { sameProject } from "./00-project-identity.js";
import { openMenu, updateMenu } from "./00-surface.js";
import { invoke } from "./00-ipc.js";
import { t } from "./02-i18n.js";
import { currentProject, processItems, toast, toastError, navigate_view } from "./03-shell.js";
import { isAbsolutePath, normalizeRoot, relativeToRoot } from "./04-structured-parse.js";
import { openToolItems } from "./12-open-tools.js";
import { openProjectSpace } from "./12-workbench.js";
import { openFilePreview, switchFilesTree } from "./17-files.js";
import { filesRoot } from "./17-files-editor.js";
import { setStructuredNav, structuredNav } from "./04-structured.js";
import { openMemoryDetailById } from "./13-memory.js";
export async function openUrlInApp(url) {
  const target = String(url ?? "");
  if (!target) return;
  try {
    await navigator.clipboard.writeText(target);
    toast(`${t("已复制")}: ${target}`, { kind: "ok" });
  } catch (error) {
    toastError(`${t("打开失败")}:${error}`);
  }
}
/// 打开项目内文件路径（在 Phase 6 将升级为对话右侧伴生上下文抽屉）。
export async function openStructuredPath(path, line = null, target = null) {
  const rel = String(path ?? "").replace(/\\/g, "/").replace(/^\.\//, "");
  if (!rel) return;
  if (target) return runPathAction(() => openPathTargetInApp(target));
  navigate_view("files");
  void openFilePreview({ path: rel, line });
}
export function openStructuredMemory(scope, id) {
  navigate_view("memory");
  void openMemoryDetailById(scope, id);
}
/// M-(项目)/U-(全局)进记忆详情。
export function openStructuredRef(id) {
  const ref = String(id ?? "");
  if (/^[MU]-\d/.test(ref)) {
    openStructuredMemory(ref.startsWith("U-") ? "global" : "project", ref);
  }
}

// Resolve the link's conversation before awaiting native tool discovery. Menu actions
// keep that target even if the user changes the active project in the meantime.
function fileLinkTarget(link) {
  const sessionId = link.closest(".msg-pane")?.dataset.sessionId;
  const process = link.dataset.processId
    ? processItems.find(item => item.id === link.dataset.processId)
    : sessionId ? processItems.find(item => item.session_id === sessionId) : null;
  const processId = link.dataset.processId || process?.id;
  const projectDir = link.dataset.projectDir || process?.origin_project || currentProject;
  const path = normalizeRoot(link.dataset.path);
  const root = normalizeRoot(process?.worktree_path || projectDir);
  const absolutePath = isAbsolutePath(path) ? path : `${root}/${path.replace(/^\.\//, "")}`;
  const previewRoot = [root, normalizeRoot(projectDir)].find(base => relativeToRoot(absolutePath, base) !== null);
  return {
    args: { projectDir, ...(processId ? { processId } : {}), path },
    absolutePath,
    preview: previewRoot ? { root: previewRoot, path: relativeToRoot(absolutePath, previewRoot) } : null,
    previewPath: process?.worktree_path && !isAbsolutePath(path) ? `${root}/${path.replace(/^\.\//, "")}` : path,
    line: Number(link.dataset.line) || null,
  };
}

async function runPathAction(action) {
  try { await action(); } catch (error) { toastError(`${t("打开失败")}:${error}`); }
}

async function openPathTargetInApp(target) {
  if (!target.preview) throw new Error(t("文件不在当前项目或对话工作目录中"));
  if (!sameProject(currentProject, target.args.projectDir) && !await openProjectSpace(target.args.projectDir, "files")) return;
  if (!await switchFilesTree(target.preview.root)) return;
  if (!sameProject(currentProject, target.args.projectDir) || !sameProject(filesRoot(), target.preview.root)) return;
  navigate_view("files");
  await openFilePreview({ path: target.preview.path, line: target.line });
}

function openFileLinkMenu(link) {
  const target = fileLinkTarget(link);
  const items = tools => [
    { label: t("在应用内打开"), onSelect: () => void runPathAction(() => openPathTargetInApp(target)) },
    { label: t("在资源管理器中显示"), onSelect: () => void runPathAction(() => invoke("reveal_path", target.args)) },
    ...tools,
    "separator",
    { label: t("复制路径"), onSelect: async () => {
      try {
        await navigator.clipboard.writeText(target.absolutePath);
        toast(t("已复制"), { kind: "ok" });
      } catch (error) { toastError(`${t("复制失败")}:${error}`); }
    } },
  ];
  const handle = openMenu(link, items([{ label: t("正在读取打开方式…"), disabled: true }]), { label: link.dataset.path });
  if (handle.closed) return;
  void invoke("open_tools_list").then(tools => {
    updateMenu(handle, items(openToolItems(tools, tool => void runPathAction(() => invoke("open_with", { ...target.args, toolId: tool.id })))));
  }, error => {
    updateMenu(handle, items([{ label: t("打开方式读取失败"), desc: String(error), disabled: true }]));
  });
}

defer(() => {
  setStructuredNav({
    openRef: openStructuredRef,
    openPath: openStructuredPath,
    openUrl: (url) => openUrlInApp(url),
    openMemory: openStructuredMemory,
  });
  // markdown 路径链接没有 href(不让 WebView 自己导航),点击统一委托到这里。
  document.addEventListener("click", (event) => {
    const link = event.target?.closest?.("a.md-path");
    if (!link) return;
    event.preventDefault?.();
    const target = fileLinkTarget(link);
    void structuredNav.openPath(target.previewPath, target.line, target);
  });
  document.addEventListener("contextmenu", event => {
    const link = event.target?.closest?.("a.md-path, button.delivery-inline");
    if (!link) return;
    event.preventDefault();
    openFileLinkMenu(link);
  });
  document.addEventListener("keydown", event => {
    if (event.isComposing) return;
    const link = event.target?.closest?.("a.md-path, button.delivery-inline");
    if (!link) return;
    if (event.key === "Enter" && link.matches("a.md-path")) {
      event.preventDefault();
      link.click();
    } else if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
      event.preventDefault();
      openFileLinkMenu(link);
    }
  });
});
