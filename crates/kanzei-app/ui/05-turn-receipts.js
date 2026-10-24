// Durable wall-duration receipts are separate from the pausable activity clock.
import { languageIsEnglish, t } from "./02-i18n.js";

export function renderTurnReceipt(payload, container, append) {
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
  const outcome = payload.outcome === "halted" ? "已停止"
    : ["failed", "incomplete"].includes(payload.outcome) ? "本轮未完成" : "本轮结束";
  const seconds = (payload.elapsedMs / 1000).toFixed(1);
  receipt.textContent = `${t(outcome)} · ${t("耗时")} ${seconds}s`;
  receipt.title = languageIsEnglish()
    ? `Whole round, including tools and waits · ${payload.runId}`
    : `本轮实际用时，含工具与等待 · ${payload.runId}`;
  append(receipt);
  return receipt;
}

export function renderRecoveredTurnReceipts(traces, pane, append) {
  const receipts = (traces || []).filter(trace => trace.kind === "turn.elapsed");
  if (!receipts.length || !pane) return;
  let group = pane.querySelector(".recovered-turn-times");
  if (!group) {
    group = document.createElement("details");
    group.className = "recovered-turn-times";
    group.open = true;
    group.dataset.sessionId = pane.dataset.sessionId || "";
    const summary = document.createElement("summary");
    summary.textContent = `${t("历史消息")} · ${t("耗时")}`;
    group.append(summary);
    append(group);
  }
  // A separate recovered report: compaction/rewind can remove messages, so do
  // not pretend to associate historical receipts using bubble order or text.
  for (const receipt of receipts) {
    if (![...pane.querySelectorAll(".turn-elapsed-receipt")].some(el => el.dataset.runId === receipt.runId)) {
      renderTurnReceipt(receipt, group, node => group.append(node));
    }
  }
}
