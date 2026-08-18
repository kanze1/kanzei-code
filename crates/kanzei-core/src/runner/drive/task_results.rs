//! 子代理 task 结果到模型工具结果的转换。
//!
//! 并行与串行路径都由驱动循环负责结果归位；本模块只统一转换动作，保持
//! calls[i] ↔ results[i] 的索引配对和事件顺序由调用方控制。

use super::*;

pub(super) fn task_result_part(call_id: String, output: kanzei_harness::ToolOutput) -> Part {
    Part::ToolResult {
        call_id,
        content: output.model_content(),
        is_error: output.is_error,
    }
}
