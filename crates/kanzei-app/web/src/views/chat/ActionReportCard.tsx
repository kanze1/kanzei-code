import React, { useMemo, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import type { ActionReportData, ChatMessage, MessagePart } from "../../ipc/types";
import { projectDisplayName, useAppStore } from "../../store/appStore";
import { openFileInDrawer, useDrawerStore } from "../../store/drawerStore";
import { useSessionStore } from "../../store/sessionStore";
import { openViewer, pushToast } from "../../store/uiStore";
import {
  IconActivity,
  IconCopy,
  IconDownload,
  IconFileCode,
  IconShield,
} from "../../components/Icons";

interface GateStepResult {
  name: string;
  ok: boolean;
  summary: string;
}

interface ActionReportCardProps {
  messages: ChatMessage[];
  toolResultMap: Map<string, MessagePart>;
  report: ActionReportData | null;
}

function buildFallbackReport(
  messages: ChatMessage[],
  toolResultMap: Map<string, MessagePart>,
  touchedFiles: string[]
): ActionReportData | null {
  let lastUserIdx = -1;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i].role === "user") {
      lastUserIdx = i;
      break;
    }
  }
  const tail = lastUserIdx >= 0 ? messages.slice(lastUserIdx + 1) : messages;
  const assistantTail = tail.filter((m) => m.role === "assistant");
  if (assistantTail.length === 0) return null;

  let toolsTotal = 0;
  let toolsFailed = 0;
  const toolNames = new Set<string>();
  const filesFromTools = new Set<string>(touchedFiles);

  for (const msg of assistantTail) {
    for (const part of msg.parts ?? []) {
      if (part.type === "tool_use" && typeof part.name === "string") {
        toolsTotal += 1;
        toolNames.add(part.name);
        const inp = part.input as Record<string, unknown> | undefined;
        if (inp) {
          for (const key of ["path", "file_path", "target_file"]) {
            const val = inp[key];
            if (typeof val === "string" && val.trim()) {
              filesFromTools.add(val.trim());
            }
          }
        }
        if (typeof part.id === "string") {
          const res = toolResultMap.get(part.id);
          if (res?.is_error) toolsFailed += 1;
        }
      }
    }
  }

  if (toolsTotal === 0 && filesFromTools.size === 0) {
    return null;
  }

  return {
    sessionId: "",
    completedAt: 0,
    elapsedMs: null,
    inputTokens: 0,
    outputTokens: 0,
    toolsTotal,
    toolsFailed,
    toolNames: Array.from(toolNames),
    touchedFiles: Array.from(filesFromTools),
  };
}

