import { moduleNames, laneNames, receiptLabel, latestSentence, plainText, visibleReply, interactionKind, usageSeries, usageSummary, sparkPoints, sparkPath, formatCount } from "./25-softwire-model.js";
import { languageIsEnglish, localizedDocStatus, t } from "./02-i18n.js";
import { fillTemplate } from "./04-structured-parse.js";
import { needsRequirementResearch, requirementResearchStatus } from "./26-requirement-research.js";

// 概览整页的文案:长句走 t()(词条在 02-i18n.js),短词/同字不同义的词用 en(中文, English)直接给出两种说法,
// 免得和别处的同字词条(「文件」=file、「交付」=delivered)撞译。
const en = (zh, english) => (languageIsEnglish() ? english : zh);
const LANE_EN = { work: "Requirements", memory: "Memory", history: "History", tools: "Tools", batch: "Verification & delivery", tasks: "Subtasks", usage: "Run usage" };
const laneLabel = module => (languageIsEnglish() ? LANE_EN[module] ?? laneNames[module] : laneNames[module]);
const comma = () => (languageIsEnglish() ? ", " : "，");
// 后端给的默认对话名(「主对话」「独立任务 N」「讨论 N」)是中文字面量:英文界面照词表翻;用户自己起的名字原样。
export function lineLabel(label) {
  const generated = /^(主对话|讨论|独立任务)(?: (\d+))?$/.exec(String(label ?? "").trim());
  return generated ? `${t(generated[1])}${generated[2] ? ` ${generated[2]}` : ""}` : String(label ?? "");
}

export function node(tag, text, className = "") {
  const element = document.createElement(tag);
  if (text != null) element.textContent = String(text);
  if (className) element.className = className;
  return element;
}
export function button(label, action, className = "sw-link") {
  const element = node("button", label, className);
  element.type = "button";
  element.addEventListener("click", action);
  return element;
}
export const glyphs = { work: "◎", batch: "▤", main: "☼", memory: "▱", tools: "›_", tasks: "♧", history: "▥", inbox: "☷", usage: "∿" };
const emptyToolRecord = state => state.trace?.error ? t("记录读取失败") : state.trace?.loaded ? t("记录中未见调用") : t("记录未加载");
// 概览六行的顺序与每行右侧的次级入口(「运行画像」并进第 7 行「运行用量」)。
const laneOrder = ["work", "memory", "history", "tools", "batch", "tasks"];
const laneLinks = Object.freeze({
  work: [["conventions", "开发规范", "Conventions"]],
  memory: [["memory", "记忆图谱", "Memory graph"], ["files", "文件", "Files"], ["arch", "架构", "Architecture"]],
  history: [["chat", "历史对话", "Chat history"]],
  tasks: [["lines", "并行线路", "Parallel tasks"]],
});
// 活动波形只在有真实活动的行上动;事件脉冲(kick)是单次有限动画,轮询与回放历史记录从不触发。
export function stopSignals(root) {
  for (const wave of root?.querySelectorAll(".sw-wave") || []) wave.getAnimations?.().forEach(animation => animation.cancel());
}
function kick(graph, module) {
  const wave = graph.querySelector(`[data-module="${module}"] .sw-wave`);
  if (!wave?.animate) return;
  wave.getAnimations().forEach(animation => animation.cancel());
  const animation = wave.animate([{ transform: "scaleY(1)" }, { transform: "scaleY(1.55)", offset: .3 }, { transform: "scaleY(1)" }], { duration: 700, easing: "ease-out" });
  animation.onfinish = () => animation.cancel();
}

