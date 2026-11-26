import React, { useCallback, useEffect, useRef, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import type { OpenToolInfo } from "../../ipc/types";
import { loadModelsCatalog, loadOpenTools, useAppStore } from "../../store/appStore";
import { pushToast } from "../../store/uiStore";
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
  auth?: string | null;
  contextLimit?: number | null;
  builtin?: boolean;
  source?: string;
}

interface LimitsForm {
  maxTokens?: number | null;
  subagentMaxTokens?: number | null;
  subagentTimeoutSecs?: number | null;
  subagentGlobalConcurrency?: number | null;
  contextBudgetRatio?: number | null;
  maxParallelTools?: number | null;
  transportRetries?: number | null;
}

interface CadenceForm {
  full_test?: string | null;
  full_test_batches?: number | null;
  targeted_test?: string | null;
  commit?: string | null;
  push?: string | null;
}

interface PermissionRuleItem {
  index?: number;
  scope?: string;
  action?: string;
  pattern?: string;
  effect?: string;
}

const TOC_ITEMS: Array<{ id: string; label: string; icon: React.ReactElement }> = [
  { id: "models", label: "模型与快速模型", icon: <IconCpu size={14} /> },
  { id: "providers", label: "模型服务商", icon: <IconLayers size={14} /> },
  { id: "network", label: "网络与默认", icon: <IconGlobe size={14} /> },
  { id: "limits", label: "高级运行参数", icon: <IconSliders size={14} /> },
  { id: "tools", label: "打开方式与权限", icon: <IconShield size={14} /> },
  { id: "agents", label: "Agent 与数据", icon: <IconBot size={14} /> },
  { id: "update", label: "版本与更新", icon: <IconDownload size={14} /> },
];

