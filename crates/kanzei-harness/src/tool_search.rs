//! R-364 B2:deferred tool discovery and the runner-only `tool_search` stub.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{Tool, ToolConcurrency, ToolCtx, ToolOutput};

pub const TOOL_SEARCH: &str = "tool_search";
const MAX_RESULTS: usize = 10;
const DEFAULT_LIMIT: usize = 5;

/// Runner consumes this result before ordinary tool dispatch.
pub struct SearchResult {
    pub selected: Vec<Arc<dyn Tool>>,
    pub already_available: Vec<String>,
    pub unknown: Vec<String>,
    pub all_deferred_names: Vec<String>,
}

pub struct ToolSearchTool;

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &'static str {
        TOOL_SEARCH
    }

    fn description(&self) -> String {
        "Discover deferred tools by keyword, or load exact names with query `select:name1,name2`."
            .into()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keywords to search for, or `select:name1,name2` to load exact tools."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "default": 5
                }
            },
            "additionalProperties": false
        })
    }

    fn resources(&self, _input: &Value) -> Vec<String> {
        Vec::new()
    }

    fn concurrency(&self, _input: &Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::shared_worktree(ctx)
    }

    async fn execute(&self, _input: Value, _ctx: &ToolCtx) -> ToolOutput {
        ToolOutput::error("tool_search must be handled by the runner")
    }
}

/// Keep directory entries short while preserving the tool's first useful sentence.
pub fn one_line(description: &str) -> String {
    let end = [". ", "。", "\n"]
        .iter()
        .filter_map(|separator| description.find(separator))
        .min()
        .unwrap_or(description.len());
    let sentence = description[..end].trim();
    let mut chars = sentence.chars();
    let first: String = chars.by_ref().take(120).collect();
    if chars.next().is_some() {
        let mut truncated: String = first.chars().take(119).collect();
        truncated.push('…');
        truncated
    } else {
        first
    }
}

/// Search deferred tools by exact `select:` names or weighted case-insensitive keywords.
pub fn search(
    query: &str,
    limit: Option<usize>,
    deferred: &[Arc<dyn Tool>],
    available_names: &HashSet<String>,
) -> SearchResult {
    let all_deferred_names: Vec<String> = deferred
        .iter()
        .map(|tool| tool.name().to_owned())
        .collect();
    let mut result = SearchResult {
        selected: Vec::new(),
        already_available: Vec::new(),
        unknown: Vec::new(),
        all_deferred_names,
    };

    if let Some(selection) = query.strip_prefix("select:") {
        let mut seen = HashSet::new();
        for name in selection.split(',').map(str::trim).filter(|name| !name.is_empty()) {
            if !seen.insert(name.to_owned()) {
                continue;
            }
            if result.selected.len() + result.already_available.len() + result.unknown.len()
                >= MAX_RESULTS
            {
                break;
            }
            if available_names.contains(name) {
                result.already_available.push(name.to_owned());
            } else if let Some(tool) = deferred.iter().find(|tool| tool.name() == name) {
                result.selected.push(Arc::clone(tool));
            } else {
                result.unknown.push(name.to_owned());
            }
        }
        return result;
    }

    let terms: Vec<String> = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<VecDeque<_>>()
        .into_iter()
        .fold(Vec::new(), |mut unique, term| {
            if !unique.contains(&term) {
                unique.push(term);
            }
            unique
        });
    let mut ranked: Vec<(usize, usize, Arc<dyn Tool>)> = deferred
        .iter()
        .enumerate()
        .filter_map(|(index, tool)| {
            let name = tool.name().to_lowercase();
            let description = tool.description().to_lowercase();
            let score: usize = terms
                .iter()
                .map(|term| {
                    usize::from(name.contains(term)) * 3
                        + usize::from(description.contains(term))
                })
                .sum();
            if score == 0 {
                None
            } else if available_names.contains(tool.name()) {
                result.already_available.push(tool.name().to_owned());
                None
            } else {
                Some((score, index, Arc::clone(tool)))
            }
        })
        .collect();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_RESULTS);
    result.selected = ranked
        .into_iter()
        .take(limit)
        .map(|(_, _, tool)| tool)
        .collect();
    result
}

/// Render the stable, registration-ordered directory injected into the system prompt.
pub fn render_catalog(deferred: &[Arc<dyn Tool>]) -> Option<String> {
    if deferred.is_empty() {
        return None;
    }
    let mut catalog = String::from(
        "<deferred-tools>\nDeferred tools are registered but their schemas are omitted from the current request. Prefer the resident `tool_search`: search by keyword or load exact names with query `select:name1,name2`. Their schemas become callable from the next step and stay loaded for this run; direct calls also auto-load before execution.\n",
    );
    for tool in deferred {
        catalog.push_str(&format!("- {} — {}\n", tool.name(), one_line(&tool.description())));
    }
    catalog.push_str("</deferred-tools>");
    Some(catalog)
}

