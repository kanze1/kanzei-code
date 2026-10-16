import { invoke } from "./00-ipc.js";
import { sessionState, reduceSession, onSessionStateChange } from "./00-session-state.js";

// An accepted request should produce an execution signal promptly. At the
// deadline we query a fresh backend fact; this deadline never resends a prompt
// and never fabricates task success/failure from elapsed time.
export const START_CONFIRMATION_MS = 10000;
const confirmations = new Map();
export async function confirmSessionRuntime(sessionId, projectDir, processId) {
  const revision = sessionState(sessionId).revision;
  try {
    const items = await invoke("process_list", { projectDir });
    if (sessionState(sessionId).revision !== revision) return;
    const process = items.find(item => item.id === processId);
    reduceSession(sessionId, process?.running ? "running" : "idle", {
      stage: process?.running ? process.stage || "运行中" : "空闲", detail: "",
    });
  } catch (error) {
    if (sessionState(sessionId).revision === revision) reduceSession(sessionId, "unconfirmed", { stage: "运行状态未确认", detail: String(error) });
  }
}
export function awaitRunConfirmation(sessionId, request) {
  if (!sessionId || sessionState(sessionId).phase !== "starting") return;
  const previous = confirmations.get(sessionId);
  if (previous) clearTimeout(previous);
  confirmations.set(sessionId, setTimeout(() => {
    confirmations.delete(sessionId);
    if (sessionState(sessionId).phase === "starting") void confirmSessionRuntime(sessionId, request.projectDir, request.processId);
  }, START_CONFIRMATION_MS));
}
onSessionStateChange((sessionId, state) => {
  if (state.phase !== "starting" && confirmations.has(sessionId)) {
    clearTimeout(confirmations.get(sessionId)); confirmations.delete(sessionId);
  }
});
