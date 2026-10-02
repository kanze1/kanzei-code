import { $, confirmDialog, defer, invoke, on } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, log, processItems, toast, toastError } from "./03-shell.js";
import {
  confirmLeaveDirty, filesDirtyPaths, filesDoc, filesTreeRoot, humanSize, hydrateFilesDrafts, initFilesEditor, openFileDoc,
  setFilesTreeRoot, stashFilesDraft, startFilesWatch, stopFilesWatch,
} from "./17-files-editor.js";

// ---------- 文件(R-148):树 + 度量 + AI 用途说明;编辑器在 17-files-editor.js(UI2-0926 #6) ----------
// UX-088 树选择器(主项目 / 各独立任务的工作树)、UX-090 路径筛选、UX-092 三态块 + 展开定位 + 焦点保持、
// UX-094 用途说明的确认 / 停止 / 费用提示都在这里;编辑器那边只认「当前树的根」(filesRoot)。
export let filesSnapshotData = null;
export let filesSortByLines = false;
export const filesExpanded = new Set([""]);
export let filesActivePath = null;
export { humanSize };

// 树的三态(UX-092):loading / error 时树区显示说明块(error 带重试),ready 且没有文件显示空态,不再是一片空白。
let filesLoad = { state: "idle", error: "" };
let filesFilter = "";
let filesLoadedAt = 0;
let annotating = false;
let annotateStopping = false;
let annotateProgress = null;

// 编辑器 → 树:活动文件高亮只在真的切过去之后才变(取消切换时留在原文件);脏标记变化时重画树上的点;
// 保存/新建后静默重扫(行数、大小跟着变),新建的文件展开到可见。
initFilesEditor({
  onActiveChange(path) {
    filesActivePath = path;
    // 链接 / 命令打开的文件要在树里「看得见」:展开祖先目录、被筛选挡住就清掉筛选、滚到可见。
    if (path) {
      expandAncestors(path);
      if (filesFilter && !filesMatcher(filesFilter)?.(path)) setFilesFilter("");
    }
    renderFilesTree();
    scrollActiveIntoView();
  },
  onDirtyChange() {
    renderFilesTree();
  },
  onSaved() {
    void refreshFiles();
  },
  onCreated(path) {
    expandAncestors(path);
    void refreshFiles();
  },
});

function expandAncestors(path) {
  const parts = String(path).split("/");
  for (let i = 1; i < parts.length; i += 1) filesExpanded.add(parts.slice(0, i).join("/"));
}
function viewActive() {
  return Boolean(document.getElementById("view-files")?.classList.contains("active"));
}

export function reset_files_scope() {
  // 切项目是同步的,没法等确认框:未保存的修改先暂存成草稿,回到该文件时恢复。
  stashFilesDraft();
  filesViewLeft();
  setFilesTreeRoot(null); // 回到新项目的主树;内含 resetFilesDoc:关掉当前文件、占位提示
  filesSnapshotData = null;
  filesActivePath = null;
  filesLoad = { state: "idle", error: "" };
  setFilesFilter("");
  $("files-tree")?.replaceChildren();
  syncTreeSelect();
}

// 当前树的根:所选工作树,没选就是当前项目。
function currentRoot() {
  return filesTreeRoot() ?? currentProject;
}

