/* global window, document */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { chromium } from 'playwright-core';
import { startPreviewServer } from './ui-preview/server.mjs';
const before = process.argv.includes('--before');
const output = `output/audit-WB5/diagram/${before ? 'before' : 'current'}`;
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
  if (before) await page.route('**/04-diagram.js', route => route.fulfill({ contentType: 'text/javascript', body: execFileSync('git', ['show', '26acb7f0:crates/kanzei-app/ui/04-diagram.js'], { encoding: 'utf8' }) }));
  await page.goto(`${server.origin}/?scene=chat`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  const results = await page.evaluate(async () => {
    const d = await import('/04-diagram.js'), arch = await import('/19-arch.js');
    const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
    const svg = id => ({ svg: `<svg xmlns="http://www.w3.org/2000/svg" id="${id}" width="400" height="200"><g class="node" id="${id}-flowchart-a-0"><text>A</text></g></svg>` });
    let gate = deferred(), entered = deferred();
    d.setDiagramEngine({ initialize() {}, async parse() {}, async render(id, text) {
      if (text.includes('SLOW')) { entered.resolve(); await gate.promise; }
      if (text.includes('BROKEN')) throw new Error('Rendering rejected');
      return svg(id);
    } });
    const b = 'flowchart LR\n b[B]', a = 'flowchart LR\n a[SLOW]';
    await d.renderDiagram(b);
    const snap = { diagrams: [{ id: 'wb5-a', title: 'A', path: 'docs/architecture/a.md', source: a }, { id: 'wb5-b', title: 'B', path: 'docs/architecture/b.md', source: b }] };
    arch.selectArchDiagram(snap, 'wb5-a'); await entered.promise;
    arch.selectArchDiagram(snap, 'wb5-b');
    gate.resolve(); await d.renderDiagram('flowchart LR\n barrier[barrier]');
    const switchedFoot = document.querySelector('#arch-diagram-foot .arch-foot-path').textContent;
    const switchedState = document.querySelector('#arch-diagram-canvas figure').dataset.state;
    const host = document.createElement('div'); document.body.append(host);
    let notified = 0;
    const NativeObserver = window.ResizeObserver, observed = [];
    window.ResizeObserver = class extends NativeObserver {
      observe(target) { observed.push(this); return super.observe(target); }
      disconnect() { this.released = true; return super.disconnect(); }
    };
    gate = deferred(); entered = deferred();
    const retired = d.mountDiagram(host, 'flowchart LR\n c[SLOW destroyed]', { onRendered: () => { notified += 1; } });
    window.ResizeObserver = NativeObserver;
    await entered.promise; retired.destroy(); gate.resolve();
    await d.renderDiagram('flowchart LR\n barrier2[barrier2]');
    const destroyed = { notified, connected: retired.root.isConnected, result: retired.result };
    const released = observed.length === 1 && observed.every(observer => observer.released);
    const live = d.mountDiagram(host, b);
    const success = { state: live.root.dataset.state, nodes: live.graph.nodes.size };
    await live.setSource('flowchart LR\n broken[BROKEN]');
    const failure = { state: live.root.dataset.state, svg: live.root.querySelectorAll('.kz-diagram-stage svg').length, graph: live.graph === null, fallback: live.root.querySelector('.kz-diagram-fallback code')?.textContent };
    await live.setSource(b);
    const recovered = { state: live.root.dataset.state, nodes: live.graph.nodes.size, errorHidden: live.root.querySelector('.kz-diagram-error').classList.contains('hidden') };
    gate = deferred(); entered = deferred();
    const slow = live.setSource('flowchart LR\n latest[SLOW stale]'); await entered.promise;
    const newest = live.setSource(b); gate.resolve(); await Promise.all([slow, newest]);
    const latest = { source: live.source, state: live.root.dataset.state, errorHidden: live.root.querySelector('.kz-diagram-error').classList.contains('hidden') };
    live.destroy(); host.remove();
    document.documentElement.setAttribute('data-theme', 'dark');
    await new Promise(resolve => setTimeout(resolve, 0));
    gate = deferred(); entered = deferred();
    let config;
    d.setDiagramEngine({ initialize(next) { config = next; }, async parse() {}, async render(id, text) {
      if (text.includes('QUEUE_BLOCK')) { entered.resolve(); await gate.promise; }
      return { svg: `<svg id="${id}" width="400" height="200"><text fill="${config.themeVariables.primaryTextColor}">Theme</text></svg>` };
    } });
    const expectedColor = d.diagramTokens().text;
    const blocker = d.renderDiagram('flowchart LR\n block[QUEUE_BLOCK]'); await entered.promise;
    const queued = d.renderDiagram('flowchart LR\n queued[dark cache]');
    document.documentElement.setAttribute('data-theme', 'light');
    gate.resolve(); await blocker;
    const queuedColor = (await queued).svg.match(/fill="([^"]+)"/)[1];
    return { switchedFoot, switchedState, released, expectedColor, queuedColor, destroyed: { notified: destroyed.notified, connected: destroyed.connected, result: destroyed.result === null }, success, failure, recovered, latest };
  });
  check('Real architecture tab switch keeps the latest footer after old render completes', results.switchedFoot, 'docs/architecture/b.md');
  check('Current architecture graph remains ready', results.switchedState, 'ready');
  check('Destroyed view cannot publish a late render callback or result', results.destroyed, { notified: 0, connected: false, result: true });
  check('Destroy disconnects the real native resize observer', results.released, true);
  check('Normal cached render remains ready', results.success, { state: 'ready', nodes: 1 });
  check('Failed source replacement removes the old graph and preserves failed source', results.failure, { state: 'error', svg: 0, graph: true, fallback: 'flowchart LR\n broken[BROKEN]' });
  check('Failure does not poison subsequent successful rendering', results.recovered, { state: 'ready', nodes: 1, errorHidden: true });
  check('New source wins over a slower previous source', results.latest, { source: 'flowchart LR\n b[B]', state: 'ready', errorHidden: true });
  check('Queued rendering retains the requested theme tokens for its cache entry', results.queuedColor, results.expectedColor);
  check('No browser runtime errors', errors, []);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ before, checks, errors }, null, 2));
  console.log(`Diagram owner: ${checks.filter(c => c.passed).length} PASS / ${checks.filter(c => !c.passed).length} FAIL`);
  assert(checks.every(c => c.passed), JSON.stringify(checks.filter(c => !c.passed)));
} finally { await browser.close(); await server.close(); }
