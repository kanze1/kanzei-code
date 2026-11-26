import React, { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { MessagePart } from "../../ipc/types";
import { chatStore, loadOlderMessages, useChatStore } from "../../store/chatStore";
import { useSessionStore } from "../../store/sessionStore";
import { MarkdownView } from "../../components/MarkdownView";
import { MessageItem } from "./MessageItem";

export function MessageList(): React.ReactElement {
  const messages = useChatStore((s) => s.messages);
  const startIndex = useChatStore((s) => s.startIndex);
  const total = useChatStore((s) => s.total);
  const hasMore = useChatStore((s) => s.hasMore);
  const loading = useChatStore((s) => s.loading);
  const loadingOlder = useChatStore((s) => s.loadingOlder);
  const streamingText = useChatStore((s) => s.streamingText);
  const streamingReasoning = useChatStore((s) => s.streamingReasoning);
  const liveTools = useChatStore((s) => s.liveTools);
  const liveSubagents = useChatStore((s) => s.liveSubagents);
  const lastError = useChatStore((s) => s.lastError);
  const searchOpen = useChatStore((s) => s.searchOpen);
  const searchQuery = useChatStore((s) => s.searchQuery);
  const activeProcess = useSessionStore((s) =>
    s.processes.find((p) => p.id === s.activeProcessId) ?? null
  );
  const running = Boolean(activeProcess?.running);

  const parentRef = useRef<HTMLDivElement>(null);
  const [pinnedBottom, setPinnedBottom] = useState(true);

  // 构建 tool_use_id -> tool_result 索引，供助手工具调用卡片就地关联结果
  const toolResultMap = useMemo(() => {
    const map = new Map<string, MessagePart>();
    for (const msg of messages) {
      for (const part of msg.parts ?? []) {
        if (part.type === "tool_result" && typeof part.tool_use_id === "string") {
          map.set(part.tool_use_id, part);
        }
      }
    }
    return map;
  }, [messages]);

  const visibleEntries = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    const list: Array<{ index: number; globalIndex: number }> = [];
    messages.forEach((msg, idx) => {
      if (msg.role === "tool") return;
      const textParts = (msg.parts ?? [])
        .filter((p) => p.type === "text" && typeof p.text === "string")
        .map((p) => p.text ?? "")
        .join("\n");
      const hasContent =
        textParts.trim().length > 0 ||
        (msg.parts ?? []).some((p) => p.type === "tool_use" || p.type === "image");
      if (!hasContent) return;
      if (q && !textParts.toLowerCase().includes(q)) return;
      list.push({ index: idx, globalIndex: startIndex + idx });
    });
    return list;
  }, [messages, startIndex, searchQuery]);

  const virtualizer = useVirtualizer({
    count: visibleEntries.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 110,
    overscan: 8,
  });

  const scrollToBottom = () => {
    const el = parentRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    setPinnedBottom(true);
  };

  useEffect(() => {
    if (pinnedBottom) {
      requestAnimationFrame(scrollToBottom);
    }
  }, [visibleEntries.length, streamingText, streamingReasoning, liveTools.length, pinnedBottom]);

  const handleScroll = () => {
    const el = parentRef.current;
    if (!el) return;
    const distanceBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    setPinnedBottom(distanceBottom < 64);
    if (el.scrollTop < 48 && hasMore && !loadingOlder) {
      const prevHeight = el.scrollHeight;
      void loadOlderMessages().then((added) => {
        if (added > 0 && parentRef.current) {
          const delta = parentRef.current.scrollHeight - prevHeight;
          parentRef.current.scrollTop += delta;
        }
      });
    }
  };

  const hasLiveContent =
    running ||
    streamingText.length > 0 ||
    streamingReasoning.length > 0 ||
    liveTools.length > 0 ||
    liveSubagents.length > 0;

  return (
    <>
      {searchOpen && (
        <div className="chat-search-bar">
          <input
            autoFocus
            className="input"
            placeholder="在已加载对话中搜索…"
            value={searchQuery}
            onChange={(e) => chatStore.setState({ searchQuery: e.target.value })}
          />
          <span style={{ color: "var(--dim)" }}>匹配 {visibleEntries.length} 条</span>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => chatStore.setState({ searchOpen: false, searchQuery: "" })}
          >
            关闭
          </button>
        </div>
      )}

      <div ref={parentRef} className="messages-viewport" onScroll={handleScroll}>
        <div className="messages-inner">
          {hasMore && (
            <div className="messages-load-more">
              <button
                type="button"
                className="btn btn-sm"
                disabled={loadingOlder}
                onClick={() => void loadOlderMessages()}
              >
                {loadingOlder
                  ? "正在加载更早消息…"
                  : `加载更早消息（当前显示 ${messages.length} / 共 ${total} 条）`}
              </button>
            </div>
          )}

          {loading ? (
            <div className="empty-state">
              <p>正在加载对话…</p>
            </div>
          ) : visibleEntries.length === 0 && !hasLiveContent ? (
            <div className="empty-state">
              <h3>开始一段新对话</h3>
              <p>
                直接描述你的目标、粘贴截图或拖入文件。代码变更和引用文件路径会自动汇总到右侧「文件上下文」抽屉。
              </p>
            </div>
          ) : (
            <div
              style={{
                height: `${virtualizer.getTotalSize()}px`,
                width: "100%",
                position: "relative",
              }}
            >
              {virtualizer.getVirtualItems().map((vItem) => {
                const entry = visibleEntries[vItem.index];
                if (!entry) return null;
                const msg = messages[entry.index];
                return (
                  <div
                    key={vItem.key}
                    data-index={vItem.index}
                    ref={virtualizer.measureElement}
                    style={{
                      position: "absolute",
                      top: 0,
                      left: 0,
                      width: "100%",
                      transform: `translateY(${vItem.start}px)`,
                    }}
                  >
                    <MessageItem
                      message={msg}
                      globalIndex={entry.globalIndex}
                      toolResultMap={toolResultMap}
                    />
                  </div>
                );
              })}
            </div>
          )}

          {hasLiveContent && (
            <div className="msg-row">
              <div className="msg-assistant-wrap">
                {streamingReasoning && (
                  <details className="reasoning-block" open>
                    <summary className="reasoning-summary">正在思考…</summary>
                    <div className="reasoning-body">{streamingReasoning}</div>
                  </details>
                )}

                {liveSubagents.map((sub) => (
                  <div key={sub.id} className="tool-card">
                    <div className="tool-card-head">
                      <span
                        className={`badge ${
                          sub.status === "running"
                            ? "badge-accent"
                            : sub.status === "error"
                            ? "badge-err"
                            : "badge-ok"
                        }`}
                      >
                        子代理 · {sub.status === "running" ? "运行中" : sub.status === "error" ? "失败" : "完成"}
                      </span>
                      <span className="tool-name">{sub.agent ?? sub.label}</span>
                      {sub.model && <span className="tool-summary mono">{sub.model}</span>}
                    </div>
                    {sub.progress.length > 0 && (
                      <div className="tool-body">
                        <pre className="tool-pre">{sub.progress.slice(-6).join("\n")}</pre>
                      </div>
                    )}
                  </div>
                ))}

                {liveTools.map((tool) => (
                  <div key={tool.id} className="tool-card">
                    <div className="tool-card-head">
                      <span
                        className={`badge ${
                          !tool.done ? "badge-accent" : tool.ok ? "badge-ok" : "badge-err"
                        }`}
                      >
                        {!tool.done ? "执行中" : tool.ok ? "完成" : "失败"}
                      </span>
                      <span className="tool-name">{tool.name}</span>
                      <span className="tool-summary mono">{tool.summary}</span>
                      {typeof tool.durationMs === "number" && (
                        <span style={{ color: "var(--dim)", fontSize: 11 }}>
                          {(tool.durationMs / 1000).toFixed(1)}s
                        </span>
                      )}
                    </div>
                    {(tool.preview || tool.output) && (
                      <div className="tool-body">
                        <pre className="tool-pre">
                          {(tool.output ?? tool.preview ?? "").slice(0, 4000)}
                        </pre>
                      </div>
                    )}
                  </div>
                ))}

                {streamingText && <MarkdownView text={streamingText} />}
              </div>
            </div>
          )}

          {lastError && (
            <div
              style={{
                marginTop: 10,
                padding: "10px 12px",
                borderRadius: 8,
                border: "1px solid var(--err)",
                background: "var(--badge-err)",
                color: "var(--err)",
                fontSize: 12,
              }}
            >
              {lastError}
            </div>
          )}
        </div>

        {!pinnedBottom && (
          <button
            type="button"
            className="btn btn-primary btn-sm jump-bottom-btn"
            onClick={scrollToBottom}
          >
            回到最新 ↓
          </button>
        )}
      </div>
    </>
  );
}
