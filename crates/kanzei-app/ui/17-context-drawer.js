import { $, defer } from "./00-dom.js";
import { confirmDialog } from "./00-dialogs.js";
import { invoke } from "./00-ipc.js";
import { sameProject } from "./00-project-identity.js";
import { activePane } from "./01-chat-panes.js";
import { isGeneralChat } from "./03-general-scope.js";
import { activeProcessId, currentProject, navigate_view, processItems, toast, toastError } from "./03-shell.js";
import { isAbsolutePath, normalizeRoot, relativeToRoot } from "./04-structured-parse.js";
import { openFilePreview, switchFilesTree } from "./17-files.js";

// 对话伴生上下文抽屉:在对话页右侧滑出查看本轮触及文件、Git 改动、原文定位、Diff 对照与轻量编辑保存,
// 避免频繁切去 VS Code 或离开当前对话。
const IMAGE_EXTS = new Set(["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "avif"]);
const NARROW_CHAT_WIDTH = 860;

let drawerOpen = false;
let drawerMode = "source"; // "source" | "diff" | "edit"
let drawerLoadGen = 0;
let drawerWidthPx = null;
let lastGitStatus = null;

// 当前打开的文件上下文与编辑态。
let current = null; // { projectDir, worktreePath, root, path, line }
let loadedPreview = null; // file_preview 返回对象
let loadedDiff = null; // file_git_diff 返回对象
let editDirty = false;
let editSaving = false;
let conflictDiskHash = null;

function extOf(path) {
  const name = String(path ?? "").split("/").pop() || "";
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

function isImagePath(path) {
  return IMAGE_EXTS.has(extOf(path));
}

function activeWorktreePath() {
  const proc = processItems.find((item) => item.id === activeProcessId);
  const wt = proc?.worktree_path;
  if (typeof wt === "string" && wt.trim() && !sameProject(wt, currentProject)) return wt;
  return null;
}

function effectiveRoot(target) {
  return target?.root || target?.worktreePath || target?.projectDir || activeWorktreePath() || currentProject;
}

function syncNarrowLayout() {
  const chat = $("view-chat");
  if (!chat) return;
  if (!drawerOpen) {
    delete chat.dataset.drawerNarrow;
    return;
  }
  const narrow = chat.clientWidth > 0 && chat.clientWidth < NARROW_CHAT_WIDTH;
  if (narrow) chat.dataset.drawerNarrow = "drawer";
  else delete chat.dataset.drawerNarrow;
}

function syncToggleBadge() {
  const btn = $("context-drawer-toggle");
  const badge = $("context-drawer-badge");
  if (!btn) return;
  btn.setAttribute("aria-expanded", String(drawerOpen));
  btn.classList.toggle("active", drawerOpen);
  if (badge) {
    const count = (lastGitStatus?.files?.length ?? 0);
    badge.textContent = count > 0 ? String(count) : "";
    badge.classList.toggle("hidden", count === 0);
  }
}

export function isContextDrawerOpen() {
  return drawerOpen;
}

async function confirmDiscardEdit() {
  if (!editDirty) return true;
  return confirmDialog({
    title: "有未保存的修改",
    message: `「${current?.path ?? ""}」尚未保存，确定放弃修改吗？`,
    okText: "放弃修改",
    danger: true,
  });
}

export async function closeContextDrawer({ force = false } = {}) {
  if (!drawerOpen) return true;
  if (!force && !(await confirmDiscardEdit())) return false;
  editDirty = false;
  conflictDiskHash = null;
  drawerOpen = false;
  const chat = $("view-chat");
  const drawer = $("context-drawer");
  if (chat) {
    delete chat.dataset.drawer;
    delete chat.dataset.drawerNarrow;
  }
  drawer?.classList.add("hidden");
  syncToggleBadge();
  return true;
}

function setDrawerOpenDom() {
  drawerOpen = true;
  const chat = $("view-chat");
  const drawer = $("context-drawer");
  if (chat) {
    chat.dataset.drawer = "open";
    if (drawerWidthPx) chat.style.setProperty("--kz-split-drawer", `${drawerWidthPx}px`);
  }
  drawer?.classList.remove("hidden");
  syncNarrowLayout();
  syncToggleBadge();
}

/// 聚合当前对话 pane 中触及过的项目内文件路径（a.md-path 与工具调用块路径，最新触及在前）。
export function collectSessionTouchedFiles() {
  const pane = activePane;
  const base = normalizeRoot(activeWorktreePath() || currentProject || "");
  if (!pane || !base) return [];
  const seen = new Map();
  const nodes = pane.querySelectorAll("a.md-path, [data-path]");
  for (const node of nodes) {
    const raw = node.dataset?.path;
    if (!raw) continue;
    const cleaned = normalizeRoot(raw);
    let rel = null;
    if (isAbsolutePath(cleaned)) {
      rel = relativeToRoot(cleaned, base) ?? (currentProject ? relativeToRoot(cleaned, normalizeRoot(currentProject)) : null);
    } else {
      rel = cleaned.replace(/^\.\//, "");
    }
    if (!rel || rel.startsWith("..") || rel.includes("://")) continue;
    const line = Number(node.dataset?.line) || null;
    // 后出现覆盖旧记录以保持「最新触及排前」。
    if (seen.has(rel)) seen.delete(rel);
    seen.set(rel, { path: rel, line });
  }
  return [...seen.values()].reverse().slice(0, 24);
}

function renderFileLists() {
  const touchedHost = $("cd-touched-list");
  const gitHost = $("cd-git-list");
  const touchedCount = $("cd-touched-count");
  const gitCount = $("cd-git-count");
  if (!touchedHost || !gitHost) return;

  const touched = collectSessionTouchedFiles();
  if (touchedCount) touchedCount.textContent = touched.length ? String(touched.length) : "0";
  touchedHost.replaceChildren();
  if (!touched.length) {
    const empty = document.createElement("span");
    empty.className = "cd-empty-chip dim";
    empty.textContent = "本轮对话尚未引用项目文件";
    touchedHost.append(empty);
  } else {
    for (const item of touched) {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = `cd-file-chip${current?.path === item.path ? " active" : ""}`;
      chip.title = item.line ? `${item.path}:${item.line}` : item.path;
      const name = document.createElement("span");
      name.className = "cd-chip-name";
      name.textContent = item.path.split("/").pop() || item.path;
      chip.append(name);
      if (item.line) {
        const ln = document.createElement("span");
        ln.className = "cd-chip-line";
        ln.textContent = `:${item.line}`;
        chip.append(ln);
      }
      chip.addEventListener("click", () => void openContextDrawer({ path: item.path, line: item.line, mode: "source" }));
      touchedHost.append(chip);
    }
  }

  const gitFiles = Array.isArray(lastGitStatus?.files) ? lastGitStatus.files : [];
  if (gitCount) gitCount.textContent = gitFiles.length ? String(gitFiles.length) : "0";
  gitHost.replaceChildren();
  if (!gitFiles.length) {
    const empty = document.createElement("span");
    empty.className = "cd-empty-chip dim";
    empty.textContent = lastGitStatus?.repo === "none" ? "当前目录不是 Git 仓库" : "工作区暂无未提交改动";
    gitHost.append(empty);
  } else {
    for (const file of gitFiles) {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = `cd-file-chip${current?.path === file.path ? " active" : ""}`;
      chip.title = file.path;
      const name = document.createElement("span");
      name.className = "cd-chip-name";
      name.textContent = String(file.path).split("/").pop() || file.path;
      chip.append(name);
      const meta = document.createElement("span");
      meta.className = "cd-chip-stat";
      if (file.untracked) {
        meta.textContent = "新";
      } else if (file.binary) {
        meta.textContent = "二进制";
      } else {
        meta.textContent = `+${file.additions ?? 0} -${file.deletions ?? 0}`;
      }
      chip.append(meta);
      chip.addEventListener("click", () => {
        void openContextDrawer({ path: file.path, mode: file.untracked ? "source" : "diff" });
      });
      gitHost.append(chip);
    }
  }
}

function syncModeTabs() {
  for (const btn of document.querySelectorAll("#cd-mode-tabs .cd-tab")) {
    const active = btn.dataset.mode === drawerMode;
    btn.classList.toggle("active", active);
    btn.setAttribute("aria-pressed", String(active));
  }
  const hasFile = Boolean(current?.path);
  $("cd-open-full")?.toggleAttribute("disabled", !hasFile);
  $("cd-reveal")?.toggleAttribute("disabled", !hasFile);
  $("cd-copy-ref")?.toggleAttribute("disabled", !hasFile);
}

function renderEmptyBody(message) {
  const body = $("cd-body");
  if (!body) return;
  body.replaceChildren();
  const box = document.createElement("div");
  box.className = "cd-placeholder dim";
  box.textContent = message;
  body.append(box);
}

async function copyRef(path, line) {
  const text = line ? `${path}:${line}` : path;
  try {
    await navigator.clipboard.writeText(text);
    toast(`已复制: ${text}`, { kind: "ok" });
  } catch (error) {
    toastError(`复制失败: ${error}`);
  }
}

function renderSourceView(preview, { path, line }) {
  const body = $("cd-body");
  if (!body) return;
  body.replaceChildren();
  if (preview.imageUrl) {
    const wrap = document.createElement("div");
    wrap.className = "cd-image-wrap";
    const img = document.createElement("img");
    img.src = preview.imageUrl;
    img.alt = path;
    wrap.append(img);
    body.append(wrap);
    return;
  }
  if (preview.binary) {
    renderEmptyBody(`二进制文件（${preview.size ?? 0} 字节），不支持文本预览。`);
    return;
  }
  if (preview.truncated) {
    const banner = document.createElement("div");
    banner.className = "cd-banner";
    banner.textContent = "文件超过 4 MB，仅预览前 4 MB（只读）。";
    body.append(banner);
  }
  const pre = document.createElement("pre");
  pre.className = "cd-source";
  let targetRow = null;
  const lines = String(preview.content ?? "").replace(/\r\n?/g, "\n").split("\n");
  lines.forEach((text, index) => {
    const lineNo = index + 1;
    const row = document.createElement("div");
    row.className = `cd-line${line === lineNo ? " is-target" : ""}`;
    const num = document.createElement("button");
    num.type = "button";
    num.className = "cd-line-no";
    num.textContent = String(lineNo);
    num.title = `复制 ${path}:${lineNo}`;
    num.addEventListener("click", () => void copyRef(path, lineNo));
    const code = document.createElement("span");
    code.className = "cd-line-code";
    code.textContent = text || " ";
    row.append(num, code);
    if (line === lineNo) targetRow = row;
    pre.append(row);
  });
  body.append(pre);
  if (targetRow) {
    requestAnimationFrame(() => targetRow.scrollIntoView?.({ block: "center" }));
  } else {
    body.scrollTop = 0;
  }
}

function renderDiffView(diffData, { path }) {
  const body = $("cd-body");
  if (!body) return;
  body.replaceChildren();
  if (!diffData || diffData.status === "no_repo") {
    renderEmptyBody("当前目录不是独立 Git 仓库，无法显示 Git Diff。");
    return;
  }
  if (diffData.status === "untracked") {
    const wrap = document.createElement("div");
    wrap.className = "cd-placeholder";
    const p = document.createElement("p");
    p.className = "dim";
    p.textContent = "这是一个未跟踪的新文件，尚未纳入 Git 对比。";
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ghost mini";
    btn.textContent = "查看原文";
    btn.addEventListener("click", () => void switchDrawerMode("source"));
    wrap.append(p, btn);
    body.append(wrap);
    return;
  }
  if (diffData.status === "clean" || !diffData.diff) {
    renderEmptyBody("此文件相对 HEAD 没有未提交改动。");
    return;
  }
  const meta = document.createElement("div");
  meta.className = "cd-diff-meta";
  const add = document.createElement("span");
  add.className = "change-add";
  add.textContent = `+${diffData.additions ?? 0}`;
  const del = document.createElement("span");
  del.className = "change-del";
  del.textContent = `−${diffData.deletions ?? 0}`;
  meta.append(add, del);
  if (diffData.truncated) {
    const note = document.createElement("span");
    note.className = "dim";
    note.textContent = "（Diff 过大，已截断显示前 512 KB）";
    meta.append(note);
  }
  body.append(meta);

  const pre = document.createElement("pre");
  pre.className = "cd-diff";
  for (const rawLine of String(diffData.diff).replace(/\r\n?/g, "\n").split("\n")) {
    const row = document.createElement("div");
    let cls = "cd-diff-line";
    if (rawLine.startsWith("@@")) cls += " is-hunk";
    else if (rawLine.startsWith("+++") || rawLine.startsWith("---") || rawLine.startsWith("diff ") || rawLine.startsWith("index ")) cls += " is-meta";
    else if (rawLine.startsWith("+")) cls += " is-add";
    else if (rawLine.startsWith("-")) cls += " is-del";
    row.className = cls;
    row.textContent = rawLine || " ";
    pre.append(row);
  }
  body.append(pre);
  body.scrollTop = 0;
  void path;
}

function renderEditView(preview) {
  const body = $("cd-body");
  if (!body) return;
  body.replaceChildren();
  if (preview.imageUrl || preview.binary) {
    renderEmptyBody("二进制或图片文件不支持文本编辑。");
    return;
  }
  if (preview.readonly) {
    const banner = document.createElement("div");
    banner.className = "cd-banner is-warn";
    banner.textContent = `只读文件：${preview.readonly}`;
    body.append(banner);
    renderSourceView(preview, current);
    body.prepend(banner);
    return;
  }

  const bar = document.createElement("div");
  bar.className = "cd-edit-bar";
  const info = document.createElement("span");
  info.id = "cd-edit-state";
  info.className = "dim";
  info.textContent = editDirty ? "未保存" : `${preview.encoding || "UTF-8"} · ${preview.eol || "LF"}`;
  const saveBtn = document.createElement("button");
  saveBtn.id = "cd-edit-save";
  saveBtn.type = "button";
  saveBtn.className = "primary mini";
  saveBtn.textContent = "保存 (Ctrl+S)";
  saveBtn.disabled = !editDirty || editSaving;
  const revertBtn = document.createElement("button");
  revertBtn.id = "cd-edit-revert";
  revertBtn.type = "button";
  revertBtn.className = "ghost mini";
  revertBtn.textContent = "重新读取";
  bar.append(info, revertBtn, saveBtn);

  const conflictBar = document.createElement("div");
  conflictBar.id = "cd-edit-conflict";
  conflictBar.className = `cd-banner is-warn${conflictDiskHash ? "" : " hidden"}`;
  const conflictText = document.createElement("span");
  conflictText.textContent = "磁盘上的文件内容已发生变化，未覆盖。";
  const reloadBtn = document.createElement("button");
  reloadBtn.type = "button";
  reloadBtn.className = "ghost mini";
  reloadBtn.textContent = "载入磁盘版本";
  const overwriteBtn = document.createElement("button");
  overwriteBtn.type = "button";
  overwriteBtn.className = "ghost mini danger";
  overwriteBtn.textContent = "覆盖磁盘";
  conflictBar.append(conflictText, reloadBtn, overwriteBtn);

  const editor = document.createElement("textarea");
  editor.id = "cd-editor";
  editor.className = "cd-editor";
  editor.spellcheck = false;
  editor.value = String(preview.content ?? "");

  const updateState = () => {
    editDirty = editor.value !== String(loadedPreview?.content ?? "");
    saveBtn.disabled = !editDirty || editSaving;
    info.textContent = editSaving ? "正在保存…" : editDirty ? "未保存" : `${loadedPreview?.encoding || "UTF-8"} · ${loadedPreview?.eol || "LF"}`;
  };

  const doSave = async (overwrite = false) => {
    if (!current || !loadedPreview || editSaving) return;
    editSaving = true;
    updateState();
    try {
      const root = effectiveRoot(current);
      const res = await invoke("file_write", {
        projectDir: root,
        path: current.path,
        content: editor.value,
        expectedHash: loadedPreview.hash ?? null,
        bom: Boolean(loadedPreview.bom),
        eol: loadedPreview.eol || "lf",
        overwrite,
      });
      if (res?.status === "conflict") {
        conflictDiskHash = res.diskHash || "changed";
        conflictBar.classList.remove("hidden");
        toastError("保存中止：磁盘文件已被外部修改");
        return;
      }
      conflictDiskHash = null;
      conflictBar.classList.add("hidden");
      loadedPreview = { ...loadedPreview, content: editor.value, hash: res?.hash ?? loadedPreview.hash };
      loadedDiff = null;
      editDirty = false;
      toast(`已保存: ${current.path}`, { kind: "ok" });
      document.dispatchEvent(new CustomEvent("kz:context-file-saved", { detail: { path: current.path } }));
    } catch (error) {
      toastError(`保存失败: ${error}`);
    } finally {
      editSaving = false;
      updateState();
    }
  };

  editor.addEventListener("input", updateState);
  editor.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      if (!saveBtn.disabled) void doSave(false);
    } else if (event.key === "Tab") {
      event.preventDefault();
      const start = editor.selectionStart;
      const end = editor.selectionEnd;
      editor.setRangeText("  ", start, end, "end");
      updateState();
    }
  });
  saveBtn.addEventListener("click", () => void doSave(false));
  revertBtn.addEventListener("click", async () => {
    if (editDirty && !(await confirmDiscardEdit())) return;
    editDirty = false;
    conflictDiskHash = null;
    await loadCurrentContent({ forceReload: true });
  });
  reloadBtn.addEventListener("click", async () => {
    editDirty = false;
    conflictDiskHash = null;
    await loadCurrentContent({ forceReload: true });
  });
  overwriteBtn.addEventListener("click", () => void doSave(true));

  body.append(bar, conflictBar, editor);
  if (current?.line && current.line > 1) {
    const lines = editor.value.split("\n");
    const pos = lines.slice(0, current.line - 1).reduce((sum, l) => sum + l.length + 1, 0);
    editor.focus();
    editor.setSelectionRange(pos, pos);
  }
}

async function loadCurrentContent({ forceReload = false } = {}) {
  const gen = ++drawerLoadGen;
  syncModeTabs();
  renderFileLists();
  const pathEl = $("cd-path");
  const metaEl = $("cd-meta");
  if (!current?.path) {
    if (pathEl) pathEl.textContent = "未选择文件";
    if (metaEl) metaEl.textContent = "";
    renderEmptyBody("在对话中点击任意文件路径，或从上方选择本轮触及文件 / Git 改动文件。");
    return;
  }
  if (pathEl) {
    pathEl.textContent = current.line ? `${current.path}:${current.line}` : current.path;
    pathEl.title = current.path;
  }
  if (metaEl) metaEl.textContent = "读取中…";
  const root = effectiveRoot(current);
  try {
    if (drawerMode === "diff") {
      if (!loadedDiff || forceReload) {
        loadedDiff = await invoke("file_git_diff", {
          projectDir: current.projectDir || currentProject,
          path: current.path,
          worktreePath: current.worktreePath || null,
        });
      }
      if (gen !== drawerLoadGen) return;
      if (metaEl) metaEl.textContent = loadedDiff?.status === "modified" ? `+${loadedDiff.additions} −${loadedDiff.deletions}` : "";
      renderDiffView(loadedDiff, current);
      return;
    }
    if (!loadedPreview || forceReload) {
      if (isImagePath(current.path)) {
        const img = await invoke("file_image", { projectDir: root, path: current.path });
        loadedPreview = { imageUrl: `data:${img.mime};base64,${img.base64}`, binary: true, size: img.size, readonly: "图片文件不可文本编辑" };
      } else {
        loadedPreview = await invoke("file_preview", { projectDir: root, path: current.path });
      }
    }
    if (gen !== drawerLoadGen) return;
    if (metaEl) {
      const sizeText = loadedPreview?.size != null ? `${loadedPreview.size} B` : "";
      metaEl.textContent = loadedPreview?.readonly ? `${sizeText} · 只读` : sizeText;
    }
    if (drawerMode === "edit") renderEditView(loadedPreview);
    else renderSourceView(loadedPreview, current);
  } catch (error) {
    if (gen !== drawerLoadGen) return;
    if (metaEl) metaEl.textContent = "读取失败";
    renderEmptyBody(`无法读取文件：${error}`);
  }
}

export async function switchDrawerMode(nextMode) {
  if (nextMode === drawerMode) return true;
  if (drawerMode === "edit" && !(await confirmDiscardEdit())) return false;
  editDirty = false;
  conflictDiskHash = null;
  drawerMode = nextMode;
  await loadCurrentContent();
  return true;
}

/// 打开对话伴生上下文抽屉。不传 path 时仅打开抽屉并展示文件清单与上次文件。
export async function openContextDrawer(options = {}) {
  if (!currentProject || isGeneralChat()) {
    toast("当前对话未关联项目目录");
    return false;
  }
  const hasNewPath = typeof options.path === "string" && options.path.trim().length > 0;
  if (hasNewPath && current && current.path !== options.path && !(await confirmDiscardEdit())) {
    return false;
  }
  setDrawerOpenDom();
  if (hasNewPath) {
    const projectDir = options.projectDir || currentProject;
    const worktreePath = options.worktreePath !== undefined ? options.worktreePath : activeWorktreePath();
    const root = options.root || worktreePath || projectDir;
    const cleaned = normalizeRoot(options.path);
    const rel = isAbsolutePath(cleaned)
      ? (relativeToRoot(cleaned, normalizeRoot(root)) ?? relativeToRoot(cleaned, normalizeRoot(projectDir)) ?? cleaned)
      : cleaned.replace(/^\.\//, "");
    const sameFile = current && current.path === rel && sameProject(effectiveRoot(current), root);
    current = { projectDir, worktreePath, root, path: rel, line: Number(options.line) || null };
    if (!sameFile) {
      loadedPreview = null;
      loadedDiff = null;
      editDirty = false;
      conflictDiskHash = null;
    }
    if (options.mode) drawerMode = options.mode;
    else if (!sameFile) drawerMode = "source";
  } else if (options.mode) {
    drawerMode = options.mode;
  }
  // 没有拿到过 git_status 时顺手拉一次，保证 Git 改动列表立即可见。
  if (!lastGitStatus && currentProject) {
    void invoke("git_status", { projectDir: currentProject, worktreePath: activeWorktreePath() })
      .then((status) => {
        lastGitStatus = status;
        syncToggleBadge();
        if (drawerOpen) renderFileLists();
      })
      .catch(() => {});
  }
  await loadCurrentContent();
  return true;
}

export async function toggleContextDrawer() {
  if (drawerOpen) return closeContextDrawer();
  return openContextDrawer();
}

function installResizeHandle() {
  const handle = $("cd-resize");
  const chat = $("view-chat");
  if (!handle || !chat) return;
  handle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    const rect = chat.getBoundingClientRect();
    const onMove = (moveEvent) => {
      const width = Math.round(Math.max(320, Math.min(rect.right - moveEvent.clientX, rect.width - 360)));
      drawerWidthPx = width;
      chat.style.setProperty("--kz-split-drawer", `${width}px`);
    };
    const onUp = () => {
      document.removeEventListener("pointermove", onMove, true);
      document.removeEventListener("pointerup", onUp, true);
      document.removeEventListener("pointercancel", onUp, true);
    };
    document.addEventListener("pointermove", onMove, true);
    document.addEventListener("pointerup", onUp, true);
    document.addEventListener("pointercancel", onUp, true);
  });
}

