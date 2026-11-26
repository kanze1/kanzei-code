import React, { useEffect, useRef, useState } from "react";
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
import {
  IconClose,
  IconExternal,
  IconFileCode,
  IconPlus,
  IconRefresh,
} from "../../components/Icons";

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

function splitFilePath(full: string): { name: string; dir: string } {
  const norm = full.replace(/\\/g, "/");
  const idx = norm.lastIndexOf("/");
  if (idx < 0) return { name: norm, dir: "" };
  return { name: norm.slice(idx + 1), dir: norm.slice(0, idx) };
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
  const codeViewRef = useRef<HTMLDivElement>(null);

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

  useEffect(() => {
    if (!open || mode !== "files" || fileTab !== "preview" || !highlightLine) return;
    const target = codeViewRef.current?.querySelector(".code-line.highlight");
    target?.scrollIntoView({ block: "center" });
  }, [open, mode, fileTab, highlightLine, preview?.path]);

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
  const mergedFiles = Array.from(new Set([...gitFiles.map((f) => f.path), ...touchedFiles]));

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
        <div className="drawer-main-tabs" aria-label="抽屉面板切换">
          <button
            type="button"
            aria-pressed={mode === "files"}
            className={`drawer-main-tab${mode === "files" ? " active" : ""}`}
            onClick={() => drawerStore.setState({ mode: "files" })}
          >
            <span>文件</span>
            <span className="drawer-tab-count">{mergedFiles.length}</span>
          </button>
          <button
            type="button"
            aria-pressed={mode === "activity"}
            className={`drawer-main-tab${mode === "activity" ? " active" : ""}`}
            onClick={() => drawerStore.setState({ mode: "activity" })}
          >
            <span>活动</span>
            <span className="drawer-tab-count">{liveSubagents.length + terminals.length}</span>
          </button>
          {!generalMode && (
            <button
              type="button"
              aria-pressed={mode === "worktrees"}
              className={`drawer-main-tab${mode === "worktrees" ? " active" : ""}`}
              onClick={() => drawerStore.setState({ mode: "worktrees" })}
            >
              <span>工作树</span>
              <span className="drawer-tab-count">{worktrees.length}</span>
            </button>
          )}
        </div>
        <button
          type="button"
          className="btn btn-ghost btn-icon btn-sm"
          onClick={closeDrawer}
          aria-label="关闭抽屉"
          title="关闭抽屉"
        >
          <IconClose size={13} />
        </button>
      </div>

      {mode === "files" ? (
        <div className="drawer-body">
          <div className="drawer-path-bar">
            <input
              className="input mono"
              placeholder="输入相对路径回车打开，如 src/main.rs"
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
              className="btn btn-icon"
              title="刷新 Git 文件状态"
              aria-label="刷新文件状态"
              onClick={() => projectDir && void refreshGitStatus(projectDir, worktreePath)}
            >
              <IconRefresh size={13} />
            </button>
          </div>

          {mergedFiles.length > 0 && (
            <div className="drawer-file-summary">
              <span>变更与触及 ({mergedFiles.length})</span>
              {gitStatus && (gitStatus.additions > 0 || gitStatus.deletions > 0) && (
                <span className="mono">
                  <span style={{ color: "var(--diff-add)" }}>+{gitStatus.additions}</span>{" "}
                  <span style={{ color: "var(--diff-del)" }}>-{gitStatus.deletions}</span>
                </span>
              )}
            </div>
          )}

          <div className="drawer-file-list">
            {mergedFiles.length === 0 ? (
              <div className="field-hint" style={{ padding: 10 }}>
                本轮暂无触及或未提交变动的文件。点击对话中的文件路径链接或上方输入路径即可查看。
              </div>
            ) : (
              mergedFiles.map((file) => {
                const stat = gitFiles.find((f) => f.path === file);
                const { name, dir } = splitFilePath(file);
                return (
                  <button
                    key={file}
                    type="button"
                    className={`drawer-file-item${activeFile === file ? " active" : ""}`}
                    onClick={() => void openFileInDrawer(file)}
                    title={file}
                  >
                    <span className="drawer-file-name-wrap">
                      <span className="drawer-file-basename">{name}</span>
                      {dir && <span className="drawer-file-dir">{dir}</span>}
                    </span>
                    {stat && (
                      <span className="drawer-file-stats">
                        {stat.untracked ? (
                          <span className="badge badge-ok">新增</span>
                        ) : (
                          <>
                            <span style={{ color: "var(--diff-add)" }}>+{stat.additions}</span>
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
              <div className="drawer-subbar">
                <div className="segmented" aria-label="文件查看模式">
                  <button
                    type="button"
                    aria-pressed={fileTab === "preview"}
                    className={`segmented-btn${fileTab === "preview" ? " active" : ""}`}
                    onClick={() => setDrawerFileTab("preview")}
                  >
                    原文
                  </button>
                  <button
                    type="button"
                    aria-pressed={fileTab === "diff"}
                    className={`segmented-btn${fileTab === "diff" ? " active" : ""}`}
                    onClick={() => setDrawerFileTab("diff")}
                  >
                    Diff
                  </button>
                  <button
                    type="button"
                    aria-pressed={fileTab === "edit"}
                    className={`segmented-btn${fileTab === "edit" ? " active" : ""}`}
                    disabled={Boolean(preview?.readonly || preview?.binary)}
                    onClick={() => setDrawerFileTab("edit")}
                  >
                    轻编辑{editDirty ? " •" : ""}
                  </button>
                </div>

                <div className="drawer-subbar-actions">
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
                    <IconExternal size={12} />
                    <span>外部打开</span>
                  </button>
                </div>
              </div>

              {conflictMessage && (
                <div className="drawer-conflict-bar">
                  <span>{conflictMessage}</span>
                  <div style={{ display: "flex", gap: 4, flex: "none" }}>
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
                    <div className="drawer-code-inner">
                      {diff.diff.split("\n").map((line, i) => (
                        <div
                          key={i}
                          className={`diff-line ${
                            line.startsWith("+") && !line.startsWith("+++")
                              ? "diff-line-add"
                              : line.startsWith("-") && !line.startsWith("---")
                              ? "diff-line-del"
                              : line.startsWith("@@")
                              ? "diff-line-hunk"
                              : ""
                          }`}
                        >
                          {line || " "}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="empty-state" style={{ padding: "28px 16px" }}>
                      <p>{diff?.message ?? "该文件相对 HEAD 暂无文本差异"}</p>
                    </div>
                  )}
                </div>
              ) : (
                <div ref={codeViewRef} className="drawer-code-view">
                  {preview?.binary ? (
                    <div className="empty-state">
                      <p>二进制文件不支持文本预览，请使用外部程序打开。</p>
                    </div>
                  ) : (
                    <div className="drawer-code-inner">
                      {(preview?.content ?? "").split("\n").map((line, idx) => {
                        const no = idx + 1;
                        return (
                          <div
                            key={no}
                            className={`code-line${highlightLine === no ? " highlight" : ""}`}
                          >
                            <span className="code-line-no">{no}</span>
                            <span>{line || " "}</span>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              )}
            </>
          ) : (
            <div className="empty-state" style={{ flex: 1 }}>
              <div className="empty-state-icon">
                <IconFileCode size={18} />
              </div>
              <p>选择上方文件以查看源码、Diff 或进行轻量修改。</p>
            </div>
          )}
        </div>
      ) : mode === "worktrees" ? (
        <div className="drawer-scroll-body">
          <div className="drawer-section-head">
            <span className="field-label">并行 Git 工作树 ({worktrees.length})</span>
            <div style={{ display: "flex", gap: 6 }}>
              <button
                type="button"
                className="btn btn-icon btn-sm"
                title="刷新工作树列表"
                onClick={loadWorktrees}
                disabled={worktreeBusy}
              >
                <IconRefresh size={12} />
              </button>
              <button
                type="button"
                className="btn btn-sm btn-primary"
                onClick={() => void handleCreateWorktree()}
                disabled={worktreeBusy}
              >
                <IconPlus size={12} />
                <span>新建</span>
              </button>
            </div>
          </div>
          {worktrees.length === 0 ? (
            <div className="empty-state">
              <p>当前项目没有独立的并行工作树。新建工作树可在隔离分支上并行开发并预览合并。</p>
            </div>
          ) : (
            worktrees.map((wt) => (
              <div key={wt.path} className="drawer-card">
                <div className="drawer-card-head">
                  <strong className="mono" style={{ fontSize: 12.5 }}>{wt.branch}</strong>
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
        <div className="drawer-scroll-body">
          <div>
            <div className="drawer-section-head">
              <span className="field-label">子代理活动 ({liveSubagents.length})</span>
            </div>
            {liveSubagents.length === 0 ? (
              <div className="field-hint">当前会话没有活跃或近期的子代理任务。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {liveSubagents.map((sub) => (
                  <div key={sub.id} className="drawer-card">
                    <div className="drawer-card-head">
                      <strong style={{ fontSize: 12.5 }}>{sub.agent ?? sub.label}</strong>
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
            <div className="drawer-section-head">
              <span className="field-label">后台终端进程 ({terminals.length})</span>
            </div>
            {terminals.length === 0 ? (
              <div className="field-hint">暂无运行中的后台终端进程。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {terminals.map((term) => (
                  <div key={term.id} className="drawer-card">
                    <div className="drawer-card-head">
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
            <div className="drawer-section-head">
              <span className="field-label">本轮工具轨迹 ({liveTools.length})</span>
            </div>
            {liveTools.length === 0 ? (
              <div className="field-hint">本轮暂无工具调用。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
                {liveTools.map((t) => (
                  <div key={t.id} className="drawer-trace-row">
                    <span className="mono">{t.name}</span>
                    <span className={`badge ${!t.done ? "badge-accent" : t.ok ? "badge-ok" : "badge-err"}`}>
                      {t.done ? (t.ok ? "成功" : "失败") : "运行中"}
                    </span>
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
