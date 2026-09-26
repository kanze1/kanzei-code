import { defer } from "./01-core.js";
import { $, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { layoutPref, setLayoutPref } from "./03-layout.js";
import { mountDiagram, preloadDiagramEngine, SEMANTIC_CLASSES } from "./04-diagram.js";
import { renderMarkdownInto } from "./04-markdown.js";
import { resolveRelativePath } from "./04-structured-parse.js";
import { currentProject, toastError } from "./03-shell.js";
import { openDocViewer, openRuntimeMarkdown } from "./15-views-misc.js";

// ---------- R-122 架构浏览 / UI2-0926 #7 架构图 ----------
// 上方大图卡:标签页第一张是 crate 依赖图(后端每次从 Cargo 清单生成,不落盘,可切「直接依赖 / 全部依赖」),
// 其后是 docs/architecture/*.md 的手写图(agent 用普通 write/edit 改)。图由 04-diagram.js 用 Mermaid 渲染,
// 配色只来自 --diagram-* token,节点点击走 structuredNav(源码进文件页、docs/*.md 进查看器)。
// 下方两栏:docs/design 文档树(按索引章节分层,未入册单列)| 架构索引原文(markdown)。
// 数据来自 architecture_snapshot(只读);设计见 docs/design/architecture_diagrams.md。
export let latestArchSnapshot = null;
const CRATE_TAB = "crates";
let diagramView = null;

export async function refreshArch() {
  if (!currentProject) {
    $("arch-tree").textContent = t("先选择一个项目");
    return;
  }
  const project = currentProject;
  preloadDiagramEngine();
  try {
    const snap = await invoke("architecture_snapshot", { projectDir: project });
    if (project !== currentProject) return;
    latestArchSnapshot = snap;
    renderArch(snap);
  } catch (err) {
    if (project !== currentProject) return;
    $("arch-tree").textContent = "";
    toastError(`${t("架构索引读取失败")}:${err}`);
  }
}

/// 索引文本 → 行(CRLF 先归一:磁盘上的 README 是 CRLF,按 "\n" 切会在每行尾留 \r,章节标题正则全落空)。
export function archIndexLines(index) {
  return String(index ?? "").replace(/\r\n?/g, "\n").split("\n");
}
/// 链接里的设计文档文件名:snake_case 与 kebab-case 都认——kebab 名是「命名不合规」,不是「未入册」。
const DOC_LINK = /\[`?([a-z0-9][a-z0-9_-]*\.md)`?\]/;
const isKebab = (name) => name.includes("-");

export function renderArch(snap) {
  renderArchDiagrams(snap);
  renderArchTree(snap);
  // 索引:右侧按 markdown 渲染(标题/列表/表格/路径链接可点),只读。
  const body = $("arch-index-body");
  renderMarkdownInto(body, snap.index ?? "");
  // 索引里的链接相对 README 所在目录(`../../../docs/design/x.md`):解析成项目相对路径,
  // 点击才能落到真实文件(a.md-path 的点击委托在 19-research.js)。
  for (const link of body.querySelectorAll?.("a.md-path") ?? []) {
    link.dataset.path = resolveRelativePath(".kanzei/project/architecture", link.dataset.path);
  }
  body.scrollTop = 0;
}

function renderArchTree(snap) {
  const tree = $("arch-tree");
  tree.replaceChildren();
  const docs = snap.design_docs ?? [];
  const lines = archIndexLines(snap.index);
  $("arch-summary").textContent = `${docs.length}${t("篇设计文档")} · ${lines.length}${t("行索引")}`;

  // 从索引里抽出已入册的文档名。
  const indexed = new Set();
  for (const line of lines) {
    const m = line.match(DOC_LINK);
    if (m) indexed.add(m[1]);
  }
  const unindexed = docs.filter((d) => !indexed.has(d.name));

  // 已入册文档按索引出现顺序分组展示:从索引章节标题切出分组。
  const groups = [];
  let current = null;
  for (const line of lines) {
    const heading = line.match(/^#{2,3}\s+(.+)$/);
    if (heading) {
      current = { title: heading[1].trim(), items: [] };
      groups.push(current);
      continue;
    }
    const item = line.match(/\[`([a-z0-9][a-z0-9_-]*\.md)`\]/);
    if (item && current) current.items.push(item[1]);
  }

  const renderEntry = (name, { unindexed: isUnindexed = false } = {}) => {
    const row = document.createElement("div");
    row.className = "arch-entry";
    row.setAttribute("role", "treeitem");
    row.tabIndex = 0;
    const meta = docs.find((d) => d.name === name);
    const label = document.createElement("span");
    label.className = "arch-entry-name";
    label.textContent = meta?.title || name;
    const dim = document.createElement("span");
    dim.className = "dim arch-entry-dim";
    const flags = [];
    if (isUnindexed) flags.push(t("未入册"));
    if (isKebab(name)) flags.push(t("命名不合规"));
    dim.textContent = `${name}${flags.length ? ` · ${flags.join(" · ")}` : ""}`;
    if (isKebab(name)) dim.title = t("设计文档文件名应为 snake_case(architecture 工具会报 not snake_case)");
    row.append(label, dim);
    row.addEventListener("click", () => openArchDoc(name));
    row.addEventListener("keydown", (e) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      openArchDoc(name);
    });
    return row;
  };

  for (const g of groups) {
    const items = g.items.filter((n) => docs.some((d) => d.name === n));
    if (!items.length) continue;
    const head = document.createElement("h3");
    head.className = "arch-group-head";
    head.textContent = `${g.title}(${items.length})`;
    tree.appendChild(head);
    for (const name of items) tree.appendChild(renderEntry(name));
  }
  if (unindexed.length) {
    const head = document.createElement("h3");
    head.className = "arch-group-head unindexed";
    head.textContent = `${t("未入册")}(${unindexed.length})`;
    tree.appendChild(head);
    for (const d of unindexed) tree.appendChild(renderEntry(d.name, { unindexed: true }));
  }
  if (!docs.length) {
    const empty = document.createElement("p");
    empty.className = "dim";
    empty.textContent = t("暂无设计文档");
    tree.appendChild(empty);
  }
}

// ---------- 图 ----------
/// 标签页清单:crate 图(有工作区时)在前,其后是 docs/architecture 的手写图。
export function archDiagramTabs(snap) {
  const tabs = [];
  const crates = snap?.crates;
  if (crates?.mermaid?.reduced) {
    tabs.push({ key: CRATE_TAB, title: t("Crate 依赖"), path: null, sourceLine: 1, issues: [], crates });
  }
  for (const doc of snap?.diagrams ?? []) {
    tabs.push({
      key: doc.id,
      title: doc.title || doc.id,
      path: doc.path,
      sourceLine: doc.source_line || 1,
      source: doc.source ?? "",
      summary: doc.summary ?? "",
      issues: doc.issues ?? [],
    });
  }
  return tabs;
}
const depsFull = () => layoutPref("arch", "deps_full") === true;
function tabSource(tab) {
  if (tab.key !== CRATE_TAB) return tab.source;
  return depsFull() ? tab.crates.mermaid.full : tab.crates.mermaid.reduced;
}
function legendKinds(tab, source) {
  const kinds = SEMANTIC_CLASSES.filter((name) => new RegExp(`:::${name}\\b|^\\s*class\\s+\\S+\\s+${name}\\b`, "m").test(source));
  if (tab.key === CRATE_TAB && depsFull()) kinds.push("transitive");
  return kinds;
}
const legendLabel = (kind) => ({
  entry: t("入口"),
  ext: t("外部"),
  store: t("存储"),
  focus: t("本图主角"),
  muted: t("次要"),
  transitive: t("可由传递得到的依赖"),
})[kind] ?? kind;

export function renderArchDiagrams(snap) {
  const tabsHost = $("arch-diagram-tabs");
  const canvas = $("arch-diagram-canvas");
  if (!tabsHost || !canvas) return;
  const tabs = archDiagramTabs(snap);
  tabsHost.replaceChildren();
  $("arch-diagram-tools")?.replaceChildren();
  $("arch-diagram-foot")?.replaceChildren();
  const issuesHost = $("arch-diagram-issues");
  issuesHost?.replaceChildren();
  issuesHost?.classList.add("hidden");
  if (!tabs.length) {
    diagramView = null;
    const empty = document.createElement("p");
    empty.className = "arch-diagram-empty";
    empty.textContent = t("暂无架构图:在 docs/architecture/ 下新建「两位数字_名字.md」(# 标题 + 一段说明 + ```mermaid 围栏),改完调 architecture 工具的 diagrams 动作自查。");
    canvas.replaceChildren(empty);
    return;
  }
  const saved = layoutPref("arch", "diagram");
  const selected = tabs.find((tab) => tab.key === saved) ?? tabs[0];
  const buttons = [];
  for (const tab of tabs) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "arch-diagram-tab";
    btn.id = `arch-tab-${tab.key}`;
    btn.dataset.tab = tab.key;
    btn.setAttribute("role", "tab");
    btn.setAttribute("aria-controls", "arch-diagram-canvas");
    btn.setAttribute("aria-selected", tab === selected ? "true" : "false");
    btn.tabIndex = tab === selected ? 0 : -1;
    const label = document.createElement("span");
    label.textContent = tab.title;
    btn.append(label);
    if (tab.issues.length) {
      const count = document.createElement("span");
      count.className = "arch-tab-count";
      count.textContent = String(tab.issues.length);
      count.title = `${tab.issues.length} ${t("条检查提示")}`;
      btn.append(count);
    }
    if (tab.summary) btn.title = tab.summary;
    btn.addEventListener("click", () => selectArchDiagram(snap, tab.key));
    btn.addEventListener("keydown", (event) => {
      const index = buttons.indexOf(btn);
      let next = null;
      if (event.key === "ArrowRight") next = buttons[(index + 1) % buttons.length];
      else if (event.key === "ArrowLeft") next = buttons[(index - 1 + buttons.length) % buttons.length];
      else if (event.key === "Home") next = buttons[0];
      else if (event.key === "End") next = buttons.at(-1);
      if (!next) return;
      event.preventDefault();
      selectArchDiagram(snap, next.dataset.tab);
      $(`arch-tab-${next.dataset.tab}`)?.focus?.();
    });
    buttons.push(btn);
    tabsHost.appendChild(btn);
  }
  showArchDiagram(selected);
}

