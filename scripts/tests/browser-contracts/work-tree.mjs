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
      child.acceptance = args.children[0].acceptance;
      child.verification = args.children[0].verification;
      units.push(child);
      docs.requirements[0].work_readiness[child.unit_id] = { step: child.acceptance.length ? "execute" : "plan", blockers: [], uncovered: [] };
      docs.requirements[0].work_planning_blockers[child.unit_id] = [];
      return { children: [child], ids: { child1: child.unit_id } };
    });
    (await import("/12-docs-pages.js")).renderDocsSnapshot(docs);
  });
  const entry = page.locator('.doc-item[data-doc-id="R-910"]').first();
  await entry.locator(".doc-row").click();
  const outer = entry.locator(".work-unit-details");
  check("Goal tree is open by default", await outer.evaluate(node => node.open), true);
  check("Desktop starts with the card map", await outer.locator(".work-tree-workspace").getAttribute("data-view"), "map");
  check("Every node is shown exactly once", await outer.locator(".work-tree-node").count(), 4);
  check("Cancelled work is separate from verified work", await outer.locator(":scope > summary").innerText(), "目标树 · 1 已验证节点 · 1 已替代节点");
  check("The defect belongs under its implementation node", await outer.locator('[data-work-node-id="R-910/W3"] .work-tree-children [data-work-node-id="R-910/W4"]').count(), 1);
  check("Parent state is waiting rather than pretending to execute", await outer.locator('[data-work-node-id="R-910/W1"] > .work-tree-row .work-unit-status').innerText(), "等待前提满足");
  check("Parent waiting reason comes from the domain", (await outer.innerText()).includes("子节点未结束: 实现可以纠正的偏好记忆"), true);
  const research = outer.locator('[data-work-node-id="R-910/W2"]');
  await research.locator(":scope > .work-tree-row").click();
  check("Evidence and record are folded until requested", await outer.locator('.work-tree-inspector details[open]').count(), 0);
  check("Internal identifiers are absent from default human detail", (await outer.locator(".work-tree-inspector").innerText()).includes("R-910/W2"), false);
  check("No empty-field placeholders are shown", /未记录|未声明|0\/1/.test(await outer.locator(".work-tree-inspector").innerText()), false);
  await outer.locator('[data-section="outcome"] > summary').click();
  check("Accepted research exposes evidence", (await outer.locator(".work-tree-inspector").innerText()).includes("evidence/preferences.json"), true);
  check("Completed nodes cannot be decomposed", await outer.locator(".work-node-split").isDisabled(), true);
  check("Explanation and sources remain in the inspector", await outer.locator(".work-tree-inspector").evaluate(node => node.textContent.includes("根据偏好纠正场景选择方案") && node.textContent.includes("design.md")), true);
  check("Custom type deliverable stays inspectable", (await outer.locator(".work-tree-inspector").innerText()).includes("偏好研究报告"), true);
  const implementation = outer.locator('[data-work-node-id="R-910/W3"]');
  await implementation.locator(":scope > .work-tree-row").click();
  check("Cross-branch dependency uses its human title", await outer.locator(".work-node-reference").innerText(), "查看依赖 · 研究偏好保存和纠正机制");
  check("Map cards keep explanation out of the overview", (await research.locator(":scope > .work-tree-row").innerText()).includes("根据偏好纠正场景选择方案"), false);
  check("Compact cards fit within 120px for the fixture titles", await outer.locator(".work-tree-row").evaluateAll(rows => rows.every(row => row.getBoundingClientRect().height <= 120)), true);
  await outer.getByRole("button", { name: "大纲", exact: true }).click();
  check("Outline uses the same selected node", await implementation.getAttribute("aria-selected"), "true");
  await page.evaluate(async () => (await import("/12-docs-pages.js")).renderDocsSnapshot(window.__workTreeDocs));
  check("Refresh preserves the chosen view", await outer.locator(".work-tree-workspace").getAttribute("data-view"), "outline");
  await outer.getByRole("button", { name: "目标地图", exact: true }).click();
  await outer.locator(".work-node-reference").click();
  check("Evidence expansion survives selection and refresh", await outer.locator('[data-section="outcome"]').evaluate(node => node.open), true);
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
  check("Optional planning fields start folded", await dialog.locator(".work-plan-advanced").evaluate(node => node.open), false);
  check("Acceptance and verification are optional at planning time", await dialog.locator('[aria-label="完成条件，可稍后补充"], [aria-label="验证方法"]').evaluateAll(nodes => nodes.every(node => !node.required)), true);
  await dialog.getByText("更多设置", { exact: true }).click();
  await dialog.getByLabel("扩展类型", { exact: true }).selectOption("__new__");
  await dialog.getByLabel("类型名称", { exact: true }).fill("性能分析");
  await dialog.getByLabel("类型定义", { exact: true }).fill("测量实际瓶颈");
  await dialog.getByLabel("预期产物", { exact: true }).fill("性能报告");


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
  check("Draft submission does not fabricate criteria or verification", await page.evaluate(() => [window.__workPlanCalls[0].children[0].acceptance, window.__workPlanCalls[0].children[0].verification]), [[], []]);
  check("Custom type carries a definition, not grants", await page.evaluate(() => Object.keys(window.__workPlanCalls[0].children[0].node.custom_type).sort()), ["deliverable", "description", "name"]);
  await page.evaluate(async () => (await import("/12-docs-pages.js")).renderDocsSnapshot(window.__workTreeDocs));
  await outer.locator('[data-work-node-id="R-910/W5"] > .work-tree-row').click();
  check("Draft node has a concrete planning next step", (await outer.locator(".work-tree-inspector").innerText()).includes("先明确完成条件，再开始执行"), true);
  check("Draft node cannot start execution", await outer.locator(".work-node-start").count(), 0);
  check("Draft parent cannot decompose without coverage criteria", await outer.locator(".work-node-split").isDisabled(), true);
  check("Refresh does not replay branch entrance", await outer.locator(".work-node-new").count(), 0);
  for (const theme of ["light", "dark"]) {
    await page.evaluate(value => document.documentElement.dataset.theme = value, theme);
    const palette = await outer.locator(".work-tree-row").first().evaluate(node => {
      node.style.transition = "none";
      const root = getComputedStyle(document.documentElement), row = getComputedStyle(node);
      const probe = document.createElement("span"); probe.style.background = root.getPropertyValue("--panel"); document.body.append(probe);
      const expected = getComputedStyle(probe).backgroundColor; probe.remove();
      return { actual: row.backgroundColor, expected };
    });
    check(`${theme}: cards use the existing panel surface`, palette.actual, palette.expected);
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
    const docs = window.__workTreeDocs;
    const node = docs.work_units.find(unit => unit.unit_id === "R-910/W5");
    node.status = "blocked"; node.blocked_reason = "等待范围决定";
    docs.requirements[0].work_readiness[node.unit_id] = { step: "wait", blockers: [node.blocked_reason], uncovered: [] };
    (await import("/12-docs-pages.js")).renderDocsSnapshot(docs);
  });
  check("Outer summary names the blocked work without its machine ID", await outer.locator(":scope > summary").innerText(), "目标树 · 1 已验证节点 · 1 已替代节点 · 分析偏好读取性能 · 阻塞");
  check("Node counts do not pretend to be project completion fractions", await entry.locator(".work-unit-badge").innerText(), "1 已验证节点 · 阻塞");
  await page.evaluate(async () => {
    (await import("/02-i18n.js")).setLanguagePreference("en");
    (await import("/12-docs-pages.js")).renderDocsSnapshot(window.__workTreeDocs);
  });
  check("Tree headings follow the language setting", (await outer.locator(":scope > summary").innerText()).startsWith("Goal tree"), true);
  check("Mode picker uses the new product name", await page.locator('#profile-select option[value="dev-auto"]').innerText(), "Ongoing development and maintenance");
  check("No browser runtime errors", errors, []);
}
