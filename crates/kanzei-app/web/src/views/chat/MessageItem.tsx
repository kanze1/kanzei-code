import React, { useState } from "react";
import type { ChatMessage, MessagePart } from "../../ipc/types";
import { performConversationAction } from "../../store/chatStore";
import { openFileInDrawer } from "../../store/drawerStore";
import { confirmDialog, openViewer, pushToast } from "../../store/uiStore";
import { IconChevronRight, IconCopy, IconFileCode } from "../../components/Icons";
import { MarkdownView } from "../../components/MarkdownView";

interface MessageItemProps {
  message: ChatMessage;
  globalIndex: number;
  toolResultMap: Map<string, MessagePart>;
}

function extractText(parts: MessagePart[]): string {
  return parts
    .filter((p) => p.type === "text" && typeof p.text === "string")
    .map((p) => p.text ?? "")
    .join("\n");
}

function ToolCallCard({
  part,
  result,
}: {
  part: MessagePart;
  result?: MessagePart;
}): React.ReactElement {
  const [expanded, setExpanded] = useState(false);
  const name = part.name ?? "tool";
  const inputObj = (part.input ?? {}) as Record<string, unknown>;
  const pathCandidate =
    (typeof inputObj.path === "string" && inputObj.path) ||
    (typeof inputObj.file_path === "string" && inputObj.file_path) ||
    (typeof inputObj.command === "string" && inputObj.command) ||
    "";
  const isError = Boolean(result?.is_error);

  return (
    <div className="tool-card">
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
        <span className="tool-name">{name}</span>
        <span className="tool-summary mono">{pathCandidate}</span>
        {typeof inputObj.path === "string" && (
          <span
            role="button"
            tabIndex={0}
            className="md-file-link"
            style={{ fontSize: 11 }}
            onClick={(e) => {
              e.stopPropagation();
              void openFileInDrawer(inputObj.path as string);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                e.stopPropagation();
                void openFileInDrawer(inputObj.path as string);
              }
            }}
          >
            <IconFileCode size={11} />
            <span>打开文件</span>
          </span>
        )}
      </button>
      {expanded && (
        <div className="tool-body">
          <div>
            <div className="field-label" style={{ marginBottom: 4 }}>入参</div>
            <pre className="tool-pre">{JSON.stringify(inputObj, null, 2)}</pre>
          </div>
          {result && (
            <div>
              <div className="field-label" style={{ marginBottom: 4 }}>结果</div>
              <pre className="tool-pre">
                {typeof result.content === "string"
                  ? result.content.slice(0, 8000)
                  : JSON.stringify(result, null, 2)}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export const MessageItem = React.memo(function MessageItem({
  message,
  toolResultMap,
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

    const handleAction = async (action: "rewind" | "fork") => {
      const ok = await confirmDialog({
        title: action === "rewind" ? "回退到此消息？" : "从此消息分叉新对话？",
        message:
          action === "rewind"
            ? "此消息之后的对话轮次将被移出当前上下文。"
            : "将以截至此消息的历史创建一条新的独立对话分支。",
        confirmLabel: action === "rewind" ? "回退" : "分叉",
      });
      if (ok) await performConversationAction(action, text);
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
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => void handleAction("rewind")}
            >
              回退至此
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => void handleAction("fork")}
            >
              分叉
            </button>
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

        {toolCalls.map((call, idx) => (
          <ToolCallCard
            key={call.id ?? idx}
            part={call}
            result={call.id ? toolResultMap.get(call.id) : undefined}
          />
        ))}

        {text.trim() && <MarkdownView text={text} />}

        <div className="msg-meta-row">
          <div className="msg-actions">
            {text.trim() && (
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => {
                  navigator.clipboard?.writeText(text).then(() => pushToast("已复制回答", "ok", 1500));
                }}
              >
                <IconCopy size={11} />
                <span>复制回答</span>
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
});
