// Complete ESM stage/model owners plus the real core event router and session state machine.
// --before loads the exact committed pre-fix modules without changing production files.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';

const root = resolve(import.meta.dirname, '..');
const baseline = '26acb7f0ce2d9643826cc9229f430f84e57c13f0';
const before = process.argv.includes('--before');
const names = ['03-session-stage.js', '12-conversation-model.js', '01-core.js', '03-shell.js'];
const sources = new Map(names.map(name => [name, before
  ? execFileSync('git', ['show', `${baseline}:crates/kanzei-app/ui/${name}`], { cwd: root, encoding: 'utf8' })
  : readFileSync(resolve(root, 'crates/kanzei-app/ui', name), 'utf8').replace(/\r\n/g, '\n')]));
const source_sha256_lf = Object.fromEntries([...sources].map(([name, text]) => [name, createHash('sha256').update(text).digest('hex')]));
const pure = async name => (await import(`data:text/javascript;base64,${Buffer.from(sources.get(name)).toString('base64')}`));
const stage = await pure('03-session-stage.js');
const model = await pure('12-conversation-model.js');
const results = [];
const toolCount = state => state.stage_tools?.size ?? 0;
async function check(name, expectedBefore, run) {
  try { results.push({ name, expected_before: expectedBefore, passed: true, evidence: await run() }); }
  catch (error) { results.push({ name, expected_before: expectedBefore, passed: false, code: error.code ?? '', message: error.message, actual: error.actual, expected: error.expected }); }
}
const initial = () => ({ stage: '等待模型', detail: '', resume_stage: '等待模型' });
const apply = (state, event, payload = {}) => stage.updateSessionStage(state, `kz:${event}`, payload);

