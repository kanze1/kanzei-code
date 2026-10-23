/* global window, document */
import path from "node:path";

export default async function run({ browser, server, check, output }) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const errors = []; page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${server.origin}/?scene=chat&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async () => {
    const fixture = await window.__kzPreview.fixtures(), docs = fixture.commands.docs_snapshot();
    const base = docs.requirements[0];
    const legacy = { ...base, id: "R-901", title: "保留原进展的现有需求：桌面端/前端 E2 测试 harness 与延期 E2 清单", status: "doing", closed: false, blocked: false, block_reasons: [], fields: [["目标", "完成现有交付"]], work_units: [] };
    const unit = (n, parent, kind, title) => ({ unit_id: `R-902/W${n}`, requirement_id: "R-902", objective: title, status: "ready", source_sequence: 1, acceptance: ["可纠正偏好"], dependencies: [], evidence: [], node: { kind, parent_id: parent, covers: parent ? ["可纠正偏好"] : [], questions: [] } });
    const goal = unit(1, null, "goal", "实现偏好纠正"), research = unit(2, goal.unit_id, "research", "调研偏好纠正方案");
    const managed = { ...legacy, id: "R-902", title: "开发个人适配 Agent", status: "todo", work_units: [goal, research],
      work_readiness: { [goal.unit_id]: { step: "wait", blockers: ["子节点未结束: R-902/W2"], uncovered: [] }, [research.unit_id]: { step: "execute", blockers: [], uncovered: [] } },
      work_planning_blockers: { [goal.unit_id]: [], [research.unit_id]: [] } };
    docs.requirements = [legacy, managed]; docs.work_units = [goal, research]; docs.archived.req = 0;
    window.__managementTreeDocs = docs;
    window.__kzPreview.setCommand("docs_snapshot", () => structuredClone(docs));
    const shell = await import("/03-shell.js");
    await (await import("/01-project-resources.js")).readProjectDocuments(shell.currentProject, { force: true });
  });
  await page.locator('[data-work-surface="project"]').click();
  const tree = page.locator(".management-goal-tree"); await tree.waitFor();
  const row = id => tree.locator(`[data-work-node-id="${id}"] > .work-tree-row`);
  check("The real management entry defaults to a goal tree", await page.locator(".management-work-views").getByRole("button", { name: "目标树", exact: true }).getAttribute("aria-pressed"), "true");
  check("Existing requirements remain visible without a Work Unit opt-in", await row("R-901").count(), 1);
  check("Legacy batch progress is not fabricated into child nodes", await tree.locator('[data-work-node-id="R-901"] [role="treeitem"]').count(), 0);
  check("Only recorded children appear below their tracker root", await tree.locator('[data-work-node-id="R-902"] [role="treeitem"]').count(), 2);
  check("The actual child keeps its research type", await row("R-902/W2").locator(".work-tree-kind").innerText(), "调研节点");
  check("Long existing titles keep decision cards compact", await tree.locator(".work-tree-row").evaluateAll(rows => rows.every(row => row.getBoundingClientRect().height <= 120)), true);
  await row("R-901").click();
  check("An unplanned requirement explains the missing tree", (await tree.locator(".work-tree-inspector").innerText()).includes("尚未拆分为工作节点"), true);
  check("Legacy roots have no invalid Work Unit mutation buttons", await tree.locator(".work-node-split, .work-node-revise, .work-node-start").count(), 0);
  await tree.getByRole("button", { name: "查看需求", exact: true }).click();
  check("The root opens the existing requirement editor", (await page.locator(".management-body .doc-full-title").innerText()).includes("保留原进展的现有需求"), true);
  await page.locator(".management-detail-nav button").first().click();
  await row("R-902/W2").click();
  await page.evaluate(async () => {
    const shell = await import("/03-shell.js");
    await (await import("/01-project-resources.js")).readProjectDocuments(shell.currentProject, { force: true });
  });
  check("Background refresh preserves the selected real child", await tree.locator('[data-work-node-id="R-902/W2"]').getAttribute("aria-selected"), "true");
  const search = page.locator('.management-tools input[type="search"]');
  await search.fill("调研偏好");
  check("Searching child content retains the entire ancestor path", await tree.locator('[role="treeitem"]:visible').count(), 3);
  check("Search updates the visible tracker count", await page.locator(".management-list-count").innerText(), "显示条目 · 1 / 2");
  await search.fill("没有这样的节点");
  check("No matches show an explicit empty result", await tree.locator(".work-tree-empty").isVisible(), true);
  check("No matches do not leave an unrelated inspector visible", await tree.locator(".work-tree-inspector").isVisible(), false);
  await search.fill("");
  await row("R-902/W1").click();
  await tree.getByRole("button", { name: "聚焦此分支", exact: true }).click();
  check("Focused project branches retain their management identity", await tree.getAttribute("data-tracker-kind"), "req");
  await search.fill("没有这样的节点");
  check("Scoped search keeps the visible count connected", await page.locator(".management-list-count").innerText(), "显示条目 · 0 / 2");
  await search.fill("");
  await tree.getByRole("button", { name: "全部目标", exact: true }).click();
  await row("R-902/W2").click();
  await page.locator(".management-work-views").getByRole("button", { name: "列表", exact: true }).click();
  check("The original list remains available", await page.locator(".management-row").count(), 2);
  await page.locator(".management-work-views").getByRole("button", { name: "目标树", exact: true }).click();
  check("Switching back keeps the selected child", await tree.locator('[data-work-node-id="R-902/W2"]').getAttribute("aria-selected"), "true");
  await page.screenshot({ path: path.join(output, "management-tree-light.png") });
  await page.evaluate(() => document.documentElement.dataset.theme = "dark");
  await tree.evaluate(node => Promise.all(node.getAnimations({ subtree: true }).map(animation => animation.finished)));
  await page.screenshot({ path: path.join(output, "management-tree-dark.png") });
  await page.setViewportSize({ width: 840, height: 1000 });
  check("Management tree fits the narrow workspace", await page.locator(".management-body").evaluate(node => node.scrollWidth <= node.clientWidth + 1), true);
  await row("R-901").click();
  await page.evaluate(async () => {
    const fixture = await window.__kzPreview.fixtures();
    for (const process of fixture.state.processes) process.running = false;
    await (await import("/09-sessions.js")).refreshProcesses();
    window.__managementPlanRequests = [];
    window.__kzPreview.setCommand("run_prompt", args => { window.__managementPlanRequests.push(structuredClone(args)); return null; });
  });
  await tree.getByRole("button", { name: "让 Agent 规划", exact: true }).click();
  await page.waitForFunction(() => window.__managementPlanRequests.length === 1);
  const request = await page.evaluate(() => window.__managementPlanRequests[0]);
  check("Planning dispatch preserves the exact legacy requirement", request.prompt.includes("R-901") && request.prompt.includes("保留原进展的现有需求"), true);
  check("Planning does not claim or manufacture work", { workItemId: request.workItemId, executionBatch: request.executionBatch }, { workItemId: null, executionBatch: false });
  check("Management has no browser runtime errors", errors, []);
}
