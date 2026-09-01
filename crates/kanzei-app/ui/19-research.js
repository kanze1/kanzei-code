import { defer } from "./01-core.js";
import { renderMarkdown } from "./04-markdown.js";
import { $, invoke } from "./01-core.js";
import { localizedDocStatus, t } from "./02-i18n.js";
import { currentProject, log, toast, toastError } from "./03-shell.js";
import { researchLinkField, researchOpenLink } from "./11-docs-list.js";
import { openFilePreview } from "./17-files.js";

// 研究工作台(R-276 批3)。
//
// 为什么独立成一个主视图,而不是继续在侧栏那两条列表上打补丁:侧栏一列一百多像素,
// 论文标题要换三行、按钮往哪放都别扭,还得和 req/defect 共用一套渲染分支——前两轮
// 补丁(D-413/D-414)每轮都出新问题,那本身就是「地方不对」的信号。研究工件是研究
// 模式的核心资产,值得一块自己的地方:左边卡片流(来源/发现),右边报告正文。
//
// 设计依据 docs/design/research_workspace.md:结果>过程(报告是主角)、溯源冗余
// (卡片里能开、报告里能跳)、数据已结构化的不许降级成字符串。

export let researchTab = "sources";
export let selectedResearchTopic = "";
export let researchSnapshot = { sources: [], findings: [], research_topics: [] };

export let researchFilters = { query: "", type: "", level: "", year: "", sort: "" };

export function researchEntryType(entry) {
  return researchField(entry, "类型", "type", "域", "domain");
}

export function researchEntryYear(entry) {
  return researchField(entry, "年份", "year");
}

export function researchCitationCount(entry, topic = selectedResearchTopicData()) {
  const id = entry?.id;
  if (!id) return 0;
  return (topic.findings ?? []).filter((finding) =>
    researchField(finding, "refs").split(/[\\s,]+/).includes(id),
  ).length;
}

export function filteredResearchEntries() {
  const topic = selectedResearchTopicData();
  const query = researchFilters.query.trim().toLowerCase();
  const entries = selectedResearchEntries().filter((entry) => {
    const type = researchEntryType(entry);
    const level = researchField(entry, "等级", "level");
    const year = researchEntryYear(entry);
    const haystack = [entry.title, ...(entry.fields ?? []).flat()].join(" ").toLowerCase();
    return (!query || haystack.includes(query))
      && (!researchFilters.type || type === researchFilters.type)
      && (!researchFilters.level || level === researchFilters.level)
      && (!researchFilters.year || year === researchFilters.year);
  });
  const topicEntries = topic;
  if (researchFilters.sort === "year") {
    entries.sort((a, b) => researchEntryYear(b).localeCompare(researchEntryYear(a), undefined, { numeric: true }));
  } else if (researchFilters.sort === "cited") {
    entries.sort((a, b) => researchCitationCount(b, topicEntries) - researchCitationCount(a, topicEntries));
  }
  return entries;
}

export function setResearchSelectOptions(select, values, emptyLabel) {
  if (!select) return;
  const current = select.value;
  select.innerHTML = "";
  const all = document.createElement("option");
  all.value = "";
  all.textContent = emptyLabel;
  select.appendChild(all);
  for (const value of values) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = value;
    select.appendChild(option);
  }
  select.value = values.includes(current) ? current : "";
}

export function renderResearchFilters() {
  const topic = selectedResearchTopicData();
  const entries = selectedResearchEntries();
  const types = [...new Set(entries.map(researchEntryType).filter(Boolean))].sort((a, b) => a.localeCompare(b));
  const levels = [...new Set(entries.map((entry) => researchField(entry, "等级", "level")).filter(Boolean))]
    .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
  const years = [...new Set(entries.map(researchEntryYear).filter(Boolean))]
    .sort((a, b) => b.localeCompare(a, undefined, { numeric: true }));
  setResearchSelectOptions($("research-filter-type"), types, t("全部类型"));
  setResearchSelectOptions($("research-filter-level"), levels, t("全部等级"));
  setResearchSelectOptions($("research-filter-year"), years, t("全部年份"));
  const query = $("research-filter-query");
  if (query && query.value !== researchFilters.query) query.value = researchFilters.query;
  const type = $("research-filter-type");
  const level = $("research-filter-level");
  const year = $("research-filter-year");
  const sort = $("research-filter-sort");
  if (type) type.value = researchFilters.type;
  if (level) level.value = researchFilters.level;
  if (year) year.value = researchFilters.year;
  if (sort) sort.value = researchFilters.sort;
  const visible = filteredResearchEntries().length;
  const count = $("research-filter-count");
  if (count) count.textContent = `${visible}/${entries.length} ${t("条")}`;
  const cited = topic.sources?.reduce((total, source) => total + researchCitationCount(source, topic), 0) ?? 0;
  const citedCount = $("research-citation-count");
  if (citedCount) citedCount.textContent = `${cited} ${t("处反查")}`;
}

export function researchBibtex(entry) {
  const author = researchField(entry, "作者", "author") || "Unknown";
  const year = researchEntryYear(entry) || "n.d.";
  const title = entry.title || researchField(entry, "标题", "title") || entry.id;
  const url = researchField(entry, "URL", "url", "DOI", "doi");
  const anchor = researchField(entry, "出处", "证据锚", "evidence", "anchor");
  const keyAuthor = author.split(/[\\s,]+/).filter(Boolean)[0] || "source";
  const key = `${keyAuthor.toLowerCase().replace(/[^a-z0-9_-]/g, "") || "source"}_${year}_${entry.id}`;
  const location = url ? `  url = {${url}},` : anchor ? `  note = {${anchor}},` : "";
  return `@misc{${key},\\n  author = {${author}},\\n  title = {${title}},\\n  year = {${year}},\\n${location}\\n}`;
}

