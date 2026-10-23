/* global window, document */
import path from "node:path";

export default async function run({ browser, server, check, output }) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
  const errors = [];
  page.on("pageerror", error => { errors.push(error.message); console.error(error.message); });
  await page.goto(`${server.origin}/?scene=docs&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async () => {
    const fixture = await window.__kzPreview.fixtures();
    const docs = fixture.commands.docs_snapshot();
    const unit = (n, kind, parent, status, objective) => ({
      unit_id: `R-910/W${n}`, requirement_id: "R-910", objective, status, source_sequence: n,
      node: { kind, parent_id: parent, covers: parent ? ["个人偏好可以纠正"] : [], questions: [], explanation: "根据偏好纠正场景选择方案", source_refs: ["design.md"] },
      acceptance: ["个人偏好可以纠正"], verification: ["运行偏好纠正场景"], evidence: status === "done" ? [{ criterion: "个人偏好可以纠正", evidence_refs: ["evidence/preferences.json"] }] : [],
      dependencies: [], last_checkpoint: null,
    });
    const units = [unit(1, "goal", null, "ready", "开发个人适配的 agent"), unit(2, "research", "R-910/W1", "done", "研究偏好保存和纠正机制"), unit(3, "implementation", "R-910/W1", "ready", "实现可以纠正的偏好记忆"), unit(4, "defect", "R-910/W3", "superseded", "已被新存储方案替代的问题")];
    units[1].node.custom_type = { name: "偏好研究", description: "研究偏好修正", deliverable: "偏好研究报告" };
    units[2].dependencies = ["R-910/W2"];
    units[3].terminal_reason = "旧方案已退役，保留原问题";
    const base = docs.requirements[0];
    docs.requirements = [{ ...base, id: "R-910", title: "个人适配 agent", status: "doing", blocked: false, block_reasons: [], dependencies: [], dependents: [], prior_art: null, requirement: { format: 1 }, fields: [["目标", "开发个人适配 agent"]], work_units: units,
      work_readiness: { "R-910/W1": { step: "wait", blockers: ["子节点未结束: R-910/W3"], uncovered: [] }, "R-910/W2": { step: "finished", blockers: [], uncovered: [] }, "R-910/W3": { step: "execute", blockers: [], uncovered: [] }, "R-910/W4": { step: "finished", blockers: [], uncovered: [] } } }];
    docs.requirements[0].work_planning_blockers = Object.fromEntries(units.map(node => [node.unit_id, ["done", "superseded"].includes(node.status) ? ["节点已结束"] : []]));
    docs.work_units = units;
    docs.archived.req = 0;
    window.__workTreeDocs = docs;
    window.__kzPreview.setCommand("docs_snapshot", () => structuredClone(docs));
    window.__workPlanCalls = [];
    window.__kzPreview.setCommand("work_node_decompose", args => {
      window.__workPlanCalls.push(args);
      if (window.__workPlanCalls.length === 1) { units[2].source_sequence = 9; throw new Error("WORK_PLAN_REJECTED: version changed"); }
      const child = unit(5, args.children[0].node.kind, args.id, "ready", args.children[0].objective);
      child.node.custom_type = args.children[0].node.custom_type;
      units.push(child);
      docs.requirements[0].work_readiness[child.unit_id] = { step: "execute", blockers: [], uncovered: [] };
      docs.requirements[0].work_planning_blockers[child.unit_id] = [];
      return { children: [child], ids: { child1: child.unit_id } };
    });
    (await import("/12-docs-pages.js")).renderDocsSnapshot(docs);
  });
  const entry = page.locator('.doc-item[data-doc-id="R-910"]').first();
  await entry.locator(".doc-row").click();
  const outer = entry.locator(".work-unit-details");
  check("Goal tree is open by default", await outer.evaluate(node => node.open), true);
  check("Every node is shown exactly once", await outer.locator(".work-tree-node").count(), 4);
  check("Cancelled work is separate from verified work", await outer.locator(":scope > summary").innerText(), "目标树 · 1 已验证节点 · 1 已替代节点");
  check("The defect belongs under its implementation node", await outer.locator('[data-work-node-id="R-910/W3"] .work-tree-children [data-work-node-id="R-910/W4"]').count(), 1);
  check("Parent state is waiting rather than pretending to execute", await outer.locator('[data-work-node-id="R-910/W1"] > .work-tree-row .work-unit-status').innerText(), "等待前提满足");
  check("Parent waiting reason comes from the domain", (await outer.innerText()).includes("子节点未结束: R-910/W3"), true);
  const research = outer.locator('[data-work-node-id="R-910/W2"]');
  await research.locator(":scope > .work-tree-row").click();
  check("Accepted research exposes evidence", (await outer.locator(".work-tree-inspector").innerText()).includes("evidence/preferences.json"), true);
  check("Completed nodes cannot be decomposed", await outer.locator(".work-node-split").isDisabled(), true);
  check("Custom type definition stays inspectable", (await outer.locator(".work-tree-inspector").innerText()).includes("偏好研究报告"), true);
  const implementation = outer.locator('[data-work-node-id="R-910/W3"]');
  await implementation.locator(":scope > .work-tree-row").click();
  check("Cross-branch dependency is visible", (await outer.locator(".work-tree-inspector").innerText()).includes("依赖结果: R-910/W2"), true);
  await outer.locator(".work-node-reference").click();
  check("Dependency reference selects its actual node", await research.getAttribute("aria-selected"), "true");
  await implementation.locator(":scope > .work-tree-row > .work-tree-toggle").click();
  await page.evaluate(async () => (await import("/12-docs-pages.js")).renderDocsSnapshot(window.__workTreeDocs));
  check("Refresh preserves a user's collapsed branch", await implementation.getAttribute("aria-expanded"), "false");
  await implementation.focus();
  await page.keyboard.press("ArrowRight");
  check("A branch opens from the keyboard", await implementation.getAttribute("aria-expanded"), "true");
  await page.keyboard.press("Enter");
  await outer.locator('input[type="search"]').fill("研究偏好");
  check("Search retains matching node ancestry", await outer.locator('[role="treeitem"]:visible').count(), 2);
  await outer.locator('input[type="search"]').fill("");
  await implementation.locator(":scope > .work-tree-row").click();
  await outer.locator(".work-node-focus").click();
  check("Focusing a branch removes unrelated ancestry and indentation", await outer.locator(".work-tree-node").count(), 2);
  await outer.locator(".work-node-reference").click();
  check("Dependency navigation can leave a focused branch", await research.getAttribute("aria-selected"), "true");
  await implementation.locator(":scope > .work-tree-row").click();
  await outer.locator(".work-node-split").click();
  const dialog = page.locator(".work-plan-dialog");
  await dialog.getByLabel("拆分理由", { exact: true }).fill("先验证偏好读取性能");
  await dialog.getByLabel("工作目标", { exact: true }).fill("分析偏好读取性能");
  await dialog.getByLabel("节点类型", { exact: true }).selectOption("research");
  await dialog.getByLabel("扩展类型", { exact: true }).selectOption("__new__");
  await dialog.getByLabel("类型名称", { exact: true }).fill("性能分析");
  await dialog.getByLabel("类型定义", { exact: true }).fill("测量实际瓶颈");
  await dialog.getByLabel("预期产物", { exact: true }).fill("性能报告");
  await dialog.getByLabel("验收标准，每行一条", { exact: true }).fill("提供性能基线");
  await dialog.getByLabel("验证方法", { exact: true }).fill("运行基准测量");
  await dialog.locator('input[type="checkbox"]').check();
  await page.screenshot({ path: path.join(output, "work-tree-split.png") });
  await dialog.getByRole("button", { name: "应用裂变", exact: true }).click();
  await dialog.locator(".work-plan-error").filter({ hasText: "WORK_PLAN_REJECTED" }).waitFor();
  check("Failed submission retains the draft", await dialog.getByLabel("工作目标", { exact: true }).inputValue(), "分析偏好读取性能");
  check("Failed submission creates no pretend branch", await outer.locator(".work-tree-node").count(), 4);
  await dialog.getByRole("button", { name: "重读父节点", exact: true }).click();
  await dialog.locator(".work-plan-error").filter({ hasText: "父节点已更新" }).waitFor();
  check("Re-reading a changed parent preserves the child objective", await dialog.getByLabel("工作目标", { exact: true }).inputValue(), "分析偏好读取性能");
  await dialog.getByRole("button", { name: "应用裂变", exact: true }).click();
  await dialog.waitFor({ state: "detached" });
  await page.evaluate(async () => (await import("/12-docs-pages.js")).renderDocsSnapshot(window.__workTreeDocs));
  check("Committed split creates one child", await outer.locator(".work-tree-node").count(), 5);
  check("Split carries exact node and observed version", await page.evaluate(() => [window.__workPlanCalls[0].id, window.__workPlanCalls[0].expectedSequence]), ["R-910/W3", 3]);
  check("Resubmission uses the explicitly refreshed version", await page.evaluate(() => window.__workPlanCalls[1].expectedSequence), 9);
  check("Custom type carries a definition, not grants", await page.evaluate(() => Object.keys(window.__workPlanCalls[0].children[0].node.custom_type).sort()), ["deliverable", "description", "name"]);
  await page.evaluate(async () => (await import("/12-docs-pages.js")).renderDocsSnapshot(window.__workTreeDocs));
  check("Refresh does not replay branch entrance", await outer.locator(".work-node-new").count(), 0);
  for (const theme of ["light", "dark"]) {
    await page.evaluate(value => document.documentElement.dataset.theme = value, theme);
    const contrastFailures = await page.evaluate(() => {
      const colors = css => css.match(/[\d.]+/g).map(Number);
      const luminance = rgb => rgb.slice(0, 3).map(value => { const c = value / 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; }).reduce((sum, value, i) => sum + value * [0.2126, 0.7152, 0.0722][i], 0);
      const failures = [], tree = document.querySelector(".work-tree");
      for (const kind of ["goal", "requirement", "implementation", "defect", "research", "design", "documentation", "verification"]) {
        const item = document.createElement("li"); item.className = "work-tree-node"; item.dataset.kind = kind;
        const row = document.createElement("div"); row.className = "work-tree-row";
        const label = document.createElement("span"); label.className = "work-tree-kind"; label.textContent = kind;
        row.append(label); item.append(row); tree.append(item);
        for (const selected of [false, true]) {
          item.setAttribute("aria-selected", String(selected));
          // Disable transition sampling: this check measures each settled surface.
          row.style.transition = "none";
          const chain = []; for (let node = label; node; node = node.parentElement) chain.unshift(node);
          let background = [255, 255, 255];
          for (const node of chain) {
            const rgba = colors(getComputedStyle(node).backgroundColor), alpha = rgba[3] ?? 1;
            background = background.map((channel, i) => channel * (1 - alpha) + rgba[i] * alpha);
          }
          const fg = luminance(colors(getComputedStyle(label).color)), bg = luminance(background);
          const ratio = (Math.max(fg, bg) + .05) / (Math.min(fg, bg) + .05);
          if (ratio < 4.5) failures.push({ kind, selected, ratio });
        }
        item.remove();
      }
      return failures;
    });
    check(`${theme}: all eight type labels meet 4.5:1 on normal and selected surfaces`, contrastFailures, []);
  }
  await page.evaluate(() => document.documentElement.dataset.theme = "light");
  await page.screenshot({ path: path.join(output, "work-tree-wide.png") });
  await page.evaluate(() => document.documentElement.dataset.theme = "dark");
  await page.screenshot({ path: path.join(output, "work-tree-dark.png") });
  await page.emulateMedia({ reducedMotion: "reduce" });
  check("Reduced motion removes perceptible branch transitions", await implementation.locator(":scope > .work-tree-row").evaluate(node => parseFloat(getComputedStyle(node).transitionDuration) <= 0.001), true);
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
