import React from "react";
import { clearLogs, uiStore, useUiStore } from "../store/uiStore";
import { IconClose, IconTerminal, IconTrash } from "../components/Icons";

export function LogPanel(): React.ReactElement | null {
  const open = useUiStore((s) => s.logPanelOpen);
  const logs = useUiStore((s) => s.logs);

  if (!open) return null;

  return (
    <section className="log-panel" aria-label="运行日志">
      <div className="log-panel-header">
        <span className="log-panel-title">
          <IconTerminal size={13} />
          <span>运行日志</span>
          <span className="badge">{logs.length}</span>
        </span>
        <div className="log-panel-actions">
          <button type="button" className="btn btn-ghost btn-sm" onClick={clearLogs}>
            <IconTrash size={12} />
            <span>清空</span>
          </button>
          <button
            type="button"
            className="btn btn-ghost btn-icon btn-sm"
            onClick={() => uiStore.setState({ logPanelOpen: false })}
            aria-label="收起日志"
            title="收起"
          >
            <IconClose size={12} />
          </button>
        </div>
      </div>
      <div className="log-panel-body">
        {logs.length === 0 ? (
          <div className="field-hint">暂无运行日志</div>
        ) : (
          logs.map((entry) => (
            <div key={entry.id} className={`log-line log-${entry.level}`}>
              <span className="log-line-time">[{new Date(entry.at).toLocaleTimeString()}]</span>
              <span>{entry.text}</span>
            </div>
          ))
        )}
      </div>
    </section>
  );
}
