/* global window, document, navigator */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { chromium } from 'playwright-core';
import { startPreviewServer } from './ui-preview/server.mjs';
const before = process.argv.includes('--before');
const output = `output/audit-WB5/structured/${before ? 'before' : 'current'}`;
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
    const source = execFileSync('git', ['show', '26acb7f0:crates/kanzei-app/ui/04-structured.js'], { encoding: 'utf8' });
    await page.route('**/04-structured.js', route => route.fulfill({ contentType: 'text/javascript', body: source }));
  }
  await page.goto(`${server.origin}/?scene=chat`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  await page.evaluate(async () => {
    window.__sv = await import('/04-structured.js');
    const host = document.createElement('div'); host.id = 'wb5-structured'; document.body.append(host);
    window.__svHost = host;
    window.__svRows = Array.from({ length: 200 }, (_, index) => Object.fromEntries(Array.from({ length: 10 }, (_, field) => [`field${field}`, index * 10 + field])));
    host.replaceChildren(window.__sv.renderToolResult('bash', window.__svRows));
  });
  const snapshot = () => page.evaluate(() => {
    const root = window.__svHost.querySelector('.sv-json');
    const children = root.querySelector(':scope > .sv-json-row > details > .sv-children');
    const keys = [...children.querySelectorAll(':scope > .sv-json-row > details > summary > .sv-key')].map(node => Number(node.textContent));
    return { keys, more: children.querySelector(':scope > .sv-more')?.textContent ?? null, notes: root.querySelectorAll(':scope > .sv-note').length, nodes: root.querySelectorAll('.sv-json-row').length };
  });
  check('Default initial page contains exactly the first 100 nested rows', (await snapshot()).keys, Array.from({ length: 100 }, (_, i) => i));
  check('Initial rendering preserves the existing node budget', (await snapshot()).nodes, 1101);
  await page.locator('#wb5-structured .sv-json > .sv-json-row > details > .sv-children > .sv-more').click();
  let view = await snapshot();
  check('One user expansion preserves the existing incremental node budget', view.nodes, 1600);
  check('Paged rows remain a contiguous prefix when the shared node budget is hit', view.keys, Array.from({ length: view.keys.length }, (_, i) => i));
  check('Remaining count reflects rows actually rendered rather than requested page size', view.more, `还有 ${200 - view.keys.length} 项`);
  check('Reaching the limit after a user click displays the limit notice', view.notes, 1);
  const expanded = await page.evaluate(() => {
    let clicks = 0;
    while (window.__svHost.querySelector('.sv-more') && clicks < 1000) { window.__svHost.querySelector('.sv-more').click(); clicks++; }
    return { values: [...window.__svHost.querySelectorAll('.sv-num')].map(node => Number(node.textContent)).sort((a, b) => a - b), pending: !!window.__svHost.querySelector('.sv-more') };
  });
  check('Repeated expansion reaches every nested value without silently skipped rows or fields', expanded.values, Array.from({ length: 2000 }, (_, i) => i));
  check('Fully expanded JSON has no stale continuation buttons', expanded.pending, false);
  view = await snapshot();
  check('Continuation does not duplicate the shared limit notice', view.notes, 1);
  const rawCopy = await page.evaluate(async () => {
    let copied = null;
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async text => { copied = text; } } });
    window.__svHost.querySelector('.sv-copy-json').click();
    await Promise.resolve();
    return copied === JSON.stringify(window.__svRows, null, 2);
  });
  check('Raw JSON copy remains complete after repeated expansion', rawCopy, true);
  const controls = await page.evaluate(() => {
    const s = window.__sv, host = window.__svHost;
    host.replaceChildren(s.renderJsonTree([{ a: 1, b: 2, c: 3 }, { a: 4, b: 5, c: 6 }], { maxNodes: 3, maxItems: 2 }));
    const cappedInitially = !!host.querySelector('.sv-note');
    const resumable = !!host.querySelector('.sv-more');
    let clicks = 0;
    while (host.querySelector('.sv-more') && clicks++ < 20) host.querySelector('.sv-more').click();
    const restored = [...host.querySelectorAll('.sv-num')].map(node => Number(node.textContent));
    host.replaceChildren(s.renderToolResult('bash', JSON.parse('[{"name":"first","constructor":"own","toString":"explicit"},{"name":"second"}]')));
    const cells = [...host.querySelectorAll('tbody td')].map(node => node.textContent);
    host.replaceChildren(s.renderJsonTree(Array.from({ length: 130 }, (_, i) => i)));
    host.querySelector('.sv-more').click();
    const flatCount = host.querySelectorAll('.sv-num').length;
    host.replaceChildren(s.renderToolResult('bash', { output: '<img src=x onerror=alert(1)>' }));
    const inert = host.querySelectorAll('img, script').length === 0 && host.textContent.includes('<img src=x');
    host.replaceChildren(s.renderToolArgs('edit', { path: 'src/main.rs', old_string: 'old', new_string: 'new' }, { display: { kind: 'diff' } }));
    const foldedEdit = host.querySelectorAll('.sv-raw-args .sv-kv-row').length;
    host.replaceChildren(s.renderPermissionResource('bash', 'echo ok'));
    const permissionLiteral = host.textContent.includes('echo ok');
    let builds = 0;
    host.replaceChildren();
    s.lazyMount(host, () => { builds++; return s.renderToolResult('bash', { ok: true }); });
    const firstFlush = s.flushLazy(host), secondFlush = s.flushLazy(host);
    return { cappedInitially, resumable, restored, cells, flatCount, inert, foldedEdit, permissionLiteral, lazy: [builds, firstFlush, secondFlush] };
  });
  check('Initial node cap is still visible', controls.cappedInitially, true);
  check('Initially capped nested nodes retain a continuation', controls.resumable, true);
  check('Initial cap continuation preserves every value', controls.restored, [1, 2, 3, 4, 5, 6]);
  check('Missing JSON fields remain blank even when another row has a prototype-named own key', controls.cells, ['first', 'own', 'explicit', 'second', '', '']);
  check('Existing scalar pagination still completes in one click', controls.flatCount, 130);
  check('Untrusted tool strings remain inert DOM text', controls.inert, true);
  check('Edit bodies stay available in the original-arguments fold', controls.foldedEdit, 2);
  check('Permission command remains visible', controls.permissionLiteral, true);
  check('Lazy tool result builds once and only once', controls.lazy, [1, 1, 0]);
  check('No browser runtime errors', errors, []);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ before, checks, errors }, null, 2));
  console.log(`Structured render contracts: ${checks.filter(c => c.passed).length} passed, ${checks.filter(c => !c.passed).length} failed`);
  assert(checks.every(c => c.passed), JSON.stringify(checks.filter(c => !c.passed)));
} finally { await browser.close(); await server.close(); }