await check('parallel first end retains the remaining tool activity/name', 'FAIL', () => {
  const state = initial(); apply(state, 'tool-start', { id: 'fast', name: 'read_file' }); apply(state, 'tool-start', { id: 'slow', name: 'bash' });
  apply(state, 'tool-end', { id: 'fast', name: 'read_file' });
  assert.equal(state.stage, '工具执行中'); assert.equal(state.detail, 'bash'); return { stage: state.stage, detail: state.detail };
});
await check('real progress id/chunk payload retains its own tool name', 'FAIL', () => {
  const state = initial(); apply(state, 'tool-start', { id: 'shell', name: 'bash' }); apply(state, 'tool-progress', { id: 'shell', chunk: 'stdout' });
  assert.equal(state.detail, 'bash'); assert.equal(state.stage, '工具执行中'); return { detail: state.detail };
});
await check('parallel progress selects its id rather than the most recent tool', 'FAIL', () => {
  const state = initial(); apply(state, 'tool-start', { id: 'shell', name: 'bash' }); apply(state, 'tool-start', { id: 'file', name: 'read_file' });
  apply(state, 'tool-progress', { id: 'shell', chunk: 'stdout' }); assert.equal(state.detail, 'bash'); return { detail: state.detail };
});
await check('serial final end waits for the next provider message', 'PASS', () => {
  const state = initial(); apply(state, 'tool-start', { id: 'single', name: 'read_file' }); apply(state, 'tool-end', { id: 'single', name: 'read_file' });
  assert.equal(state.stage, '等待模型'); assert.equal(state.detail, ''); assert.equal(toolCount(state), 0); return { stage: state.stage };
});
await check('parallel last end clears activity after the whole batch', 'PASS', () => {
  const state = initial(); apply(state, 'tool-start', { id: 'a', name: 'read_file' }); apply(state, 'tool-start', { id: 'b', name: 'bash' });
  apply(state, 'tool-end', { id: 'a', name: 'read_file' }); apply(state, 'tool-end', { id: 'b', name: 'bash' });
  assert.equal(state.stage, '等待模型'); assert.equal(state.detail, ''); assert.equal(toolCount(state), 0); return { stage: state.stage };
});
await check('next turn removes unresolved tools from the preceding turn', 'PASS', () => {
  const state = initial(); apply(state, 'tool-start', { id: 'old', name: 'bash' }); apply(state, 'turn', { step: 2 });
  apply(state, 'tool-start', { id: 'new', name: 'read_file' }); apply(state, 'tool-end', { id: 'new', name: 'read_file' });
  assert.equal(state.stage, '等待模型'); assert.equal(toolCount(state), 0); return { stage: state.stage };
});
await check('permission audit does not replace live tool activity', 'PASS', () => {
  const state = initial(); apply(state, 'tool-start', { id: 'shell', name: 'bash' });
  apply(state, 'status', { stage: '权限', detail: '已记住允许规则' }); apply(state, 'permission-resolved', { id: 'request', allowed: true });
  assert.equal(state.stage, '工具执行中'); assert.equal(state.detail, 'bash'); assert.equal(state.permission_notice, '已记住允许规则');
  apply(state, 'tool-end', { id: 'shell', name: 'bash' }); assert.equal(state.stage, '等待模型'); return { notice: state.permission_notice };
});
await check('permission resolution restores its actual previous stage', 'PASS', () => {
  const state = { stage: '权限确认中', detail: 'waiting', resume_stage: '工具执行中' };
  apply(state, 'permission-resolved', { allowed: false }); assert.equal(state.stage, '工具执行中'); assert.equal(state.detail, ''); return { stage: state.stage };
});
await check('production completed/pruned/fallback compaction details do not claim ongoing work', 'PASS', () => {
  const details = ['上下文约 9000 tokens, 就地压缩为 3000 tokens', '已机械清理 8 条中段消息，未动 LLM 纪要', '中段为空压不动,保留原历史', '压缩完成: 9000 → 3000'];
  for (const detail of details) { const state = initial(); assert.equal(stage.compactionInProgress({ stage: '压缩', detail }), false); apply(state, 'status', { stage: '压缩', detail }); assert.equal(state.stage, '等待模型'); }
  assert.equal(stage.compactionInProgress({ stage: '压缩', detail: '正在生成纪要' }), true); return { details };
});
await check('nonterminal error preserves the still-live tool projection', 'PASS', () => {
  const state = initial(); apply(state, 'tool-start', { id: 'live', name: 'bash' }); apply(state, 'error', { terminal: false, message: 'round failure' });
  assert.equal(state.stage, '工具执行中'); assert.equal(state.detail, 'bash'); return { stage: state.stage };
});

class Element {
  constructor(tag = 'div') { this.tagName = tag.toUpperCase(); this.children = []; this.dataset = {}; this.style = { setProperty() {} }; this.attributes = new Map(); this.classes = new Set(); this.textContent = ''; this.value = ''; this.parentNode = null; this.classList = { add: (...names) => names.forEach(name => this.classes.add(name)), remove: (...names) => names.forEach(name => this.classes.delete(name)), contains: name => this.classes.has(name), toggle: (name, on) => { on ??= !this.classes.has(name); on ? this.classes.add(name) : this.classes.delete(name); return on; } }; }
  set className(value) { this.classes = new Set(value.split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classes].join(' '); }
  get childElementCount() { return this.children.length; }
  get firstElementChild() { return this.children[0]; }
  appendChild(child) { this.children.push(child); child.parentNode = this; return child; }
  append(...children) { children.forEach(child => this.appendChild(child)); }
  remove() { if (this.parentNode) this.parentNode.children.splice(this.parentNode.children.indexOf(this), 1); }
  addEventListener() {}
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  querySelector(selector) { return this.children.find(child => selector.startsWith('.') && child.classList.contains(selector.slice(1))) ?? null; }
  querySelectorAll() { return []; }
}

