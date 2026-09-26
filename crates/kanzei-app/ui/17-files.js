import { defer } from "./01-core.js";
import { $, invoke, on } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, toast, toastError } from "./03-shell.js";
import {
  filesDirtyPaths, humanSize, initFilesEditor, openFileDoc, resetFilesDoc, stashFilesDraft, startFilesWatch, stopFilesWatch,
} from "./17-files-editor.js";

// ---------- 文件导览(R-148):树 + 度量 + AI 用途标注;编辑器在 17-files-editor.js(UI2-0926 #6) ----------
export let filesSnapshotData = null;
export let filesSortByLines = false;
export const filesExpanded = new Set([""]);
export let filesActivePath = null;
export { humanSize };

// 编辑器 → 树:活动文件高亮只在真的切过去之后才变(取消切换时留在原文件);脏标记变化时重画树上的点;
// 保存/新建后静默重扫(行数、大小跟着变),新建的文件展开到可见。
initFilesEditor({
  onActiveChange(path) {
    filesActivePath = path;
    renderFilesTree();
  },
  onDirtyChange() {
    renderFilesTree();
  },
  onSaved() {
    void refreshFiles();
  },
  onCreated(path) {
    const parts = String(path).split("/");
    for (let i = 1; i < parts.length; i += 1) filesExpanded.add(parts.slice(0, i).join("/"));
    void refreshFiles();
  },
});

export function reset_files_scope() {
  // 切项目是同步的,没法等确认框:未保存的修改先暂存成草稿,回到该文件时恢复。
  stashFilesDraft();
  filesViewLeft();
  resetFilesDoc();
  filesSnapshotData = null;
  filesActivePath = null;
  $("files-tree")?.replaceChildren();
}

export async function refreshFiles() {
  if (!currentProject) return;
  const root = currentProject;
  try {
    const snapshot = await invoke("files_snapshot", { projectDir: root });
    if (root !== currentProject) return;
    filesSnapshotData = snapshot;
    renderFilesTree();
  } catch (err) {
    toastError(`${t("文件树加载失败")}:${err}`);
  }
}
// D-233 批1:切回视图不重扫——filesSnapshotData 是「最近一次成功快照」,
// 视图切换时先拿它渲染(立即可用),后台静默刷新保持新鲜;只有用户点
// 「刷新」按钮(或标注后)才强制走完整重扫。
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
}
export function filesViewLeft() {
  stopFilesWatch();
  if (filesSilentRefresh) {
    clearTimeout(filesSilentRefresh);
    filesSilentRefresh = null;
  }
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

export function renderFilesTree() {
  const snapshot = filesSnapshotData;
  if (!snapshot) return;
  const tree = $("files-tree");
  tree.innerHTML = "";
  const total = snapshot.dirs?.[""] ?? { files: snapshot.files.length, size: 0, lines: 0 };
  // 已标注量/全量(D-213 用户点名):分母只数可标注的(代码/md),不含二进制。
  const annotatedPart =
    snapshot.annotatable > 0
      ? ` · ${t("已标注")} ${snapshot.annotated}/${snapshot.annotatable}`
      : "";
  $("files-summary").textContent = `${total.files} ${t("个文件")} · ${humanSize(total.size)} · ${total.lines} ${t("行")}${annotatedPart}`;
  const annotateBtn = $("files-annotate");
  if (!annotateBtn.disabled) {
    annotateBtn.textContent = snapshot.unannotated > 0 ? `${t("标注")}(${snapshot.unannotated})` : t("标注");
  }
  renderFilesDir(tree, buildFilesTree(snapshot), 0, snapshot, filesDirtyPaths());
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

export function renderFilesDir(container, node, depth, snapshot, dirtyPaths = new Set()) {
  const { dirs, files } = filesDirSorted(node, snapshot);
  for (const dir of dirs) {
    const stat = snapshot.dirs?.[dir.path] ?? { files: 0, size: 0, lines: 0 };
    const row = document.createElement("div");
    const open = filesExpanded.has(dir.path);
    row.className = "files-row files-dir";
    row.style.paddingLeft = `${8 + depth * 14}px`;
    row.setAttribute("role", "treeitem");
    row.setAttribute("aria-expanded", String(open));
    row.tabIndex = 0;
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
      if (filesExpanded.has(dir.path)) filesExpanded.delete(dir.path);
      else filesExpanded.add(dir.path);
      renderFilesTree();
    };
    row.addEventListener("click", toggleDir);
    row.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggleDir(); }
    });
    container.appendChild(row);
    if (open) renderFilesDir(container, dir, depth + 1, snapshot, dirtyPaths);
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
export function openFilePreview(file) {
  return openFileDoc(file);
}

defer(() => {
  $("files-refresh").addEventListener("click", refreshFiles);
});
defer(() => {
  $("files-sort").addEventListener("click", () => {
    filesSortByLines = !filesSortByLines;
    $("files-sort").textContent = filesSortByLines ? t("按名称") : t("按行数");
    renderFilesTree();
  });
});
defer(() => {
  $("files-annotate").addEventListener("click", async () => {
    const btn = $("files-annotate");
    if (btn.disabled) return;
    btn.disabled = true;
    btn.textContent = `${t("标注中")}…`;
    try {
      const result = await invoke("files_annotate", { projectDir: currentProject });
      if (result.failed && result.firstError) {
        // 失败原因必须可见:全 failed 只报数字没有原因,排查无从下手(D-213)。
        toastError(`${t("标注完成")}:${result.annotated}/${result.total} · ${result.failed} ${t("失败")} · ${result.firstError}`);
      } else {
        toast(`${t("标注完成")}:${result.annotated}/${result.total}${result.failed ? ` · ${result.failed} ${t("失败")}` : ""}`);
      }
      await refreshFiles();
    } catch (err) {
      toastError(`${t("标注失败")}:${err}`);
    } finally {
      btn.disabled = false;
      renderFilesTree();
    }
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
    const btn = $("files-annotate");
    if (btn.disabled) btn.textContent = `${t("标注中")} ${p.done + p.failed}/${p.total}`;
  });
});
