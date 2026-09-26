//! 按需工具发现、加载与上下文字数账单。
use super::{HarnessSnapshot, Tool, ToolSpec};

pub(super) fn tool_spec(tool: &dyn Tool) -> ToolSpec {
    ToolSpec {
        name: tool.name().to_owned(),
        description: tool.description(),
        input_schema: tool.input_schema(),
    }
}

fn append_tool_spec(
    tool: &dyn Tool,
    specs: &mut Vec<ToolSpec>,
    context_report: &mut Vec<(String, usize)>,
) -> bool {
    let spec = tool_spec(tool);
    if specs.iter().any(|loaded| loaded.name == spec.name) {
        return false;
    }
    context_report.push((format!("tools/loaded:{}", spec.name), spec.char_len()));
    specs.push(spec);
    true
}

pub(super) fn auto_load_deferred_tool(
    snapshot: &HarnessSnapshot,
    name: &str,
    specs: &mut Vec<ToolSpec>,
    context_report: &mut Vec<(String, usize)>,
) {
    if !snapshot.is_deferred(name) {
        return;
    }
    if let Some(tool) = snapshot
        .deferred_tools()
        .into_iter()
        .find(|tool| tool.name() == name)
    {
        append_tool_spec(tool.as_ref(), specs, context_report);
    }
}

pub(super) fn run_tool_search(
    snapshot: &HarnessSnapshot,
    input: &serde_json::Value,
    specs: &mut Vec<ToolSpec>,
    context_report: &mut Vec<(String, usize)>,
) -> kanzei_harness::ToolOutput {
    let Some(query) = input.get("query").and_then(serde_json::Value::as_str) else {
        return kanzei_harness::ToolOutput::needs_correction(
            "TOOL_SEARCH_QUERY",
            "tool_search requires a string `query`; use keywords or `select:name1,name2`.",
        );
    };
    let limit = match input.get("limit") {
        None => None,
        Some(value) => match value.as_u64().and_then(|value| usize::try_from(value).ok()) {
            Some(limit) => Some(limit),
            None => {
                return kanzei_harness::ToolOutput::needs_correction(
                    "TOOL_SEARCH_LIMIT",
                    "tool_search `limit` must be a positive integer no greater than 10.",
                );
            }
        },
    };
    let deferred = snapshot.deferred_tools();
    let available_names: std::collections::HashSet<String> =
        specs.iter().map(|spec| spec.name.clone()).collect();
    let mut result = kanzei_harness::tool_search::search(query, limit, &deferred, &available_names);
    let mut selected = Vec::new();
    for tool in std::mem::take(&mut result.selected) {
        if append_tool_spec(tool.as_ref(), specs, context_report) {
            selected.push(tool);
        } else {
            result.already_available.push(tool.name().to_owned());
        }
    }
    result.selected = selected;
    let mut seen = std::collections::HashSet::new();
    result
        .already_available
        .retain(|name| seen.insert(name.clone()));
    kanzei_harness::tool_search::render_result(&result)
}
