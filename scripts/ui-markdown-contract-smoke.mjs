/* global window, document */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { chromium } from 'playwright-core';
import { startPreviewServer } from './ui-preview/server.mjs';

const before = process.argv.includes('--before');
const output = `output/audit-WB4/markdown/${before ? 'before' : 'current'}`;
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: 'msedge', headless: true });
const page = await browser.newPage();
const errors = [], checks = [];
page.on('pageerror', error => errors.push(String(error)));
const check = (name, actual, expected) => {
  try { assert.deepEqual(actual, expected); checks.push({ name, passed: true }); }
  catch (error) { checks.push({ name, passed: false, actual, expected, message: error.message }); }
};
try {
  if (before) {
    const source = execFileSync('git', ['show', '26acb7f0:crates/kanzei-app/ui/04-markdown.js'], { encoding: 'utf8' });
    await page.route('**/04-markdown.js', route => route.fulfill({ contentType: 'text/javascript', body: source }));
  }
  await page.goto(`${server.origin}/?scene=chat`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async () => {
    const host = document.createElement('div'); host.id = 'wb4-markdown'; document.body.append(host);
    window.__mdOwner = { host, render: (await import('/04-markdown.js')).renderMarkdownInto, paths: [] };
    (await import('/04-structured.js')).setStructuredNav({ openPath: (path, line) => window.__mdOwner.paths.push({ path, line }) });
  });
  const render = raw => page.evaluate(raw => {
    const { host, render } = window.__mdOwner; render(host, raw);
    return { text: host.textContent, html: host.innerHTML, paths: [...host.querySelectorAll('a.md-path')].map(a => a.dataset.path),
      hrefs: [...host.querySelectorAll('a[href]')].map(a => a.getAttribute('href')),
      cells: [...host.querySelectorAll('tbody td')].map(c => c.textContent),
      codes: [...host.querySelectorAll('pre code')].map(c => c.textContent), open: !!host.querySelector('pre[data-open]') };
  }, raw);
  let result = await render('[source](C:/project/(draft)/main.rs:12)');
  check('Balanced parentheses retain the full local path', result.paths, ['C:/project/(draft)/main.rs']);
  await page.locator('#wb4-markdown a.md-path').first().click();
  check('Actual delegated path click receives the complete path and line', await page.evaluate(() => window.__mdOwner.paths.at(-1)), { path: 'C:/project/(draft)/main.rs', line: 12 });
  result = await render('[reference](https://example.com/Function_(mathematics))');
  check('Balanced parentheses retain the full external URL', result.hrefs, ['https://example.com/Function_(mathematics)']);
  result = await render('See (https://example.com/Function_(mathematics)).');
  check('Bare URL keeps balanced parentheses while excluding prose punctuation', result.hrefs, ['https://example.com/Function_(mathematics)']);
  result = await render('[space](<C:/project/my report.md:3>) [`label`](docs/readme.md)');
  check('Existing angle-wrapped path and nested inline-code label remain valid', result.paths, ['C:/project/my report.md', 'docs/readme.md']);
  result = await render('| kind | effect |\n| --- | --- |\n| `back \\| forward` | navigate |');
  check('Escaped pipe in an actual API table preserves both columns', result.cells, ['back | forward', 'navigate']);
  result = await render('| kind | effect |\n| --- | --- |\n| a\\|b | unchanged |');
  check('Escaped pipe in plain table text stays within its cell', result.cells, ['a|b', 'unchanged']);
  result = await render('````markdown\n```js\nconst n = 1;\n```\n````');
  check('An outer code fence preserves nested shorter fences literally', result.codes, ['```js\nconst n = 1;\n```']);
  check('A matching longer fence closes the code block', result.open, false);
  result = await render('```text\n```js\nkeep this literal\n```');
  check('A language-bearing fence inside code cannot prematurely close it', result.codes, ['```js\nkeep this literal']);
  result = await render('````html\n<script>unsafe()</script>\n```');
  check('A shorter fence cannot close a streaming code block', result.open, true);
  check('Streaming code keeps HTML literal', result.codes, ['<script>unsafe()</script>\n```']);
  result = await render('[bad](javascript:alert(1)) <img src=x onerror=alert(1)>');
  check('Unsafe links and HTML remain inert', [result.hrefs.length, await page.locator('#wb4-markdown img, #wb4-markdown script').count()], [0, 0]);
  check('No browser runtime errors', errors, []);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ before, checks, errors }, null, 2));
  console.log(`Markdown contracts: ${checks.filter(c => c.passed).length} passed, ${checks.filter(c => !c.passed).length} failed`);
  assert(checks.every(c => c.passed), JSON.stringify(checks.filter(c => !c.passed)));
} finally { await browser.close(); await server.close(); }