async function harness() {
  const elements = new Map(), timers = [], listeners = new Map(), handled = [];
  const get = id => { if (!elements.has(id)) { const el = new Element(); el.id = id; elements.set(id, el); } return elements.get(id); };
  const placeholder = new Element(); placeholder.className = 'msg-pane'; placeholder.dataset.sessionId = ''; get('messages').appendChild(placeholder);
  const document = { readyState: 'loading', title: 'Kanzei test', hidden: false, body: new Element('body'), documentElement: new Element('html'), getElementById: get, createElement: tag => new Element(tag), addEventListener() {}, dispatchEvent() {}, hasFocus: () => true, querySelector: () => null, querySelectorAll: () => [] };
  const window = { addEventListener() {}, innerWidth: 1200, innerHeight: 800, __TAURI__: { core: { invoke: async () => null }, event: { listen: async (event, callback) => { listeners.set(event, callback); return () => listeners.delete(event); } } } };
  // Defer callbacks stay behind DOMContentLoaded; only explicit core on() registrations run.
  const context = vm.createContext({ document, window, console: { ...console }, localStorage: { getItem: () => null, setItem() {} }, CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init?.detail; } }, setTimeout: callback => { timers.push(callback); return timers.length; }, clearTimeout() {}, setInterval: () => 1, clearInterval() {}, performance: { now: () => 0 } });
  const modules = new Map(names.map(name => [`./${name}`, new vm.SourceTextModule(sources.get(name), { context, identifier: name })]));
  const imported = new Map();
  for (const text of sources.values()) for (const match of text.matchAll(/import\s*\{([^}]+)\}\s*from\s*["']([^"']+)["']/g)) {
    if (modules.has(match[2])) continue;
    const exports = imported.get(match[2]) ?? new Set();
    for (const entry of match[1].split(',')) { const name = entry.trim().split(/\s+as\s+/)[0]; if (name) exports.add(name); }
    imported.set(match[2], exports);
  }
  const values = { t: value => value, localizeDynamic: value => value, I18N_EN: {}, autoContinueTimers: new Map(), takeAwaitingUser: () => false, autoStopReason: '', state: () => {}, active_space: 'dev', view_allowed: () => true, workbenchProject: null, lastProjectPrefs: {}, lastWorkspaceSnapshot: null, prefObject: () => ({}), layoutPref: () => undefined, parseErrorText: () => ({ json: false, chain: [] }), surfaceElements: () => [], isGeneralChat: () => false };
  const external = new Map([...imported].map(([specifier, exports]) => [specifier, new vm.SyntheticModule([...exports], function () { for (const name of exports) this.setExport(name, Object.hasOwn(values, name) ? values[name] : () => {}); }, { context })]));
  const linker = specifier => modules.get(specifier) ?? external.get(specifier);
  await modules.get('./01-core.js').link(linker);
  await modules.get('./01-core.js').evaluate();
  const core = modules.get('./01-core.js').namespace, shell = modules.get('./03-shell.js').namespace;
  shell.setCurrentProject('C:/fixture-project'); shell.setActiveSessionId('A');
  for (const event of ['turn', 'tool-start', 'tool-progress', 'tool-end', 'status', 'permission-resolved', 'done', 'idle', 'stopped', 'error', 'text']) core.on(`kz:${event}`, envelope => handled.push({ event, sessionId: envelope.payload.sessionId }));
  while (timers.length) timers.shift()();
  return { core, shell, handled, emit: (event, sessionId, payload = {}) => { const listener = listeners.get(`kz:${event}`); assert.ok(listener, `registered ${event}`); listener({ payload: { ...payload, sessionId } }); }, state: sid => shell.sessionState(sid) };
}

