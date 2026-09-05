import { $, defer, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeProcessId, currentProject, navigate_view, processItems, toastError } from "./03-shell.js";
import { active_space, create_workspace_process, project_workspace, save_research_workspace } from "./03-workspaces.js";
import { renderParallelTaskStatus, switchProcess } from "./09-sessions.js";
import { refreshResearch, researchPlan, researchSnapshot, researchTopicKey, researchTopicLabel, selectedResearchTopicData, select_research_topic } from "./19-research.js";

export const research_pages = { overview: "概览", chat: "对话", literature: "文献与发现", plan: "研究计划", experiments: "实验", report: "成果", writing: "论文写作" };
export function research_status_label(status) {
  const labels = { draft: "草稿", awaiting_approval: "待批准", approved: "已批准", pending: "待开始", ready: "可开始", running: "运行中", completed: "已完成", blocked: "受阻", failed: "失败" };
  return labels[status] ? t(labels[status]) : status;
}
let research_chat_pending = false;

export function research_category(topic) {
  return topic.kind || (topic.legacy ? "legacy" : "research");
}

export function render_research_navigation() {
  const topic = selectedResearchTopicData();
  const saved = project_workspace().research;
  const select = $("research-category-select");
  if (select) select.value = saved.category;
  const heading = $("research-heading");
  if (heading) heading.textContent = topic.topic || topic.legacy ? researchTopicLabel(topic) : t("研究课题");
  const scope = $("research-scope-label");
  if (scope) scope.textContent = [$("project-switch-name")?.textContent, topic.topic || ""].filter(Boolean).join(" / ");
  for (const button of document.querySelectorAll("[data-research-page]")) {
    const page = button.dataset.researchPage;
    button.classList.toggle("active", saved.page === page);
    if (saved.page === page) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  }
  renderParallelTaskStatus(processItems);
}

export function sync_research_page() {
  const page = project_workspace().research.page;
  const effective = research_pages[page] && page !== "chat" ? page : "overview";
  const workspace = document.querySelector(".research-workspace");
  if (workspace) workspace.dataset.page = effective;
  if ($("research-page-label")) $("research-page-label").textContent = t(research_pages[effective]);
  render_research_navigation();
}

export async function open_research_chat() {
  if (research_chat_pending || active_space !== "research" || !currentProject) return;
  const topic = selectedResearchTopicData();
  const project = currentProject;
  const topic_id = topic.topic || "";
  const is_current = () => project === currentProject && active_space === "research" && (selectedResearchTopicData().topic || "") === topic_id;
  research_chat_pending = true;
  try {
    const candidates = processItems.filter((item) => item.profile === "research" && (item.research_topic || "") === topic_id);
    const target = candidates.find((item) => item.id === activeProcessId) ?? candidates[0];
    if (target) await switchProcess(target.id);
    else await create_workspace_process(topic_id, is_current);
    if (!is_current()) return;
    save_research_workspace({ page: "chat" });
    navigate_view("chat");
    render_research_navigation();
    $("prompt")?.focus();
  } catch (error) {
    toastError(`${t("打开研究对话失败")}: ${error}`);
  } finally {
    research_chat_pending = false;
  }
}

export function show_research_page(page) {
  if (!(page in research_pages)) return;
  if (page === "chat") return open_research_chat();
  save_research_workspace({ page });
  sync_research_page();
  navigate_view("research");
}

export function render_research_overview() {
  const host = $("research-overview");
  if (!host) return;
  host.replaceChildren();
  const topic = selectedResearchTopicData();
  const heading = document.createElement("h2");
  heading.textContent = topic.topic || topic.legacy ? researchTopicLabel(topic) : t("开始一个研究课题");
  const text = document.createElement("p");
  text.className = "dim";
  text.textContent = topic.topic || topic.legacy
    ? t("围绕同一课题组织对话、证据、实验和成果。")
    : t("新建课题，或切换内容范围查看已有材料。");
  host.append(heading, text);
  if (!topic.topic && !topic.legacy) {
    const create = document.createElement("button");
    create.className = "primary";
    create.type = "button";
    create.textContent = t("新建课题");
    create.addEventListener("click", () => $("research-topic-new").click());
    host.appendChild(create);
    return;
  }
  const rows = [
    ["文献与发现", `${(topic.sources ?? []).length} ${t("来源")} · ${(topic.findings ?? []).length} ${t("发现")}`, "literature"],
    ["研究计划", researchPlan ? research_status_label(researchPlan.status) || t("已创建") : t("尚未创建计划"), "plan"],
    ["实验", `${(topic.runs ?? []).length} ${t("次运行")}`, "experiments"],
    ["成果", topic.report ? t("研究报告已生成") : t("尚未生成报告"), "report"],
  ];
  const list = document.createElement("div");
  list.className = "research-overview-list";
  for (const [label, value, page] of rows) {
    const button = document.createElement("button");
    button.type = "button";
    const name = document.createElement("strong");
    name.textContent = t(label);
    const status = document.createElement("span");
    status.textContent = value;
    button.append(name, status);
    button.addEventListener("click", () => show_research_page(page));
    list.appendChild(button);
  }
  host.appendChild(list);
  const chat = document.createElement("button");
  chat.type = "button";
  chat.className = "primary";
  chat.textContent = t("继续课题对话");
  chat.addEventListener("click", () => void open_research_chat());
  host.appendChild(chat);
}

defer(() => {
  for (const button of document.querySelectorAll("[data-research-page]")) {
    button.addEventListener("click", () => show_research_page(button.dataset.researchPage));
  }
  $("research-category-select")?.addEventListener("change", async (event) => {
    const category = event.currentTarget.value;
    const first = researchSnapshot.research_topics.find((topic) => research_category(topic) === category);
    save_research_workspace({ category, topic: first ? researchTopicKey(first) : "", page: "overview" });
    await select_research_topic(first ? researchTopicKey(first) : "");
    show_research_page("overview");
  });
  $("research-topic-new")?.addEventListener("click", () => {
    $("research-topic-form").classList.toggle("hidden");
    $("research-topic-title").focus();
  });
  $("research-topic-cancel")?.addEventListener("click", () => $("research-topic-form").classList.add("hidden"));
  $("research-topic-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const project = currentProject;
    if (!project) return;
    const button = $("research-topic-submit");
    button.disabled = true;
    $("research-topic-error").textContent = "";
    try {
      const result = await invoke("research_topic_create", { projectDir: project, topic: $("research-topic-slug").value.trim(), title: $("research-topic-title").value.trim() });
      if (project !== currentProject) return;
      save_research_workspace({ category: "research", topic: result.topic, page: "overview" });
      await refreshResearch();
      if (project !== currentProject) return;
      await select_research_topic(result.topic);
      $("research-topic-form").reset();
      $("research-topic-form").classList.add("hidden");
      show_research_page("overview");
    } catch (error) {
      if (project === currentProject) $("research-topic-error").textContent = `${t("创建课题失败")}: ${error}`;
    } finally {
      button.disabled = false;
    }
  });
});
