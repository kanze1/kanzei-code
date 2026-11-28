import { ChoiceGroup, ControlPopover } from "../../components/ControlPopover";
import { OutputPreview } from "./ActivityTimeline";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import type { PromptAttachment, QuickShellResult } from "../../ipc/types";
import {
  filterDirectModels,
  GENERAL_COMPOSER_PROFILES,
  MANUAL_MODEL_PATTERN,
  profileDisplayLabel,
  PROJECT_COMPOSER_PROFILES,
  REASONING_OPTIONS,
  reasoningShortLabel,
  sourceDisplayLabel,
  SUBAGENT_MODE_OPTIONS,
  withFallbackOption,
} from "../../configOptions";
import { loadModelsCatalog, useAppStore } from "../../store/appStore";
import {
  cancelPendingInput,
  chatStore,
  clearCurrentConversation,
  compactCurrentConversation,
  draftKeyFor,
  sendPrompt,
  stopCurrentRun,
  updateDraft,
  useChatStore,
} from "../../store/chatStore";
import { toggleDrawer } from "../../store/drawerStore";
import {
  updateAutoRun,
  updateProcessSettings,
  useSessionStore,
} from "../../store/sessionStore";
import {
  navigateView,
  openBtwOverlay,
  openProjectModelsDialog,
  openTraceDialog,
  openViewer,
  promptDialog,
  pushToast,
  triggerContextInspector,
} from "../../store/uiStore";
import {
  IconAttach,
  IconClose,
  IconCpu,
  IconSend,
  IconShield,
  IconSparkles,
  IconStop,
  IconTarget,
} from "../../components/Icons";

const MANUAL_MODEL_SENTINEL = "__manual__";

interface SuggestOption {
  id: string;
  label: string;
  hint?: string;
  kind: "file" | "command" | "skill";
  value: string;
}

interface SlashDef {
  cmd: string;
  desc: string;
}

const SLASH_COMMANDS: SlashDef[] = [
  { cmd: "/btw", desc: "旁路只读提问（共享上下文但不污染主历史）" },
  { cmd: "/grill-me", desc: "先对我进行需求与边界拷问，再出方案（防跑偏）" },
  { cmd: "/simplify", desc: "审查近期代码变更，精简冗余并提升可维护性" },
  { cmd: "/context", desc: "查看当前上下文窗口各模块占比拆解" },
  { cmd: "/trace", desc: "查看当前会话的执行轨迹与工具耗时" },
  { cmd: "/plan", desc: "切换到只读规划模式（先出方案不改代码）" },
  { cmd: "/exec", desc: "切换到自主开发模式（dev-auto）" },
  { cmd: "/pair", desc: "切换到结伴开发模式（dev-pair）" },
  { cmd: "/compact", desc: "压缩当前上下文（后可接保留重点）" },
  { cmd: "/clear", desc: "开启新的空白对话段" },
  { cmd: "/goal", desc: "设置或更新自动推进目标" },
  { cmd: "/model", desc: "打开本项目模型覆盖配置" },
  { cmd: "/files", desc: "打开右侧文件上下文与 Diff 抽屉" },
  { cmd: "/wiki", desc: "打开 Repo Wiki 仓库架构百科" },
  { cmd: "/settings", desc: "跳转到全局设置" },
];

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

function detectTrigger(
  value: string,
  cursor: number
): { kind: "file" | "command" | "skill"; start: number; query: string } | null {
  const before = value.slice(0, cursor);
  const slashMatch = before.match(/^\/([^\s]*)$/);
  if (slashMatch) {
    return { kind: "command", start: 0, query: slashMatch[1] };
  }
  const atMatch = before.match(/(?:^|\s)@([^\s@]*)$/);
  if (atMatch) {
    const query = atMatch[1];
    return { kind: "file", start: cursor - query.length - 1, query };
  }
  const skillMatch = before.match(/(?:^|\s)\$([^\s$]*)$/);
  if (skillMatch) {
    const query = skillMatch[1];
    return { kind: "skill", start: cursor - query.length - 1, query };
  }
  return null;
}

