/* global window, document */
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

export default async function run({ browser, server, output, check }) {
  const oldDir = path.resolve(output, 'old'), newDir = path.resolve(output, 'new');
  await mkdir(oldDir, { recursive: true }); await mkdir(newDir, { recursive: true });
  // Read-only Git produces the protocol: Markdown rules and ++ expressions are real content.
  await writeFile(path.join(oldDir, '中文.md'), '--old\n---\n');
  await writeFile(path.join(newDir, '中文.md'), '++new\n+++\n');
  let diff;
  try { diff = execFileSync('git', ['-c', 'core.quotePath=true', 'diff', '--no-index', '--no-ext-diff', oldDir, newDir], { encoding: 'utf8' }); }
  catch (error) { if (error.status !== 1) throw error; diff = String(error.stdout); }
  await writeFile(path.join(output, 'actual-git.diff'), diff);
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(String(error)));

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
    ['task', { content: '[]', input: { action: 'collect' } }, '暂无完成结果'],
    ['task', { content: JSON.stringify({ state: 'waiting', delivery: 'automatic', jobs: [], pending: [{ id: 'child', state: 'running' }] }), input: { action: 'wait', id: 'child' } }, '等待结果 · 完成后自动唤醒'],
    ['task', { content: JSON.stringify({ job: { id: 'child', state: 'running', result_available: false, result: '' } }), input: { action: 'get', id: 'child' } }, '运行中'],
    ['task', { content: JSON.stringify([{ id: 'child', state: 'done', result: 'Actual evidence' }]), input: { action: 'wait' } }, '已完成'],
    ['task', { content: JSON.stringify([{ id: 'child', state: 'failed', result: 'Actual failure' }]), input: { action: 'collect' } }, '失败'],
  ]) check(`${name} control: ${expected}`, (await summarize(name, ctx)).text, expected);
  const timed = await page.evaluate(async () => {
    const { toolResultSummary, withToolDuration } = await import('/05-tool-summary.js');
    return withToolDuration(toolResultSummary('git', { content: '## main', input: { action: 'status' }, roots: [] }), 2200).text;
  });
  check('Late duration decorates the existing summary once', timed, '工作区干净 · 2.2s');
  const taskWaitArg = await page.evaluate(async () => {
    const { toolArgSummary } = await import('/05-tool-summary.js');
    return toolArgSummary('task', { action: 'wait', id: 'child-id' }, []);
  });
  check('Task wait shows its action and keeps the task identity in its tooltip', { text: taskWaitArg.text, title: taskWaitArg.title }, { text: '等待结果', title: 'wait child-id' });
  check('Browser module graph has no runtime errors', errors, []);

}