export function createWorkspace(actions) {
  const root = node("section", null, "sw-workspace");
  root.id = "softwire-workspace";
  const identified = (el, id) => { el.id = id; return el; };
  const heading = node("header", null, "sw-heading"), title = node("div");
  title.append(identified(button("", () => actions.module("work"), "sw-requirement"), "sw-requirement"), identified(node("h1"), "sw-title"));
  const runtime = identified(node("span"), "sw-runtime"); runtime.setAttribute("role", "status");
  const controls = node("div", null, "sw-heading-actions");
  controls.append(runtime); heading.append(title, controls);
  const batch = node("div", null, "sw-batch"), stages = node("ol");
  stages.setAttribute("aria-label", t("当前工作阶段"));
  for (const [status, label] of [["active", en("实施", "Implement")], ["verifying", en("验证", "Verify")], ["done", en("交付", "Deliver")]]) {
    const stage = node("li", label); stage.dataset.stage = status; stages.append(stage);
  }
  batch.append(identified(button("", () => actions.module("batch")), "sw-batch"), stages, identified(button("", () => actions.module("batch")), "sw-checkpoint"));
  const tabs = node("nav", null, "sw-tabs"); tabs.setAttribute("aria-label", t("工作面"));
  for (const [id, label] of [["network", t("概览")], ["conversation", t("对话")], ["inbox", t("待我处理") + " "]]) {
    const tab = button(label, () => actions.tab(id)); tab.dataset.tab = id;
    if (id === "inbox") tab.append(identified(node("span"), "sw-message-count"));
    tabs.append(tab);
  }
  const refresh = identified(button("↻", actions.refresh), "sw-refresh"); refresh.setAttribute("aria-label", t("刷新工作状态"));
  tabs.append(node("span", null, "sw-spacer"), refresh);
  const surface = identified(node("div", null, "sw-surface"), "sw-surface"); surface.tabIndex = -1;
  root.append(heading, batch, tabs, surface, identified(node("div"), "sw-dock-slot"));
  return root;
}

function resourceLinks(entries, actions) {
  const links = node("nav", null, "sw-resources");
  for (const [view, label, english] of entries) {
    const link = button(en(label, english ?? t(label)) + "  ↗", () => actions.resource(view)); link.dataset.resource = view; links.append(link);
  }
  return links;
}
// 一行 = 图标 + 名称 + 一行状态 + 右侧一条直线基线;有活动时基线变橙色起伏波形(见 softwire.css 的 .sw-wave)。
function createLane(module, actions) {
  const lane = node("div", null, "sw-lane"); lane.dataset.lane = module; lane.setAttribute("role", "listitem");
  const main = button("", () => actions.module(module), "sw-node sw-node-" + module);
  main.dataset.module = module; main.dataset.state = "idle";
  main.setAttribute("aria-label", laneLabel(module));
  const copy = node("span", null, "sw-node-copy");
  copy.append(node("strong", laneLabel(module)), node("small", ""));
  const wave = node("span", null, "sw-wave"); wave.setAttribute("aria-hidden", "true");
  main.append(node("span", glyphs[module], "sw-glyph"), copy, wave);
  lane.append(main);
  if (laneLinks[module]) lane.append(resourceLinks(laneLinks[module], actions));
  return lane;
}
// 第 7 行「运行用量」:最近 N 轮的 token 迷你波形 + 均值,点开展开三条曲线与逐轮明细;完整页仍是「运行画像」。
function createUsageLane(actions) {
  const lane = node("div", null, "sw-lane sw-lane-usage"); lane.dataset.lane = "usage"; lane.setAttribute("role", "listitem");
  const main = button("", () => actions.usageToggle(), "sw-node sw-node-usage");
  main.dataset.module = "usage"; main.dataset.state = "idle";
  main.setAttribute("aria-expanded", "false"); main.setAttribute("aria-controls", "sw-usage-detail");
  const copy = node("span", null, "sw-node-copy");
  copy.append(node("strong", laneLabel("usage")), node("small", ""));
  const spark = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  spark.setAttribute("class", "sw-spark"); spark.setAttribute("aria-hidden", "true"); spark.setAttribute("preserveAspectRatio", "none");
  main.append(node("span", glyphs.usage, "sw-glyph"), copy, spark);
  lane.append(main, resourceLinks([["metrics", "运行画像", "Run profile"]], actions));
  const detail = node("div", null, "sw-usage-detail"); detail.id = "sw-usage-detail"; detail.hidden = true;
  lane.append(detail);
  return lane;
}

export function renderGraph(root, state, actions) {
  const retained = root.querySelector(".sw-overview-layout");
  if (retained && root._graphProject === state.project.path) {
    updateGraph(root, state, actions);
    return;
  }
  root.replaceChildren(); root._graphProject = state.project.path;
  const layout = node("div", null, "sw-overview-layout");
  const focus = node("div", null, "sw-focus-slot"); layout.append(focus);
  const observation = node("div", null, "sw-observation");
  const graph = node("div", null, "sw-network");
  graph.setAttribute("role", "list"); graph.setAttribute("aria-label", t("运行概览"));
  for (const module of laneOrder) graph.append(createLane(module, actions));
  graph.append(createUsageLane(actions));
  const recent = node("section", null, "sw-recent"); recent.setAttribute("aria-label", t("模型最近回复"));
  const live = button("", actions.conversation, "sw-live-reply"); live.setAttribute("aria-label", t("最新一句，打开完整对话"));
  live.append(node("span", "", "sw-live-label"), node("span", "", "sw-live-text"));
  const history = node("div", null, "sw-recent-history"); recent.append(live, history);
  observation.append(graph); layout.append(observation, recent); root.append(layout);
  root.append(button("", () => actions.interaction(root._waiting), "sw-attention"), node("p", "", "sw-error"));
  updateGraph(root, state, actions);
}