export function Composer(): React.ReactElement {
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const modelsCatalog = useAppStore((s) => s.modelsCatalog);
  const modelsLoading = useAppStore((s) => s.modelsLoading);
  const activeProcess = useSessionStore((s) =>
    s.processes.find((p) => p.id === s.activeProcessId) ?? null
  );
  const pendingInputs = useSessionStore((s) => s.pendingInputs);
  const effectiveModel = useSessionStore((s) => s.effectiveModel);
  const autoRun = useSessionStore((s) => s.autoRun);
  const autoAllow = useChatStore((s) => s.autoAllow);
  const deliveryMode = useChatStore((s) => s.deliveryMode);
  const selectedAgent = useChatStore((s) => s.selectedAgent);
  const promptHistory = useChatStore((s) => s.promptHistory);
  const messages = useChatStore((s) => s.messages);
  const drafts = useChatStore((s) => s.drafts);

  const key = draftKeyFor(activeProjectDir, activeProcess?.id ?? null);
  const storedDraft = drafts[key];

  const [text, setText] = useState(storedDraft?.text ?? "");
  const [attachments, setAttachments] = useState<PromptAttachment[]>(storedDraft?.attachments ?? []);
  const [dragOver, setDragOver] = useState(false);
  const [historyCursor, setHistoryCursor] = useState<number | null>(null);
  const [suggests, setSuggests] = useState<SuggestOption[]>([]);
  const [suggestIndex, setSuggestIndex] = useState(0);
  const [activeTrigger, setActiveTrigger] = useState<{
    kind: "file" | "command" | "skill";
    start: number;
    end: number;
  } | null>(null);
  const [skillsCache, setSkillsCache] = useState<
    Array<{ id: string; displayName: string; description: string; enabled: boolean }>
  >([]);
  const [primaryAgents, setPrimaryAgents] = useState<string[]>([]);
  const [quickShellRunning, setQuickShellRunning] = useState(false);
  const [quickShellResult, setQuickShellResult] = useState<QuickShellResult | null>(null);

  const fileInputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setText(storedDraft?.text ?? "");
    setAttachments(storedDraft?.attachments ?? []);
    setHistoryCursor(null);
  }, [key, storedDraft?.text]);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(200, Math.max(46, el.scrollHeight))}px`;
  }, [text]);

  useEffect(() => {
    if (!activeProjectDir) {
      setPrimaryAgents([]);
      return;
    }
    invoke<{
      agents?: Array<{ name: string; mode: string; status: string }>;
    }>("agent_directory_get", {
      projectDir: generalMode ? null : activeProjectDir,
      profile: activeProcess?.profile ?? null,
    })
      .then((res) => {
        const names = (res?.agents ?? [])
          .filter((a) => a.mode === "primary" && a.status === "available")
          .map((a) => a.name);
        setPrimaryAgents(Array.from(new Set(names)));
      })
      .catch(() => setPrimaryAgents([]));
  }, [activeProjectDir, generalMode, activeProcess?.profile]);

  const directModels = useMemo(() => filterDirectModels(modelsCatalog), [modelsCatalog]);

  const selectableModels = useMemo(() => {
    const seen = new Set<string>();
    const list: Array<{ id: string; label: string }> = [];
    const push = (id: string | null | undefined, label?: string) => {
      const clean = id?.trim();
      if (!clean || seen.has(clean)) return;
      seen.add(clean);
      list.push({ id: clean, label: label ?? clean });
    };
    if (activeProcess?.model) {
      push(activeProcess.model);
    }
    for (const m of directModels) {
      push(m.id, m.id);
    }
    for (const m of activeProcess?.manual_models ?? []) {
      push(m, `${m}（手填）`);
    }
    return list;
  }, [activeProcess?.model, activeProcess?.manual_models, directModels]);

  const refreshSuggestions = async (nextText: string, cursorPos: number) => {
    const trig = detectTrigger(nextText, cursorPos);
    if (!trig) {
      setActiveTrigger(null);
      setSuggests([]);
      return;
    }
    setActiveTrigger({ kind: trig.kind, start: trig.start, end: cursorPos });
    setSuggestIndex(0);

    if (trig.kind === "command") {
      const q = trig.query.toLowerCase();
      const matched = SLASH_COMMANDS.filter(
        (c) => !q || c.cmd.slice(1).includes(q) || c.desc.toLowerCase().includes(q)
      ).map<SuggestOption>((c) => ({
        id: c.cmd,
        label: c.cmd,
        hint: c.desc,
        kind: "command",
        value: c.cmd,
      }));
      setSuggests(matched);
      return;
    }

    if (trig.kind === "file") {
      if (!activeProjectDir || generalMode) {
        setSuggests([]);
        return;
      }
      try {
        const files = await invoke<string[]>("project_files", {
          projectDir: activeProjectDir,
          query: trig.query,
        });
        setSuggests(
          (Array.isArray(files) ? files : []).slice(0, 12).map((path) => ({
            id: path,
            label: path,
            kind: "file",
            value: `\`${path}\` `,
          }))
        );
      } catch {
        setSuggests([]);
      }
      return;
    }

    if (trig.kind === "skill") {
      let list = skillsCache;
      if (list.length === 0) {
        try {
          const loaded = await invoke<
            Array<{ id: string; displayName: string; description: string; enabled: boolean }>
          >("skills_list");
          list = Array.isArray(loaded) ? loaded : [];
          setSkillsCache(list);
        } catch {
          list = [];
        }
      }
      const q = trig.query.toLowerCase();
      setSuggests(
        list
          .filter(
            (s) =>
              s.enabled &&
              (!q ||
                s.id.toLowerCase().includes(q) ||
                s.displayName.toLowerCase().includes(q) ||
                s.description.toLowerCase().includes(q))
          )
          .slice(0, 10)
          .map((s) => ({
            id: s.id,
            label: `$${s.id}`,
            hint: s.displayName,
            kind: "skill",
            value: `$${s.id} `,
          }))
      );
    }
  };

  const handleTextChange = (next: string, cursorPos?: number) => {
    setText(next);
    setHistoryCursor(null);
    updateDraft(activeProjectDir, activeProcess?.id ?? null, next, attachments);
    void refreshSuggestions(next, cursorPos ?? next.length);
  };

  const executeSlashCommand = async (rawLine: string): Promise<boolean> => {
    const trimmed = rawLine.trim();
    if (!trimmed.startsWith("/")) return false;
    const [cmd, ...rest] = trimmed.split(/\s+/);
    const arg = rest.join(" ").trim();
    switch (cmd.toLowerCase()) {
      case "/btw":
        openBtwOverlay(arg);
        return true;
      case "/context":
        triggerContextInspector();
        return true;
      case "/trace":
        openTraceDialog();
        return true;
      case "/grill-me": {
        const topic = arg || "我接下来要做的需求或方案";
        const prompt = `请先不要修改任何代码。针对「${topic}」，请你用严格、系统的方式对我进行交互式需求拷问：指出模糊点、边界条件、架构取舍与潜在风险，每轮提出 3~5 个关键决策问题并给出你的推荐选项，直到我们彻底对齐后再进入实施。`;
        void sendPrompt(prompt, []);
        return true;
      }
      case "/simplify": {
        const focus = arg ? `（重点关注：${arg}）` : "";
        const prompt = `请先检查当前工作树的 git diff 与最近修改的文件${focus}，从代码复用性、简洁度、不必要的复杂度、潜在边界漏洞和可读性五个维度做一次严格自审，直接精简清理并验证原有测试全部通过。`;
        void sendPrompt(prompt, []);
        return true;
      }
      case "/plan":
        if (activeProjectDir && activeProcess && !generalMode) {
          await updateProcessSettings(activeProjectDir, activeProcess.id, { profile: "readonly" });
          pushToast("已切换到只读规划模式（Plan）", "ok");
        } else {
          pushToast("无项目对话不支持 readonly 规划档位", "warn");
        }
        return true;
      case "/exec":
        if (activeProjectDir && activeProcess) {
          await updateProcessSettings(activeProjectDir, activeProcess.id, { profile: "dev-auto" });
          pushToast("已切换到自主开发模式（dev-auto）", "ok");
        }
        return true;
      case "/pair":
        if (activeProjectDir && activeProcess) {
          await updateProcessSettings(activeProjectDir, activeProcess.id, { profile: "dev-pair" });
          pushToast("已切换到结伴开发模式（dev-pair）", "ok");
        }
        return true;
      case "/compact":
        await compactCurrentConversation(arg || null);
        return true;
      case "/clear":
        await clearCurrentConversation();
        return true;
      case "/goal":
        if (!generalMode) {
          if (arg) {
            await updateAutoRun({ goal: arg, enabled: true, paused: false });
            pushToast("已设置自动推进目标", "ok");
          } else {
            await handleEditGoal();
          }
        }
        return true;
      case "/model":
        if (!generalMode && activeProjectDir) openProjectModelsDialog(activeProjectDir);
        else navigateView("settings");
        return true;
      case "/files":
        if (!generalMode) toggleDrawer("files");
        return true;
      case "/wiki":
        navigateView("wiki");
        return true;
      case "/settings":
        navigateView("settings");
        return true;
      default:
        return false;
    }
  };

  const applySuggestion = (opt: SuggestOption) => {
    if (!activeTrigger) return;
    if (opt.kind === "command") {
      if (
        opt.value === "/compact" ||
        opt.value === "/goal" ||
        opt.value === "/btw" ||
        opt.value === "/grill-me"
      ) {
        const next = `${opt.value} `;
        setText(next);
        updateDraft(activeProjectDir, activeProcess?.id ?? null, next, attachments);
        setSuggests([]);
        setActiveTrigger(null);
        textareaRef.current?.focus();
        return;
      }
      setText("");
      updateDraft(activeProjectDir, activeProcess?.id ?? null, "", attachments);
      setSuggests([]);
      setActiveTrigger(null);
      void executeSlashCommand(opt.value);
      return;
    }
    const next = text.slice(0, activeTrigger.start) + opt.value + text.slice(activeTrigger.end);
    setText(next);
    updateDraft(activeProjectDir, activeProcess?.id ?? null, next, attachments);
    setSuggests([]);
    setActiveTrigger(null);
    window.setTimeout(() => {
      const el = textareaRef.current;
      if (el) {
        const pos = activeTrigger.start + opt.value.length;
        el.focus();
        el.selectionStart = el.selectionEnd = pos;
      }
    }, 0);
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
    setSuggests([]);
    setActiveTrigger(null);
    if (attachments.length === 0 && text.trim().startsWith("!")) {
      const cmd = text.trim().slice(1).trim();
      if (!cmd) {
        pushToast("请输入要执行的本地命令，例如 !git status", "info");
        return;
      }
      if (!activeProjectDir) {
        pushToast("当前没有活跃工作目录", "warn");
        return;
      }
      setQuickShellRunning(true);
      setText("");
      updateDraft(activeProjectDir, activeProcess?.id ?? null, "", []);
      try {
        const res = await invoke<QuickShellResult>("shell_exec_quick", {
          projectDir: activeProjectDir,
          processId: activeProcess?.id ?? null,
          command: cmd,
        });
        setQuickShellResult(res);
      } catch (err) {
        pushToast(`执行本地命令失败：${formatError(err)}`, "error");
        setText(`!${cmd}`);
      } finally {
        setQuickShellRunning(false);
      }
      return;
    }
    if (attachments.length === 0 && text.trim().startsWith("/")) {
      const handled = await executeSlashCommand(text);
      if (handled) {
        setText("");
        updateDraft(activeProjectDir, activeProcess?.id ?? null, "", []);
        return;
      }
    }
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

  const handleModelChange = async (nextValue: string) => {
    if (!activeProjectDir || !activeProcess) return;
    if (nextValue === MANUAL_MODEL_SENTINEL) {
      const entered = await promptDialog({
        title: "手填模型（provider:model）",
        label: "适用于未提供 /models 探测接口的服务商，例如 deepseek:deepseek-chat",
        initialValue: activeProcess.model ?? "",
        placeholder: "provider:model",
        confirmLabel: "应用到当前对话",
      });
      if (entered === null) return;
      const spec = entered.trim();
      if (!MANUAL_MODEL_PATTERN.test(spec)) {
        pushToast("模型格式应为 provider:model，例如 openai:gpt-5", "warn");
        return;
      }
      const existingManual = activeProcess.manual_models ?? [];
      const nextManual = existingManual.includes(spec)
        ? existingManual
        : [...existingManual, spec];
      await updateProcessSettings(activeProjectDir, activeProcess.id, {
        model: spec,
        manualModels: nextManual,
      });
      return;
    }
    await updateProcessSettings(activeProjectDir, activeProcess.id, {
      model: nextValue,
    });
  };

  const hasProcess = Boolean(activeProjectDir && activeProcess);
  const running = Boolean(activeProcess?.running);
  const resolvedModel =
    effectiveModel?.model?.resolved ??
    effectiveModel?.model?.value ??
    activeProcess?.model ??
    null;
  const modelError = effectiveModel?.model?.error ?? null;
  const modelSource = sourceDisplayLabel(effectiveModel?.model?.source);
  const defaultModelId =
    effectiveModel?.defaultModel?.resolved ?? effectiveModel?.defaultModel?.value ?? null;
  const effectiveModelLabel = modelError
    ? "模型解析失败"
    : resolvedModel ?? "默认主模型";
  const fastModeActive = Boolean(effectiveModel?.codexFastMode?.active);

  const currentProfile = activeProcess?.profile ?? "dev-auto";
  const currentReasoning = activeProcess?.reasoning ?? "";
  const defaultReasoningVal = effectiveModel?.defaultReasoning?.value ?? null;
  const currentSubagentMode = activeProcess?.subagent_mode ?? "auto";
  const researchLocked = Boolean(activeProcess?.research_topic);
  const autoRunActive = !generalMode && autoRun.enabled && !autoRun.paused;

  const profileOptions = withFallbackOption(
    generalMode ? GENERAL_COMPOSER_PROFILES : PROJECT_COMPOSER_PROFILES,
    currentProfile,
    (v) => profileDisplayLabel(v)
  );
  const reasoningOptions = withFallbackOption(REASONING_OPTIONS, currentReasoning);
  const subagentOptions = withFallbackOption(SUBAGENT_MODE_OPTIONS, currentSubagentMode);

  const cycleProfile = () => {
    if (!activeProjectDir || !activeProcess || researchLocked) return;
    const values = profileOptions.map((o) => o.value);
    const idx = values.indexOf(currentProfile);
    const nextProfile = values[(idx + 1) % values.length];
    void updateProcessSettings(activeProjectDir, activeProcess.id, { profile: nextProfile }).then(
      () => pushToast(`已切换执行模式：${profileDisplayLabel(nextProfile)}`, "info", 1600)
    );
  };

  const showPlanApproveBar =
    !generalMode &&
    !running &&
    hasProcess &&
    currentProfile === "readonly" &&
    messages.some((m) => m.role === "assistant");

  const modelChipTitle = [
    modelError
      ? `模型解析失败：${modelError}`
      : `下一轮生效模型：${effectiveModelLabel}${modelSource ? `（${modelSource}）` : ""}`,
    fastModeActive ? "Codex Fast Mode 已生效" : "",
    generalMode
      ? "点击前往全局设置调整默认模型"
      : activeProjectDir
      ? "点击配置本项目模型覆盖"
      : "",
  ]
    .filter(Boolean)
    .join("\n");

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
        {suggests.length > 0 && (
          <div className="composer-suggest-menu" role="listbox" aria-label="快捷补全建议">
            {suggests.map((opt, idx) => (
              <button
                key={`${opt.kind}:${opt.id}`}
                type="button"
                role="option"
                aria-selected={idx === suggestIndex}
                className={`composer-suggest-item${idx === suggestIndex ? " active" : ""}`}
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => setSuggestIndex(idx)}
                onClick={() => applySuggestion(opt)}
              >
                <span className="mono">{opt.label}</span>
                {opt.hint && <span className="field-hint">{opt.hint}</span>}
              </button>
            ))}
          </div>
        )}

        {quickShellRunning && (
          <div className="quick-shell-card">
            <div className="quick-shell-head">
              <span className="mono">正在执行本地 Shell 命令…</span>
            </div>
          </div>
        )}

        {quickShellResult && (
          <div className="quick-shell-card" role="region" aria-label="本地命令执行结果">
            <div className="quick-shell-head">
              <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
                <span
                  className={`badge ${quickShellResult.exitCode === 0 && !quickShellResult.timedOut ? "badge-ok" : "badge-err"}`}
                >
                  exit {quickShellResult.exitCode}
                </span>
                <span className="mono quick-shell-cmd" title={quickShellResult.command}>
                  $ {quickShellResult.command}
                </span>
                <span className="field-hint">{quickShellResult.durationMs}ms</span>
              </div>
              <div style={{ display: "flex", gap: 4, flex: "none" }}>
                <button
                  type="button"
                  className="btn btn-sm"
                  onClick={() => {
                    const outText = [
                      quickShellResult.stdout.trim(),
                      quickShellResult.stderr.trim() ? `[stderr]\n${quickShellResult.stderr.trim()}` : "",
                    ]
                      .filter(Boolean)
                      .join("\n");
                    const block = `本地命令 \`${quickShellResult.command}\`（exit ${quickShellResult.exitCode}）输出：\n\`\`\`\n${outText || "(无输出)"}\n\`\`\`\n`;
                    const next = text.trim() ? `${text}\n\n${block}` : block;
                    handleTextChange(next);
                    setQuickShellResult(null);
                    textareaRef.current?.focus();
                  }}
                >
                  附带到输入框
                </button>
                <button
                  type="button"
                  className="btn btn-sm btn-primary"
                  onClick={() => {
                    const outText = [
                      quickShellResult.stdout.trim(),
                      quickShellResult.stderr.trim() ? `[stderr]\n${quickShellResult.stderr.trim()}` : "",
                    ]
                      .filter(Boolean)
                      .join("\n");
                    const prompt = `我在本地执行了 \`${quickShellResult.command}\`（exit ${quickShellResult.exitCode}），输出如下，请帮我分析或处理：\n\`\`\`\n${outText || "(无输出)"}\n\`\`\``;
                    setQuickShellResult(null);
                    void sendPrompt(prompt, []);
                  }}
                >
                  发给模型
                </button>
                <button
                  type="button"
                  className="btn btn-ghost btn-icon"
                  aria-label="关闭命令结果"
                  onClick={() => setQuickShellResult(null)}
                >
                  <IconClose size={12} />
                </button>
              </div>
            </div>
            {(quickShellResult.stdout || quickShellResult.stderr) && (
              <OutputPreview
                tail
                text={[
                  quickShellResult.stdout,
                  quickShellResult.stderr && "[stderr]\n" + quickShellResult.stderr,
                  quickShellResult.truncated ? "…[输出已截断]" : "",
                ]
                  .filter(Boolean)
                  .join("\n")}
              />
            )}
          </div>
        )}

        {(autoRun.goal || showPlanApproveBar) && (
          <div className="composer-context-strip">
            <div className="composer-context-left">
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

              {showPlanApproveBar && (
                <button
                  type="button"
                  className="composer-chip composer-chip-goal"
                  onClick={() => {
                    if (!activeProjectDir || !activeProcess) return;
                    void updateProcessSettings(activeProjectDir, activeProcess.id, {
                      profile: "dev-auto",
                    }).then(() => sendPrompt("批准以上方案，请按计划开始实施并验证。"));
                  }}
                  title="切换到自主开发模式并执行刚才讨论的方案"
                >
                  <IconSparkles size={11} />
                  <span>批准方案并实施</span>
                </button>
              )}
            </div>
          </div>
        )}

        {pendingInputs.length > 0 && (
          <div className="composer-queue-strip">
            {pendingInputs.map((item) => (
              <div key={item.input_id} className="composer-queue-item">
                <span
                  style={{
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
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
              const src = `data:${att.media_type};base64,${att.data}`;
              return (
                <span key={idx} className="composer-att-chip">
                  {isImg && (
                    <img
                      src={src}
                      alt=""
                      className="composer-att-thumb"
                      style={{ cursor: "zoom-in" }}
                      onClick={() =>
                        openViewer({
                          title: att.file_name,
                          content: "",
                          kind: "image",
                          imageSrc: src,
                        })
                      }
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
              : "输入需求或指令（@ 文件 · / 命令 · $ 技能 · ! 本地命令 · Shift+Tab 切换模式）…"
          }
          value={text}
          onChange={(e) => handleTextChange(e.target.value, e.target.selectionStart)}
          onPaste={(e) => {
            const files = Array.from(e.clipboardData.files ?? []);
            if (files.length > 0) {
              e.preventDefault();
              void addFiles(files);
            }
          }}
          onKeyDown={(e) => {
            if (suggests.length > 0) {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setSuggestIndex((i) => (i + 1) % suggests.length);
                return;
              }
              if (e.key === "ArrowUp") {
                e.preventDefault();
                setSuggestIndex((i) => (i <= 0 ? suggests.length - 1 : i - 1));
                return;
              }
              if ((e.key === "Enter" || e.key === "Tab") && !e.nativeEvent.isComposing) {
                e.preventDefault();
                applySuggestion(suggests[suggestIndex]);
                return;
              }
              if (e.key === "Escape") {
                e.preventDefault();
                setSuggests([]);
                setActiveTrigger(null);
                return;
              }
            }
            if (e.key === "Tab" && e.shiftKey) {
              e.preventDefault();
              cycleProfile();
              return;
            }
            if (
              e.key === "ArrowUp" &&
              promptHistory.length > 0 &&
              textareaRef.current &&
              textareaRef.current.selectionStart === 0 &&
              (!text.includes("\n") || historyCursor !== null)
            ) {
              e.preventDefault();
              const nextCursor =
                historyCursor === null ? promptHistory.length - 1 : Math.max(0, historyCursor - 1);
              setHistoryCursor(nextCursor);
              setText(promptHistory[nextCursor]);
              return;
            }
            if (e.key === "ArrowDown" && historyCursor !== null) {
              e.preventDefault();
              if (historyCursor >= promptHistory.length - 1) {
                setHistoryCursor(null);
                setText("");
              } else {
                const nextCursor = historyCursor + 1;
                setHistoryCursor(nextCursor);
                setText(promptHistory[nextCursor]);
              }
              return;
            }
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

            <ControlPopover
              label="模型选择"
              title={modelChipTitle}
              trigger={
                <>
                  <IconCpu size={12} />
                  {effectiveModelLabel} ▾
                </>
              }
              onOpen={() => {
                if (!modelsCatalog.length && !modelsLoading) void loadModelsCatalog(activeProjectDir, false);
              }}
            >
              {(close) => (
                <>
                  <div role="listbox" aria-label="对话模型" className="model-options">
                    {[
                      { id: "", label: defaultModelId ? `默认 (${defaultModelId})` : "默认模型" },
                      ...selectableModels,
                      { id: MANUAL_MODEL_SENTINEL, label: "手填模型…" },
                    ].map((model) => (
                      <button
                        type="button"
                        key={model.id}
                        role="option"
                        aria-selected={model.id === (activeProcess?.model ?? "")}
                        className="btn btn-sm btn-ghost"
                        disabled={!hasProcess}
                        onClick={() => {
                          close();
                          void handleModelChange(model.id);
                        }}
                      >
                        {model.label}
                      </button>
                    ))}
                  </div>
                  <button
                    type="button"
                    className="btn btn-sm"
                    onClick={() => {
                      close();
                      if (generalMode || !activeProjectDir) navigateView("settings");
                      else openProjectModelsDialog(activeProjectDir);
                    }}
                  >
                    {generalMode ? "全局模型设置" : "项目模型配置"}
                  </button>
                </>
              )}
            </ControlPopover>
            <ControlPopover
              label="协作参数"
              disabled={!hasProcess}
              trigger={profileDisplayLabel(currentProfile) + " ▾"}
            >
              {() => (
                <>
                  <ChoiceGroup
                    label="执行模式"
                    value={currentProfile}
                    options={profileOptions}
                    disabled={researchLocked}
                    onChange={(profile) => {
                      if (activeProjectDir && activeProcess)
                        void updateProcessSettings(activeProjectDir, activeProcess.id, { profile });
                    }}
                  />
                  <ChoiceGroup
                    label="思考强度"
                    value={currentReasoning}
                    options={[
                      {
                        value: "",
                        label: defaultReasoningVal
                          ? `默认 (${reasoningShortLabel(defaultReasoningVal)})`
                          : "默认",
                      },
                      ...reasoningOptions.filter((o) => o.value),
                    ]}
                    onChange={(reasoning) => {
                      if (activeProjectDir && activeProcess)
                        void updateProcessSettings(activeProjectDir, activeProcess.id, { reasoning });
                    }}
                  />
                  <ChoiceGroup
                    label="子代理协作"
                    value={currentSubagentMode}
                    options={subagentOptions}
                    onChange={(subagentMode) => {
                      if (activeProjectDir && activeProcess)
                        void updateProcessSettings(activeProjectDir, activeProcess.id, { subagentMode });
                    }}
                  />
                  {primaryAgents.length > 1 && (
                    <ChoiceGroup
                      label="主 Agent 选择"
                      value={selectedAgent ?? ""}
                      options={[
                        { value: "", label: "默认" },
                        ...primaryAgents.map((name) => ({ value: name, label: name })),
                      ]}
                      onChange={(agent) => chatStore.setState({ selectedAgent: agent })}
                    />
                  )}
                  {!generalMode && (
                    <>
                      <button
                        type="button"
                        className={`composer-toggle-pill${autoAllow && autoRunActive ? " active" : ""}`}
                        disabled={!hasProcess || !autoRunActive}
                        onClick={() => chatStore.setState({ autoAllow: !autoAllow })}
                        title={
                          autoRunActive
                            ? "自动推进期间自动放行常规工具权限"
                            : "仅在开启「自动推进」时生效（单轮交互按常规规则确认）"
                        }
                      >
                        <IconShield size={12} />
                        <span>自动放行</span>
                      </button>
                      <button
                        type="button"
                        className={`btn btn-ghost btn-icon btn-sm${autoRun.goal ? " active" : ""}`}
                        disabled={!hasProcess}
                        onClick={() => void handleEditGoal()}
                        title={autoRun.goal ? `修改自动推进目标：${autoRun.goal}` : "设置自动推进停止目标"}
                        aria-label="设置自动推进目标"
                      >
                        <IconTarget size={13} />
                      </button>
                    </>
                  )}
                </>
              )}
            </ControlPopover>
            {!generalMode && (
              <button
                type="button"
                className={`composer-toggle-pill${autoRun.enabled ? " active-accent" : ""}`}
                disabled={!hasProcess}
                onClick={() => void updateAutoRun({ enabled: !autoRun.enabled, paused: false })}
                title={hasProcess ? "开启或关闭多轮自主推进" : "请先选择或创建对话"}
              >
                <IconSparkles size={12} />
                <span>{autoRun.enabled ? "推进中" : "自动推进"}</span>
              </button>
            )}
          </div>

          <div className="composer-toolbar-right">
            {running && (
              <>
                <div className="segmented" aria-label="运行中投递方式">
                  {(["steer", "queue"] as const).map((mode) => (
                    <button
                      type="button"
                      key={mode}
                      aria-pressed={deliveryMode === mode}
                      className={"segmented-btn" + (deliveryMode === mode ? " active" : "")}
                      onClick={() => chatStore.setState({ deliveryMode: mode })}
                    >
                      {mode === "steer" ? "插话" : "排队"}
                    </button>
                  ))}
                </div>
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
