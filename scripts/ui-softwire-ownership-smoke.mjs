// Complete production ESM owners: shared composer, native scope cache, run controls.
// --before loads their exact committed pre-fix sources without altering production.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';

const root = resolve(import.meta.dirname, '..');
const baseline = '746b69a824f38b4d5fc5502c4ec9ff78bc06e956';
const before = process.argv.includes('--before');
const sourceNames = ['25-softwire-composer.js', '25-softwire-run.js', '25-softwire-model.js', '03-workspaces.js'];
const sources = new Map(sourceNames.map(name => [name, before ? execFileSync('git', ['show', `${baseline}:crates/kanzei-app/ui/${name}`], { cwd: root, encoding: 'utf8' }) : readFileSync(resolve(root, 'crates/kanzei-app/ui', name), 'utf8').replace(/\r\n/g, '\n')]));
const fingerprints = Object.fromEntries([...sources].map(([name, body]) => [name, createHash('sha256').update(body).digest('hex')]));
const drain = async () => { await new Promise(done => setImmediate(done)); await new Promise(done => setImmediate(done)); };
const deferred = () => { let accept, reject; const promise = new Promise((yes, no) => { accept = yes; reject = no; }); return { promise, accept, reject }; };

class Element {
  constructor(tag = 'div') {
    this.tagName = tag.toUpperCase(); this.children = []; this.parentNode = null; this.dataset = {}; this.listeners = new Map(); this.attrs = new Map(); this.classes = new Set(); this.value = ''; this.disabled = false; this.hidden = false; this.text = '';
    this.style = { height: '', setProperty() {} };
    this.classList = { add: (...names) => names.forEach(name => this.classes.add(name)), remove: (...names) => names.forEach(name => this.classes.delete(name)), contains: name => this.classes.has(name), toggle: (name, value) => { value ??= !this.classes.has(name); value ? this.classes.add(name) : this.classes.delete(name); return value; } };
  }
  set className(value) { this.classes = new Set(value.split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classes].join(' '); }
  get parentElement() { return this.parentNode; }
  get firstChild() { return this.children[0]; }
  get lastChild() { return this.children.at(-1); }
  set textContent(value) { this.text = String(value); this.children = []; }
  get textContent() { return this.children.length ? this.children.map(child => child.textContent).join('') : this.text; }
  remove() { if (this.parentNode) this.parentNode.children.splice(this.parentNode.children.indexOf(this), 1); this.parentNode = null; }
  append(...nodes) { for (const child of nodes) { child.remove(); this.children.push(child); child.parentNode = this; } }
  prepend(...nodes) { for (const child of [...nodes].reverse()) { child.remove(); this.children.unshift(child); child.parentNode = this; } }
  insertBefore(child, before) { child.remove(); const index = this.children.indexOf(before); this.children.splice(index < 0 ? this.children.length : index, 0, child); child.parentNode = this; }
  before(node) { this.parentNode.insertBefore(node, this); }
  after(node) { const parent = this.parentNode; node.remove(); parent.children.splice(parent.children.indexOf(this) + 1, 0, node); node.parentNode = parent; }
  setAttribute(key, value) { this.attrs.set(key, String(value)); }
  getAttribute(key) { return this.attrs.get(key); }
  addEventListener(type, listener) { const list = this.listeners.get(type) ?? []; list.push(listener); this.listeners.set(type, list); }
  dispatchEvent(event) { for (const listener of this.listeners.get(event.type) ?? []) listener(event); return true; }
  getBoundingClientRect() { return { left: 0, top: 0, width: 600, height: 200 }; }
  focus() {}
  querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; }
  querySelectorAll(selector) { const results = []; const matches = element => selector.startsWith('#') ? element.id === selector.slice(1) : selector.startsWith('.') ? element.classes.has(selector.slice(1)) : element.tagName.toLowerCase() === selector; const visit = element => { for (const child of element.children) { if (matches(child)) results.push(child); visit(child); } }; visit(this); return results; }
}

