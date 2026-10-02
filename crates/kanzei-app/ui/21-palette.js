import { closeSurface, isModalOpen, openDialog } from "./00-surface.js";
import { defer } from "./01-core.js";
import { $, invoke, isImeComposing, on, promptBox } from "./01-core.js";
import { localizeDynamic, t } from "./02-i18n.js";
import { backAvailable, currentProject, expandSidebar, log, navigate_back, navigate_view } from "./03-shell.js";
import { state } from "./08-compose.js";
import { projectMenuEntries, switchProject } from "./09-sessions.js";
import { openSessionHistoryAnywhere, sessionPaletteEntries } from "./12-session-tree.js";
import { openFilePreview } from "./17-files.js";

// ---------- 命令面板(Ctrl/Cmd+P) ----------
// 这个应用有 10 个主视图、N 个项目、N 条并行线,外加一批散在各处的动作按钮。
// 想切到「运行画像」得先认出 rail 上第 9 个图标;想切到 p14 得先在侧栏滚到它。
// 命令面板把这三类目标合并成一次键盘检索。
//
// 关键设计:**候选项不持有自己的行为**,一律 `.click()` 既有控件。视图切换、项目
// 选择、线路切换、新对话、停止……每一条都已经有唯一实现和自己的守卫(比如运行中
// 不许开新对话、运行中不许切历史)。面板复制一份行为 = 复制一份要同步的守卫,
// 迟早漂移;点它本人则永远和界面上的按钮同一个语义。
export const PALETTE_LIMIT = 40;
export let paletteEntries = [];
export let paletteIndex = 0;
let paletteMatched = [];

export function paletteEl() {
  return $("palette");
}
export function paletteIsOpen() {
  return !paletteEl()?.classList.contains("hidden");
}

