/* global window, document, File, FileReader */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { chromium } from 'playwright-core';
import { startPreviewServer } from './ui-preview/server.mjs';

const before = process.argv.includes('--before');
const output = `output/audit-WB4/resources/${before ? 'before' : 'current'}`;
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
    const source = execFileSync('git', ['show', '26acb7f0:crates/kanzei-app/ui/04-resource-types.js'], { encoding: 'utf8' });
    await page.route('**/04-resource-types.js', route => route.fulfill({ contentType: 'text/javascript', body: source }));
  }
  await page.goto(`${server.origin}/?scene=chat`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  const attach = (name, type, body) => page.evaluate(async ({ name, type, body }) => {
    const shell = await import('/03-shell.js');
    const compose = await import('/08-compose-runtime.js');
    shell.setAttachments([]);
    compose.renderAttachments();
    const readers = [], NativeReader = window.FileReader;
    // Observe native completion without replacing file reads, callbacks or attachment logic.
    window.FileReader = class extends NativeReader {
      readAsDataURL(file) {
        readers.push(new Promise(resolve => this.addEventListener('loadend', resolve, { once: true })));
        return super.readAsDataURL(file);
      }
    };
    try {
      compose.addFiles([new File([body], name, { type })]);
      await Promise.all(readers);
      return {
        reads: readers.length,
        attachments: shell.attachments.map(item => ({ name: item.file_name, mime: item.media_type, body: window.atob(item.data), bytes: item.bytes })),
        chips: [...document.querySelectorAll('#attachments .attachment-name')].map(node => node.textContent),
      };
    } finally { window.FileReader = NativeReader; }
  }, { name, type, body });
  for (const [name, type, body] of [
    ['budget#2026.csv', 'text/csv', 'year,total\n2026,42'],
    ['plan#final.txt', 'text/plain', 'A real plan'],
    ['plain.csv', 'text/csv', 'value\n1'],
  ]) {
    check(`Real addFiles and native FileReader preserve ${name}`, await attach(name, type, body), {
      reads: 1, attachments: [{ name, mime: type, body, bytes: body.length }], chips: [name],
    });
  }
  check('Unsupported executables remain rejected before reading', await attach('program.exe', 'application/octet-stream', 'test'), {
    reads: 0, attachments: [], chips: [],
  });
  const contract = await page.evaluate(async () => {
    const resource = await import('/04-resource-types.js');
    const status = await import('/04-status-words.js');
    return {
      local: resource.resourceType('C:/reports/budget#2026.csv'),
      url: resource.resourceType('https://example.com/budget%232026.csv?download=1#preview'),
      urlMime: resource.attachmentMime('https://example.com/budget.csv?download=1#preview'),
      pathLine: resource.resourceType('C:\\src\\MAIN.RS:12-14'),
      fallback: [resource.attachmentMime('scan.unknown', 'image/png'), resource.attachmentMime('report.unknown', 'application/pdf')],
      git: resource.resourceType('https://github.com/org/project/blob/main/readme.md'),
      links: resource.resourceLinks('See (https://example.com/Function_(math)).').map(link => link.url),
      status: [status.statusWord('active'), status.statusWord('active', false, 'unit'), status.statusWord('active', true, 'unit')],
      unknown: [status.statusWord('new_state'), status.statusWord('new_state', true), status.stageWord('新阶段', true)],
      missing: [status.statusWord(null), status.stageWord(undefined)],
      stage: [status.stageWord('工具执行中'), status.stageWord('工具执行中', true)],
    };
  });
  check('Local resource icon retains filename after a hash', contract.local, { kind: 'sheet', label: '表格' });
  check('URL decoding keeps encoded filename hashes distinct from URL fragments', contract.url, { kind: 'sheet', label: '表格' });
  check('URL query and fragment do not change an attachment extension', contract.urlMime, 'text/csv');
  check('Windows uppercase source paths and line ranges retain their type', contract.pathLine, { kind: 'code', label: '代码' });
  check('Known image and PDF MIME fallbacks remain accepted', contract.fallback, ['image/png', 'application/pdf']);
  check('Git host recognition retains precedence over file type', contract.git, { kind: 'git', label: 'github.com' });
  check('Resource links retain balanced parentheses and exclude prose punctuation', contract.links, ['https://example.com/Function_(math)']);
  check('Status words retain scoped meaning without changing raw state', contract.status, ['启用', '开发中', 'In development']);
  check('Unknown state and stage labels remain visible', contract.unknown, ['new_state', 'new_state', '新阶段']);
  check('Missing labels display empty text', contract.missing, ['', '']);
  check('Stage translation preserves source language behavior', contract.stage, ['工具执行中', 'Running tool']);
  check('No browser runtime errors', errors, []);
  await writeFile(`${output}/acceptance.json`, JSON.stringify({ before, checks, errors }, null, 2));
  console.log(`Resource contracts: ${checks.filter(c => c.passed).length} passed, ${checks.filter(c => !c.passed).length} failed`);
  assert(checks.every(c => c.passed), JSON.stringify(checks.filter(c => !c.passed)));
} finally { await browser.close(); await server.close(); }
