// Behavioral regressions for the 2026-10-16 design audit. Production modules,
// real Edge DOM and explicitly mocked IPC; never touches the user's app data.
import assert from "node:assert/strict";
import { writeFile, mkdir } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const result = { at: new Date().toISOString(), boundary: "Production ESM in Edge; mock IPC; no native data", checks: [], errors: [] };
const check = (label, condition) => { assert.ok(condition, label); result.checks.push(label); };
const drafts = new Map(); let draftFailure = false, draftWriteGate = null, draftReadGate = null;
let closeRegistrationGate = null, closeRegistrationFailure = false;
async function open(scene = "chat") {
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
  page.on("pageerror", error => result.errors.push(String(error)));
  await page.exposeFunction("auditCloseRegister", async () => {
    if (closeRegistrationGate) await closeRegistrationGate;
    if (closeRegistrationFailure) throw new Error("audit close listener unavailable");
  });
  await page.exposeFunction("auditDraftRead", async () => { const snapshot = [...drafts].map(([key, draft]) => ({ key, draft })); if (draftReadGate) await draftReadGate; return snapshot; });
  await page.exposeFunction("auditDraftWrite", async ({ key, draft }) => {
    if (draftWriteGate) await draftWriteGate;
    if (draftFailure) throw new Error("audit disk unavailable");
    if (draft) drafts.set(key, draft); else drafts.delete(key);
  });
  await page.addInitScript(() => {
    let tauri;
    Object.defineProperty(window, "__TAURI__", { configurable: true, get: () => tauri, set: value => {
      const invoke = value.core.invoke;
      value.core.invoke = (command, args) => command === "composer_drafts_load" ? window.auditDraftRead()
        : command === "composer_draft_save" ? window.auditDraftWrite(args) : invoke(command, args);
      const getCurrentWindow = value.window?.getCurrentWindow;
      value.window = { ...value.window, getCurrentWindow: () => ({ ...getCurrentWindow?.(), onCloseRequested: async handler => {
        await window.auditCloseRegister();
        window.auditClose = handler; return () => { window.auditClose = null; };
      } }) };
      tauri = value;
    } });
  });
  await page.goto(`${server.origin}/?scene=${scene}&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  return page;
}
const settle = page => page.evaluate(() => window.__kzPreview.settle());
try {
  let releaseCloseRegistration;
  closeRegistrationGate = new Promise(resolve => { releaseCloseRegistration = resolve; });
  const registering = await open();
  check("UI-03: composer stays inert until native close listener is registered", await registering.locator("#composer").evaluate(el => el.inert));
  await registering.locator("#prompt").evaluate(el => el.focus()); await registering.keyboard.type("cannot enter yet");
  check("UI-03: pending close registration cannot accept input", await registering.locator("#prompt").inputValue() === "");
  releaseCloseRegistration(); closeRegistrationGate = null;
  await registering.waitForFunction(() => !document.querySelector("#composer").inert);
  check("UI-03: native close listener exists before composer becomes editable", await registering.evaluate(() => typeof window.auditClose === "function"));
  await registering.evaluate(async () => {
    const { registerLeaveGuard } = await import("/00-leave-guard.js");
    window.auditWaitingOwner = registerLeaveGuard({ dirty: () => true, flush: async () => {}, error: error => { throw error; }, leave: () => new Promise(resolve => { window.auditCloseDecision = resolve; }) });
    window.auditClose({ preventDefault() {} });
  });
  await registering.waitForFunction(() => typeof window.auditCloseDecision === "function");
  await registering.locator("#prompt").evaluate(el => el.focus()); await registering.keyboard.type("must not arrive during another save");
  check("UI-03: another owner's pending close decision blocks fresh composer input", await registering.locator("#composer").evaluate(el => el.inert) && await registering.locator("#prompt").inputValue() === "");
  await registering.evaluate(() => { window.auditCloseDecision(false); window.auditWaitingOwner.dispose(); });
  await registering.waitForFunction(() => !document.querySelector("#composer").inert);
  check("UI-03: cancelled close restores editing", true);
  await registering.close();
  closeRegistrationFailure = true;
  const unavailable = await open();
  await unavailable.waitForFunction(() => document.querySelector("#composer-draft-status").textContent.includes("audit close listener unavailable"));
  check("UI-03: close registration failure keeps composer blocked with an explicit error", await unavailable.locator("#composer").evaluate(el => el.inert));
  closeRegistrationFailure = false;
  await unavailable.locator("#composer-draft-status button").click();
  await unavailable.waitForFunction(() => !document.querySelector("#composer").inert);
  check("UI-03: explicit retry restores safe editing after registration failure", await unavailable.evaluate(() => typeof window.auditClose === "function"));
  await unavailable.close();
  const settings = await open("settings");
  await settings.locator("#sg-limits > summary").click();
  await settings.evaluate(async () => {
    const preview = window.__kzPreview, baseline = structuredClone(await (await preview.fixtures()).commands.settings_get());
    preview.setCommand("settings_get", () => baseline);
    preview.setCommand("settings_save", args => new Promise(resolve => { window.auditSave = { args, resolve: () => { baseline.limits = { ...args.payload.limits }; resolve(null); } }; }));
  });
  await settings.locator("#set-max-tokens").fill("6000"); await settings.locator("#settings-save").click();
  await settings.waitForFunction(() => window.auditSave);
  await settings.locator("#set-max-tokens").fill("7000"); await settings.evaluate(() => window.auditSave.resolve());
  await settings.waitForFunction(() => !document.querySelector("#settings-save").disabled);
  check("UI-01: later settings draft survives successful older save", await settings.locator("#set-max-tokens").inputValue() === "7000");
  check("UI-01: later settings draft stays dirty", await settings.locator("#settings-dirty").isVisible());
  await settings.evaluate(() => window.__kzPreview.setCommand("settings_get", () => { throw "audit settings unavailable"; }));
  await settings.locator("#settings-discard").click(); await settle(settings);
  check("UI-02: failed hydration disables save and exposes retry", await settings.locator("#settings-save").isDisabled() && await settings.locator("#settings-load-retry").isVisible());
  const denied = await settings.evaluate(async () => {
    const before = window.__kzPreview.calls.filter(row => row.cmd === "settings_save").length;
    document.querySelector("#settings-save").dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await window.__kzPreview.settle();
    return window.__kzPreview.calls.filter(row => row.cmd === "settings_save").length === before;
  });
  check("UI-02: handler also rejects synthetic save before hydration", denied);
  await settings.evaluate(async () => {
    const fixtures = await window.__kzPreview.fixtures(); window.__kzPreview.setCommand("settings_get", fixtures.commands.settings_get);
    window.__kzPreview.setCommand("update_receipt", { expected_build: "audit123", phase: "failed", message: "installer failed" });
  });
  await settings.locator("#settings-load-retry").click(); await settle(settings);
  check("UI-02: retry restores editable settings", !await settings.locator("#settings-save").isDisabled());
  check("OPS-01: stored update failure is visible on settings load", (await settings.locator("#update-receipt").textContent()).includes("installer failed"));
  await settings.evaluate(() => {
    window.__kzPreview.setCommand("update_receipt", { expected_build: "b97f8a2e", phase: "completed", message: "old completed update" });
    window.__kzPreview.setCommand("update_check", { current: "b97f8a2e", latest: "build-f4df62b2", newer: true, status: "update", expected_build: "f4df62b2", url: "https://github.com/kanze1/kanzei-code/releases/download/build-f4df62b2/kanzei-setup.exe" });
    window.__kzPreview.setCommand("update_install", () => new Promise((resolve, reject) => {
      window.auditRejectInstall = () => {
        window.__kzPreview.setCommand("update_receipt", { expected_build: "f4df62b2", phase: "failed", message: "sqlite error: Query returned no rows" });
        reject(new Error("sqlite error: Query returned no rows"));
      };
    }));
  });
  await settings.evaluate(async () => { await (await import("/16-settings.js")).loadSettings({ force: true }); });
  await settle(settings);
  await settings.locator('[data-settings-target="sg-update"]').click();
  check("OPS-01: historical completed receipt is visible before a new attempt", (await settings.locator("#update-receipt").textContent()).includes("old completed update"));
  await settings.locator("#update-check").click(); await settle(settings);
  await settings.locator("#update-install").click();
  await settings.locator("#confirm-ok").click();
  await settings.waitForFunction(() => typeof window.auditRejectInstall === "function");
  check("OPS-01: new attempt hides the previous completed receipt", await settings.locator("#update-receipt").isHidden());
  check("OPS-01: download owns both update controls", await settings.locator("#update-check").isDisabled() && await settings.locator("#update-install").isDisabled());
  await settings.evaluate(() => window.auditRejectInstall()); await settle(settings);
  check("OPS-01: failed attempt refreshes its durable receipt", await settings.locator("#update-receipt").getAttribute("data-state") === "failed" && (await settings.locator("#update-receipt").textContent()).includes("f4df62b2"));
  check("OPS-01: failure remains visible and releases controls for retry", (await settings.locator("#update-result").textContent()).includes("Query returned no rows") && !await settings.locator("#update-check").isDisabled() && !await settings.locator("#update-install").isDisabled());
  await settings.close();

  const chat = await open();
  let releaseDraftWrite;
  draftWriteGate = new Promise(resolve => { releaseDraftWrite = resolve; });
  await chat.locator("#prompt").fill("immediate reload draft");
  const unload = chat.waitForEvent("dialog");
  const reloading = chat.reload().catch(error => String(error));
  const dialog = await unload;
  check("UI-03: immediate reload is protected while draft is pending", dialog.type() === "beforeunload");
  await dialog.dismiss(); await reloading;
  check("UI-03: cancelled reload retains immediate input", await chat.locator("#prompt").inputValue() === "immediate reload draft");
  await chat.waitForFunction(() => typeof window.auditClose === 'function');
  await chat.evaluate(() => { window.auditCloseFinished = false; window.auditClosePrevented = false;
    window.auditClose({ preventDefault() { window.auditClosePrevented = true; } }).then(() => { window.auditCloseFinished = true; }); });
  check("UI-03: native close awaits in-flight persistence", !await chat.evaluate(() => window.auditCloseFinished));
  releaseDraftWrite(); draftWriteGate = null;
  await chat.waitForFunction(() => window.auditCloseFinished);
  check("UI-03: native close permits leaving after persistence", !await chat.evaluate(() => window.auditClosePrevented));
  await chat.locator("#prompt").fill("audit persistent draft");
  await chat.evaluate(async () => {
    const shell = await import("/03-shell.js"); shell.setAttachments([{ file_name: "a.png", media_type: "image/png", data: "YWJj" }]);
    (await import("/08-compose-runtime.js")).renderAttachments();
    await (await import("/00-composer-drafts.js")).flushComposerDrafts();
  });
  await chat.reload(); await chat.waitForFunction(() => window.__kzPreview?.ready);
  check("UI-03: native text recovers after reload", await chat.locator("#prompt").inputValue() === "audit persistent draft");
  check("UI-03: native attachment recovers after reload", (await chat.locator("#attachments").textContent()).includes("a.png"));
  const ack = await chat.evaluate(async () => {
    const shell = await import("/03-shell.js"), workspaces = await import("/03-workspaces.js");
    const sent = { text: document.querySelector("#prompt").value, attachments: [...shell.attachments] };
    document.querySelector("#prompt").value = "later draft"; document.querySelector("#prompt").dispatchEvent(new Event("input", { bubbles: true }));
    workspaces.acknowledge_composer_draft(shell.currentProject, shell.activeProcessId, sent);
    return document.querySelector("#prompt").value;
  });
  check("UI-03: acknowledgement cannot clear a newer draft", ack === "later draft");
  draftFailure = true;
  await chat.locator("#prompt").fill("unsaved during disk failure");
  await chat.evaluate(async () => (await import("/00-composer-drafts.js")).flushComposerDrafts());
  check("UI-03: failed durable save stays explicit and keeps text", await chat.locator("#composer-draft-status").isVisible() && await chat.locator("#prompt").inputValue() === "unsaved during disk failure");
  check("UI-03: failed persistence prevents native close", await chat.evaluate(async () => { let prevented = false; await window.auditClose({ preventDefault() { prevented = true; } }); return prevented; }));
  draftFailure = false; await chat.locator("#composer-draft-status button").click();
  await chat.waitForFunction(() => document.querySelector("#composer-draft-status").hidden);
  check("UI-03: user retry persists failed draft", [...drafts.values()].some(row => row.text === "unsaved during disk failure"));
  await chat.locator('[data-work-surface="project"]').click(); await chat.locator(".management-tools").waitFor();
  const dedup = await chat.evaluate(async () => {
    const { readProjectDocuments, invalidateProjectDocuments } = await import("/01-project-resources.js"), shell = await import("/03-shell.js");
    const count = () => window.__kzPreview.calls.filter(row => row.cmd === "docs_snapshot").length;
    invalidateProjectDocuments(shell.currentProject);
    const before = count(); await Promise.all([readProjectDocuments(shell.currentProject), readProjectDocuments(shell.currentProject), readProjectDocuments(shell.currentProject)]);
    return count() - before;
  });
  check("UI-10: three concurrent readers share one docs request", dedup === 1);
  await chat.locator(".management-tools input").fill("R-");
  await chat.evaluate(async () => (await import("/02-i18n.js")).setLanguagePreference("en"));
  await settle(chat);
  check("UI-06: management search and actions switch language", (await chat.locator(".management-tools").textContent()).includes("Refresh") && !(await chat.locator(".management-tools input").getAttribute("placeholder")).includes("搜索"));
  check("UI-06: language change preserves query and focus", await chat.locator(".management-tools input").inputValue() === "R-" && await chat.locator(".management-tools input").evaluate(el => el === document.activeElement));
  await chat.evaluate(async () => (await import("/02-i18n.js")).setLanguagePreference("zh"));
  await chat.locator(".management-work-views").getByRole("button", { name: "列表", exact: true }).click();
  await chat.locator(".management-row").first().click(); await chat.locator(".management-body .doc-edit-toggle").click();
  await chat.locator('.management-body [data-field=":title"]').fill("language switch keeps this draft");
  await chat.evaluate(async () => (await import("/02-i18n.js")).setLanguagePreference("en")); await settle(chat);
  check("UI-06: editing document actions also switch language", (await chat.locator(".management-body .doc-edit-actions").textContent()).includes("Save"));
  check("UI-06: language change retains the document draft and field focus", await chat.locator('.management-body [data-field=":title"]').inputValue() === "language switch keeps this draft" && await chat.locator('.management-body [data-field=":title"]').evaluate(el => el === document.activeElement));
  await chat.locator(".management-body .doc-edit-toggle").click();
  await chat.evaluate(async () => (await import("/02-i18n.js")).setLanguagePreference("zh"));
  await chat.evaluate(() => window.__kzPreview.setCommand("architecture_snapshot", () => new Promise(resolve => { window.auditMap = resolve; })));
  await chat.locator('[data-management-tab="map"]').click(); await chat.waitForFunction(() => window.auditMap);
  await chat.locator('[data-management-tab="req"]').click(); await chat.locator('[data-management-tab="map"]').click();
  await chat.evaluate(() => window.auditMap({ crates: { members: [{ name: "audit-A" }, { name: "audit-B" }], edges: [{ from: "audit-A", to: "audit-B" }] } }));
  await chat.waitForFunction(() => document.querySelectorAll(".management-map-node").length === 2);
  check("UI-04: map late result renders into current page", true);
  await chat.locator('[data-management-tab="req"]').click();
  await chat.evaluate(async () => {
    const shell = await import("/03-shell.js"), docs = (await import("/01-project-resources.js")).projectDocuments(shell.currentProject).data;
    document.dispatchEvent(new CustomEvent("kz:management-snapshot", { detail: { project: shell.currentProject, snapshot: { ...docs, archived: { req: 1, defect: 0 } } } }));
    window.__kzPreview.setCommand("docs_archive_entries", () => new Promise(resolve => { window.auditArchive = resolve; }));
  });
  await chat.getByRole("button", { name: "已归档 · 1", exact: true }).click(); await chat.waitForFunction(() => window.auditArchive);
  await chat.locator('[data-management-tab="defect"]').click();
  await chat.evaluate(() => window.auditArchive([{ id: "R-AUDIT", title: "late archive", closed: true }])); await settle(chat);
  check("UI-04: requirements archive never appears in defects", await chat.locator(".management-archive").count() === 0);
  await chat.locator('[data-management-tab="req"]').click();
  check("UI-04: cached archive appears on its own tab", (await chat.locator(".management-archive").textContent()).includes("R-AUDIT"));
  await chat.evaluate(() => window.__kzPreview.setCommand("docs_snapshot", () => { throw "audit docs unavailable"; }));
  await chat.getByRole("button", { name: "刷新", exact: true }).click(); await settle(chat);
  check("UI-05: stale documents show explicit failure", (await chat.locator(".management-error").allTextContents()).join(" ").includes("audit docs unavailable"));
  await chat.evaluate(async () => { const fixtures = await window.__kzPreview.fixtures(); window.__kzPreview.setCommand("docs_snapshot", fixtures.commands.docs_snapshot); });
  await chat.getByRole("button", { name: "刷新", exact: true }).click(); await settle(chat);
  check("UI-05: successful retry clears stale failure", await chat.locator(".management-error").count() === 0);
  const ime = await chat.evaluate(async () => {
    const shell = await import("/03-shell.js"), team = await import("/27-agent-team.js");
    const host = document.createElement("div"); document.body.append(host);
    team.mountAgentControls(host, { key: "audit-ime", description: "audit", agent: "audit", managed: { id: "audit-job", project_dir: shell.currentProject, process_id: shell.activeProcessId, owner: shell.activeSessionId, state: "done", role: "research", messages: [] } });
    const input = host.querySelector("textarea"); input.value = "中文输入";
    window.__kzPreview.setCommand("agent_team_command", () => ({ message_id: "audit" }));
    const count = () => window.__kzPreview.calls.filter(row => row.cmd === "agent_team_command" && row.args.input.action === "message").length;
    const before = count();
    for (const init of [{ keyCode: 229 }, { isComposing: true }, { shiftKey: true }]) input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true, ...init }));
    await window.__kzPreview.settle(); const composing = count() - before;
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); await window.__kzPreview.settle();
    host.remove(); return { composing, normal: count() - before };
  });
  check("UI-07: composition/229/Shift+Enter do not send", ime.composing === 0);
  check("UI-07: ordinary Enter still sends", ime.normal === 1);
  await chat.evaluate(async () => {
    document.querySelector('.activity-item[data-view="settings"]')?.remove(); (await import("/03-shell.js")).navigate_view("settings");
  });
  check("UI-09: routing does not require retired navigation DOM", await chat.locator("body").getAttribute("data-view") === "settings");
  await chat.evaluate(async () => {
    const fixture = await window.__kzPreview.fixtures(), built = fixture.state.skills.find(skill => skill.name === 'pdf');
    fixture.state.skills.push({ ...built, id: 'codex:pdf', source: 'codex', enabled: true });
    (await import('/03-shell.js')).navigate_view('skills');
  }); await settle(chat);
  await chat.locator('[data-skill-id="codex:pdf"] .skill-library-details').click(); await settle(chat);
  check("TOOLS-09/10: same-name skills keep a single source-specific selection", await chat.locator('.skill-library-row.selected').count() === 1 && (await chat.locator('#skills-editor-source').textContent()).includes('$codex:pdf'));
  await chat.locator('[data-skill-id="codex:pdf"] [role="switch"]').uncheck(); await settle(chat);
  check("TOOLS-09/10: toggle addresses only the chosen source identity", await chat.evaluate(() => window.__kzPreview.calls.filter(row => row.cmd === 'skills_set_enabled').at(-1).args.name === 'codex:pdf') && await chat.locator('[data-skill-id="builtin:pdf"] [role="switch"]').isChecked());
  await chat.close();
  let releaseDraftRead;
  draftReadGate = new Promise(resolve => { releaseDraftRead = resolve; });
  const late = await open();
  await late.evaluate(async () => {
    const shell = await import('/03-shell.js');
    document.querySelector('#prompt').value = 'already accepted input';
    (await import('/03-workspaces.js')).acknowledge_composer_draft(shell.currentProject, shell.activeProcessId, { text: 'already accepted input', attachments: [...shell.attachments] });
  });
  releaseDraftRead(); draftReadGate = null;
  await late.evaluate(async () => { window.auditDraftStatus = (await import('/00-composer-drafts.js')).composerDraftStatus; });
  await late.waitForFunction(() => window.auditDraftStatus.loaded);
  check("UI-03: late recovery cannot resurrect an acknowledged draft", await late.locator('#prompt').inputValue() === '');
  await late.close();
  const empty = await open();
  await empty.evaluate(async () => {
    window.__kzPreview.setCommand("docs_snapshot", () => { throw "audit initial docs failure"; });
    const shell = await import("/03-shell.js");
    (await import("/01-project-resources.js")).invalidateProjectDocuments(shell.currentProject);
  });
  await empty.locator('[data-work-surface="project"]').click(); await empty.locator(".management-tools").waitFor(); await settle(empty);
  check("UI-05: initial read failure is not perpetual loading or empty success", (await empty.locator(".management-error").allTextContents()).join(" ").includes("audit initial docs failure") && !(await empty.locator(".management-list").textContent()).includes("正在读取"));
  await empty.close();
  const polling = await open(); await polling.locator('[data-work-surface="project"]').click(); await polling.locator(".management-tools").waitFor(); await settle(polling);
  await polling.evaluate(() => { window.auditPolling = { at: performance.now(), count: window.__kzPreview.calls.length }; });
  await polling.waitForFunction(() => performance.now() - window.auditPolling.at >= 17000, null, { timeout: 20000 });
  result.idlePolls = await polling.evaluate(() => window.__kzPreview.calls.slice(window.auditPolling.count).reduce((counts, row) => { const key = row.cmd + (row.cmd === 'agent_team_command' ? ':' + row.args.input.action : ''); counts[key] = (counts[key] || 0) + 1; return counts; }, {}));
  check("UI-10: management idle has no overview or agent-list polling", !result.idlePolls.workspace_overview && !result.idlePolls['agent_team_command:list']);
  check("UI-10: questions keep one calibration stream", result.idlePolls.softwire_questions <= 2);
  await polling.close();
  check("No uncaught browser errors", result.errors.length === 0);
} catch (error) { result.failure = String(error.stack || error); process.exitCode = 1; }
finally {
  await browser.close(); await server.close();
  await mkdir(new URL("../output/design-audit-20261016/", import.meta.url), { recursive: true });
  await writeFile(new URL("../output/design-audit-20261016/ui-repair-regression-result.json", import.meta.url), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
}
