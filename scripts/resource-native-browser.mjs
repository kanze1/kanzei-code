// Actual production event/render chain; docs_snapshot is always real native data.
// Non-document UI startup commands use the existing isolated preview fixtures.
import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";

export async function measureDocumentEvents({ project, rpc, mark }) {
  const server = await startPreviewServer({ port: 0 });
  const browser = await chromium.launch({ channel: "msedge", headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 960 } });
  const errors = [], samples = [];
  let nativeCalls = 0, logicalProject = null;
  page.on("pageerror", error => errors.push(error.message));
  try {
    await page.exposeFunction("__nativeDocuments", async args => {
      assert.equal(args.projectDir, logicalProject, "Browser proxy only accepts the selected preview project");
      nativeCalls += 1;
      return rpc("docs_snapshot", { projectDir: project });
    });
    await page.goto(`${server.origin}/?scene=docs`);
    await page.waitForFunction(() => window.__kzPreview?.ready);
    logicalProject = await page.evaluate(async () => (await import("/03-shell.js")).currentProject);
    await page.evaluate(async () => {
      const shell = await import("/03-shell.js");
      if (!shell.currentProject) throw new Error("Preview project missing");
      window.__kzPreview.setCommand("docs_snapshot", args => window.__nativeDocuments(args));
      window.__documentPages = await import("/12-docs-pages.js");
      await (await import("/14-docs-actions.js")).refreshDocs({ extras: false, force: true });
    });
    const nativeBefore = nativeCalls;
    for (let index = 0; index < 20; index += 1) {
      const marker = await mark(`browser-tool-event-${index}`);
      const start = performance.now();
      await page.evaluate(async index => {
        const shell = await import("/03-shell.js");
        if (!shell.activeSessionId) throw new Error("Fixture requires a real UI active session identity");
        window.__kzPreview.emit("kz:tool-end", { sessionId: shell.activeSessionId,
          id: `resource-native-${index}`, name: "req", ok: true, outcome: "success",
          preview: "updated: R-001", content: "updated: R-001", durationMs: 1 });
      }, index);
      await page.waitForFunction(marker => window.__documentPages.latestDocsSnapshot?.conventions?.headings?.includes(marker),
        marker, { timeout: 8000 });
      samples.push(performance.now() - start);
    }
    assert.equal(errors.length, 0, errors.join("\n"));
    const eventCalls = nativeCalls - nativeBefore;
    // Each event is waited to completion before the next one. A calibration may
    // also become due during slow machines, so retain actual calls without hiding it.
    assert(eventCalls >= 20 && eventCalls <= 21, `Unexpected native calls during 20 events: ${eventCalls}`);
    return { samples, nativeCalls: eventCalls, errors,
      scope: "Production ESM in headless Edge; injected kz:tool-end(req success) passes through the actual event handler, resource owner and renderDocsSnapshot. The logical preview project's docs_snapshot requests route to the isolated native project; other bootstrap commands are preview fixtures. Isolated convention markers are real file writes. No live agent/tool or GUI user's data is touched." };
  } finally {
    await browser.close(); await server.close();
  }
}
