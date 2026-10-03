// Actual Edge and production ESM; isolated IPC verifies exact question reply values.
/* global window */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = path.resolve(process.env.KANZEI_CHOICE_OUTPUT || "output/audit-WB1/view-review/choice");
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
page.setDefaultTimeout(10000);
const checks = [], errors = [];
page.on("pageerror", error => errors.push(error.message));
const check = (value, name) => { assert(value, name); checks.push(name); };
const settle = () => page.evaluate(() => window.__kzPreview.settle());
try {
  await page.goto(`${server.origin}/?scene=workspace&theme=light`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.locator(".workspace-card-open").first().click(); await settle();
  await page.locator(".workbench-project-link.active").click(); await settle();
  await page.locator('[data-work-surface="project"]').click(); await settle();
  const identity = await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures();
    const snapshot = f.commands.workspace_snapshot(), project = snapshot.projects[0];
    const question = { id: 9701, projectDir: project.path, sessionId: project.lines[0].session_id,
      revision: "choice-v1", question: "WB1 exact label question", background: true,
      options: [{ label: "回答", note: "first original value" }, { label: "取消", note: "second original value" }] };
    window.__choice = { snapshot, questions: [question], replies: [] };
    window.__kzPreview.setCommand("workspace_snapshot", () => structuredClone(snapshot));
    window.__kzPreview.setCommand("softwire_questions", () => structuredClone(window.__choice.questions));
    window.__kzPreview.setCommand("softwire_answer_question", args => {
      window.__choice.replies.push(structuredClone(args));
      window.__choice.questions = [];
      return { ...args, status: "delivered" };
    });
    const i18n = await import("/02-i18n.js");
    i18n.setLanguagePreference("en", { persist: true, rerender: true });
    return { project: project.path, session: question.sessionId };
  });
  await page.locator("#sw-refresh").click(); await settle();
  check(await page.evaluate(async () => (await import("/02-i18n.js")).languageIsEnglish()), "Fixture actually runs in English");
  await page.locator('.sw-tabs [data-tab="inbox"]').click(); await settle();
  await page.locator(".sw-inbox-row").filter({ hasText: "WB1 exact label question" }).click();
  await page.locator(".sw-reply-explain").click();
  await page.locator("#prompt").fill("keep my explanation");
  const options = page.locator(".sw-choices [data-reply-choice]");
  await options.nth(0).click();
  await options.nth(1).click();
  const draft = await page.locator("#prompt").inputValue();
  await writeFile(path.join(output, "observed-draft.json"), JSON.stringify({ draft }, null, 2));
  check(draft === "keep my explanation\n取消", "English single-choice replacement preserves explanation and only the latest original label");
  check(await options.nth(0).getAttribute("data-reply-choice") === "回答"
    && await options.nth(1).getAttribute("data-reply-choice") === "取消", "Model choices expose canonical original values in data attributes");
  check(await options.nth(0).getAttribute("aria-label") === "回答"
    && await options.nth(1).getAttribute("aria-label") === "取消", "Model options remain source-faithful in English UI");
  check(await options.nth(1).getAttribute("aria-pressed") === "true"
    && await options.nth(0).getAttribute("aria-pressed") === "false", "Single-choice selected state follows canonical draft values");
  await page.locator("#send").click(); await settle();
  const replies = await page.evaluate(() => window.__choice.replies);
  check(replies.length === 1 && replies[0].reply === draft && replies[0].projectDir === identity.project
    && replies[0].sessionId === identity.session && replies[0].id === 9701
    && replies[0].expectedRevision === "choice-v1", "Actual IPC sends the original option and explanation once to the original question");
  // Render the same production view directly to isolate its built-in action contract.
  const builtin = await page.evaluate(async () => {
    const { renderInteraction } = await import("/25-softwire-view.js");
    const root = document.createElement("section"), selected = [];
    renderInteraction(root, { kind: "decision", project: "fixture", body: "review", choices: [] },
      { back() {}, refresh() {}, explain() {}, choice(value) { selected.push(value); } });
    const choices = [...root.querySelectorAll("[data-reply-choice]")];
    choices[0].click();
    return { labels: choices.map(button => button.getAttribute("aria-label")),
      values: choices.map(button => button.dataset.replyChoice), selected };
  });
  check(builtin.labels[0] === "Accept this decision" && builtin.values[0] === "Accept this decision"
    && builtin.selected[0] === "Accept this decision", "Built-in review actions still display and return English labels");
  check(errors.length === 0, `No browser runtime errors: ${errors.join("; ")}`);
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ status: "passed", checks, errors,
    boundary: "Real Edge, production view/controller, isolated IPC; no real backend or model." }, null, 2));
  console.log(`Softwire choice PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path: path.join(output, "failure.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ status: "failed", checks, errors,
    error: String(error.stack || error) }, null, 2));
  throw error;
} finally { await browser.close(); await server.close(); }