export async function refreshFiles() {
  const root = currentRoot();
  if (!root) return;
  if (!filesSnapshotData) {
    filesLoad = { state: "loading", error: "" };
    renderFilesTree();
  }
  try {
    const snapshot = await invoke("files_snapshot", { projectDir: root });
    if (root !== currentRoot()) return;
    filesSnapshotData = snapshot;
    filesLoad = { state: "ready", error: "" };
    filesLoadedAt = Date.now();
    renderFilesTree();
  } catch (err) {
    if (root !== currentRoot()) return;
    const message = `${t("文件树加载失败")}:${err}`;
    if (filesSnapshotData) {
      // 静默刷新失败:旧树还在,日志面板给原因和重试。
      toastError(message, { retry: refreshFiles });
    } else {
      // 首次加载失败:页内显示带重试的错误块(日志里留一份,不弹日志面板)。
      filesLoad = { state: "error", error: String(err) };
      log(message, "err");
      renderFilesTree();
    }
  }
}
// D-233 批1:切回视图不重扫——filesSnapshotData 是「最近一次成功快照」,
// 视图切换时先拿它渲染(立即可用),后台静默刷新保持新鲜;只有用户点
// 「刷新」按钮(或用途说明生成后)才强制走完整重扫。
export let filesSilentRefresh = null;
export function showFilesView() {
  const view = document.getElementById("view-files");
  if (!view) return;
  view.classList.add("active");
  if (filesSnapshotData) {
    renderFilesTree();
    filesSilentRefresh = setTimeout(refreshFiles, 400);
  } else {
    refreshFiles();
  }
  // 文件页可见期间轮询当前文件的外部改动(代理、别的编辑器);离开即停。
  startFilesWatch();
  // 上次没保存就退出的草稿读回来,树上标点(UX-089)。
  void hydrateFilesDrafts().then((added) => { if (added) renderFilesTree(); });
}
export function filesViewLeft() {
  stopFilesWatch();
  if (filesSilentRefresh) {
    clearTimeout(filesSilentRefresh);
    filesSilentRefresh = null;
  }
}

// ---------- 树选择器(UX-088) ----------
// 项目有独立任务(并行线)的工作树时,文件页可以在「主项目」与各工作树之间切换:路径 chip、工具结果里指向工作树的
// 绝对路径不再报「路径不合法」,而是切到那棵树再打开。后端 files_snapshot / file_* 本来就按传入的目录工作,
// 这里只是把 projectDir 换成工作树根。用途说明(.kanzei/file-annotations.json)是主项目的资产,工作树里不生成。
const samePath = (a, b) => normalizePath(a) === normalizePath(b);
// 树的身份:null = 主项目,否则是工作树根路径。
const sameTree = (a, b) => (a && b ? samePath(a, b) : !a && !b);
function normalizePath(path) {
  return String(path ?? "").replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}
