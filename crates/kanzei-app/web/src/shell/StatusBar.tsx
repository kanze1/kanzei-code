import React, { useState } from "react";
import { formatError, invoke } from "../ipc/client";
import { useAppStore } from "../store/appStore";
import { useChatStore } from "../store/chatStore";
import { getActiveProcess, useSessionStore } from "../store/sessionStore";
import { openViewer, pushToast, uiStore, useUiStore } from "../store/uiStore";
import { IconClose, IconLayers, IconTerminal } from "../components/Icons";

interface ContextBlockItem {
  source?: string;
  chars?: number;
  sha256?: string;
}

interface ContextSnapshotResponse {
  requests?: Array<{ sequence: number; step?: number; model?: string }>;
  selected?: {
    sequence: number;
    manifest?: {
      model?: string;
      estimated_tokens?: number;
      blocks?: ContextBlockItem[];
    };
  } | null;
}

export function StatusBar(): React.ReactElement {
  const appInfo = useAppStore((s) => s.appInfo);
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const stageText = useChatStore((s) => s.stageText);
  const lastUsage = useChatStore((s) => s.lastUsage);
  const autoAllow = useChatStore((s) => s.autoAllow);
  const logPanelOpen = useUiStore((s) => s.logPanelOpen);
  const logCount = useUiStore((s) => s.logs.length);
  const activeProcess = useSessionStore((s) =>
    s.processes.find((p) => p.id === s.activeProcessId) ?? null
  );
  const running = Boolean(activeProcess?.running);
  const [ctxSnapshot, setCtxSnapshot] = useState<ContextSnapshotResponse | null>(null);
  const [ctxOpen, setCtxOpen] = useState(false);

  const openContextInspector = async () => {
    const proc = activeProcess ?? getActiveProcess();
    if (!projectDir || !proc) {
      pushToast("当前没有可检查上下文的会话", "info");
      return;
    }
    try {
      const snap = await invoke<ContextSnapshotResponse>("context_snapshot", {
        projectDir,
        processId: proc.id,
      });
      setCtxSnapshot(snap);
      setCtxOpen(true);
    } catch (err) {
      pushToast(`读取上下文构成失败：${formatError(err)}`, "error");
    }
  };

  const inspectBlock = async (sequence: number, block: ContextBlockItem) => {
    const proc = activeProcess ?? getActiveProcess();
    if (!projectDir || !proc || !block.sha256) return;
    try {
      const text = await invoke<string>("context_content", {
        projectDir,
        processId: proc.id,
        sequence,
        sha256: block.sha256,
      });
      openViewer({
        title: block.source ?? "上下文块",
        subtitle: `请求 #${sequence} · ${block.chars ?? text.length} 字符`,
        content: text,
        kind: "text",
      });
    } catch (err) {
      pushToast(`读取原文失败：${formatError(err)}`, "error");
    }
  };

  return (
    <>
      <footer id="statusbar" className={running ? "running" : ""}>
        <div className="statusbar-group">
          <span className={`status-dot${running ? " running" : " ok"}`} />
          <span className="statusbar-stage">{running ? stageText : "就绪"}</span>
          {lastUsage && (lastUsage.input > 0 || lastUsage.output > 0) && (
            <span className="statusbar-usage" title="最近一轮 Token 消耗">
              In {lastUsage.input.toLocaleString()} / Out {lastUsage.output.toLocaleString()}
            </span>
          )}
          {autoAllow && <span className="badge badge-warn">自动放行</span>}
        </div>

        <div className="statusbar-group">
          <button
            type="button"
            className={`statusbar-btn${ctxOpen ? " active" : ""}`}
            onClick={() => void openContextInspector()}
          >
            <IconLayers size={12} />
            <span>上下文构成</span>
          </button>
          <button
            type="button"
            className={`statusbar-btn${logPanelOpen ? " active" : ""}`}
            onClick={() => uiStore.setState({ logPanelOpen: !logPanelOpen })}
          >
            <IconTerminal size={12} />
            <span>运行日志 ({logCount})</span>
          </button>
          <span className="statusbar-version" title={appInfo.build}>
            v{appInfo.version}
          </span>
        </div>
      </footer>

      {ctxOpen && (
        <div
          className="modal-scrim"
          role="dialog"
          aria-modal="true"
          aria-label="本轮请求上下文构成"
          onClick={() => setCtxOpen(false)}
        >
          <div className="modal-panel modal-lg" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <div className="modal-title">本轮请求上下文构成</div>
              <button
                type="button"
                className="btn btn-ghost btn-icon"
                onClick={() => setCtxOpen(false)}
                aria-label="关闭"
              >
                <IconClose size={14} />
              </button>
            </div>
            <div className="modal-body">
              {!ctxSnapshot?.selected?.manifest ? (
                <div className="empty-state">
                  <p>当前会话还没有记录到模型请求快照。</p>
                </div>
              ) : (
                <>
                  <div className="ctx-meta-strip">
                    <span>模型：{ctxSnapshot.selected.manifest.model ?? "-"}</span>
                    <span>
                      估算 Token：{ctxSnapshot.selected.manifest.estimated_tokens?.toLocaleString() ?? "-"}
                    </span>
                    <span>请求序号：#{ctxSnapshot.selected.sequence}</span>
                  </div>
                  <div className="ctx-block-list">
                    {(ctxSnapshot.selected.manifest.blocks ?? []).map((b, i) => (
                      <div key={i} className="ctx-block-row">
                        <span className="mono">{b.source ?? `块 #${i + 1}`}</span>
                        <div className="ctx-block-right">
                          <span className="field-hint">{b.chars ?? 0} 字符</span>
                          {b.sha256 && (
                            <button
                              type="button"
                              className="btn btn-sm"
                              onClick={() => void inspectBlock(ctxSnapshot.selected!.sequence, b)}
                            >
                              查看原文
                            </button>
                          )}
                        </div>
                      </div>
                    ))}
                  </div>
                </>
              )}
            </div>
          </div>
        </div>
      )}
    </>
  );
}
