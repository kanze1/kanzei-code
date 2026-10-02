import { defer } from "./01-core.js";
import { __kzAutoTestState } from "./08-auto.js";

// R-264 B3：测试钩子是正式 ESM namespace 的唯一出口。
// 业务 runtime 保持在 08-compose-runtime.js；这里不复制业务逻辑，只把 classic
// runtime 提供的可观测状态桥接成可显式 import 的测试 API。
export const state = () => globalThis.__kzAutoTestState;

export const __kzTest = Object.freeze({
  rounds: () => state()?.rounds() ?? 0,
  noAction: () => state()?.noAction() ?? 0,
  stopReason: () => state()?.stopReason() ?? "",
  timerSessions: () => state()?.timerSessions() ?? [],
  retryLabel: (id) => state()?.retryLabel(id) ?? null,
  setAutoState: (id, value) => state()?.setAutoState(id, value),
  getAutoState: (id) => state()?.getAutoState(id),
  setRounds: (value) => state()?.setRounds(value),
  setStopAfterRound: (value) => state()?.setStopAfterRound(value),
  setPaused: (value) => state()?.setPaused(value),
  paused: () => state()?.paused() ?? false,
  reset: () => state()?.reset(),
  cancelTimers: () => state()?.cancelTimers(),
});