function baseName(path) {
  return String(path ?? "").replace(/\\/g, "/").replace(/\/+$/, "").split("/").pop() || String(path ?? "");
}
function worktreeLines() {
  const lines = [];
  for (const item of Array.isArray(processItems) ? processItems : []) {
    const path = item?.worktree_path;
    if (typeof path !== "string" || !path.trim() || samePath(path, currentProject)) continue;
    if (lines.some((line) => samePath(line.path, path))) continue;
    lines.push({ path, label: String(item.title || item.label || item.branch || baseName(path)) });
  }
  return lines;
}
let treeSelectSig = "";
function syncTreeSelect(force = false) {
  const bar = $("files-treebar");
  const select = $("files-tree-select");
  if (!bar || !select) return;
  const lines = worktreeLines();
  const current = filesTreeRoot();
  bar.classList.toggle("hidden", !lines.length && !current);
  const sig = JSON.stringify([lines, current]);
  if (sig === treeSelectSig && !force) return;
  treeSelectSig = sig;
  select.replaceChildren();
  const option = (value, label) => {
    const node = document.createElement("option");
    node.value = value;
    node.textContent = label;
    select.append(node);
  };
  option("", t("主项目"));
  for (const line of lines) option(line.path, `${t("独立任务")} · ${line.label}`);
  // 选着的那条线已经关掉(不在清单里了):选项保留着,别让下拉悄悄跳回主项目。
  if (current && !lines.some((line) => samePath(line.path, current))) option(current, `${t("独立任务")} · ${baseName(current)}`);
  select.value = current ?? "";
}
/// 换树。有未保存修改先问(保存 / 不保存 / 取消);取消则下拉还原。新树里也有当前打开的那个文件时接着打开它。
export async function switchFilesTree(next) {
  const target = next && !samePath(next, currentProject) ? next : null;
  if (sameTree(target, filesTreeRoot())) return true;
  const reopen = filesDoc?.path ?? null;
  if (!(await confirmLeaveDirty("switch"))) {
    syncTreeSelect(true);
    return false;
  }
  setFilesTreeRoot(target); // 关掉当前文件(未保存的已在上面处理)
  filesActivePath = null;
  filesSnapshotData = null;
  filesLoad = { state: "loading", error: "" };
  syncTreeSelect(true);
  renderFilesTree();
  if (viewActive()) startFilesWatch();
  await refreshFiles();
  void hydrateFilesDrafts().then((added) => { if (added) renderFilesTree(); });
  if (reopen && filesSnapshotData?.files?.some((file) => file.path === reopen)) await openFileDoc({ path: reopen });
  return true;
}
// 绝对路径落在哪棵树里:返回 { tree: null(主项目)| 工作树根, rel }。相对路径、不在任何已知树里的路径返回 null(交给当前树)。
const ABSOLUTE_PATH = /^(?:[A-Za-z]:[\\/]|[\\/])/;
function locateTree(path) {
  const raw = String(path ?? "").trim();
  if (!ABSOLUTE_PATH.test(raw)) return null;
  const normalized = raw.replace(/\\/g, "/");
  const candidates = [{ tree: null, base: currentProject }, ...worktreeLines().map((line) => ({ tree: line.path, base: line.path }))];
  let best = null;
  for (const candidate of candidates) {
    const base = String(candidate.base ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
    if (!base || !normalized.toLowerCase().startsWith(`${base.toLowerCase()}/`)) continue;
    if (!best || base.length > best.base.length) best = { tree: candidate.tree, base };
  }
  return best ? { tree: best.tree, rel: normalized.slice(best.base.length + 1) } : null;
}

// ---------- 路径筛选(UX-090) ----------
// 空格分词、都要命中(路径小写子串);命中的文件连同上级目录一起展开,清空即恢复原来的展开状态。
function filesMatcher(query) {
  const terms = String(query ?? "").toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return null;
  return (path) => {
    const lower = String(path).toLowerCase();
    return terms.every((term) => lower.includes(term));
  };
}
function setFilesFilter(value) {
  filesFilter = String(value ?? "");
  const input = $("files-filter");
  if (input && input.value !== filesFilter) input.value = filesFilter;
}

// 平面清单 → 嵌套树。目录节点带聚合与目录标注。
export function buildFilesTree(snapshot) {
  const root = { name: "", path: "", dirs: new Map(), files: [] };
  for (const file of snapshot.files) {
    const parts = file.path.split("/");
    let node = root;
    for (let i = 0; i < parts.length - 1; i++) {
      const dirPath = parts.slice(0, i + 1).join("/");
      if (!node.dirs.has(parts[i])) {
        node.dirs.set(parts[i], { name: parts[i], path: dirPath, dirs: new Map(), files: [] });
      }
      node = node.dirs.get(parts[i]);
    }
    node.files.push(file);
  }
  return root;
}

// 按钮文字写进带 data-i18n-key 的 span:切语言时 applyDataI18nKeys 就地重算;计数 / 进度放在旁边的 span,不混进译文。
function setLabel(id, key, text) {
  const node = $(id);
  if (!node) return;
  node.dataset.i18nKey = key;
  node.textContent = text;
}
function renderFilesToolbar(snapshot) {
  const inWorktree = Boolean(filesTreeRoot());
  const sortKey = filesSortByLines ? "排序:行数" : "排序:名称";
  setLabel("files-sort-label", sortKey, t(sortKey));
  const annotateBtn = $("files-annotate");
  if (!annotateBtn) return;
  annotateBtn.disabled = inWorktree || (!annotating && !snapshot);
  annotateBtn.title = inWorktree
    ? t("用途说明按主项目生成,工作树里不生成")
    : t("用 fast 模型为每个新增或已变化的文件生成一句话用途说明(每个文件一次模型调用,会消耗 token;可随时停止)");
  const extra = $("files-annotate-count");
  if (annotating) {
    const progress = annotateProgress ? ` ${annotateProgress.done + annotateProgress.failed}/${annotateProgress.total}` : "";
    if (annotateStopping) setLabel("files-annotate-label", "停止中", t("停止中"));
    else setLabel("files-annotate-label", "停止", t("停止"));
    if (extra) extra.textContent = annotateStopping ? "…" : progress;
    annotateBtn.disabled = annotateStopping;
  } else {
    const pending = !inWorktree && snapshot?.unannotated > 0 ? snapshot.unannotated : 0;
    setLabel("files-annotate-label", "用途说明", t("用途说明"));
    if (extra) extra.textContent = pending ? `(${pending})` : "";
  }
}

// 树区的说明块:loading / error(带重试)/ 空 / 筛选无命中。
function filesStateBlock(kind, title, detail = "", retry = null) {
  const box = document.createElement("div");
  box.className = "files-state";
  box.dataset.kind = kind;
  box.setAttribute("role", kind === "error" ? "alert" : "status");
  const head = document.createElement("div");
  head.className = "files-state-title";
  head.textContent = title;
  box.append(head);
  if (detail) {
    const text = document.createElement("div");
    text.className = "files-state-detail";
    text.textContent = detail;
    box.append(text);
  }
  if (retry) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ghost mini";
    button.textContent = t("重试");
    button.addEventListener("click", retry);
    box.append(button);
  }
  return box;
}

