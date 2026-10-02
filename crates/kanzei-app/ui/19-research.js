import { defer } from "./01-core.js";
import { renderInlineMarkdown, renderMarkdownInto } from "./04-markdown.js";
import { $, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, log, toast, toastError } from "./03-shell.js";
import { jumpToEntry, researchLinkField, researchOpenLink } from "./11-docs-list.js";
import { openFilePreview } from "./17-files.js";
// UI-0926 #10:结构化实体导航(见文件末尾的注册)与运行卡的执行配置解析。
import { parseJsonish } from "./04-structured-parse.js";
import { setStructuredNav, structuredNav } from "./04-structured.js";
import { openMemoryDetailById } from "./13-memory.js";
import { openRuntimeMarkdown } from "./15-views-misc.js";
import { active_space, project_workspace, save_research_workspace } from "./03-workspaces.js";
import { research_category, research_enum_label, research_status_label, render_research_navigation, render_research_overview, show_research_page, sync_research_page } from "./19-research-navigation.js";
import { refreshResearchWorkflow, researchWorkflow, resetResearchWorkflow } from "./19-research-auto.js";
import { load_research_library, research_library, selected_library_entry, select_library_entry } from "./03-research-library.js";

// 研究(R-276 批3)。
//
// 为什么独立成一个主视图,而不是继续在侧栏那两条列表上打补丁:侧栏一列一百多像素,
// 论文标题要换三行、按钮往哪放都别扭,还得和 req/defect 共用一套渲染分支——前两轮
// 补丁(D-413/D-414)每轮都出新问题,那本身就是「地方不对」的信号。研究工件是研究
// 模式的核心资产,值得一块自己的地方:左边卡片流(来源/发现),右边报告正文。
//
// 设计依据 docs/design/research_workspace.md:结果>过程(报告是主角)、溯源冗余
// (卡片里能开、报告里能跳)、数据已结构化的不许降级成字符串。

export let researchTab = "sources";
// 从报告点引用跳到来源/发现时记下返回点;高亮持久到「返回报告」或离开文献页(原 1.6 秒定时器会被轮询重绘抹掉,UX-107)。
let researchReturn = null;
let researchHighlightId = "";
export function clear_research_focus() {
  researchReturn = null;
  researchHighlightId = "";
  // 已画出来的卡片不会自己重画:高亮与返回条要当场撤掉,否则回到文献页还是旧样子。
  for (const card of document.querySelectorAll?.(".research-card.ref-highlight") ?? []) card.classList.remove("ref-highlight");
  document.querySelector?.(".research-return")?.remove();
}
export let selectedResearchTopic = "";
export let researchSnapshot = { sources: [], findings: [], research_topics: [] };
export let researchLatexTemplates = [];
let research_context_generation = 0;
export function research_context_guard() {
  const project = currentProject;
  const topic = selectedResearchTopic;
  const generation = research_context_generation;
  return () => project === currentProject && topic === selectedResearchTopic && generation === research_context_generation;
}

export function reset_research_project() {
  resetResearchWorkflow();
  research_context_generation += 1;
  researchSnapshot = { sources: [], findings: [], research_topics: [] };
  selectedResearchTopic = project_workspace().research.topic;
  researchPlan = null;
  researchPlanKey = "";
  researchRenderKey = "";
  clear_research_focus();
  selectedResearchExplorationId = "";
  research_report_text = null;
  renderResearchCards();
  renderResearchRoadmap();
  renderResearchExplorationDetail();
  renderResearchRuns();
  renderResearchPlan(null);
  renderResearchReport("");
  clear_research_writing();
  const select = $("research-topic-select");
  if (select) { select.replaceChildren(); select.disabled = true; }
  render_research_overview();
}

function clear_research_writing() {
  const frame = $("research-latex-pdf");
  if (frame) { frame.hidden = true; frame.removeAttribute("src"); }
  for (const id of ["research-latex-status", "research-latex-history", "research-latex-diagnostics"]) {
    if ($(id)) $(id).textContent = "";
  }
  for (const id of ["research-latex-title", "research-latex-figure-name", "research-latex-figure-caption", "research-latex-figure-label"]) {
    if ($(id)) $(id).value = "";
  }
  if ($("research-latex-document-name")) $("research-latex-document-name").value = "main";
}


export function renderResearchLatexTemplates() {
  const select = $("research-latex-template");
  if (!select) return;
  const current = select.value;
  select.replaceChildren();
  for (const item of researchLatexTemplates) {
    const option = document.createElement("option");
    option.value = item.id;
    option.textContent = item.description ? `${item.name} · ${item.description}` : item.name;
    select.appendChild(option);
  }
  if (researchLatexTemplates.some((item) => item.id === current)) select.value = current;
}

export async function refreshResearchLatexTemplates() {
  try {
    const templates = await invoke("research_latex_templates");
    researchLatexTemplates = Array.isArray(templates) ? templates : [];
  } catch (error) {
    researchLatexTemplates = [];
    log(`${t("LaTeX 模板加载失败")}:${error}`, "warn");
  }
  renderResearchLatexTemplates();
}

