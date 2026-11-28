export type Block =
  | { kind: "code"; lang: string; code: string }
  | { kind: "mermaid"; code: string }
  | { kind: "math"; expr: string }
  | { kind: "heading"; level: number; text: string }
  | { kind: "hr" }
  | { kind: "list"; ordered: boolean; start: number; items: Array<{ text: string; children: Block[] }> }
  | { kind: "details"; title: string; children: Block[] }
  | { kind: "section"; title: string; level: number; children: Block[] }
  | { kind: "quote"; lines: string[] }
  | { kind: "table"; headers: string[]; rows: string[][] }
  | { kind: "p"; text: string };

const FENCE = /^ {0,3}(\x60{3,}|~{3,})(.*)$/;
const LIST = /^(\s*)([-*+]|\d+\.)\s+(.+)$/;

function stripPartialTag(text: string): string {
  const start = text.lastIndexOf("<");
  if (start < 0) return text;
  const suffix = text.slice(start).toLowerCase();
  const partial = ["<details>", "</details>", "<summary>", "</summary>"].some(
    (tag) => tag !== suffix && tag.startsWith(suffix),
  );
  return partial ? text.slice(0, start) : text;
}

// Split structural tags only at block boundaries, leaving literal HTML in fences intact.
function markdownLines(raw: string, streaming: boolean): string[] {
  const result: string[] = [];
  let fence = "";
  const source = raw.replace(/\r\n/g, "\n").split("\n");
  for (const [index, line] of source.entries()) {
    const delimiter = line.match(FENCE)?.[1];
    if (fence) {
      result.push(line);
      if (delimiter?.startsWith(fence) && line.trim() === delimiter) fence = "";
    } else if (delimiter) {
      fence = delimiter;
      result.push(line);
    } else if (/^\s*<\/?(?:details|summary)>|<\/(?:details|summary)>\s*$/i.test(line)) {
      const parts = line.split(/(<\/?(?:details|summary)>)/i).filter((part) => part.trim());
      if (streaming && index === source.length - 1 && parts.length) {
        const last = parts.length - 1;
        parts[last] = stripPartialTag(parts[last]);
      }
      result.push(...parts);
    } else {
      result.push(streaming && index === source.length - 1 ? stripPartialTag(line) : line);
    }
  }
  return result;
}

function isTableHead(lines: string[], index: number): boolean {
  return (
    lines[index].includes("|") &&
    /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$/.test(lines[index + 1] ?? "")
  );
}

