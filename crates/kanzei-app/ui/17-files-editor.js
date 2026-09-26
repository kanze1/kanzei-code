import { isModalOpen } from "./00-surface.js";
import { $, confirmDialog, defer, inputDialog, invoke } from "./01-core.js";
import { languageIsEnglish, t } from "./02-i18n.js";
import { currentProject, currentTheme, toast, toastError } from "./03-shell.js";

// ---------- 文件页编辑(UI2-0926 #6「文件浏览要带编辑功能，而且也是做成可拖拽伸缩的」) ----------
// 设计见 docs/design/files_editor.md。本模块只管「当前打开的那一个文件」:Monaco 编辑器、脏标记、
// 保存(按内容指纹比较并交换,后端 file_write)、放弃修改、冲突横幅(比较 / 用磁盘版本 / 覆盖磁盘版本)、
// 外部改动轮询(file_stat 粗筛 → file_preview 比指纹)、切项目时的草稿暂存、新建文件、从链接定位到行。
// 树与度量留在 17-files.js;两边用 initFilesEditor 注册的回调解耦(本模块不 import 17-files.js)。

export function humanSize(bytes) {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)}KB`;
  return `${bytes}B`;
}

// 只读原因码(后端 files_edit.rs 的 readonly / READONLY:<code>)→ 人话。托管文档要说清楚为什么、去哪改。
export const READONLY_TEXT = {
  binary: "二进制文件,不能在这里编辑",
  truncated: "文件超过 4MB,只预览前 4MB,不能在这里编辑",
  encoding: "不是 UTF-8 编码(如 GBK),在这里保存会写坏原字节",
  managed: "托管文档(需求/缺陷/记忆等)只能经专用工具修改:直接改会被托管围栏隔离并回滚。请到对应页面编辑",
  git: "Git 内部文件,只读",
  internal: "kanzei 内部状态文件,只读",
  attr: "文件带只读属性,不能在这里保存",
  unknown: "后端没有返回内容指纹,无法安全保存(请更新 kzapp)",
};
export function readonlyText(code) {
  return t(READONLY_TEXT[code] ?? READONLY_TEXT.unknown);
}
// 后端 Err("READONLY:<code>") → 原因码;其余错误原样。
function readonlyCodeOf(error) {
  const match = /READONLY:([a-z]+)/.exec(String(error ?? ""));
  return match ? match[1] : null;
}
// 「标签:内容」:中文紧贴全角习惯的半角冒号,英文冒号后留一个空格("Save refused: …")。
function labelled(key, text) {
  return `${t(key)}${languageIsEnglish() ? ": " : ":"}${text}`;
}
function errorText(error) {
  const code = readonlyCodeOf(error);
  return code ? readonlyText(code) : String(error);
}

// ---------- Monaco 懒加载(测试接缝:setMonacoLoader,仿 04-markdown.js setRenderMarkdown) ----------
export let monacoLoadPromise = null;
function defaultMonacoLoader() {
  return new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = "vendor/monaco/loader.js";
    script.onload = () => {
      const base = new URL("vendor/monaco/", document.baseURI).href;
      const paths = { vs: base.replace(/\/$/, "") };
      window.require.config({ paths });
      const boot = () => {
        // Monaco's default worker assumes a directory named vs. Keep our vendor path mapping in workers too.
        const worker = new Blob([
          `self.MonacoEnvironment = ${JSON.stringify({ baseUrl: base })};`,
          `self.require = ${JSON.stringify({ paths })};`,
          `self._VSCODE_NLS_MESSAGES = ${JSON.stringify(globalThis._VSCODE_NLS_MESSAGES)};`,
          `self._VSCODE_NLS_LANGUAGE = ${JSON.stringify(globalThis._VSCODE_NLS_LANGUAGE)};`,
          `importScripts(${JSON.stringify(`${base}base/worker/workerMain.js`)});`,
        ], { type: "application/javascript" });
        const workerUrl = URL.createObjectURL(worker);
        globalThis.MonacoEnvironment = { ...globalThis.MonacoEnvironment, getWorkerUrl: () => workerUrl };
        window.require(["vs/editor/editor.main"], () => resolve(window.monaco), reject);
      };
      // The bundled translation is an AMD module: load it after the loader, before the editor.
      if (languageIsEnglish()) boot();
      else window.require(["vs/nls.messages.zh-cn"], boot, boot);
    };
    script.onerror = () => reject(new Error("monaco loader load failed"));
    document.head.appendChild(script);
  });
}
let monacoLoader = defaultMonacoLoader;
export function setMonacoLoader(fn) {
  monacoLoader = typeof fn === "function" ? fn : defaultMonacoLoader;
  monacoLoadPromise = null;
}
// Monaco 懒加载:切到文件页并首次打开文件才拉起,不拖慢主界面启动。
export function loadMonaco() {
  if (!monacoLoadPromise) {
    monacoLoadPromise = Promise.resolve().then(() => monacoLoader());
    // 加载失败不把坏的 promise 缓存下来:下次打开文件再试一次。
    monacoLoadPromise.catch(() => { monacoLoadPromise = null; });
  }
  return monacoLoadPromise;
}

// ---------- 状态 ----------
// doc:当前打开的文件。hash 是打开(或上次保存)时的内容指纹,保存时交给后端比较并交换;
// savedVersion 是那一刻 model 的 alternativeVersionId(撤销回原点 = 干净);conflict 是磁盘现状 {hash, exists}。
export let filesDoc = null;
export let filesEditor = null;
let monacoApi = null;
let openGeneration = 0;
let lastDirty = false;
let applyingDisk = false; // 读盘内容正在写进 model(不是用户输入):内容监听据此不清同步提示
let syncNote = "";
const drafts = new Map(); // `${root}\n${path}` → { content, hash, bom, eol }
const draftKey = (root, path) => `${root}\n${path}`;
let compare = null; // { diff, original }
let watchTimer = null;
let watchBusy = false;
export const FILES_WATCH_MS = 2000;

const hooks = { onActiveChange() {}, onDirtyChange() {}, onSaved() {}, onCreated() {} };
/// 17-files.js 在模块求值时注册:树高亮、树上的脏标记、保存后静默重扫、新建后展开祖先目录。
export function initFilesEditor(next = {}) {
  for (const key of Object.keys(hooks)) if (typeof next[key] === "function") hooks[key] = next[key];
}

// 脏 = model 离开了干净点。只有「打开时即只读」(doc.readonly)的文件永远不脏;保存被后端拒(doc.blocked:
// 打开之后磁盘上的文件变成只读属性/非 UTF-8/超 4MB/二进制)不算只读——修改还在编辑器里,切文件照样要确认,
// 切项目照样存草稿。
export function isFilesDirty() {
  const model = filesEditor?.getModel?.();
  return Boolean(filesDoc && !filesDoc.readonly && model && model.getAlternativeVersionId() !== filesDoc.savedVersion);
}
/// 树上要标「未保存」的路径:当前文件(脏时)+ 本项目暂存的草稿。
export function filesDirtyPaths(root = currentProject) {
  const paths = new Set();
  if (isFilesDirty() && filesDoc.root === root) paths.add(filesDoc.path);
  for (const key of drafts.keys()) {
    const [draftRoot, path] = key.split("\n");
    if (draftRoot === root) paths.add(path);
  }
  return paths;
}
export function filesDraftCount() {
  return drafts.size;
}

// 工具结果里的路径可能是项目根下的绝对路径(C:/…/src/a.rs):转成相对根的路径;其余原样交给后端判定。
export function toProjectRel(path, root = currentProject) {
  let rel = String(path ?? "").replace(/\\/g, "/").trim();
  const base = String(root ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
  if (base && rel.toLowerCase().startsWith(`${base.toLowerCase()}/`)) rel = rel.slice(base.length + 1);
  return rel.replace(/^(\.\/)+/, "");
}

// ---------- 头部 / 横幅 / 占位 ----------
function show(el, visible) {
  el?.classList.toggle("hidden", !visible);
}
// 头部每次内容变化都会重画:文字没变就不碰节点。role=alert / role=status 的容器里换一次文本节点,
// 读屏就重读一遍——冲突横幅不能每敲一个键被念一次。
function setText(el, text) {
  if (el && el.textContent !== text) el.textContent = text;
}
function metaText(doc) {
  if (!doc) return "";
  if (doc.binary) return `${t("二进制文件")} · ${humanSize(doc.size)}`;
  const parts = [humanSize(doc.size)];
  if (doc.encoding === "utf-8") parts.push(doc.bom ? "UTF-8 BOM" : "UTF-8");
  parts.push(doc.eol === "crlf" ? "CRLF" : "LF");
  if (doc.truncated) parts.push(t("已截断预览前 4MB"));
  if (doc.mixedEol && !doc.readonly) parts.push(`${t("混合换行,保存时统一为")} ${doc.eol === "crlf" ? "CRLF" : "LF"}`);
  return parts.join(" · ");
}
export function renderFilesHead() {
  const doc = filesDoc;
  const dirty = isFilesDirty();
  show($("files-preview-head"), Boolean(doc));
  if (!doc) {
    for (const id of ["files-readonly", "files-conflict", "files-compare-head"]) show($(id), false);
    if (lastDirty) {
      lastDirty = false;
      hooks.onDirtyChange(false);
    }
    return;
  }
  const pathEl = $("files-preview-path");
  setText(pathEl, doc.path);
  if (pathEl && pathEl.title !== doc.path) pathEl.title = doc.path;
  setText($("files-preview-meta"), metaText(doc));
  setText($("files-sync"), syncNote);
  show($("files-dirty"), dirty);
  const save = $("files-save");
  const discard = $("files-discard");
  show(save, !doc.readonly);
  show(discard, dirty && !doc.readonly);
  save.disabled = !dirty || doc.saving || Boolean(doc.readonly);
  save.setAttribute("aria-busy", doc.saving ? "true" : "false");
  // 只读原因条:为什么只读、去哪改(托管文档给跳转);保存被拒(doc.blocked)也在这里说明,编辑器照样可编辑、可复制。
  // 打开即只读的二进制文件,原因占位里已经写了,不再重复一条。
  const reason = doc.readonly || doc.blocked;
  show($("files-readonly"), Boolean(reason) && !(doc.readonly && doc.binary));
  if (reason) {
    setText($("files-readonly-text"), doc.readonly
      ? labelled("只读", readonlyText(doc.readonly))
      : `${labelled("保存被拒", readonlyText(doc.blocked))} · ${t("你的修改还在编辑器里,可以复制出来;处理好之后再保存,或放弃修改")}`);
    const goto = $("files-readonly-goto");
    const memory = /^\.kanzei\/memory\//i.test(doc.path);
    show(goto, reason === "managed");
    goto.dataset.view = memory ? "memory" : "documents";
    setText(goto, memory ? t("打开记忆页") : t("打开需求页"));
  }
  // 冲突横幅。
  const conflict = doc.conflict;
  show($("files-conflict"), Boolean(conflict));
  if (conflict) {
    setText($("files-conflict-text"), conflict.exists
      ? t("磁盘上的文件在你打开之后被改动过(可能是代理或其它程序)。你的修改还在编辑器里,选一个处理方式:")
      : t("这个文件已在磁盘上被删除。你的修改还在编辑器里:"));
    show($("files-compare"), conflict.exists);
    $("files-compare").setAttribute("aria-pressed", compare ? "true" : "false");
    setText($("files-use-disk"), conflict.exists ? t("用磁盘版本") : t("放弃修改"));
    setText($("files-overwrite"), conflict.exists ? t("覆盖磁盘版本") : t("重新创建"));
    for (const id of ["files-compare", "files-use-disk", "files-overwrite"]) $(id).disabled = Boolean(doc.saving);
  }
  show($("files-compare-head"), Boolean(compare));
  if (dirty !== lastDirty) {
    lastDirty = dirty;
    hooks.onDirtyChange(dirty);
  }
}
function showPlaceholder(text) {
  const placeholder = $("files-placeholder");
  setText(placeholder, text);
  show(placeholder, true);
  show($("files-editor"), false);
  show($("files-diff"), false);
}

// ---------- 编辑器 ----------
function ensureEditor(monaco) {
  monacoApi = monaco;
  if (filesEditor) return filesEditor;
  // R-189:Monaco 主题跟随全局(暗=vs-dark/亮=vs);CSS 变量到不了 Monaco,用与 03-shell.js 相同的主题源。
  const monacoTheme = typeof currentTheme === "function" && currentTheme() === "light" ? "vs" : "vs-dark";
  filesEditor = monaco.editor.create($("files-editor"), {
    readOnly: true,
    automaticLayout: true,
    theme: monacoTheme,
    minimap: { enabled: true },
    fontSize: 13,
    scrollBeyondLastLine: false,
    // 默认 "prompt" 会在打开含 U+2028 的文件时弹窗;原样保留,不改用户的字节。
    unusualLineTerminators: "off",
  });
  // Ctrl/Cmd+S:编辑器命中键位时 Monaco 自己 preventDefault(WebView2 不再弹「另存为」)。
  filesEditor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => void saveFilesDoc());
  return filesEditor;
}
function eolSequence(monaco, eol) {
  return eol === "crlf" ? monaco.editor.EndOfLineSequence.CRLF : monaco.editor.EndOfLineSequence.LF;
}
function watchModel(model) {
  if (model._kzFilesWatched) return;
  model._kzFilesWatched = true;
  model.onDidChangeContent(() => {
    if (applyingDisk) return;
    if (filesDoc && !filesDoc.readonly) syncNote = ""; // 用户开始改了,「已从磁盘更新」不再成立
    renderFilesHead();
  });
}
function disposeModel(model) {
  try { model?.dispose?.(); } catch { /* 已释放 */ }
}
// 把 model 换成 text,但只改变化的那一段,并保留撤销:「用磁盘版本」之后 Ctrl+Z 还能回到自己的修改;
// 代理改了别处时光标、选区随编辑平移而不是跳走(整篇替换会把光标挤到替换区的边上)。
// 先把 text 的换行统一成 model 当前的换行(Monaco 插入时本来也会这么做),两边同一种换行、不含孤立 CR,
// 公共前后缀的边界就不会落在 CRLF 中间;再退开代理对(UTF-16 高/低位),不把一个字符劈成两半。
// 换行本身的变化(LF ↔ CRLF)由调用方随后 setEOL。
function replaceContent(model, text) {
  if (typeof model.pushEditOperations !== "function" || typeof model.getPositionAt !== "function") {
    model.setValue(text);
    return;
  }
  const eol = model.getEOL?.() === "\r\n" ? "\r\n" : "\n";
  const before = model.getValue();
  const after = String(text ?? "").split(/\r\n|\r|\n/).join(eol);
  if (before === after) return;
  const limit = Math.min(before.length, after.length);
  let start = 0;
  while (start < limit && before.charCodeAt(start) === after.charCodeAt(start)) start += 1;
  if (start > 0 && /[\uD800-\uDBFF]/.test(before[start - 1])) start -= 1;
  let tail = 0;
  while (tail < limit - start && before.charCodeAt(before.length - 1 - tail) === after.charCodeAt(after.length - 1 - tail)) tail += 1;
  if (tail > 0 && /[\uDC00-\uDFFF]/.test(before[before.length - tail])) tail -= 1;
  const from = model.getPositionAt(start);
  const to = model.getPositionAt(before.length - tail);
  model.pushStackElement?.();
  model.pushEditOperations([], [{
    range: { startLineNumber: from.lineNumber, startColumn: from.column, endLineNumber: to.lineNumber, endColumn: to.column },
    text: after.slice(start, after.length - tail),
  }], () => null);
  model.pushStackElement?.();
}
function applyReadonly(doc) {
  filesEditor?.updateOptions({
    readOnly: Boolean(doc.readonly),
    readOnlyMessage: { value: doc.readonly ? readonlyText(doc.readonly) : "" },
    unusualLineTerminators: "off",
  });
}
function revealLine(line) {
  const model = filesEditor?.getModel?.();
  if (!model || !(line > 0)) return;
  const target = Math.min(Math.floor(line), model.getLineCount());
  filesEditor.revealLineInCenter(target);
  filesEditor.setPosition({ lineNumber: target, column: 1 });
  filesEditor.setSelection({ startLineNumber: target, startColumn: 1, endLineNumber: target, endColumn: model.getLineMaxColumn(target) });
}

function docFromPreview(root, path, preview) {
  const hash = typeof preview.hash === "string" ? preview.hash : null;
  const readonly = preview.readonly || (preview.binary ? "binary" : preview.truncated ? "truncated" : hash ? null : "unknown");
  return {
    root, path, hash, readonly,
    binary: Boolean(preview.binary),
    truncated: Boolean(preview.truncated),
    bom: Boolean(preview.bom),
    eol: preview.eol === "crlf" ? "crlf" : "lf",
    mixedEol: Boolean(preview.mixedEol),
    encoding: preview.encoding ?? (preview.binary ? "unknown" : "utf-8"),
    size: Number(preview.size) || 0,
    mtimeMs: preview.mtimeMs ?? null,
    savedVersion: null,
    saving: false,
    conflict: null,
    blocked: null, // 保存被后端以 READONLY:<code> 拒绝(打开之后磁盘上的文件变了性质);不同于打开即只读的 readonly
  };
}

// 读盘并装进编辑器。keep = 同一文件重载(外部改动/用磁盘版本/放弃修改):复用 model,保留撤销,只改变化的区间
// (光标与选区随编辑平移)。preview = 调用方刚读到的磁盘版本(轮询已经读过一次,不再读第二次);
// ifClean = 只在编辑器仍干净时替换(轮询的静默重载):等待途中用户开始打字了,就改进冲突态,不盖掉。
async function loadDoc(root, path, { line = 0, keep = false, note = "", preview: given = null, ifClean = false } = {}) {
  const generation = ++openGeneration;
  const isCurrent = () => generation === openGeneration && root === currentProject;
  closeCompare();
  if (!keep) hooks.onActiveChange(path);
  syncNote = "";
  let preview = given;
  try {
    if (!preview) preview = await invoke("file_preview", { projectDir: root, path });
    if (!isCurrent()) return false;
    if (!preview || typeof preview !== "object") throw new Error(t("后端没有返回文件内容"));
  } catch (error) {
    if (!isCurrent()) return false;
    // 重载时读盘失败(多半是刚被删了):编辑器里还有未保存的修改就绝不释放,改成「已删除」冲突让用户选。
    if (keep && isFilesDirty()) {
      filesDoc.conflict = { hash: null, exists: false };
      renderFilesHead();
      toast(labelled("读取磁盘版本失败", error), { kind: "warn" });
      return false;
    }
    const detached = filesEditor?.getModel?.();
    filesEditor?.setModel?.(null);
    disposeModel(detached);
    filesDoc = null;
    renderFilesHead();
    showPlaceholder(labelled("预览失败", error));
    return false;
  }
  const doc = docFromPreview(root, path, preview);
  const key = draftKey(root, path);
  const draft = drafts.get(key) ?? null;
  // 有草稿、但磁盘上的文件在这期间变得不能保存了(只读属性/非 UTF-8/超限/二进制):草稿照样恢复进编辑器
  // (能看、能复制),按「保存被拒」处理而不是只读——否则草稿既恢复不出来,也永远不会被清掉。
  if (draft && doc.readonly) {
    doc.blocked = doc.readonly;
    doc.readonly = null;
  }
  if (doc.binary && !draft) {
    const detached = filesEditor?.getModel?.();
    filesEditor?.setModel?.(null);
    disposeModel(detached);
    filesDoc = doc;
    renderFilesHead();
    showPlaceholder(`${t("二进制文件")} · ${humanSize(doc.size)}`);
    return true;
  }
  show($("files-placeholder"), false);
  show($("files-editor"), true);
  let monaco;
  try {
    monaco = await loadMonaco();
  } catch (error) {
    if (!isCurrent()) return false;
    showPlaceholder(labelled("预览失败", error));
    return false;
  }
  if (!isCurrent()) return false;
  // 轮询的静默重载:脏检查与替换之间隔着 await,这期间用户开始打字了——不替换,改进冲突态(编辑器内容不动)。
  if (ifClean && isFilesDirty()) {
    filesDoc.conflict = { hash: doc.hash, exists: true };
    renderFilesHead();
    return false;
  }
  const editor = ensureEditor(monaco);
  const uri = monaco.Uri.file(path);
  const previous = editor.getModel();
  let model = keep && previous ? previous : monaco.editor.getModel(uri);
  applyingDisk = true;
  try {
    if (model) {
      if (model.getValue() !== preview.content) replaceContent(model, preview.content);
    } else {
      model = monaco.editor.createModel(preview.content, undefined, uri);
    }
    // 换行按后端探测显式设定(Monaco 建模同一条多数派规则;CRLF 文件保存后仍是 CRLF)。
    model.setEOL(eolSequence(monaco, doc.eol));
    watchModel(model);
    if (previous !== model) {
      editor.setModel(model);
      disposeModel(previous);
    }
    doc.savedVersion = model.getAlternativeVersionId();
    filesDoc = doc;
    applyReadonly(doc);
    // 切项目时暂存的草稿:回到该文件时恢复(变脏,Ctrl+Z 回到磁盘版本);草稿之后磁盘又变了 → 直接进冲突态。
    if (draft) {
      drafts.delete(key);
      replaceContent(model, draft.content);
      if (draft.hash !== doc.hash) {
        doc.conflict = { hash: doc.hash, exists: true };
        doc.hash = draft.hash;
      }
    }
  } finally {
    applyingDisk = false;
  }
  if (draft) toast(t("已恢复未保存的修改"), { kind: "info" });
  syncNote = note;
  renderFilesHead();
  if (line > 0) revealLine(line);
  return true;
}

/// 打开文件(树、链接、新建共用入口)。有未保存修改时先问:保存 / 不保存 / 取消。返回是否已切换。
export async function openFileDoc(file) {
  const root = currentProject;
  if (!root || !file?.path) return false;
  const path = toProjectRel(file.path, root);
  const line = Number(file.line) > 0 ? Math.floor(Number(file.line)) : 0;
  if (filesDoc && filesDoc.root === root && filesDoc.path === path) {
    if (line > 0) revealLine(line);
    return true;
  }
  if (isFilesDirty()) {
    // 保存已被后端拒过(磁盘上的文件变得不能写):不给「保存」,只给 不保存 / 取消,并说明切走会丢。
    const blocked = filesDoc.blocked;
    const choice = await confirmDialog(blocked ? {
      title: t("未保存的修改"),
      message: `${filesDoc.path} ${t("有未保存的修改,但保存被拒")}`,
      list: [readonlyText(blocked), t("切换会丢掉这些修改;要留着就先取消,把内容复制出来。")],
      okText: t("不保存"),
      danger: true,
    } : {
      title: t("未保存的修改"),
      message: `${filesDoc.path} ${t("有未保存的修改。切换前要保存吗?")}`,
      okText: t("保存"),
      safeText: t("不保存"),
    });
    if (choice === false) return false; // 取消:留在原文件,树高亮不动
    if (choice === true && !blocked && !(await saveFilesDoc())) return false; // 保存没成功(冲突/出错)就不走
    if (root !== currentProject) return false;
  }
  return loadDoc(root, path, { line });
}

// ---------- 保存 ----------
/// overwrite = 冲突横幅上的「覆盖磁盘版本 / 重新创建」:按磁盘现状的指纹交换,覆盖前后端留证。
export async function saveFilesDoc({ overwrite = false } = {}) {
  const doc = filesDoc;
  const model = filesEditor?.getModel?.();
  if (!doc || !model || doc.saving) return false;
  if (doc.readonly) {
    toast(readonlyText(doc.readonly), { kind: "warn" });
    return false;
  }
  if (doc.conflict && !overwrite) {
    // 冲突未决时普通保存一定被拒:明说该点哪里,不让 Ctrl+S 静默失败。
    toast(t("磁盘上的版本已变:先在横幅里选「覆盖磁盘版本」或「用磁盘版本」"), { kind: "warn" });
    $("files-overwrite")?.focus?.();
    return false;
  }
  if (!overwrite && !isFilesDirty()) return true;
  const version = model.getAlternativeVersionId();
  const expectedHash = overwrite ? (doc.conflict?.exists ? doc.conflict.hash : null) : doc.hash;
  const evidence = Boolean(overwrite && doc.conflict?.exists);
  doc.saving = true;
  renderFilesHead();
  try {
    const result = await invoke("file_write", {
      projectDir: doc.root, path: doc.path, content: model.getValue(), expectedHash, bom: doc.bom, evidence,
    });
    if (filesDoc !== doc) return false;
    if (result?.status === "conflict") {
      doc.conflict = { hash: result.hash ?? null, exists: result.exists !== false };
      if (overwrite) toast(t("磁盘上的版本又变了,已刷新冲突信息,请重新选择"), { kind: "warn" });
      return false;
    }
    if (result?.status !== "saved") throw new Error(t("后端没有确认写入"));
    doc.hash = result.hash;
    doc.size = Number(result.size) || 0;
    doc.mtimeMs = result.mtimeMs ?? null;
    doc.savedVersion = version; // 保存期间继续输入的部分仍算未保存
    doc.conflict = null;
    doc.blocked = null;
    closeCompare();
    drafts.delete(draftKey(doc.root, doc.path));
    syncNote = "";
    toast(result.evidence ? `${t("已保存")} · ${labelled("被覆盖的磁盘版本已留证", result.evidence)}` : t("已保存"), { kind: "ok" });
    hooks.onSaved(doc.path);
    return true;
  } catch (error) {
    if (filesDoc !== doc) return false;
    const code = readonlyCodeOf(error);
    if (code) {
      // 打开之后磁盘上的文件变了性质(只读属性/非 UTF-8/超 4MB/二进制):记成「保存被拒」,不改 doc.readonly——
      // 修改仍算未保存、编辑器仍可编辑可复制,切文件照样确认、切项目照样存草稿;原因写进只读原因条。
      doc.blocked = code;
      toast(labelled("保存被拒", readonlyText(code)), { kind: "warn" });
    } else {
      toastError(labelled("保存失败", errorText(error)));
    }
    return false;
  } finally {
    doc.saving = false;
    renderFilesHead();
  }
}

/// 重新读盘覆盖编辑器(保留撤销:Ctrl+Z 能回到刚才的修改)。
export async function reloadFromDisk({ note = "" } = {}) {
  const doc = filesDoc;
  if (!doc) return false;
  drafts.delete(draftKey(doc.root, doc.path));
  return loadDoc(doc.root, doc.path, { keep: true, note });
}
export async function discardFilesChanges() {
  if (!filesDoc || !isFilesDirty()) return false;
  const ok = await confirmDialog({
    title: t("放弃修改"),
    message: `${t("放弃对这个文件的未保存修改,恢复成磁盘上的版本?")} ${filesDoc.path}`,
    okText: t("放弃修改"),
    danger: true,
  });
  if (ok !== true || !filesDoc) return false;
  return reloadFromDisk();
}
// 文件已在磁盘上被删、用户选「放弃修改」:关掉这个文件。
function closeDeletedDoc() {
  const doc = filesDoc;
  if (!doc) return;
  drafts.delete(draftKey(doc.root, doc.path));
  const detached = filesEditor?.getModel?.();
  filesEditor?.setModel?.(null);
  disposeModel(detached);
  filesDoc = null;
  openGeneration += 1;
  renderFilesHead();
  hooks.onActiveChange(null);
  showPlaceholder(labelled("文件已在磁盘上被删除", doc.path));
}

// ---------- 比较(冲突时:原始侧 = 磁盘版本,修改侧 = 你的 model,可继续编辑;窄时 Monaco 自动改为上下内联) ----------
export async function openFilesCompare() {
  const doc = filesDoc;
  const model = filesEditor?.getModel?.();
  if (!doc || !model || !monacoApi || compare) return false;
  let preview;
  try {
    preview = await invoke("file_preview", { projectDir: doc.root, path: doc.path });
  } catch (error) {
    toastError(labelled("读取磁盘版本失败", error));
    return false;
  }
  if (filesDoc !== doc || compare) return false;
  if (doc.conflict && typeof preview?.hash === "string") doc.conflict.hash = preview.hash;
  const original = monacoApi.editor.createModel(String(preview?.content ?? ""), model.getLanguageId?.());
  const diff = monacoApi.editor.createDiffEditor($("files-diff"), {
    automaticLayout: true,
    renderSideBySide: true,
    originalEditable: false,
    readOnly: false,
    // 中缝的「还原」按钮栏:Monaco 自带样式给它画一圈 1px 焦点蓝框(未分层 CSS,压不过),且与横幅上的三个选择重复。
    renderGutterMenu: false,
    minimap: { enabled: false },
    fontSize: 13,
    scrollBeyondLastLine: false,
  });
  diff.setModel({ original, modified: model });
  diff.getModifiedEditor?.()?.addCommand?.(monacoApi.KeyMod.CtrlCmd | monacoApi.KeyCode.KeyS, () => void saveFilesDoc());
  compare = { diff, original };
  show($("files-editor"), false);
  show($("files-diff"), true);
  renderFilesHead();
  return true;
}
export function closeCompare() {
  if (!compare) return;
  const { diff, original } = compare;
  compare = null;
  try { diff.setModel?.(null); } catch { /* 已释放 */ }
  try { diff.dispose?.(); } catch { /* 已释放 */ }
  disposeModel(original);
  show($("files-diff"), false);
  if (filesDoc && !filesDoc.binary) show($("files-editor"), true);
  renderFilesHead();
}
export function isFilesComparing() {
  return Boolean(compare);
}

// ---------- 外部改动轮询 ----------
// 文件页可见时每 2 秒 + 窗口回到前台时:file_stat 粗筛(大小/修改时间),变了才 file_preview 比指纹。
// 干净 → 静默重载并提示;有未保存修改 → 进冲突态(不动编辑器里的内容)。
export async function filesWatchTick() {
  const doc = filesDoc;
  if (!doc || doc.binary || doc.saving || doc.conflict || compare || watchBusy) return false;
  if (typeof document !== "undefined" && document.visibilityState === "hidden") return false;
  if (!$("view-files")?.classList.contains("active")) return false;
  watchBusy = true;
  const generation = openGeneration;
  const stale = () => filesDoc !== doc || generation !== openGeneration;
  try {
    const stat = await invoke("file_stat", { projectDir: doc.root, path: doc.path });
    if (stale() || !stat) return false;
    if (!stat.exists) {
      if (isFilesDirty()) doc.conflict = { hash: null, exists: false };
      else closeDeletedDoc();
      renderFilesHead();
      return true;
    }
    if (stat.size === doc.size && stat.mtimeMs === doc.mtimeMs) return false;
    const preview = await invoke("file_preview", { projectDir: doc.root, path: doc.path });
    if (stale() || !preview) return false;
    if (typeof preview.hash === "string" && preview.hash === doc.hash) {
      // 只是 touch(内容没变):记下新的大小/时间,下次不再读内容。
      doc.size = Number(preview.size) || doc.size;
      doc.mtimeMs = preview.mtimeMs ?? stat.mtimeMs;
      return false;
    }
    if (isFilesDirty()) {
      doc.conflict = { hash: typeof preview.hash === "string" ? preview.hash : null, exists: true };
      renderFilesHead();
      return true;
    }
    // 刚读到的就是新版本:交给 loadDoc 直接用,不再读第二次(第二次读盘途中打的字会被盖掉);
    // ifClean 兜住余下的 await——那期间变脏就进冲突态。
    await loadDoc(doc.root, doc.path, { keep: true, note: t("已从磁盘更新"), preview, ifClean: true });
    return true;
  } catch {
    return false; // 轮询失败不打扰;下一拍再试
  } finally {
    watchBusy = false;
  }
}
export function startFilesWatch() {
  stopFilesWatch();
  watchTimer = setInterval(() => void filesWatchTick(), FILES_WATCH_MS);
  void filesWatchTick();
}
export function stopFilesWatch() {
  if (watchTimer) clearInterval(watchTimer);
  watchTimer = null;
}

// ---------- 切项目:暂存草稿并清空 ----------
/// 切项目是同步的(activate_execution_root),没法等确认框:有未保存修改就存成草稿,回到该文件时恢复。
export function stashFilesDraft() {
  if (!isFilesDirty()) return false;
  const doc = filesDoc;
  drafts.set(draftKey(doc.root, doc.path), {
    content: filesEditor.getModel().getValue(), hash: doc.hash, bom: doc.bom, eol: doc.eol,
  });
  toast(labelled("未保存的修改已暂存,回到该文件时恢复", doc.path), { kind: "info" });
  return true;
}
export function resetFilesDoc() {
  openGeneration += 1;
  stopFilesWatch();
  closeCompare();
  const detached = filesEditor?.getModel?.();
  filesEditor?.setModel?.(null);
  disposeModel(detached);
  filesDoc = null;
  syncNote = "";
  renderFilesHead();
  show($("files-editor"), false);
  const placeholder = $("files-placeholder");
  if (placeholder) {
    placeholder.textContent = t("选择左侧文件查看内容 · 目录行显示聚合度量 · 「标注」用 fast 模型生成用途说明");
    show(placeholder, true);
  }
}

// ---------- 新建文件 ----------
export async function createNewFile() {
  const root = currentProject;
  if (!root) return false;
  const base = filesDoc?.path?.includes("/") ? filesDoc.path.slice(0, filesDoc.path.lastIndexOf("/") + 1) : "";
  const value = await inputDialog({
    title: t("新建文件"),
    message: t("相对项目根的路径;父目录不存在会自动创建"),
    value: base,
    placeholder: "src/new_file.rs",
    okText: t("创建文件"),
  });
  if (value == null || root !== currentProject) return false;
  const path = toProjectRel(value, root);
  if (!path || path.endsWith("/")) {
    toast(t("请填写文件名"), { kind: "warn" });
    return false;
  }
  try {
    const result = await invoke("file_write", { projectDir: root, path, content: "", expectedHash: null, bom: false, evidence: false });
    if (root !== currentProject) return false;
    if (result?.status === "conflict") toast(t("文件已存在,已直接打开"), { kind: "info" });
    else toast(labelled("已创建", path), { kind: "ok" });
    hooks.onCreated(path);
  } catch (error) {
    toast(labelled("新建失败", errorText(error)), { kind: "warn" });
    return false;
  }
  const opened = await openFileDoc({ path });
  if (opened) filesEditor?.focus?.();
  return opened;
}

// ---------- 接线 ----------
defer(() => {
  $("files-save")?.addEventListener("click", () => void saveFilesDoc());
  $("files-discard")?.addEventListener("click", () => void discardFilesChanges());
  $("files-overwrite")?.addEventListener("click", () => void saveFilesDoc({ overwrite: true }));
  $("files-use-disk")?.addEventListener("click", () => {
    if (filesDoc?.conflict && !filesDoc.conflict.exists) closeDeletedDoc();
    else void reloadFromDisk();
  });
  $("files-compare")?.addEventListener("click", () => (compare ? closeCompare() : void openFilesCompare()));
  $("files-compare-close")?.addEventListener("click", () => closeCompare());
  // 比较视图里的 Esc 挂在容器自己身上(Monaco 自己的查找框等先消费 Esc;没人要才关比较)。
  $("files-diff")?.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !event.defaultPrevented) {
      event.preventDefault?.();
      closeCompare();
    }
  });
  $("files-new")?.addEventListener("click", () => void createNewFile());
  $("files-readonly-goto")?.addEventListener("click", () => {
    const view = $("files-readonly-goto")?.dataset?.view || "documents";
    document.querySelector(`.activity-item[data-view="${view}"]`)?.click();
  });
  // 焦点在树/头部时的 Ctrl/Cmd+S 兜底:编辑器里的由 Monaco 命令处理(已 preventDefault)。
  window.addEventListener("keydown", (event) => {
    if (event.defaultPrevented || event.altKey || event.shiftKey || !(event.ctrlKey || event.metaKey)) return;
    if (String(event.key).toLowerCase() !== "s") return;
    if (!$("view-files")?.classList.contains("active") || isModalOpen()) return;
    event.preventDefault();
    void saveFilesDoc();
  });
  window.addEventListener("focus", () => void filesWatchTick());
  // 切语言:头部、横幅按钮、只读原因条的文字都是渲染点 t() 写的(没有 data-i18n-key),Monaco 的只读提示也是。
  document.addEventListener("kz:language", () => {
    if (filesDoc) applyReadonly(filesDoc);
    renderFilesHead();
  });
});
