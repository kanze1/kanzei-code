// Complete production ESM owners; shell/DOM/native receipts are deterministic fixtures.
// --before reads complete committed sources without changing production files.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import vm from "node:vm";

const root = resolve(import.meta.dirname, "..");
const baseline = "84a3892bc80483a61cc6335e7a8ca654ca2b3c18";
const before = process.argv.includes("--before");
const backendMode = process.argv.includes("--key-only") ? "key-only" : before ? "old" : "delta";
const clone = value => JSON.parse(JSON.stringify(value));
const drain = () => new Promise(done => setImmediate(done));
function deferred() {
  let resolvePromise, rejectPromise;
  const promise = new Promise((done, fail) => { resolvePromise = done; rejectPromise = fail; });
  return { promise, resolve: resolvePromise, reject: rejectPromise };
}
class Element {
  constructor(tag = "div") {
    this.tagName = tag.toUpperCase(); this.children = []; this.dataset = {}; this.attributes = new Map(); this.listeners = new Map(); this.classes = new Set(); this.value = ""; this.checked = false;
    this.css = new Map(); this.style = { setProperty: (key, value) => this.css.set(key, value), removeProperty: key => this.css.delete(key) };
    this.classList = { add: (...keys) => keys.forEach(key => this.classes.add(key)), remove: (...keys) => keys.forEach(key => this.classes.delete(key)), contains: key => this.classes.has(key), toggle: (key, value) => { const on = value ?? !this.classes.has(key); on ? this.classes.add(key) : this.classes.delete(key); return on; } };
  }
  set className(value) { this.classes = new Set(value.split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classes].join(" "); }
  get isConnected() { return true; }
  get parentElement() { return this.parentNode; }
  append(...children) { for (const child of children) { child.parentNode = this; this.children.push(child); } }
  appendChild(child) { this.append(child); return child; }
  prepend(child) { child.parentNode = this; this.children.unshift(child); }
  replaceChildren(...children) { this.children = []; this.append(...children); }
  setAttribute(key, value) { this.attributes.set(key, String(value)); }
  getAttribute(key) { return this.attributes.get(key) ?? null; }
  hasAttribute(key) { return this.attributes.has(key); }
  removeAttribute(key) { this.attributes.delete(key); }
  addEventListener(key, callback) { const values = this.listeners.get(key) ?? []; values.push(callback); this.listeners.set(key, values); }
  removeEventListener(key, callback) { this.listeners.set(key, (this.listeners.get(key) ?? []).filter(fn => fn !== callback)); }
  dispatchEvent(event) { for (const callback of this.listeners.get(event.type) ?? []) callback(event); return true; }
  querySelector() { return null; }
  querySelectorAll() { return []; }
  getBoundingClientRect() { return { left: 0, top: 0, right: 300, bottom: 600, width: 300, height: 600 }; }
  focus() {}
  setSelectionRange(start, end) { this.selectionStart = start; this.selectionEnd = end; }
}
function applyPrefs(durable, patch) {
  if (patch.work_priority) durable.work_priority = backendMode === "old" ? clone(patch.work_priority) : { ...(durable.work_priority ?? {}), ...clone(patch.work_priority) };
  if (patch.process_auto_state) {
    if (backendMode === "old") durable.process_auto_state = clone(patch.process_auto_state);
    else {
      durable.process_auto_state ??= {};
      for (const [id, fields] of Object.entries(patch.process_auto_state)) {
        if (fields === null) delete durable.process_auto_state[id];
        else durable.process_auto_state[id] = backendMode === "key-only" ? clone(fields) : { ...(durable.process_auto_state[id] ?? {}), ...clone(fields) };
      }
    }
  }
  for (const key of ["continue_prompt", "auto_max", "theme"]) if (Object.hasOwn(patch, key)) durable[key] = patch[key];
}
async function harness(initial, { project = "project-A", backend = { value: clone(initial), runtime: {} }, holdAuto = false, holdLookups = false, active = false } = {}) {
  backend.runtime ??= {};
  const getGate = deferred(), elements = new Map(), initializers = [], storage = new Map(), calls = [], autoReceipts = [], lookups = [];
  let timer = 0, failSave = false;
  const get = id => { if (!elements.has(id)) { const el = new Element(); el.id = id; elements.set(id, el); } return elements.get(id); };
  get("profile-select").value = "dev-pair";
  const sessions = new Map();
  const shellValues = {
    currentProject: project, activeProcessId: active ? "p|A" : null, activeSessionId: active ? "session-A" : null,
    processItems: [], sessionStates: sessions, attachments: [], running: false,
    sessionState: id => { if (!sessions.has(id)) sessions.set(id, { phase: "running" }); return sessions.get(id); },
  };
  const timers = new Map();
  const native = async (command, args = {}) => {
    calls.push({ command, args: clone(args) });
    if (command === "ui_prefs_get") return getGate.promise;
    if (command === "ui_prefs_set") {
      if (failSave) { failSave = false; throw new Error("injected native write rejection"); }
      applyPrefs(backend.value, args); return null;
    }
    if (command === "auto_state_update") {
      const gate = deferred();
      const receipt = { args: clone(args), accept: () => { const { sessionId, ...patch } = clone(args); backend.runtime[sessionId] = { ...(backend.runtime[sessionId] ?? {}), ...patch }; gate.resolve(null); }, reject: () => gate.reject(new Error("injected runtime update rejection")) };
      autoReceipts.push(receipt); if (!holdAuto) receipt.accept(); return gate.promise;
    }
    if (holdLookups && ["project_files", "memory_entries"].includes(command)) {
      const gate = deferred(); lookups.push({ command, args: clone(args), ...gate }); return gate.promise;
    }
    return [];
  };
  const document = { readyState: "loading", activeElement: null, body: new Element("body"), documentElement: new Element("html"), getElementById: get, createElement: tag => new Element(tag), querySelector: () => null, querySelectorAll: () => [], dispatchEvent() {}, addEventListener: (event, callback) => { if (event === "DOMContentLoaded") initializers.push(callback); } };
  const context = vm.createContext({ document, console: { warn() {}, error() {}, log() {} }, navigator: {}, TextEncoder, performance: { now: () => 0 },
    window: { innerWidth: 1200, innerHeight: 900, __TAURI__: { core: { invoke: native }, event: { listen() {} } }, addEventListener() {} },
    Event: class { constructor(type) { this.type = type; } }, CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init?.detail; } },
    localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value), removeItem: key => storage.delete(key) },
    setTimeout: (callback, ms) => { const id = ++timer; timers.set(id, { callback, ms }); return id; }, clearTimeout: id => timers.delete(id), setInterval: () => ++timer, clearInterval() {}, requestAnimationFrame() {},
  });
  const names = ["00-surface.js", "00-frame.js", "01-core.js", "03-layout.js", "03-workspaces.js", "08-auto.js", "08-compose-runtime.js", "09-sessions.js"];
  const sources = new Map(names.map(name => [name, before ? execFileSync("git", ["show", `${baseline}:crates/kanzei-app/ui/${name}`], { cwd: root, encoding: "utf8" }) : readFileSync(resolve(root, "crates/kanzei-app/ui", name), "utf8")]));
  const needed = new Map();
  for (const source of sources.values()) for (const [, body, specifier] of source.matchAll(/(?:import|export)\s*\{([\s\S]*?)\}\s*from\s*"([^"]+)"/g)) {
    const exports = needed.get(specifier) ?? new Set(); for (const name of body.split(",").map(value => value.trim()).filter(Boolean)) exports.add(name.split(/\s+as\s+/)[0]); needed.set(specifier, exports);
  }
  const deps = new Map(), noop = () => {};
  for (const [specifier, exports] of needed) {
    const values = specifier === "./03-shell.js" ? shellValues : specifier === "./02-i18n.js" ? { t: value => value, localizeDynamic: value => value } : specifier === "./03-general-scope.js" ? { isGeneralChat: () => false } : specifier === "./08-auto.js" ? { autoContinueTimers: new Map(), autoContinueInFlight: new Set(), awaitingUserSessions: new Set(), autoPaused: false, autoStopAfterRound: false, DEFAULT_AUTO_CONTINUE_MAX: 10 } : specifier === "./08-compose.js" ? { state: {} } : specifier === "./09-sessions.js" ? { processRunning: item => item.running } : specifier === "./20-lines.js" ? { collaborationLines: [] } : {};
    deps.set(specifier, new vm.SyntheticModule([...exports], function () { for (const key of exports) this.setExport(key, Object.hasOwn(values, key) ? values[key] : noop); }, { context }));
  }
  // The existing core/runtime/auto cycle is linked in two stages solely by the fixture.
  // First evaluate complete core/runtime sources with auto function placeholders, then
  // evaluate complete auto source against those real owners and install its receipts.
  const first = names.filter(name => !["08-auto.js", "09-sessions.js"].includes(name));
  const modules = new Map(first.map(name => [name, new vm.SourceTextModule(sources.get(name), { context, identifier: name })]));
  const linker = specifier => modules.get(specifier.slice(2)) ?? deps.get(specifier);
  for (const module of modules.values()) { if (module.status === "unlinked") await module.link(linker); if (module.status === "linked") await module.evaluate(); }
  const shell = deps.get("./03-shell.js");
  for (const [name, field] of [["setProcessItems", "processItems"], ["setActiveProcessId", "activeProcessId"], ["setActiveSessionId", "activeSessionId"], ["setCurrentProject", "currentProject"]]) shell.setExport(name, value => { shellValues[field] = value; shell.setExport(field, value); });
  shell.setExport("transitionSession", (id, phase, fields) => Object.assign(shellValues.sessionState(id), { phase }, fields));
  const autoModule = new vm.SourceTextModule(sources.get("08-auto.js"), { context, identifier: "08-auto.js" });
  await autoModule.link(linker); await autoModule.evaluate();
  const auto = autoModule.namespace;
  const autoStub = deps.get("./08-auto.js");
  const refreshAuto = () => { for (const key of needed.get("./08-auto.js")) if (typeof auto[key] !== "function") autoStub.setExport(key, auto[key]); };
  for (const key of needed.get("./08-auto.js")) {
    autoStub.setExport(key, typeof auto[key] === "function" ? (...args) => { const result = auto[key](...args); refreshAuto(); return result; } : auto[key]);
  }
  const sessionsModule = new vm.SourceTextModule(sources.get("09-sessions.js"), { context, identifier: "09-sessions.js" });
  await sessionsModule.link(linker); await sessionsModule.evaluate();
  const runtime = modules.get("08-compose-runtime.js").namespace, core = modules.get("01-core.js").namespace;
  const init = fragment => { const callback = initializers.find(fn => fn.toString().includes(fragment)); assert(callback, `initializer ${fragment} exists`); callback(); };
  return { runtime, auto, core, sessions: sessionsModule.namespace, storage, timers, calls, autoReceipts, lookups, get, document,
    init, emit: (id, event) => get(id).dispatchEvent({ type: event, preventDefault() {}, stopPropagation() {} }),
    setProcesses: items => { shellValues.processItems = items; shell.setExport("processItems", items); },
    setActive: (id, session) => { shellValues.activeProcessId = id; shellValues.activeSessionId = session; shell.setExport("activeProcessId", id); shell.setExport("activeSessionId", session); },
    setProject: value => { shellValues.currentProject = value; shell.setExport("currentProject", value); },
    phase: (id, phase) => { shellValues.sessionState(id).phase = phase; },
    durable: () => clone(backend.value), runtimeState: () => clone(backend.runtime), rejectNextSave: () => { failSave = true; },
    resolveGet: () => getGate.resolve(clone(initial)),
    ready: async () => { const loading = core.uiPrefsLoad(); getGate.resolve(clone(initial)); await loading; runtime.mergeBackendAutoState(core.uiPrefsCache.process_auto_state); await drain(); },
  };
}
const autoFixture = (entries = {}) => ({ process_auto_state: { "p|A": { enabled: true, paused: false, stopAfterRound: false, maxRounds: 5, mode: "dev-pair" }, ...entries } });
const item = (id = "p|A", session = "session-A") => ({ id, session_id: session, origin_project: "project-A", profile: "dev", running: true });
const prefsWrites = h => h.calls.filter(call => call.command === "ui_prefs_set");
const runtimeWrites = h => h.calls.filter(call => call.command === "auto_state_update");
const evidence = (name, value) => console.log(JSON.stringify({ evidence: name, source: before ? baseline : "working tree", backendMode, ...value }));