async function harness({ storage = new Map() } = {}) {
  const elements = new Map(), timers = new Map(), calls = [], closeCalls = [], pending = [], configurations = new Map();
  const get = id => { if (!elements.has(id)) { const node = new Element(); node.id = id; elements.set(id, node); } return elements.get(id); };
  const body = new Element('body'), native = new Element(), toolbar = new Element(), slot = new Element();
  body.append(native, toolbar, slot); native.append(get('composer')); get('composer').append(get('prompt'), get('send'));
  toolbar.append(get('model-picker-group'), get('auto-allow-wrap'), get('autorun-menu'), get('continue-panel'));
  const documentListeners = new Map();
  const document = { body, createElement: tag => new Element(tag), createComment: text => { const node = new Element('comment'); node.textContent = text; return node; }, addEventListener(type, callback) { const callbacks = documentListeners.get(type) ?? []; callbacks.push(callback); documentListeners.set(type, callbacks); }, dispatchEvent(event) { for (const callback of documentListeners.get(event.type) ?? []) callback(event); return true; } };
  const node = (tag, text, className = '') => { const result = document.createElement(tag); if (text != null) result.textContent = String(text); if (className) result.className = className; return result; };
  const button = (text, action, className = 'sw-link') => { const result = node('button', text, className); result.type = 'button'; result.addEventListener('click', action); return result; };
  const shellValues = { attachments: [], activeProcessId: 'p-A', currentProject: 'C:/project-A', processItems: [], navigate_view() {}, toastError() {}, sessionState: () => ({ phase: 'idle' }) };
  const context = vm.createContext({ document, window: { innerWidth: 1200, innerHeight: 900, addEventListener() {} }, console, Event: class { constructor(type) { this.type = type; } }, CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init?.detail; } }, localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) }, setTimeout: callback => { const id = timers.size + 1; timers.set(id, callback); return id; }, clearTimeout: id => timers.delete(id) });
  const shell = new vm.SyntheticModule([...Object.keys(shellValues), 'setAttachments'], function () { for (const [name, value] of Object.entries(shellValues)) this.setExport(name, value); this.setExport('setAttachments', value => { shellValues.attachments = value; this.setExport('attachments', value); }); }, { context });
  const synthetic = (values) => new vm.SyntheticModule(Object.keys(values), function () { for (const [name, value] of Object.entries(values)) this.setExport(name, value); }, { context });
  const dependencies = new Map([
    ['./01-core.js', synthetic({ $: get, defer() {}, invoke: async () => null, mergeWorkspaceState: (prior, delta) => ({ ...prior, ...delta }), readJson: (_key, fallback) => fallback, uiPrefsLoad: async () => ({}), uiPrefsSave: async () => null, writeJson() {} })], ['./02-i18n.js', synthetic({ t: value => value })], ['./03-shell.js', shell],
    ['./00-frame.js', synthetic({ installDragHandle() {} })],
    ['./04-structured-parse.js', synthetic({ stripInternalHandoff: value => value, fillTemplate: (value, fields) => value.replace(/\{(\w+)\}/g, (_, key) => fields[key]) })],
    ['./00-surface.js', synthetic({ closeSurface: menu => { closeCalls.push(menu); }, openMenu: (_anchor, items) => { calls.push({ type: 'menu', items }); }, openPopover: (anchor, menu) => { calls.push({ type: 'popover', anchor, menu }); } })],
    ['./08-auto.js', synthetic({ lineAgent: () => ({ agent: 'dev' }), currentAutoRounds: () => 0, autoContinueTimers: new Map(), awaitingUserSessions: new Set() })],
    ['./08-compose-runtime.js', synthetic({ renderAttachments() {}, lineAutoConfig: id => configurations.get(id) ?? { enabled: false, paused: false, stopAfterRound: false }, setLineAutoState: (id, patch) => { const gate = deferred(); pending.push({ id, patch: { ...patch }, ...gate }); return gate.promise; }, processUpdateQueues: new Map() })],
    ['./25-softwire-view.js', synthetic({ node, button, lineLabel: value => value })],
    ['./09-sessions.js', synthetic({ activate_execution_root() {}, processSwitchGeneration: 0, refreshProcesses() {}, renderParallelTaskStatus() {}, switchProcess() {} })],
    ['./12-workbench.js', synthetic({ workbenchNavigationGuard: () => () => true })],
    ['./19-research.js', synthetic({ sync_research_process_context() {} })],
    ['./03-research-library.js', synthetic({ enter_research_library() {}, research_topic_label: value => value })],
    ['./03-general-scope.js', synthetic({ isGeneralChat: () => false, syncGeneralChatView() {} })],
  ]);
  const modules = new Map(sourceNames.map(name => [`./${name}`, new vm.SourceTextModule(sources.get(name), { context, identifier: name })]));
  const linker = specifier => modules.get(specifier) ?? dependencies.get(specifier) ?? (() => { throw new Error(`missing dependency ${specifier}`); })();
  for (const module of modules.values()) if (module.status === 'unlinked') await module.link(linker);
  for (const module of modules.values()) await module.evaluate();
  const setShell = fields => { for (const [key, value] of Object.entries(fields)) { shellValues[key] = value; shell.setExport(key, value); } };
  const composer = modules.get('./25-softwire-composer.js').namespace.createComposer({ stop() {}, back() {}, changed() {} });
  const workspace = modules.get('./03-workspaces.js').namespace;
  // Exact controller hook; scope switching itself is the complete real owner.
  document.addEventListener('kz:before-composer-scope', () => composer.leave());
  workspace.sync_composer_scope();
  const line = { project: shellValues.currentProject, id: 'p-A', session_id: 's-A', label: 'A', profile: 'dev' };
  const resolved = [];
  const runner = modules.get('./25-softwire-run.js').namespace.createRunControl({ resolve: async destination => { resolved.push({ ...destination }); return destination; }, changed() {} });
  const target = (module = 'main', fields = {}) => ({ project: shellValues.currentProject, processId: shellValues.activeProcessId, sessionId: 's-A', module, label: 'A', ...fields });
  const switchScope = (project, processId) => { setShell({ currentProject: project, activeProcessId: processId }); workspace.sync_composer_scope(); };
  return { composer, runner, workspace, switchScope, get, slot, target, line, setShell, shellValues, storage, calls, pending, resolved, closeCalls, configurations };
}

