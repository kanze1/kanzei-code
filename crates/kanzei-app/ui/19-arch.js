// ---------- R-122 架构浏览:索引 + 设计文档树可视化 ----------
// 数据来自 architecture_snapshot(只读):架构索引文本 + docs/design 文档清单。
// 文档树按「索引状态」分层——索引里出现的归入其所在章节,未入册的单独列出,
// 让「有文档没入册」(D-173 类缺口)在界面上直接可见。点击文档/索引走应用内
// Markdown 查看器(openDocViewer 既有能力),不重复造查看器。
let latestArchSnapshot = null;

async function refreshArch() {
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

function renderArch(snap) {
  const tree = $("arch-tree");
  tree.replaceChildren();
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
async function openArchDoc(name) {
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

$("arch-refresh").addEventListener("click", refreshArch);
$("arch-open-index").addEventListener("click", () => openDocViewer("architecture"));
// 批3:记忆管理入口——跳转记忆页触达既有 memory_* 维护命令(编辑/整理/重心设置)。
// 复用导航栏 memory 按钮的既有切换逻辑,不重复实现视图激活。
$("arch-goto-memory").addEventListener("click", () => {
  const memoryBtn = document.querySelector('.activity-item[data-view="memory"]');
  if (memoryBtn) memoryBtn.click();
  else document.querySelectorAll(".view").forEach((v) => v.classList.remove("active")), $("view-memory").classList.add("active");
});
