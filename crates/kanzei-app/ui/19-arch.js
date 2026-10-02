import { defer } from "./01-core.js";
import { $, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { layoutPref, setLayoutPref } from "./03-layout.js";
import { mountDiagram, preloadDiagramEngine, SEMANTIC_CLASSES } from "./04-diagram.js";
import { fillTemplate } from "./04-structured-parse.js";
import { currentProject, toastError } from "./03-shell.js";
import { openDocViewer, openRuntimeMarkdown } from "./15-views-misc.js";

// ---------- R-122 架构 / UI2-0926 #7 架构图 ----------
// 上方大图卡:标签页第一张是 crate 依赖图(后端每次从 Cargo 清单生成,不落盘,可切「直接依赖 / 全部依赖」),
// 其后是 docs/architecture/*.md 的手写图(agent 用普通 write/edit 改)。图由 04-diagram.js 用 Mermaid 渲染,
// 配色只来自 --diagram-* token,节点点击走 structuredNav(源码进文件页、docs/*.md 进查看器)。
// 下方是 docs/design 文档树(按索引章节分层,未入册单列;可搜索、可折叠);索引原文不在页内重复,
// 「打开架构索引」进查看器看。索引 README 是可选的:没有它,图与文档树照常显示。
// 数据来自 architecture_snapshot(只读);设计见 docs/design/architecture_diagrams.md。
export let latestArchSnapshot = null;
const CRATE_TAB = "crates";
let diagramView = null;
let archRenderedProject = null;
let archGeneration = 0;
let archFilter = "";

/// 清空页面上所有来自上一次快照的内容:换项目、读取出错时调用,免得旧项目的图顶着新项目的名字。
function clearArch() {
  diagramView = null;
  latestArchSnapshot = null;
  for (const id of ["arch-diagram-tabs", "arch-diagram-tools", "arch-diagram-foot", "arch-diagram-issues", "arch-diagram-canvas", "arch-tree"]) {
    $(id)?.replaceChildren();
  }
  $("arch-diagram-issues")?.classList.add("hidden");
  $("arch-open-index")?.classList.add("hidden");
  $("arch-layout")?.classList.add("hidden");
  const summary = $("arch-summary");
  if (summary) summary.textContent = "";
  archFilter = "";
  const filter = $("arch-filter");
  if (filter) filter.value = "";
}

/// 在图区写一句状态(加载中 / 出错);出错时带「重试」。
function showArchNote(text, { retry = false } = {}) {
  const canvas = $("arch-diagram-canvas");
  if (!canvas) return;
  const note = document.createElement("p");
  note.className = "arch-diagram-empty";
  note.textContent = text;
  if (retry) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ghost mini";
    button.textContent = t("重试");
    button.addEventListener("click", () => void refreshArch());
    note.append(" ", button);
  }
  canvas.replaceChildren(note);
}

export async function refreshArch() {
  if (!currentProject) {
    clearArch();
    archRenderedProject = null;
    showArchNote(t("先选择一个项目"));
    return;
  }
  const project = currentProject;
  const generation = ++archGeneration;
  preloadDiagramEngine();
  if (project !== archRenderedProject) {
    clearArch();
    archRenderedProject = project;
  }
  // 加载指示:旧内容在就让它先留着,只让 ⟳ 转圈;页面是空的才写一句「正在读取」。
  const refresh = $("arch-refresh");
  refresh?.setAttribute("aria-busy", "true");
  if (!latestArchSnapshot) showArchNote(t("正在读取架构…"));
  try {
    const snap = await invoke("architecture_snapshot", { projectDir: project });
    if (project !== currentProject || generation !== archGeneration) return;
    latestArchSnapshot = snap;
    renderArch(snap);
  } catch (err) {
    if (project !== currentProject || generation !== archGeneration) return;
    // 出错时旧内容必须清掉:上一次的图与文档树看起来就像「当前项目的架构」。
    clearArch();
    showArchNote(`${t("架构信息读取失败")}:${err}`, { retry: true });
  } finally {
    if (generation === archGeneration) refresh?.removeAttribute("aria-busy");
  }
}