await check('actual core router retains parallel background activity before the final end', 'FAIL', async () => {
  const h = await harness(); h.emit('turn', 'B', { step: 1 }); h.emit('tool-start', 'B', { id: 'fast', name: 'read_file' }); h.emit('tool-start', 'B', { id: 'slow', name: 'bash' }); h.emit('tool-end', 'B', { id: 'fast', name: 'read_file' });
  assert.equal(h.state('B').stage, '工具执行中'); assert.equal(h.state('B').detail, 'bash'); assert.equal(h.state('B').phase, 'running'); h.emit('tool-end', 'B', { id: 'slow', name: 'bash' }); assert.equal(h.state('B').stage, '等待模型'); return { phase: h.state('B').phase, finalStage: h.state('B').stage };
});
await check('core background progress projects name even though its visual handler is not routed', 'FAIL', async () => {
  const h = await harness(); h.emit('turn', 'B', { step: 1 }); h.emit('tool-start', 'B', { id: 'shell', name: 'bash' }); const count = h.handled.length; h.emit('tool-progress', 'B', { id: 'shell', chunk: 'stdout' });
  assert.equal(h.handled.length, count); assert.equal(h.state('B').detail, 'bash'); return { backgroundVisualCalls: 0, detail: h.state('B').detail };
});
await check('same tool ids in two sessions keep separate activity and names', 'FAIL', async () => {
  const h = await harness(); h.emit('turn', 'A', { step: 1 }); h.emit('turn', 'B', { step: 1 }); h.emit('tool-start', 'A', { id: 'shared', name: 'bash' }); h.emit('tool-start', 'B', { id: 'shared', name: 'read_file' }); h.emit('tool-progress', 'A', { id: 'shared', chunk: 'stdout' });
  assert.equal(h.state('A').detail, 'bash'); assert.equal(h.state('B').detail, 'read_file'); h.emit('tool-end', 'B', { id: 'shared', name: 'read_file' }); assert.equal(h.state('A').stage, '工具执行中'); assert.equal(h.state('B').stage, '等待模型'); return { A: h.state('A').stage, B: h.state('B').stage };
});
await check('core permission status and resolution preserve the live tool contract', 'PASS', async () => {
  const h = await harness(); h.emit('turn', 'A', { step: 1 }); h.emit('tool-start', 'A', { id: 'shell', name: 'bash' }); h.emit('status', 'A', { stage: '权限', detail: '规则已记住' }); h.emit('permission-resolved', 'A', { allowed: true });
  assert.equal(h.state('A').stage, '工具执行中'); assert.equal(h.state('A').detail, 'bash'); h.emit('tool-end', 'A', { id: 'shell', name: 'bash' }); assert.equal(h.state('A').stage, '等待模型'); return { notice: h.state('A').permission_notice };
});
await check('core stopping-to-terminal clears tools without permitting late progress to revive it', 'PASS', async () => {
  const phases = [];
  for (const terminal of ['idle', 'stopped', 'error']) {
    const h = await harness(); h.emit('turn', 'A', { step: 1 }); h.emit('tool-start', 'A', { id: 'unfinished', name: 'bash' }); h.shell.transitionSession('A', 'stopping'); h.emit(terminal, 'A', { message: 'terminal failure' });
    assert.equal(toolCount(h.state('A')), 0); assert.equal(h.state('A').stage, '空闲'); assert.equal(h.state('A').detail, ''); const phase = h.state('A').phase;
    h.emit('tool-progress', 'A', { id: 'unfinished', chunk: 'late' }); assert.equal(h.state('A').phase, phase); assert.equal(toolCount(h.state('A')), 0); phases.push(phase);
  }
  assert.deepEqual(phases, ['idle', 'stopped', 'failed']); return { phases };
});
await check('done followed by converged auto-pending idle clears tools and keeps its state owner', 'PASS', async () => {
  const h = await harness(); h.emit('turn', 'A', { step: 1 }); h.emit('tool-start', 'A', { id: 'unfinished', name: 'bash' }); h.emit('done', 'A'); assert.equal(toolCount(h.state('A')), 0);
  h.shell.transitionSession('A', 'auto_pending'); h.emit('idle', 'A'); assert.equal(h.state('A').phase, 'auto_pending'); assert.equal(h.state('A').converged, true); assert.equal(toolCount(h.state('A')), 0); return { phase: h.state('A').phase };
});
await check('core nonterminal error retains active tools; terminal error closes only its own session', 'PASS', async () => {
  const h = await harness(); for (const sid of ['A', 'B']) { h.emit('turn', sid, { step: 1 }); h.emit('tool-start', sid, { id: 'tool', name: sid === 'A' ? 'bash' : 'read_file' }); }
  h.emit('error', 'A', { terminal: false, message: 'round failure' }); assert.equal(h.state('A').phase, 'running'); assert.equal(h.state('A').detail, 'bash'); h.emit('error', 'A', { terminal: true, message: 'run failure' });
  assert.equal(h.state('A').phase, 'failed'); assert.equal(toolCount(h.state('A')), 0); assert.equal(h.state('B').phase, 'running'); assert.equal(h.state('B').detail, 'read_file'); return { A: h.state('A').phase, B: h.state('B').phase };
});
await check('core new turn discards prior unresolved tools before the new serial batch', 'PASS', async () => {
  const h = await harness(); h.emit('turn', 'A', { step: 1 }); h.emit('tool-start', 'A', { id: 'old', name: 'bash' }); h.emit('done', 'A'); h.shell.transitionSession('A', 'auto_pending'); h.emit('turn', 'A', { step: 2 }); h.emit('tool-start', 'A', { id: 'new', name: 'read_file' }); h.emit('tool-end', 'A', { id: 'new', name: 'read_file' });
  assert.equal(h.state('A').stage, '等待模型'); assert.equal(h.state('A').converged, false); assert.equal(toolCount(h.state('A')), 0); return { stage: h.state('A').stage, phase: h.state('A').phase };
});

