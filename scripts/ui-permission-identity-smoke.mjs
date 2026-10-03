// A3: execute the actual permission UI modules with controllable IPC/confirmation
// promises. These fixtures verify project/rule ownership, not layout or Rust IPC.
// --before loads the exact pre-fix source from Git for a data-failure replay.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import vm from "node:vm";

const root = resolve(import.meta.dirname, "..");
const before = process.argv.includes("--before");
const beforeCommit = "441e0c2983b8e8aeaf287bfc330d3df76d5b01b2";
const A = "C:/fixture/project-a";
const B = "C:/fixture/project-b";
const ruleA = { action: "bash", resource: "command A", effect: "allow" };
const ruleB = { action: "edit", resource: "file B", effect: "allow" };
const clone = value => JSON.parse(JSON.stringify(value));
const deferred = () => {
  let resolvePromise, reject;
  const promise = new Promise((resolveValue, rejectValue) => { resolvePromise = resolveValue; reject = rejectValue; });
  return { promise, resolve: resolvePromise, reject };
};
const flush = () => new Promise(resolveValue => setImmediate(resolveValue));

class Element {
  constructor(tag = "div") {
    this.tagName = tag.toUpperCase(); this.children = []; this.dataset = {}; this.listeners = new Map();
    this.attributes = new Map(); this.value = ""; this.options = []; this.checked = false; this.disabled = false;
    this.classes = new Set(); this.ownText = "";
    this.classList = {
      add: (...values) => values.forEach(value => this.classes.add(value)),
      remove: (...values) => values.forEach(value => this.classes.delete(value)),
      contains: value => this.classes.has(value),
      toggle: (value, force) => { const enabled = force ?? !this.classes.has(value); enabled ? this.classes.add(value) : this.classes.delete(value); return enabled; },
    };
  }
  set className(value) { this.classes = new Set(value.split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classes].join(" "); }
  set textContent(value) { this.ownText = String(value); this.children = []; }
  get textContent() { return this.ownText + this.children.map(child => child.textContent).join(""); }
  append(...children) { this.children.push(...children); }
  appendChild(child) { this.append(child); return child; }
  replaceChildren(...children) { this.ownText = ""; this.children = children; }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  addEventListener(name, callback) { const values = this.listeners.get(name) ?? []; values.push(callback); this.listeners.set(name, values); }
  async click() { if (this.disabled) return; for (const callback of this.listeners.get("click") ?? []) await callback({ target: this, detail: 0 }); }
  querySelector(selector) { return this.children.find(child => selector.startsWith(".") ? child.classList.contains(selector.slice(1)) : child.tagName === selector.toUpperCase()) ?? null; }
  querySelectorAll() { return []; }
}

