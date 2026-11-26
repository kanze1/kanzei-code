import React, { useCallback, useEffect, useMemo, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import { confirmDialog, promptDialog, pushToast } from "../../store/uiStore";

interface SkillEntry {
  id: string;
  name: string;
  displayName: string;
  description: string;
  path?: string;
  source: string;
  editable: boolean;
  enabled: boolean;
  manualOnly?: boolean;
  userInvocable?: boolean;
  instructions?: string;
  revision?: string;
}

export default function SkillsView(): React.ReactElement {
  const [skills, setSkills] = useState<SkillEntry[]>([]);
  const [sourceFilter, setSourceFilter] = useState("all");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<SkillEntry | null>(null);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [instructions, setInstructions] = useState("");
  const [manualOnly, setManualOnly] = useState(false);
  const [busy, setBusy] = useState(false);

  const loadSkills = useCallback(async () => {
    try {
      const list = await invoke<SkillEntry[]>("skills_list");
      setSkills(Array.isArray(list) ? list : []);
    } catch (error) {
      pushToast(`加载 Skills 失败：${formatError(error)}`, "error");
    }
  }, []);

  useEffect(() => {
    void loadSkills();
  }, [loadSkills]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return skills.filter((s) => {
      if (sourceFilter !== "all" && s.source !== sourceFilter) return false;
      if (!q) return true;
      return (
        s.name.toLowerCase().includes(q) ||
        s.displayName.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q)
      );
    });
  }, [skills, sourceFilter, query]);

  const openSkill = async (item: SkillEntry) => {
    try {
      const full = await invoke<SkillEntry>("skills_read", { name: item.id });
      setSelected(full);
      setEditing(false);
    } catch (error) {
      pushToast(`读取技能详情失败：${formatError(error)}`, "error");
    }
  };

  const toggleEnabled = async (item: SkillEntry) => {
    try {
      await invoke("skills_set_enabled", { name: item.id, enabled: !item.enabled });
      await loadSkills();
      if (selected?.id === item.id) {
        setSelected({ ...selected, enabled: !item.enabled });
      }
    } catch (error) {
      pushToast(`更新启用状态失败：${formatError(error)}`, "error");
    }
  };

  const startCreate = () => {
    setSelected(null);
    setName("");
    setDescription("");
    setInstructions("");
    setManualOnly(false);
    setEditing(true);
  };

  const startEdit = () => {
    if (!selected) return;
    setName(selected.name);
    setDescription(selected.description);
    setInstructions(selected.instructions ?? "");
    setManualOnly(Boolean(selected.manualOnly));
    setEditing(true);
  };

  const saveSkill = async () => {
    setBusy(true);
    try {
      await invoke("skills_save", {
        draft: {
          name: name.trim(),
          description: description.trim(),
          instructions,
          manualOnly,
          userInvocable: true,
        },
        expectedHash: selected?.editable ? selected.revision ?? null : null,
      });
      pushToast("已保存技能", "ok");
      setEditing(false);
      await loadSkills();
    } catch (error) {
      pushToast(`保存失败：${formatError(error)}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const handleDuplicate = async () => {
    if (!selected) return;
    const nextName = await promptDialog({
      title: "复制为个人技能",
      label: "新技能名称（小写字母、数字与连字符）",
      initialValue: `${selected.name}-copy`,
      confirmLabel: "复制",
    });
    if (!nextName) return;
    try {
      await invoke("skills_duplicate", { name: selected.id, newName: nextName });
      pushToast("已复制技能", "ok");
      await loadSkills();
    } catch (error) {
      pushToast(`复制失败：${formatError(error)}`, "error");
    }
  };

  const handleDelete = async () => {
    if (!selected || !selected.editable) return;
    const ok = await confirmDialog({
      title: "删除个人技能？",
      message: `确定删除「${selected.displayName}」吗？`,
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await invoke("skills_delete", {
        name: selected.name,
        expectedHash: selected.revision ?? "",
      });
      setSelected(null);
      pushToast("已删除技能", "ok");
      await loadSkills();
    } catch (error) {
      pushToast(`删除失败：${formatError(error)}`, "error");
    }
  };

  const handleGenerate = async () => {
    const prompt = await promptDialog({
      title: "AI 生成技能草稿",
      label: "描述你想沉淀的流程、规范或工具技能",
      placeholder: "例如：帮我生成一个代码审查与安全检查清单技能",
      confirmLabel: "生成草稿",
    });
    if (!prompt) return;
    setBusy(true);
    try {
      const draft = await invoke<{ name?: string; description?: string; instructions?: string }>(
        "skills_generate",
        { description: prompt }
      );
      setSelected(null);
      setName(draft?.name ?? "");
      setDescription(draft?.description ?? "");
      setInstructions(draft?.instructions ?? "");
      setManualOnly(false);
      setEditing(true);
      pushToast("已生成技能草稿，请审阅后保存", "ok");
    } catch (error) {
      pushToast(`生成失败：${formatError(error)}`, "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page-shell">
      <div className="page-header">
        <div>
          <h1 className="page-title">Skills 技能库</h1>
          <div className="page-subtitle">管理内置与个人全局技能，决定模型可自动调用的能力集。</div>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button type="button" className="btn" disabled={busy} onClick={() => void handleGenerate()}>
            AI 生成草稿
          </button>
          <button type="button" className="btn btn-primary" onClick={startCreate}>
            + 新建技能
          </button>
        </div>
      </div>

      <div className="split-page">
        <div className="split-left" style={{ width: 320 }}>
          <div style={{ padding: 10, display: "flex", flexDirection: "column", gap: 6, borderBottom: "1px solid var(--border-soft)" }}>
            <input
              className="input"
              placeholder="搜索技能名称或描述…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <select className="select" value={sourceFilter} onChange={(e) => setSourceFilter(e.target.value)}>
              <option value="all">全部来源 ({skills.length})</option>
              <option value="builtin">内置技能</option>
              <option value="personal">个人技能</option>
              <option value="external">外部库</option>
            </select>
          </div>
          <div className="split-left-scroll">
            {filtered.map((item) => (
              <div
                key={item.id}
                className={`list-card-item${selected?.id === item.id ? " active" : ""}`}
                role="button"
                tabIndex={0}
                onClick={() => void openSkill(item)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void openSkill(item);
                }}
              >
                <div className="list-card-title">
                  <span>{item.displayName}</span>
                  <input
                    type="checkbox"
                    checked={item.enabled}
                    onClick={(e) => e.stopPropagation()}
                    onChange={() => void toggleEnabled(item)}
                    title={item.enabled ? "已启用" : "已停用"}
                  />
                </div>
                <div className="list-card-desc">{item.description}</div>
                <div style={{ fontSize: 11, color: "var(--dim)" }}>{item.source}</div>
              </div>
            ))}
          </div>
        </div>

        <div className="split-right" style={{ padding: 20, overflowY: "auto" }}>
          {editing ? (
            <div style={{ maxWidth: 740, display: "flex", flexDirection: "column", gap: 12 }}>
              <h2 style={{ fontSize: 16, color: "var(--fg-strong)" }}>
                {selected ? `编辑技能：${selected.name}` : "新建个人技能"}
              </h2>
              <div className="field-group">
                <label className="field-label">名称（小写字母、数字和连字符）</label>
                <input className="input mono" value={name} onChange={(e) => setName(e.target.value)} />
              </div>
              <div className="field-group">
                <label className="field-label">简介（决定模型何时触发该技能）</label>
                <input className="input" value={description} onChange={(e) => setDescription(e.target.value)} />
              </div>
              <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12 }}>
                <input
                  type="checkbox"
                  checked={manualOnly}
                  onChange={(e) => setManualOnly(e.target.checked)}
                />
                仅手动触发（禁止模型自动激活）
              </label>
              <div className="field-group">
                <label className="field-label">技能指令正文</label>
                <textarea
                  className="textarea mono"
                  style={{ minHeight: 280 }}
                  value={instructions}
                  onChange={(e) => setInstructions(e.target.value)}
                />
              </div>
              <div style={{ display: "flex", gap: 8 }}>
                <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void saveSkill()}>
                  {busy ? "保存中…" : "保存"}
                </button>
                <button type="button" className="btn" onClick={() => setEditing(false)}>
                  取消
                </button>
              </div>
            </div>
          ) : selected ? (
            <div style={{ maxWidth: 780, display: "flex", flexDirection: "column", gap: 12 }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 12 }}>
                <div>
                  <h2 style={{ fontSize: 18, color: "var(--fg-strong)" }}>{selected.displayName}</h2>
                  <div className="field-hint">
                    {selected.id} · {selected.source} · {selected.enabled ? "已启用" : "已停用"}
                  </div>
                </div>
                <div style={{ display: "flex", gap: 6 }}>
                  <button type="button" className="btn btn-sm" onClick={() => void handleDuplicate()}>
                    复制
                  </button>
                  {selected.editable && (
                    <>
                      <button type="button" className="btn btn-sm btn-primary" onClick={startEdit}>
                        编辑
                      </button>
                      <button type="button" className="btn btn-sm btn-danger" onClick={() => void handleDelete()}>
                        删除
                      </button>
                    </>
                  )}
                </div>
              </div>
              <p style={{ fontSize: 13 }}>{selected.description}</p>
              <pre className="tool-pre selectable" style={{ padding: 12, background: "var(--code-bg)", borderRadius: 8 }}>
                {selected.instructions ?? ""}
              </pre>
            </div>
          ) : (
            <div className="empty-state">
              <p>从左侧选择技能查看指令详情，或新建/AI 生成专属个人技能。</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