export async function createResearchLatexDocument() {
  const is_current = research_context_guard();
  const topic = selectedResearchTopicData();
  const template = $("research-latex-template")?.value;
  const documentName = $("research-latex-document-name")?.value || "main";
  const title = $("research-latex-title")?.value || "";
  const status = $("research-latex-status");
  if (!topic.topic || !template) {
    if (status) status.textContent = t("请先选择研究课题");
    return;
  }
  const button = $("research-latex-create");
  if (button) button.disabled = true;
  try {
    const result = await invoke("research_latex_create", {
      projectDir: currentProject,
      topic: topic.topic,
      templateId: template,
      documentName,
      title,
    });
    if (!is_current()) return;
    if (status) status.textContent = `${t("已创建")}: ${result.tex_path}`;
    toast(`${t("LaTeX 文档已创建")}: ${result.tex_name}`);
  } catch (error) {
    if (!is_current()) return;
    if (status) status.textContent = `${t("创建失败")}: ${error}`;
    toastError(`${t("LaTeX 文档创建失败")}: ${error}`);
  } finally {
    if (button) button.disabled = false;
  }
}



export async function insertResearchLatexFigure() {
  const is_current = research_context_guard();
  const topic = researchLatexTopic();
  const status = $("research-latex-status");
  if (!topic) {
    if (status) status.textContent = t("请先选择研究课题");
    return;
  }
  try {
    const result = await invoke("research_latex_insert_figure", {
      projectDir: currentProject,
      topic,
      documentName: $("research-latex-document-name")?.value || "main",
      figureName: $("research-latex-figure-name")?.value || "",
      caption: $("research-latex-figure-caption")?.value || undefined,
      label: $("research-latex-figure-label")?.value || undefined,
    });
    if (!is_current()) return;
    if (status) status.textContent = `${t("已插入图表引用")}: ${result.reference}`;
    toast(`${t("图表引用已写入")}: ${result.tex_path}`);
  } catch (error) {
    if (!is_current()) return;
    if (status) status.textContent = `${t("图表引用失败")}: ${error}`;
    toastError(`${t("图表引用失败")}: ${error}`);
  }
}

function researchLatexTopic() {
  return selectedResearchTopicData().topic || selectedResearchTopic;
}

export function renderResearchLatexHistory(entries = []) {
  const host = $("research-latex-history");
  if (!host) return;
  host.replaceChildren();
  for (const entry of entries) {
    const row = document.createElement("div");
    row.className = "research-latex-history-row";
    const label = document.createElement("span");
    label.textContent = `${entry.document_name || "main.tex"} · ${entry.success ? t("编译成功") : t("编译失败")} · ${entry.run_id || ""}`;
    row.appendChild(label);
    if (entry.pdf_path) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "ghost mini";
      button.textContent = t("预览 PDF");
      button.addEventListener("click", () => previewResearchLatexPdf(entry.pdf_path));
      row.appendChild(button);
    }
    host.appendChild(row);
  }
}

export async function refreshResearchLatexHistory() {
  const is_current = research_context_guard();
  const topic = researchLatexTopic();
  if (!topic) {
    renderResearchLatexHistory();
    return;
  }
  try {
    const documentName = $("research-latex-document-name")?.value || undefined;
    const entries = await invoke("research_latex_history", {
      projectDir: currentProject,
      topic,
      documentName,
    });
    if (!is_current()) return;
    renderResearchLatexHistory(Array.isArray(entries) ? entries : []);
  } catch (error) {
    if (!is_current()) return;
    renderResearchLatexHistory();
    log(`${t("编译历史读取失败")}:${error}`, "warn");
  }
}

export async function previewResearchLatexPdf(pdfPath) {
  const is_current = research_context_guard();
  const frame = $("research-latex-pdf");
  try {
    const result = await invoke("research_latex_pdf", {
      projectDir: currentProject,
      topic: researchLatexTopic(),
      pdfPath,
    });
    if (!is_current()) return;
    if (!frame) return;
    frame.src = `data:${result.media_type};base64,${result.data}`;
    frame.hidden = false;
  } catch (error) {
    if (!is_current()) return;
    const status = $("research-latex-status");
    if (status) status.textContent = `${t("PDF 预览失败")}: ${error}`;
    toastError(`${t("PDF 预览失败")}: ${error}`);
  }
}

export async function compileResearchLatexDocument() {
  const is_current = research_context_guard();
  const topic = researchLatexTopic();
  const documentName = $("research-latex-document-name")?.value || "main";
  const status = $("research-latex-status");
  const diagnostics = $("research-latex-diagnostics");
  const button = $("research-latex-compile");
  if (!topic) {
    if (status) status.textContent = t("请先选择研究课题");
    return;
  }
  if (button) button.disabled = true;
  try {
    const result = await invoke("research_latex_compile", {
      projectDir: currentProject,
      topic,
      documentName,
    });
    if (!is_current()) return;
    if (diagnostics) {
      diagnostics.textContent = result.diagnostics || "";
      diagnostics.hidden = !result.diagnostics;
    }
    if (status) status.textContent = result.success ? t("编译成功") : t("编译失败，详见日志");
    await refreshResearchLatexHistory();
    if (!is_current()) return;
    if (result.pdf_path) await previewResearchLatexPdf(result.pdf_path);
  } catch (error) {
    if (!is_current()) return;
    if (status) status.textContent = `${t("编译失败")}: ${error}`;
    toastError(`${t("LaTeX 编译失败")}: ${error}`);
  } finally {
    if (button) button.disabled = false;
  }
}

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
  const category = project_workspace().research.category;
  const topics = researchSnapshot.research_topics ?? [];
  const entry = active_space === "research" ? selected_library_entry() : null;
  const data = topics.find((topic) => researchTopicKey(topic) === selectedResearchTopic && research_category(topic) === category)
    ?? { topic: null, legacy: false, label: "", sources: [], findings: [], runs: [] };
  return entry ? { ...data, ...entry } : data;
}