async function harness() {
  const elements = new Map();
  const get = id => { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); };
  get("permission-rules-table").append(new Element("tbody"));
  const calls = [], toasts = [], errors = [], persistentErrors = [], initializers = [], events = new Map(), subscriptions = new Map();
  const chatToolBlocks = new Map();
  const document = {
    createElement: tag => new Element(tag),
    createTextNode: text => { const node = new Element(); node.textContent = text; return node; },
    addEventListener: (name, callback) => { const values = events.get(name) ?? []; values.push(callback); events.set(name, values); },
    dispatchEvent: event => { for (const callback of events.get(event.type) ?? []) callback(event); },
    querySelector: () => null, querySelectorAll: () => [], body: new Element("body"),
  };
  const rules = new Map([[A, [clone(ruleA)]], [B, [clone(ruleB)]]]);
  const handlers = new Map();
  let confirm = () => Promise.resolve(true);
  const snapshot = projectDir => ({ path: `${projectDir}/.kanzei/kanzei.toml`, rules: (rules.get(projectDir) ?? []).map((rule, index) => ({ index, ...clone(rule) })) });
  const invoke = async (command, args = {}) => {
    calls.push({ command, args: clone(args) });
    if (handlers.has(command)) return handlers.get(command)(args);
    if (command === "permission_rules_get") return snapshot(args.projectDir);
    if (command === "permission_rule_delete") {
      const current = rules.get(args.projectDir)?.[args.index];
      if (!current) throw new Error("rule missing");
      // New payloads emulate the coordinated expected-rule contract. Old UI
      // replay keeps old index-only behavior to reproduce wrong-project deletion.
      if (args.expectedRule && JSON.stringify(current) !== JSON.stringify(args.expectedRule)) throw new Error("permission rule changed; conflict");
      rules.get(args.projectDir).splice(args.index, 1);
      return null;
    }
    if (command === "permission_rule_add") {
      rules.get(args.projectDir).push({ action: args.action, resource: args.resource, effect: "allow" });
      return null;
    }
    if (command === "fast_model_status") return { managed: false };
    return null;
  };
  const timerIds = [];
  const context = vm.createContext({
    document, console, window: {}, navigator: {}, CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init?.detail; } },
    setTimeout: (callback, ms) => { const timer = setTimeout(callback, ms); timerIds.push(timer); return timer; }, clearTimeout,
    localStorage: { getItem: () => null }, requestAnimationFrame: callback => callback(),
  });
  const sources = new Map(["07-events.js", "16-settings.js"].map(name => {
    const path = `crates/kanzei-app/ui/${name}`;
    return [name, before ? execFileSync("git", ["show", `${beforeCommit}:${path}`], { cwd: root, encoding: "utf8" }) : readFileSync(resolve(root, path), "utf8")];
  }));
  const imported = new Map();
  for (const source of sources.values()) {
    for (const [, body, specifier] of source.matchAll(/import\s*\{([\s\S]*?)\}\s*from\s*"([^"]+)"/g)) {
      const names = imported.get(specifier) ?? new Set();
      for (const name of body.split(",").map(value => value.trim()).filter(Boolean)) names.add(name.split(/\s+as\s+/)[0]);
      imported.set(specifier, names);
    }
  }
  const core = { $: get, invoke, confirmDialog: (...args) => confirm(...args), defer: callback => initializers.push(callback), on: (name, callback) => subscriptions.set(name, callback), messages: new Element(), activePane: new Element(), promptBox: new Element() };
  const shell = { currentProject: A, activeSessionId: "session-a", processItems: [], sessionStates: new Map(), sessionMetaCache: new Map(), runTokens: {},
    toast: (text, options) => toasts.push({ text, options }), toastError: (text, options) => errors.push({ text, options }), reportPersistentError: text => persistentErrors.push(text) };
  const deps = new Map();
  for (const [specifier, names] of imported) {
    const values = specifier === "./01-core.js" ? core : specifier === "./03-shell.js" ? shell :
      specifier === "./02-i18n.js" ? { t: text => text, LANGUAGE_PREFERENCES: new Set() } :
      specifier === "./03-general-scope.js" ? { isGeneralChat: project => project === "general" } :
      specifier === "./04-structured-parse.js" ? { permissionResourceText: (action, resource) => `${action} ${resource}` } :
      specifier === "./04-structured.js" ? { renderPermissionResource: (_action, resource) => { const node = new Element("span"); node.textContent = resource; return node; } } :
      specifier === "./05-chat-render.js" ? { chatToolBlocks } : {};
    deps.set(specifier, new vm.SyntheticModule([...names], function () {
      for (const name of names) this.setExport(name, Object.hasOwn(values, name) ? values[name] : () => {});
    }, { context, identifier: specifier }));
  }
  const modules = new Map();
  for (const [name, source] of sources) {
    const module = new vm.SourceTextModule(source, { context, identifier: name });
    await module.link(specifier => { assert(deps.has(specifier), `unmapped import ${specifier}`); return deps.get(specifier); });
    await module.evaluate();
    modules.set(name, module.namespace);
  }
  return {
    settings: modules.get("16-settings.js"), events: modules.get("07-events.js"), calls, toasts, errors, persistentErrors, handlers, rules, snapshot, get,
    project: project => deps.get("./03-shell.js").setExport("currentProject", project),
    confirm: callback => { confirm = callback; },
    rows: () => get("permission-rules-table").querySelector("tbody").children.map(row => row.children[1].textContent),
    remove: () => get("permission-rules-table").querySelector("tbody").children[0]?.children[2].children[0],
    oldTool: id => { const wrap = new Element(); chatToolBlocks.set(id, { wrap }); return wrap; },
    deliver: (name, payload) => {
      if (!subscriptions.has(name)) {
        const initializer = initializers.find(callback => callback.toString().includes(`on("${name}"`));
        assert(initializer, `actual subscriber for ${name} must exist`); initializer();
      }
      return subscriptions.get(name)({ payload });
    },
    language: () => {
      const initializer = initializers.find(callback => callback.toString().includes('document.addEventListener("kz:language"'));
      assert(initializer, "actual language handler must exist"); initializer(); document.dispatchEvent({ type: "kz:language" });
    },
    close: () => timerIds.forEach(clearTimeout),
  };
}

