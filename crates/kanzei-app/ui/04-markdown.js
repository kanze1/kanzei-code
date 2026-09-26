// ---------- markdown-lite(安全子集:代码围栏/语言标识/行内码/加粗/删除线/标题/列表/任务列表/引用/分隔线/表格/安全外链/路径链接) ----------
// 写进 DOM 的唯一入口是 renderMarkdownInto(el, raw, { streaming }):它在写完 HTML 后把闭合的 ```mermaid 围栏
// 换成图(04-diagram.js)。别处不得再写 `x.innerHTML = renderMarkdown(…)`(ui-markdown-smoke 静态门禁)。
import { hydrateDiagrams } from "./04-diagram.js";

export function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
export function splitTableRow(line) {
  const value = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  return value.split("|").map((cell) => cell.trim());
}
export function tableAlignment(cell) {
  if (/^:-+:$/.test(cell)) return "center";
  if (/^-+:$/.test(cell)) return "right";
  return "left";
}
export function safeMarkdownUrl(value) {
  const url = value.trim();
  return /^(?:https?:\/\/|mailto:)/i.test(url) && !/[\s"'<]/.test(url) ? url : null;
}
/// UI-0926 #10:无 scheme 的相对/盘符路径目标(可带 `:行` / `:行-行`)→ {path, line, endLine}。
/// 只放行路径字符;带控制字符、引号、尖括号或其它 scheme 的一律 null(调用方回落成「label (url)」)。
export function safeMarkdownPath(value) {
  let target = String(value ?? "").trim();
  try { target = decodeURIComponent(target); } catch { return null; }
  if (!target || /[\u0000-\u001f\u007f"'<>`]/.test(target)) return null;
  if (/^[a-z][\w+.-]*:/i.test(target) && !/^[A-Za-z]:[\\/]/.test(target)) return null;
  const match = target.match(/^((?:[A-Za-z]:[\\/])?[\p{L}\p{N}\p{M}_.@~\-\/\\ ()]+?)(?::(\d+)(?:-(\d+))?)?$/u);
  if (!match) return null;
  const path = match[1];
  if (!/[\\/]/.test(path) && !/\.[A-Za-z0-9]{1,8}$/.test(path)) return null;
  return { path, line: match[2] ? Number(match[2]) : null, endLine: match[3] ? Number(match[3]) : null };
}
export function renderInlineMarkdown(raw) {
  const placeholders = [];
  const stash = (html) => {
    const token = `\u0000md-${placeholders.length}\u0000`;
    placeholders.push(html);
    return token;
  };
  let html = escapeHtml(raw);
  html = html.replace(/`([^`\n]+)`/g, (_, code) => stash(`<code>${code}</code>`));
  html = html.replace(/\[([^\]]+)\]\((&lt;[^\n]+?&gt;|[^)\s]+)\)/g, (_, label, url) => {
    const decodedUrl = url.replace(/^&lt;|&gt;$/g, "").replace(/&amp;/g, "&").replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">");
    const safeUrl = safeMarkdownUrl(decodedUrl);
    if (safeUrl) return stash(`<a href="${escapeHtml(safeUrl)}" target="_blank" rel="noopener noreferrer">${label}</a>`);
    // 相对/绝对路径链接:不带 href(不让 WebView 自己导航),点击由 04-structured 的
    // structuredNav.openPath 委托处理;属性值全部转义。
    const local = safeMarkdownPath(decodedUrl);
    if (local) {
      const lineAttr = local.line ? ` data-line="${local.line}"` : "";
      const title = `${local.path}${local.line ? `:${local.line}${local.endLine ? `-${local.endLine}` : ""}` : ""}`;
      return stash(`<a class="md-path" data-path="${escapeHtml(local.path)}"${lineAttr} title="${escapeHtml(title)}">${label}</a>`);
    }
    return `${label} (${escapeHtml(decodedUrl)})`;
  });
  // 裸 URL 自动成链:只作用在代码与已成链接之外的文本段(二者已换成占位符),且必须过
  // safeMarkdownUrl。转义后的引号/尖括号实体与中文标点不属于 URL。
  html = html.replace(/https?:\/\/[^\s<>\u0000，。；）】」、]+/g, (hit) => {
    let url = hit;
    let tail = "";
    const cut = url.search(/&(?:quot|#39|lt|gt);/);
    if (cut >= 0) {
      tail = url.slice(cut);
      url = url.slice(0, cut);
    }
    const trailing = url.match(/[.,:!?)]+$/)?.[0] ?? "";
    if (trailing) url = url.slice(0, -trailing.length);
    const safeUrl = safeMarkdownUrl(url.replace(/&amp;/g, "&"));
    if (!safeUrl || url.length < 10) return hit;
    return `${stash(`<a href="${escapeHtml(safeUrl)}" target="_blank" rel="noopener noreferrer">${url}</a>`)}${trailing}${tail}`;
  });
  html = html.replace(/~~([^~\n]+)~~/g, "<del>$1</del>");
  html = html.replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>");
  html = html.replace(/\*([^*\n]+)\*/g, "<em>$1</em>");
  // 下划线强调只在词边界成立(CommonMark:词内的 _ 不开闭强调)。`live_design; last_verified_commit`、
  // `old_string` 这类标识符里的下划线原样保留;CJK 也算「词」,`中_文_字` 不斜体。
  html = html.replace(/(^|[^\p{L}\p{N}_])_([^_\n]+)_(?![\p{L}\p{N}_])/gu, "$1<em>$2</em>");
  // 占位符可以嵌套(链接文字里有行内码:`[`x.md`](path)`):链接的占位内容里还有码的占位符,
  // 单趟替换会把 `md-0` 原样漏到界面上。按引用逐层展开(只会引用更早的占位,不会成环)。
  const restore = (text) => text.replace(/\u0000md-(\d+)\u0000/g, (_, index) => restore(placeholders[Number(index)]));
  return restore(html);
}
export let renderMarkdown = function renderMarkdown(raw) {
  const lines = String(raw).replace(/\r\n?/g, "\n").split("\n");
  let html = "";
  let paragraph = [];
  let list = null;
  let listStack = [];
  let code = null;
  const flushParagraph = () => {
    if (!paragraph.length) return;
    html += `<p>${renderInlineMarkdown(paragraph.join("\n"))}</p>`;
    paragraph = [];
  };
  // UI-0926 #10:任务列表 `- [ ] x` / `- [x] x` 渲染成 ☐/☑ 字形(不用 input,只读)。
  const renderListItem = (item) => {
    const task = item.text.match(/^\[([ xX])\]\s+([\s\S]*)$/);
    const body = task
      ? `<span class="md-task" data-checked="${task[1] === " " ? "false" : "true"}">${task[1] === " " ? "☐" : "☑"}</span> ${renderInlineMarkdown(task[2])}`
      : renderInlineMarkdown(item.text);
    return `<li${task ? ' class="md-task-item"' : ""}>${body}${item.children.map(renderList).join("")}</li>`;
  };
  const renderList = (node) => `<${node.type}>${node.items.map(renderListItem).join("")}</${node.type}>`;
  let quote = [];
  const flushQuote = () => {
    if (!quote.length) return;
    html += `<blockquote class="md-quote">${quote.map(renderInlineMarkdown).join("<br>")}</blockquote>`;
    quote = [];
  };
  const flushList = () => {
    if (!list) return;
    html += renderList(list);
    list = null;
    listStack = [];
  };
  // open:到文末还没闭合的围栏(流式输出写到一半)打 data-open,hydrateDiagrams 据此不渲染半截图。
  const flushCode = ({ open = false } = {}) => {
    if (!code) return;
    const language = code.language ? code.language.replace(/[^a-zA-Z0-9_+-]/g, "") : "";
    const className = language ? ` class="language-${language}"` : "";
    html += `<pre class="code"${open ? ' data-open="true"' : ""}><code${className}>${escapeHtml(code.lines.join("\n"))}</code></pre>`;
    code = null;
  };
  const renderTable = (header, separator, rows) => {
    const alignments = separator.map(tableAlignment);
    const cell = (tag, value, index) => `<${tag} style="text-align:${alignments[index] || "left"}">${renderInlineMarkdown(value)}</${tag}>`;
    html += `<table><thead><tr>${header.map((value, index) => cell("th", value, index)).join("")}</tr></thead><tbody>`;
    for (const row of rows) html += `<tr>${header.map((_, index) => cell("td", row[index] || "", index)).join("")}</tr>`;
    html += "</tbody></table>";
  };
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const fence = line.match(/^\s*```\s*([^\s`]*)\s*$/);
    if (fence) {
      if (code) flushCode();
      else {
        flushQuote();
        flushParagraph();
        flushList();
        code = { language: fence[1], lines: [] };
      }
      continue;
    }
    if (code) {
      code.lines.push(line);
      continue;
    }
    // UI-0926 #10:连续的 `> ` 行合成一个引用块(行内语法照常,仍先转义)。
    const quoteLine = line.match(/^\s{0,3}>\s?(.*)$/);
    if (quoteLine) {
      flushParagraph();
      flushList();
      quote.push(quoteLine[1]);
      continue;
    }
    flushQuote();
    if (!line.trim()) {
      flushParagraph();
      flushList();
      continue;
    }
    // 单独一行的 --- / *** / ___ 是分隔线(先于列表判定:`* * *` 不是列表项)。
    if (/^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/.test(line)) {
      flushParagraph();
      flushList();
      html += "<hr>";
      continue;
    }
    const heading = line.match(/^\s{0,3}(#{1,6})\s+(.+)$/);
    if (heading) {
      flushParagraph();
      flushList();
      html += `<h${heading[1].length} class="md-h md-h-${heading[1].length}">${renderInlineMarkdown(heading[2])}</h${heading[1].length}>`;
      continue;
    }
    const listItem = line.match(/^(\s*)(?:([-*+]\s+)|(\d+[.]\s+))(.+)$/);
    if (listItem) {
      flushParagraph();
      const indent = listItem[1].replace(/\t/g, "  ").length;
      const type = listItem[3] ? "ol" : "ul";
      if (!list) {
        list = { type, items: [] };
        listStack = [{ node: list, indent, item: null }];
      } else {
        while (listStack.length > 1 && indent < listStack.at(-1).indent) listStack.pop();
        const current = listStack.at(-1);
        if (indent > current.indent && current.item) {
          const nested = { type, items: [] };
          current.item.children.push(nested);
          listStack.push({ node: nested, indent, item: null });
        } else if (indent === current.indent && type !== current.node.type) {
          flushList();
          list = { type, items: [] };
          listStack = [{ node: list, indent, item: null }];
        }
      }
      const current = listStack.at(-1);
      const item = { text: listItem[4], children: [] };
      current.node.items.push(item);
      current.item = item;
      continue;
    }
    const nextLine = lines[index + 1];
    if (line.includes("|") && nextLine && isTableSeparator(nextLine)) {
      flushParagraph();
      flushList();
      const header = splitTableRow(line);
      const separator = splitTableRow(nextLine);
      const rows = [];
      index += 2;
      while (index < lines.length && lines[index].includes("|") && lines[index].trim()) {
        rows.push(splitTableRow(lines[index]));
        index += 1;
      }
      index -= 1;
      renderTable(header, separator, rows);
      continue;
    }
    flushList();
    paragraph.push(line);
  }
  if (code) flushCode({ open: true });
  flushQuote();
  flushParagraph();
  flushList();
  return html;
}
export function isTableSeparator(line) {
  const cells = splitTableRow(line);
  return cells.length >= 2 && cells.every((cell) => /^:?-{3,}:?$/.test(cell));
}

export function setRenderMarkdown(value) { renderMarkdown = value; }

// UI2-0926 #8:渲染后装饰钩子(24-preview.js 给闭合的 ```html / ```svg 代码块加「预览」)。本模块不知道装饰方是谁,
// 保持只依赖 04-diagram;钩子抛错只告警,不影响正文渲染。
const markdownHooks = [];
export function addMarkdownHook(fn) {
  if (typeof fn === "function") markdownHooks.push(fn);
}

/// 把 markdown 渲染进 el(唯一入口):写 HTML,再把闭合的 ```mermaid 围栏换成图,最后跑装饰钩子。
/// streaming:流式每帧重渲的调用方(聊天正文/思考块)——未闭合的围栏本来就带 data-open 不渲染,
/// 已闭合的缓存命中同步替换,不闪。
export function renderMarkdownInto(el, raw, { streaming = false } = {}) {
  if (!el) return el;
  el.innerHTML = renderMarkdown(String(raw ?? ""));
  hydrateDiagrams(el, { streaming });
  for (const hook of markdownHooks) {
    try { hook(el, { streaming }); } catch (err) { console.warn(`markdown 装饰钩子失败: ${err}`); }
  }
  return el;
}
