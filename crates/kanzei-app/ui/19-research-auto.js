import { $, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeProcessId, activeSessionId, currentProject, processItems, running, toast, toastError } from "./03-shell.js";
import { cancelAutoContinueTimer, setAutoPaused, setAutoStopAfterRound, syncAutoRunState } from "./08-auto.js";
import { applyAutoStopToSession, rememberAutoUiState, sendText } from "./08-compose-runtime.js";
import { openFilePreview } from "./17-files.js";
import { research_context_guard, selectedResearchTopicData } from "./19-research.js";
import { open_research_chat } from "./19-research-navigation.js";

export let researchWorkflow = null;
let workflow_error = "";
let panel = null;
let panel_key = "";
let action_pending = false;
const stages = {
  survey: "文献调研", map: "研究地图", choose_direction: "等待选题", design_mvp: "MVP 方案",
  prepare: "实验准备", run_mvp: "MVP 实验", interpret: "结果解读", completed: "MVP 已完成",
};

export function resetResearchWorkflow() {
  researchWorkflow = null;
  workflow_error = "";
  panel = null;
  panel_key = "";
}

export async function refreshResearchWorkflow() {
  const topic = selectedResearchTopicData();
  const is_current = research_context_guard();
  if (!topic.topic || topic.legacy || (topic.kind && topic.kind !== "research")) {
    resetResearchWorkflow();
    return;
  }
  try {
    const value = await invoke("research_workflow_get", { projectDir: currentProject, topic: topic.topic });
    if (!is_current()) return;
    researchWorkflow = value;
    workflow_error = "";
  } catch (error) {
    if (is_current()) workflow_error = `${t("研究流程读取失败")}: ${error}`;
  }
}

function element(tag, text, class_name) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (class_name) node.className = class_name;
  return node;
}

function button(label, callback, primary = false) {
  const node = element("button", t(label), primary ? "primary" : "ghost mini");
  node.type = "button";
  node.disabled = action_pending;
  node.addEventListener("click", callback);
  return node;
}

function artifact_link(host, label, file, topic) {
  if (!file) return;
  host.appendChild(button(label, () => openFilePreview(`.kanzei/research/${topic}/${file}`)));
}

async function continue_research(project, topic) {
  await open_research_chat();
  const process = processItems.find((item) => item.id === activeProcessId);
  if (project !== currentProject || selectedResearchTopicData().topic !== topic
      || process?.profile !== "research" || process.research_topic !== topic || !activeSessionId) return;
  setAutoPaused(false);
  setAutoStopAfterRound(false);
  $("auto-stop-round").checked = false;
  $("auto-continue").checked = true;
  rememberAutoUiState();
  await syncAutoRunState();
  if (project !== currentProject || process.id !== activeProcessId) return;
  if (running) { toast(t("研究将在本轮结束后继续")); return; }
  await sendText(t("继续当前课题的 AUTO research。先回读 research_workflow get 和已有实验记录，按当前阶段推进。"));
}

async function update_workflow(action, direction, budget) {
  if (action_pending) return;
  const topic = selectedResearchTopicData().topic;
  const project = currentProject;
  const revision = researchWorkflow?.revision;
  const is_current = research_context_guard();
  action_pending = true;
  for (const control of panel?.querySelectorAll("button") ?? []) control.disabled = true;
  try {
    const value = action === "start"
      ? await invoke("research_workflow_start", { projectDir: project, topic, ...budget })
      : await invoke("research_workflow_update", { projectDir: project, topic, revision, action, direction, ...budget });
    if (!is_current()) return;
    researchWorkflow = value;
    workflow_error = "";
    if (action === "pause") {
      for (const item of processItems.filter((p) => p.profile === "research" && p.research_topic === topic && (!p.project_dir || p.project_dir === project))) {
        cancelAutoContinueTimer(item.session_id);
        applyAutoStopToSession(item.session_id, { enabled: false });
      }
    }
    if (!["pause", "budget"].includes(action)) await continue_research(project, topic);
  } catch (error) {
    if (is_current()) {
      toastError(`${t("研究流程操作失败")}: ${error}`);
      await refreshResearchWorkflow();
    }
  } finally {
    action_pending = false;
    panel_key = "";
    if (panel?.isConnected) renderResearchWorkflow(panel.parentElement);
  }
}