function parseLines(lines: string[]): Block[] {
  const root: Block[] = [];
  let blocks = root;
  const parents: Block[][] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i += 1;
      continue;
    }

    if (line.trim().toLowerCase() === "<details>") {
      i++;
      while (i < lines.length && !lines[i].trim()) i++;
      let title = "展开详情";
      if (lines[i]?.trim().toLowerCase() === "<summary>") {
        const parts: string[] = [];
        i++;
        while (i < lines.length && !/^<\/(?:summary|details)>$/i.test(lines[i].trim()))
          parts.push(lines[i++]);
        title = parts.join(" ").trim() || title;
        if (lines[i]?.trim().toLowerCase() === "</summary>") i++;
      }
      const children: Block[] = [];
      blocks.push({ kind: "details", title, children });
      parents.push(blocks);
      blocks = children;
      continue;
    }
    if (line.trim().toLowerCase() === "</details>") {
      blocks = parents.pop() ?? root;
      i++;
      continue;
    }

    // Horizontal rule
    if (/^\s*(?:---+|\*\*\*+|___+)\s*$/.test(line)) {
      blocks.push({ kind: "hr" });
      i += 1;
      continue;
    }

    // Fenced code or mermaid
    const fence = line.match(FENCE);
    if (fence) {
      const lang = fence[2].trim().toLowerCase();
      const buf: string[] = [];
      i += 1;
      while (
        i < lines.length &&
        !(lines[i].trim().startsWith(fence[1]) && [...lines[i].trim()].every((ch) => ch === fence[1][0]))
      ) {
        buf.push(lines[i]);
        i += 1;
      }
      if (i < lines.length) i += 1;
      const code = buf.join("\n");
      if (lang === "mermaid") {
        blocks.push({ kind: "mermaid", code });
      } else {
        blocks.push({ kind: "code", lang, code });
      }
      continue;
    }

    // Display math $$...$$
    if (line.trim().startsWith("$$")) {
      const rest = line.trim().slice(2);
      if (rest.endsWith("$$") && rest.length > 2) {
        blocks.push({ kind: "math", expr: rest.slice(0, -2).trim() });
        i += 1;
        continue;
      }
      const buf: string[] = [];
      if (rest) buf.push(rest);
      i += 1;
      while (i < lines.length && !lines[i].trim().endsWith("$$")) {
        buf.push(lines[i]);
        i += 1;
      }
      if (i < lines.length) {
        const tail = lines[i].trim().slice(0, -2);
        if (tail) buf.push(tail);
        i += 1;
      }
      blocks.push({ kind: "math", expr: buf.join("\n").trim() });
      continue;
    }

    // Heading
    const heading = line.match(/^(#{1,4})\s+(.+)$/);
    if (heading) {
      blocks.push({ kind: "heading", level: heading[1].length, text: heading[2].trim() });
      i += 1;
      continue;
    }

    // Blockquote
    if (line.startsWith(">")) {
      const buf: string[] = [];
      while (i < lines.length && lines[i].startsWith(">")) {
        buf.push(lines[i].replace(/^>\s?/, ""));
        i += 1;
      }
      blocks.push({ kind: "quote", lines: buf });
      continue;
    }

    // Markdown Table
    if (isTableHead(lines, i)) {
      const splitRow = (row: string) =>
        row
          .trim()
          .replace(/^\|/, "")
          .replace(/\|$/, "")
          .split("|")
          .map((c) => c.trim());
      const headers = splitRow(line);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && lines[i].includes("|") && lines[i].trim()) {
        rows.push(splitRow(lines[i]));
        i += 1;
      }
      blocks.push({ kind: "table", headers, rows });
      continue;
    }

    const first = line.match(LIST);
    if (first) {
      const ordered = /^\d/.test(first[2]);
      const indent = first[1].length;
      const items = [];
      while (i < lines.length) {
        const next = lines[i].match(LIST);
        if (!next || next[1].length !== indent || /^\d/.test(next[2]) !== ordered) break;
        i++;
        const nested = [];
        while (i < lines.length && lines[i].trim() && lines[i].search(/\S/) > indent) {
          nested.push(lines[i++].slice(indent + 1));
        }
        items.push({ text: next[3], children: parseLines(nested) });
      }
      blocks.push({ kind: "list", ordered, start: ordered ? parseInt(first[2], 10) : 1, items });
      continue;
    }

    // Paragraph
    const buf: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() &&
      !FENCE.test(lines[i]) &&
      !/^<\/?details>$/i.test(lines[i].trim()) &&
      !/^(#{1,4})\s+/.test(lines[i]) &&
      !/^\s*(?:---+|\*\*\*+|___+)\s*$/.test(lines[i]) &&
      !lines[i].startsWith(">") &&
      !isTableHead(lines, i) &&
      !LIST.test(lines[i])
    ) {
      buf.push(lines[i]);
      i += 1;
    }
    blocks.push({ kind: "p", text: buf.join("\n") });
  }

  return root;
}

export function parseBlocks(raw: string, streaming = false): Block[] {
  const blocks = parseLines(markdownLines(raw, streaming));
  const headings = blocks.filter((b) => b.kind === "heading" && (b.level === 2 || b.level === 3));
  if (raw.length <= 1200 || headings.length < 3) return blocks;
  const result: Block[] = [];
  const sections: Array<{ level: number; children: Block[] }> = [];
  for (const block of blocks) {
    if (block.kind === "heading") {
      while (sections.length && sections[sections.length - 1].level >= block.level) sections.pop();
      if (block.level === 2 || block.level === 3) {
        const children: Block[] = [];
        (sections.at(-1)?.children ?? result).push({
          kind: "section",
          level: block.level,
          title: block.text,
          children,
        });
        sections.push({ level: block.level, children });
        continue;
      }
    }
    (sections.at(-1)?.children ?? result).push(block);
  }
  return result;
}
