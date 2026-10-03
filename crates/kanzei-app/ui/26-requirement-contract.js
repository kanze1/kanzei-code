// Requirement meaning and acceptance are read before source, design, and execution history.
import { t } from "./02-i18n.js";
import { renderTrackerFields, richText } from "./04-structured.js";

const SPEC_KEYS = new Set(["需求格式", "需求类型", "内容", "验收", "来源", "开放问题", "需求关联", "验收证据"]);
const EXECUTION_KEYS = new Set(["进展", "对账", "批次", "批次计划", "批次表", "复杂度", "取得线", "observed_head", "observed_worktree_hash", "recorded_at", "取活依据"]);
const SOURCE_KEYS = new Set(["来源", "原始描述", "发现记录", "确认记录", "迁移原文"]);
const DESIGN_KEYS = new Set(["边界", "迁移与回滚", "设计文档", "refs", "前置", "关联"]);
function node(tag, text, cls) {
  const element = document.createElement(tag);
  if (text !== undefined) element.textContent = text;
  if (cls) element.className = cls;
  return element;
}
function section(root, title, fields) {
  if (!fields.length) return;
  const details = node("details", undefined, "requirement-supplement");
  details.append(node("summary", t(title)), renderTrackerFields(fields));
  root.append(details);
}

export function renderRequirementDocument(entry) {
  const root = node("div", undefined, "requirement-document");
  const view = entry.requirement;
  const spec = view?.spec;
  if (view?.evidence_error) root.append(node("p", view.evidence_error, "requirement-error"));
  const fields = (entry.fields ?? []).filter(([, value]) => String(value ?? "").trim());
  if (spec) {
    const meta = fields.filter(([key]) => ["优先级", "标签"].includes(key));
    if (spec.kind) meta.push(["类型", t(spec.kind === "functional" ? "功能需求" : "非功能需求")]);
    root.append(renderTrackerFields(meta));
    const statement = node("div", undefined, "requirement-statement");
    statement.append(richText(spec.statement || t("待补充需求正文")));
    root.append(statement, node("h4", t("验收标准")));
    const list = node("ol", undefined, "requirement-acceptance");
    for (const criterion of spec.acceptance ?? []) {
      const item = node("li");
      item.dataset.criterionId = criterion.id;
      item.append(node("code", criterion.id), richText(criterion.text));
      const evidence = (view.evidence ?? []).find(e => e.criterion_id === criterion.id);
      const current = evidence?.revision === view.revision;
      item.append(node("small", t(current ? "已有证据" : evidence ? "要求已变更，待复核" : "待验证"), "requirement-evidence-state"));
      list.append(item);
    }
    root.append(list);
    if (!spec.acceptance?.length) root.append(node("p", t("待补充验收标准")));
    if (view.gaps?.length) root.append(renderTrackerFields([["待解决事项", view.gaps.join("\n")]]));
    section(root, "来源与说明", [
      ...spec.source?.reference ? [["来源", spec.source.reference]] : [],
      ...spec.source?.quote ? [["原话", spec.source.quote]] : [],
      ...fields.filter(([key]) => SOURCE_KEYS.has(key) && key !== "来源"),
    ]);
    section(root, "关联与设计", (spec.links ?? []).map(link => [t(({ parent: "上级目标", depends_on: "依赖", design: "设计文档", related: "关联" })[link.relation]), link.target]));
    section(root, "验收证据", (view.evidence ?? []).map(e => [e.criterion_id, `${e.reference}\n${e.revision === view.revision ? t("当前版本") : t("旧版本证据")}`]));
  } else {
    if (view?.error) root.append(node("p", view.error, "requirement-error"));
    // Legacy boundaries can contain real obligations. Keep them visible until reviewed.
    root.append(renderTrackerFields(fields.filter(([key]) => ["内容", "验收", "边界", "迁移与回滚", "优先级", "标签"].includes(key))));
    section(root, "来源与说明", fields.filter(([key]) => SOURCE_KEYS.has(key)));
  }
  root.append(renderTrackerFields(fields.filter(([key]) => ["外部验收", "阻塞", "停车", "依赖", "待确认"].includes(key))));
  section(root, "执行记录", fields.filter(([key]) => EXECUTION_KEYS.has(key)));
  const shown = new Set([...SOURCE_KEYS, ...EXECUTION_KEYS, "外部验收", "阻塞", "停车", "依赖", "待确认", "优先级", "标签"]);
  if (spec) for (const key of SPEC_KEYS) shown.add(key);
  else for (const key of ["内容", "验收", "边界", "迁移与回滚"]) shown.add(key);
  section(root, "关联与设计", fields.filter(([key]) => !shown.has(key) && DESIGN_KEYS.has(key)));
  section(root, "补充资料", fields.filter(([key]) => !shown.has(key) && !DESIGN_KEYS.has(key)));
  return root;
}

// Human editor uses lines, while IPC keeps stable criterion IDs and typed arrays.
export function requirementEditRows(entry) {
  const spec = entry.requirement?.spec;
  if (!spec) return null;
  return [
    ["需求正文", ":statement", spec.statement],
    ["验收标准", ":acceptance", spec.acceptance.map(c => `${c.id} | ${c.text}`).join("\n")],
    ["开放问题", ":questions", (spec.questions ?? []).join("\n")],
    ["来源", ":source", spec.source?.reference || ""],
    ["原话", ":quote", spec.source?.quote || ""],
  ];
}
export function requirementFromEditor(entry, values) {
  const spec = structuredClone(entry.requirement.spec);
  spec.statement = values[":statement"];
  spec.source = { reference: values[":source"], quote: values[":quote"] };
  spec.acceptance = values[":acceptance"].split("\n").map(s => s.trim()).filter(Boolean).map(line => {
    const match = line.match(/^(AC-\d+)\s*\|\s*(.+)$/);
    return match ? { id: match[1], text: match[2] } : { id: "", text: line };
  });
  spec.questions = values[":questions"].split("\n").map(s => s.trim()).filter(Boolean);
  return spec;
}