function updateUsage(graph, state, actions) {
  const lane = graph.querySelector(".sw-lane-usage"), main = lane.querySelector(".sw-node-usage"), detail = lane.querySelector(".sw-usage-detail");
  const usage = state.usage || {}, series = usageSeries(usage.rounds), summary = usageSummary(series), open = Boolean(actions.usageOpen?.());
  const running = state.line?.activity === "running";
  // 展开时右列改为自然高度(由 .sw-surface 滚动),不再被固定行高裁住或压到下面的最近回复。
  const layout = graph.closest(".sw-overview-layout");
  if (layout) layout.dataset.usageOpen = String(open);
  const text = usage.error ? t("读取失败 · 点开重试") : !usage.loaded ? t("读取中…") : !series.length ? t("暂无运行记录")
    : fillTemplate(t("近 {n} 轮 · 均 {steps} 步 · {tokens} token · {calls} 次调用"), { n: series.length, steps: Math.round(summary.steps * 10) / 10, tokens: formatCount(summary.tokens), calls: Math.round(summary.calls * 10) / 10 });
  main.dataset.state = running ? "running" : "idle";
  main.setAttribute("aria-expanded", String(open));
  main.setAttribute("aria-label", laneLabel("usage") + comma() + text);
  const small = main.querySelector("small");
  if (small.textContent !== text) small.textContent = text;
  // 迷你波形:每轮 token 总量,点与点之间是直线段;最后一个点是最新一轮。
  const spark = main.querySelector(".sw-spark"), sparkKey = JSON.stringify(series.map(row => [row.at, row.tokens]));
  if (spark._key !== sparkKey) {
    spark._key = sparkKey; spark.replaceChildren();
    spark.setAttribute("viewBox", "0 0 200 22");
    if (series.length) {
      const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
      path.setAttribute("d", sparkPath(series.map(row => row.tokens), 200, 22, 4));
      const [x, y] = sparkPoints(series.map(row => row.tokens), 200, 22, 4).at(-1);
      const dot = document.createElementNS("http://www.w3.org/2000/svg", "path");
      dot.setAttribute("d", `M${x} ${y}h0`); dot.setAttribute("class", "sw-spark-dot");
      spark.append(path, dot);
    } else {
      const base = document.createElementNS("http://www.w3.org/2000/svg", "line");
      for (const [name, value] of [["x1", 0], ["y1", 11], ["x2", 200], ["y2", 11]]) base.setAttribute(name, String(value));
      spark.append(base);
    }
  }
  detail.hidden = !open;
  const detailKey = JSON.stringify([open, usage.error, series.map(row => [row.at, row.steps, row.tokens, row.calls, row.failed, row.outcome])]);
  if (open && detail._key !== detailKey) { detail._key = detailKey; renderUsageDetail(detail, series, summary, usage.error, actions); }
}
function renderUsageDetail(detail, series, summary, error, actions) {
  detail.replaceChildren();
  if (error) {
    detail.append(node("p", t("运行用量读取失败：") + error, "sw-error"), button(t("重试读取"), () => actions.usageRetry(), "sw-link"));
    return;
  }
  if (!series.length) { detail.append(node("p", t("这个项目还没有已完成的运行轮次"), "sw-empty")); return; }
  const charts = node("div", null, "sw-usage-charts");
  for (const [key, label, mean] of [["steps", t("步数"), summary.steps], ["tokens", "token", summary.tokens], ["calls", en("工具调用", "Tool calls"), summary.calls]]) {
    const figure = node("figure", null, "sw-usage-chart"), values = series.map(row => row[key]);
    figure.append(node("figcaption", fillTemplate(t("{label} · 均 {mean} · 最高 {max}"), { label, mean: key === "tokens" ? formatCount(mean) : Math.round(mean * 10) / 10, max: key === "tokens" ? formatCount(Math.max(...values)) : Math.max(...values) })));
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 160 40"); svg.setAttribute("preserveAspectRatio", "none"); svg.setAttribute("role", "img");
    svg.setAttribute("aria-label", fillTemplate(t("{label}：最近 {n} 轮"), { label, n: series.length }));
    const line = document.createElementNS("http://www.w3.org/2000/svg", "path");
    line.setAttribute("d", sparkPath(values, 160, 40, 4)); line.setAttribute("class", "sw-usage-line");
    svg.append(line);
    // 每一轮一条整高的悬停带,原生提示给出时间与具体数值。
    const step = 160 / values.length;
    for (const [index, value] of values.entries()) {
      const hit = document.createElementNS("http://www.w3.org/2000/svg", "rect"), tip = document.createElementNS("http://www.w3.org/2000/svg", "title");
      for (const [name, number] of [["x", index * step], ["y", 0], ["width", step], ["height", 40]]) hit.setAttribute(name, String(number));
      hit.setAttribute("class", "sw-usage-hit");
      tip.textContent = `${new Date(series[index].at).toLocaleString()} · ${label} ${key === "tokens" ? formatCount(value) : value}`;
      hit.append(tip); svg.append(hit);
    }
    figure.append(svg); charts.append(figure);
  }
  detail.append(charts);
  const list = node("ol", null, "sw-usage-rounds");
  list.setAttribute("aria-label", t("逐轮用量"));
  for (const row of series.slice(-8).reverse()) {
    const item = node("li"), when = new Date(row.at);
    item.append(node("time", Number.isNaN(when.getTime()) ? "" : when.toLocaleString([], { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })),
      node("span", t(({ completed: "完成", halted: "中断" })[row.outcome] || row.outcome || "")),
      node("span", fillTemplate(t("{n} 步"), { n: row.steps })), node("span", `↑${formatCount(row.input)} ↓${formatCount(row.output)}`), node("span", fillTemplate(t("{n} 次调用"), { n: row.calls })));
    if (row.failed) item.append(node("span", fillTemplate(t("失败 {n}"), { n: row.failed }), "sw-usage-failed"));
    list.append(item);
  }
  detail.append(list, button(t("完整运行画像") + "  ↗", () => actions.resource("metrics")));
}

