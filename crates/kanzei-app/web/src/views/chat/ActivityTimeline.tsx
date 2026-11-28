import React, { useState } from "react";

export function ActivityTimeline({
  summary,
  failures,
  issue,
  children,
}: {
  summary: string;
  failures: number;
  issue?: string;
  children: React.ReactNode;
}): React.ReactElement {
  return (
    <details className="activity-timeline">
      <summary>
        {summary}
        {failures > 0 && (
          <span className="badge badge-err">
            {failures} 项失败{issue ? " · " + issue : ""}
          </span>
        )}
      </summary>
      <div className="activity-steps">{children}</div>
    </details>
  );
}

export function OutputPreview({
  text,
  lines = 8,
  tail = false,
}: {
  text: string;
  lines?: number;
  tail?: boolean;
}): React.ReactElement {
  const [expanded, setExpanded] = useState(false);
  const rows = text.split("\n");
  const long = rows.length > lines;
  return (
    <div className="output-preview">
      <pre className="tool-pre">
        {long && !expanded ? (tail ? rows.slice(-lines) : rows.slice(0, lines)).join("\n") : text}
      </pre>
      {long && (
        <button
          className="btn btn-xs btn-ghost"
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
        >
          {expanded ? "收起输出" : `展开全部输出（${rows.length} 行）`}
        </button>
      )}
    </div>
  );
}
