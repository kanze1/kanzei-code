import { $, defer } from "./00-dom.js";
import { invoke } from "./00-ipc.js";
import { t } from "./02-i18n.js";
import { memoryProject } from "./03-memory-scope.js";
import { toast, toastError } from "./03-shell.js";

const MEMORY_JOB_REFRESH_MS = 1000;
let current = null;
let inFlight = null;

export function memoryJobActive(project) {
  return current?.project === project && ["running", "cancelling"].includes(current?.job?.status);
}

function render() {
  const progress = $("memory-job-progress"), stop = $("memory-consolidate-stop");
  if (!progress || !stop) return;
  const data = current?.project === memoryProject ? current : null;
  progress.hidden = !data?.job && !data?.error;
  stop.hidden = !data?.can_stop || !memoryJobActive(memoryProject);
  stop.disabled = data?.job?.status === "cancelling";
  if (data?.error) { progress.textContent = `${t("作业进度读取失败")}: ${data.error}`; return; }
  if (!data?.job) return;
  const job = data.job;
  const labels = { running: t("整理中…"), cancelling: t("正在停止整理"), completed: t("整理已完成"),
    cancelled: t("整理已停止，可继续"), incomplete: t("整理未完成，可继续"), failed: t("整理失败"), interrupted: t("上次整理中断，可继续") };
  const model = job.model ? `${job.provider}:${job.model}` : ["running", "cancelling"].includes(job.status) ? t("正在准备模型") : t("未调用模型");
  const usage = job.usage_reported ? `${job.input_tokens} in · ${job.output_tokens} out tokens` : job.steps ? t("用量未报告") : "";
  progress.textContent = [labels[job.status] || job.status, model, `${job.steps} ${t("步")}`, usage, job.error].filter(Boolean).join(" · ");
}

export async function refreshMemoryJobStatus() {
  if (inFlight) return inFlight;
  const project = memoryProject;
  if (!project || document.body.dataset.view !== "memory") { render(); return; }
  const request = invoke("memory_job_status", { projectDir: project }).then(data => {
    if (project === memoryProject) current = { project, ...data };
  }).catch(error => {
    if (project === memoryProject) current = { project, error: String(error) };
  }).finally(() => {
    inFlight = null;
    render();
    document.dispatchEvent(new CustomEvent("kz:memory-job-state", { detail: { project } }));
  });
  inFlight = request;
  return request;
}

defer(() => {
  $("memory-consolidate-stop")?.addEventListener("click", async () => {
    const state = current;
    if (state?.project !== memoryProject || !state.can_stop || !state.job) return;
    try {
      const result = await invoke("memory_job_stop", { projectDir: state.project, jobId: state.job.id });
      toast(result.stopped ? t("已请求停止整理") : t("整理作业已经结束"));
      await refreshMemoryJobStatus();
    } catch (error) { toastError(`${t("停止整理失败")}: ${error}`); }
  });
  setInterval(() => { if (document.body.dataset.view === "memory") void refreshMemoryJobStatus(); }, MEMORY_JOB_REFRESH_MS);
});