function updateGraph(root, state, actions) {
  const graph = root.querySelector(".sw-network");
  const activity = state.line?.activity || (state.waiting ? "attention" : state.running ? "running" : "idle");
  if (activity !== "running") stopSignals(graph);
  const unresolved = state.tools.filter(tool => tool.live && tool.pending);
  const pending = activity === "running" ? unresolved : [];
  const latest = state.tools.reduce((last, tool) => !last || (tool.observedAt || 0) >= (last.observedAt || 0) ? tool : last, null);
  const tool = unresolved.at(-1) || latest, activeTool = pending.includes(tool);
  const memoryTool = pending.find(tool => /memory|recall/.test(tool.name));
  const writingMemory = memoryTool && /write|save|update|add|delete|forget|store/.test(memoryTool.name + " " + (memoryTool.input?.action || ""));
  const waitingTasks = activeTool && tool.name === "task";
  const children = state.tasks.filter(task => !task.historical);
  const childState = ["running", "starting", "stopping", "waiting"].find(status => children.some(task => task.state === status));
  const missingResult = tool?.interrupted || (tool?.pending && !activeTool);
  const suspended = tool?.live && tool.pending && ["attention", "stopping"].includes(activity);
  const toolState = activeTool ? waitingTasks ? "waiting" : "working" : suspended ? activity : missingResult ? "stopped" : tool ? "result" : "idle";
  const running = activity === "running", unitStatus = state.unit?.status;
  // 每行:[一行状态, 基线状态]。基线状态沿用旧词表:running/working/reading/writing/starting 画橙色起伏,
  // waiting/stopping 虚线,attention/blocked 琥珀,failed 红,其余(idle/result/stopped/unknown)中性直线。
  const statuses = {
    work: [state.item ? state.item.id + " · " + (state.item.title || "") : t("尚未领取需求"), running && state.item ? "running" : "idle"],
    memory: [memoryTool ? writingMemory ? t("写入中") : t("读取中") : t("检索与管理"), memoryTool ? writingMemory ? "writing" : "reading" : "idle"],
    history: [running && state.compacting ? t("压缩中") : state.compaction ? t("已有压缩记录") : t("查看运行记录"), running && state.compacting ? "working" : "idle"],
    tools: [tool ? tool.name + " · " + (activeTool ? waitingTasks ? t("等待子任务") : pending.length > 1 ? fillTemplate(t("{n} 项调用中"), { n: pending.length }) : t("调用中") : suspended ? activity === "attention" ? t("待你回复") : t("停止中") : missingResult ? t("结果未确认") : tool.ok === false ? t("最近调用失败 · 查看记录") : t("最近调用完成")) : emptyToolRecord(state), toolState],
    tasks: [childState ? fillTemplate(t(({ running: "{n} 项执行中", starting: "{n} 项启动中", stopping: "{n} 项停止中", waiting: "{n} 项待批准" })[childState]), { n: children.filter(task => task.state === childState).length }) : state.tasks.length ? fillTemplate(t("{n} 个任务 · 查看记录"), { n: state.tasks.length }) : t("尚无子任务"), childState === "waiting" ? "attention" : childState || "idle"],
    batch: [state.batchEvidenceCount ? fillTemplate(t("{n} 条批次记录"), { n: state.batchEvidenceCount }) : state.deliveryCount ? fillTemplate(t("{n} 个交付文件"), { n: state.deliveryCount }) : unitStatus === "done" ? t("机器已完成") : unitStatus === "verifying" ? t("验证阶段") : unitStatus === "blocked" ? t("批次受阻") : t("查看当前批次"),
      unitStatus === "blocked" ? "blocked" : unitStatus === "verifying" && running ? "working" : unitStatus === "done" || state.deliveryCount || state.batchEvidenceCount ? "result" : "idle"],
  };
  for (const [module, [detail, status]] of Object.entries(statuses)) {
    const el = graph.querySelector(`[data-module="${module}"]`);
    el.dataset.state = status;
    if (module === "tools") el.dataset.result = !activeTool && tool?.ok === false ? "failed" : "";
    const symbol = status === "attention" ? "?" : status === "blocked" ? "!" : module === "tools" && tool && !activeTool ? missingResult ? "—" : tool.ok === false ? "!" : "✓"
      : module === "batch" && status === "result" ? "✓" : null;
    el.querySelector(".sw-glyph").textContent = symbol ?? glyphs[module];
    el.setAttribute("aria-label", laneLabel(module) + comma() + detail);
    if (el.querySelector("small").textContent !== detail) el.querySelector("small").textContent = detail;
  }
  updateUsage(graph, state, actions);
  // 基线本身常驻;波形只随真实工具事件脉动一次(不随轮询、不随回放历史记录)。
  graph._signals ??= new Map();
  const signalKeys = new Set(state.tools.map(event => JSON.stringify([event.sessionId, event.id])));
  for (const key of graph._signals.keys()) if (!signalKeys.has(key)) graph._signals.delete(key);
  for (const event of state.tools) {
    const key = JSON.stringify([event.sessionId, event.id]);
    if (!event.live || event.observedAt == null || graph._signals.get(key) === event.observedAt) continue;
    graph._signals.set(key, event.observedAt);
    if (activity !== "running" || event.interrupted || performance.now() - event.observedAt > 1200 || document.hidden || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) continue;
    kick(graph, "tools");
    if (/memory|recall/.test(event.name)) kick(graph, "memory");
    if (event.name === "task") kick(graph, "tasks");
  }
  const focus = root.querySelector(".sw-focus-slot");
  const queueKey = JSON.stringify([state.project.current_items, state.project.lines, state.backlog, state.line?.id]);
  if (focus._key !== queueKey) {
    const scroll = focus.querySelector(".sw-work-rows")?.scrollTop || 0, open = focus.querySelector("details")?.open;
    focus.replaceChildren(); renderWorkFocus(focus, state, actions); focus._key = queueKey;
    focus.querySelector(".sw-work-rows").scrollTop = scroll;
    if (focus.querySelector("details")) focus.querySelector("details").open = Boolean(open);
  }
  const recent = root.querySelector(".sw-recent");
  // 只剩内部交接块的回复(过滤后为空)不进预览,免得出现空行。
  const allReplies = (state.messages || []).filter(m => m.role === "assistant" && visibleReply(m.text));
  const latestReply = allReplies.at(-1), live = recent.querySelector(".sw-live-reply");
  // 「最新一句」只在模型正流式输出时出现;输出结束后它就是下方「最近回复」的第一条,同屏不重复(B29)。
  const streaming = state.running && latestReply?.status === "streaming";
  live.hidden = !streaming; live.dataset.streaming = String(Boolean(streaming));
  live.querySelector(".sw-live-label").textContent = t("正在回复");
  live.querySelector(".sw-live-text").textContent = latestSentence(latestReply?.text) || t("等待模型输出…");
  const replies = allReplies.filter(m => m.status !== "streaming").slice(-3).reverse();
  const recentKey = JSON.stringify([state.line?.session_id, replies.map(m => [m.id, m.text, m.status]), state.replyError]);
  if (recent._key !== recentKey) {
    const history = recent.querySelector(".sw-recent-history");
    history.replaceChildren(); recent._key = recentKey;
    const heading = node("header"); heading.append(node("span", t("最近回复")), button(t("完整对话") + " ↗", actions.conversation)); history.append(heading);
    if (state.replyError) {
      const retry = button(t("回复读取失败 · 重试"), actions.retryReplies, "sw-link sw-recent-retry"); retry.title = state.replyError; history.append(retry);
    } else if (!replies.length) history.append(node("p", t("模型回复会出现在这里"), "sw-recent-empty"));
    for (const [index, reply] of replies.entries()) {
      const row = button("", () => actions.reply(reply), "sw-recent-reply");
      row.dataset.messageId = reply.id;
      row.append(node("span", index === 0 ? en("最新", "Latest") : "·", "sw-recent-mark"), node("span", plainText(reply.text).replace(/\s+/g, " "), "sw-recent-text"));
      row.title = t("打开完整对话"); history.append(row);
    }
  }
  root._waiting = state.waiting;
  const attention = root.querySelector(".sw-attention"); attention.hidden = !state.waiting; attention.textContent = state.waiting ? "☷  " + state.waiting.body : "";
  const error = root.querySelector(":scope > .sw-error"); error.hidden = !state.error; error.textContent = state.error || "";
}

