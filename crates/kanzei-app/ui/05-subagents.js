// UI-0926 #8 子代理呈现(docs/design/subagent_presentation.md):一次委派 = 主对话里的一张卡。
//
// 跑的时候看得见它在干什么(人格 · 描述 · 实时计数 · 最近 ≤3 次工具),跑完收成一行,点卡头就地
// 展开「指令 / 过程 / 结果」,↗ 在侧栏(06-agent-panel.js)看完整版。同一批并行派发合成一组。
// 实时事件与历史回放(消息历史 + run.trace 里落库的 task-progress)喂同一个数据模型,两边同形。
//
// 数据只在这里:runsBySession(侧栏列表)与 liveIndex(`${sessionId}|${id}` → 最新一次委派)。
// DOM 只是投影:卡片节点被裁剪/清空后,模型照常推进,侧栏照常可看。
import { $, activePane, appendToPane, defer, invoke, motionOnce, motionSync, renderingBackground } from "./01-core.js";
import { languageIsEnglish, t } from "./02-i18n.js";
import { activeProcessId, activeSessionId, currentProject, processItems, toast, toastError } from "./03-shell.js";
import { renderMarkdown } from "./04-markdown.js";
import { cleanInline, parseJsonish, stripToolOutcome } from "./04-structured-parse.js";
import { renderJsonTree } from "./04-structured.js";
import { buildToolBlock, clearEmptyState, fillToolBlock, scrollBottom, setFollowLatest, toolIconNode, updateLatestButton } from "./05-chat-render.js";
import { toolRoots } from "./05-tool-summary.js";
import { orchPhaseLabel, orchPhaseOf, traceArgText } from "./06-activity.js";
import { openSubagentPanel } from "./06-agent-panel.js";

// ---------- 状态 ----------
// starting/running/stopping/waiting 是「还在跑」;其余都是终态。background/resumed 为 R-369 预留。
export const SA_ACTIVE = new Set(["starting", "running", "stopping", "waiting"]);
const SA_FAILED = new Set(["failed", "timeout", "rejected", "interrupted"]);
// 字形字符 + .kz-glyph 的 data-state(动画与基础色来自第一波原语,本组不写 keyframes)。
const SA_GLYPH = {
  starting: ["●", "running"],
  running: ["●", "running"],
  stopping: ["●", "stopping"],
  // 等待批准属于「需要你」(琥珀 attention),不是主线「等首个 token」的 waiting(强调色,进行中)。
  waiting: ["⏸", "attention"],
  background: ["◌", "pending"],
  done: ["✓", "done"],
  empty: ["○", "idle"],
  failed: ["✕", "failed"],
  timeout: ["⏱", "failed"],
  cancelled: ["■", "idle"],
  rejected: ["⊘", "failed"],
  interrupted: ["⚠", "failed"],
};
/// 状态词。running 没有状态词——计数本身就是状态。
export function subagentStateWord(state) {
  switch (state) {
    case "starting": return t("启动中");
    case "stopping": return t("停止中…");
    case "waiting": return t("等待批准");
    case "background": return t("后台运行");
    case "done": return t("完成");
    case "empty": return t("无回答");
    case "failed": return t("失败");
    case "timeout": return t("超时");
    case "cancelled": return t("已停止");
    case "rejected": return t("未启动");
    case "interrupted": return t("中断");
    default: return "";
  }
}

export const SA_SESSION_MAX = 200;
const SA_INPUT_KEEP = 4096;
const SA_REPLAY_KEEP = 1000;
export const subagentRunsBySession = new Map(); // sessionId -> Run[](按开始顺序)
export const subagentLive = new Map(); // `${sessionId}|${id}` -> Run(同键指向最新一次)
const replayCache = new Map(); // key -> { events: [], durationMs }
const changeListeners = new Set();
const tickListeners = new Set();
let batchSeq = 0;
let bodySeq = 0;
// 「载入更早的消息」渲染那一窗期间:{ sessionId, runs, groups }。窗里建出的 run 与组都比已有的旧,
// 结束时统一排到已有最小批次之下、插到列表前部(subagentPrependEnd)。
let prepending = null;

export function subagentKey(sessionId, id) {
  return `${sessionId || ""}|${id}`;
}
export function subagentByKey(key) {
  return subagentLive.get(key) ?? null;
}
export function subagentRunsFor(sessionId) {
  return [...(subagentRunsBySession.get(sessionId || "") ?? [])];
}
export function subagentRunningCount(sessionId) {
  return subagentRunsFor(sessionId).filter((run) => SA_ACTIVE.has(run.state)).length;
}
export function onSubagentChange(fn) {
  changeListeners.add(fn);
  return () => changeListeners.delete(fn);
}
export function onSubagentTick(fn) {
  tickListeners.add(fn);
  return () => tickListeners.delete(fn);
}
function notify(run) {
  // 补更早的一窗期间不逐条通知(侧栏会整表重建 N 次),结束时统一通知一次。
  if (prepending) return;
  for (const fn of changeListeners) {
    try { fn(run); } catch (error) { console.warn(error); }
  }
}

