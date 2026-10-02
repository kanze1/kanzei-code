//! Cooperative stop waits and paired results for tool calls cancelled before execution.

use super::{CancellationToken, Part};

/// D-342:停止信号等待器。halt 未配置时永不就绪——select 里这个分支等价于不存在,
/// CLI(无停止通道)与引入前逐字节同行为。
pub(super) async fn halt_signalled(halt: Option<&CancellationToken>) {
    match halt {
        Some(token) => token.cancelled().await,
        None => std::future::pending().await,
    }
}

/// D-342:停止后未执行的工具调用统一以「取消占位」配对,与权限拒绝占位同款形态,
/// 保证 calls[i]↔results[i] 对齐、历史里没有孤儿 ToolCall。
pub(super) fn append_halted_tool_results(
    results: &mut Vec<Part>,
    calls: &[(String, String, serde_json::Value, String)],
    from_index: usize,
) {
    for (id, _, _, _) in calls.iter().skip(from_index) {
        results.push(Part::ToolResult {
            call_id: id.clone(),
            content: "cancelled: run stopped by user before this tool executed".into(),
            is_error: true,
        });
    }
}
