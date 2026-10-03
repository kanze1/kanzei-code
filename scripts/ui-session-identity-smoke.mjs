// M4: run the actual sessions module with controlled IPC completion ordering.
// --before replays exact committed source; dependency fixtures model existing
// shell/queue contracts and do not copy any function being repaired.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import vm from "node:vm";

const root = resolve(import.meta.dirname, "..");
const file = "crates/kanzei-app/ui/09-sessions.js";
const before = process.argv.includes("--before");
const readModule = path => before
  ? execFileSync("git", ["show", `aff48f96:${path}`], { cwd: root, encoding: "utf8" })
  : readFileSync(resolve(root, path), "utf8");
const source = readModule(file);
const workbenchSource = readModule("crates/kanzei-app/ui/12-workbench.js");
const A = "C:/fixture/project-a", B = "C:/fixture/project-b";
const ids = project => ({ process: `d|${project}`, session: `session-${project.endsWith("a") ? "a" : "b"}` });
const deferred = () => {
  let resolveValue, reject;
  const promise = new Promise((yes, no) => { resolveValue = yes; reject = no; });
  return { promise, resolve: resolveValue, reject };
};
const flush = () => new Promise(yes => setImmediate(yes));
const clone = value => JSON.parse(JSON.stringify(value));

class Element {
  constructor(tag = "div") {
    this.tagName = tag.toUpperCase(); this.children = []; this.dataset = {}; this.listeners = new Map();
    this.attributes = new Map(); this.classes = new Set(); this.ownText = ""; this.value = "";
    this.disabled = false; this.options = []; this.checked = false;
    this.classList = {
      add: (...values) => values.forEach(value => this.classes.add(value)),
      remove: (...values) => values.forEach(value => this.classes.delete(value)),
      contains: value => this.classes.has(value),
      toggle: (value, force) => { const on = force ?? !this.classes.has(value); on ? this.classes.add(value) : this.classes.delete(value); return on; },
    };
  }
  set className(value) { this.classes = new Set(value.split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classes].join(" "); }
  set textContent(value) { this.ownText = String(value); this.children = []; }
  get textContent() { return this.ownText + this.children.map(child => child.textContent).join(""); }
  set innerHTML(value) { this.ownText = value; this.children = []; }
  append(...children) { this.children.push(...children); }
  appendChild(child) { this.append(child); return child; }
  prepend(...children) { this.children.unshift(...children); }
  replaceChildren(...children) { this.ownText = ""; this.children = children; }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  removeAttribute(name) { this.attributes.delete(name); }
  addEventListener(name, callback) { const callbacks = this.listeners.get(name) ?? []; callbacks.push(callback); this.listeners.set(name, callbacks); }
  querySelector() { return null; }
  querySelectorAll() { return []; }
  async click() { if (!this.disabled) for (const callback of this.listeners.get("click") ?? []) await callback({ target: this }); }
}

