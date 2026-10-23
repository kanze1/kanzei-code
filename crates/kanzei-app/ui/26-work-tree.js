import { renderTreeView } from "./26-tree-view.js";
import { localizedStatusWord, t } from "./02-i18n.js";

export const kindLabels = { goal: "目标节点", requirement: "需求节点", implementation: "实现节点", defect: "缺陷节点", research: "调研节点", design: "设计节点", documentation: "文档节点", verification: "验证节点" };
const kindGlyphs = { goal: "◎", requirement: "◇", implementation: "⌘", defect: "!", research: "⌕", design: "◈", documentation: "≡", verification: "✓" };
const stepLabels = { plan: "补齐理解与计划", execute: "推进当前工作", integrate: "验收整体结果", wait: "等待前提满足", finished: "工作已结束" };
const dispositionLabels = { retain: "继续沿用", revise: "已修订", reverify: "由后继节点重验", replace: "已被替代", retire: "已撤销" };
export const workReviewPending = unit => Boolean(unit.review && !unit.review.resolution);
export const workIsCurrent = unit => unit.status !== "superseded" && !["reverify", "replace", "retire"].includes(unit.review?.resolution?.decision?.disposition);
export const workResultCurrent = unit => unit.status === "done" && workIsCurrent(unit) && !workReviewPending(unit);
export const reviewStatus = unit => workReviewPending(unit) ? t("方向待复核") : !workIsCurrent(unit) && unit.review?.resolution ? t(dispositionLabels[unit.review.resolution.decision.disposition]) : null;

function text(tag, className, content) {
  const element = document.createElement(tag);
  element.className = className;
  element.textContent = content;
  return element;
}

/** Human detail is a projection of recorded facts; execution contracts stay in the store. */
function workUnitCard(unit, readiness, nodes, expanded) {
  const card = text("article", `work-unit-card ${unit.status}`, "");
  const node = unit.node || {}, checkpoint = unit.last_checkpoint || {};
  const nameReferences = value => value.replace(/R-\d+\/W\d+/g, id => nodes.get(id)?.objective || id);
  const meta = (parent, label, content, state = "") => {
    if (!content) return;
    const row = text("div", `work-unit-meta ${state}`, "");
    row.append(text("span", "work-meta-label", t(label)), text("span", "work-meta-value", content));
    parent.append(row);
  };
  const section = (key, label) => {
    const details = text("details", "work-inspector-section", "");
    details.dataset.section = key;
    details.open = expanded.get(`${unit.unit_id}:${key}`) || false;
    details.append(text("summary", "", t(label)));
    details.ontoggle = () => expanded.set(`${unit.unit_id}:${key}`, details.open);
    card.append(details); return details;
  };
  const terminal = ["done", "superseded"].includes(unit.status);
  const status = reviewStatus(unit) || (!terminal && ["plan", "wait", "integrate"].includes(readiness?.step)
    ? t(stepLabels[readiness.step]) : localizedStatusWord(unit.status, "unit"));
  meta(card, "当前情况", status);
  if (workReviewPending(unit)) {
    meta(card, "新的方向", unit.review.request.direction);
    meta(card, "调整原因", unit.review.request.reason);
    meta(card, "下一步", t("Agent 评估受影响工作，保留有效结果并修正计划"));
  }
  meta(card, checkpoint.summary ? "实质进展" : "当前理解", checkpoint.summary || node.explanation);
  if (!terminal && !workReviewPending(unit)) {
    const blockers = [...new Set([...(readiness?.blockers || []), unit.blocked_reason].filter(Boolean))];
    meta(card, "阻塞原因", blockers.map(nameReferences).join("\n"), "blocked");
    if (!(unit.acceptance || []).length) meta(card, "下一步", t("先明确完成条件，再开始执行"));
    else if (readiness?.step !== "wait") meta(card, "下一步", checkpoint.next_action);
  }
  meta(card, "结束原因", unit.terminal_reason);
  meta(card, "预期产物", node.custom_type?.deliverable);
  if (unit.review?.resolution) {
    const revision = section("revision", "方向复核记录");
    const decision = unit.review.resolution.decision;
    meta(revision, "处置结果", t(dispositionLabels[decision.disposition]));
    meta(revision, "调整原因", decision.reason);
    meta(revision, "新的方向", unit.review.request.direction);
    meta(revision, "后继工作", (decision.replacements || []).map(id => nodes.get(id)?.objective || id).join("；"));
  }
  if (node.questions?.length) {
    const questions = section("questions", "待澄清的问题");
    // An unresolved research question does not imply a request for human input.
    for (const question of node.questions) questions.append(text("p", "work-inspector-note", question));
  }
  const acceptance = unit.acceptance || [], evidence = unit.evidence || [];
  if (acceptance.length) {
    const outcome = section("outcome", "完成条件与结果");
    for (const criterion of acceptance) {
      const refs = [...new Set(evidence.filter(proof => proof.criterion === criterion).flatMap(proof => proof.evidence_refs || []))];
      meta(outcome, refs.length ? "已记录证据" : "待验证", criterion);
      if (refs.length) meta(outcome, "证据引用", refs.join(" · "));
    }
    for (const criterion of readiness?.uncovered || []) meta(outcome, "尚未覆盖", criterion);
    meta(outcome, "验证方法", (unit.verification || []).join("；"));
  }
  if (node.source_refs?.length || checkpoint.decisions?.length || checkpoint.retrieval_refs?.length) {
    const basis = section("basis", "研究与依据");
    meta(basis, "决策记录", (checkpoint.decisions || []).join("；"));
    meta(basis, "来源", (node.source_refs || []).join(" · "));
    meta(basis, "记忆来源", (checkpoint.retrieval_refs || []).join(" · "));
  }
  const record = section("record", "节点记录");
  meta(record, "节点编号", unit.unit_id);
  if (unit.source_sequence != null) meta(record, "记录版本", String(unit.source_sequence));
  meta(record, "类型定义", node.custom_type?.description);
  meta(record, "支撑父级验收", (node.covers || []).join("；"));
  return card;
}

