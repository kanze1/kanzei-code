import React from "react";
import { formatError, invoke } from "../../ipc/client";
import {
  projectDisplayName,
  selectProject,
  useAppStore,
} from "../../store/appStore";
import {
  chatStore,
  clearCurrentConversation,
  compactCurrentConversation,
} from "../../store/chatStore";
import { refreshGitStatus } from "../../store/drawerStore";
import { createConversation, useSessionStore } from "../../store/sessionStore";
import { confirmDialog, navigateView, pushToast } from "../../store/uiStore";
import {
  IconBranch,
  IconCompact,
  IconDownload,
  IconFolder,
  IconPlus,
  IconTrash,
} from "../../components/Icons";
import { AskOverlay } from "./AskOverlay";
import { Composer } from "./Composer";
import { ContextDrawer } from "./ContextDrawer";
import { MessageList } from "./MessageList";

export function ChatView(): React.ReactElement {
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const projects = useAppStore((s) => s.projects);
  const projectFacts = useAppStore((s) => s.projectFacts);
  const activeProcess = useSessionStore((s) =>
    s.processes.find((p) => p.id === s.activeProcessId) ?? null
  );

  if (!activeProjectDir) {
    return (
      <div className="empty-state" style={{ flex: 1 }}>
        <div className="empty-state-icon">
          <IconFolder size={20} />
        </div>
        <h3>尚未选择项目</h3>
        <p>前往工作台添加或新建项目，或点击左侧「无项目对话」直接开始。</p>
        <button type="button" className="btn btn-primary" onClick={() => navigateView("workspace")}>
          打开工作台
        </button>
      </div>
    );
  }

  const handleInitGit = async () => {
    try {
      await invoke("project_git_init", { projectDir: activeProjectDir, initialCommit: true });
      await refreshGitStatus(activeProjectDir, activeProcess?.worktree_path ?? null);
      pushToast("已初始化独立 Git 仓库", "ok");
    } catch (error) {
      pushToast(`初始化 Git 失败：${formatError(error)}`, "error");
    }
  };

  const handleLinkToProject = async (targetDir: string) => {
    if (!activeProcess || !targetDir) return;
    try {
      const created = await invoke<{ id: string }>("general_chat_link", {
        processId: activeProcess.id,
        projectDir: targetDir,
      });
      await selectProject(targetDir, created.id);
      pushToast("已将对话复制到目标项目", "ok");
    } catch (error) {
      pushToast(`关联项目失败：${formatError(error)}`, "error");
    }
  };

  const handleClear = async () => {
    const ok = await confirmDialog({
      title: "清空当前对话？",
      message: "将在当前会话开启新的空白对话段，历史记录仍保留在数据库中。",
      confirmLabel: "开启新段",
    });
    if (ok) await clearCurrentConversation();
  };

  const handleExport = async () => {
    const msgs = chatStore.getState().messages;
    if (msgs.length === 0) {
      pushToast("当前对话暂无可导出的消息", "info");
      return;
    }
    const transcript = msgs
      .filter((m) => m.role === "user" || m.role === "assistant")
      .map((m) => {
        const header = m.role === "user" ? "## 用户" : "## 助手";
        const body = (m.parts ?? [])
          .filter((p) => p.type === "text" && typeof p.text === "string")
          .map((p) => p.text ?? "")
          .join("\n")
          .trim();
        return body ? `${header}\n\n${body}` : "";
      })
      .filter(Boolean)
      .join("\n\n---\n\n");
    const withSummary = await confirmDialog({
      title: "导出对话为 Markdown",
      message: "是否先调用快速模型在文件顶部生成简明纪要？选择「取消」则直接导出原始对话。",
      confirmLabel: "生成纪要并导出",
      cancelLabel: "直接导出原文",
    });
    let content = `# ${activeProcess?.label ?? projectDisplayName(activeProjectDir)}\n\n${transcript}\n`;
    if (withSummary) {
      try {
        const res = await invoke<{ summary?: string }>("summarize_chat", {
          projectDir: activeProjectDir,
          transcript,
        });
        if (res?.summary) {
          content = `# ${activeProcess?.label ?? projectDisplayName(activeProjectDir)}\n\n## 纪要\n\n${res.summary}\n\n---\n\n${transcript}\n`;
        }
      } catch (err) {
        pushToast(`生成纪要失败，将直接导出原文：${formatError(err)}`, "warn");
      }
    }
    try {
      const saved = await invoke<string | null>("save_conversation_export", {
        name: activeProcess?.label ?? "conversation",
        content,
      });
      if (saved) pushToast(`已导出到 ${saved}`, "ok");
    } catch (err) {
      pushToast(`导出失败：${formatError(err)}`, "error");
    }
  };

  const needsGitBanner = !generalMode && projectFacts && projectFacts.has_git === false;
  const profileLabel =
    activeProcess?.profile === "dev-pair"
      ? "结伴开发"
      : activeProcess?.profile === "isolated"
      ? "只读讨论"
      : "自主开发";

  return (
    <div className="chat-layout">
      <div className="chat-column">
        <div className="chat-topbar">
          <div className="chat-topbar-left">
            {activeProcess?.branch && (
              <span className="chat-topbar-meta mono" title={`当前分支：${activeProcess.branch}`}>
                <IconBranch size={12} />
                <span>{activeProcess.branch}</span>
              </span>
            )}
            {generalMode ? (
              <span className="badge badge-accent">无项目模式</span>
            ) : (
              <span className="badge">{profileLabel}</span>
            )}
            {activeProcess?.worktree_path && (
              <span className="badge badge-info" title={activeProcess.worktree_path}>
                隔离工作树
              </span>
            )}
          </div>

          <div className="chat-topbar-right">
            {generalMode && projects.projects.length > 0 && (
              <select
                className="composer-select"
                defaultValue=""
                aria-label="关联到项目"
                onChange={(e) => {
                  const dir = e.target.value;
                  e.target.value = "";
                  if (dir) void handleLinkToProject(dir);
                }}
              >
                <option value="">关联到项目…</option>
                {projects.projects.map((dir) => (
                  <option key={dir} value={dir}>
                    {projects.names[dir] ?? dir}
                  </option>
                ))}
              </select>
            )}
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              title="在当前项目开启新对话 (Ctrl+Shift+N)"
              onClick={() => void createConversation(activeProjectDir)}
            >
              <IconPlus size={12} />
              <span>新对话</span>
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              title="导出对话为 Markdown"
              onClick={() => void handleExport()}
            >
              <IconDownload size={12} />
              <span>导出</span>
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              title="压缩当前上下文以节省 Token"
              onClick={() => void compactCurrentConversation()}
            >
              <IconCompact size={12} />
              <span>压缩</span>
            </button>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              title="开启新的空白对话段"
              onClick={() => void handleClear()}
            >
              <IconTrash size={12} />
              <span>清空</span>
            </button>
          </div>
        </div>

        {needsGitBanner && (
          <div className="facts-banner">
            <span>当前项目尚未初始化独立 Git 仓库，建议初始化以启用 Diff 对照与安全回滚。</span>
            <div className="facts-banner-actions">
              <button type="button" className="btn btn-sm btn-primary" onClick={() => void handleInitGit()}>
                一键初始化 Git
              </button>
            </div>
          </div>
        )}

        <MessageList />
        <AskOverlay />
        <Composer />
      </div>

      <ContextDrawer />
    </div>
  );
}
