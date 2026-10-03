import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import vm from "node:vm";

const root = resolve(import.meta.dirname, "..");
const baseline = "aff48f96b3c310fd83196e143541515c9a18643f";
const beforeOwnership = process.argv.includes("--before-ownership");
const before = process.argv.includes("--before") || beforeOwnership;
const flush = async () => { await new Promise(resolveValue => setImmediate(resolveValue)); };
const deferred = () => { let release; const promise = new Promise(resolveValue => { release = resolveValue; }); return { promise, release }; };
class Element {
  constructor(tag) { this.tagName = tag; this.children = []; this.listeners = new Map(); this.attributes = new Map(); this.value = ""; this.ownText = ""; }
  set textContent(value) { this.ownText = String(value); this.replaceChildren(); }
  get textContent() { return this.ownText + this.children.map(child => child.textContent).join(""); }
  append(...children) { for (const child of children) { child.parent = this; this.children.push(child); } }
  appendChild(child) { this.append(child); return child; }
  replaceChildren(...children) { this.children = []; this.append(...children); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter(child => child !== this); }
  setAttribute(key, value) { this.attributes.set(key, value); }
  addEventListener(key, value) { const listeners = this.listeners.get(key) ?? []; listeners.push(value); this.listeners.set(key, listeners); }
  dispatch(key) { for (const listener of this.listeners.get(key) ?? []) listener({ preventDefault() {} }); }
  click() { this.dispatch("click"); }
  querySelectorAll(selector) { return this.children.flatMap(child => [...(child.tagName === selector || child.className === selector.slice(1) ? [child] : []), ...child.querySelectorAll(selector)]); }
  querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; }
  closest(selector) { return this.tagName === selector ? this : this.parent?.closest(selector); }
}
const definition = (body, name = "same") => ({ name, enabled: true, when: "每天 09:00", host: "app", catch_up: "once", agent: "readonly", model: "primary", timeout_secs: 1800, max_steps: 32, steps: [{ prompt: body }], writeback: ["notify"], body });
const listed = body => ({ tasks: [{ definition: definition(body), revision: `${body}-revision`, next_ms: 0, history: [] }], diagnostics: [] });
async function harness(currentProject = "A") {
  const dialog = new Element("dialog"), calls = [], handlers = new Map(), errors = [];
  let close, opened = false;
  const closeDialog = () => { if (opened) { opened = false; close?.(); } };
  const context = vm.createContext({ document: { createElement: tag => new Element(tag), getElementById: () => dialog }, console });
  const invoke = async (command, args = {}) => {
    calls.push({ command, args });
    if (handlers.has(command)) return handlers.get(command)(args);
    if (command === "projects_get") return { projects: ["A", "B"], current: "A" };
    if (command === "schedule_action") return args.action === "list" ? listed(args.projectDir) : { queued: true };
    return [];
  };
  const imports = {
    // Real openDialog returns the existing handle when the same modal is open;
    // a repeated opening does not replace that handle's original close callback.
    "./00-surface.js": { openDialog: (_, options) => { if (!opened) { opened = true; close = options.onClose; } }, closeSurface: closeDialog },
    "./01-core.js": { invoke, on() {}, uiPrefsLoad: async () => ({ theme: "dark", ui_layout: {} }) },
    "./03-shell.js": { currentProject, toast() {}, toastError: error => errors.push(error) },
    "./02-i18n.js": { t: text => text }, "./03-general-scope.js": { isGeneralChat: () => false },
  };
  const path = "crates/kanzei-app/ui/24-schedules.js";
  let source = before ? execFileSync("git", ["show", `${baseline}:${path}`], {cwd:root, encoding:"utf8"}) : readFileSync(resolve(root, path), "utf8");
  // The original selector has no B option. This stage applies only the required
  // projects source repair, then proves the original owner logic still fails.
  if (beforeOwnership) source = source.replace("await uiPrefsLoad()", 'await invoke("projects_get")');
  const module = new vm.SourceTextModule(source, { context });
  await module.link(specifier => {
    const values = imports[specifier]; assert(values, `known import ${specifier}`);
    return new vm.SyntheticModule(Object.keys(values), function () { for (const [key, value] of Object.entries(values)) this.setExport(key, value); }, { context });
  });
  await module.evaluate();
  return {
    open: () => module.namespace.showSchedules(), dialog, calls, handlers, errors,
    project: () => dialog.querySelector("select"),
    choose: value => { const select = dialog.querySelector("select"); select.value = value; select.dispatch("change"); },
    button: label => { const button = dialog.querySelectorAll("button").find(button => button.textContent === label); assert(button, `button ${label} is mounted`); return button; },
    close: closeDialog,
  };
}