export function selectArchDiagram(snap, key) {
  setLayoutPref("arch", "diagram", key);
  renderArchDiagrams(snap);
}

function showArchDiagram(tab) {
  const canvas = $("arch-diagram-canvas");
  canvas.setAttribute("aria-labelledby", `arch-tab-${tab.key}`);
  const tools = $("arch-diagram-tools");
  tools.replaceChildren();
  if (tab.key === CRATE_TAB) tools.append(depsToggle(tab));
  const source = tabSource(tab);
  renderArchFoot(tab, source, null);
  renderArchIssues(tab);
  diagramView = mountDiagram(canvas, source, {
    mode: "page",
    title: tab.title,
    path: tab.path,
    sourceLine: tab.sourceLine,
    toolbarHost: tools,
    onRendered: ({ view }) => renderArchFoot(tab, view.source, view.graph),
  });
}

/// 「直接依赖 / 全部依赖」:同一张 crate 图的两份源码(后端一次给齐),切换只换源码不重取快照。
function depsToggle(tab) {
  const seg = document.createElement("div");
  seg.className = "arch-seg";
  seg.setAttribute("role", "group");
  seg.setAttribute("aria-label", t("依赖范围"));
  for (const [full, label, title] of [
    [false, t("直接依赖"), t("只画直接依赖,可由传递得到的依赖隐藏")],
    [true, t("全部依赖"), t("全部依赖,可由传递得到的画成虚线")],
  ]) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.dataset.full = String(full);
    btn.textContent = label;
    btn.title = title;
    btn.setAttribute("aria-pressed", String(full === depsFull()));
    btn.addEventListener("click", () => {
      if (depsFull() === full) return;
      setLayoutPref("arch", "deps_full", full ? true : null);
      for (const other of seg.children) other.setAttribute("aria-pressed", String(other === btn));
      const source = tabSource(tab);
      renderArchFoot(tab, source, null);
      diagramView?.setSource(source);
    });
    seg.append(btn);
  }
  return seg;
}