test("priority action retains its original project while the initial read is pending", async () => {
  const h = await harness({ work_priority: { "project-A": "defect-first", "project-B": "defect-first" } });
  h.init('$("work-priority-select").addEventListener'); h.get("work-priority-select").value = "requirement-first"; h.emit("work-priority-select", "change");
  await drain(); h.setProject("project-B"); h.resolveGet(); await drain();
  evidence("priority captured project", { durable: h.durable(), calls: h.calls });
  assert.equal(h.durable().work_priority["project-A"], "requirement-first");
  assert.equal(h.durable().work_priority["project-B"], "defect-first");
});
test("initial priority read cannot overwrite a newer user selection", async () => {
  const h = await harness({ work_priority: { "project-A": "defect-first" } });
  h.auto.syncWorkPriorityControl(); h.init('$("work-priority-select").addEventListener');
  h.get("work-priority-select").value = "requirement-first"; h.emit("work-priority-select", "change"); h.resolveGet(); await drain();
  evidence("priority generation", { value: h.get("work-priority-select").value, durable: h.durable() });
  assert.equal(h.get("work-priority-select").value, "requirement-first");
});
test("two independent windows preserve both project priority edits", async () => {
  const initial = { work_priority: { "project-A": "defect-first", "project-B": "defect-first" } }, backend = { value: clone(initial) };
  const a = await harness(initial, { backend }), b = await harness(initial, { project: "project-B", backend });
  for (const h of [a, b]) { await h.ready(); h.init('$("work-priority-select").addEventListener'); h.get("work-priority-select").value = "requirement-first"; h.emit("work-priority-select", "change"); await drain(); }
  evidence("priority window deltas", { durable: backend.value, a: prefsWrites(a), b: prefsWrites(b) });
  assert.equal(backend.value.work_priority["project-A"], "requirement-first"); assert.equal(backend.value.work_priority["project-B"], "requirement-first");
});
test("different process edits preserve the other window's auto state", async () => {
  const initial = autoFixture({ "p|B": { enabled: false, paused: false, mode: "dev-pair", maxRounds: 7 } }), backend = { value: clone(initial) };
  const a = await harness(initial, { backend }), b = await harness(initial, { backend });
  for (const h of [a, b]) { await h.ready(); h.setProcesses([item(), item("p|B", "session-B")]); }
  await a.runtime.setLineAutoState("p|A", { paused: true }); await drain();
  await b.runtime.setLineAutoState("p|B", { enabled: true }); await drain();
  evidence("process window deltas", { durable: backend.value, a: prefsWrites(a), b: prefsWrites(b) });
  assert.equal(backend.value.process_auto_state["p|A"].paused, true); assert.equal(backend.value.process_auto_state["p|B"].enabled, true);
});
test("same process independent field actions preserve durable and actual runtime fields", async () => {
  const initial = autoFixture(), backend = { value: clone(initial), runtime: { "session-A": clone(initial.process_auto_state["p|A"]) } };
  const a = await harness(initial, { backend }), b = await harness(initial, { backend });
  for (const h of [a, b]) { await h.ready(); h.setProcesses([item()]); }
  await a.runtime.setLineAutoState("p|A", { paused: true }); await drain();
  await b.runtime.setLineAutoState("p|A", { enabled: false }); await drain();
  evidence("same process field deltas", { durable: backend.value, runtime: backend.runtime, b: b.calls });
  assert.equal(backend.value.process_auto_state["p|A"].enabled, false); assert.equal(backend.value.process_auto_state["p|A"].paused, true);
  assert.equal(backend.runtime["session-A"].enabled, false); assert.equal(backend.runtime["session-A"].paused, true);
  assert.equal(Object.hasOwn(runtimeWrites(b).at(-1).args, "paused"), false);
});
test("same-window queued line actions cannot replay an older receipt over a later field", async () => {
  const initial = autoFixture(), h = await harness(initial, { holdAuto: true, active: true });
  await h.ready(); h.setProcesses([item()]); h.runtime.applyAutoUiState("p|A");
  const first = h.runtime.setLineAutoState("p|A", { paused: true }); await drain();
  const second = h.runtime.setLineAutoState("p|A", { enabled: false }); await drain();
  if (before) { h.autoReceipts[1].accept(); await second; h.autoReceipts[0].accept(); await first; }
  else { assert.equal(h.autoReceipts.length, 1, "second update awaits the first receipt"); h.autoReceipts[0].accept(); await first; await drain(); h.autoReceipts[1].accept(); await second; }
  await drain();
  evidence("ACK ordering", { durable: h.durable(), local: h.runtime.lineAutoConfig("p|A"), runtime: h.runtimeState(), calls: runtimeWrites(h), checked: h.get("auto-continue").checked });
  assert.equal(h.runtime.lineAutoConfig("p|A").enabled, false); assert.equal(h.runtime.lineAutoConfig("p|A").paused, true);
  assert.equal(h.get("auto-continue").checked, false); assert.equal(h.durable().process_auto_state["p|A"].enabled, false);
  assert.equal(h.auto.autoContinueTimers.has("session-A"), false);
});
test("a rejected runtime update leaves state intact and its queue continues", async () => {
  const h = await harness(autoFixture(), { holdAuto: true }); await h.ready(); h.setProcesses([item()]);
  const first = h.runtime.setLineAutoState("p|A", { paused: true }); const rejected = assert.rejects(first, /injected runtime/); await drain();
  const second = h.runtime.setLineAutoState("p|A", { enabled: false }); await drain();
  h.autoReceipts[0].reject(); await rejected; await drain(); h.autoReceipts[1].accept(); await second; await drain();
  evidence("queue failure continuation", { local: h.runtime.lineAutoConfig("p|A"), durable: h.durable() });
  assert.equal(h.runtime.lineAutoConfig("p|A").paused, false); assert.equal(h.runtime.lineAutoConfig("p|A").enabled, false);
  assert.equal(h.durable().process_auto_state["p|A"].paused, false); assert.equal(h.auto.autoContinueTimers.size, 0);
});
test("pending off intent prevents a delayed enable receipt from arming a new continuation", async () => {
  const h = await harness(autoFixture({ "p|A": { enabled: false, paused: false, mode: "dev-pair" } }), { holdAuto: true, active: true });
  await h.ready(); h.setProcesses([{ ...item(), running: false }]); h.phase("session-A", "idle"); h.runtime.applyAutoUiState("p|A");
  const enable = h.runtime.setLineAutoState("p|A", { enabled: true }); await drain();
  const disable = h.runtime.setLineAutoState("p|A", { enabled: false }); await drain();
  h.autoReceipts[0].accept(); await enable; await drain();
  const armedWhileOffPending = h.auto.autoContinueTimers.has("session-A");
  h.autoReceipts[1].accept(); await disable; await drain();
  evidence("pending off timer", { armedWhileOffPending, final: h.runtime.lineAutoConfig("p|A") });
  assert.equal(armedWhileOffPending, false, "an older enable receipt must not restart a timer cancelled by newer off intent");
  assert.equal(h.auto.autoContinueTimers.has("session-A"), false);
});
test("first enable captures the visible line mode before an IPC wait and project switch", async () => {
  const initial = autoFixture({ "p|A": { enabled: false, paused: false, maxRounds: 5 } });
  const h = await harness(initial, { holdAuto: true, active: true }); await h.ready(); h.setProcesses([item(), item("p|B", "session-B")]);
  h.runtime.applyAutoUiState("p|A"); h.get("profile-select").value = "dev-pair";
  const pending = h.runtime.setLineAutoState("p|A", { enabled: true }); await drain();
  h.setProject("project-B"); h.setActive("p|B", "session-B"); h.get("profile-select").value = "dev-auto";
  h.autoReceipts[0].accept(); await pending; await drain();
  evidence("first enable captured mode", { durable: h.durable(), local: h.runtime.lineAutoConfig("p|A"), calls: runtimeWrites(h) });
  assert.equal(h.runtime.lineAutoConfig("p|A").mode, "dev-pair"); assert.equal(h.durable().process_auto_state["p|A"].mode, "dev-pair");
  assert.equal(runtimeWrites(h).at(-1).args.sessionId, "session-A");
});
test("top-bar independent controls publish only their own runtime and preference fields", async () => {
  const h = await harness(autoFixture(), { active: true }); await h.ready(); h.setProcesses([item()]); h.runtime.applyAutoUiState("p|A");
  h.init('$("auto-pause").addEventListener'); h.init('$("auto-continue").addEventListener');
  h.emit("auto-pause", "click"); await drain(); h.get("auto-continue").checked = false; h.emit("auto-continue", "change"); await drain();
  evidence("top bar deltas", { prefs: prefsWrites(h), runtime: runtimeWrites(h) });
  assert.deepEqual(runtimeWrites(h).at(-2).args, { sessionId: "session-A", paused: true });
  assert.deepEqual(runtimeWrites(h).at(-1).args, { sessionId: "session-A", enabled: false });
  assert.deepEqual(prefsWrites(h).at(-1).args.process_auto_state, { "p|A": { enabled: false } });
});
test("explicit softwire start and off keep their coupled three-field intent", async () => {
  const h = await harness(autoFixture()); await h.ready(); h.setProcesses([item()]);
  await h.runtime.setLineAutoState("p|A", { enabled: true, paused: false, stopAfterRound: false }); await drain();
  await h.runtime.setLineAutoState("p|A", { enabled: false, paused: false, stopAfterRound: false }); await drain();
  assert.deepEqual(runtimeWrites(h).at(-2).args, { sessionId: "session-A", enabled: true, paused: false, stopAfterRound: false });
  assert.deepEqual(runtimeWrites(h).at(-1).args, { sessionId: "session-A", enabled: false, paused: false, stopAfterRound: false });
});
test("explicit edit before hydrate survives while untouched backend fields stay authoritative", async () => {
  const h = await harness(autoFixture()); h.setProcesses([item()]);
  await h.runtime.setLineAutoState("p|A", { paused: true });
  assert.equal(prefsWrites(h).length, 0, "initial read is awaited before durable publication");
  await h.ready();
  evidence("hydrate explicit delta", { local: h.runtime.lineAutoConfig("p|A"), durable: h.durable(), writes: prefsWrites(h) });
  assert.equal(h.runtime.lineAutoConfig("p|A").paused, true); assert.equal(h.runtime.lineAutoConfig("p|A").enabled, true);
  assert.equal(h.durable().process_auto_state["p|A"].paused, true); assert.equal(h.durable().process_auto_state["p|A"].maxRounds, 5);
});
test("confirmed retirement sends a tombstone and preserves other projects", async () => {
  const initial = autoFixture({ "p|B": { enabled: true, mode: "dev-pair" } }), h = await harness(initial); await h.ready();
  h.setProcesses([item(), { ...item("p|B", "session-B"), origin_project: "project-B" }]);
  h.sessions.renderProcesses([]); await drain();
  evidence("retired process tombstone", { durable: h.durable(), writes: prefsWrites(h) });
  assert.equal(Object.hasOwn(h.durable().process_auto_state, "p|A"), false); assert.equal(h.durable().process_auto_state["p|B"].enabled, true);
});
test("core cache merges field deltas, honors tombstones and refuses failed native cache publication", async () => {
  const initial = autoFixture({ "p|B": { enabled: true, maxRounds: 7 } }), h = await harness(initial); await h.ready();
  await h.core.uiPrefsSave({ process_auto_state: { "p|A": { paused: true } }, work_priority: { "project-A": "requirement-first" } });
  await h.core.uiPrefsSave({ process_auto_state: { "p|A": { enabled: false }, "p|B": null }, work_priority: { "project-B": "defect-first" } });
  h.rejectNextSave(); await h.core.uiPrefsSave({ process_auto_state: { "p|A": { paused: false } } });
  evidence("cache delta failure", { cache: clone(h.core.uiPrefsCache), durable: h.durable() });
  assert.equal(h.core.uiPrefsCache.process_auto_state["p|A"].paused, true); assert.equal(h.core.uiPrefsCache.process_auto_state["p|A"].maxRounds, 5);
  assert.equal(Object.hasOwn(h.core.uiPrefsCache.process_auto_state, "p|B"), false); assert.equal(h.core.uiPrefsCache.work_priority["project-A"], "requirement-first");
  assert.deepEqual(clone(h.core.uiPrefsCache.process_auto_state), h.durable().process_auto_state);
});
test("initial continue prompt hydrate cannot replay an older value after a real change", async () => {
  const h = await harness({ continue_prompt: "old persisted intent" });
  h.init('const stored = (localStorage.getItem("kz-continue-prompt")'); h.init('$("continue-prompt").addEventListener');
  h.get("continue-prompt").value = "new typed intent"; h.emit("continue-prompt", "change"); h.resolveGet(); await drain();
  evidence("continue prompt hydrate", { visible: h.get("continue-prompt").value, durable: h.durable() });
  assert.equal(h.get("continue-prompt").value, "new typed intent"); assert.equal(h.auto.continuePrompt(), "new typed intent");
  assert.equal(h.durable().continue_prompt, "new typed intent");
});
test("fresh priority hydrate feeds the actual background continuation request", async () => {
  const h = await harness({ work_priority: { "project-A": "requirement-first" } });
  h.auto.syncWorkPriorityControl(); await h.ready(); h.setProcesses([item()]);
  assert.equal(h.get("work-priority-select").value, "requirement-first");
  await h.runtime.sendAutoToSession("fixture intent", "session-A");
  const request = h.calls.find(call => call.command === "run_prompt");
  evidence("background priority", { request, visible: h.get("work-priority-select").value, cache: clone(h.core.uiPrefsCache.work_priority) });
  assert(request, "actual continuation request was emitted"); assert.equal(request.args.workPriority, "requirement-first");
});
test("hiding file completion invalidates the in-flight lookup", async () => {
  const h = await harness({}, { holdLookups: true }); h.core.promptBox.value = "@src"; h.core.promptBox.setSelectionRange(4, 4);
  const loading = h.runtime.refreshFileSuggestions(); await drain(); h.runtime.hideFileSuggestions();
  h.lookups[0].resolve(["src/old-A.rs"]); await loading;
  evidence("closed file suggestions", { suggestions: [...h.runtime.fileSuggestions], calls: h.calls });
  assert.equal(h.runtime.fileSuggestions.length, 0);
});
test("an old project's delayed completion response cannot enter the new project", async () => {
  const h = await harness({}, { holdLookups: true }); h.core.promptBox.value = "@src"; h.core.promptBox.setSelectionRange(4, 4);
  const loading = h.runtime.refreshFileSuggestions(); await drain(); h.setProject("project-B");
  h.lookups[0].resolve(["src/private-A.rs"]); await loading;
  evidence("file suggestions project", { suggestions: [...h.runtime.fileSuggestions], calls: h.calls });
  assert.equal(h.runtime.fileSuggestions.length, 0);
});
test("a stale project's SOP cannot render or submit into another project", async () => {
  const h = await harness({}, { holdLookups: true });
  const loading = h.runtime.openSopPicker(); await drain(); h.setProject("project-B");
  h.lookups[0].resolve([{ title: "A-only workflow", body: "A-only command", status: "active" }]); h.lookups[1].resolve([]); await loading;
  const entries = h.get("sop-list").children.filter(el => el.classList.contains("sop-entry"));
  for (const entry of entries) entry.dispatchEvent({ type: "click" }); await drain();
  evidence("SOP project scope", { entries: entries.length, requests: h.calls.filter(call => call.command === "run_prompt"), prompt: h.core.promptBox.value });
  assert.equal(entries.length, 0); assert.equal(h.calls.some(call => call.command === "run_prompt"), false); assert.equal(h.core.promptBox.value, "");
});
test("a typed but unblurred continue prompt is preserved by initial hydrate", async () => {
  const h = await harness({ continue_prompt: "persisted old intent" });
  h.init('const stored = (localStorage.getItem("kz-continue-prompt")'); h.init('$("continue-prompt").addEventListener');
  h.get("continue-prompt").value = "new draft before blur"; h.emit("continue-prompt", "input"); h.resolveGet(); await drain();
  assert.equal(h.get("continue-prompt").value, "new draft before blur"); assert.equal(prefsWrites(h).length, 0);
});
test("unchanged prompt hydrate and legacy priority fallback retain their existing contracts", async () => {
  const h = await harness({ continue_prompt: "stored custom intent" });
  h.init('const stored = (localStorage.getItem("kz-continue-prompt")'); h.resolveGet(); await drain();
  assert.equal(h.auto.continuePrompt(), "stored custom intent");
  h.storage.set("kz-work-priority:project-A", "requirement-first"); h.setProcesses([item()]);
  await h.runtime.sendAutoToSession("fallback control", "session-A");
  assert.equal(h.calls.find(call => call.command === "run_prompt").args.workPriority, "requirement-first");
});
test("a late line receipt cannot resurrect an actually retired process", async () => {
  const h = await harness(autoFixture(), { holdAuto: true }); await h.ready(); h.setProcesses([item()]);
  const pending = h.runtime.setLineAutoState("p|A", { paused: true }); await drain(); h.sessions.renderProcesses([]); await drain();
  h.autoReceipts[0].accept(); await pending; await drain();
  assert.equal(h.runtime.processAutoState.has("p|A"), false); assert.equal(Object.hasOwn(h.durable().process_auto_state, "p|A"), false);
});
test("an older line ACK merges its field into newer top-bar state without replaying others", async () => {
  const h = await harness(autoFixture(), { holdAuto: true, active: true }); await h.ready(); h.setProcesses([item()]); h.runtime.applyAutoUiState("p|A");
  const pending = h.runtime.setLineAutoState("p|A", { paused: true }); await drain();
  h.init('$("auto-continue").addEventListener'); h.get("auto-continue").checked = false; h.emit("auto-continue", "change"); await drain();
  h.autoReceipts[1].accept(); await drain(); h.autoReceipts[0].accept(); await pending; await drain();
  assert.equal(h.runtime.lineAutoConfig("p|A").enabled, false); assert.equal(h.runtime.lineAutoConfig("p|A").paused, true);
  assert.equal(h.get("auto-continue").checked, false); assert.equal(h.runtimeState()["session-A"].enabled, false);
  assert.equal(h.durable().process_auto_state["p|A"].enabled, false); assert.equal(h.durable().process_auto_state["p|A"].paused, true);
});
test("mode migration publishes only real missing modes and plain echo publishes no preference map", async () => {
  const initial = autoFixture({ "p|A": { enabled: true }, "p|B": { enabled: true, mode: "dev-pair" } }), h = await harness(initial);
  await h.ready(); const writes = prefsWrites(h);
  assert.deepEqual(writes.at(-1).args.process_auto_state, { "p|A": { mode: "dev-auto" } });
  const count = writes.length; h.runtime.applyAutoUiState("p|A"); await drain(); assert.equal(prefsWrites(h).length, count);
  assert.equal(h.durable().process_auto_state["p|B"].mode, "dev-pair");
});
test("current completion selection still inserts its actual path", async () => {
  const h = await harness({}, { holdLookups: true }); h.core.promptBox.value = "@src"; h.core.promptBox.setSelectionRange(4, 4);
  const pending = h.runtime.refreshFileSuggestions(); await drain(); h.lookups[0].resolve(["src/current.rs"]); await pending;
  h.runtime.chooseFileSuggestion(0); assert.equal(h.core.promptBox.value, "@src/current.rs "); assert.equal(h.runtime.fileSuggestions.length, 0);
});
test("a loaded old project's completion cannot be chosen after a project switch", async () => {
  const h = await harness({}, { holdLookups: true }); h.core.promptBox.value = "@src"; h.core.promptBox.setSelectionRange(4, 4);
  const pending = h.runtime.refreshFileSuggestions(); await drain(); h.lookups[0].resolve(["src/private-A.rs"]); await pending;
  h.setProject("project-B"); h.runtime.chooseFileSuggestion(0); assert.equal(h.core.promptBox.value, "@src");
});
test("stale completion rejection cannot clear a newer successful list", async () => {
  const h = await harness({}, { holdLookups: true }); h.core.promptBox.value = "@src"; h.core.promptBox.setSelectionRange(4, 4);
  const old = h.runtime.refreshFileSuggestions(); await drain(); const newer = h.runtime.refreshFileSuggestions(); await drain();
  h.lookups[1].resolve(["src/current.rs"]); await newer; h.lookups[0].reject(new Error("old lookup failed")); await old;
  assert.deepEqual([...h.runtime.fileSuggestions], ["src/current.rs"]);
});
test("closing and reopening SOP keeps the newer real result against old success or error", async () => {
  for (const outcome of ["success", "error"]) {
    const h = await harness({}, { holdLookups: true }); const old = h.runtime.openSopPicker(); await drain();
    await h.runtime.openSopPicker(); const newer = h.runtime.openSopPicker(); await drain();
    h.lookups[2].resolve([{ title: "Current workflow", body: "current command", status: "active" }]); h.lookups[3].resolve([]); await newer;
    if (outcome === "success") h.lookups[0].resolve([{ title: "Old workflow", body: "old command", status: "active" }]);
    else h.lookups[0].reject(new Error("old SOP failed")); h.lookups[1].resolve([]); await old;
    const entries = h.get("sop-list").children.filter(el => el.classList.contains("sop-entry")); assert.equal(entries.length, 1);
    assert.equal(entries[0].children[0].textContent, "Current workflow"); entries[0].dispatchEvent({ type: "click" }); await drain();
    assert.equal(h.calls.find(call => call.command === "run_prompt").args.prompt, "current command");
  }
});
test("a loaded SOP button checks project again while the valid same-project choice still runs", async () => {
  for (const switched of [false, true]) {
    const h = await harness({}, { holdLookups: true }); const pending = h.runtime.openSopPicker(); await drain();
    h.lookups[0].resolve([{ title: "Original workflow", body: "original command", status: "active" }]); h.lookups[1].resolve([]); await pending;
    if (switched) h.setProject("project-B");
    h.get("sop-list").children.find(el => el.classList.contains("sop-entry")).dispatchEvent({ type: "click" }); await drain();
    const request = h.calls.find(call => call.command === "run_prompt");
    if (switched) { assert.equal(request, undefined); assert.equal(h.core.promptBox.value, ""); }
    else { assert.equal(request.args.projectDir, "project-A"); assert.equal(request.args.prompt, "original command"); }
  }
});
