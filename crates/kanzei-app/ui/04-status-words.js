// 状态词映射表(UX-070 / C9):界面里凡是把后端状态枚举(todo/doing/open/active/shadow…)
// 摆给人看的地方,一律经这张表出中文(英文界面出英文),全站只此一份。
// 原先中文模式直接把枚举原样显示(「→ 转 done」),另有两三处各抄了一套自己的中文
// (进行中/修复中/待开始;推进中/待继续),同一状态在不同页面叫法对不上。
//
// 只做**显示**:选项的 value、CSS 类(st-todo)、IPC 参数一律保持原始枚举,不要拿显示词回传后端。
// 本模块不 import 任何东西——02-i18n.js 要调它(localizedDocStatus),再反向依赖会成环。
//
// 用法:
//   statusWord("doing")            → "进行中"
//   statusWord("doing", true)      → "In progress"
//   statusWord("active", false, "unit") → "开发中"  (执行单元的 active 与记忆的 active 不是一回事)
//   未登记的状态原样返回(不吞信息)。
export const STATUS_WORDS_ZH = Object.freeze({
  // 需求 / 缺陷 / 想法台账
  draft: "草稿",
  todo: "待开始",
  doing: "进行中",
  done: "已完成",
  dropped: "已放弃",
  open: "未解决",
  fixing: "修复中",
  awaiting_external: "待外部验收",
  fixed: "已修复",
  wontfix: "不修复",
  inbox: "待整理",
  split: "已拆解",
  blocked: "受阻",
  // 记忆条目状态与分类(memory 包共用)
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
  // 测试记录
  passed: "通过",
  failed: "失败",
  running: "运行中",
  skipped: "已跳过",
});

export const STATUS_WORDS_EN = Object.freeze({
  draft: "Draft",
  todo: "To do",
  doing: "In progress",
  done: "Done",
  dropped: "Dropped",
  open: "Open",
  fixing: "Fixing",
  awaiting_external: "Awaiting external acceptance",
  fixed: "Fixed",
  wontfix: "Won't fix",
  inbox: "Inbox",
  split: "Split",
  blocked: "Blocked",
  active: "Active",
  candidate: "Candidate",
  shadow: "Shadow",
  stale: "Stale",
  archived: "Archived",
  deprecated: "Archived",
  invalid: "Disproved",
  fact: "Fact",
  sop: "Procedure",
  habit: "Habit",
  preference: "Preference",
  passed: "Passed",
  failed: "Failed",
  running: "Running",
  skipped: "Skipped",
});

// 作用域覆盖:同一个枚举在别的语境里是另一个意思。目前只有执行单元(work unit)用。
export const STATUS_WORD_SCOPES = Object.freeze({
  unit: Object.freeze({
    zh: Object.freeze({ ready: "待执行", active: "开发中", blocked: "阻塞", verifying: "机器验证中", done: "机器已完成", superseded: "已替代" }),
    en: Object.freeze({ ready: "Ready", active: "In development", blocked: "Blocked", verifying: "Verifying", done: "Machine done", superseded: "Superseded" }),
  }),
});

// 运行阶段名:后端 stage 事件与线路 phase 直接给中文字面量(配置/复核/空闲…),英文界面经这张表出英文;
// 表里没有的阶段原样显示。中文界面不查表。
export const STAGE_WORDS_EN = Object.freeze({
  配置: "Config", 权限: "Permission", 对话: "Conversation", 压缩: "Compacting", 上下文: "Context", 重试: "Retrying",
  屏障: "Barrier", 协作受阻: "Collaboration blocked", 勘察: "Scouting", 领取: "Claiming", 请求: "Requesting",
  实现: "Implementation", 复核: "Review", 修正: "Fixing", 复验: "Re-verifying", 推送: "Pushing", 空闲: "Idle",
  等待模型: "Waiting for model", 生成中: "Generating", 思考中: "Thinking", 工具执行中: "Running tool", 运行中: "Running",
  出错: "Error", 工作树不可用: "Worktree unavailable",
  // 执行活动标签(25-softwire-model.js 的 activityLabels)也走这张表。
  启动中: "Starting", 停止中: "Stopping", 等待下一轮: "Waiting for next round", 待你回复: "Needs your reply",
  已停止: "Stopped", 运行失败: "Run failed", 状态未确认: "Status unconfirmed",
});
export function stageWord(stage, english = false) {
  const key = String(stage ?? "");
  return english ? (STAGE_WORDS_EN[key] ?? key) : key;
}

/// 状态枚举 → 展示词。english 由调用方按当前界面语言传(02-i18n 的 languageIsEnglish())。
export function statusWord(status, english = false, scope = "") {
  const key = String(status ?? "");
  if (!key) return "";
  const scoped = STATUS_WORD_SCOPES[scope]?.[english ? "en" : "zh"]?.[key];
  if (scoped) return scoped;
  return (english ? STATUS_WORDS_EN : STATUS_WORDS_ZH)[key] ?? key;
}