export async function copyResearchCitation(entry) {
  try {
    await navigator.clipboard.writeText(researchBibtex(entry));
    toast(`${entry.id} ${t("BibTeX 已复制")}`);
  } catch (error) {
    toastError(`${t("复制 BibTeX 失败")}:${error}`);
  }
}

export function researchTopicKey(topic) {
  return topic?.topic ?? "";
}

export function researchTopicLabel(topic) {
  return topic?.label || topic?.topic || t("旧版平铺");
}

export function selectedResearchTopicData() {
  const topics = researchSnapshot.research_topics ?? [];
  return topics.find((topic) => researchTopicKey(topic) === selectedResearchTopic) ?? topics[0] ?? {
    topic: null,
    legacy: true,
    label: t("旧版平铺"),
    sources: researchSnapshot.sources ?? [],
    findings: researchSnapshot.findings ?? [],
  };
}

export function renderResearchTopicPicker() {
  const select = $("research-topic-select");
  if (!select) return;
  const topics = researchSnapshot.research_topics ?? [];
  if (!topics.length) {
    select.innerHTML = `<option value="">${t("暂无研究课题")}</option>`;
    select.disabled = true;
    selectedResearchTopic = "";
    return;
  }
  const available = new Set(topics.map(researchTopicKey));
  if (!available.has(selectedResearchTopic)) selectedResearchTopic = researchTopicKey(topics[0]);
  select.disabled = false;
  select.innerHTML = "";
  for (const topic of topics) {
    const option = document.createElement("option");
    option.value = researchTopicKey(topic);
    option.textContent = researchTopicLabel(topic);
    select.appendChild(option);
  }
  select.value = selectedResearchTopic;
}

export function selectedResearchEntries() {
  const topic = selectedResearchTopicData();
  return researchTab === "findings" ? (topic.findings ?? []) : (topic.sources ?? []);
}

export function selectedResearchTopicArg() {
  const topic = selectedResearchTopicData();
  return topic.legacy || !topic.topic ? {} : { topic: topic.topic };
}

export let researchPlan = null;

export function renderResearchPlan(plan) {
  const panel = $("research-plan-panel");
  const status = $("research-plan-status");
  const tree = $("research-plan-tree");
  const approve = $("research-plan-approve");
  if (!panel || !status || !tree || !approve) return;
  panel.hidden = !plan;
  tree.innerHTML = "";
  if (!plan) return;
  status.textContent = plan.status || "draft";
  approve.hidden = plan.status !== "awaiting_approval";
  const appendNode = (node, parent) => {
    const item = document.createElement("li");
    item.className = `research-plan-node plan-${node.status || "pending"}`;
    item.textContent = `${node.id}: ${node.title} · ${node.status || "pending"}`;
    if (node.objective) item.title = node.objective;
    parent.appendChild(item);
    if ((node.children ?? []).length) {
      const children = document.createElement("ol");
      for (const child of node.children) appendNode(child, children);
      item.appendChild(children);
    }
  };
  for (const node of plan.nodes ?? []) appendNode(node, tree);
}

export async function refreshResearchPlan() {
  const topic = selectedResearchTopicData();
  if (topic.legacy || !topic.topic) {
    researchPlan = null;
    renderResearchPlan(null);
    return;
  }
  try {
    const snapshot = await invoke("research_plan_get", { projectDir: currentProject, topic: topic.topic });
    researchPlan = snapshot.exists ? snapshot.plan : null;
    renderResearchPlan(researchPlan);
  } catch (error) {
    researchPlan = null;
    renderResearchPlan(null);
    log(`${t("研究计划刷新失败")}:${error}`, "warn");
  }
}


/// 取字段值(大小写与中英别名都认;取不到给空串)。
export function researchField(entry, ...names) {
  const wanted = names.map((n) => n.toLowerCase());
  const hit = (entry.fields ?? []).find(([k]) => wanted.includes(String(k).toLowerCase()));
  return hit ? String(hit[1]) : "";
}