export default function SettingsView(): React.ReactElement {
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const modelsCatalog = useAppStore((s) => s.modelsCatalog);
  const openTools = useAppStore((s) => s.openTools);
  const appInfo = useAppStore((s) => s.appInfo);

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
  const [cadence, setCadence] = useState<CadenceForm>({});
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);

  const [fastStatus, setFastStatus] = useState<Record<string, unknown> | null>(null);
  const [fastSettingUp, setFastSettingUp] = useState(false);
  const [permRules, setPermRules] = useState<PermissionRuleItem[]>([]);
  const [updateInfo, setUpdateInfo] = useState<Record<string, unknown> | null>(null);
  const [updateReceipt, setUpdateReceipt] = useState<Record<string, unknown> | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [customTools, setCustomTools] = useState<OpenToolInfo[]>([]);
  const [agentsInfo, setAgentsInfo] = useState<Record<string, unknown> | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const loadAll = useCallback(async () => {
    try {
      const data = await invoke<Record<string, unknown>>("settings_get", {
        projectDir: activeProjectDir,
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
      setCadence((data.cadence as CadenceForm) ?? {});
      setDirty(false);
    } catch (error) {
      pushToast(`读取设置失败：${formatError(error)}`, "error");
    }
  }, [activeProjectDir]);

  useEffect(() => {
    void loadAll();
    void loadOpenTools();
    invoke<Record<string, unknown>>("fast_model_status").then(setFastStatus).catch(() => {});
    if (activeProjectDir) {
      invoke<PermissionRuleItem[]>("permission_rules_get", { projectDir: activeProjectDir })
        .then((list) => setPermRules(Array.isArray(list) ? list : []))
        .catch(() => {});
    } else {
      setPermRules([]);
    }
    invoke<Record<string, unknown>>("agent_directory_get", { projectDir: activeProjectDir })
      .then(setAgentsInfo)
      .catch(() => {});
    invoke<Record<string, unknown> | null>("update_receipt")
      .then(setUpdateReceipt)
      .catch(() => {});
  }, [loadAll, activeProjectDir]);

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
            apiKeyEnv: p.apiKeyEnv || null,
            apiKey: p.apiKey || null,
            auth: p.auth || null,
            contextLimit: p.contextLimit ? Number(p.contextLimit) : null,
          })),
          limits,
          cadence,
        },
      });
      setDirty(false);
      pushToast("全局设置已保存", "ok");
      void loadModelsCatalog(activeProjectDir, true);
    } catch (error) {
      pushToast(`保存设置失败：${formatError(error)}`, "error");
    } finally {
      setSaving(false);
    }
  };

  const testProvider = async (p: ProviderRow) => {
    try {
      const res = await invoke<{ message?: string; ok?: boolean }>("provider_test", {
        protocol: p.protocol,
        baseUrl: p.baseUrl,
        apiKeyEnv: p.apiKeyEnv || null,
        apiKey: p.apiKey || null,
        auth: p.auth || null,
        proxy: proxy || null,
      });
      pushToast(res?.message ?? `${p.name} 连通正常`, "ok");
    } catch (error) {
      pushToast(`${p.name} 测试失败：${formatError(error)}`, "error");
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
            onClick={() =>
              void loadModelsCatalog(activeProjectDir, true).then(() => pushToast("已刷新模型目录", "ok"))
            }
          >
            <IconRefresh size={13} />
            <span>刷新模型目录</span>
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
                document.getElementById(`settings-sec-${item.id}`)?.scrollIntoView({ behavior: "smooth" });
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
                <label className="field-label">主模型（primary）</label>
                <input
                  className="input mono"
                  list="kz-settings-models"
                  value={primary}
                  onChange={(e) => {
                    setPrimary(e.target.value);
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">快速模型（fast）</label>
                <input
                  className="input mono"
                  list="kz-settings-models"
                  value={fast}
                  onChange={(e) => {
                    setFast(e.target.value);
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">压缩模型（compact，留空跟随主模型）</label>
                <input
                  className="input mono"
                  list="kz-settings-models"
                  value={compact}
                  onChange={(e) => {
                    setCompact(e.target.value);
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">默认思考强度（reasoning）</label>
                <select
                  className="select"
                  value={reasoning}
                  onChange={(e) => {
                    setReasoning(e.target.value);
                    setDirty(true);
                  }}
                >
                  <option value="off">off</option>
                  <option value="low">low</option>
                  <option value="medium">medium</option>
                  <option value="high">high</option>
                  <option value="xhigh">xhigh</option>
                </select>
              </div>
            </div>

            <div className="toggle-row">
              <div className="toggle-row-text">
                <span className="toggle-row-title">Codex Fast Mode</span>
                <span className="field-hint">在支持的 Codex 模型上优先采用低延迟响应通道</span>
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
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12 }}>
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
                        : `安装:${String(fastStatus.installed)} · 服务:${String(fastStatus.serviceUp)} · 模型:${String(fastStatus.modelPresent)}`
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
            <datalist id="kz-settings-models">
              {modelsCatalog.map((m) => (
                <option key={m.id} value={m.id} />
              ))}
            </datalist>
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
                    { name: "", protocol: "openai", baseUrl: "https://", apiKeyEnv: "", apiKey: "" },
                  ]);
                  setDirty(true);
                }}
              >
                <IconPlus size={12} />
                <span>添加服务商</span>
              </button>
            </div>

            {providers.map((p, idx) => (
              <div key={idx} className="provider-card">
                <div className="provider-card-head">
                  <div className="provider-card-title">
                    <span className="mono">{p.name || "未命名服务商"}</span>
                    <span className="badge">{p.protocol}</span>
                    <span className="badge badge-info">{p.source ?? "global"}</span>
                  </div>
                  <div style={{ display: "flex", gap: 6 }}>
                    <button type="button" className="btn btn-sm" onClick={() => void testProvider(p)}>
                      连通性测试
                    </button>
                    {!p.builtin && (
                      <button
                        type="button"
                        className="btn btn-danger btn-sm"
                        onClick={() => {
                          setProviders((prev) => prev.filter((_, i) => i !== idx));
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
                    <label className="field-label">API Key 或环境变量名</label>
                    <input
                      className="input mono"
                      type="password"
                      placeholder={p.apiKeyEnv ? `环境变量: ${p.apiKeyEnv}` : "直填 API Key"}
                      value={p.apiKey ?? ""}
                      onChange={(e) => {
                        const next = [...providers];
                        next[idx] = { ...p, apiKey: e.target.value };
                        setProviders(next);
                        setDirty(true);
                      }}
                    />
                  </div>
                </div>
              </div>
            ))}
          </section>

          <section id="settings-sec-network" className="settings-section">
            <h2 className="settings-section-title">网络、默认与验证提交节奏</h2>
            <div className="settings-grid-2">
              <div className="field-group">
                <label className="field-label">代理模式（env / off / 显式代理 URL）</label>
                <input
                  className="input mono"
                  value={proxy}
                  onChange={(e) => {
                    setProxy(e.target.value);
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">默认执行档位</label>
                <select
                  className="select"
                  value={profileDefault}
                  onChange={(e) => {
                    setProfileDefault(e.target.value);
                    setDirty(true);
                  }}
                >
                  <option value="dev-auto">自主开发 (dev-auto)</option>
                  <option value="dev-pair">结伴开发 (dev-pair)</option>
                  <option value="isolated">只读讨论 (isolated)</option>
                </select>
              </div>
              <div className="field-group">
                <label className="field-label">提交节奏（commit）</label>
                <select
                  className="select"
                  value={cadence.commit ?? ""}
                  onChange={(e) => {
                    setCadence({ ...cadence, commit: e.target.value || null });
                    setDirty(true);
                  }}
                >
                  <option value="">内置默认</option>
                  <option value="per_batch">每批提交 (per_batch)</option>
                  <option value="per_entry">每项提交 (per_entry)</option>
                  <option value="manual">手动提交 (manual)</option>
                </select>
              </div>
              <div className="field-group">
                <label className="field-label">推送节奏（push）</label>
                <select
                  className="select"
                  value={cadence.push ?? ""}
                  onChange={(e) => {
                    setCadence({ ...cadence, push: e.target.value || null });
                    setDirty(true);
                  }}
                >
                  <option value="">内置默认</option>
                  <option value="manual">仅手动推送 (manual)</option>
                  <option value="per_commit">每次提交后推送 (per_commit)</option>
                  <option value="periodic">定期推送 (periodic)</option>
                </select>
              </div>
            </div>
          </section>

          <section id="settings-sec-limits" className="settings-section">
            <h2 className="settings-section-title">高级运行参数</h2>
            <div className="settings-grid-2">
              <div className="field-group">
                <label className="field-label">主模型最大输出 Tokens</label>
                <input
                  className="input mono"
                  type="number"
                  value={limits.maxTokens ?? ""}
                  onChange={(e) => {
                    setLimits({ ...limits, maxTokens: e.target.value ? Number(e.target.value) : null });
                    setDirty(true);
                  }}
                />
              </div>
              <div className="field-group">
                <label className="field-label">子代理全局并发上限</label>
                <input
                  className="input mono"
                  type="number"
                  value={limits.subagentGlobalConcurrency ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      subagentGlobalConcurrency: e.target.value ? Number(e.target.value) : null,
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
                  value={limits.subagentTimeoutSecs ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      subagentTimeoutSecs: e.target.value ? Number(e.target.value) : null,
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
                  value={limits.maxParallelTools ?? ""}
                  onChange={(e) => {
                    setLimits({
                      ...limits,
                      maxParallelTools: e.target.value ? Number(e.target.value) : null,
                    });
                    setDirty(true);
                  }}
                />
              </div>
            </div>
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
                <button type="button" className="btn btn-sm btn-primary" onClick={() => void handleSaveOpenTools()}>
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
            </div>
            {permRules.length === 0 ? (
              <div className="field-hint">当前没有持久化的权限放行规则。</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {permRules.map((rule, idx) => (
                  <div key={idx} className="perm-rule-row">
                    <span className="mono">
                      [{rule.scope}] {rule.action} → {rule.pattern} ({rule.effect})
                    </span>
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      disabled={!activeProjectDir}
                      onClick={() =>
                        activeProjectDir &&
                        void invoke("permission_rule_delete", {
                          projectDir: activeProjectDir,
                          index: rule.index ?? idx,
                          expectedRule: rule,
                        })
                          .then(() => setPermRules((prev) => prev.filter((_, i) => i !== idx)))
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
            <h2 className="settings-section-title">Agent 目录与存储整理</h2>
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              <button
                type="button"
                className="btn"
                onClick={() => {
                  const globalDir =
                    (agentsInfo?.globalDir as string | undefined) ??
                    (agentsInfo?.global_dir as string | undefined) ??
                    "";
                  if (!globalDir) {
                    pushToast("未获取到全局 Agents 目录路径", "warn");
                    return;
                  }
                  void invoke("agent_directory_open", {
                    path: globalDir,
                    projectDir: activeProjectDir,
                  }).catch((e) => pushToast(formatError(e), "error"));
                }}
              >
                打开全局 Agents 目录
              </button>
              {activeProjectDir && (
                <>
                  <button
                    type="button"
                    className="btn"
                    onClick={() =>
                      void invoke("conversation_cleanup", { projectDir: activeProjectDir })
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
                                projectDir: activeProjectDir,
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
            {agentsInfo && (
              <div className="field-hint">
                已发现 Agent 定义：{Array.isArray(agentsInfo.agents) ? agentsInfo.agents.length : 0} 个
              </div>
            )}
          </section>

          <section id="settings-sec-update" className="settings-section">
            <h2 className="settings-section-title">版本与更新</h2>
            <div className="provider-card">
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12 }}>
                <div>
                  <div className="provider-card-title mono">
                    kanzei v{appInfo.version}
                  </div>
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
                  {String(updateReceipt.expected_build ?? "-")} · {String(updateReceipt.message ?? "")}
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
          <button type="button" className="btn" disabled={!dirty || saving} onClick={() => void loadAll()}>
            放弃修改
          </button>
          <button type="button" className="btn btn-primary" disabled={!dirty || saving} onClick={() => void handleSave()}>
            {saving ? "保存中…" : "保存全局设置"}
          </button>
        </div>
      </div>
    </div>
  );
}