export function renderWorkFocus(parent, state, actions) {
  const section = node("section", null, "sw-work-focus");
  section.setAttribute("aria-label", t("当前需求"));
  const heading = node("header");
  const all = button("☷", () => actions.list("req")); all.setAttribute("aria-label", t("打开完整需求与缺陷列表"));
  // 记需求 / 记缺陷与标题同一行:窄栏里省出一整行,待办表和需求卡不再被挤出可视区(UX-056)。
  heading.append(button(t("需求"), () => actions.list("req")), node("span", null, "sw-spacer"),
    button(t("＋ 记需求"), () => actions.capture("req"), "sw-capture"), button(t("＋ 记缺陷"), () => actions.capture("defect"), "sw-capture"), all);
  section.append(heading);
  const items = state.project.current_items || [], lines = state.project.lines || [], represented = new Set();
  const rows = node("div", null, "sw-work-rows");
  const render = (line, item) => {
    if (item) represented.add(item.id);
    const row = node("div", null, "sw-work-row"); row.dataset.selected = String(Boolean(line && line.id === state.line?.id));
    if (line) {
      const identity = button(lineLabel(line.label), () => actions.line(line), "sw-line-select");
      identity.title = t("查看这个对话");
      identity.dataset.running = String(Boolean(line.running)); row.append(identity);
    }
    if (!item) row.append(node("span", line?.running ? t("运行中，尚未关联需求") : t("未绑定条目"), "sw-work-unclaimed"));
    else {
      const entry = button("", () => actions.item(item), "sw-work-entry"); entry.title = item.title;
      entry.dataset.blocked = String(Boolean(item.blocked));
      const meta = node("span", null, "sw-work-meta");
      const status = node("span", localizedDocStatus(item.status), "sw-work-state");
      meta.append(node("span", item.id), status);
      if (needsRequirementResearch(item)) meta.append(node("span", t(requirementResearchStatus(item)), "sw-work-blocked"));
      if (item.blocked) meta.append(node("span", t("阻塞"), "sw-work-blocked"));
      meta.append(node("span", item.priority || "", "sw-work-priority"));
      entry.append(meta, node("strong", item.title));
      if (item.batches?.total) {
        const progress = node("span", null, "sw-work-progress"), blocks = node("span", null, "sw-batch-blocks");
        const { done, total } = item.batches;
        blocks.setAttribute("aria-hidden", "true");
        for (let i = 0, count = Math.min(total, 12); i < count; i++) { const block = node("i"); block.dataset.done = String(i < Math.floor(done / total * count)); block.dataset.blocked = String(Boolean(item.blocked) && i === Math.floor(done / total * count)); blocks.append(block); }
        progress.append(blocks, node("span", fillTemplate(t("批次 {done}/{total}"), { done, total }))); entry.append(progress);
      }
      if (item.blocked) {
        const reason = (item.block_reasons || []).map(reason => typeof reason === "string" ? reason : reason.reason || reason.message || "").filter(Boolean).join(languageIsEnglish() ? "; " : "；") || t("等待外部条件");
        const blocked = node("span", reason, "sw-work-reason"); blocked.title = reason; entry.append(blocked);
      }
      row.append(entry);
    }
    rows.append(row);
  };
  for (const line of lines) {
    const item = items.find(item => item.id === line.current_item_id || item.owner_lines?.some(owner => owner.id === line.id));
    if (item || line.running) render(line, item);
  }
  for (const item of items) if (!represented.has(item.id)) render(null, item);
  if (!rows.children.length) rows.append(node("p", t("暂无活动需求"), "sw-work-unclaimed"));
  section.append(rows);
  const idle = lines.filter(line => !items.some(item => item.id === line.current_item_id || item.owner_lines?.some(owner => owner.id === line.id)));
  if (idle.length) {
    const picker = node("details", null, "sw-idle-lines");
    picker.append(node("summary", fillTemplate(t("未绑定需求的对话 · {n}"), { n: idle.length })));
    for (const line of idle) picker.append(button(lineLabel(line.label) || line.id, () => actions.line(line), "sw-line-select"));
    section.append(picker);
  }
  if ((state.project.current_items_total || 0) > items.length) section.append(button(t("查看全部进行中的工作 ›"), () => actions.list("req")));
  const table = node("table", null, "sw-backlog"); table.setAttribute("aria-label", t("未完成统计"));
  const head = node("tr"); for (const label of [t("未完成"), t("可执行"), t("阻塞")]) head.append(node("th", label)); table.append(head);
  for (const [kind, label] of [["req", t("需求")], ["defect", t("缺陷")]]) {
    const tally = state.backlog?.[kind], row = node("tr"), name = node("td");
    name.append(button(label + (tally ? " " + tally.active : ""), () => actions.list(kind))); row.append(name);
    for (const key of ["workable", "blocked"]) { const cell = node("td", tally?.[key] ?? "—"); cell.dataset.kind = key; row.append(cell); }
    table.append(row);
  }
  section.append(table);
  if (state.backlog?.error) section.append(node("small", t("需求读取失败，点刷新重试"), "sw-error"));
  parent.append(section);
}