async function harness() {
  const elements = new Map(), deps = new Map(), calls = [], handlers = new Map(), notices = [];
  const get = id => { if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id); };
  get("project-shared-warn").classList.add("hidden");
  const queues = new Map(), states = new Map(), processAutoState = new Map(), processProfileUi = new Map();
  const state = { currentProject: A, activeProcessId: ids(A).process, activeSessionId: ids(A).session, processItems: [], running: false };
  const line = (project, id = ids(project).process, session = ids(project).session) => ({ id, session_id: session, origin_project: project, project_dir: project, profile: "dev", running: false, label: id });
  const projects = new Map([[A, [line(A)]], [B, [line(B)]]]); state.processItems = projects.get(A);
  const set = (name, value) => { state[name] = value; deps.get("./03-shell.js").setExport(name, value); };
  const sessionState = id => { if (!states.has(id)) states.set(id, { phase: "idle", running: false, converged: false }); return states.get(id); };
  const transitionSession = (id, phase, detail = {}) => Object.assign(sessionState(id), { phase, running: phase === "running", converged: ["idle", "stopped", "failed"].includes(phase) }, detail);
  let confirm = () => Promise.resolve(true), created = 0;
  const invoke = async (command, args = {}) => {
    calls.push({ command, args: clone(args) });
    if (handlers.has(command)) return handlers.get(command)(args);
    if (command === "project_facts") return { git: { state: "repo", has_commits: true }, stacks: [] };
    if (command === "project_root_info") return { selected: args.projectDir, resolved: args.projectDir, shared: false };
    if (command === "process_list") return projects.get(args.projectDir) ?? [];
    if (command === "process_create") { const item = line(args.projectDir, `p|created-${++created}`, `created-${created}`); projects.get(args.projectDir).push(item); return item; }
    if (["worktree_list", "list_pending_inputs", "pending_asks_get"].includes(command)) return [];
    return null;
  };
  const document = { body: new Element("body"), createElement: tag => new Element(tag), querySelector: () => null, querySelectorAll: () => [], addEventListener: () => {}, dispatchEvent: () => {} };
  const context = vm.createContext({ document, console, Date, Option: class { constructor(text, value) { this.textContent = text; this.value = value; } }, CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init?.detail; } }, Event: class {}, setTimeout, clearTimeout });
  const values = new Map([
    ["./01-core.js", { $: get, invoke, confirmDialog: (...args) => confirm(...args), defer: () => {}, promptBox: new Element() }],
    ["./03-shell.js", { ...state, setCurrentProject: value => set("currentProject", value), setActiveProcessId: value => set("activeProcessId", value), setActiveSessionId: value => set("activeSessionId", value), setProcessItems: value => set("processItems", value), sessionState, sessionStates: states, transitionSession, setRunning: value => set("running", value), toast: text => notices.push(text), toastError: text => notices.push(text), ensureChatView: () => notices.push("ensureChatView") }],
    ["./02-i18n.js", { t: text => text, localizeDynamic: text => text, localizedStage: text => text }],
    ["./03-general-scope.js", { isGeneralChat: () => false }],
    ["./03-workspaces.js", { active_space: "dev", workspace_switch_pending: false, workspace_processes: items => items, preferred_workspace_process: items => items[0], project_workspace: () => ({ research: {} }) }],
    ["./08-compose-runtime.js", { processAutoState, processProfileUi }],
    ["./07-events.js", { askQueues: queues, askActive: null, askQueueFor: id => { if (!queues.has(id)) queues.set(id, []); return queues.get(id); }, pumpAsk: () => {} }],
    ["./08-auto.js", { awaitingUserSessions: new Set() }],
    ["./05-subagents.js", { SA_ACTIVE: new Set(), subagentRunsFor: () => [] }],
    ["./12-session-tree.js", { processName: item => item.label ?? item.id, orderedProjects: () => [], setSidebarOpen: () => {} }],
    ["./08-models.js", { modelCatalogProject: A }],
  ]);
  const imported = new Map();
  for (const actual of [source, workbenchSource]) {
    for (const [, body, specifier] of actual.matchAll(/import\s*\{([\s\S]*?)\}\s*from\s*"([^"]+)"/g)) {
      const names = imported.get(specifier) ?? new Set();
      for (const name of body.split(",").map(value => value.trim()).filter(Boolean)) names.add(name.split(/\s+as\s+/)[0]);
      imported.set(specifier, names);
    }
  }
  for (const [specifier, names] of imported) {
    const exports = values.get(specifier) ?? {};
    deps.set(specifier, new vm.SyntheticModule([...names], function () {
      for (const name of names) this.setExport(name, Object.hasOwn(exports, name) ? exports[name] : () => {});
    }, { context, identifier: specifier }));
  }
  const module = new vm.SourceTextModule(source, { context, identifier: file });
  const workbench = new vm.SourceTextModule(workbenchSource, { context, identifier: "12-workbench.js" });
  await module.link(specifier => {
    if (specifier === "./09-sessions.js") return module;
    if (specifier === "./12-workbench.js") return workbench;
    assert(deps.has(specifier)); return deps.get(specifier);
  }); await module.evaluate();
  return { sessions: module.namespace, calls, handlers, queues, states, projects, line, get, state, notices,
    transitionSession,
    select: (project, process = ids(project).process, session = ids(project).session) => { set("currentProject", project); set("activeProcessId", process); set("activeSessionId", session); set("processItems", projects.get(project)); },
    confirm: callback => { confirm = callback; },
    cancelButton: () => get("queue-list").children[0]?.children[2],
    prompts: () => get("queue-list").children.filter(child => child.classList.contains("queue-entry")).map(child => child.children[0].textContent),
  };
}

test("permission restoration belongs to requested A while B becomes active", async () => {
  const h = await harness(), pending = deferred(); h.handlers.set("pending_asks_get", () => pending.promise);
  const request = h.sessions.refreshPendingAsks(); h.select(B);
  pending.resolve([{ id: 11, kind: "permission", sessionId: ids(A).session, action: "bash", resource: "A command" }]); await request;
  assert.equal(h.queues.get(ids(A).session)?.[0]?.id, 11); assert.equal(h.queues.get(ids(B).session)?.length ?? 0, 0);
});

test("initial permission read failure permits the next real process poll to retry", async () => {
  const h = await harness(); let attempt = 0;
  h.handlers.set("pending_asks_get", () => { attempt += 1; if (attempt === 1) throw new Error("temporary IPC failure"); return []; });
  h.sessions.renderProcesses(h.state.processItems); await flush();
  h.sessions.renderProcesses(h.state.processItems); await flush(); assert.equal(attempt, 2);
});

