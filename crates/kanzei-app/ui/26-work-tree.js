import { localizedStatusWord, t } from "./02-i18n.js";

const kindLabels = { goal: "目标节点", requirement: "需求节点", defect: "缺陷节点", research: "调研节点", design: "设计节点", documentation: "文档节点", verification: "验证节点" };
const stepLabels = { plan: "补齐理解与计划", execute: "推进当前工作", integrate: "验收整体结果", wait: "等待前提满足", finished: "工作已结束" };

function text(tag, className, content) {
  const element = document.createElement(tag);
  element.className = className;
  element.textContent = content;
  return element;
}

function workUnitCard(unit, readiness) {
  const card = document.createElement("article");
  card.className = `work-unit-card ${unit.status}`;
  const head = document.createElement("div");
  head.className = "work-unit-head";
  head.append(text("code", "", unit.unit_id), text("span", "work-unit-status", localizedStatusWord(unit.status, "unit")));
  card.append(head, text("div", "work-unit-objective", unit.objective));
  const meta = (label, content, state = "") => {
    if (content) card.append(text("div", `work-unit-meta ${state}`, `${t(label)}: ${content}`));
  };
  const node = unit.node || {};
  if (readiness) meta("下一步", t(stepLabels[readiness.step] || readiness.step));
  meta("当前理解", node.explanation);
  for (const question of node.questions || []) meta("未决问题", question, "blocked");
  meta("支撑父级验收", (node.covers || []).join("；"));
  meta("依赖结果", (unit.dependencies || []).join(" · "));
  for (const blocker of readiness?.blockers || []) meta("阻塞原因", blocker, "blocked");
  for (const criterion of readiness?.uncovered || []) meta("尚未覆盖", criterion, "blocked");
  const checkpoint = unit.last_checkpoint || {};
  meta("实质进展", checkpoint.summary);
  meta("决策记录", (checkpoint.decisions || []).join("；"));
  meta("记忆来源", (checkpoint.retrieval_refs || []).join(", ") || t("未记录"));
  meta("验证方法", (unit.verification || []).join("；") || t("未声明"));
  if (!readiness) meta("阻塞原因", unit.blocked_reason, "blocked");
  meta("下一步", checkpoint.next_action);
  const evidence = unit.evidence || [];
  const acceptance = unit.acceptance || [];
  meta("验收证据", `${evidence.length}/${acceptance.length}`);
  for (const criterion of acceptance) {
    const proof = evidence.find(item => item.criterion === criterion);
    meta(proof ? "已记录证据" : "待验证", criterion);
    if (proof) meta("证据引用", (proof.evidence_refs || []).join(" · "));
  }
  meta("来源", (node.source_refs || []).join(" · "));
  if (unit.terminal_reason) meta("结束原因", unit.terminal_reason);
  return card;
}

/** Hierarchy is presentation only. Readiness comes from the core domain snapshot. */
export function renderWorkTree(units, readiness = {}, prior = new Map()) {
  const tree = document.createElement("ul");
  tree.className = "work-unit-list work-tree";
  tree.setAttribute("aria-label", t("目标树"));
  const ids = new Set(units.map(unit => unit.unit_id));
  const children = new Map();
  for (const unit of units) {
    const parent = unit.node?.parent_id || null;
    if (!children.has(parent)) children.set(parent, []);
    children.get(parent).push(unit);
  }
  const rendered = new Set();
  const append = (list, unit, invalid = false) => {
    if (rendered.has(unit.unit_id)) return;
    rendered.add(unit.unit_id);
    const item = document.createElement("li");
    item.className = "work-tree-item";
    const branch = document.createElement("details");
    branch.className = "work-tree-node";
    branch.dataset.workNodeId = unit.unit_id;
    branch.dataset.workNodeState = `node:${unit.unit_id}`;
    branch.open = prior.has(branch.dataset.workNodeState) ? prior.get(branch.dataset.workNodeState) : !["done", "superseded"].includes(unit.status);
    const summary = document.createElement("summary");
    const step = readiness[unit.unit_id]?.step;
    const status = ["wait", "plan", "integrate"].includes(step) ? t(stepLabels[step]) : localizedStatusWord(unit.status, "unit");
    summary.append(text("span", "work-tree-kind", t(kindLabels[unit.node?.kind || "requirement"] || unit.node.kind)), text("span", "work-tree-title", unit.objective), text("span", "work-unit-status", status));
    branch.append(summary);
    if (invalid) branch.append(text("div", "work-unit-meta blocked", t("节点归属异常，请核对工作图")));
    const inspector = document.createElement("details");
    inspector.className = "work-node-inspector";
    inspector.dataset.workNodeState = `info:${unit.unit_id}`;
    inspector.open = prior.get(inspector.dataset.workNodeState) === true;
    inspector.append(text("summary", "", t("验收与依据")), workUnitCard(unit, readiness[unit.unit_id]));
    branch.append(inspector);
    const descendants = children.get(unit.unit_id) || [];
    if (descendants.length) {
      const nested = document.createElement("ul");
      nested.className = "work-tree-children";
      for (const child of descendants) append(nested, child);
      branch.append(nested);
    }
    item.append(branch);
    list.append(item);
  };
  for (const unit of units.filter(unit => !unit.node?.parent_id || !ids.has(unit.node.parent_id))) append(tree, unit, Boolean(unit.node?.parent_id && !ids.has(unit.node.parent_id)));
  // Invalid cycles must remain visible instead of silently dropping work from the tree.
  for (const unit of units) if (!rendered.has(unit.unit_id)) append(tree, unit, true);
  return tree;
}
