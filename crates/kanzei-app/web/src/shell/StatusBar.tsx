import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { formatError, invoke } from "../ipc/client";
import { useAppStore } from "../store/appStore";
import {
  clearCurrentConversation,
  compactCurrentConversation,
  useChatStore,
} from "../store/chatStore";
import { getActiveProcess, useSessionStore } from "../store/sessionStore";
import { openViewer, pushToast, uiStore, useUiStore } from "../store/uiStore";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { IconClose, IconLayers, IconTerminal } from "../components/Icons";

interface ContextBlockItem {
  position?: number;
  kind?: string;
  role?: string;
  label?: string;
  sha256?: string;
  chars?: number;
  bytes?: number;
  inspectable?: boolean;
  source?: unknown;
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
  items: Array<{block: ContextBlockItem; index: number}>;
}

export function formatContextBlockTitle(block: ContextBlockItem, index: number): string {
  if (typeof block.label === "string" && block.label.trim()) {
    return block.label.trim();
  }
  if (typeof block.source === "string" && block.source.trim()) {
    return block.source.trim();
  }
  const fallback = [block.kind, block.role].filter(
    (part): part is string => typeof part === "string" && part.trim().length > 0
  );
  return fallback.length > 0 ? fallback.join("/") : `块 #${index + 1}`;
}

export function formatContextBlockSource(source: unknown): string | null {
  if (source === null || source === undefined) return null;
  if (typeof source === "string") {
    const trimmed = source.trim();
    return trimmed || null;
  }
  if (typeof source === "number" || typeof source === "boolean") {
    return String(source);
  }
  if (typeof source !== "object" || Array.isArray(source)) {
    try {
      return JSON.stringify(source);
    } catch {
      return null;
    }
  }
  const obj = source as Record<string, unknown>;
  const parts: string[] = [];
  if (typeof obj.origin === "string" && obj.origin) parts.push(`origin=${obj.origin}`);
  if (typeof obj.scope === "string" && obj.scope) parts.push(`scope=${obj.scope}`);
  if (typeof obj.tool === "string" && obj.tool) parts.push(`tool=${obj.tool}`);
  if (typeof obj.is_error === "boolean" && obj.is_error) parts.push("error");
  if (typeof obj.system_index === "number") {
    const seg = [`system[${obj.system_index}]`];
    if (typeof obj.offset_bytes === "number" && typeof obj.length_bytes === "number") {
      seg.push(`${obj.offset_bytes}+${obj.length_bytes}B`);
    }
    parts.push(seg.join(" "));
  }
  if (parts.length > 0) return parts.join(" · ");
  try {
    const raw = JSON.stringify(obj);
    return raw && raw !== "{}" ? raw : null;
  } catch {
    return null;
  }
}

const contextCategories = {
  memory: {key:"memory", label:"长期记忆与偏好", color:"#a855f7"},
  skill: {key:"skill", label:"技能规范", color:"#ec4899"},
  tools: {key:"tools", label:"工具与结果", color:"#f59e0b"},
  system: {key:"system", label:"系统提示与规则", color:"#3b82f6"},
  web: {key:"web", label:"网页与附件", color:"#14b8a6"},
  history: {key:"history", label:"对话历史与消息", color:"#22c55e"},
};

function classifyContextBlock(block: ContextBlockItem): { key: string; label: string; color: string } {
  const sourceHint = formatContextBlockSource(block.source) ?? "";
  const src = `${block.label ?? ""} ${block.kind ?? ""} ${block.role ?? ""} ${sourceHint}`.toLowerCase();
  if (src.includes("memory") || src.includes("recall") || src.includes("preference")) {
    return contextCategories.memory;
  }
  if (src.includes("skill")) {
    return contextCategories.skill;
  }
  if (
    block.kind === "tool_call" ||
    block.kind === "tool_result" ||
    block.kind === "tool_schema" ||
    src.includes("tool") ||
    src.includes("schema")
  ) {
    return contextCategories.tools;
  }
  if (
    block.kind === "system" ||
    block.kind === "runtime_context" ||
    src.includes("system") ||
    src.includes("agent") ||
    src.includes("instruction")
  ) {
    return contextCategories.system;
  }
  if (
    block.kind === "attachment" ||
    src.includes("web") ||
    src.includes("attach") ||
    src.includes("image")
  ) {
    return contextCategories.web;
  }
  return contextCategories.history;
}