// ---------- 纯函数:描述/计数/分类 ----------
function clip(text, max) {
  const value = String(text ?? "");
  return value.length > max ? `${value.slice(0, max - 1)}…` : value;
}
function firstLine(text) {
  return String(text ?? "").split(/\r?\n/).map((line) => line.trim()).find(Boolean) ?? "";
}
function stripMarkdown(text) {
  // 只去成对的强调/代码记号与行首的列表/标题符号;`denial_hint` 这种标识符里的下划线保留。
  return String(text ?? "")
    .replace(/^\s*(?:[#>]+|[-*+]\s|\d+[.)]\s)\s*/, "")
    .replace(/\*\*|__|~~|`/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .trim();
}
/// 后端 summarize_input 是截到 160 字的入参 JSON:只抽 description/prompt 的值,绝不把 JSON 片段当描述。
function summaryText(summary) {
  const raw = String(summary ?? "").trim();
  if (!raw.startsWith("{")) return cleanInline(raw, toolRoots());
  const field = raw.match(/"(?:description|prompt)"\s*:\s*"((?:\\.|[^"\\])*)/);
  return field ? field[1].replace(/\\n/g, "\n").replace(/\\(.)/g, "$1") : "";
}
/// 短描述:task 的 description(模型/编排都用这个字段)→ prompt 首行首句(去 markdown,≤60 字)。
/// 句读只认句末标点后跟空白/行尾,`verify-policy.mjs` 这种文件名里的点不切。
export function subagentDescription(input, summary = "") {
  const own = typeof input?.description === "string" ? input.description.replace(/\s+/g, " ").trim() : "";
  if (own) return clip(own, 60);
  const prompt = typeof input?.prompt === "string" ? input.prompt : "";
  const line = stripMarkdown(firstLine(prompt || summaryText(summary)));
  const sentence = (line.split(/(?<=[。！？!?])|(?<=\.)(?=\s|$)/)[0] ?? "").trim().replace(/[。.！？!?]+$/, "");
  return clip(sentence || line, 60);
}
/// 累计 usage 的总 token(后端每步发的是**累计值**:直接替换,不累加)。
export function subagentUsageTokens(usage) {
  if (!usage) return 0;
  return ["input", "output", "cache_read", "cacheRead", "cache_write", "cacheWrite"]
    .reduce((sum, key) => sum + (Number(usage[key]) || 0), 0);
}
export function formatTokenCount(value) {
  const n = Math.max(0, Number(value) || 0);
  if (n < 1000) return String(Math.round(n));
  if (n < 1e6) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1e6).toFixed(1)}M`;
}
/// Claude Code 同款:`41s` / `1m 3s` / `1h 2m`。
export function formatElapsed(ms) {
  const total = Math.max(0, Math.floor((Number(ms) || 0) / 1000));
  if (total < 60) return `${total}s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}m ${total % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}
/// 终态分类:code 优先,旧文案正则只给没有 code 的历史数据兜底(§6)。
export function classifySubagentEnd({ ok, outcome, code, preview = "", content = "" } = {}) {
  switch (code) {
    case "subagent_timeout": return "timeout";
    case "subagent_cancelled": return "cancelled";
    case "subagent_limit": return "rejected";
    case "subagent_empty_answer": return "empty";
    default: break;
  }
  if (ok) return outcome === "noop" ? "empty" : "done";
  const text = `${preview ?? ""}\n${content ?? ""}`;
  if (/wall-clock safety limit|timed out|超时/i.test(text)) return "timeout";
  if (/stopped by the user|cancelled: run stopped|被停|已被停止/i.test(text)) return "cancelled";
  if (/too many parallel subagent tasks/i.test(text)) return "rejected";
  return "failed";
}
/// 「N 次工具」「N 个子代理」:英文界面 N=1 用单数(词表里的是复数)。
export function subagentToolCount(n) {
  return `${n} ${n === 1 && languageIsEnglish() ? "tool use" : t("次工具")}`;
}
export function subagentCountLabel(n) {
  return `${n} ${n === 1 && languageIsEnglish() ? "subagent" : t("个子代理")}`;
}
/// 身份签:编排角色名(身份)优先,其次实际人格(meta)、请求的人格。都不知道时不显示签。
export function subagentAgentName(run) {
  return run?.role || run?.agent || "";
}
export function subagentElapsed(run, now = Date.now()) {
  if (Number.isFinite(run.durationMs)) return run.durationMs;
  if (run.replay) return null;
  return (run.endedAt ?? now) - run.startedAt;
}
export function subagentMetaText(run, now = Date.now()) {
  const bits = [];
  const word = subagentStateWord(run.state);
  if (word) bits.push(word);
  const tools = run.toolOrder.length;
  if (tools > 0) bits.push(subagentToolCount(tools));
  const tokens = subagentUsageTokens(run.usage);
  if (tokens > 0) bits.push(`${formatTokenCount(tokens)} ${t("token")}`);
  const elapsed = subagentElapsed(run, now);
  if (elapsed !== null) bits.push(formatElapsed(elapsed));
  return bits.join(" · ");
}
function resultBody(run) {
  const result = run.result;
  if (result?.content && String(result.content).trim()) return String(result.content);
  if (run.lastText && run.lastText.trim()) return run.lastText;
  return String(result?.preview ?? "");
}
function errorLine(run) {
  if (!SA_FAILED.has(run.state)) return "";
  const raw = run.result?.preview || run.result?.content || "";
  const line = firstLine(stripToolOutcome(raw).body);
  if (line) return clip(cleanInline(line, toolRoots()), 160);
  return run.state === "interrupted" ? t("无结果(轮次中断)") : "";
}

// ---------- 数据模型 ----------
function newRun({ sessionId, id, input, summary, replay }) {
  const source = input && typeof input === "object" && !Array.isArray(input) ? input : {};
  const name = (value) => (typeof value === "string" && value.trim() ? value.trim() : "");
  return {
    key: subagentKey(sessionId, id),
    id: String(id),
    sessionId: sessionId || "",
    phase: orchPhaseOf(source),
    role: orchPhaseOf(source) ? name(source.role) || String(id) : "",
    agent: name(source.agent),
    model: "",
    tier: name(source.model),
    description: subagentDescription(source, summary),
    prompt: name(source.prompt) ? source.prompt : "",
    schema: Boolean(source.schema),
    resume: source.resume ?? null,
    state: "starting",
    replay: Boolean(replay),
    startedAt: Date.now(),
    endedAt: null,
    durationMs: null,
    tools: new Map(),
    toolOrder: [],
    timeline: [],
    usage: null,
    round: "",
    lastText: "",
    lastEvent: "",
    pendingCancel: false,
    result: null,
    settled: false,
    ended: false,
    batch: 0,
    cardEl: null,
    groupEl: null,
  };
}
function sessionRuns(sessionId) {
  let list = subagentRunsBySession.get(sessionId);
  if (!list) {
    list = [];
    subagentRunsBySession.set(sessionId, list);
  }
  return list;
}
function remember(run) {
  // 补更早的一窗:先记下,结束时插到列表前部(列表按开始顺序,它们比已有的都早)。
  if (prepending && run.replay && prepending.sessionId === run.sessionId) {
    prepending.runs.push(run);
    return;
  }
  const list = sessionRuns(run.sessionId);
  list.push(run);
  trimRuns(list);
}
function trimRuns(list) {
  // 每会话上限:超出丢最早的终态 run(运行中的永远保留)。
  while (list.length > SA_SESSION_MAX) {
    const index = list.findIndex((item) => !SA_ACTIVE.has(item.state));
    if (index < 0) break;
    const [dropped] = list.splice(index, 1);
    if (subagentLive.get(dropped.key) === dropped) subagentLive.delete(dropped.key);
  }
}
function keepInput(input) {
  if (input && typeof input === "object") {
    try {
      return JSON.stringify(input).length > SA_INPUT_KEEP ? null : input;
    } catch {
      return null;
    }
  }
  // 回放里的入参是截到 4K 的字符串:能解析才当对象用,否则只用 summary。
  if (typeof input === "string") {
    const parsed = parseJsonish(input);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : null;
  }
  return null;
}
/// 一条 task-progress 落进模型(实时与回放共用;不改 state,不碰 DOM)。
function applyProgress(run, text, trace) {
  if (!trace) {
    if (text) {
      run.round = String(text);
      run.lastEvent = "round";
    }
    return;
  }
  switch (trace.phase) {
    case "meta":
      if (trace.agent) run.agent = String(trace.agent);
      if (trace.model) run.model = String(trace.model);
      if (trace.summary) run.tier = String(trace.summary);
      return;
    case "start": {
      const childId = String(trace.child_id ?? run.toolOrder.length);
      if (run.tools.has(childId)) return;
      run.tools.set(childId, {
        name: String(trace.name || "tool"),
        input: keepInput(trace.input),
        summary: String(trace.summary ?? ""),
        done: false,
      });
      run.toolOrder.push(childId);
      run.timeline.push({ kind: "tool", childId });
      run.lastEvent = "tool";
      return;
    }
    case "end": {
      const childId = String(trace.child_id ?? "");
      let tool = run.tools.get(childId);
      if (!tool) {
        tool = { name: String(trace.name || "tool"), input: null, summary: "", done: false };
        run.tools.set(childId, tool);
        run.toolOrder.push(childId);
        run.timeline.push({ kind: "tool", childId });
      }
      Object.assign(tool, {
        done: true,
        ok: trace.ok !== false,
        outcome: trace.outcome ?? null,
        code: trace.code ?? null,
        preview: trace.preview ?? "",
        display: trace.display ?? null,
      });
      return;
    }
    case "usage":
      // 累计值:直接替换(旧实现逐次相加,三角数式地重复计数)。
      run.usage = trace.usage;
      return;
    case "text": {
      const said = String(trace.text ?? "");
      if (!said.trim()) return;
      run.lastText = said;
      run.timeline.push({ kind: "text", text: said });
      run.lastEvent = "text";
      return;
    }
    case "cancelled":
      run.pendingCancel = true;
      return;
    default:
      return;
  }
}

// ---------- 实时入口(07-events.js) ----------
export function subagentStart({ sessionId, id, input, summary } = {}) {
  if (!id) return null;
  const key = subagentKey(sessionId, id);
  const existing = subagentLive.get(key);
  // D-725:同一调用仍在跑时去重;上一轮已结束的同名编排角色则开新卡(上一轮结果保留在旧卡里)。
  if (existing && SA_ACTIVE.has(existing.state)) return existing;
  const run = newRun({ sessionId, id, input, summary, replay: false });
  remember(run);
  subagentLive.set(key, run);
  mountSubagentCard(run, { scroll: true });
  renderSubagentCard(run);
  notify(run);
  return run;
}

export function subagentProgress({ sessionId, id, text, trace } = {}) {
  const run = subagentLive.get(subagentKey(sessionId, id));
  // 终态之后迟到的进度不复活卡片(与 #7 的 turnPhase 粘滞同一口径)。
  if (!run || !SA_ACTIVE.has(run.state)) return;
  applyProgress(run, text, trace);
  if (run.state === "starting") run.state = "running";
  // 后端报「已被停止」(单条停止生效,ToolEnd 紧随其后):先转停止中,不再显示在跑。
  if (trace?.phase === "cancelled") run.state = "stopping";
  sealGroup(run);
  renderSubagentCard(run);
  // 尾迹长高了:跟随最新时照常贴底(scrollBottom 自己判断跟随态,按帧合并)。后台线不动滚动条。
  if (!renderingBackground && run.sessionId === (activeSessionId || "")) scrollBottom();
  notify(run);
}

export function subagentEnd({ sessionId, id, ok, outcome, code, preview, display, content, durationMs } = {}) {
  const run = subagentLive.get(subagentKey(sessionId, id));
  if (!run || run.ended) return;
  const settledBefore = run.settled;
  const body = content === undefined || content === null ? "" : stripToolOutcome(content).body;
  run.result = { ok: Boolean(ok), outcome: outcome ?? null, code: code ?? null, preview: String(preview ?? ""), display: display ?? null, content: body };
  run.state = classifySubagentEnd({ ok, outcome, code, preview, content: body });
  run.ended = true;
  run.endedAt = Date.now();
  const measured = Number(durationMs);
  if (durationMs !== undefined && durationMs !== null && Number.isFinite(measured)) run.durationMs = measured;
  else if (!Number.isFinite(run.durationMs)) run.durationMs = run.endedAt - run.startedAt;
  // 超额派发(subagent_limit)在后端是逐个「start、end」紧跟在同批 ToolStart 之后发出的:没跑起来的卡
  // 不封口,同批其余超额卡照常并进这一组(组内成员收到第一条进度时照常封口)。
  const neverRan = run.state === "rejected" && !run.toolOrder.length && !run.timeline.length;
  if (!neverRan) sealGroup(run);
  renderSubagentCard(run, { motion: !settledBefore });
  // 播报只给活动线路的真实终态(停止收尾之后补发的 ToolEnd 不再播一遍)。
  if (!settledBefore && !renderingBackground && run.sessionId === activeSessionId) announce(run);
  notify(run);
}

/// 整轮停止 / 终态出错 / 轮末仍未收到终态:把该会话还在跑的卡收尾(01-core.js 路由层调用,
/// 活动线与后台线都走这里)。之后到达的 ToolEnd(停止补发)只校准终态,不复活运行态。
export function subagentSettle(sessionId, state) {
  const list = subagentRunsBySession.get(sessionId || "");
  if (!list) return 0;
  let settled = 0;
  for (const run of list) {
    if (!SA_ACTIVE.has(run.state)) continue;
    run.state = state;
    run.settled = true;
    run.endedAt = Date.now();
    if (!run.replay && !Number.isFinite(run.durationMs)) run.durationMs = run.endedAt - run.startedAt;
    sealGroup(run);
    renderSubagentCard(run);
    notify(run);
    settled += 1;
  }
  return settled;
}

/// 重载该会话的历史前调用:丢弃终态 run 与回放缓存,保留还在跑的。
export function subagentResetSession(sessionId) {
  const sid = sessionId || "";
  const list = subagentRunsBySession.get(sid) ?? [];
  const kept = [];
  for (const run of list) {
    if (SA_ACTIVE.has(run.state) && !run.replay) {
      kept.push(run);
      continue;
    }
    if (subagentLive.get(run.key) === run) subagentLive.delete(run.key);
  }
  subagentRunsBySession.set(sid, kept);
  const prefix = `${sid}|`;
  for (const key of [...replayCache.keys()]) if (key.startsWith(prefix)) replayCache.delete(key);
  notify(null);
}

// ---------- 历史回放入口(15-views-misc.js / 06-activity.js) ----------
export function subagentHistoryCall(sessionId, callId, input) {
  if (!callId) return null;
  const key = subagentKey(sessionId, callId);
  const live = subagentLive.get(key);
  // 线路还在跑(切回/重载时历史里已经有这次调用):复用实时 run,只重挂卡片。
  if (live && !live.replay && SA_ACTIVE.has(live.state)) {
    mountSubagentCard(live);
    renderSubagentCard(live);
    return live;
  }
  const run = newRun({ sessionId, id: callId, input, replay: true });
  remember(run);
  subagentLive.set(key, run);
  mountSubagentCard(run);
  const cached = replayCache.get(key);
  if (cached) {
    replayCache.delete(key);
    for (const event of cached.events) applyProgress(run, event.text, normalizeReplayTrace(event.trace));
    if (Number.isFinite(cached.durationMs)) run.durationMs = cached.durationMs;
  }
  renderSubagentCard(run);
  notify(run);
  return run;
}

export function subagentHistoryResult(sessionId, callId, { ok, content, interrupted = false } = {}) {
  const run = subagentLive.get(subagentKey(sessionId, callId));
  // 实时 run 的终态归它自己的事件;这里只收回放建出来的卡。
  if (!run || !run.replay) return;
  sealGroup(run);
  if (interrupted) {
    if (SA_ACTIVE.has(run.state)) {
      run.state = "interrupted";
      run.settled = true;
    }
  } else {
    applyHistoryResult(run, { ok, content });
  }
  renderSubagentCard(run);
  notify(run);
}
function applyHistoryResult(run, { ok, content }) {
  const parsed = stripToolOutcome(content);
  run.result = { ok: Boolean(ok), outcome: parsed.outcome, code: parsed.code, preview: firstLine(parsed.body), display: null, content: parsed.body };
  run.state = classifySubagentEnd({ ok, outcome: parsed.outcome, code: parsed.code, preview: parsed.body.slice(0, 600) });
  run.ended = true;
}

/// 窗口边界把 task 的调用与结果切开(调用在更早的窗口里、结果在已渲染的这一窗):在结果的位置用历史里
/// 那次调用的入参建卡,直接收成终态——不再落成一个「tool result」通用块,补齐更早的窗口时也不会被标成
/// 「中断」(15-views-misc.js 认领那次调用,不建第二张)。不封口:同批其余孤儿结果照常并进这一组。
export function subagentHistoryOrphan(sessionId, callId, input, { ok, content } = {}) {
  const run = subagentHistoryCall(sessionId, callId, input);
  if (!run?.replay) return run;
  applyHistoryResult(run, { ok, content });
  renderSubagentCard(run);
  notify(run);
  return run;
}

/// 历史回放:同一批 = 同一条助手消息里的调用。每条带调用的消息开头封住 pane 末尾仍 open 的组,
/// 孤儿结果建出的组(不自己封口)才不会把下一批并进来。
export function subagentSealPaneTail() {
  const kids = activePane?.children;
  const last = kids?.length ? kids[kids.length - 1] : null;
  if (last?.classList?.contains("sa-group") && last.dataset?.open === "1") last.dataset.open = "0";
}

/// 「载入更早的消息」前后调用(15-views-misc.js loadEarlierMessages)。这一窗里建出的组比已有的都旧:
/// 按出现顺序排到已有最小批次之下,run 插到列表前部——侧栏列表按批次降序,最新一批仍在最上面。
export function subagentPrependBegin(sessionId) {
  prepending = { sessionId: sessionId || "", runs: [], groups: [] };
}
export function subagentPrependEnd() {
  const pending = prepending;
  prepending = null;
  if (!pending || (!pending.runs.length && !pending.groups.length)) return;
  const list = sessionRuns(pending.sessionId);
  const fresh = new Set(pending.groups);
  let floor = batchSeq + 1;
  for (const run of list) if (!fresh.has(run.groupEl)) floor = Math.min(floor, run.batch);
  pending.groups.forEach((group, index) => {
    group.dataset.batch = String(floor - pending.groups.length + index);
  });
  for (const run of [...pending.runs, ...list]) {
    if (run.groupEl && fresh.has(run.groupEl)) run.batch = Number(run.groupEl.dataset.batch);
  }
  list.unshift(...pending.runs);
  trimRuns(list);
  notify(null);
}

function normalizeReplayTrace(trace) {
  if (!trace || typeof trace !== "object") return null;
  return { ...trace, input: keepInput(trace.input) };
}
/// run.trace 里落库的 task-progress 条目(无 kind 字段)。卡片已在就直接应用,否则进缓存等
/// 「载入更早的消息」补出卡片时再消费。
export function subagentReplayTrace(sessionId, event) {
  if (!event?.id) return;
  const key = subagentKey(sessionId, event.id);
  const run = subagentLive.get(key);
  if (run) {
    if (!run.replay) return; // 实时 run 已有完整事件
    applyProgress(run, event.text, normalizeReplayTrace(event.trace));
    renderSubagentCard(run);
    notify(run);
    return;
  }
  let cached = replayCache.get(key);
  if (!cached) {
    cached = { events: [], durationMs: null };
    replayCache.set(key, cached);
  }
  if (cached.events.length < SA_REPLAY_KEEP) cached.events.push(event);
}
export function subagentReplayDuration(sessionId, id, durationMs, { isTask = true } = {}) {
  const ms = Number(durationMs);
  if (!id || !Number.isFinite(ms)) return;
  const key = subagentKey(sessionId, id);
  const run = subagentLive.get(key);
  if (run) {
    if (!run.replay) return;
    run.durationMs = ms;
    renderSubagentCard(run);
    notify(run);
    return;
  }
  if (!isTask) return;
  let cached = replayCache.get(key);
  if (!cached) {
    cached = { events: [], durationMs: null };
    replayCache.set(key, cached);
  }
  cached.durationMs = ms;
}

// ---------- 主对话卡片 ----------
function sealGroup(run) {
  const group = run.groupEl;
  if (group?.dataset && group.dataset.open === "1") group.dataset.open = "0";
}

function buildGroup(run) {
  const group = document.createElement("div");
  group.className = "sa-group solo";
  batchSeq += 1;
  group.dataset.batch = String(batchSeq);
  if (prepending) prepending.groups.push(group);
  group.dataset.open = "1";
  group.dataset.saPhase = run.phase || "";
  group.dataset.saCount = "0";
  const head = document.createElement("div");
  head.className = "sa-group-head hidden";
  const glyph = document.createElement("span");
  glyph.className = "kz-glyph sa-glyph";
  glyph.setAttribute("aria-hidden", "true");
  const label = document.createElement("span");
  label.className = "sa-group-label";
  const open = document.createElement("button");
  open.type = "button";
  open.className = "icon-btn sa-group-open";
  open.textContent = "↗";
  open.setAttribute("aria-label", t("查看全部子代理"));
  open.dataset.i18nAriaLabel = "查看全部子代理";
  open.title = t("查看全部子代理");
  open.dataset.i18nTitle = "查看全部子代理";
  open.addEventListener("click", (event) => {
    event?.stopPropagation?.();
    openSubagentPanel(null, open);
  });
  head.append(glyph, label, open);
  const body = document.createElement("div");
  body.className = "sa-group-body";
  group.append(head, body);
  group._sa = { head, glyph, label, body };
  return group;
}

function buildCard(run) {
  const card = document.createElement("div");
  card.className = "sa-card";
  card.dataset.saKey = run.key;
  card._saRun = run;
  bodySeq += 1;
  const bodyId = `sa-body-${bodySeq}`;
  const row = document.createElement("div");
  row.className = "sa-row";
  const head = document.createElement("button");
  head.type = "button";
  head.className = "sa-head";
  head.setAttribute("aria-expanded", "false");
  head.setAttribute("aria-controls", bodyId);
  const glyph = document.createElement("span");
  glyph.className = "kz-glyph sa-glyph";
  glyph.setAttribute("aria-hidden", "true");
  const agent = document.createElement("span");
  agent.className = "sa-agent";
  const desc = document.createElement("span");
  desc.className = "sa-desc";
  const chip = document.createElement("span");
  chip.className = "sa-chip hidden";
  const now = document.createElement("span");
  now.className = "sa-now hidden";
  const meta = document.createElement("span");
  meta.className = "sa-meta";
  head.append(glyph, agent, desc, chip, now, meta);
  const actions = document.createElement("span");
  actions.className = "sa-actions";
  const stop = iconButton("sa-stop", "■", "停止这个子代理");
  const open = iconButton("sa-open", "↗", "在侧栏查看完整过程");
  actions.append(stop, open);
  row.append(head, actions);
  const live = document.createElement("div");
  live.className = "sa-live hidden";
  const error = document.createElement("div");
  error.className = "sa-error hidden";
  const body = document.createElement("div");
  body.className = "sa-body hidden";
  body.id = bodyId;
  card.append(row, live, error, body);
  card._sa = { head, glyph, agent, desc, chip, now, meta, stop, open, live, error, body, lineEls: new Map(), moreEl: null, idleEl: null, lastState: null, lastLabel: null, view: null };
  head.addEventListener("click", () => toggleSubagentBody(run));
  stop.addEventListener("click", (event) => {
    event?.stopPropagation?.();
    subagentStop(run);
  });
  open.addEventListener("click", (event) => {
    event?.stopPropagation?.();
    openSubagentPanel(run.key, open);
  });
  return card;
}

function iconButton(cls, text, key) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = `icon-btn ${cls}`;
  button.textContent = text;
  button.setAttribute("aria-label", t(key));
  button.dataset.i18nAriaLabel = key;
  button.title = t(key);
  button.dataset.i18nTitle = key;
  return button;
}

/// 挂卡:当前 pane 的最后一个子节点若是仍 open 的同阶段组就并入,否则新建组。同一批 task 的
/// tool-start 是连续到达的(drive.rs 先发完全部 ToolStart 再并发执行),任何其它内容追加进 pane
/// 或组内成员收到第一条进度,组就自然封口。
function mountSubagentCard(run, { scroll = false } = {}) {
  const pane = activePane;
  if (!pane) return;
  clearEmptyState();
  const kids = pane.children;
  const last = kids.length ? kids[kids.length - 1] : null;
  let group = null;
  if (last?.classList?.contains("sa-group") && last.dataset.open === "1" && (last.dataset.saPhase || "") === (run.phase || "")) group = last;
  if (!group) {
    group = buildGroup(run);
    appendToPane(group);
  }
  run.groupEl = group;
  run.batch = Number(group.dataset.batch) || 0;
  run.cardEl = buildCard(run);
  group._sa.body.appendChild(run.cardEl);
  syncGroup(group);
  if (scroll) scrollBottom();
}

function groupRuns(group) {
  const body = group?._sa?.body;
  if (!body) return [];
  const runs = [];
  const kids = body.children;
  for (let index = 0; index < kids.length; index += 1) if (kids[index]._saRun) runs.push(kids[index]._saRun);
  return runs;
}

function syncGroup(group) {
  if (!group?._sa) return;
  const runs = groupRuns(group);
  const count = runs.length;
  const solo = count < 2;
  group.dataset.saCount = String(count);
  group.classList.toggle("solo", solo);
  group._sa.head.classList.toggle("hidden", solo);
  for (const run of runs) {
    const variant = solo ? "solo" : "member";
    if (run.cardEl && run.cardEl.dataset.saVariant !== variant) {
      run.cardEl.dataset.saVariant = variant;
      renderSubagentCard(run, { skipGroup: true });
    }
  }
  const running = runs.filter((run) => SA_ACTIVE.has(run.state)).length;
  const done = runs.filter((run) => run.state === "done" || run.state === "empty").length;
  const failed = runs.filter((run) => SA_FAILED.has(run.state)).length;
  const stopped = runs.filter((run) => run.state === "cancelled").length;
  group.dataset.running = String(running > 0);
  const glyphState = running ? "running" : failed ? "failed" : stopped && !done ? "cancelled" : "done";
  const [char, state] = SA_GLYPH[glyphState];
  const glyph = group._sa.glyph;
  if (glyph.dataset.state !== state) {
    glyph.dataset.state = state;
    if (state === "running") motionSync(glyph);
  }
  glyph.textContent = char;
  const phase = group.dataset.saPhase;
  const bits = [subagentCountLabel(count)];
  if (running) bits.push(`${running} ${t("运行中")}`);
  if (done) bits.push(`${done} ${t("完成")}`);
  if (failed) bits.push(`${failed} ${t("失败")}`);
  if (stopped) bits.push(`${stopped} ${t("已停止")}`);
  const label = `${phase ? `${orchPhaseLabel(phase)} · ` : ""}${bits.join(" · ")}`;
  if (group._sa.label.textContent !== label) group._sa.label.textContent = label;
}

/// 卡片原地更新。节点已被裁掉/清空(isConnected 明确为 false)时只动模型,不碰 DOM。
export function renderSubagentCard(run, { motion = false, skipGroup = false } = {}) {
  const card = run?.cardEl;
  if (!card?._sa || card.isConnected === false) return;
  const ui = card._sa;
  const member = card.dataset.saVariant === "member";
  const state = run.state;
  const stateChanged = ui.lastState !== state;
  card.dataset.saState = state;
  const [char, glyphState] = SA_GLYPH[state] ?? SA_GLYPH.running;
  if (ui.glyph.dataset.state !== glyphState) {
    ui.glyph.dataset.state = glyphState;
    if (SA_ACTIVE.has(state)) motionSync(ui.glyph);
  }
  ui.glyph.textContent = char;
  if (stateChanged && motion && !run.replay && !SA_ACTIVE.has(state) && state !== "cancelled") motionOnce(ui.glyph, "kz-pop", 360);
  const label = subagentAgentName(run);
  ui.agent.textContent = label;
  ui.agent.className = `sa-agent${label ? "" : " hidden"}`;
  const description = run.description || t("子代理");
  if (ui.desc.textContent !== description) ui.desc.textContent = description;
  ui.desc.title = run.description;
  const chip = run.resume ? t("续聊") : "";
  ui.chip.textContent = chip;
  ui.chip.classList.toggle("hidden", !chip);
  const metaText = subagentMetaText(run);
  if (ui.meta.textContent !== metaText) ui.meta.textContent = metaText;
  // ■ 的位置常驻(不跑时禁用 + 隐形):组内各行的计数列才对得齐,悬停时也不跳。
  const canStop = SA_ACTIVE.has(state) && state !== "stopping";
  ui.stop.disabled = !canStop;
  ui.stop.classList.toggle("is-off", !canStop);
  // member 变体一行:运行中在描述后跟一段最新工具的摘要,不出三行尾迹。
  const nowText = member && SA_ACTIVE.has(state) ? latestToolText(run) : "";
  if (ui.now.textContent !== nowText) ui.now.textContent = nowText;
  ui.now.classList.toggle("hidden", !nowText);
  renderTail(run, ui, member);
  const error = errorLine(run);
  ui.error.textContent = error;
  ui.error.classList.toggle("hidden", !error);
  // 可访问名只在状态变化时重写,不随每秒计时刷新。
  if (stateChanged || ui.lastLabel !== label) {
    const word = subagentStateWord(state) || t("运行中");
    ui.head.setAttribute("aria-label", [[label, run.description].filter(Boolean).join(" "), word, metaText].filter(Boolean).join(" — "));
  }
  ui.lastState = state;
  ui.lastLabel = label;
  if (ui.view && !ui.body.classList.contains("hidden")) renderSubagentTimeline(ui.view);
  if (!skipGroup && run.groupEl) syncGroup(run.groupEl);
}

function latestToolText(run) {
  const childId = run.toolOrder[run.toolOrder.length - 1];
  const tool = childId === undefined ? null : run.tools.get(childId);
  if (!tool) return run.lastEvent === "text" ? clip(firstLine(run.lastText), 60) : "";
  return [tool.name, traceArgText(tool.name, tool.input, tool.summary)].filter(Boolean).join(" ");
}

/// 实时尾迹:最近 3 次子工具调用(与主对话工具行同一套摘要口径),更早的折成「+N」。
/// 行节点按 child id 复用——每条进度都整段重建会让淡入动效反复重播。
function renderTail(run, ui, member) {
  const show = SA_ACTIVE.has(run.state) && !member;
  const live = ui.live;
  if (!show) {
    live.classList.add("hidden");
    if (ui.lineEls.size || ui.moreEl || ui.idleEl) {
      live.replaceChildren();
      ui.lineEls.clear();
      ui.moreEl = null;
      ui.idleEl = null;
    }
    return;
  }
  const order = run.toolOrder;
  const shown = order.slice(-3);
  const more = order.length - shown.length;
  const latest = shown[shown.length - 1];
  for (const [childId, el] of [...ui.lineEls]) {
    if (!shown.includes(childId)) {
      el.remove();
      ui.lineEls.delete(childId);
    }
  }
  for (const childId of shown) {
    const tool = run.tools.get(childId);
    let el = ui.lineEls.get(childId);
    if (!el) {
      el = document.createElement("div");
      el.className = "sa-line";
      const mark = document.createElement("span");
      mark.className = "sa-line-mark";
      mark.setAttribute("aria-hidden", "true");
      const name = document.createElement("span");
      name.className = "sa-line-name";
      name.textContent = tool.name;
      const arg = document.createElement("span");
      arg.className = "sa-line-arg";
      el.append(mark, toolIconNode(tool.name), name, arg);
      el._sa = { mark, arg };
      ui.lineEls.set(childId, el);
      live.appendChild(el);
    }
    const argText = traceArgText(tool.name, tool.input, tool.summary);
    if (el._sa.arg.textContent !== argText) el._sa.arg.textContent = argText;
    const failed = tool.done && tool.ok === false;
    el.classList.toggle("is-current", !tool.done && childId === latest);
    el.classList.toggle("is-done", Boolean(tool.done) && !failed);
    el.classList.toggle("is-failed", failed);
    el._sa.mark.textContent = failed ? "✕" : "";
  }
  if (more > 0) {
    if (!ui.moreEl) {
      ui.moreEl = document.createElement("div");
      ui.moreEl.className = "sa-more";
    }
    ui.moreEl.textContent = `+${more} ${t("次更早的工具调用")}`;
    if (live.firstChild !== ui.moreEl) live.insertBefore(ui.moreEl, live.firstChild);
  } else if (ui.moreEl) {
    ui.moreEl.remove();
    ui.moreEl = null;
  }
  // 还没有任何工具调用:显示轮次文本,或子代理最近一句自述的首行。
  const idle = order.length ? "" : run.lastEvent === "text" ? clip(firstLine(run.lastText), 80) : run.round;
  if (idle) {
    if (!ui.idleEl) {
      ui.idleEl = document.createElement("div");
      ui.idleEl.className = "sa-line sa-idle";
      live.appendChild(ui.idleEl);
    }
    ui.idleEl.textContent = idle;
  } else if (ui.idleEl) {
    ui.idleEl.remove();
    ui.idleEl = null;
  }
  live.classList.toggle("hidden", !(order.length || idle));
}

export function toggleSubagentBody(run, force) {
  const card = run?.cardEl;
  if (!card?._sa) return;
  const ui = card._sa;
  const open = force ?? ui.body.classList.contains("hidden");
  ui.body.classList.toggle("hidden", !open);
  ui.head.setAttribute("aria-expanded", String(open));
  card.classList.toggle("is-open", open);
  if (open) {
    if (!ui.view) ui.view = createSubagentView(ui.body, run, { inline: true });
    renderSubagentTimeline(ui.view);
  }
}

// ---------- 指令 / 过程 / 结果(内联展开与侧栏详情共用同一个渲染器) ----------
export function createSubagentView(container, run, { inline = false } = {}) {
  return { container, run, inline, built: false, rendered: 0, blocks: new Map(), resultSig: null };
}

function sectionTitle(key) {
  const title = document.createElement("div");
  title.className = "sa-section-title";
  title.textContent = t(key);
  title.dataset.i18nKey = key;
  return title;
}

function buildView(view) {
  const { container, run } = view;
  container.replaceChildren();
  view.rendered = 0;
  view.blocks.clear();
  view.resultSig = null;
  view.emptyEl = null;
  const prompt = document.createElement("section");
  prompt.className = "sa-section sa-section-prompt";
  const promptBody = document.createElement("div");
  promptBody.className = "sa-prompt md sv-md";
  promptBody.innerHTML = renderMarkdown(run.prompt || run.description || "");
  prompt.append(sectionTitle("指令"), promptBody);
  // 默认显示 4 行左右;长指令给「展开全文」。
  if (view.inline && (run.prompt.split(/\r?\n/).length > 4 || run.prompt.length > 280)) {
    promptBody.classList.add("is-clamped");
    const more = document.createElement("button");
    more.type = "button";
    more.className = "ghost mini sa-prompt-more";
    more.textContent = t("展开全文");
    more.addEventListener("click", () => {
      const clamped = promptBody.classList.toggle("is-clamped");
      more.textContent = clamped ? t("展开全文") : t("收起");
    });
    prompt.append(more);
  }
  if (run.schema) {
    const schema = document.createElement("div");
    schema.className = "sa-schema";
    schema.textContent = t("要求结构化返回");
    prompt.append(schema);
  }
  const steps = document.createElement("section");
  steps.className = "sa-section sa-section-steps";
  const stepsBody = document.createElement("div");
  stepsBody.className = "sa-steps";
  steps.append(sectionTitle("过程"), stepsBody);
  const result = document.createElement("section");
  result.className = "sa-section sa-section-result";
  const resultBodyEl = document.createElement("div");
  resultBodyEl.className = "sa-result";
  result.append(sectionTitle("结果"), resultBodyEl);
  container.append(prompt, steps, result);
  if (view.inline) {
    const actions = document.createElement("div");
    actions.className = "sa-body-actions";
    const stop = document.createElement("button");
    stop.type = "button";
    stop.className = "ghost mini sa-body-stop";
    stop.textContent = t("停止");
    stop.addEventListener("click", () => subagentStop(run));
    const copy = document.createElement("button");
    copy.type = "button";
    copy.className = "ghost mini sa-body-copy";
    copy.textContent = t("复制结果");
    copy.addEventListener("click", () => subagentCopyResult(run));
    const open = document.createElement("button");
    open.type = "button";
    open.className = "ghost mini sa-body-open";
    open.textContent = t("在侧栏查看完整过程");
    open.addEventListener("click", () => openSubagentPanel(run.key, open));
    actions.append(stop, copy, open);
    container.append(actions);
    view.stopEl = stop;
  }
  view.stepsEl = stepsBody;
  view.resultEl = resultBodyEl;
  view.built = true;
}

/// 增量渲染:时间线按 view.rendered 追加,子工具行复用主对话的 buildToolBlock/fillToolBlock
/// (同一套图标、⎿ 摘要与可展开详情);子代理轨迹不带正文,摘要器走 preview + display 降级口径。
export function renderSubagentTimeline(view) {
  if (!view?.container) return;
  const run = view.run;
  if (!view.built) buildView(view);
  const timeline = run.timeline;
  for (; view.rendered < timeline.length; view.rendered += 1) {
    const entry = timeline[view.rendered];
    if (entry.kind === "text") {
      const message = document.createElement("div");
      message.className = "sa-msg md sv-md";
      message.innerHTML = renderMarkdown(entry.text);
      view.stepsEl.appendChild(message);
      continue;
    }
    const tool = run.tools.get(entry.childId);
    if (!tool) continue;
    const argText = tool.input ? null : traceArgText(tool.name, null, tool.summary);
    const block = buildToolBlock(tool.name, tool.input ?? (argText ? { summary: argText } : {}));
    // 只有摘要替身时不当入参存:收尾时不把它当「完整入参」贴进展开区。
    block.input = tool.input ?? null;
    view.stepsEl.appendChild(block.wrap);
    view.blocks.set(entry.childId, { block, filled: false });
  }
  const terminal = !SA_ACTIVE.has(run.state);
  for (const [childId, slot] of view.blocks) {
    if (slot.filled) continue;
    const tool = run.tools.get(childId);
    if (tool?.done) {
      fillToolBlock(slot.block, { ok: tool.ok, outcome: tool.outcome ?? undefined, code: tool.code ?? undefined, preview: tool.preview, display: tool.display });
      slot.filled = true;
    } else if (terminal) {
      // 子代理已结束而这次调用没等到结果:停在「中断」,不在展开区里转到天荒地老。
      const { block } = slot;
      block.wrap.classList.remove("running");
      block.wrap.classList.add("interrupted");
      block.wrap.dataset.toolOutcome = "interrupted";
      block.icon.textContent = "⏹";
      block.result.textContent = `⎿ ${t("无结果(轮次中断)")}`;
      block.result.classList.remove("hidden");
      slot.filled = true;
    }
  }
  // 还没有任何过程:运行中给轮次/占位,结束了给一条短横(不留空节)。
  if (!timeline.length) {
    if (!view.emptyEl) {
      view.emptyEl = document.createElement("div");
      view.emptyEl.className = "sa-steps-empty";
      view.stepsEl.appendChild(view.emptyEl);
    }
    view.emptyEl.textContent = run.round || (SA_ACTIVE.has(run.state) ? t("运行中…") : "—");
  } else if (view.emptyEl) {
    view.emptyEl.remove();
    view.emptyEl = null;
  }
  // 结果节:终态才有;运行中给占位。带 schema 的结果能解析就画 JSON 树。
  const body = resultBody(run);
  const sig = `${run.state}\u0000${body}`;
  if (sig !== view.resultSig) {
    view.resultSig = sig;
    const host = view.resultEl;
    host.replaceChildren();
    host.className = "sa-result";
    if (SA_ACTIVE.has(run.state)) {
      host.classList.add("is-pending");
      host.textContent = t("运行中…");
    } else if (!body.trim() || run.state === "empty") {
      host.classList.add("is-pending");
      host.textContent = run.state === "empty" ? t("子代理未给出回答") : run.state === "interrupted" ? t("无结果(轮次中断)") : "—";
    } else {
      const json = run.schema ? parseJsonish(body) : null;
      if (json && typeof json === "object") host.appendChild(renderJsonTree(json));
      else {
        host.classList.add("md", "sv-md");
        host.innerHTML = renderMarkdown(body);
      }
    }
  }
  if (view.stopEl) view.stopEl.classList.toggle("hidden", !SA_ACTIVE.has(run.state) || run.state === "stopping");
}

// ---------- 动作 ----------
export async function subagentStop(run) {
  if (!run || !SA_ACTIVE.has(run.state) || run.state === "stopping") return;
  const processId = processItems.find((item) => item.session_id === run.sessionId)?.id
    || (run.sessionId === activeSessionId ? activeProcessId : null);
  const previous = run.state;
  run.state = "stopping";
  renderSubagentCard(run);
  notify(run);
  try {
    // R-174:单条停止通道——只停这一条,不停整轮(不调 stop_run)。
    await invoke("stop_task", { projectDir: currentProject, processId, taskId: String(run.id) });
    toast(t("已请求停止该子代理"));
  } catch (error) {
    if (run.state === "stopping") run.state = previous;
    renderSubagentCard(run);
    notify(run);
    toastError(`${t("停止失败")}:${error}`);
  }
}

export async function subagentCopyResult(run) {
  const text = resultBody(run).trim();
  if (!text) {
    toast(t("没有可复制的内容"));
    return;
  }
  try {
    await navigator.clipboard.writeText(text);
    toast(t("已复制结果"));
  } catch (error) {
    toastError(`${t("复制失败")}:${error}`);
  }
}

/// ⌖ 定位到对话:卡片居中并闪一下描边;卡片已被裁掉/清空时说清楚,而不是点了没反应。
export function subagentLocate(run) {
  const card = run?.cardEl;
  const pane = card?.closest?.(".msg-pane");
  if (!card || card.isConnected === false || !pane || pane !== activePane) {
    toast(t("该子代理已滚出当前视图"));
    return false;
  }
  // 跳到旧内容 = 用户明确在读某一处,不再跟随最新(同聊天搜索命中)。
  setFollowLatest(false);
  card.scrollIntoView({ block: "center" });
  updateLatestButton();
  card.classList.add("sa-flash");
  clearTimeout(card._saFlash);
  card._saFlash = setTimeout(() => card.classList.remove("sa-flash"), 1200);
  return true;
}

/// 复制上下文(07-events.js):以卡片为单位导出——身份 · 描述 / 计数 / 结果前 400 字。
export function subagentCopyText(el) {
  const cards = el?.classList?.contains("sa-card") ? [el] : [...(el?.querySelectorAll?.(".sa-card") ?? [])];
  const parts = [];
  for (const card of cards) {
    const run = card._saRun;
    if (!run) continue;
    const who = [subagentAgentName(run), run.description].filter(Boolean).join(" · ");
    const lines = [`> ${t("子代理")}:${who}`, `> ${subagentMetaText(run)}`];
    const result = resultBody(run).trim();
    if (result) lines.push(`> ${t("结果")}:${result.slice(0, 400).split("\n").join("\n> ")}`);
    parts.push(lines.join("\n"));
  }
  return parts.join("\n\n");
}

/// 侧栏行(member 变体的一行版):字形 · 身份 · 描述 · 计数。点击行由调用方决定去向。
export function buildSubagentRow(run, { onOpen, onLocate } = {}) {
  const row = document.createElement("div");
  row.className = "sa-panel-row";
  row.dataset.saKey = run.key;
  const main = document.createElement("button");
  main.type = "button";
  main.className = "sa-panel-main";
  const glyph = document.createElement("span");
  glyph.className = "kz-glyph sa-glyph";
  glyph.setAttribute("aria-hidden", "true");
  const agent = document.createElement("span");
  const desc = document.createElement("span");
  desc.className = "sa-desc";
  const meta = document.createElement("span");
  meta.className = "sa-meta";
  main.append(glyph, agent, desc, meta);
  const locate = iconButton("sa-locate", "⌖", "定位到对话");
  row.append(main, locate);
  row._sa = { glyph, agent, desc, meta, main, aria: null };
  main.addEventListener("click", () => onOpen?.(run, main));
  locate.addEventListener("click", (event) => {
    event?.stopPropagation?.();
    onLocate?.(run);
  });
  updateSubagentRow(row, run);
  return row;
}
export function updateSubagentRow(row, run) {
  const ui = row?._sa;
  if (!ui) return;
  row.dataset.saState = run.state;
  const [char, glyphState] = SA_GLYPH[run.state] ?? SA_GLYPH.running;
  if (ui.glyph.dataset.state !== glyphState) {
    ui.glyph.dataset.state = glyphState;
    if (SA_ACTIVE.has(run.state)) motionSync(ui.glyph);
  }
  ui.glyph.textContent = char;
  const label = subagentAgentName(run);
  ui.agent.textContent = label;
  ui.agent.className = `sa-agent${label ? "" : " hidden"}`;
  ui.desc.textContent = run.description || t("子代理");
  ui.desc.title = run.description;
  updateSubagentRowMeta(row, run);
  // 可访问名与卡片同一口径:只随状态/身份变化重写,不带逐秒变化的耗时(焦点停在行上时读屏不反复播报);
  // 终态的计数不再变化,带上。
  const aria = [[label, run.description].filter(Boolean).join(" "), subagentStateWord(run.state) || t("运行中"), SA_ACTIVE.has(run.state) ? "" : ui.meta.textContent]
    .filter(Boolean).join(" — ");
  if (ui.aria !== aria) {
    ui.aria = aria;
    ui.main.setAttribute("aria-label", aria);
  }
}
/// 侧栏行的 1 秒刷新:只动计数文本(耗时),不碰可访问名。
export function updateSubagentRowMeta(row, run, now = Date.now()) {
  const meta = row?._sa?.meta;
  if (!meta) return;
  const text = subagentMetaText(run, now);
  if (meta.textContent !== text) meta.textContent = text;
}

/// 侧栏详情头部用:字形字符与 data-state。
export function subagentGlyph(run) {
  return SA_GLYPH[run?.state] ?? SA_GLYPH.running;
}

function announce(run) {
  const host = $("sa-announcer");
  if (!host) return;
  host.textContent = [t("子代理"), subagentAgentName(run), run.description, subagentStateWord(run.state)].filter(Boolean).join(" ");
}

/// 切语言:卡片与展开区里的文案在渲染点经 t() 产出,切换后按当前语言重画一遍。
export function subagentRelocalize() {
  const groups = new Set();
  for (const list of subagentRunsBySession.values()) {
    for (const run of list) {
      const ui = run.cardEl?._sa;
      if (!ui) continue;
      ui.lastState = null;
      ui.moreEl?.remove();
      ui.moreEl = null;
      if (ui.view) ui.view.built = false;
      renderSubagentCard(run, { skipGroup: true });
      if (run.groupEl) groups.add(run.groupEl);
    }
  }
  for (const group of groups) syncGroup(group);
  notify(null);
}

// 1 秒刷新:只动运行中且已挂载卡片的计数(耗时),不重写可访问名。
defer(() => {
  setInterval(() => {
    const now = Date.now();
    let any = false;
    for (const list of subagentRunsBySession.values()) {
      for (const run of list) {
        if (!SA_ACTIVE.has(run.state)) continue;
        any = true;
        const meta = run.cardEl?._sa?.meta;
        if (!meta || run.cardEl.isConnected === false) continue;
        const text = subagentMetaText(run, now);
        if (meta.textContent !== text) meta.textContent = text;
      }
    }
    if (any) for (const fn of tickListeners) fn();
  }, 1000);
});
