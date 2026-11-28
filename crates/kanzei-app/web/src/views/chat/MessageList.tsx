import React, { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { ChatMessage, MessagePart } from "../../ipc/types";
import { useAppStore } from "../../store/appStore";
import { chatStore, loadOlderMessages, updateDraft, useChatStore } from "../../store/chatStore";
import { toggleDrawer } from "../../store/drawerStore";
import { useSessionStore } from "../../store/sessionStore";
import {
  IconFileCode,
  IconSearch,
  IconSparkles,
  IconTerminal,
} from "../../components/Icons";
import { MarkdownView } from "../../components/MarkdownView";
import { ActionReportCard } from "./ActionReportCard";
import { ActivityTimeline, OutputPreview } from "./ActivityTimeline";
import { MessageItem } from "./MessageItem";

export function MessageList(): React.ReactElement {
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
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
  const lastActionReport = useChatStore((s) => s.lastActionReport);
  const lastError = useChatStore((s) => s.lastError);
  const searchOpen = useChatStore((s) => s.searchOpen);
  const searchQuery = useChatStore((s) => s.searchQuery);
  const activeProcess = useSessionStore((s) => s.processes.find((p) => p.id === s.activeProcessId) ?? null);
  const running = Boolean(activeProcess?.running);

  const parentRef = useRef<HTMLDivElement>(null);
  const [pinnedBottom, setPinnedBottom] = useState(true);

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

  // Project tool-only assistant messages onto the same turn, preserving persisted indices for user actions.
  const historyRows = useMemo(() => {
    const rows: Array<{ message: ChatMessage; index: number }> = [];
    let pending: ChatMessage[] = [];
    let firstIndex = 0;
    function flush() {
      if (!pending.length) return;
      rows.push({
        index: firstIndex,
        message: {
          role: "assistant",
          parts: pending.flatMap((msg) => msg.parts),
          reasoning: pending
            .map((msg) => msg.reasoning)
            .filter(Boolean)
            .join("\n\n"),
        },
      });
      pending = [];
    }
    messages.forEach((message, index) => {
      if (
        message.role === "tool" ||
        (message.role === "user" && message.parts.every((part) => part.type === "tool_result"))
      )
        return;
      if (message.role === "assistant") {
        if (!pending.length) firstIndex = index;
        pending.push(message);
        if (message.parts.some((part) => part.type === "text" && part.text?.trim())) flush();
      } else {
        flush();
        rows.push({ message, index });
      }
    });
    flush();
    return rows;
  }, [messages]);

  const visibleEntries = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    const list: Array<{
      index: number;
      globalIndex: number;
      occurrenceFromEnd: number;
      precedingUserText: string | null;
    }> = [];
    let lastUserText: string | null = null;
    const rawItems: Array<{
      index: number;
      globalIndex: number;
      role: string;
      text: string;
      precedingUserText: string | null;
    }> = [];
    historyRows.forEach(({ message: msg }, idx) => {
      if (msg.role === "tool") return;
      const textParts = (msg.parts ?? [])
        .filter((p) => p.type === "text" && typeof p.text === "string")
        .map((p) => p.text ?? "")
        .join("\n");
      const hasContent =
        Boolean(msg.reasoning) ||
        textParts.trim().length > 0 ||
        (msg.parts ?? []).some((p) => p.type === "tool_use" || p.type === "image");
      if (!hasContent) return;
      if (msg.role === "user") {
        lastUserText = textParts.trim() || lastUserText;
      }
      if (q && !textParts.toLowerCase().includes(q)) return;
      rawItems.push({
        index: idx,
        globalIndex: startIndex + historyRows[idx].index,
        role: msg.role,
        text: textParts,
        precedingUserText: msg.role === "assistant" ? lastUserText : null,
      });
    });
    // 从后往前统计同文本用户消息的倒序出现次序，对齐后端 rewind_plan(occurrence_from_end)
    const seenFromEnd = new Map<string, number>();
    for (let i = rawItems.length - 1; i >= 0; i -= 1) {
      const item = rawItems[i];
      let occ = 0;
      if (item.role === "user") {
        occ = seenFromEnd.get(item.text) ?? 0;
        seenFromEnd.set(item.text, occ + 1);
      }
      list[i] = {
        index: item.index,
        globalIndex: item.globalIndex,
        occurrenceFromEnd: occ,
        precedingUserText: item.precedingUserText,
      };
    }
    return list;
  }, [historyRows, startIndex, searchQuery]);

  // 提取实时步骤清单（支持结构化 todos/steps 工具参数或实时工具/子代理执行汇总）
  const liveChecklist = useMemo(() => {
    const todoTool = [...liveTools].reverse().find((t) => {
      const inp = t.input as Record<string, unknown> | undefined;
      return inp && (Array.isArray(inp.todos) || Array.isArray(inp.steps));
    });
    if (todoTool) {
      const inp = todoTool.input as Record<string, unknown>;
      const rawList = (Array.isArray(inp.todos) ? inp.todos : inp.steps) as Array<Record<string, unknown>>;
      return rawList
        .map((item, idx) => ({
          id: String(item.id ?? idx),
          title: String(item.title ?? item.content ?? item.text ?? `步骤 ${idx + 1}`),
          status: String(item.status ?? "pending"),
        }))
        .filter((x) => x.title);
    }
    return null;
  }, [liveTools]);

  const activeTool = liveTools.find((tool) => !tool.done);
  const activeAgent = liveSubagents.find((agent) => agent.status === "running");
  const failedTool = liveTools.find((tool) => tool.done && tool.ok === false);
  const failedAgent = liveSubagents.find((agent) => agent.status === "error");
  const issue = failedTool
    ? [failedTool.name, (failedTool.output || failedTool.preview || "").split("\n")[0]]
        .filter(Boolean)
        .join(" · ")
    : failedAgent?.label;
  const currentStep = liveChecklist?.find((step) => step.status === "in_progress");
  const completedSteps =
    liveChecklist?.filter((step) => step.status === "done" || step.status === "completed").length ?? 0;

  const virtualizer = useVirtualizer({
    count: visibleEntries.length,
    getScrollElement: () => parentRef.current,
    getItemKey: (index) => visibleEntries[index].globalIndex,
    estimateSize: () => 118,
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

  const fillSuggestion = (prompt: string) => {
    updateDraft(activeProjectDir, activeProcess?.id ?? null, prompt, []);
    const input = document.getElementById("kz-composer-input") as HTMLTextAreaElement | null;
    if (input) {
      input.focus();
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
          <IconSearch size={13} />
          <input
            autoFocus
            className="input"
            placeholder="在已加载对话中搜索…"
            value={searchQuery}
            onChange={(e) => chatStore.setState({ searchQuery: e.target.value })}
          />
          <span className="field-hint">匹配 {visibleEntries.length} 条</span>
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
                  : `加载更早消息（当前 ${messages.length} / 共 ${total} 条）`}
              </button>
            </div>
          )}

          {loading ? (
            <div className="empty-state">
              <p>正在加载对话…</p>
            </div>
          ) : visibleEntries.length === 0 && !hasLiveContent ? (
            <div className="chat-welcome">
              <div className="chat-welcome-mark">
                <img src="./assets/kanzei.svg" alt="kanzei" />
              </div>
              <h3>开始一段新对话</h3>
              <p className="chat-welcome-desc">
                直接描述你的工程目标、粘贴截图或拖入文件。代码改动与引用路径将自动汇聚到右侧「文件上下文」抽屉。
              </p>

              <div className="chat-welcome-cards">
                <button
                  type="button"
                  className="chat-welcome-card"
                  onClick={() => fillSuggestion("梳理当前项目的模块结构、核心入口与近期改动。")}
                >
                  <span className="chat-welcome-card-title">
                    <IconSparkles size={13} />
                    <span>梳理项目结构</span>
                  </span>
                  <span className="chat-welcome-card-desc">快速理解目录边界、关键入口与模块协作关系</span>
                </button>
                <button
                  type="button"
                  className="chat-welcome-card"
                  onClick={() =>
                    generalMode
                      ? fillSuggestion("帮我拆解这个任务的实现步骤与边界条件：")
                      : toggleDrawer("files")
                  }
                >
                  <span className="chat-welcome-card-title">
                    <IconFileCode size={13} />
                    <span>{generalMode ? "拆解复杂任务" : "查看文件与 Diff"}</span>
                  </span>
                  <span className="chat-welcome-card-desc">
                    {generalMode
                      ? "明确方案权衡、风险与可执行步骤"
                      : "打开右侧伴生抽屉，就地对照工作树改动并轻编辑"}
                  </span>
                </button>
                <button
                  type="button"
                  className="chat-welcome-card"
                  onClick={() => fillSuggestion("检查当前工作树变更是否存在边界漏洞或测试缺口。")}
                >
                  <span className="chat-welcome-card-title">
                    <IconTerminal size={13} />
                    <span>自查变更质量</span>
                  </span>
                  <span className="chat-welcome-card-desc">聚焦潜在缺陷、未覆盖分支与本地验证门禁</span>
                </button>
              </div>

              <div className="chat-welcome-hints">
                <span className="chat-welcome-hint-item">
                  <kbd className="kbd">Ctrl+K</kbd> 聚焦输入
                </span>
                <span className="chat-welcome-hint-item">
                  <kbd className="kbd">Ctrl+P</kbd> 命令面板
                </span>
                <span className="chat-welcome-hint-item">
                  <kbd className="kbd">Ctrl+F</kbd> 搜索对话
                </span>
              </div>
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
                const msg = historyRows[entry.index].message;
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
                      occurrenceFromEnd={entry.occurrenceFromEnd}
                      precedingUserText={entry.precedingUserText}
                    />
                  </div>
                );
              })}
            </div>
          )}

          {hasLiveContent && (
            <div className="msg-row">
              <div className="msg-assistant-wrap">
                {(liveTools.length > 0 || liveSubagents.length > 0 || streamingReasoning) && (
                  <ActivityTimeline
                    summary={
                      currentStep && liveChecklist
                        ? currentStep.title + "（已完成 " + completedSteps + "/" + liveChecklist.length + "）"
                        : activeTool
                          ? `正在执行 ${activeTool.name} ${activeTool.summary ?? ""}`
                          : activeAgent
                            ? `子代理运行中 ${activeAgent.agent ?? activeAgent.label}`
                            : streamingReasoning && !streamingText
                              ? "正在思考…"
                              : `执行过程（${liveTools.length} 次工具调用）`
                    }
                    issue={issue}
                    failures={
                      liveTools.filter((t) => t.done && t.ok === false).length +
                      liveSubagents.filter((s) => s.status === "error").length
                    }
                  >
                    {liveChecklist && (
                      <ol>
                        {liveChecklist.map((step) => (
                          <li key={step.id}>
                            {step.title} <small>{step.status}</small>
                          </li>
                        ))}
                      </ol>
                    )}
                    {streamingReasoning && (
                      <details className="reasoning-block">
                        <summary>思考过程（{streamingReasoning.length} 字）</summary>
                        <div className="reasoning-body">{streamingReasoning}</div>
                      </details>
                    )}
                    {liveTools.map((tool) => (
                      <details key={tool.id}>
                        <summary>
                          <span className={tool.ok === false && tool.done ? "error-text" : ""}>
                            {tool.name}
                          </span>{" "}
                          {tool.summary}
                          <small>{!tool.done ? "执行中" : tool.ok ? "完成" : "失败"}</small>
                        </summary>
                        <OutputPreview text={tool.output ?? tool.preview ?? ""} tail />
                      </details>
                    ))}
                    {liveSubagents.map((sub) => (
                      <details key={sub.id}>
                        <summary>
                          {sub.agent ?? sub.label}{" "}
                          <small>
                            {sub.status === "running" ? "运行中" : sub.status === "error" ? "失败" : "完成"}
                          </small>
                        </summary>
                        <OutputPreview text={sub.progress.join("\n")} tail />
                      </details>
                    ))}
                    <button className="btn btn-ghost btn-sm" onClick={() => toggleDrawer("activity")}>
                      活动面板
                    </button>
                  </ActivityTimeline>
                )}
                {streamingReasoning && !streamingText && (
                  <div className="thinking-preview">
                    {streamingReasoning.split("\n").slice(-4).join("\n")}
                  </div>
                )}
                {streamingText && <MarkdownView text={streamingText} streaming />}
              </div>
            </div>
          )}

          {lastError && <div className="chat-error-banner">{lastError}</div>}

          {!running && visibleEntries.length > 0 && (
            <ActionReportCard
              key={`${activeProcess?.session_id}:${lastActionReport?.completedAt ?? total}`}
              messages={messages}
              toolResultMap={toolResultMap}
              report={
                lastActionReport && lastActionReport.sessionId === activeProcess?.session_id
                  ? lastActionReport
                  : null
              }
            />
          )}
        </div>

        {!pinnedBottom && (
          <button
            type="button"
            className="btn btn-primary btn-sm btn-pill jump-bottom-btn"
            onClick={scrollToBottom}
          >
            回到最新 ↓
          </button>
        )}
      </div>
    </>
  );
}
