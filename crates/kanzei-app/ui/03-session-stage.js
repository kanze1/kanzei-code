// Runtime activity follows the latest execution signal. Permission notices and
// completed compaction reports are audit records, not a continuing activity.
export function compactionInProgress(payload = {}) {
  return /压缩|compact/i.test(payload.stage || "")
    && !/已机械清理|已自动压缩|已压缩|压缩为|压缩完成|压不动|保留原历史|completed|finished/i.test(payload.detail || "");
}

export function updateSessionStage(state, event, payload = {}) {
  const finished = ["kz:done", "kz:idle", "kz:stopped"].includes(event)
    || event === "kz:error" && payload.terminal !== false;
  if (event === "kz:turn" || finished) state.stage_tools = new Map();
  if (finished) return; // The session state machine owns terminal stage labels.
  if (["kz:tool-start", "kz:tool-progress", "kz:tool-end"].includes(event)) {
    const tools = state.stage_tools ??= new Map();
    if (event === "kz:tool-start" && payload.id) tools.set(payload.id, payload.name || "");
    if (event === "kz:tool-end") tools.delete(payload.id);
    // Parallel tools finish individually; the provider cannot continue until
    // the whole batch has returned. Progress payloads carry id/chunk, not name.
    state.stage = event === "kz:tool-end" && !tools.size ? "等待模型" : "工具执行中";
    state.detail = state.stage === "等待模型" ? "" : event === "kz:tool-end"
      ? [...tools.values()].at(-1) || ""
      : payload.name || tools.get(payload.id) || state.detail || "";
    state.resume_stage = state.stage;
    return;
  }
  if (event === "kz:status") {
    if (/权限|permission/i.test(payload.stage || "")) {
      state.permission_notice = payload.detail || "";
      if (/权限|permission/i.test(state.stage || "")) state.stage = state.resume_stage || "运行中";
      return;
    }
    state.stage = /压缩|compact/i.test(payload.stage || "") && !compactionInProgress(payload)
      ? "等待模型" : payload.stage || state.stage || "运行中";
    state.detail = payload.detail || "";
  } else {
    const stage = ({
      "kz:turn": "等待模型", "kz:meta": "等待模型", "kz:text": "生成中", "kz:reasoning": "思考中",
      "kz:compacted": "等待模型",
    })[event];
    if (stage) { state.stage = stage; state.detail = stage === "工具执行中" ? payload.name || "" : ""; }
    else if (event === "kz:permission-resolved" && /权限|permission/i.test(state.stage || "")) {
      state.stage = state.resume_stage || "运行中"; state.detail = "";
    }
  }
  if (state.stage && !/权限|压缩|permission|compact/i.test(state.stage)) state.resume_stage = state.stage;
}
