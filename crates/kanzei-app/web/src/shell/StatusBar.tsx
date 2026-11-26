import React, { useCallback, useEffect, useMemo, useState } from "react";
import { formatError, invoke } from "../ipc/client";
import { useAppStore } from "../store/appStore";
import {
  clearCurrentConversation,
  compactCurrentConversation,
  useChatStore,
} from "../store/chatStore";
import { getActiveProcess, useSessionStore } from "../store/sessionStore";
import { openViewer, pushToast, uiStore, useUiStore } from "../store/uiStore";
import { IconClose, IconLayers, IconTerminal } from "../components/Icons";

interface ContextBlockItem {
  source?: string;
  role?: string;
  kind?: string;
  chars?: number;
  sha256?: string;
}

interface ContextSnapshotResponse {
  requests?: Array<{ sequence: number; step?: number; model?: string; at?: number }>;
  selected?: {
    sequence: number;
    step?: number;
    manifest?: {
      model?: string;
      estimated_tokens?: number;
      blocks?: ContextBlockItem[];
    };
  } | null;
}

interface BreakdownCategory {
  key: string;
  label: string;
  color: string;
  chars: number;
  count: number;
}

function classifyContextBlock(block: ContextBlockItem): { key: string; label: string; color: string } {
  const src = `${block.source ?? ""} ${block.kind ?? ""} ${block.role ?? ""}`.toLowerCase();
  if (src.includes("memory") || src.includes("recall") || src.includes("preference")) {
    return { key: "memory", label: "长期记忆与偏好", color: "#a855f7" };
  }
  if (src.includes("skill")) {
    return { key: "skill", label: "技能规范", color: "#ec4899" };
  }
  if (src.includes("tool") || src.includes("schema")) {
    return { key: "tools", label: "工具与结果", color: "#f59e0b" };
  }
  if (src.includes("system") || src.includes("agent") || src.includes("instruction")) {
    return { key: "system", label: "系统提示与规则", color: "#3b82f6" };
  }
  if (src.includes("web") || src.includes("attach") || src.includes("image")) {
    return { key: "web", label: "网页与附件", color: "#14b8a6" };
  }
  return { key: "history", label: "对话历史与消息", color: "#22c55e" };
}

