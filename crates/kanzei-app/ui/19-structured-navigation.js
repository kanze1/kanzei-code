import { defer } from "./00-dom.js";
import { invoke } from "./00-ipc.js";
import { t } from "./02-i18n.js";
import { currentProject, toastError, navigate_view } from "./03-shell.js";
import { jumpToEntry } from "./11-docs-list.js";
import { openFilePreview } from "./17-files.js";
import { setStructuredNav, structuredNav } from "./04-structured.js";
import { openMemoryDetailById } from "./13-memory.js";
import { openRuntimeMarkdown } from "./15-views-misc.js";
export async function openUrlInApp(url, topic = "") {
  const target = String(url ?? "");
  if (!target) return;
  try {
    const isArxiv = Boolean(topic) && /^https?:\/\/(?:export\.)?arxiv\.org\//i.test(target);
    const page = isArxiv
      ? await invoke("research_arxiv_preview", { projectDir: currentProject, topic, url: target })
      : await invoke("webfetch_preview", { url: target });
    const depth = page.depth ? `[${page.depth}]\n` : "";
    openRuntimeMarkdown(page.title || target, `${depth}${page.text || ""}`);
  } catch (error) {
    toastError(`${t("打开失败")}:${error}`);
  }
}
/// docs/ 下的 markdown 进应用内查看器;其余文件切到文件打开预览。
export async function openStructuredPath(path, line = null) {
  const rel = String(path ?? "").replace(/\\/g, "/").replace(/^\.\//, "");
  if (!rel) return;
  if (/^docs\/.+\.md$/i.test(rel) && currentProject) {
    try {
      const file = await invoke("docs_read_custom", { projectDir: currentProject, relPath: rel });
      openRuntimeMarkdown(file.name || rel, file.content);
      return;
    } catch {
      /* 读不到(不在 docs 白名单/已删除)就退回文件预览,由预览如实报错。 */
    }
  }
  navigate_view("files");
  void openFilePreview({ path: rel, line });
}
export function openStructuredMemory(scope, id) {
  navigate_view("memory");
  void openMemoryDetailById(scope, id);
}
/// R/D/I/S/F/T 走条目跳转;M-(项目)/U-(全局)进记忆详情;A- 没有对应视图,不跳。
export function openStructuredRef(id) {
  const ref = String(id ?? "");
  if (/^[MU]-\d/.test(ref)) {
    openStructuredMemory(ref.startsWith("U-") ? "global" : "project", ref);
    return;
  }
  if (!ref || /^A-/.test(ref)) return;
  void jumpToEntry(ref, { expand: true });
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
    structuredNav.openPath(link.dataset.path, Number(link.dataset.line) || null);
  });
});