/// 一张来源/发现卡片。与侧栏的一行不同,这里给全:完整标题不截断、要点摘要、
/// 可打开入口、可编辑与归档——研究工件与 req/defect 同权(D-413 的初衷)。
export function researchCard(entry, kind) {
  const card = document.createElement("article");
  card.className = "research-card";
  card.dataset.docId = entry.id;

  const head = document.createElement("div");
  head.className = "research-card-head";
  const id = document.createElement("span");
  id.className = "research-card-id";
  id.textContent = entry.id;
  head.appendChild(id);

  const type = researchField(entry, "类型", "type") || researchField(entry, "域", "domain");
  if (type) {
    const badge = document.createElement("span");
    badge.className = "research-badge";
    badge.textContent = type;
    head.appendChild(badge);
  }
  const level = researchField(entry, "等级", "level");
  if (level) {
    const badge = document.createElement("span");
    // V 等级是研究报告的可信度分层,给它固定色阶而不是混进普通徽章。
    badge.className = `research-badge v-badge v-${level.toLowerCase().replace(/[^v0-9]/g, "")}`;
    badge.textContent = level;
    head.appendChild(badge);
  }
  const depth = researchField(entry, "证据深度", "evidence_depth", "evidence depth");
  if (depth) {
    const badge = document.createElement("span");
    badge.className = "research-badge evidence-depth";
    badge.textContent = depth;
    badge.title = t("证据深度说明");
    head.appendChild(badge);
  }
  const status = document.createElement("span");
  status.className = `research-badge st-${entry.status || ""}`;
  status.textContent = localizedDocStatus(entry.status || "");
  head.appendChild(status);
  card.appendChild(head);

  const title = document.createElement("div");
  title.className = "research-card-title";
  title.textContent = entry.title;
  card.appendChild(title);

  // 正文摘要:来源看「要点」,发现看「结论」——这是人扫一眼要读的东西。
  const gist = researchField(entry, "要点", "结论", "说明");
  if (gist) {
    const body = document.createElement("div");
    body.className = "research-card-gist";
    body.textContent = gist;
    card.appendChild(body);
  }

  const meta = [researchField(entry, "作者", "author"), researchField(entry, "年份", "year")]
    .filter(Boolean)
    .join(" · ");
  if (meta) {
    const line = document.createElement("div");
    line.className = "research-card-meta";
    line.textContent = meta;
    card.appendChild(line);
  }

  const actions = document.createElement("div");
  actions.className = "research-card-actions";
  // 打开:文献走 URL 进内置 viewer,代码域走证据锚跳文件定位(用户定调不跳出应用)。
  const openable = (entry.fields ?? []).find(([k, v]) => researchLinkField(k, v));
  if (openable) {
    const open = researchOpenLink(openable[0], String(openable[1]), selectedResearchTopic);
    open.className = "ghost mini research-open";
    open.textContent = `↗ ${t("打开")}`;
    actions.appendChild(open);
  }
  if (kind === "source") {
    const copy = document.createElement("button");
    copy.type = "button";
    copy.className = "ghost mini";
    copy.textContent = t("复制 BibTeX");
    copy.addEventListener("click", () => copyResearchCitation(entry));
    actions.appendChild(copy);
  }
  // refs 反向可跳:发现→来源。
  const refs = researchField(entry, "refs");
  for (const ref of refs.split(/[\s,]+/).filter(Boolean)) {
    const link = document.createElement("button");
    link.type = "button";
    link.className = "ref-link";
    link.textContent = ref;
    link.title = t("跳到该来源");
    link.addEventListener("click", () => researchFocus(ref));
    actions.appendChild(link);
  }
  if (kind === "source") {
    const topic = selectedResearchTopicData();
    const citedBy = (topic.findings ?? []).filter((finding) =>
      researchField(finding, "refs").split(/[\\s,]+/).includes(entry.id),
    );
    if (citedBy.length) {
      const backrefs = document.createElement("div");
      backrefs.className = "research-card-backrefs";
      backrefs.append(`${t("被发现引用")}: `);
      for (const finding of citedBy) {
        const link = document.createElement("button");
        link.type = "button";
        link.className = "ref-link";
        link.textContent = finding.id;
        link.title = t("跳到该发现");
        link.addEventListener("click", () => {
          researchTab = "findings";
          renderResearchCards();
          researchFocus(finding.id);
        });
        backrefs.appendChild(link);
      }
      card.appendChild(backrefs);
    }
  }
  const edit = document.createElement("button");
  edit.type = "button";
  edit.className = "ghost mini";
  edit.textContent = t("编辑");
  edit.addEventListener("click", () => researchEdit(entry, kind, card));
  actions.appendChild(edit);
  if ((entry.nextStatuses ?? []).length > 0) {
    for (const next of entry.nextStatuses) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "ghost mini";
      btn.textContent = `→ ${localizedDocStatus(next)}`;
      btn.addEventListener("click", async () => {
        try {
          await invoke("docs_update", { projectDir: currentProject, kind, action: "update", id: entry.id, status: next });
          toast(`${entry.id} → ${localizedDocStatus(next)}`);
          refreshResearch();
        } catch (error) {
          toastError(`${t("保存失败")}:${error}`);
        }
      });
      actions.appendChild(btn);
    }
  }
  card.appendChild(actions);
  return card;
}

/// 行内编辑:字段逐条给输入框,保存走 docs_update(与 req/defect 同一条写通道)。
export function researchEdit(entry, kind, card) {
  if (card.querySelector(".research-edit")) {
    card.querySelector(".research-edit").remove();
    return;
  }
  const box = document.createElement("div");
  box.className = "research-edit";
  const inputs = new Map();
  for (const [key, value] of entry.fields ?? []) {
    const row = document.createElement("label");
    row.className = "research-edit-row";
    row.append(`${key}: `);
    const long = String(value).length > 60;
    const input = document.createElement(long ? "textarea" : "input");
    if (long) input.rows = 3;
    input.value = value;
    inputs.set(key, input);
    row.appendChild(input);
    box.appendChild(row);
  }
  const save = document.createElement("button");
  save.type = "button";
  save.className = "primary mini";
  save.textContent = t("保存");
  save.addEventListener("click", async () => {
    const fields = {};
    for (const [key, input] of inputs) fields[key] = input.value;
    try {
      await invoke("docs_update", { projectDir: currentProject, kind, action: "update", id: entry.id, topic: entry.topic || undefined, fields });
      toast(`${entry.id} ${t("已保存")}`);
      refreshResearch();
    } catch (error) {
      toastError(`${t("保存失败")}:${error}`);
    }
  });
  box.appendChild(save);
  card.appendChild(box);
}

