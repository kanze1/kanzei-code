import { $, defer, invoke } from "./01-core.js";
import { languageIsEnglish, localizedDocStatus, t } from "./02-i18n.js";
import { activeProcessId, currentProject, navigate_view, processItems, toastError } from "./03-shell.js";
import { active_space, create_workspace_process, project_workspace, save_research_workspace } from "./03-workspaces.js";
import { renderParallelTaskStatus, switchProcess } from "./09-sessions.js";
import { clear_research_focus, refreshResearchLatexHistory, refreshResearchLatexTemplates, researchPlan, researchSnapshot, researchTopicLabel, selectedResearchTopicData, select_research_topic } from "./19-research.js";
import { renderResearchWorkflow, researchWorkflow } from "./19-research-auto.js";
import { load_research_library, research_library, selected_library_entry } from "./03-research-library.js";

// 研究计划并入概览(B32):原「研究计划」页只有一棵树加一个批准按钮,不值一个页面;现在计划直接画在概览里。
export const research_pages = { overview: "概览", chat: "对话", literature: "文献与发现", experiments: "实验", report: "成果", writing: "论文写作" };
export function research_status_label(status) {
  const labels = {
    draft: "草稿", awaiting_approval: "待批准", approved: "已批准", pending: "待开始", ready: "可开始", running: "运行中", completed: "已完成",
    blocked: "受阻", failed: "失败", succeeded: "成功", cancelled: "已取消", confirmed: "已确认", abandoned: "已放弃", done: "已完成",
  };
  return labels[status] ? t(labels[status]) : localizedDocStatus(status);
}
// 研究页上其余的英文枚举(执行策略 relaxed/…、执行方式 local/ssh、来源类型 paper/code/dataset…)经这张表出人话;
// 表里没有的值(用户自己写的类型等)原样返回,不吞信息。
const RESEARCH_ENUM_WORDS = {
  relaxed: ["宽松", "Relaxed"], managed: ["受管", "Managed"], approval: ["需批准", "Needs approval"], strict: ["严格", "Strict"],
  local: ["本机", "Local"], ssh: ["远程 SSH", "Remote SSH"],
  paper: ["论文", "Paper"], code: ["代码", "Code"], dataset: ["数据集", "Dataset"], web: ["网页", "Web page"], doc: ["文档", "Document"],
};
export function research_enum_label(value) {
  const hit = RESEARCH_ENUM_WORDS[String(value ?? "").trim().toLowerCase()];
  return hit ? hit[languageIsEnglish() ? 1 : 0] : String(value ?? "");
}
let research_chat_pending = false;

export function research_category(topic) {
  return topic.kind || (topic.legacy ? "legacy" : "research");
}
// 「未绑定课题的对话」没有文献、计划、实验、成果、论文,只剩概览与对话两页。
const unbound_pages = new Set(["overview", "chat"]);
const is_unbound = (topic) => research_category(topic) === "unbound";

export function render_research_navigation() {
  const topic = selectedResearchTopicData();
  const saved = project_workspace().research;
  const heading = $("research-heading");
  if (heading) heading.textContent = topic.topic || topic.legacy ? researchTopicLabel(topic) : is_unbound(topic) ? researchTopicLabel(topic) : t("研究课题");
  const scope = $("research-scope-label");
  if (scope) scope.textContent = topic.topic || "";
  if ($("research-switch-name")) $("research-switch-name").textContent = topic.topic || topic.legacy || is_unbound(topic) ? researchTopicLabel(topic) : t("开始一个研究课题");
  for (const button of document.querySelectorAll("[data-research-page]")) {
    const page = button.dataset.researchPage;
    button.hidden = is_unbound(topic) && !unbound_pages.has(page);
    button.disabled = topic.available === false && page !== "overview";
    button.classList.toggle("active", saved.page === page);
    if (saved.page === page) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  }
  renderParallelTaskStatus(processItems);
}

export function sync_research_page() {
  const page = project_workspace().research.page;
  const topic = selectedResearchTopicData();
  const effective = research_pages[page] && page !== "chat" && !(is_unbound(topic) && !unbound_pages.has(page)) ? page : "overview";
  const workspace = document.querySelector(".research-workspace");
  if (workspace) workspace.dataset.page = effective;
  if ($("research-page-label")) $("research-page-label").textContent = t(research_pages[effective]);
  render_research_navigation();
}

