import React, { useEffect, useRef, useState } from "react";
import type { PromptAttachment } from "../../ipc/types";
import { loadModelsCatalog, projectDisplayName, useAppStore } from "../../store/appStore";
import {
  cancelPendingInput,
  chatStore,
  draftKeyFor,
  sendPrompt,
  stopCurrentRun,
  updateDraft,
  useChatStore,
} from "../../store/chatStore";
import { toggleDrawer, useDrawerStore } from "../../store/drawerStore";
import {
  updateAutoRun,
  updateProcessSettings,
  useSessionStore,
} from "../../store/sessionStore";
import { promptDialog, pushToast, uiStore } from "../../store/uiStore";
import {
  IconAttach,
  IconBranch,
  IconClose,
  IconCpu,
  IconSend,
  IconShield,
  IconSparkles,
  IconStop,
  IconTarget,
} from "../../components/Icons";

function fileToBase64(file: File): Promise<PromptAttachment> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result ?? "");
      const comma = result.indexOf(",");
      const data = comma >= 0 ? result.slice(comma + 1) : result;
      resolve({
        file_name: file.name || "attachment",
        media_type: file.type || "application/octet-stream",
        data,
      });
    };
    reader.onerror = () => reject(reader.error ?? new Error("读取附件失败"));
    reader.readAsDataURL(file);
  });
}

