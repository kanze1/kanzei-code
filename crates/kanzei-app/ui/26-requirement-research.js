// The snapshot validates the artifact; UI never infers completion from a path.
export const needsRequirementResearch = entry => Boolean(entry?.prior_art && !["complete", "waived"].includes(entry.prior_art.status));
export const requirementResearchAction = entry => entry?.prior_art?.status === "invalid" ? "继续调研" : "开始调研";
export function requirementResearchStatus(entry) {
  return ({ pending: "待调研", invalid: "调研待补齐", complete: "调研已完成", waived: "已记录跳过理由" })[entry?.prior_art?.status] || "";
}
export function requirementStart(entry) {
  if (entry.status === "draft") return { executionBatch: false, workItemId: null,
    prompt: `补充需求草稿 ${entry.id}「${entry.title}」的正文和验收，列出仍需澄清的问题。当前只整理需求。` };
  if (!needsRequirementResearch(entry)) return { prompt: `继续推进 ${entry.id}「${entry.title}」。`, executionBatch: true, workItemId: entry.id };
  return { executionBatch: false, workItemId: null,
    prompt: `推进需求 ${entry.id}「${entry.title}」，补齐仍缺少的调研依据并继续实施。${entry.prior_art.path ? `现有调研文件：${entry.prior_art.path}。` : ""}` };
}
export function requirementResearchRequest(entry) {
  return { executionBatch: false, workItemId: null,
    prompt: `调研需求 ${entry.id}「${entry.title}」的已有实现、相关设计与可采用方案，记录依据、结论和开放问题。${entry.prior_art?.path ? `现有调研文件：${entry.prior_art.path}。` : ""}` };
}
export function registrationFailureDetails(error) {
  const parts = String(error || "").split(/；|\r?\n/).map(part => part.trim()).filter(Boolean);
  return [...new Set(parts)].join("；");
}
export async function openRequirementResearch(project, entry) {
  const path = entry?.prior_art?.path;
  if (!path) return false;
  const [{ navigate_view }, { openFilePreview }] = await Promise.all([import("./03-shell.js"), import("./17-files.js")]);
  const absolute = /^(?:[a-z]:[\\/]|\/|\\\\)/i.test(path) ? path : `${project.replace(/[\\/]+$/, "")}/${path}`;
  navigate_view("files");
  return openFilePreview({ path: absolute });
}
