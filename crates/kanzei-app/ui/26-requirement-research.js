// The snapshot validates the artifact; UI never infers completion from a path.
export const needsRequirementResearch = entry => Boolean(entry?.prior_art && !["complete", "waived"].includes(entry.prior_art.status));
export const requirementResearchAction = entry => entry?.prior_art?.status === "invalid" ? "继续调研" : "开始调研";
export function requirementResearchStatus(entry) {
  return ({ pending: "待调研", invalid: "调研待补齐", complete: "调研已完成", waived: "已记录跳过理由" })[entry?.prior_art?.status] || "";
}
export function requirementStart(entry) {
  if (!needsRequirementResearch(entry)) return { prompt: `继续推进 ${entry.id}：${entry.title}`, executionBatch: true, workItemId: entry.id };
  const path = entry.prior_art.path;
  return { executionBatch: false, workItemId: null, prompt: [
    `先完成 ${entry.id}：${entry.title} 的先行调研，再实施。需求已登记，不要重复创建。`,
    `先读 ${entry.id} 的原始描述和现有要求，${path ? `再读取调研文件 ${path}` : "定位该需求的调研文件"}。`,
    "核实真实外部已有实现与仓内既有设计，写明出处、差异和采用决定，遵守检索预算。",
    "需要我回答的问题放到「待我处理」，继续独立工作；不要编造跳过调研的理由。",
    "证据验证通过、必要问题解决后，才领取需求开始实施；验证失败先补齐。",
  ].join("\n") };
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