test("late project A response cannot replace current project B rules", async t => {
  const h = await harness(); t.after(h.close);
  const a = deferred(), b = deferred();
  h.handlers.set("permission_rules_get", args => args.projectDir === A ? a.promise : b.promise);
  const first = h.settings.loadPermissionRules();
  h.project(B); const second = h.settings.loadPermissionRules();
  b.resolve(h.snapshot(B)); await second;
  a.resolve(h.snapshot(A)); await first;
  assert.deepEqual(h.rows(), [ruleB.resource], "late A rules must not render in B");
});

test("older request in the same project cannot replace a newer response", async t => {
  const h = await harness(); t.after(h.close);
  const older = deferred(), newer = deferred(); let request = 0;
  h.handlers.set("permission_rules_get", () => (++request === 1 ? older : newer).promise);
  const first = h.settings.loadPermissionRules(), second = h.settings.loadPermissionRules();
  newer.resolve({ rules: [{ index: 0, ...ruleB }] }); await second;
  older.resolve({ rules: [{ index: 0, ...ruleA }] }); await first;
  assert.deepEqual(h.rows(), [ruleB.resource], "request generation must retain the newer result");
});

test("late project A error cannot clear B or offer its retry", async t => {
  const h = await harness(); t.after(h.close);
  const a = deferred(); h.handlers.set("permission_rules_get", args => args.projectDir === A ? a.promise : h.snapshot(B));
  const first = h.settings.loadPermissionRules(); h.project(B); await h.settings.loadPermissionRules();
  a.reject(new Error("old A read failed")); await first;
  assert.deepEqual(h.rows(), [ruleB.resource], "late A error must not clear B");
  assert.equal(h.errors.length, 0, "old project error must not offer a current-project retry");
});

test("switching project while confirmation is open cannot delete B", async t => {
  const h = await harness(); t.after(h.close);
  await h.settings.loadPermissionRules(); const confirm = deferred(); h.confirm(() => confirm.promise);
  const click = h.remove().click(); h.project(B); confirm.resolve(true); await click;
  assert.deepEqual(h.rules.get(B), [ruleB], "confirming an A row must not remove B's rule");
  assert.equal(h.calls.filter(call => call.command === "permission_rule_delete").length, 0);
});

test("normal delete carries its rendered project and complete rule identity", async t => {
  const h = await harness(); t.after(h.close);
  await h.settings.loadPermissionRules(); await h.remove().click();
  const deletion = h.calls.find(call => call.command === "permission_rule_delete");
  assert.deepEqual(deletion.args, { projectDir: A, index: 0, expectedRule: ruleA });
  assert.deepEqual(h.rules.get(A), []); assert.deepEqual(h.rules.get(B), [ruleB]);
});

test("an A deletion's error retry cannot become a deletion in B", async t => {
  const h = await harness(); t.after(h.close);
  await h.settings.loadPermissionRules(); let attempts = 0;
  h.handlers.set("permission_rule_delete", () => { if (++attempts === 1) throw new Error("write failed"); h.rules.get(B).splice(0, 1); });
  await h.remove().click(); assert.equal(h.errors.length, 1);
  h.project(B); await h.errors[0].options.retry();
  assert.deepEqual(h.rules.get(B), [ruleB], "old retry must not delete the newly selected project's rule");
  assert.equal(attempts, 1);
});

