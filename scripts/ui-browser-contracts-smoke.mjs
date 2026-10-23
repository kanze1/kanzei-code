// One browser and preview server; each domain owns fresh pages and its assertions.
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import test, { before, after } from "node:test";
import { chromium } from "playwright-core";
import { startPreviewServer } from "./ui-preview/server.mjs";
import workTree from "./tests/browser-contracts/work-tree.mjs";
import resources from "./tests/browser-contracts/resource.mjs";
import markdown from "./tests/browser-contracts/markdown.mjs";
import i18n from "./tests/browser-contracts/i18n.mjs";
import toolSummary from "./tests/browser-contracts/tool-summary.mjs";
import structured from "./tests/browser-contracts/structured-render.mjs";
import delivery from "./tests/browser-contracts/delivery.mjs";
import sidePolicy from "./tests/browser-contracts/side-policy.mjs";
import experience from "./tests/browser-contracts/experience.mjs";

const output = path.resolve("output/browser-contracts");
const scenarios = { workTree, resources, markdown, i18n, toolSummary, structured, delivery, sidePolicy, experience };
const results = [];
let server, browser;
before(async () => {
  await mkdir(output, { recursive: true });
  server = await startPreviewServer({ port: 0 });
  browser = await chromium.launch({ channel: "msedge", headless: true });
});
after(async () => {
  try {
    await writeFile(path.join(output, "acceptance.json"), JSON.stringify({
      boundary: "Production ESM in Chromium with isolated IPC; no native backend acceptance",
      all_pass: results.length === Object.keys(scenarios).length && results.every(result => result.passed),
      results,
    }, null, 2));
  } finally {
    try { await browser?.close(); }
    finally { await server?.close(); }
  }
});

for (const [name, run] of Object.entries(scenarios)) {
  test(name, async t => {
    const pages = [], checks = [];
    const directory = path.join(output, name);
    await mkdir(directory, { recursive: true });
    const result = { name, completed: false, checks };
    results.push(result);
    const check = (label, actual, expected) => {
      // Collect assertion failures so every independent contract is reported.
      try { assert.deepEqual(actual, expected); checks.push({ name: label, passed: true }); }
      catch (error) {
        if (!(error instanceof assert.AssertionError)) throw error;
        checks.push({ name: label, passed: false, actual, expected, message: error.message });
      }
    };
    try {
      await run({ server, output: directory, check, browser: {
        newPage: async options => {
          const page = await browser.newPage(options);
          pages.push(page);
          return page;
        },
      } });
      result.completed = true;
    } finally {
      result.passed = result.completed && checks.length > 0 && checks.every(item => item.passed);
      try { await writeFile(path.join(directory, "acceptance.json"), JSON.stringify(result, null, 2)); }
      finally { await Promise.all(pages.map(page => page.close())); }
    }
    assert(checks.length > 0, name + " must execute assertions");
    assert(checks.every(item => item.passed), JSON.stringify(checks.filter(item => !item.passed)));
    t.diagnostic(checks.length + " assertions passed");
  });
}
