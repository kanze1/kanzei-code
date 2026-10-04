// A4 executes complete UI owner modules and the actual stream/history callers.
// Controlled native receipts model prefs.rs field/layout merge semantics.
// --before replays the exact baseline owners without changing production files.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import vm from "node:vm";

const root = resolve(import.meta.dirname, "..");
const baseline = "0e5355c67e716afcb3e9ad892b7e74245bb25d24";
const before = process.argv.includes("--before");
const clone = value => JSON.parse(JSON.stringify(value));
const deferred = () => {
  let resolvePromise, reject;
  const promise = new Promise((resolveValue, rejectValue) => { resolvePromise = resolveValue; reject = rejectValue; });
  return { promise, resolve: resolvePromise, reject };
};
const flush = async () => { await new Promise(resolveValue => setImmediate(resolveValue)); };

class Element {
  constructor(tag = "div") {
    this.tagName = tag.toUpperCase(); this.children = []; this.dataset = {}; this.listeners = new Map();
    this.attributes = new Map(); this.value = ""; this.options = []; this.checked = false; this.disabled = false;
    this.classes = new Set(); this.ownText = ""; this.scrollHeight = 100; this.scrollTop = 0; this.style = { setProperty() {} };
    this.classList = {
      add: (...values) => values.forEach(value => this.classes.add(value)),
      remove: (...values) => values.forEach(value => this.classes.delete(value)),
      contains: value => this.classes.has(value),
      toggle: (value, force) => { const enabled = force ?? !this.classes.has(value); enabled ? this.classes.add(value) : this.classes.delete(value); return enabled; },
    };
  }
  set className(value) { this.classes = new Set(value.split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classes].join(" "); }
  set textContent(value) { this.ownText = String(value); this.replaceChildren(); }
  get textContent() { return this.ownText + this.children.map(child => child.textContent).join(""); }
  get childNodes() { return this.children; }
  get childElementCount() { return this.children.length; }
  get firstElementChild() { return this.children[0]; }
  get isConnected() { return true; }
  get parentElement() { return this.parentNode; }
  append(...children) { for (const child of children) { child.parentNode = this; this.children.push(child); } }
  appendChild(child) { this.append(child); return child; }
  replaceChildren(...children) { for (const child of this.children) child.parentNode = null; this.children = []; this.append(...children); }
  remove() { if (this.parentNode) this.parentNode.children = this.parentNode.children.filter(child => child !== this); this.parentNode = null; }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  hasAttribute(name) { return this.attributes.has(name); }
  removeAttribute(name) { this.attributes.delete(name); }
  addEventListener(name, callback) { const values = this.listeners.get(name) ?? []; values.push(callback); this.listeners.set(name, values); }
  contains(node) { return this === node || this.children.some(child => child.contains(node)); }
  matches(selector) {
    if (selector.startsWith(".")) {
      const match = selector.match(/^\.([\w-]+)(?:\[data-session-id="([^"]*)"\])?$/);
      return !!match && this.classes.has(match[1]) && (match[2] === undefined || this.dataset.sessionId === match[2]);
    }
    return this.tagName === selector.toUpperCase();
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; }
  querySelectorAll(selector) { return this.children.flatMap(child => [...(child.matches(selector) ? [child] : []), ...child.querySelectorAll(selector)]); }
}

function mergePrefs(target, patch) {
  const next = clone(target);
  for (const key of ["theme", "backdrop", "work_priority", "auto_max", "continue_prompt", "process_auto_state", "workspace_state"]) {
    if (patch[key] !== undefined && patch[key] !== null) next[key] = clone(patch[key]);
  }
  if (["list", "graph"].includes(patch.memory_view)) next.memory_view = patch.memory_view;
  if (patch.ui_layout && typeof patch.ui_layout === "object" && !Array.isArray(patch.ui_layout) && Buffer.byteLength(JSON.stringify(patch.ui_layout)) <= 64 * 1024) {
    next.ui_layout = target.ui_layout && typeof target.ui_layout === "object" && !Array.isArray(target.ui_layout) ? clone(target.ui_layout) : {};
    for (const [section, bucket] of Object.entries(patch.ui_layout)) {
      if (bucket === null) delete next.ui_layout[section];
      else if (typeof bucket === "object" && !Array.isArray(bucket)) {
        const merged = next.ui_layout[section] && typeof next.ui_layout[section] === "object" && !Array.isArray(next.ui_layout[section]) ? next.ui_layout[section] : {};
        for (const [key, value] of Object.entries(bucket)) { if (value === null) delete merged[key]; else merged[key] = value; }
        next.ui_layout[section] = merged;
      } else next.ui_layout[section] = bucket;
    }
  }
  return next;
}

async function harness(prefs = {}, localTheme = null) {
  const elements = new Map();
  const get = id => { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); };
  const placeholder = new Element(); placeholder.className = "msg-pane"; placeholder.dataset.sessionId = "";
  get("messages").append(placeholder);
  const initializers = [], subscriptions = new Map(), timers = new Map(), frames = [];
  const calls = [], logs = [], handlers = new Map(), storage = new Map(localTheme ? [["kz-theme", localTheme]] : []);
  let durable = clone(prefs), nextTimer = 0;
  const native = async (command, args = {}) => {
    calls.push({ command, args: clone(args) });
    if (handlers.has(command)) return handlers.get(command)(args);
    if (command === "ui_prefs_get") return clone(durable);
    if (command === "ui_prefs_set") { durable = mergePrefs(durable, args); return null; }
    return [];
  };
  const document = {
    readyState: "loading", body: new Element("body"), documentElement: new Element("html"), title: "Kanzei", hidden: false,
    getElementById: get, createElement: tag => new Element(tag), createTextNode: text => { const el = new Element(); el.textContent = text; return el; },
    addEventListener: (name, callback) => { if (name === "DOMContentLoaded") initializers.push(callback); },
    dispatchEvent() {}, querySelector: () => null, querySelectorAll: () => [], hasFocus: () => true,
  };
  const context = vm.createContext({
    document, window: { __TAURI__: { core: { invoke: native }, event: { listen: async (name, callback) => subscriptions.set(name, callback) } }, addEventListener() {} },
    console: { log() {}, warn: (...args) => logs.push(args), error: (...args) => logs.push(args) }, navigator: {}, TextEncoder,
    CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init?.detail; } },
    localStorage: { getItem: key => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) },
    performance: { now: () => 10 }, requestAnimationFrame: callback => frames.push(callback),
    setTimeout: (callback, ms = 0) => { const id = ++nextTimer; timers.set(id, { callback, ms }); return id; }, clearTimeout: id => timers.delete(id),
    setInterval: () => ++nextTimer, clearInterval() {},
  });
  const names = ["03-shell.js", "01-core.js", "05-chat-render.js", "07-events.js", "15-views-misc.js"];
  const sources = new Map(names.map(name => {
    const path = `crates/kanzei-app/ui/${name}`;
    return [name, before && ["03-shell.js", "01-core.js"].includes(name)
      ? execFileSync("git", ["show", `${baseline}:${path}`], { cwd: root, encoding: "utf8" }) : readFileSync(resolve(root, path), "utf8")];
  }));
  const imported = new Map();
  for (const source of sources.values()) {
    for (const [, body, specifier] of source.matchAll(/(?:import|export)\s*\{([\s\S]*?)\}\s*from\s*"([^"]+)"/g)) {
      const exports = imported.get(specifier) ?? new Set();
      for (const name of body.split(",").map(value => value.trim()).filter(Boolean)) exports.add(name.split(/\s+as\s+/)[0]);
      imported.set(specifier, exports);
    }
  }
  const deps = new Map(), noop = () => {};
  for (const [specifier, exports] of imported) {
    if (sources.has(specifier.slice(2))) continue;
    const values = specifier === "./02-i18n.js" ? { t: text => text, localizeDynamic: text => text, I18N_EN: {}, localizedStage: text => text, languageIsEnglish: () => false } :
      specifier === "./03-general-scope.js" ? { isGeneralChat: () => false } :
      specifier === "./03-workspaces.js" ? { active_space: "dev", view_allowed: () => true } :
      specifier === "./25-softwire-model.js" ? { sameProject: (a, b) => a === b } :
      specifier === "./04-structured-parse.js" ? { stripInternalHandoff: text => text, parseErrorText: () => ({ chain: [] }) } :
      specifier === "./04-markdown.js" ? { renderMarkdownInto: (el, text) => { el.textContent = text; } } :
      specifier === "./09-sessions.js" ? { processSwitchGeneration: 0 } :
      specifier === "./08-auto.js" ? { autoContinueTimers: new Map() } :
      specifier === "./08-compose.js" ? { state: {} } : {};
    deps.set(specifier, new vm.SyntheticModule([...exports], function () {
      for (const name of exports) this.setExport(name, Object.hasOwn(values, name) ? values[name] : noop);
    }, { context, identifier: specifier }));
  }
  const modules = new Map([...sources].map(([name, source]) => [name, new vm.SourceTextModule(source, { context, identifier: name })]));
  const entry = modules.get("03-shell.js");
  await entry.link(specifier => modules.get(specifier.slice(2)) ?? deps.get(specifier));
  await entry.evaluate();
  // 07 is a consumer, so link/evaluate it after owners if not reached already.
  for (const module of modules.values()) { if (module.status === "unlinked") await module.link(specifier => modules.get(specifier.slice(2)) ?? deps.get(specifier)); if (module.status === "linked") await module.evaluate(); }
  const runTimers = () => { for (const [id, timer] of [...timers]) if (timer.ms === 0) { timers.delete(id); timer.callback(); } };
  return {
    shell: modules.get("03-shell.js").namespace, core: modules.get("01-core.js").namespace, chat: modules.get("05-chat-render.js").namespace,
    views: modules.get("15-views-misc.js").namespace, calls, handlers, logs, storage, get, durable: () => clone(durable),
    commitPrefs: patch => { durable = mergePrefs(durable, patch); },
    initialize: marker => { const fn = initializers.find(callback => callback.toString().includes(marker)); assert(fn, `initializer ${marker} exists`); fn(); },
    registerStream: async () => { for (const name of ["kz:text", "kz:reasoning"]) { const fn = initializers.find(callback => callback.toString().includes(`on("${name}"`)); assert(fn); fn(); } runTimers(); await flush(); },
    deliver: (name, payload) => { assert(subscriptions.has(name), `native subscription ${name}`); return subscriptions.get(name)({ payload }); },
    show: session => { modules.get("03-shell.js").namespace.setActiveSessionId(session); return modules.get("01-core.js").namespace.showPane(session); },
    finishFrames: () => { for (const callback of frames.splice(0)) callback(); },
  };
}