export async function open_research_chat() {
  if (research_chat_pending || active_space !== "research") return;
  const topic = selectedResearchTopicData();
  if (!topic.topic && project_workspace().research.category !== "unbound") {
    $("research-topic-form")?.classList.remove("hidden");
    $("research-topic-title")?.focus();
    return;
  }
  if (!currentProject) return;
  const project = currentProject;
  const topic_id = topic.topic || "";
  const is_current = () => project === currentProject && active_space === "research" && (selectedResearchTopicData().topic || "") === topic_id;
  research_chat_pending = true;
  try {
    const candidates = processItems.filter((item) => item.profile === "research" && (item.research_topic || "") === topic_id);
    const target = candidates.find((item) => item.id === activeProcessId) ?? candidates[0];
    if (!target && !topic_id) return create_workspace_process(null);
    if (target) await switchProcess(target.id, true);
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
  // 离开文献页就撤掉「从报告跳来」的高亮与返回点;researchFocus 自己先切页再重画,不受影响。
  if (page !== "literature") clear_research_focus();
  save_research_workspace({ page });
  sync_research_page();
  navigate_view("research");
  if (page === "writing") void Promise.all([refreshResearchLatexTemplates(), refreshResearchLatexHistory()]);
}

// 概览按数据签名重画:原来每秒 replaceChildren,AUTO 预算输入框一秒内就失焦。
// 签名没变不动;焦点在概览里的输入框上时也不动(放下焦点后下一次刷新补画)。
export function render_research_overview() {
  const host = $("research-overview");
  if (!host) return;
  const topic = selectedResearchTopicData();
  const entry = selected_library_entry();
  const key = JSON.stringify([entry?.id || "", entry?.kind, topic.topic, topic.legacy, topic.label, topic.available, topic.storage_root,
    (topic.sources ?? []).length, (topic.findings ?? []).length, (topic.runs ?? []).length, Boolean(topic.report), researchWorkflow, Boolean(researchPlan)]);
  if (host.dataset.sig === key) return;
  const active = document.activeElement;
  if (active && host.contains?.(active) && /^(INPUT|TEXTAREA|SELECT)$/.test(String(active.tagName ?? ""))) return;
  host.dataset.sig = key;
  // 计划面板先放回原位再重建概览:replaceChildren 会把它从文档里摘掉,摘掉后按 id 找不到它,计划就再也画不出来
  // (比如先看了「未绑定课题的对话」再回到正式课题)。
  const plan_panel = $("research-plan-panel"), plan_home = document.querySelector(".research-main");
  if (plan_panel && plan_home && host.contains?.(plan_panel)) plan_home.appendChild(plan_panel);
  host.replaceChildren();
  const unbound = is_unbound(topic);
  const heading = document.createElement("h2");
  heading.textContent = topic.topic || topic.legacy || unbound ? researchTopicLabel(topic) : t("开始一个研究课题");
  const text = document.createElement("p");
  text.className = "dim";
  text.textContent = topic.topic || topic.legacy
    ? t("围绕同一课题组织对话、证据、实验和成果。")
    : unbound ? t("这是没有绑定课题的研究对话，没有文献、实验和成果。可以直接对话，也可以新建一个课题。")
      : t("新建课题，或在左侧选择已有课题。");
  host.append(heading, text);
  if (topic.available === false) {
    text.textContent = `${t("课题目录不可用")}: ${topic.storage_root}`;
    return;
  }
  const create_topic = () => {
    const create = document.createElement("button");
    create.className = unbound ? "ghost" : "primary";
    create.type = "button";
    create.textContent = t("新建课题");
    create.addEventListener("click", () => $("research-topic-new").click());
    return create;
  };
  if (unbound) {
    const chat = document.createElement("button");
    chat.type = "button";
    chat.className = "primary";
    chat.textContent = t("继续课题对话");
    chat.addEventListener("click", () => void open_research_chat());
    host.append(chat, create_topic());
    return;
  }
  if (!topic.topic && !topic.legacy) {
    host.appendChild(create_topic());
    return;
  }
  renderResearchWorkflow(host);
  const rows = [
    ["文献与发现", `${(topic.sources ?? []).length} ${t("来源")} · ${(topic.findings ?? []).length} ${t("发现")}`, "literature"],
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
  // 研究计划就画在概览里:面板节点是 index.html 里原有的那一个(状态、树、批准按钮由 19-research.js 维护),这里只负责摆位置。
  const plan = $("research-plan-panel");
  if (plan) host.appendChild(plan);
  const chat = document.createElement("button");
  chat.type = "button";
  chat.className = "primary";
  chat.textContent = t("继续课题对话");
  chat.addEventListener("click", () => void open_research_chat());
  host.appendChild(chat);
}

// 课题标识可选:留空时按名称生成(英文数字取小写连字符;纯中文等取不出字母就用 topic-<随机 6 位>;重名加序号)。
export function generate_topic_slug(title, taken_slugs = []) {
  const taken = new Set(taken_slugs);
  const ascii = String(title || "").normalize("NFKD").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 40).replace(/-+$/g, "");
  const base = ascii.length >= 3 ? ascii : `topic-${Math.random().toString(36).slice(2, 8).padEnd(6, "0")}`;
  let slug = base;
  for (let n = 2; taken.has(slug); n += 1) slug = `${base}-${n}`;
  return slug;
}

function close_topic_form() {
  $("research-topic-form")?.classList.add("hidden");
  $("research-topic-new")?.focus();
}

defer(() => {
  for (const button of document.querySelectorAll("[data-research-page]")) {
    button.addEventListener("click", () => show_research_page(button.dataset.researchPage));
  }
  $("research-topic-new")?.addEventListener("click", () => {
    $("research-topic-form").classList.toggle("hidden");
    $("research-topic-title").focus();
  });
  $("research-topic-cancel")?.addEventListener("click", () => close_topic_form());
  // Esc 关掉新建表单(此前只能点「取消」)。
  $("research-topic-form")?.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    event.preventDefault?.();
    event.stopPropagation?.();
    close_topic_form();
  });
  $("research-topic-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const button = $("research-topic-submit");
    button.disabled = true;
    $("research-topic-error").textContent = "";
    try {
      const title = $("research-topic-title").value.trim();
      const entered = $("research-topic-slug").value.trim();
      const slug = entered || generate_topic_slug(title, (research_library?.entries ?? researchSnapshot.research_topics ?? []).map((entry) => entry.topic).filter(Boolean));
      const result = await invoke("research_library_create", { topic: slug, title });
      await load_research_library(true);
      if (active_space !== "research") return;
      save_research_workspace({ category: "research", page: "overview" });
      await select_research_topic(result.id);
      $("research-topic-form").reset();
      $("research-topic-form").classList.add("hidden");
      show_research_page("overview");
    } catch (error) {
      $("research-topic-error").textContent = `${t("创建课题失败")}: ${error}`;
    } finally {
      button.disabled = false;
    }
  });
});