export function captureWorkTreeState(root) { return root?.captureState?.() || null; }

/** Domain facts become view items without creating or changing work records. */
export function workTreeItems(units, readiness = {}, options = {}) {
  const nodes = new Map(units.map(unit => [unit.unit_id, unit]));
  return units.map(unit => ({
    id: unit.unit_id, title: unit.objective, parentId: unit.node?.parent_id || null,
    kind: unit.node?.kind || "requirement", glyph: kindGlyphs[unit.node?.kind] || "◇",
    kindLabel: unit.node?.custom_type?.name || t(kindLabels[unit.node?.kind || "requirement"]),
    status: unit.status, statusLabel: reviewStatus(unit) || (["wait", "plan", "integrate"].includes(readiness[unit.unit_id]?.step) ? t(stepLabels[readiness[unit.unit_id].step]) : localizedStatusWord(unit.status, "unit")),
    reviewState: workReviewPending(unit) ? "pending" : workIsCurrent(unit) ? "current" : "historical",
    primary: unit.status === "active", expanded: !["done", "superseded"].includes(unit.status),
    renderDetails({ inspector, actions, detailOpen, reveal }) {
    if (options.onDecompose) {
      const split = text("button", "ghost work-node-split", t("裂变需求"));
      split.type = "button";
      const blockers = options.planningBlockers?.[unit.unit_id];
      const missingCriteria = !(unit.acceptance || []).length;
      split.disabled = !blockers || blockers.length > 0 || missingCriteria;
      const reason = blockers?.join("；") || (!blockers ? t("正在读取操作条件") : missingCriteria ? t("先明确完成条件，再拆分子工作") : "");
      split.title = reason;
      split.onclick = () => options.onDecompose(unit, split);
      actions.append(split);
    }
    if (options.onRevise && workIsCurrent(unit) && !workReviewPending(unit)) {
      const revise = text("button", "ghost work-node-revise", t("调整方向")); revise.type = "button";
      revise.onclick = () => options.onRevise(unit); actions.append(revise);
    }
    if (options.onStart && ["execute", "integrate"].includes(readiness[unit.unit_id]?.step)) {
      const start = text("button", "ghost work-node-start", t("推进此节点"));
      start.type = "button"; start.onclick = () => options.onStart(unit); actions.append(start);
    }
    inspector.append(workUnitCard(unit, readiness[unit.unit_id], nodes, detailOpen));
    for (const dependency of unit.dependencies || []) {
      if (!nodes.has(dependency)) continue;
      const link = text("button", "ghost work-node-reference", `${t("查看依赖")} · ${nodes.get(dependency).objective}`);
      link.type = "button"; link.onclick = () => reveal(dependency); inspector.append(link);
    }
    for (const successor of unit.review?.resolution?.decision?.replacements || []) {
      if (!nodes.has(successor)) continue;
      const link = text("button", "ghost work-node-reference", `${t("后继工作")} · ${nodes.get(successor).objective}`);
      link.type = "button"; link.onclick = () => reveal(successor); inspector.append(link);
    }
    },
  }));
}

export function renderWorkTree(units, readiness = {}, prior = null, options = {}) {
  return renderTreeView(workTreeItems(units, readiness, options), prior);
}