test("theme boot with missing local cache preserves persisted light preference", async () => {
  const h = await harness({ theme: "light" });
  h.shell.initTheme(); await flush();
  assert.equal(h.durable().theme, "light", "rendering startup default must not overwrite backend preference");
  assert.equal(h.shell.currentTheme(), "light");
  assert.equal(h.calls.filter(call => call.command === "ui_prefs_set").length, 0, "hydration must not write back defaults");
});

test("late startup theme read cannot replace a user's newer choice", async () => {
  const h = await harness({ theme: "dark" }), read = deferred();
  h.handlers.set("ui_prefs_get", () => read.promise);
  h.shell.initTheme(); await flush(); h.shell.applyTheme("light");
  read.resolve({ theme: "dark" }); await flush();
  assert.equal(h.shell.currentTheme(), "light", "newer local intent wins over delayed hydration");
  assert.equal(h.durable().theme, "light");
});

test("only a valid legacy theme is migrated when backend theme is absent", async () => {
  const h = await harness({}, "light");
  h.shell.initTheme(); await flush();
  assert.equal(h.shell.currentTheme(), "light"); assert.equal(h.durable().theme, "light");
  const empty = await harness({}, "invalid"); empty.shell.initTheme(); await flush();
  assert.equal(empty.shell.currentTheme(), "dark");
  assert.equal(empty.calls.filter(call => call.command === "ui_prefs_set").length, 0, "missing/invalid legacy preference is not a save");
});

