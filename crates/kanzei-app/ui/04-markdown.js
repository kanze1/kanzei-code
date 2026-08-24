// ---------- markdown-lite(安全子集:代码围栏/语言标识/行内码/加粗/标题/列表/表格/安全外链) ----------
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
export function renderInlineMarkdown(raw) {
  const placeholders = [];
  const stash = (html) => {
    const token = `\u0000md-${placeholders.length}\u0000`;
    placeholders.push(html);
    return token;
  };
  let html = escapeHtml(raw);
  html = html.replace(/`([^`\n]+)`/g, (_, code) => stash(`<code>${code}</code>`));
  html = html.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, label, url) => {
    const decodedUrl = url.replace(/&amp;/g, "&").replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">");
    const safeUrl = safeMarkdownUrl(decodedUrl);
    return safeUrl
      ? stash(`<a href="${escapeHtml(safeUrl)}" target="_blank" rel="noopener noreferrer">${label}</a>`)
      : `${label} (${escapeHtml(decodedUrl)})`;
  });
  html = html.replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>");
  return html.replace(/\u0000md-(\d+)\u0000/g, (_, index) => placeholders[Number(index)]);
}
export let renderMarkdown = function renderMarkdown(raw) {
  const lines = String(raw).replace(/\r\n?/g, "\n").split("\n");
  let html = "";
  let paragraph = [];
  let list = null;
  let code = null;
  const flushParagraph = () => {
    if (!paragraph.length) return;
    html += `<p>${renderInlineMarkdown(paragraph.join("\n"))}</p>`;
    paragraph = [];
  };
  const flushList = () => {
    if (!list) return;
    html += `<${list.type}>${list.items.map((item) => `<li>${renderInlineMarkdown(item)}</li>`).join("")}</${list.type}>`;
    list = null;
  };
  const flushCode = () => {
    if (!code) return;
    const language = code.language ? code.language.replace(/[^a-zA-Z0-9_+-]/g, "") : "";
    const className = language ? ` class="language-${language}"` : "";
    html += `<pre class="code"><code${className}>${escapeHtml(code.lines.join("\n"))}</code></pre>`;
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
    if (!line.trim()) {
      flushParagraph();
      flushList();
      continue;
    }
    const heading = line.match(/^\s{0,3}(#{1,6})\s+(.+)$/);
    if (heading) {
      flushParagraph();
      flushList();
      html += `<strong class="md-h">${renderInlineMarkdown(heading[2])}</strong>`;
      continue;
    }
    const listItem = line.match(/^\s*(?:[-*+]\s+|\d+[.]\s+)(.+)$/);
    if (listItem) {
      flushParagraph();
      const type = /^\s*\d+[.]\s+/.test(line) ? "ol" : "ul";
      if (list && list.type !== type) flushList();
      list ??= { type, items: [] };
      list.items.push(listItem[1]);
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
  if (code) flushCode();
  flushParagraph();
  flushList();
  return html;
}
export function isTableSeparator(line) {
  const cells = splitTableRow(line);
  return cells.length >= 2 && cells.every((cell) => /^:?-{3,}:?$/.test(cell));
}

export function setRenderMarkdown(value) { renderMarkdown = value; }