await check('conversation records match active/closed capability and numeric millisecond contracts', 'PASS', () => {
  const item = { id: 'p2|C:/project', session_id: 'canonical-session', updated_at: 1791000000123, worktree_path: 'C:/tree', branch: 'feature', title_custom: true };
  const active = model.conversationRecord('C:/project', item, { kind: 'main', name: 'Main' }); const closed = model.conversationRecord('C:/project', item, { closed: true, name: 'Retained' });
  assert.equal(active.identity, item.session_id); assert.equal(active.execution, item); assert.equal(active.updatedAt, item.updated_at); assert.equal(active.ordinal, 2); assert.equal(active.capabilities.delete, false);
  assert.equal(closed.execution, null); assert.equal(closed.capabilities.send, false); assert.equal(closed.capabilities.delete, true); assert.equal(closed.worktree, ''); assert.equal(closed.branch, ''); return { timestamp: active.updatedAt, closed: closed.capabilities };
});
await check('active-first dedup keeps actual session identity and distinct retained history', 'PASS', () => {
  const active = model.conversationRecord('P', { id: 'p1', session_id: 'same' }, { kind: 'main', name: 'Active' }); const duplicate = model.conversationRecord('P', { id: 'p1', session_id: 'same' }, { closed: true, name: 'Closed duplicate' }); const retained = model.conversationRecord('P', { id: 'p2', session_id: 'retained' }, { closed: true, name: 'History' });
  const rows = model.uniqueConversations([active, duplicate, retained]); assert.equal(rows.length, 2); assert.equal(rows[0], active); assert.equal(rows[1], retained); return { identities: rows.map(row => row.identity) };
});
await check('conversation search uses visible names/project labels rather than hidden runtime identity', 'PASS', () => {
  const row = model.conversationRecord('C:/secret-path', { id: 'hidden-id', session_id: 'hidden-session' }, { kind: 'main', name: 'Visible topic' });
  assert.equal(model.matchesConversation(row, 'visible project', 'Project label'), true); assert.equal(model.matchesConversation(row, 'hidden-id', 'Project label'), false); assert.equal(model.matchesConversation(row, 'secret-path', 'Project label'), false); assert.equal(model.matchesConversation(row, 'no-match', 'Project label'), false); return { visibleMatch: true };
});

console.log(JSON.stringify({ mode: before ? 'before' : 'current', baseline, source_sha256_lf, results, passed: results.filter(item => item.passed).length, failed: results.filter(item => !item.passed).length }, null, 2));
if (results.some(item => !item.passed)) process.exitCode = 1;
