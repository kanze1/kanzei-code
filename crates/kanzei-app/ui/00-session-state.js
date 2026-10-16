// Session execution state has one source: phase. Views may read the historical
// boolean names, but cannot write contradictory combinations into the record.
export const SESSION_PHASES = new Set(["unknown", "unconfirmed", "starting", "running", "stopping", "idle", "stopped", "failed", "auto_pending"]);
const active = new Set(["starting", "running", "stopping"]);
const terminal = new Set(["idle", "stopped", "failed", "auto_pending"]);
const detailFields = new Set(["stage", "detail", "roundResult", "auto_rounds", "runtime_managed"]);
const phases = new WeakMap();
const revisions = new WeakMap(), listeners = new Set();
export function onSessionStateChange(listener) { listeners.add(listener); return () => listeners.delete(listener); }
export const sessionStates = new Map();
export function sessionState(sessionId) {
  if (!sessionStates.has(sessionId)) {
    const state = {
      get phase() { return phases.get(this); },
      get revision() { return revisions.get(this); },
      get running() { return active.has(this.phase); },
      get converged() { return terminal.has(this.phase); },
      get auto_pending() { return this.phase === "auto_pending"; },
      get local_start_pending() { return this.phase === "starting"; },
      get live_running() { return this.phase === "running" ? true : ["unknown", "unconfirmed", "starting"].includes(this.phase) ? null : false; },
      get terminal_status() { return this.phase === "stopped" ? "已停止" : this.phase === "failed" ? "出错" : ""; },
      stage: "空闲", detail: "", roundResult: null,
    };
    phases.set(state, "unknown"); revisions.set(state, 0); sessionStates.set(sessionId, state);
  }
  return sessionStates.get(sessionId);
}
export function sessionDetails(state) {
  return Object.fromEntries([...detailFields].filter(key => Object.hasOwn(state, key)).map(key => [key, state[key]]));
}
export function reduceSession(sessionId, phase, detail = {}) {
  if (!SESSION_PHASES.has(phase)) throw new TypeError(`Unknown session phase: ${phase}`);
  for (const key of Object.keys(detail)) if (!detailFields.has(key)) throw new TypeError(`Session transition cannot write ${key}`);
  const state = sessionState(sessionId);
  phases.set(state, phase);
  if (phase === "starting") state.roundResult = null;
  for (const [key, value] of Object.entries(detail)) state[key] = value;
  revisions.set(state, state.revision + 1);
  for (const listener of listeners) listener(sessionId, state);
  return state;
}