defer(() => {
  $("context-drawer-toggle")?.addEventListener("click", () => void toggleContextDrawer());
  $("cd-close")?.addEventListener("click", () => void closeContextDrawer());
  $("cd-back-chat")?.addEventListener("click", () => void closeContextDrawer());
  $("cd-refresh")?.addEventListener("click", () => void loadCurrentContent({ forceReload: true }));
  $("cd-copy-ref")?.addEventListener("click", () => {
    if (current?.path) void copyRef(current.path, current.line);
  });
  $("cd-reveal")?.addEventListener("click", () => {
    if (!current?.path) return;
    void invoke("reveal_path", { projectDir: current.projectDir || currentProject, path: current.path })
      .catch((error) => toastError(`打开失败: ${error}`));
  });
  $("cd-open-full")?.addEventListener("click", async () => {
    if (!current?.path) return;
    if (drawerMode === "edit" && !(await confirmDiscardEdit())) return;
    const targetRoot = effectiveRoot(current);
    const targetPath = current.path;
    const targetLine = current.line;
    if (targetRoot && !sameProject(targetRoot, currentProject)) {
      await switchFilesTree(targetRoot);
    }
    navigate_view("files");
    void openFilePreview({ path: targetPath, line: targetLine });
  });
  for (const btn of document.querySelectorAll("#cd-mode-tabs .cd-tab")) {
    btn.addEventListener("click", () => void switchDrawerMode(btn.dataset.mode));
  }
  document.addEventListener("kz:git-status", (event) => {
    lastGitStatus = event.detail ?? null;
    syncToggleBadge();
    if (drawerOpen) renderFileLists();
  });
  document.addEventListener("kz:conversation-selected", () => {
    loadedDiff = null;
    if (drawerOpen) renderFileLists();
  });
  window.addEventListener("resize", () => syncNarrowLayout());
  installResizeHandle();
  syncToggleBadge();
});
