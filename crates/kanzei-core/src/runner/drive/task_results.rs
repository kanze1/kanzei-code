//! 子代理 task 结果到模型工具结果的转换。
//!
//! 并行与串行路径都由驱动循环负责结果归位；本模块只统一转换动作，保持
//! calls[i] ↔ results[i] 的索引配对和事件顺序由调用方控制。

use super::*;

pub(super) fn tool_result_part(call_id: String, output: kanzei_harness::ToolOutput) -> Part {
    Part::ToolResult {
        call_id,
        content: output.model_content(),
        is_error: output.is_error,
    }
}

pub(super) fn tool_result_part_with_images(
    call_id: String,
    output: kanzei_harness::ToolOutput,
    images_supported: bool,
) -> (Part, Vec<Part>) {
    let mut model_content = output.model_content();
    let (images, dropped_note) =
        crate::runner::tool_exec::tool_images_to_parts(&output, images_supported);
    if let Some(note) = dropped_note {
        model_content.push_str(&note);
    }
    (
        Part::ToolResult {
            call_id,
            content: model_content,
            is_error: output.is_error,
        },
        images,
    )
}