export function sync_research_process_context(item) {
  const topic = item.research_topic || "";
  const metadata = researchSnapshot.research_topics.find((entry) => entry.topic === topic);
  if (metadata) save_research_workspace({ category: research_category(metadata) });
  if (selectedResearchTopic === topic) return;
  if (metadata || !topic) void select_research_topic(topic);
  else selectedResearchTopic = topic;
}

// 内容范围并入课题选择器(B32 / UX-104):原侧栏有一个 5 档「内容范围」下拉,选「未绑定课题」时选择器里是项目名、
// 主区写「开始一个研究课题」、七个页面全是空壳。现在只有一个选择器,按范围分组;选中哪一项,范围跟着它走。
const RESEARCH_CATEGORY_ORDER = [["research", "研究课题"], ["dev_recon", "开发调研"], ["unclassified", "待分类材料"], ["unbound", "未绑定课题的对话"], ["legacy", "历史材料"]];

export function renderResearchTopicPicker() {
  const select = $("research-topic-select");
  if (!select) return;
  const diagnostic = $("research-library-error");
  if (diagnostic) {
    diagnostic.textContent = (research_library?.diagnostics || []).join("\n");
    diagnostic.classList.toggle("hidden", !diagnostic.textContent);
  }
  const saved = project_workspace().research;
  const library_mode = active_space === "research" && research_library;
  const all = library_mode ? research_library.entries : researchSnapshot.research_topics ?? [];
  if (library_mode) {
    selectedResearchTopic = selected_library_entry()?.topic || "";
  } else {
    const topics = all.filter((topic) => research_category(topic) === saved.category);
    selectedResearchTopic = topics.some((topic) => researchTopicKey(topic) === saved.topic)
      ? saved.topic : topics.length ? researchTopicKey(topics[0]) : "";
    if (saved.topic !== selectedResearchTopic) save_research_workspace({ topic: selectedResearchTopic });
  }
  select.replaceChildren();
  select.disabled = !all.length;
  const known = new Set(RESEARCH_CATEGORY_ORDER.map(([kind]) => kind));
  const groups = [...RESEARCH_CATEGORY_ORDER, ["", "其他"]]
    .map(([kind, label]) => [label, all.filter((topic) => (kind ? research_category(topic) === kind : !known.has(research_category(topic))))])
    .filter(([, list]) => list.length);
  const addOption = (parent, topic) => {
    const option = document.createElement("option");
    option.value = topic.id || researchTopicKey(topic);
    const duplicate = all.filter((entry) => researchTopicLabel(entry) === researchTopicLabel(topic)).length > 1;
    option.textContent = `${researchTopicLabel(topic)}${duplicate ? ` · ${topic.id}` : ""}${topic.available === false ? ` · ${t("目录不可用")}` : ""}`;
    parent.appendChild(option);
  };
  if (groups.length > 1) {
    for (const [label, list] of groups) {
      const group = document.createElement("optgroup");
      group.label = t(label);
      for (const topic of list) addOption(group, topic);
      select.appendChild(group);
    }
  } else {
    for (const [, list] of groups) for (const topic of list) addOption(select, topic);
  }
  if (!all.length) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = t("暂无研究课题");
    select.appendChild(option);
  }
  select.value = selected_library_entry()?.id || selectedResearchTopic;
  render_research_navigation();
}