/// 报告里点 [S-00x] 或卡片里点 refs → 滚到那张卡并高亮。溯源要能双向走。
export function researchFocus(id) {
  const target = String(id).trim();
  researchTab = target.startsWith("F-") ? "findings" : "sources";
  renderResearchCards();
  const card = document.querySelector(`.research-card[data-doc-id="${target}"]`);
  if (!card) return;
  card.scrollIntoView({ block: "center" });
  card.classList.add("ref-highlight");
  setTimeout(() => card.classList.remove("ref-highlight"), 1600);
}

export function renderResearchCards() {
  const host = $("research-cards");
  if (!host) return;
  const kind = researchTab === "findings" ? "finding" : "source";
  const topic = selectedResearchTopicData();
  renderResearchFilters();
  const entries = filteredResearchEntries();
  $("research-tab-sources")?.classList.toggle("active", researchTab === "sources");
  $("research-tab-findings")?.classList.toggle("active", researchTab === "findings");
  $("research-tab-sources")?.setAttribute("aria-selected", String(researchTab === "sources"));
  $("research-tab-findings")?.setAttribute("aria-selected", String(researchTab === "findings"));
  host.innerHTML = "";

  const group = document.createElement("section");
  group.className = "research-topic-group";
  group.dataset.topic = researchTopicKey(topic) || "legacy";
  const heading = document.createElement("h2");
  heading.className = "research-topic-title";
  heading.textContent = researchTopicLabel(topic);
  group.appendChild(heading);
  if (!entries.length) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    // 空态给指引而不是一个孤零零的「(空)」(设计 §4.5)。
    empty.textContent = researchTab === "findings"
      ? t("还没有发现。研究时用 finding add 记录结论,每条须挂来源。")
      : t("还没有来源。研究时用 source add 登记文献或代码位置,引用前先登记。");
    group.appendChild(empty);
  } else {
    for (const entry of entries) group.appendChild(researchCard(entry, kind));
  }
  host.appendChild(group);
}

export let selectedResearchExplorationId = "";

function explorationDocumentId(document) {
  return document?.frontmatter?.id || "";
}

function researchExplorationRuns(topic, explorationId) {
  return (topic?.runs ?? []).filter((item) => item?.run?.exploration_id === explorationId);
}

function researchRunTime(item) {
  return Number(item?.run?.finished_at ?? item?.run?.started_at ?? 0);
}

/// 从当前 topic 的 Markdown 投影出稳定的节点和边；不写入、不缓存、不推断关系。
export function researchRouteProjection(topic = selectedResearchTopicData()) {
  const explorations = [...(topic?.explorations ?? [])]
    .filter((document) => explorationDocumentId(document))
    .sort((left, right) => explorationDocumentId(left).localeCompare(explorationDocumentId(right), undefined, { numeric: true }));
  const columns = 4;
  const nodes = explorations.map((document, index) => {
    const id = explorationDocumentId(document);
    const runs = researchExplorationRuns(topic, id).sort((left, right) => researchRunTime(right) - researchRunTime(left));
    return {
      id,
      title: document.frontmatter.title || id,
      status: document.frontmatter.status || "draft",
      hypothesis: document.frontmatter.hypothesis || "",
      resultCount: (document.results ?? []).length,
      runCount: runs.length,
      recentStatus: runs[0]?.run?.status || "—",
      x: 28 + (index % columns) * 190,
      y: 28 + Math.floor(index / columns) * 112,
    };
  });
  const edges = [];
  for (const document of explorations) {
    const id = explorationDocumentId(document);
    for (const dependency of document.frontmatter.depends_on ?? []) {
      edges.push({ from: dependency, to: id, kind: "depends_on" });
    }
    const superseded = document.frontmatter.supersedes;
    if (superseded) edges.push({ from: id, to: superseded, kind: "supersedes" });
  }
  edges.sort((left, right) => `${left.kind}:${left.from}:${left.to}`.localeCompare(`${right.kind}:${right.from}:${right.to}`, undefined, { numeric: true }));
  const diagnostics = [...(topic?.exploration_diagnostics ?? [])].sort((left, right) =>
    `${left.path}:${left.line}:${left.message}`.localeCompare(`${right.path}:${right.line}:${right.message}`, undefined, { numeric: true }),
  );
  return { nodes, edges, diagnostics };
}

function appendRoadmapText(parent, className, text, x, y) {
  const node = document.createElementNS("http://www.w3.org/2000/svg", "text");
  node.setAttribute("class", className);
  node.setAttribute("x", String(x));
  node.setAttribute("y", String(y));
  node.textContent = text;
  parent.appendChild(node);
}

