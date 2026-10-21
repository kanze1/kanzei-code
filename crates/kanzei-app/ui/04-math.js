import katex from "./vendor/katex/katex.mjs";
import { escapeHtml } from "./00-html.js";

const delimiters = [
  { open: "$$", close: "$$", display: true },
  { open: "\\[", close: "\\]", display: true },
  { open: "\\(", close: "\\)", display: false },
  { open: "$", close: "$", display: false },
];
function escapedAt(text, index) {
  let slashes = 0;
  while (index > 0 && text[--index] === "\\") slashes += 1;
  return slashes % 2 === 1;
}

// Shared by inline rendering and table splitting: TeX's |, \\ and Markdown
// punctuation belong to the formula, and backticks belong to code spans.
export function codeSpanAt(text, start) {
  if (text[start] !== "`" || escapedAt(text, start)) return null;
  let width = 1;
  while (text[start + width] === "`") width += 1;
  const marker = "`".repeat(width);
  let end = text.indexOf(marker, start + width);
  while (end >= 0 && (text[end - 1] === "`" || text[end + width] === "`")) end = text.indexOf(marker, end + width);
  return end < 0 ? null : { raw: text.slice(start, end + width), code: text.slice(start + width, end), end: end + width };
}
export function mathSpanAt(text, start) {
  if (escapedAt(text, start)) return null;
  const delimiter = delimiters.find(item => text.startsWith(item.open, start));
  if (!delimiter || delimiter.open === "$" && (!text[start + 1] || /\s/.test(text[start + 1]))) return null;
  for (let end = start + delimiter.open.length; end < text.length; end += 1) {
    if (text.startsWith(delimiter.close, end) && !escapedAt(text, end)) {
      // Dollar-delimited math follows the usual whitespace/currency boundary:
      // "$5 and $10" is prose, while "$1/3$" is a complete math expression.
      if (delimiter.open === "$" && (/\s/.test(text[end - 1]) || /[\d$]/.test(text[end + 1] || ""))) continue;
      return { raw: text.slice(start, end + delimiter.close.length), tex: text.slice(start + delimiter.open.length, end), end: end + delimiter.close.length, display: delimiter.display, closed: true };
    }
  }
  return { raw: text.slice(start), tex: text.slice(start + delimiter.open.length), end: text.length, display: delimiter.display, closed: false };
}
export function renderMathSpan(span) {
  const classes = `md-math${span.display ? " md-math-display" : ""}`;
  const source = ` data-math-source="${escapeHtml(span.raw)}"`;
  if (!span.closed) return `<span class="${classes} md-math-pending"${source}>${escapeHtml(span.raw)}</span>`;
  try {
    const html = katex.renderToString(span.tex, { displayMode: span.display, output: "htmlAndMathml", trust: false });
    return `<span class="${classes}"${source}>${html}</span>`;
  } catch (error) {
    if (!(error instanceof katex.ParseError)) throw error;
    return `<span class="${classes} md-math-error"${source} title="${escapeHtml(error.message)}">${escapeHtml(span.raw)}</span>`;
  }
}
