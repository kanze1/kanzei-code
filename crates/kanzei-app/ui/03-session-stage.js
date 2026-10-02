// Runtime activity follows the latest execution signal. Permission notices and
// completed compaction reports are audit records, not a continuing activity.
export function compactionInProgress(payload = {}) {
  return /压缩|compact/i.test(payload.stage || "")
    && !/已机械清理|已自动压缩|已压缩|压缩为|压缩完成|压不动|保留原历史|completed|finished/i.test(payload.detail || "");
}

export function updateSessionStage(state, event, payload = {}) {
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
      "kz:tool-start": "工具执行中", "kz:tool-progress": "工具执行中", "kz:tool-end": "等待模型",
      "kz:compacted": "等待模型",
    })[event];
    if (stage) { state.stage = stage; state.detail = stage === "工具执行中" ? payload.name || "" : ""; }
    else if (event === "kz:permission-resolved" && /权限|permission/i.test(state.stage || "")) {
      state.stage = state.resume_stage || "运行中"; state.detail = "";
    }
  }
  if (state.stage && !/权限|压缩|permission|compact/i.test(state.stage)) state.resume_stage = state.stage;
}
