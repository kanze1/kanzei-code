import React, { useMemo, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import type { ChatMessage, MessagePart } from "../../ipc/types";
import { useAppStore } from "../../store/appStore";
import { performConversationAction, sendPrompt } from "../../store/chatStore";
import { openFileInDrawer } from "../../store/drawerStore";
import { confirmDialog, openViewer, pushToast } from "../../store/uiStore";
import {
  IconChevronRight,
  IconCopy,
  IconFileCode,
  IconRefresh,
  IconTerminal,
} from "../../components/Icons";
import { MarkdownView } from "../../components/MarkdownView";

interface MessageItemProps {
  message: ChatMessage;
  globalIndex: number;
  toolResultMap: Map<string, MessagePart>;
  occurrenceFromEnd?: number;
  precedingUserText?: string | null;
}

const READONLY_EXPLORE_TOOLS = new Set([
  "read",
  "glob",
  "grep",
  "list_dir",
  "tool_search",
  "memory_search",
  "webfetch",
  "websearch",
]);

const TOOL_IMAGE_RE = /(\.kanzei\/artifacts\/tool-images\/[0-9a-f]{64}\.png)/gi;

function extractText(parts: MessagePart[]): string {
  return parts
    .filter((p) => p.type === "text" && typeof p.text === "string")
    .map((p) => p.text ?? "")
    .join("\n");
}

function getInlineDiffLines(
  inputObj: Record<string, unknown>
): { added: number; removed: number; lines: Array<{ kind: "del" | "add"; text: string }> } | null {
  const oldStr =
    typeof inputObj.old_string === "string"
      ? inputObj.old_string
      : typeof inputObj.old_text === "string"
      ? inputObj.old_text
      : typeof inputObj.target === "string"
      ? inputObj.target
      : null;
  const newStr =
    typeof inputObj.new_string === "string"
      ? inputObj.new_string
      : typeof inputObj.new_text === "string"
      ? inputObj.new_text
      : typeof inputObj.replacement === "string"
      ? inputObj.replacement
      : typeof inputObj.content === "string"
      ? inputObj.content
      : null;
  if (oldStr === null && newStr === null) return null;
  const oldLines = oldStr ? oldStr.split("\n") : [];
  const newLines = newStr ? newStr.split("\n") : [];
  const lines: Array<{ kind: "del" | "add"; text: string }> = [
    ...oldLines.slice(0, 60).map((text) => ({ kind: "del" as const, text })),
    ...newLines.slice(0, 60).map((text) => ({ kind: "add" as const, text })),
  ];
  return { added: newLines.length, removed: oldLines.length, lines };
}

function ToolCallCard({
  part,
  result,
}: {
  part: MessagePart;
  result?: MessagePart;
}): React.ReactElement {
  const [expanded, setExpanded] = useState(false);
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const name = part.name ?? "tool";
  const inputObj = (part.input ?? {}) as Record<string, unknown>;
  const filePath =
    (typeof inputObj.path === "string" && inputObj.path) ||
    (typeof inputObj.file_path === "string" && inputObj.file_path) ||
    "";
  const commandText = typeof inputObj.command === "string" ? inputObj.command : "";
  const summaryText = filePath || commandText || (typeof inputObj.pattern === "string" ? inputObj.pattern : "");
  const isError = Boolean(result?.is_error);
  const isEditLike = name === "edit" || name === "write" || name === "multi_edit";
  const isBash = name === "bash";
  const inlineDiff = useMemo(
    () => (isEditLike ? getInlineDiffLines(inputObj) : null),
    [isEditLike, inputObj]
  );

  const resultContent =
    typeof result?.content === "string"
      ? result.content
      : result
      ? JSON.stringify(result, null, 2)
      : "";
  const toolImages = useMemo(
    () => Array.from(new Set(resultContent.match(TOOL_IMAGE_RE) ?? [])),
    [resultContent]
  );

  const openToolImage = async (rel: string) => {
    if (!projectDir) return;
    try {
      const res = await invoke<{ png?: string }>("tool_image", { projectDir, rel });
      if (res?.png) {
        openViewer({
          title: "工具截图",
          subtitle: rel,
          content: "",
          kind: "image",
          imageSrc: `data:image/png;base64,${res.png}`,
        });
      }
    } catch (err) {
      pushToast(`加载截图失败：${formatError(err)}`, "error");
    }
  };

  return (
    <div className={`tool-card${isBash ? " tool-card-bash" : ""}`}>
      <button
        type="button"
        className="tool-card-head"
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
      >
        <span className={`tool-chevron${expanded ? " open" : ""}`}>
          <IconChevronRight size={12} />
        </span>
        <span className={`badge ${isError ? "badge-err" : "badge-ok"}`}>
          {isError ? "失败" : "完成"}
        </span>
        {isBash && <IconTerminal size={12} />}
        <span className="tool-name">{name}</span>
        <span className="tool-summary mono">{summaryText}</span>
        {inlineDiff && (
          <span className="mono" style={{ fontSize: 11, flex: "none" }}>
            {inlineDiff.added > 0 && (
              <span style={{ color: "var(--diff-add)", marginRight: 4 }}>+{inlineDiff.added}</span>
            )}
            {inlineDiff.removed > 0 && (
              <span style={{ color: "var(--diff-del)" }}>-{inlineDiff.removed}</span>
            )}
          </span>
        )}
        {toolImages.length > 0 && (
          <span
            role="button"
            tabIndex={0}
            className="md-file-link"
            style={{ fontSize: 11 }}
            onClick={(e) => {
              e.stopPropagation();
              void openToolImage(toolImages[0]);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                e.stopPropagation();
                void openToolImage(toolImages[0]);
              }
            }}
          >
            查看截图 ({toolImages.length})
          </span>
        )}
        {filePath && (
          <span
            role="button"
            tabIndex={0}
            className="md-file-link"
            style={{ fontSize: 11 }}
            onClick={(e) => {
              e.stopPropagation();
              void openFileInDrawer(filePath, { tab: isEditLike ? "diff" : "preview" });
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                e.stopPropagation();
                void openFileInDrawer(filePath, { tab: isEditLike ? "diff" : "preview" });
              }
            }}
          >
            <IconFileCode size={11} />
            <span>{isEditLike ? "查看 Diff" : "打开文件"}</span>
          </span>
        )}
      </button>
      {expanded && (
        <div className="tool-body">
          {inlineDiff ? (
            <div>
              <div className="field-label" style={{ marginBottom: 4 }}>
                内联变更预览
              </div>
              <div className="inline-diff-box mono">
                {inlineDiff.lines.map((line, idx) => (
                  <div
                    key={idx}
                    className={`diff-line ${line.kind === "add" ? "diff-line-add" : "diff-line-del"}`}
                  >
                    {line.kind === "add" ? "+" : "-"} {line.text}
                  </div>
                ))}
              </div>
            </div>
          ) : isBash ? (
            <div>
              <div className="field-label" style={{ marginBottom: 4 }}>
                命令
              </div>
              <pre className="tool-pre">$ {commandText || JSON.stringify(inputObj)}</pre>
            </div>
          ) : (
            <div>
              <div className="field-label" style={{ marginBottom: 4 }}>
                入参
              </div>
              <pre className="tool-pre">{JSON.stringify(inputObj, null, 2)}</pre>
            </div>
          )}
          {result && (
            <div>
              <div className="field-label" style={{ marginBottom: 4 }}>
                结果
              </div>
              <pre className="tool-pre">{resultContent.slice(0, 8000)}</pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function ToolCallList({
  toolCalls,
  toolResultMap,
}: {
  toolCalls: MessagePart[];
  toolResultMap: Map<string, MessagePart>;
}): React.ReactElement {
  const [expandedGroups, setExpandedGroups] = useState<Record<number, boolean>>({});

  // 将连续 >= 2 个只读探索工具折叠成一个分组，减少长探索链对正文的干扰
  const segments = useMemo(() => {
    const out: Array<
      | { kind: "single"; call: MessagePart; idx: number }
      | { kind: "explore-group"; calls: Array<{ call: MessagePart; idx: number }> }
    > = [];
    let buf: Array<{ call: MessagePart; idx: number }> = [];
    const flush = () => {
      if (buf.length >= 2) {
        out.push({ kind: "explore-group", calls: buf });
      } else {
        for (const item of buf) out.push({ kind: "single", ...item });
      }
      buf = [];
    };
    toolCalls.forEach((call, idx) => {
      if (READONLY_EXPLORE_TOOLS.has(call.name ?? "")) {
        buf.push({ call, idx });
      } else {
        flush();
        out.push({ kind: "single", call, idx });
      }
    });
    flush();
    return out;
  }, [toolCalls]);

  return (
    <>
      {segments.map((seg, sIdx) => {
        if (seg.kind === "single") {
          return (
            <ToolCallCard
              key={seg.call.id ?? seg.idx}
              part={seg.call}
              result={seg.call.id ? toolResultMap.get(seg.call.id) : undefined}
            />
          );
        }
        const open = Boolean(expandedGroups[sIdx]);
        const counts = new Map<string, number>();
        for (const item of seg.calls) {
          const n = item.call.name ?? "read";
          counts.set(n, (counts.get(n) ?? 0) + 1);
        }
        const summary = Array.from(counts.entries())
          .map(([n, c]) => `${n} ×${c}`)
          .join(" · ");
        return (
          <div key={`grp-${sIdx}`} className="tool-group-card">
            <button
              type="button"
              className="tool-card-head"
              onClick={() =>
                setExpandedGroups((prev) => ({ ...prev, [sIdx]: !prev[sIdx] }))
              }
              aria-expanded={open}
            >
              <span className={`tool-chevron${open ? " open" : ""}`}>
                <IconChevronRight size={12} />
              </span>
              <span className="badge badge-info">探索 {seg.calls.length} 步</span>
              <span className="tool-summary mono">{summary}</span>
            </button>
            {open && (
              <div className="tool-group-body">
                {seg.calls.map(({ call, idx }) => (
                  <ToolCallCard
                    key={call.id ?? idx}
                    part={call}
                    result={call.id ? toolResultMap.get(call.id) : undefined}
                  />
                ))}
              </div>
            )}
          </div>
        );
      })}
    </>
  );
}

export const MessageItem = React.memo(function MessageItem({
  message,
  toolResultMap,
  occurrenceFromEnd = 0,
  precedingUserText = null,
}: MessageItemProps): React.ReactElement | null {
  if (message.role === "tool") return null;

  const text = extractText(message.parts ?? []);
  const images = (message.parts ?? []).filter(
    (p) => p.type === "image" && typeof p.data === "string"
  );
  const toolCalls = (message.parts ?? []).filter((p) => p.type === "tool_use");

  if (message.role === "user") {
    const hasOnlyToolResults =
      !text.trim() &&
      images.length === 0 &&
      (message.parts ?? []).every((p) => p.type === "tool_result");
    if (hasOnlyToolResults) return null;

    const handleFork = async () => {
      const ok = await confirmDialog({
        title: "从此消息分叉新对话？",
        message: "将以截至此消息的历史创建一条新的独立对话分支并自动切换。",
        confirmLabel: "分叉",
      });
      if (ok) await performConversationAction("fork", text, occurrenceFromEnd);
    };

    const handleEditResend = async () => {
      const ok = await confirmDialog({
        title: "编辑并重新发送？",
        message: "将回退此轮对话并把原指令填回输入框，你可以修改后重新发送（不会修改代码文件）。",
        confirmLabel: "编辑重发",
      });
      if (ok) await performConversationAction("edit", text, occurrenceFromEnd);
    };

    return (
      <div className="msg-row">
        <div className="msg-user-wrap">
          <div className="msg-user-bubble">
            {text}
            {images.length > 0 && (
              <div className="msg-user-images">
                {images.map((img, idx) => {
                  const src = `data:${img.media_type ?? "image/png"};base64,${img.data}`;
                  return (
                    <img
                      key={idx}
                      src={src}
                      alt="附件图片"
                      className="msg-user-thumb"
                      onClick={() =>
                        openViewer({
                          title: String(img.file_name ?? "图片附件"),
                          content: "",
                          kind: "image",
                          imageSrc: src,
                        })
                      }
                    />
                  );
                })}
              </div>
            )}
          </div>
          <div className="msg-actions">
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => {
                navigator.clipboard?.writeText(text).then(() => pushToast("已复制", "ok", 1500));
              }}
            >
              <IconCopy size={11} />
              <span>复制</span>
            </button>
            {text.trim() && (
              <>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => void handleEditResend()}
                >
                  编辑重发
                </button>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => void performConversationAction("rewind", text, occurrenceFromEnd)}
                >
                  检查点回退
                </button>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => void handleFork()}
                >
                  分叉
                </button>
              </>
            )}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="msg-row">
      <div className="msg-assistant-wrap">
        <div className="msg-assistant-head">
          <img src="./assets/kanzei.svg" alt="" className="msg-assistant-avatar" />
          <span>kanzei</span>
        </div>

        {message.reasoning && (
          <details className="reasoning-block">
            <summary className="reasoning-summary">
              <span className="reasoning-chevron">
                <IconChevronRight size={12} />
              </span>
              <span>思考过程</span>
              <span className="field-hint mono" style={{ marginLeft: "auto", fontSize: 10.5 }}>
                {message.reasoning.length} 字
              </span>
            </summary>
            <div className="reasoning-body">{message.reasoning}</div>
          </details>
        )}

        {toolCalls.length > 0 && (
          <ToolCallList toolCalls={toolCalls} toolResultMap={toolResultMap} />
        )}

        {text.trim() && <MarkdownView text={text} />}

        <div className="msg-meta-row">
          <div className="msg-actions">
            {text.trim() && (
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => {
                  navigator.clipboard
                    ?.writeText(text)
                    .then(() => pushToast("已复制回答", "ok", 1500));
                }}
              >
                <IconCopy size={11} />
                <span>复制回答</span>
              </button>
            )}
            {precedingUserText && (
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => void sendPrompt(precedingUserText)}
                title="使用同一条用户指令重新生成回复"
              >
                <IconRefresh size={11} />
                <span>重新生成</span>
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
});