function renderArchFoot(tab, source, graph) {
  const foot = $("arch-diagram-foot");
  if (!foot) return;
  foot.replaceChildren();
  const span = (className, text) => {
    const node = document.createElement("span");
    if (className) node.className = className;
    node.textContent = text;
    return node;
  };
  if (tab.key === CRATE_TAB) {
    foot.append(span("arch-foot-path", t("由 Cargo 清单生成")));
    const hidden = Number(tab.crates.hidden_transitive ?? 0);
    if (hidden && !depsFull()) foot.append(span("", `${t("已隐藏")} ${hidden} ${t("条可由传递得到的依赖")}`));
  } else {
    foot.append(span("arch-foot-path", tab.path));
  }
  if (graph) foot.append(span("", `${graph.nodes.size} ${t("个节点")} · ${graph.edges.length} ${t("条边")}`));
  foot.append(span("", t("点击节点打开实现或文档")));
  const kinds = legendKinds(tab, source ?? "");
  if (kinds.length) {
    const legend = document.createElement("span");
    legend.className = "arch-legend";
    legend.setAttribute("aria-label", t("图例"));
    for (const kind of kinds) {
      const item = document.createElement("span");
      item.className = "arch-legend-item";
      const swatch = document.createElement("span");
      swatch.className = "arch-legend-swatch";
      swatch.dataset.kind = kind;
      item.append(swatch, span("", legendLabel(kind)));
      legend.append(item);
    }
    foot.append(legend);
  }
}