/// 采集候选项。每次打开时重新采集——项目/线路是活的,缓存只会给出过期清单。
export function collectPaletteEntries() {
  const entries = [];
  const push = (group, label, detail, run) => {
    if (!label) return;
    entries.push({ group, label, detail: detail || "", run });
  };

  for (const button of document.querySelectorAll("#activitybar .activity-item[data-view]")) {
    if (button.classList.contains("hidden")) continue;
    const label = localizeDynamic(button.dataset.i18nTitle || button.title || button.dataset.view);
    push(t("视图"), label, "", () => button.click());
  }

  // UI2-0926 #1:侧栏不再有项目列表可点,项目读 projects_get 的偏好缓存;切换走 switchProject——
  // 侧栏项目菜单、项目总览卡片、这里三处都调它,仍然是「一处实现」。
  for (const project of projectMenuEntries()) {
    push(t("项目"), project.name, project.path, () => void switchProject(project.path));
  }

  // 对话:读会话模型(名字/类型/运行态与侧栏树同一份),不再抓隐藏的线路行文字(那里露 pN/「默认」)。
  // 选中后切到那段对话;不在对话页就跳回对话,别让「切了线」页面纹丝不动(UX-052)。
  for (const entry of sessionPaletteEntries()) push(t("对话"), entry.label, entry.detail, entry.run);

  // 返回上一页(非对话页才有):与页头「← 返回」、Esc、Alt+←、鼠标侧键同一个导航栈。
  if (backAvailable()) push(t("动作"), t("返回"), "Esc / Alt+←", () => void navigate_back());

  // 动作:只收**当前可用**的(隐藏或禁用的不进清单——面板不该提供点了没反应的条目)。
  // 「可用」必须连祖先一起看:控件自己没有 hidden/disabled,但住在收起的 <details>
  // 里时,.click() 的可见后果是零。这类宿主先展开再点,而不是把它当作不可用剔掉——
  // 剔掉的话「搜索」这类功能就从面板里凭空消失了。
  // 研究空间没有「当前项目」和讨论入口:新讨论按钮被 CSS 藏起来(点了毫无反应),记需求/记缺陷/记想法/总结
  // 点了只会弹「先选择项目」。这些条目在研究空间不进清单(面板自述「不提供点了没反应的条目」)。
  const devOnlyIds = new Set(["new-chat", "summarize-btn", "documents-new-req", "documents-new-defect", "documents-new-idea"]);
  const action = (label, id) => {
    const el = $(id);
    if (!el || el.disabled || el.classList.contains("hidden")) return;
    if (document.body.dataset.space === "research" && devOnlyIds.has(id)) return;
    // 这里**不替它展开宿主 details**。预先展开会让处理器看到「宿主已开 + 自己没有
    // hidden 类」从而判定为"当前可见"、反手执行关闭——被修的那条高危路径原样复活。
    // 展开责任统一留在各自的 click 处理器里(见 07-events.js chat-search-toggle),
    // 那样无论从菜单点还是从面板点,行为都是同一套。
    push(t("动作"), label, "", () => el.click());
  };
  action(t("新讨论"), "new-chat");
  // 「对话与历史」:与侧栏时钟图标同一个弹层(展开侧栏、回到对话页再打开)。
  if (document.body.dataset.space !== "research") push(t("动作"), t("对话与历史"), "", () => openSessionHistoryAnywhere());
  // 「切换项目」:项目菜单已不在开发档界面里(#project-switch 是看不见的死入口,菜单还会漂在左上角),
  // 改成展开侧栏并聚焦项目列表(当前项目优先),方向键 / Enter 在那里选(UX-028)。开发空间里有项目才给。
  if (document.body.dataset.space !== "research" && projectMenuEntries().length) {
    push(t("动作"), t("切换项目"), "", () => {
      expandSidebar();
      const list = $("workbench-project-list");
      (list?.querySelector('[aria-current="page"]') ?? list?.querySelector("button, [role=\"treeitem\"]"))?.focus?.();
    });
  }
  action(t("添加项目文件夹…"), "project-add");
  action(t("新建项目…"), "project-init");
  action(t("停止"), "stop");
  action(t("总结"), "summarize-btn");
  action(t("复制上下文"), "copy-context");
  action(t("搜索"), "chat-search-toggle");
  action(t("活动"), "tasks-toggle");
  // UI2-0926 #8:网页预览(rail 开关同一实现:关着就打开,开着就收起)与文件页「在预览中打开」。
  action(t("网页预览"), "preview-toggle");
  action(t("在预览中打开当前文件"), "files-open-preview");
  action(t("切换主题"), "theme-toggle");
  action(t("新建独立任务"), "worktree-add");
  // 记需求/记缺陷/记想法(UX-053/B7):点需求页「＋ 新建」菜单里的同名项——它们会打开需求页上**可见**的表单。
  // 原来点的是被隐藏的侧栏里的 #req-quick/#defect-quick,表单进了 0×0 的容器,点了毫无反应。
  action(t("记需求"), "documents-new-req");
  action(t("记缺陷"), "documents-new-defect");
  action(t("记想法"), "documents-new-idea");
  // 开发规范(UX-007):入口在需求页「更多」菜单里,面板给一条直达;状态(有建议稿待审阅 / 未创建)写在候选的细字上。
  if (document.body.dataset.space !== "research" && currentProject) {
    const conventions = $("documents-conventions-open");
    const conventionsState = conventions?.dataset.state;
    push(t("动作"), t("开发规范…"), conventionsState === "proposal" ? t("有新建议稿 · 点击审阅") : conventionsState === "missing" ? t("未创建 · 可让 Agent 生成") : "", () => conventions?.click());
  }
  push(t("动作"), t("聚焦输入"), "Ctrl/Cmd+K", () => promptBox.focus());
  return entries;
}

// ---------- 文件分组(UX-090) ----------
// 输入两个字符以上才出现的「文件」候选:模糊搜当前项目的文件,选中跳到文件页并打开。清单取文件页同一条 files_snapshot
// (后端增量扫描 + 缓存);每次打开面板后台取一遍,取到后就地补进正在显示的列表,不阻塞面板。
// 候选不进 paletteEntries(上万个文件会把其它候选挤出 40 条上限),渲染时单独算、固定追加在末尾。
export const PALETTE_FILE_LIMIT = 12;
export const PALETTE_FILE_MIN_QUERY = 2;
let paletteFiles = { project: null, paths: [], request: null };