/// Convert a search result to the model-facing result without changing ToolOutput's contract.
pub fn render_result(result: &SearchResult) -> ToolOutput {
    if !result.unknown.is_empty() {
        return ToolOutput::needs_correction(
            "TOOL_SEARCH_UNKNOWN",
            format!(
                "Unknown deferred tool(s): {}. Available deferred tools: {}{}",
                result.unknown.join(", "),
                result.all_deferred_names.join(", "),
                if result.selected.is_empty() {
                    String::new()
                } else {
                    format!(". Loaded: {}", names(&result.selected))
                }
            ),
        );
    }

    if result.selected.is_empty() {
        if !result.already_available.is_empty() {
            return ToolOutput::ok(format!(
                "Already available: {}.",
                result.already_available.join(", ")
            ));
        }
        return ToolOutput::ok(format!(
            "No deferred tools matched. Available deferred tools: {}",
            result.all_deferred_names.join(", ")
        ));
    }

    let specs: Vec<Value> = result
        .selected
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name(),
                "description": tool.description(),
                "input_schema": tool.input_schema()
            })
        })
        .collect();
    let mut content = format!(
        "Loaded: {} — callable from your next step, for the rest of this run.",
        names(&result.selected)
    );
    if !result.already_available.is_empty() {
        content.push_str(&format!(
            " Already available: {}.",
            result.already_available.join(", ")
        ));
    }
    content.push('\n');
    content.push_str(&serde_json::to_string(&specs).expect("tool specs serialize"));
    ToolOutput::ok(content)
}

fn names(tools: &[Arc<dyn Tool>]) -> String {
    tools
        .iter()
        .map(|tool| tool.name())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTool {
        name: &'static str,
        description: &'static str,
    }

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &'static str {
            self.name
        }
        fn description(&self) -> String {
            self.description.into()
        }
        fn input_schema(&self) -> Value {
            json!({"type":"object", "properties":{}})
        }
        async fn execute(&self, _input: Value, _ctx: &ToolCtx) -> ToolOutput {
            ToolOutput::ok("unused")
        }
    }

    fn tools() -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(DummyTool {
                name: "process",
                description: "Manage long-running processes. Read their output.",
            }),
            Arc::new(DummyTool {
                name: "read_output",
                description: "Read process output and status.",
            }),
            Arc::new(DummyTool {
                name: "background",
                description: "Start a background task. Track its process.",
            }),
        ]
    }

    #[test]
    fn select_deduplicates_preserves_order_and_classifies_names() {
        let deferred = tools();
        let resident = HashSet::from(["read".to_owned()]);
        let result = search(
            "select:background,read,missing,background",
            None,
            &deferred,
            &resident,
        );
        assert_eq!(names(&result.selected), "background");
        assert_eq!(result.already_available, ["read"]);
        assert_eq!(result.unknown, ["missing"]);
    }

    #[test]
    fn keyword_search_ranks_name_hits_and_keeps_registration_order_for_ties() {
        let deferred = tools();
        let result = search("process", Some(10), &deferred, &HashSet::new());
        assert_eq!(
            names(&result.selected),
            "process, read_output, background"
        );
    }

    #[test]
    fn keyword_search_skips_already_available_tools_without_spending_result_limit() {
        let deferred = tools();
        let available = HashSet::from(["process".to_owned()]);
        let result = search("process", Some(10), &deferred, &available);
        assert_eq!(names(&result.selected), "read_output, background");
        assert_eq!(result.already_available, ["process"]);
    }

    #[test]
    fn select_marks_previously_loaded_deferred_tool_already_available() {
        let deferred = tools();
        let available = HashSet::from(["background".to_owned()]);
        let result = search("select:background", None, &deferred, &available);
        assert!(result.selected.is_empty());
        assert_eq!(result.already_available, ["background"]);
    }

    #[test]
    fn keyword_limit_is_clamped_and_defaults_to_five() {
        let deferred = tools();
        let result = search("process", Some(0), &deferred, &HashSet::new());
        assert_eq!(result.selected.len(), 1);
        assert_eq!(search("process", None, &deferred, &HashSet::new()).selected.len(), 3);
    }

    #[test]
    fn catalog_uses_one_sentence_and_truncates_at_120_characters() {
        let mut long = "x".repeat(130);
        long.push_str(". second sentence");
        let deferred = vec![Arc::new(DummyTool {
            name: "long",
            description: Box::leak(long.into_boxed_str()),
        }) as Arc<dyn Tool>];
        let catalog = render_catalog(&deferred).unwrap();
        let entry = catalog.lines().nth(2).unwrap();
        assert!(entry.starts_with("- long — "));
        assert!(entry.chars().count() <= 130);
        assert!(entry.ends_with('…'));
        assert!(render_catalog(&[]).is_none());
    }

    #[test]
    fn unknown_select_returns_repair_code_and_full_directory() {
        let deferred = tools();
        let result = search("select:missing", None, &deferred, &HashSet::new());
        let output = render_result(&result);
        assert_eq!(output.code, Some("TOOL_SEARCH_UNKNOWN"));
        assert!(output.content.contains("process, read_output, background"));
        assert!(output.is_error);
    }
}