export function Composer(): React.ReactElement {
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const modelsCatalog = useAppStore((s) => s.modelsCatalog);
  const activeProcess = useSessionStore((s) =>
    s.processes.find((p) => p.id === s.activeProcessId) ?? null
  );
  const pendingInputs = useSessionStore((s) => s.pendingInputs);
  const effectiveModel = useSessionStore((s) => s.effectiveModel);
  const autoRun = useSessionStore((s) => s.autoRun);
  const autoAllow = useChatStore((s) => s.autoAllow);
  const deliveryMode = useChatStore((s) => s.deliveryMode);
  const drafts = useChatStore((s) => s.drafts);
  const gitStatus = useDrawerStore((s) => s.gitStatus);

  const key = draftKeyFor(activeProjectDir, activeProcess?.id ?? null);
  const storedDraft = drafts[key];

  const [text, setText] = useState(storedDraft?.text ?? "");
  const [attachments, setAttachments] = useState<PromptAttachment[]>(storedDraft?.attachments ?? []);
  const [dragOver, setDragOver] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setText(storedDraft?.text ?? "");
    setAttachments(storedDraft?.attachments ?? []);
  }, [key, storedDraft?.text]);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(200, Math.max(46, el.scrollHeight))}px`;
  }, [text]);

  const handleTextChange = (next: string) => {
    setText(next);
    updateDraft(activeProjectDir, activeProcess?.id ?? null, next, attachments);
  };

  const addFiles = async (files: FileList | File[]) => {
    const list = Array.from(files);
    if (list.length === 0) return;
    try {
      const encoded = await Promise.all(list.map(fileToBase64));
      const next = [...attachments, ...encoded];
      setAttachments(next);
      updateDraft(activeProjectDir, activeProcess?.id ?? null, text, next);
    } catch {
      pushToast("读取附件失败", "error");
    }
  };

  const removeAttachment = (idx: number) => {
    const next = attachments.filter((_, i) => i !== idx);
    setAttachments(next);
    updateDraft(activeProjectDir, activeProcess?.id ?? null, text, next);
  };

  const handleSubmit = async () => {
    if (!text.trim() && attachments.length === 0) return;
    const payloadText = text;
    const payloadFiles = attachments;
    setText("");
    setAttachments([]);
    const ok = await sendPrompt(payloadText, payloadFiles);
    if (!ok) {
      setText(payloadText);
      setAttachments(payloadFiles);
    }
  };

  const handleEditGoal = async () => {
    const next = await promptDialog({
      title: "设置自动推进目标（Goal）",
      label: "留空即清除目标；挂上后自动推进会持续向该停止条件推进",
      initialValue: autoRun.goal ?? "",
      placeholder: "例如：修复全部 clippy 警告并确保单元测试全绿",
      confirmLabel: "保存目标",
    });
    if (next !== null) {
      await updateAutoRun({ goal: next, enabled: Boolean(next.trim()) || autoRun.enabled });
    }
  };

  const running = Boolean(activeProcess?.running);
  const effectiveModelLabel =
    effectiveModel?.model?.value ?? activeProcess?.model ?? "默认主模型";
  const currentProfile = activeProcess?.profile ?? "dev-auto";
  const currentReasoning = activeProcess?.reasoning ?? "";

  return (
    <div className="composer-shell">
      <div
        className={`composer-card${dragOver ? " drag-over" : ""}`}
        onDragOver={(e) => {
          e.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDragOver(false);
          if (e.dataTransfer.files?.length) {
            void addFiles(e.dataTransfer.files);
          }
        }}
      >
        <div className="composer-context-strip">
          <div className="composer-context-left">
            {!generalMode && gitStatus?.branch ? (
              <button
                type="button"
                className="composer-chip mono"
                onClick={() => toggleDrawer("files")}
                title="点击查看本轮文件改动与 Diff"
              >
                <IconBranch size={11} />
                <span className="composer-chip-text">{gitStatus.branch}</span>
                {gitStatus.changes > 0 && (
                  <span style={{ color: "var(--ok)", fontWeight: 600 }}>
                    · {gitStatus.changes} 改动
                  </span>
                )}
              </button>
            ) : (
              <span className="composer-chip-text">
                {generalMode ? "无项目对话" : projectDisplayName(activeProjectDir)}
              </span>
            )}

            {!generalMode && gitStatus?.repo === "none" && (
              <span className="badge badge-warn">未初始化 Git</span>
            )}

            {autoRun.goal && (
              <button
                type="button"
                className="composer-chip composer-chip-goal"
                onClick={() => void handleEditGoal()}
                title={`自动推进目标：${autoRun.goal}（点击修改）`}
              >
                <IconTarget size={11} />
                <span className="composer-chip-text">{autoRun.goal}</span>
              </button>
            )}
          </div>

          <button
            type="button"
            className="composer-chip composer-chip-model mono"
            onClick={() => uiStore.setState({ projectModelsOpen: true })}
            disabled={generalMode || !activeProjectDir}
            title={`当前生效模型：${effectiveModelLabel}（点击配置项目覆盖）`}
          >
            <IconCpu size={11} />
            <span className="composer-chip-text">{effectiveModelLabel}</span>
          </button>
        </div>

        {pendingInputs.length > 0 && (
          <div className="composer-queue-strip">
            {pendingInputs.map((item) => (
              <div key={item.input_id} className="composer-queue-item">
                <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  <span className="badge badge-accent" style={{ marginRight: 6 }}>
                    {item.delivery === "steer" ? "插话" : "排队"}
                  </span>
                  {item.prompt.slice(0, 80)}
                </span>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => void cancelPendingInput(item.input_id)}
                >
                  撤销
                </button>
              </div>
            ))}
          </div>
        )}

        {attachments.length > 0 && (
          <div className="composer-attachments">
            {attachments.map((att, idx) => {
              const isImg = att.media_type.startsWith("image/");
              return (
                <span key={idx} className="composer-att-chip">
                  {isImg && (
                    <img
                      src={`data:${att.media_type};base64,${att.data}`}
                      alt=""
                      className="composer-att-thumb"
                    />
                  )}
                  <span className="mono">{att.file_name}</span>
                  <button
                    type="button"
                    className="composer-att-remove"
                    onClick={() => removeAttachment(idx)}
                    aria-label="移除附件"
                  >
                    <IconClose size={10} />
                  </button>
                </span>
              );
            })}
          </div>
        )}

        <textarea
          id="kz-composer-input"
          ref={textareaRef}
          className="composer-textarea"
          rows={2}
          placeholder={
            running
              ? deliveryMode === "steer"
                ? "模型运行中：输入插话将在安全检查点介入…"
                : "模型运行中：输入内容将加入下一轮排队…"
              : "输入需求、问题或指令（Enter 发送，Shift+Enter 换行，支持粘贴图片与拖入文件）…"
          }
          value={text}
          onChange={(e) => handleTextChange(e.target.value)}
          onPaste={(e) => {
            const files = Array.from(e.clipboardData.files ?? []);
            if (files.length > 0) {
              e.preventDefault();
              void addFiles(files);
            }
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void handleSubmit();
            }
          }}
        />

        <div className="composer-toolbar">
          <div className="composer-toolbar-left">
            <input
              ref={fileInputRef}
              type="file"
              multiple
              style={{ display: "none" }}
              onChange={(e) => {
                if (e.target.files) void addFiles(e.target.files);
                e.target.value = "";
              }}
            />
            <button
              type="button"
              className="btn btn-ghost btn-icon btn-sm"
              onClick={() => fileInputRef.current?.click()}
              title="添加文件或图片附件"
              aria-label="添加附件"
            >
              <IconAttach size={14} />
            </button>

            <select
              className="composer-select"
              aria-label="模型选择"
              value={activeProcess?.model ?? ""}
              onFocus={() => {
                if (modelsCatalog.length === 0) void loadModelsCatalog(activeProjectDir, false);
              }}
              onChange={(e) => {
                if (activeProjectDir && activeProcess) {
                  void updateProcessSettings(activeProjectDir, activeProcess.id, {
                    model: e.target.value || null,
                  });
                }
              }}
            >
              <option value="">默认模型</option>
              {modelsCatalog.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label}
                </option>
              ))}
            </select>

            <select
              className="composer-select"
              aria-label="思考强度"
              value={currentReasoning}
              onChange={(e) => {
                if (activeProjectDir && activeProcess) {
                  void updateProcessSettings(activeProjectDir, activeProcess.id, {
                    reasoning: e.target.value || null,
                  });
                }
              }}
            >
              <option value="">思考: 默认</option>
              <option value="off">思考: 关</option>
              <option value="low">思考: 低</option>
              <option value="medium">思考: 中</option>
              <option value="high">思考: 高</option>
              <option value="xhigh">思考: 极高</option>
            </select>

            <select
              className="composer-select"
              aria-label="执行模式"
              value={currentProfile}
              onChange={(e) => {
                if (activeProjectDir && activeProcess) {
                  void updateProcessSettings(activeProjectDir, activeProcess.id, {
                    profile: e.target.value,
                  });
                }
              }}
            >
              <option value="dev-auto">自主开发</option>
              <option value="dev-pair">结伴开发</option>
              <option value="isolated">只读讨论</option>
            </select>

            <button
              type="button"
              className={`composer-toggle-pill${autoAllow ? " active" : ""}`}
              onClick={() => chatStore.setState({ autoAllow: !autoAllow })}
              title="本会话自动放行常规工具权限"
            >
              <IconShield size={12} />
              <span>放行</span>
            </button>

            {!generalMode && (
              <>
                <button
                  type="button"
                  className={`composer-toggle-pill${autoRun.enabled ? " active-accent" : ""}`}
                  onClick={() => void updateAutoRun({ enabled: !autoRun.enabled, paused: false })}
                  title="开启或关闭多轮自主推进"
                >
                  <IconSparkles size={12} />
                  <span>{autoRun.enabled ? "推进中" : "自动推进"}</span>
                </button>
                <button
                  type="button"
                  className={`btn btn-ghost btn-icon btn-sm${autoRun.goal ? " active" : ""}`}
                  onClick={() => void handleEditGoal()}
                  title={autoRun.goal ? `修改自动推进目标：${autoRun.goal}` : "设置自动推进停止目标"}
                  aria-label="设置自动推进目标"
                >
                  <IconTarget size={13} />
                </button>
              </>
            )}
          </div>

          <div className="composer-toolbar-right">
            {running && (
              <>
                <select
                  className="composer-select"
                  aria-label="运行中投递方式"
                  value={deliveryMode}
                  onChange={(e) =>
                    chatStore.setState({
                      deliveryMode: e.target.value === "queue" ? "queue" : "steer",
                    })
                  }
                >
                  <option value="steer">插话</option>
                  <option value="queue">排队</option>
                </select>
                <button
                  type="button"
                  className="btn btn-danger btn-pill"
                  onClick={() => void stopCurrentRun()}
                >
                  <IconStop size={11} />
                  <span>停止</span>
                </button>
              </>
            )}
            <button
              type="button"
              className="btn btn-accent composer-send-btn"
              disabled={!text.trim() && attachments.length === 0}
              onClick={() => void handleSubmit()}
            >
              <IconSend size={13} />
              <span>{running ? (deliveryMode === "steer" ? "插话" : "排队") : "发送"}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
