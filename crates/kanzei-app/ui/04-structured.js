// ---------- 结构化渲染(UI-0926 #10) ----------
// 全应用「机器格式 → 给人看」的唯一 DOM 出口:实体 chip(条目编号/路径/URL)、键值表、
// 可折叠 JSON 树、工具入参、工具 JSON 结果、tracker 字段只读视图、搜索结果、权限资源、
// 错误详情、局部校验。纯解析在 04-structured-parse.js(零 import,Node 可测);⎿ 一行摘要
// 在 05-tool-summary.js——这里只管展开区。
//
// 纪律:
// - 只用 createElement / createTextNode / append / textContent / dataset / setAttribute;
//   绝不用 innerHTML 写数据(XSS)。markdown 一律经 renderMarkdownInto 写进 div.md——
//   它先整体转义再构造,闭合的 mermaid 围栏换成图。
// - 不用 DocumentFragment(冒烟的假 DOM 没有);模块顶层不碰 document。
// - 跳转不直接依赖 11/13/17/19:点击走 structuredNav,真实实现由 19-research.js 注册,
//   冒烟可以换成 spy。
import { t } from "./02-i18n.js";
import { setDiagramHost } from "./04-diagram.js";
import { renderMarkdownInto } from "./04-markdown.js";
import {
  DISCOVERY_LABEL_KEYS,
  classifyTrackerField,
  clipText,
  displayPath,
  fillTemplate,
  formatEpoch,
  isAbsolutePath,
  normalizeRoot,
  parseConditionField,
  parseErrorText,
  parseJsonish,
  parsePermissionResource,
  parseSourceFingerprint,
  relativeToRoot,
  splitCircledList,
  splitMarkedList,
  splitSemicolonList,
  splitTimeline,
  stripAnsi,
  tokenizeRich,
} from "./04-structured-parse.js";
import { toolProjectRoot, toolRoots } from "./05-tool-summary.js";

// ---------- 导航注入 ----------
/// 默认 no-op;19-research.js 在 defer 里用 setStructuredNav 注册真实跳转。
export const structuredNav = {
  openRef(_id) {},
  openPath(_path, _line) {},
  openUrl(_url) {},
  openMemory(_scope, _id) {},
};
export function setStructuredNav(partial) {
  for (const [key, fn] of Object.entries(partial ?? {})) {
    if (typeof fn === "function") structuredNav[key] = fn;
  }
}
// 图节点点击(04-diagram.js,零 import)走同一套导航:调用时才取 structuredNav,冒烟换成 spy 照样命中。
setDiagramHost({
  t: (key) => t(key),
  openPath: (path, line) => structuredNav.openPath(path, line),
  openRef: (id) => structuredNav.openRef(id),
});

function el(tag, className = "", text = null) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== null && text !== undefined) node.textContent = String(text);
  return node;
}
/// 界面标签:写入 data-i18n-key,语言切换时由 applyDataI18nKeys 就地重算。
function label(tag, className, key) {
  const node = el(tag, className, t(key));
  node.dataset.i18nKey = key;
  return node;
}
function roots() {
  try {
    return toolRoots();
  } catch {
    return [];
  }
}
function projectRoot() {
  try {
    return toolProjectRoot();
  } catch {
    return "";
  }
}

// ---------- 语法着色(原 06-activity.js,06 转出同名导出) ----------
export function highlightLine(container, text, language) {
  const pattern = /("(?:\\.|[^"])*"|'(?:\\.|[^'])*'|\/\/.*|#.*|\b\d+(?:\.\d+)?\b|\b(?:fn|let|const|function|class|return|if|else|for|while|pub|struct|use|import|from|true|false|null|None|async|await)\b)/g;
  let cursor = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > cursor) container.appendChild(document.createTextNode(text.slice(cursor, match.index)));
    const token = document.createElement("span");
    token.className = match[0].startsWith("//") || match[0].startsWith("#") ? "syntax-comment" : /^['"]/.test(match[0]) ? "syntax-string" : /^\d/.test(match[0]) ? "syntax-number" : "syntax-keyword";
    token.textContent = match[0];
    container.appendChild(token);
    cursor = match.index + match[0].length;
  }
  if (cursor < text.length) container.appendChild(document.createTextNode(text.slice(cursor)));
}

