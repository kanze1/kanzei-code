/* global document */
// Real WebView2 clicks, Rust document transitions and on-disk archives.
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { nativeApp, textReply } from "./fixtures/native-app.mjs";

assert(process.platform === "win32" && process.argv[2], "Pass a fresh kzapp.exe");
const output = path.resolve("output/playwright/management-archive/native");
const app = await nativeApp(process.argv[2], output, "归档详情验收", async (_body, res) => textReply(res, "No candidates."));
const checks = [], check = (value, label) => { assert(value, label); checks.push(label); console.log(`PASS ${label}`); };
const { page } = app;
const root = path.join(app.project, ".kanzei/project");
const rows = {
  req: [["R-001", "完成需求", "doing"], ["R-002", "放弃需求", "doing"], ["R-003", "等待验收需求", "doing"], ["R-004", "后台归档需求", "doing"]],
  defect: [["D-001", "修复缺陷", "fixing"], ["D-002", "不修复缺陷", "fixing"]],
};
const refresh = () => page.evaluate(async () => (await import("./14-docs-actions.js")).refreshDocs({ extras: false }));
const open = async (kind, id) => {
  await page.locator(`[data-management-tab="${kind}"]`).click();
  await page.locator(`.management-row[data-work-id="${id}"]`).click();
  await page.locator(".management-detail-nav").waitFor();
};
const choose = label => page.locator(".management-body .doc-detail-actions").getByRole("button", { name: label, exact: true }).click();
try {
  await mkdir(root, { recursive: true });
  for (const [kind, entries] of Object.entries(rows)) await writeFile(path.join(root, kind === "req" ? "requirements.md" : "defects.md"),
    entries.map(([id, title, status]) => `## ${id} ${title} [${status}]\n- 内容: 原生归档操作验收\n- 验收: 状态和详情与实际文件一致\n- 进展: 实现与本地操作已验证，保留原生验收记录\n`).join("\n"));
  await page.locator('[data-work-surface="project"]').click();
  await refresh();
  for (const [kind, id, label, status, reason] of [
    ["req", "R-001", "→ 转 已完成", "done"],
    ["req", "R-002", "→ 转 已放弃", "dropped", "这条需求已经取消"],
    ["defect", "D-001", "→ 转 已修复", "fixed"],
    ["defect", "D-002", "→ 转 不修复", "wontfix", "确认不修复此项"],
  ]) {
    await open(kind, id);
    if (id === "R-001") {
      await choose(label); await page.locator("#confirm-cancel").click();
      check(await page.locator(".management-detail-nav").isVisible(), "Cancelling completion preserves the active detail");
    }
    await choose(label);
    if (reason) { await page.locator("#input-value").fill(reason); await page.locator("#input-ok").click(); }
    else await page.locator("#confirm-ok").click();
    await page.waitForFunction(() => !document.querySelector(".management-detail-nav"), null, { timeout: 5000 });
    check(await page.locator(".management-list").isVisible(), `${id}: successful archival returns to its list`);
    check(await page.locator(`.management-row[data-work-id="${id}"]`).count() === 0, `${id}: archived work is absent from active rows`);
    const archived = await app.invoke("docs_archive_entries", { projectDir: app.project, kind });
    check(archived.some(entry => entry.id === id && entry.status === status), `${id}: the actual archive contains the selected terminal state`);
    await page.locator(".management-body").getByRole("button", { name: /^已归档 ·/ }).click();
    const history = page.locator(".management-archive-row").filter({ hasText: id });
    await history.waitFor(); await history.click();
    await page.locator(".management-detail-nav").waitFor();
    check(await page.locator(".management-detail-nav").isVisible() && await page.locator(".management-body .doc-edit-toggle").count() === 0
      && await page.locator(".management-body .doc-detail-actions button").count() === 0, `${id}: explicitly opening history remains read-only`);
  }
  await open("req", "R-003");
  await choose("→ 转 待外部验收"); await page.locator("#input-value").fill("等待真实设备验收"); await page.locator("#input-ok").click();
  await page.waitForFunction(() => document.querySelector(".management-detail-nav")?.textContent.includes("待外部验收"));
  check(await page.locator(".management-body").getByRole("button", { name: "退回开发", exact: true }).isVisible(), "A nonterminal transition retains the detail and updates its actions");
  await open("req", "R-004");
  await page.locator(".management-body .doc-edit-toggle").click();
  await page.locator('.management-body [data-field=":title"]').fill("正在编辑的草稿");
  await refresh();
  check(await page.locator('.management-body [data-field=":title"]').inputValue() === "正在编辑的草稿", "An unchanged active document retains its editing draft across a refresh");
  await app.invoke("docs_update", { projectDir: app.project, kind: "req", action: "close", id: "R-004", status: "done" });
  await refresh();
  check(await page.locator(".management-detail-nav").count() === 0 && await page.locator(".management-body .editing").count() === 0,
    "Background archival retires the obsolete editor as well as its selected detail");
  await page.locator(".management-body").getByRole("button", { name: /^已归档 ·/ }).click();
  await page.locator(".management-archive-row").filter({ hasText: "R-004" }).waitFor();
  check(true, "Previously loaded archives refresh to include newly archived work");
  check((await readFile(path.join(root, "requirements-archive.md"), "utf8")).includes("R-004"), "Archival is persisted on disk");
  check(app.errors.length === 0, `No native UI or provider errors: ${app.errors.join("; ")}`);
  await page.screenshot({ path: path.join(output, "native-management-archive.png") });
  await writeFile(path.join(output, "acceptance.json"), JSON.stringify({ checks, passed: checks.length, executable: app.exe, project: app.project }, null, 2));
  console.log(`Management archive native PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path: path.join(output, "failure.png") });
  await writeFile(path.join(output, "failure.json"), JSON.stringify({ checks, errors: app.errors, error: String(error), project: app.project }, null, 2));
  throw error;
} finally { await app.close(); }
