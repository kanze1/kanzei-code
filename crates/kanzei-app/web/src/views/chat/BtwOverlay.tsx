import React, { useEffect, useRef, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import type { SideQuestionTurn } from "../../ipc/types";
import { useAppStore } from "../../store/appStore";
import { appendQuoteToComposer } from "../../store/chatStore";
import { useSessionStore } from "../../store/sessionStore";
import { closeBtwOverlay, pushToast, useUiStore } from "../../store/uiStore";
import { IconClose, IconSend, IconSparkles } from "../../components/Icons";
import { MarkdownView } from "../../components/MarkdownView";

export function BtwOverlay(): React.ReactElement | null {
  const open = useUiStore((s) => s.btwOpen);
  const initialQuestion = useUiStore((s) => s.btwInitialQuestion);
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const activeProcessId = useSessionStore((s) => s.activeProcessId);
  const [turns, setTurns] = useState<SideQuestionTurn[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [lastMeta, setLastMeta] = useState<string>("");
  const inputRef = useRef<HTMLInputElement>(null);
  const autoFiredRef = useRef<string>("");

  const askSide = async (questionText: string, prevTurns: SideQuestionTurn[]) => {
    if (!projectDir || !questionText.trim()) return;
    const q = questionText.trim();
    const nextHistory: SideQuestionTurn[] = [...prevTurns, { role: "user", text: q }];
    setTurns(nextHistory);
    setInput("");
    setBusy(true);
    try {
      const res = await invoke<{
        answer?: string;
        model?: string;
        inputTokens?: number;
        outputTokens?: number;
      }>("side_question", {
        projectDir,
        processId: activeProcessId,
        question: q,
        history: prevTurns.map((t) => ({ role: t.role, text: t.text })),
      });
      const answer = (res?.answer ?? "").trim() || "（模型未返回正文）";
      setTurns([...nextHistory, { role: "assistant", text: answer, model: res?.model }]);
      if (res?.model) {
        setLastMeta(
          `${res.model}${
            typeof res.inputTokens === "number"
              ? ` · In ${res.inputTokens.toLocaleString()} / Out ${(res.outputTokens ?? 0).toLocaleString()}`
              : ""
          }`
        );
      }
    } catch (err) {
      pushToast(`旁路提问失败：${formatError(err)}`, "error");
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (!open) {
      autoFiredRef.current = "";
      return;
    }
    const trimmed = initialQuestion.trim();
    if (trimmed && autoFiredRef.current !== trimmed) {
      autoFiredRef.current = trimmed;
      void askSide(trimmed, []);
    } else {
      window.setTimeout(() => inputRef.current?.focus(), 20);
    }
  }, [open, initialQuestion]);

  if (!open) return null;

  const promoteToMain = async (answerText: string) => {
    await appendQuoteToComposer(`> 旁路问答参考：\n${answerText}\n`);
    pushToast("已将旁路回答插入主对话输入框", "ok", 1800);
    closeBtwOverlay();
  };

  return (
    <div className="btw-overlay" role="dialog" aria-label="/btw 旁路只读提问">
      <div className="btw-header">
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <IconSparkles size={13} />
          <strong>/btw 旁路只读提问</strong>
          <span className="badge">不写入主历史</span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          {lastMeta && <span className="field-hint mono">{lastMeta}</span>}
          {turns.length > 0 && (
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => {
                setTurns([]);
                setLastMeta("");
              }}
            >
              清空
            </button>
          )}
          <button
            type="button"
            className="btn btn-ghost btn-icon"
            aria-label="关闭旁路提问"
            onClick={closeBtwOverlay}
          >
            <IconClose size={13} />
          </button>
        </div>
      </div>

      <div className="btw-body">
        {turns.length === 0 ? (
          <div className="field-hint">
            共享当前对话的只读上下文快照，适合快速查 API、确认概念或核对边界条件，不会污染主任务历史。
          </div>
        ) : (
          turns.map((t, idx) => (
            <div key={idx} className={`btw-turn btw-turn-${t.role}`}>
              <div className="btw-turn-label">{t.role === "user" ? "你" : "旁路回答"}</div>
              {t.role === "assistant" ? (
                <>
                  <MarkdownView text={t.text} />
                  <div className="btw-turn-actions">
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      onClick={() => void promoteToMain(t.text)}
                    >
                      引用到主对话
                    </button>
                  </div>
                </>
              ) : (
                <div className="btw-user-text">{t.text}</div>
              )}
            </div>
          ))
        )}
        {busy && <div className="field-hint">正在结合当前只读上下文思考…</div>}
      </div>

      <form
        className="btw-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (!busy && input.trim()) {
            void askSide(input, turns);
          }
        }}
      >
        <input
          ref={inputRef}
          className="input"
          placeholder="输入临时问题（不会污染主对话）…"
          value={input}
          disabled={busy}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") closeBtwOverlay();
          }}
        />
        <button type="submit" className="btn btn-primary btn-sm" disabled={busy || !input.trim()}>
          <IconSend size={12} />
          <span>提问</span>
        </button>
      </form>
    </div>
  );
}