test("global schedule entry obtains registered projects from the real projects_get shape", async () => {
  const h = await harness(null); await h.open();
  assert.deepEqual(h.project().children.map(option => option.value), ["A", "B"], "ui_prefs_get has no projects; registered projects must still be selectable");
  assert.equal(h.calls.find(call => call.command === "schedule_action").args.projectDir, "A");
});

test("late A list cannot replace the selected B list", { skip: before && !beforeOwnership }, async () => {
  const h = await harness(), a = deferred();
  h.handlers.set("schedule_action", args => args.action === "list" && args.projectDir === "A" ? a.promise : listed("B"));
  const opening = h.open(); await flush(); h.choose("B"); await flush();
  a.release(listed("A")); await opening; await flush();
  assert.equal(h.dialog.querySelectorAll("section").length, 1, "exactly one selected project list is visible");
  h.button("停用").click(); await flush();
  const toggle = h.calls.find(call => call.args.action === "toggle");
  assert.equal(toggle.args.expectedHash, "B-revision", "the displayed row must belong to B's loaded definition");
});

test("a clicked A run retains its original project when selection changes before the queued handler", { skip: before && !beforeOwnership }, async () => {
  const h = await harness(); await h.open();
  h.button("立即运行").click(); h.choose("B"); await flush();
  const run = h.calls.find(call => call.args.action === "run");
  assert.equal(run.args.projectDir, "A", "A's explicit run intent cannot execute B's same-name task");
});

test("late history cannot overwrite a newer list of the same project", async () => {
  const h = await harness(), history = deferred(); await h.open();
  h.handlers.set("schedule_action", args => args.action === "history" ? history.promise : listed(args.projectDir));
  h.button("运行历史").click(); await flush();
  const back = h.dialog.querySelectorAll("button").find(button => ["返回", "刷新"].includes(button.textContent)); assert(back); back.click(); await flush();
  history.release([{ type: "schedule.run_finished", data: { summary: "old A history", run_id: "run-a" } }]); await flush();
  assert.equal(h.dialog.textContent.includes("old A history"), false);
  assert.equal(h.dialog.querySelectorAll("section").length, 1);
});

test("late save receipt cannot discard a new editor draft in the same project", async () => {
  const h = await harness(), saved = deferred(); await h.open(); h.button("编辑").click(); await flush();
  h.handlers.set("schedule_action", args => args.action === "save" ? saved.promise : listed(args.projectDir));
  h.dialog.querySelector("form").dispatch("submit"); await flush(); h.button("返回").click(); await flush();
  h.button("编辑").click(); await flush(); const editor = h.dialog.querySelector("form");
  editor.querySelector("textarea").value = "new unsaved draft";
  saved.release({ saved: true }); await flush();
  assert.equal(h.dialog.querySelector("form") === editor, true, "completed save cannot reload over a newer editor");
  assert.equal(editor.querySelector("textarea").value, "new unsaved draft");
  assert.equal(h.calls.find(call => call.args.action === "save").args.projectDir, "A");
});

test("closed schedule dialog stays empty when its list receipt arrives", async () => {
  const h = await harness(), list = deferred(); h.handlers.set("schedule_action", () => list.promise);
  const opening = h.open(); await flush(); h.close(); list.release(listed("A")); await opening;
  assert.equal(h.dialog.children.length, 0, "closed scope cannot repopulate the shared modal");
});

test("out-of-order open requests leave the latest dialog intact", { skip: before && !beforeOwnership }, async () => {
  const h = await harness(), first = deferred(); let count = 0;
  h.handlers.set("projects_get", () => ++count === 1 ? first.promise : { projects: ["A", "B"] });
  const oldOpening = h.open(); await flush(); await h.open();
  h.button("编辑").click(); await flush(); const editor = h.dialog.querySelector("form");
  first.release({ projects: ["A", "B"] }); await oldOpening;
  assert.equal(h.dialog.querySelector("form") === editor, true, "obsolete opening must not replace the newest dialog state");
});

test("closing a reused real surface handle cancels a pending later opening", { skip: before && !beforeOwnership }, async () => {
  const h = await harness(); await h.open(); await h.open();
  const late = deferred(); h.handlers.set("projects_get", () => late.promise);
  const opening = h.open(); await flush(); h.close();
  late.release({ projects: ["A", "B"] }); await opening;
  assert.equal(h.dialog.children.length, 0, "the existing handle's close callback must invalidate every pending opening");
});