function renderArchIssues(tab) {
  const host = $("arch-diagram-issues");
  if (!host) return;
  host.replaceChildren();
  host.classList.toggle("hidden", !tab.issues.length);
  for (const issue of tab.issues) {
    const row = document.createElement("div");
    row.className = "arch-issue";
    row.dataset.severity = issue.severity;
    const code = document.createElement("span");
    code.className = "arch-issue-code";
    code.textContent = `${issue.code} · ${t("第")} ${issue.line} ${t("行")}`;
    const text = document.createElement("span");
    text.textContent = issue.message;
    const hint = document.createElement("span");
    hint.className = "arch-issue-hint";
    hint.textContent = `→ ${issue.hint}`;
    row.append(code, text, hint);
    host.append(row);
  }
}

// 打开设计文档/索引:docs_read_custom 读取 docs/ 下任意 md(只读),
// 架构索引走既有 docs_read("architecture")。
export async function openArchDoc(name) {
  try {
    if (name === "README.md") {
      openDocViewer("architecture");
      return;
    }
    const file = await invoke("docs_read_custom", {
      projectDir: currentProject,
      relPath: `docs/design/${name}`,
    });
    openRuntimeMarkdown(file.name, file.content);
  } catch (err) {
    toastError(`${t("打开失败")}:${err}`);
  }
}

defer(() => {
  $("arch-refresh").addEventListener("click", refreshArch);
});
defer(() => {
  $("arch-open-index").addEventListener("click", () => openDocViewer("architecture"));
});
// 批3:记忆管理入口——跳转记忆页触达既有 memory_* 维护命令(编辑/整理/重心设置)。
// 复用导航栏 memory 按钮的既有切换逻辑,不重复实现视图激活。
defer(() => {
  $("arch-goto-memory").addEventListener("click", () => {
    const memoryBtn = document.querySelector('.activity-item[data-view="memory"]');
    if (memoryBtn) memoryBtn.click();
    else document.querySelectorAll(".view").forEach((v) => v.classList.remove("active")), $("view-memory").classList.add("active");
  });
});
