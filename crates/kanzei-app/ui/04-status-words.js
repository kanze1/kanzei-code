// 状态词映射表(纯简体中文):界面里展示后端状态枚举时一律经这张表转中文,全站单源。
// 只做显示:选项 value、CSS 类、IPC 参数保持原始枚举。未登记的状态原样返回。
export const STATUS_WORDS_ZH = Object.freeze({
  // 记忆条目状态与分类
  active: "启用",
  candidate: "候选",
  shadow: "试运行",
  stale: "已失效",
  archived: "已归档",
  deprecated: "已归档",
  invalid: "已证伪",
  fact: "事实",
  sop: "流程",
  habit: "习惯",
  preference: "偏好",
  // 通用运行状态
  passed: "通过",
  failed: "失败",
  running: "运行中",
  skipped: "已跳过",
  unknown: "无法判定",
});

export function statusWord(status) {
  const key = String(status ?? "");
  if (!key) return "";
  return STATUS_WORDS_ZH[key] ?? key;
}
