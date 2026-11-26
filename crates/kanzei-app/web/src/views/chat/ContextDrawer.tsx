import React, { useEffect, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import { useAppStore } from "../../store/appStore";
import { useChatStore } from "../../store/chatStore";
import {
  closeDrawer,
  drawerStore,
  openExternalPath,
  openFileInDrawer,
  refreshGitStatus,
  saveDrawerFile,
  setDrawerEditContent,
  setDrawerFileTab,
  setDrawerWidth,
  useDrawerStore,
} from "../../store/drawerStore";
import { getActiveProcess } from "../../store/sessionStore";
import { confirmDialog, openViewer, promptDialog, pushToast } from "../../store/uiStore";

interface TerminalProcessInfo {
  id: string;
  command?: string;
  running?: boolean;
  status?: string;
}

interface WorktreeItem {
  path: string;
  branch: string;
  clean: boolean;
  files: string[];
  diff: string;
  bound_process?: string | null;
}

export function ContextDrawer(): React.ReactElement | null {
  const open = useDrawerStore((s) => s.open);
  const mode = useDrawerStore((s) => s.mode);
  const width = useDrawerStore((s) => s.width);
  const projectDir = useDrawerStore((s) => s.projectDir);
  const worktreePath = useDrawerStore((s) => s.worktreePath);
  const gitStatus = useDrawerStore((s) => s.gitStatus);
  const touchedFiles = useDrawerStore((s) => s.touchedFiles);
  const activeFile = useDrawerStore((s) => s.activeFile);
  const fileTab = useDrawerStore((s) => s.fileTab);
  const highlightLine = useDrawerStore((s) => s.highlightLine);
  const loadingFile = useDrawerStore((s) => s.loadingFile);
  const preview = useDrawerStore((s) => s.preview);
  const diff = useDrawerStore((s) => s.diff);
  const editContent = useDrawerStore((s) => s.editContent);
  const editDirty = useDrawerStore((s) => s.editDirty);
  const saving = useDrawerStore((s) => s.saving);
  const conflictMessage = useDrawerStore((s) => s.conflictMessage);
  const error = useDrawerStore((s) => s.error);

  const openTools = useAppStore((s) => s.openTools);
  const generalMode = useAppStore((s) => s.generalMode);
  const liveSubagents = useChatStore((s) => s.liveSubagents);
  const liveTools = useChatStore((s) => s.liveTools);
  const [terminals, setTerminals] = useState<TerminalProcessInfo[]>([]);
  const [worktrees, setWorktrees] = useState<WorktreeItem[]>([]);
  const [worktreeBusy, setWorktreeBusy] = useState(false);
  const [manualPath, setManualPath] = useState("");

  const loadWorktrees = () => {
    if (!projectDir || generalMode) return;
    invoke<WorktreeItem[]>("worktree_list", { projectDir })
      .then((list) => setWorktrees(Array.isArray(list) ? list : []))
      .catch(() => setWorktrees([]));
  };

  useEffect(() => {
    if (!open || mode !== "worktrees" || !projectDir || generalMode) return;
    loadWorktrees();
  }, [open, mode, projectDir, generalMode]);

  useEffect(() => {
    if (!open || mode !== "activity" || !projectDir) return;
    const loadTerminals = () => {
      invoke<{ processes?: TerminalProcessInfo[] } | TerminalProcessInfo[]>("terminal_monitor", {
        projectDir,
        action: "list",
      })
        .then((res) => {
          const list = Array.isArray(res) ? res : res?.processes ?? [];
          setTerminals(list);
        })
        .catch(() => {});
    };
    loadTerminals();
    const timer = window.setInterval(loadTerminals, 3000);
    return () => window.clearInterval(timer);
  }, [open, mode, projectDir]);

  if (!open) return null;

  const startResize = (e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = width;
    const onMove = (ev: MouseEvent) => setDrawerWidth(startW + (startX - ev.clientX));
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  const gitFiles = gitStatus?.files ?? [];
  const mergedFiles = Array.from(
    new Set([...gitFiles.map((f) => f.path), ...touchedFiles])
  );

  const stopTerminal = async (id: string) => {
    if (!projectDir) return;
    try {
      await invoke("run_tool_process_stop", { projectDir, processId: id });
      setTerminals((prev) => prev.filter((t) => t.id !== id));
      pushToast("已停止后台终端进程", "ok");
    } catch (err) {
      pushToast(`停止终端失败：${formatError(err)}`, "error");
    }
  };

  const stopSubagent = async (taskId: string) => {
    const proc = getActiveProcess();
    if (!projectDir || !proc) return;
    try {
      await invoke("stop_task", { projectDir, processId: proc.id, taskId });
      pushToast("已发送子代理取消信号", "ok");
    } catch (err) {
      pushToast(`取消子代理失败：${formatError(err)}`, "error");
    }
  };

  const preferredEditor = openTools.find((t) => t.available !== false);

  const handleCreateWorktree = async () => {
    if (!projectDir) return;
    const name = await promptDialog({
      title: "新建并行 Git 工作树",
      label: "分支/工作树名称（字母、数字、连字符）",
      placeholder: "例如：feat-refactor",
      confirmLabel: "创建",
    });
    if (!name) return;
    setWorktreeBusy(true);
    try {
      await invoke("worktree_create", { projectDir, name });
      pushToast(`已创建工作树 ${name}`, "ok");
      loadWorktrees();
    } catch (err) {
      pushToast(`创建工作树失败：${formatError(err)}`, "error");
    } finally {
      setWorktreeBusy(false);
    }
  };

  const handleMergeWorktree = async (wt: WorktreeItem) => {
    if (!projectDir) return;
    setWorktreeBusy(true);
    try {
      const conflicts = await invoke<string[]>("worktree_merge_preview", {
        projectDir,
        worktreePath: wt.path,
      });
      const msg =
        conflicts && conflicts.length > 0
          ? `合并预检发现以下冲突文件：\n${conflicts.join("\n")}\n\n确认仍尝试合并分支 ${wt.branch} 吗？`
          : `合并预检无冲突。确认将分支 ${wt.branch} 合并回主分支？`;
      const ok = await confirmDialog({
        title: `合并工作树 ${wt.branch}`,
        message: msg,
        confirmLabel: "确认合并",
      });
      if (!ok) return;
      const result = await invoke<string>("worktree_merge", {
        projectDir,
        worktreePath: wt.path,
      });
      pushToast(result || `已合并 ${wt.branch}`, "ok");
      loadWorktrees();
      void refreshGitStatus(projectDir, worktreePath);
    } catch (err) {
      pushToast(`合并失败：${formatError(err)}`, "error");
    } finally {
      setWorktreeBusy(false);
    }
  };

  const handleDiscardWorktree = async (wt: WorktreeItem) => {
    if (!projectDir) return;
    const ok = await confirmDialog({
      title: `放弃工作树 ${wt.branch}？`,
      message: `将移除工作树 ${wt.path} 并删除其分支，未合并的改动会丢失。`,
      confirmLabel: "放弃并删除",
      danger: true,
    });
    if (!ok) return;
    setWorktreeBusy(true);
    try {
      await invoke("worktree_discard", { projectDir, worktreePath: wt.path });
      pushToast(`已移除工作树 ${wt.branch}`, "ok");
      loadWorktrees();
    } catch (err) {
      pushToast(`移除工作树失败：${formatError(err)}`, "error");
    } finally {
      setWorktreeBusy(false);
    }
  };

  return (
    <aside className="side-drawer" style={{ width }} aria-label="右侧伴生抽屉">
      <div className="side-drawer-resizer" onMouseDown={startResize} aria-hidden="true" />

      <div className="drawer-header">
        <div className="drawer-tabs">
          <button
            type="button"
            className={`drawer-tab${mode === "files" ? " active" : ""}`}
            onClick={() => drawerStore.setState({ mode: "files" })}
          >
            文件 ({mergedFiles.length})
          </button>
          <button
            type="button"
            className={`drawer-tab${mode === "activity" ? " active" : ""}`}
            onClick={() => drawerStore.setState({ mode: "activity" })}
          >
            活动 ({liveSubagents.length + terminals.length})
          </button>
          {!generalMode && (
            <button
              type="button"
              className={`drawer-tab${mode === "worktrees" ? " active" : ""}`}
              onClick={() => drawerStore.setState({ mode: "worktrees" })}
            >
              工作树 ({worktrees.length})
            </button>
          )}
        </div>
        <button type="button" className="btn btn-ghost btn-icon" onClick={closeDrawer} aria-label="关闭抽屉">
          ×
        </button>
      </div>

      {mode === "files" ? (
        <div className="drawer-body">
          <div style={{ display: "flex", gap: 6, padding: "8px 10px", borderBottom: "1px solid var(--border-soft)" }}>
            <input
              className="input mono"
              style={{ height: 28, fontSize: 12 }}
              placeholder="输入相对路径直接查看，如 src/main.rs"
              value={manualPath}
              onChange={(e) => setManualPath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && manualPath.trim()) {
                  void openFileInDrawer(manualPath.trim());
                }
              }}
            />
            <button
              type="button"
              className="btn btn-sm"
              onClick={() => projectDir && void refreshGitStatus(projectDir, worktreePath)}
            >
              刷新
            </button>
          </div>

          <div className="drawer-file-list">
            {mergedFiles.length === 0 ? (
              <div style={{ padding: 10, fontSize: 12, color: "var(--dim)" }}>
                本轮暂无触及或未提交变动的文件。点击对话中的文件路径链接或上方输入路径即可查看。
              </div>
            ) : (
              mergedFiles.map((file) => {
                const stat = gitFiles.find((f) => f.path === file);
                return (
                  <button
                    key={file}
                    type="button"
                    className={`drawer-file-item${activeFile === file ? " active" : ""}`}
                    onClick={() => void openFileInDrawer(file)}
                  >
                    <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                      {file}
                    </span>
                    {stat && (
                      <span style={{ fontSize: 11, flex: "none" }}>
                        {stat.untracked ? (
                          <span className="badge badge-ok">新增</span>
                        ) : (
                          <>
                            <span style={{ color: "var(--diff-add)" }}>+{stat.additions}</span>{" "}
                            <span style={{ color: "var(--diff-del)" }}>-{stat.deletions}</span>
                          </>
                        )}
                      </span>
                    )}
                  </button>
                );
              })
            )}
          </div>

          {activeFile ? (
            <>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 6,
                  padding: "6px 10px",
                  borderBottom: "1px solid var(--border-soft)",
                }}
              >
                <div className="drawer-tabs">
                  <button
                    type="button"
                    className={`drawer-tab${fileTab === "preview" ? " active" : ""}`}
                    onClick={() => setDrawerFileTab("preview")}
                  >
                    原文
                  </button>
                  <button
                    type="button"
                    className={`drawer-tab${fileTab === "diff" ? " active" : ""}`}
                    onClick={() => setDrawerFileTab("diff")}
                  >
                    Diff
                  </button>
                  <button
                    type="button"
                    className={`drawer-tab${fileTab === "edit" ? " active" : ""}`}
                    disabled={Boolean(preview?.readonly || preview?.binary)}
                    onClick={() => setDrawerFileTab("edit")}
                  >
                    轻编辑{editDirty ? " *" : ""}
                  </button>
                </div>
                <div style={{ display: "flex", gap: 4 }}>
                  {fileTab === "edit" && (
                    <button
                      type="button"
                      className="btn btn-accent btn-sm"
                      disabled={saving || !editDirty}
                      onClick={() => void saveDrawerFile(false)}
                    >
                      {saving ? "保存中…" : "保存"}
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    onClick={() => void openExternalPath(activeFile, preferredEditor?.id)}
                    title={preferredEditor ? `用 ${preferredEditor.label} 打开` : "在资源管理器显示"}
                  >
                    外部打开
                  </button>
                </div>
              </div>

              {conflictMessage && (
                <div
                  style={{
                    padding: "8px 10px",
                    background: "var(--badge-warn)",
                    color: "var(--warn)",
                    fontSize: 12,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 8,
                  }}
                >
                  <span>{conflictMessage}</span>
                  <div style={{ display: "flex", gap: 4 }}>
                    <button
                      type="button"
                      className="btn btn-sm"
                      onClick={() => void openFileInDrawer(activeFile, { tab: "edit" })}
                    >
                      重新加载
                    </button>
                    <button
                      type="button"
                      className="btn btn-danger btn-sm"
                      onClick={() => void saveDrawerFile(true)}
                    >
                      强制覆盖
                    </button>
                  </div>
                </div>
              )}

              {loadingFile ? (
                <div className="empty-state">
                  <p>正在读取 {activeFile}…</p>
                </div>
              ) : error ? (
                <div className="empty-state">
                  <p style={{ color: "var(--err)" }}>{error}</p>
                </div>
              ) : fileTab === "edit" ? (
                <textarea
                  className="drawer-editor"
                  value={editContent}
                  onChange={(e) => setDrawerEditContent(e.target.value)}
                  spellCheck={false}
                />
              ) : fileTab === "diff" ? (
                <div className="drawer-code-view">
                  {diff?.diff ? (
                    diff.diff.split("\n").map((line, i) => (
                      <div
                        key={i}
                        className={
                          line.startsWith("+") && !line.startsWith("+++")
                            ? "diff-line-add"
                            : line.startsWith("-") && !line.startsWith("---")
                            ? "diff-line-del"
                            : line.startsWith("@@")
                            ? "diff-line-hunk"
                            : ""
                        }
                        style={{ whiteSpace: "pre" }}
                      >
                        {line}
                      </div>
                    ))
                  ) : (
                    <div style={{ color: "var(--dim)" }}>
                      {diff?.message ?? "该文件相对 HEAD 暂无文本差异"}
                    </div>
                  )}
                </div>
              ) : (
                <div className="drawer-code-view">
                  {preview?.binary ? (
                    <div style={{ color: "var(--dim)" }}>二进制文件不支持文本预览，请使用外部程序打开。</div>
                  ) : (
                    (preview?.content ?? "").split("\n").map((line, idx) => {
                      const no = idx + 1;
                      return (
                        <div
                          key={no}
                          className={`code-line${highlightLine === no ? " highlight" : ""}`}
                        >
                          <span className="code-line-no">{no}</span>
                          <span>{line}</span>
                        </div>
                      );
                    })
                  )}
                </div>
              )}
            </>
          ) : (
            <div className="empty-state">
              <p>选择上方文件以查看源码、Diff 或进行轻量修改。</p>
            </div>
          )}
        </div>
      ) : mode === "worktrees" ? (
        <div className="drawer-body" style={{ padding: 12, gap: 10, overflowY: "auto" }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <span className="field-label">并行 Git 工作树 ({worktrees.length})</span>
            <div style={{ display: "flex", gap: 6 }}>
              <button type="button" className="btn btn-sm" onClick={loadWorktrees} disabled={worktreeBusy}>
                刷新
              </button>
              <button
                type="button"
                className="btn btn-sm btn-primary"
                onClick={() => void handleCreateWorktree()}
                disabled={worktreeBusy}
              >
                + 新建工作树
              </button>
            </div>
          </div>
          {worktrees.length === 0 ? (
            <div className="empty-state">
              <p>当前项目没有独立的并行工作树。新建工作树可在隔离分支上并行开发并预览合并。</p>
            </div>
          ) : (
            worktrees.map((wt) => (
              <div key={wt.path} className="provider-card">
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 6 }}>
                  <strong className="mono">{wt.branch}</strong>
                  <span className={`badge ${wt.clean ? "badge-ok" : "badge-warn"}`}>
                    {wt.clean ? "干净" : `${wt.files.length} 改动`}
                  </span>
                </div>
                <div className="project-card-path">{wt.path}</div>
                <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                  <button
                    type="button"
                    className="btn btn-sm"
                    onClick={() =>
                      openViewer({
                        title: `工作树差异：${wt.branch}`,
                        subtitle: wt.path,
                        content: wt.diff || "该工作树相对主分支暂无差异",
                        kind: "text",
                      })
                    }
                  >
                    查看差异
                  </button>
                  <button
                    type="button"
                    className="btn btn-sm btn-primary"
                    disabled={worktreeBusy}
                    onClick={() => void handleMergeWorktree(wt)}
                  >
                    预检并合并
                  </button>
                  <button
                    type="button"
                    className="btn btn-sm btn-danger"
                    disabled={worktreeBusy}
                    onClick={() => void handleDiscardWorktree(wt)}
                  >
                    放弃
                  </button>
                </div>
              </div>
            ))
          )}
        </div>
      ) : (
        <div className="drawer-body" style={{ padding: 12, gap: 12, overflowY: "auto" }}>
          <div>
            <div className="field-label" style={{ marginBottom: 6 }}>
              子代理活动 ({liveSubagents.length})
            </div>
            {liveSubagents.length === 0 ? (
              <div className="field-hint">当前会话没有活跃或近期的子代理任务。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {liveSubagents.map((sub) => (
                  <div key={sub.id} className="provider-card">
                    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                      <strong>{sub.agent ?? sub.label}</strong>
                      <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                        <span className={`badge ${sub.status === "running" ? "badge-accent" : "badge-ok"}`}>
                          {sub.status}
                        </span>
                        {sub.status === "running" && (
                          <button
                            type="button"
                            className="btn btn-danger btn-sm"
                            onClick={() => void stopSubagent(sub.id)}
                          >
                            取消
                          </button>
                        )}
                      </div>
                    </div>
                    {sub.progress.length > 0 && (
                      <pre className="tool-pre">{sub.progress.slice(-4).join("\n")}</pre>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>

          <div>
            <div className="field-label" style={{ marginBottom: 6 }}>
              后台终端进程 ({terminals.length})
            </div>
            {terminals.length === 0 ? (
              <div className="field-hint">暂无运行中的后台终端进程。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {terminals.map((term) => (
                  <div key={term.id} className="provider-card">
                    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                      <span className="mono" style={{ fontSize: 12, overflow: "hidden", textOverflow: "ellipsis" }}>
                        {term.command ?? term.id}
                      </span>
                      <button
                        type="button"
                        className="btn btn-danger btn-sm"
                        onClick={() => void stopTerminal(term.id)}
                      >
                        停止
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div>
            <div className="field-label" style={{ marginBottom: 6 }}>
              本轮工具轨迹 ({liveTools.length})
            </div>
            {liveTools.length === 0 ? (
              <div className="field-hint">本轮暂无工具调用。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                {liveTools.map((t) => (
                  <div
                    key={t.id}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      fontSize: 12,
                      padding: "4px 6px",
                      borderRadius: 4,
                      background: "var(--panel)",
                    }}
                  >
                    <span className="mono">{t.name}</span>
                    <span style={{ color: "var(--dim)" }}>{t.done ? (t.ok ? "成功" : "失败") : "运行中"}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </aside>
  );
}
