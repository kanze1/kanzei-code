// 真实 UI + 模拟 IPC；Rust 测试单独覆盖持久化/记忆写入，不能据此声称真实模型已跑通。
/* global window, innerWidth */
import assert from "node:assert/strict";
import { mkdir, readFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
import { createFixtures } from "./ui-preview/fixtures.mjs";
import { verifyDecisionStability } from "./ui-decision-stability-smoke.mjs";

const root = path.resolve(import.meta.dirname, "..");
// 与真实 Rust 命令输出生成的契约比对，避免两边夹具自说自话。
const contract = JSON.parse(await readFile(path.join(root, "scripts/ipc-contract.json"), "utf8"));
function assertShape(value, shape) {
  if (shape === "nullable") return;
  if (shape === "array") return assert(Array.isArray(value));
  if (typeof shape === "string") return assert.equal(typeof value, shape === "bool" ? "boolean" : shape);
  if (Array.isArray(shape)) return value.forEach((v) => assertShape(v, shape[0]));
  assert.deepEqual(Object.keys(value).sort(), Object.keys(shape).sort());
  for (const key of Object.keys(shape)) assertShape(value[key], shape[key]);
}
const { commands } = createFixtures();
const contractProject = commands.workspace_snapshot().projects[0];
const contractDecision = contractProject.decisions[0];
assertShape(commands.decision_review({ projectDir: contractProject.path, decisionId: contractDecision.id,
  review: { request_id: "contract-review", expected_revision: contractDecision.revision, action: "correct", feedback: "复用已有实现", scope: "project" } }), contract.decision_review);
const deliveredProject = commands.workspace_snapshot().projects[1];
assertShape(commands.work_delivery_accept({ projectDir: deliveredProject.path, unitId: deliveredProject.work_units[0].unit_id, sourceSequence: deliveredProject.work_units[0].source_sequence }), contract.work_delivery_accept);
const trialProject = commands.workspace_snapshot().projects[2];
const output = path.join(root, "output/playwright/decision-console");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
try {
  await verifyDecisionStability(browser, server.origin);
  await page.goto(`${server.origin}/?scene=workspace&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.getByRole("heading", { name: "所有项目", exact: true }).waitFor();
  assert.equal(await page.locator(".workspace-card").count(), 3);
  await page.screenshot({ path: path.join(output, "projects-dark.png") });
  await page.getByRole("button", { name: "决策复核", exact: true }).click();
  await page.waitForFunction(() => document.querySelectorAll(".console-decision:not([hidden])").length === 4);
  await page.locator(".console-decision").first().evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
  await page.screenshot({ path: path.join(output, "decisions-dark.png") });
  // 批量通过两条；通过不能写任何范围的偏好。
  await page.getByRole("checkbox", { name: "选择决策 普通选择需要停下来问用户吗？" }).check();
  await page.getByRole("checkbox", { name: "选择决策 图谱使用现有渲染库吗？" }).check();
  await page.getByRole("button", { name: "通过所选 (2)", exact: true }).click();
  await page.waitForFunction(() => window.__kzPreview.calls.filter((c) => c.cmd === "decision_review").length === 2);
  await page.waitForFunction(() => document.querySelectorAll(".console-decision:not([hidden])").length === 2);
  const accepts = await page.evaluate(() => window.__kzPreview.calls.filter((c) => c.cmd === "decision_review").map((c) => c.args));
  assert(accepts.every((a) => a.review.action === "accept" && a.review.scope === "once" && a.review.feedback === ""));
  // 当前项目是 A，纠正 B 的选择：IPC 必须携带 B，而非当前全局项目。
  const row = page.locator(".console-decision").filter({ hasText: "搜索结果用分页还是连续滚动？" });
  await row.getByRole("button", { name: "纠正决定" }).click();
  await row.getByRole("textbox", { name: "希望如何修改" }).fill("改为虚拟滚动，保留当前位置");
  await row.getByRole("combobox", { name: "纠正适用范围" }).selectOption("project");
  await row.getByRole("button", { name: "保存纠正并排入原对话" }).dblclick();
  await page.waitForFunction(() => window.__kzPreview.calls.filter((c) => c.cmd === "decision_review").length === 3);
  const correction = await page.evaluate(() => window.__kzPreview.calls.filter((c) => c.cmd === "decision_review").at(-1).args);
  assert.notEqual(correction.projectDir, accepts[0].projectDir);
  assert.equal(correction.review.scope, "project");
  assert.equal(correction.review.feedback, "改为虚拟滚动，保留当前位置");
  assert(correction.review.request_id && correction.review.expected_revision > 0);
  await page.getByRole("combobox", { name: "决策状态" }).selectOption("all");
  await row.getByText("修改意见已排入原对话", { exact: true }).waitFor();
  await row.getByText(/项目偏好 · M-preview/).waitFor();
  // 后台验证和用户试用独立显示；纠正的旧证据不可接受。
  await page.getByRole("button", { name: "交付与验证", exact: true }).click();
  assert.equal(await page.getByText("纠正后待重新验证", { exact: true }).count(), 1);
  const deliveries = page.locator(".console-delivery");
  const deliveryFor = (project) => page.locator(`.console-delivery[data-project-path=${JSON.stringify(project.path)}][data-unit-id=${JSON.stringify(project.work_units[0].unit_id)}]`);
  const verifyingDelivery = deliveryFor(contractProject);
  const trialDelivery = deliveryFor(trialProject);
  // The fixture is newest-first, intentionally different from project order. Actions follow identity.
  assert.equal(await deliveries.first().getAttribute("data-unit-id"), trialProject.work_units[0].unit_id);
  await verifyingDelivery.locator("summary").click();
  await verifyingDelivery.getByText("冻结版本: example-source-fingerprint", { exact: true }).waitFor();
  await verifyingDelivery.getByRole("button", { name: "取消验证", exact: true }).click();
  await page.waitForFunction(() => window.__kzPreview.calls.some((c) => c.cmd === "verification_cancel"));
  const cancel = await page.evaluate(() => window.__kzPreview.calls.find((c) => c.cmd === "verification_cancel").args);
  assert.equal(cancel.jobId, "v-preview-1");
  assert.equal(cancel.projectDir, accepts[0].projectDir);
  await trialDelivery.locator("summary").click();
  await trialDelivery.getByRole("button", { name: "我已试用，通过" }).click();
  await trialDelivery.getByText("试用已通过", { exact: true }).waitFor();
  const trialAccepts = await page.evaluate(() => window.__kzPreview.calls.filter((c) => c.cmd === "work_delivery_accept").map((c) => c.args));
  assert.deepEqual(trialAccepts, [{ projectDir: trialProject.path, unitId: trialProject.work_units[0].unit_id, sourceSequence: trialProject.work_units[0].source_sequence }]);
  // 请求失败不抹掉复核草稿，可恢复；用户选择 global 才发送 global。
  await page.getByRole("button", { name: "决策复核", exact: true }).click();
  const third = page.locator(".console-decision").filter({ hasText: "新功能优先复用现有服务吗？" });
  await third.getByRole("button", { name: "纠正决定" }).click();
  await third.getByRole("textbox", { name: "希望如何修改" }).fill("各项目默认优先复用已有服务");
  await third.getByRole("combobox", { name: "纠正适用范围" }).selectOption("global");
  await page.evaluate(() => window.__kzPreview.setCommand("decision_review", () => { throw new Error("模拟连接失败"); }));
  await third.getByRole("button", { name: "保存纠正并排入原对话" }).click();
  await page.waitForFunction(() => document.querySelector(".decision-editing button[type=submit]")?.disabled === false);
  assert.equal(await third.getByRole("textbox", { name: "希望如何修改" }).inputValue(), "各项目默认优先复用已有服务");
  const failed = await page.evaluate(() => window.__kzPreview.calls.filter((c) => c.cmd === "decision_review").at(-1).args);
  assert.equal(failed.review.scope, "global");
  await page.setViewportSize({ width: 420, height: 860 });
  await third.locator("textarea").scrollIntoViewIfNeeded();
  await third.evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
  await page.screenshot({ path: path.join(output, "correction-mobile.png") });
  const overflow = await page.evaluate(() => [...document.querySelectorAll("body *")].filter((el) => el.getBoundingClientRect().right > innerWidth + 1 && el.getBoundingClientRect().width > 0).slice(0, 8).map((el) => ({ tag: el.tagName, id: el.id, cls: typeof el.className === "string" ? el.className : "svg", width: el.getBoundingClientRect().width, right: el.getBoundingClientRect().right })));
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `窄屏不能横向溢出: ${JSON.stringify(overflow)}`);
  await page.goto(`${server.origin}/?scene=workspace&theme=light&lang=en`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.getByRole("heading", { name: "All projects", exact: true }).waitFor();
  await page.getByRole("button", { name: "Decision review", exact: true }).click();
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.locator(".console-decision").first().evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished)));
  await page.screenshot({ path: path.join(output, "decisions-light.png") });
  assert.deepEqual(errors, []);
  console.log("决策控制台浏览器验证通过：跨项目归属、批量复核、重复点击、纠正范围、旧证据失效、交付验收、失败保留草稿、窄屏和英文主题。");
} finally { await browser.close(); await server.close(); }