export function StatusBar(): React.ReactElement {
  const appInfo = useAppStore((s) => s.appInfo);
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const stageText = useChatStore((s) => s.stageText);
  const lastUsage = useChatStore((s) => s.lastUsage);
  const autoAllow = useChatStore((s) => s.autoAllow);
  const autoRun = useSessionStore((s) => s.autoRun);
  const effectiveModel = useSessionStore((s) => s.effectiveModel);
  const logPanelOpen = useUiStore((s) => s.logPanelOpen);
  const logCount = useUiStore((s) => s.logs.length);
  const contextInspectorNonce = useUiStore((s) => s.contextInspectorNonce);
  const activeProcess = useSessionStore((s) =>
    s.processes.find((p) => p.id === s.activeProcessId) ?? null
  );
  const running = Boolean(activeProcess?.running);
  const [ctxSnapshot, setCtxSnapshot] = useState<ContextSnapshotResponse | null>(null);
  const [ctxOpen, setCtxOpen] = useState(false);

  const openContextInspector = useCallback(
    async (sequence?: number | null) => {
      const proc = activeProcess ?? getActiveProcess();
      if (!projectDir || !proc) {
        pushToast("当前没有可检查上下文的会话", "info");
        return;
      }
      try {
        const snap = await invoke<ContextSnapshotResponse>("context_snapshot", {
          projectDir,
          processId: proc.id,
          sequence: sequence ?? null,
        });
        setCtxSnapshot(snap);
        setCtxOpen(true);
      } catch (err) {
        pushToast(`读取上下文构成失败：${formatError(err)}`, "error");
      }
    },
    [activeProcess, projectDir]
  );

  useEffect(() => {
    if (contextInspectorNonce > 0) {
      void openContextInspector();
    }
  }, [contextInspectorNonce, openContextInspector]);

  const breakdown = useMemo<BreakdownCategory[]>(() => {
    const blocks = ctxSnapshot?.selected?.manifest?.blocks ?? [];
    const map = new Map<string, BreakdownCategory>();
    for (const b of blocks) {
      const cat = classifyContextBlock(b);
      const existing = map.get(cat.key);
      const chars = Math.max(0, Number(b.chars ?? 0));
      if (existing) {
        existing.chars += chars;
        existing.count += 1;
      } else {
        map.set(cat.key, { ...cat, chars, count: 1 });
      }
    }
    return Array.from(map.values()).sort((a, b) => b.chars - a.chars);
  }, [ctxSnapshot]);

  const totalBlockChars = useMemo(
    () => breakdown.reduce((acc, item) => acc + item.chars, 0),
    [breakdown]
  );

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

  const contextLimit = effectiveModel?.contextLimit ?? null;
  const usedTokens = lastUsage ? lastUsage.input + lastUsage.output : 0;
  const contextPct =
    contextLimit && contextLimit > 0 && usedTokens > 0
      ? Math.min(100, Math.round((usedTokens / contextLimit) * 100))
      : null;
  const contextHigh = contextPct !== null && contextPct >= 80;

  return (
    <>
      <footer id="statusbar" className={running ? "running" : ""}>
        <div className="statusbar-group">
          <span className={`status-dot${running ? " running" : " ok"}`} />
          <span className="statusbar-stage">{running ? stageText : "就绪"}</span>
          {lastUsage && (lastUsage.input > 0 || lastUsage.output > 0) && (
            <button
              type="button"
              className={`statusbar-btn statusbar-usage${contextHigh ? " warn" : ""}`}
              onClick={() => void openContextInspector()}
              title={
                contextLimit
                  ? `最近一轮 Token：输入 ${lastUsage.input.toLocaleString()} / 输出 ${lastUsage.output.toLocaleString()}，窗口上限 ${contextLimit.toLocaleString()}（点击查看上下文构成）`
                  : "最近一轮 Token 消耗（点击查看上下文构成）"
              }
            >
              <span>
                In {lastUsage.input.toLocaleString()} / Out {lastUsage.output.toLocaleString()}
              </span>
              {contextPct !== null && (
                <span className="statusbar-ctx-meter" aria-label={`上下文占用 ${contextPct}%`}>
                  <span className="statusbar-ctx-bar">
                    <span
                      className={`statusbar-ctx-fill${contextHigh ? " warn" : ""}`}
                      style={{ width: `${contextPct}%` }}
                    />
                  </span>
                  <span>{contextPct}%</span>
                </span>
              )}
            </button>
          )}
          {contextHigh && !running && (
            <button
              type="button"
              className="statusbar-btn"
              style={{ color: "var(--warn)" }}
              onClick={() => void compactCurrentConversation()}
              title="上下文已超过 80%，建议压缩历史以保留空间"
            >
              建议压缩
            </button>
          )}
          {!generalMode && autoAllow && autoRun.enabled && !autoRun.paused && (
            <span className="badge badge-warn">自动放行</span>
          )}
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
                      {contextLimit
                        ? ` / ${contextLimit.toLocaleString()} (${Math.min(
                            100,
                            Math.round(
                              ((ctxSnapshot.selected.manifest.estimated_tokens ?? 0) / contextLimit) * 100
                            )
                          )}%)`
                        : ""}
                    </span>
                    {ctxSnapshot.requests && ctxSnapshot.requests.length > 1 ? (
                      <select
                        className="composer-select"
                        aria-label="切换历史请求快照"
                        value={String(ctxSnapshot.selected.sequence)}
                        onChange={(e) => void openContextInspector(Number(e.target.value))}
                      >
                        {ctxSnapshot.requests.map((r) => (
                          <option key={r.sequence} value={String(r.sequence)}>
                            请求 #{r.sequence}
                            {typeof r.step === "number" ? ` · 第 ${r.step} 步` : ""}
                          </option>
                        ))}
                      </select>
                    ) : (
                      <span>请求序号：#{ctxSnapshot.selected.sequence}</span>
                    )}
                    <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
                      <button
                        type="button"
                        className="btn btn-sm"
                        disabled={running}
                        onClick={() => {
                          setCtxOpen(false);
                          void compactCurrentConversation();
                        }}
                      >
                        立即压缩
                      </button>
                      <button
                        type="button"
                        className="btn btn-ghost btn-sm"
                        disabled={running}
                        onClick={() => {
                          setCtxOpen(false);
                          void clearCurrentConversation();
                        }}
                      >
                        清空重开
                      </button>
                    </div>
                  </div>

                  {breakdown.length > 0 && totalBlockChars > 0 && (
                    <div className="ctx-breakdown-box">
                      <div className="ctx-breakdown-bar" role="img" aria-label="上下文构成占比条">
                        {breakdown.map((cat) => {
                          const pct = Math.max(1, Math.round((cat.chars / totalBlockChars) * 100));
                          return (
                            <span
                              key={cat.key}
                              className="ctx-breakdown-seg"
                              style={{ width: `${pct}%`, background: cat.color }}
                              title={`${cat.label}：${cat.chars.toLocaleString()} 字符 (${pct}%)`}
                            />
                          );
                        })}
                      </div>
                      <div className="ctx-breakdown-legend">
                        {breakdown.map((cat) => {
                          const pct = Math.round((cat.chars / totalBlockChars) * 100);
                          return (
                            <span key={cat.key} className="ctx-legend-item">
                              <span className="ctx-legend-dot" style={{ background: cat.color }} />
                              <span>{cat.label}</span>
                              <span className="mono field-hint">
                                {pct}% · {cat.chars.toLocaleString()} 字 ({cat.count})
                              </span>
                            </span>
                          );
                        })}
                      </div>
                    </div>
                  )}

                  <div className="ctx-block-list">
                    {(ctxSnapshot.selected.manifest.blocks ?? []).map((b, i) => {
                      const cat = classifyContextBlock(b);
                      return (
                        <div key={i} className="ctx-block-row">
                          <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                            <span className="ctx-legend-dot" style={{ background: cat.color }} />
                            <span className="mono">{b.source ?? `块 #${i + 1}`}</span>
                          </span>
                          <div className="ctx-block-right">
                            <span className="field-hint">{(b.chars ?? 0).toLocaleString()} 字符</span>
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
                      );
                    })}
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