export function renderFilesTree() {
  const snapshot = filesSnapshotData;
  const tree = $("files-tree");
  syncTreeSelect();
  renderFilesToolbar(snapshot);
  if (!tree) return;
  if (!snapshot) {
    tree.replaceChildren();
    if (filesLoad.state === "error") tree.append(filesStateBlock("error", t("文件树加载失败"), filesLoad.error, () => void refreshFiles()));
    else if (filesLoad.state === "loading") tree.append(filesStateBlock("loading", `${t("正在读取文件")}…`));
    $("files-summary").textContent = "";
    return;
  }
  const focusKey = focusedRowKey();
  const scrollTop = tree.scrollTop;
  tree.innerHTML = "";
  const total = snapshot.dirs?.[""] ?? { files: snapshot.files.length, size: 0, lines: 0 };
  // 已有用途说明的量/全量(D-213 用户点名):分母只数可标注的(代码/md),不含二进制。工作树里没有用途说明,不显示。
  const annotatedPart =
    snapshot.annotatable > 0 && !filesTreeRoot()
      ? ` · ${t("用途说明")} ${snapshot.annotated}/${snapshot.annotatable}`
      : "";
  const match = filesMatcher(filesFilter);
  const shown = match ? snapshot.files.filter((file) => match(file.path)) : snapshot.files;
  $("files-summary").textContent = match
    ? `${shown.length} ${t("个匹配")} / ${total.files} ${t("个文件")}`
    : `${total.files} ${t("个文件")} · ${humanSize(total.size)} · ${total.lines} ${t("行")}${annotatedPart}`;
  if (!shown.length) {
    tree.append(match
      ? filesStateBlock("empty", t("没有匹配的文件"), filesFilter.trim())
      : filesStateBlock("empty", t("没有找到文件"), t("目录是空的,或者文件都被 .gitignore 忽略了")));
    return;
  }
  renderFilesDir(tree, buildFilesTree({ files: shown }), 0, snapshot, filesDirtyPaths(), Boolean(match));
  tree.scrollTop = scrollTop;
  if (focusKey) {
    const row = Array.from(tree.querySelectorAll(".files-row")).find((node) => rowKey(node) === focusKey);
    row?.focus?.({ preventScroll: true });
  }
}
const rowKey = (row) => `${row.dataset?.kind}:${row.dataset?.path}`;
// 重画前记下哪一行有键盘焦点,画完还给它(展开/折叠、保存后重扫都会整棵重画)。
function focusedRowKey() {
  const active = document.activeElement;
  return active?.classList?.contains("files-row") ? rowKey(active) : null;
}
function scrollActiveIntoView() {
  const row = $("files-tree")?.querySelector?.(".files-file.active");
  row?.scrollIntoView?.({ block: "nearest" });
}