test("prefs read receipt cannot arrive after and replace a newer successful write", async () => {
  const h = await harness({ theme: "dark" }), read = deferred();
  h.handlers.set("ui_prefs_get", () => read.promise);
  const loading = h.core.uiPrefsLoad(); await flush();
  const saving = h.core.uiPrefsSave({ theme: "light" }); await flush();
  read.resolve({ theme: "dark" }); await loading; await saving;
  assert.equal((await h.core.uiPrefsLoad()).theme, "light", "late old get must not replace the successfully saved preference"); assert.equal(h.durable().theme, "light");
});

test("consecutive prefs writes retain call order even when native first receipt is delayed", async () => {
  const h = await harness({ theme: "dark" }), first = deferred();
  await h.core.uiPrefsLoad();
  h.handlers.set("ui_prefs_set", async args => { if (args.theme === "light") await first.promise; h.commitPrefs(args); return null; });
  const one = h.core.uiPrefsSave({ theme: "light" }); const two = h.core.uiPrefsSave({ theme: "dark" }); await flush();
  first.resolve(null); await one; await two;
  assert.equal(h.durable().theme, "dark", "last user choice must remain the last durable write");
  assert.equal((await h.core.uiPrefsLoad()).theme, "dark");
});

test("prefs cache preserves layout siblings and follows exact key/section null deletion", async () => {
  const h = await harness({ ui_layout: { prefs: { language: "zh", conversation_mode: "general" }, frames: { panel: { left: 10, width: 20 } }, splits: { sidebar: 200 } } });
  await h.core.uiPrefsLoad();
  await h.core.uiPrefsSave({ ui_layout: { prefs: { language: "en" }, frames: { panel: { right: 5 } } } });
  assert.deepEqual(clone(await h.core.uiPrefsLoad()), h.durable(), "frontend cache must match durable two-level merge, including atomic geometry replacement");
  await h.core.uiPrefsSave({ ui_layout: { prefs: { language: null }, splits: null } });
  assert.deepEqual(clone(await h.core.uiPrefsLoad()), h.durable(), "null deletes a key or a section instead of retaining null in cache");
  assert.equal((await h.core.uiPrefsLoad()).ui_layout.prefs.conversation_mode, "general");
});

