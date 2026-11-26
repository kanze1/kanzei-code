import React, { useEffect, useState } from "react";
import { formatError, invoke } from "../ipc/client";
import type { ProjectModelFieldView, ProjectModelsResponse } from "../ipc/types";
import {
  REASONING_OPTIONS,
  sourceDisplayLabel,
  withFallbackOption,
} from "../configOptions";
import {
  appStore,
  createNewProject,
  loadModelsCatalog,
  projectDisplayName,
  useAppStore,
} from "../store/appStore";
import { refreshActiveSessionMeta } from "../store/sessionStore";
import {
  closeProjectModelsDialog,
  closeViewer,
  dismissToast,
  pushToast,
  resolveConfirmDialog,
  resolveInputDialog,
  resolveRewindDialog,
  uiStore,
  useUiStore,
} from "../store/uiStore";
import { IconClose } from "./Icons";
import { MarkdownView } from "./MarkdownView";
import { ModelCombobox } from "./ModelCombobox";

export function ToastRegion(): React.ReactElement {
  const toasts = useUiStore((s) => s.toasts);
  return (
    <div className="toast-region" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast-item toast-${t.kind}`} role="status">
          <span style={{ flex: 1 }}>{t.text}</span>
          <button
            type="button"
            className="btn btn-ghost btn-icon btn-sm"
            onClick={() => dismissToast(t.id)}
            aria-label="关闭通知"
          >
            <IconClose size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}

export function ConfirmDialogHost(): React.ReactElement | null {
  const req = useUiStore((s) => s.confirmDialog);
  if (!req) return null;
  return (
    <div
      className="modal-scrim"
      role="dialog"
      aria-modal="true"
      aria-label={req.title}
      onClick={() => resolveConfirmDialog(false)}
    >
      <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div className="modal-title">{req.title}</div>
          <button
            type="button"
            className="btn btn-ghost btn-icon"
            onClick={() => resolveConfirmDialog(false)}
            aria-label="关闭"
          >
            <IconClose size={14} />
          </button>
        </div>
        <div className="modal-body">
          <p style={{ whiteSpace: "pre-wrap", fontSize: 13, lineHeight: 1.6 }}>{req.message}</p>
        </div>
        <div className="modal-footer">
          <button type="button" className="btn" onClick={() => resolveConfirmDialog(false)}>
            {req.cancelLabel ?? "取消"}
          </button>
          <button
            type="button"
            className={req.danger ? "btn btn-danger" : "btn btn-primary"}
            onClick={() => resolveConfirmDialog(true)}
          >
            {req.confirmLabel ?? "确认"}
          </button>
        </div>
      </div>
    </div>
  );
}

export function InputDialogHost(): React.ReactElement | null {
  const req = useUiStore((s) => s.inputDialog);
  const [value, setValue] = useState("");

  useEffect(() => {
    setValue(req?.initialValue ?? "");
  }, [req]);

  if (!req) return null;
  const submit = () => resolveInputDialog(value.trim());

  return (
    <div
      className="modal-scrim"
      role="dialog"
      aria-modal="true"
      aria-label={req.title}
      onClick={() => resolveInputDialog(null)}
    >
      <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div className="modal-title">{req.title}</div>
          <button
            type="button"
            className="btn btn-ghost btn-icon"
            onClick={() => resolveInputDialog(null)}
            aria-label="关闭"
          >
            <IconClose size={14} />
          </button>
        </div>
        <div className="modal-body">
          <div className="field-group">
            {req.label && <label className="field-label">{req.label}</label>}
            <input
              autoFocus
              className="input"
              value={value}
              placeholder={req.placeholder}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
                if (e.key === "Escape") resolveInputDialog(null);
              }}
            />
          </div>
        </div>
        <div className="modal-footer">
          <button type="button" className="btn" onClick={() => resolveInputDialog(null)}>
            取消
          </button>
          <button type="button" className="btn btn-primary" onClick={submit}>
            {req.confirmLabel ?? "确定"}
          </button>
        </div>
      </div>
    </div>
  );
}

export function NewProjectDialogHost(): React.ReactElement | null {
  const open = useUiStore((s) => s.newProjectOpen);
  const [name, setName] = useState("");
  const [dirPath, setDirPath] = useState("");
  const [gitInit, setGitInit] = useState(true);
  const [description, setDescription] = useState("");
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const close = () => uiStore.setState({ newProjectOpen: false });
  const pickFolder = async () => {
    try {
      const picked = await invoke<string | null>("projects_pick");
      if (picked) setDirPath(picked);
    } catch (error) {
      pushToast(`选择目录失败：${formatError(error)}`, "error");
    }
  };
  const submit = async () => {
    if (!dirPath.trim() || !name.trim()) {
      pushToast("请填写项目名称和目标目录", "warn");
      return;
    }
    setBusy(true);
    const ok = await createNewProject({
      path: dirPath.trim(),
      name: name.trim(),
      gitInit,
      description: description.trim(),
    });
    setBusy(false);
    if (ok) {
      setName("");
      setDirPath("");
      setDescription("");
      close();
    }
  };

  return (
    <div className="modal-scrim" role="dialog" aria-modal="true" aria-label="新建项目" onClick={close}>
      <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div className="modal-title">新建项目</div>
          <button type="button" className="btn btn-ghost btn-icon" onClick={close} aria-label="关闭">
            <IconClose size={14} />
          </button>
        </div>
        <div className="modal-body">
          <div className="field-group">
            <label className="field-label">项目名称</label>
            <input
              autoFocus
              className="input"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="例如：my-service"
            />
          </div>
          <div className="field-group">
            <label className="field-label">项目目录（完整路径）</label>
            <div style={{ display: "flex", gap: 8 }}>
              <input
                className="input mono"
                value={dirPath}
                onChange={(e) => setDirPath(e.target.value)}
                placeholder="C:\Users\...\my-service"
              />
              <button type="button" className="btn" onClick={() => void pickFolder()}>
                浏览…
              </button>
            </div>
          </div>
          <div className="toggle-row">
            <div className="toggle-row-text">
              <span className="toggle-row-title">初始化独立 Git 仓库与首提交</span>
              <span className="field-hint">启用右侧文件 Diff 对照与安全回滚能力</span>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={gitInit}
              className="switch"
              onClick={() => setGitInit(!gitInit)}
            />
          </div>
          <div className="field-group">
            <label className="field-label">项目目标描述（可选）</label>
            <textarea
              className="textarea"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="简要描述这个项目要解决什么问题…"
            />
          </div>
        </div>
        <div className="modal-footer">
          <button type="button" className="btn" onClick={close} disabled={busy}>
            取消
          </button>
          <button type="button" className="btn btn-primary" onClick={() => void submit()} disabled={busy}>
            {busy ? "创建中…" : "创建并打开"}
          </button>
        </div>
      </div>
    </div>
  );
}

function describeInherited(
  field: ProjectModelFieldView<string | null> | undefined,
  fallback = "未设置"
): string {
  if (!field) return fallback;
  const val = field.inherited ?? fallback;
  if (field.inheritedFollowsPrimary) return `跟随主模型：${val}`;
  return `继承${sourceDisplayLabel(field.inheritedSource)}：${val}`;
}

function describeInheritedBool(field: ProjectModelFieldView<boolean | null> | undefined): string {
  if (!field) return "继承默认";
  const val = field.inherited ? "开" : "关";
  return `继承${sourceDisplayLabel(field.inheritedSource)}（${val}）`;
}

export function ProjectModelsDialogHost(): React.ReactElement | null {
  const open = useUiStore((s) => s.projectModelsOpen);
  const targetDirOverride = useUiStore((s) => s.projectModelsTargetDir);
  const activeProjectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const modelsCatalog = useAppStore((s) => s.modelsCatalog);
  const modelsLoading = useAppStore((s) => s.modelsLoading);

  const projectDir = targetDirOverride ?? (!generalMode ? activeProjectDir : null);

  const [fieldsMeta, setFieldsMeta] = useState<ProjectModelsResponse["fields"]>({});
  const [configPath, setConfigPath] = useState("");
  const [configExists, setConfigExists] = useState(false);
  const [loaded, setLoaded] = useState<{
    primary: string;
    fast: string;
    compact: string;
    reasoning: string;
    codexFastMode: "" | "on" | "off";
  }>({ primary: "", fast: "", compact: "", reasoning: "", codexFastMode: "" });

  const [primary, setPrimary] = useState("");
  const [fast, setFast] = useState("");
  const [compact, setCompact] = useState("");
  const [reasoning, setReasoning] = useState("");
  const [codexFastMode, setCodexFastMode] = useState<"" | "on" | "off">("");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open || !projectDir) return;
    let cancelled = false;
    setLoading(true);
    const { modelsCatalog: curModels, modelsLoading: curLoading } = appStore.getState();
    if (curModels.length === 0 && !curLoading) {
      void loadModelsCatalog(projectDir, false);
    }
    invoke<ProjectModelsResponse>("project_models_get", { projectDir })
      .then((data) => {
        if (cancelled) return;
        const fields = data?.fields ?? {};
        const nextLoaded = {
          primary: fields.primary?.project ?? "",
          fast: fields.fast?.project ?? "",
          compact: fields.compact?.project ?? "",
          reasoning: fields.reasoning?.project ?? "",
          codexFastMode:
            fields.codexFastMode?.project === true
              ? ("on" as const)
              : fields.codexFastMode?.project === false
              ? ("off" as const)
              : ("" as const),
        };
        setFieldsMeta(fields);
        setConfigPath(data?.configPath ?? "");
        setConfigExists(Boolean(data?.exists));
        setLoaded(nextLoaded);
        setPrimary(nextLoaded.primary);
        setFast(nextLoaded.fast);
        setCompact(nextLoaded.compact);
        setReasoning(nextLoaded.reasoning);
        setCodexFastMode(nextLoaded.codexFastMode);
      })
      .catch((err) => {
        if (!cancelled) pushToast(`读取项目模型覆盖失败：${formatError(err)}`, "error");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, projectDir]);

  if (!open || !projectDir) return null;
  const close = () => closeProjectModelsDialog();

  const save = async () => {
    const nextDraft = {
      primary: primary.trim(),
      fast: fast.trim(),
      compact: compact.trim(),
      reasoning: reasoning.trim(),
      codexFastMode,
    };
    const set: Record<string, string | boolean> = {};
    const unset: string[] = [];
    for (const key of ["primary", "fast", "compact", "reasoning"] as const) {
      if (nextDraft[key] === loaded[key]) continue;
      if (nextDraft[key]) set[key] = nextDraft[key];
      else unset.push(key);
    }
    if (nextDraft.codexFastMode !== loaded.codexFastMode) {
      if (nextDraft.codexFastMode === "") unset.push("codexFastMode");
      else set.codexFastMode = nextDraft.codexFastMode === "on";
    }
    if (Object.keys(set).length === 0 && unset.length === 0) {
      close();
      return;
    }
    setSaving(true);
    try {
      await invoke("project_models_save", {
        projectDir,
        set,
        unset,
      });
      await refreshActiveSessionMeta();
      pushToast("已保存本项目模型覆盖", "ok");
      close();
    } catch (error) {
      pushToast(`保存失败：${formatError(error)}`, "error");
    } finally {
      setSaving(false);
    }
  };

  const reasoningOptions = withFallbackOption(REASONING_OPTIONS, reasoning);

  return (
    <div
      className="modal-scrim"
      role="dialog"
      aria-modal="true"
      aria-label="本项目模型配置"
      onClick={close}
    >
      <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div>
            <div className="modal-title">本项目模型覆盖 · {projectDisplayName(projectDir)}</div>
            {configPath && <div className="field-hint mono">{configPath}</div>}
          </div>
          <button type="button" className="btn btn-ghost btn-icon" onClick={close} aria-label="关闭">
            <IconClose size={14} />
          </button>
        </div>
        <div className="modal-body">
          <p className="field-hint">
            留空即继承全局设置；仅提交本次修改的字段到当前项目的 `.kanzei/kanzei.toml`。
          </p>
          {loading ? (
            <p className="field-hint">正在读取…</p>
          ) : (
            <>
              <div className="field-group">
                <label className="field-label" htmlFor="pm-primary">
                  主模型（primary）
                </label>
                <ModelCombobox
                  id="pm-primary"
                  ariaLabel="项目主模型（primary）"
                  value={primary}
                  models={modelsCatalog}
                  loading={modelsLoading}
                  allowEmpty
                  emptyLabel={describeInherited(fieldsMeta.primary)}
                  placeholder={describeInherited(fieldsMeta.primary, "留空继承全局")}
                  onOpen={() => {
                    if (modelsCatalog.length === 0 && !modelsLoading) {
                      void loadModelsCatalog(projectDir, false);
                    }
                  }}
                  onRefresh={() => void loadModelsCatalog(projectDir, true)}
                  onChange={setPrimary}
                />
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="pm-fast">
                  快速模型（fast）
                </label>
                <ModelCombobox
                  id="pm-fast"
                  ariaLabel="项目快速模型（fast）"
                  value={fast}
                  models={modelsCatalog}
                  loading={modelsLoading}
                  allowEmpty
                  emptyLabel={describeInherited(fieldsMeta.fast)}
                  placeholder={describeInherited(fieldsMeta.fast, "留空继承全局")}
                  onOpen={() => {
                    if (modelsCatalog.length === 0 && !modelsLoading) {
                      void loadModelsCatalog(projectDir, false);
                    }
                  }}
                  onRefresh={() => void loadModelsCatalog(projectDir, true)}
                  onChange={setFast}
                />
              </div>
              <div className="field-group">
                <label className="field-label" htmlFor="pm-compact">
                  压缩模型（compact）
                </label>
                <ModelCombobox
                  id="pm-compact"
                  ariaLabel="项目压缩模型（compact）"
                  value={compact}
                  models={modelsCatalog}
                  loading={modelsLoading}
                  allowEmpty
                  emptyLabel={describeInherited(fieldsMeta.compact)}
                  placeholder={describeInherited(fieldsMeta.compact, "留空跟随主模型")}
                  onOpen={() => {
                    if (modelsCatalog.length === 0 && !modelsLoading) {
                      void loadModelsCatalog(projectDir, false);
                    }
                  }}
                  onRefresh={() => void loadModelsCatalog(projectDir, true)}
                  onChange={setCompact}
                />
              </div>
              <div className="settings-grid-2">
                <div className="field-group">
                  <label className="field-label" htmlFor="pm-reasoning">
                    思考强度（reasoning）
                  </label>
                  <select
                    id="pm-reasoning"
                    className="select"
                    value={reasoning}
                    onChange={(e) => setReasoning(e.target.value)}
                  >
                    <option value="">{describeInherited(fieldsMeta.reasoning, "继承全局")}</option>
                    {reasoningOptions.map((opt) => (
                      <option key={opt.value} value={opt.value}>
                        {opt.label}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field-group">
                  <label className="field-label" htmlFor="pm-codex-fast">
                    Codex Fast Mode
                  </label>
                  <select
                    id="pm-codex-fast"
                    className="select"
                    value={codexFastMode}
                    onChange={(e) =>
                      setCodexFastMode(e.target.value as "" | "on" | "off")
                    }
                  >
                    <option value="">{describeInheritedBool(fieldsMeta.codexFastMode)}</option>
                    <option value="on">开启（本项目覆盖）</option>
                    <option value="off">关闭（本项目覆盖）</option>
                  </select>
                </div>
              </div>
            </>
          )}
        </div>
        <div className="modal-footer">
          <button
            type="button"
            className="btn btn-ghost"
            style={{ marginRight: "auto" }}
            disabled={!configExists}
            title={
              configExists
                ? `打开 ${configPath}`
                : "项目配置文件尚未创建，保存一次覆盖后会自动生成"
            }
            onClick={() =>
              void invoke("project_config_open", { projectDir }).catch((err) =>
                pushToast(formatError(err), "error")
              )
            }
          >
            打开配置文件
          </button>
          <button type="button" className="btn" onClick={close} disabled={saving}>
            取消
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void save()}
            disabled={loading || saving}
          >
            {saving ? "保存中…" : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}

export function RewindDialogHost(): React.ReactElement | null {
  const req = useUiStore((s) => s.rewindDialog);
  const [force, setForce] = useState(false);

  useEffect(() => {
    setForce(false);
  }, [req]);

  if (!req) return null;
  const { preview } = req;
  const files = Array.isArray(preview.files) ? preview.files : [];
  const unhandled = Array.isArray(preview.unhandled) ? preview.unhandled : [];
  const hasCodeChanges = files.length > 0;

  return (
    <div
      className="modal-scrim"
      role="dialog"
      aria-modal="true"
      aria-label="检查点回退（Checkpoint）"
      onClick={() => resolveRewindDialog(null)}
    >
      <div className="modal-panel" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div className="modal-title">检查点回退（Checkpoint）</div>
          <button
            type="button"
            className="btn btn-ghost btn-icon"
            onClick={() => resolveRewindDialog(null)}
            aria-label="关闭"
          >
            <IconClose size={14} />
          </button>
        </div>
        <div className="modal-body">
          <p className="field-hint">
            回退后将保留截至该节点前的 <strong>{preview.keptMessages}</strong> 条历史消息，并将原提问重新填回输入框。
          </p>
          <div className="provider-card" style={{ padding: 10 }}>
            <div className="field-label">目标节点提示词</div>
            <div style={{ fontSize: 12.5, whiteSpace: "pre-wrap", maxHeight: 96, overflowY: "auto" }}>
              {preview.prompt || req.text}
            </div>
          </div>

          {hasCodeChanges ? (
            <div className="field-group">
              <div className="field-label">可还原的文件改动 ({files.length})</div>
              <div className="rewind-file-list">
                {files.map((item, idx) => {
                  const path = typeof item === "string" ? item : String(item.path ?? `文件 #${idx + 1}`);
                  const act = typeof item === "string" ? "" : String(item.action ?? "");
                  return (
                    <div key={idx} className="perm-rule-row">
                      <span className="mono">{path}</span>
                      {act && <span className="badge">{act}</span>}
                    </div>
                  );
                })}
              </div>
            </div>
          ) : (
            <div className="field-hint">该检查点之后没有可追踪的文件改动，仅支持回退对话上下文。</div>
          )}

          {unhandled.length > 0 && (
            <div className="settings-notice settings-notice-warn">
              <span>以下变更无法自动还原（可能由外部命令产生）：</span>
              <ul style={{ margin: 0, paddingLeft: 18 }}>
                {unhandled.map((u, i) => (
                  <li key={i} className="mono">
                    {typeof u === "string" ? u : `${u.path ?? ""} ${u.reason ?? ""}`.trim()}
                  </li>
                ))}
              </ul>
              <label style={{ display: "inline-flex", alignItems: "center", gap: 6, marginTop: 4 }}>
                <input
                  type="checkbox"
                  checked={force}
                  onChange={(e) => setForce(e.target.checked)}
                />
                <span>忽略冲突强制还原代码文件</span>
              </label>
            </div>
          )}
        </div>
        <div className="modal-footer" style={{ flexWrap: "wrap" }}>
          <button type="button" className="btn" onClick={() => resolveRewindDialog(null)}>
            取消
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => resolveRewindDialog({ action: "conversation" })}
          >
            仅回退对话
          </button>
          {hasCodeChanges && (
            <>
              <button
                type="button"
                className="btn"
                onClick={() => resolveRewindDialog({ action: "code", force })}
              >
                仅还原代码
              </button>
              <button
                type="button"
                className="btn btn-danger"
                onClick={() => resolveRewindDialog({ action: "both", force })}
              >
                对话与代码一起回退
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

export function ViewerDialogHost(): React.ReactElement | null {
  const viewer = useUiStore((s) => s.viewer);
  if (!viewer) return null;
  return (
    <div className="modal-scrim" role="dialog" aria-modal="true" aria-label={viewer.title} onClick={closeViewer}>
      <div className="modal-panel modal-lg" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div>
            <div className="modal-title">{viewer.title}</div>
            {viewer.subtitle && <div className="field-hint">{viewer.subtitle}</div>}
          </div>
          <button type="button" className="btn btn-ghost btn-icon" onClick={closeViewer} aria-label="关闭">
            <IconClose size={14} />
          </button>
        </div>
        <div className="modal-body selectable">
          {viewer.kind === "image" && viewer.imageSrc ? (
            <img src={viewer.imageSrc} alt={viewer.title} style={{ maxWidth: "100%", borderRadius: 8 }} />
          ) : viewer.kind === "markdown" ? (
            <MarkdownView text={viewer.content} />
          ) : (
            <pre className="tool-pre">{viewer.content}</pre>
          )}
        </div>
      </div>
    </div>
  );
}
