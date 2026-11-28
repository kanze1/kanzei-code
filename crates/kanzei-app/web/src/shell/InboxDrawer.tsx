import React, { useEffect, useState } from "react";
import { formatError, invoke } from "../ipc/client";
import type { PendingAskItem } from "../ipc/types";
import { projectDisplayName, selectProject } from "../store/appStore";
import { removePendingAsk } from "../store/sessionStore";
import {
  dismissCompletedNotice,
  navigateView,
  pushToast,
  refreshGlobalInbox,
  uiStore,
  useUiStore,
} from "../store/uiStore";
import { IconActivity, IconClose, IconRefresh, IconShield } from "../components/Icons";

export function InboxDrawer(): React.ReactElement | null {
  const open = useUiStore((s) => s.inboxOpen);
  const globalAsks = useUiStore((s) => s.globalAsks);
  const globalAsksError = useUiStore((s) => s.globalAsksError);
  const recentCompleted = useUiStore((s) => s.recentCompleted);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [answers, setAnswers] = useState<Record<number, string>>({});

  useEffect(() => {
    if (!open) return;
    void refreshGlobalInbox();
  }, [open]);

  if (!open) return null;

  const handleAnswer = async (ask: PendingAskItem, reply: string) => {
    setBusyId(ask.id);
    try {
      await invoke("answer_ask", { id: ask.id, reply });
      removePendingAsk(ask.id);
      await refreshGlobalInbox();
      pushToast(
        ask.kind === "question"
          ? reply === "cancel"
            ? "已取消该提问"
            : "已回复会话提问"
          : reply === "deny"
          ? "已拒绝该权限请求"
          : reply === "always"
          ? "已允许并记住规则"
          : "已放行本次操作",
        "ok",
        2200
      );
    } catch (err) {
      pushToast(`处理失败：${formatError(err)}`, "error");
    } finally {
      setBusyId(null);
    }
  };

  const jumpToSession = async (projectDir?: string, processId?: string, sessionId?: string) => {
    uiStore.setState({ inboxOpen: false });
    if (sessionId) dismissCompletedNotice(sessionId);
    if (projectDir) {
      await selectProject(projectDir, processId ?? null);
      navigateView("chat");
    }
  };

  return (
    <div
      className="modal-scrim"
      role="dialog"
      aria-modal="true"
      aria-label="全局待办收件箱"
      onClick={() => uiStore.setState({ inboxOpen: false })}
    >
      <div className="modal-panel modal-lg inbox-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div className="modal-title" style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <IconShield size={15} />
            <span>全局待办收件箱（Inbox）</span>
            {globalAsks.length + recentCompleted.length > 0 && (
              <span className="badge badge-warn">
                {globalAsks.length + recentCompleted.length}
              </span>
            )}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => void refreshGlobalInbox()}
            >
              <IconRefresh size={12} />
              <span>刷新</span>
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-icon"
              aria-label="关闭收件箱"
              onClick={() => uiStore.setState({ inboxOpen: false })}
            >
              <IconClose size={14} />
            </button>
          </div>
        </div>

        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <section>
            <div className="inbox-section-head">
              <strong>待确认权限与提问（{globalAsksError ? "读取失败" : globalAsks.length}）</strong>
              <span className="field-hint">跨项目直接审批，无需来回切换窗口</span>
            </div>
            {globalAsksError ? (
              <div
                className="inbox-card"
                role="alert"
                style={{ borderColor: "var(--err, #ef4444)" }}
              >
                <div className="inbox-card-head">
                  <span className="badge badge-err">待办读取失败</span>
                  <button
                    type="button"
                    className="btn btn-sm"
                    onClick={() => void refreshGlobalInbox()}
                  >
                    重试读取
                  </button>
                </div>
                <div className="inbox-card-body">
                  <div style={{ color: "var(--err)", fontSize: 12.5 }}>
                    无法确认是否存在待审批任务：{globalAsksError}
                  </div>
                </div>
              </div>
            ) : globalAsks.length === 0 ? (
              <div className="inbox-empty">当前没有待处理的权限请求或提问。</div>
            ) : (
              <div className="inbox-list">
                {globalAsks.map((ask) => {
                  const isQuestion = ask.kind === "question";
                  const customText = answers[ask.id] ?? "";
                  return (
                    <div key={ask.id} className="inbox-card">
                      <div className="inbox-card-head">
                        <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                          <span className={`badge ${isQuestion ? "badge-accent" : "badge-warn"}`}>
                            {isQuestion ? "等待回答" : "权限审批"}
                          </span>
                          <span className="mono" style={{ fontSize: 11.5 }}>
                            {projectDisplayName(ask.projectDir ?? ask.projectRoot ?? null)}
                          </span>
                          {ask.source && <span className="badge">{ask.source}</span>}
                        </div>
                        {ask.projectDir && (
                          <button
                            type="button"
                            className="btn btn-ghost btn-sm"
                            onClick={() =>
                              void jumpToSession(ask.projectDir, ask.processId, ask.sessionId)
                            }
                          >
                            跳转会话
                          </button>
                        )}
                      </div>

                      {isQuestion ? (
                        <div className="inbox-card-body">
                          <div style={{ fontSize: 13, marginBottom: 8 }}>
                            {ask.question || ask.resource}
                          </div>
                          {ask.options && ask.options.length > 0 && (
                            <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginBottom: 8 }}>
                              {ask.options.map((opt) => (
                                <button
                                  key={opt}
                                  type="button"
                                  className="btn btn-sm"
                                  disabled={busyId === ask.id}
                                  onClick={() => void handleAnswer(ask, opt)}
                                >
                                  {opt}
                                </button>
                              ))}
                            </div>
                          )}
                          <div style={{ display: "flex", gap: 6 }}>
                            <input
                              className="input"
                              placeholder="直接输入回复内容…"
                              value={customText}
                              onChange={(e) =>
                                setAnswers((prev) => ({ ...prev, [ask.id]: e.target.value }))
                              }
                              onKeyDown={(e) => {
                                if (e.key === "Enter" && customText.trim()) {
                                  void handleAnswer(ask, customText.trim());
                                }
                              }}
                            />
                            <button
                              type="button"
                              className="btn btn-primary btn-sm"
                              disabled={busyId === ask.id || !customText.trim()}
                              onClick={() => void handleAnswer(ask, customText.trim())}
                            >
                              发送
                            </button>
                            <button
                              type="button"
                              className="btn btn-ghost btn-sm"
                              disabled={busyId === ask.id}
                              onClick={() => void handleAnswer(ask, "cancel")}
                            >
                              取消
                            </button>
                          </div>
                        </div>
                      ) : (
                        <div className="inbox-card-body">
                          <div className="mono" style={{ fontSize: 12, marginBottom: 8, wordBreak: "break-all" }}>
                            <strong>{ask.action}</strong> · {ask.resource}
                          </div>
                          <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                            <button
                              type="button"
                              className="btn btn-primary btn-sm"
                              disabled={busyId === ask.id}
                              onClick={() => void handleAnswer(ask, "once")}
                            >
                              本次允许
                            </button>
                            <button
                              type="button"
                              className="btn btn-sm"
                              disabled={busyId === ask.id}
                              onClick={() => void handleAnswer(ask, "always")}
                            >
                              始终允许
                            </button>
                            <button
                              type="button"
                              className="btn btn-ghost btn-sm"
                              style={{ color: "var(--err)" }}
                              disabled={busyId === ask.id}
                              onClick={() => void handleAnswer(ask, "deny")}
                            >
                              拒绝
                            </button>
                          </div>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </section>

          <section>
            <div className="inbox-section-head">
              <strong>后台已完工待验收（{recentCompleted.length}）</strong>
              {recentCompleted.length > 0 && (
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => dismissCompletedNotice()}
                >
                  全部已读
                </button>
              )}
            </div>
            {recentCompleted.length === 0 ? (
              <div className="inbox-empty">暂无新的后台完工会话提醒。</div>
            ) : (
              <div className="inbox-list">
                {recentCompleted.map((item) => (
                  <div key={item.sessionId} className="inbox-card inbox-card-done">
                    <div className="inbox-card-head">
                      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        <IconActivity size={13} />
                        <strong>{item.label}</strong>
                        <span className="field-hint">
                          {new Date(item.at).toLocaleTimeString()}
                        </span>
                      </div>
                      <div style={{ display: "flex", gap: 6 }}>
                        {item.projectDir && (
                          <button
                            type="button"
                            className="btn btn-sm btn-primary"
                            onClick={() =>
                              void jumpToSession(item.projectDir, item.processId, item.sessionId)
                            }
                          >
                            前往验收
                          </button>
                        )}
                        <button
                          type="button"
                          className="btn btn-ghost btn-sm"
                          onClick={() => dismissCompletedNotice(item.sessionId)}
                        >
                          忽略
                        </button>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>
      </div>
    </div>
  );
}