/// 索引文本 → 行(CRLF 先归一:磁盘上的 README 是 CRLF,按 "\n" 切会在每行尾留 \r,章节标题正则全落空)。
export function archIndexLines(index) {
  return String(index ?? "").replace(/\r\n?/g, "\n").split("\n");
}
/// 链接里的设计文档文件名:snake_case 与 kebab-case 都认——kebab 名是「命名不合规」,不是「未入册」。
const DOC_LINK = /\[`?([a-z0-9][a-z0-9_-]*\.md)`?\]/;
const isKebab = (name) => name.includes("-");
/// 索引章节标题是文档身份的内部枚举(见索引 README 的字段约定),界面上写成人话。
const archGroupTitle = (heading) => ({
  live_design: t("现行设计"),
  validated_design: t("已交付设计"),
  historical_snapshot: t("历史快照"),
  superseded: t("已被取代"),
})[heading] ?? heading;
/// 默认折叠的章节:历史快照与已被取代的文档不是日常要看的,用户折叠/展开过就以用户为准。
const ARCH_DEFAULT_COLLAPSED = new Set(["historical_snapshot", "superseded"]);

export function renderArch(snap) {
  renderArchDiagrams(snap);
  renderArchTree(snap);
  $("arch-layout")?.classList.remove("hidden");
  // 索引原文只在查看器里看(树就是从它生成的,不在页内再摆一份);没有索引就不给入口。
  const hasIndex = snap.index_exists ?? Boolean(String(snap.index ?? "").trim());
  $("arch-open-index")?.classList.toggle("hidden", !hasIndex);
}

/// 从索引里抽出每篇文档的章节与一句话说明:{ groups: [{ heading, items: [name] }], indexed: Set, notes: Map }。
function parseArchIndex(lines) {
  const groups = [];
  const indexed = new Set();
  const notes = new Map();
  let current = null;
  for (const line of lines) {
    const heading = line.match(/^#{2,3}\s+(.+)$/);
    if (heading) {
      current = { heading: heading[1].trim(), items: [] };
      groups.push(current);
      continue;
    }
    const link = line.match(DOC_LINK);
    if (!link) continue;
    indexed.add(link[1]);
    // 链接后面的一句说明(前面的 [identity: …; last_verified_commit: …] 是治理元数据,不进界面)。
    const note = line.slice(line.indexOf(link[0]) + link[0].length).replace(/^\([^)]*\)\s*[:：]?\s*/, "").trim();
    if (note) notes.set(link[1], note);
    const item = line.match(/\[`([a-z0-9][a-z0-9_-]*\.md)`\]/);
    if (item && current) current.items.push(item[1]);
  }
  return { groups, indexed, notes };
}

