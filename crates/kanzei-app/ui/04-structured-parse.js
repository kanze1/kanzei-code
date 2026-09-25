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
  return symbolRatio(text) > 0.25;
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
