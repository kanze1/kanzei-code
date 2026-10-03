/* global window, document */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from 'playwright-core';
import { startPreviewServer } from './ui-preview/server.mjs';

const before = process.argv.includes('--before');
const output = `output/audit-WB5/summary/${before ? 'before' : 'current'}`;
await mkdir(output, { recursive: true });
const oldDir = path.resolve(output, 'old'), newDir = path.resolve(output, 'new');
await mkdir(oldDir, { recursive: true }); await mkdir(newDir, { recursive: true });
// Read-only Git produces the protocol: Markdown rules and ++ expressions are real content.
await writeFile(path.join(oldDir, '中文.md'), '--old\n---\n');
await writeFile(path.join(newDir, '中文.md'), '++new\n+++\n');
let diff;
try { diff = execFileSync('git', ['-c', 'core.quotePath=true', 'diff', '--no-index', '--no-ext-diff', oldDir, newDir], { encoding: 'utf8' }); }
catch (error) { if (error.status !== 1) throw error; diff = String(error.stdout); }
await writeFile(path.join(output, 'actual-git.diff'), diff);
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
    const source = execFileSync('git', ['show', '26acb7f0:crates/kanzei-app/ui/05-tool-summary.js'], { encoding: 'utf8' });
    await page.route('**/05-tool-summary.js', route => route.fulfill({ contentType: 'text/javascript', body: source }));
  }
  await page.goto(`${server.origin}/?scene=chat`);
  await page.waitForFunction(() => window.__kzPreview?.ready);
  const summarize = (name, ctx) => page.evaluate(async ({ name, ctx }) => {
    const module = await import('/05-tool-summary.js');
    const result = module.toolResultSummary(name, { roots: [], ok: true, ...ctx });
    const host = document.createElement('div'); module.renderToolSummary(host, result);
    return { text: result.text, outcome: result.outcome, key: result.key, title: host.title, rendered: host.textContent, rest: result.rest };
  }, { name, ctx });
  const facts = [
    'not a git repository: C:/project 不是 Git 仓库(也不在任何仓库里)。并行线/工作树、提交与差异不可用;需要版本管理时用 git action=init 建库(会先征得用户同意)。',
    'not an independent git repository: C:/parent/project 只是位于上级仓库 C:/parent 内。git 工具不读、不改上级仓库;需要版本管理时用 git action=init 在项目根建独立仓库,或让用户把上级仓库根登记为项目。',
  ];
  for (const content of facts) for (const action of ['status', 'diff', 'log']) {
    const result = await summarize('git', { content, input: { action } });
    check(`${action} preserves unavailable repository fact: ${content.split(':')[0]}`, result.text, '本项目不是 Git 仓库');
    check(`${action} keeps the repository explanation and successful observation outcome`, { title: result.title, outcome: result.outcome }, { title: content, outcome: 'success' });
  }
  const changed = await summarize('git', { content: diff, input: { action: 'diff' } });
  check('Actual Git hunks count plus/minus-prefixed content as changed lines', changed.text, '1 个文件 +2 −2');
  check('DOM summary agrees with the parsed diff count', changed.rendered, '⎿ 1 个文件 +2 −2');
  for (const [name, ctx, expected] of [
    ['git', { content: '## main...origin/main', input: { action: 'status' } }, '工作区干净'],
    ['git', { content: '## main\n M src/a.rs\n?? b.rs', input: { action: 'status' } }, '2 处改动'],
    ['git', { content: '(no diff)', input: { action: 'diff' } }, '无差异'],
    ['git', { content: 'diff --git a/a.bin b/a.bin\nBinary files a/a.bin and b/a.bin differ', input: { action: 'diff' } }, '1 个文件 +0 −0'],
    ['git', { content: 'diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new', input: { action: 'diff' } }, '1 个文件 +1 −1'],
    ['git', { content: 'abc1234 first\ndef5678 second', input: { action: 'log' } }, '2 条提交'],
    ['edit', { ok: false, content: '[tool_outcome=noop code=EDIT_NO_CHANGE]\nNothing changed' }, '无需修改'],
    ['bash', { ok: false, content: 'exit code: 1\nError: command failed' }, '退出码 1 · Error: command failed'],
    ['read', { ok: false, content: '[tool_outcome=needs_correction code=READ_PATH_NOT_FOUND]\npath not found: src/missing.rs' }, '路径不存在 · src/missing.rs'],
  ]) check(`${name} control: ${expected}`, (await summarize(name, ctx)).text, expected);
  const timed = await page.evaluate(async () => {
    const { toolResultSummary, withToolDuration } = await import('/05-tool-summary.js');
    return withToolDuration(toolResultSummary('git', { content: '## main', input: { action: 'status' }, roots: [] }), 2200).text;
  });
  check('Late duration decorates the existing summary once', timed, '工作区干净 · 2.2s');
  check('Browser module graph has no runtime errors', errors, []);
} finally { await browser.close(); await server.close(); }
const failures = checks.filter(check => !check.passed);
await writeFile(path.join(output, 'acceptance.json'), JSON.stringify({ before, checks, failures }, null, 2));
console.log(`Tool summary contracts: ${checks.length - failures.length} PASS, ${failures.length} FAIL`);
for (const failure of failures) console.error(`${failure.name}: ${failure.message}`);
if (failures.length) process.exitCode = 1;
