import React, { useCallback, useEffect, useMemo, useState } from "react";
import { formatError, invoke } from "../../ipc/client";
import type { RepoWikiDetail, RepoWikiOverview, RepoWikiPageSummary } from "../../ipc/types";
import { projectDisplayName, useAppStore } from "../../store/appStore";
import { appendQuoteToComposer } from "../../store/chatStore";
import { openFileInDrawer } from "../../store/drawerStore";
import { navigateView, pushToast } from "../../store/uiStore";
import {
  IconFileCode,
  IconFolder,
  IconLayers,
  IconRefresh,
  IconSearch,
  IconSparkles,
} from "../../components/Icons";
import { MarkdownView } from "../../components/MarkdownView";

type WikiRightTab = "doc" | "edit" | "structure";
type WikiStatusFilter = "all" | "stale" | "fresh" | "missing";

function statusMeta(status: RepoWikiPageSummary["status"]): { text: string; cls: string } {
  switch (status) {
    case "fresh":
      return { text: "最新", cls: "badge-ok" };
    case "stale":
      return { text: "代码已漂移", cls: "badge-warn" };
    default:
      return { text: "未生成", cls: "" };
  }
}

export default function RepoWikiView(): React.ReactElement {
  const projectDir = useAppStore((s) => s.activeProjectDir);
  const generalMode = useAppStore((s) => s.generalMode);
  const [overview, setOverview] = useState<RepoWikiOverview | null>(null);
  const [loadingOverview, setLoadingOverview] = useState(false);
  const [selectedId, setSelectedId] = useState<string>("overview");
  const [detail, setDetail] = useState<RepoWikiDetail | null>(null);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [generatingId, setGeneratingId] = useState<string | null>(null);
  const [syncingAll, setSyncingAll] = useState(false);
  const [rightTab, setRightTab] = useState<WikiRightTab>("doc");
  const [editDraft, setEditDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<WikiStatusFilter>("all");

  const loadOverview = useCallback(async () => {
    if (!projectDir || generalMode) {
      setOverview(null);
      return;
    }
    setLoadingOverview(true);
    try {
      const data = await invoke<RepoWikiOverview>("repo_wiki_overview", { projectDir });
      setOverview(data);
    } catch (err) {
      pushToast(`扫描仓库百科失败：${formatError(err)}`, "error");
    } finally {
      setLoadingOverview(false);
    }
  }, [projectDir, generalMode]);

  const loadDetail = useCallback(
    async (areaId: string) => {
      if (!projectDir || generalMode) {
        setDetail(null);
        return;
      }
      setLoadingDetail(true);
      try {
        const data = await invoke<RepoWikiDetail>("repo_wiki_read", {
          projectDir,
          areaId,
        });
        setDetail(data);
        setEditDraft(data.content ?? "");
      } catch (err) {
        pushToast(`读取百科页失败：${formatError(err)}`, "error");
        setDetail(null);
      } finally {
        setLoadingDetail(false);
      }
    },
    [projectDir, generalMode]
  );

  useEffect(() => {
    setSelectedId("overview");
    setRightTab("doc");
    void loadOverview();
  }, [loadOverview]);

  useEffect(() => {
    if (projectDir && !generalMode && selectedId) {
      void loadDetail(selectedId);
    }
  }, [projectDir, generalMode, selectedId, loadDetail]);

  const filteredPages = useMemo(() => {
    const pages = overview?.pages ?? [];
    const q = query.trim().toLowerCase();
    return pages.filter((p) => {
      if (statusFilter !== "all" && p.status !== statusFilter) return false;
      if (!q) return true;
      return (
        p.areaId.toLowerCase().includes(q) ||
        p.title.toLowerCase().includes(q) ||
        p.bandLabel.toLowerCase().includes(q)
      );
    });
  }, [overview, query, statusFilter]);

  const groupedByBand = useMemo(() => {
    const groups = new Map<string, RepoWikiPageSummary[]>();
    for (const p of filteredPages) {
      const key = p.bandLabel || "其他";
      const list = groups.get(key) ?? [];
      list.push(p);
      groups.set(key, list);
    }
    return Array.from(groups.entries());
  }, [filteredPages]);

  const handleGenerate = async (areaId: string) => {
    if (!projectDir || generalMode) return;
    setGeneratingId(areaId);
    try {
      const generated = await invoke<RepoWikiDetail>("repo_wiki_generate", {
        projectDir,
        areaId,
      });
      if (areaId === selectedId) {
        setDetail(generated);
        setEditDraft(generated.content ?? "");
        setRightTab("doc");
      }
      await loadOverview();
      pushToast(`已生成并同步「${generated.title}」架构文档`, "ok");
    } catch (err) {
      pushToast(`生成百科失败：${formatError(err)}`, "error");
    } finally {
      setGeneratingId(null);
    }
  };

  const handleSyncAllStale = async () => {
    if (!projectDir || !overview) return;
    const stalePages = overview.pages.filter((p) => p.status === "stale");
    if (stalePages.length === 0) {
      pushToast("当前没有因代码改动而漂移的百科页", "info");
      return;
    }
    setSyncingAll(true);
    let okCount = 0;
    try {
      for (const page of stalePages) {
        setGeneratingId(page.areaId);
        try {
          const updated = await invoke<RepoWikiDetail>("repo_wiki_generate", {
            projectDir,
            areaId: page.areaId,
          });
          okCount += 1;
          if (page.areaId === selectedId) {
            setDetail(updated);
            setEditDraft(updated.content ?? "");
          }
        } catch (err) {
          pushToast(`同步 ${page.title} 失败：${formatError(err)}`, "warn");
        }
      }
      await loadOverview();
      pushToast(`已完成 ${okCount}/${stalePages.length} 页过时百科的增量同步`, "ok");
    } finally {
      setGeneratingId(null);
      setSyncingAll(false);
    }
  };

  const handleSaveEdit = async () => {
    if (!projectDir || !detail) return;
    setSaving(true);
    try {
      const saved = await invoke<RepoWikiDetail>("repo_wiki_save", {
        projectDir,
        areaId: detail.areaId,
        content: editDraft,
      });
      setDetail(saved);
      setRightTab("doc");
      await loadOverview();
      pushToast("已保存百科修订并更新源码指纹", "ok");
    } catch (err) {
      pushToast(`保存失败：${formatError(err)}`, "error");
    } finally {
      setSaving(false);
    }
  };

  const handleQuoteToChat = async () => {
    if (!detail?.content) return;
    const snippet = detail.content.slice(0, 3200);
    await appendQuoteToComposer(
      `参考 Repo Wiki 架构百科「${detail.title}」（\`${detail.areaId}\`）：\n\n${snippet}\n`
    );
    navigateView("chat");
    pushToast("已将模块架构摘要注入对话输入框", "ok", 2000);
  };

  if (!projectDir || generalMode) {
    return (
      <div className="page-shell">
        <div className="empty-state" style={{ flex: 1 }}>
          <div className="empty-state-icon">
            <IconFolder size={20} />
          </div>
          <h3>请先选择一个具体代码项目</h3>
          <p>Repo Wiki 会自动扫描仓库两级模块结构、依赖图与源码指纹，仅对已登记的代码项目开放。</p>
          <button type="button" className="btn btn-primary" onClick={() => navigateView("workspace")}>
            前往工作台选择项目
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="page-shell">
      <div className="page-header">
        <div>
          <h1 className="page-title">Repo Wiki 仓库百科 · {projectDisplayName(projectDir)}</h1>
          <div className="page-subtitle">
            自动识别代码区域、依赖层带与源码指纹漂移，让团队和 Agent 随时掌握最新架构地图。
          </div>
        </div>
        <div className="page-header-actions">
          <button
            type="button"
            className="btn"
            disabled={loadingOverview || syncingAll}
            onClick={() => void loadOverview()}
          >
            <IconRefresh size={13} />
            <span>重新扫描</span>
          </button>
          {(overview?.staleCount ?? 0) > 0 && (
            <button
              type="button"
              className="btn btn-accent"
              disabled={syncingAll || generatingId !== null}
              onClick={() => void handleSyncAllStale()}
            >
              <IconSparkles size={13} />
              <span>
                {syncingAll ? "正在同步…" : `同步全部过时页 (${overview?.staleCount})`}
              </span>
            </button>
          )}
          <button
            type="button"
            className="btn btn-primary"
            disabled={generatingId !== null || syncingAll}
            onClick={() => void handleGenerate(selectedId)}
          >
            <IconSparkles size={13} />
            <span>
              {generatingId === selectedId
                ? "AI 正在分析生成…"
                : detail?.content
                ? "AI 重新同步本页"
                : "AI 生成本页百科"}
            </span>
          </button>
        </div>
      </div>

      {overview && (
        <div className="wiki-stats-bar">
          <span>
            已识别区域 <strong>{overview.totalPages}</strong>
          </span>
          <span className="wiki-stat-ok">
            最新 <strong>{overview.freshCount}</strong>
          </span>
          <span className={overview.staleCount > 0 ? "wiki-stat-warn" : ""}>
            代码漂移 <strong>{overview.staleCount}</strong>
          </span>
          <span>
            未生成 <strong>{overview.missingCount}</strong>
          </span>
          <span>
            内部依赖边 <strong>{overview.deps.length}</strong>
          </span>
          <span className="mono field-hint" style={{ marginLeft: "auto" }}>
            {overview.wikiDir}
          </span>
        </div>
      )}

      <div className="split-page">
        <div className="split-left">
          <div className="split-left-filters">
            <div className="sidebar-search-wrap">
              <span className="sidebar-search-icon" aria-hidden="true">
                <IconSearch size={12} />
              </span>
              <input
                className="input sidebar-search"
                placeholder="筛选模块或层带…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </div>
            <select
              className="select"
              aria-label="按同步状态筛选"
              value={statusFilter}
              onChange={(e) => setStatusFilter(e.target.value as WikiStatusFilter)}
            >
              <option value="all">全部状态</option>
              <option value="stale">代码已漂移 ({overview?.staleCount ?? 0})</option>
              <option value="fresh">已生成最新 ({overview?.freshCount ?? 0})</option>
              <option value="missing">尚未生成 ({overview?.missingCount ?? 0})</option>
            </select>
          </div>

          <div className="split-left-scroll">
            {loadingOverview && !overview ? (
              <div className="sidebar-empty-hint">正在扫描仓库结构…</div>
            ) : groupedByBand.length === 0 ? (
              <div className="sidebar-empty-hint">暂无匹配的代码区域</div>
            ) : (
              groupedByBand.map(([bandName, items]) => (
                <div key={bandName} className="wiki-band-group">
                  <div className="wiki-band-title">{bandName}</div>
                  {items.map((item) => {
                    const badge = statusMeta(item.status);
                    const isGenerating = generatingId === item.areaId;
                    return (
                      <div
                        key={item.areaId}
                        role="button"
                        tabIndex={0}
                        className={`list-card-item${selectedId === item.areaId ? " active" : ""}`}
                        onClick={() => setSelectedId(item.areaId)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") setSelectedId(item.areaId);
                        }}
                      >
                        <div className="list-card-title">
                          <span className="list-card-title-text">{item.title}</span>
                          <span className={`badge ${badge.cls}`}>
                            {isGenerating ? "生成中…" : badge.text}
                          </span>
                        </div>
                        <div className="list-card-meta">
                          <span className="mono">{item.areaId}</span>
                          <span>{item.fileCount} 文件</span>
                        </div>
                      </div>
                    );
                  })}
                </div>
              ))
            )}
          </div>
        </div>

        <div className="split-right">
          {loadingDetail ? (
            <div className="empty-state">
              <p>正在加载架构百科详情…</p>
            </div>
          ) : !detail ? (
            <div className="empty-state">
              <p>请从左侧选择一个代码区域查看或生成架构文档。</p>
            </div>
          ) : (
            <div className="wiki-detail-shell">
              <div className="wiki-detail-head">
                <div>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                    <h2 style={{ margin: 0, fontSize: 16 }}>{detail.title}</h2>
                    <span className={`badge ${statusMeta(detail.status).cls}`}>
                      {statusMeta(detail.status).text}
                    </span>
                    <span className="mono field-hint">{detail.areaId}</span>
                  </div>
                  <div className="field-hint" style={{ marginTop: 4 }}>
                    {detail.generatedAt
                      ? `上次同步：${new Date(detail.generatedAt).toLocaleString()}${
                          detail.model ? ` · 模型：${detail.model}` : ""
                        }`
                      : `包含 ${detail.files.length} 个源码文件，尚未生成百科文档`}
                  </div>
                </div>

                <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                  <div className="segmented-tabs">
                    <button
                      type="button"
                      className={`segmented-tab${rightTab === "doc" ? " active" : ""}`}
                      onClick={() => setRightTab("doc")}
                    >
                      文档
                    </button>
                    <button
                      type="button"
                      className={`segmented-tab${rightTab === "edit" ? " active" : ""}`}
                      onClick={() => setRightTab("edit")}
                    >
                      编辑
                    </button>
                    <button
                      type="button"
                      className={`segmented-tab${rightTab === "structure" ? " active" : ""}`}
                      onClick={() => setRightTab("structure")}
                    >
                      依赖与文件 ({detail.files.length})
                    </button>
                  </div>
                  {detail.content && (
                    <button
                      type="button"
                      className="btn btn-sm"
                      onClick={() => void handleQuoteToChat()}
                    >
                      引用到对话
                    </button>
                  )}
                </div>
              </div>

              {detail.status === "stale" && (
                <div className="facts-banner" style={{ margin: "0 0 12px" }}>
                  <span>该区域下的源码自上次生成后已发生变更，建议点击同步以更新架构文档与依赖说明。</span>
                  <button
                    type="button"
                    className="btn btn-sm btn-primary"
                    disabled={generatingId !== null}
                    onClick={() => void handleGenerate(detail.areaId)}
                  >
                    {generatingId === detail.areaId ? "正在同步…" : "立即增量同步"}
                  </button>
                </div>
              )}

              <div className="wiki-detail-body">
                {rightTab === "edit" ? (
                  <div style={{ display: "flex", flexDirection: "column", gap: 10, height: "100%" }}>
                    <textarea
                      className="input mono"
                      style={{ flex: 1, minHeight: 380, resize: "vertical", lineHeight: 1.55 }}
                      value={editDraft}
                      placeholder="在此直接编写或修订该模块的架构文档（Markdown + Mermaid）…"
                      onChange={(e) => setEditDraft(e.target.value)}
                    />
                    <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
                      <button
                        type="button"
                        className="btn"
                        onClick={() => {
                          setEditDraft(detail.content ?? "");
                          setRightTab("doc");
                        }}
                      >
                        取消
                      </button>
                      <button
                        type="button"
                        className="btn btn-primary"
                        disabled={saving}
                        onClick={() => void handleSaveEdit()}
                      >
                        {saving ? "保存中…" : "保存并同步指纹"}
                      </button>
                    </div>
                  </div>
                ) : rightTab === "structure" ? (
                  <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
                    <div className="wiki-deps-grid">
                      <div className="provider-card">
                        <div className="provider-card-title">
                          <IconLayers size={13} /> 依赖的下游模块 ({detail.outboundDeps.length})
                        </div>
                        {detail.outboundDeps.length === 0 ? (
                          <div className="field-hint">无显式内部依赖</div>
                        ) : (
                          <div className="wiki-dep-chips">
                            {detail.outboundDeps.map((dep) => (
                              <button
                                key={dep}
                                type="button"
                                className="btn btn-sm"
                                onClick={() => setSelectedId(dep)}
                              >
                                → {dep}
                              </button>
                            ))}
                          </div>
                        )}
                      </div>
                      <div className="provider-card">
                        <div className="provider-card-title">
                          <IconLayers size={13} /> 被以下上游模块依赖 ({detail.inboundDeps.length})
                        </div>
                        {detail.inboundDeps.length === 0 ? (
                          <div className="field-hint">暂无上游模块直接依赖</div>
                        ) : (
                          <div className="wiki-dep-chips">
                            {detail.inboundDeps.map((dep) => (
                              <button
                                key={dep}
                                type="button"
                                className="btn btn-sm"
                                onClick={() => setSelectedId(dep)}
                              >
                                ← {dep}
                              </button>
                            ))}
                          </div>
                        )}
                      </div>
                    </div>

                    <div className="provider-card">
                      <div className="provider-card-title">
                        <IconFileCode size={13} /> 区域源码文件清单（{detail.files.length}）
                      </div>
                      <div className="wiki-file-list">
                        {detail.files.map((rel) => (
                          <button
                            key={rel}
                            type="button"
                            className="wiki-file-row mono"
                            onClick={() => {
                              navigateView("chat");
                              void openFileInDrawer(rel, { tab: "preview" });
                            }}
                          >
                            {rel}
                          </button>
                        ))}
                      </div>
                    </div>
                  </div>
                ) : detail.content ? (
                  <MarkdownView text={detail.content} />
                ) : (
                  <div className="empty-state" style={{ padding: "48px 16px" }}>
                    <div className="empty-state-icon">
                      <IconSparkles size={20} />
                    </div>
                    <h3>尚未生成「{detail.title}」的架构百科</h3>
                    <p>
                      Kanzei 已扫描该区域下的 {detail.files.length} 个文件与依赖关系，可一键调用模型生成结构化文档与 Mermaid 架构图，或手动编写。
                    </p>
                    <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
                      <button
                        type="button"
                        className="btn btn-primary"
                        disabled={generatingId !== null}
                        onClick={() => void handleGenerate(detail.areaId)}
                      >
                        <IconSparkles size={13} />
                        <span>{generatingId === detail.areaId ? "正在生成…" : "立即 AI 生成"}</span>
                      </button>
                      <button
                        type="button"
                        className="btn"
                        onClick={() => {
                          setEditDraft(`# ${detail.title}\n\n`);
                          setRightTab("edit");
                        }}
                      >
                        手动编写
                      </button>
                    </div>
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
