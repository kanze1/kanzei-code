import React, { useEffect, useState } from "react";
import { formatError, invoke } from "../ipc/client";
import { createNewProject, useAppStore } from "../store/appStore";
import { refreshActiveSessionMeta } from "../store/sessionStore";
import {
  closeViewer,
  dismissToast,
  pushToast,
  resolveConfirmDialog,
  resolveInputDialog,
  uiStore,
  useUiStore,
} from "../store/uiStore";
import { MarkdownView } from "./MarkdownView";

export function ToastRegion(): React.ReactElement {
  const toasts = useUiStore((s) => s.toasts);
  return (
    <div className="toast-region" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast-item toast-${t.kind}`} role="status">
          <span style={{ flex: 1 }}>{t.text}</span>
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => dismissToast(t.id)}
            aria-label="关闭通知"
          >
            ×
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
    <div className="modal-scrim" role="dialog" aria-modal="true" aria-label={req.title}>
      <div className="modal-panel">
        <div className="modal-header">
          <div className="modal-title">{req.title}</div>
          <button type="button" className="btn btn-ghost btn-icon" onClick={() => resolveConfirmDialog(false)}>
            ×
          </button>
        </div>
        <div className="modal-body">
          <p style={{ whiteSpace: "pre-wrap" }}>{req.message}</p>
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
    <div className="modal-scrim" role="dialog" aria-modal="true" aria-label={req.title}>
      <div className="modal-panel">
        <div className="modal-header">
          <div className="modal-title">{req.title}</div>
          <button type="button" className="btn btn-ghost btn-icon" onClick={() => resolveInputDialog(null)}>
            ×
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
    <div className="modal-scrim" role="dialog" aria-modal="true" aria-label="新建项目">
      <div className="modal-panel">
        <div className="modal-header">
          <div className="modal-title">新建项目</div>
          <button type="button" className="btn btn-ghost btn-icon" onClick={close}>
            ×
          </button>
        </div>
        <div className="modal-body">
          <div className="field-group">
            <label className="field-label">项目名称</label>
            <input
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
          <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12 }}>
            <input
              type="checkbox"
              checked={gitInit}
              onChange={(e) => setGitInit(e.target.checked)}
            />
            初始化独立 Git 仓库与首提交
          </label>
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

export function ProjectModelsDialogHost(): React.ReactElement | null {
  const open = useUiStore((s) => s.projectModelsOpen);
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const modelsCatalog = useAppStore((s) => s.modelsCatalog);
  const [primary, setPrimary] = useState("");
  const [fast, setFast] = useState("");
  const [compact, setCompact] = useState("");
  const [reasoning, setReasoning] = useState("");
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open || !projectDir) return;
    setLoading(true);
    invoke<Record<string, { project?: string | null }>>("project_models_get", { projectDir })
      .then((data) => {
        setPrimary(data?.primary?.project ?? "");
        setFast(data?.fast?.project ?? "");
        setCompact(data?.compact?.project ?? "");
        setReasoning(data?.reasoning?.project ?? "");
      })
      .catch((err) => pushToast(`读取项目模型覆盖失败：${formatError(err)}`, "error"))
      .finally(() => setLoading(false));
  }, [open, projectDir]);

  if (!open || !projectDir) return null;
  const close = () => uiStore.setState({ projectModelsOpen: false });

  const save = async () => {
    try {
      const entries: Array<[string, string]> = [
        ["primary", primary.trim()],
        ["fast", fast.trim()],
        ["compact", compact.trim()],
        ["reasoning", reasoning.trim()],
      ];
      const set: Record<string, string> = {};
      const unset: string[] = [];
      for (const [k, v] of entries) {
        if (v) set[k] = v;
        else unset.push(k);
      }
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
    }
  };

  return (
    <div className="modal-scrim" role="dialog" aria-modal="true" aria-label="本项目模型配置">
      <div className="modal-panel">
        <div className="modal-header">
          <div className="modal-title">本项目模型覆盖</div>
          <button type="button" className="btn btn-ghost btn-icon" onClick={close}>
            ×
          </button>
        </div>
        <div className="modal-body">
          <p className="field-hint">留空即继承全局设置；修改只影响当前项目的 `.kanzei/kanzei.toml`。</p>
          {loading ? (
            <p className="field-hint">正在读取…</p>
          ) : (
            <>
              <div className="field-group">
                <label className="field-label">主模型（primary）</label>
                <input
                  className="input mono"
                  list="kz-project-models-datalist"
                  value={primary}
                  onChange={(e) => setPrimary(e.target.value)}
                  placeholder="留空继承全局"
                />
              </div>
              <div className="field-group">
                <label className="field-label">快速模型（fast）</label>
                <input
                  className="input mono"
                  list="kz-project-models-datalist"
                  value={fast}
                  onChange={(e) => setFast(e.target.value)}
                  placeholder="留空继承全局"
                />
              </div>
              <div className="field-group">
                <label className="field-label">压缩模型（compact）</label>
                <input
                  className="input mono"
                  list="kz-project-models-datalist"
                  value={compact}
                  onChange={(e) => setCompact(e.target.value)}
                  placeholder="留空跟随主模型"
                />
              </div>
              <div className="field-group">
                <label className="field-label">思考强度（reasoning）</label>
                <select className="select" value={reasoning} onChange={(e) => setReasoning(e.target.value)}>
                  <option value="">继承全局</option>
                  <option value="off">off</option>
                  <option value="low">low</option>
                  <option value="medium">medium</option>
                  <option value="high">high</option>
                  <option value="xhigh">xhigh</option>
                </select>
              </div>
              <datalist id="kz-project-models-datalist">
                {modelsCatalog.map((m) => (
                  <option key={m.id} value={m.id} />
                ))}
              </datalist>
            </>
          )}
        </div>
        <div className="modal-footer">
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => void invoke("project_config_open", { projectDir }).catch(() => {})}
          >
            打开配置文件
          </button>
          <button type="button" className="btn" onClick={close}>
            取消
          </button>
          <button type="button" className="btn btn-primary" onClick={() => void save()}>
            保存
          </button>
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
          <button type="button" className="btn btn-ghost btn-icon" onClick={closeViewer}>
            ×
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