export function detailHeading(root, name, back) {
  root.replaceChildren();
  const header = node("header", null, "sw-detail-head");
  header.append(button("←", back, "sw-back"), node("h2", name));
  header.firstChild.setAttribute("aria-label", t("返回上一级"));
  root.append(header);
}
export function facts(root, entries) {
  const list = node("dl", null, "sw-facts");
  for (const [label, value] of entries) {
    if (value == null || value === "") continue;
    list.append(node("dt", label), node("dd", Array.isArray(value) ? value.join("\n") : value));
  }
  root.append(list);
}
export function evidence(root, title, content, metadata, back) {
  detailHeading(root, title, back);
  if (metadata) root.append(node("p", metadata, "sw-evidence-meta"));
  const pre = node("pre", content || t("没有可用正文"), "sw-evidence");
  pre.tabIndex = 0; root.append(pre);
}
export function renderMessages(root, messages) {
  const following = root.scrollHeight - root.scrollTop - root.clientHeight < 72;
  const rows = new Map([...root.querySelectorAll("[data-message-id]")].map(el => [el.dataset.messageId, el]));
  const keep = new Set(messages.map(message => message.id));
  for (const [id, row] of rows) if (!keep.has(id)) row.remove();
  if (!messages.length && !root.childElementCount) root.append(node("p", t("在下方给当前对话发消息"), "sw-empty"));
  if (messages.length) root.querySelector(".sw-empty")?.remove();
  for (const [index, message] of messages.entries()) {
    let row = rows.get(message.id);
    if (!row) {
      row = node("article", null, "sw-message"); row.dataset.messageId = message.id;
      row.append(node("small", null, "sw-message-owner"), node("div", null, "sw-message-body"), node("small", null, "sw-message-receipt"));
      root.append(row);
    }
    if (root.children[index] !== row) root.insertBefore(row, root.children[index] || null);
    row.dataset.role = message.role;
    row.querySelector(".sw-message-owner").textContent = message.role === "user" ? t("你") : t(message.recipient || "主对话");
    // 助手回复里的内部交接块与对话页一样不显示;整条只剩交接块的已完成回复写成「交付记录已登记」。
    const shown = message.role === "assistant" ? visibleReply(message.text) : message.text;
    row.querySelector(".sw-message-body").textContent = (shown || (message.role === "user" ? "" : message.text && message.status !== "streaming" ? t("交付记录已登记") : t("正在处理…")))
      + (message.files?.length ? "\n" + message.files.map(name => "▧ " + name).join("\n") : "");
    row.querySelector(".sw-message-receipt").textContent = message.error || t(receiptLabel(message.status));
  }
  if (following) root.scrollTop = root.scrollHeight;
}
export function renderInboxList(root, messages, open) {
  root.replaceChildren(node("h2", t("待我处理")));
  if (!messages.length) root.append(node("p", t("没有待我处理的事项"), "sw-empty"));
  for (const message of messages) {
    const kind = interactionKind(message);
    const row = button("", () => open(message), "sw-inbox-row");
    row.dataset.interactionId = message.key;
    const title = node("span");
    title.append(node("small", [message.project.split(/[\\/]/).filter(Boolean).at(-1), message.source?.agentId ? message.source.source : ""].filter(Boolean).join(" · ")), node("strong", message.body));
    row.append(node("span", kind === "question" ? "?" : kind === "decision" ? "◇" : "✓", "sw-inbox-icon"), title,
      node("small", t(({ question: "待你回复", decision: "待你复核", delivery: "待你试用" })[kind])));
    root.append(row);
  }
}
export function renderInteraction(root, message, actions) {
  const kind = interactionKind(message);
  detailHeading(root, t(({ question: "回复提问", decision: "复核决定", delivery: "试用反馈" })[kind]), actions.back);
  root.append(node("p", message.project, "sw-evidence-meta"), node("h3", message.body, "sw-question"));
  if (message.source?.agentId) root.querySelector(".sw-evidence-meta").textContent += ` · ${message.source.source || message.source.agentId}`;
  if (message.answer) root.append(node("p", message.answer, "sw-answer"));
  if (message.source?.resolution?.rationale) {
    const detail = node("details", null, "sw-reason"); detail.append(node("summary", t("查看依据")), node("p", message.source.resolution.rationale)); root.append(detail);
  }
  const choices = node("div", null, "sw-choices");
  const addChoice = (label, edit = false, note = "") => {
    const choice = button("", () => actions.choice(t(label), { edit }));
    choice.setAttribute("aria-label", t(label));
    if (note) choice.setAttribute("aria-description", note);
    choice.dataset.replyChoice = t(label); choice.dataset.replyEdit = String(edit);
    choice.append(node("span", t(label)));
    if (note) choice.append(node("small", note, "sw-choice-note"));
    choices.append(choice);
  };
  for (const option of message.choices || []) {
    const label = typeof option === "string" ? option : option.label;
    const note = typeof option === "string" ? "" : option.note || "";
    const custom = /^(其他|自定义|other\b|custom\b)/i.test(label || "")
      && /请.{0,8}(说明|补充|描述)|please.{0,12}(specify|describe|explain)/i.test(note);
    if (label) addChoice(label, custom, note);
  }
  if (kind === "decision") { addChoice("本次通过"); addChoice("需要修改", true); }
  if (message.kind === "delivery") { addChoice("验收通过"); addChoice("需要修改", true); }
  if (message.kind === "delivery") root.append(node("p", t("修改意见发给原执行；通过后才记录验收。"), "sw-evidence-meta"));
  const refresh = button(t("查看最新事项"), actions.refresh, "sw-link sw-interaction-refresh");
  refresh.hidden = true;
  choices.append(button(t("补充说明"), actions.explain, "sw-link sw-reply-explain"));
  const receipt = node("div", null, "sw-interaction-receipt");
  receipt.setAttribute("role", "status");
  root.append(choices, receipt, refresh);
}