// ---------- 实体 chip ----------
/// 条目编号。A- 只显示不跳转(没有对应视图)。
export function refChip(id) {
  const value = String(id ?? "");
  if (/^A-/.test(value)) {
    const span = el("span", "sv-chip sv-ref is-static", value);
    span.dataset.ref = value;
    return span;
  }
  const button = el("button", "sv-chip sv-ref", value);
  button.type = "button";
  button.dataset.ref = value;
  button.title = `${t("跳转到")} ${value}`;
  button.addEventListener("click", (event) => {
    event?.stopPropagation?.();
    structuredNav.openRef(value);
  });
  return button;
}
/// 路径 chip:项目根/工作树下显示相对路径,过长只留末两段;悬浮看完整路径。
/// absolute=true(工作目录):不相对化——项目根本身相对化后只剩「.」,读不出是哪个目录。
/// 显示与跳转分开:显示可以相对化/`~/` 缩写,跳转目标只有落在**当前项目**下才用项目相对
/// 路径(file_preview 以项目根为基准);线路工作树、项目外的路径一律给完整路径——相对化后
/// 会在主项目里打开同名文件,`~/…` 则根本打不开。
export function pathChip(raw, { line = null, endLine = null, absolute = false } = {}) {
  const full = normalizeRoot(raw);
  const rel = (absolute ? displayPath(full, [], { max: Infinity }) : displayPath(full, roots(), { max: Infinity })) || full;
  const segments = rel.split("/").filter(Boolean);
  const short = rel.length > 44 && segments.length > 2 ? `…/${segments.slice(-2).join("/")}` : rel;
  const suffix = line ? `:${line}${endLine ? `-${endLine}` : ""}` : "";
  const button = el("button", "sv-chip sv-path", `${short}${suffix}`);
  button.type = "button";
  const target = absolute || !isAbsolutePath(full) ? full : (relativeToRoot(full, projectRoot()) ?? full);
  button.title = `${target}${suffix}`;
  button.dataset.path = target;
  if (line) button.dataset.line = String(line);
  button.addEventListener("click", (event) => {
    event?.stopPropagation?.();
    structuredNav.openPath(target, line || null);
  });
  return button;
}
/// URL chip:显示 host+path(截 60 字),点开走应用内查看器。
export function urlChip(url) {
  const value = String(url ?? "");
  const match = value.match(/^[a-z][\w+.-]*:\/\/([^/?#\s]+)([^?#\s]*)/i);
  const shown = match ? `${match[1]}${match[2] === "/" ? "" : match[2]}` : value;
  const button = el("button", "sv-chip sv-url", clipText(shown, 60));
  button.type = "button";
  button.title = value;
  button.dataset.url = value;
  button.addEventListener("click", (event) => {
    event?.stopPropagation?.();
    structuredNav.openUrl(value);
  });
  return button;
}

/// 富文本:编号/路径/URL 变成可点 chip,其余原样。
/// 项目根/工作树根下的绝对路径按根整体认领(根可能带空格),再切通用实体。
export function richText(text, { className = "" } = {}) {
  const span = el("span", `sv-rich${className ? ` ${className}` : ""}`);
  for (const token of tokenizeRich(text, { roots: roots() })) {
    if (token.type === "ref") span.append(refChip(token.value));
    else if (token.type === "path") span.append(pathChip(token.path, token));
    else if (token.type === "url") span.append(urlChip(token.value));
    else span.append(document.createTextNode(token.value));
  }
  return span;
}

// ---------- 值 ----------
const isScalar = (value) => value === null || ["string", "number", "boolean"].includes(typeof value);
const isPlainObject = (value) => Boolean(value) && typeof value === "object" && !Array.isArray(value);
const MARKDOWN_HINT = /^\s{0,3}#{1,6}\s|^\s*[-*+]\s|^\s*\d+[.)]\s|```|^\s*\|.*\|\s*$/m;

function markdownBlock(text) {
  const box = el("div", "md sv-md");
  renderMarkdownInto(box, String(text ?? ""));
  return box;
}
/// 多行字符串原样显示真实换行,绝不再 JSON 转义成字面的 `\n`。
function stringBlock(text) {
  return el("pre", "sv-str-block", stripAnsi(text));
}
function scalarNode(value) {
  if (value === null || value === undefined) return el("span", "sv-bool", "null");
  if (typeof value === "number") return el("span", "sv-num", String(value));
  if (typeof value === "boolean") return el("span", "sv-bool", String(value));
  const text = String(value);
  if (text.includes("\n")) return MARKDOWN_HINT.test(text) ? markdownBlock(text) : stringBlock(text);
  if (text.length > 120) return stringBlock(text);
  return richText(text, { className: "sv-str" });
}

/// 键值表:值递归走 renderValue。labels 把机器键映射成界面标签(title 保留原键)。
export function renderKV(pairs, { labels = null, className = "", depth = 0 } = {}) {
  const list = Array.isArray(pairs) ? pairs : Object.entries(pairs ?? {});
  const box = el("div", `sv-kv${className ? ` ${className}` : ""}`);
  box.setAttribute("data-i18n-raw", "");
  for (const [key, value] of list) {
    const row = el("div", "sv-kv-row");
    row.dataset.key = String(key);
    const labelKey = labels?.[key];
    const keyNode = labelKey ? label("span", "sv-k", labelKey) : el("span", "sv-k", key);
    if (labelKey) keyNode.title = String(key);
    const valueNode = el("div", "sv-v");
    valueNode.append(renderValue(value, { name: key, depth: depth + 1 }));
    row.append(keyNode, valueNode);
    box.append(row);
  }
  return box;
}

function flatTableColumns(rows) {
  if (!rows.length || rows.length > 50) return null;
  if (!rows.every((row) => isPlainObject(row) && Object.values(row).every(isScalar))) return null;
  const columns = [...new Set(rows.flatMap((row) => Object.keys(row)))];
  return columns.length && columns.length <= 8 ? columns : null;
}
function renderTable(rows, columns) {
  const table = el("table", "sv-table");
  table.setAttribute("data-i18n-raw", "");
  const head = el("tr");
  for (const column of columns) head.append(el("th", "", column));
  const thead = el("thead");
  thead.append(head);
  const tbody = el("tbody");
  for (const row of rows) {
    const tr = el("tr");
    for (const column of columns) {
      const td = el("td");
      if (column in row) td.append(scalarNode(row[column]));
      tr.append(td);
    }
    tbody.append(tr);
  }
  table.append(thead, tbody);
  return table;
}

/// 值的统一分派:tracker 列表/条目、搜索结果、键值表、扁平表格、字符串,其余 JSON 树。
export function renderValue(value, { name = "", depth = 0 } = {}) {
  if (isScalar(value) || value === undefined) return scalarNode(value);
  if (isPlainObject(value)) {
    if (Array.isArray(value.entries) && value.entries.every((entry) => isPlainObject(entry) && typeof entry.id === "string")) {
      return renderTrackerList(value);
    }
    if (typeof value.id === "string" && "lifecycle_status" in value) return renderTrackerEntry(value);
    if (Array.isArray(value.results) && typeof value.query === "string") return renderSearchResults(value);
    const keys = Object.keys(value);
    if (depth <= 1 && keys.length && keys.length <= 12) return renderKV(value, { depth });
    return renderJsonTree(value, { openDepth: depth === 0 ? 1 : 0, toolbar: depth === 0 });
  }
  if (Array.isArray(value)) {
    if (!value.length) return el("span", "sv-bool", "[]");
    const columns = flatTableColumns(value);
    if (columns) return renderTable(value, columns);
    if (value.length <= 30 && value.every((item) => isScalar(item) && String(item ?? "").length <= 120 && !String(item ?? "").includes("\n"))) {
      const list = el("span", "sv-inline-list");
      value.forEach((item, index) => {
        if (index) list.append(document.createTextNode(", "));
        list.append(scalarNode(item));
      });
      return list;
    }
    return renderJsonTree(value, { openDepth: depth === 0 ? 1 : 0, toolbar: depth === 0 });
  }
  return el("span", "sv-str", String(value));
}

// ---------- JSON 树 ----------
async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    /* 剪贴板不可用(无焦点/权限)时静默:原始 JSON 仍在折叠区可手动选中。 */
  }
}
/// 可折叠 JSON 树:对象/数组是 details.sv-node(depth < openDepth 时展开);多行/超长字符串
/// 是 pre.sv-str-block(真实换行);超过 maxItems 给「还有 N 项」按钮;超过 maxNodes 停止
/// 展开并提示看原始 JSON。根部工具条:复制 JSON + 原始 JSON 折叠区。
export function renderJsonTree(value, { openDepth = 1, maxNodes = 1500, maxItems = 100, toolbar = true } = {}) {
  const root = el("div", "sv-json");
  root.setAttribute("data-i18n-raw", "");
  const state = { nodes: 0, maxNodes, maxItems, openDepth, capped: false };
  root.append(jsonNode(value, undefined, 0, state));
  if (state.capped) root.append(label("div", "sv-note", "节点过多,其余内容见原始 JSON"));
  if (toolbar) root.append(jsonToolbar(value));
  return root;
}
/// 「复制 JSON」+「原始 JSON」折叠区:结构化视图之外,原文永远拿得到。
function jsonToolbar(value) {
  const raw = JSON.stringify(value, null, 2) ?? "";
  const tools = el("div", "sv-json-tools");
  const copy = label("button", "ghost mini sv-copy-json", "复制 JSON");
  copy.type = "button";
  copy.addEventListener("click", (event) => {
    event?.stopPropagation?.();
    void copyText(raw);
  });
  const details = el("details", "sv-raw");
  details.append(label("summary", "", "原始 JSON"), el("pre", "sv-str-block", raw.length > 8000 ? `${raw.slice(0, 8000)}\n…(${t("已截断")})` : raw));
  tools.append(copy, details);
  return tools;
}
function jsonKey(key) {
  const span = el("span", "sv-key", key);
  return span;
}
function jsonNode(value, key, depth, state) {
  state.nodes += 1;
  const row = el("div", "sv-json-row");
  const keyed = key !== undefined;
  if (!isPlainObject(value) && !Array.isArray(value)) {
    if (keyed) row.append(jsonKey(key), document.createTextNode(": "));
    row.append(scalarNode(value));
    return row;
  }
  const entries = Array.isArray(value) ? value.map((item, index) => [index, item]) : Object.entries(value);
  const details = el("details", "sv-node");
  if (depth < state.openDepth) details.open = true;
  const summary = el("summary");
  if (keyed) summary.append(jsonKey(key), document.createTextNode(": "));
  const count = Array.isArray(value)
    ? `[] ${fillTemplate(t("{n} 项"), { n: entries.length })}`
    : `{} ${fillTemplate(t("{n} 个字段"), { n: entries.length })}`;
  summary.append(el("span", "sv-count", count));
  const children = el("div", "sv-children");
  const appendRange = (from, to) => {
    for (const [childKey, child] of entries.slice(from, to)) {
      if (state.nodes >= state.maxNodes) {
        state.capped = true;
        return false;
      }
      children.append(jsonNode(child, childKey, depth + 1, state));
    }
    return true;
  };
  let shown = Math.min(entries.length, state.maxItems);
  if (appendRange(0, shown) && entries.length > shown) {
    const more = el("button", "ghost mini sv-more", fillTemplate(t("还有 {n} 项"), { n: entries.length - shown }));
    more.type = "button";
    more.addEventListener("click", (event) => {
      event?.stopPropagation?.();
      const next = Math.min(entries.length, shown + state.maxItems);
      more.remove();
      state.maxNodes += next - shown;
      appendRange(shown, next);
      shown = next;
      if (shown < entries.length) {
        more.textContent = fillTemplate(t("还有 {n} 项"), { n: entries.length - shown });
        children.append(more);
      }
    });
    children.append(more);
  }
  details.append(summary, children);
  row.append(details);
  return row;
}

// ---------- 工具入参 ----------
const PATH_ARG_KEYS = new Set(["path", "file_path", "file", "workdir", "cwd", "dir", "directory", "root"]);
const PROSE_ARG_KEYS = new Set(["prompt", "question", "content", "body", "description", "summary", "message", "note", "reason"]);
const BODY_ARG_KEYS = new Set(["old_string", "new_string", "content", "edits", "patch"]);
const EDIT_TOOLS = new Set(["edit", "multiedit", "write", "insert", "apply_patch"]);
export const TRACKER_FIELD_TOOLS = new Set(["req", "defect", "idea", "source", "finding", "decision"]);

function argValue(tool, key, value) {
  if (typeof value === "string") {
    if (PATH_ARG_KEYS.has(key) && value.trim() && !/\n/.test(value)) return pathChip(value);
    if (key === "command") {
      const pre = el("pre", "sv-cmd");
      highlightLine(pre, value, "bash");
      return pre;
    }
    if (value.includes("\n")) {
      if (!PROSE_ARG_KEYS.has(key)) return stringBlock(value);
      // 多行自然语言:折叠,summary 是首行,正文按 markdown 渲染。
      const details = el("details", "sv-prose");
      const first = value.split(/\r?\n/).find((line) => line.trim()) ?? "";
      details.append(el("summary", "", clipText(first.replace(/^#+\s*/, ""), 80)), markdownBlock(value));
      return details;
    }
    return scalarNode(value);
  }
  if (key === "fields" && TRACKER_FIELD_TOOLS.has(tool) && isPlainObject(value)) return renderTrackerFields(value);
  return renderValue(value, { name: key, depth: 1 });
}
function argRows(tool, pairs) {
  const kv = el("div", "sv-kv");
  for (const [key, value] of pairs) {
    const row = el("div", "sv-kv-row");
    row.dataset.key = key;
    const valueNode = el("div", "sv-v");
    valueNode.append(argValue(tool, key, value));
    row.append(el("span", "sv-k", key), valueNode);
    kv.append(row);
  }
  return kv;
}
/// 工具入参:按键值表渲染;路径成 chip、命令成代码块、多行说明折叠成 markdown;
/// edit/write 已有 diff/新建展示时,正文类参数收进「原始入参」,外面只露 path 等。
/// 根上 dataset.raw 是入参 JSON(复制/核对用),超过 RAW_ARGS_MAX 字截断——完整入参已经
/// 逐键渲染在键值表里,大 write/edit 的正文不该在 DOM 属性里再存一整份。空入参返回 null。
const RAW_ARGS_MAX = 8000;
export function renderToolArgs(name, input, { display = null, className = "" } = {}) {
  if (!isPlainObject(input) || !Object.keys(input).length) return null;
  const tool = String(name ?? "");
  const box = el("div", `${className ? `${className} ` : ""}sv-args`);
  box.setAttribute("data-i18n-raw", "");
  const rawJson = JSON.stringify(input, null, 2);
  box.dataset.raw = rawJson.length > RAW_ARGS_MAX ? `${rawJson.slice(0, RAW_ARGS_MAX)}\n…` : rawJson;
  const foldBodies = EDIT_TOOLS.has(tool) && ["diff", "create"].includes(display?.kind);
  const shown = [];
  const folded = [];
  for (const [key, value] of Object.entries(input)) {
    (foldBodies && BODY_ARG_KEYS.has(key) ? folded : shown).push([key, value]);
  }
  if (shown.length) box.append(argRows(tool, shown));
  if (folded.length) {
    const details = el("details", "sv-raw-args");
    details.append(label("summary", "", "原始入参"), argRows(tool, folded));
    box.append(details);
  }
  return box;
}

/// 工具 JSON 结果的展开区(自动吃到 .tool-msg-detail > .tool-display 的 420px 上限)。
export function renderToolResult(name, value) {
  const box = el("div", "tool-display sv-result");
  box.dataset.tool = String(name ?? "");
  const view = renderValue(value, { name, depth: 0 });
  box.append(view);
  // 键值表/条目行/搜索结果之外也给原文出口(JSON 树自带工具条,不重复)。
  if (!view.classList.contains("sv-json")) box.append(jsonToolbar(value));
  return box;
}

// ---------- tracker ----------
function statusChip(status) {
  const chip = el("span", "sv-chip sv-status", status);
  chip.dataset.status = String(status ?? "");
  return chip;
}
/// tracker list:每行 编号 · 标题 · 状态 · 阻塞原因;全部阻塞时的指引做成提示条。
export function renderTrackerList(value) {
  const box = el("div", "sv-tracker-list");
  box.setAttribute("data-i18n-raw", "");
  if (typeof value.deadlock_guidance === "string" && value.deadlock_guidance.trim()) {
    box.append(el("div", "sv-note is-warn", value.deadlock_guidance));
  }
  const entries = value.entries ?? [];
  for (const entry of entries.slice(0, 200)) {
    const row = el("div", `sv-tl-row${entry.blocked ? " is-blocked" : ""}`);
    row.dataset.id = entry.id;
    row.append(refChip(entry.id), el("span", "sv-tl-title", entry.title ?? ""), statusChip(entry.lifecycle_status ?? entry.status ?? ""));
    const reasons = Array.isArray(entry.block_reasons) ? entry.block_reasons.filter(Boolean) : [];
    if (reasons.length) row.append(richText(reasons.join("; "), { className: "sv-tl-reason" }));
    box.append(row);
  }
  if (entries.length > 200) box.append(el("div", "sv-note", fillTemplate(t("还有 {n} 项"), { n: entries.length - 200 })));
  return box;
}
/// tracker get:头(编号/标题/状态/阻塞原因)+ 字段只读视图。
export function renderTrackerEntry(value) {
  const box = el("div", "sv-tracker-entry");
  box.setAttribute("data-i18n-raw", "");
  const head = el("div", "sv-te-head");
  head.append(refChip(value.id), el("strong", "sv-te-title", value.title ?? ""), statusChip(value.lifecycle_status ?? ""));
  if (value.archived) head.append(label("span", "sv-chip", "已归档"));
  box.append(head);
  const reasons = Array.isArray(value.block_reasons) ? value.block_reasons.filter(Boolean) : [];
  if (reasons.length) {
    const list = el("ul", "sv-te-reasons");
    for (const reason of reasons) {
      const item = el("li");
      item.append(richText(reason));
      list.append(item);
    }
    box.append(list);
  }
  if (Array.isArray(value.fields) && value.fields.length) box.append(renderTrackerFields(value.fields));
  return box;
}

/// 四种字段形状统一成 [{key, value, unknown}]:`[[k,v]]`、`[{name,value}]`、`[{key,value}]`、`{k:v}`。
export function normalizeTrackerFields(fields) {
  const list = Array.isArray(fields) ? fields : isPlainObject(fields) ? Object.entries(fields) : [];
  const stringify = (value) => (typeof value === "string" ? value : value === null || value === undefined ? "" : JSON.stringify(value));
  return list.map((item) => {
    if (Array.isArray(item)) return { key: String(item[0] ?? ""), value: stringify(item[1]), unknown: false };
    if (isPlainObject(item)) {
      const key = String(item.key ?? item.name ?? "");
      return { key, value: stringify(item.value), unknown: item.known === false || item.presentation === "gray" };
    }
    return null;
  }).filter((item) => item && item.key);
}

function listNode(value, { compact = false, className = "" } = {}) {
  const circled = splitCircledList(value);
  const marked = circled ? null : splitMarkedList(value);
  const semicolon = circled || marked ? null : splitSemicolonList(value);
  if (!circled && !marked && !semicolon) return null;
  const wrap = el("div", `tf-listwrap${className ? ` ${className}` : ""}`);
  const intro = circled?.intro || marked?.intro;
  if (intro && !compact) wrap.append(richText(intro, { className: "tf-intro" }));
  const list = el(circled ? "ol" : "ul", `tf-list${marked ? " tf-marked" : ""}`);
  const items = circled ? circled.items.map((text) => ({ text })) : marked ? marked.items : semicolon.map((text) => ({ text }));
  const visible = compact ? items.slice(0, 3) : items;
  for (const item of visible) {
    const li = el("li");
    if (item.label) li.append(el("strong", "tf-mark", item.label), document.createTextNode(" "));
    li.append(richText(item.text));
    list.append(li);
  }
  wrap.append(list);
  if (compact && items.length > visible.length) wrap.append(el("span", "tf-more", `+${items.length - visible.length}`));
  return wrap;
}
function proseNode(value) {
  const text = richText(value);
  if ([...value].length <= 240) return text;
  const wrap = el("div", "tf-prose");
  text.classList.add("sv-clamp");
  const toggle = label("button", "ghost mini sv-expand", "展开全部");
  toggle.type = "button";
  toggle.addEventListener("click", (event) => {
    event?.stopPropagation?.();
    const clamped = text.classList.toggle("sv-clamp");
    const key = clamped ? "展开全部" : "收起";
    toggle.dataset.i18nKey = key;
    toggle.textContent = t(key);
  });
  wrap.append(text, toggle);
  return wrap;
}
function conditionNode(value) {
  const parsed = parseConditionField(value);
  if (!parsed.owner && !parsed.release) return proseNode(value);
  const box = el("div", "tf-cond");
  if (parsed.reason) box.append(richText(parsed.reason, { className: "tf-cond-reason" }));
  const chips = el("div", "tf-cond-chips");
  if (parsed.owner) {
    const chip = el("span", "sv-chip tf-owner");
    chip.append(label("span", "", "恢复人"), document.createTextNode(`: ${parsed.owner}`));
    chips.append(chip);
  }
  if (parsed.release) {
    const chip = el("span", "sv-chip tf-release");
    chip.append(label("span", "", "解除条件"), document.createTextNode(": "), richText(parsed.release));
    chips.append(chip);
  }
  box.append(chips);
  return box;
}
function timelineNode(value, { compact = false } = {}) {
  const segments = splitTimeline(value);
  if (segments.length <= 1) {
    const text = segments[0]?.text ?? value;
    const body = listNode(text, { compact }) ?? proseNode(text);
    if (!segments[0]?.date) return body;
    // 单段也保留段首日期(来源/进展常以日期开头),不能被切掉。
    const wrap = el("div", "tf-dated");
    wrap.append(el("span", "sv-chip tf-date", segments[0].date), body);
    return wrap;
  }
  const visible = compact ? segments.slice(0, 1) : segments;
  const list = el("ol", "tf-timeline");
  for (const segment of visible) {
    const li = el("li");
    if (segment.date) li.append(el("span", "sv-chip tf-date", segment.date));
    li.append(listNode(segment.text, { compact }) ?? richText(segment.text));
    list.append(li);
  }
  if (compact && segments.length > 1) {
    const wrap = el("div");
    wrap.append(list, el("span", "tf-more", `+${segments.length - 1}`));
    return wrap;
  }
  return list;
}
function metaNode(key, value) {
  const item = el("span", "tf-meta-item");
  item.dataset.field = key;
  item.append(el("span", "tf-meta-key", key));
  if (key === "标签") {
    for (const tag of value.split(/[\s,，]+/).filter(Boolean)) item.append(el("span", "sv-chip tf-tag", tag));
    return item;
  }
  const batch = key === "批次" ? value.match(/^(\d+)\s*\/\s*(\d+)$/) : null;
  if (batch && Number(batch[2]) > 0) {
    const bar = el("span", "tf-progress");
    bar.setAttribute("role", "img");
    bar.setAttribute("aria-label", value);
    const fill = el("span", "tf-progress-fill");
    fill.style.setProperty("--tf-progress", `${Math.min(100, Math.round((Number(batch[1]) / Number(batch[2])) * 100))}%`);
    bar.append(fill);
    item.append(el("span", "sv-chip", value), bar);
    return item;
  }
  item.append(richText(value, { className: "sv-chip" }));
  return item;
}
function engineNode(engine) {
  const details = el("details", "tf-engine");
  details.append(label("summary", "", "引擎记录"));
  const body = el("div", "tf-engine-body");
  const parts = [];
  for (const { key, value } of engine) {
    let shown = value;
    if (key === "observed_head") shown = value.slice(0, 8);
    else if (key === "recorded_at") {
      const ms = formatEpoch(value);
      shown = ms ? new Date(ms).toLocaleString() : value;
    }
    const part = el("span", "tf-engine-item", shown);
    part.title = `${key}: ${value}`;
    part.dataset.field = key;
    parts.push(part);
  }
  parts.forEach((part, index) => {
    if (index) body.append(document.createTextNode(" · "));
    body.append(part);
  });
  details.append(body);
  return details;
}
const COMPACT_KEYS = ["进展", "验收", "复现", "内容", "影响"];
/// tracker 字段只读视图(需求/缺陷单页详情、tracker get 结果、req 入参)。
/// 按字段类别渲染:元数据成顶部 chip 行、编号/路径可点、停车/阻塞拆出恢复人与解除条件、
/// ①②③/批N/「；」切成列表、「||」切成时间线、字段值是 JSON(发现记录)成键值表、
/// 引擎字段收进末尾「引擎记录」折叠区。compact=true(卡片)只渲染进展/验收/复现/内容/影响
/// 的前 3 个,列表只露前 3 项,时间线只取第一段,不渲染元数据与引擎记录。
export function renderTrackerFields(fields, { compact = false, grayKeys = null } = {}) {
  const box = el("div", `tf${compact ? " tf-compact" : ""}`);
  box.setAttribute("data-i18n-raw", "");
  const gray = new Set(grayKeys ?? []);
  const items = normalizeTrackerFields(fields);
  const meta = [];
  const engine = [];
  const rows = [];
  for (const item of items) {
    const kind = classifyTrackerField(item.key);
    if (kind === "engine") engine.push(item);
    else if (kind === "meta") meta.push(item);
    else rows.push({ ...item, kind });
  }
  const visibleRows = compact
    ? COMPACT_KEYS.map((key) => rows.find((row) => row.key === key)).filter((row) => row && row.value.trim()).slice(0, 3)
    : rows;
  if (!compact && meta.length) {
    const metaRow = el("div", "tf-meta");
    for (const { key, value } of meta) if (value.trim()) metaRow.append(metaNode(key, value.trim()));
    if (metaRow.children.length) box.append(metaRow);
  }
  for (const row of visibleRows) {
    const value = row.value.trim();
    if (!value && compact) continue;
    const line = el("div", `tf-row${compact ? " doc-field" : ""}${row.unknown || gray.has(row.key) ? " tf-unknown" : ""}`);
    line.dataset.field = row.key;
    line.dataset.kind = row.kind;
    const val = el("div", "tf-val");
    const json = value.startsWith("{") || value.startsWith("[") ? parseJsonish(value) : null;
    if (json && typeof json === "object") {
      val.append(isPlainObject(json) ? renderKV(json, { labels: DISCOVERY_LABEL_KEYS, depth: 1 }) : renderValue(json, { depth: 1 }));
    } else if (!value) {
      val.append(el("span", "sv-bool", "—"));
    } else if (row.kind === "refs") {
      val.append(richText(value, { className: "tf-refs" }));
    } else if (row.kind === "condition") {
      val.append(conditionNode(value));
    } else if (row.kind === "list") {
      val.append(listNode(value, { compact }) ?? proseNode(value));
    } else if (row.kind === "timeline") {
      val.append(timelineNode(value, { compact }));
    } else {
      val.append(proseNode(value));
    }
    line.append(el("span", "tf-key", row.key), val);
    box.append(line);
  }
  if (!compact && engine.length) box.append(engineNode(engine));
  return box;
}

// ---------- 测试记录 ----------
/// 测试记录字段(test_runs_snapshot 的 `[{key,value}]`,也吃 `[[k,v]]`):命令按「; 」拆成代码
/// 列表、收尾时间戳转本地时间、源码指纹拆成逐文件路径 chip + hash,其余走富文本。
export function renderTestRecordFields(fields) {
  const box = el("div", "sv-kv sv-test-fields");
  box.setAttribute("data-i18n-raw", "");
  for (const { key, value } of normalizeTrackerFields(fields)) {
    const row = el("div", "sv-kv-row");
    row.dataset.key = key;
    const cell = el("div", "sv-v");
    const text = value.trim();
    if (key === "命令") {
      const list = el("ul", "sv-cmd-list");
      for (const command of text.split(/;\s+/).map((item) => item.trim()).filter(Boolean)) {
        const li = el("li");
        li.append(el("code", "", command));
        list.append(li);
      }
      cell.append(list);
    } else if (key === "收尾") {
      const ms = formatEpoch(text);
      const time = el("span", "sv-num", ms ? new Date(ms).toLocaleString() : text);
      time.title = text;
      cell.append(time);
    } else if (key === "时长") {
      cell.append(el("span", "sv-chip", text));
    } else if (key === "源码指纹") {
      const fingerprint = parseSourceFingerprint(text);
      if (fingerprint?.items.length) {
        for (const item of fingerprint.items) {
          const line = el("div", "sv-fingerprint");
          line.append(pathChip(item.path), document.createTextNode(" "), el("code", "", item.hash));
          cell.append(line);
        }
      } else {
        cell.append(el("code", "", text));
      }
    } else if (key === "摘要") {
      const parts = splitSemicolonList(text);
      if (parts) {
        const list = el("ul", "tf-list");
        for (const part of parts) {
          const li = el("li");
          li.append(richText(part));
          list.append(li);
        }
        cell.append(list);
      } else {
        cell.append(richText(text));
      }
    } else {
      cell.append(richText(text));
    }
    row.append(el("span", "sv-k", key), cell);
    box.append(row);
  }
  return box;
}

// ---------- 搜索结果 ----------
export function renderSearchResults(value) {
  const box = el("div", "sv-search");
  box.setAttribute("data-i18n-raw", "");
  const query = el("div", "sv-search-query");
  query.append(label("span", "sv-k", "查询"), el("span", "sv-chip", value.query ?? ""));
  box.append(query);
  for (const result of value.results ?? []) {
    const item = el("div", "sv-search-result");
    const url = String(result?.url ?? "");
    const title = el("button", "sv-search-title", result?.title || url);
    title.type = "button";
    title.title = url;
    title.dataset.url = url;
    title.addEventListener("click", (event) => {
      event?.stopPropagation?.();
      if (url) structuredNav.openUrl(url);
    });
    const host = url.match(/^[a-z][\w+.-]*:\/\/([^/?#\s]+)/i)?.[1] ?? "";
    item.append(title);
    if (host) item.append(el("span", "sv-search-host", host));
    const snippet = String(result?.snippet ?? result?.description ?? "").trim();
    if (snippet) item.append(el("div", "sv-search-snippet", clipText(snippet, 300)));
    box.append(item);
  }
  return box;
}

// ---------- 权限资源 ----------
/// bash:命令代码块 + 「工作目录」路径 chip;路径类:路径 chip + 灰色全路径;其余原样。
export function renderPermissionResource(action, resource) {
  const parsed = parsePermissionResource(action, resource);
  const box = el("div", `sv-perm is-${parsed.kind}`);
  box.setAttribute("data-i18n-raw", "");
  if (parsed.kind === "command") {
    const pre = el("pre", "sv-cmd");
    highlightLine(pre, parsed.command, "bash");
    box.append(pre);
    if (parsed.workdir) {
      const line = el("div", "sv-perm-workdir");
      line.append(label("span", "sv-k", "工作目录"), pathChip(parsed.workdir, { absolute: true }));
      box.append(line);
    }
  } else if (parsed.kind === "path") {
    const chip = pathChip(parsed.path);
    box.append(chip);
    const full = normalizeRoot(parsed.path);
    if (full !== chip.dataset.path) box.append(el("span", "sv-perm-full", full));
  } else {
    box.append(el("code", "sv-perm-text", parsed.raw));
  }
  return box;
}

// ---------- 错误详情 ----------
/// provider HTTP 错误体/错误链 → 人话消息 + chips + 原因链 + 原始错误。dataset.raw 保留原文。
export function renderErrorDetail(message) {
  const raw = String(message ?? "");
  const info = parseErrorText(raw);
  const box = el("div", "sv-error");
  box.setAttribute("data-i18n-raw", "");
  box.dataset.raw = raw;
  if (!info.json && !info.chain.length) {
    box.append(el("div", "sv-error-message", raw));
    return box;
  }
  if (info.head) box.append(el("div", "sv-error-head", info.head));
  const text = info.message ?? (info.chain.length ? info.chain[info.chain.length - 1] : "");
  if (text) box.append(el("div", "sv-error-message", text));
  const chips = el("div", "sv-error-chips");
  if (info.status) chips.append(el("span", "sv-chip sv-error-status", `HTTP ${info.status}`));
  for (const [key, value] of Object.entries(info.fields)) {
    const chip = el("span", "sv-chip", `${key}: ${value}`);
    chip.dataset.field = key;
    chips.append(chip);
  }
  if (chips.children.length) box.append(chips);
  if (info.chain.length > 1) {
    const details = el("details", "sv-error-chain");
    const list = el("ol");
    for (const cause of info.chain) list.append(el("li", "", cause));
    details.append(label("summary", "", "原因链"), list);
    box.append(details);
  }
  const rawDetails = el("details", "sv-error-raw");
  rawDetails.append(label("summary", "", "原始错误"), info.json ? renderJsonTree(info.json, { toolbar: false }) : el("pre", "sv-str-block", raw));
  box.append(rawDetails);
  return box;
}

// ---------- 局部校验 ----------
const CHECK_GLYPHS = { passed: "✓", failed: "✗" };
/// display.local_validation:每个检查一个 chip(✓/✗/○ + kind,title 为命令);失败项下挂
/// 首个错误(路径可点)与修复上下文。
export function renderLocalValidation(lv) {
  const checks = Array.isArray(lv?.checks) ? lv.checks : [];
  if (!checks.length) return null;
  const box = el("div", "sv-checks");
  box.setAttribute("data-i18n-raw", "");
  const chips = el("div", "sv-check-chips");
  chips.append(label("span", "sv-k", "局部校验"));
  const failures = [];
  for (const check of checks) {
    const status = String(check?.status ?? "");
    const chip = el("span", `sv-chip sv-check is-${status || "unknown"}`, `${CHECK_GLYPHS[status] ?? "○"} ${check?.kind ?? ""}`);
    chip.dataset.status = status;
    if (check?.command) chip.title = String(check.command);
    chips.append(chip);
    if (status === "failed") failures.push(check);
  }
  box.append(chips);
  for (const check of failures) {
    const fail = el("div", "sv-check-fail");
    if (check.first_error) {
      const line = el("div", "sv-check-error");
      line.append(label("span", "sv-k", "首个错误"), richText(String(check.first_error)));
      fail.append(line);
    }
    if (check.repair_context) {
      const details = el("details", "sv-check-context");
      details.append(label("summary", "", "修复上下文"), stringBlock(String(check.repair_context)));
      fail.append(details);
    }
    if (fail.children.length) box.append(fail);
  }
  return box;
}

// ---------- 延迟构建 ----------
/// 插入占位,首次展开(flushLazy)时再构建:历史回放一窗 120 条,大 JSON 不在首屏构建。
export function lazyMount(host, build) {
  const slot = el("div", "sv-lazy");
  slot._kzBuild = build;
  host.append(slot);
  return slot;
}
export function flushLazy(host) {
  if (!host?.querySelectorAll) return 0;
  let built = 0;
  for (const slot of [...host.querySelectorAll(".sv-lazy")]) {
    const build = slot._kzBuild;
    slot._kzBuild = null;
    let node = null;
    try {
      node = typeof build === "function" ? build() : null;
    } catch (error) {
      node = el("pre", "sv-str-block", String(error?.message ?? error));
    }
    if (node) slot.replaceWith(node);
    else slot.remove();
    built += 1;
  }
  return built;
}
