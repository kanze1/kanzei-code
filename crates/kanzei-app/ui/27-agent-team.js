import { defer, invoke, on } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeProcessId, activeSessionId, currentProject } from "./03-shell.js";
import { onSubagentChange, upsertManagedAgent } from "./05-subagents.js";

import { agentAuditManagedJob } from "./06-activity.js";

const drafts = new Map();
const controls = new WeakMap();
const node = (tag, className, text = "") => {
  const el = document.createElement(tag); el.className = className; el.textContent = text; return el;
};
export async function agentCommand(run, input) {
  const job = run.managed;
  return invoke("agent_team_command", { projectDir: job.project_dir, processId: job.process_id, input: { id: job.id, ...input } });
}
export async function restoreAgent(id, owner) {
  if (!currentProject || owner !== activeSessionId) return false;
  const project = currentProject, processId = activeProcessId;
  const result = await invoke("agent_team_command", { projectDir: project, processId, input: { action: "get", id } });
  if (currentProject !== project || activeProcessId !== processId || activeSessionId !== owner || result.job?.owner !== owner) return false;
  upsertManagedAgent(result.job, { replay: true });
  return true;
}
export function mountAgentControls(host, run) {
  if (!run.managed) return;
  const panel = node("section", "team-controls");
  const facts = node("div", "team-facts");
  const actions = node("div", "team-actions");
  const diff = node("button", "ghost mini", t("查看改动")); diff.type = "button";
  const adopt = node("button", "ghost mini", t("采纳改动")); adopt.type = "button";
  const restart = node("button", "ghost mini", t("重新派发")); restart.type = "button";
  restart.title = t("用原任务目标创建新子任务，保留这次的记录和改动");
  const patch = node("pre", "team-diff"); patch.hidden = true;
  const messages = node("div", "team-messages"); messages.setAttribute("aria-live", "polite");
  const form = node("form", "team-reply");
  const label = node("label", "team-recipient", `${t("发给")} ${run.description || run.agent}`);
  const input = node("textarea", "team-reply-input"); input.rows = 2; input.placeholder = t("补充要求，或让这个子任务继续…"); input.setAttribute("aria-label", label.textContent);
  input.value = drafts.get(run.key) || "";
  input.addEventListener("input", () => drafts.set(run.key, input.value));
  const send = node("button", "primary team-send", t("发送")); send.type = "submit";
  const status = node("span", "team-send-status"); status.setAttribute("role", "status");
  const footer = node("div", "team-reply-footer"); footer.append(status, send);
  label.append(input); form.append(label, footer);
  actions.append(diff, adopt, restart); panel.append(facts, actions, patch, messages);
  (host.querySelector(".sa-detail-scroll") || host).append(panel);
  const dock = node("div", "team-reply-dock"); dock.append(form); host.append(dock);
  const view = { facts, diff, adopt, restart, patch, messages, form, input, send, status, busy: false, seenHead: null, sentMessageId: null };
  controls.set(run, view);
  restart.addEventListener("click", async () => {
    if (view.busy) return;
    view.busy = true; view.sentMessageId = null; status.textContent = t("正在派发…"); refreshControls(run);
    const text = input.value.trim();
    try {
      await agentCommand(run, { action: "restart", prompt: text || undefined });
      if (text && input.value.trim() === text) { input.value = ""; drafts.delete(run.key); }
      status.textContent = t("已派发新任务；本次记录已保留");
      await refreshAgents();
    } catch (error) { status.textContent = String(error); }
    finally { view.busy = false; refreshControls(run); }
  });
  diff.addEventListener("click", async () => {
    view.sentMessageId = null;
    diff.disabled = true;
    const head = run.managed.head;
    try {
      const result = await agentCommand(run, { action: "diff" });
      patch.textContent = result.diff || t("没有文件改动"); patch.hidden = false;
      view.seenHead = run.managed.head === head ? head : null;
    }
    catch (error) { status.textContent = String(error); }
    finally { refreshControls(run); }
  });
  adopt.addEventListener("click", async () => {
    if (view.busy) return; view.busy = true; view.sentMessageId = null; refreshControls(run);
    try { const result = await agentCommand(run, { action: "adopt" }); upsertManagedAgent(result); status.textContent = t("已采纳，主对话仍需验证"); }
    catch (error) { status.textContent = String(error); }
    finally { view.busy = false; refreshControls(run); }
  });
  const submit = async () => {
    const text = input.value.trim(); if (!text || view.busy) return;
    view.busy = true; view.sentMessageId = null; status.textContent = t("发送中…"); refreshControls(run);
    try {
      const receipt = await agentCommand(run, { action: "message", prompt: text });
      view.sentMessageId = receipt.message_id;
      if (input.value.trim() === text) { input.value = ""; drafts.delete(run.key); }
      status.textContent = t("已送达，下一次模型请求前处理");
      await refreshAgents();
    } catch (error) { status.textContent = String(error); }
    finally { view.busy = false; refreshControls(run); }
  };
  form.addEventListener("submit", event => { event.preventDefault(); void submit(); });
  input.addEventListener("keydown", event => { if (event.key === "Enter" && !event.shiftKey && !event.isComposing) { event.preventDefault(); void submit(); } });
  refreshControls(run);
}
function refreshControls(run) {
  const view = controls.get(run); if (!view) return;
  const job = run.managed;
  const changed = (job.files || []).length;
  view.facts.textContent = [job.worktree ? t("独立工作树") : ["general", "implement", "verify"].includes(job.role) ? t("准备工作树") : t("只读"), changed ? `${changed} ${t("个文件")}` : "", job.outcome === "adopted" ? t("已采纳 · 待主对话验证") : job.outcome === "candidate" ? t("待检查与整合") : "", job.depends_on?.length ? `${job.depends_on.length} ${t("个依赖")}` : ""].filter(Boolean).join(" · ");
  view.diff.hidden = !job.head; view.diff.disabled = view.busy;
  view.adopt.hidden = !job.head || !changed || job.outcome === "adopted";
  view.adopt.disabled = view.busy || view.seenHead !== job.head || job.state !== "done";
  view.adopt.title = view.seenHead === job.head ? t("将子任务改动应用到当前工作区") : t("先查看改动");
  view.send.disabled = view.busy || job.outcome === "adopted";
  view.input.disabled = job.outcome === "adopted";
  view.restart.disabled = view.busy || ["queued", "waiting", "waiting_user", "running", "stopping"].includes(job.state);
  view.send.textContent = ["done", "failed", "interrupted", "stopped", "blocked"].includes(job.state) ? t("续做") : t("发送");
  const receipt = (job.messages || []).find(message => message.id === view.sentMessageId);
  if (receipt && !view.busy) view.status.textContent = ({ queued: t("已送达，等待接收"), received: t("已接收，正在处理"), processed: t("已处理") })[receipt.state] || receipt.state;
  const replies = (job.messages || []).slice(1).slice(-4);
  const signature = JSON.stringify(replies);
  if (view.messages.dataset.signature !== signature) {
    view.messages.dataset.signature = signature; view.messages.replaceChildren();
    for (const reply of replies) {
      const row = node("div", "team-message");
      row.append(node("span", "", reply.text), node("small", "muted", ({ queued: t("待接收"), received: t("已接收"), processed: t("已处理") })[reply.state] || reply.state));
      view.messages.append(row);
    }
  }
}
let refreshing = false;
export async function refreshAgents() {
  if (!currentProject || !activeSessionId || refreshing || document.hidden) return;
  const project = currentProject, processId = activeProcessId, owner = activeSessionId;
  refreshing = true;
  try {
    const jobs = await invoke("agent_team_command", { projectDir: project, processId, input: { action: "list" } });
    for (const job of Array.isArray(jobs) ? jobs : []) if (job.owner === owner) upsertManagedAgent(job, { replay: true });
  } finally { refreshing = false; }
}
defer(() => {
  onSubagentChange(run => { if (run?.managed) { refreshControls(run); agentAuditManagedJob(run.managed); } });
  on("kz:agent-job", event => {
    if (event.payload.job?.owner === event.payload.sessionId) upsertManagedAgent(event.payload.job);
  });
  for (const name of ["kz:conversation-selected", "kz:view-changed"]) document.addEventListener(name, () => { void refreshAgents().catch(error => console.warn(error)); });
  setInterval(() => { void refreshAgents().catch(error => console.warn(error)); }, 3000);
});
