// A6 loads complete production owner modules; native receipts are controlled.
// --before loads the complete committed owners without mutating production.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import vm from "node:vm";
import { createFixtures } from "./ui-preview/fixtures.mjs";
import { mergeWorkspacePrefs } from "./ui-preview/workspace-prefs-fixture.mjs";

const root = resolve(import.meta.dirname, "..");
const baseline = "7fbe6a9f692b3d5cb6940987a707a02532a16c1b";
const before = process.argv.includes("--before");
const clone = value => JSON.parse(JSON.stringify(value));
const drain = async () => { await new Promise(done => setImmediate(done)); };
function deferred() { let resolvePromise; const promise = new Promise(done => { resolvePromise = done; }); return { promise, resolve: resolvePromise }; }
class Element {
  constructor(tag = "div") {
    this.tagName = tag.toUpperCase(); this.children = []; this.dataset = {}; this.attributes = new Map(); this.listeners = new Map(); this.classes = new Set(); this.value = "";
    this.css = new Map(); this.style = { setProperty: (key, value) => this.css.set(key, value), removeProperty: key => this.css.delete(key) };
    this.classList = { add: (...keys) => keys.forEach(key => this.classes.add(key)), remove: (...keys) => keys.forEach(key => this.classes.delete(key)), contains: key => this.classes.has(key), toggle: (key, value) => { const on = value ?? !this.classes.has(key); on ? this.classes.add(key) : this.classes.delete(key); return on; } };
  }
  set className(value) { this.classes = new Set(value.split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classes].join(" "); }
  get isConnected() { return true; }
  get parentElement() { return this.parentNode; }
  append(...children) { for (const child of children) { child.parentNode = this; this.children.push(child); } }
  appendChild(child) { this.append(child); return child; }
  replaceChildren(...children) { this.children = []; this.append(...children); }
  setAttribute(key, value) { this.attributes.set(key, String(value)); }
  getAttribute(key) { return this.attributes.get(key) ?? null; }
  hasAttribute(key) { return this.attributes.has(key); }
  removeAttribute(key) { this.attributes.delete(key); }
  addEventListener(key, callback) { const values = this.listeners.get(key) ?? []; values.push(callback); this.listeners.set(key, values); }
  removeEventListener(key, callback) { this.listeners.set(key, (this.listeners.get(key) ?? []).filter(fn => fn !== callback)); }
  querySelector() { return null; }
  querySelectorAll() { return []; }
  getBoundingClientRect() { return { left: 0, top: 0, right: 300, bottom: 600, width: 300, height: 600 }; }
}
async function harness(initial, { project = "project-A", backend = { value: clone(initial) } } = {}) {
  const getGate = deferred(), elements = new Map(), initializers = [], storage = new Map(), calls = [];
  let timer = 0, failSave = false;
  const get = id => { if (!elements.has(id)) { const el = new Element(); el.id = id; elements.set(id, el); } return elements.get(id); };
  const shellValues = { currentProject: project, activeProcessId: "d|A", activeSessionId: "session-A", processItems: [], sessionStates: new Map(), attachments: [] };
  const timers = new Map();
  const native = async (command, args = {}) => {
    calls.push({ command, args: clone(args) });
    if (command === "ui_prefs_get") return getGate.promise;
    if (command === "ui_prefs_set") {
      if (failSave) { failSave = false; throw new Error("injected native write rejection"); }
      const durable = backend.value;
      if (args.workspace_state) durable.workspace_state = before ? clone(args.workspace_state) : mergeWorkspacePrefs(durable.workspace_state, args.workspace_state);
      for (const [section, bucket] of Object.entries(args.ui_layout ?? {})) {
        durable.ui_layout ??= {}; durable.ui_layout[section] ??= {};
        for (const [key, value] of Object.entries(bucket)) { if (value === null) delete durable.ui_layout[section][key]; else durable.ui_layout[section][key] = clone(value); }
      }
      return null;
    }
    return [];
  };
  const document = { readyState: "loading", activeElement: null, body: new Element("body"), documentElement: new Element("html"), getElementById: get, createElement: tag => new Element(tag), querySelector: () => null, querySelectorAll: () => [], dispatchEvent() {}, addEventListener: (event, callback) => { if (event === "DOMContentLoaded") initializers.push(callback); } };
  const context = vm.createContext({ document, console: { warn() {}, error() {}, log() {} }, navigator: {}, TextEncoder, performance: { now: () => 0 },
    window: { innerWidth: 1200, innerHeight: 900, __TAURI__: { core: { invoke: native }, event: { listen() {} } }, addEventListener() {} },
    CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init?.detail; } },
    localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value), removeItem: key => storage.delete(key) },
    setTimeout: (callback, ms) => { const id = ++timer; timers.set(id, { callback, ms }); return id; }, clearTimeout: id => timers.delete(id), setInterval: () => ++timer, clearInterval() {}, requestAnimationFrame() {},
  });
  const names = ["00-surface.js", "00-frame.js", "01-core.js", "03-layout.js", "03-workspaces.js"];
  const sources = new Map(names.map(name => [name, before ? execFileSync("git", ["show", `${baseline}:crates/kanzei-app/ui/${name}`], { cwd: root, encoding: "utf8" }) : readFileSync(resolve(root, "crates/kanzei-app/ui", name), "utf8")]));
  const needed = new Map();
  for (const source of sources.values()) for (const [, body, specifier] of source.matchAll(/(?:import|export)\s*\{([\s\S]*?)\}\s*from\s*"([^"]+)"/g)) {
    const names = needed.get(specifier) ?? new Set(); for (const name of body.split(",").map(value => value.trim()).filter(Boolean)) names.add(name.split(/\s+as\s+/)[0]); needed.set(specifier, names);
  }
  const deps = new Map(), noop = () => {};
  for (const [specifier, exports] of needed) {
    if (sources.has(specifier.slice(2))) continue;
    const values = specifier === "./03-shell.js" ? shellValues : specifier === "./02-i18n.js" ? { t: value => value } : specifier === "./03-general-scope.js" ? { isGeneralChat: () => false } : specifier === "./08-auto.js" ? { autoContinueTimers: new Map() } : specifier === "./08-compose.js" ? { state: {} } : {};
    deps.set(specifier, new vm.SyntheticModule([...exports], function () { for (const key of exports) this.setExport(key, Object.hasOwn(values, key) ? values[key] : noop); }, { context }));
  }
  const modules = new Map([...sources].map(([name, source]) => [name, new vm.SourceTextModule(source, { context, identifier: name })]));
  for (const module of modules.values()) { if (module.status === "unlinked") await module.link(specifier => modules.get(specifier.slice(2)) ?? deps.get(specifier)); if (module.status === "linked") await module.evaluate(); }
  return { core: modules.get("01-core.js").namespace, layout: modules.get("03-layout.js").namespace, workspace: modules.get("03-workspaces.js").namespace, get, document, calls, durable: () => clone(backend.value), rejectNextSave: () => { failSave = true; }, resolveGet: () => getGate.resolve(clone(initial)), initializeLayout: () => { const callback = initializers.find(fn => fn.toString().includes("bindFrames(document)")); assert(callback); callback(); } };
}