function paletteFilesWanted() {
  return Boolean(currentProject) && document.body.dataset.space !== "research";
}
/// 每次打开面板都在后台刷新一遍清单(files_snapshot 在后端是增量扫描,代价很小);上一份清单(同一项目)先顶着用,
/// 新清单到了再换——新建的文件不用等缓存过期才搜得到。
function ensurePaletteFiles() {
  if (!paletteFilesWanted() || paletteFiles.request) return;
  const project = currentProject;
  paletteFiles.request = invoke("files_snapshot", { projectDir: project })
    .then((snapshot) => {
      // 取的过程中切了项目:这份清单属于旧项目,丢掉。
      if (project !== currentProject) { paletteFiles = { project: null, paths: [], request: null }; return; }
      paletteFiles = { project, paths: (snapshot?.files ?? []).map((file) => String(file.path)), request: null };
      if (paletteIsOpen()) renderPaletteList();
    })
    // 拿不到清单只是没有「文件」候选,面板其余功能不受影响;下次打开再试。
    .catch(() => { paletteFiles = { ...paletteFiles, request: null }; });
}
/// 子序列:term 的每个字符按顺序出现在 text 里。
function isSubsequence(text, term) {
  let at = 0;
  for (const ch of term) {
    at = text.indexOf(ch, at);
    if (at < 0) return false;
    at += 1;
  }
  return true;
}
/// 文件模糊匹配:空格分词、每个词都要命中。词在**文件名**里整段出现最优(开头命中更优),其次在整条路径里整段出现,
/// 最后才是字符子序列(文件名上的先于整条路径)——「palette」先出 21-palette.js,不是路径里恰好散落这些字母的文件。
/// 同分的文件名短者在前(更接近整名),再比路径长度、字典序。返回项目相对路径数组。
export function paletteFileMatches(paths, query, limit = PALETTE_FILE_LIMIT) {
  const terms = String(query ?? "").trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return [];
  const scored = [];
  for (const path of paths) {
    const lower = path.toLowerCase();
    const name = lower.slice(lower.lastIndexOf("/") + 1);
    let score = 0;
    let hit = true;
    for (const term of terms) {
      const inName = name.indexOf(term);
      if (inName >= 0) score += inName === 0 ? 0 : 1;
      else if (lower.includes(term)) score += 10;
      else if (isSubsequence(name, term)) score += 20;
      else if (isSubsequence(lower, term)) score += 30;
      else { hit = false; break; }
    }
    if (hit) scored.push({ path, score, nameLength: name.length });
  }
  scored.sort((a, b) => a.score - b.score || a.nameLength - b.nameLength || a.path.length - b.path.length || (a.path < b.path ? -1 : 1));
  return scored.slice(0, limit).map((item) => item.path);
}
function openProjectFile(path) {
  // 绝对路径:文件页若正停在某个独立任务的工作树上,openFilePreview 会先切回主项目树再打开(UX-088)。
  const base = String(currentProject ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
  navigate_view("files");
  void openFilePreview({ path: `${base}/${path}` });
}
function fileEntriesFor(query) {
  if (!paletteFilesWanted() || paletteFiles.project !== currentProject || query.length < PALETTE_FILE_MIN_QUERY) return [];
  return paletteFileMatches(paletteFiles.paths, query).map((path) => {
    const slash = path.lastIndexOf("/");
    return { group: t("文件"), label: path.slice(slash + 1), detail: slash > 0 ? path.slice(0, slash) : "", run: () => openProjectFile(path) };
  });
}

/// 子序列匹配:输入的每个字符按顺序出现即命中。比 includes 宽松(「运画」能命中
/// 「运行画像」),又不像全模糊那样什么都命中。空查询返回全部。
export function paletteMatches(entry, query) {
  if (!query) return true;
  const hay = `${entry.group} ${entry.label} ${entry.detail}`.toLowerCase();
  let at = 0;
  for (const ch of query) {
    at = hay.indexOf(ch, at);
    if (at < 0) return false;
    at += 1;
  }
  return true;
}

export function renderPaletteList() {
  const list = $("palette-list");
  if (!list) return;
  const query = ($("palette-input")?.value || "").trim().toLowerCase();
  // 文件候选固定追加在末尾、先预留名额,其它候选占满 40 条上限时它们也不会被挤掉。
  const files = fileEntriesFor(query);
  const matched = [...paletteEntries.filter((entry) => paletteMatches(entry, query)).slice(0, PALETTE_LIMIT - files.length), ...files];
  paletteIndex = Math.min(paletteIndex, Math.max(0, matched.length - 1));
  list.replaceChildren();
  if (!matched.length) {
    const empty = document.createElement("div");
    empty.className = "palette-empty";
    empty.textContent = t("没有匹配项");
    list.appendChild(empty);
    paletteMatched = [];
    paletteEntries.active = null;
    return;
  }
  matched.forEach((entry, index) => {
    const row = document.createElement("div");
    row.className = `palette-row${index === paletteIndex ? " active" : ""}`;
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", index === paletteIndex ? "true" : "false");
    const group = document.createElement("span");
    group.className = "palette-group";
    group.textContent = entry.group;
    const label = document.createElement("span");
    label.className = "palette-label";
    label.textContent = entry.label;
    const detail = document.createElement("span");
    detail.className = "palette-detail";
    detail.textContent = entry.detail;
    row.append(group, label, detail);
    // mousedown 而不是 click:click 之前会先触发输入框 blur,那条路径会关面板,
    // 于是「点一下候选项」变成「什么都没发生」。
    row.addEventListener("mousedown", (event) => {
      event.preventDefault();
      runPaletteEntry(entry);
    });
    row.addEventListener("mouseenter", () => {
      paletteIndex = index;
      syncPaletteSelection({ scroll: false });
    });
    list.appendChild(row);
  });
  paletteMatched = matched;
  syncPaletteSelection();
}

/// 只改选中态,不重建列表:重建会把滚动位置打回顶部,方向键选到第 N 行时选中项就滚出视野,
/// Enter 执行的是看不见的条目(UX-013)。键盘移动时把选中行滚进可视区(block: nearest = 只滚到刚好露出);
/// 鼠标悬停时不滚,免得指针下的行自己动起来。
function syncPaletteSelection({ scroll = true } = {}) {
  const rows = $("palette-list")?.querySelectorAll(".palette-row") ?? [];
  rows.forEach((row, index) => {
    const selected = index === paletteIndex;
    row.classList.toggle("active", selected);
    row.setAttribute("aria-selected", selected ? "true" : "false");
    if (selected && scroll) row.scrollIntoView?.({ block: "nearest" });
  });
  paletteEntries.active = paletteMatched[paletteIndex] ?? null;
}

export function runPaletteEntry(entry) {
  closePalette();
  try {
    entry?.run?.();
  } catch (error) {
    log(`${t("命令面板")}:${error}`, "warn");
  }
}

/// aria-modal="true" 只是**声明**,浏览器不会因此把焦点关在里面:旧实现不加处理时
/// Tab 两下就走到背后的 rail,回车能在半透明遮罩下真的把视图切走、甚至点到「新对话」。
/// 现在宿主是 <dialog>,经 00-surface 的 openDialog 用 showModal 打开:背景由浏览器原生
/// 惰性化(焦点与读屏都关在面板里),Esc/点外关闭走弹层栈,关闭后焦点还回打开前的位置。
export function openPalette() {
  const panel = paletteEl();
  const input = $("palette-input");
  if (!panel || !input) return;
  paletteEntries = collectPaletteEntries();
  paletteIndex = 0;
  input.value = "";
  openDialog(panel, { initialFocus: "#palette-input" });
  renderPaletteList();
  ensurePaletteFiles();
}

export function closePalette() {
  const panel = paletteEl();
  if (!panel || !paletteIsOpen()) return;
  closeSurface(panel);
}

export function movePaletteSelection(delta) {
  const rows = $("palette-list")?.querySelectorAll(".palette-row") ?? [];
  if (!rows.length) return;
  paletteIndex = (paletteIndex + delta + rows.length) % rows.length;
  syncPaletteSelection();
}

defer(() => {
  $("palette-input")?.addEventListener("input", () => {
    paletteIndex = 0;
    renderPaletteList();
  });
});
defer(() => {
  $("palette-input")?.addEventListener("keydown", (event) => {
    // 输入法选词中的 Enter/方向键归输入法,不能当成「执行选中项」(UX-123)。
    if (isImeComposing(event)) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      movePaletteSelection(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      movePaletteSelection(-1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (paletteEntries.active) runPaletteEntry(paletteEntries.active);
    }
  });
});

defer(() => {
  // Esc 归 00-surface 的弹层栈(document 捕获阶段,焦点在面板哪里都收得到),这里只管 Ctrl/Cmd+P。
  window.addEventListener("keydown", (event) => {
    const modifier = event.ctrlKey || event.metaKey;
    if (!modifier || event.altKey) return;
    if (event.key.toLowerCase() !== "p") return;
    // WebView 里 Ctrl+P 默认是打印,必须拦下。
    event.preventDefault();
    if (paletteIsOpen()) closePalette();
    // 别的模态(确认框、输入框、查看器)开着时不叠一个命令面板上去。
    else if (!isModalOpen()) openPalette();
  });
});

// R-264 B10：命令面板进入 ESM；未迁移的 classic 提供方通过 globalThis 兼容桥提供 API。