export function renderResearchWorkflow(host) {
  const topic = selectedResearchTopicData();
  if (!host || !topic.topic || topic.legacy || (topic.kind && topic.kind !== "research")) return;
  const key = JSON.stringify([currentProject, topic.topic, researchWorkflow, workflow_error, action_pending]);
  if (panel && panel_key === key) { host.appendChild(panel); return; }
  panel?.remove();
  panel = element("section", undefined, "research-auto-panel");
  panel.id = "research-auto-panel";
  panel.setAttribute("aria-label", t("AUTO research"));
  panel_key = key;
  panel.appendChild(element("h3", t("AUTO research")));
  if (workflow_error) {
    panel.appendChild(element("p", workflow_error, "error"));
    host.appendChild(panel);
    return;
  }
  const state = researchWorkflow;
  if (!state) {
    panel.appendChild(element("p", t("自动调研并绘制研究地图，等待你选方向，再推进到 MVP 结果解读。")));
    const form = element("form", undefined, "research-auto-budget");
    const inputs = {};
    for (const [name, label, value, min, max] of [
      ["rounds", "检索轮次", 3, 1, 20], ["tokens", "检索证据预算", 16000, 1000, 1000000], ["runs", "实验次数预算（含基线）", 4, 2, 100],
    ]) {
      const wrap = element("label", t(label));
      const input = document.createElement("input");
      Object.assign(input, { type: "number", name, value: String(value), min: String(min), max: String(max), required: true });
      input.disabled = action_pending;
      wrap.appendChild(input);
      inputs[name] = input;
      form.appendChild(wrap);
    }
    const start = element("button", t("启动 AUTO research"), "primary");
    start.type = "submit";
    start.disabled = action_pending;
    form.appendChild(start);
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (!form.reportValidity()) return;
      void update_workflow("start", undefined, { budget: { max_rounds: Number(inputs.rounds.value), max_tokens: Number(inputs.tokens.value), max_concurrency: 2 }, maxMvpRuns: Number(inputs.runs.value) });
    });
    panel.appendChild(form);
  } else {
    const flow = element("ol", undefined, "research-auto-stages");
    for (const [id, label] of Object.entries(stages)) {
      const item = element("li", t(label));
      if (id === state.stage) item.setAttribute("aria-current", "step");
      flow.appendChild(item);
    }
    panel.appendChild(flow);
    if (state.paused || state.waiting_reason) panel.appendChild(element("p", state.waiting_reason || t("研究已暂停")));
    const artifacts = element("div", undefined, "research-auto-actions");
    artifact_link(artifacts, "阅读调研", state.survey, topic.topic);
    artifact_link(artifacts, "打开研究地图", state.map, topic.topic);
    artifact_link(artifacts, "查看 MVP 方案", state.mvp?.protocol, topic.topic);
    artifact_link(artifacts, "阅读结果解读", state.interpretation, topic.topic);
    panel.appendChild(artifacts);
    if (state.stage === "choose_direction") {
      panel.appendChild(element("p", t("选择一个方向后继续；研究价值、疑点与成本都列在下方。")));
      panel.appendChild(element("p", topic.label || topic.topic, "research-auto-map-root"));
      const list = element("div", undefined, "research-auto-directions");
      for (const direction of state.directions) {
        const row = element("article");
        row.dataset.direction = direction.id;
        row.append(element("h4", direction.title), element("p", direction.question));
        const details = element("dl");
        for (const [label, value] of [["研究依据", direction.rationale], ["尚待验证", direction.uncertainty], ["预计成本", direction.cost], ["最小验证", direction.validation], ["来源", direction.source_ids.join(", ")]]) {
          details.append(element("dt", t(label)), element("dd", value));
        }
        row.append(details, button("选择并继续", () => void update_workflow("select", direction.id), true));
        list.appendChild(row);
      }
      panel.appendChild(list);
    } else if (state.stage === "completed") {
      const verdicts = { supported: "支持假设", rejected: "否定假设", inconclusive: "证据不足" };
      panel.appendChild(element("p", t(verdicts[state.verdict] || "MVP 已完成")));
      panel.appendChild(element("p", t("本次 MVP 已收敛。下一步可根据结论规划完整实验。"), "dim"));
    } else {
      if (state.mvp) panel.appendChild(element("p", state.mvp.question));
      if (state.baseline_result) panel.appendChild(element("p", `${t("基线记录")}: ${state.baseline_result}`));
      if (state.result_ids.length) panel.appendChild(element("p", `${t("实验记录")}: ${state.result_ids.join(", ")}`));
      const actions = element("div", undefined, "research-auto-actions");
      actions.appendChild(button("继续研究", () => void update_workflow("resume"), true));
      if (!state.paused) actions.appendChild(button("本轮后暂停研究", () => void update_workflow("pause")));
      panel.appendChild(actions);
    }
    if (state.stage !== "completed") {
      const details = element("details");
      details.appendChild(element("summary", `${t("实验次数预算（含基线）")}: ${state.max_mvp_runs}`));
      const form = element("form", undefined, "research-auto-budget");
      const label = element("label", t("实验次数预算（含基线）"));
      const input = document.createElement("input");
      Object.assign(input, { type: "number", min: "2", max: "100", value: String(state.max_mvp_runs), required: true });
      label.appendChild(input);
      const submit = element("button", t("更新实验预算"), "ghost");
      submit.type = "submit";
      submit.disabled = action_pending;
      form.append(label, submit);
      form.addEventListener("submit", (event) => {
        event.preventDefault();
        if (form.reportValidity()) void update_workflow("budget", undefined, { maxMvpRuns: Number(input.value) });
      });
      details.appendChild(form);
      panel.appendChild(details);
    }
  }
  host.appendChild(panel);
}