test("actual initialization keeps a user split flushed while the old get is waiting", async () => {
  const h = await harness({ ui_layout: { splits: { sidebar: 250 } } });
  h.initializeLayout(); await drain();
  h.layout.layoutFrameStore.set("splits", "sidebar", 360, "kz-sidebar-width"); h.layout.flushLayout();
  h.resolveGet(); await drain();
  assert.equal(h.durable().ui_layout.splits.sidebar, 360);
  assert.equal(h.core.uiPrefsCache.ui_layout.splits.sidebar, 360, "the existing core queue already preserves durable cache");
  assert.equal(h.layout.layoutPref("splits", "sidebar"), 360, "late hydration must preserve the user split even after flush clears pending");
  assert.equal(h.document.documentElement.css.get("--kz-split-sidebar"), "360px");
});
test("actual initialization cannot resurrect a flushed null reset", async () => {
  const h = await harness({ ui_layout: { splits: { sidebar: 350 } } });
  h.initializeLayout(); await drain(); h.layout.layoutFrameStore.set("splits", "sidebar", null, "kz-sidebar-width"); h.layout.flushLayout();
  h.resolveGet(); await drain();
  assert.equal(h.durable().ui_layout.splits.sidebar, undefined);
  assert.equal(h.layout.layoutPref("splits", "sidebar"), null, "a reset must remain absent after stale initialization");
  assert.equal(h.document.documentElement.css.has("--kz-split-sidebar"), false);
});
test("hydration preserves later pending edits above an earlier flushed edit", async () => {
  const h = await harness({ ui_layout: { splits: { sidebar: 250 }, side_panel: { auto_close: false } } });
  h.initializeLayout(); await drain(); h.layout.setLayoutPref("splits", "sidebar", 320); h.layout.flushLayout(); h.layout.setLayoutPref("splits", "sidebar", 400);
  h.resolveGet(); await drain();
  assert.equal(h.layout.layoutPref("splits", "sidebar"), 400);
  assert.equal(h.layout.sidePanelPrefs().autoClose, false, "unmodified remote sections still hydrate");
  h.layout.flushLayout(); await drain(); assert.equal(h.durable().ui_layout.splits.sidebar, 400);
});
test("actual workspace restore cannot overwrite a view selected during the get", async () => {
  const h = await harness({ workspace_state: { "project-A": { dev: { view: "chat", process_id: "d|A" } }, remote: { dev: { view: "documents" } } } });
  const restoring = h.workspace.restore_workspace_preferences(); await drain(); h.workspace.remember_workspace_view("lines");
  h.resolveGet(); await restoring; await drain();
  assert.equal(h.durable().workspace_state["project-A"].dev.view, "lines");
  assert.equal(h.workspace.project_workspace("project-A").dev.view, "lines", "late restore cannot replace the new navigation projection");
  assert.equal(h.workspace.project_workspace("remote").dev.view, "documents");
});
test("unmodified workspace restore still adopts the durable saved view", async () => {
  const h = await harness({ workspace_state: { "project-A": { dev: { view: "documents" } } } });
  const restoring = h.workspace.restore_workspace_preferences(); h.resolveGet(); await restoring;
  assert.equal(h.workspace.project_workspace("project-A").dev.view, "documents");
});

