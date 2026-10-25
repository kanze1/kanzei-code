// Session execution state has one source: phase. Views may read the historical
// boolean names, but cannot write contradictory combinations into the record.
export const SESSION_PHASES = new Set(["unknown", "unconfirmed", "starting", "running", "stopping", "idle", "stopped", "failed", "auto_pending"]);
const active = new Set(["starting", "running", "stopping"]);
const terminal = new Set(["idle", "stopped", "failed", "auto_pending"]);
const detailFields = new Set(["stage", "detail", "roundResult", "auto_rounds", "runtime_managed"]);
const phases = new WeakMap();
const authorities = new WeakMap(), retiredEpochs = new WeakMap(), autoPolicies = new WeakMap(), queryFloors = new WeakMap();
let querySequence = 0;
export function beginRuntimeQuery() { return ++querySequence; }
const revisions = new WeakMap(), listeners = new Set();
export function onSessionStateChange(listener) { listeners.add(listener); return () => listeners.delete(listener); }
export const sessionStates = new Map();
export function sessionState(sessionId) {
  if (!sessionStates.has(sessionId)) {
    const state = {
      get phase() { return phases.get(this); },
      get runtime() { return authorities.get(this) ?? null; },
      get revision() { return revisions.get(this); },
      get running() { return active.has(this.phase) || Boolean(this.runtime && this.runtime.phase !== "idle"); },
      get converged() { return terminal.has(this.phase); },
      get auto_pending() { return !this.running && (this.phase === "auto_pending" || autoPolicies.get(this) === true); },
      get execution_phase() { return this.runtime?.phase ?? null; },
      get transport() { return this.phase === "unconfirmed" ? "unconfirmed" : "confirmed"; },
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
  if (phase === "auto_pending") autoPolicies.set(state, true);
  if (["starting", "stopping", "stopped"].includes(phase)) autoPolicies.set(state, false);
  // Local intentions and transport health do not overwrite execution receipts.
  if (state.runtime && !["starting", "stopping", "unconfirmed"].includes(phase)) return state;
  phases.set(state, phase);
  if (phase === "starting") state.roundResult = null;
  for (const [key, value] of Object.entries(detail)) state[key] = value;
  revisions.set(state, state.revision + 1);
  for (const listener of listeners) listener(sessionId, state);
  return state;
}

// 不改相位、只落 detail 的唯一入口。直接给 state 赋值会跳过 revision 递增与监听通知,
// 界面读到旧值且没有任何信号;把当前相位回灌给 reduceSession 也不安全——starting
// 分支会清空 roundResult,把刚写入的本轮结果抹掉。
export function setSessionDetails(sessionId, detail = {}) {
  for (const key of Object.keys(detail)) if (!detailFields.has(key)) throw new TypeError(`Session details cannot write ${key}`);
  const state = sessionState(sessionId);
  for (const [key, value] of Object.entries(detail)) state[key] = value;
  revisions.set(state, state.revision + 1);
  for (const listener of listeners) listener(sessionId, state);
  return state;
}

// Epoch changes are accepted through an explicit authoritative query. Once
// retired, delayed events and delayed queries can never restore that actor.
export function applyRuntimeSnapshot(sessionId, snapshot, { queryId = null } = {}) {
  if (!snapshot?.actor_epoch || !Number.isSafeInteger(snapshot.revision)) throw new TypeError("Invalid lifecycle snapshot");
  if (queryId !== null && (!Number.isSafeInteger(queryId) || queryId <= 0)) throw new TypeError("Invalid lifecycle query identity");
  const phase = ({ idle: snapshot.last_outcome === "stopped" ? "stopped" : snapshot.last_outcome === "failed" ? "failed" : "idle", acquiring:"starting", active:"running", cancelling:"stopping", reaping:"stopping", blocked:"failed" })[snapshot.phase];
  if (!phase) throw new TypeError(`Unknown lifecycle phase: ${snapshot.phase}`);
  const state = sessionState(sessionId), current = state.runtime;
  const queried = queryId !== null;
  // Query ordering also fences an older actor that this page never observed.
  // Epoch uniqueness alone cannot order two replies from different processes.
  if (queried && queryId < (queryFloors.get(state) ?? 0)) return false;
  if (queried) queryFloors.set(state, queryId);
  const retired = new Set(retiredEpochs.get(state));
  if (retired.has(snapshot.actor_epoch)) return false;
  if (current) {
    if (snapshot.actor_epoch === current.actor_epoch && (snapshot.revision < current.revision || snapshot.revision === current.revision && !queried)) return false;
    if (snapshot.actor_epoch !== current.actor_epoch) {
      if (!queried) return false;
      retired.add(current.actor_epoch);
    }
  } else if (!queried) queryFloors.set(state, ++querySequence);
  retiredEpochs.set(state, retired); authorities.set(state, Object.freeze(structuredClone(snapshot)));
  if (snapshot.phase === "active" || snapshot.last_outcome === "stopped") autoPolicies.set(state, false);
  phases.set(state, phase);
  state.stage = ({starting:"启动中", running:"运行中", stopping:"回收中", failed:"出错", stopped:"已停止", idle:"空闲"})[phase];
  state.detail = snapshot.error || "";
  revisions.set(state, state.revision + 1);
  for (const listener of listeners) listener(sessionId, state);
  return true;
}