test("failed prefs write retains committed cache and queue continues to later read/save", async () => {
  const h = await harness({ theme: "dark", ui_layout: { prefs: { language: "zh" } } });
  await h.core.uiPrefsLoad();
  h.handlers.set("ui_prefs_set", () => { throw new Error("native persistence rejected"); });
  await h.core.uiPrefsSave({ theme: "light" });
  assert.equal((await h.core.uiPrefsLoad()).theme, "dark");
  h.handlers.delete("ui_prefs_set"); await h.core.uiPrefsSave({ theme: "light" });
  assert.equal((await h.core.uiPrefsLoad(true)).theme, "light");
});

test("real text delta after the cached history switch belongs to B and switching back resumes A", async () => {
  const h = await harness(); await h.registerStream();
  h.shell.setCurrentProject("C:/fixture/project"); h.shell.setActiveProcessId("process-a"); h.show("session-a");
  h.deliver("kz:text", { sessionId: "session-a", text: "A1" });
  const a = h.shell.currentAssistant;
  h.deliver("kz:text", { sessionId: "session-b", text: "B1" });
  const b = h.core.streamStateFor("session-b").assistant;
  // The real 09 switch contract changes process/session before its 15 history await.
  h.shell.setActiveProcessId("process-b"); h.shell.setActiveSessionId("session-b");
  const historyReads = h.calls.filter(call => call.command === "conversation_display_get").length;
  await h.views.loadConversation();
  assert.equal(h.calls.filter(call => call.command === "conversation_display_get").length, historyReads, "actual 15 cached-pane shortcut must execute");
  h.deliver("kz:text", { sessionId: "session-b", text: "B2" });
  assert.equal(a.dataset.raw, "A1", "B's live delta cannot write A's hidden assistant"); assert.equal(b.dataset.raw, "B1B2");
  h.deliver("kz:text", { sessionId: "session-a", text: "A2" });
  h.shell.setActiveProcessId("process-a"); h.shell.setActiveSessionId("session-a"); await h.views.loadConversation();
  h.deliver("kz:text", { sessionId: "session-a", text: "A3" });
  assert.equal(a.dataset.raw, "A1A2A3");
  assert.equal(h.core.messagePanes.get("session-a").querySelectorAll(".assistant").length, 1, "returning A must resume its original block, not duplicate it");
});

test("real reasoning delta and head retain their session when swapping cached panes", async () => {
  const h = await harness(); await h.registerStream(); h.show("session-a");
  h.deliver("kz:reasoning", { sessionId: "session-a", text: "A thought" });
  const a = h.shell.currentReasoning, aHead = h.chat.currentReasoningHead;
  h.deliver("kz:reasoning", { sessionId: "session-b", text: "B thought" });
  const b = h.core.streamStateFor("session-b").reasoning, bHead = h.core.streamStateFor("session-b").reasoningHead;
  h.show("session-b"); h.deliver("kz:reasoning", { sessionId: "session-b", text: " + B" });
  assert.equal(a.dataset.raw, "A thought"); assert.equal(b.dataset.raw, "B thought + B"); assert.equal(h.chat.currentReasoningHead, bHead);
  h.show("session-a"); assert.equal(h.chat.currentReasoningHead, aHead);
});

test("same-pane redisplay retains live pointers and throwing temporary render restores them", async () => {
  const h = await harness(); await h.registerStream(); h.show("session-a");
  h.deliver("kz:text", { sessionId: "session-a", text: "A" }); const a = h.shell.currentAssistant;
  h.core.showPane("session-a"); assert.equal(h.shell.currentAssistant, a);
  assert.throws(() => h.core.withSessionRender("session-b", () => { throw new Error("render failure"); }));
  assert.equal(h.shell.currentAssistant, a); assert.equal(h.core.activePane.dataset.sessionId, "session-a"); assert.equal(h.core.renderingBackground, false);
});

