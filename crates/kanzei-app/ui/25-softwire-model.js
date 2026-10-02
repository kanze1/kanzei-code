// Pure identity and projection helpers, shared by browser checks and the workspace.
import { stripInternalHandoff } from "./04-structured-parse.js";
export const moduleNames = Object.freeze({
  work: "需求与计划", batch: "验证与交付", main: "主对话", memory: "记忆",
  tools: "工具管理", tasks: "子任务", history: "历史与压缩", inbox: "待我处理",
});
// 概览里六个模块的行名(竖向堆叠,每行一条基线);第 7 行「运行用量」另有自己的渲染。
export const laneNames = Object.freeze({ work: "需求", memory: "记忆", history: "历史", tools: "工具", batch: "验证交付", tasks: "子任务", usage: "运行用量" });
// 行状态 → 基线表现:活动态画橙色起伏波形;等待/停止中虚线;需要你/受阻琥珀;失败红;其余中性灰直线。
export const laneActiveStates = Object.freeze(["running", "working", "reading", "writing", "starting"]);
export const pathKey = value => String(value || "").replaceAll("\\", "/").replace(/^\/\/\?\//, "").replace(/\/$/, "").toLowerCase();
export const sameProject = (a, b) => Boolean(a && b) && pathKey(a) === pathKey(b);
// Runtime activity is independent of navigation, loop policy and delivery stages.
export const activityLabels = Object.freeze({ idle: "空闲", starting: "启动中", running: "运行中", stopping: "停止中",
  waiting: "等待下一轮", attention: "待你回复", stopped: "已停止", failed: "运行失败", unknown: "状态未确认" });
export const activitySymbols = Object.freeze({ idle: "○", starting: "◌", running: "●", stopping: "■", waiting: "Ⅱ",
  attention: "?", stopped: "■", failed: "!", unknown: "—" });
export function executionActivity(line, live, { waiting = false, unavailable = false } = {}) {
  const authoritative = live && (live.converged || live.running || live.auto_pending || live.local_start_pending);
  const phase = authoritative ? live.phase : unavailable ? "unknown" : line?.running ? "running" : "idle";
  const state = ["stopping", "starting"].includes(phase) ? phase : waiting ? "attention"
    : phase === "auto_pending" ? "waiting" : activityLabels[phase] ? phase : "idle";
  const stage = authoritative ? live.stage : line?.stage;
  return { state, label: state === "running" && stage && stage !== "空闲" && !/权限|permission/i.test(stage) ? stage : activityLabels[state] };
}
export function targetKey(target) {
  return JSON.stringify([pathKey(target.project), target.processId || "", target.sessionId || "", target.module,
    target.objectId || "", target.interactionId || ""]);
}
// Management views share the actual agent conversation; only their drafts differ.
export const conversationKey = target => targetKey({ ...target, module: "main", objectId: "", interactionId: "" });
// Storage/transport stays "decision"; a missing fact is presented and answered as a question.
export function interactionKind(message) {
  return message.kind === "decision" && message.source?.status === "needs_input" ? "question" : message.kind;
}
export function isLegacyReviewPlaceholder(message, text) {
  if (message.kind !== "decision" || interactionKind(message) !== "question") return false;
  const reply = text.trim();
  return /^(本次通过|Accept this decision|需要修改[：:]?|Needs changes[：:]?)$/i.test(reply)
    && !(message.choices || []).some(option => (typeof option === "string" ? option : option.label) === reply);
}
export function isMultiReply(message) {
  return interactionKind(message) === "question" && (message.source?.multiple === true
    || message.source?.multiple == null && /多选|multi[- ]?select/i.test(message.body || ""));
}
export function selectWork(project, processId) {
  const items = project?.current_items || [];
  const lines = (project?.lines || []).filter(l => l.profile !== "research");
  const line = lines.find(l => l.id === processId) || lines.find(l => l.running) || lines[0] || null;
  const item = line ? items.find(i => i.id === line.current_item_id || i.claimed_by === line.id
    || (i.owner_lines || []).some(l => l.id === line.id)) || null : null;
  const units = (project?.work_units || []).filter(u => line && u.claimed_by === line.id && (!item || u.requirement_id === item.id));
  const unit = units.find(u => ["active", "verifying", "blocked"].includes(u.status)) || units.at(-1) || null;
  return { item, unit, line };
}
export function inboxFromProjects(projects, questions = []) {
  const rows = questions.map(q => ({
    key: JSON.stringify(["question", pathKey(q.projectDir), q.sessionId, q.id, q.revision]), kind: "question",
    project: q.projectDir, sessionId: q.sessionId, id: q.id, revision: q.revision, body: q.question,
    choices: q.options || [], status: "pending", source: q,
  }));
  for (const project of projects || []) {
    for (const d of project.decisions || []) {
      if (d.status === "deciding" || d.review) continue;
      rows.push({ key: JSON.stringify(["decision", pathKey(project.path), d.id, d.revision]), kind: "decision",
        project: project.path, sessionId: d.session_id, processId: d.process_id, id: d.id,
        revision: d.revision, body: d.question, status: "pending", choices: d.status === "needs_input" ? d.options || [] : [],
        source: d, answer: d.resolution?.answer || d.missing_fact || "" });
    }
    for (const u of project.work_units || []) {
      if (u.status !== "done" || (project.work_acceptances || []).some(a => a.unit_id === u.unit_id && a.source_sequence === u.source_sequence)
        || (project.decisions || []).some(d => d.work_unit_id === u.unit_id && d.review?.action === "correct")) continue;
      rows.push({ key: JSON.stringify(["delivery", pathKey(project.path), u.unit_id, u.source_sequence]), kind: "delivery",
        project: project.path, id: u.unit_id, revision: u.source_sequence, body: u.objective,
        processId: u.claimed_by, sessionId: project.lines?.find(l => l.id === u.claimed_by)?.session_id,
        status: "pending", choices: [], source: u });
    }
  }
  return rows;
}
export function receiptLabel(status) {
  return ({ sending: "发送中", queued: "已排队 · 本轮后执行", resumed: "已恢复原对话", delivered: "已送达", reviewed: "已复核",
    failed: "发送失败", expired: "已失效", completed: "已处理" })[status] || status || "";
}

// 概览里的回复预览只给人读的字:模型回复是 Markdown,直接当纯文本会露出「## 」「**」「|---|」(UX-108)。
// 围栏代码与表格行整段略去(一行预览里读不出意思),标题/列表/引用记号、加粗斜体、行内代码和链接只留文字。
// 先过一遍内部交接块过滤(与对话页同源:04-structured-parse.js 的 stripInternalHandoff),
// 概览的「最近回复」「最新一句」与对话页读到的是同一份人话(UX-008/037)。
export function plainText(markdown) {
  const kept = stripInternalHandoff(String(markdown ?? "").replace(/\r\n?/g, "\n")).replace(/```[\s\S]*?(?:```|$)/g, "\n").split("\n")
    .filter(line => !/^\s*\|/.test(line) && !/^\s*([-*_])(\s*\1){2,}\s*$/.test(line));
  return kept.join("\n")
    .replace(/!?\[([^\]\n]*)\]\([^)\n]*\)/g, "$1")
    .replace(/<\/?[a-z][^>\n]*>/gi, "")
    .replace(/(\*\*|__)(?=\S)([^\n]*?\S)\1/g, "$2")
    .replace(/(^|[\s(（])\*(?=\S)([^*\n]*?\S)\*(?=$|[\s)），。,.;:!?！？])/g, "$1$2")
    .replace(/~~([^~\n]+)~~/g, "$1")
    .replace(/`+([^`\n]*)`+/g, "$1")
    .replace(/^[ \t]*(?:#{1,6}|>+|[-*+]|\d+[.)])[ \t]+/gm, "")
    .replace(/[ \t]+/g, " ").replace(/\n{2,}/g, "\n").trim();
}
// 验证与交付页的证据详情不再直吐 JSON(UX-108):对象按「标签：值」列出,数组按条目列出,时间戳转本地时间。
// 已知字段用中文标签,不认识的字段保留原名(宁可难看也不吞信息)。
const recordLabels = Object.freeze({
  summary: "摘要", next_action: "下一步", base_revision: "基线版本", objective: "目标", scope: "范围", acceptance: "验收", verification: "验证",
  evidence: "依据", evidence_refs: "依据", commit: "提交", entry_id: "条目", run_id: "运行", created_at: "记录时间", kind: "类别", target: "对象",
  status: "状态", paths: "涉及文件", test_record_ids: "测试记录", source: "来源", handoff_target: "交付对象", handoff_scope: "交付范围",
  id: "编号", reason: "原因", result: "结果", unit_id: "工作单元", message: "说明",
});
const recordLabelsEn = Object.freeze({
  summary: "Summary", next_action: "Next step", base_revision: "Base revision", objective: "Objective", scope: "Scope", acceptance: "Acceptance", verification: "Verification",
  evidence: "Basis", evidence_refs: "Basis", commit: "Commit", entry_id: "Item", run_id: "Run", created_at: "Recorded at", kind: "Kind", target: "Target",
  status: "Status", paths: "Files involved", test_record_ids: "Test records", source: "Source", handoff_target: "Delivery target", handoff_scope: "Delivery scope",
  id: "ID", reason: "Reason", result: "Result", unit_id: "Work unit", message: "Message",
});
const recordValues = Object.freeze({ git_checkpoint: "Git 提交", handoff: "交付记录", declared: "已登记", recorded: "已记录" });
const recordValuesEn = Object.freeze({ git_checkpoint: "Git commit", handoff: "Delivery record", declared: "Registered", recorded: "Recorded" });
// english = true 时标签与已知取值出英文(调用方传 languageIsEnglish();本模块保持纯函数,不 import i18n)。
export function readableText(value, indent = "", english = false) {
  if (value == null || value === "") return "";
  if (typeof value !== "object") return indent + String(value);
  if (Array.isArray(value)) {
    const values = english ? recordValuesEn : recordValues;
    return value.filter(item => item != null && item !== "").map(item => indent + "• " + (typeof item === "object" ? readableText(item, indent + "  ", english).trimStart() : (values[item] || item))).join("\n");
  }
  return Object.entries(value).filter(([, item]) => item != null && item !== "" && !(Array.isArray(item) && !item.length)).map(([key, item]) => {
    const label = (english ? recordLabelsEn : recordLabels)[key] || key, colon = english ? ": " : "：";
    if (typeof item === "object") return indent + label + colon.trimEnd() + "\n" + readableText(item, indent + "  ", english);
    const shown = /_at$/.test(key) && Number.isFinite(Number(item)) ? new Date(Number(item)).toLocaleString() : (english ? recordValuesEn : recordValues)[item] || item;
    return indent + label + colon + shown;
  }).join("\n");
}

// 概览里展示给人的回复正文:去掉内部交接块;整条回复只剩交接块时为空串。
export const visibleReply = text => stripInternalHandoff(text).trim();

export function latestSentence(text) {
  const sentences = plainText(text).match(/[^。！？!?\n]+[。！？!?]?/g) || [];
  const tail = (sentences.at(-1) || "").trim().split(/(?<=[.])\s+(?=[A-Z])/).at(-1).replace(/^\s*[-#*>]+\s*/, "");
  return tail.length > 220 ? "…" + tail.slice(-220) : tail;
}

// 运行用量:run_metrics 返回新→旧的轮次(at/steps/inputTokens/outputTokens/tools/metrics),图要左旧右新。
// 早于度量落地的轮次 metrics 为 {}(measured=false):步数与 token 仍真实,调用数退回 tools 之和。
export function usageSeries(rounds, limit = 20) {
  return (Array.isArray(rounds) ? rounds : []).filter(round => round && Number.isFinite(Number(round.steps))).slice(0, limit).reverse().map(round => {
    const input = Number(round.inputTokens) || 0, output = Number(round.outputTokens) || 0, metrics = round.metrics || {};
    const toolCalls = Object.values(round.tools || {}).reduce((sum, count) => sum + (Number(count) || 0), 0);
    return { at: round.at, outcome: round.outcome, steps: Number(round.steps) || 0, input, output, tokens: input + output,
      calls: Number.isFinite(Number(metrics.total_calls)) && round.measured !== false ? Number(metrics.total_calls) : toolCalls,
      failed: Number(metrics.failed_calls) || 0 };
  });
}
export function usageSummary(series) {
  const count = series.length, mean = key => count ? series.reduce((sum, row) => sum + row[key], 0) / count : 0;
  return { count, steps: mean("steps"), tokens: mean("tokens"), output: mean("output"), calls: mean("calls"), failed: series.reduce((sum, row) => sum + row.failed, 0) };
}
// 迷你波形:点与点之间一律直线段(不画贝塞尔);全相等画水平线,单点画一小段。
export function sparkPoints(values, width, height, pad = 3) {
  const list = values.filter(value => Number.isFinite(value));
  if (!list.length) return [];
  const min = Math.min(...list), span = Math.max(...list) - min;
  const x = index => list.length === 1 ? width / 2 : pad + index * (width - 2 * pad) / (list.length - 1);
  const y = value => span ? height - pad - (value - min) / span * (height - 2 * pad) : height / 2;
  return list.map((value, index) => [Math.round(x(index) * 10) / 10, Math.round(y(value) * 10) / 10]);
}
export function sparkPath(values, width, height, pad = 3) {
  const points = sparkPoints(values, width, height, pad);
  if (points.length === 1) return `M${points[0][0] - 8} ${points[0][1]} L${points[0][0] + 8} ${points[0][1]}`;
  return points.map(([x, y], index) => (index ? "L" : "M") + x + " " + y).join(" ");
}
export function formatCount(value) {
  const n = Number(value) || 0;
  return n >= 1e6 ? (n / 1e6).toFixed(1) + "M" : n >= 1e4 ? Math.round(n / 1e3) + "k" : n >= 1e3 ? (n / 1e3).toFixed(1) + "k" : String(Math.round(n * 10) / 10);
}
export function safeRestore(text) {
  try {
    const saved = JSON.parse(text || "{}");
    if (!saved || saved.version !== 1 || !Array.isArray(saved.drafts)) return { drafts: [] };
    return { drafts: saved.drafts.filter(x => Array.isArray(x) && typeof x[0] === "string" && typeof x[1] === "string").slice(-100),
      position: saved.position && Number.isFinite(saved.position.x) && Number.isFinite(saved.position.y) ? saved.position : null, floating: saved.floating === true };
  } catch { return { drafts: [] }; }
}
