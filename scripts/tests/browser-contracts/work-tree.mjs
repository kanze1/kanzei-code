/* global window, document */
import path from "node:path";

export default async function run({ browser, server, check, output }) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto(`${server.origin}/?scene=docs&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async () => {
    const fixture = await window.__kzPreview.fixtures();
    const docs = fixture.commands.docs_snapshot();
    const unit = (n, kind, parent, status, objective) => ({
      unit_id: `R-910/W${n}`, requirement_id: "R-910", objective, status,
      node: { kind, parent_id: parent, covers: parent ? ["个人偏好可以纠正"] : [], questions: [], explanation: "根据偏好纠正场景选择方案", source_refs: ["design.md"] },
      acceptance: ["个人偏好可以纠正"], verification: ["运行偏好纠正场景"], evidence: status === "done" ? [{ criterion: "个人偏好可以纠正", evidence_refs: ["evidence/preferences.json"] }] : [],
      dependencies: [], last_checkpoint: null,
    });
    const units = [unit(1, "goal", null, "ready", "开发个人适配的 agent"), unit(2, "research", "R-910/W1", "done", "研究偏好保存和纠正机制"), unit(3, "requirement", "R-910/W1", "active", "实现可以纠正的偏好记忆"), unit(4, "defect", "R-910/W3", "superseded", "已被新存储方案替代的问题")];
    units[2].dependencies = ["R-910/W2"];
    units[3].terminal_reason = "旧方案已退役，保留原问题";
    const base = docs.requirements[0];
    docs.requirements = [{ ...base, id: "R-910", title: "个人适配 agent", status: "doing", blocked: false, block_reasons: [], dependencies: [], dependents: [], prior_art: null, requirement: { format: 1 }, fields: [["目标", "开发个人适配 agent"]], work_units: units,
      work_readiness: { "R-910/W1": { step: "wait", blockers: ["子节点未结束: R-910/W3"], uncovered: [] }, "R-910/W2": { step: "finished", blockers: [], uncovered: [] }, "R-910/W3": { step: "execute", blockers: [], uncovered: [] }, "R-910/W4": { step: "finished", blockers: [], uncovered: [] } } }];
    docs.work_units = units;
    docs.archived.req = 0;
    window.__workTreeDocs = docs;
    window.__kzPreview.setCommand("docs_snapshot", () => structuredClone(docs));
    (await import("/12-docs-pages.js")).renderDocsSnapshot(docs);
  });
  const entry = page.locator('.doc-item[data-doc-id="R-910"]').first();
  await entry.locator(".doc-row").click();
  const outer = entry.locator(".work-unit-details");
  await outer.locator(":scope > summary").click();
  check("Every node is shown exactly once", await outer.locator(".work-tree-node").count(), 4);
  check("Cancelled work is separate from verified work", await outer.locator(":scope > summary").innerText(), "目标树 · 1 已验证节点 · 1 已替代节点 · R-910/W3 · 开发中");
  check("The defect belongs under its implementation node", await outer.locator('[data-work-node-id="R-910/W3"] .work-tree-children [data-work-node-id="R-910/W4"]').count(), 1);
  check("Parent state is waiting rather than pretending to execute", await outer.locator('[data-work-node-id="R-910/W1"] > summary .work-unit-status').innerText(), "等待前提满足");
  await outer.locator('[data-work-node-id="R-910/W1"] > .work-node-inspector > summary').click();
  check("Parent waiting reason comes from the domain", (await outer.innerText()).includes("子节点未结束: R-910/W3"), true);
  const research = outer.locator('[data-work-node-id="R-910/W2"]');
  await research.locator(":scope > summary").click();
  await research.locator(":scope > .work-node-inspector > summary").click();
  check("Accepted research exposes evidence", (await research.innerText()).includes("evidence/preferences.json"), true);
  const implementation = outer.locator('[data-work-node-id="R-910/W3"]');
  await implementation.locator(":scope > .work-node-inspector > summary").click();
  check("Cross-branch dependency is visible", (await implementation.innerText()).includes("依赖结果: R-910/W2"), true);
  await implementation.locator(":scope > summary").click();
  await page.evaluate(async () => (await import("/12-docs-pages.js")).renderDocsSnapshot(window.__workTreeDocs));
  check("Refresh preserves a user's collapsed branch", await implementation.getAttribute("open"), null);
  await implementation.locator(":scope > summary").focus();
  await page.keyboard.press("Enter");
  check("A branch opens from the keyboard", await implementation.evaluate(node => node.open), true);
  await research.locator(":scope > .work-node-inspector > summary").click();
  await outer.locator('[data-work-node-id="R-910/W1"] > .work-node-inspector > summary').click();
  await page.screenshot({ path: path.join(output, "work-tree-wide.png") });
  await page.setViewportSize({ width: 840, height: 1000 });
  check("Tree stays inside the document at a narrow width", await outer.evaluate(node => node.scrollWidth <= node.clientWidth + 1), true);
  await page.screenshot({ path: path.join(output, "work-tree-narrow.png") });
  await page.evaluate(async () => {
    (await import("/02-i18n.js")).setLanguagePreference("en");
    (await import("/12-docs-pages.js")).renderDocsSnapshot(window.__workTreeDocs);
  });
  check("Tree headings follow the language setting", (await outer.locator(":scope > summary").innerText()).startsWith("Goal tree"), true);
  check("Mode picker uses the new product name", await page.locator('#profile-select option[value="dev-auto"]').innerText(), "Ongoing development and maintenance");
  check("No browser runtime errors", errors, []);
}
