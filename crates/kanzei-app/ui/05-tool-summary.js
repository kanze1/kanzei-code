// ---------- 工具行人话摘要(UI-0926 #6) ----------
// 主对话工具行、历史回放、活动面板、子代理子行、记忆对话共用的**唯一**摘要器:
// - toolArgSummary:参数列(路径相对化、长正则/长命令智能截断、代码记号标 code);
// - toolResultSummary:⎿ 结果列。按工具注册的摘要器解析真实 Rust 输出格式,
//   查不到就走兜底——兜底宁可显示「输出 N 行」,也绝不回显代码、路径、JSON;
// - renderToolSummary:把摘要 parts 渲染成带类名的 span(textContent 仍是纯文本)。
// 实时(kz:tool-end 带 content)与历史(conversation_get 的正文)喂进来的是同一份文本,
// 走同一个摘要器,两边显示逐字一致。纯解析都在 04-structured-parse.js,这里只做
// 「按工具取哪些事实、怎么说成人话」。
import { localizedDocStatus, t } from "./02-i18n.js";
import { currentProject, processItems } from "./03-shell.js";
import {
  cleanInline,
  cleanPaths,
  clipText,
  countOccurrences,
  displayPath,
  fillTemplate,
  formatByteSize,
  formatCount,
  formatDuration,
  isProse,
  lineDiffCounts,
  looksLikeNoise,
  looksLikeNumberedSource,
  mismatchFacts,
  parseJsonish,
  parsePreview,
  parseStorageMarker,
  splitAlternation,
  stripAnsi,
  stripToolOutcome,
  trackerEntryFacts,
  trackerListFacts,
  websearchFacts,
  workFacts,
} from "./04-structured-parse.js";

/// 当前项目根(路径 chip 的跳转基准:file_preview 只认项目相对路径)。
export function toolProjectRoot() {
  return typeof currentProject === "string" ? currentProject : "";
}
/// 当前项目根 + 各线路工作树根:落在其中的绝对路径一律显示成相对路径。
export function toolRoots() {
  const roots = [currentProject];
  for (const item of Array.isArray(processItems) ? processItems : []) roots.push(item?.worktree_path);
  return roots.filter((root) => typeof root === "string" && root.trim());
}

// ---------- 失败行:摘要与剩余互斥切分(原 05-chat-render.js,语义不变) ----------
/// ⎿ 行的字数预算。摘要在这里切断,剩余原文从同一个位置接着给——
/// 一个字要么在摘要里、要么在详情里,不会两边都有。
export const TOOL_PREVIEW_MAX = 110;

/// 把工具结果切成互不重叠的两段:
/// - `text`:⎿ 行的摘要,取第一行有信息量的内容(bash 的 "exit code: 0" 独占首行时顺延到下一行),
///   超过预算就截断;
/// - `rest`:摘要没覆盖到的剩余原文(被顺延跳过的行、被截断的首行尾巴、以及后续所有行)。
/// 从"取摘要"改成"切两段",是因为原先摘要与详情各自独立地从同一份 content 取一遍,
/// 详情那边靠 `full !== preview` 去重——只挡得住单行短结果,首行超长或多行一律双写。
export function toolResultSplit(content, isError) {
  const lines = String(content ?? "").split("\n");
  const informative = (line) => line.trim() && !/^exit code:\s*0$/i.test(line.trim());
  let idx = lines.findIndex(informative);
  // 全篇只有 "exit code: 0" 时仍然显示它,别把唯一的结果吞成"完成"。
  if (idx < 0) idx = lines.findIndex((line) => line.trim());
  if (idx < 0) return { text: isError ? t("失败") : t("完成"), rest: "" };
  const head = lines[idx].trim();
  const cut = head.length > TOOL_PREVIEW_MAX;
  const text = cut ? `${head.slice(0, TOOL_PREVIEW_MAX - 1)}…` : head;
  // 被顺延跳过的行没在 ⎿ 露过面,归入剩余部分而不是丢掉;首行被截时把没显示完的尾巴接上,
  // 前置的 … 与 ⎿ 行结尾的 … 呼应,读起来是明确的续接关系。
  const skipped = lines.slice(0, idx).filter((line) => line.trim());
  const tail = cut ? [`…${head.slice(TOOL_PREVIEW_MAX - 1)}`] : [];
  return { text, rest: [...skipped, ...tail, ...lines.slice(idx + 1)].join("\n") };
}

// ---------- parts ----------
const txt = (value) => ({ k: "text", v: String(value) });
const code = (value) => ({ k: "code", v: String(value) });
const num = (value) => ({ k: "num", v: formatCount(value) });
const add = (value) => ({ k: "add", v: `+${formatCount(value)}` });
const del = (value) => ({ k: "del", v: `−${formatCount(value)}` });
const SEPARATOR = " · ";

