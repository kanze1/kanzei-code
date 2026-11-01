// Durable wall-duration receipts are separate from the pausable activity clock.
import { languageIsEnglish, t } from "./02-i18n.js";

function receiptElement(payload, container) {
  if (!container || typeof payload?.runId !== "string" || !payload.runId
      || typeof payload.elapsedMs !== "number" || !Number.isFinite(payload.elapsedMs)
      || payload.elapsedMs < 0) return null;
  const session = container.dataset?.sessionId;
  if (payload.sessionId && session && payload.sessionId !== session) return null;
  // Search attributes as values, not CSS interpolations (identity is not markup).
  let receipt = [...container.querySelectorAll(".turn-elapsed-receipt")]
    .find(el => el.dataset.runId === payload.runId);
  if (receipt) return receipt; // An ended run is fixed, including after duplicate events.
  receipt = document.createElement("div");
  receipt.className = "turn-elapsed-receipt";
  receipt.dataset.runId = payload.runId;
  receipt.dataset.sessionId = payload.sessionId || session || "";
  receipt.dataset.elapsedMs = String(payload.elapsedMs);
  receipt.dataset.outcome = payload.outcome || "completed";
  receipt.dataset.source = payload.source || "backend_elapsed";
  receipt.title = languageIsEnglish()
    ? `Whole round, including tools and waits  ${payload.runId}`
    : `该轮实际用时，含工具与等待  ${payload.runId}`;
  return receipt;
}

export function renderTurnReceipt(payload, container, append) {
  const receipt = receiptElement(payload, container);
  if (!receipt || receipt.parentElement) return receipt;
  const outcome = payload.outcome === "halted" ? "已停止"
    : ["failed", "incomplete"].includes(payload.outcome) ? "本轮未完成" : "本轮结束";
  receipt.textContent = `${t(outcome)}  ${t("耗时")} ${(payload.elapsedMs / 1000).toFixed(1)}s`;
  append(receipt);
  return receipt;
}

export function renderRecoveredTurnReceipts(traces, pane, append) {
  if (!pane) return;
  let group = pane.querySelector(".recovered-turn-times");
  let list = group?.querySelector(".recovered-turn-times-body");
  // A separate recovered report: compaction/rewind can remove messages, so do
  // not pretend to associate historical receipts using bubble order or text.
  for (const payload of traces || []) {
    if (payload.kind !== "turn.elapsed") continue;
    const receipt = receiptElement(payload, pane);
    if (!receipt || receipt.parentElement) continue;
    if (!group) {
      group = document.createElement("details");
      group.className = "recovered-turn-times";
      group.dataset.sessionId = pane.dataset.sessionId || "";
      list = document.createElement("div");
      list.className = "recovered-turn-times-body";
      list.setAttribute("role", "list");
      list.setAttribute("aria-label", t("历史耗时"));
      group.append(document.createElement("summary"), list);
      append(group);
    }
    const status = document.createElement("span"), duration = document.createElement("span");
    status.textContent = t(payload.outcome === "halted" ? "已停止"
      : ["failed", "incomplete"].includes(payload.outcome) ? "未完成" : "已完成");
    duration.textContent = `${(payload.elapsedMs / 1000).toFixed(1)}s`;
    receipt.setAttribute("role", "listitem");
    receipt.replaceChildren(status, duration);
    list.append(receipt);
  }
  if (group) group.querySelector("summary").textContent = `${t("历史耗时")} (${list.childElementCount})`;
}
