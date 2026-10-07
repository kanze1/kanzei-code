/* global document, window, getComputedStyle */
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { chromium } from 'playwright-core';
import { startPreviewServer } from './ui-preview/server.mjs';

const before = process.argv.includes('--before');
const output = `output/playwright/exploration-status/${before ? 'before' : 'current'}`;
await mkdir(output, { recursive: true });
const server = await startPreviewServer({ port: 0 });
const browser = await chromium.launch({ channel: 'msedge', headless: true });
const checks = [], errors = [];
try {
  for (const width of [1305, 420]) {
    const page = await browser.newPage({ viewport: { width, height: 1000 } });
    page.on('pageerror', error => errors.push(String(error)));
    if (before) for (const file of ['style.css', '02-i18n.js', '05-chat-render.js', '05-subagents.js', '06-agent-panel.js']) {
      const source = execFileSync('git', ['show', `80cef34a:crates/kanzei-app/ui/${file}`], { encoding: 'utf8' });
      await page.route(`**/${file}`, route => route.fulfill({ contentType: file.endsWith('.css') ? 'text/css' : 'text/javascript', body: source }));
    }
    await page.goto(`${server.origin}/?scene=chat`);
    await page.waitForFunction(() => window.__kzPreview?.ready);
    const result = await page.evaluate(async () => {
      const chat = await import('/05-chat-render.js');
      const sa = await import('/05-subagents.js');
      const shell = await import('/03-shell.js');
      const panel = await import('/06-agent-panel.js');
      const i18n = await import('/02-i18n.js');
      const checks = [];
      const check = (name, actual, expected) => checks.push({ name, passed: JSON.stringify(actual) === JSON.stringify(expected), actual, expected });
      const sid = shell.activeSessionId;
      const pane = document.querySelector('#messages .msg-pane:not(.hidden)');
      const group = chat.buildToolGroup();
      const read = chat.buildToolBlock('read', { path: 'crates/kanzei-app/src/run/assembly.rs' });
      const bad = chat.buildToolBlock('architecture', { action: 'context', area: 'kanzei-app/src/run/assembly.rs' });
      const search = chat.buildToolBlock('grep', { pattern: 'build_harness' });
      chat.fillToolBlock(read, { ok: true, content: '1\tfn assemble() {}' });
      chat.fillToolBlock(bad, { ok: false, content: '未找到项目区域 kanzei-app/src/run/assembly.rs' });
      chat.fillToolBlock(search, { ok: true, content: 'crates/kanzei-tools/src/run.rs: build_harness' });
      group._kzGroup.body.append(read.wrap, bad.wrap, search.wrap);
      pane.append(group);
      chat.syncToolGroup(group);
      check('Collapsed group keeps the failed call inside details', bad.wrap.getClientRects().length > 0, false);
      check('Header describes a call exception, not task failure', group._kzGroup.fail.textContent, '· 1 次调用异常');
      check('Exception count uses the neutral header color', getComputedStyle(group._kzGroup.fail).color, getComputedStyle(group._kzGroup.head).color);
      group._kzGroup.head.click();
      check('Expanding keeps the failed call and its original position', [bad.wrap.getClientRects().length > 0, [...group._kzGroup.body.children].indexOf(bad.wrap)], [true, 1]);
      group._kzGroup.head.click();
      check('Collapsing again hides the failed call', bad.wrap.getClientRects().length > 0, false);

      const input = { agent: 'explore', description: '只读梳理当前项目架构及关键调用链', prompt: '只读探索项目边界' };
      const run = sa.subagentStart({ sessionId: sid, id: 'status-recovered', input });
      sa.subagentProgress({ sessionId: sid, id: run.id, trace: { phase: 'start', child_id: 'bad', name: 'architecture', input: { action: 'context', area: 'missing' } } });
      sa.subagentProgress({ sessionId: sid, id: run.id, trace: { phase: 'end', child_id: 'bad', ok: false, preview: '未找到项目区域' } });
      check('A child call exception does not terminate the task', run.state, 'running');
      sa.subagentEnd({ sessionId: sid, id: run.id, ok: true, outcome: 'success', content: '已核对架构与入口。' });
      check('Recovered task still reports completion', [run.state, run.cardEl.querySelector('.sa-meta').textContent.includes('失败')], ['done', false]);
      const limited = sa.subagentStart({ sessionId: sid, id: 'status-limited', input });
      const reason = '子任务达到步骤上限；已保存上下文，需要明确续做或重新派发。以下是未完成的进展：';
      sa.subagentEnd({ sessionId: sid, id: limited.id, ok: true, outcome: 'noop', code: 'subagent_step_limit_reached', content: reason });
      check('Live step ceiling is a distinct incomplete state', limited.state, 'limited');
      check('Step ceiling header gives the reason', limited.cardEl.querySelector('.sa-meta').textContent.includes('达到步数上限'), true);
      check('Step ceiling preserves the saved-progress explanation', limited.cardEl.querySelector('.sa-error').classList.contains('hidden'), false);
      panel.openTasksPanel();
      panel.renderTasksList();
      const reviewRow = [...document.querySelectorAll('.tp-agent-row')].find(row => row.dataset.saKey === limited.key);
      check('Step ceiling remains in the attention section', [reviewRow?.dataset.s, Boolean(reviewRow?.closest('[data-state="attention"]'))], ['warn', true]);
      const history = sa.subagentHistoryCall(sid, 'status-history-limit', input);
      sa.subagentHistoryResult(sid, history.id, { ok: true, content: `[tool_outcome=noop code=subagent_step_limit_reached]\n${reason}` });
      check('Historical step ceiling agrees with the live state', history.state, 'limited');
      check('Ordinary task execution errors remain failures', sa.classifySubagentEnd({ ok: false, code: 'EXEC_FAILED', preview: 'boom' }), 'failed');
      check('Successful answers mentioning old step ceilings remain complete', sa.classifySubagentEnd({ ok: true, outcome: 'success', content: '已修复子任务达到步骤上限的问题' }), 'done');
      check('Explicit error code is not overridden by progress wording', sa.classifySubagentEnd({ ok: false, code: 'EXEC_FAILED', preview: reason }), 'failed');
      i18n.setLanguagePreference('en', { persist: true, rerender: true });
      chat.syncToolGroup(group);
      sa.renderSubagentCard(limited);
      check('English call exception uses singular wording', group._kzGroup.fail.textContent, '· 1 tool call error');
      check('English step ceiling has the same explicit status', limited.cardEl.querySelector('.sa-meta').textContent.includes('Step limit reached'), true);
      i18n.setLanguagePreference('zh', { persist: true, rerender: true });
      group.scrollIntoView({ block: 'center' });
      return checks;
    });
    checks.push(...result.map(check => ({ width, ...check })));
    await page.screenshot({ path: `${output}/${width}.png`, fullPage: true });
    await page.close();
  }
} finally { await browser.close(); await server.close(); }
checks.push({ name: 'Browser module graph has no runtime errors', passed: errors.length === 0, actual: errors, expected: [] });
const failures = checks.filter(check => !check.passed);
await writeFile(`${output}/acceptance.json`, JSON.stringify({ before, checks, failures }, null, 2));
console.log(`Exploration status: ${checks.length - failures.length} PASS, ${failures.length} FAIL`);
for (const failure of failures) console.error(`${failure.width || ''} ${failure.name}: ${JSON.stringify(failure.actual)} != ${JSON.stringify(failure.expected)}`);
if (!before) assert.equal(failures.length, 0);
else assert(failures.length > 0, 'The released baseline should reproduce the display defects.');