function normalizeGroup(group) {
  if (group === null || group === undefined || group === "") return [];
  if (typeof group === "string" || typeof group === "number") return [txt(group)];
  if (Array.isArray(group)) return group.flatMap((part) => normalizeGroup(part));
  if (typeof group === "object" && typeof group.k === "string") return [group];
  return [];
}
function normalizeResult(result) {
  if (result === null || result === undefined || result === "") return null;
  if (typeof result === "string") return { groups: [[txt(result)]] };
  if (Array.isArray(result)) return { groups: result.map(normalizeGroup) };
  if (typeof result === "object" && Array.isArray(result.groups)) {
    return { ...result, groups: result.groups.map(normalizeGroup) };
  }
  return null;
}
function assembleParts(groups, durationText, durAt) {
  const kept = groups.filter((group) => group.some((part) => String(part.v).trim()));
  if (durationText) kept.splice(Math.min(durAt ?? kept.length, kept.length), 0, [{ k: "dur", v: durationText }]);
  const parts = [];
  kept.forEach((group, index) => {
    if (index) parts.push(txt(SEPARATOR));
    parts.push(...group);
  });
  return parts;
}
const partsText = (parts) => parts.map((part) => part.v).join("");
/// UI2-0926 #12:文本 part 里成对的反引号切成 code part(≤80 字、不跨行),⎿ 行不再原样露出 `k/N` 的反引号。
/// 不成对的反引号原样保留;非文本 part 不动。
export function inlineCodeParts(part) {
  if (!part || part.k !== "text" || !String(part.v).includes("`")) return [part];
  const out = [];
  const pattern = /`([^`\n]{1,80})`/g;
  const value = String(part.v);
  let last = 0;
  for (const match of value.matchAll(pattern)) {
    if (match.index > last) out.push(txt(value.slice(last, match.index)));
    out.push(code(match[1]));
    last = match.index + match[0].length;
  }
  if (!out.length) return [part];
  if (last < value.length) out.push(txt(value.slice(last)));
  return out;
}
/// 计数文案:n=1 用单数专用 key(英文 "1 line of output" 而不是 "1 lines of output";
/// 中文 key 就是填好 1 的原文,中文态原样显示)。n 可带 `+` 后缀,那种一律按复数。
function countText(n, many, one) {
  return Number(n) === 1 && typeof n === "number" ? one : fillTemplate(many, { n: typeof n === "number" ? formatCount(n) : n });
}
const outputLines = (n) => countText(n, t("输出 {n} 行"), t("输出 1 行"));
const filesCount = (n) => countText(n, t("{n} 个文件"), t("1 个文件"));
const resultsCount = (n) => countText(n, t("{n} 条结果"), t("1 条结果"));

// ---------- 通用 ----------
const nonEmpty = (lines) => lines.filter((line) => line.trim());
function firstLine(lines) {
  return lines.find((line) => line.trim()) ?? "";
}
function lineCountOf(s) {
  return s.fromPreview ? s.lineCount : nonEmpty(s.lines).length;
}
/// 计数兜底:绝不回显正文。
function countFallback(s) {
  const lines = lineCountOf(s);
  if (lines >= 2) return outputLines(lines);
  if (Number.isFinite(s.bytes) && s.bytes > 0) return fillTemplate(t("输出 {size}"), { size: formatByteSize(s.bytes) });
  return s.ok ? t("完成") : t("失败");
}
/// 取第一条像人话的信息行(跳过空行、exit code: 0 与 [tool_…] 机器标记);否则计数。
function genericSummary(s) {
  for (const line of s.lines) {
    const trimmed = line.trim();
    if (!trimmed || /^exit code:\s*0$/i.test(trimmed) || /^\[tool_/.test(trimmed)) continue;
    // 噪声判据看清洗前(保留行号 Tab)与清洗后两份:合并空白会抹掉 `  12\t` 这种源码行特征。
    const clean = cleanInline(trimmed, s.roots);
    if (looksLikeNoise(cleanPaths(line, s.roots)) || looksLikeNoise(clean)) break;
    if (isProse(clean)) return clipText(clean, 72);
    break;
  }
  return countFallback(s);
}
/// 末条 / 首条满足条件的人话行。
function proseLine(lines, roots, { last = false, max = 80, test = null } = {}) {
  const ordered = last ? [...lines].reverse() : lines;
  for (const line of ordered) {
    const clean = cleanInline(line, roots);
    // 噪声判据同 genericSummary:清洗前(保留行号 Tab)与清洗后两份都看。
    if (!clean || looksLikeNoise(cleanPaths(line, roots)) || looksLikeNoise(clean) || !isProse(clean) || clean.length > max) continue;
    if (test && !test.test(clean)) continue;
    return clean;
  }
  return "";
}
function validationGroup(s) {
  const failed = Number(s.display?.local_validation?.counts?.failed)
    || Number(s.text.match(/局部结构校验发现 (\d+) 个/)?.[1]);
  return failed > 0 ? fillTemplate(t("校验 {n} 个错误"), { n: formatCount(failed) }) : null;
}
function withValidation(s, groups, key) {
  const check = validationGroup(s);
  if (!check) return { groups, key };
  return { groups: [...groups, check], key, tone: "warn" };
}
const lineTotal = (value) => (String(value ?? "") ? String(value).split("\n").length : 0);

// ---------- 各工具结果摘要器(成功态) ----------
function summarizeRead(s) {
  const head = firstLine(s.lines).trim();
  let m;
  if ((m = head.match(/^\[image\] .*\(([^,()]+), (\d+) bytes\)/))) {
    return { groups: [t("图片"), formatByteSize(m[2])], key: "read.image" };
  }
  if ((m = head.match(/^pdf: (\d+) pages; showing (\d+)-(\d+)/))) {
    return {
      groups: [fillTemplate(t("PDF 第 {from}–{to} 页"), { from: m[2], to: m[3] }), fillTemplate(t("共 {total} 页"), { total: formatCount(m[1]) })],
      key: "read.pdf",
    };
  }
  if ((m = head.match(/^notebook: (\d+) cells.*showing (\d+)-(\d+)/))) {
    return {
      groups: [fillTemplate(t("第 {from}–{to} 格"), { from: m[2], to: m[3] }), fillTemplate(t("共 {total} 格"), { total: formatCount(m[1]) })],
      key: "read.notebook",
    };
  }
  if ((m = head.match(/^\(last (\d+) lines of ([^)]+)\)/))) {
    return { groups: [fillTemplate(t("末尾 {n} 行"), { n: formatCount(m[1]) }), fillTemplate(t("文件 {size}"), { size: m[2] })], key: "read.tail" };
  }
  if ((m = head.match(/^\(empty range: (?:file|PDF text) has (\d+) lines/))) {
    return { groups: [t("空范围"), fillTemplate(t("文件共 {total} 行"), { total: formatCount(m[1]) })], key: "read.empty" };
  }
  const range = (from, to) => fillTemplate(t("第 {from}–{to} 行"), { from: formatCount(from), to: formatCount(to) });
  const total = (value) => fillTemplate(t("共 {total} 行"), { total: formatCount(value) });
  const whole = (value) => fillTemplate(t("全文 {total} 行"), { total: formatCount(value) });
  if (s.fromPreview) {
    // 旧后端只给首行 + 行数:按 limit 推断是否截断。
    const from = Number(s.previewFirst.match(/^\s*(\d+)\t/)?.[1]);
    if (!Number.isFinite(from)) return null;
    const limit = Number(s.input.limit) > 0 ? Number(s.input.limit) : 2000;
    if (s.lineCount === limit + 1) return { groups: [range(from, from + limit - 1), t("未读完")], key: "read.partial" };
    const to = from + s.lineCount - 1;
    return { groups: from === 1 ? [whole(to)] : [range(from, to), total(to)], key: from === 1 ? "read.full" : "read.range" };
  }
  const numbered = s.lines.map((line) => line.match(/^\s*(\d+)\t/)).filter(Boolean).map((hit) => Number(hit[1]));
  if (!numbered.length) return null;
  const from = numbered[0];
  const to = numbered[numbered.length - 1];
  const cut = s.text.match(/\.\.\. \(truncated at line (\d+)(?: of (\d+))?;/);
  if (cut?.[2]) return { groups: [range(from, to), total(cut[2])], key: "read.truncated" };
  if (cut || s.truncated) return { groups: [range(from, to), t("未读完")], key: "read.partial" };
  if (s.input.tail || from === 1) return { groups: [whole(to)], key: "read.full" };
  return { groups: [range(from, to), total(to)], key: "read.range" };
}

function summarizeGrep(s) {
  const head = firstLine(s.lines).trim();
  if (/^\(no matches/.test(head)) return { groups: [t("无匹配")], key: "grep.none" };
  if (s.fromPreview) return { groups: [resultsCount(s.lineCount)], key: "grep.degraded" };
  const matchesText = (n, plus) => countText(plus ? `${formatCount(n)}+` : Number(n), t("{n} 处匹配"), t("1 处匹配"));
  const filesText = (n, plus) => filesCount(plus ? `${formatCount(n)}+` : Number(n));
  const totals = s.text.match(/\(total (\d+) matches in (\d+) files\)/);
  if (totals) return { groups: [matchesText(totals[1]), filesText(totals[2])], key: "grep.count" };
  const stopped = /^\.\.\. \(stopped at limit/m.test(s.text) || s.truncated;
  const body = s.lines.filter((line) => line.trim() && !/^\.\.\. \(stopped at limit/.test(line));
  if (s.input.files_only) return { groups: [filesText(body.length, stopped)], key: "grep.files" };
  const files = new Set();
  let matches = 0;
  for (const line of body) {
    const hit = line.match(/^(.+?):(\d+): /);
    if (!hit) continue;
    matches += 1;
    files.add(hit[1]);
  }
  if (!matches) return null;
  const groups = [matchesText(matches, stopped)];
  if (files.size > 1) groups.push(filesText(files.size, stopped));
  return { groups, key: "grep.matches" };
}

function summarizeGlob(s) {
  const head = firstLine(s.lines).trim();
  if (/^\(no files match/.test(head)) return { groups: [t("无匹配文件")], key: "glob.none" };
  if (s.fromPreview) return { groups: [filesCount(s.lineCount)], key: "glob.degraded" };
  const files = s.lines.filter((line) => line.trim() && !/^\.\.\. \(\d+ more;/.test(line) && !/^\(scan capped/.test(line)).length;
  const more = Number(s.text.match(/^\.\.\. \((\d+) more;/m)?.[1]) || 0;
  const groups = [filesCount(files + more)];
  if (/^\(scan capped/m.test(s.text)) groups.push(t("扫描已封顶"));
  return { groups, key: "glob.files" };
}

function summarizeSymbols(s) {
  const head = firstLine(s.lines).trim();
  let m;
  if (/^\((?:no symbols|no public symbols|no \.rs files)/.test(head)) return { groups: [t("无符号")], key: "symbols.none" };
  if (/^\(no callers of/.test(head)) return { groups: [t("无调用方")], key: "symbols.none" };
  if ((m = head.match(/^callers of `[^`]*` \((\d+) hits?\)/))) {
    return { groups: [countText(Number(m[1]), t("{n} 处调用"), t("1 处调用"))], key: "symbols.callers" };
  }
  if ((m = s.text.match(/definition of `[^`]*` \((\d+) hits?\)/))) {
    return { groups: [countText(Number(m[1]), t("{n} 处定义"), t("1 处定义"))], key: "symbols.define" };
  }
  if (/^\(no definition of/.test(head)) return { groups: [t("未找到定义")], key: "symbols.none" };
  if ((m = head.match(/^repo map \(crates: \d+, modules: (\d+), public_symbols: (\d+)\)/))) {
    return {
      groups: [fillTemplate(t("{n} 个模块"), { n: formatCount(m[1]) }), fillTemplate(t("{n} 个符号"), { n: formatCount(m[2]) })],
      key: "symbols.map",
    };
  }
  if (s.fromPreview) return { groups: [fillTemplate(t("{n} 行"), { n: formatCount(s.lineCount) })], key: "symbols.degraded" };
  // 符号行形如 `  pub fn name:12` / `     fn name:12`(vis 为 "pub" 或两个空格)。
  const symbols = s.lines.filter((line) => /^ {2}(?:pub| {2}) \S+ \S.*:\d+$/.test(line)).length;
  const files = s.lines.filter((line) => line.startsWith("== ")).length;
  if (!symbols) return null;
  const groups = [fillTemplate(t("{n} 个符号"), { n: formatCount(symbols) })];
  if (files > 1) groups.push(filesCount(files));
  return { groups, key: "symbols.list" };
}

function summarizeFiles(s) {
  const head = firstLine(s.lines).trim();
  if (/^\(no files\)/.test(head)) return { groups: [t("无文件")], key: "files.none" };
  if (s.fromPreview) return null;
  const top = /^files by line count/.test(head);
  const items = s.lines.filter((line) => line.trim()
    && !/^files by line count/.test(line)
    && !/^\S.*\/ {2}\(\d+ files, /.test(line)).length;
  return {
    groups: [top ? filesCount(items) : fillTemplate(t("文件地图 · {n} 项"), { n: formatCount(items) })],
    key: "files",
  };
}

function editCounts(s) {
  const display = s.display;
  if (display?.kind === "diff" && Number.isFinite(Number(display.additions))) {
    return { additions: Number(display.additions), deletions: Number(display.deletions) || 0 };
  }
  return null;
}
function summarizeEdit(s) {
  const counts = editCounts(s);
  const replaced = Number(s.text.match(/replaced (\d+) occurrence/)?.[1]);
  if (counts) return withValidation(s, [[add(counts.additions), txt(" "), del(counts.deletions)]], "edit.diff");
  if (s.name === "insert" && typeof s.input.content === "string") {
    return withValidation(s, [[add(lineTotal(s.input.content.replace(/\n$/, ""))), txt(" "), del(0)]], "edit.insert");
  }
  if (typeof s.input.old_string === "string" && typeof s.input.new_string === "string") {
    const times = replaced > 1 ? replaced : 1;
    const diff = lineDiffCounts(s.input.old_string, s.input.new_string);
    const groups = [[add(diff.additions * times), txt(" "), del(diff.deletions * times)]];
    if (times > 1) groups.push(fillTemplate(t("替换 {n} 处"), { n: formatCount(times) }));
    return withValidation(s, groups, "edit.input");
  }
  if (Number.isFinite(replaced)) return withValidation(s, [fillTemplate(t("已替换 {n} 处"), { n: formatCount(replaced) })], "edit.replaced");
  if (/^inserted content/m.test(s.text)) return withValidation(s, [t("已插入")], "edit.inserted");
  return null;
}

function summarizeWrite(s) {
  const content = typeof s.input.content === "string" ? s.input.content : null;
  const lines = content !== null ? lineTotal(content.replace(/\n$/, "")) : null;
  if (s.display?.kind === "create") {
    const size = lines !== null ? fillTemplate(t("{n} 行"), { n: formatCount(lines) }) : formatByteSize(s.display.bytes);
    return withValidation(s, [t("新建"), size], "write.create");
  }
  const counts = editCounts(s);
  if (counts) return withValidation(s, [[add(counts.additions), txt(" "), del(counts.deletions)]], "write.diff");
  if (lines !== null) return withValidation(s, [fillTemplate(t("写入 {n} 行"), { n: formatCount(lines) })], "write.lines");
  const bytes = Number(s.text.match(/^wrote (\d+) bytes/m)?.[1]);
  if (Number.isFinite(bytes)) return withValidation(s, [fillTemplate(t("写入 {size}"), { size: formatByteSize(bytes) })], "write.bytes");
  return null;
}

/// bash 输出的亮点行:测试计数 > 编译错误 > 警告 > 提交 > 推送 > 其它测试框架 > 编译完成 >
/// 失败取首个报错行、成功取末条人话行 > 行数。
function bashHighlight(s, bodyLines, failed) {
  const body = bodyLines.join("\n");
  let passed = 0;
  let failedTests = 0;
  let sawTests = false;
  for (const hit of body.matchAll(/test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed/g)) {
    sawTests = true;
    passed += Number(hit[1]);
    failedTests += Number(hit[2]);
  }
  if (sawTests) {
    const passText = fillTemplate(t("{n} 通过"), { n: formatCount(passed) });
    return failedTests ? [fillTemplate(t("{n} 失败"), { n: formatCount(failedTests) }), passText] : [passText];
  }
  // 取第一条**具体**的编译错误;"could not compile … due to N previous errors" 只贡献计数。
  const compileError = bodyLines.map((line) => line.trim())
    .find((line) => /^error(?:\[E\d+\])?: \S/.test(line) && !/^error: could not compile\b/.test(line));
  const errorCount = Number(body.match(/could not compile .* due to (\d+) previous errors?/)?.[1]);
  if (compileError) {
    const groups = [[code(clipText(cleanInline(compileError, s.roots), 60))]];
    if (errorCount > 1) groups.push(fillTemplate(t("{n} 个编译错误"), { n: formatCount(errorCount) }));
    return groups;
  }
  if (errorCount) return [fillTemplate(t("{n} 个编译错误"), { n: formatCount(errorCount) })];
  const warnings = Number(body.match(/generated (\d+) warnings?/)?.[1]);
  if (warnings) return [fillTemplate(t("{n} 个警告"), { n: formatCount(warnings) })];
  const commit = bodyLines.map((line) => line.trim().match(/^\[([\w/.-]+)(?: \([^)]*\))? ([0-9a-f]{7,})\] (.+)$/)).find(Boolean);
  if (commit) return [[code(commit[2]), txt(` ${clipText(cleanInline(commit[3], s.roots), 60)}`)]];
  if (bodyLines.some((line) => /^To \S/.test(line.trim()))) {
    const pushed = bodyLines.map((line) => line.match(/->\s+(\S+)\s*$/)).find(Boolean);
    if (pushed) return [fillTemplate(t("已推送 {ref}"), { ref: pushed[1] })];
  }
  const jsTests = Number(body.match(/(\d+) passing\b/)?.[1] ?? body.match(/Tests:\s+(?:\d+ failed, )?(\d+) passed/)?.[1]);
  if (jsTests) return [fillTemplate(t("{n} 通过"), { n: formatCount(jsTests) })];
  if (!failed && bodyLines.some((line) => /^\s*Finished\b/.test(line))) return [t("编译完成")];
  if (failed) {
    const reason = proseLine(bodyLines, s.roots, { max: 80, test: /error|failed|panicked|exception|错误|失败/i })
      || bodyLines.map((line) => cleanInline(line, s.roots)).find((line) => line && !looksLikeNoise(line) && /error|failed|panicked|exception|错误|失败/i.test(line) && line.length <= 80);
    if (reason) return [reason];
  } else {
    const last = proseLine(bodyLines, s.roots, { last: true, max: 80 });
    if (last) return [last];
  }
  const count = nonEmpty(bodyLines).length;
  return count ? [outputLines(count)] : [];
}
function summarizeBash(s) {
  const display = s.display?.kind === "terminal" ? s.display : null;
  if (display?.background || /^background: true$/m.test(s.text)) {
    const id = display?.processId ?? s.text.match(/^process_id: (\S+)/m)?.[1] ?? "";
    return { groups: [t("后台运行"), id ? [code(id)] : null], key: "bash.background" };
  }
  const timedOut = Boolean(display?.timeout) || /^timeout: true\b/m.test(s.text);
  const exitLine = s.text.match(/^exit code: (\S+)/m)?.[1];
  const exit = display && display.exitCode !== undefined && display.exitCode !== null ? String(display.exitCode) : exitLine;
  // 失败却既没退出码也没超时:命令根本没跑(工具自身报错、被拦下),交给失败切分显示原因首行,
  // 不能在这里吞成空摘要或「输出 N 行」。
  if (!s.ok && !timedOut && (exit === undefined || exit === null)) return null;
  const groups = [];
  if (timedOut) groups.push(t("超时"), t("已终止"));
  else if (exit !== undefined && exit !== null) groups.push(fillTemplate(t("退出码 {code}"), { code: exit }));
  const durAt = groups.length;
  const bodyLines = s.lines.filter((line) => !/^exit code: \S+$/.test(line.trim())
    && !/^timeout: true\b/.test(line.trim())
    && !/^\[(?:no output captured before timeout|partial (?:stdout|stderr) before timeout|stderr|stdout truncated at 1 MiB|stderr truncated at 1 MiB)\]$/.test(line.trim()));
  const body = bodyLines.join("\n").trim();
  if (body === "(no output)") {
    groups.push(t("无输出"));
    return { groups, durAt, key: "bash" };
  }
  if (!body && !s.fromPreview) return { groups, durAt, key: "bash" };
  if (s.fromPreview) {
    // 旧后端:首行多半是 exit code,正文只剩行数。
    if (s.lineCount > 1) groups.push(outputLines(s.lineCount - 1));
    return { groups, durAt, key: "bash.degraded" };
  }
  groups.push(...bashHighlight(s, bodyLines, !s.ok || timedOut));
  return { groups, durAt, key: "bash" };
}

function summarizeProcess(s) {
  const head = firstLine(s.lines).trim();
  const action = String(s.input.action ?? "");
  let m;
  if (/^\(no background processes\)/.test(head)) return { groups: [t("无后台进程")], key: "process.none" };
  if ((m = head.match(/^stopped (\S+)/))) return { groups: [[txt(`${t("已停止")} `), code(m[1])]], key: "process.stop" };
  if (/was already finished/.test(head)) return { groups: [t("已结束")], key: "process.stop" };
  if (action === "list" || /^\S+ \[(?:running|exited\(|terminated)/.test(head)) {
    const count = s.lines.filter((line) => /^\S+ \[(?:running|exited\(-?\d+\)|terminated)\]/.test(line)).length;
    return { groups: [fillTemplate(t("{n} 个后台进程"), { n: formatCount(count) })], key: "process.list" };
  }
  const state = head.match(/^state: (\S+)/)?.[1];
  const body = s.lines.filter((line) => !/^state: /.test(line) && !/^\[(?:earlier output dropped|memory output bounded|persistent log on disk)/.test(line.trim()));
  const groups = [];
  if (state) groups.push(state === "running" ? t("运行中") : state);
  const last = proseLine(body, s.roots, { last: true, max: 80 });
  if (last) groups.push(last);
  else if (nonEmpty(body).length && !/^\(no output yet\)$/.test(body.join("").trim())) groups.push(outputLines(nonEmpty(body).length));
  return groups.length ? { groups, key: "process.output" } : null;
}

function summarizeGit(s) {
  const action = String(s.input.action ?? "");
  const head = firstLine(s.lines).trim();
  let m;
  if (/^committed verified staged set/.test(head) || action === "commit") {
    const subject = s.lines.map((line) => line.match(/^([0-9a-f]{7,40}) (.+)$/)).find(Boolean);
    const stat = s.text.match(/(\d+) files? changed(?:, (\d+) insertions?\(\+\))?(?:, (\d+) deletions?\(-\))?/);
    const hash = subject?.[1] ?? head.match(/\(([0-9a-f]{7,})\)/)?.[1];
    if (!hash) return null;
    const groups = [[code(hash.slice(0, 8)), subject ? txt(` ${clipText(cleanInline(subject[2], s.roots), 50)}`) : null]];
    if (stat) {
      groups.push([
        txt(`${filesCount(Number(stat[1]))} `),
        add(Number(stat[2]) || 0),
        txt(" "),
        del(Number(stat[3]) || 0),
      ]);
    }
    return { groups, key: "git.commit" };
  }
  if ((m = s.text.match(/staged (\d+) file/))) return { groups: [fillTemplate(t("已暂存 {n} 个文件"), { n: formatCount(m[1]) })], key: "git.stage" };
  if (/^\[finalize\] complete/.test(head)) return { groups: [t("测试通过"), t("已提交")], key: "git.finalize" };
  if ((m = head.match(/^fast-forwarded (\S+): (\S+) -> (\S+)/))) {
    return { groups: [[txt(`${m[1]} → `), code(m[3].slice(0, 8))]], key: "git.ff" };
  }
  if (action === "status" || /^## /.test(head) || /^\(clean worktree\)/.test(head)) {
    const changes = s.lines.filter((line) => line.trim() && !/^## /.test(line) && !/^\(clean worktree\)/.test(line)).length;
    return { groups: [changes ? fillTemplate(t("{n} 处改动"), { n: formatCount(changes) }) : t("工作区干净")], key: "git.status" };
  }
  if (action === "diff" || /^diff --git /m.test(s.text)) {
    if (/^\(no diff\)/.test(head)) return { groups: [t("无差异")], key: "git.diff" };
    const files = (s.text.match(/^diff --git /gm) ?? []).length;
    const additions = s.lines.filter((line) => line.startsWith("+") && !line.startsWith("+++")).length;
    const deletions = s.lines.filter((line) => line.startsWith("-") && !line.startsWith("---")).length;
    return {
      groups: [[txt(`${filesCount(files)} `), add(additions), txt(" "), del(deletions)]],
      key: "git.diff",
    };
  }
  if (action === "log") {
    if (/^\(no commits\)/.test(head)) return { groups: [t("无提交")], key: "git.log" };
    return { groups: [fillTemplate(t("{n} 条提交"), { n: formatCount(nonEmpty(s.lines).length) })], key: "git.log" };
  }
  return null;
}

function summarizeWebfetch(s) {
  const head = firstLine(s.lines).trim();
  const status = head.match(/^HTTP (\d+)/)?.[1];
  if (!status) return null;
  const groups = [`HTTP ${status}`];
  if (!s.fromPreview) {
    const body = s.text.split("\n").slice(1).join("\n").replace(/…\(截断\)\s*$/, "").trim();
    groups.push(fillTemplate(t("{n} 字"), { n: formatCount([...body].length) }));
    if (/…\(截断\)\s*$/.test(s.text)) groups.push(t("已截断"));
  }
  return { groups, key: "webfetch" };
}

function summarizeWebsearch(s) {
  const facts = websearchFacts(parseJsonish(s.text));
  if (facts) {
    if (!facts.count) return { groups: [t("无结果")], key: "websearch" };
    const first = cleanInline(facts.firstTitle, s.roots);
    return { groups: [resultsCount(facts.count), first ? clipText(first, 40) : null], key: "websearch" };
  }
  const count = countOccurrences(s.text, '"url"');
  return count ? { groups: [resultsCount(count)], key: "websearch.partial" } : null;
}

function summarizeTask(s) {
  if (/\(超时,未产出结果\)|subagent hit the \d+s wall-clock/.test(s.text)) {
    return { groups: [t("超时"), t("未产出结果")], key: "task.timeout" };
  }
  for (const line of s.lines) {
    if (/^\s*(?:```|~~~)/.test(line)) continue;
    const plain = cleanInline(line.replace(/^\s*(?:#{1,6}\s+|[-*+]\s+|>\s*|\d+[.)]\s+)/, "").replace(/\*\*|__|`/g, ""), s.roots);
    if (!plain) continue;
    if (looksLikeNoise(plain)) return null;
    return { groups: [clipText(plain, 80)], key: "task" };
  }
  return null;
}

function summarizeQuestion(s) {
  const answer = s.text.match(/^User answer: ([\s\S]*)$/m)?.[1];
  if (answer === undefined) return null;
  return { groups: [fillTemplate(t("用户回答: {answer}"), { answer: clipText(cleanInline(answer, s.roots), 60) })], key: "question" };
}

function summarizeTracker(s) {
  const head = firstLine(s.lines).trim();
  const action = String(s.input.action ?? "");
  let m;
  if ((m = head.match(/^added ([A-Z]+-\d+)/))) return { groups: [[txt(`${t("新增")} `), code(m[1])]], key: "tracker.add" };
  if (/^no-op:/.test(head)) return { groups: [t("无变化")], key: "tracker.noop" };
  if ((m = head.match(/^updated: (\S+) \[([^\]]+)\]/))) {
    const change = s.text.match(/^变更: (.*)$/m)?.[1] ?? "";
    const status = change.match(/(?:^|; )状态: \S+ → (\S+?)(?:;|$)/)?.[1]
      ?? (typeof s.input.status === "string" ? s.input.status : null);
    if (status) return { groups: [[code(m[1]), txt(` → ${localizedDocStatus(status)}`)]], key: "tracker.status" };
    let keys = [...change.matchAll(/(?:^|; )([^:;→\s][^:;→]{0,19}): /g)].map((hit) => hit[1].trim());
    if (!keys.length && s.input.fields && typeof s.input.fields === "object") keys = Object.keys(s.input.fields);
    keys = [...new Set(keys)].slice(0, 3);
    return {
      groups: [[code(m[1]), txt(` ${keys.length ? `${t("已更新")} ${keys.join("、")}` : t("已更新")}`)]],
      key: "tracker.update",
    };
  }
  if ((m = head.match(/^reopened (\S+)/))) return { groups: [[txt(`${t("已重开")} `), code(m[1])]], key: "tracker.reopen" };
  if ((m = head.match(/^reordered (\d+)/))) return { groups: [fillTemplate(t("已重排 {n} 条"), { n: formatCount(m[1]) })], key: "tracker.reorder" };
  if ((m = head.match(/^archived (\d+) terminal/))) return { groups: [fillTemplate(t("已归档 {n} 条"), { n: formatCount(m[1]) })], key: "tracker.archive" };
  const json = parseJsonish(s.text);
  const list = trackerListFacts(json);
  if (list) {
    const groups = [fillTemplate(t("{n} 条"), { n: formatCount(list.total) }), fillTemplate(t("{n} 条可执行"), { n: formatCount(list.executable) })];
    if (list.deadlocked) groups.push(t("全部阻塞"));
    return { groups, key: "tracker.list", tone: list.deadlocked ? "warn" : undefined };
  }
  const entry = trackerEntryFacts(json);
  if (entry) {
    return {
      groups: [[code(entry.id), txt(` ${localizedDocStatus(entry.status)}`)], clipText(cleanInline(entry.title, s.roots), 40)],
      key: "tracker.get",
    };
  }
  // req audit_acceptance_scope:验收范围对账报告。
  const mismatch = mismatchFacts(json);
  if (mismatch) {
    return {
      groups: [mismatch.count ? fillTemplate(t("不一致 {n}"), { n: formatCount(mismatch.count) }) : t("无不一致")],
      key: "tracker.audit",
      tone: mismatch.count ? "warn" : undefined,
    };
  }
  if (action === "list" && /^\{/.test(head)) {
    const count = countOccurrences(s.text, '"lifecycle_status"');
    if (count) return { groups: [fillTemplate(t("{n} 条"), { n: `${formatCount(count)}+` })], key: "tracker.list.partial" };
  }
  return null;
}

function summarizeWork(s) {
  const action = String(s.input.action ?? "");
  const head = firstLine(s.lines).trim();
  if (/^model completion declared/.test(head) || action === "handoff") return { groups: [t("已声明完成")], key: "work.handoff" };
  const json = parseJsonish(s.text);
  const facts = workFacts(json) ?? (() => {
    const decision = s.text.match(/"decision":\s*"(\w+)"/)?.[1];
    return decision ? { decision, id: s.text.match(/"selected":\s*\{[\s\S]*?"id":\s*"([^"]+)"/)?.[1] ?? "", title: "" } : null;
  })();
  if (!facts) return null;
  if (facts.claimed) return { groups: [[txt(`${t("已认领")} `), code(facts.claimed)]], key: "work.claim" };
  const item = (label) => [[txt(`${label} `), facts.id ? code(facts.id) : null], facts.title ? clipText(cleanInline(facts.title, s.roots), 40) : null];
  switch (facts.decision) {
    case "resume": return { groups: item(t("继续")), key: "work.next" };
    case "start": return { groups: item(t("开始")), key: "work.next" };
    case "blocked": return { groups: [t("全部阻塞")], key: "work.next", tone: "warn" };
    case "wip_violation": return { groups: [t("WIP 冲突")], key: "work.next", tone: "warn" };
    case "empty": return { groups: [t("无可执行条目")], key: "work.next" };
    default: return null;
  }
}

function testStatusLabel(status) {
  if (status === "passed") return t("通过");
  if (status === "failed") return t("失败");
  if (status === "running") return t("运行中");
  if (status === "skipped") return t("跳过");
  return status;
}
function summarizeTestRecord(s) {
  const head = firstLine(s.lines).trim();
  const id = head.match(/^recorded (T-\d+)/)?.[1];
  if (!id && !/^recorded\b/.test(head)) return null;
  const idPart = id ? [txt(`${t("已记录")} `), code(`T-…${id.slice(-4)}`)] : [txt(t("已记录"))];
  const summaryText = String(s.input.summary ?? "");
  const passed = Number(summaryText.match(/(\d+) passed/)?.[1]);
  const failed = Number(summaryText.match(/(\d+) failed/)?.[1]);
  const groups = [idPart];
  if (Number.isFinite(passed)) {
    const total = passed + (Number.isFinite(failed) ? failed : 0);
    if (failed > 0) groups.push(fillTemplate(t("{n} 失败"), { n: formatCount(failed) }), `${formatCount(passed)}/${formatCount(total)}`);
    else groups.push(fillTemplate(t("{n} 通过"), { n: `${formatCount(passed)}/${formatCount(total)}` }));
  } else if (typeof s.input.status === "string" && s.input.status) {
    groups.push(testStatusLabel(s.input.status));
  }
  return { groups, key: "test_record", title: id || undefined, tone: s.input.status === "failed" || failed > 0 ? "warn" : undefined };
}

function summarizeMemorySearch(s) {
  const head = firstLine(s.lines).trim();
  if (/^\(no memory matched/.test(head)) return { groups: [t("无匹配记忆")], key: "memory_search.none" };
  const hits = s.lines.map((line) => line.match(/^[A-Z]+-\d+ \[[^\]]+\] (.+?)(?: — |$)/)).filter(Boolean);
  if (!hits.length) return null;
  const first = cleanInline(hits[0][1], s.roots);
  return { groups: [fillTemplate(t("{n} 条记忆"), { n: formatCount(hits.length) }), first ? clipText(first, 40) : null], key: "memory_search" };
}
function summarizeMemoryNote(s) {
  const head = firstLine(s.lines).trim();
  let m;
  if ((m = s.text.match(/pending notes: (\d+)/))) {
    return { groups: [t("已记入收件箱"), fillTemplate(t("待整理 {n} 条"), { n: formatCount(m[1]) })], key: "memory_note" };
  }
  if (/^noted as duplicate/.test(head)) return { groups: [t("与已有记忆重复,未记录")], key: "memory_note.duplicate" };
  if ((m = head.match(/^corrected (\S+)/))) return { groups: [[txt(`${t("已更正")} `), code(m[1])]], key: "memory_note.correct" };
  return null;
}
/// memory_add/update/promote/merge/stale/inbox_*:首行动词 → 人话 + 编号。
function summarizeMemoryChange(s) {
  const head = firstLine(s.lines).trim();
  let m;
  const idPart = (label, id) => ({ groups: [[txt(`${label} `), code(id)]], key: `memory.${s.name}` });
  if ((m = head.match(/^added (\S+)/))) return idPart(t("新增"), m[1]);
  if ((m = head.match(/^updated (\S+)/))) return idPart(t("更新"), m[1]);
  if ((m = head.match(/^promoted (\S+)/))) return idPart(t("晋升"), m[1]);
  if ((m = head.match(/^merged (\S+)/))) return idPart(t("合并入"), m[1]);
  if ((m = head.match(/^staled (\S+)/))) return idPart(t("标记过时"), m[1]);
  if (/^inbox cleared/.test(head)) return { groups: [t("收件箱已清空")], key: "memory.inbox" };
  if (/^discarded inbox note/.test(head)) return { groups: [t("已丢弃笔记")], key: "memory.inbox" };
  return null;
}

function summarizeArchitecture(s) {
  const head = firstLine(s.lines).trim();
  let m;
  if (/^Draft only/.test(head)) return { groups: [t("草稿(未写入)")], key: "architecture" };
  if (/^updated /.test(head)) return { groups: [t("已更新架构索引")], key: "architecture" };
  if (/^validation: ok\b/m.test(s.text)) return { groups: [t("校验通过")], key: "architecture" };
  if ((m = s.text.match(/^validation: (\d+) issue/m))) {
    return { groups: [fillTemplate(t("{n} 个问题"), { n: formatCount(m[1]) })], key: "architecture", tone: "warn" };
  }
  return null;
}
function summarizeConventions(s) {
  const head = firstLine(s.lines).trim();
  if (/^patched /.test(head)) return { groups: [t("已更新规范")], key: "conventions" };
  const start = s.lines.findIndex((line) => line.trim() === "headings:");
  if (start < 0) return null;
  const end = s.lines.findIndex((line, index) => index > start && line.trim() === "---");
  const headings = s.lines.slice(start + 1, end < 0 ? undefined : end).filter((line) => /^ {2}#/.test(line)).length;
  return { groups: [fillTemplate(t("{n} 节"), { n: formatCount(headings) })], key: "conventions" };
}
function summarizeBrowser(s) {
  const head = firstLine(s.lines).trim();
  if (/^浏览器已打开并截图/.test(head)) {
    const host = urlParts(s.text.match(/^url: (\S+)/m)?.[1])?.host ?? "";
    return { groups: [t("截图"), host ? [code(host)] : null], key: "browser.screenshot" };
  }
  return null;
}
function summarizeFrontend(s) {
  const head = firstLine(s.lines).trim();
  let m;
  if ((m = head.match(/^(\d+) 处定义/))) return { groups: [countText(Number(m[1]), t("{n} 处定义"), t("1 处定义"))], key: "frontend" };
  if (/结构完整/.test(head)) return { groups: [t("通过")], key: "frontend" };
  if ((m = s.text.match(/(\d+) 个问题/))) return { groups: [fillTemplate(t("{n} 个问题"), { n: formatCount(m[1]) })], key: "frontend", tone: "warn" };
  return null;
}
function summarizeDeliver(s) {
  if (s.display?.kind === "file") {
    return { groups: [[txt(`${t("已交付")} `), code(String(s.display.name ?? ""))], formatByteSize(s.display.bytes)], key: "deliver" };
  }
  const m = firstLine(s.lines).trim().match(/^\[delivered\] (.+?) \((\d+) bytes\)/);
  return m ? { groups: [[txt(`${t("已交付")} `), code(m[1])], formatByteSize(m[2])], key: "deliver" } : null;
}
function summarizeCollaboration(s) {
  if (Array.isArray(s.display?.lines)) {
    return { groups: [countText(s.display.lines.length, t("{n} 条线路"), t("1 条线路"))], key: "collaboration_status" };
  }
  if (/^No other line is currently running/.test(firstLine(s.lines).trim())) return { groups: [t("无其他线路")], key: "collaboration_status" };
  return null;
}
const summarizeScreenshot = () => ({ groups: [t("截图")], key: "ui_screenshot" });

const TRACKER_TOOLS = ["req", "defect", "idea", "decision", "source", "finding"];
/// 成功态摘要器注册表:覆盖全部已注册工具。表里没有的工具(latex/plot/incident/prior_art/
/// research_*/tool_search/ui_dom/ui_console/ui_style/memory_stats/memory_read/MCP)走兜底。
export const TOOL_RESULT_SUMMARIZERS = {
  read: summarizeRead,
  grep: summarizeGrep,
  glob: summarizeGlob,
  symbols: summarizeSymbols,
  files: summarizeFiles,
  edit: summarizeEdit,
  insert: summarizeEdit,
  multiedit: summarizeEdit,
  apply_patch: summarizeEdit,
  write: summarizeWrite,
  bash: summarizeBash,
  process: summarizeProcess,
  git: summarizeGit,
  webfetch: summarizeWebfetch,
  websearch: summarizeWebsearch,
  task: summarizeTask,
  question: summarizeQuestion,
  ...Object.fromEntries(TRACKER_TOOLS.map((name) => [name, summarizeTracker])),
  work: summarizeWork,
  test_record: summarizeTestRecord,
  memory_search: summarizeMemorySearch,
  memory_note: summarizeMemoryNote,
  memory_add: summarizeMemoryChange,
  memory_update: summarizeMemoryChange,
  memory_promote: summarizeMemoryChange,
  memory_merge: summarizeMemoryChange,
  memory_stale: summarizeMemoryChange,
  memory_inbox_clear: summarizeMemoryChange,
  memory_inbox_discard: summarizeMemoryChange,
  architecture: summarizeArchitecture,
  conventions: summarizeConventions,
  browser: summarizeBrowser,
  frontend_locate: summarizeFrontend,
  frontend_check: summarizeFrontend,
  ui_screenshot: summarizeScreenshot,
  deliver: summarizeDeliver,
  collaboration_status: summarizeCollaboration,
};
/// 「根本没执行」的失败与工具无关:权限拒绝(用户/规则集/自主运行跳过)、入参修复提示、
/// 停止取消。先于按工具的失败摘要器,说成人话;原文全文仍在展开区。
/// 实时 USER_DECLINED 的 content 是空串、preview 是 "(user declined)";历史正文是
/// "permission request declined by user"——两边同一句话。
export function toolGateFailure(s) {
  const head = firstLine(s.lines).trim();
  let m;
  if (s.code === "USER_DECLINED" || /^\(user declined\)$|^permission request declined by user\b/.test(head)) {
    return { groups: [t("已拒绝")], key: "gate.declined" };
  }
  if (/^tool call cancelled because a previous permission request was declined/.test(head)) {
    return { groups: [t("已取消"), t("前一项权限被拒绝")], key: "gate.declined-chain" };
  }
  if ((m = head.match(/^permission denied by ruleset: (\S+) on `/))) {
    return { groups: [[txt(`${t("被权限规则拒绝")} `), code(m[1])]], key: "gate.ruleset" };
  }
  if (/^permission requires user approval: \S+ on `/.test(head)) {
    return { groups: [t("需要批准"), t("自主运行已跳过")], key: "gate.noninteractive" };
  }
  if (s.code === "INVALID_TOOL_INPUT" || /^Invalid input for tool `/.test(head)) {
    const field = s.text.match(/缺少必填参数 `([^`\n]+)`/)?.[1] ?? s.text.match(/missing field `([^`\n]+)`/)?.[1];
    return { groups: [t("入参无效"), field ? [txt(`${t("缺少参数")} `), code(field)] : null], key: "gate.invalid-input" };
  }
  if (/^cancelled: run stopped by user/.test(head)) return { groups: [t("已停止")], key: "gate.cancelled" };
  return null;
}

/// 失败态(failed / needs_*)的专用摘要器;没有登记的工具失败行走互斥切分。
export const TOOL_FAIL_SUMMARIZERS = {
  bash: summarizeBash,
  // 超时角色:屏障砍掉、什么都没产出,与「跑了但失败」分开说;其余失败走切分。
  task: (s) => (/\(超时,未产出结果\)|subagent hit the \d+s wall-clock/.test(s.text)
    ? { groups: [t("超时"), t("未产出结果")], key: "task.timeout" }
    : null),
  read(s) {
    if (s.code === "READ_PATH_NOT_FOUND" || /^path not found: /.test(firstLine(s.lines).trim())) {
      // 历史轨迹回放没有入参:路径从报错首行取(preview 首行被截到 120 字时以 … 结尾,不取)。
      const fromText = firstLine(s.lines).trim().match(/^path not found: (.+[^…])$/)?.[1] ?? "";
      const rawPath = typeof s.input.path === "string" ? s.input.path : fromText;
      const path = rawPath ? displayPath(rawPath, s.roots) : "";
      return { groups: [t("路径不存在"), path ? [code(path)] : null], key: "read.missing" };
    }
    const lines = s.text.match(/the file has (\d+) lines/)?.[1];
    if (s.code === "READ_RANGE_OUT_OF_BOUNDS" && lines) {
      return { groups: [t("超出范围"), fillTemplate(t("文件共 {total} 行"), { total: formatCount(lines) })], key: "read.range-error" };
    }
    return null;
  },
};
const PREFIX_SUMMARIZERS = [["memory_", genericSummary], ["ui_", genericSummary], ["frontend_", summarizeFrontend], ["research_", genericSummary]];
function lookupSummarizer(table, name) {
  const key = String(name ?? "");
  if (Object.hasOwn(table, key)) return table[key];
  if (table !== TOOL_RESULT_SUMMARIZERS) return null;
  return PREFIX_SUMMARIZERS.find(([prefix]) => key.startsWith(prefix))?.[1] ?? null;
}

/// 结果摘要。ctx = {ok, outcome, code, content, preview, contentTruncated, contentBytes, display, input, durationMs}。
/// 返回 {parts, text, title, tone, mode, rest, key, groups, durAt, storage, outcome, code}:
/// text 不含 "⎿ " 前缀;mode 为 'summary' | 'split'(失败行互斥切分)| 'fallback';
/// rest 是展开区原文(失败切分的剩余,或成功多行时的完整原文)。
export function toolResultSummary(name, ctx = {}) {
  const roots = Array.isArray(ctx.roots) ? ctx.roots : toolRoots();
  const input = ctx.input && typeof ctx.input === "object" && !Array.isArray(ctx.input) ? ctx.input : {};
  const display = ctx.display && typeof ctx.display === "object" ? ctx.display : null;
  // 空串 content + 非空 preview(实时 USER_DECLINED 等后端直发的 ToolEnd)按「没有正文」处理,
  // 否则摘要器拿着空串,⎿ 行就成了空白。
  const hasContent = typeof ctx.content === "string" && !(ctx.content === "" && String(ctx.preview ?? "").trim());
  let outcome = ctx.outcome || null;
  let errorCode = ctx.code || null;
  let raw = hasContent ? ctx.content : String(ctx.preview ?? "");
  const marker = stripToolOutcome(raw);
  if (marker.outcome) {
    outcome = outcome || marker.outcome;
    errorCode = errorCode || marker.code;
    raw = marker.body;
  }
  const storage = parseStorageMarker(raw);
  const truncated = Boolean(ctx.contentTruncated);
  const terminalFull = display?.kind === "terminal" && typeof display.full === "string" ? display.full : null;
  // 文本来源:完整 content > 终端 display.full(content 被截断或旧后端只给 preview 时)> preview。
  let source = raw;
  const useTerminal = terminalFull !== null && (!hasContent || truncated) && !storage;
  if (useTerminal) source = terminalFull;
  const fromPreview = !hasContent && !useTerminal;
  const preview = fromPreview ? parsePreview(raw) : null;
  if (preview) source = preview.first;
  const text = stripAnsi(source).replace(/\r\n?/g, "\n");
  const lines = text.split("\n");
  const state = outcome || (ctx.ok === false ? "failed" : "success");
  const s = {
    name: String(name ?? ""),
    input,
    display,
    text,
    lines,
    roots,
    fromPreview,
    previewFirst: preview?.first ?? "",
    lineCount: preview ? preview.lineCount : nonEmpty(lines).length,
    truncated: truncated && !useTerminal,
    bytes: Number(ctx.contentBytes),
    ok: state === "success",
    state,
    code: errorCode,
  };
  const durationText = Number(ctx.durationMs) >= 1000 ? formatDuration(ctx.durationMs) : "";
  // 展开区原文:去掉存储标记行,路径相对化,不合并空白。
  const restLines = storage ? lines.slice(1) : lines;
  const fullRest = fromPreview || nonEmpty(restLines).length < 2 ? "" : cleanPaths(restLines.join("\n"), roots);
  const finish = (result, mode, extra = {}) => {
    const normalized = normalizeResult(result) ?? { groups: [[txt(countFallback(s))]], key: "fallback" };
    let groups = normalized.groups;
    let key = normalized.key ?? s.name;
    const durAt = normalized.durAt;
    let parts = assembleParts(groups, durationText, durAt);
    // 安全网:摘要器漏网的代码/路径/JSON 一律改走计数兜底(code part 是有意的代码记号,不查)。
    // 整行看噪声;另逐组看「行号 + 源码」——合并空白后 `  1\t// 注释` 成了 `1 // 注释`,
    // 夹在别的组后面时整行判据看不出来。
    const plain = parts.filter((part) => part.k === "text").map((part) => part.v).join("").replace(/ · /g, " ").trim();
    const numberedSource = groups.some((group) => looksLikeNumberedSource(group.filter((part) => part.k === "text").map((part) => part.v).join("")));
    if (mode !== "split" && plain && (looksLikeNoise(plain) || numberedSource)) {
      groups = [[txt(countFallback(s))]];
      key = "fallback.noise";
      parts = assembleParts(groups, durationText, undefined);
    }
    // 噪声检查之后再切行内代码(反引号是摘要器自己保留的原文,不参与噪声判据);悬停 title 保留带反引号的原句。
    const rawText = partsText(parts);
    groups = groups.map((group) => group.flatMap(inlineCodeParts));
    parts = assembleParts(groups, durationText, key === "fallback.noise" ? undefined : durAt);
    const summaryText = partsText(parts);
    return {
      parts,
      text: summaryText,
      title: normalized.title ?? rawText,
      tone: normalized.tone ?? null,
      mode,
      rest: extra.rest ?? "",
      key,
      groups,
      durAt,
      storage,
      outcome: state,
      code: errorCode,
    };
  };

  if (storage?.kind === "externalized" || display?.kind === "artifact") {
    const size = Number(display?.bytes ?? storage?.bytes);
    return finish({
      groups: [fillTemplate(t("输出较大 · {size} · 已外置"), { size: Number.isFinite(size) ? formatByteSize(size) : "?" })],
      key: "artifact",
    }, "summary", { rest: fullRest });
  }
  if (state === "noop") return finish({ groups: [t("无需修改")], key: "noop" }, "summary", { rest: fullRest });
  if (state !== "success") {
    const gate = toolGateFailure(s);
    if (gate) return finish(gate, "summary", { rest: fullRest });
    const failSummarizer = lookupSummarizer(TOOL_FAIL_SUMMARIZERS, s.name);
    let failed = null;
    try { failed = failSummarizer ? normalizeResult(failSummarizer(s)) : null; } catch { failed = null; }
    if (failed) return finish(failed, "summary", { rest: fullRest });
    // 没有专用摘要器:对清洗后的原文互斥切分,摘要 + 剩余仍能逐字拼回。
    const split = toolResultSplit(cleanPaths(fromPreview ? raw : text, roots), true);
    return finish({ groups: [split.text], key: "split" }, "split", { rest: split.rest });
  }
  const summarizer = lookupSummarizer(TOOL_RESULT_SUMMARIZERS, s.name);
  let result = null;
  try { result = summarizer ? normalizeResult(summarizer(s)) : null; } catch { result = null; }
  if (!result) return finish({ groups: [genericSummary(s)], key: "fallback" }, "fallback", { rest: fullRest });
  return finish(result, "summary", { rest: fullRest });
}

/// 在摘要里补耗时(历史回放先渲染、轨迹里的 durationMs 后到时用)。
export function withToolDuration(summary, durationMs) {
  if (!summary || !Array.isArray(summary.groups) || !(Number(durationMs) >= 1000)) return summary;
  const parts = assembleParts(summary.groups, formatDuration(durationMs), summary.durAt);
  const text = partsText(parts);
  return { ...summary, parts, text, title: summary.title === summary.text ? text : summary.title };
}

/// 把摘要写进元素:前缀文本 + 各 part(非纯文本的 part 包 span.tool-sum-*)。
/// 纯 DOM API,不经 innerHTML;textContent 恰好等于 prefix + summary.text。
export function renderToolSummary(el, summary, { prefix = "⎿ " } = {}) {
  if (!el) return el;
  el.textContent = "";
  if (prefix) el.append(document.createTextNode(prefix));
  for (const part of summary?.parts ?? []) {
    if (part.k === "text") {
      el.append(document.createTextNode(part.v));
      continue;
    }
    const span = document.createElement("span");
    span.className = `tool-sum-${part.k}`;
    span.textContent = part.v;
    el.append(span);
  }
  if (summary?.title) el.title = summary.title;
  return el;
}

// ---------- 参数摘要 ----------
function pickString(input, ...keys) {
  for (const key of keys) {
    const value = input[key];
    if (typeof value === "string" && value.trim()) return value.trim();
    if (typeof value === "number") return String(value);
  }
  return "";
}
const TRACKER_ACTION_LABELS = {
  // 参数列是「要做的动作」,英文用祈使式 Add;结果列的「新增 R-1」是已发生,英文 Added。
  add: () => t("新增条目"),
  update: () => t("更新条目"),
  close: () => t("关闭"),
  reopen: () => t("重开"),
  list: () => t("列表"),
  get: () => t("查看"),
  reorder: () => t("重排"),
};
/// 正则:顶层 `|` 分出 ≥3 个分支时显示首个分支 + 「等 N 项」,否则截头。
function smartPattern(pattern) {
  const branches = splitAlternation(pattern);
  if (branches.length >= 3) {
    return `${clipText(branches[0], 24)} ${fillTemplate(t("等 {n} 项"), { n: branches.length })}`;
  }
  return clipText(pattern, 40);
}
/// 命令:剥掉开头「cd 到项目根」的前缀,多行只取首行,64 字内按词截断。
function smartCommand(command, roots) {
  let value = stripAnsi(command).trim();
  const lead = value.match(/^(?:cd|Set-Location|pushd|sl)\s+(?:-(?:Literal)?Path\s+)?("[^"]*"|'[^']*'|[^\s;&|]+)\s*(?:&&|;|\|\|)\s*/i);
  if (lead && displayPath(lead[1].replace(/^["']|["']$/g, ""), roots, { max: Infinity }) === ".") {
    value = value.slice(lead[0].length);
  }
  const lines = value.split(/\r?\n/).filter((line) => line.trim());
  let first = cleanInline(lines[0] ?? "", roots);
  if (lines.length > 1) first = `${first} …`;
  return { text: clipText(first, 64), title: cleanInline(value, roots) };
}
/// URL 的 host 与 path(不含协议、query、hash)。手工切分,不依赖宿主 URL 构造器。
function urlParts(url) {
  const match = String(url ?? "").trim().match(/^[a-z][\w+.-]*:\/\/([^/?#\s]+)([^?#\s]*)/i);
  return match ? { host: match[1], path: match[2] === "/" ? "" : match[2] } : null;
}
function urlSummary(url) {
  const parts = urlParts(url);
  return clipText(parts ? `${parts.host}${parts.path}` : String(url ?? ""), 48);
}
function pathArg(input, roots) {
  const path = pickString(input, "path", "file_path", "file");
  if (!path) return null;
  return { text: displayPath(path, roots), title: displayPath(path, roots, { max: Infinity }), code: true };
}
/// 参数摘要注册表:每个工具挑最有信息量的参数,说成一行。
export const TOOL_ARG_SUMMARIZERS = {
  read: pathArg, write: pathArg, edit: pathArg, insert: pathArg, multiedit: pathArg, apply_patch: pathArg,
  files(input, roots) {
    const path = pathArg(input, roots);
    if (input.top) return { text: `top ${input.top}${path ? ` · ${path.text}` : ""}`, code: true };
    return path;
  },
  conventions: (input, roots) => pathArg(input, roots) ?? { text: pickString(input, "action") },
  architecture: (input, roots) => pathArg(input, roots) ?? { text: pickString(input, "action") },
  symbols(input, roots) {
    if (input.define) return { text: fillTemplate(t("定义 {name}"), { name: input.define }), code: true };
    if (input.callers) return { text: fillTemplate(t("调用方 {name}"), { name: input.callers }), code: true };
    if (input.crate) return { text: `crate ${input.crate}${input.module ? `::${input.module}` : ""}`, code: true };
    const path = pickString(input, "path");
    const shown = path ? displayPath(path, roots) : "";
    const text = [shown, input.filter ? String(input.filter) : ""].filter(Boolean).join(" · ");
    return text ? { text, code: true } : null;
  },
  grep(input, roots) {
    const pattern = pickString(input, "pattern");
    if (!pattern) return null;
    const scope = input.glob ? String(input.glob) : input.path ? displayPath(String(input.path), roots) : "";
    return { text: `${smartPattern(pattern)}${scope ? ` · ${scope}` : ""}`, title: `${pattern}${scope ? ` · ${scope}` : ""}`, code: true };
  },
  glob(input, roots) {
    const pattern = pickString(input, "pattern");
    const path = input.path ? displayPath(String(input.path), roots) : "";
    const text = [pattern, path].filter(Boolean).join(" · ");
    return text ? { text, code: true } : null;
  },
  bash(input, roots) {
    const command = pickString(input, "command");
    if (!command) return null;
    return { ...smartCommand(command, roots), code: true };
  },
  process: (input) => ({ text: [pickString(input, "action"), pickString(input, "id")].filter(Boolean).join(" ") }),
  webfetch(input) {
    const url = pickString(input, "url");
    return url ? { text: urlSummary(url), title: url, code: true } : null;
  },
  websearch(input) {
    const query = pickString(input, "query");
    return query ? { text: `“${clipText(query, 60)}”`, title: query } : null;
  },
  task(input) {
    const prompt = pickString(input, "prompt").split(/\r?\n/).find((line) => line.trim()) ?? "";
    const text = pickString(input, "description") || prompt.trim() || pickString(input, "role");
    return text ? { text: clipText(text, 48), title: pickString(input, "description", "prompt", "role") } : null;
  },
  question(input) {
    const question = pickString(input, "question");
    return question ? { text: clipText(question.replace(/\s+/g, " "), 60), title: question } : null;
  },
  ...Object.fromEntries(TRACKER_TOOLS.map((name) => [name, (input) => {
    const action = pickString(input, "action");
    const label = TRACKER_ACTION_LABELS[action]?.() ?? action;
    if (action === "add") {
      const title = pickString(input, "title");
      return { text: title ? `${label} · ${clipText(title, 32)}` : label, title: title || label };
    }
    if (action === "list") return { text: label };
    return { text: [label, pickString(input, "id")].filter(Boolean).join(" ") };
  }])),
  work(input) {
    const action = pickString(input, "action");
    const id = pickString(input, "id");
    if (action === "next") return { text: t("取活") };
    if (action === "claim") return { text: [t("认领"), id].filter(Boolean).join(" ") };
    if (action === "handoff") return { text: t("声明完成") };
    if (action === "reconcile") return { text: t("对账") };
    return { text: [action, id].filter(Boolean).join(" ") };
  },
  test_record: (input) => ({ text: clipText(pickString(input, "title", "id"), 60) }),
  memory_search: (input) => ({ text: pickString(input, "query") }),
  memory_note: (input) => ({ text: clipText(pickString(input, "summary"), 60) }),
  memory_add: (input) => ({ text: clipText(pickString(input, "title"), 60) }),
  git(input) {
    const action = pickString(input, "action");
    if (action === "commit" || action === "finalize") {
      const message = pickString(input, "message").split(/\r?\n/)[0] ?? "";
      return { text: message ? `${t("提交")} · ${clipText(message, 40)}` : t("提交"), title: pickString(input, "message") };
    }
    if (action === "stage") {
      const files = Array.isArray(input.files) ? input.files.length : 0;
      return { text: fillTemplate(t("暂存 {n} 个文件"), { n: files }) };
    }
    return { text: action };
  },
  browser(input) {
    const action = pickString(input, "action");
    const url = pickString(input, "url");
    const target = url ? urlSummary(url) : pickString(input, "selector");
    return { text: [action, target].filter(Boolean).join(" "), code: Boolean(target) };
  },
  deliver(input, roots) {
    const path = pickString(input, "path");
    return path ? { text: displayPath(path, roots).split("/").pop(), title: displayPath(path, roots, { max: Infinity }), code: true } : null;
  },
  tool_search: (input) => ({ text: pickString(input, "query") }),
};
function defaultArg(input, roots) {
  for (const key of ["path", "command", "query", "pattern", "url", "id", "title", "action", "summary"]) {
    const value = pickString(input, key);
    if (!value) continue;
    if (key === "path") return { text: displayPath(value, roots), title: value, code: true };
    if (key === "command") return { ...smartCommand(value, roots), code: true };
    if (key === "url") return { text: urlSummary(value), title: value, code: true };
    if (key === "pattern") return { text: clipText(value, 40), title: value, code: true };
    return { text: value };
  }
  return null;
}

/// 参数摘要:返回 {text, title, code}。code=true 时用等宽渲染;title 是清洗后的完整值。
export function toolArgSummary(name, input, { roots = toolRoots() } = {}) {
  const source = input && typeof input === "object" && !Array.isArray(input) ? input : {};
  const key = String(name ?? "");
  let summarizer = Object.hasOwn(TOOL_ARG_SUMMARIZERS, key) ? TOOL_ARG_SUMMARIZERS[key] : null;
  if (!summarizer && key.startsWith("memory_")) summarizer = (value) => ({ text: pickString(value, "id", "query", "title") });
  if (!summarizer && key.startsWith("ui_")) summarizer = TOOL_ARG_SUMMARIZERS.browser;
  let result = null;
  try { result = (summarizer ?? defaultArg)(source, roots); } catch { result = null; }
  if (!result && summarizer) result = defaultArg(source, roots);
  const raw = cleanInline(String(result?.text ?? ""), roots);
  const text = raw.length > 76 ? `${raw.slice(0, 75)}…` : raw;
  const title = cleanInline(String(result?.title ?? raw), roots);
  return { text, title, code: Boolean(result?.code && text) };
}