export function StatusBar(): React.ReactElement {
  const appInfo = useAppStore((s) => s.appInfo);
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const stageText = useChatStore((s) => s.stageText);
  const lastUsage = useChatStore((s) => s.lastUsage);
  const autoAllow = useChatStore((s) => s.autoAllow);
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
  const lastHandledNonceRef = useRef(contextInspectorNonce);
  const activeProcessId = activeProcess?.id ?? null;

  const openContextInspector = useCallback(
    async (sequence?: number | null) => {
      const proc = activeProcess ?? getActiveProcess();
      if (!projectDir || !proc) {
        pushToast("当前没有可检查上下文的会话", "info");
        return;
      }
      const targetProjectDir = projectDir;
      const targetProcessId = proc.id;
      try {
        const snap = await invoke<ContextSnapshotResponse>("context_snapshot", {
          projectDir: targetProjectDir,
          processId: targetProcessId,
          sequence: sequence ?? null,
        });
        if (getActiveProcess()?.id !== targetProcessId) return;
        setCtxSnapshot(snap);
        setCtxOpen(true);
      } catch (err) {
        pushToast(`读取上下文构成失败：${formatError(err)}`, "error");
      }
    },
    [activeProcess, projectDir]
  );

  useEffect(() => {
    setCtxOpen(false);
    setCtxSnapshot(null);
  }, [projectDir, activeProcessId]);

  useEffect(() => {
    if (contextInspectorNonce > lastHandledNonceRef.current) {
      lastHandledNonceRef.current = contextInspectorNonce;
      void openContextInspector();
    }
  }, [contextInspectorNonce, openContextInspector]);

  const breakdown = useMemo<BreakdownCategory[]>(() => {
    const blocks = ctxSnapshot?.selected?.manifest?.blocks ?? [];
    const map = new Map<string, BreakdownCategory>(Object.values(contextCategories).map(cat =>
      [cat.key, {...cat, chars: 0, items: []}]));
    for (const [index, block] of blocks.entries()) {
      const cat = map.get(classifyContextBlock(block).key)!;
      cat.chars += Math.max(0, Number(block.chars ?? 0));
      cat.items.push({block, index});
    }
    return Array.from(map.values()).sort((a, b) => b.chars - a.chars);
  }, [ctxSnapshot]);

  const totalBlockChars = useMemo(
    () => breakdown.reduce((acc, item) => acc + item.chars, 0),
    [breakdown]
  );

  const inspectBlock = async (sequence: number, block: ContextBlockItem, index: number) => {
    const proc = activeProcess ?? getActiveProcess();
    if (!projectDir || !proc || !block.sha256 || block.inspectable === false) return;
    try {
      const text = await invoke<string>("context_content", {
        projectDir,
        processId: proc.id,
        sequence,
        sha256: block.sha256,
      });
      openViewer({
        title: formatContextBlockTitle(block, index),
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
          {autoAllow && (
            <span className="badge badge-warn" title="常规非敏感工具权限将自动放行">
              自动放行
            </span>
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
              <ErrorBoundary
                scope="上下文构成弹窗"
                resetKeys={[ctxOpen, ctxSnapshot?.selected?.sequence, activeProcessId]}
                onReset={() => setCtxOpen(false)}
                resetLabel="关闭弹窗"
              >
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
                                ((ctxSnapshot.selected.manifest.estimated_tokens ?? 0) / contextLimit) * 100,
                              ),
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
                          {breakdown
                            .filter((cat) => cat.chars > 0)
                            .map((cat) => {
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
                      </div>
                    )}

                    <div className="ctx-block-list">
                      {breakdown.map((cat) =>
                        cat.items.length === 0 ? (
                          <div className="context-empty-group" key={cat.key}>
                            <span className="ctx-legend-dot" style={{ background: cat.color }} />
                            {cat.label} <small>0 项</small>
                          </div>
                        ) : (
                          <details key={ctxSnapshot.selected?.sequence + cat.key} className="context-group">
                            <summary>
                              <span className="ctx-legend-dot" style={{ background: cat.color }} />
                              <strong>{cat.label}</strong>
                              <span>{cat.items.length} 项</span>
                              <small>
                                {cat.chars.toLocaleString()} 字符 ·{" "}
                                {totalBlockChars ? Math.round((cat.chars * 100) / totalBlockChars) : 0}%
                              </small>
                              <meter
                                aria-label={cat.label + "字符占比"}
                                value={cat.chars}
                                max={totalBlockChars || 1}
                              />
                            </summary>
                            {cat.items.map(({ block: b, index: i }) => {
                              const title = formatContextBlockTitle(b, i);
                              const sourceMeta = formatContextBlockSource(b.source);
                              return (
                                <div key={`${b.position ?? i}-${b.sha256 ?? i}`} className="ctx-block-row">
                                  <span
                                    style={{
                                      display: "inline-flex",
                                      alignItems: "center",
                                      gap: 6,
                                      minWidth: 0,
                                    }}
                                  >
                                    <span className="ctx-legend-dot" style={{ background: cat.color }} />
                                    <span className="mono" title={sourceMeta ?? undefined}>
                                      {title}
                                    </span>
                                  </span>
                                  <div className="ctx-block-right">
                                    <span className="field-hint">{(b.chars ?? 0).toLocaleString()} 字符</span>
                                    {b.sha256 && b.inspectable !== false && (
                                      <button
                                        type="button"
                                        className="btn btn-sm"
                                        onClick={() =>
                                          void inspectBlock(ctxSnapshot.selected!.sequence, b, i)
                                        }
                                      >
                                        查看原文
                                      </button>
                                    )}
                                  </div>
                                </div>
                              );
                            })}
                          </details>
                        ),
                      )}
                    </div>
                  </>
                )}
              </ErrorBoundary>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