export function filesDirSorted(node, snapshot) {
  const dirs = [...node.dirs.values()];
  const files = [...node.files];
  if (filesSortByLines) {
    dirs.sort((a, b) => (snapshot.dirs?.[b.path]?.lines ?? 0) - (snapshot.dirs?.[a.path]?.lines ?? 0));
    files.sort((a, b) => (b.lines ?? 0) - (a.lines ?? 0) || b.size - a.size);
  } else {
    dirs.sort((a, b) => a.name.localeCompare(b.name));
    files.sort((a, b) => a.path.localeCompare(b.path));
  }
  return { dirs, files };
}

export function renderFilesDir(container, node, depth, snapshot, dirtyPaths = new Set(), forceOpen = false) {
  const { dirs, files } = filesDirSorted(node, snapshot);
  for (const dir of dirs) {
    const stat = snapshot.dirs?.[dir.path] ?? { files: 0, size: 0, lines: 0 };
    const row = document.createElement("div");
    const open = forceOpen || filesExpanded.has(dir.path);
    row.className = "files-row files-dir";
    row.style.paddingLeft = `${8 + depth * 14}px`;
    row.setAttribute("role", "treeitem");
    row.setAttribute("aria-expanded", String(open));
    row.tabIndex = 0;
    row.dataset.kind = "dir";
    row.dataset.path = dir.path;
    row.title = dir.path;
    const arrow = document.createElement("span");
    arrow.className = "files-arrow";
    arrow.textContent = open ? "▾" : "▸";
    const name = document.createElement("span");
    name.className = "files-name";
    name.textContent = `${dir.name}/`;
    const measure = document.createElement("span");
    measure.className = "files-measure";
    measure.textContent = `${stat.files} · ${humanSize(stat.size)} · ${stat.lines} ${t("行")}`;
    row.append(arrow, name, measure);
    const dirNote = snapshot.dirNotes?.[dir.path];
    if (dirNote) {
      const note = document.createElement("span");
      note.className = "files-note";
      note.textContent = dirNote;
      note.title = dirNote;
      row.appendChild(note);
    }
    const toggleDir = () => {
      if (forceOpen) return; // 筛选中目录一律展开,清掉筛选后恢复各自的展开状态
      if (filesExpanded.has(dir.path)) filesExpanded.delete(dir.path);
      else filesExpanded.add(dir.path);
      renderFilesTree();
    };
    row.addEventListener("click", toggleDir);
    row.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggleDir(); }
    });
    container.appendChild(row);
    if (open) renderFilesDir(container, dir, depth + 1, snapshot, dirtyPaths, forceOpen);
  }
  for (const file of files) {
    const row = document.createElement("div");
    const active = file.path === filesActivePath;
    const dirty = dirtyPaths.has(file.path);
    row.className = `files-row files-file${active ? " active" : ""}${dirty ? " dirty" : ""}`;
    row.style.paddingLeft = `${8 + depth * 14 + 14}px`;
    row.setAttribute("role", "treeitem");
    row.setAttribute("aria-selected", active ? "true" : "false");
    row.tabIndex = 0;
    row.dataset.kind = "file";
    row.dataset.path = file.path;
    row.title = file.path;
    const name = document.createElement("span");
    name.className = "files-name";
    name.textContent = file.path.split("/").pop();
    if (dirty) {
      // 未保存:名字后一个琥珀点(与设置页「未保存」同一种颜色),读屏名带上「未保存」。
      const dot = document.createElement("span");
      dot.className = "files-dirty-dot";
      dot.setAttribute("aria-hidden", "true");
      name.appendChild(dot);
    }
    const measure = document.createElement("span");
    measure.className = "files-measure";
    measure.textContent = file.oversized
      ? `${humanSize(file.size)} ${t("过大未计")}`
      : file.lines != null
        ? `${humanSize(file.size)} · ${file.lines} ${t("行")}`
        : file.chars != null
          ? `${humanSize(file.size)} · ${file.chars} ${t("字")}`
          : humanSize(file.size);
    row.append(name, measure);
    if (file.note) {
      const note = document.createElement("span");
      note.className = "files-note";
      note.textContent = file.note;
      note.title = file.note;
      row.appendChild(note);
    }
    if (dirty) row.setAttribute("aria-label", `${row.textContent} · ${t("未保存")}`);
    const openFile = () => openFilePreview(file);
    row.addEventListener("click", openFile);
    row.addEventListener("keydown", (e) => {
      if (e.key === "Enter") openFile();
    });
    container.appendChild(row);
  }
}

