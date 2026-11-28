import { parseBlocks, type Block } from "./markdownBlocks";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { openFileInDrawer } from "../store/drawerStore";
import { pushToast } from "../store/uiStore";
import { IconCopy } from "./Icons";

interface MarkdownViewProps {
  text: string;
  searchQuery?: string;
  streaming?: boolean;
}

const FILE_REF_RE = /^([\w./-]+\.(?:rs|ts|tsx|js|jsx|mjs|json|toml|md|css|html|py|go|sql|ps1|sh))(?::(\d+))?$/;

function renderInline(text: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  const tokenRe =
    /(`[^`]+`|\*\*[^*]+\*\*|~~[^~]+~~|\*[^*\n]+\*|\[([^\]]+)\]\((<[^<>\n]+>|[^)\n]+)\)|\b(?:[\w./-]+\.(?:rs|ts|tsx|js|jsx|mjs|json|toml|md|css|html|py|go|sql|ps1|sh))(?::\d+)?\b)/g;
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
      const target = match[3].trim();
      const href = target.startsWith("<") && target.endsWith(">") ? target.slice(1, -1) : target;
      if (/^https?:\/\//i.test(href)) {
        nodes.push(
          <a key={key++} href={href} target="_blank" rel="noreferrer" className="md-file-link">
            {label}
          </a>
        );
      } else {
        const anchor = /(?:#L([1-9]\d*)(?:-L[1-9]\d*)?|:([1-9]\d*))$/i.exec(href);
        const path = anchor ? href.slice(0, anchor.index) : href;
        const line = anchor ? Number(anchor[1] ?? anchor[2]) : null;
        nodes.push(
          <button
            key={key++}
            type="button"
            className="md-file-link"
            onClick={() => void openFileInDrawer(path, { line })}
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

function CodeBlock({ lang, code }: { lang: string; code: string }) {
  const [expanded, setExpanded] = useState(false);
  const lines = code.split("\n");
  const long = lines.length > 18;
  const copyCode = (code: string) => {
    navigator.clipboard?.writeText(code).then(
      () => pushToast("已复制代码", "ok", 1800),
      () => pushToast("复制失败", "error", 2200),
    );
  };

  return (
    <div className="md-code-block">
      <div className="md-code-head">
        <span>{lang || "text"}</span>
        <button type="button" className="btn btn-ghost btn-sm" onClick={() => copyCode(code)}>
          <IconCopy size={12} />
          <span>复制</span>
        </button>
      </div>
      <pre className={long && !expanded ? "reading-fade" : undefined}>
        <code>{long && !expanded ? lines.slice(0, 14).join("\n") : code}</code>
      </pre>
      {long && (
        <FoldControl
          expanded={expanded}
          toggle={() => setExpanded(!expanded)}
          label={"展开完整代码（共 " + lines.length + " 行）"}
        />
      )}
    </div>
  );
}

function FoldControl({ expanded, toggle, label }: { expanded: boolean; toggle: () => void; label: string }) {
  return (
    <button
      type="button"
      className="btn btn-xs btn-ghost fold-control"
      aria-expanded={expanded}
      onClick={toggle}
    >
      {expanded ? "收起" : label}
    </button>
  );
}

function RenderBlocks({ blocks }: { blocks: Block[] }) {
  return (
    <>
      {blocks.map((block, idx) => {
        switch (block.kind) {
          case "details":
          case "section":
            return (
              <details key={idx} open={block.kind === "section"} className={"md-" + block.kind}>
                <summary>
                  {block.kind === "section" ? (
                    <span role="heading" aria-level={block.level}>
                      {renderInline(block.title)}
                    </span>
                  ) : (
                    renderInline(block.title)
                  )}
                </summary>
                <RenderBlocks blocks={block.children} />
              </details>
            );
          case "hr":
            return <hr key={idx} />;
          case "code":
            return <CodeBlock key={idx} code={block.code} lang={block.lang} />;
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
          case "list": {
            const Tag = block.ordered ? "ol" : "ul";
            return (
              <Tag key={idx} start={block.ordered ? block.start : undefined}>
                {block.items.map((item, index) => (
                  <li key={index}>
                    {renderInline(item.text)}
                    <RenderBlocks blocks={item.children} />
                  </li>
                ))}
              </Tag>
            );
          }
          case "p":
            return <p key={idx}>{renderInline(block.text)}</p>;
          case "table":
            return <MarkdownTable key={idx} block={block} />;
        }
      })}
    </>
  );
}

function MarkdownTable({ block }: { block: Extract<Block, { kind: "table" }> }) {
  const [expanded, setExpanded] = useState(false);
  const long = block.rows.length > 10;
  return (
    <div className="md-table-wrap">
      <table>
        <thead>
          <tr>
            {block.headers.map((h, i) => (
              <th key={i}>{renderInline(h)}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {(long && !expanded ? block.rows.slice(0, 8) : block.rows).map((row, rIdx) => (
            <tr key={rIdx}>
              {row.map((cell, cIdx) => (
                <td key={cIdx}>{renderInline(cell)}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      {long && (
        <FoldControl
          expanded={expanded}
          toggle={() => setExpanded(!expanded)}
          label={"显示全部 " + block.rows.length + " 行"}
        />
      )}
    </div>
  );
}

export const MarkdownView = React.memo(function MarkdownView({ text, streaming = false }: MarkdownViewProps) {
  const blocks = useMemo(() => parseBlocks(text, streaming), [text, streaming]);
  const root = useRef<HTMLDivElement>(null);
  const outline = useMemo(() => {
    const titles: string[] = [];
    function collect(nodes: Block[]) {
      for (const block of nodes)
        if (block.kind === "section") {
          titles.push(block.title);
          collect(block.children);
        }
    }
    if (text.length > 2000) collect(blocks);
    return titles;
  }, [blocks, text.length]);
  return (
    <div className="md-body" ref={root}>
      {outline.length > 0 && (
        <details className="md-outline">
          <summary>本条提纲（{outline.length} 节）</summary>
          {outline.map((title, index) => (
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              key={index}
              onClick={() => {
                const section = root.current?.querySelectorAll<HTMLDetailsElement>(".md-section")[index];
                if (section) {
                  for (
                    let parent: HTMLDetailsElement | null = section;
                    parent;
                    parent = parent.parentElement?.closest("details") ?? null
                  )
                    parent.open = true;
                  section.querySelector("summary")?.focus();
                  section.scrollIntoView({ block: "nearest" });
                }
              }}
            >
              {title}
            </button>
          ))}
        </details>
      )}
      <RenderBlocks blocks={blocks} />
    </div>
  );
});