export function ActionReportCard({
  messages,
  toolResultMap,
  report,
}: ActionReportCardProps): React.ReactElement | null {
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const gitStatus = useDrawerStore((s) => s.gitStatus);
  const touchedFiles = useDrawerStore((s) => s.touchedFiles);
  const activeProcess = useSessionStore((s) =>
    s.processes.find((p) => p.id === s.activeProcessId) ?? null
  );
  const [expanded, setExpanded] = useState(true);
  const [gateBusy, setGateBusy] = useState(false);
  const [gateSteps, setGateSteps] = useState<GateStepResult[] | null>(null);

  const effective = useMemo(() => {
    const fallback = buildFallbackReport(messages, toolResultMap, touchedFiles);
    if (!report && !fallback) return null;
    if (!report) return fallback;
    const mergedFiles = Array.from(
      new Set([...(report.touchedFiles ?? []), ...(fallback?.touchedFiles ?? []), ...touchedFiles])
    );
    return {
      ...report,
      toolsTotal: report.toolsTotal || fallback?.toolsTotal || 0,
      toolsFailed: report.toolsFailed || fallback?.toolsFailed || 0,
      toolNames:
        report.toolNames.length > 0 ? report.toolNames : fallback?.toolNames ?? [],
      touchedFiles: mergedFiles,
    };
  }, [report, messages, toolResultMap, touchedFiles]);

  const changedFiles = useMemo(() => {
    const gitFiles = gitStatus?.files ?? [];
    if (gitFiles.length > 0) return gitFiles;
    return (effective?.touchedFiles ?? []).map((p) => ({
      path: p,
      additions: 0,
      deletions: 0,
    }));
  }, [gitStatus?.files, effective?.touchedFiles]);

  if (!effective && changedFiles.length === 0) return null;

  const toolsTotal = effective?.toolsTotal ?? 0;
  const toolsFailed = effective?.toolsFailed ?? 0;
  const toolsPassed = Math.max(0, toolsTotal - toolsFailed);
  const additions = gitStatus?.additions ?? 0;
  const deletions = gitStatus?.deletions ?? 0;

  const buildMarkdownReport = (): string => {
    const lines = [
      `# 交付验收报告 · ${activeProcess?.label ?? projectDisplayName(projectDir)}`,
      "",
      `- **生成时间**：${new Date().toLocaleString()}`,
      `- **工作目录**：\`${activeProcess?.worktree_path ?? projectDir ?? "-"}\``,
      activeProcess?.branch ? `- **分支**：\`${activeProcess.branch}\`` : "",
      effective?.elapsedMs ? `- **本轮耗时**：${(effective.elapsedMs / 1000).toFixed(1)}s` : "",
      effective && (effective.inputTokens > 0 || effective.outputTokens > 0)
        ? `- **本轮 Token**：In ${effective.inputTokens.toLocaleString()} / Out ${effective.outputTokens.toLocaleString()}`
        : "",
      `- **工具执行**：共 ${toolsTotal} 次（成功 ${toolsPassed} / 失败 ${toolsFailed}）`,
      `- **文件变更**：${changedFiles.length} 个文件（+${additions} / -${deletions}）`,
      "",
      "## 变更文件清单",
      ...(changedFiles.length > 0
        ? changedFiles.map(
            (f) =>
              `- \`${f.path}\`${
                f.additions || f.deletions ? ` (+${f.additions} / -${f.deletions})` : ""
              }`
          )
        : ["- （当前工作树无未提交代码改动）"]),
      "",
      "## 验证门禁状态",
      ...(gateSteps && gateSteps.length > 0
        ? gateSteps.map((s) => `- [${s.ok ? "x" : " "}] **${s.name}**：${s.summary}`)
        : ["- [ ] 尚未在验收单内运行代码门禁"]),
    ].filter((line) => line !== "");
    return `${lines.join("\n")}\n`;
  };

  const handleRunGate = async () => {
    if (!projectDir || generalMode) return;
    setGateBusy(true);
    try {
      const steps = activeProcess?.worktree_path
        ? await invoke<GateStepResult[]>("worktree_gate", {
            projectDir,
            worktreePath: activeProcess.worktree_path,
          })
        : await invoke<GateStepResult[]>("worktree_post_merge_gate", { projectDir });
      const list = Array.isArray(steps) ? steps : [];
      setGateSteps(list);
      const failed = list.filter((s) => !s.ok).length;
      pushToast(
        failed === 0
          ? `验收门禁全部通过（${list.length} 项）`
          : `验收门禁有 ${failed}/${list.length} 项未通过`,
        failed === 0 ? "ok" : "warn"
      );
    } catch (err) {
      pushToast(`运行代码门禁失败：${formatError(err)}`, "error");
    } finally {
      setGateBusy(false);
    }
  };

  const handleCopyMarkdown = async () => {
    try {
      await navigator.clipboard.writeText(buildMarkdownReport());
      pushToast("已复制 Markdown 交付验收单", "ok", 1800);
    } catch {
      pushToast("复制失败，请检查剪贴板权限", "warn");
    }
  };

  const handleExportMarkdown = async () => {
    try {
      const saved = await invoke<string | null>("save_conversation_export", {
        name: `action-report-${activeProcess?.label ?? "task"}`,
        content: buildMarkdownReport(),
      });
      if (saved) pushToast(`已导出交付报告到 ${saved}`, "ok");
    } catch (err) {
      pushToast(`导出交付报告失败：${formatError(err)}`, "error");
    }
  };

  return (
    <section className="action-report-card" aria-label="本轮交付验收单">
      <div className="action-report-head">
        <button
          type="button"
          className="action-report-toggle"
          onClick={() => setExpanded((v) => !v)}
        >
          <IconShield size={13} />
          <strong>交付验收单（Action Report）</strong>
          <span className="badge">{changedFiles.length} 文件</span>
          {(additions > 0 || deletions > 0) && (
            <span className="mono action-report-diff-stat">
              <span className="diff-stat-add">+{additions}</span>{" "}
              <span className="diff-stat-del">-{deletions}</span>
            </span>
          )}
          {toolsTotal > 0 && (
            <span className={`badge ${toolsFailed > 0 ? "badge-warn" : "badge-ok"}`}>
              工具 {toolsPassed}/{toolsTotal}
            </span>
          )}
          {effective?.elapsedMs ? (
            <span className="field-hint mono">{(effective.elapsedMs / 1000).toFixed(1)}s</span>
          ) : null}
        </button>

        <div className="action-report-actions">
          {!generalMode && projectDir && (
            <button
              type="button"
              className="btn btn-sm btn-accent"
              disabled={gateBusy}
              onClick={() => void handleRunGate()}
            >
              <IconActivity size={12} />
              <span>{gateBusy ? "门禁检查中…" : "运行门禁"}</span>
            </button>
          )}
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => void handleCopyMarkdown()}
          >
            <IconCopy size={12} />
            <span>复制报告</span>
          </button>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => void handleExportMarkdown()}
          >
            <IconDownload size={12} />
            <span>导出</span>
          </button>
        </div>
      </div>

      {expanded && (
        <div className="action-report-body">
          {changedFiles.length > 0 && (
            <div className="action-report-files">
              {changedFiles.slice(0, 12).map((f) => (
                <button
                  key={f.path}
                  type="button"
                  className="action-report-file-chip mono"
                  onClick={() => void openFileInDrawer(f.path, { tab: "diff" })}
                  title={`点击查看 ${f.path} 的 Diff`}
                >
                  <IconFileCode size={11} />
                  <span>{f.path}</span>
                  {(f.additions > 0 || f.deletions > 0) && (
                    <span className="field-hint">
                      +{f.additions}/-{f.deletions}
                    </span>
                  )}
                </button>
              ))}
              {changedFiles.length > 12 && (
                <span className="field-hint">+{changedFiles.length - 12} 个文件</span>
              )}
            </div>
          )}

          {effective?.handoffSummary && (
            <div className="field-hint">完成声明：{effective.handoffSummary}</div>
          )}

          {gateSteps && gateSteps.length > 0 && (
            <div className="action-report-gates">
              {gateSteps.map((step, i) => (
                <button
                  key={i}
                  type="button"
                  className={`badge ${step.ok ? "badge-ok" : "badge-err"}`}
                  style={{ cursor: "pointer" }}
                  onClick={() =>
                    openViewer({
                      title: `门禁步骤：${step.name}`,
                      subtitle: step.ok ? "通过" : "未通过",
                      content: step.summary || "(无详情)",
                      kind: "text",
                    })
                  }
                >
                  {step.ok ? "✓" : "✗"} {step.name}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