/// 打开文件并(可选)定位到行:树、需求页锚点、研究页产物、工具结果里的「打开文件并定位」共用。
/// file = { path, line? };实现在 17-files-editor.js(未保存修改先问、按行定位)。
/// 路径是某条独立任务工作树里的绝对路径(路径 chip、工具结果)时,先把文件树切到那棵树(UX-088)。
export async function openFilePreview(file) {
  const where = locateTree(file?.path);
  if (!where) return openFileDoc(file);
  if (!sameTree(where.tree, filesTreeRoot()) && !(await switchFilesTree(where.tree))) return false;
  return openFileDoc({ ...file, path: where.rel });
}

// ---------- 用途说明(UX-094,原「标注」) ----------
// 一键对全部「新增/已变化」文件各调一次 fast 模型:先确认(写清文件数与成本),生成中同一个按钮变「停止」。
async function onAnnotateClick() {
  if (annotating) {
    if (annotateStopping) return;
    annotateStopping = true;
    renderFilesToolbar(filesSnapshotData);
    try {
      await invoke("files_annotate_cancel");
    } catch (err) {
      annotateStopping = false;
      renderFilesToolbar(filesSnapshotData);
      toastError(`${t("停止失败")}:${err}`);
    }
    return;
  }
  if (filesTreeRoot() || !currentProject) return;
  const pending = filesSnapshotData?.unannotated ?? 0;
  if (!pending) {
    toast(t("所有文件的用途说明都是最新的"));
    return;
  }
  const ok = await confirmDialog({
    title: t("生成用途说明"),
    message: t("将用 fast 模型为 {n} 个文件各生成一句话用途说明。").replace("{n}", String(pending)),
    list: [
      t("每个文件调用一次模型,会消耗 token;文件多时要几分钟"),
      t("随时可以点「停止」,已经生成的会保留"),
      t("只处理新增或内容变化的文件,已有的不会重复生成"),
    ],
    okText: t("开始生成"),
  });
  if (ok !== true) return;
  const root = currentProject;
  annotating = true;
  annotateStopping = false;
  annotateProgress = { done: 0, failed: 0, total: pending };
  renderFilesToolbar(filesSnapshotData);
  try {
    const result = await invoke("files_annotate", { projectDir: root });
    const counts = `${result.annotated}/${result.total}`;
    if (result.failed && result.firstError) {
      // 失败原因必须可见:全 failed 只报数字没有原因,排查无从下手(D-213)。
      toastError(`${result.cancelled ? t("用途说明已停止") : t("用途说明生成完成")}:${counts} · ${result.failed} ${t("失败")} · ${result.firstError}`);
    } else {
      toast(`${result.cancelled ? t("用途说明已停止") : t("用途说明生成完成")}:${counts}${result.failed ? ` · ${result.failed} ${t("失败")}` : ""}`);
    }
    if (root === currentProject && !filesTreeRoot()) await refreshFiles();
  } catch (err) {
    toastError(`${t("用途说明生成失败")}:${err}`);
  } finally {
    annotating = false;
    annotateStopping = false;
    annotateProgress = null;
    renderFilesTree();
  }
}

