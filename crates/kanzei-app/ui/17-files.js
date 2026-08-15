// ---------- 文件导览(R-148):树 + 度量 + Monaco 只读预览 + AI 用途标注 ----------
let filesSnapshotData = null;
let filesSortByLines = false;
const filesExpanded = new Set([""]);
let filesActivePath = null;
let monacoLoadPromise = null;
let filesEditor = null;

async function refreshFiles() {
  if (!currentProject) return;
  try {
    filesSnapshotData = await invoke("files_snapshot", { projectDir: currentProject });
    renderFilesTree();
  } catch (err) {
    toastError(`${t("文件树加载失败")}:${err}`);
  }
}
// D-233 批1:切回视图不重扫——filesSnapshotData 是「最近一次成功快照」,
// 视图切换时先拿它渲染(立即可用),后台静默刷新保持新鲜;只有用户点
// 「刷新」按钮(或标注后)才强制走完整重扫。
let filesSilentRefresh = null;
function showFilesView() {
  const view = document.getElementById("view-files");
  if (!view) return;
  view.classList.add("active");
  if (filesSnapshotData) {
    renderFilesTree();
    filesSilentRefresh = setTimeout(refreshFiles, 400);
  } else {
    refreshFiles();
  }
}
function filesViewLeft() {
  if (filesSilentRefresh) {
    clearTimeout(filesSilentRefresh);
    filesSilentRefresh = null;
  }
}

function humanSize(bytes) {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)}KB`;
  return `${bytes}B`;
}

// 平面清单 → 嵌套树。目录节点带聚合与目录标注。
function buildFilesTree(snapshot) {
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

function renderFilesTree() {
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
  renderFilesDir(tree, buildFilesTree(snapshot), 0, snapshot);
}

function filesDirSorted(node, snapshot) {
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

function renderFilesDir(container, node, depth, snapshot) {
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
    if (open) renderFilesDir(container, dir, depth + 1, snapshot);
  }
  for (const file of files) {
    const row = document.createElement("div");
    row.className = `files-row files-file${file.path === filesActivePath ? " active" : ""}`;
    row.style.paddingLeft = `${8 + depth * 14 + 14}px`;
    row.setAttribute("role", "treeitem");
    row.tabIndex = 0;
    const name = document.createElement("span");
    name.className = "files-name";
    name.textContent = file.path.split("/").pop();
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
    const openFile = () => openFilePreview(file);
    row.addEventListener("click", openFile);
    row.addEventListener("keydown", (e) => {
      if (e.key === "Enter") openFile();
    });
    container.appendChild(row);
  }
}

// Monaco 懒加载:切到文件页并首次打开文件才拉起,不拖慢主界面启动。
function loadMonaco() {
  if (monacoLoadPromise) return monacoLoadPromise;
  monacoLoadPromise = new Promise((resolve, reject) => {
    const boot = () => {
      const script = document.createElement("script");
      script.src = "vendor/monaco/loader.js";
      script.onload = () => {
        window.require.config({ paths: { vs: "vendor/monaco" } });
        window.require(["vs/editor/editor.main"], () => resolve(window.monaco), reject);
      };
      script.onerror = () => reject(new Error("monaco loader load failed"));
      document.head.appendChild(script);
    };
    if (!languageIsEnglish()) {
      // nls 必须先于 loader 注入,Monaco 启动时读全局翻译表。
      const nls = document.createElement("script");
      nls.src = "vendor/monaco/nls.messages.zh-cn.js";
      nls.onload = boot;
      nls.onerror = boot; // 翻译丢了退回英文界面,不挡功能
      document.head.appendChild(nls);
    } else {
      boot();
    }
  });
  return monacoLoadPromise;
}

async function openFilePreview(file) {
  filesActivePath = file.path;
  renderFilesTree();
  const head = $("files-preview-head");
  head.classList.remove("hidden");
  $("files-preview-path").textContent = file.path;
  $("files-preview-meta").textContent = "";
  const placeholder = $("files-placeholder");
  try {
    const preview = await invoke("file_preview", { projectDir: currentProject, path: file.path });
    if (preview.binary) {
      placeholder.textContent = `${t("二进制文件")} · ${humanSize(preview.size)}`;
      placeholder.classList.remove("hidden");
      $("files-editor").classList.add("hidden");
      return;
    }
    placeholder.classList.add("hidden");
    $("files-editor").classList.remove("hidden");
    const monaco = await loadMonaco();
    if (!filesEditor) {
      // R-189:Monaco 主题跟随全局(暗=vs-dark/亮=vs);CSS 变量到不了 Monaco,
      // 用与 03-shell.js 相同的主题源,初始即正确。
      const monacoTheme = typeof currentTheme === "function" && currentTheme() === "light" ? "vs" : "vs-dark";
      filesEditor = monaco.editor.create($("files-editor"), {
        readOnly: true,
        automaticLayout: true,
        theme: monacoTheme,
        minimap: { enabled: true },
        fontSize: 13,
        scrollBeyondLastLine: false,
      });
    }
    const uri = monaco.Uri.file(file.path);
    let model = monaco.editor.getModel(uri);
    if (model) {
      model.setValue(preview.content);
    } else {
      model = monaco.editor.createModel(preview.content, undefined, uri);
    }
    const old = filesEditor.getModel();
    filesEditor.setModel(model);
    if (old && old !== model) old.dispose();
    $("files-preview-meta").textContent = `${humanSize(preview.size)}${preview.truncated ? ` · ${t("已截断预览前 4MB")}` : ""}`;
  } catch (err) {
    placeholder.textContent = `${t("预览失败")}:${err}`;
    placeholder.classList.remove("hidden");
  }
}

$("files-refresh").addEventListener("click", refreshFiles);
$("files-sort").addEventListener("click", () => {
  filesSortByLines = !filesSortByLines;
  $("files-sort").textContent = filesSortByLines ? t("按名称") : t("按行数");
  renderFilesTree();
});
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
// D-381:走 on() 而不是裸 listen。01-core 的 on() 里那套「没有 sessionId 就丢弃」的
// 纪律(注释写得很重:「没有身份就不能安全地投影到当前对话,宁可只留下后端持久事实
// 也不能串线」)只在它自己那条路径上强制;这里绕过去,规则就只覆盖了一半的订阅。
// 本事件确实没有 session 归属(标注是项目级批处理),已登记进 SESSIONLESS_EVENTS。
// 附带好处:订阅失败不再是 `.catch(() => {})` 静默,而是走 on() 的可见报错(D-005)。
on("kz:annotate-progress", (e) => {
  const p = e.payload;
  const btn = $("files-annotate");
  if (btn.disabled) btn.textContent = `${t("标注中")} ${p.done + p.failed}/${p.total}`;
});
