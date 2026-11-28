import { modifiedPaths, normalizeRelPath } from "../../store/roundFiles";
import { ControlPopover } from "../../components/ControlPopover";
import React, { useMemo, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import type {
  ActionReportData,
  ChatMessage,
  GitStatusSummary,
  MessagePart,
} from "../../ipc/types";
import { projectDisplayName, useAppStore } from "../../store/appStore";
import { openFileInDrawer, toggleDrawer, useDrawerStore } from "../../store/drawerStore";
import { useSessionStore } from "../../store/sessionStore";
import { openViewer, pushToast } from "../../store/uiStore";
import {
  IconActivity,
  IconCopy,
  IconDownload,
  IconFileCode,
  IconShield,
} from "../../components/Icons";

export interface GateStepResult {
  name: string;
  ok: boolean;
  summary: string;
}

export type GateVerdict =
  | { state: "not_run"; label: string; detail: string }
  | { state: "not_executed"; label: string; detail: string }
  | { state: "passed"; total: number; label: string; detail: string }
  | { state: "failed"; failed: number; total: number; label: string; detail: string };

interface ActionReportCardProps {
  messages: ChatMessage[];
  toolResultMap: Map<string, MessagePart>;
  report: ActionReportData | null;
}

export function evaluateGateVerdict(steps: GateStepResult[] | null): GateVerdict {
  if (steps === null) {
    return {
      state: "not_run",
      label: "验证未执行",
      detail: "尚未在验收单内运行代码门禁",
    };
  }
  if (steps.length === 0) {
    return {
      state: "not_executed",
      label: "验证未执行（0 项）",
      detail: "验证未执行：当前项目无适用门禁检查（0 项）",
    };
  }
  const failed = steps.filter((s) => !s.ok).length;
  if (failed === 0) {
    return {
      state: "passed",
      total: steps.length,
      label: `验证通过（${steps.length} 项）`,
      detail: `验收门禁全部通过（${steps.length} 项）`,
    };
  }
  return {
    state: "failed",
    failed,
    total: steps.length,
    label: `验证失败（${failed}/${steps.length}）`,
    detail: `验收门禁有 ${failed}/${steps.length} 项未通过`,
  };
}

export function buildRoundReportSummary(
  messages: ChatMessage[],
  toolResultMap: Map<string, MessagePart>,
  report: ActionReportData | null,
  gitStatus: GitStatusSummary | null
): {
  effective: ActionReportData | null;
  roundFiles: Array<{ path: string }>;
  otherGitFilesCount: number;
  shouldDisplay: boolean;
} {
  let lastUserIdx = -1;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i].role === "user" && messages[i].parts.some(part => part.type === "text" || part.type === "image")) {
      lastUserIdx = i;
      break;
    }
  }
  const tail = lastUserIdx >= 0 ? messages.slice(lastUserIdx + 1) : messages;
  const assistantTail = tail.filter((m) => m.role === "assistant");

  let fallbackToolsTotal = 0;
  let fallbackToolsFailed = 0;
  const fallbackToolNames = new Set<string>();
  const fallbackModified = new Set<string>();

  for (const msg of assistantTail) {
    for (const part of msg.parts ?? []) {
      if (part.type === "tool_use" && typeof part.name === "string") {
        fallbackToolsTotal += 1;
        fallbackToolNames.add(part.name);
        const res = typeof part.id === "string" ? toolResultMap.get(part.id) : undefined;
        const isError = Boolean(res?.is_error);
        if (isError) fallbackToolsFailed += 1;
        if (res && !isError) {
          for (const path of modifiedPaths(part.name, part.input)) fallbackModified.add(path);
        }
      }
    }
  }

  const fallback: ActionReportData | null =
    fallbackToolsTotal > 0 || fallbackModified.size > 0
      ? {
          sessionId: "",
          completedAt: 0,
          elapsedMs: null,
          inputTokens: 0,
          outputTokens: 0,
          toolsTotal: fallbackToolsTotal,
          toolsFailed: fallbackToolsFailed,
          toolNames: Array.from(fallbackToolNames),
          touchedFiles: Array.from(fallbackModified),
          modifiedFiles: Array.from(fallbackModified),
        }
      : null;

  const effective: ActionReportData | null = report
    ? {
        ...report,
        toolNames:
          report.toolNames.length > 0 ? report.toolNames : fallback?.toolNames ?? [],
        modifiedFiles:
          report.modifiedFiles !== undefined
            ? report.modifiedFiles.map(normalizeRelPath).filter(Boolean)
            : fallback?.modifiedFiles && fallback.modifiedFiles.length > 0
            ? fallback.modifiedFiles
            : [],
      }
    : fallback;

  const roundPaths = Array.from(new Set(effective?.modifiedFiles ?? []));
  const roundFiles = roundPaths.map((p) => ({ path: p }));
  const roundPathSet = new Set(roundPaths);
  const otherGitFilesCount = (gitStatus?.files ?? []).filter(
    (f) => !roundPathSet.has(normalizeRelPath(f.path))
  ).length;

  const shouldDisplay =
    roundFiles.length > 0 ||
    (effective?.toolsFailed ?? 0) > 0 ||
    Boolean(effective?.handoffSummary);

  return {
    effective,
    roundFiles,
    otherGitFilesCount,
    shouldDisplay,
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
  const activeProcess = useSessionStore((s) =>
    s.processes.find((p) => p.id === s.activeProcessId) ?? null
  );
  const [userExpanded, setExpanded] = useState<boolean | null>(null);
  const [gateError, setGateError] = useState<string | null>(null);
  const [gateBusy, setGateBusy] = useState(false);
  const [gateSteps, setGateSteps] = useState<GateStepResult[] | null>(null);

  const summary = useMemo(
    () => buildRoundReportSummary(messages, toolResultMap, report, gitStatus),
    [messages, toolResultMap, report, gitStatus]
  );

  if (!summary.shouldDisplay && gateSteps === null) return null;

  const { effective, roundFiles, otherGitFilesCount } = summary;
  const toolsTotal = effective?.toolsTotal ?? 0;
  const toolsFailed = effective?.toolsFailed ?? 0;
  const toolsPassed = Math.max(0, toolsTotal - toolsFailed);
  const gateVerdict = gateError
    ? { state: "failed", label: "门禁运行失败", detail: gateError }
    : evaluateGateVerdict(gateSteps);

  const expanded = userExpanded ?? (toolsFailed > 0 || gateVerdict.state === "failed");

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
      `- **本轮文件修改**：${roundFiles.length} 个文件`,
      `- **验证结论**：${gateVerdict.label}`,
      "",
      "## 本轮修改文件清单",
      ...(roundFiles.length > 0
        ? roundFiles.map((f) => `- \`${f.path}\``)
        : ["- （本轮未修改文件）"]),
      "",
      "## 验证门禁状态",
      ...(gateSteps && gateSteps.length > 0
        ? gateSteps.map((s) => `- [${s.ok ? "x" : " "}] **${s.name}**：${s.summary}`)
        : [`- [ ] ${gateVerdict.detail}`]),
    ].filter((line) => line !== "");
    return `${lines.join("\n")}\n`;
  };

  const handleRunGate = async () => {
    if (!projectDir || generalMode) return;
    setGateBusy(true);
    setGateError(null);
    try {
      const steps = activeProcess?.worktree_path
        ? await invoke<GateStepResult[]>("worktree_gate", {
            projectDir,
            worktreePath: activeProcess.worktree_path,
          })
        : await invoke<GateStepResult[]>("worktree_post_merge_gate", { projectDir });
      const list = steps;
      if (!Array.isArray(list)) throw new Error("门禁结果格式错误");
      setGateSteps(list);
      if (list.some(step => !step.ok)) setExpanded(true);
      const verdict = evaluateGateVerdict(list);
      pushToast(
        verdict.detail,
        verdict.state === "passed" ? "ok" : verdict.state === "failed" ? "warn" : "info"
      );
    } catch (err) {
      setGateError(formatError(err));
      setGateSteps(null);
      setExpanded(true);
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

  const gateBadgeClass =
    gateVerdict.state === "passed"
      ? "badge-ok"
      : gateVerdict.state === "failed"
      ? "badge-err"
      : "badge-warn";

  return (
    <section className="action-report-card" aria-label="本轮交付验收单">
      <div className="action-report-head">
        <button
          type="button"
          className="action-report-toggle"
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
        >
          <IconShield size={13} />
          <strong>本轮交付</strong>
          <span className="badge">本轮 {roundFiles.length} 文件</span>
          {toolsTotal > 0 && (
            <span className={`badge ${toolsFailed > 0 ? "badge-warn" : "badge-ok"}`}>
              工具 {toolsPassed}/{toolsTotal}
            </span>
          )}
          <span className={`badge ${gateBadgeClass}`}>{gateVerdict.label}</span>
          {effective?.elapsedMs ? (
            <span className="field-hint mono">{(effective.elapsedMs / 1000).toFixed(1)}s</span>
          ) : null}
        </button>

        <div className="action-report-actions">
          {!generalMode && projectDir && (
            <button
              type="button"
              className="btn btn-sm"
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
          <ControlPopover label="更多交付操作" trigger="···">{close => (
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => { close(); void handleExportMarkdown(); }}
          >
            <IconDownload size={12} />
            <span>导出</span>
          </button>
          )}</ControlPopover>
        </div>
      </div>

      {expanded && (
        <div className="action-report-body">
          {roundFiles.length > 0 ? (
            <div className="action-report-files">
              {roundFiles.map((f) => (
                <button
                  key={f.path}
                  type="button"
                  className="chip-btn mono"
                  onClick={() => void openFileInDrawer(f.path, { tab: "diff" })}
                  title={`点击查看 ${f.path} 的 Diff`}
                >
                  <IconFileCode size={11} />
                  <span>{f.path}</span>
                </button>
              ))}
            </div>
          ) : (
            <div className="field-hint">本轮修改 0 个文件。</div>
          )}

          {otherGitFilesCount > 0 && !generalMode && (
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span className="field-hint">
                工作树另有 {otherGitFilesCount} 个非本轮未提交改动（未计入本轮交付）
              </span>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => toggleDrawer("files")}
              >
                查看工作树改动
              </button>
            </div>
          )}

          {effective?.handoffSummary && (
            <div className="field-hint">完成声明：{effective.handoffSummary}</div>
          )}

          {gateError && <div role="alert" className="error-text">{gateError}</div>}
          {gateSteps !== null && (
            <div className="action-report-gates">
              {gateSteps.length === 0 ? (
                <span className="field-hint">{gateVerdict.detail}</span>
              ) : (
                gateSteps.map((step, i) => (
                  <button
                    key={i}
                    type="button"
                    className={`chip-btn ${step.ok ? "badge-ok" : "badge-err"}`}
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
                ))
              )}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
