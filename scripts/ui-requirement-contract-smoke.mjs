/* global window, document */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/requirements-contract";
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1500, height: 1000 } });
const errors = [], checks = [];
const check = (condition, label) => { assert(condition, label); checks.push(label); };
page.on("pageerror", e => errors.push(e.message));
try {
  await page.goto(`${server.origin}/?scene=docs&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async () => {
    const fixtures = await window.__kzPreview.fixtures();
    const docs = fixtures.commands.docs_snapshot();
    const base = { ...docs.requirements[0], priority: "P2", complexity: "", batches: null, execution_model: null, work_units: [] };
    const typed = { ...base, id: "R-900", title: "导出所选评分", status: "draft", nextStatuses: ["todo", "dropped"],
      prior_art: null, blocked: false, block_reasons: [], dependencies: [], dependents: [],
      fields: [["优先级", "P2"], ["进展", "长执行历史应该折叠"], ["observed_head", "abcd1234"]],
      requirement: { format: 2, revision: "spec-current", gaps: ["导出字段待确认"],
        spec: { statement: "收到导出请求时，系统应提供包含所选评分的 CSV 文件。", acceptance: [{ id: "AC-1", text: "选择两条评分并导出，文件包含所选记录及评分。" }],
          source: { reference: ".kanzei/project/requirement-sources/original.md" }, questions: ["导出字段待确认"], links: [] },
        evidence: [{ criterion_id: "AC-1", revision: "spec-old", reference: "docs/evidence/export.md" }] } };
    docs.requirements = [typed, { ...base, id: "R-245", title: "Tool Result Spill 与显式空间整理", status: "todo", prior_art: null,
      fields: [["内容", "完整工具结果应可恢复读取"], ["验收", "sha256 与原始输出一致"], ["边界", "仍被引用的 artifact 不得清理"], ["迁移与回滚", "已有引用仍必须可读"], ["来源", "历史工程记录"], ["进展", "旧执行记录保留"]], requirement: { format: 1 } }];
    docs.archived.req = 0; docs.work_units = [];
    window.__contractDocs = docs;
    window.__kzPreview.setCommand("docs_snapshot", () => structuredClone(docs));
    window.__kzPreview.setCommand("docs_update", args => { window.__contractSave = args; return "saved"; });
    await (await import("/14-docs-actions.js")).refreshDocs();
    (await import("/12-docs-pages.js")).renderDocsSnapshot(docs);
  });
  const entry = page.locator('#documents-req-list .doc-item[data-doc-id="R-900"]');
  await entry.locator(".doc-row").click();
  check(await entry.locator(".requirement-statement").isVisible(), "The statement is visible without expanding supplementary material");
  check((await entry.locator(".requirement-acceptance").innerText()).includes("要求已变更，待复核"), "Old evidence is visibly stale");
  check(await entry.locator("details[open]").count() === 0, "Source and execution history start collapsed");
  check((await entry.innerText()).includes("导出字段待确认"), "Draft questions stay visible");
  await page.screenshot({ path: `${output}/requirement-light.png` });
  await entry.locator(".doc-edit-toggle").click();
  await entry.locator('[data-field=":statement"]').fill("收到导出请求时，系统应生成 UTF-8 CSV 文件。");
  await entry.locator('[data-field=":questions"]').fill("");
  await entry.getByRole("button", { name: "保存修改", exact: true }).click();
  await page.waitForFunction(() => Boolean(window.__contractSave));
  const saved = await page.evaluate(() => window.__contractSave);
  check(saved.requirement.acceptance[0].id === "AC-1", "Editing preserves criterion identity");
  check(saved.requirement.source.reference.endsWith("original.md"), "Editing preserves provenance");
  check(!("observed_head" in saved.fields) && !("验收" in saved.fields), "Editor submits the typed spec without engine metadata or duplicate acceptance");
  check(saved.requirement.questions.length === 0, "Resolved draft questions can be cleared");
  const legacy = page.locator('#documents-req-list .doc-item[data-doc-id="R-245"]');
  await legacy.locator(".doc-row").click();
  check((await legacy.innerText()).includes("仍被引用的 artifact 不得清理"), "Legacy normative boundaries remain visible");
  check((await legacy.innerText()).includes("已有引用仍必须可读"), "Legacy rollback obligations remain visible");
  const draftStart = await page.evaluate(async () => (await import("/26-requirement-research.js")).requirementStart(window.__contractDocs.requirements[0]));
  check(draftStart.executionBatch === false && draftStart.workItemId === null, "Draft completion never starts an implementation batch");
  await page.setViewportSize({ width: 840, height: 900 });
  check(await entry.locator(".requirement-statement").isVisible(), "Requirement text remains accessible at a narrow width");
  check(errors.length === 0, `No browser errors: ${errors.join("; ")}`);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ checks, errors }, null, 2));
  console.log(`Requirement contract browser PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path: `${output}/failure.png` }); throw error;
} finally { await browser.close(); await server.close(); }