export function renderResearchRoadmap() {
  const graph = $("research-roadmap-graph");
  const diagnosticsHost = $("research-roadmap-diagnostics");
  if (!graph || !diagnosticsHost) return;
  const projection = researchRouteProjection();
  graph.replaceChildren();
  const width = Math.max(640, 28 + (Math.min(4, projection.nodes.length) || 1) * 190);
  const height = Math.max(96, 28 + Math.ceil(projection.nodes.length / 4) * 112);
  graph.setAttribute("viewBox", `0 0 ${width} ${height}`);
  graph.setAttribute("aria-label", t("实验路线图"));
  if (!projection.nodes.length) {
    const empty = document.createElementNS("http://www.w3.org/2000/svg", "text");
    empty.setAttribute("class", "research-roadmap-empty");
    empty.setAttribute("x", "28");
    empty.setAttribute("y", "56");
    empty.textContent = t("暂无探索路线图");
    graph.appendChild(empty);
  }
  const nodeById = new Map(projection.nodes.map((node) => [node.id, node]));
  for (const edge of projection.edges) {
    const from = nodeById.get(edge.from);
    const to = nodeById.get(edge.to);
    if (!from || !to) continue;
    const line = document.createElementNS("http://www.w3.org/2000/svg", "line");
    line.setAttribute("class", `research-roadmap-edge edge-${edge.kind}`);
    line.dataset.edgeFrom = edge.from;
    line.dataset.edgeTo = edge.to;
    line.dataset.edgeKind = edge.kind;
    line.setAttribute("x1", String(from.x + 82));
    line.setAttribute("y1", String(from.y + 28));
    line.setAttribute("x2", String(to.x + 82));
    line.setAttribute("y2", String(to.y + 28));
    graph.appendChild(line);
  }
  for (const node of projection.nodes) {
    const group = document.createElementNS("http://www.w3.org/2000/svg", "g");
    group.classList.add("research-roadmap-node", `node-${node.status}`);
    group.dataset.nodeId = node.id;
    group.setAttribute("tabindex", "0");
    group.setAttribute("role", "button");
    const title = document.createElementNS("http://www.w3.org/2000/svg", "title");
    title.textContent = node.hypothesis ? `${node.title} — ${node.hypothesis}` : node.title;
    group.appendChild(title);
    const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    rect.setAttribute("x", String(node.x));
    rect.setAttribute("y", String(node.y));
    rect.setAttribute("width", "164");
    rect.setAttribute("height", "56");
    rect.setAttribute("rx", "7");
    group.appendChild(rect);
    appendRoadmapText(group, "research-roadmap-node-id", node.id, node.x + 9, node.y + 17);
    appendRoadmapText(group, "research-roadmap-node-title", node.title, node.x + 9, node.y + 34);
    appendRoadmapText(group, "research-roadmap-node-meta", `${node.runCount} ${t("次运行")} · ${node.recentStatus}`, node.x + 9, node.y + 49);
    const open = () => selectResearchExploration(node.id);
    group.addEventListener("click", open);
    group.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        open();
      }
    });
    graph.appendChild(group);
  }
  const count = $("research-roadmap-count");
  if (count) count.textContent = `${projection.nodes.length} ${t("个探索")}`;
  diagnosticsHost.replaceChildren();
  diagnosticsHost.hidden = projection.diagnostics.length === 0;
  for (const diagnostic of projection.diagnostics) {
    const item = document.createElement("div");
    item.className = "research-roadmap-diagnostic";
    item.textContent = `${diagnostic.path}:${diagnostic.line} · ${diagnostic.message}`;
    diagnosticsHost.appendChild(item);
  }
}

export function selectResearchExploration(id) {
  selectedResearchExplorationId = id;
  renderResearchExplorationDetail();
}

function researchResultRun(topic, resultId) {
  return (topic?.runs ?? []).find((item) => item?.run?.result_id === resultId);
}

export function focusResearchRun(resultId) {
  const card = [...document.querySelectorAll(".research-run-card")]
    .find((item) => item.dataset.resultId === resultId);
  if (!card) return false;
  document.querySelectorAll(".research-run-card.is-selected").forEach((item) => item.classList.remove("is-selected"));
  card.classList.add("is-selected");
  card.scrollIntoView?.({ block: "nearest" });
  return true;
}

function appendResearchDetailSection(body, title, text) {
  const section = document.createElement("section");
  section.className = "research-detail-section";
  const heading = document.createElement("h3");
  heading.textContent = title;
  section.appendChild(heading);
  const content = document.createElement("p");
  content.textContent = text || t("暂无");
  section.appendChild(content);
  body.appendChild(section);
}

function researchArtifactButton(path, kind) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "ghost mini research-artifact-link";
  button.textContent = `${kind || t("产物")}: ${path}`;
  button.title = t("在文件预览中打开");
  button.addEventListener("click", () => {
    document.querySelector('.activity-item[data-view="files"]')?.click();
    openFilePreview({ path: String(path).replace(/\\\\/g, "/") });
  });
  return button;
}

export function renderResearchExplorationDetail() {
  const panel = $("research-exploration-detail");
  const body = $("research-exploration-detail-body");
  if (!panel || !body) return;
  const topic = selectedResearchTopicData();
  const exploration = (topic.explorations ?? []).find((item) => explorationDocumentId(item) === selectedResearchExplorationId);
  if (!exploration) {
    panel.hidden = true;
    body.replaceChildren();
    return;
  }
  panel.hidden = false;
  body.replaceChildren();
  const frontmatter = exploration.frontmatter ?? {};
  const title = document.createElement("h2");
  title.className = "research-detail-title";
  title.textContent = `${frontmatter.id || ""} · ${frontmatter.title || frontmatter.id || t("未命名探索")}`;
  body.appendChild(title);
  const status = document.createElement("span");
  status.className = `research-badge st-${frontmatter.status || ""}`;
  status.textContent = localizedDocStatus(frontmatter.status || "");
  body.appendChild(status);
  appendResearchDetailSection(body, t("假设"), [frontmatter.hypothesis, exploration.assumption].filter(Boolean).join("\n"));
  const results = exploration.results ?? [];
  const resultsSection = document.createElement("section");
  resultsSection.className = "research-detail-section research-results-section";
  const resultsHeading = document.createElement("h3");
  resultsHeading.textContent = `${t("实验结果")} (${results.length})`;
  resultsSection.appendChild(resultsHeading);
  if (!results.length) {
    const empty = document.createElement("p");
    empty.className = "doc-empty";
    empty.textContent = t("暂无实验结果");
    resultsSection.appendChild(empty);
  } else {
    const table = document.createElement("table");
    table.className = "research-results-table";
    const header = document.createElement("tr");
    for (const label of [t("实验"), t("参数"), t("状态"), t("关键指标"), t("运行")]) {
      const cell = document.createElement("th");
      cell.textContent = label;
      header.appendChild(cell);
    }
    table.appendChild(header);
    for (const result of results) {
      const row = document.createElement("tr");
      for (const value of [result.result_id, result.params_text, result.status, result.key_metrics_text]) {
        const cell = document.createElement("td");
        cell.textContent = value || "—";
        row.appendChild(cell);
      }
      const actionCell = document.createElement("td");
      const runItem = researchResultRun(topic, result.result_id);
      if (runItem) {
        const open = document.createElement("button");
        open.type = "button";
        open.className = "ref-link research-result-open";
        open.dataset.resultId = result.result_id;
        open.textContent = t("打开运行");
        open.addEventListener("click", () => focusResearchRun(result.result_id));
        actionCell.appendChild(open);
      } else {
        actionCell.textContent = t("暂无运行记录");
      }
      row.appendChild(actionCell);
      table.appendChild(row);
    }
    resultsSection.appendChild(table);
  }
  body.appendChild(resultsSection);
  appendResearchDetailSection(body, t("结论"), exploration.conclusion);
  appendResearchDetailSection(body, t("后续"), exploration.follow_up);
}