test("independent real module windows preserve different projects and send only touched fields", async () => {
  const initial = { workspace_state: { "project-A": { dev: { view: "chat", process_id: "d|A" } }, "project-B": { dev: { view: "chat", process_id: "d|B" } } } };
  const backend = { value: clone(initial) };
  const a = await harness(initial, { backend }); const b = await harness(initial, { project: "project-B", backend });
  for (const h of [a, b]) { const restoring = h.workspace.restore_workspace_preferences(); h.resolveGet(); await restoring; }
  a.workspace.remember_workspace_view("lines"); await drain();
  b.workspace.remember_workspace_view("arch"); await drain();
  const state = backend.value.workspace_state;
  assert.equal(state["project-A"].dev.view, "lines", "B's stale snapshot must not revert A");
  assert.equal(state["project-B"].dev.view, "arch"); assert.equal(state["project-A"].dev.process_id, "d|A");
  const patch = b.calls.find(call => call.command === "ui_prefs_set").args.workspace_state;
  assert.deepEqual(patch, { "project-B": { dev: { view: "arch" } } }, "unchanged project/process/section fields are not writes");
  assert.equal(b.core.uiPrefsCache.workspace_state["project-A"].dev.view, "chat", "this window is not a new cross-window live registry");
  assert.equal(b.core.uiPrefsCache.workspace_state["project-B"].dev.process_id, "d|B", "success cache uses the same merge contract");
});
test("independent real module windows keep separate topic states and explicit null resets", async () => {
  const initial = { workspace_state: { "@research-library": { space: "dev", research: { topic_id: "origin", topic: "old", page: "overview", process_id: "p|old" }, topic_states: { origin: { topic_id: "origin", page: "overview" } } } } };
  const backend = { value: clone(initial) };
  const a = await harness(initial, { backend }); const b = await harness(initial, { backend });
  for (const h of [a, b]) { const restoring = h.workspace.restore_workspace_preferences(); h.resolveGet(); await restoring; }
  a.workspace.save_research_workspace({ topic_id: "alpha", topic: "Alpha", page: "writing" }); await drain();
  b.workspace.save_research_workspace({ topic_id: "beta", topic: "Beta", page: "reading" }); await drain();
  const library = backend.value.workspace_state["@research-library"];
  assert.equal(library.topic_states.alpha?.page, "writing", "B must not delete the saved Alpha topic");
  assert.equal(library.topic_states.beta.page, "reading"); assert.equal(library.topic_states.origin.page, "overview");
  assert.equal(library.research.process_id, null, "new topic intentionally resets the former process");
  assert.equal(library.topic_states.beta.process_id, null);
  assert.equal(library.space, "dev", "save research does not rewrite the active space");
});
test("workspace queue freezes each delta before a later edit of the same field", async () => {
  const h = await harness({ workspace_state: { "project-A": { dev: { view: "chat", process_id: "d|A" } } } });
  const restoring = h.workspace.restore_workspace_preferences(); h.resolveGet(); await restoring;
  h.workspace.remember_workspace_view("lines"); h.workspace.remember_workspace_view("arch"); await drain();
  const writes = h.calls.filter(call => call.command === "ui_prefs_set");
  assert.equal(writes[0].args.workspace_state["project-A"].dev.view, "lines", "queued first write cannot be rewritten by the second navigation");
  assert.equal(writes[1].args.workspace_state["project-A"].dev.view, "arch");
});
test("independent windows changing the same project's view and selected process keep both", async () => {
  const initial = { workspace_state: { "project-A": { dev: { view: "chat", process_id: "d|A", main_process_id: "d|A" } } } };
  const backend = { value: clone(initial) };
  const a = await harness(initial, { backend }); const b = await harness(initial, { backend });
  for (const h of [a, b]) { const restoring = h.workspace.restore_workspace_preferences(); h.resolveGet(); await restoring; }
  a.workspace.remember_workspace_view("lines"); await drain();
  b.workspace.adopt_process_workspace({ id: "p|new", profile: "dev" }); await drain();
  assert.equal(backend.value.workspace_state["project-A"].dev.view, "lines");
  assert.equal(backend.value.workspace_state["project-A"].dev.process_id, "p|new");
  assert.equal(backend.value.workspace_state["project-A"].dev.main_process_id, "d|A");
});
test("independent windows changing one topic's page and process keep both fields", async () => {
  const initial = { workspace_state: { "@research-library": {
    space: "research", research: { topic_id: "alpha", topic: "Alpha", page: "overview", process_id: "p|old", view: "research", category: "research" },
    topic_states: { alpha: { topic_id: "alpha", topic: "Alpha", page: "overview", process_id: "p|old", view: "research", category: "research" } },
  } } };
  const backend = { value: clone(initial) };
  const a = await harness(initial, { backend }); const b = await harness(initial, { backend });
  for (const h of [a, b]) { const restoring = h.workspace.restore_workspace_preferences(); h.resolveGet(); await restoring; }
  a.workspace.save_research_workspace({ page: "writing" }); await drain();
  b.workspace.save_research_workspace({ process_id: "p|new" }); await drain();
  const library = backend.value.workspace_state["@research-library"];
  assert.equal(library.research.page, "writing"); assert.equal(library.research.process_id, "p|new");
  assert.equal(library.topic_states.alpha.page, "writing"); assert.equal(library.topic_states.alpha.process_id, "p|new");
});
test("rejected workspace write cannot update success cache and the queue still accepts the next change", async () => {
  const h = await harness({ workspace_state: { "project-A": { dev: { view: "chat", process_id: "d|A" } } } });
  const restoring = h.workspace.restore_workspace_preferences(); h.resolveGet(); await restoring;
  h.rejectNextSave(); h.workspace.remember_workspace_view("lines"); await drain();
  assert.equal(h.durable().workspace_state["project-A"].dev.view, "chat");
  assert.equal(h.core.uiPrefsCache.workspace_state["project-A"].dev.view, "chat");
  h.workspace.remember_workspace_view("arch"); await drain();
  assert.equal(h.durable().workspace_state["project-A"].dev.view, "arch");
  assert.equal(h.core.uiPrefsCache.workspace_state["project-A"].dev.process_id, "d|A");
});
test("actual preview native fixture preserves different project deltas", () => {
  const { commands } = createFixtures({});
  commands.ui_prefs_set({ workspace_state: { A: { dev: { view: "lines", process_id: "d|A" } } } });
  commands.ui_prefs_set({ workspace_state: { B: { dev: { view: "arch" } } } });
  const prefs = commands.ui_prefs_get();
  assert.equal(prefs.workspace_state.A.dev.view, "lines"); assert.equal(prefs.workspace_state.B.dev.view, "arch");
});
