import React from "react";
import { clearLogs, uiStore, useUiStore } from "../store/uiStore";

export function LogPanel(): React.ReactElement | null {
  const open = useUiStore((s) => s.logPanelOpen);
  const logs = useUiStore((s) => s.logs);

  if (!open) return null;

  return (
    <section className="log-panel" aria-label="运行日志">
      <div className="log-panel-header">
        <span>运行日志（最近 {logs.length} 条）</span>
        <div style={{ display: "flex", gap: 6 }}>
          <button type="button" className="btn btn-ghost btn-sm" onClick={clearLogs}>
            清空
          </button>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => uiStore.setState({ logPanelOpen: false })}
          >
            收起
          </button>
        </div>
      </div>
      <div className="log-panel-body">
        {logs.length === 0 ? (
          <div style={{ color: "var(--dim)" }}>暂无运行日志</div>
        ) : (
          logs.map((entry) => (
            <div key={entry.id} className={`log-line log-${entry.level}`}>
              [{new Date(entry.at).toLocaleTimeString()}] {entry.text}
            </div>
          ))
        )}
      </div>
    </section>
  );
}