export const RESEARCH_REPORT_WINDOW_SIZE = 40;
export let researchReportBlocks = [];
export let researchReportWindowStart = 0;
export let researchReportScrollHost = null;

export function splitResearchReportBlocks(text) {
  const lines = String(text ?? "").replace(/\r\n?/g, "\n").split("\n");
  const blocks = [];
  let block = [];
  let inFence = false;
  const flush = () => {
    if (block.length) blocks.push(block.join("\n"));
    block = [];
  };
  for (const line of lines) {
    const fence = /^\s*```/.test(line);
    if (!inFence && !line.trim()) {
      flush();
      continue;
    }
    block.push(line);
    if (fence) inFence = !inFence;
  }
  flush();
  return blocks;
}

export function decorateResearchReportReferences(host) {
  // 渲染后回扫文本节点,把引用编号替换为按钮。只认已登记的编号,避免把普通
  // 文本里的 S-/F- 误变成死链。
  const topic = selectedResearchTopicData();
  const known = new Set([
    ...(topic.sources ?? []).map((e) => e.id),
    ...(topic.findings ?? []).map((e) => e.id),
  ]);
  if (!known.size) return;
  const walker = document.createTreeWalker(host, 4 /* TEXT_NODE */);
  const targets = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (/\b[SF]-\d{3}\b/.test(node.nodeValue ?? "")) targets.push(node);
  }
  for (const node of targets) {
    const frag = document.createDocumentFragment();
    let last = 0;
    const text2 = node.nodeValue ?? "";
    for (const m of text2.matchAll(/\b[SF]-\d{3}\b/g)) {
      if (!known.has(m[0])) continue;
      if (m.index > last) frag.appendChild(document.createTextNode(text2.slice(last, m.index)));
      const link = document.createElement("button");
      link.type = "button";
      link.className = "ref-link";
      link.textContent = m[0];
      link.addEventListener("click", () => researchFocus(m[0]));
      frag.appendChild(link);
      last = m.index + m[0].length;
    }
    if (!last) continue;
    if (last < text2.length) frag.appendChild(document.createTextNode(text2.slice(last)));
    node.parentNode?.replaceChild(frag, node);
  }
}

export function renderResearchReportWindow() {
  const host = $("research-report");
  if (!host) return;
  host.innerHTML = "";
  if (researchReportWindowStart > 0) {
    const earlier = document.createElement("button");
    earlier.type = "button";
    earlier.className = "earlier-hint research-report-earlier";
    earlier.textContent = `↑ ${t("载入更早的报告内容")}`;
    earlier.addEventListener("click", () => loadEarlierResearchReport());
    host.appendChild(earlier);
  }
  const visible = researchReportBlocks.slice(researchReportWindowStart).join("\n\n");
  if (visible) {
    const body = document.createElement("div");
    body.className = "research-report-window";
    body.innerHTML = renderMarkdown(visible);
    host.appendChild(body);
    decorateResearchReportReferences(body);
  }
  host.dataset.reportWindowStart = String(researchReportWindowStart);
  host.dataset.reportWindowSize = String(RESEARCH_REPORT_WINDOW_SIZE);
}

export function loadEarlierResearchReport() {
  const host = $("research-report");
  if (!host || researchReportWindowStart <= 0) return false;
  const before = host.scrollHeight || 0;
  researchReportWindowStart = Math.max(0, researchReportWindowStart - RESEARCH_REPORT_WINDOW_SIZE);
  renderResearchReportWindow();
  const after = host.scrollHeight || 0;
  host.scrollTop = (host.scrollTop || 0) + Math.max(0, after - before);
  return true;
}

export function bindResearchReportScroll(host) {
  if (researchReportScrollHost === host) return;
  researchReportScrollHost = host;
  host.addEventListener("scroll", () => {
    if (host.scrollTop < 80) loadEarlierResearchReport();
  });
}

/// 报告正文按窗口渲染 markdown,并把 [S-00x]/[F-00x] 变成可点角标。
export function renderResearchReport(text) {
  const host = $("research-report");
  if (!host) return;
  researchReportBlocks = splitResearchReportBlocks(text);
  researchReportWindowStart = Math.max(0, researchReportBlocks.length - RESEARCH_REPORT_WINDOW_SIZE);
  bindResearchReportScroll(host);
  renderResearchReportWindow();
}

export async function refreshResearchReport() {
  try {
    const doc = await invoke("docs_read", {
      projectDir: currentProject,
      kind: "report",
      ...selectedResearchTopicArg(),
    });
    renderResearchReport(doc.content ?? "");
  } catch {
    // 报告还没产出是正常状态(研究刚开始),给指引不报错。
    renderResearchReport(`_${t("还没有报告。研究结束时把结论写进 .kanzei/research/report.md。")}_`);
  }
}

function researchRunPayload(event) {
  if (!event) return {};
  if (typeof event.payload_json === "string") {
    try { return JSON.parse(event.payload_json); } catch { return {}; }
  }
  return event.payload_json ?? {};
}

function renderResearchRunMetricChart(card, events) {
  const points = (events ?? [])
    .filter((event) => event.event_type === "metric")
    .map(researchRunPayload)
    .filter((payload) => typeof payload.name === "string" && Number.isFinite(Number(payload.value)))
    .slice(-80);
  if (!points.length) return;
  const chart = document.createElement("div");
  chart.className = "research-run-chart";
  const label = document.createElement("span");
  label.className = "research-run-chart-label";
  label.textContent = `${points[0].name}: ${Number(points.at(-1).value).toPrecision(5)}`;
  chart.appendChild(label);
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 320 80");
  svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", `${points[0].name} metric curve`);
  const values = points.map((point) => Number(point.value));
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || 1;
  const polyline = document.createElementNS("http://www.w3.org/2000/svg", "polyline");
  polyline.setAttribute("fill", "none");
  polyline.setAttribute("stroke", "currentColor");
  polyline.setAttribute("stroke-width", "2");
  polyline.setAttribute("points", values.map((value, index) =>
    `${(index / Math.max(1, values.length - 1)) * 316 + 2},${76 - ((value - min) / span) * 72}`
  ).join(" "));
  svg.appendChild(polyline);
  chart.appendChild(svg);
  card.appendChild(chart);
}

function renderResearchRunArtifacts(card, run) {
  const links = document.createElement("div");
  links.className = "research-run-artifacts";
  const paths = [];
  try {
    const artifacts = JSON.parse(run.artifacts_json || "[]");
    if (Array.isArray(artifacts)) {
      for (const artifact of artifacts) {
        if (artifact?.path) paths.push([artifact.kind || t("产物"), artifact.path]);
      }
    }
  } catch {
    // 运行记录损坏时保留其它事实，不让详情面板崩溃。
  }
  if (run.terminal_log_path) paths.push([t("终端"), run.terminal_log_path]);
  if (run.metrics_series_path) paths.push([t("指标"), run.metrics_series_path]);
  const unique = new Set();
  for (const [kind, path] of paths) {
    const key = `${kind}:${path}`;
    if (unique.has(key)) continue;
    unique.add(key);
    links.appendChild(researchArtifactButton(path, kind));
  }
  if (links.childNodes.length) card.appendChild(links);
}

export function renderResearchRuns() {
  const host = $("research-run-cards");
  if (!host) return;
  const runs = selectedResearchTopicData().runs ?? [];
  host.innerHTML = "";
  const count = $("research-runs-count");
  if (count) count.textContent = `${runs.length} ${t("条")}`;
  if (!runs.length) {
    const empty = document.createElement("div");
    empty.className = "doc-empty";
    empty.textContent = t("暂无实验运行记录");
    host.appendChild(empty);
    return;
  }
  for (const item of runs) {
    const run = item.run ?? {};
    const events = item.events ?? [];
    const card = document.createElement("article");
    card.className = "research-run-card";
    card.dataset.resultId = run.result_id ?? "";
    const title = document.createElement("strong");
    title.textContent = run.result_id || t("未知运行");
    card.appendChild(title);
    const status = document.createElement("span");
    status.className = `research-badge st-${run.status || ""}`;
    status.textContent = localizedDocStatus(run.status || "");
    card.appendChild(status);
    const drift = Array.isArray(item.drift) ? item.drift : [];
    const driftBadge = document.createElement("span");
    driftBadge.className = `research-run-drift ${drift.length ? "has-drift" : "no-drift"}`;
    driftBadge.textContent = drift.length
      ? `${t("环境漂移")}: ${drift.join(", ")}`
      : t("环境声明一致");
    driftBadge.title = drift.length ? drift.join(", ") : t("登记环境与运行配置一致");
    card.appendChild(driftBadge);
    const meta = document.createElement("span");
    meta.className = "research-run-meta";
    meta.textContent = `${run.policy || "relaxed"} · ${run.execution_json || ""}`;
    card.appendChild(meta);

    let progress = {};
    try { progress = JSON.parse(run.progress_json || "{}"); } catch { progress = {}; }
    const done = Number(progress.done);
    const total = Number(progress.total);
    if (Number.isFinite(done) && Number.isFinite(total) && total > 0) {
      const progressBox = document.createElement("label");
      progressBox.className = "research-run-progress";
      progressBox.textContent = `${t("进度")}: ${done}/${total}${progress.unit ? ` ${progress.unit}` : ""}`;
      const bar = document.createElement("progress");
      bar.max = total;
      bar.value = Math.min(total, Math.max(0, done));
      bar.title = progressBox.textContent;
      progressBox.appendChild(bar);
      card.appendChild(progressBox);
    }
    let cost = {};
    try { cost = JSON.parse(run.cost_json || "{}"); } catch { cost = {}; }
    const costLine = document.createElement("span");
    costLine.className = "research-run-cost";
    const gpuSeconds = Number(cost.gpu_seconds || 0);
    const amount = Number(cost.amount || 0);
    costLine.textContent = `${t("成本")}: ${gpuSeconds.toFixed(1)} gpu_seconds · ${amount.toFixed(4)} ${cost.currency || "billing_unit"}`;
    card.appendChild(costLine);
    renderResearchRunMetricChart(card, events);
    renderResearchRunArtifacts(card, run);

    const terminal = document.createElement("pre");
    terminal.className = "research-run-terminal";
    const messages = events
      .filter((event) => event.event_type === "message")
      .map((event) => {
        const payload = researchRunPayload(event);
        return `[${payload.level || "info"}] ${payload.text || ""}`;
      });
    terminal.textContent = [...messages, item.terminal_preview || ""].filter(Boolean).join("\n");
    terminal.hidden = !terminal.textContent;
    if (!terminal.hidden) card.appendChild(terminal);
    host.appendChild(card);
  }
}

let researchRefreshTimer = null;
export function startResearchPolling() {
  if (researchRefreshTimer) return;
  researchRefreshTimer = setInterval(() => {
    if ($("profile-select")?.value === "research" && $("view-research")?.classList.contains("active")) {
      refreshResearch();
    }
  }, 1000);
}


export async function refreshResearch() {
  if (!currentProject) return;
  try {
    const snapshot = await invoke("docs_snapshot", { projectDir: currentProject });
    researchSnapshot = {
      sources: snapshot.sources ?? [],
      findings: snapshot.findings ?? [],
      research_topics: snapshot.research_topics ?? [],
    };
  } catch (error) {
    log(`${t("研究工件刷新失败")}:${error}`, "warn");
  }
  renderResearchTopicPicker();
  const topic = selectedResearchTopicData();
  const counts = [$("research-sources-count"), $("research-findings-count")];
  if (counts[0]) counts[0].textContent = String((topic.sources ?? []).length);
  if (counts[1]) counts[1].textContent = String((topic.findings ?? []).length);
  renderResearchCards();
  renderResearchRoadmap();
  renderResearchExplorationDetail();
  renderResearchRuns();
  await refreshResearchPlan();
  await refreshResearchReport();
}

defer(() => {
  $("research-tab-sources")?.addEventListener("click", () => {
    researchTab = "sources";
    renderResearchCards();
  });
});
defer(() => {
  $("research-tab-findings")?.addEventListener("click", () => {
    researchTab = "findings";
    renderResearchCards();
  });
});
defer(() => {
  $("research-topic-select")?.addEventListener("change", async (event) => {
    selectedResearchTopic = event.currentTarget.value;
    const topic = selectedResearchTopicData();
    const counts = [$("research-sources-count"), $("research-findings-count")];
    if (counts[0]) counts[0].textContent = String((topic.sources ?? []).length);
    if (counts[1]) counts[1].textContent = String((topic.findings ?? []).length);
    renderResearchCards();
    renderResearchRoadmap();
    renderResearchExplorationDetail();
    renderResearchRuns();
    await refreshResearchPlan();
    await refreshResearchReport();
  });
});

defer(() => {
  for (const [id, key] of [["research-filter-query", "query"], ["research-filter-type", "type"], ["research-filter-level", "level"], ["research-filter-year", "year"], ["research-filter-sort", "sort"]]) {
    const control = $(id);
    control?.addEventListener(control.tagName === "INPUT" ? "input" : "change", (event) => {
      researchFilters[key] = event.currentTarget.value;
      renderResearchCards();
    });
  };
});
defer(() => {
  $("research-exploration-close")?.addEventListener("click", () => {
    selectedResearchExplorationId = "";
    renderResearchExplorationDetail();
  });
});
defer(() => {
  $("research-report-refresh")?.addEventListener("click", () => refreshResearchReport());
});
defer(() => {
  $("research-plan-approve")?.addEventListener("click", async () => {
    const topic = selectedResearchTopicData();
    if (!topic.topic) return;
    try {
      const result = await invoke("research_plan_approve", { projectDir: currentProject, topic: topic.topic });
      researchPlan = result.plan ?? researchPlan;
      renderResearchPlan(researchPlan);
      toast(t("研究计划已批准"));
    } catch (error) {
      toastError(`${t("研究计划审批失败")}:${error}`);
    }
  });
});

/// research 档才出现研究工作台;dev 档反过来藏起研究入口。
/// 用户定调:research 档下 dev 视图(需求/缺陷/测试/git)完全隐藏,模式感优先。
export const DEV_ONLY_VIEWS = ["documents", "metrics", "arch"];
export function syncResearchWorkspaceVisibility() {
  const isResearch = $("profile-select")?.value === "research";
  if (isResearch) startResearchPolling();
  $("activity-research")?.classList.toggle("hidden", !isResearch);
  for (const view of DEV_ONLY_VIEWS) {
    document.querySelector(`.activity-item[data-view="${view}"]`)?.classList.toggle("hidden", isResearch);
  }
  // 正停在被隐藏的视图上时退回对话,避免留在一个入口已消失的页面里。
  const active = document.querySelector(".view.active")?.id ?? "";
  const strandedOnDev = isResearch && DEV_ONLY_VIEWS.some((v) => active === `view-${v}`);
  const strandedOnResearch = !isResearch && active === "view-research";
  if (strandedOnDev || strandedOnResearch) {
    document.querySelector('.activity-item[data-view="chat"]')?.click();
  }
}