test("an A read error retry cannot silently read a different project", async t => {
  const h = await harness(); t.after(h.close);
  h.handlers.set("permission_rules_get", () => { throw new Error("read failed"); });
  await h.settings.loadPermissionRules(); assert.equal(h.errors.length, 1);
  const count = h.calls.length; h.project(B); await h.errors[0].options.retry();
  assert.equal(h.calls.length, count, "retry must verify the original project before IPC");
});

test("language redraw cannot rebind old A rows to current project B", async t => {
  const h = await harness(); t.after(h.close);
  await h.settings.loadPermissionRules(); h.project(B); h.language(); await flush();
  await h.remove().click();
  assert.deepEqual(h.rules.get(B), [ruleB], "language redraw must preserve the rendered project's ownership");
});

async function remembered(h) {
  h.events.askQueueFor("session-a").push({ id: 1, sessionId: "session-a", kind: "permission", action: ruleA.action, resource: ruleA.resource });
  h.events.pumpAsk(); await h.events.answerAsk("always");
  const notice = h.toasts.find(toast => toast.options?.action?.label === "撤销");
  assert(notice, "actual remember flow must produce undo action"); return notice.options.action.onClick;
}

test("remember undo keeps original project after a switch and sends complete identity", async t => {
  const h = await harness(); t.after(h.close);
  const undo = await remembered(h); h.project(B); await undo();
  const deletion = h.calls.find(call => call.command === "permission_rule_delete");
  assert.deepEqual(deletion.args, { projectDir: A, index: 0, expectedRule: ruleA });
  assert.deepEqual(h.rules.get(A), []); assert.deepEqual(h.rules.get(B), [ruleB]);
});

test("remember undo rejects shifted index instead of removing a different rule", async t => {
  const h = await harness(); t.after(h.close);
  const undo = await remembered(h), query = deferred();
  const old = h.snapshot(A); h.handlers.set("permission_rules_get", () => query.promise);
  const pending = undo(); h.rules.set(A, [clone(ruleB)]); query.resolve(old); await pending;
  assert.deepEqual(h.rules.get(A), [ruleB], "shifted index must not delete the replacement rule");
  assert.equal(h.persistentErrors.length, 1, "conflict must remain visible");
});

test("late blocked events in an old pane remember in their source project", async t => {
  const h = await harness(); t.after(h.close);
  const wrap = h.oldTool("old-call"), lateRule = { action: "bash", resource: "late command from A", effect: "allow" };
  h.project(B);
  h.deliver("kz:permission-resolved", { sessionId: "session-a", projectDir: A, tool_call_id: "old-call", ...lateRule, decision: "declined", source: "noninteractive" });
  h.deliver("kz:tool-end", { sessionId: "session-a", id: "old-call", name: "bash", ok: false, preview: "permission requires approval" });
  const button = wrap.querySelector(".blocked-allow");
  assert(button && !button.disabled, "source-bearing old-pane result must offer remember");
  await button.click();
  assert.deepEqual(h.rules.get(B), [ruleB], "old pane must not add its rule to current project B");
  assert.deepEqual(h.rules.get(A), [ruleA, lateRule]);
});

test("old blocked payload without a source project disables remembering", async t => {
  const h = await harness(); t.after(h.close);
  const wrap = h.oldTool("legacy-call"); h.project(B);
  h.deliver("kz:permission-resolved", { sessionId: "session-a", tool_call_id: "legacy-call", action: "bash", resource: "legacy command", decision: "declined", source: "noninteractive" });
  h.deliver("kz:tool-end", { sessionId: "session-a", id: "legacy-call", name: "bash", ok: false, preview: "permission requires approval" });
  const button = wrap.querySelector(".blocked-allow");
  assert(button?.disabled, "missing source must not fall back to current project");
  assert(button.textContent.includes("来源项目"), "disabled remember must explain the missing source");
  await button.click();
  assert.equal(h.calls.filter(call => call.command === "permission_rule_add").length, 0);
  assert.deepEqual(h.rules.get(B), [ruleB]);
});