test("late project A input list cannot replace B", async () => {
  const h = await harness(), a = deferred(), b = deferred();
  h.handlers.set("list_pending_inputs", args => args.projectDir === A ? a.promise : b.promise);
  const first = h.sessions.refreshPendingInputs(); h.select(B); const second = h.sessions.refreshPendingInputs();
  b.resolve([{ input_id: "b1", prompt: "B prompt" }]); await second;
  a.resolve([{ input_id: "a1", prompt: "A prompt" }]); await first; assert.deepEqual(h.prompts(), ["B prompt"]);
});

test("older same-session input snapshot cannot replace a newer list", async () => {
  const h = await harness(), older = deferred(), newer = deferred(); let count = 0;
  h.handlers.set("list_pending_inputs", () => ++count === 1 ? older.promise : newer.promise);
  const first = h.sessions.refreshPendingInputs(), second = h.sessions.refreshPendingInputs();
  newer.resolve([]); await second; older.resolve([{ input_id: "already-cancelled", prompt: "old" }]); await first; assert.deepEqual(h.prompts(), []);
});

test("old A queue button never sends A input ID to B", async () => {
  const h = await harness(); h.sessions.renderPendingInputs([{ input_id: "a1", prompt: "A prompt" }]); const button = h.cancelButton();
  h.select(B); await button.click(); assert.equal(h.calls.filter(call => call.command === "cancel_input").length, 0);
});

test("queue cancellation retains legitimate original routing and refreshes", async () => {
  const h = await harness(); h.sessions.renderPendingInputs([{ input_id: "a1", prompt: "A prompt" }]);
  h.handlers.set("cancel_input", () => true); await h.cancelButton().click();
  assert.deepEqual(h.calls.find(call => call.command === "cancel_input").args, { projectDir: A, inputId: "a1", processId: ids(A).process }); assert.deepEqual(h.prompts(), []);
});

test("already-consumed input false receipt still reloads authoritative queue", async () => {
  const h = await harness(); h.sessions.renderPendingInputs([{ input_id: "a1", prompt: "A prompt" }]);
  h.handlers.set("cancel_input", () => false); await h.cancelButton().click(); assert.deepEqual(h.prompts(), []);
});

test("late A isolation response cannot replace current B warning", async () => {
  const h = await harness(), a = deferred(); h.handlers.set("project_root_info", args => args.projectDir === A ? a.promise : { selected: B, resolved: B, shared: false });
  const first = h.sessions.checkProjectIsolation(); h.select(B); await h.sessions.checkProjectIsolation();
  a.resolve({ selected: A, resolved: "A parent", shared: true }); await first; assert.equal(h.get("project-shared-warn").classList.contains("hidden"), true);
});

test("old A isolation button cannot detach the newly selected B", async () => {
  const h = await harness(); h.handlers.set("project_root_info", () => ({ selected: A, resolved: "A parent", shared: true }));
  await h.sessions.checkProjectIsolation(); const button = h.get("project-shared-warn").children[1]; h.select(B); await button.click();
  assert.equal(h.calls.filter(call => call.command === "project_detach").length, 0);
});

test("creation confirmed after project switch never creates a tree in B", async () => {
  const h = await harness(), confirmation = deferred(); h.confirm(() => confirmation.promise);
  const creation = h.sessions.createWorktreeLine(); await flush(); h.select(B); confirmation.resolve(true); await creation;
  assert.equal(h.calls.filter(call => call.command === "process_create" && call.args.projectDir === B).length, 0);
  assert.equal(h.sessions.worktreeLineCreateInFlight, false);
});

test("project switch during facts cannot retarget worktree creation", async () => {
  const h = await harness(), facts = deferred(); h.handlers.set("project_facts", () => facts.promise);
  const creation = h.sessions.createWorktreeLine(); h.select(B); facts.resolve({ git: { state: "repo", has_commits: true } }); await creation;
  assert.equal(h.calls.filter(call => call.command === "process_create").length, 0);
});

test("facts await is already inside worktree creation admission", async () => {
  const h = await harness(), facts = deferred(); h.handlers.set("project_facts", () => facts.promise);
  const first = h.sessions.createWorktreeLine(), second = h.sessions.createWorktreeLine(); facts.resolve({ git: { state: "repo", has_commits: true } }); await Promise.all([first, second]);
  assert.equal(h.calls.filter(call => call.command === "process_create").length, 1); assert.equal(h.sessions.worktreeLineCreateInFlight, false);
});

