// Real UI modules/Edge, deterministic IPC fixtures only. No model/network or native resource reads.
/* global window, document, File, DataTransfer, DragEvent, ClipboardEvent, getComputedStyle */
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

const output = "output/playwright/conversation-media-time";
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: "msedge", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
page.setDefaultTimeout(12000);
const checks = [], errors = [];
const check = (ok, label) => { assert(ok, label); checks.push(label); };
const settle = () => page.evaluate(() => window.__kzPreview.settle());
page.on("pageerror", error => errors.push(error.message));
page.on("console", message => { if (message.text().includes("处理函数抛错")) errors.push(message.text()); });
let owner;
const emit = async (event, data = {}, sessionId = owner.session) => {
  await page.evaluate(({ event, data, sessionId }) => window.__kzPreview.emit(event, { sessionId, ...data }), { event, data, sessionId });
  await settle();
};
const receipt = run => page.locator(`.msg-pane:not(.hidden) .turn-elapsed-receipt[data-run-id="${run}"]`);
const imageMetrics = selector => page.locator(selector).evaluateAll(async images => {
  await Promise.all(images.map(image => image.decode()));
  return images.map(image => { const box = image.getBoundingClientRect(); return {
    natural: [image.naturalWidth, image.naturalHeight], shown: [box.width, box.height], fit: getComputedStyle(image).objectFit,
  }; });
});
let seed = null;
try {
  await page.goto(`${server.origin}/?scene=chat&theme=dark`);
  await page.waitForFunction(() => window.__kzPreview?.ready); await settle();
  owner = await page.evaluate(async () => {
    const shell = await import("/03-shell.js"), fixture = await window.__kzPreview.fixtures();
    for (const process of fixture.state.processes) process.running = false;
    await (await import("/09-sessions.js")).refreshProcesses();
    window.__kzPreview.emit("kz:idle", { sessionId: shell.activeSessionId });
    window.__kzPreview.setCommand("run_prompt", null);
    return { session: shell.activeSessionId, process: shell.activeProcessId, project: shell.currentProject };
  });
  const pngs = await page.evaluate(() => [[480, 160], [40, 120], [24, 16]].map(([width, height]) => {
    const canvas = document.createElement("canvas"); canvas.width = width; canvas.height = height;
    const ctx = canvas.getContext("2d"); ctx.fillStyle = "#138e86"; ctx.fillRect(0, 0, width, height);
    ctx.fillStyle = "#f0c77c"; ctx.fillRect(0, 0, width / 2, height / 2);
    return canvas.toDataURL("image/png").split(",")[1];
  }));
  await page.locator("#attachment-input").setInputFiles([
    { name: "same.png", mimeType: "image/png", buffer: Buffer.from(pngs[0], "base64") },
    { name: "same.png", mimeType: "image/png", buffer: Buffer.from(pngs[1], "base64") },
    { name: "guide.pdf", mimeType: "application/pdf", buffer: Buffer.from("%PDF-1.4\nfixture") },
  ]);
  await page.waitForFunction(() => document.querySelectorAll("#attachments .attachment-chip").length === 3);
  let metrics = await imageMetrics("#attachments .attachment-thumbnail");
  check(metrics.length === 2 && metrics[0].natural[0] === 480 && metrics[1].natural[1] === 120, "Picker images decode to their actual, distinct natural dimensions");
  check(metrics.every(({ natural: [w, h], shown: [sw, sh], fit }) => Math.abs(w / h - sw / sh) < .02 && sw <= 96 && sh <= 64 && fit === "contain"), "Composer thumbnails are bounded and retain portrait/landscape aspect ratios");
  check(await page.locator("#attachments img").evaluateAll(images => images.every(image => {
    const imageBox = image.getBoundingClientRect(), chip = image.closest(".attachment-chip").getBoundingClientRect();
    return imageBox.top >= chip.top && imageBox.bottom <= chip.bottom;
  })), "Thumbnail height is contained by its chip instead of overflowing the composer header");
  check(await page.locator('#attachments [data-kind="pdf"]').count() === 1, "PDF keeps its file type icon instead of an image");
  check(await page.getByRole("button", { name: "预览 same.png", exact: true }).count() === 2 && await page.getByRole("button", { name: "移除附件 same.png", exact: true }).count() === 2, "Duplicate names have accessible previews and separate remove buttons");
  await page.locator("#attachments .attachment-image-preview").first().click();
  check(await page.locator("#viewer-body img").evaluate(img => img.naturalWidth === 480 && img.src.startsWith("data:image/png;base64,")), "Click uses the existing enlarged-image viewer with held bytes, not a path read");
  await page.locator("#viewer-close").click();
  await page.getByRole("button", { name: "移除附件 same.png", exact: true }).first().click();
  check(await page.evaluate(async expected => (await import("/03-shell.js")).attachments[0].data === expected, pngs[1]), "Removing the first duplicate preserves the second image's exact bytes");
  await page.locator("#prompt").fill("图片草稿");
  await page.locator("#new-chat").click(); await settle();
  const other = await page.evaluate(async () => { const s = await import("/03-shell.js"); return { process: s.activeProcessId, session: s.activeSessionId }; });
  check(other.session !== owner.session && await page.locator("#attachments img").count() === 0, "A new conversation does not inherit another conversation's image draft");
  await page.evaluate(async id => (await import("/09-sessions.js")).switchProcess(id), owner.process); await settle();
  check(await page.locator("#prompt").inputValue() === "图片草稿" && (await imageMetrics("#attachments img"))[0].natural[1] === 120, "Switching back restores the correct text and thumbnail from the real draft store");
  const drafts = await page.evaluate(async () => {
    const drafts = await import("/00-composer-drafts.js"); await drafts.flushComposerDrafts();
    return drafts.composerDraftEntries().map(([key, draft]) => ({ key, draft }));
  });
  check(drafts.some(row => row.draft.attachments?.[0]?.data === pngs[1]), "Draft persistence carries the full image bytes, not a thumbnail-only copy");
  seed = { drafts, history: null, traces: [] };
  // Seed backend responses before module boot on reload; production modules remain unchanged.
  await page.route("**/__preview/mock-ipc.js", async route => {
    const response = await route.fetch();
    const boot = `\n;window.__kzPreview.setCommand('composer_drafts_load', ${JSON.stringify(seed.drafts)});`
      + (seed.history ? `\nwindow.__kzPreview.setCommand('conversation_display_get', ${JSON.stringify(seed.history)});\nwindow.__kzPreview.setCommand('conversation_trace_get', ${JSON.stringify(seed.traces)});` : "");
    await route.fulfill({ response, body: (await response.text()) + boot });
  });
  await page.reload(); await page.waitForFunction(() => window.__kzPreview?.ready); await settle();
  check(await page.locator("#prompt").inputValue() === "图片草稿" && (await imageMetrics("#attachments img"))[0].natural[1] === 120, "Reload rehydrates a persisted image draft before rendering its thumbnail");
  await page.evaluate(async () => {
    const f = await window.__kzPreview.fixtures(), s = await import("/03-shell.js");
    for (const p of f.state.processes) p.running = false;
    await (await import("/09-sessions.js")).refreshProcesses();
    window.__kzPreview.emit("kz:idle", { sessionId: s.activeSessionId });
    window.__kzPreview.setCommand("run_prompt", null);
  });
  for (const type of ["paste", "drop"]) {
    await page.locator("#prompt").evaluate((el, { data, type }) => {
      const dt = new DataTransfer();
      dt.items.add(new File([Uint8Array.from(atob(data), char => char.charCodeAt(0))], `${type}.png`, { type: "image/png" }));
      el.dispatchEvent(type === "paste" ? new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: dt }) : new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: dt }));
    }, { data: pngs[2], type });
    await page.waitForFunction(count => document.querySelectorAll("#attachments img").length === count, type === "paste" ? 2 : 3);
  }
  metrics = await imageMetrics("#attachments img");
  check(metrics.length === 3 && metrics.slice(1).every(m => m.shown[0] === 24 && m.shown[1] === 16), "Paste and drop share real thumbnail rendering without upscaling small images");
  await page.screenshot({ path: `${output}/composer-images-dark.png` });
  await page.locator("#send").click(); await settle();
  const sent = await page.evaluate(() => window.__kzPreview.calls.findLast(call => call.cmd === "run_prompt")?.args);
  check(sent?.attachments?.filter(file => file.media_type === "image/png").length === 3 && sent.attachments[0].data === pngs[1], "The real send request retains all original image bytes");
  metrics = await imageMetrics(".msg-pane:not(.hidden) .msg.user .attachment-thumbnail");
  check(metrics.length === 3 && metrics.every(({ natural: [w, h], shown: [sw, sh] }) => sw <= 300 && sh <= 220 && Math.abs(w / h - sw / sh) < .02), "Sent user images render with bounded, unstretched dimensions");
  check(await page.locator("#attachments img").count() === 0, "Sending clears the composer thumbnails with the submitted draft");
  await emit("kz:turn", { step: 1 });
  await emit("kz:text", { text: "不调用工具的答复。" });
  await emit("kz:turn-elapsed", { runId: "plain-run", elapsedMs: 1250, outcome: "completed" });
  await emit("kz:done", { runId: "plain-run", elapsedMs: 1250, steps: 1 });
  await emit("kz:idle");
  check((await receipt("plain-run").textContent()).includes("1.3s") && await page.locator("#status-elapsed").textContent() === "", "A no-tool response keeps backend elapsed time after the activity clock clears");
  await emit("kz:turn-elapsed", { runId: "plain-run", elapsedMs: 99999, outcome: "completed" });
  check(await receipt("plain-run").count() === 1 && await receipt("plain-run").getAttribute("data-elapsed-ms") === "1250", "Duplicate terminal events cannot duplicate or alter an ended run's fixed receipt");
  await emit("kz:turn", { step: 1 });
  await emit("kz:tool-start", { id: "timed-tool", name: "read", input: { path: "README.md" } });
  await emit("kz:tool-end", { id: "timed-tool", name: "read", ok: true, content: "fixture", durationMs: 4500 });
  await emit("kz:text", { text: "已检查文件。" });
  await emit("kz:turn-elapsed", { runId: "tool-run", elapsedMs: 7350, outcome: "completed" });
  await emit("kz:done", { runId: "tool-run", elapsedMs: 7350, steps: 2 }); await emit("kz:idle");
  check((await receipt("tool-run").textContent()).includes("7.3s"), "Tool response shows whole backend duration rather than only the final text/tool segment");
  for (const [runId, event, outcome, elapsedMs] of [["failed-run", "kz:error", "failed", 2100], ["stopped-run", "kz:stopped", "halted", 3200]]) {
    await emit("kz:turn", { step: 1 });
    await emit(event, { message: "fixture failure", terminal: true });
    await emit("kz:turn-elapsed", { runId, outcome, elapsedMs }); await emit("kz:idle");
    check(await receipt(runId).getAttribute("data-outcome") === outcome && await receipt(runId).getAttribute("data-elapsed-ms") === String(elapsedMs), `${outcome} preserves a fixed receipt without a Done event`);
  }
  await emit("kz:turn-elapsed", { runId: "unknown-run", elapsedMs: null });
  check(await receipt("unknown-run").count() === 0, "Missing reliable duration is not fabricated from browser time");
  const peer = await page.evaluate(async ownerProcess => {
    const s = await import("/03-shell.js"); const target = s.processItems.find(item => item.id !== ownerProcess);
    return target ? { process: target.id, session: target.session_id } : null;
  }, owner.process);
  check(Boolean(peer), "A distinct session exists for isolation coverage");
  await page.evaluate(async id => (await import("/09-sessions.js")).switchProcess(id), peer.process); await settle();
  const backgroundSid = owner.session === peer.session ? "media-background" : owner.session;
  await emit("kz:turn-elapsed", { runId: "background-run", elapsedMs: 4400, outcome: "completed" }, backgroundSid);
  check(await page.locator('.msg-pane:not(.hidden) .turn-elapsed-receipt[data-run-id="background-run"]').count() === 0, "A background completion never appears in the active conversation");
  check(await page.locator(`.msg-pane[data-session-id="${backgroundSid}"] .turn-elapsed-receipt[data-run-id="background-run"]`).count() === 1, "Background receipts route to their owning session pane");
  await page.evaluate(async id => (await import("/09-sessions.js")).switchProcess(id), owner.process); await settle();
  check(await receipt("plain-run").count() === 1 && await receipt("background-run").count() === 1, "Returning to the owner preserves its fixed receipts");
  const safe = await page.evaluate(async () => {
    const { attachmentImageSource } = await import("/04-attachment-preview.js");
    const data = btoa('<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>');
    return ["image/svg+xml", "image/png", 'image/png\" onerror=\"alert(1)'].every(media_type => attachmentImageSource({ media_type, data }) === null)
      && attachmentImageSource({ media_type: "image/png", data: "https://example.com/a.png" }) === null;
  });
  check(safe, "SVG, forged raster MIME, attribute injection and remote URL data are refused before DOM insertion");
  seed.history = [{ role: "user", parts: [{ type: "text", text: "恢复图片" }, { type: "image", media_type: "image/png", data: pngs[0] }] }, { role: "assistant", parts: [{ type: "text", text: "已恢复。" }] }];
  seed.traces = [
    { kind: "turn.elapsed", runId: "recovered-run", sessionId: owner.session, elapsedMs: 12345, outcome: "completed", source: "backend_elapsed" },
    { kind: "turn.elapsed", runId: "recovered-old", sessionId: owner.session, elapsedMs: 2500, outcome: "failed", source: "persisted_timestamps" },
    { kind: "turn.elapsed", runId: "no-duration", sessionId: owner.session },
  ];
  seed.drafts = [];
  await page.reload(); await page.waitForFunction(() => window.__kzPreview?.ready); await settle();
  metrics = await imageMetrics(".msg-pane:not(.hidden) .msg.user img");
  check(metrics.length === 1 && metrics[0].natural[0] === 480 && metrics[0].shown[0] <= 300, "Historical recovery retains actual image data and reasonable dimensions");
  check(await receipt("recovered-run").getAttribute("data-elapsed-ms") === "12345" && await receipt("recovered-old").getAttribute("data-source") === "persisted_timestamps", "Reload restores fixed backend receipts and explicitly sourced legacy timestamps");
  await page.evaluate(async traces => {
    const pane = await import("/01-chat-panes.js");
    (await import("/05-turn-receipts.js")).renderRecoveredTurnReceipts(traces, pane.activePane, pane.appendToPane);
  }, seed.traces);
  check(await receipt("recovered-run").count() === 1 && await receipt("no-duration").count() === 0, "Historical replay is idempotent and legacy history without time stays blank");
  await page.locator('.msg-pane:not(.hidden) .attachment-image-preview').click();
  check(await page.locator("#viewer-body img").evaluate(img => img.naturalWidth === 480), "Recovered image can use the same safe enlarged viewer");
  await page.locator("#viewer-close").click();
  await page.evaluate(async () => (await import("/03-shell.js")).applyTheme("light"));
  await page.screenshot({ path: `${output}/history-images-time-light.png` });
  await page.setViewportSize({ width: 800, height: 760 }); await settle();
  await page.screenshot({ path: `${output}/history-images-time-narrow.png` });
  check(errors.length === 0, "No browser/event handler errors: " + errors.join("; "));
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ passed: checks.length, checks, errors }, null, 2));
  console.log(`Conversation media/time PASS: ${checks.length} checks`);
} catch (error) {
  await page.screenshot({ path: `${output}/failure.png` }); console.error({ checks, errors }); throw error;
} finally { await browser.close(); await server.close(); }