function renderArchTree(snap) {
  const tree = $("arch-tree");
  tree.replaceChildren();
  const docs = snap.design_docs ?? [];
  const lines = archIndexLines(snap.index);
  const hasIndex = snap.index_exists ?? lines.some((line) => line.trim());
  const { groups, indexed, notes } = parseArchIndex(lines);
  const unindexed = hasIndex ? docs.filter((d) => !indexed.has(d.name)) : [];
  const query = archFilter.trim().toLowerCase();
  const matches = (name) => {
    if (!query) return true;
    const meta = docs.find((d) => d.name === name);
    return `${name} ${meta?.title ?? ""} ${notes.get(name) ?? ""}`.toLowerCase().includes(query);
  };
  let shown = 0;

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
    if (notes.has(name)) row.title = notes.get(name);
    row.append(label, dim);
    row.addEventListener("click", () => openArchDoc(name));
    row.addEventListener("keydown", (e) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      openArchDoc(name);
    });
    return row;
  };

  // 一个可折叠的章节:标题是按钮(aria-expanded),条目一直渲染在 DOM 里、折叠只是藏起来;
  // 搜索时命中的章节强制展开,折叠状态存 app.json(ui_layout.arch.collapsed),不动用户的偏好。
  const appendGroup = (key, title, names, { warn = false, defaultCollapsed = false } = {}) => {
    const visible = names.filter(matches);
    if (!visible.length) return;
    shown += visible.length;
    const saved = layoutPref("arch", "collapsed");
    const collapsed = query ? false : (saved && key in saved ? Boolean(saved[key]) : defaultCollapsed);
    const head = document.createElement("h3");
    head.className = `arch-group-head${warn ? " unindexed" : ""}`;
    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "arch-group-toggle";
    toggle.textContent = `${title}(${visible.length})`;
    toggle.setAttribute("aria-expanded", String(!collapsed));
    head.appendChild(toggle);
    const body = document.createElement("div");
    body.className = `arch-group-body${collapsed ? " hidden" : ""}`;
    body.setAttribute("role", "group");
    for (const name of visible) body.appendChild(renderEntry(name, { unindexed: warn }));
    toggle.addEventListener("click", () => {
      if (query) return;
      const next = body.classList.toggle("hidden");
      toggle.setAttribute("aria-expanded", String(!next));
      setLayoutPref("arch", "collapsed", { ...(layoutPref("arch", "collapsed") ?? {}), [key]: next });
    });
    tree.append(head, body);
  };

  if (!hasIndex) {
    // 没有索引 README:文档不分章节,也不能说它们「未入册」——给一句原因,再平铺全部文档。
    const note = document.createElement("p");
    note.className = "dim arch-tree-note";
    note.textContent = snap.index_error || t("还没有架构索引(.kanzei/project/architecture/README.md),设计文档暂不分章节;建好索引后这里会按章节分组。");
    tree.appendChild(note);
    appendGroup("all", t("设计文档"), docs.map((d) => d.name));
  } else {
    for (const g of groups) {
      const items = g.items.filter((n) => docs.some((d) => d.name === n));
      appendGroup(g.heading, archGroupTitle(g.heading), items, { defaultCollapsed: ARCH_DEFAULT_COLLAPSED.has(g.heading) });
    }
    appendGroup("unindexed", t("未入册"), unindexed.map((d) => d.name), { warn: true });
  }
  $("arch-summary").textContent = query
    ? fillTemplate(t("{n}篇设计文档 · 匹配 {m} 篇"), { n: docs.length, m: shown })
    : `${docs.length}${t("篇设计文档")}`;
  if (!docs.length) {
    const empty = document.createElement("p");
    empty.className = "dim";
    empty.textContent = t("暂无设计文档");
    tree.appendChild(empty);
  } else if (query && !shown) {
    const empty = document.createElement("p");
    empty.className = "dim arch-tree-note";
    empty.textContent = t("没有匹配的设计文档");
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
/// 「全部依赖」的版式:后端给的是不分组的源码(分组框会把跨组的传递边并成一圈虚线框),这里再关掉 ELK 的同向边合并,
/// 每条传递边单独走线;样式按 data-variant 把传递边画淡,悬停节点时它的传递边才亮起来。
function tabLayout(tab) {
  const full = tab.key === CRATE_TAB && depsFull();
  return { layout: full ? { mergeEdges: false } : null, variant: full ? "deps-full" : null };
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
    ...tabLayout(tab),
    // 画失败时不带图的节点/边数与「点击节点」提示(没有可点的节点)。
    onRendered: ({ view, error }) => renderArchFoot(tab, view.source, error ? null : view.graph),
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
      diagramView?.setSource(source, tabLayout(tab));
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
    // 来源说明是一句话,不是路径:不套等宽的 arch-foot-path。
    foot.append(span("arch-foot-origin", t("由 Cargo 清单生成")));
    const hidden = Number(tab.crates.hidden_transitive ?? 0);
    if (hidden && !depsFull()) foot.append(span("", fillTemplate(t("已隐藏 {n} 条可由传递得到的依赖"), { n: hidden })));
    if (depsFull()) foot.append(span("", t("悬停节点看它的全部依赖")));
  } else {
    foot.append(span("arch-foot-path", tab.path));
  }
  if (graph) foot.append(span("", `${graph.nodes.size} ${t("个节点")} · ${graph.edges.length} ${t("条边")}`));
  // 只在图画出来且确有可点节点时提示(加载中、出错、没有 click 行的图都不提示)。
  if (graph?.mapped?.length) foot.append(span("", t("点击节点打开实现或文档")));
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
    code.textContent = `${issue.code} · ${fillTemplate(t("第 {line} 行"), { line: issue.line })}`;
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
// 搜索设计文档:按标题/文件名/索引里的一句话说明过滤,章节自动展开(树从最近一次快照重画,不再取数)。
defer(() => {
  $("arch-filter")?.addEventListener("input", (event) => {
    archFilter = event.target.value;
    if (latestArchSnapshot) renderArchTree(latestArchSnapshot);
  });
});
