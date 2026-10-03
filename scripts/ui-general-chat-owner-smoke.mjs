// Complete general scope/controller and real navigation/session/workspace owners.
// IPC/DOM/other views are isolated fixtures; --before reads exact committed sources.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import vm from 'node:vm';
const root = resolve(import.meta.dirname, '..');
const baseline = '26acb7f0ce2d9643826cc9229f430f84e57c13f0';
const before = process.argv.includes('--before');
const names = ['03-general-scope.js', '29-general-chat.js', '03-workspaces.js', '09-sessions.js', '12-workbench.js', '15-views-misc.js'];
const sources = new Map(names.map(name => [name, before ? execFileSync('git', ['show', `${baseline}:crates/kanzei-app/ui/${name}`], { cwd: root, encoding: 'utf8' }) : readFileSync(resolve(root, 'crates/kanzei-app/ui', name), 'utf8').replace(/\r\n/g, '\n')]));
const fingerprint = Object.fromEntries([...sources].map(([name, text]) => [name, createHash('sha256').update(text).digest('hex')]));
const GENERAL = 'C:/fixture-home/conversations/general', PROJECT = 'C:/fixture-project';
const pause = () => { let accept, reject; const promise = new Promise((yes, no) => { accept = yes; reject = no; }); return { promise, accept, reject }; };
const flush = async () => { await new Promise(yes => setImmediate(yes)); await new Promise(yes => setImmediate(yes)); };
class Element {
  constructor(tag = 'div') { this.tagName = tag.toUpperCase(); this.children = []; this.dataset = {}; this.listeners = new Map(); this.attributes = new Map(); this.classes = new Set(); this.ownText = ''; this.value = ''; this.disabled = false; this.options = []; this.style = { setProperty() {} }; this.classList = { add: (...names) => names.forEach(name => this.classes.add(name)), remove: (...names) => names.forEach(name => this.classes.delete(name)), contains: name => this.classes.has(name), toggle: (name, on) => { on ??= !this.classes.has(name); on ? this.classes.add(name) : this.classes.delete(name); return on; } }; }
  set className(value) { this.classes = new Set(value.split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classes].join(' '); }
  set textContent(value) { this.ownText = String(value); this.children = []; }
  get textContent() { return this.ownText + this.children.map(child => child.textContent).join(''); }
  get firstChild() { return this.children[0]; }
  get nextSibling() { return this.parentNode?.children[this.parentNode.children.indexOf(this) + 1]; }
  append(...nodes) { for (const node of nodes) { this.children.push(node); node.parentNode = this; } }
  appendChild(node) { this.append(node); return node; }
  prepend(...nodes) { for (const node of [...nodes].reverse()) { this.children.unshift(node); node.parentNode = this; } }
  insertBefore(node, next) { if (node.parentNode) node.parentNode.children.splice(node.parentNode.children.indexOf(node), 1); const index = this.children.indexOf(next); this.children.splice(index < 0 ? this.children.length : index, 0, node); node.parentNode = this; }
  replaceChildren(...nodes) { this.ownText = ''; this.children = []; this.append(...nodes); }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  hasAttribute(name) { return this.attributes.has(name); }
  removeAttribute(name) { this.attributes.delete(name); }
  addEventListener(name, callback) { const callbacks = this.listeners.get(name) ?? []; callbacks.push(callback); this.listeners.set(name, callbacks); }
  querySelectorAll(selector) { const found = []; const visit = node => { for (const child of node.children) { if (selector.startsWith('.') && child.classes.has(selector.slice(1)) || selector === 'span' && child.tagName === 'SPAN') found.push(child); visit(child); } }; visit(this); return found; }
  querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; }
  async click() { if (!this.disabled) for (const callback of this.listeners.get('click') ?? []) await callback({ target: this, currentTarget: this }); }
  focus() { this.focused = true; }
  remove() { if (this.parentNode) this.parentNode.children.splice(this.parentNode.children.indexOf(this), 1); }
}
async function harness() {
  const elements = new Map(), deps = new Map(), calls = [], handlers = new Map(), notices = [], deferred = [], menus = [], layouts = [], modelCalls = [];
  const get = id => { if (!elements.has(id)) { const element = new Element(); element.id = id; elements.set(id, element); } return elements.get(id); };
  get('new-chat').append(new Element('span')); get('project-shared-warn').classList.add('hidden');
  const line = (project, id) => ({ id, session_id: `session:${id}`, origin_project: project, project_dir: project, profile: 'dev', running: false, label: id });
  const lines = new Map([[GENERAL, [line(GENERAL, 'general-main')]], [PROJECT, [line(PROJECT, 'project-main'), line(PROJECT, 'user-selected')]]]);
  const shellValues = { currentProject: GENERAL, activeProcessId: 'general-main', activeSessionId: 'session:general-main', processItems: lines.get(GENERAL), running: false, attachments: [] };
  const set = (key, value) => { shellValues[key] = value; deps.get('./03-shell.js').setExport(key, value); if (key === 'currentProject') modules.get('./03-general-scope.js').namespace.setConversationRoot(value); };
  const states = new Map(), queues = new Map(), processAutoState = new Map(), processProfileUi = new Map();
  const sessionState = id => { if (!states.has(id)) states.set(id, { phase: 'idle', running: false, converged: false }); return states.get(id); };
  let models = async () => {}, created = 0, paneReady = true;
  const document = { body: new Element('body'), getElementById: get, createElement: tag => new Element(tag), querySelector: () => null, querySelectorAll: () => [], addEventListener() {}, dispatchEvent() {} };
  document.body.dataset.space = 'dev'; document.body.dataset.view = 'chat'; document.body.dataset.appScope = 'global';
  const invoke = async (command, args = {}) => {
    calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
    if (handlers.has(command)) return handlers.get(command)(args);
    if (command === 'general_chat_location') return GENERAL;
    if (command === 'general_chat_open') return GENERAL;
    if (command === 'general_chat_link') { const linked = line(PROJECT, `linked-${++created}`); lines.get(PROJECT).push(linked); return linked; }
    if (command === 'projects_select') return { current: args.path, projects: [PROJECT], names: {} };
    if (command === 'process_list') return lines.get(args.projectDir) ?? [];
    if (command === 'process_create') { const item = line(args.projectDir, `created-${++created}`); lines.get(args.projectDir).push(item); return item; }
    if (['conversation_list', 'conversation_get', 'conversation_trace_get'].includes(command)) return [];
    if (command === 'project_facts') return { git: { state: 'repo', has_commits: true }, stacks: [] };
    if (command === 'project_root_info') return { selected: args.projectDir, resolved: args.projectDir, shared: false };
    if (['worktree_list', 'list_pending_inputs', 'pending_asks_get'].includes(command)) return [];
    return null;
  };
  const context = vm.createContext({ document, console, Date, Option: class { constructor(text, value) { this.textContent = text; this.value = value; } }, CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init?.detail; } }, setTimeout, clearTimeout, localStorage: { getItem: () => null, setItem() {} } });
  const importDynamic = async specifier => { const module = modules.get(specifier) ?? deps.get(specifier); assert.ok(module, `dynamic ${specifier}`); if (module.status === 'unlinked') await module.link(linker); if (module.status === 'linked') await module.evaluate(); return module; };
  const modules = new Map(names.map(name => [`./${name}`, new vm.SourceTextModule(sources.get(name), { context, identifier: name, importModuleDynamically: importDynamic })]));
  const navigate = (view, options = {}) => { if (!options.prepared && ['workspace', 'settings'].includes(view)) modules.get('./12-workbench.js').namespace.cancelProjectNavigation(); document.body.dataset.view = view; };
  const values = new Map([
    ['./01-core.js', { $: get, invoke, defer: callback => deferred.push(callback), confirmDialog: async () => true, promptBox: get('prompt'), activePane: get('pane'), messages: get('messages'), showPane: () => paneReady, resetPane: () => get('pane').replaceChildren(), readJson: (_key, fallback) => fallback, uiPrefsLoad: async () => ({}), uiPrefsSave: async () => {}, mergeWorkspaceState: (prior, delta) => ({ ...prior, ...delta }) }],
    ['./03-shell.js', { ...shellValues, setCurrentProject: value => set('currentProject', value), setActiveProcessId: value => set('activeProcessId', value), setActiveSessionId: value => set('activeSessionId', value), setProcessItems: value => set('processItems', value), setAttachments: value => set('attachments', value), sessionState, sessionStates: states, transitionSession: (id, phase, detail) => Object.assign(sessionState(id), { phase, running: phase === 'running', converged: ['idle', 'stopped', 'failed'].includes(phase) }, detail), setRunning: value => set('running', value), toast: value => notices.push(value), toastError: value => notices.push(value), navigate_view: navigate, ensureChatView: () => navigate('chat') }],
    ['./02-i18n.js', { t: text => text, localizeDynamic: text => text, localizedStage: text => text }],
    ['./00-surface.js', { openMenu: (_anchor, items) => menus.push(items) }],
    ['./03-layout.js', { layoutPref: () => null, setLayoutPref: (section, key, value) => layouts.push({ section, key, value }), flushLayout() {} }],
    ['./08-compose-runtime.js', { processAutoState, processProfileUi }],
    ['./07-events.js', { askQueues: queues, askActive: null, askQueueFor: id => { if (!queues.has(id)) queues.set(id, []); return queues.get(id); } }],
    ['./08-auto.js', { awaitingUserSessions: new Set() }],
    ['./05-subagents.js', { SA_ACTIVE: new Set(), subagentRunsFor: () => [] }],
    ['./08-models.js', { modelCatalogProject: PROJECT, loadModels: async () => { modelCalls.push({ project: shellValues.currentProject, process: shellValues.activeProcessId }); await models(); } }],
    ['./25-softwire-model.js', { sameProject: (a, b) => a === b }],
    ['./20-lines.js', { forProject: () => [] }],
    ['./12-session-tree.js', { processName: item => item.label ?? item.id, orderedProjects: prefs => prefs.projects ?? [], wrapProjectRow: (_path, button) => { const wrapper = new Element(); wrapper._link = button; wrapper.append(button); return wrapper; } }],
  ]);
  const imported = new Map();
  for (const text of sources.values()) for (const [, body, specifier] of text.matchAll(/import\s*\{([\s\S]*?)\}\s*from\s*"([^"]+)"/g)) {
    if (modules.has(specifier)) continue; const exports = imported.get(specifier) ?? new Set(); for (const name of body.split(',').map(value => value.trim()).filter(Boolean)) exports.add(name.split(/\s+as\s+/)[0]); imported.set(specifier, exports);
  }
  imported.get('./03-layout.js').add('flushLayout');
  for (const [specifier, names] of imported) { const exports = values.get(specifier) ?? {}; deps.set(specifier, new vm.SyntheticModule([...names], function () { for (const name of names) this.setExport(name, Object.hasOwn(exports, name) ? exports[name] : () => {}); }, { context })); }
  const linker = specifier => modules.get(specifier) ?? deps.get(specifier);
  await modules.get('./29-general-chat.js').link(linker); await modules.get('./29-general-chat.js').evaluate();
  const scope = modules.get('./03-general-scope.js').namespace, general = modules.get('./29-general-chat.js').namespace, sessions = modules.get('./09-sessions.js').namespace, workbench = modules.get('./12-workbench.js').namespace, workspace = modules.get('./03-workspaces.js').namespace, history = modules.get('./15-views-misc.js').namespace;
  scope.setGeneralChatRoot(GENERAL); scope.setConversationRoot(GENERAL);
  sessions.renderProjects({ current: PROJECT, projects: [PROJECT], names: {} }, { activate: false });
  for (const callback of deferred.filter(callback => callback.toString().includes('general_chat_location'))) callback(); await flush();
  return { get, scope, general, sessions, workbench, workspace, history, calls, handlers, menus, layouts, modelCalls, shellValues, document, notices, lines, models: callback => { models = callback; }, paneReady: value => { paneReady = value; }, navigate,
    select: (project, process) => { set('currentProject', project); set('activeProcessId', process); set('activeSessionId', `session:${process}`); set('processItems', lines.get(project)); },
    beginLink: async () => { await get('general-chat-link').click(); assert.equal(menus.length, 1); return menus[0][0].onSelect(); },
  };
}
const results = [];
async function check(name, expectedBefore, run) { try { results.push({ name, expected_before: expectedBefore, passed: true, evidence: await run() }); } catch (error) { results.push({ name, expected_before: expectedBefore, passed: false, code: error.code ?? '', message: error.message, actual: error.actual, expected: error.expected }); } }

