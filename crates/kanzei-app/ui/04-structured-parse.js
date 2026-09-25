// ---------- 结构化文本纯解析(UI-0926 #6/#10) ----------
// 零 import、零 DOM:工具行摘要(05-tool-summary.js)与结构化渲染(04-structured.js)共用的
// 唯一解析真源。这里只产出「事实」(路径、计数、JSON 值、标记字段),不产出任何界面文案——
// 文案要走 t(),而本模块必须能在 Node 里被直接 import 做单测,所以不碰 i18n。

// ---- 终端转义 ----
const ANSI_CSI_RE = /\x1b\[[0-?]*[ -/]*[@-~]/g;
const ANSI_OSC_RE = /\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g;
/// 去掉 CSI(颜色/光标)与 OSC(标题/超链接)转义。cargo 等彩色输出原样进 DOM 会显示成
/// `\u001b[1m\u001b[32m` 乱码。
export function stripAnsi(text) {
  return String(text ?? "").replace(ANSI_OSC_RE, "").replace(ANSI_CSI_RE, "");
}

// ---- 路径 ----
/// 路径归一:剥 Windows verbatim 前缀(`\\?\`、`\\?\UNC\`、`//?/`)与 `file://`,
/// 反斜杠转 `/`,去掉尾部 `/`。项目根经 canonicalize 后是 verbatim 形态,不剥就永远
/// 对不上普通写法的同一路径。
export function normalizeRoot(path) {
  let value = String(path ?? "").trim();
  if (!value) return "";
  value = value.replace(/^file:\/\/\/?/i, "");
  if (/^\\\\\?\\UNC\\/i.test(value)) value = `\\\\${value.slice(8)}`;
  else if (/^\\\\\?\\/.test(value)) value = value.slice(4);
  else if (/^\/\/\?\//.test(value)) value = value.slice(4);
  value = value.replace(/\\/g, "/");
  if (value.length > 1 && !/^[A-Za-z]:\/$/.test(value)) value = value.replace(/\/+$/, "");
  return value;
}

function rootList(roots) {
  const list = (Array.isArray(roots) ? roots : [roots])
    .map((root) => normalizeRoot(root))
    .filter(Boolean);
  return [...new Set(list)].sort((a, b) => b.length - a.length);
}

/// 显示用的短路径:落在某个项目根下 → 相对路径;否则 `C:/Users/<名>/` 缩成 `~/`;
/// 超过 `max` 字做中段省略(保留首段与末两段,形如 `crates/…/run/mod.rs`)。
export function displayPath(raw, roots = [], { max = 56 } = {}) {
  let value = normalizeRoot(raw);
  if (!value) return "";
  const lower = value.toLowerCase();
  for (const root of rootList(roots)) {
    const rootLower = root.toLowerCase();
    if (lower === rootLower) {
      value = ".";
      break;
    }
    if (lower.startsWith(`${rootLower}/`)) {
      value = value.slice(root.length + 1);
      break;
    }
  }
  value = value.replace(/^[A-Za-z]:\/Users\/[^/]+\//, "~/");
  if (value.length <= max) return value;
  const segments = value.split("/");
  if (segments.length <= 3) return `…${value.slice(-(max - 1))}`;
  const head = segments[0] || `/${segments[1]}`;
  const tail = segments.slice(-2).join("/");
  const short = `${head}/…/${tail}`;
  return short.length <= max ? short : `…/${tail}`.slice(-max);
}

/// 落在 root 下 → 相对 root 的路径(root 本身为 `.`);不在其下或任一方为空 → null。
/// 与 displayPath 的区别:不做 `~/` 缩写、不省略——结果要当跳转目标用,必须可解析。
export function relativeToRoot(raw, root) {
  const value = normalizeRoot(raw);
  const base = normalizeRoot(root);
  if (!value || !base) return null;
  const lower = value.toLowerCase();
  const baseLower = base.toLowerCase();
  if (lower === baseLower) return ".";
  return lower.startsWith(`${baseLower}/`) ? value.slice(base.length + 1) : null;
}
/// 盘符 / `/` / UNC 开头(归一后)的绝对路径。
export function isAbsolutePath(raw) {
  return /^(?:[A-Za-z]:\/|\/)/.test(normalizeRoot(raw));
}

/// 结构化路径信息(04-structured.js 的路径 chip 用):相对根的 rel、末两段 short、
/// 可选的 `:行` / `:行-行`。
export function prettyPath(raw, projectRoot) {
  const full = normalizeRoot(raw);
  const match = full.match(/^(.*?)(?::(\d+)(?:-(\d+))?)?$/);
  const bare = match?.[1] ?? full;
  const line = match?.[2] ? Number(match[2]) : null;
  const endLine = match?.[3] ? Number(match[3]) : null;
  const rel = displayPath(bare, projectRoot ? [projectRoot] : [], { max: Infinity });
  const segments = rel.split("/").filter(Boolean);
  const short = segments.length > 2 ? `…/${segments.slice(-2).join("/")}` : segments.join("/");
  return { full, rel, short, line, endLine };
}

/// 文本里的绝对路径片段(可带 verbatim 前缀)。只认盘符路径:相对路径本就是可读的。
/// 左侧不许紧贴字母数字:`https://` 里的 `s:/` 不是盘符。
export const ABS_PATH_RE = /(?<![A-Za-z0-9])(?:\\\\\?\\(?:UNC\\)?|\/\/\?\/)?[A-Za-z]:[\\/][^\s"'<>|*?,;()[\]{}]*/g;

const PATH_TAIL = String.raw`[^\s"'<>|*?,;()[\]{}]*`;
const rootPatternCache = new Map();
/// 按项目根逐个匹配(根里可能有空格,如 `Documents/kanzei code`——通用盘符正则在空格处
/// 就断了)。分隔符 `/` 与 `\` 互认,大小写不敏感,可带 verbatim 前缀。
function rootPatterns(roots) {
  const list = rootList(roots);
  const key = list.join("\n");
  let patterns = rootPatternCache.get(key);
  if (patterns) return patterns;
  patterns = list.map((root) => {
    const body = root
      .split("/")
      .map((segment) => segment.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
      .join(String.raw`[\\/]`);
    return new RegExp(
      String.raw`(?<![A-Za-z0-9])(?:\\\\\?\\|\/\/\?\/)?${body}(?:[\\/](${PATH_TAIL}))?(?![^\s"'<>|*?,;()[\]{}\\/])`,
      "gi",
    );
  });
  if (rootPatternCache.size > 16) rootPatternCache.clear();
  rootPatternCache.set(key, patterns);
  return patterns;
}

/// 逐段把绝对路径换成显示路径并去 ANSI,**不动空白**——失败行的摘要 + 剩余必须能逐字
/// 拼回(清洗后的)原文,合并空白会让拼接对不上。
export function cleanPaths(text, roots = []) {
  let value = stripAnsi(text);
  for (const pattern of rootPatterns(roots)) {
    value = value.replace(pattern, (_hit, tail) => (tail ? tail.replace(/\\/g, "/") : "."));
  }
  return value.replace(ABS_PATH_RE, (hit) => displayPath(hit, roots, { max: Infinity }));
}

/// 行内展示用的清洗:去 ANSI、路径相对化、合并空白。
export function cleanInline(text, roots = []) {
  return cleanPaths(text, roots).replace(/\s+/g, " ").trim();
}

// ---- 噪声判据 ----
const SYMBOL_CHARS = /[{}[\]();<>=|\\/:"']/g;
/// 代码/机器符号占非空白字符的比例。
export function symbolRatio(line) {
  const compact = String(line ?? "").replace(/\s+/g, "");
  if (!compact) return 0;
  return (compact.match(SYMBOL_CHARS) ?? []).length / compact.length;
}
// GBK 程序输出被按 UTF-8 解码后的高频误码字。
const MOJIBAKE_RE = /[鍒鏈璺銆浠瀹绋鐨锛鎴閿]/g;
/// 这一行不适合作为「人话摘要」:代码、JSON、带行号的源码、grep 命中行、绝对路径、
/// 乱码、长哈希/base64、符号密集的机器输出。
export function looksLikeNoise(line) {
  const raw = String(line ?? "");
  const text = raw.trim();
  if (!text) return true;
  if (/^[{["]/.test(text) || text.startsWith("==")) return true;
  if (/^\s*\d+\t/.test(raw)) return true;
  if (/^[\w./\\-]+[:-]\d+[:-]/.test(text)) return true;
  if (/\\\\\?\\|\/\/\?\/|(?:^|[\s"'(=])[A-Za-z]:[\\/]/.test(text)) return true;
  if (text.includes("\uFFFD")) return true;
  if ((text.match(MOJIBAKE_RE) ?? []).length >= 2) return true;
  if (/[A-Za-z0-9+/=]{60,}/.test(text)) return true;
  if (looksLikeNumberedSource(text)) return true;
  return symbolRatio(text) > 0.25;
}
/// 合并空白之后的「行号 + 源码」:`  12\t// 注释` 经 cleanInline 变成 `12 // 注释`,行号 Tab
/// 特征没了,符号占比也不高。认的是「数字 + 空白 + 注释记号/括号/代码关键字」——
/// 「3 通过」「12 files」这类计数人话不命中。
export function looksLikeNumberedSource(line) {
  const text = String(line ?? "").trim();
  const match = text.match(/^\d+\s+(\S[\s\S]*)$/);
  if (!match) return false;
  const rest = match[1];
  if (/^(?:\/\/|\/\*|\*\/|#[!\s[]|--\s|<!--|[{}()[\];])/.test(rest)) return true;
  return /^(?:fn|let|const|var|pub|impl|struct|enum|use|mod|def|class|function|import|export|return|async|await|match)\b/.test(rest)
    && /[(){};=<>:[\]]/.test(rest);
}
/// 像一句人话:≥2 个中日韩字符或 ≥3 个英文单词,且符号占比 ≤20%。
export function isProse(line) {
  const text = String(line ?? "").trim();
  if (!text) return false;
  const cjk = (text.match(/[\u3400-\u9fff\u3040-\u30ff\uac00-\ud7af]/g) ?? []).length;
  const words = (text.match(/[A-Za-z]{2,}/g) ?? []).length;
  return (cjk >= 2 || words >= 3) && symbolRatio(text) <= 0.2;
}

// ---- 结构化正文 ----
/// trim 后以 `{`/`[` 开头且能 JSON.parse 就返回值,否则 null。
export function parseJsonish(text) {
  const value = String(text ?? "").trim();
  if (!/^[{[]/.test(value)) return null;
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}

const OUTCOME_RE = /^\[tool_outcome=([a-z_]+)(?: code=([\w.-]+))?\]\r?\n?/;
/// 历史回放的正文带模型侧机器头 `[tool_outcome=X code=Y]\n`;剥掉并返回终态与错误码。
/// 不带头的文本原样作为 body 返回(outcome/code 为 null)。
export function stripToolOutcome(content) {
  const text = String(content ?? "");
  const match = text.match(OUTCOME_RE);
  if (!match) return { outcome: null, code: null, body: text };
  return { outcome: match[1], code: match[2] ?? null, body: text.slice(match[0].length) };
}
export const parseOutcomeMarker = stripToolOutcome;

/// 结果存储标记:外置(`[tool_result_externalized … bytes=N …]`)或配额截断
/// (`[tool_result_truncated reason=… bytes=… storage_used_bytes=… quota_bytes=…]`)。
/// 历史回放没有 display,只能从这行标记还原出与实时相同的信息。
export function parseStorageMarker(text) {
  const first = String(text ?? "").split("\n", 1)[0].trim();
  const externalized = first.match(/^\[tool_result_externalized\b([^\]]*)\]/);
  if (externalized) {
    const bytes = Number(externalized[1].match(/\bbytes=(\d+)/)?.[1]);
    return { kind: "externalized", bytes: Number.isFinite(bytes) ? bytes : null };
  }
  const truncated = first.match(/^\[tool_result_truncated\b([^\]]*)\]/);
  if (truncated) {
    const field = (key) => truncated[1].match(new RegExp(`\\b${key}=(\\S+)`))?.[1] ?? null;
    const number = (key) => {
      const value = Number(field(key));
      return field(key) !== null && Number.isFinite(value) ? value : null;
    };
    return {
      kind: "truncated",
      reason: field("reason") ?? "",
      bytes: number("bytes"),
      storage_used_bytes: number("storage_used_bytes"),
      quota_bytes: number("quota_bytes"),
    };
  }
  return null;
}

/// 后端 runner::preview 的单行摘要:首行 120 字 + ` (+N lines)`。拆出首行与总行数。
export function parsePreview(preview) {
  const text = String(preview ?? "");
  const match = text.match(/^([\s\S]*?) \(\+(\d+) lines\)$/);
  if (match) return { first: match[1], lineCount: Number(match[2]) + 1 };
  return { first: text, lineCount: text.trim() ? 1 : 0 };
}

// ---- 数字与模板 ----
/// 千分位整数(界面上所有计数同一口径)。
export function formatCount(value) {
  const number = Number(value);
  return Number.isFinite(number) ? Math.round(number).toLocaleString("en-US") : String(value ?? "");
}
/// 耗时:<60s 保留一位小数(`12.3s`),否则 `1m 05s`。
export function formatDuration(ms) {
  const value = Number(ms);
  if (!Number.isFinite(value) || value < 0) return "";
  if (value < 1000) return `${Math.round(value)}ms`;
  const seconds = value / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${String(Math.floor(seconds % 60)).padStart(2, "0")}s`;
}
/// 字节数的短写法(与 06-activity formatBytes 同口径,但纯函数、不依赖 DOM 模块)。
export function formatByteSize(bytes) {
  const n = Number(bytes) || 0;
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}
/// `{name}` 占位符填充。调用方写 `fillTemplate(t("第 {from}–{to} 行"), {...})`,
/// 源码里仍是可被 i18n 冒烟扫到的 t 字面量调用。
export function fillTemplate(template, vars = {}) {
  return String(template ?? "").replace(/\{(\w+)\}/g, (hit, key) => (key in vars ? String(vars[key]) : hit));
}
/// 截到 max 字:优先在词边界断开,末尾补 `…`。
export function clipText(text, max) {
  const value = String(text ?? "").trim();
  if (value.length <= max) return value;
  const cut = value.slice(0, max - 1);
  const space = cut.lastIndexOf(" ");
  return `${(space > max * 0.6 ? cut.slice(0, space) : cut).trimEnd()}…`;
}

// ---- 正则与命令 ----
/// 顶层(不在括号/字符类内、未转义)的 `|` 分支。
export function splitAlternation(pattern) {
  const text = String(pattern ?? "");
  const parts = [];
  let depth = 0;
  let inClass = false;
  let current = "";
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (ch === "\\") {
      current += ch + (text[i + 1] ?? "");
      i += 1;
      continue;
    }
    if (inClass) {
      if (ch === "]") inClass = false;
    } else if (ch === "[") {
      inClass = true;
    } else if (ch === "(") {
      depth += 1;
    } else if (ch === ")") {
      depth = Math.max(0, depth - 1);
    } else if (ch === "|" && depth === 0) {
      parts.push(current);
      current = "";
      continue;
    }
    current += ch;
  }
  parts.push(current);
  return parts;
}

// ---- 行级 diff 计数 ----
/// 两段文本的行级增删数(LCS)。历史回放没有 diff display,只能按 old/new_string 自己算。
/// 超大输入退化为按行集合估算,避免 O(n·m) 卡住主线程。
export function lineDiffCounts(oldText, newText) {
  const a = String(oldText ?? "").split("\n");
  const b = String(newText ?? "").split("\n");
  if (a.length * b.length > 250000) {
    const inB = new Set(b);
    const inA = new Set(a);
    return { additions: b.filter((line) => !inA.has(line)).length, deletions: a.filter((line) => !inB.has(line)).length };
  }
  const width = b.length + 1;
  const table = new Uint32Array((a.length + 1) * width);
  for (let i = a.length - 1; i >= 0; i -= 1) {
    for (let j = b.length - 1; j >= 0; j -= 1) {
      table[i * width + j] = a[i] === b[j]
        ? table[(i + 1) * width + j + 1] + 1
        : Math.max(table[(i + 1) * width + j], table[i * width + j + 1]);
    }
  }
  const common = table[0];
  return { additions: b.length - common, deletions: a.length - common };
}

/// 子串出现次数(截断的 JSON 解析不了时,按键名计数估条数)。
export function countOccurrences(text, needle) {
  if (!needle) return 0;
  return String(text ?? "").split(needle).length - 1;
}

// ---- JSON 结果事实(tracker / websearch / work) ----
/// tracker list:`{entries:[{blocked}], deadlocked}`。
export function trackerListFacts(value) {
  if (!value || typeof value !== "object" || !Array.isArray(value.entries)) return null;
  const total = value.entries.length;
  const executable = value.entries.filter((entry) => entry && entry.blocked === false).length;
  return { total, executable, deadlocked: Boolean(value.deadlocked) };
}
/// tracker get:`{id, title, lifecycle_status}`。
export function trackerEntryFacts(value) {
  if (!value || typeof value !== "object" || typeof value.id !== "string") return null;
  return { id: value.id, title: String(value.title ?? ""), status: String(value.lifecycle_status ?? value.status ?? "") };
}
/// websearch:`{query, results:[{title,url}]}`。
export function websearchFacts(value) {
  if (!value || typeof value !== "object" || !Array.isArray(value.results)) return null;
  const first = value.results[0];
  return { query: String(value.query ?? ""), count: value.results.length, firstTitle: String(first?.title ?? "") };
}
/// work next / claim:`{decision, selected:{id,title}}` 或 `{claimed}`。
export function workFacts(value) {
  if (!value || typeof value !== "object") return null;
  if (typeof value.claimed === "string") return { claimed: value.claimed };
  if (typeof value.decision !== "string") return null;
  const selected = value.selected && typeof value.selected === "object" ? value.selected : null;
  return {
    decision: value.decision,
    id: selected ? String(selected.id ?? selected.unit_id ?? "") : "",
    title: selected ? String(selected.title ?? selected.objective ?? "") : "",
  };
}
/// req audit_acceptance_scope:`{mismatch_count, mismatches:[…]}`。
export function mismatchFacts(value) {
  if (!value || typeof value !== "object" || !Number.isFinite(Number(value.mismatch_count))) return null;
  return { count: Number(value.mismatch_count) };
}

// ======== UI-0926 #10:结构化文本渲染的纯解析 ========
// 04-structured.js 的 DOM 渲染器与 ui-markdown-smoke 的夹具共用。全部是「文本 → 事实」,
// 不产出界面文案;宁可返回 null 让调用方回落成原文,也不猜。

// ---- 富文本实体:URL > 路径 > 条目编号,互不重叠 ----
const RICH_URL_RE = /https?:\/\/[^\s<>"'`）)】」，。；]+/g;
// 必须含 `/` 或 `\`、以 `.扩展名` 结尾,可带 `:行` / `:行-行`,允许盘符。`0.7em`、`B1/B2`、
// 纯中文都不会命中(前者无分隔符,后者无扩展名)。
const RICH_PATH_RE = /(?<![\w./\\@-])(?:[A-Za-z]:[\\/])?(?:[\w.@-]+[\\/])+[\w.@-]*\.[A-Za-z0-9]{1,8}(?::(\d+)(?:-(\d+))?)?(?![\w\\/-])/g;
const RICH_REF_RE = /\b(?:[RDISF]-\d{1,4}|A-\d{3}|T-\d{6,}|[MU]-\d{3,})\b/g;

function splitTextTokens(tokens, pattern, make) {
  const out = [];
  for (const token of tokens) {
    // locked:已被项目根认领、但不是文件路径的片段(根目录本身、根下目录)——后续通用
    // 路径正则不许再从中间切。
    if (token.type !== "text" || token.locked) {
      out.push(token);
      continue;
    }
    let cursor = 0;
    for (const match of token.value.matchAll(new RegExp(pattern.source, pattern.flags))) {
      const made = make(match);
      if (!made) continue;
      if (match.index > cursor) out.push({ type: "text", value: token.value.slice(cursor, match.index) });
      out.push(made);
      cursor = match.index + made.value.length;
    }
    if (cursor < token.value.length) out.push({ type: "text", value: token.value.slice(cursor) });
  }
  return out;
}

/// 项目根下的绝对路径 → path token(句末 `.`/`:` 不算路径);不是文件(没有扩展名)→
/// 锁定的文本 token。
function rootPathToken(raw) {
  const value = raw.replace(/[.:]+$/, "");
  const line = value.match(/:(\d+)(?:-(\d+))?$/);
  const bare = line ? value.slice(0, -line[0].length) : value;
  if (!/\.[A-Za-z0-9]{1,8}$/.test(bare)) return value ? { type: "text", value, locked: true } : null;
  return { type: "path", value, path: bare, line: line ? Number(line[1]) : null, endLine: line?.[2] ? Number(line[2]) : null };
}

/// 文本 → `[{type:'text'|'url'|'ref'|'path', value, path?, line?, endLine?}]`。
/// 各 token 的 value 按顺序拼回来就是输入原文(不丢字、不改字)。
/// roots:项目根/工作树根。根下的绝对路径先按根整体认领——根里可能有空格
/// (`Documents/kanzei code`),通用路径正则在空格处就断,会切出 `code\crates\a.rs` 这种
/// 指向错误位置的 chip。
export function tokenizeRich(text, { roots = [] } = {}) {
  const source = String(text ?? "");
  if (!source) return [];
  let tokens = [{ type: "text", value: source }];
  tokens = splitTextTokens(tokens, RICH_URL_RE, (match) => {
    // 句末标点不属于 URL。
    const value = match[0].replace(/[.,;:!?]+$/, "");
    return value.length > 8 ? { type: "url", value } : null;
  });
  for (const pattern of rootPatterns(roots)) {
    tokens = splitTextTokens(tokens, pattern, (match) => rootPathToken(match[0]));
  }
  tokens = splitTextTokens(tokens, RICH_PATH_RE, (match) => ({
    type: "path",
    value: match[0],
    path: match[0].replace(/:\d+(?:-\d+)?$/, ""),
    line: match[1] ? Number(match[1]) : null,
    endLine: match[2] ? Number(match[2]) : null,
  }));
  tokens = splitTextTokens(tokens, RICH_REF_RE, (match) => ({ type: "ref", value: match[0] }));
  return tokens.filter((token) => token.value !== "");
}

// ---- 列表类切分(只对 list/timeline 类字段启用;切不出来就返回 null,调用方显示原文) ----
const TRAILING_SEPARATORS = /[\s；;。,，]+$/;
/// ①–⑳ 编号列表:至少 2 个标记才成立。intro 是第一个标记之前的引导语。
export function splitCircledList(text) {
  const source = String(text ?? "");
  const marks = [...source.matchAll(/[①-⑳]/g)];
  if (marks.length < 2) return null;
  const items = marks.map((mark, index) => {
    const end = index + 1 < marks.length ? marks[index + 1].index : source.length;
    return source.slice(mark.index + 1, end).trim().replace(TRAILING_SEPARATORS, "");
  }).filter(Boolean);
  if (items.length < 2) return null;
  return { intro: source.slice(0, marks[0].index).trim().replace(/[\s:：]+$/, ""), items };
}

/// 「批1 …;批2 …」「B1 …；B2 …」:标记前必须是行首或分隔符,标记后必须是冒号或空白
/// (`B1-B4 已完成` 不算)。括号里的标记(「(与 B4 同版发布)」)和已出现过的标记只是正文里的引用,
/// 不切新项。至少 2 个不同标记。
export function splitMarkedList(text) {
  const source = String(text ?? "");
  const seen = new Set();
  const marks = [...source.matchAll(/(^|[；;。，,:：\s])(批\s*\d+|B\d+)(?=[:：\s])/g)]
    .map((match) => ({ start: match.index + match[1].length, raw: match[2], label: match[2].replace(/\s+/g, "") }))
    .filter((mark) => {
      const before = source.slice(0, mark.start);
      const depth = (before.match(/[(（]/g) ?? []).length - (before.match(/[)）]/g) ?? []).length;
      if (depth > 0 || seen.has(mark.label)) return false;
      seen.add(mark.label);
      return true;
    });
  if (marks.length < 2) return null;
  const items = marks.map((mark, index) => {
    const end = index + 1 < marks.length ? marks[index + 1].start : source.length;
    const body = source.slice(mark.start + mark.raw.length, end).replace(/^[\s:：]+/, "").replace(TRAILING_SEPARATORS, "");
    return { label: mark.label, text: body };
  });
  if (new Set(items.map((item) => item.label)).size < 2) return null;
  return { intro: source.slice(0, marks[0].start).trim().replace(/[\s:：]+$/, ""), items };
}

/// 全角「；」或 ASCII「; 」(后接空白)切段;至少 2 段且每段 ≥4 字,否则 null
/// (普通句子里偶尔的分号不该被拆成列表)。
export function splitSemicolonList(text) {
  const parts = String(text ?? "").split(/；|;(?=\s)/).map((part) => part.trim().replace(/[。\s]+$/, "")).filter(Boolean);
  if (parts.length < 2 || parts.some((part) => [...part].length < 4)) return null;
  return parts;
}

/// 「||」分段的时间线;段首的 `YYYY-MM-DD[ HH:MM]` 抽成 date。
export function splitTimeline(text) {
  return String(text ?? "")
    .split(/\s*\|\|\s*/)
    .map((segment) => segment.trim())
    .filter(Boolean)
    .map((segment) => {
      const match = segment.match(/^(\d{4}-\d{2}-\d{2})(?:[ T]\d{2}:\d{2})?\s*/);
      return match ? { date: match[1], text: segment.slice(match[0].length).trim() } : { date: null, text: segment };
    });
}

/// 停车/阻塞字段:`原因;恢复人:X;解除条件:Y` → {reason, owner, release}。
export function parseConditionField(text) {
  const source = String(text ?? "");
  const owner = source.match(/恢复人\s*[:：]\s*([^;；。]+)/);
  const release = source.match(/解除条件\s*[:：]\s*([^;；。]+)/);
  let rest = source;
  for (const hit of [owner, release]) if (hit) rest = rest.replace(hit[0], "\u0000");
  const reason = rest.split("\u0000")
    .map((part) => part.trim().replace(/^[;；。,，\s]+|[;；。,，\s]+$/g, ""))
    .filter(Boolean)
    .join(";");
  return { reason, owner: owner ? owner[1].trim() : null, release: release ? release[1].trim() : null };
}

// ---- 错误文本 ----
/// provider 的 HTTP 错误体、reqwest 错误链 → {head, message, status, fields, json, chain, raw}。
export function parseErrorText(text) {
  const raw = String(text ?? "");
  const status = raw.match(/HTTP (\d{3})\b/)?.[1] ?? null;
  const open = raw.indexOf("{");
  const close = raw.lastIndexOf("}");
  let json = null;
  if (open >= 0 && close > open) {
    try { json = JSON.parse(raw.slice(open, close + 1)); } catch { json = null; }
  }
  if (!json || typeof json !== "object") json = null;
  const error = json && json.error && typeof json.error === "object" ? json.error : null;
  const scalar = (value) => (typeof value === "string" || typeof value === "number" ? String(value) : null);
  const message = json
    ? scalar(error?.message) ?? scalar(json.message) ?? scalar(json.error_description) ?? scalar(json.detail) ?? scalar(json.error)
    : null;
  const fields = {};
  for (const key of ["type", "code", "param"]) {
    const value = scalar(error?.[key]) ?? scalar(json?.[key]);
    if (value) fields[key] = value;
  }
  let chain = [];
  if (!json && /^(?:transport error|provider|protocol violation|invalid configuration|context overflow)/i.test(raw.trim())) {
    const parts = raw.trim().split(": ");
    chain = parts.length > 8 ? [...parts.slice(0, 7), parts.slice(7).join(": ")] : parts;
    if (chain.length < 2) chain = [];
  }
  const head = json ? raw.slice(0, open).trim().replace(/[:：]\s*$/, "") : (chain[0] ?? raw.trim().split("\n")[0]);
  return { head, message, status, fields, json, chain, raw };
}

// ---- 权限资源 ----
/// bash 的资源是 `{"command","workdir"}` JSON;路径类工具是路径;其余原样。
export function parsePermissionResource(action, resource) {
  const raw = String(resource ?? "");
  const trimmed = raw.trim();
  const base = { action: String(action ?? ""), command: "", workdir: "", path: "", raw };
  if (trimmed.startsWith("{")) {
    const value = parseJsonish(trimmed);
    if (value && typeof value.command === "string") {
      return { ...base, kind: "command", command: value.command, workdir: typeof value.workdir === "string" ? value.workdir : "" };
    }
  }
  if (trimmed && !/\s/.test(trimmed) && (/[\\/]/.test(trimmed) || /^[A-Za-z]:/.test(trimmed))) {
    return { ...base, kind: "path", path: trimmed };
  }
  return { ...base, kind: "text" };
}
/// 队列预览/拦截通知用的一行短文本:「bash · cargo test …」,绝不贴 JSON。
export function permissionResourceText(action, resource, max = 72) {
  const parsed = parsePermissionResource(action, resource);
  let body;
  if (parsed.kind === "command") {
    const lines = parsed.command.split(/\r?\n/).filter((line) => line.trim());
    body = `${(lines[0] ?? "").trim()}${lines.length > 1 ? " …" : ""}`;
  } else {
    body = parsed.kind === "path" ? parsed.path : parsed.raw.replace(/\s+/g, " ").trim();
  }
  return [parsed.action, clipText(body, max)].filter(Boolean).join(" · ");
}

/// 相对路径按所在文档目录解析成项目相对路径(`../../docs/x.md` + `.kanzei/project/architecture/`
/// → `.kanzei/docs/x.md`);越过项目根的 `..` 截在根上。绝对路径与盘符路径原样返回。
export function resolveRelativePath(baseDir, target) {
  const value = String(target ?? "").replace(/\\/g, "/");
  if (!value || value.startsWith("/") || /^[A-Za-z]:\//.test(value)) return value;
  const parts = String(baseDir ?? "").replace(/\\/g, "/").split("/").filter(Boolean);
  for (const segment of value.split("/")) {
    if (!segment || segment === ".") continue;
    if (segment === "..") parts.pop();
    else parts.push(segment);
  }
  return parts.join("/");
}

// ---- tracker 字段分类 ----
const TRACKER_FIELD_CLASSES = [
  ["engine", ["observed_head", "observed_worktree_hash", "recorded_at", "取活依据"]],
  ["meta", ["优先级", "复杂度", "标签", "批次", "阶段", "取得线", "严重度"]],
  ["refs", ["refs", "依赖", "前置", "关联", "关联缺陷", "设计文档"]],
  ["condition", ["停车", "阻塞", "parked", "blocked"]],
  ["list", ["验收", "边界", "批次表", "内容", "复现", "影响", "验收对账", "测试用例"]],
  ["timeline", ["进展", "对账", "来源", "确认记录"]],
];
/// engine | meta | refs | condition | list | timeline | prose。
export function classifyTrackerField(key) {
  const name = String(key ?? "").trim();
  for (const [kind, keys] of TRACKER_FIELD_CLASSES) if (keys.includes(name)) return kind;
  if (name.startsWith("状态纠正")) return "timeline";
  return "prose";
}
/// 发现记录的英文键 → 中文标签(中文键原样通过)。
export const DISCOVERY_LABEL_KEYS = { Intent: "意图", Explicit: "用户原话", Assumptions: "假设", Ambiguities: "歧义" };

// ---- 时间与指纹 ----
/// 10 位按秒、13 位按毫秒,返回毫秒数;其余 null。
export function formatEpoch(value) {
  const text = String(value ?? "").trim();
  if (/^\d{10}$/.test(text)) return Number(text) * 1000;
  if (/^\d{13}$/.test(text)) return Number(text);
  return null;
}
/// 测试记录的源码指纹:`v2 path@hash,path@hash` → 逐文件;旧格式只有一串 hash。
export function parseSourceFingerprint(text) {
  const value = String(text ?? "").trim();
  if (!value) return null;
  const match = value.match(/^(v\d+)\s+(.+)$/);
  if (!match) return { version: null, hash: value, items: [] };
  const items = match[2].split(",").map((item) => {
    const at = item.lastIndexOf("@");
    return at > 0 ? { path: item.slice(0, at).trim(), hash: item.slice(at + 1).trim() } : null;
  }).filter(Boolean);
  return { version: match[1], hash: "", items };
}

// ---- unified diff ----
const DIFF_LANGUAGES = {
  rs: "rust", js: "javascript", mjs: "javascript", ts: "typescript", py: "python", md: "markdown", json: "json",
  css: "css", html: "html", toml: "toml", ps1: "powershell", sh: "bash", yml: "yaml", yaml: "yaml",
};
/// `git diff` 文本 → 按文件的增删计数与行(与 06-activity renderDiff 的 display.lines 同形)。
/// hunk 内按 `@@ -a,b +c,d @@` 头给出的剩余行数逐行计数:被删掉的 SQL/Lua 注释行
/// `-- x` 在 diff 里写成 `--- x`,不能当成文件头吞掉;`---`/`+++` 只在 hunk 之外才是文件头。
export function parseUnifiedDiff(text) {
  const files = [];
  let file = null;
  let oldLine = 0;
  let newLine = 0;
  let oldLeft = 0;
  let newLeft = 0;
  const begin = (path) => {
    const clean = String(path ?? "").replace(/^[ab]\//, "").trim();
    const ext = clean.match(/\.([A-Za-z0-9]+)$/)?.[1]?.toLowerCase() ?? "";
    file = { path: clean, additions: 0, deletions: 0, language: DIFF_LANGUAGES[ext] ?? "text", binary: false, lines: [] };
    files.push(file);
  };
  for (const line of String(text ?? "").replace(/\r\n?/g, "\n").split("\n")) {
    if (file && (oldLeft > 0 || newLeft > 0)) {
      if (line.startsWith("\\")) continue; // `\ No newline at end of file`
      if (line.startsWith("+")) {
        newLeft -= 1;
        file.additions += 1;
        file.lines.push({ kind: "add", text: line.slice(1), old_line: null, new_line: newLine++ });
        continue;
      }
      if (line.startsWith("-")) {
        oldLeft -= 1;
        file.deletions += 1;
        file.lines.push({ kind: "del", text: line.slice(1), old_line: oldLine++, new_line: null });
        continue;
      }
      if (line.startsWith(" ") || line === "") {
        // 有的工具会剥掉空上下文行的前导空格:空行在 hunk 内按上下文计。
        oldLeft -= 1;
        newLeft -= 1;
        file.lines.push({ kind: "ctx", text: line.slice(1), old_line: oldLine++, new_line: newLine++ });
        continue;
      }
      // 行数对不上(被截断的 diff):退出 hunk,按头部规则重新识别这一行。
      oldLeft = 0;
      newLeft = 0;
    }
    const header = line.match(/^diff --git a\/(.+?) b\/(.+)$/);
    if (header) {
      begin(header[2]);
      continue;
    }
    if (line.startsWith("+++ ")) {
      const target = line.slice(4).trim();
      if (!file) begin(target);
      else if (target !== "/dev/null") file.path = target.replace(/^b\//, "");
      continue;
    }
    if (line.startsWith("--- ")) {
      if (!file) begin(line.slice(4).trim());
      continue;
    }
    if (!file) continue;
    const hunk = line.match(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/);
    if (hunk) {
      oldLine = Number(hunk[1]);
      newLine = Number(hunk[3]);
      oldLeft = hunk[2] === undefined ? 1 : Number(hunk[2]);
      newLeft = hunk[4] === undefined ? 1 : Number(hunk[4]);
      continue;
    }
    if (/^Binary files /.test(line)) {
      file.binary = true;
      continue;
    }
    // 没有 hunk 头的残缺 diff(截断或手写):宽松地按首字符计数,`---`/`+++` 已在上面当文件头。
    if (line.startsWith("+")) {
      file.additions += 1;
      file.lines.push({ kind: "add", text: line.slice(1), old_line: null, new_line: newLine++ });
    } else if (line.startsWith("-")) {
      file.deletions += 1;
      file.lines.push({ kind: "del", text: line.slice(1), old_line: oldLine++, new_line: null });
    } else if (line.startsWith(" ")) {
      file.lines.push({ kind: "ctx", text: line.slice(1), old_line: oldLine++, new_line: newLine++ });
    }
  }
  return files;
}
