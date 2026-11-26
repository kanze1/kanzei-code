import React, { useEffect, useMemo, useRef, useState } from "react";
import { openFileInDrawer } from "../store/drawerStore";
import { pushToast } from "../store/uiStore";
import { IconCopy } from "./Icons";

interface MarkdownViewProps {
  text: string;
  searchQuery?: string;
}

type Block =
  | { kind: "code"; lang: string; code: string }
  | { kind: "mermaid"; code: string }
  | { kind: "math"; expr: string }
  | { kind: "heading"; level: number; text: string }
  | { kind: "hr" }
  | { kind: "ul"; items: string[] }
  | { kind: "ol"; items: string[] }
  | { kind: "quote"; lines: string[] }
  | { kind: "table"; headers: string[]; rows: string[][] }
  | { kind: "p"; text: string };

function parseBlocks(raw: string): Block[] {
  const lines = raw.replace(/\r\n/g, "\n").split("\n");
  const blocks: Block[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i += 1;
      continue;
    }

    // Horizontal rule
    if (/^\s*(?:---+|\*\*\*+|___+)\s*$/.test(line)) {
      blocks.push({ kind: "hr" });
      i += 1;
      continue;
    }

    // Fenced code or mermaid
    const fence = line.match(/^```([\w-]*)\s*$/);
    if (fence) {
      const lang = (fence[1] ?? "").toLowerCase();
      const buf: string[] = [];
      i += 1;
      while (i < lines.length && !/^```\s*$/.test(lines[i])) {
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
    if (
      line.includes("|") &&
      i + 1 < lines.length &&
      /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?\s*$/.test(lines[i + 1])
    ) {
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

    // Unordered list
    if (/^\s*[-*+]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*[-*+]\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*[-*+]\s+/, ""));
        i += 1;
      }
      blocks.push({ kind: "ul", items });
      continue;
    }

    // Ordered list
    if (/^\s*\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*\d+\.\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*\d+\.\s+/, ""));
        i += 1;
      }
      blocks.push({ kind: "ol", items });
      continue;
    }

    // Paragraph
    const buf: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() &&
      !/^```/.test(lines[i]) &&
      !/^(#{1,4})\s+/.test(lines[i]) &&
      !/^\s*(?:---+|\*\*\*+|___+)\s*$/.test(lines[i]) &&
      !lines[i].startsWith(">") &&
      !/^\s*[-*+]\s+/.test(lines[i]) &&
      !/^\s*\d+\.\s+/.test(lines[i])
    ) {
      buf.push(lines[i]);
      i += 1;
    }
    blocks.push({ kind: "p", text: buf.join("\n") });
  }

  return blocks;
}

const FILE_REF_RE = /^([\w./-]+\.(?:rs|ts|tsx|js|jsx|mjs|json|toml|md|css|html|py|go|sql|ps1|sh))(?::(\d+))?$/;

function renderInline(text: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  const tokenRe =
    /(`[^`]+`|\*\*[^*]+\*\*|~~[^~]+~~|\*[^*\n]+\*|\[([^\]]+)\]\(([^)]+)\)|\b(?:[\w./-]+\.(?:rs|ts|tsx|js|jsx|mjs|json|toml|md|css|html|py|go|sql|ps1|sh))(?::\d+)?\b)/g;
  let lastIndex = 0;
  let key = 0;

  for (const match of text.matchAll(tokenRe)) {
    const idx = match.index ?? 0;
    if (idx > lastIndex) {
      nodes.push(text.slice(lastIndex, idx));
    }
    const full = match[0];
    if (full.startsWith("`") && full.endsWith("`")) {
      const inner = full.slice(1, -1);
      const fileMatch = inner.match(FILE_REF_RE);
      if (fileMatch) {
        const filePath = fileMatch[1];
        const line = fileMatch[2] ? Number(fileMatch[2]) : null;
        nodes.push(
          <button
            key={key++}
            type="button"
            className="md-file-link mono"
            onClick={() => void openFileInDrawer(filePath, { line })}
            title="在右侧文件上下文抽屉打开"
          >
            {inner}
          </button>
        );
      } else {
        nodes.push(<code key={key++}>{inner}</code>);
      }
    } else if (full.startsWith("**") && full.endsWith("**")) {
      nodes.push(<strong key={key++}>{full.slice(2, -2)}</strong>);
    } else if (full.startsWith("~~") && full.endsWith("~~")) {
      nodes.push(<del key={key++}>{full.slice(2, -2)}</del>);
    } else if (full.startsWith("*") && full.endsWith("*") && full.length > 2) {
      nodes.push(<em key={key++}>{full.slice(1, -1)}</em>);
    } else if (match[2] && match[3]) {
      const label = match[2];
      const href = match[3];
      if (/^https?:\/\//i.test(href)) {
        nodes.push(
          <a key={key++} href={href} target="_blank" rel="noreferrer" className="md-file-link">
            {label}
          </a>
        );
      } else {
        nodes.push(
          <button
            key={key++}
            type="button"
            className="md-file-link"
            onClick={() => void openFileInDrawer(href)}
          >
            {label}
          </button>
        );
      }
    } else {
      const fileMatch = full.match(FILE_REF_RE);
      if (fileMatch) {
        const filePath = fileMatch[1];
        const line = fileMatch[2] ? Number(fileMatch[2]) : null;
        nodes.push(
          <button
            key={key++}
            type="button"
            className="md-file-link mono"
            onClick={() => void openFileInDrawer(filePath, { line })}
          >
            {full}
          </button>
        );
      } else {
        nodes.push(full);
      }
    }
    lastIndex = idx + full.length;
  }

  if (lastIndex < text.length) {
    nodes.push(text.slice(lastIndex));
  }
  return nodes;
}

