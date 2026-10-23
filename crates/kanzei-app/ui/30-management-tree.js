import { t, localizedDocStatus } from "./02-i18n.js";
import { renderTreeView } from "./26-tree-view.js";
import { workTreeItems } from "./26-work-tree.js";

const text = (tag, content, className = "") => {
  const node = document.createElement(tag); node.textContent = content; node.className = className; return node;
};
const button = (label, run) => {
  const node = text("button", t(label), "ghost"); node.type = "button"; node.onclick = run; return node;
};

/** Tracker roots are view records, never fabricated WorkProjection facts. */
export function renderManagementTree(entries, prior, search, handlers) {
  const owners = new Map(entries.map(entry => [entry.id, entry]));
  const units = entries.flatMap(entry => entry.work_units || []);
  const unitOwners = new Map(units.map(unit => [unit.unit_id, unit.requirement_id]));
  const readiness = Object.assign({}, ...entries.map(entry => entry.work_readiness || {}));
  const planningBlockers = Object.assign({}, ...entries.map(entry => entry.work_planning_blockers || {}));
  const work = workTreeItems(units, readiness, {
    planningBlockers,
    onDecompose: unit => handlers.onDecompose(unit, units),
    onRevise: unit => handlers.onRevise(owners.get(unit.requirement_id), unit),
    onStart: unit => handlers.onStart(owners.get(unit.requirement_id), unit),
  }).map(item => ({ ...item, parentId: item.parentId || unitOwners.get(item.id) }));
  const roots = entries.map(entry => ({
    id: entry.id, title: entry.title, parentId: null,
    kind: entry.id.startsWith("D-") ? "defect" : "goal", glyph: entry.id.startsWith("D-") ? "!" : "◎",
    kindLabel: t(entry.id.startsWith("D-") ? "缺陷" : "需求"),
    status: entry.status, statusLabel: localizedDocStatus(entry.status),
    primary: ["doing", "fixing"].includes(entry.status), expanded: !entry.closed,
    reviewState: "current",
    renderDetails({ inspector, actions }) {
      actions.append(button(entry.id.startsWith("D-") ? "查看缺陷" : "查看需求", () => handlers.onOpen(entry)));
      if (!entry.closed && !entry.id.startsWith("D-") && !entry.blocked) {
        actions.append(button("让 Agent 规划", () => handlers.onPlan(entry)));
      }
      const detail = text("div", "", "management-goal-detail");
      detail.append(text("p", localizedDocStatus(entry.status), "dim"));
      const objective = entry.fields?.find(([key]) => key === "目标")?.[1];
      if (objective && objective !== entry.title) detail.append(text("p", objective));
      const blockers = entry.block_reasons || [];
      if (blockers.length) detail.append(text("p", blockers.join("；"), "work-unit-meta blocked"));
      if (!(entry.work_units || []).length) detail.append(text("p", t("尚未拆分为工作节点"), "management-goal-unplanned"));
      inspector.append(detail);
    },
  }));
  const root = renderTreeView([...roots, ...work], { ...prior, query: search.value }, { searchControl: search, onFilter: handlers.onFilter });
  root.classList.add("management-goal-tree");
  return root;
}
