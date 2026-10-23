/* Production UI with isolated IPC. No writes to the user's requirements or work graph. */
/* global window, document, innerWidth */
import assert from "node:assert/strict";
import test from "node:test";
import { mkdir, writeFile, readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
import { buildWorkMap } from "../crates/kanzei-app/ui/26-work-map-model.js";

const unit = (n, kind, parent, status, objective, dependencies = []) => ({
  unit_id: `R-910/W${n}`, requirement_id: "R-910", objective, status, dependencies,
  node: { kind, parent_id: parent, covers: parent ? ["可以纠正偏好"] : [], questions: [], explanation: "按偏好纠正场景分解", source_refs: ["design.md"] },
  acceptance: ["可以纠正偏好"], verification: ["运行纠正场景"],
  evidence: status === "done" ? [{ criterion: "可以纠正偏好", evidence_refs: ["evidence/preferences.json"] }] : [],
});
const units = [
  unit(1, "goal", null, "ready", "开发可适配的 agent"),
  unit(2, "research", "R-910/W1", "done", "研究偏好保存和纠正"),
  unit(3, "implementation", "R-910/W1", "active", "实现偏好记忆", ["R-910/W2"]),
  unit(4, "verification", "R-910/W3", "blocked", "验证纠正行为", ["R-910/W2", "R-911/W1", "R-800/W1"]),
  unit(5, "defect", "R-910/W1", "superseded", "已替代的旧方案"),
];
units[4].terminal_reason = "旧存储方案退役，不算验收通过";
const readiness = {
  "R-910/W1": { step: "wait", blockers: ["子节点未结束: R-910/W3"], uncovered: [] },
  "R-910/W2": { step: "finished", blockers: [], uncovered: [] },
  "R-910/W3": { step: "execute", blockers: [], uncovered: [] },
  "R-910/W4": { step: "wait", blockers: ["等待独立验证环境"], uncovered: ["可以纠正偏好"] },
  "R-910/W5": { step: "finished", blockers: [], uncovered: [] },
};
const entries = [{ id: "R-910", title: "个人适配 agent", status: "doing", closed: false, dependencies: [], work_units: units, work_readiness: readiness },
  { id: "R-911", title: "独立验证环境", status: "done", closed: true, dependencies: [], work_units: [{ unit_id: "R-911/W1", requirement_id: "R-911", objective: "准备验证环境", status: "done", node: { kind: "verification" }, dependencies: [] }] }];
const snapshot = { requirements: entries, defects: [], work_units: [...units, ...entries[1].work_units] };

test("Map preserves ownership, branching, shared results and unavailable references", () => {
  const before = JSON.stringify(snapshot);
  const graph = buildWorkMap([entries[0]], snapshot);
  assert.equal(graph.nodes.size, 9);
  assert.equal(graph.nodes.get("R-911/W1").context, true);
  assert.equal(graph.nodes.get("R-800/W1").status, "unknown");
  assert.equal(graph.nodes.get("R-910/W5").status, "superseded");
  assert.equal(graph.nodes.get("R-910/W1").readiness.step, "wait");
  assert.equal(graph.edges.filter(edge => edge.kind === "dependency" && edge.from === "R-910/W2").length, 2);
  assert.equal(graph.edges.filter(edge => edge.kind === "parent" && edge.to === "R-910/W4")[0].from, "R-910/W3");
  assert.equal(graph.positions.get("R-910/W2").x, graph.positions.get("R-910/W3").x);
  assert.notEqual(graph.positions.get("R-910/W2").y, graph.positions.get("R-910/W3").y);
  assert.equal(graph.children.get("R-910/W1").length, 3);
  assert.equal(JSON.stringify(snapshot), before, "Projection must not mutate the execution contract");
});

test("Legacy flat requirements get no invented sequence edges; invalid cycles remain visible", () => {
  const flat = buildWorkMap([{ id: "R-1", title: "A" }, { id: "R-2", title: "B" }]);
  assert.equal(flat.nodes.size, 2); assert.equal(flat.edges.length, 0);
  const corrupt = units.slice(0, 2).map(item => structuredClone(item));
  corrupt[0].node.parent_id = corrupt[1].unit_id;
  const graph = buildWorkMap([{ ...entries[0], work_units: corrupt }]);
  assert.equal(graph.invalid.size, 2);
  assert.equal(graph.positions.size, 3);
  assert.equal(graph.edges.filter(edge => edge.kind === "parent").length, 2);
});

test("Production management goal map: relationships, navigation, refresh and responsive rendering", async () => {
  const output = "output/playwright/work-map";
  await mkdir(output, { recursive: true });
  const sourceFiles = ["26-work-map-model.js", "26-work-map.js", "work-map.css", "30-management.js"];
  const hashes = Object.fromEntries(await Promise.all(sourceFiles.map(async file => [file, createHash("sha256").update(await readFile(`crates/kanzei-app/ui/${file}`)).digest("hex")])));
  const checks = [], errors = [];
  const check = (label, actual, expected = true) => { assert.deepEqual(actual, expected, label); checks.push(label); };
  const server = await startPreviewServer({ port: 0 });
  let browser, page, passed = false;
  try {
    browser = await chromium.launch({ channel: "msedge", headless: true });
    page = await browser.newPage({ viewport: { width: 1500, height: 1100 } });
    page.setDefaultTimeout(15000);
    page.on("pageerror", error => errors.push(error.message));
    await page.goto(`${server.origin}/?scene=chat&theme=light`);
    await page.waitForFunction(() => window.__kzPreview?.ready);
    await page.evaluate(async ({ entries, snapshot }) => {
      const fixture = await window.__kzPreview.fixtures();
      const docs = fixture.commands.docs_snapshot();
      const base = docs.requirements[0];
      docs.requirements = entries.map(entry => ({ ...base, ...entry, blocked: false, block_reasons: [], dependents: [], prior_art: null, requirement: { format: 1 }, fields: [["目标", entry.title]] }));
      docs.defects = []; docs.archived.req = 0; docs.work_units = snapshot.work_units;
      window.__workMapDocs = docs;
      window.__kzPreview.setCommand("docs_snapshot", () => structuredClone(window.__workMapDocs));
      const { currentProject } = await import("/03-shell.js");
      await (await import("/01-project-resources.js")).readProjectDocuments(currentProject, { force: true });
      await (await import("/12-workbench.js")).openProjectSpace(currentProject, "project");
    }, { entries, snapshot });
    await page.locator('[data-management-work-view="map"]').click();
    const map = page.locator(".management-body .goal-map");
    const card = id => map.locator(`[data-work-map-node="${id}"]`);
    await card("R-910/W3").waitFor();
    await page.waitForFunction(() => getComputedStyle(document.querySelector(".goal-map-node")).position === "absolute");
    check("Each work node and referenced context is shown exactly once", await map.locator(".goal-map-node").count(), 9);
    check("Shared research has two consuming branches", await map.locator('path[data-relation="dependency"][data-from="R-910/W2"]').count(), 2);
    check("Multi-level decomposition is separate from cross-branch links", await map.locator('path[data-relation="parent"][data-from="R-910/W3"][data-to="R-910/W4"]').count(), 1);
    check("Parallel siblings are not arranged as sequential steps", await card("R-910/W2").evaluate((node, sibling) => node.style.left === document.querySelector(`[data-work-map-node="${sibling}"]`).style.left, "R-910/W3"));
    await card("R-910/W1").click();
    check("The map itself distinguishes a waiting parent from executable work", (await card("R-910/W1").innerText()).includes("等待前提满足"));
    check("Parent waiting reason uses backend readiness", (await map.locator(".goal-map-inspector").innerText()).includes("子节点未结束: R-910/W3"));
    await card("R-910/W2").click();
    check("Accepted research exposes its actual evidence", (await map.locator(".goal-map-inspector").innerText()).includes("evidence/preferences.json"));
    await card("R-910/W4").click();
    check("A blocked leaf exposes the backend reason", (await map.locator(".goal-map-inspector").innerText()).includes("等待独立验证环境"));
    await map.locator(".goal-map-inspector").getByRole("button", { name: "R-911/W1 · 准备验证环境", exact: true }).click();
    check("A cross-Outcome dependency navigates to one shared result", await card("R-911/W1").getAttribute("aria-pressed"), "true");
    await card("R-800/W1").click();
    check("Unavailable references do not pretend to be completed", (await map.locator(".goal-map-inspector").innerText()).includes("不据此推断它已完成或被阻塞"));
    await card("R-910/W5").click();
    check("Superseded work retains its reason, not a success badge", (await map.locator(".goal-map-inspector").innerText()).includes("旧存储方案退役，不算验收通过"));
    await card("R-910/W3").focus(); await page.keyboard.press("Enter");
    check("Nodes can be selected with the keyboard", await card("R-910/W3").getAttribute("aria-pressed"), "true");
    await page.locator('.management-tools input[type="search"]').fill("验证纠正行为");
    check("Searching a descendant retains its ownership and prerequisite context", await card("R-910/W4").count(), 1);
    await page.locator('.management-tools input[type="search"]').fill("");
    await page.evaluate(async () => {
      window.__workMapDocs.requirements[0].work_readiness["R-910/W3"].blockers = ["刷新后的实际阻塞"];
      const project = (await import("/03-shell.js")).currentProject;
      await (await import("/01-project-resources.js")).readProjectDocuments(project, { force: true });
    });
    check("Refresh preserves node selection", await card("R-910/W3").getAttribute("aria-pressed"), "true");
    check("Refresh updates the selected node's details", (await map.locator(".goal-map-inspector").innerText()).includes("刷新后的实际阻塞"));
    await page.locator('[data-management-work-view="list"]').click();
    check("Legacy list remains available", await page.locator(".management-row").count(), 1);
    await page.locator('[data-management-work-view="map"]').click();
    await card("R-910/W3").click();
    await map.locator(".goal-map-inspector").getByRole("button", { name: "打开需求详情", exact: true }).click();
    await page.locator(".management-body > .doc-detail").waitFor();
    check("Map links open the real requirement document", await page.locator(".management-body > .doc-detail").count(), 1);
    await page.locator('[data-management-tab="req"]').click();
    check("Returning to the map preserves its selected node", await card("R-910/W3").getAttribute("aria-pressed"), "true");
    await page.screenshot({ path: `${output}/map-wide-light.png` });
    await page.evaluate(async () => {
      (await import("/02-i18n.js")).setLanguagePreference("en");
    });
    check("Map labels follow language changes", await map.getAttribute("aria-label"), "Goal map");
    for (const width of [840, 390]) {
      await page.setViewportSize({ width, height: 1000 });
      check(`No page overflow at ${width}px`, await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
      check(`Map scroll stays bounded at ${width}px`, await map.locator(".goal-map-scroll").evaluate(node => node.clientWidth <= node.parentElement.clientWidth));
      await page.screenshot({ path: `${output}/map-${width}-light.png` });
    }
    await page.setViewportSize({ width: 1500, height: 1100 });
    await page.evaluate(() => { document.documentElement.dataset.theme = "dark"; document.body.dataset.theme = "dark"; });
    await page.screenshot({ path: `${output}/map-wide-dark.png` });
    check("No browser runtime errors", errors, []);
    passed = true;
  } catch (error) {
    await page?.screenshot({ path: `${output}/failure.png` }).catch(() => {});
    throw error;
  } finally {
    await writeFile(`${output}/acceptance.json`, JSON.stringify({ passed, boundary: "Production ESM with isolated IPC, not native backend or installed-version acceptance", source_sha256: hashes, checks, errors }, null, 2));
    await browser?.close(); await server.close();
  }
});
