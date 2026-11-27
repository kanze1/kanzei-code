import React, { useCallback, useEffect, useRef, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import type {
  HookRuleItem,
  OpenToolInfo,
  PermissionRuleRecord,
  ProjectModelOverrideSummary,
} from "../../ipc/types";
import {
  ALL_PROFILE_OPTIONS,
  CADENCE_COMMIT_OPTIONS,
  CADENCE_FULL_TEST_OPTIONS,
  CADENCE_PUSH_OPTIONS,
  CADENCE_TARGETED_TEST_OPTIONS,
  profileDisplayLabel,
  REASONING_OPTIONS,
  withFallbackOption,
} from "../../configOptions";
import { appStore, loadModelsCatalog, loadOpenTools, useAppStore } from "../../store/appStore";
import { refreshActiveSessionMeta } from "../../store/sessionStore";
import {
  openProjectModelsDialog,
  playNotificationChime,
  pushToast,
  setSoundEnabled,
  useUiStore,
} from "../../store/uiStore";
import { ModelCombobox } from "../../components/ModelCombobox";
import {
  IconBot,
  IconCpu,
  IconDownload,
  IconExternal,
  IconGlobe,
  IconLayers,
  IconPlus,
  IconRefresh,
  IconShield,
  IconSliders,
} from "../../components/Icons";

interface ProviderRow {
  name: string;
  protocol: string;
  baseUrl: string;
  apiKeyEnv?: string | null;
  apiKey?: string | null;
  keyPresent?: boolean | null;
  auth?: string | null;
  legacyClaudeSubscription?: boolean;
  contextLimit?: number | null;
  builtin?: boolean;
  source?: string;
}

interface LimitsForm {
  maxTokens?: number | null;
  subagentMaxTokens?: number | null;
  subagentTimeoutSecs?: number | null;
  subagentAutoConcurrency?: number | null;
  subagentUltraConcurrency?: number | null;
  subagentGlobalConcurrency?: number | null;
  contextBudgetRatio?: number | null;
  recentVerbatimRatio?: number | null;
  maxTasksPerTurn?: number | null;
  maxParallelTools?: number | null;
  transportRetries?: number | null;
  rateLimitRetries?: number | null;
  streamRestarts?: number | null;
  autoRunTokenBudget?: number | null;
}

interface CadenceForm {
  full_test?: string | null;
  full_test_batches?: number | null;
  targeted_test?: string | null;
  commit?: string | null;
  push?: string | null;
}

interface EffectiveOverrideInfo {
  proxy?: string;
  profileDefault?: string;
}

interface AgentDirectoryEntry {
  name: string;
  source: string;
  path?: string | null;
  profile: string;
  mode: string;
  model: string;
  steps?: number | null;
  status: string;
  systemPreview?: string;
  error?: string | null;
}

interface AgentDirectorySnapshot {
  profile?: string;
  globalDir?: string;
  global_dir?: string;
  agents?: AgentDirectoryEntry[];
}

const TOC_ITEMS: Array<{ id: string; label: string; icon: React.ReactElement }> = [
  { id: "models", label: "模型与快速模型", icon: <IconCpu size={14} /> },
  { id: "providers", label: "模型服务商", icon: <IconLayers size={14} /> },
  { id: "network", label: "网络、档位与节奏", icon: <IconGlobe size={14} /> },
  { id: "limits", label: "高级参数与预算", icon: <IconSliders size={14} /> },
  { id: "hooks", label: "生命周期 Hooks", icon: <IconShield size={14} /> },
  { id: "tools", label: "打开方式与权限", icon: <IconShield size={14} /> },
  { id: "agents", label: "Agent、提醒与快捷键", icon: <IconBot size={14} /> },
  { id: "update", label: "版本与更新", icon: <IconDownload size={14} /> },
];

function agentStatusLabel(status: string): { text: string; badge: string } {
  switch (status) {
    case "available":
      return { text: "可用", badge: "badge-ok" };
    case "configurationError":
      return { text: "配置错误", badge: "badge-err" };
    case "untrusted":
      return { text: "尚未信任", badge: "badge-warn" };
    case "overridden":
      return { text: "已被覆盖", badge: "badge-info" };
    case "hidden":
      return { text: "当前档位不适用", badge: "" };
    default:
      return { text: status || "未知", badge: "" };
  }
}

function overrideKeyLabel(key: string): string {
  switch (key) {
    case "primary":
      return "主模型";
    case "fast":
      return "快速模型";
    case "compact":
      return "压缩模型";
    case "reasoning":
      return "思考强度";
    case "codexFastMode":
      return "Codex Fast Mode";
    default:
      return key;
  }
}

function parseOptionalInt(raw: string): number | null {
  if (!raw.trim()) return null;
  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : null;
}

function parseOptionalNonNegInt(raw: string): number | null {
  if (!raw.trim()) return null;
  const n = Number(raw);
  return Number.isFinite(n) && n >= 0 ? Math.floor(n) : null;
}

export default function SettingsView(): React.ReactElement {
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const modelsCatalog = useAppStore((s) => s.modelsCatalog);
  const modelsErrors = useAppStore((s) => s.modelsErrors);
  const modelsLoading = useAppStore((s) => s.modelsLoading);
  const openTools = useAppStore((s) => s.openTools);
  const appInfo = useAppStore((s) => s.appInfo);
  const soundEnabled = useUiStore((s) => s.soundEnabled);

  const [section, setSection] = useState("models");
  const [configPath, setConfigPath] = useState("");
  const [primary, setPrimary] = useState("");
  const [fast, setFast] = useState("");
  const [compact, setCompact] = useState("");
  const [reasoning, setReasoning] = useState("off");
  const [codexFastMode, setCodexFastMode] = useState(false);
  const [proxy, setProxy] = useState("env");
  const [language, setLanguage] = useState("");
  const [profileDefault, setProfileDefault] = useState("dev-auto");
  const [providers, setProviders] = useState<ProviderRow[]>([]);
  const [limits, setLimits] = useState<LimitsForm>({});
  const [limitDefaults, setLimitDefaults] = useState<LimitsForm>({});
  const [cadence, setCadence] = useState<CadenceForm>({});
  const [hooks, setHooks] = useState<HookRuleItem[]>([]);
  const [projectModelOverrides, setProjectModelOverrides] = useState<ProjectModelOverrideSummary[]>([]);
  const [effectiveOverrides, setEffectiveOverrides] = useState<EffectiveOverrideInfo | null>(null);
  const [projectConfigPath, setProjectConfigPath] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);

  const [testingProviderIdx, setTestingProviderIdx] = useState<number | null>(null);
  const [providerTestResults, setProviderTestResults] = useState<Record<number, string>>({});

  const [fastStatus, setFastStatus] = useState<Record<string, unknown> | null>(null);
  const [fastSettingUp, setFastSettingUp] = useState(false);
  const [permRules, setPermRules] = useState<PermissionRuleRecord[]>([]);
  const [permRulesPath, setPermRulesPath] = useState<string>("");
  const [updateInfo, setUpdateInfo] = useState<Record<string, unknown> | null>(null);
  const [updateReceipt, setUpdateReceipt] = useState<Record<string, unknown> | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [customTools, setCustomTools] = useState<OpenToolInfo[]>([]);
  const [agentsInfo, setAgentsInfo] = useState<AgentDirectorySnapshot | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const effectiveProjectDir = generalMode ? null : activeProjectDir;

  const loadPermissionRules = useCallback(async () => {
    if (!effectiveProjectDir) {
      setPermRules([]);
      setPermRulesPath("");
      return;
    }
    try {
      const res = await invoke<
        { path?: string; rules?: PermissionRuleRecord[] } | PermissionRuleRecord[]
      >("permission_rules_get", { projectDir: effectiveProjectDir });
      if (Array.isArray(res)) {
        setPermRules(res);
        setPermRulesPath("");
      } else {
        setPermRules(Array.isArray(res?.rules) ? res.rules : []);
        setPermRulesPath(typeof res?.path === "string" ? res.path : "");
      }
    } catch {
      setPermRules([]);
    }
  }, [effectiveProjectDir]);

  const loadAll = useCallback(async () => {
    try {
      const data = await invoke<Record<string, unknown>>("settings_get", {
        projectDir: effectiveProjectDir,
      });
      setConfigPath(String(data.path ?? ""));
      setPrimary(String(data.primary ?? ""));
      setFast(String(data.fast ?? ""));
      setCompact(String(data.compact ?? ""));
      setReasoning(String(data.reasoning ?? "off"));
      setCodexFastMode(Boolean(data.codexFastMode));
      setProxy(String(data.proxy ?? "env"));
      setLanguage(String(data.language ?? ""));
      setProfileDefault(String(data.profileDefault ?? "dev-auto"));
      setProviders(Array.isArray(data.providers) ? (data.providers as ProviderRow[]) : []);
      setLimits((data.limits as LimitsForm) ?? {});
      setLimitDefaults((data.limitDefaults as LimitsForm) ?? {});
      setCadence((data.cadence as CadenceForm) ?? {});
      setHooks(Array.isArray(data.hooks) ? (data.hooks as HookRuleItem[]) : []);
      setProjectModelOverrides(
        Array.isArray(data.projectModelOverrides)
          ? (data.projectModelOverrides as ProjectModelOverrideSummary[])
          : []
      );
      setEffectiveOverrides((data.effective as EffectiveOverrideInfo | null) ?? null);
      setProjectConfigPath(typeof data.projectConfig === "string" ? data.projectConfig : null);
      setProviderTestResults({});
      setDirty(false);
    } catch (error) {
      pushToast(`读取设置失败：${formatError(error)}`, "error");
    }
  }, [effectiveProjectDir]);

  useEffect(() => {
    void loadAll();
    void loadOpenTools();
    void loadPermissionRules();
    const { modelsCatalog: curModels, modelsLoading: curLoading } = appStore.getState();
    if (curModels.length === 0 && !curLoading) {
      void loadModelsCatalog(effectiveProjectDir, false);
    }
    invoke<Record<string, unknown>>("fast_model_status").then(setFastStatus).catch(() => {});
    invoke<AgentDirectorySnapshot>("agent_directory_get", { projectDir: effectiveProjectDir })
      .then(setAgentsInfo)
      .catch(() => {});
    invoke<Record<string, unknown> | null>("update_receipt")
      .then(setUpdateReceipt)
      .catch(() => {});
  }, [loadAll, loadPermissionRules, effectiveProjectDir]);

  useEffect(() => {
    setCustomTools(openTools.filter((t) => !t.builtin));
  }, [openTools]);

  const handleScrollSpy = () => {
    const container = scrollRef.current;
    if (!container) return;
    const topOffset = container.scrollTop + 48;
    let activeId = TOC_ITEMS[0].id;
    for (const item of TOC_ITEMS) {
      const el = document.getElementById(`settings-sec-${item.id}`);
      if (el && el.offsetTop - container.offsetTop <= topOffset) {
        activeId = item.id;
      }
    }
    if (activeId !== section) setSection(activeId);
  };

  const handleRefreshModels = async () => {
    await loadModelsCatalog(effectiveProjectDir, true);
    const errors = appStore.getState().modelsErrors;
    if (errors.length > 0) {
      pushToast(`已刷新模型目录，但有 ${errors.length} 个服务商探测失败`, "warn");
    } else {
      pushToast("已刷新模型目录", "ok");
    }
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await invoke("settings_save", {
        payload: {
          language: language || null,
          primary: primary.trim(),
          fast: fast.trim(),
          compact: compact.trim(),
          proxy: proxy.trim() || "env",
          reasoning,
          codexFastMode,
          profileDefault,
          providers: providers.map((p) => ({
            name: p.name,
            protocol: p.protocol,
            baseUrl: p.baseUrl,
            apiKeyEnv: p.apiKeyEnv?.trim() || null,
            apiKey: p.apiKey?.trim() || null,
            auth: p.auth || null,
            contextLimit: p.contextLimit ? Number(p.contextLimit) : null,
          })),
          limits,
          cadence: {
            full_test: cadence.full_test || null,
            full_test_batches: cadence.full_test_batches ?? null,
            targeted_test: cadence.targeted_test || null,
            commit: cadence.commit || null,
            push: cadence.push || null,
          },
          hooks: hooks
            .filter((h) => h.command.trim().length > 0)
            .map((h) => ({
              event: h.event,
              toolMatcher: h.toolMatcher?.trim() || null,
              command: h.command.trim(),
              timeoutSecs: h.timeoutSecs && h.timeoutSecs > 0 ? Number(h.timeoutSecs) : null,
              enabled: h.enabled,
            })),
        },
      });
      setDirty(false);
      setSaving(false);
      pushToast("全局设置已保存", "ok");
      void Promise.all([
        loadModelsCatalog(effectiveProjectDir, true),
        refreshActiveSessionMeta(),
        invoke<Record<string, unknown>>("fast_model_status")
          .then(setFastStatus)
          .catch(() => {}),
      ]);
    } catch (error) {
      pushToast(`保存设置失败：${formatError(error)}`, "error");
    } finally {
      setSaving(false);
    }
  };

  const testProvider = async (p: ProviderRow, idx: number) => {
    setTestingProviderIdx(idx);
    try {
      const res = await invoke<string | { message?: string }>("provider_test", {
        protocol: p.protocol,
        baseUrl: p.baseUrl,
        apiKeyEnv: p.apiKeyEnv?.trim() || null,
        apiKey: p.apiKey?.trim() || null,
        auth: p.auth || null,
        proxy: proxy.trim() || "env",
      });
      const text = typeof res === "string" ? res : res?.message ?? `${p.name} 连通正常`;
      setProviderTestResults((prev) => ({ ...prev, [idx]: text }));
      const kind = text.startsWith("✗") ? "error" : text.startsWith("?") ? "warn" : "ok";
      pushToast(`${p.name || "服务商"}：${text}`, kind);
    } catch (error) {
      const text = `✗ 测试失败：${formatError(error)}`;
      setProviderTestResults((prev) => ({ ...prev, [idx]: text }));
      pushToast(`${p.name || "服务商"} 测试失败：${formatError(error)}`, "error");
    } finally {
      setTestingProviderIdx(null);
    }
  };

  const handleFastSetup = async () => {
    setFastSettingUp(true);
    try {
      const msg = await invoke<string>("fast_model_setup");
      pushToast(msg || "快速模型已就绪", "ok");
      const status = await invoke<Record<string, unknown>>("fast_model_status");
      setFastStatus(status);
    } catch (error) {
      pushToast(`快速模型配置失败：${formatError(error)}`, "error");
    } finally {
      setFastSettingUp(false);
    }
  };

  const handleCheckUpdate = async () => {
    setUpdateBusy(true);
    try {
      const res = await invoke<Record<string, unknown>>("update_check");
      setUpdateInfo(res);
    } catch (error) {
      pushToast(`检查更新失败：${formatError(error)}`, "error");
    } finally {
      setUpdateBusy(false);
    }
  };

  const handleInstallUpdate = async () => {
    if (!updateInfo?.url || !updateInfo?.expected_build) return;
    setUpdateBusy(true);
    try {
      const msg = await invoke<string>("update_install", {
        url: String(updateInfo.url),
        expectedBuild: String(updateInfo.expected_build),
      });
      pushToast(msg, "ok");
    } catch (error) {
      pushToast(`安装更新失败：${formatError(error)}`, "error");
      setUpdateBusy(false);
    }
  };

  const handleSaveOpenTools = async () => {
    try {
      await invoke("open_tools_save", {
        tools: customTools.map((t) => ({
          id: t.id || "",
          label: t.label,
          command: t.command ?? "",
          args: t.args ?? ["{path}"],
        })),
      });
      await loadOpenTools();
      pushToast("已保存外部打开方式", "ok");
    } catch (error) {
      pushToast(`保存打开方式失败：${formatError(error)}`, "error");
    }
  };

  const reasoningOptions = withFallbackOption(REASONING_OPTIONS, reasoning);
  const profileOptions = withFallbackOption(ALL_PROFILE_OPTIONS, profileDefault);
  const fullTestOptions = withFallbackOption(CADENCE_FULL_TEST_OPTIONS, cadence.full_test);
  const targetedTestOptions = withFallbackOption(
    CADENCE_TARGETED_TEST_OPTIONS,
    cadence.targeted_test
  );
  const commitOptions = withFallbackOption(CADENCE_COMMIT_OPTIONS, cadence.commit);
  const pushOptions = withFallbackOption(CADENCE_PUSH_OPTIONS, cadence.push);

  const effectiveDiffs: string[] = [];
  if (effectiveOverrides) {
    if (effectiveOverrides.proxy && effectiveOverrides.proxy !== (proxy.trim() || "env")) {
      effectiveDiffs.push(`代理：当前项目生效为 ${effectiveOverrides.proxy}`);
    }
    if (effectiveOverrides.profileDefault && effectiveOverrides.profileDefault !== profileDefault) {
      effectiveDiffs.push(
        `默认档位：当前项目生效为 ${profileDisplayLabel(effectiveOverrides.profileDefault)}`
      );
    }
  }

  return (
    <div id="view-settings" className="settings-layout">
      <div className="page-header">
        <div>
          <h1 className="page-title">全局设置</h1>
          <div className="page-subtitle mono">{configPath || "~/.kanzei/kanzei.toml"}</div>
        </div>
        <div className="page-header-actions">
          <button
            type="button"
            className="btn"
            onClick={() => void invoke("settings_open").catch(() => {})}
          >
            <IconExternal size={13} />
            <span>打开配置文件</span>
          </button>
          <button
            type="button"
            className="btn"
            disabled={modelsLoading}
            onClick={() => void handleRefreshModels()}
          >
            <IconRefresh size={13} />
            <span>{modelsLoading ? "探测模型中…" : "刷新模型目录"}</span>
          </button>
        </div>
      </div>

      <div className="settings-body">
        <nav className="settings-toc" aria-label="设置分区">
          {TOC_ITEMS.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`settings-toc-btn${section === item.id ? " active" : ""}`}
              onClick={() => {
                setSection(item.id);
                document
                  .getElementById(`settings-sec-${item.id}`)
                  ?.scrollIntoView({ behavior: "smooth" });
              }}
            >
              {item.icon}
              <span>{item.label}</span>
            </button>
          ))}
        </nav>

        <div ref={scrollRef} className="settings-scroll" onScroll={handleScrollSpy}>
          <section id="settings-sec-models" className="settings-section">
            <h2 className="settings-section-title">模型配置</h2>
            <div className="settings-grid-2">
              <div className="field-group">
                <label className="field-label" htmlFor="set-primary">
                  主模型（primary）
                </label>
                <ModelCombobox
                  id="set-primary"
                  ariaLabel="主模型（primary）"
                  value={primary}
                  models={modelsCatalog}
                  loading={modelsLoading}
                  placeholder="选择或输入 provider:model"
                  onOpen={() => {
                    if (modelsCatalog.length === 0 && !modelsLoading) {
                      void loadModelsCatalog(effectiveProjectDir, false);
                    }
                  }}
                  onRefresh={() => void handleRefreshModels()}
                  onChange={(next) => {
                    setPrimary(next);
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="set-fast">
                  快速模型（fast，留空跟随主模型）
                </label>
                <ModelCombobox
                  id="set-fast"
                  ariaLabel="快速模型（fast）"
                  value={fast}
                  models={modelsCatalog}
                  loading={modelsLoading}
                  allowEmpty
                  emptyLabel="留空（跟随主模型）"
                  placeholder="留空跟随主模型，或输入 provider:model"
                  onOpen={() => {
                    if (modelsCatalog.length === 0 && !modelsLoading) {
                      void loadModelsCatalog(effectiveProjectDir, false);
                    }
                  }}
                  onRefresh={() => void handleRefreshModels()}
                  onChange={(next) => {
                    setFast(next);
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="set-compact">
                  压缩模型（compact，留空跟随主模型）
                </label>
                <ModelCombobox
                  id="set-compact"
                  ariaLabel="压缩模型（compact）"
                  value={compact}
                  models={modelsCatalog}
                  loading={modelsLoading}
                  allowEmpty
                  emptyLabel="留空（跟随主模型）"
                  placeholder="留空跟随主模型，或输入 provider:model"
                  onOpen={() => {
                    if (modelsCatalog.length === 0 && !modelsLoading) {
                      void loadModelsCatalog(effectiveProjectDir, false);
                    }
                  }}
                  onRefresh={() => void handleRefreshModels()}
                  onChange={(next) => {
                    setCompact(next);
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="set-reasoning">
                  默认思考强度（reasoning）
                </label>
                <select
                  id="set-reasoning"
                  className="select"
                  value={reasoning}
                  onChange={(e) => {
                    setReasoning(e.target.value);
                    setDirty(true);
                  }}
                >
                  {reasoningOptions.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            {modelsErrors.length > 0 && (
              <div className="settings-notice settings-notice-warn">
                <span>
                  部分服务商模型目录探测失败（仍可直接手填 <code className="mono">provider:model</code>
                  ）：
                  {modelsErrors.map((e) => `${e.provider}(${e.message})`).join("；")}
                </span>
              </div>
            )}

            {projectModelOverrides.length > 0 && (
              <div className="settings-notice">
                <span>以下项目配置了独立的模型覆盖（不跟随此处的全局默认）：</span>
                <div className="settings-override-links">
                  {projectModelOverrides.map((entry) => (
                    <button
                      key={entry.project}
                      type="button"
                      className="btn btn-ghost btn-sm"
                      title={entry.configPath ?? entry.project}
                      onClick={() => openProjectModelsDialog(entry.project)}
                    >
                      <span>{entry.name}</span>
                      <span className="field-hint">
                        ({(entry.keys ?? []).map(overrideKeyLabel).join("、")})
                      </span>
                    </button>
                  ))}
                </div>
              </div>
            )}

            <div className="toggle-row">
              <div className="toggle-row-text">
                <span className="toggle-row-title">Codex Fast Mode</span>
                <span className="field-hint">在支持的 Codex 订阅模型上优先采用低延迟响应通道</span>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={codexFastMode}
                aria-label="开启 Codex Fast Mode"
                className="switch"
                onClick={() => {
                  setCodexFastMode(!codexFastMode);
                  setDirty(true);
                }}
              />
            </div>

            <div className="provider-card">
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                  gap: 12,
                }}
              >
                <div>
                  <div className="provider-card-title">
                    <span>本地快速模型（Ollama）</span>
                    {Boolean(fastStatus?.managed) && (
                      <span className={`badge ${fastStatus?.ready ? "badge-ok" : "badge-warn"}`}>
                        {fastStatus?.ready ? "已就绪" : "待配置"}
                      </span>
                    )}
                  </div>
                  <div className="field-hint" style={{ marginTop: 4 }}>
                    {fastStatus?.managed
                      ? fastStatus.ready
                        ? `当前托管模型：${String(fastStatus.model ?? "")}`
                        : `安装:${String(fastStatus.installed)} · 服务:${String(
                            fastStatus.serviceUp
                          )} · 模型:${String(fastStatus.modelPresent)}`
                      : "当前 fast 角色指向外部服务商"}
                  </div>
                </div>
                {Boolean(fastStatus?.managed) && !fastStatus?.ready && (
                  <button
                    type="button"
                    className="btn btn-primary btn-sm"
                    disabled={fastSettingUp}
                    onClick={() => void handleFastSetup()}
                  >
                    {fastSettingUp ? "配置中…" : "一键就绪"}
                  </button>
                )}
              </div>
            </div>
          </section>

          <section id="settings-sec-providers" className="settings-section">
            <div className="settings-section-head">
              <h2 className="settings-section-title">模型服务商 ({providers.length})</h2>
              <button
                type="button"
                className="btn btn-sm"
                onClick={() => {
                  setProviders((prev) => [
                    ...prev,
                    {
                      name: "",
                      protocol: "openai",
                      baseUrl: "https://",
                      apiKeyEnv: "",
                      apiKey: "",
                      contextLimit: null,
                    },
                  ]);
                  setDirty(true);
                }}
              >
                <IconPlus size={12} />
                <span>添加服务商</span>
              </button>
            </div>

            {providers.map((p, idx) => {
              const isSubscription = Boolean(p.auth);
              const testOutput = providerTestResults[idx];
              return (
                <div key={idx} className="provider-card">
                  <div className="provider-card-head">
                    <div className="provider-card-title">
                      <span className="mono">{p.name || "未命名服务商"}</span>
                      <span className="badge">{p.protocol}</span>
                      <span className="badge badge-info">
                        {p.source === "project"
                          ? "项目配置"
                          : p.source === "global"
                          ? "全局配置"
                          : "内置"}
                      </span>
                      {p.builtin && <span className="badge">内置不可删</span>}
                      {!isSubscription && p.keyPresent !== null && p.keyPresent !== undefined && (
                        <span className={`badge ${p.keyPresent ? "badge-ok" : "badge-warn"}`}>
                          {p.keyPresent ? "密钥已就绪" : "缺少密钥"}
                        </span>
                      )}
                    </div>
                    <div style={{ display: "flex", gap: 6 }}>
                      <button
                        type="button"
                        className="btn btn-sm"
                        disabled={testingProviderIdx === idx}
                        onClick={() => void testProvider(p, idx)}
                      >
                        {testingProviderIdx === idx ? "测试中…" : "连通性测试"}
                      </button>
                      {!p.builtin && (
                        <button
                          type="button"
                          className="btn btn-danger btn-sm"
                          onClick={() => {
                            setProviders((prev) => prev.filter((_, i) => i !== idx));
                            setProviderTestResults({});
                            setDirty(true);
                          }}
                        >
                          删除
                        </button>
                      )}
                    </div>
                  </div>

                  <div className="settings-grid-2">
                    <div className="field-group">
                      <label className="field-label">名称</label>
                      <input
                        className="input mono"
                        value={p.name}
                        disabled={Boolean(p.builtin)}
                        title={p.builtin ? "内置服务商名称固定，不可修改" : undefined}
                        onChange={(e) => {
                          const next = [...providers];
                          next[idx] = { ...p, name: e.target.value };
                          setProviders(next);
                          setDirty(true);
                        }}
                      />
                    </div>
                    <div className="field-group">
                      <label className="field-label">协议</label>
                      <select
                        className="select"
                        value={p.protocol}
                        onChange={(e) => {
                          const next = [...providers];
                          next[idx] = { ...p, protocol: e.target.value };
                          setProviders(next);
                          setDirty(true);
                        }}
                      >
                        <option value="openai">openai</option>
                        <option value="openai-responses">openai-responses</option>
                        <option value="anthropic">anthropic</option>
                        <option value="deepseek-responses">deepseek-responses</option>
                      </select>
                    </div>
                    <div className="field-group">
                      <label className="field-label">Base URL</label>
                      <input
                        className="input mono"
                        value={p.baseUrl}
                        onChange={(e) => {
                          const next = [...providers];
                          next[idx] = { ...p, baseUrl: e.target.value };
                          setProviders(next);
                          setDirty(true);
                        }}
                      />
                    </div>
                    <div className="field-group">
                      <label className="field-label">上下文窗口上限（Tokens，留空跟随内置）</label>
                      <input
                        className="input mono"
                        type="number"
                        min={1}
                        placeholder="留空自动识别"
                        value={p.contextLimit ?? ""}
                        onChange={(e) => {
                          const next = [...providers];
                          next[idx] = { ...p, contextLimit: parseOptionalInt(e.target.value) };
                          setProviders(next);
                          setDirty(true);
                        }}
                      />
                    </div>

                    {isSubscription ? (
                      <div className="field-group" style={{ gridColumn: "1 / -1" }}>
                        <label className="field-label">认证方式</label>
                        <div className="field-hint">
                          <span className="badge badge-ok" style={{ marginRight: 6 }}>
                            订阅登录态 ({p.auth})
                          </span>
                          复用 Codex CLI 登录凭证，无需单独填写 API Key。
                        </div>
                      </div>
                    ) : (
                      <>
                        <div className="field-group">
                          <label className="field-label">API Key 环境变量名（推荐）</label>
                          <input
                            className="input mono"
                            placeholder="例如 OPENAI_API_KEY / DEEPSEEK_API_KEY"
                            value={p.apiKeyEnv ?? ""}
                            onChange={(e) => {
                              const next = [...providers];
                              next[idx] = { ...p, apiKeyEnv: e.target.value };
                              setProviders(next);
                              setDirty(true);
                            }}
                          />
                        </div>
                        <div className="field-group">
                          <label className="field-label">直填 API Key（可选，优先于环境变量）</label>
                          <input
                            className="input mono"
                            type="password"
                            placeholder={
                              p.apiKeyEnv ? `留空则读取 $${p.apiKeyEnv}` : "直接粘贴 API Key（明文存盘）"
                            }
                            value={p.apiKey ?? ""}
                            onChange={(e) => {
                              const next = [...providers];
                              next[idx] = { ...p, apiKey: e.target.value };
                              setProviders(next);
                              setDirty(true);
                            }}
                          />
                        </div>
                      </>
                    )}
                  </div>

                  {p.legacyClaudeSubscription && (
                    <div className="field-hint" style={{ color: "var(--warn)" }}>
                      旧版 Claude 订阅登录已停用，请配置 Anthropic API Key 或环境变量。
                    </div>
                  )}

                  {testOutput && (
                    <div
                      className="field-hint mono"
                      style={{
                        color: testOutput.startsWith("✗")
                          ? "var(--err)"
                          : testOutput.startsWith("?")
                          ? "var(--warn)"
                          : "var(--ok)",
                      }}
                    >
                      {testOutput}
                    </div>
                  )}
                </div>
              );
            })}
          </section>

          <section id="settings-sec-network" className="settings-section">
            <h2 className="settings-section-title">网络、默认档位与验证提交节奏</h2>
            {effectiveDiffs.length > 0 && (
              <div className="settings-notice settings-notice-warn">
                <span>
                  当前项目配置{projectConfigPath ? `（${projectConfigPath}）` : ""}覆盖了部分全局项：
                  {effectiveDiffs.join("；")}
                </span>
              </div>
            )}
            <div className="settings-grid-2">
              <div className="field-group">
                <label className="field-label" htmlFor="set-proxy">
                  代理模式（env / off / 显式代理 URL）
                </label>
                <input
                  id="set-proxy"
                  className="input mono"
                  value={proxy}
                  onChange={(e) => {
                    setProxy(e.target.value);
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="set-profile">
                  默认执行档位
                </label>
                <select
                  id="set-profile"
                  className="select"
                  value={profileDefault}
                  onChange={(e) => {
                    setProfileDefault(e.target.value);
                    setDirty(true);
                  }}
                >
                  {profileOptions.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="set-cadence-full-test">
                  全量测试时机（full_test）
                </label>
                <select
                  id="set-cadence-full-test"
                  className="select"
                  value={cadence.full_test ?? ""}
                  onChange={(e) => {
                    setCadence({ ...cadence, full_test: e.target.value || null });
                    setDirty(true);
                  }}
                >
                  {fullTestOptions.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="set-cadence-full-test-batches">
                  全量测试批次间隔（仅 every_n_batches 生效）
                </label>
                <input
                  id="set-cadence-full-test-batches"
                  className="input mono"
                  type="number"
                  min={1}
                  disabled={cadence.full_test !== "every_n_batches"}
                  placeholder={
                    cadence.full_test === "every_n_batches" ? "正整数，例如 3" : "选择 every_n_batches 后可填"
                  }
                  value={cadence.full_test_batches ?? ""}
                  onChange={(e) => {
                    setCadence({
                      ...cadence,
                      full_test_batches: parseOptionalInt(e.target.value),
                    });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="set-cadence-targeted-test">
                  定向测试时机（targeted_test）
                </label>
                <select
                  id="set-cadence-targeted-test"
                  className="select"
                  value={cadence.targeted_test ?? ""}
                  onChange={(e) => {
                    setCadence({ ...cadence, targeted_test: e.target.value || null });
                    setDirty(true);
                  }}
                >
                  {targetedTestOptions.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="set-cadence-commit">
                  提交节奏（commit）
                </label>
                <select
                  id="set-cadence-commit"
                  className="select"
                  value={cadence.commit ?? ""}
                  onChange={(e) => {
                    setCadence({ ...cadence, commit: e.target.value || null });
                    setDirty(true);
                  }}
                >
                  {commitOptions.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="set-cadence-push">
                  推送节奏（push）
                </label>
                <select
                  id="set-cadence-push"
                  className="select"
                  value={cadence.push ?? ""}
                  onChange={(e) => {
                    setCadence({ ...cadence, push: e.target.value || null });
                    setDirty(true);
                  }}
                >
                  {pushOptions.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </div>
            </div>
          </section>

          <section id="settings-sec-limits" className="settings-section">
            <h2 className="settings-section-title">高级运行参数</h2>
            <p className="field-hint">所有参数留空即跟随内置默认值，不会将当前默认写死到配置文件中。</p>
            <div className="settings-grid-2">
              <div className="field-group">
                <label className="field-label">主模型最大输出 Tokens</label>
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  placeholder={limitDefaults.maxTokens ? `默认 ${limitDefaults.maxTokens}` : "默认 8192"}
                  value={limits.maxTokens ?? ""}
                  onChange={(e) => {
                    setLimits({ ...limits, maxTokens: parseOptionalInt(e.target.value) });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">子代理最大输出 Tokens</label>
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  placeholder={
                    limitDefaults.subagentMaxTokens
                      ? `默认 ${limitDefaults.subagentMaxTokens}`
                      : "默认 4096"
                  }
                  value={limits.subagentMaxTokens ?? ""}
                  onChange={(e) => {
                    setLimits({ ...limits, subagentMaxTokens: parseOptionalInt(e.target.value) });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">子代理全局并发上限</label>
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  placeholder={
                    limitDefaults.subagentGlobalConcurrency
                      ? `默认 ${limitDefaults.subagentGlobalConcurrency}`
                      : "默认 16"
                  }
                  value={limits.subagentGlobalConcurrency ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      subagentGlobalConcurrency: parseOptionalInt(e.target.value),
                    });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">子代理超时（秒）</label>
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  placeholder={
                    limitDefaults.subagentTimeoutSecs
                      ? `默认 ${limitDefaults.subagentTimeoutSecs}`
                      : "默认 900"
                  }
                  value={limits.subagentTimeoutSecs ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      subagentTimeoutSecs: parseOptionalInt(e.target.value),
                    });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">按需模式单会话子代理并发</label>
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  placeholder={
                    limitDefaults.subagentAutoConcurrency
                      ? `默认 ${limitDefaults.subagentAutoConcurrency}`
                      : "默认 2"
                  }
                  value={limits.subagentAutoConcurrency ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      subagentAutoConcurrency: parseOptionalInt(e.target.value),
                    });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">积极模式单会话子代理并发</label>
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  placeholder={
                    limitDefaults.subagentUltraConcurrency
                      ? `默认 ${limitDefaults.subagentUltraConcurrency}`
                      : "默认 8"
                  }
                  value={limits.subagentUltraConcurrency ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      subagentUltraConcurrency: parseOptionalInt(e.target.value),
                    });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">单轮最大并行工具数</label>
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  placeholder={
                    limitDefaults.maxParallelTools
                      ? `默认 ${limitDefaults.maxParallelTools}`
                      : "默认 8"
                  }
                  value={limits.maxParallelTools ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      maxParallelTools: parseOptionalInt(e.target.value),
                    });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">单轮最大子代理派发数</label>
                <input
                  className="input mono"
                  type="number"
                  min={1}
                  placeholder={
                    limitDefaults.maxTasksPerTurn
                      ? `默认 ${limitDefaults.maxTasksPerTurn}`
                      : "默认 16"
                  }
                  value={limits.maxTasksPerTurn ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      maxTasksPerTurn: parseOptionalInt(e.target.value),
                    });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">传输层重试次数</label>
                <input
                  className="input mono"
                  type="number"
                  min={0}
                  placeholder={
                    limitDefaults.transportRetries !== undefined &&
                    limitDefaults.transportRetries !== null
                      ? `默认 ${limitDefaults.transportRetries}`
                      : "默认 2"
                  }
                  value={limits.transportRetries ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      transportRetries: parseOptionalNonNegInt(e.target.value),
                    });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">限流重试次数</label>
                <input
                  className="input mono"
                  type="number"
                  min={0}
                  placeholder={
                    limitDefaults.rateLimitRetries !== undefined &&
                    limitDefaults.rateLimitRetries !== null
                      ? `默认 ${limitDefaults.rateLimitRetries}`
                      : "默认 2"
                  }
                  value={limits.rateLimitRetries ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      rateLimitRetries: parseOptionalNonNegInt(e.target.value),
                    });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">自主推进 Token 预算熔断上限</label>
                <input
                  className="input mono"
                  type="number"
                  min={10000}
                  step={50000}
                  placeholder="留空=不限（例如 500000）"
                  value={limits.autoRunTokenBudget ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      autoRunTokenBudget: parseOptionalInt(e.target.value),
                    });
                    setDirty(true);
                  }}
                />
                <span className="field-hint">
                  dev-auto 连续自主推进累计消耗达到该总 Token 上限时自动停机，防止无人值守超预算。
                </span>
              </div>
            </div>
          </section>

          <section id="settings-sec-hooks" className="settings-section">
            <div className="settings-section-head">
              <div>
                <h2 className="settings-section-title">生命周期 Hooks（{hooks.length}）</h2>
                <div className="field-hint">
                  零 Token 消耗的确定性本地命令护栏：pre_tool 返回非 0 退出码可阻断工具执行；post_tool 输出自动回喂模型；on_stop 在每轮结束时执行。
                </div>
              </div>
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                <button
                  type="button"
                  className="btn btn-sm"
                  onClick={() => {
                    setHooks((prev) => [
                      ...prev,
                      {
                        event: "pre_tool",
                        toolMatcher: "read,edit,write",
                        command:
                          "if ($env:KANZEI_TOOL_INPUT -match '\\.env') { Write-Error '禁止访问 .env 敏感文件'; exit 2 }",
                        timeoutSecs: 10,
                        enabled: true,
                      },
                    ]);
                    setDirty(true);
                  }}
                >
                  + 预设：保护 .env
                </button>
                <button
                  type="button"
                  className="btn btn-sm"
                  onClick={() => {
                    setHooks((prev) => [
                      ...prev,
                      {
                        event: "post_tool",
                        toolMatcher: "edit,write",
                        command: "",
                        timeoutSecs: 15,
                        enabled: true,
                      },
                    ]);
                    setDirty(true);
                  }}
                >
                  <IconPlus size={12} />
                  <span>添加 Hook</span>
                </button>
              </div>
            </div>

            {hooks.length === 0 ? (
              <div className="field-hint">
                暂未配置全局生命周期钩子。支持通过环境变量 `KANZEI_HOOK_EVENT`、`KANZEI_TOOL_NAME`、`KANZEI_TOOL_INPUT`、`KANZEI_TOOL_OUTCOME`、`KANZEI_CWD` 获取上下文。
              </div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                {hooks.map((rule, idx) => (
                  <div key={idx} className="provider-card">
                    <div className="provider-card-head">
                      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                        <select
                          className="select"
                          aria-label={`Hook #${idx + 1} 触发事件`}
                          value={rule.event}
                          onChange={(e) => {
                            const next = [...hooks];
                            next[idx] = {
                              ...rule,
                              event: e.target.value as HookRuleItem["event"],
                            };
                            setHooks(next);
                            setDirty(true);
                          }}
                        >
                          <option value="pre_tool">pre_tool（执行前拦截）</option>
                          <option value="post_tool">post_tool（执行后检查）</option>
                          <option value="on_stop">on_stop（轮末收尾）</option>
                        </select>
                        <input
                          className="input mono"
                          style={{ width: 170 }}
                          placeholder="工具匹配（如 edit,write 或 *）"
                          disabled={rule.event === "on_stop"}
                          value={rule.toolMatcher ?? ""}
                          onChange={(e) => {
                            const next = [...hooks];
                            next[idx] = { ...rule, toolMatcher: e.target.value };
                            setHooks(next);
                            setDirty(true);
                          }}
                        />
                        <input
                          className="input mono"
                          style={{ width: 92 }}
                          type="number"
                          min={1}
                          max={120}
                          placeholder="15s"
                          title="超时秒数（1~120）"
                          value={rule.timeoutSecs ?? ""}
                          onChange={(e) => {
                            const next = [...hooks];
                            next[idx] = {
                              ...rule,
                              timeoutSecs: parseOptionalInt(e.target.value),
                            };
                            setHooks(next);
                            setDirty(true);
                          }}
                        />
                      </div>
                      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                        <button
                          type="button"
                          role="switch"
                          aria-checked={rule.enabled}
                          aria-label={`启用 Hook #${idx + 1}`}
                          className="switch"
                          onClick={() => {
                            const next = [...hooks];
                            next[idx] = { ...rule, enabled: !rule.enabled };
                            setHooks(next);
                            setDirty(true);
                          }}
                        />
                        <button
                          type="button"
                          className="btn btn-ghost btn-sm"
                          style={{ color: "var(--err)" }}
                          onClick={() => {
                            setHooks((prev) => prev.filter((_, i) => i !== idx));
                            setDirty(true);
                          }}
                        >
                          删除
                        </button>
                      </div>
                    </div>
                    <input
                      className="input mono"
                      placeholder="要执行的本地命令，例如 npm run lint 或 cargo check"
                      value={rule.command}
                      onChange={(e) => {
                        const next = [...hooks];
                        next[idx] = { ...rule, command: e.target.value };
                        setHooks(next);
                        setDirty(true);
                      }}
                    />
                  </div>
                ))}
              </div>
            )}
          </section>

          <section id="settings-sec-tools" className="settings-section">
            <div className="settings-section-head">
              <h2 className="settings-section-title">外部打开方式与已记住权限</h2>
              <div style={{ display: "flex", gap: 6 }}>
                <button
                  type="button"
                  className="btn btn-sm"
                  onClick={() =>
                    setCustomTools((prev) => [
                      ...prev,
                      { id: "", label: "自定义编辑器", command: "", args: ["{path}"] },
                    ])
                  }
                >
                  <IconPlus size={12} />
                  <span>添加打开方式</span>
                </button>
                <button
                  type="button"
                  className="btn btn-sm btn-primary"
                  onClick={() => void handleSaveOpenTools()}
                >
                  保存打开方式
                </button>
              </div>
            </div>
            {customTools.map((tool, idx) => (
              <div key={idx} className="settings-grid-2">
                <input
                  className="input"
                  placeholder="名称，如 Cursor"
                  value={tool.label}
                  onChange={(e) => {
                    const next = [...customTools];
                    next[idx] = { ...tool, label: e.target.value };
                    setCustomTools(next);
                  }}
                />
                <input
                  className="input mono"
                  placeholder="可执行程序路径"
                  value={tool.command ?? ""}
                  onChange={(e) => {
                    const next = [...customTools];
                    next[idx] = { ...tool, command: e.target.value };
                    setCustomTools(next);
                  }}
                />
              </div>
            ))}

            <div className="field-label" style={{ marginTop: 6 }}>
              已记住权限规则 ({permRules.length})
              {permRulesPath && (
                <span className="field-hint mono" style={{ marginLeft: 8 }}>
                  {permRulesPath}
                </span>
              )}
            </div>
            {!effectiveProjectDir ? (
              <div className="field-hint">
                {generalMode
                  ? "无项目对话不适用项目级已记住权限规则。"
                  : "请先选择一个项目以查看和管理该项目的已记住权限规则。"}
              </div>
            ) : permRules.length === 0 ? (
              <div className="field-hint">当前项目没有持久化的权限放行规则。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {permRules.map((rule) => (
                  <div key={`${rule.index}:${rule.action}:${rule.resource}`} className="perm-rule-row">
                    <span className="mono" style={{ overflowWrap: "anywhere" }}>
                      [{rule.action}] {rule.resource} ({rule.effect})
                    </span>
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      onClick={() =>
                        void invoke("permission_rule_delete", {
                          projectDir: effectiveProjectDir,
                          index: rule.index,
                          expectedRule: {
                            action: rule.action,
                            resource: rule.resource,
                            effect: rule.effect,
                          },
                        })
                          .then(() => {
                            pushToast("已删除权限规则", "ok");
                            return loadPermissionRules();
                          })
                          .catch((e) => pushToast(formatError(e), "error"))
                      }
                    >
                      删除
                    </button>
                  </div>
                ))}
              </div>
            )}
          </section>

          <section id="settings-sec-agents" className="settings-section">
            <h2 className="settings-section-title">Agent 目录、提醒偏好与快捷键</h2>

            <div className="toggle-row">
              <div className="toggle-row-text">
                <span className="toggle-row-title">完成与等待确认提示音</span>
                <span className="field-hint">
                  在长任务结束或后台会话等待权限审批时播放轻柔提示音
                </span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => playNotificationChime("done")}
                >
                  试听
                </button>
                <button
                  type="button"
                  role="switch"
                  aria-checked={soundEnabled}
                  aria-label="开启任务完成提示音"
                  className="switch"
                  onClick={() => setSoundEnabled(!soundEnabled)}
                />
              </div>
            </div>

            <div className="provider-card">
              <div className="provider-card-title">快捷键速查</div>
              <div className="shortcuts-grid">
                <div className="shortcut-row">
                  <span>命令面板</span>
                  <kbd className="kbd">Ctrl+P</kbd>
                </div>
                <div className="shortcut-row">
                  <span>聚焦输入框</span>
                  <kbd className="kbd">Ctrl+K</kbd>
                </div>
                <div className="shortcut-row">
                  <span>循环切换执行模式</span>
                  <kbd className="kbd">Shift+Tab</kbd>
                </div>
                <div className="shortcut-row">
                  <span>搜索当前对话</span>
                  <kbd className="kbd">Ctrl+F</kbd>
                </div>
                <div className="shortcut-row">
                  <span>新建对话</span>
                  <kbd className="kbd">Ctrl+Shift+N</kbd>
                </div>
                <div className="shortcut-row">
                  <span>清空当前对话段</span>
                  <kbd className="kbd">Ctrl+Shift+C</kbd>
                </div>
                <div className="shortcut-row">
                  <span>输入框补全文件 / 命令 / 技能</span>
                  <kbd className="kbd">@ / $</kbd>
                </div>
                <div className="shortcut-row">
                  <span>翻阅历史指令</span>
                  <kbd className="kbd">↑ / ↓</kbd>
                </div>
              </div>
            </div>

            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              {effectiveProjectDir && (
                <>
                  <button
                    type="button"
                    className="btn"
                    onClick={() =>
                      void invoke("conversation_cleanup", { projectDir: effectiveProjectDir })
                        .then(() => pushToast("已完成存储整理", "ok"))
                        .catch((e) => pushToast(formatError(e), "error"))
                    }
                  >
                    整理当前项目对话存储
                  </button>
                  <button
                    type="button"
                    className="btn"
                    onClick={() =>
                      void (async () => {
                        try {
                          const outputDir = await invoke<string | null>("export_pick_dir");
                          if (!outputDir) return;
                          const res = await invoke<{ path?: string; files?: string[] }>(
                            "export_project_data",
                            {
                              options: {
                                projectDir: effectiveProjectDir,
                                outputDir,
                                includeMemory: true,
                                includeRequirements: true,
                                includeDefects: true,
                                includeConfig: true,
                              },
                            }
                          );
                          pushToast(
                            `已导出 ${res?.files?.length ?? 0} 个工作资料到 ${res?.path ?? outputDir}`,
                            "ok"
                          );
                        } catch (e) {
                          pushToast(`导出失败：${formatError(e)}`, "error");
                        }
                      })()
                    }
                  >
                    导出当前项目工作资料…
                  </button>
                </>
              )}
            </div>

            <div className="field-label">
              已注册 Agent 定义 ({agentsInfo?.agents?.length ?? 0})
              {agentsInfo?.profile && (
                <span className="field-hint" style={{ marginLeft: 8 }}>
                  当前档位：{profileDisplayLabel(agentsInfo.profile)}
                </span>
              )}
            </div>
            {agentsInfo?.agents && agentsInfo.agents.length > 0 ? (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {agentsInfo.agents.map((agent, idx) => {
                  const st = agentStatusLabel(agent.status);
                  return (
                    <div key={`${agent.name}:${agent.path ?? idx}`} className="provider-card">
                      <div className="provider-card-head">
                        <div className="provider-card-title">
                          <span className="mono">{agent.name}</span>
                          <span className={`badge ${st.badge}`}>{st.text}</span>
                          <span className="badge">{agent.mode === "subagent" ? "子代理" : "主代理"}</span>
                          <span className="badge badge-info">{agent.source}</span>
                        </div>
                        {agent.path && (
                          <button
                            type="button"
                            className="btn btn-ghost btn-sm"
                            onClick={() =>
                              void invoke("agent_directory_open", {
                                path: agent.path!,
                                projectDir: effectiveProjectDir,
                              }).catch((e) => pushToast(formatError(e), "error"))
                            }
                          >
                            <IconExternal size={11} />
                            <span>打开原文</span>
                          </button>
                        )}
                      </div>
                      <div className="field-hint mono">
                        档位：{agent.profile || "all"} · 模型：{agent.model || "primary"} · 步数上限：
                        {agent.steps && agent.steps > 0 ? agent.steps : "无上限"}
                      </div>
                      {agent.error && (
                        <div className="field-hint" style={{ color: "var(--err)" }}>
                          配置错误：{agent.error}
                        </div>
                      )}
                      {agent.systemPreview && (
                        <div className="field-hint" style={{ whiteSpace: "pre-wrap" }}>
                          {agent.systemPreview}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            ) : (
              <div className="field-hint">暂未发现 Agent 定义。</div>
            )}
          </section>

          <section id="settings-sec-update" className="settings-section">
            <h2 className="settings-section-title">版本与更新</h2>
            <div className="provider-card">
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                  gap: 12,
                }}
              >
                <div>
                  <div className="provider-card-title mono">kanzei v{appInfo.version}</div>
                  <div className="field-hint mono" style={{ marginTop: 3 }}>
                    Build: {appInfo.build}
                  </div>
                </div>
                <div style={{ display: "flex", gap: 8 }}>
                  <button
                    type="button"
                    className="btn"
                    disabled={updateBusy}
                    onClick={() => void handleCheckUpdate()}
                  >
                    {updateBusy ? "检查中…" : "检查更新"}
                  </button>
                  {Boolean(updateInfo?.newer) && (
                    <button
                      type="button"
                      className="btn btn-accent"
                      disabled={updateBusy}
                      onClick={() => void handleInstallUpdate()}
                    >
                      下载并安装 {String(updateInfo?.latest ?? "")}
                    </button>
                  )}
                </div>
              </div>
              {updateInfo && (
                <div className="field-hint">
                  状态：{String(updateInfo.status ?? "")}
                  {updateInfo.latest ? ` · 最新发布：${String(updateInfo.latest)}` : ""}
                  {updateInfo.message ? ` · ${String(updateInfo.message)}` : ""}
                </div>
              )}
              {updateReceipt && (
                <div className="field-hint">
                  上次更新回执：阶段 {String(updateReceipt.phase ?? "-")} · 目标构建{" "}
                  {String(updateReceipt.expected_build ?? "-")} ·{" "}
                  {String(updateReceipt.message ?? "")}
                </div>
              )}
            </div>
          </section>
        </div>
      </div>

      <div className="settings-savebar">
        <span className="settings-savebar-status">
          <span className={`status-dot${dirty ? " running" : " ok"}`} />
          <span>{dirty ? "存在未保存的全局设置修改" : "所有设置已同步"}</span>
        </span>
        <div style={{ display: "flex", gap: 8 }}>
          <button
            type="button"
            className="btn"
            disabled={!dirty || saving}
            onClick={() => void loadAll()}
          >
            放弃修改
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={!dirty || saving}
            onClick={() => void handleSave()}
          >
            {saving ? "保存中…" : "保存全局设置"}
          </button>
        </div>
      </div>
    </div>
  );
}