export async function select_research_topic(topic) {
  if (active_space === "research" && research_library) {
    const entry = research_library.entries.find((entry) => entry.id === topic)
      ?? research_library.entries.find((entry) => entry.topic === topic && entry.storage_root === currentProject && entry.kind === project_workspace().research.category);
    return select_library_entry(entry?.id || "");
  }
  resetResearchWorkflow();
  research_context_generation += 1;
  selectedResearchTopic = topic;
  selectedResearchExplorationId = "";
  researchPlan = null;
  researchPlanKey = "";
  researchRenderKey = "";
  clear_research_focus();
  researchFilters = { query: "", type: "", level: "", year: "", sort: "" };
  save_research_workspace({ topic });
  clear_research_writing();
  renderResearchTopicPicker();
  renderResearchCards();
  renderResearchRoadmap();
  renderResearchExplorationDetail();
  renderResearchRuns();
  renderResearchPlan(null);
  research_report_text = null;
  renderResearchReport("");
  const is_current = research_context_guard();
  await Promise.all([refreshResearchPlan(), refreshResearchReport(), refreshResearchWorkflow()]);
  if (!is_current()) return;
  render_research_overview();
  sync_research_page();
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
// 计划与整页的数据签名:轮询回来数据没变就不重绘(原每秒整页重建,编辑框与焦点撑不过 1 秒,UX-015)。
let researchPlanKey = "";
let researchRenderKey = "";

export function renderResearchPlan(plan) {
  const panel = $("research-plan-panel");
  const status = $("research-plan-status");
  const tree = $("research-plan-tree");
  const approve = $("research-plan-approve");
  if (!panel || !status || !tree || !approve) return;
  panel.hidden = false;
  tree.innerHTML = "";
  approve.hidden = !plan;
  if (!plan) { status.textContent = t("尚未创建计划"); return; }
  status.textContent = research_status_label(plan.status || "draft");
  approve.hidden = plan.status !== "awaiting_approval";
  const appendNode = (node, parent) => {
    const item = document.createElement("li");
    item.className = `research-plan-node plan-${node.status || "pending"}`;
    item.textContent = `${node.title} · ${research_status_label(node.status || "pending")}`;
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
  const is_current = research_context_guard();
  const project = currentProject;
  const topic = selectedResearchTopicData();
  if (topic.legacy || !topic.topic) {
    researchPlan = null;
    researchPlanKey = "";
    renderResearchPlan(null);
    return;
  }
  try {
    const snapshot = await invoke("research_plan_get", { projectDir: currentProject, topic: topic.topic });
    if (!is_current()) return;
    if (project !== currentProject || topic.topic !== selectedResearchTopicData().topic) return;
    researchPlan = snapshot.exists ? snapshot.plan : null;
    const key = JSON.stringify([topic.topic, researchPlan]);
    if (key === researchPlanKey) return;
    researchPlanKey = key;
    renderResearchPlan(researchPlan);
    if (snapshot.controller === "workflow") {
      $("research-plan-approve").disabled = true;
      $("research-plan-status").textContent = t("计划由 AUTO 主流程管理");
    }
  } catch (error) {
    if (!is_current()) return;
    if (project !== currentProject || topic.topic !== selectedResearchTopicData().topic) return;
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
    badge.textContent = research_enum_label(type);
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
  // 来源默认就是 active:每张卡都挂一个状态词只是噪音(UX-106),只在状态有信息量时才显示。
  if ((entry.status || "") && entry.status !== "active") {
    const status = document.createElement("span");
    status.className = `research-badge st-${entry.status}`;
    status.textContent = research_status_label(entry.status);
    head.appendChild(status);
  }
  card.appendChild(head);

  const title = document.createElement("div");
  title.className = "research-card-title";
  title.textContent = entry.title;
  card.appendChild(title);

  // 正文摘要:来源看「要点」,发现看「结论」——这是人扫一眼要读的东西。
  const gist = researchField(entry, "要点", "结论", "说明");
  if (gist) {
    // 要点/结论常带行内 markdown(粗体/代码/链接):按行内语法渲染(先转义再构造)。
    const body = document.createElement("div");
    body.className = "research-card-gist";
    body.innerHTML = renderInlineMarkdown(gist);
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
  // 后端给 nextStatuses 就照它;没给(来源/发现的读取通道不带)用固定状态机兜底,写入仍由 docs_update 校验。
  const nextStatuses = Array.isArray(entry.nextStatuses) ? entry.nextStatuses : RESEARCH_NEXT_STATUSES[kind]?.[entry.status] ?? [];
  for (const next of nextStatuses) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ghost mini";
    btn.textContent = RESEARCH_TRANSITION_LABELS[next] ? t(RESEARCH_TRANSITION_LABELS[next]) : `→ ${research_status_label(next)}`;
    btn.addEventListener("click", async () => {
      try {
        await invoke("docs_update", { projectDir: currentProject, kind, action: "update", id: entry.id, topic: entry.topic || undefined, status: next });
        toast(`${entry.id} → ${research_status_label(next)}`);
        refreshResearch();
      } catch (error) {
        toastError(`${t("保存失败")}:${error}`);
      }
    });
    actions.appendChild(btn);
  }
  card.appendChild(actions);
  return card;
}
// 来源 active → archived;发现 draft → confirmed / dropped(model.rs SOURCES / FINDINGS,终态不可回退)。
const RESEARCH_NEXT_STATUSES = { source: { active: ["archived"] }, finding: { draft: ["confirmed", "dropped"] } };
const RESEARCH_TRANSITION_LABELS = { archived: "归档", confirmed: "确认", dropped: "放弃" };

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
export function researchFocus(id, { fromReport = false } = {}) {
  const target = String(id).trim();
  if (fromReport) researchReturn = { scroll: $("research-report")?.scrollTop || 0, id: target };
  researchHighlightId = target;
  researchTab = target.startsWith("F-") ? "findings" : "sources";
  show_research_page("literature");
  renderResearchCards();
  document.querySelector(`.research-card[data-doc-id="${target}"]`)?.scrollIntoView({ block: "center" });
}

/// 文献页顶部的「← 返回报告」:回到成果页并还原滚动位置,同时撤掉高亮。
export function researchReturnToReport() {
  const saved = researchReturn;
  clear_research_focus();
  show_research_page("report");
  const host = $("research-report");
  if (saved && host) {
    const restore = () => { host.scrollTop = saved.scroll; };
    restore();
    if (typeof requestAnimationFrame === "function") requestAnimationFrame(restore);
  }
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
  if (researchReturn) {
    const back = document.createElement("div");
    back.className = "research-return";
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ghost mini";
    button.textContent = `← ${t("返回报告")}`;
    button.addEventListener("click", () => researchReturnToReport());
    const note = document.createElement("span");
    note.className = "dim";
    note.textContent = `${t("从报告跳来，正在查看")} ${researchReturn.id}`;
    back.append(button, note);
    group.appendChild(back);
  }
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
    for (const entry of entries) {
      const card = researchCard(entry, kind);
      if (entry.id === researchHighlightId) card.classList.add("ref-highlight");
      group.appendChild(card);
    }
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
    appendRoadmapText(group, "research-roadmap-node-meta", `${node.runCount} ${t("次运行")} · ${research_status_label(node.recentStatus)}`, node.x + 9, node.y + 49);
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
  // 假设/结论/后续是模型写的 markdown(列表、代码、路径链接)。
  const content = document.createElement("div");
  content.className = "md sv-md";
  if (text) renderMarkdownInto(content, text);
  else content.textContent = t("暂无");
  section.appendChild(content);
  body.appendChild(section);
}

function researchArtifactButton(path, kind) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "ghost mini research-artifact-link";
  button.textContent = `${kind || t("产物")}: ${path}`;
  button.title = t("在文件页中打开");
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
  status.textContent = research_status_label(frontmatter.status || "");
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
      for (const value of [result.result_id, result.params_text, result.status ? research_status_label(result.status) : "", result.key_metrics_text]) {
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


// 成果页从头渲染(UX-102):此前默认只渲染末尾 40 块、顶部「载入更早」要滚到顶才触发,打开就落在报告中段。
// 现在从第一块开始,每窗口 40 块;滚到底部附近自动续载,也有可点的「继续载入」,内容不够一屏时自动补满。
export const RESEARCH_REPORT_WINDOW_SIZE = 40;
export let researchReportBlocks = [];
export let researchReportWindowEnd = 0;
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

/// 图(SVG 里的 tspan 换成 HTML 按钮就不显示了)与代码块里的编号不装饰。缓存命中时 mermaid 图在
/// renderMarkdownInto 里已同步插入,这里遍历得到它的文本节点。行内 code 照旧装饰(报告里常写 `S-001`)。
function insideLiteralBlock(node, host) {
  for (let el = node.parentNode; el && el !== host; el = el.parentNode) {
    const tag = String(el.tagName ?? "").toLowerCase();
    if (tag === "svg" || tag === "pre" || el.classList?.contains?.("kz-diagram")) return true;
  }
  return false;
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
    if (/\b[SF]-\d{3}\b/.test(node.nodeValue ?? "") && !insideLiteralBlock(node, host)) targets.push(node);
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
      // 从报告跳到来源/发现:记下返回点,文献页顶部给「← 返回报告」(UX-107)。
      link.addEventListener("click", () => researchFocus(m[0], { fromReport: true }));
      frag.appendChild(link);
      last = m.index + m[0].length;
    }
    if (!last) continue;
    if (last < text2.length) frag.appendChild(document.createTextNode(text2.slice(last)));
    node.parentNode?.replaceChild(frag, node);
  }
}

// 报告正文按块分窗口渲染,窗口只追加不重画(滚动位置与已展开的内容都不动)。
function appendResearchReportChunk(host, from, to) {
  const chunk = researchReportBlocks.slice(from, to).join("\n\n");
  if (!chunk) return;
  const body = document.createElement("div");
  body.className = "research-report-window";
  renderMarkdownInto(body, chunk);
  const more = host.querySelector?.(".research-report-more") ?? null;
  if (more) host.insertBefore(body, more);
  else host.appendChild(body);
  decorateResearchReportReferences(body);
}

function syncResearchReportMore(host) {
  host.querySelector?.(".research-report-more")?.remove();
  host.dataset.reportWindowStart = "0";
  host.dataset.reportWindowEnd = String(researchReportWindowEnd);
  host.dataset.reportWindowSize = String(RESEARCH_REPORT_WINDOW_SIZE);
  if (researchReportWindowEnd >= researchReportBlocks.length) return;
  const more = document.createElement("button");
  more.type = "button";
  more.className = "earlier-hint research-report-more";
  more.textContent = `↓ ${t("继续载入后面的内容")}`;
  more.addEventListener("click", () => loadMoreResearchReport());
  host.appendChild(more);
  // 滚动容器可能是报告框也可能是外层:按「按钮进入视口」自动续载,不依赖 scroll 事件落在哪一层。
  if (typeof IntersectionObserver === "function") {
    const watcher = new IntersectionObserver((entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      watcher.disconnect();
      loadMoreResearchReport();
    }, { rootMargin: "240px" });
    watcher.observe(more);
  }
}

export function renderResearchReportWindow() {
  const host = $("research-report");
  if (!host) return;
  host.innerHTML = "";
  appendResearchReportChunk(host, 0, researchReportWindowEnd);
  syncResearchReportMore(host);
  fillResearchReport(host);
}

/// 内容不够一屏时(滚不动就收不到 scroll 事件)继续补,最多补几窗;没有真实高度(未布局)就不补。
function fillResearchReport(host) {
  for (let round = 0; round < 6 && researchReportWindowEnd < researchReportBlocks.length; round += 1) {
    if (!host.clientHeight || host.scrollHeight > host.clientHeight + 40) return;
    loadMoreResearchReport();
  }
}

export function loadMoreResearchReport() {
  const host = $("research-report");
  if (!host || researchReportWindowEnd >= researchReportBlocks.length) return false;
  const from = researchReportWindowEnd;
  researchReportWindowEnd = Math.min(researchReportBlocks.length, from + RESEARCH_REPORT_WINDOW_SIZE);
  appendResearchReportChunk(host, from, researchReportWindowEnd);
  syncResearchReportMore(host);
  return true;
}

export function bindResearchReportScroll(host) {
  if (researchReportScrollHost === host) return;
  researchReportScrollHost = host;
  host.addEventListener("scroll", () => {
    if (host.scrollTop + host.clientHeight >= host.scrollHeight - 160) loadMoreResearchReport();
  });
}

/// 报告正文按窗口渲染 markdown,并把 [S-00x]/[F-00x] 变成可点角标。
export function renderResearchReport(text) {
  const host = $("research-report");
  if (!host) return;
  researchReportBlocks = splitResearchReportBlocks(text);
  researchReportWindowEnd = Math.min(researchReportBlocks.length, RESEARCH_REPORT_WINDOW_SIZE);
  bindResearchReportScroll(host);
  renderResearchReportWindow();
}

export async function refreshResearchReport() {
  const project = currentProject;
  const topic = selectedResearchTopicData();
  if ((!topic.topic && !topic.legacy) || !topic.report) {
    renderResearchReport(`_${t("尚未生成报告。可在课题对话中继续研究并整理成果。")}_`);
    return;
  }
  try {
    const doc = await invoke("docs_read", { projectDir: project, kind: "report", ...selectedResearchTopicArg() });
    if (project !== currentProject || topic.topic !== selectedResearchTopicData().topic) return;
    if (research_report_text !== doc.content) {
      research_report_text = doc.content;
      renderResearchReport(doc.content ?? "");
    }
  } catch (error) {
    if (project !== currentProject || topic.topic !== selectedResearchTopicData().topic) return;
    renderResearchReport(`_${t("报告读取失败，请刷新重试。")}_`);
    log(`${t("研究报告读取失败")}: ${error}`, "warn");
  }
}
let research_report_text = null;

function researchRunPayload(event) {
  if (!event) return {};
  if (typeof event.payload_json === "string") {
    try { return JSON.parse(event.payload_json); } catch { return {}; }
  }
  return event.payload_json ?? {};
}

/// 实验指标曲线(UX-101):按指标名分组,每个名字一张图(此前所有指标混成一条折线、标签取第一个名字、只有 87×22px)。
/// 图是直线段折线;每个采样点一条整高悬停带,原生提示给出步数与数值;标题行给最新值与区间。
function renderResearchRunMetricChart(card, events) {
  const series = new Map();
  for (const event of events ?? []) {
    if (event.event_type !== "metric") continue;
    const payload = researchRunPayload(event);
    if (typeof payload.name !== "string" || !Number.isFinite(Number(payload.value))) continue;
    if (!series.has(payload.name)) series.set(payload.name, []);
    series.get(payload.name).push({ value: Number(payload.value), step: Number.isFinite(Number(payload.step)) && payload.step !== null ? Number(payload.step) : null });
  }
  if (!series.size) return;
  const box = document.createElement("div");
  box.className = "research-run-charts";
  for (const [name, all] of [...series].slice(0, 8)) {
    const points = all.slice(-120);
    const values = points.map((point) => point.value);
    const min = Math.min(...values);
    const max = Math.max(...values);
    const span = max - min || 1;
    const chart = document.createElement("div");
    chart.className = "research-run-chart";
    chart.dataset.metric = name;
    const label = document.createElement("span");
    label.className = "research-run-chart-label";
    label.textContent = `${name}: ${Number(values.at(-1)).toPrecision(5)}`;
    const range = document.createElement("span");
    range.className = "research-run-chart-range dim";
    range.textContent = `${t("最低")} ${Number(min).toPrecision(4)} · ${t("最高")} ${Number(max).toPrecision(4)} · ${points.length} ${t("个点")}`;
    chart.append(label, range);
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 480 120");
    svg.setAttribute("preserveAspectRatio", "none");
    svg.setAttribute("role", "img");
    svg.setAttribute("aria-label", `${name} ${t("指标曲线")}`);
    const x = (index) => (index / Math.max(1, values.length - 1)) * 472 + 4;
    const y = (value) => 112 - ((value - min) / span) * 104;
    const polyline = document.createElementNS("http://www.w3.org/2000/svg", "polyline");
    polyline.setAttribute("fill", "none");
    polyline.setAttribute("stroke", "currentColor");
    polyline.setAttribute("stroke-width", "2");
    polyline.setAttribute("stroke-linejoin", "round");
    polyline.setAttribute("vector-effect", "non-scaling-stroke");
    polyline.setAttribute("points", values.map((value, index) => `${x(index).toFixed(1)},${y(value).toFixed(1)}`).join(" "));
    svg.appendChild(polyline);
    const band = 472 / Math.max(1, values.length - 1);
    for (const [index, point] of points.entries()) {
      const hit = document.createElementNS("http://www.w3.org/2000/svg", "rect");
      hit.setAttribute("class", "research-run-chart-hit");
      hit.setAttribute("x", (x(index) - band / 2).toFixed(1));
      hit.setAttribute("y", "0");
      hit.setAttribute("width", band.toFixed(1));
      hit.setAttribute("height", "120");
      const tip = document.createElementNS("http://www.w3.org/2000/svg", "title");
      tip.textContent = `${point.step == null ? `#${index + 1}` : `${t("步")} ${point.step}`} · ${name} ${Number(point.value).toPrecision(5)}`;
      hit.appendChild(tip);
      svg.appendChild(hit);
    }
    chart.appendChild(svg);
    box.appendChild(chart);
  }
  card.appendChild(box);
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
    status.textContent = research_status_label(run.status || "");
    card.appendChild(status);
    const drift = Array.isArray(item.drift) ? item.drift : [];
    const driftBadge = document.createElement("span");
    driftBadge.className = `research-run-drift ${drift.length ? "has-drift" : "no-drift"}`;
    driftBadge.textContent = drift.length
      ? `${t("环境漂移")}: ${drift.join(", ")}`
      : t("环境声明一致");
    driftBadge.title = drift.length ? drift.join(", ") : t("登记环境与运行配置一致");
    card.appendChild(driftBadge);
    // UI-0926 #10:执行配置是 {kind, command, host…} JSON——拆成策略/类型 chip + 命令,
    // 不再原样拼进 meta 行。
    const meta = document.createElement("span");
    meta.className = "research-run-meta sv-run-meta";
    const metaChip = (text, title = "") => {
      const chip = document.createElement("span");
      chip.className = "sv-chip";
      chip.textContent = text;
      if (title) chip.title = title;
      meta.appendChild(chip);
    };
    metaChip(research_enum_label(run.policy || "relaxed"), t("执行策略"));
    const execution = parseJsonish(run.execution_json || "");
    if (execution && typeof execution === "object" && !Array.isArray(execution)) {
      if (execution.kind) metaChip(research_enum_label(execution.kind), t("执行方式"));
      if (typeof execution.host === "string" && execution.host) metaChip(execution.host);
      if (typeof execution.command === "string" && execution.command.trim()) {
        const command = document.createElement("code");
        command.className = "sv-cmd";
        command.textContent = execution.command.split(/\r?\n/)[0];
        command.title = execution.command;
        meta.appendChild(command);
      }
    } else if (run.execution_json) {
      meta.appendChild(document.createTextNode(String(run.execution_json)));
    }
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
    const recorded = cost.amount != null || cost.gpu_seconds != null;
    costLine.textContent = recorded
      ? `${t("成本")}: ${cost.gpu_seconds == null ? "—" : Number(cost.gpu_seconds).toFixed(1)} GPU s · ${cost.amount == null ? "—" : Number(cost.amount).toFixed(4)} ${cost.currency || ""}`
      : t("成本未记录");
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
    // 快照只带每个运行最近的一段事件(UX-105,docs.rs RESEARCH_RUN_EVENT_TAIL):被截断时写明,不让截断变成静默丢数据。
    if (item.events_truncated) {
      const note = document.createElement("span");
      note.className = "research-run-truncated dim";
      note.textContent = t("仅显示最近 {shown} 条事件(共 {total} 条)").replace("{shown}", String(events.length)).replace("{total}", String(item.events_total));
      card.appendChild(note);
    }
    host.appendChild(card);
  }
}

// 研究页的后台刷新(UX-015 / UX-105)。原来每秒一次 docs_snapshot + 计划 + 报告 + 流程(含全量事件)并整页重建,
// 来源卡的行内编辑框、AUTO 预算输入框、路线图焦点撑不过一秒。现在:4 秒一轮、页面隐藏不刷;只读当前页用得到的;
// 拿回来的数据按签名比对,没变就不重绘;用户正在输入/选择/编辑时本轮不重绘(签名不更新,放下焦点后的下一轮补上)。
const RESEARCH_POLL_MS = 4000;
let researchRefreshTimer = null;
let researchPollTick = 0;
export function startResearchPolling() {
  if (researchRefreshTimer) return;
  researchRefreshTimer = setInterval(() => {
    if (document.hidden) return;
    if (active_space === "research" && $("view-research")?.classList.contains("active")) {
      refreshResearch({ poll: true });
    }
  }, RESEARCH_POLL_MS);
}

/// 研究页里正有人在输入、选择或编辑:此时整页重建会吃掉焦点与未保存的输入。
export function researchUserEditing() {
  const active = document.activeElement;
  const view = $("view-research");
  if (active && view?.contains?.(active) && /^(INPUT|TEXTAREA|SELECT)$/.test(String(active.tagName ?? ""))) return true;
  return Boolean(document.querySelector?.("#research-cards .research-edit"));
}

const research_refresh_inflight = new Map();
export async function refreshResearch({ poll = false } = {}) {
  if (active_space === "research") {
    // 课题库平时用缓存;轮询每 4 轮(约 16 秒)才强刷一次,代理新建的课题才会出现。
    const reload = poll && ++researchPollTick % 4 === 0;
    try { await load_research_library(reload); }
    catch (error) { if (!poll) toastError(`${t("研究课题库读取失败")}: ${error}`); return; }
  }
  if (!currentProject) {
    renderResearchTopicPicker();
    render_research_overview();
    sync_research_page();
    return;
  }
  const project = currentProject;
  // 在途的轮询不能吞掉手动刷新(保存编辑后要立刻重绘):手动刷新排在它后面再跑一遍。
  while (research_refresh_inflight.has(project)) {
    const pending = research_refresh_inflight.get(project);
    if (poll) return pending;
    await pending;
    if (project !== currentProject) return;
  }
  const request = (async () => {
    try {
      const snapshot = await invoke("docs_snapshot", { projectDir: project });
      if (project !== currentProject) return;
      researchSnapshot = { sources: snapshot.sources ?? [], findings: snapshot.findings ?? [], research_topics: snapshot.research_topics ?? [] };
      $("research-load-error")?.classList.add("hidden");
      const page = project_workspace().research.page;
      const key = JSON.stringify([researchSnapshot, research_library?.entries, project_workspace().research, researchTab, researchFilters, selectedResearchExplorationId]);
      if (poll && (key === researchRenderKey || researchUserEditing())) {
        // 数据没变,或正在编辑:不重绘。仍按当前页补读它依赖的计划/报告/流程(各自有签名,没变也不重绘)。
        await refreshResearchPageData(page, true);
        return;
      }
      researchRenderKey = key;
      renderResearchTopicPicker();
      const topic = selectedResearchTopicData();
      if ($("research-sources-count")) $("research-sources-count").textContent = String((topic.sources ?? []).length);
      if ($("research-findings-count")) $("research-findings-count").textContent = String((topic.findings ?? []).length);
      renderResearchCards();
      renderResearchRoadmap();
      renderResearchExplorationDetail();
      renderResearchRuns();
      await refreshResearchPageData(page, poll);
      if (project !== currentProject) return;
      if (page === "writing") {
        if (!researchLatexTemplates.length) await refreshResearchLatexTemplates();
        await refreshResearchLatexHistory();
      }
      if (project !== currentProject) return;
      render_research_overview();
      sync_research_page();
    } catch (error) {
      if (project !== currentProject) return;
      const message = $("research-load-error");
      if (message) { message.textContent = `${t("研究工件刷新失败")}: ${error}`; message.classList.remove("hidden"); }
      log(`${t("研究工件刷新失败")}: ${error}`, "warn");
    } finally {
      research_refresh_inflight.delete(project);
    }
  })();
  research_refresh_inflight.set(project, request);
  startResearchPolling();
  return request;
}

/// 计划/报告/流程:手动刷新全读;轮询只读当前页用得到的(概览:计划 + 流程;成果:报告)。
async function refreshResearchPageData(page, poll) {
  const wants = poll ? { plan: page === "overview", report: page === "report", workflow: page === "overview" } : { plan: true, report: true, workflow: true };
  const before = JSON.stringify(researchWorkflow);
  await Promise.all([wants.plan && refreshResearchPlan(), wants.report && refreshResearchReport(), wants.workflow && refreshResearchWorkflow()]);
  // 流程只在概览里画:它变了(阶段推进、暂停)概览才需要重画;没变的话 render_research_overview 自己也会按签名跳过。
  if (poll && wants.workflow && before !== JSON.stringify(researchWorkflow)) {
    render_research_overview();
    sync_research_page();
  }
}

defer(() => {
  $("research-latex-create")?.addEventListener("click", () => createResearchLatexDocument());
  $("research-latex-compile")?.addEventListener("click", () => compileResearchLatexDocument());
  $("research-latex-history-refresh")?.addEventListener("click", () => refreshResearchLatexHistory());
  $("research-latex-insert-figure")?.addEventListener("click", () => insertResearchLatexFigure());
});

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
    await select_research_topic(event.currentTarget.value);
    show_research_page("overview");
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
    const is_current = research_context_guard();
    if (!topic.topic) return;
    try {
      const result = await invoke("research_plan_approve", { projectDir: currentProject, topic: topic.topic });
      if (!is_current()) return;
      researchPlan = result.plan ?? researchPlan;
      renderResearchPlan(researchPlan);
      toast(t("研究计划已批准"));
    } catch (error) {
      toastError(`${t("研究计划审批失败")}:${error}`);
    }
  });
});