test("nested background render restores each existing owner and leaves the visible pane active", async () => {
  const h = await harness(); await h.registerStream(); h.show("session-a");
  h.deliver("kz:text", { sessionId: "session-a", text: "A" }); const a = h.shell.currentAssistant;
  h.core.withSessionRender("session-b", () => {
    h.chat.appendAssistant("B"); const b = h.shell.currentAssistant;
    h.core.withSessionRender("session-c", () => h.chat.appendAssistant("C"));
    assert.equal(h.shell.currentAssistant, b); assert.equal(h.core.activePane.dataset.sessionId, "session-b");
    assert.equal(h.core.renderingBackground, true);
  });
  assert.equal(h.shell.currentAssistant, a); assert.equal(h.core.activePane.dataset.active, "1");
  assert.equal(h.core.messagePanes.get("session-b").dataset.active, undefined);
  assert.equal(h.core.messagePanes.get("session-c").dataset.active, undefined);
  assert.equal(h.core.streamStateFor("session-b").assistant.dataset.raw, "B");
  assert.equal(h.core.streamStateFor("session-c").assistant.dataset.raw, "C");
});

test("discard and eviction release only idle background pane state, retaining active/running owners", async () => {
  const h = await harness(); await h.registerStream(); h.show("session-a");
  h.deliver("kz:text", { sessionId: "session-a", text: "A" }); const a = h.shell.currentAssistant;
  h.deliver("kz:text", { sessionId: "session-b", text: "discard" }); h.core.discardSessionPane("session-b");
  assert.equal(h.core.messagePanes.has("session-b"), false); assert.equal(h.core.sessionStreams.has("session-b"), false);
  h.shell.sessionState("session-running").phase = "running";
  for (const id of ["session-running", "session-oldest", "session-idle-2", "session-idle-3"]) {
    h.deliver("kz:text", { sessionId: id, text: id });
    if (id !== "session-running") h.shell.sessionState(id).phase = "idle";
    h.core.messagePanes.get(id).dataset.usedAt = id === "session-oldest" ? "1" : "2";
  }
  h.core.evictStalePanes();
  assert.equal(h.core.messagePanes.has("session-oldest"), false); assert.equal(h.core.sessionStreams.has("session-oldest"), false);
  assert.equal(h.core.messagePanes.has("session-running"), true); assert.equal(h.core.messagePanes.has("session-a"), true);
  assert.equal(h.shell.currentAssistant, a);
});

test("real clearChat resets the active pointer before later switching and streaming", async () => {
  const h = await harness(); await h.registerStream(); h.show("session-a");
  h.deliver("kz:text", { sessionId: "session-a", text: "old A" });
  h.views.clearChat(); assert.equal(h.shell.currentAssistant, null); assert.equal(h.core.activePane.children.length, 0);
  h.show("session-b"); h.deliver("kz:text", { sessionId: "session-b", text: "B" });
  h.show("session-a"); h.deliver("kz:text", { sessionId: "session-a", text: "new A" });
  assert.equal(h.shell.currentAssistant.dataset.raw, "new A");
  assert.equal(h.core.activePane.querySelectorAll(".assistant").length, 1);
});

test("failed initial prefs read can be retried without trapping later queued operations", async () => {
  const h = await harness({ theme: "light" });
  h.handlers.set("ui_prefs_get", () => { throw new Error("read unavailable"); });
  assert.deepEqual(clone(await h.core.uiPrefsLoad()), {});
  h.handlers.delete("ui_prefs_get"); assert.equal((await h.core.uiPrefsLoad()).theme, "light");
  await h.core.uiPrefsSave({ theme: "dark" }); assert.equal((await h.core.uiPrefsLoad(true)).theme, "dark");
});

test("cache preserves fields which native prefs intentionally ignores", async () => {
  const h = await harness({ theme: "dark", memory_view: "graph", ui_layout: { prefs: { language: "zh" } } });
  await h.core.uiPrefsLoad();
  await h.core.uiPrefsSave({ theme: null, memory_view: "invalid", ui_layout: "invalid" });
  assert.deepEqual(clone(await h.core.uiPrefsLoad()), h.durable(), "null Option and ignored values must not become cache writes");
  await h.core.uiPrefsSave({ ui_layout: { frames: { huge: "界".repeat(24000) } } });
  assert.deepEqual(clone(await h.core.uiPrefsLoad()), h.durable(), "native UTF-8 layout size rejection must also leave cache unchanged");
});