let katexPromise: Promise<{ renderToString: (expr: string, opts?: Record<string, unknown>) => string }> | null =
  null;

function ensureKatex(): Promise<{
  renderToString: (expr: string, opts?: Record<string, unknown>) => string;
}> {
  if (!katexPromise) {
    katexPromise = (async () => {
      if (typeof document !== "undefined" && !document.getElementById("kz-katex-css")) {
        const link = document.createElement("link");
        link.id = "kz-katex-css";
        link.rel = "stylesheet";
        link.href = "./vendor/katex/katex.min.css";
        document.head.appendChild(link);
      }
      const modUrl = "./vendor/katex/katex.mjs";
      const mod = (await import(/* @vite-ignore */ modUrl)) as {
        default?: { renderToString: (expr: string, opts?: Record<string, unknown>) => string };
        renderToString?: (expr: string, opts?: Record<string, unknown>) => string;
      };
      return mod.default ?? (mod as { renderToString: (expr: string, opts?: Record<string, unknown>) => string });
    })();
  }
  return katexPromise;
}

function MathBlock({ expr }: { expr: string }): React.ReactElement {
  const [html, setHtml] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    ensureKatex()
      .then((katex) => {
        if (active) {
          setHtml(katex.renderToString(expr, { displayMode: true, throwOnError: false }));
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [expr]);

  if (html) {
    return <div className="md-mermaid-box" dangerouslySetInnerHTML={{ __html: html }} />;
  }
  return (
    <pre className="tool-pre">
      <code>{expr}</code>
    </pre>
  );
}

let mermaidScriptPromise: Promise<unknown> | null = null;

function ensureMermaid(): Promise<{
  initialize: (cfg: Record<string, unknown>) => void;
  render: (id: string, code: string) => Promise<{ svg: string }>;
}> {
  if (!mermaidScriptPromise) {
    mermaidScriptPromise = new Promise((resolve, reject) => {
      const w = window as unknown as { mermaid?: unknown };
      if (w.mermaid) {
        resolve(w.mermaid);
        return;
      }
      const script = document.createElement("script");
      script.src = "./vendor/mermaid/mermaid.min.js";
      script.onload = () => resolve((window as unknown as { mermaid?: unknown }).mermaid);
      script.onerror = reject;
      document.head.appendChild(script);
    });
  }
  return mermaidScriptPromise as Promise<{
    initialize: (cfg: Record<string, unknown>) => void;
    render: (id: string, code: string) => Promise<{ svg: string }>;
  }>;
}

let mermaidSeq = 1;

function MermaidBlock({ code }: { code: string }): React.ReactElement {
  const ref = useRef<HTMLDivElement>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    ensureMermaid()
      .then(async (mermaid) => {
        if (!mermaid || cancelled) return;
        const dark = document.documentElement.dataset.theme !== "light";
        mermaid.initialize({
          startOnLoad: false,
          theme: dark ? "dark" : "default",
          securityLevel: "strict",
        });
        const id = `kz-mmd-${mermaidSeq++}`;
        const res = await mermaid.render(id, code);
        if (!cancelled && ref.current) {
          ref.current.innerHTML = res.svg;
        }
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [code]);

  if (failed) {
    return (
      <div className="md-code-block">
        <div className="md-code-head">mermaid</div>
        <pre>
          <code>{code}</code>
        </pre>
      </div>
    );
  }
  return <div ref={ref} className="md-mermaid-box" />;
}

export const MarkdownView = React.memo(function MarkdownView({
  text,
}: MarkdownViewProps): React.ReactElement {
  const blocks = useMemo(() => parseBlocks(text), [text]);

  const copyCode = (code: string) => {
    navigator.clipboard?.writeText(code).then(
      () => pushToast("已复制代码", "ok", 1800),
      () => pushToast("复制失败", "error", 2200)
    );
  };

  return (
    <div className="md-body">
      {blocks.map((block, idx) => {
        switch (block.kind) {
          case "hr":
            return <hr key={idx} />;
          case "code":
            return (
              <div key={idx} className="md-code-block">
                <div className="md-code-head">
                  <span>{block.lang || "text"}</span>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    onClick={() => copyCode(block.code)}
                  >
                    <IconCopy size={12} />
                    <span>复制</span>
                  </button>
                </div>
                <pre>
                  <code>{block.code}</code>
                </pre>
              </div>
            );
          case "mermaid":
            return <MermaidBlock key={idx} code={block.code} />;
          case "math":
            return <MathBlock key={idx} expr={block.expr} />;
          case "heading": {
            const Tag = `h${block.level}` as "h1" | "h2" | "h3" | "h4";
            return <Tag key={idx}>{renderInline(block.text)}</Tag>;
          }
          case "quote":
            return (
              <blockquote key={idx}>
                {block.lines.map((l, i) => (
                  <p key={i}>{renderInline(l)}</p>
                ))}
              </blockquote>
            );
          case "ul":
            return (
              <ul key={idx}>
                {block.items.map((item, i) => (
                  <li key={i}>{renderInline(item)}</li>
                ))}
              </ul>
            );
          case "ol":
            return (
              <ol key={idx}>
                {block.items.map((item, i) => (
                  <li key={i}>{renderInline(item)}</li>
                ))}
              </ol>
            );
          case "table":
            return (
              <div key={idx} className="md-table-wrap">
                <table>
                  <thead>
                    <tr>
                      {block.headers.map((h, i) => (
                        <th key={i}>{renderInline(h)}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {block.rows.map((row, rIdx) => (
                      <tr key={rIdx}>
                        {row.map((cell, cIdx) => (
                          <td key={cIdx}>{renderInline(cell)}</td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          case "p":
            return <p key={idx}>{renderInline(block.text)}</p>;
        }
      })}
    </div>
  );
});
