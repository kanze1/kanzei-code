/* global window, document, localStorage, Event */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { chromium } from 'playwright-core';
import { startPreviewServer } from './ui-preview/server.mjs';

const before = process.argv.includes('--before');
const output = `output/audit-WB5/i18n/${before ? 'before' : 'current'}`;
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: 'msedge', headless: true });
const checks = [];
const check = (name, actual, expected) => {
  try { assert.deepEqual(actual, expected); checks.push({ name, passed: true }); }
  catch (error) { checks.push({ name, passed: false, actual, expected, message: error.message }); }
};
try {
  for (const scenario of ['modern-race', 'legacy-race', 'modern-normal', 'legacy-normal']) {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    const legacy = scenario.startsWith('legacy');
    const race = scenario.endsWith('race');
    if (before) {
      for (const file of ['02-i18n.js', '18-startup.js']) {
        const source = execFileSync('git', ['show', `26acb7f0:crates/kanzei-app/ui/${file}`], { encoding: 'utf8' });
        await page.route(`**/${file}`, route => route.fulfill({ contentType: 'text/javascript', body: source }));
      }
    }
    const mock = await readFile('scripts/ui-preview/mock-ipc.js', 'utf8');
    await page.route('**/__preview/mock-ipc.js', route => route.fulfill({ contentType: 'text/javascript', body: mock + `
      window.__languageGate = {};
      window.__kzPreview.setCommand('ui_prefs_get', async () => {
        const prefs = { ui_layout: { prefs: ${legacy ? '{}' : "{ language: 'zh' }"} } };
        ${legacy ? 'return prefs;' : "await new Promise(resolve => window.__languageGate.release = resolve); return prefs;"}
      });
      ${legacy ? `
      let firstSettings = true;
      window.__kzPreview.setCommand('settings_get', async args => {
        const fixtures = await window.__kzPreview.fixtures();
        const handler = fixtures.commands.settings_get;
        const settings = typeof handler === 'function' ? await handler(args) : handler;
        if (firstSettings) {
          firstSettings = false;
          await new Promise(resolve => window.__languageGate.release = resolve);
        }
        return { ...settings, language: 'zh' };
      });` : ''}
    ` }));
    await page.goto(`${server.origin}/?scene=chat`);
    await page.waitForFunction(() => typeof window.__languageGate?.release === 'function' && typeof window.t === 'function');
    if (race) {
      await page.evaluate(() => {
        const select = document.getElementById('language-select');
        select.value = 'en'; select.dispatchEvent(new Event('change', { bubbles: true }));
      });
      check(`${scenario}: user choice applies before startup finishes`, await page.evaluate(() => [localStorage.getItem('kz-language'), document.documentElement.lang]), ['en', 'en']);
    }
    await page.evaluate(() => window.__languageGate.release());
    await page.waitForFunction(() => window.__kzPreview?.ready);
    const result = await page.evaluate(() => ({
      cache: localStorage.getItem('kz-language'), lang: document.documentElement.lang,
      select: document.getElementById('language-select').value,
      writes: window.__kzPreview.calls.filter(call => call.cmd === 'ui_prefs_set' && call.args.ui_layout?.prefs?.language).map(call => call.args.ui_layout.prefs.language),
      send: window.t('发送'),
    }));
    const expected = race ? 'en' : 'zh';
    check(`${scenario}: cache retains winning preference`, result.cache, expected);
    check(`${scenario}: rendered language matches preference`, result.lang, race ? 'en' : 'zh-CN');
    check(`${scenario}: selector matches preference`, result.select, expected);
    check(`${scenario}: migration cannot overwrite a user write`, result.writes, race ? ['en'] : legacy ? ['zh'] : []);
    check(`${scenario}: translator uses winning preference`, result.send, race ? 'Send' : '发送');
    check(`${scenario}: no browser runtime errors`, errors, []);
    await page.close();
  }
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ before, checks }, null, 2));
  console.log(`I18n contracts: ${checks.filter(c => c.passed).length} passed, ${checks.filter(c => !c.passed).length} failed`);
  assert(checks.every(c => c.passed), JSON.stringify(checks.filter(c => !c.passed)));
} finally { await browser.close(); await server.close(); }