test("successful close cannot clear another process selected while close awaited", async () => {
  const h = await harness(), close = deferred();
  const first = h.line(A, "p|first", "session-first"), second = h.line(A, "p|second", "session-second");
  h.projects.get(A).push(first, second); h.select(A, first.id, first.session_id); h.handlers.set("process_close", () => close.promise);
  const closing = h.sessions.closeParallelProcess(first.id); await flush(); h.select(A, second.id, second.session_id);
  h.projects.set(A, h.projects.get(A).filter(item => item.id !== first.id)); close.resolve("closed"); await closing; assert.equal(h.state.activeProcessId, second.id);
});

test("late project selection uses the existing navigation owner and cannot undo the latest selection", async () => {
  const h = await harness(), old = deferred();
  h.handlers.set("projects_select", args => args.path === B ? old.promise : { current: A, projects: [A, B], names: {} });
  const older = h.sessions.switchProject(B); await flush();
  const latest = h.sessions.switchProject(A); await latest;
  old.resolve({ current: B, projects: [A, B], names: {} }); await older; assert.equal(h.state.currentProject, A);
});

test("closing failure cannot overwrite the terminal event received while IPC awaited", async () => {
  const h = await harness(), close = deferred(), process = h.line(A, "p|first", "session-first");
  h.projects.get(A).push(process); h.select(A, process.id, process.session_id); h.handlers.set("process_close", () => close.promise);
  const closing = h.sessions.closeParallelProcess(process.id); await flush(); h.transitionSession(process.session_id, "stopped");
  close.reject(new Error("close failed after terminal")); await closing;
  // The state owner is the same object renderProcesses reads, not a copied UI label.
  assert.equal(h.states.get(process.session_id).phase, "stopped");
});

test("legacy project switch preserves same-project refresh through the shared navigation owner", async () => {
  const h = await harness(); h.handlers.set("projects_select", () => ({ current: A, projects: [A, B], names: {} }));
  await h.sessions.switchProject(A); assert.equal(h.calls.filter(call => call.command === "projects_select").length, 1);
});

test("cancelled worktree confirmation releases admission without creating", async () => {
  const h = await harness(); h.confirm(() => false); await h.sessions.createWorktreeLine();
  assert.equal(h.sessions.worktreeLineCreateInFlight, false); assert.equal(h.calls.filter(call => call.command === "process_create").length, 0);
});

test("failed worktree creation releases admission and allows a legitimate retry", async () => {
  const h = await harness(); h.handlers.set("process_create", () => { throw new Error("git creation rejected"); });
  await h.sessions.createWorktreeLine(); assert.equal(h.sessions.worktreeLineCreateInFlight, false);
  assert.equal(h.get("worktree-add").disabled, false); h.handlers.delete("process_create"); await h.sessions.createWorktreeLine();
  assert.equal(h.calls.filter(call => call.command === "process_create").length, 2); assert.equal(h.sessions.worktreeLineCreateInFlight, false);
});

test("blocked Git facts reject creation and release admission", async () => {
  const h = await harness(); h.handlers.set("project_facts", () => ({ git: { state: "none" } }));
  await h.sessions.createWorktreeLine(); assert.equal(h.calls.filter(call => call.command === "process_create").length, 0);
  assert.equal(h.sessions.worktreeLineCreateInFlight, false);
});

test("legitimate isolation action uses its original project", async () => {
  const h = await harness(); h.handlers.set("project_root_info", () => ({ selected: A, resolved: "A parent", shared: true }));
  await h.sessions.checkProjectIsolation(); await h.get("project-shared-warn").children[1].click();
  assert.deepEqual(h.calls.find(call => call.command === "project_detach").args, { projectDir: A });
});

test("failed close restores only its still-pending optimistic phase", async () => {
  const h = await harness(), process = h.line(A, "p|first", "session-first"); process.running = true;
  h.projects.get(A).push(process); h.select(A, process.id, process.session_id); h.transitionSession(process.session_id, "running");
  h.handlers.set("process_close", () => { throw new Error("close rejected"); }); await h.sessions.closeParallelProcess(process.id);
  assert.equal(h.states.get(process.session_id).phase, "running"); assert.equal(h.state.activeProcessId, process.id);
});

test("changing root clears old actionable Git facts while B facts are pending", async () => {
  const h = await harness(), facts = deferred();
  h.handlers.set("project_facts", args => args.projectDir === A ? { git: { state: "none" } } : facts.promise);
  await h.sessions.refreshProjectFacts(A); assert.equal(h.sessions.projectFactsFor, A);
  h.sessions.activate_execution_root(B); await flush();
  assert.equal(h.sessions.projectFacts, null); assert.equal(h.get("project-facts").classList.contains("hidden"), true);
  facts.resolve({ git: { state: "repo", has_commits: true } }); await flush();
});