// 可见性由空间导航统一控制；保留入口供原有导入方调用。
export const DEV_ONLY_VIEWS = ["documents", "metrics", "arch", "lines"];
export function syncResearchWorkspaceVisibility() {
  if (active_space === "research") startResearchPolling();
}

// ---------- 结构化实体导航(UI-0926 #10) ----------
// 04-structured.js 的 chip(条目编号/路径/URL)与 markdown 路径链接只调 structuredNav;
// 真实跳转在这里注册——本模块本来就依赖文档列表、文件预览与应用内查看器,04 不必在
// 求值期反向依赖它们。
export async function openUrlInApp(url, topic = "") {
  const target = String(url ?? "");
  if (!target) return;
  try {
    const isArxiv = Boolean(topic) && /^https?:\/\/(?:export\.)?arxiv\.org\//i.test(target);
    const page = isArxiv
      ? await invoke("research_arxiv_preview", { projectDir: currentProject, topic, url: target })
      : await invoke("webfetch_preview", { url: target });
    const depth = page.depth ? `[${page.depth}]\n` : "";
    openRuntimeMarkdown(page.title || target, `${depth}${page.text || ""}`);
  } catch (error) {
    toastError(`${t("打开失败")}:${error}`);
  }
}
/// docs/ 下的 markdown 进应用内查看器;其余文件切到文件打开预览。
export async function openStructuredPath(path, line = null) {
  const rel = String(path ?? "").replace(/\\/g, "/").replace(/^\.\//, "");
  if (!rel) return;
  if (/^docs\/.+\.md$/i.test(rel) && currentProject) {
    try {
      const file = await invoke("docs_read_custom", { projectDir: currentProject, relPath: rel });
      openRuntimeMarkdown(file.name || rel, file.content);
      return;
    } catch {
      /* 读不到(不在 docs 白名单/已删除)就退回文件预览,由预览如实报错。 */
    }
  }
  document.querySelector('.activity-item[data-view="files"]')?.click();
  void openFilePreview({ path: rel, line });
}
export function openStructuredMemory(scope, id) {
  document.querySelector('.activity-item[data-view="memory"]')?.click();
  void openMemoryDetailById(scope, id);
}
/// R/D/I/S/F/T 走条目跳转;M-(项目)/U-(全局)进记忆详情;A- 没有对应视图,不跳。
export function openStructuredRef(id) {
  const ref = String(id ?? "");
  if (/^[MU]-\d/.test(ref)) {
    openStructuredMemory(ref.startsWith("U-") ? "global" : "project", ref);
    return;
  }
  if (!ref || /^A-/.test(ref)) return;
  void jumpToEntry(ref, { expand: true });
}
defer(() => {
  setStructuredNav({
    openRef: openStructuredRef,
    openPath: openStructuredPath,
    openUrl: (url) => openUrlInApp(url),
    openMemory: openStructuredMemory,
  });
  // markdown 路径链接没有 href(不让 WebView 自己导航),点击统一委托到这里。
  document.addEventListener("click", (event) => {
    const link = event.target?.closest?.("a.md-path");
    if (!link) return;
    event.preventDefault?.();
    structuredNav.openPath(link.dataset.path, Number(link.dataset.line) || null);
  });
});