defer(() => {
  $("files-refresh").addEventListener("click", refreshFiles);
});
defer(() => {
  $("files-sort").addEventListener("click", () => {
    filesSortByLines = !filesSortByLines;
    renderFilesTree();
  });
});
defer(() => {
  $("files-annotate").addEventListener("click", () => void onAnnotateClick());
});
defer(() => {
  $("files-tree-select")?.addEventListener("change", () => void switchFilesTree($("files-tree-select").value || null));
});
defer(() => {
  const input = $("files-filter");
  if (!input) return;
  input.addEventListener("input", () => {
    filesFilter = input.value;
    renderFilesTree();
  });
  // 输入框自己的键:Esc 清空筛选;Enter 打开第一个命中的文件;↓ 把焦点交给树。
  input.addEventListener("keydown", (event) => {
    if (event.isComposing) return;
    if (event.key === "Escape" && filesFilter) {
      event.preventDefault?.();
      setFilesFilter("");
      renderFilesTree();
    } else if (event.key === "Enter") {
      const first = $("files-tree")?.querySelector?.(".files-file");
      if (first) {
        event.preventDefault?.();
        first.click();
      }
    } else if (event.key === "ArrowDown") {
      const first = $("files-tree")?.querySelector?.(".files-row");
      if (first) {
        event.preventDefault?.();
        first.focus?.();
      }
    }
  });
});
// 回到窗口时(别的编辑器、代理改了文件)文件页静默重扫一次:树里的行数、新文件才不会一直是旧的。
defer(() => {
  window.addEventListener("focus", () => {
    if (viewActive() && Date.now() - filesLoadedAt > 5000) void refreshFiles();
  });
});
// 切语言:工具栏按钮、树选择器的文字是渲染点 t() 写的,跟着重画。
defer(() => {
  document.addEventListener("kz:language", () => {
    syncTreeSelect(true);
    renderFilesTree();
  });
});
// UX-012:主窗口关掉了 Tauri 默认的拖放处理(main.rs),页面里的 HTML5 拖放才收得到事件;代价是文件落在没人处理
// 的地方时浏览器会把整个界面导航成那个文件。输入框自己的 drop(08-compose-runtime.js 当附件)冒泡到这里时已经
// preventDefault 过,不受影响;其余位置拖入的文件一律吞掉,光标显示「不可放」。
defer(() => {
  const hasFiles = (event) => Array.from(event.dataTransfer?.types ?? []).includes("Files");
  window.addEventListener("dragover", (event) => {
    if (event.defaultPrevented || !hasFiles(event)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "none";
  });
  window.addEventListener("drop", (event) => {
    if (!event.defaultPrevented && hasFiles(event)) event.preventDefault();
  });
});
// D-381:走 on() 而不是裸 listen。01-core 的 on() 里那套「没有 sessionId 就丢弃」的
// 纪律(注释写得很重:「没有身份就不能安全地投影到当前对话,宁可只留下后端持久事实
// 也不能串线」)只在它自己那条路径上强制;这里绕过去,规则就只覆盖了一半的订阅。
// 本事件确实没有 session 归属(标注是项目级批处理),已登记进 SESSIONLESS_EVENTS。
// 附带好处:订阅失败不再是 `.catch(() => {})` 静默,而是走 on() 的可见报错(D-005)。
defer(() => {
  on("kz:annotate-progress", (e) => {
    const p = e.payload;
    if (!annotating) return;
    annotateProgress = { done: p.done, failed: p.failed, total: p.total };
    renderFilesToolbar(filesSnapshotData);
  });
});