const results = [];
async function check(name, expectedBefore, run) { try { const evidence = await run(); results.push({ name, expected_before: expectedBefore, passed: true, evidence }); } catch (error) { results.push({ name, expected_before: expectedBefore, passed: false, code: error.code ?? '', message: error.message, actual: error.actual, expectedValue: error.expected }); } }

await check('normal main leave keeps its edited native draft and attachments', 'PASS', async () => {
  const h = await harness(); h.get('prompt').value = 'original main'; h.composer.mount(h.slot, h.target()); h.composer.replace('new main'); h.shellValues.attachments = [{ file_name: 'main.txt' }]; h.setShell({ attachments: h.shellValues.attachments }); h.composer.leave(); assert.equal(h.get('prompt').value, 'new main'); assert.equal(h.shellValues.attachments[0].file_name, 'main.txt'); return { native: h.get('prompt').value };
});
await check('module draft must not replace the borrowed native main draft', 'FAIL', async () => {
  const h = await harness(); h.get('prompt').value = 'main instruction retained'; h.composer.mount(h.slot, h.target()); h.composer.mount(h.slot, h.target('memory')); h.composer.replace('memory manager instruction'); h.composer.leave(); assert.equal(h.get('prompt').value, 'main instruction retained'); return { native: h.get('prompt').value };
});
await check('same module drafts stay distinct when switching before leave', 'PASS', async () => {
  const h = await harness(); h.get('prompt').value = 'main instruction retained'; h.composer.mount(h.slot, h.target()); h.composer.mount(h.slot, h.target('memory')); h.composer.replace('memory manager instruction'); h.composer.mount(h.slot, h.target()); assert.equal(h.get('prompt').value, 'main instruction retained'); h.composer.mount(h.slot, h.target('memory')); assert.equal(h.get('prompt').value, 'memory manager instruction'); return { restored: h.get('prompt').value };
});
await check('ACK after leaving must clear the unchanged sent native draft', 'FAIL', async () => {
  const h = await harness(); h.get('prompt').value = 'submitted text'; h.composer.mount(h.slot, h.target()); const capture = h.composer.capture(); h.composer.leave(); h.composer.clear(capture); assert.equal(h.get('prompt').value, ''); return { native: h.get('prompt').value };
});
await check('ACK after leaving must not resurrect the sent draft on reopening', 'FAIL', async () => {
  const h = await harness(); h.get('prompt').value = 'submitted text'; h.composer.mount(h.slot, h.target()); const capture = h.composer.capture(); h.composer.leave(); h.composer.clear(capture); h.composer.mount(h.slot, h.target()); assert.equal(h.get('prompt').value, ''); return { reopened: h.get('prompt').value };
});
await check('normal visible ACK clears exactly captured unchanged input', 'PASS', async () => {
  const h = await harness(); h.get('prompt').value = 'submitted text'; h.composer.mount(h.slot, h.target()); const capture = h.composer.capture(); h.composer.clear(capture); assert.equal(h.get('prompt').value, ''); return { visible: h.get('prompt').value };
});
await check('ACK preserves a new native edit after leaving', 'PASS', async () => {
  const h = await harness(); h.get('prompt').value = 'submitted text'; h.composer.mount(h.slot, h.target()); const capture = h.composer.capture(); h.composer.leave(); h.get('prompt').value = 'new instruction'; h.get('prompt').dispatchEvent({ type: 'input' }); h.composer.clear(capture); assert.equal(h.get('prompt').value, 'new instruction'); return { native: h.get('prompt').value };
});
await check('main edit followed by module leave restores that latest main edit', 'FAIL', async () => {
  const h = await harness(); h.get('prompt').value = 'original main'; h.composer.mount(h.slot, h.target()); h.composer.replace('edited main'); h.composer.mount(h.slot, h.target('tools')); h.composer.replace('tools instruction'); h.composer.leave(); assert.equal(h.get('prompt').value, 'edited main'); return { native: h.get('prompt').value };
});
await check('cross-project ACK clears only original native cached scope, preserving B', 'FAIL', async () => {
  const h = await harness(); h.get('prompt').value = 'submitted A'; h.setShell({ attachments: [{ file_name: 'A.txt' }] }); h.composer.mount(h.slot, h.target()); const capture = h.composer.capture(); h.switchScope('C:/project-B', 'p-B'); h.get('prompt').value = 'new B'; h.setShell({ attachments: [{ file_name: 'B.txt' }] }); h.composer.clear(capture); assert.equal(h.get('prompt').value, 'new B'); assert.equal(h.shellValues.attachments[0].file_name, 'B.txt'); h.switchScope('C:/project-A', 'p-A'); assert.equal(h.get('prompt').value, ''); assert.equal(h.shellValues.attachments.length, 0); h.switchScope('C:/project-B', 'p-B'); assert.equal(h.get('prompt').value, 'new B'); assert.equal(h.shellValues.attachments[0].file_name, 'B.txt'); return { A: 'cleared', B: h.get('prompt').value };
});
await check('cross-process ACK clears original A process cached scope', 'FAIL', async () => {
  const h = await harness(); h.get('prompt').value = 'submitted A1'; h.composer.mount(h.slot, h.target()); const capture = h.composer.capture(); h.switchScope('C:/project-A', 'p-A2'); h.get('prompt').value = 'new A2'; h.composer.clear(capture); h.switchScope('C:/project-A', 'p-A'); assert.equal(h.get('prompt').value, ''); h.switchScope('C:/project-A', 'p-A2'); assert.equal(h.get('prompt').value, 'new A2'); return { A1: 'cleared', A2: h.get('prompt').value };
});
await check('cross-project ACK preserves a newer edit cached for original A', 'PASS', async () => {
  const h = await harness(); h.get('prompt').value = 'submitted A'; h.composer.mount(h.slot, h.target()); const capture = h.composer.capture(); h.composer.leave(); h.get('prompt').value = 'new A after submission'; h.get('prompt').dispatchEvent({ type: 'input' }); h.switchScope('C:/project-B', 'p-B'); h.get('prompt').value = 'new B'; h.composer.clear(capture); h.switchScope('C:/project-A', 'p-A'); assert.equal(h.get('prompt').value, 'new A after submission'); h.composer.mount(h.slot, h.target()); assert.equal(h.get('prompt').value, 'new A after submission'); return { A: h.get('prompt').value };
});
await check('ACK protects new attachments even with unchanged native text', 'PASS', async () => {
  const h = await harness(); h.get('prompt').value = 'submitted A'; h.composer.mount(h.slot, h.target()); const capture = h.composer.capture(); h.composer.leave(); h.setShell({ attachments: [{ file_name: 'new.txt' }] }); h.composer.clear(capture); assert.equal(h.get('prompt').value, 'submitted A'); assert.equal(h.shellValues.attachments[0].file_name, 'new.txt'); return { A: h.get('prompt').value, attachments: h.shellValues.attachments };
});
await check('ACK for module draft does not clear matching native main text', 'PASS', async () => {
  const h = await harness(); h.get('prompt').value = 'same text distinct owner'; h.composer.mount(h.slot, h.target()); h.composer.mount(h.slot, h.target('memory')); h.composer.replace('same text distinct owner'); const capture = h.composer.capture(); h.composer.leave(); h.composer.clear(capture); assert.equal(h.get('prompt').value, 'same text distinct owner'); h.composer.mount(h.slot, h.target()); assert.equal(h.get('prompt').value, 'same text distinct owner'); return { native: h.get('prompt').value };
});
await check('ACK while different module mounted clears old main without changing module', 'FAIL', async () => {
  const h = await harness(); h.get('prompt').value = 'submitted main'; h.composer.mount(h.slot, h.target()); const capture = h.composer.capture(); h.composer.mount(h.slot, h.target('history')); h.composer.replace('new history'); h.composer.clear(capture); assert.equal(h.get('prompt').value, 'new history'); h.composer.leave(); assert.equal(h.get('prompt').value, ''); h.composer.mount(h.slot, h.target('history')); assert.equal(h.get('prompt').value, 'new history'); return { native: 'cleared', module: h.get('prompt').value };
});
await check('normal run toggle sends only chosen A identity and intent', 'PASS', async () => {
  const h = await harness(); h.runner.sync(h.line); h.runner.element.querySelector('#sw-run-toggle').dispatchEvent({ type: 'click' }); await drain(); assert.equal(h.pending.length, 1); assert.equal(h.pending[0].id, 'p-A'); assert.deepEqual(h.pending[0].patch, { enabled: true, paused: false, stopAfterRound: false }); h.pending[0].accept(); await drain(); assert.equal(h.runner.element.querySelector('.sw-run-status').dataset.error, 'false'); return { args: h.pending[0].patch, id: h.pending[0].id };
});
await check('A rejected control ACK must not display A failure on later selected B', 'FAIL', async () => {
  const h = await harness(); h.runner.sync(h.line); h.runner.element.querySelector('#sw-run-toggle').dispatchEvent({ type: 'click' }); await drain(); h.runner.sync({ project: 'C:/project-B', id: 'p-B', session_id: 's-B', label: 'B', profile: 'dev' }); h.pending[0].reject(new Error('A permission refused')); await drain(); const status = h.runner.element.querySelector('.sw-run-status'); assert.equal(status.dataset.error, 'false'); assert(!status.textContent.includes('A permission refused')); return { status: status.textContent };
});
await check('same selected A receives its rejection visibly', 'PASS', async () => {
  const h = await harness(); h.runner.sync(h.line); h.runner.element.querySelector('#sw-run-toggle').dispatchEvent({ type: 'click' }); await drain(); h.pending[0].reject(new Error('A permission refused')); await drain(); const status = h.runner.element.querySelector('.sw-run-status'); assert.equal(status.dataset.error, 'true'); assert(status.textContent.includes('A permission refused')); return { status: status.textContent };
});
console.log(JSON.stringify({ baseline, before, source_sha256_lf: fingerprints, execution: 'complete production ESM plus native scope cache with isolated DOM and controlled owner receipts; no native, browser, Rust, Cargo', results, passed: results.filter(row => row.passed).length, failed: results.filter(row => !row.passed).length }, null, 2));
if (results.some(row => !row.passed)) process.exitCode = 1;