await check('general scope identity accepts actual Windows path representations without changing the root', 'PASS', async () => {
  const h = await harness(); assert.equal(h.scope.isGeneralChat(GENERAL.replaceAll('/', '\\').toUpperCase() + '\\'), true); assert.equal(h.scope.isGeneralChat(PROJECT), false); assert.equal(h.scope.generalChatRoot(), GENERAL); return { root: h.scope.generalChatRoot() };
});
await check('general view uses its conversation root only in development space', 'PASS', async () => {
  const h = await harness(); h.scope.syncGeneralChatView('chat'); assert.equal(h.document.body.dataset.generalChat, 'true'); assert.equal(h.get('project-space-name').textContent, '无项目对话'); h.document.body.dataset.space = 'research'; h.scope.syncGeneralChatView('research'); assert.equal(h.document.body.dataset.generalChat, 'false'); assert.equal(h.get('new-chat').querySelector('span').textContent, '新建课题对话'); return { researchLabel: h.get('new-chat').textContent };
});
await check('normal opening selects general history and preserves registered project preferences', 'PASS', async () => {
  const h = await harness(); const prefs = h.sessions.lastProjectPrefs; assert.equal(await h.general.openGeneralChat(), true); await flush(); assert.equal(h.shellValues.currentProject, GENERAL); assert.equal(h.sessions.lastProjectPrefs, prefs); assert.equal(h.document.body.dataset.view, 'chat'); assert.equal(h.get('prompt').focused, true); assert.equal(h.get('workbench-general-chat').hasAttribute('aria-busy'), false); assert.equal(h.layouts.at(-1).value, 'general'); return { process: h.shellValues.activeProcessId, prefsCurrent: prefs.current };
});
await check('late general-open receipt cannot activate after a newer settings navigation', 'PASS', async () => {
  const h = await harness(), request = pause(); h.select(PROJECT, 'project-main'); h.handlers.set('general_chat_open', () => request.promise); const opening = h.general.openGeneralChat(); await flush(); h.navigate('settings'); request.accept(GENERAL); assert.equal(await opening, false); assert.equal(h.shellValues.currentProject, PROJECT); assert.equal(h.document.body.dataset.view, 'settings'); return { project: h.shellValues.currentProject, view: h.document.body.dataset.view };
});
await check('normal project link selects the linked conversation and preserves original source', 'PASS', async () => {
  const h = await harness(); await h.beginLink(); assert.equal(h.shellValues.currentProject, PROJECT); assert.equal(h.shellValues.activeProcessId, 'linked-1'); assert.equal(h.lines.get(GENERAL)[0].id, 'general-main'); assert.equal(h.get('general-chat-link').disabled, false); return { selected: h.shellValues.activeProcessId, source: h.lines.get(GENERAL)[0].id };
});
await check('link completion before target navigation preserves a newer recipient', 'PASS', async () => {
  const h = await harness(), request = pause(); h.handlers.set('general_chat_link', () => request.promise); const linking = h.beginLink(); await flush(); h.select(PROJECT, 'user-selected'); request.accept({ id: 'linked-1' }); await linking; assert.equal(h.shellValues.activeProcessId, 'user-selected'); assert.equal(h.calls.some(call => call.command === 'projects_select'), false); return { selected: h.shellValues.activeProcessId };
});
await check('link target loading must not override a later conversation selection in that project', 'FAIL', async () => {
  const h = await harness(), slow = pause(); let count = 0; h.models(() => ++count === 1 ? slow.promise : Promise.resolve()); const linking = h.beginLink(); await flush();
  assert.equal(h.shellValues.currentProject, PROJECT); assert.equal(h.modelCalls.length, 1); await h.sessions.switchProcess('user-selected'); assert.equal(h.shellValues.activeProcessId, 'user-selected'); const userGeneration = h.sessions.processSwitchGeneration; slow.accept(); await linking;
  assert.equal(h.shellValues.activeProcessId, 'user-selected'); assert.equal(h.sessions.processSwitchGeneration, userGeneration); return { selected: h.shellValues.activeProcessId, generation: userGeneration };
});
await check('new general chat creation must not override a later same-root selection', 'FAIL', async () => {
  const h = await harness(), request = pause(); h.lines.get(GENERAL).push({ ...h.lines.get(GENERAL)[0], id: 'general-other', session_id: 'session:general-other' }); h.handlers.set('process_create', () => request.promise); const opening = h.general.openGeneralChat({ newChat: true }); await flush(); assert.equal(h.calls.some(call => call.command === 'process_create'), true);
  await h.sessions.switchProcess('general-other'); assert.equal(h.shellValues.activeProcessId, 'general-other'); h.lines.get(GENERAL).push({ ...h.lines.get(GENERAL)[0], id: 'created-late', session_id: 'session:created-late' }); request.accept(h.lines.get(GENERAL).at(-1)); await opening;
  assert.equal(h.shellValues.activeProcessId, 'general-other'); return { selected: h.shellValues.activeProcessId };
});
await check('failed general link releases its button and preserves the original conversation', 'PASS', async () => {
  const h = await harness(); h.handlers.set('general_chat_link', () => { throw new Error('reachable native rejection'); }); await h.beginLink(); assert.equal(h.shellValues.currentProject, GENERAL); assert.equal(h.shellValues.activeProcessId, 'general-main'); assert.equal(h.get('general-chat-link').disabled, false); assert.equal(h.notices.some(value => value.includes('reachable native rejection')), true); return { selected: h.shellValues.activeProcessId };
});
await check('normal general new-chat creates and selects its own conversation', 'PASS', async () => {
  const h = await harness(); assert.equal(await h.general.openGeneralChat({ newChat: true }), true); assert.equal(h.shellValues.activeProcessId, 'created-1'); assert.equal(h.document.body.dataset.view, 'chat'); assert.equal(h.get('general-chat-link').disabled, false); return { selected: h.shellValues.activeProcessId };
});
await check('actual startNewConversation create receipt cannot undo newer settings navigation', 'FAIL', async () => {
  const h = await harness(), request = pause(); h.select(PROJECT, 'project-main'); h.document.body.dataset.appScope = 'project'; h.handlers.set('process_create', () => request.promise); const creating = h.history.startNewConversation(); await flush(); assert.equal(h.calls.some(call => call.command === 'process_create'), true); h.navigate('settings');
  const item = { ...h.lines.get(PROJECT)[0], id: 'created-late', session_id: 'session:created-late' }; h.lines.get(PROJECT).push(item); request.accept(item); await creating;
  assert.equal(h.document.body.dataset.view, 'settings'); assert.equal(h.shellValues.activeProcessId, 'project-main'); return { view: h.document.body.dataset.view, selected: h.shellValues.activeProcessId };
});
await check('normal actual project discussion creation still selects and displays the new readonly conversation', 'PASS', async () => {
  const h = await harness(); h.select(PROJECT, 'project-main'); h.document.body.dataset.appScope = 'project'; await h.history.startNewConversation(); assert.equal(h.shellValues.activeProcessId, 'created-1'); assert.equal(h.document.body.dataset.view, 'chat'); const create = h.calls.find(call => call.command === 'process_create'); assert.equal(create.args.profile, 'readonly'); return { selected: h.shellValues.activeProcessId, profile: create.args.profile };
});
await check('own create switch waiting for history cannot replace a later selection after returning', 'PASS', async () => {
  const h = await harness(), history = pause(); h.paneReady(false); h.handlers.set('conversation_get', args => args.processId === 'created-1' ? history.promise : []);
  h.select(PROJECT, 'project-main'); h.document.body.dataset.appScope = 'project';
  const creating = h.history.startNewConversation(); await flush();
  assert.equal(h.shellValues.activeProcessId, 'created-1');
  assert.equal(h.calls.some(call => call.command === 'conversation_get' && call.args.processId === 'created-1'), true);
  h.paneReady(true); await h.sessions.switchProcess('user-selected'); assert.equal(h.shellValues.activeProcessId, 'user-selected'); h.get('pane').innerHTML = 'later conversation content'; history.accept([]); await creating;
  assert.equal(h.shellValues.activeProcessId, 'user-selected'); assert.equal(h.get('pane').innerHTML, 'later conversation content'); return { selected: h.shellValues.activeProcessId, content: h.get('pane').innerHTML };
});
console.log(JSON.stringify({ mode: before ? 'before' : 'current', baseline, source_sha256_lf: fingerprint, results, passed: results.filter(item => item.passed).length, failed: results.filter(item => !item.passed).length }, null, 2));
if (results.some(item => !item.passed)) process.exitCode = 1;
