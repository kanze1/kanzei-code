/* global window, document */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { chromium } from 'playwright-core';
import { startPreviewServer } from './ui-preview/server.mjs';

const before = process.argv.includes('--before');
const output = `output/audit-WB3/actions/${before ? 'before' : 'current'}`;
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: 'msedge', headless: true });
const checks = [], errors = [];
const check = (name, actual, expected) => {
  try { assert.deepEqual(actual, expected); checks.push({ name, passed: true }); }
  catch (error) { checks.push({ name, passed: false, actual, expected, message: error.message }); }
};
try {
  for (const scenario of ['preview-navigation', 'preview-rejection', 'fork-receipt', 'fork-refresh', 'fork-history', 'replaced-dialog', 'rewind-draft', 'fork-success']) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    page.on('pageerror', error => errors.push(String(error)));
    if (before) {
      const source = execFileSync('git', ['show', '26acb7f0:crates/kanzei-app/ui/05-conversation-actions.js'], { encoding: 'utf8' });
      await page.route('**/05-conversation-actions.js', route => route.fulfill({ contentType: 'text/javascript', body: source }));
    }
    await page.goto(`${server.origin}/?scene=chat`);
    await page.waitForFunction(() => window.__kzPreview?.ready);
    const ids = await page.evaluate(async scenario => {
      const fixture = await window.__kzPreview.fixtures();
      const sessions = await import('/09-sessions.js'), shell = await import('/03-shell.js');
      await sessions.switchProcess(fixture.ids.idleProcess, true);
      const project = shell.currentProject;
      const fork = await window.__TAURI__.core.invoke('process_create', { projectDir: project, profile: 'dev' });
      const other = await window.__TAURI__.core.invoke('process_create', { projectDir: project, profile: 'dev' });
      await sessions.refreshProcesses();
      const source = shell.activeProcessId;
      const probe = window.__actionOwner = { blocked: false, released: false, started: false };
      const gate = () => { probe.blocked = true; return new Promise(resolve => { probe.release = () => { probe.released = true; resolve(); }; }); };
      window.__kzPreview.setCommand('process_list', async args => {
        if (scenario === 'fork-refresh' && probe.started && !probe.released) await gate();
        return fixture.commands.process_list(args);
      });
      window.__kzPreview.setCommand('conversation_display_get', async args => {
        if (scenario === 'fork-history' && args.processId === fork.id && !probe.released) await gate();
        return fixture.commands.conversation_display_get(args);
      });
      window.__kzPreview.setCommand('conversation_action', async args => {
        if (args.action === 'preview') {
          if (scenario.startsWith('preview-')) await gate();
          if (scenario === 'preview-rejection') throw new Error('WB3_STALE_PREVIEW_FAILURE');
          return { sourceHash: 'owner-version', keptMessages: 1, files: [], unhandled: [], worktree: false };
        }
        probe.started = true;
        if (['fork-receipt', 'rewind-draft', 'replaced-dialog'].includes(scenario)) await gate();
        return args.action === 'fork' ? { forked: true, processId: fork.id, prompt: 'WB3 source message' } : { prompt: 'WB3 source message', skipped: [] };
      });
      const { addMessage } = await import('/05-chat-render.js');
      addMessage('user', 'WB3 source message');
      return { project, source, fork: fork.id, forkSession: fork.session_id, other: other.id };
    }, scenario);
    await page.locator('.msg.user').last().getByRole('button', { name: '回退或分叉', exact: true }).click();
    const dialog = page.locator('#conversation-action-overlay');
    if (scenario.startsWith('preview-')) {
      await page.waitForFunction(() => window.__actionOwner.blocked);
      await page.evaluate(async () => (await import('/03-shell.js')).navigate_view('settings'));
    } else {
      await dialog.getByRole('button', { name: scenario === 'rewind-draft' ? '只回退对话' : '从这里分叉', exact: true }).click();
      if (scenario !== 'fork-success') {
        await page.waitForFunction(() => window.__actionOwner.blocked);
        if (await dialog.isVisible()) await page.keyboard.press('Escape');
        if (scenario === 'replaced-dialog') {
          await page.locator('.msg.user').last().getByRole('button', { name: '回退或分叉', exact: true }).click();
          await dialog.waitFor({ state: 'visible' });
        } else {
          if (scenario !== 'rewind-draft') await page.evaluate(async id => (await import('/09-sessions.js')).switchProcess(id, true), ids.other);
          await page.locator('#prompt').fill('new user draft');
        }
      }
    }
    if (scenario !== 'fork-success') await page.evaluate(() => window.__actionOwner.release());
    await page.evaluate(() => window.__kzPreview.settle());
    // Let the event listener's continuation finish after isolated IPC settles.
    await page.waitForTimeout(150);
    const actual = await page.evaluate(async () => {
      const shell = await import('/03-shell.js');
      return { id: shell.activeProcessId, session: shell.activeSessionId, draft: document.getElementById('prompt').value,
        view: document.body.dataset.view, dialog: document.getElementById('conversation-action-overlay').open,
        staleError: document.body.textContent.includes('WB3_STALE_PREVIEW_FAILURE') };
    });
    if (scenario === 'preview-navigation') check('Late preview preserves newer navigation and does not open a dialog', [actual.view, actual.dialog], ['settings', false]);
    else if (scenario === 'preview-rejection') check('An old preview failure is not reported over the newer page', actual.staleError, false);
    else if (scenario === 'replaced-dialog') check('An earlier action cannot close a newer preview or select its fork', [actual.id, actual.dialog], [ids.source, true]);
    else if (scenario === 'rewind-draft') check('Rewind completion preserves input typed after submission', [actual.id, actual.draft], [ids.source, 'new user draft']);
    else if (scenario === 'fork-success') check('Uncanceled fork selects the new session and restores its prompt', [actual.id, actual.session, actual.draft], [ids.fork, ids.forkSession, 'WB3 source message']);
    else check(`${scenario}: late fork completion preserves the newer session and draft`, [actual.id, actual.draft], [ids.other, 'new user draft']);
    await page.close();
  }
  check('No browser runtime errors', errors, []);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ before, checks, errors }, null, 2));
  console.log(`Conversation action owner: ${checks.filter(c => c.passed).length} passed, ${checks.filter(c => !c.passed).length} failed`);
  assert(checks.every(c => c.passed), JSON.stringify(checks.filter(c => !c.passed)));
} finally { await browser.close(); await server.close(); }
