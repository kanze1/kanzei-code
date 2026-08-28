import { defer } from "./01-core.js";
import { $, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, toastError } from "./03-shell.js";
import { openDocViewer, openRuntimeMarkdown } from "./15-views-misc.js";

// ---------- R-122 架构浏览:索引 + 设计文档树可视化 ----------
// 数据来自 architecture_snapshot(只读):架构索引文本 + docs/design 文档清单。
// 文档树按「索引状态」分层——索引里出现的归入其所在章节,未入册的单独列出,
// 让「有文档没入册」(D-173 类缺口)在界面上直接可见。点击文档/索引走应用内
// Markdown 查看器(openDocViewer 既有能力),不重复造查看器。
export let latestArchSnapshot = null;

export async function refreshArch() {
  if (!currentProject) {
    $("arch-tree").textContent = t("先选择一个项目");
    return;
  }
  try {
    const snap = await invoke("architecture_snapshot", { projectDir: currentProject });
    latestArchSnapshot = snap;
    renderArch(snap);
  } catch (err) {
    $("arch-tree").textContent = "";
    toastError(`${t("架构索引读取失败")}:${err}`);
  }
}

export function renderArch(snap) {
  const tree = $("arch-tree");
  tree.replaceChildren();
  // R-188:架构图(代码生成的 SVG 依赖图)渲染在图容器;任何异常(旧环境/桩
  // 不支持 SVG API)都隐藏图并继续渲染文字树——图是增强,文字树是保底。
  try {
    renderArchGraph(snap.graph);
  } catch {
    const host = $("arch-graph");
    if (host) host.classList.add("hidden");
  }
  const summary = $("arch-summary");
  const docs = snap.design_docs ?? [];
  summary.textContent = `${docs.length}${t("篇设计文档")} · ${(snap.index ?? "").split("\n").length}${t("行索引")}`;

  // 从索引里抽出已入册的文档名(链接目标以 .md 结尾)。
  const indexed = new Set();
  for (const line of (snap.index ?? "").split("\n")) {
    const m = line.match(/\[`?([a-z0-9_]+\.md)`?\]/);
    if (m) indexed.add(m[1]);
  }
  const unindexed = docs.filter((d) => !indexed.has(d.name));

  // 已入册文档按索引出现顺序分组展示:从索引章节标题切出分组。
  const groups = [];
  let current = null;
  for (const line of (snap.index ?? "").split("\n")) {
    const heading = line.match(/^#{2,3}\s+(.+)$/);
    if (heading) {
      current = { title: heading[1].trim(), items: [] };
      groups.push(current);
      continue;
    }
    const item = line.match(/\[`([a-z0-9_]+\.md)`\]/);
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
    dim.textContent = `${name}${isUnindexed ? ` · ${t("未入册")}` : ""}`;
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

  // 索引原文:右侧固定区展示,只读。
  const body = $("arch-index-body");
  body.textContent = snap.index ?? "";
  body.scrollTop = 0;
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

// ---------- R-188 架构图:代码生成的 SVG 依赖图 ----------
// 数据来自 architecture_snapshot.graph(workspace crate 依赖边,后端从 Cargo.toml
// 抽取,纯代码生成,非文生图/预置图)。自绘 SVG(零外部依赖,桌面端离线可用):
// 每 crate 一个节点,依赖边从上向下指;节点可点击定位到对应 crate。图数据为空
// 或渲染异常时隐藏,降级为既有文字树(不空白)。
export function renderArchGraph(graph) {
  const host = $("arch-graph");
  if (!host) return;
  try {
    host.replaceChildren();
  } catch {
    /* 桩环境无 replaceChildren:清空为隐藏 */
  }
  const edges = Array.isArray(graph) ? graph : [];
  // 无 createElementNS(冒烟桩/旧环境)或图数据空:隐藏图,降级文字树,不抛错。
  if (!edges.length || typeof document.createElementNS !== "function") {
    host.classList.add("hidden");
    return;
  }
  // 收集全部节点(边两端 + 孤立成员)。
  const nodes = new Set();
  for (const [from, to] of edges) {
    nodes.add(from);
    nodes.add(to);
  }
  const names = [...nodes].sort();
  // 计算入度做分层:被依赖越多的越靠上(依赖方在下)。
  const indegree = new Map(names.map((n) => [n, 0]));
  for (const [, to] of edges) indegree.set(to, (indegree.get(to) || 0) + 1);
  const ordered = [...names].sort((a, b) => (indegree.get(b) || 0) - (indegree.get(a) || 0));
  const nodeIndex = new Map(ordered.map((n, i) => [n, i]));
  const N = ordered.length;
  const W = 320;
  const ROW_H = 56;
  const COL_W = 90;
  const PAD = 12;
  const H = Math.max(90, N * ROW_H + PAD * 2);
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
  svg.setAttribute("width", "100%");
  svg.classList.add("arch-svg");
  const pos = (name) => {
    const idx = nodeIndex.get(name) || 0;
    return { x: PAD + (idx % 3) * COL_W, y: PAD + Math.floor(idx / 3) * ROW_H };
  };
  // 依赖边(先画,节点盖在上面)。
  for (const [from, to] of edges) {
    const a = pos(from);
    const b = pos(to);
    const line = document.createElementNS(ns, "line");
    line.setAttribute("x1", String(a.x + 26));
    line.setAttribute("y1", String(a.y + 22));
    line.setAttribute("x2", String(b.x + 26));
    line.setAttribute("y2", String(b.y));
    line.setAttribute("stroke", "var(--border, #888)");
    line.setAttribute("stroke-width", "1.2");
    line.setAttribute("marker-end", "url(#arch-arrow)");
    svg.appendChild(line);
  }
  // 箭头 marker。
  const defs = document.createElementNS(ns, "defs");
  const marker = document.createElementNS(ns, "marker");
  marker.setAttribute("id", "arch-arrow");
  marker.setAttribute("viewBox", "0 0 10 10");
  marker.setAttribute("refX", "9");
  marker.setAttribute("refY", "5");
  marker.setAttribute("markerWidth", "7");
  marker.setAttribute("markerHeight", "7");
  marker.setAttribute("orient", "auto-start-reverse");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("d", "M 0 0 L 10 5 L 0 10 z");
  path.setAttribute("fill", "var(--border, #888)");
  marker.appendChild(path);
  defs.appendChild(marker);
  svg.appendChild(defs);
  // 节点。
  for (const name of ordered) {
    const { x, y } = pos(name);
    const g = document.createElementNS(ns, "g");
    g.setAttribute("class", "arch-node");
    g.setAttribute("tabindex", "0");
    g.setAttribute("role", "button");
    g.setAttribute("aria-label", name);
    const rect = document.createElementNS(ns, "rect");
    rect.setAttribute("x", String(x));
    rect.setAttribute("y", String(y));
    rect.setAttribute("width", "52");
    rect.setAttribute("height", "22");
    rect.setAttribute("rx", "4");
    rect.setAttribute("fill", "var(--bg, #eee)");
    rect.setAttribute("stroke", "var(--border, #888)");
    g.appendChild(rect);
    const text = document.createElementNS(ns, "text");
    text.setAttribute("x", String(x + 26));
    text.setAttribute("y", String(y + 14));
    text.setAttribute("text-anchor", "middle");
    text.setAttribute("font-size", "8");
    const label = name.replace(/^kanzei-/, "");
    text.textContent = label;
    g.appendChild(text);
    g.addEventListener("click", () => openArchCrate(name));
    g.addEventListener("keydown", (e) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      openArchCrate(name);
    });
    svg.appendChild(g);
  }
  host.classList.remove("hidden");
  host.appendChild(svg);
}

// R-188 验收④:图上节点点击定位——crate 无对应设计文档时打开其 Cargo.toml
// 说明(经 docs_read_custom 只读),有同名设计文档则打开文档。
export async function openArchCrate(crate) {
  const docName = `${crate}.md`;
  try {
    const file = await invoke("docs_read_custom", {
      projectDir: currentProject,
      relPath: `docs/design/${docName}`,
    });
    openRuntimeMarkdown(file.name, file.content);
    return;
  } catch {
    // 无同名设计文档:落到 crate 的 Cargo.toml(真实源码数据)。
    try {
      const cargo = await invoke("docs_read_custom", {
        projectDir: currentProject,
        relPath: `crates/${crate}/Cargo.toml`,
      });
      openRuntimeMarkdown(`${crate}/Cargo.toml`, cargo.content);
    } catch {
      toastError(`${t("打开失败")}:${crate}`);
    }
  }
}
