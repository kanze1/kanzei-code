import React, { useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import type { PendingAskItem } from "../../ipc/types";
import { getActiveSessionId, removePendingAsk, useSessionStore } from "../../store/sessionStore";
import { pushToast } from "../../store/uiStore";
import { IconShield, IconSparkles } from "../../components/Icons";

export function AskOverlay(): React.ReactElement | null {
  const asks = useSessionStore((s) => s.asks);
  const activeSessionId = getActiveSessionId();
  const [answerText, setAnswerText] = useState("");
  const [busy, setBusy] = useState(false);

  const visibleAsks = asks.filter(
    (a) => !activeSessionId || !a.sessionId || a.sessionId === activeSessionId
  );
  const current: PendingAskItem | undefined = visibleAsks[0];
  if (!current) return null;

  const replyPermission = async (allow: boolean, remember: boolean) => {
    setBusy(true);
    try {
      if (allow && remember && current.projectRoot) {
        await invoke("permission_rule_add", {
          projectDir: current.projectRoot,
          action: current.action,
          resource: current.resource,
        });
      }
      await invoke("answer_ask", {
        id: current.id,
        reply: allow ? "allow" : "deny",
      });
      removePendingAsk(current.id);
    } catch (error) {
      pushToast(`应答请求失败：${formatError(error)}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const replyQuestion = async (text: string) => {
    const cleaned = text.trim();
    if (!cleaned) return;
    setBusy(true);
    try {
      await invoke("answer_ask", {
        id: current.id,
        reply: cleaned,
      });
      removePendingAsk(current.id);
      setAnswerText("");
    } catch (error) {
      pushToast(`提交回答失败：${formatError(error)}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const isQuestion = current.kind === "question" || current.action === "question";

  return (
    <div className="ask-overlay-bar" role="region" aria-label="等待用户确认">
      <div className="ask-overlay-title">
        <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
          {isQuestion ? <IconSparkles size={13} /> : <IconShield size={13} />}
          <span>
            {isQuestion ? "模型向你提问" : `权限请求：${current.action}`}
            {current.source ? `（${current.source}）` : ""}
          </span>
        </span>
        {visibleAsks.length > 1 && <span className="badge badge-accent">还有 {visibleAsks.length - 1} 条待确认</span>}
      </div>

      <div className="ask-overlay-resource">{current.question ?? current.resource}</div>

      {isQuestion ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {current.options && current.options.length > 0 && (
            <div className="ask-overlay-actions">
              {current.options.map((opt) => (
                <button
                  key={opt}
                  type="button"
                  className="btn btn-sm"
                  disabled={busy}
                  onClick={() => void replyQuestion(opt)}
                >
                  {opt}
                </button>
              ))}
            </div>
          )}
          <div style={{ display: "flex", gap: 6 }}>
            <input
              className="input"
              placeholder="输入你的回复并回车…"
              value={answerText}
              onChange={(e) => setAnswerText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void replyQuestion(answerText);
              }}
            />
            <button
              type="button"
              className="btn btn-accent"
              disabled={busy || !answerText.trim()}
              onClick={() => void replyQuestion(answerText)}
            >
              回复
            </button>
          </div>
        </div>
      ) : (
        <div className="ask-overlay-actions">
          <button
            type="button"
            className="btn btn-accent"
            disabled={busy}
            onClick={() => void replyPermission(true, false)}
          >
            允许一次
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy}
            onClick={() => void replyPermission(true, true)}
          >
            允许并记住
          </button>
          <button
            type="button"
            className="btn btn-danger"
            disabled={busy}
            onClick={() => void replyPermission(false, false)}
          >
            拒绝
          </button>
        </div>
      )}
    </div>
  );
}
